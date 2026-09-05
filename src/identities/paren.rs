// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `(` and `)`: the matched-paren scope delimiters. `( )` groups a sub-expression
//! and (per DESIGN ›A function's surface‹) opens a scope whose value is what its
//! body returns. These are parse-only markers: they never appear as a node's logos
//! after parsing, so they carry no run or compile behaviour. v1 uses them to
//! delimit and group; pushing/popping the scope stack for declarations inside
//! comes with `record`/`fn`.

use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;

/// Register `(` and `)`, returning their handles (the parser's expect-helpers
/// compare against them). The spellings are escaped (`\(`, `\)`) because
/// `(`/`)` are regex metacharacters; escaped, they lex as literal single bytes.
pub(super) fn register(cx: &mut Cx) -> (DyadPtr, DyadPtr) {
    // `(` is one identity at the discovery threshold and it builds a group,
    // never a call: `X (…)` is X's constructor's decision (DESIGN ›The scope's
    // constructor is the driver‹, ruled 3 September 2026; #59 step 2) — a
    // function value, a record logos, and a numeric logos each consume the
    // bracket to their right themselves. Two operands left side by side, as
    // in `(1 + 2) (3)`, are the checked error, not a call.
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::OPEN);
    let open = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(r"\(", IdContext::new(open, cx.root_scope));
    cx.metas.insert(open, |p, _id, tape| {
        let body = p.parse_sequence()?;
        p.expect_close()?;
        tape.place(body);
        Ok(crate::parse::Constructed::Placed)
    });

    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
    let close = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(r"\)", IdContext::new(close, cx.root_scope));

    (open, close)
}
