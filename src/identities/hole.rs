// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `?`: the one unknown, a value like any other; with a type to its left, that
//! type's valueless place, `key := T ?`, read-vetoed until its first write.
//! A hole is a node of `?` holding `[type, default]`: what `T ?` leaves on the
//! tape, and what a record keeps for each of its fields.
//! DESIGN ›Declarations are immutable by default‹

use super::{meta, Cx};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::store::Store;

pub(crate) fn build(store: &mut Store, unknown: DyadPtr, ty: DyadPtr, default: DyadPtr) -> DyadPtr {
    store.alloc_words(unknown, &[ty, default])
}

/// # Safety
/// `node` must be null or a dyad from the store.
pub(crate) unsafe fn is_hole(unknown: DyadPtr, node: DyadPtr) -> bool {
    !node.is_null() && dyad::ty(node) == unknown
}

/// The type a hole stands for; null for a bare parameter's slot.
///
/// # Safety
/// `node` must be a hole built by [`build`].
pub(crate) unsafe fn type_in(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr)
}

/// # Safety
/// As [`type_in`].
pub(crate) unsafe fn set_type(node: DyadPtr, ty: DyadPtr) {
    *(dyad::value(node) as *mut DyadPtr) = ty;
}

/// The node a field starts as; null for none.
///
/// # Safety
/// As [`type_in`].
pub(crate) unsafe fn default_in(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr).add(1)
}

/// Escaped: `?` is a regex metacharacter.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::HOLE);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare(r"\?", id);
    cx.metas.insert(id, |p, id, tape| p.construct_hole(id, tape));
    id
}
