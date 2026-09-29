// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The node: one block, its type word first and its value bytes after it, its identity
//! its address. `dyad` is also the spelled type whose constructor builds a node,
//! `dyad (type, value)`. DESIGN ›A dyad is a type and a value: one block, the type word
//! first, and its identity is its address‹.

use super::{meta, Cx};

/// The block's first word; the value bytes follow it, laid out as the type says.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Dyad {
    pub(crate) ty: DyadPtr,
}

/// A node's handle; its address is its identity.
pub type DyadPtr = *mut Dyad;

/// Where a node's value bytes begin, for compiled code reading a node.
pub const PAYLOAD_OFFSET: i64 = std::mem::size_of::<Dyad>() as i64;

/// The readers of the block's layout; nothing outside this file spells it.
///
/// # Safety
/// `p` must be a valid dyad from the store.
pub unsafe fn ty(p: DyadPtr) -> DyadPtr {
    (*p).ty
}

/// The value bytes, right after the type word.
///
/// # Safety
/// As [`ty`].
pub unsafe fn value(p: DyadPtr) -> *mut u8 {
    (p as *mut u8).add(std::mem::size_of::<Dyad>())
}

/// The one address a head node holds: a type's record, another node, a table.
///
/// # Safety
/// As [`ty`]; `p` must be a node built by `Store::alloc_head` or `alloc_leaf`.
pub unsafe fn head(p: DyadPtr) -> *mut u8 {
    *(value(p) as *const *mut u8)
}

/// # Safety
/// As [`head`].
pub unsafe fn set_head(p: DyadPtr, v: *mut u8) {
    *(value(p) as *mut *mut u8) = v;
}

/// # Safety
/// As [`ty`].
pub unsafe fn set_ty(p: DyadPtr, t: DyadPtr) {
    (*p).ty = t;
}

/// At application rank, so `dyad (…)` reads the bracket to its right and `dyad` alone is the type.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::DYAD_TAG, meta::prec::APPLY);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare("dyad", id);
    cx.metas.insert(id, |p, id, tape| p.construct_dyad(id, tape));
    id
}
