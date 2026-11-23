// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Pointers: the `@T` type, `&x` address-of, `x@` dereference (`p@.x` folds the field
//! offset into the deref node), and where a place's bytes lie. Prefix `@` is the type, `@`
//! after a completed dyad is deref, so no ambiguity exists. A pointer value is an
//! ordinary 8-byte scalar; raw and unchecked in the seed.

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::NumType;
use super::read::carry_of;
use super::type_mint;
use super::{assign, meta, mint, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// The identities this file registers.
pub(super) struct PointerIds {
    pub(super) deref: DyadPtr,
    pub(super) amp: DyadPtr,
    pub(super) addr: DyadPtr,
    pub(super) deref_leaf: DyadPtr,
    pub(super) addr_leaf: DyadPtr,
    pub(super) at: DyadPtr,
    pub(super) convert_ctor: DyadPtr,
}

/// `deref` has no spelling: the parser builds deref nodes from postfix `@`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> PointerIds {
    let sides = &["lhs", "rhs"];
    let record = meta::token_record(cx, meta::prec::TIGHT, crate::parse::Assoc::Left, sides);
    let at = cx.store.alloc_head(cx.type_, record);
    cx.declare("@", at);
    // A completed dyad to the left makes `@` a postfix deref; none makes it the type prefix.
    cx.metas.insert(at, |p, _id, tape| {
        match p.left_operand(tape)? {
            Some(left) => {
                // SAFETY: `left` is a reduced dyad off the tape.
                let node = unsafe { p.build_deref(left) }?;
                tape.remove(-1); // the consumed left
                tape.place(node);
            }
            None => return p.construct_pointer_type(tape),
        }
        Ok(crate::parse::Constructed::Placed)
    });

    // `&x`'s node is typed by `&`'s mint for the pointee's type (›An operation mints through a
    // chooser, one mint per set of operand types, so each mint's `run` body is parsed once‹
    // [[R301]]).
    let record = address_record(cx, meta::prec::ADDRESS, Assoc::Right);
    let amp = cx.store.alloc_head(cx.type_, record);
    cx.declare("&", amp);
    cx.metas.insert(amp, |p, _id, tape| p.construct_address_of(tape));
    cx.lower.insert(amp, lower_addr);
    cx.choosers.insert(amp, choose_address);

    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["pointer", "pointee", "offset", "op", "output_type"],
    );
    let deref = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(deref, lower_deref);
    let deref_leaf = callable::mint_native(cx.store, cs.callable, run_deref, cs.seed_native);

    // `addr` has no spelling: the address a node no `&` was written for hands out, a dyad view's
    // `.value` or a receiver's.
    let record = address_record(cx, meta::prec::INERT, Assoc::Left);
    let addr = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(addr, lower_addr);
    cx.choosers.insert(addr, choose_address);
    let addr_leaf = callable::mint_native(cx.store, cs.callable, run_addr, cs.seed_native);

    // Every pointer type a program writes has this constructor, minted with the type.
    let entry = construct_conversion as crate::parse::ConstructFn as usize;
    let convert_ctor = callable::mint(cx.store, cs.callable, entry, cs.seed_parse);
    cx.pointer_ctor = convert_ctor;

    PointerIds { deref, amp, addr, deref_leaf, addr_leaf, at, convert_ctor }
}

