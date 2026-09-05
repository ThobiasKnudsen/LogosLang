// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `?`: the hole. A constructor that builds a fresh dyad with both slots
//! undefined at every appearance — a literal, never a name, so `x := ?` binds
//! `x` to its own hole — and, with a type standing to its left, that type's
//! valueless place: `key := T ?` is the valueless declaration (DESIGN
//! ›Declarations are immutable by default‹, ruled 2 September 2026). The
//! reading of the type to its left is the constructor's ([`crate::parse::Parser::construct_hole`]).

use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;

/// Register `?` (escaped: `?` is a regex metacharacter).
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::HOLE);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(r"\?", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, |p, _id, tape| p.construct_hole(tape));
    id
}
