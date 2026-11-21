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

/// The type a hole stands for; null for a bracket parameter's slot.
///
/// # Safety
/// `node` must be a hole built by [`build`].
pub(crate) unsafe fn type_in(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr)
}

/// The type the valueless form `T ?` made declares: a hole's, or the type of the empty node `?`
/// makes of a type whose nodes run (›`array` is a chooser‹ [[R223]]).
///
/// # Safety
/// `place` must be what `T ?` made: a hole built by [`build`], or an empty node of a record type.
pub(crate) unsafe fn declared_type(unknown: DyadPtr, place: DyadPtr) -> DyadPtr {
    if is_hole(unknown, place) {
        type_in(place)
    } else {
        dyad::ty(place)
    }
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

pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::token_record(cx, meta::prec::HOLE, crate::parse::Assoc::Left, &["lhs"]);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare("?", id);
    cx.metas.insert(id, |p, id, tape| p.construct_hole(id, tape));
    id
}
