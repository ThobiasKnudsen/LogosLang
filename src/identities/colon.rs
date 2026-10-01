// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `:`, the binding read: `a:scope`, `a:name` read a name's binding itself, bypassing
//! its reading rule, at `.`'s precedence, so `b:start.rhs` is `(b:start).rhs`. The reads
//! live in [`crate::parse::Parser::construct_binding_read`].
//! DESIGN ›`:` reads a name's binding, `.` reads a thing's own fields‹

use super::{meta, Cx};
use crate::dyad::DyadPtr;

/// The trie longest-matches `:=` over `:`, so the declaration operator is untouched.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::TIGHT);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare(":", id);
    cx.metas.insert(id, |p, _id, tape| p.construct_binding_read(tape));
    id
}
