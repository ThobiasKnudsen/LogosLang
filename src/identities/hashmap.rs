// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `hashmap K -> V`, the native map the seed owes (DESIGN ›The reflection boundary
//! is callable-versus-data‹): an empty map of the type interned per `(K, V)`, or
//! that type before `?`; `m[k]` read and `m[k] = v` write (›The dyad's read
//! surface‹). An instance's eight bytes hold its table in the store, null, the
//! empty map, until the first write; no field names them, so no Logos code forges
//! one. The natives run interpreted.

use super::callable::{self, Callables};
use super::read::{read_kind, Read};
use super::{array, meta, numtype_of, Cx, Operand};
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError, Parser, ParsingTape};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

type Table = std::collections::HashMap<i64, i64>;

#[derive(Debug, Clone, Copy)]
pub struct HashmapIds {
    pub hashmap: DyadPtr,
    /// The memo of mintings, an array of `K, V, mint` triples.
    pub mints: DyadPtr,
    /// `m[k]`: `[map address, key, op]`.
    pub get: DyadPtr,
    pub get_leaf: DyadPtr,
    /// `m[k] = v`: `[map address, key, value, op]`.
    pub put: DyadPtr,
    pub put_leaf: DyadPtr,
}

pub(super) fn register(cx: &mut Cx, cs: &Callables, array_ty: DyadPtr) -> HashmapIds {
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::TIGHT);
    let hashmap = cx.store.alloc_raw(cx.type_, record);
    cx.declare("hashmap", hashmap);
    cx.metas.insert(hashmap, |p, _id, tape| construct(p, tape));
    let mints = array::build(cx.store, array_ty, &[]);
    let op = |cx: &mut Cx, roles: &[&str], run: crate::run::RunFn| {
        let record = meta::operand_record(
            cx,
            meta::TUPLE_TAG,
            meta::prec::INERT,
            crate::parse::Assoc::Left,
            roles,
        );
        let id = cx.store.alloc_raw(cx.type_, record);
        let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
        (id, leaf)
    };
    let (get, get_leaf) = op(cx, &["map", "key", "op"], run_get);
    let (put, put_leaf) = op(cx, &["map", "key", "value", "op"], run_put);
    HashmapIds { hashmap, mints, get, get_leaf, put, put_leaf }
}

/// `hashmap K -> V`: consumes the three cells to its right and places a fresh empty
/// map, or the interned type when `?` follows, for `?` to read (DESIGN ›The
/// reflection boundary is callable-versus-data‹).
fn construct(p: &mut Parser, tape: &mut ParsingTape) -> Result<Constructed, ParseError> {
    let types = p.types();
    let key = param(p, tape, 1)?;
    let arrow = p.cell_at(tape, 2)?.ok_or(ParseError::HashmapShape)?;
    if arrow.constructed || arrow.identity(types) != types.arrow_ {
        return Err(ParseError::HashmapShape);
    }
    let value = param(p, tape, 3)?;
    for _ in 0..3 {
        tape.remove(1);
    }
    // SAFETY: `key` and `value` are type identities from the store.
    let ty = unsafe { mint(p.store(), types, key, value) };
    let hole_follows =
        p.cell_at(tape, 1)?.is_some_and(|c| !c.constructed && c.identity(types) == types.unknown);
    let node = if hole_follows { ty } else { p.alloc_local(ty, 8) };
    tape.place(node);
    Ok(Constructed::Placed)
}

/// A key or value type: one whose value fits the table's `i64` exactly, so a node
/// type by identity or an integer type by its bits.
fn param(p: &mut Parser, tape: &mut ParsingTape, offset: isize) -> Result<DyadPtr, ParseError> {
    let types = p.types();
    let cell = p.cell_at(tape, offset)?.ok_or(ParseError::HashmapShape)?;
    let d = p.operand_dyad(cell)?;
    // SAFETY: `d` is a resolved dyad from the store.
    unsafe {
        let t = super::type_identity_of(types, d).ok_or(ParseError::HashmapShape)?;
        let fits = t == types.type_
            || t == types.dyad_
            || (t != types.bool_
                && matches!(
                    super::read::place_layout(types, t),
                    Some((Read::Scalar(nt), _)) if !nt.is_float()
                ));
        if fits {
            Ok(t)
        } else {
            Err(ParseError::HashmapShape)
        }
    }
}

/// # Safety
/// `key` and `value` must be type identities from the store.
unsafe fn mint(store: &mut Store, types: &Core, key: DyadPtr, value: DyadPtr) -> DyadPtr {
    let memo = types.hashmap.mints;
    if let Some([_, _, t]) =
        array::items(memo).as_chunks::<3>().0.iter().find(|[k, v, _]| *k == key && *v == value)
    {
        return *t;
    }
    let scope = store.alloc_raw(types.scope, std::ptr::null_mut());
    let fields = array::build(store, types.array_, &[]);
    let layout = meta::record_layout(
        store,
        scope,
        fields,
        8,
        std::ptr::null_mut(),
        meta::prec::APPLY,
        crate::parse::Assoc::Left,
    );
    let node = store.alloc_raw(types.type_, layout);
    for item in [key, value, node] {
        array::push(store, memo, item);
    }
    node
}

/// The `(K, V)` a mint was built for, or `None` when `t` is no `hashmap` type.
///
/// # Safety
/// `t` must be null or a dyad from the store.
unsafe fn params_of(types: &Core, t: DyadPtr) -> Option<(DyadPtr, DyadPtr)> {
    array::items(types.hashmap.mints)
        .as_chunks::<3>()
        .0
        .iter()
        .find(|[_, _, m]| *m == t)
        .map(|&[k, v, _]| (k, v))
}

