// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Pointers: `@T` types, `&x` address-of, `x@` dereference, `p@.x`, and
//! store-through (`p@ = v`).
//!
//! The settled surface (Thobias, July 2026): the pointer type is **prefix**
//! `@T` — the pointer is the first thing the user interacts with, and it
//! composes (`@@i32`, `@point`) — while dereference is **postfix** `x@`, so
//! chains read left to right: `p@.x`, `p@@`. Because a dereference can never
//! *start* an expression, `@` after a completed dyad is always deref and `@`
//! elsewhere is always the type prefix; no ambiguity exists. `&x` is address-of.
//! v1 pointers are raw, unchecked addresses (DESIGN's `@`-family); checked
//! `&T`/`&mut T` references layer on when the borrow checker arrives.
//!
//! Representation: a pointer *type* is `{ty: type, value -> record}`, its
//! shared-member record [`ADDR_TAG`]-kinded with the pointee node as payload
//! (see [`crate::identities::meta`]) — created fresh per use, never interned
//! (DESIGN: ordinary source nodes are not deduped); equality anywhere compares
//! *pointees*. A pointer *value* is an ordinary 8-byte scalar (the address in
//! the i64 bit-container), so variables, parameters, struct fields, and the
//! compiled ABI all carry pointers through the existing width machinery. A
//! dereference is `{ty: deref, value: [ptr-expr, pointee-type, offset-node]}` —
//! the offset folds `p@.x` field access into the same node — and a store-through
//! is `{ty: storeptr, value: [ptr-expr, rhs, pointee-type, offset-node]}`, built
//! by `=` at parse time. Deferred, deliberately: pointer arithmetic, heap
//! allocation (pointers point at parse-allocated storage), and null-safety
//! beyond the literal-argument seam.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::{self, NumType};
use super::{commit_if_literal, meta, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError, Schedule};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register the pointer machinery: the `@` and `&` tokens (driver-dispatched),
/// and the `deref` and `storeptr` identities with their native leaves and
/// lowerings — those two have no spelling; the parser builds deref nodes from
/// postfix `@` and storeptr nodes from `=` over a deref. Returns the two
/// identities and their leaves.
pub(super) fn register(
    cx: &mut Cx,
    cs: &Callables,
) -> (DyadPtr, DyadPtr, DyadPtr, DyadPtr, DyadPtr, DyadPtr) {
    let record = meta::record(cx.store, meta::TOKEN_TAG, Schedule::At);
    let at = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("@", IdContext::new(at, cx.root_scope));
    // `@`'s constructor reads its own left context (the model's tape[-1]): a
    // completed dyad makes it a postfix deref, none makes it the pointer-type
    // prefix.
    cx.metas.insert(
        at,
        Construct::Keyword(|p, _id, left| {
            if left.is_null() {
                p.parse_pointer_type()
            } else {
                // SAFETY: `left` is the reduced dyad the driver popped for us.
                unsafe { p.build_deref(left) }
            }
        }),
    );

    let record = meta::record(cx.store, meta::TOKEN_TAG, Schedule::Amp);
    let amp = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("&", IdContext::new(amp, cx.root_scope));
    cx.metas.insert(amp, Construct::Keyword(|p, _id, _left| p.parse_address_of()));

    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        0.0,
        Assoc::Left,
        Schedule::Operand,
        &["pointer", "pointee", "offset", "op"],
    );
    let deref = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(deref, lower_deref);
    let deref_leaf = callable::mint_native(cx.store, cs.callable, run_deref, cs.seed_native);

    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        0.0,
        Assoc::Left,
        Schedule::Operand,
        &["pointer", "value", "pointee", "offset", "op"],
    );
    let storeptr = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(storeptr, lower_storeptr);
    let storeptr_leaf =
        callable::mint_native(cx.store, cs.callable, run_storeptr, cs.seed_native);

    // `addr` (prefix `&`): no spelling of its own beyond the `&` token; the
    // parser builds these from `parse_address_of`. `[place, pointee, op]`.
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        0.0,
        Assoc::Left,
        Schedule::Operand,
        &["place", "pointee", "op"],
    );
    let addr = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(addr, lower_addr);
    let addr_leaf = callable::mint_native(cx.store, cs.callable, run_addr, cs.seed_native);

    (deref, storeptr, addr, deref_leaf, storeptr_leaf, addr_leaf)
}