/// A pointer type at the center takes the pointer to its right as a written conversion, landing
/// it as an unwritten one lands and giving it this type: with `a := mut i32 5`, `s := @i32 &a`
/// points at `a` read only, and `@ mut i32 &b` of a read-only `b` is refused (›A pointer points
/// at a gated type: `&a` of a `mut i32` is `@ mut i32`, and `mut` before a pointer gates the
/// pointer itself‹ [[R382]]). Nothing to take, and the type stands as itself.
fn construct_conversion(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, crate::parse::ParseError> {
    use crate::parse::{Constructed, ParseError};
    let types = p.types();
    let Some(operand) = p.type_operand(tape, 1)? else {
        return Ok(Constructed::Decline);
    };
    if p.takes_unknown(tape, &operand, id)? {
        return Ok(Constructed::Placed);
    }
    let value = if operand.is_bracket() {
        // SAFETY: a bracket cell holds its node from the store.
        match unsafe { p.args_of(operand.dyad) }[..] {
            [value] => value,
            _ => return Err(p.fail_at(operand.start, ParseError::UnsupportedOperands)),
        }
    } else {
        p.as_operand(operand)?
    };
    tape.remove(1);
    // SAFETY: `value` is a reduced dyad from the store; `id` is this pointer type's node.
    let node = unsafe { convert_to(p.store(), types, value, id) }
        .map_err(|e| p.fail_at(operand.start, e))?;
    tape.place(node);
    Ok(Constructed::Placed)
}

/// `value` landed in the pointer type `ty`, under a conversion where its own type differs.
///
/// # Safety
/// `value` must be a reduced dyad and `ty` a pointer type node, both from the store.
unsafe fn convert_to(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    ty: DyadPtr,
) -> Result<DyadPtr, crate::parse::ParseError> {
    let landed = super::land(store, types, value, ty)?;
    let from = super::read::output_type(types, landed);
    if from == ty {
        return Ok(landed);
    }
    Ok(super::convert::build_convert(store, types, landed, from, ty))
}

fn address_record(cx: &mut Cx, parse_rank: f64, assoc: Assoc) -> *mut u8 {
    meta::operand_record(cx, meta::TUPLE_TAG, parse_rank, assoc, &["place", "held"])
}

/// `[place, held]` under `addr`'s mint. The address resolves at run/lower time, so a
/// frame-relative local yields a per-activation address. `held` is null for a place, and for a
/// value nothing names the declaration that lands it in `place`, an unspelled name its scope
/// holds, run before the address is handed out.
///
/// # Safety
/// `place` must be a frame's place or a deref node from the store; `held` null or a declare
/// node whose binding is `place`.
pub(crate) unsafe fn build_addr(
    store: &mut Store,
    types: &Core,
    place: DyadPtr,
    held: DyadPtr,
) -> DyadPtr {
    let pointee = super::read::output_type(types, place);
    build_address(store, types, types.addr_, place, held, pointee)
}

/// An address node of `ty`, `&` or `addr`, under its mint for `pointee`, handing the address out
/// as `@pointee`, which need not be the type the place's bytes are read by: `void` erases it.
///
/// # Safety
/// As [`build_addr`]; `ty` `&` or `addr`, `pointee` a type node from the store.
pub(crate) unsafe fn build_address(
    store: &mut Store,
    types: &Core,
    ty: DyadPtr,
    place: DyadPtr,
    held: DyadPtr,
    pointee: DyadPtr,
) -> DyadPtr {
    mint::node_of_any(store, types, ty, &[pointee], &[place, held])
}

/// What an address mint over the field type `key`, the pointee, gives back, `@pointee`, and runs;
/// stand-in for #462.
///
/// # Safety
/// `key` must hold a type node from the store.
unsafe fn choose_address(store: &mut Store, types: &Core, key: &[DyadPtr]) -> Option<mint::Made> {
    let &[pointee] = key else {
        return None;
    };
    let output_type = make_pointer_type(store, types, pointee);
    Some(mint::Made { output_type, op: types.ops.addr_ })
}

/// The place whose address `node` hands out when it runs the address operation, `&x`'s or an
/// `addr` node's; `None` for any other node.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn address_place(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    use super::read::{read_kind, Dispatch, Read};
    let node = types.through(node);
    let addresses = matches!(
        read_kind(types, node),
        Read::Executable(Dispatch::Leaf(leaf)) if leaf == types.ops.addr_
    );
    addresses.then(|| types.through(addr_parts(node).0))
}

/// # Safety
/// `node` must be an address node from [`build_address`].
unsafe fn addr_parts(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p, *p.add(1))
}

fn run_addr(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an address node from `build_address`.
    unsafe {
        let (place, held) = addr_parts(node);
        if !held.is_null() {
            rt.run(held)?;
        }
        place_addr(rt, place).map(|addr| addr as i64)
    }
}

fn lower_addr(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is an address node from `build_address`.
    unsafe {
        let (place, held) = addr_parts(node);
        if !held.is_null() {
            lw.lower(held)?;
        }
        lower_at_place(lw, place, NumType::U64.cranelift_type(), |_, addr| Ok(addr))
    }
}