/// # Safety
/// `t` must be null or a dyad from the store.
pub(crate) unsafe fn is_hashmap(types: &Core, t: DyadPtr) -> bool {
    params_of(types, t).is_some()
}

/// The value type a get node yields; `None` for any other node.
///
/// # Safety
/// `node` must be null or a dyad from the store.
pub(crate) unsafe fn value_type_of(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    let node = types.through(node);
    if node.is_null() || (*node).ty != types.hashmap.get {
        return None;
    }
    // The map operand is the `&place` node `build_addr` made, its pointee the mint.
    let addr = *((*node).value as *const DyadPtr);
    let mint = *((*addr).value as *const DyadPtr).add(1);
    params_of(types, mint).map(|(_, v)| v)
}

/// A get node of a node-valued map is read into a box of its value type, as a
/// `type` or `dyad` place is.
///
/// # Safety
/// `node` must be null or a dyad from the store.
pub(crate) unsafe fn box_of(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    value_type_of(types, node).filter(|&v| v == types.type_ || v == types.dyad_)
}

/// # Safety
/// `node` must be a get node.
pub(crate) unsafe fn operand_of(types: &Core, node: DyadPtr) -> Operand {
    match value_type_of(types, node) {
        Some(v) if v != types.type_ && v != types.dyad_ => {
            Operand::Concrete(super::numtype::of_type_node(v))
        }
        _ => Operand::NonNumeric,
    }
}

/// `node` checked against `want`, a literal committed to it.
///
/// # Safety
/// `want` must be a type `param` accepted and `node` a reduced dyad from the store.
unsafe fn accept(
    store: &mut Store,
    types: &Core,
    want: DyadPtr,
    node: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    if want == types.type_ || want == types.dyad_ {
        let ok = match read_kind(types, node) {
            Read::Identity => true,
            Read::Container(c) => {
                if want == types.type_ {
                    c == types.type_
                } else {
                    !c.is_null()
                }
            }
            Read::Address => want == types.dyad_,
            Read::Executable(_) => value_type_of(types, node) == Some(want),
            _ => false,
        };
        return if ok { Ok(node) } else { Err(ParseError::TypeMismatch) };
    }
    if matches!(numtype_of(types, node), Operand::Literal) {
        return super::commit_literal_to(store, types, types.through(node), want);
    }
    super::check_store_type(types, want, node)?;
    Ok(node)
}

/// `m[k]` when `lhs` is a `hashmap` place, else `None`.
///
/// # Safety
/// `lhs` and `key` must be reduced dyads from the store.
pub(crate) unsafe fn build_get(
    store: &mut Store,
    types: &Core,
    lhs: DyadPtr,
    key: DyadPtr,
) -> Result<Option<DyadPtr>, ParseError> {
    let place = types.through(lhs);
    if !crate::dyad::is_place((*place).value) {
        return Ok(None);
    }
    let Some((k, _)) = params_of(types, (*place).ty) else {
        return Ok(None);
    };
    let key = accept(store, types, k, key)?;
    let map = super::pointer::build_addr(store, types, place);
    let value = store.alloc_operands(&[map, key, types.hashmap.get_leaf]);
    Ok(Some(store.alloc_raw(types.hashmap.get, value)))
}

/// # Safety
/// `get` must be a get node and `value` a reduced dyad from the store.
pub(crate) unsafe fn build_put(
    store: &mut Store,
    types: &Core,
    get: DyadPtr,
    value: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let v = value_type_of(types, get).expect("a get node's map is a hashmap place");
    let value = accept(store, types, v, value)?;
    let ops = (*get).value as *const DyadPtr;
    let operands = store.alloc_operands(&[*ops, *ops.add(1), value, types.hashmap.put_leaf]);
    Ok(store.alloc_raw(types.hashmap.put, operands))
}

/// The slot the instance's table pointer lives in.
unsafe fn table_slot(rt: &mut Runtime, map: DyadPtr) -> Result<*mut *mut Table, RunError> {
    let addr = rt.run(map)? as *mut *mut Table;
    if addr.is_null() {
        return Err(RunError::NullPointer);
    }
    Ok(addr)
}

/// A missing key hands back the unknown where the value type's places hold it, and
/// is the checked error where they do not (DESIGN ›Declarations are immutable by
/// default‹).
fn run_get(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a get node `build_get` built; the slot is an instance's eight
    // bytes, null or a table `run_put` allocated in the store.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let key = rt.run(*ops.add(1))?;
        let table = std::ptr::read_unaligned(table_slot(rt, *ops)?);
        if let Some(&v) = table.as_ref().and_then(|t| t.get(&key)) {
            return Ok(v);
        }
        if box_of(rt.types(), node).is_some() {
            Ok(rt.types().unknown as i64)
        } else {
            Err(RunError::MissingKey)
        }
    }
}

/// The operands run first, so the table is read after anything they write.
fn run_put(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `run_get`; a null table is replaced by a fresh one before the insert.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let key = rt.run(*ops.add(1))?;
        let value = rt.run(*ops.add(2))?;
        let slot = table_slot(rt, *ops)?;
        let mut table = std::ptr::read_unaligned(slot);
        if table.is_null() {
            table = rt.store().alloc_table();
            std::ptr::write_unaligned(slot, table);
        }
        (*table).insert(key, value);
        Ok(0)
    }
}