/// Build an address-of node `{ty: addr, value: [place, pointee, op]}` over a
/// storage-backed `place` (a variable, a struct field, a pointer variable). The
/// pointee is the place's type. Unlike a baked pointer *literal*, this node
/// resolves the address at run/lower time through `place_addr`, so a
/// frame-relative local yields a *per-activation* address — `&x` inside a
/// recursive function is a different address on each call, exactly like C.
///
/// # Safety
/// `place` must be a storage-backed place node from the store.
pub(crate) unsafe fn build_addr(store: &mut Store, types: &CoreTypes, place: DyadPtr) -> DyadPtr {
    let pointee = (*place).ty;
    let value = store.alloc_operands(&[place, pointee, types.ops.addr_]);
    store.alloc_raw(types.addr_, value)
}

/// Run an address-of: the current machine address of its place — an absolute
/// pointer for a global, `activation_base + offset` for a frame-relative local
/// of the call in progress.
fn run_addr(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an addr node; its first operand is a place.
    unsafe {
        let place = *((*node).value as *const DyadPtr);
        Ok(rt.place_addr(place).ok_or(RunError::BadValue)? as i64)
    }
}

/// Lower an address-of: the place's address as an SSA value — a baked `iconst`
/// for a global, a frame `stack_addr` for a local.
fn lower_addr(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is an addr node; its first operand is a place.
    unsafe {
        let place = *((*node).value as *const DyadPtr);
        Ok(lw.place_addr(place))
    }
}

/// Build a pointer type node `@pointee`: `{ty: type, value -> record}`, the
/// record [`ADDR_TAG`]-kinded with the pointee as its payload. Fresh per use;
/// compare pointees, not nodes.
pub(crate) fn make_pointer_type(store: &mut Store, type_: DyadPtr, pointee: DyadPtr) -> DyadPtr {
    let value = super::meta::pointer_record(store, pointee);
    store.alloc_raw(type_, value)
}

/// Build a dereference node `{ty: deref, value: [ptr-expr, pointee, offset]}`,
/// the offset carried as a committed u64 literal node so the graph stays
/// self-describing.
pub(crate) fn build_deref(
    store: &mut Store,
    types: &CoreTypes,
    ptr_expr: DyadPtr,
    pointee: DyadPtr,
    offset: usize,
) -> DyadPtr {
    let off_bytes = store.alloc_bytes(&(offset as u64).to_ne_bytes());
    let off_node = store.alloc_raw(types.numtypes[NumType::U64 as usize], off_bytes);
    let value = store.alloc_operands(&[ptr_expr, pointee, off_node, types.ops.deref_]);
    store.alloc_raw(types.deref_, value)
}

/// The `(ptr-expr, pointee, offset)` of a deref node.
///
/// # Safety
/// `node` must be a deref node as [`build_deref`] lays it out.
pub(crate) unsafe fn deref_parts(node: DyadPtr) -> (DyadPtr, DyadPtr, u64) {
    let p = (*node).value as *const DyadPtr;
    let off = std::ptr::read_unaligned((**p.add(2)).value as *const u64);
    (*p, *p.add(1), off)
}