/// An `@pointee` value holding `addr`: a pointer literal with its own eight bytes of
/// storage. How a node is handed to a native by identity.
pub(crate) fn address_value(
    store: &mut Store,
    types: &Core,
    pointee: DyadPtr,
    addr: DyadPtr,
) -> DyadPtr {
    // SAFETY: `pointee` is the type node the address was taken of.
    let ty = unsafe { make_pointer_type(store, types, pointee) };
    store.alloc_blob(ty, &(addr as usize as u64).to_ne_bytes())
}

/// `@pointee`, one node per pointee (›Every type is minted by its contents through `type_mint`;
/// the name that holds a `type ( … )` gives it its identity‹ [[R383]]).
///
/// # Safety
/// `pointee` must be a type node from the store.
///
/// # Panics
/// Where `type_mint`'s own functions fail: a defect of the seed.
pub(crate) unsafe fn make_pointer_type(
    store: &mut Store,
    types: &Core,
    pointee: DyadPtr,
) -> DyadPtr {
    let (type_, ctor) = (types.type_, types.pointer_ctor);
    mint_pointer(store, types, types.at_, pointee, |store| {
        new_pointer_type(store, type_, pointee, ctor)
    })
}

/// The pointer type `former` makes of `pointee`: the one `type_mint` keeps under `former` and the
/// pointee, or the one `make` builds the first time. A pointee carrying no record gets a fresh
/// node.
///
/// # Safety
/// `pointee` must be a type node from the store, and `make` must give a pointer type node of it.
///
/// # Panics
/// As [`make_pointer_type`].
unsafe fn mint_pointer(
    store: &mut Store,
    types: &Core,
    former: DyadPtr,
    pointee: DyadPtr,
    make: impl FnOnce(&mut Store) -> DyadPtr,
) -> DyadPtr {
    // A gated pointee is one node per gated type already, kept by `type_mint`.
    if super::meta::kind_of(pointee).is_none() && !super::gate::is_node(pointee) {
        return make(store);
    }
    type_mint::mint_of(store, types, former, &[pointee], |store| Some(make(store)))
        .expect("a pointer type is made")
}

/// A pointer type taking the value to its right as [`construct_conversion`] takes it, ranked as
/// ›A parse is ranked by what it takes from its right‹ [[R116]] ranks a parse that takes a value.
unsafe fn new_pointer_type(
    store: &mut Store,
    type_: DyadPtr,
    pointee: DyadPtr,
    ctor: DyadPtr,
) -> DyadPtr {
    let value = super::meta::pointer_record(store, pointee, Assoc::Right, meta::prec::POINTER_TYPE);
    let node = store.alloc_head(type_, value);
    meta::install_constructor(node, ctor);
    node
}

/// `@pointee` while the seed registers its identities, before `@` itself is one.
///
/// # Safety
/// `pointee` must be a type node from the store.
pub(super) unsafe fn early_pointer_type(cx: &mut Cx, pointee: DyadPtr) -> DyadPtr {
    let (type_, ctor) = (cx.type_, cx.pointer_ctor);
    if super::meta::kind_of(pointee).is_none() {
        return new_pointer_type(cx.store, type_, pointee, ctor);
    }
    let made = |store: &mut Store| Some(new_pointer_type(store, type_, pointee, ctor));
    type_mint::early(cx.store, std::ptr::null_mut(), &[pointee], made)
        .expect("a pointer type is made")
}

/// `@dyad`, what a read of a cell, a line, a field, a name or a scope gives back.
pub(crate) fn at_dyad(store: &mut Store, types: &Core) -> DyadPtr {
    // SAFETY: `dyad_` is a type node `Core::build` minted.
    unsafe { make_pointer_type(store, types, types.dyad_) }
}

/// `own @pointee`, one node per pointee as `@pointee` is, kept under `own`: a pointer record
/// whose `destructor` runs the teardown, so `free`/`move` recognize owning-ness by the slot while
/// a borrow's pointer has none.
///
/// # Safety
/// `pointee` must be a type node from the store.
///
/// # Panics
/// As [`make_pointer_type`].
pub(crate) unsafe fn make_owning_pointer_type(
    store: &mut Store,
    types: &Core,
    pointee: DyadPtr,
) -> DyadPtr {
    let (type_, teardown) = (types.type_, types.ops.teardown_);
    mint_pointer(store, types, types.own_, pointee, |store| {
        let value = meta::pointer_record(store, pointee, Assoc::Left, meta::prec::INERT);
        let node = store.alloc_head(type_, value);
        meta::install_destructor(node, teardown);
        node
    })
}

