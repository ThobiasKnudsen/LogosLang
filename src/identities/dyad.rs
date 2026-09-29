// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The node cell: a `type` pointer and a `value` pointer, its identity its address.
//! `dyad` is also the spelled type whose constructor builds a cell, `dyad (type, value)`.
//! DESIGN ›A dyad is a type and a value‹.

use super::{meta, Cx};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Dyad {
    ty: DyadPtr,
    /// A type-erased address, read through `ty`.
    value: *mut u8,
}

impl Dyad {
    pub(crate) fn new(ty: DyadPtr, value: *mut u8) -> Self {
        Dyad { ty, value }
    }
}

/// A node's handle; its address is its identity.
pub type DyadPtr = *mut Dyad;

/// Where the value word sits, for compiled code reading a cell.
pub const VALUE_OFFSET: i64 = std::mem::offset_of!(Dyad, value) as i64;

/// The four readers of the cell's layout; nothing outside this file spells it.
///
/// # Safety
/// `p` must be a valid dyad from the store.
pub unsafe fn ty(p: DyadPtr) -> DyadPtr {
    (*p).ty
}

/// # Safety
/// As [`ty`].
pub unsafe fn value(p: DyadPtr) -> *mut u8 {
    (*p).value
}

/// The one address a head node holds: a type's record, another node, a table.
///
/// # Safety
/// As [`ty`]; `p` must be a node built by `Store::alloc_head`.
pub unsafe fn head(p: DyadPtr) -> *mut u8 {
    (*p).value
}

/// # Safety
/// As [`head`].
pub unsafe fn set_head(p: DyadPtr, v: *mut u8) {
    (*p).value = v;
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
