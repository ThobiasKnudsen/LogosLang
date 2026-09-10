// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `regex`: the reader that turns a quote into a recognizer (#114; DESIGN ›The
//! scope's constructor is the driver‹, ruled 10 September 2026: "`regex` is an
//! ordinary identity that reads its own quote, as `lex` does, and yields a
//! recognizer, which is what `:=` enters into the trie — the third reader of
//! the one quote beside text (`«…»`) and code (`lex «…»`) … a pattern that is
//! a pure literal splits onto the literal path anyway (the seed's regex
//! splitting), so one door serves a quoted literal (`regex «..»`) and a
//! pattern (`regex «[0-9]+»`) alike").
//!
//! `regex «[0-9]+[kK]» := type (…)` therefore declares a spelling no bare
//! name could: the node `regex` places holds the pattern's bytes, and `:=`
//! with that node to its left enters them into the index as written, never
//! escaped ([`crate::parse::ScopeStack::declare_pattern`]). The pattern is
//! compiled at the read, so a bad one is the checked error at its quote. The
//! seed's quote has no escapes yet, so the bytes are the quote's own; the
//! ruling's "escapes apply before the pattern is read" waits on them.

use super::numtype::STRING_TAG;
use super::{meta, Cx};
use crate::dyad::DyadPtr;

/// Register `regex`: a text-shaped record (its value is read as text, like a
/// string's) at the rank of a word that consumes raw text to its right at
/// discovery, the rank `import` has.
pub(crate) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, STRING_TAG, meta::prec::IMPORT);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("regex", id);
    cx.metas.insert(id, |p, _id, tape| p.construct_regex(tape));
    id
}
