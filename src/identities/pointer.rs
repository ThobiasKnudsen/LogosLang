// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Pointers: the `@T` type, `&x` address-of, `x@` dereference (`p@.x` folds the field
//! offset into the deref node), and store-through `p@ = v`. Prefix `@` is the type, `@`
//! after a completed dyad is deref, so no ambiguity exists. A pointer value is an
//! ordinary 8-byte scalar; raw and unchecked in the seed.

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::{self, NumType};
use super::{commit_if_literal, meta, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// `deref` and `storeptr` have no spelling: the parser builds deref nodes from postfix
/// `@` and storeptr nodes from `=` over a deref.
pub(super) fn register(
    cx: &mut Cx,
    cs: &Callables,
) -> (DyadPtr, DyadPtr, DyadPtr, DyadPtr, DyadPtr, DyadPtr, DyadPtr) {
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::TIGHT);
    let at = cx.store.alloc_raw(cx.type_, record);
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

    let record = meta::record_assoc(
        cx.store,
        meta::TOKEN_TAG,
        meta::prec::ADDRESS,
        crate::parse::Assoc::Right,
    );
    let amp = cx.store.alloc_raw(cx.type_, record);
    cx.declare("&", amp);
    cx.metas.insert(amp, |p, _id, tape| p.construct_address_of(tape));

    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["pointer", "pointee", "offset", "op"],
    );
    let deref = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(deref, lower_deref);
    let deref_leaf = callable::mint_native(cx.store, cs.callable, run_deref, cs.seed_native);

    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["pointer", "value", "pointee", "offset", "op"],
    );
    let storeptr = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(storeptr, lower_storeptr);
    let storeptr_leaf = callable::mint_native(cx.store, cs.callable, run_storeptr, cs.seed_native);

    // `addr` has no spelling beyond the `&` token; `[place, pointee, op]`.
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["place", "pointee", "op"],
    );
    let addr = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(addr, lower_addr);
    let addr_leaf = callable::mint_native(cx.store, cs.callable, run_addr, cs.seed_native);

    (deref, storeptr, addr, deref_leaf, storeptr_leaf, addr_leaf, at)
}

/// The address resolves at run/lower time through `place_addr`, so a frame-relative
/// local yields a per-activation address.
///
/// # Safety
/// `place` must be a storage-backed place node from the store.
pub(crate) unsafe fn build_addr(store: &mut Store, types: &Core, place: DyadPtr) -> DyadPtr {
    let pointee = (*place).ty;
    let value = store.alloc_operands(&[place, pointee, types.ops.addr_]);
    store.alloc_raw(types.addr_, value)
}

fn run_addr(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an addr node; its first operand is a place.
    unsafe {
        let place = *((*node).value as *const DyadPtr);
        Ok(rt.place_addr(place).ok_or(RunError::NoActivation)? as i64)
    }
}

