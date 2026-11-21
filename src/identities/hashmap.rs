// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `hashmap K -> V`, the native map the seed owes (DESIGN ›The reflection boundary
//! is callable-versus-data‹): an empty map of the type interned per `(K, V)`, or
//! that type before `?`; `m[k]` read and `m[k] = v` write (›The dyad's read
//! surface‹). An instance's eight bytes hold its table in the store, null, the
//! empty map, until the first write, or `?`'s address where the map is the unknown; no
//! field names them, so no Logos code forges one. The natives run interpreted.

use super::callable::{self, Callables};
use super::read::Read;
use super::{array, meta, Cx};
use crate::dyad;
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
    /// `m[k] = v`: `[map address, key, value, value type, op]`.
    pub put: DyadPtr,
    pub put_leaf: DyadPtr,
    /// `hashmap K -> V` as a value: `[op, map type]`, a new empty map.
    pub fresh: DyadPtr,
    pub fresh_leaf: DyadPtr,
    /// `hashmap K -> V { … }`: `[op, map type, key, value, …, null]`, a new map of those entries.
    pub entries: DyadPtr,
    pub entries_leaf: DyadPtr,
    /// What `m == ?` and `m != ?` run, under `==`'s and `!=`'s mints.
    pub eq_unknown_leaf: DyadPtr,
    pub ne_unknown_leaf: DyadPtr,
    /// The eight bytes of the empty map and of the unknown one, which a value copies out.
    empty_bytes: *mut u8,
    unknown_bytes: *mut u8,
}

pub(super) fn register(cx: &mut Cx, cs: &Callables, array_ty: DyadPtr) -> HashmapIds {
    let record = meta::token_record(cx, meta::prec::TIGHT, crate::parse::Assoc::Left, &[]);
    let hashmap = cx.store.alloc_head(cx.type_, record);
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
        let id = cx.store.alloc_head(cx.type_, record);
        let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
        (id, leaf)
    };
    let (get, get_leaf) = op(cx, &["map", "key", "op", "output_type"], run_get);
    let (put, put_leaf) = op(cx, &["map", "key", "value", "type", "op"], run_put);
    let (fresh, fresh_leaf) = op(cx, &["op", "output_type"], run_fresh);
    let record = meta::operand_record(
        cx,
        meta::LIST_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["op", "output_type"],
    );
    let entries = cx.store.alloc_head(cx.type_, record);
    let entries_leaf = callable::mint_native(cx.store, cs.callable, run_entries, cs.seed_native);
    let eq_unknown_leaf =
        callable::mint_native(cx.store, cs.callable, run_eq_unknown, cs.seed_native);
    let ne_unknown_leaf =
        callable::mint_native(cx.store, cs.callable, run_ne_unknown, cs.seed_native);
    let empty_bytes = cx.store.alloc_bytes(&0usize.to_ne_bytes());
    let unknown_bytes = cx.store.alloc_bytes(&(cx.unknown as usize).to_ne_bytes());
    HashmapIds {
        hashmap,
        mints,
        get,
        get_leaf,
        put,
        put_leaf,
        fresh,
        fresh_leaf,
        entries,
        entries_leaf,
        eq_unknown_leaf,
        ne_unknown_leaf,
        empty_bytes,
        unknown_bytes,
    }
}

/// `hashmap K -> V`: consumes the three cells to its right and places a new empty map, a value
/// `:=` and `=` both take, or the interned type when `?` follows, for `?` to read (DESIGN ›The
/// reflection boundary is callable-versus-data‹), or the map of a `{ … }`'s names when one
/// follows.
fn construct(p: &mut Parser, tape: &mut ParsingTape) -> Result<Constructed, ParseError> {
    let types = p.types();
    let ty = map_type(p, tape)?;
    let next = p.cell_at(tape, 1)?;
    if next.is_some_and(|c| !c.constructed && c.identity(types) == types.unknown) {
        tape.place(ty);
        return Ok(Constructed::Placed);
    }
    let filled = match next.filter(|c| c.constructed) {
        // SAFETY: a constructed cell holds a node from the store; `ty` is the mint just read.
        Some(c) => unsafe { from_scope(p, ty, c.dyad, c.start) }?,
        None => None,
    };
    if let Some(filled) = filled {
        tape.remove(1);
        tape.place(filled);
        return Ok(Constructed::Placed);
    }
    let fresh = p.store().alloc_words(types.hashmap.fresh, &[types.hashmap.fresh_leaf, ty]);
    tape.place(fresh);
    Ok(Constructed::Placed)
}

