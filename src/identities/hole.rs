// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `?`: the one unknown, a value like any other; with a type to its left, that
//! type's valueless place, `key := T ?`, read-vetoed until its first write.
//! DESIGN ›Declarations are immutable by default‹

use super::{meta, Cx};
use crate::dyad::DyadPtr;

/// Escaped: `?` is a regex metacharacter.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::HOLE);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare(r"\?", id);
    cx.metas.insert(id, |p, id, tape| p.construct_hole(id, tape));
    id
}