fn lower_addr(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is an addr node; its first operand is a place.
    unsafe {
        let place = *((*node).value as *const DyadPtr);
        lw.place_addr(place)
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
    let ty = unsafe { make_pointer_type(store, types.type_, pointee) };
    let storage = store.alloc_bytes(&(addr as usize as u64).to_ne_bytes());
    store.alloc_raw(ty, storage)
}

/// One node per pointee: the first mint is written into the pointee's record and every
/// later `@pointee` reads it back, so two spellings of `@i32` are one type. A pointee
/// carrying no record gets a fresh node; an owning pointer type is never interned.
///
/// # Safety
/// `pointee` must be a type node from the store.
pub(crate) unsafe fn make_pointer_type(
    store: &mut Store,
    type_: DyadPtr,
    pointee: DyadPtr,
) -> DyadPtr {
    let interned = super::meta::kind_of(pointee).is_some();
    if interned {
        let known = super::meta::pointer_type_of(pointee);
        if !known.is_null() {
            return known;
        }
    }
    let value = super::meta::pointer_record(store, pointee);
    let node = store.alloc_raw(type_, value);
    if interned {
        super::meta::install_pointer_type(pointee, node);
    }
    node
}

/// The same record as `make_pointer_type` with `destructor` filled: what `alloc` mints,
/// so `drop`/`own` recognize owning-ness by the slot while a borrow's pointer has none.
/// Never interned.
///
/// # Safety
/// `destructor` must be a callable leaf running the owning pointer's teardown.
pub(crate) unsafe fn make_owning_pointer_type(
    store: &mut Store,
    type_: DyadPtr,
    pointee: DyadPtr,
    destructor: DyadPtr,
) -> DyadPtr {
    let value = super::meta::pointer_record(store, pointee);
    let node = store.alloc_raw(type_, value);
    super::meta::install_destructor(node, destructor);
    node
}

/// The offset is a committed u64 literal node, so the graph stays self-describing.
pub(crate) fn build_deref(
    store: &mut Store,
    types: &Core,
    ptr_expr: DyadPtr,
    pointee: DyadPtr,
    offset: usize,
) -> DyadPtr {
    let off_bytes = store.alloc_bytes(&(offset as u64).to_ne_bytes());
    let off_node = store.alloc_raw(types.numtypes[NumType::U64 as usize], off_bytes);
    let value = store.alloc_operands(&[ptr_expr, pointee, off_node, types.ops.deref_]);
    store.alloc_raw(types.deref_, value)
}

/// # Safety
/// `node` must be a deref node from `build_deref`.
pub(crate) unsafe fn deref_parts(node: DyadPtr) -> (DyadPtr, DyadPtr, u64) {
    let p = (*node).value as *const DyadPtr;
    let off = std::ptr::read_unaligned((**p.add(2)).value as *const u64);
    (*p, *p.add(1), off)
}

/// The pointee must be a place with a whole-value store (a whole record cannot be stored);
/// a literal rhs commits to a numeric pointee and is refused for an address pointee, where
/// it would become a wild address.
///
/// # Safety
/// `deref` must be a deref node; `rhs` a reduced dyad, both from the store.
pub(crate) unsafe fn build_storeptr(
    store: &mut Store,
    types: &Core,
    deref: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let (ptr_expr, pointee, _) = deref_parts(deref);
    let off_node = *(((*deref).value as *const DyadPtr).add(2));
    // SAFETY: `pointee` is the deref node's pointee type, a node from the store.
    let scalar_pointee = unsafe { super::read::place_layout(types, pointee) }
        .is_some_and(|(read, _)| matches!(read, super::read::Read::Scalar(_)));
    if super::read::cell_numtype(types, pointee).is_none() {
        return Err(ParseError::BadAssignTarget);
    }
    let rhs = if (*types.through(rhs)).ty == types.rational {
        if !scalar_pointee {
            return Err(ParseError::TypeMismatch);
        }
        let nt = numtype::of_type_node(pointee);
        commit_if_literal(store, types, rhs, &Operand::Literal, pointee, nt)?
    } else {
        super::check_store_type(types, pointee, rhs)?;
        rhs
    };
    let value = store.alloc_operands(&[ptr_expr, rhs, pointee, off_node, types.ops.storeptr_]);
    Ok(store.alloc_raw(types.storeptr_, value))
}

/// The address `p@` names, the base checked as `run_deref` checks it.
///
/// # Safety
/// `node` must be a deref node from `build_deref`.
pub(crate) unsafe fn deref_addr(rt: &mut Runtime, node: DyadPtr) -> Result<*mut u8, RunError> {
    let (ptr_expr, _, off) = deref_parts(node);
    let base = rt.run(ptr_expr)? as u64;
    if base == 0 {
        return Err(RunError::NullPointer);
    }
    Ok(base.wrapping_add(off) as *mut u8)
}

/// A record pointee has no whole-value read; its fields go through `p@.x`.
fn run_deref(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a deref node; its parts are valid dyads.
    unsafe {
        let (ptr_expr, pointee, off) = deref_parts(node);
        let nt = super::read::cell_numtype(rt.types(), pointee).ok_or(RunError::NotDerefable)?;
        // The base is checked, not base + offset, so a field read through a null `p` is
        // caught at any offset; reading a hole is the checked error, never undefined.
        let base = rt.run(ptr_expr)? as u64;
        if base == 0 {
            return Err(RunError::NullPointer);
        }
        let addr = base.wrapping_add(off) as *const u8;
        Ok(numtype::read_scalar_nt(nt, addr))
    }
}

fn run_storeptr(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a storeptr node from `build_storeptr`.
    unsafe {
        let p = (*node).value as *const DyadPtr;
        let (ptr_expr, rhs, pointee) = (*p, *p.add(1), *p.add(2));
        let off = std::ptr::read_unaligned((**p.add(3)).value as *const u64);
        let nt = super::read::cell_numtype(rt.types(), pointee).ok_or(RunError::NotDerefable)?;
        let bits = rt.run(rhs)?;
        // Same guard as `run_deref`.
        let base = rt.run(ptr_expr)? as u64;
        if base == 0 {
            return Err(RunError::NullPointer);
        }
        let addr = base.wrapping_add(off) as *mut u8;
        numtype::write_scalar_nt(nt, addr, bits);
        Ok(bits)
    }
}

fn lower_deref(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a deref node; its parts are valid dyads.
    unsafe {
        let (ptr_expr, pointee, off) = deref_parts(node);
        let ct = super::read::cell_numtype(lw.types(), pointee)
            .ok_or(CompileError::NotDerefable)?
            .cranelift_type();
        let addr = lw.lower(ptr_expr)?;
        // The same null guard the interpreter makes, so both tiers agree.
        lw.guard_non_null(addr, ct, |s| Ok(s.load_at(ct, addr, off as i64)))
    }
}

fn lower_storeptr(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a storeptr node from `build_storeptr`.
    unsafe {
        let p = (*node).value as *const DyadPtr;
        let (ptr_expr, rhs, pointee) = (*p, *p.add(1), *p.add(2));
        let off = std::ptr::read_unaligned((**p.add(3)).value as *const u64);
        let ct = super::read::cell_numtype(lw.types(), pointee)
            .ok_or(CompileError::NotDerefable)?
            .cranelift_type();
        let v = lw.lower(rhs)?;
        let addr = lw.lower(ptr_expr)?;
        // The store happens only on the non-null arm; the yielded value is the stored one.
        lw.guard_non_null(addr, ct, |s| {
            s.store_at(ct, addr, off as i64, v);
            Ok(v)
        })
    }
}
