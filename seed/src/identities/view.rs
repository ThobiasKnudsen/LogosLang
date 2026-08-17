// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `synolon`, the spelled view identity (#52; DESIGN ›The synolon's read
//! surface‹). `(synolon a)` wraps any value as its cell — a view value whose
//! hyle is the viewed node's address — and `.` then reads the cell: `.logos`,
//! `.hyle`, `.operand(i)`, `.text` on the view, and the logos members
//! (`.arity`, `.role(i)`, `.precedence`, …) on the logos `.logos` yields.
//!
//! The ruling this realizes (August 2026): `.` does exactly one job — reading
//! fields the logos defines, which are always about the hyle. A value's logos
//! is never one of its own fields, so `x.logos` does not exist; the view puts
//! the logos *into* the hyle, and only there does `.logos` read it. The reads
//! live in [`crate::parse::Parser::view_member`] / [`Parser::logos_member`]
//! and fold at parse time — comptime reflection, the regime a Logos-written
//! constructor runs in. Read-only: writing stays with constructors and the
//! tape ops (#48). The view is ambient for now, like every identity; the
//! ruled fail-closed default waits on the grant path (logos-typed
//! parameters), recorded in DESIGN.

use super::{meta, Cx};
use crate::id_context::IdContext;
use crate::synolon::SynolonPtr;

/// Register `synolon`: a fresh-start word whose constructor views the
/// expression to its right ([`crate::parse::Parser::construct_view`]).
pub(super) fn register(cx: &mut Cx) -> SynolonPtr {
    let record = meta::record(cx.store, meta::SYNOLON_TAG, f64::NAN);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("synolon", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, |p, _id, tape| p.construct_view(tape));
    id
}