/// `hashmap K -> V { … }` when `value` is a `{ … }`: each name the scope keeps is an entry, its
/// spelling the key and its value converted into `V` (›`{ … }` outside text is a scope kept as a
/// value, read with `.`‹ [[R374]]). `hashmap` reads the scope from outside, so each name is one
/// the gate read lets through, a `pub` one; one it refuses is refused at `at`, where the `{ … }`
/// is written. The scope's last line makes the map in place of its record's value, so its lines
/// still run first. `None` when `value` is no `{ … }`.
///
/// # Safety
/// `map_ty` must be a mint of `hashmap` and `value` a node from the store.
unsafe fn from_scope(
    p: &mut Parser,
    map_ty: DyadPtr,
    value: DyadPtr,
    at: usize,
) -> Result<Option<DyadPtr>, ParseError> {
    let types = p.types();
    let Some((names, line)) = super::instance::kept_value(types, value) else {
        return Ok(None);
    };
    let (k, v) = params_of(types, map_ty).expect("`map_type` mints a hashmap");
    if k != types.string_ {
        return Err(ParseError::ScopeKeysAreText);
    }
    let mut ops = vec![types.hashmap.entries_leaf, map_ty];
    for name in names {
        let spelling = crate::binding::Binding::spelling(name);
        if crate::binding::Binding::read(name).pattern {
            return Err(ParseError::PatternIsNoKey(Box::new(spelling)));
        }
        p.read_gate(name).map_err(|e| p.fail_at(at, e))?;
        let store = p.store();
        ops.push(super::string::build_text(store, types.string_, spelling.as_bytes()));
        ops.push(super::land(store, types, name, v)?);
    }
    ops.push(std::ptr::null_mut());
    let map = p.store().alloc_words(types.hashmap.entries, &ops);
    if line == value {
        return Ok(Some(map));
    }
    let lines = array::items(super::scope::exprs_array(value));
    let at = lines.iter().rposition(|&l| l == line).expect("`kept_value` found the line in it");
    super::scope::set_item(value, at, map);
    Ok(Some(value))
}

/// The map type `K -> V` right of the center names, its three cells consumed. A value type
/// written `hashmap K2 -> V2` is read the same way where it stands, so a map's values may be maps.
fn map_type(p: &mut Parser, tape: &mut ParsingTape) -> Result<DyadPtr, ParseError> {
    let types = p.types();
    let key = param(p, tape, 1)?;
    let arrow = p.cell_at(tape, 2)?.ok_or(ParseError::HashmapShape)?;
    if arrow.constructed || arrow.identity(types) != types.arrow_ {
        return Err(ParseError::HashmapShape);
    }
    let value = match p.cell_at(tape, 3)? {
        Some(c) if !c.constructed && c.identity(types) == types.hashmap.hashmap => {
            let mark = tape.mark();
            tape.recenter(3);
            let inner = map_type(p, tape);
            tape.restore(mark);
            inner?
        }
        _ => param(p, tape, 3)?,
    };
    for _ in 0..3 {
        tape.remove(1);
    }
    // SAFETY: `key` and `value` are type identities from the store.
    Ok(unsafe { mint(p.store(), types, key, value) })
}