/// Build a store-through from `=` over a deref lhs: the pointee must be a
/// scalar place (numeric or pointer — a whole struct cannot be stored,
/// [`ParseError::BadAssignTarget`]); a literal rhs commits to a numeric pointee
/// and is rejected for a pointer pointee (it would become a wild address).
///
/// # Safety
/// `deref` must be a deref node; `rhs` a reduced dyad, both from the store.
pub(crate) unsafe fn build_storeptr(
    store: &mut Store,
    types: &CoreTypes,
    deref: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let (ptr_expr, pointee, _) = deref_parts(deref);
    let off_node = *(((*deref).value as *const DyadPtr).add(2));
    let pointer_pointee = numtype::is_pointer_type(pointee);
    if !pointer_pointee && !super::is_numtype_node(types, pointee) {
        return Err(ParseError::BadAssignTarget);
    }
    let rhs = if (*rhs).ty == types.rational {
        if pointer_pointee {
            return Err(ParseError::TypeMismatch);
        }
        let nt = numtype::of_type_node(pointee);
        commit_if_literal(store, rhs, &Operand::Literal, pointee, nt)?
    } else {
        // A non-literal rhs must already be the pointee's type — no implicit
        // coercion ([`super::check_store_type`]).
        super::check_store_type(types, pointee, rhs)?;
        rhs
    };
    let value = store.alloc_operands(&[ptr_expr, rhs, pointee, off_node, types.ops.storeptr_]);
    Ok(store.alloc_raw(types.storeptr_, value))
}

/// Run a deref: evaluate the pointer, add the offset, read the pointee's scalar
/// at that address. A struct pointee has no whole-value read (fields go through
/// `p@.x`), reported as a clean `BadValue`.
fn run_deref(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a deref node; its parts are valid dyads.
    unsafe {
        let (ptr_expr, pointee, off) = deref_parts(node);
        if !numtype::is_scalar_place_type(rt.struct_type(), pointee) {
            return Err(RunError::BadValue);
        }
        let addr = (rt.run(ptr_expr)? as u64).wrapping_add(off) as *const u8;
        Ok(numtype::read_scalar(pointee, addr))
    }
}

/// Run a store-through: evaluate the rhs and the pointer, write the pointee's
/// scalar at address + offset; yields the stored value, like `=`.
fn run_storeptr(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a storeptr node as [`build_storeptr`] lays it out.
    unsafe {
        let p = (*node).value as *const DyadPtr;
        let (ptr_expr, rhs, pointee) = (*p, *p.add(1), *p.add(2));
        let off = std::ptr::read_unaligned((**p.add(3)).value as *const u64);
        let bits = rt.run(rhs)?;
        let addr = (rt.run(ptr_expr)? as u64).wrapping_add(off) as *mut u8;
        numtype::write_scalar(pointee, addr, bits);
        Ok(bits)
    }
}

/// Lower a deref: the pointer lowers to its i64 address, the pointee loads
/// through it at the folded offset.
fn lower_deref(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a deref node; its parts are valid dyads.
    unsafe {
        let (ptr_expr, pointee, off) = deref_parts(node);
        if !numtype::is_scalar_place_type(lw.struct_type(), pointee) {
            return Err(CompileError::BadValue);
        }
        let addr = lw.lower(ptr_expr)?;
        let ct = numtype::of_type_node(pointee).cranelift_type();
        Ok(lw.load_at(ct, addr, off as i64))
    }
}

/// Lower a store-through: rhs and pointer lower, the pointee stores through the
/// address at the folded offset; yields the stored value.
fn lower_storeptr(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a storeptr node as [`build_storeptr`] lays it out.
    unsafe {
        let p = (*node).value as *const DyadPtr;
        let (ptr_expr, rhs, pointee) = (*p, *p.add(1), *p.add(2));
        let off = std::ptr::read_unaligned((**p.add(3)).value as *const u64);
        let v = lw.lower(rhs)?;
        let addr = lw.lower(ptr_expr)?;
        let ct = numtype::of_type_node(pointee).cranelift_type();
        lw.store_at(ct, addr, off as i64, v);
        Ok(v)
    }
}
