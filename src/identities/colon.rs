// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `:`, the record read (DESIGN ›The dyad's read surface‹, ruled 7–8 September
//! 2026): "`:` reads a name's record and `.` reads a thing's own fields". A
//! declared name's record — one per name, the trie entry — holds `dyad`,
//! `scope`, `start`, `end`, and `gate`, so `a:scope` is where `a` was
//! declared and `a:dyad` its cell, the dyad view, whose own two fields keep
//! their names: `a:dyad.type`, `a:dyad.value`. `:` is the field read on the
//! record itself, bypassing its reading rule — which is why it is a second
//! operator beside `.`, and the one reader of an operand that does not hop
//! through a record. A constructed node, having no record, answers `:` from
//! its path (DESIGN ›Meta-navigation‹, 8 September 2026). `a:type` is the
//! same checked error as a `.` read the type does not declare. Every read
//! folds at parse — comptime reflection, the regime a Logos-written
//! constructor runs in. The reads live in
//! [`crate::parse::Parser::construct_record_read`].
//!
//! Precedence: `.`'s, left-associative, so `a:dyad.type` is `(a:dyad).type`
//! and `x:dyad.type == i32` binds before the comparison — the shape DESIGN's
//! own examples fix (`f:dyad.value.body[0].rhs`, `b:start.rhs.type`,
//! `a:dyad.type is number`).

use super::{meta, Cx};
use crate::dyad::DyadPtr;

/// Register `:`. The trie longest-matches `:=` over it, so the declaration
/// operator is untouched; `:` as a declaration operator stays rejected
/// (DESIGN ›Substrate vocabulary‹, 2 September 2026).
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::DOT);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare(":", id);
    cx.metas.insert(id, |p, _id, tape| p.construct_record_read(tape));
    id
}