/// A key or value type: one whose value fits the table's `i64` exactly, so a node
/// type by identity, text by its node or an integer type by its bits. A map as the value is
/// [`map_type`]'s.
fn param(p: &mut Parser, tape: &mut ParsingTape, offset: isize) -> Result<DyadPtr, ParseError> {
    let types = p.types();
    let cell = p.cell_at(tape, offset)?.ok_or(ParseError::HashmapShape)?;
    let d = p.operand_dyad(cell)?;
    // SAFETY: `d` is a resolved dyad from the store.
    unsafe {
        let t = super::type_identity_of(types, d).ok_or(ParseError::HashmapShape)?;
        let fits = t == types.type_
            || t == types.dyad_
            || t == types.string_
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
    let scope = super::scope::mint(store, types.scope, std::ptr::null_mut());
    let none = array::build(store, types.array_, &[]);
    // The scope is its body too, so a map moves whole by its eight bytes, as a value of a
    // type with a body does: into a name, and into another map.
    let layout = meta::record_layout(
        store,
        scope,
        none,
        none,
        8,
        scope,
        meta::prec::APPLY,
        crate::parse::Assoc::Left,
    );
    let node = store.alloc_head(types.type_, layout);
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
    if !types.is_storage(place) {
        return Ok(None);
    }
    let Some((k, v)) = params_of(types, types.type_of(place)) else {
        return Ok(None);
    };
    let key = super::land(store, types, key, k)?;
    let map = super::pointer::build_addr(store, types, place, std::ptr::null_mut());
    Ok(Some(store.alloc_words(types.hashmap.get, &[map, key, types.hashmap.get_leaf, v])))
}

/// # Safety
/// `get` must be a get node and `value` a reduced dyad from the store.
pub(crate) unsafe fn build_put(
    store: &mut Store,
    types: &Core,
    get: DyadPtr,
    value: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let v = super::read::output_type(types, get);
    let value = super::land(store, types, value, v)?;
    let ops = dyad::value(get) as *const DyadPtr;
    let put = [*ops, *ops.add(1), value, v, types.hashmap.put_leaf];
    Ok(store.alloc_words(types.hashmap.put, &put))
}

/// `m == ?` (`equal`) or `m != ?` when one side gives a map and the other is `?`, a map's
/// places holding `?` (›A declared hashmap is the empty map: `m := hashmap K -> V`‹ [[R17]]):
/// a node of `op`'s mint for the two sides' types; else `None`.
///
/// # Safety
/// `op` must be `==` or `!=`; `lhs` and `rhs` reduced dyads from the store.
pub(crate) unsafe fn build_unknown_compare(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    equal: bool,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Option<DyadPtr> {
    let output_type = |n: DyadPtr| super::read::output_type(types, n);
    let key = [output_type(lhs), output_type(rhs)];
    match key.map(|t| is_hashmap(types, t)) {
        [true, false] if types.through(rhs) == types.unknown => {}
        [false, true] if types.through(lhs) == types.unknown => {}
        _ => return None,
    }
    let ids = &types.hashmap;
    let leaf = if equal { ids.eq_unknown_leaf } else { ids.ne_unknown_leaf };
    Some(super::mint::node(store, types, op, &key, types.bool_, leaf, &[lhs, rhs]))
}

/// The eight bytes a map kept as another map's value lives in, holding a copy of `table`, a
/// table of the map type `map_ty`.
///
/// # Safety
/// `table` must be null or a table `run_put` allocated, of a map of type `map_ty`.
unsafe fn held_copy(store: &mut Store, types: &Core, table: *mut Table, map_ty: DyadPtr) -> i64 {
    let copy = copy_table(store, types, table, map_ty);
    store.alloc_bytes(&(copy as usize).to_ne_bytes()) as i64
}

/// The eight bytes at `at`, a map of the type `map_ty` copied bit for bit, come to hold a copy
/// of the table they share.
///
/// # Safety
/// `at` must hold a map of type `map_ty`: null, `?`'s address or a table `run_put` allocated.
pub(crate) unsafe fn deepen(store: &mut Store, types: &Core, map_ty: DyadPtr, at: *mut u8) {
    let table = std::ptr::read_unaligned(at as *const *mut Table);
    let copy = copy_table(store, types, table, map_ty);
    std::ptr::write_unaligned(at as *mut *mut Table, copy);
}

/// A new table holding `table`'s entries, each map among its values copied too; the empty
/// map's null and the unknown's `?` as they are.
///
/// # Safety
/// As [`held_copy`].
unsafe fn copy_table(
    store: &mut Store,
    types: &Core,
    table: *mut Table,
    map_ty: DyadPtr,
) -> *mut Table {
    if table.is_null() || holds_unknown(types, table) {
        return table;
    }
    let maps = params_of(types, map_ty).map(|(_, v)| v).filter(|&v| is_hashmap(types, v));
    let entries: Vec<(i64, i64)> = (*table).iter().map(|(&k, &v)| (k, v)).collect();
    let copy = store.alloc_table();
    for (k, v) in entries {
        let v = match maps {
            Some(inner) => {
                let held = std::ptr::read_unaligned(v as usize as *const *mut Table);
                held_copy(store, types, held, inner)
            }
            None => v,
        };
        (*copy).insert(k, v);
    }
    copy
}

/// The slot the instance's table pointer lives in, and the table there: null for the empty
/// map; a map that is `?` has none to read or write.
unsafe fn table_slot(
    rt: &mut Runtime,
    map: DyadPtr,
) -> Result<(*mut *mut Table, *mut Table), RunError> {
    let addr = rt.run(map)? as *mut *mut Table;
    if addr.is_null() {
        return Err(RunError::NullPointer);
    }
    let table = std::ptr::read_unaligned(addr);
    if holds_unknown(rt.types(), table) {
        return Err(RunError::UnknownMap);
    }
    Ok((addr, table))
}

/// Whether a map's eight bytes hold `?` in place of a table.
fn holds_unknown(types: &Core, table: *mut Table) -> bool {
    std::ptr::eq(table.cast::<dyad::Dyad>(), types.unknown)
}

/// `key` run, as a table keys it: a text by the one node the store keeps for it, so two nodes of
/// one text are one key (›A type with a canonical form ships a normalizer; equality there is
/// identification‹ [[R325]]); any other key by its bits.
///
/// # Safety
/// `key` must be a reduced dyad from the store.
unsafe fn run_key(rt: &mut Runtime, key: DyadPtr) -> Result<i64, RunError> {
    let bits = rt.run(key)?;
    if super::read::output_type(rt.types(), key) != rt.types().string_ {
        return Ok(bits);
    }
    let node = bits as usize as DyadPtr;
    Ok(rt.store().text_key(super::string::text(node), node) as i64)
}

/// A missing key hands back the unknown where the value type's places hold it, and
/// is the checked error where they do not (›`?` is one value, the unknown; `key := T ?`
/// declares without a value‹ [[R103]]). A map value is handed back as the address of its eight
/// bytes, `?`'s for a missing key.
fn run_get(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a get node `build_get` built; the slot is an instance's eight
    // bytes, null, `?` or a table `run_put` allocated in the store.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let key = run_key(rt, *ops.add(1))?;
        let (_, table) = table_slot(rt, *ops)?;
        if let Some(&v) = table.as_ref().and_then(|t| t.get(&key)) {
            return Ok(v);
        }
        let types = rt.types();
        let v = super::read::output_type(types, node);
        if v == types.type_ || v == types.dyad_ {
            Ok(types.unknown as i64)
        } else if is_hashmap(types, v) {
            Ok(types.hashmap.unknown_bytes as i64)
        } else {
            Err(RunError::MissingKey)
        }
    }
}

/// The address of the empty map's eight bytes, which the place taking the value copies.
fn run_fresh(rt: &mut Runtime, _node: DyadPtr) -> Result<i64, RunError> {
    Ok(rt.types().hashmap.empty_bytes as i64)
}

/// A new table of the entries `from_scope` read, each key and value run in written order, a map
/// among the values copied as `=` copies one; the address of eight bytes holding it, which the
/// place taking the value copies.
fn run_entries(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an entries node `from_scope` built, `[op, map type, key, value, …,
    // null]`; a map value's address is that of a map's eight bytes.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let types: *const Core = rt.types();
        let (_, v) = params_of(&*types, *ops.add(1)).expect("`from_scope` keeps a hashmap mint");
        let mut entries = Vec::new();
        let mut at = 2;
        while !(*ops.add(at)).is_null() {
            let key = run_key(rt, *ops.add(at))?;
            let value = if is_hashmap(&*types, v) {
                let addr = super::by_copy::record_addr(rt, *ops.add(at + 1))?;
                held_copy(rt.store(), &*types, std::ptr::read_unaligned(addr as *const _), v)
            } else {
                rt.run(*ops.add(at + 1))?
            };
            entries.push((key, value));
            at += 2;
        }
        let table = rt.store().alloc_table();
        (*table).extend(entries);
        Ok(rt.store().alloc_bytes(&(table as usize).to_ne_bytes()) as i64)
    }
}