/// The offset is a committed u64 literal node, so the graph stays self-describing.
pub(crate) fn build_deref(
    store: &mut Store,
    types: &Core,
    ptr_expr: DyadPtr,
    pointee: DyadPtr,
    offset: usize,
) -> DyadPtr {
    let off_node =
        store.alloc_blob(types.numtypes[NumType::U64 as usize], &(offset as u64).to_ne_bytes());
    store.alloc_words(types.deref_, &[ptr_expr, pointee, off_node, types.ops.deref_, pointee])
}

/// # Safety
/// `node` must be a deref node from `build_deref`.
pub(crate) unsafe fn deref_parts(node: DyadPtr) -> (DyadPtr, DyadPtr, u64) {
    let p = dyad::value(node) as *const DyadPtr;
    let off = std::ptr::read_unaligned(dyad::value(*p.add(2)) as *const u64);
    (*p, *p.add(1), off)
}

/// Whether the dereference reads a field `p@.f` folded into it: past the record's start, or of
/// another type than the record its pointer points at.
///
/// # Safety
/// `node` must be a deref node from `build_deref`.
pub(crate) unsafe fn reads_folded_field(types: &Core, node: DyadPtr) -> bool {
    let (ptr, pointee, offset) = deref_parts(node);
    if offset != 0 {
        return true;
    }
    let target = super::read::output_type(types, ptr);
    if target.is_null() || !super::numtype::is_pointer_type(target) {
        return false;
    }
    let record = super::numtype::pointee_of(target);
    !record.is_null() && record != pointee && meta::is_record_type(record)
}

/// The type of `place` with its gates, what `&place` points at: the gated type of `name`, the
/// name `place` was read from, or what the pointer a dereference reads through points at. A
/// place reached any other way, a field read through a dereference among them, gives its type
/// with no gate.
///
/// # Safety
/// `name` must be null or a binding dyad, and `place` a frame's place or a deref node, from the
/// store.
pub(crate) unsafe fn gated_place_type(types: &Core, name: DyadPtr, place: DyadPtr) -> DyadPtr {
    let plain = super::read::output_type(types, place);
    let gated = if !name.is_null() {
        super::gate::word_of_name(types, name)
    } else if dyad::ty(place) == types.deref_ && !reads_folded_field(types, place) {
        let target = super::read::output_type(types, deref_parts(place).0);
        if super::numtype::is_pointer_type(target) {
            super::numtype::gated_pointee_of(target)
        } else {
            plain
        }
    } else {
        plain
    };
    if super::gate::under(gated) == plain {
        gated
    } else {
        plain
    }
}

/// Whether a value of the pointer type `from` lands in a place of the pointer type `to`: both
/// point at one type, and what `to` points at has no gate that what `from` points at lacks, so
/// a pointer drops a gate under it, written or not, and never gains one (›A pointer points at a
/// gated type: `&a` of a `mut i32` is `@ mut i32`, and `mut` before a pointer gates the pointer
/// itself‹ [[R382]], ›A type's `parse` decides what converts into it; unwritten, a value converts
/// only into a type that holds it exactly‹ [[R155]]).
///
/// # Safety
/// `from` and `to` must be pointer type nodes from the store.
pub(crate) unsafe fn lands_in(from: DyadPtr, to: DyadPtr) -> bool {
    use super::{gate, numtype::gated_pointee_of};
    let (from, to) = (gated_pointee_of(from), gated_pointee_of(to));
    // A gate further down stays: through a `@ mut @ i32` made of a `@ mut @ mut i32`, a `@i32`
    // could be stored where a `@ mut i32` is read.
    super::same_type(gate::under(from), gate::under(to))
        && gate::set_of(to).into_iter().all(|g| gate::has(from, g))
}

