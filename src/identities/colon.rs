// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `:`, the binding read: `a:scope`, `a:name` read a name's binding itself, bypassing
//! its reading rule, and `a:type` the type of the dyad it stands for, at `.`'s
//! precedence, so `a:type.arity` is `(a:type).arity`. The reads live in
//! [`crate::parse::Parser::construct_binding_read`].
//! DESIGN ›The dyad's read surface‹

use super::{meta, Cx};
use crate::dyad::DyadPtr;

/// The trie longest-matches `:=` over `:`, so the declaration operator is untouched.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::TIGHT);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare(":", id);
    cx.metas.insert(id, |p, _id, tape| p.construct_binding_read(tape));
    id
}