fn run_eq_unknown(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    is_unknown(rt, node).map(i64::from)
}

fn run_ne_unknown(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    is_unknown(rt, node).map(|unknown| i64::from(!unknown))
}

/// Whether the map a compare node reads holds `?`.
fn is_unknown(rt: &mut Runtime, node: DyadPtr) -> Result<bool, RunError> {
    // SAFETY: `node` is a compare node `build_unknown_compare` built, `[lhs, rhs]` under a mint
    // for the sides' types, one side a map whose value is the address of its eight bytes.
    unsafe {
        let sides = dyad::value(node) as *const DyadPtr;
        let lhs_is_map = super::mint::key_of(dyad::ty(node))
            .is_some_and(|key| key.first().is_some_and(|&t| is_hashmap(rt.types(), t)));
        let map = if lhs_is_map { *sides } else { *sides.add(1) };
        let addr = super::by_copy::record_addr(rt, map)?;
        Ok(holds_unknown(rt.types(), std::ptr::read_unaligned(addr as *const *mut Table)))
    }
}

/// The operands run first, so the table is read after anything they write. A map value is
/// handed over as the address of its eight bytes, a copy `=` took, and kept in eight bytes of
/// its own.
fn run_put(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `run_get`; a null table is replaced by a fresh one before the insert. A map
    // value's address is that of a map's eight bytes, null or a table `run_put` allocated.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let key = run_key(rt, *ops.add(1))?;
        let value = if is_hashmap(rt.types(), *ops.add(3)) {
            let addr = super::by_copy::record_addr(rt, *ops.add(2))?;
            let table = std::ptr::read_unaligned(addr as *const *mut Table);
            rt.store().alloc_bytes(&(table as usize).to_ne_bytes()) as i64
        } else {
            rt.run(*ops.add(2))?
        };
        let (slot, mut table) = table_slot(rt, *ops)?;
        if table.is_null() {
            table = rt.store().alloc_table();
            std::ptr::write_unaligned(slot, table);
        }
        (*table).insert(key, value);
        Ok(0)
    }
}