/// Whether a write through the dereference `node` is granted: what its pointer points at is
/// `mut` (›A pointer points at a gated type: `&a` of a `mut i32` is `@ mut i32`, and `mut`
/// before a pointer gates the pointer itself‹ [[R382]]).
///
/// # Safety
/// `node` must be a deref node from `build_deref`.
pub(crate) unsafe fn writes_through(types: &Core, node: DyadPtr) -> bool {
    let target = super::read::output_type(types, deref_parts(node).0);
    super::numtype::is_pointer_type(target)
        && super::gate::has(super::numtype::gated_pointee_of(target), types.mut_)
}

/// Where a place's bytes lie: a dereference's cell, its base checked as `run_deref` checks it;
/// a field's, found by the read that reaches it; or a frame's. The one address `&` hands out,
/// `=` writes through, and `free` and `move` reach.
///
/// # Safety
/// `place` must be a node `read::place_node` gave.
pub(crate) unsafe fn place_addr(rt: &mut Runtime, place: DyadPtr) -> Result<*mut u8, RunError> {
    if dyad::ty(place) == rt.types().deref_ {
        let (ptr_expr, _, off) = deref_parts(place);
        let base = rt.run(ptr_expr)? as u64;
        if base == 0 {
            return Err(RunError::NullPointer);
        }
        return Ok(base.wrapping_add(off) as *mut u8);
    }
    if super::this::is_field_read(rt.types(), place) {
        return super::this::field_addr(rt, place);
    }
    rt.place_addr(place).ok_or(RunError::NoActivation)
}

/// [`place_addr`], `None` where the place is a field of a record that is not there, its place
/// holding none (›`free` and `move` end a name; no drop flag‹ [[R60]]).
///
/// # Safety
/// As [`place_addr`].
pub(crate) unsafe fn held_place_addr(
    rt: &mut Runtime,
    place: DyadPtr,
) -> Result<Option<*mut u8>, RunError> {
    if super::this::is_field_read(rt.types(), place) {
        return super::this::held_field_addr(rt, place);
    }
    place_addr(rt, place).map(Some)
}

/// [`place_addr`]'s compiled half, handing the address to `at`, which gives a value of `ct`: a
/// dereference's only where its base is not null, as `lower_deref` reads. A field's has no
/// lowering: stand-in for #164.
///
/// # Safety
/// As [`place_addr`].
pub(crate) unsafe fn lower_at_place(
    lw: &mut Lowerer,
    place: DyadPtr,
    ct: cranelift_codegen::ir::Type,
    at: impl FnOnce(&mut Lowerer, Value) -> Result<Value, CompileError>,
) -> Result<Value, CompileError> {
    if dyad::ty(place) == lw.types().deref_ {
        let (ptr_expr, _, off) = deref_parts(place);
        let base = lw.lower(ptr_expr)?;
        return lw.guard_non_null(base, ct, |s| {
            let addr = s.offset_addr(base, off as i64);
            at(s, addr)
        });
    }
    if super::this::is_field_read(lw.types(), place) {
        return Err(CompileError::NotLowerable(place));
    }
    let addr = lw.place_addr(place)?;
    at(lw, addr)
}

/// A cell's whole read hands over what the pointee's type moves as: its word, or the address
/// of its bytes, from which a copy reads them.
fn run_deref(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a deref node; its parts are valid dyads.
    unsafe {
        let (ptr_expr, pointee, off) = deref_parts(node);
        let carry = carry_of(rt.types(), pointee).ok_or(RunError::NotDerefable)?;
        // The base is checked, not base + offset, so a field read through a null `p` is
        // caught at any offset; reading a hole is the checked error, never undefined.
        let base = rt.run(ptr_expr)? as u64;
        if base == 0 {
            return Err(RunError::NullPointer);
        }
        Ok(assign::load(carry, base.wrapping_add(off) as *mut u8))
    }
}

fn lower_deref(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a deref node; its parts are valid dyads.
    unsafe {
        let (ptr_expr, pointee, off) = deref_parts(node);
        let carry = carry_of(lw.types(), pointee).ok_or(CompileError::NotDerefable)?;
        let addr = lw.lower(ptr_expr)?;
        // The same null guard the interpreter makes, so both tiers agree.
        lw.guard_non_null(addr, assign::handed_type(carry), |s| {
            Ok(assign::lower_load(s, carry, addr, off as i64))
        })
    }
}
