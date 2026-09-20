// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `:`, the record read: `a:scope`, `a:name`, `a:dyad` read a name's record
//! itself, bypassing its reading rule, at `.`'s precedence, so `a:dyad.type`
//! is `(a:dyad).type`. The reads live in [`crate::parse::Parser::construct_record_read`].
//! DESIGN ›The dyad's read surface‹

use super::{meta, Cx};
use crate::dyad::DyadPtr;

/// The trie longest-matches `:=` over `:`, so the declaration operator is untouched.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::TIGHT);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare(":", id);
    cx.metas.insert(id, |p, _id, tape| p.construct_record_read(tape));
    id
}
