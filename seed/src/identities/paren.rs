// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `(` and `)`: the matched-paren scope delimiters. `( )` groups a sub-expression
//! and (per DESIGN ›A function's surface‹) opens a scope whose value is what its
//! body returns. These are parse-only markers: they never appear as a node's type
//! after parsing, so they carry no run or compile behaviour. v1 uses them to
//! delimit and group; pushing/popping the scope stack for declarations inside
//! comes with `struct`/`fn`.

use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;

/// Register `(` and `)`, returning their handles (the parser's expect-helpers
/// compare against them). The spellings are escaped (`\(`, `\)`) because
/// `(`/`)` are regex metacharacters; escaped, they lex as literal single bytes.
pub(super) fn register(cx: &mut Cx) -> (DyadPtr, DyadPtr) {
    // `(` is a *tight extender*: with a completed dyad to its left it is a
    // call (juxtaposition binds tightest — DESIGN ›the call paren tightest‹),
    // without one its constructor opens a grouping scope.
    let record = meta::record(cx.store, meta::TOKEN_TAG, f64::INFINITY);
    let open = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(r"\(", IdContext::new(open, cx.root_scope));
    cx.metas.insert(open, |p, _id, tape| {
        // The model's `tape[-1]`: a completed operand makes this a call on it
        // (`f(x)`, `i32(x)`, `point(3, 4)`); none opens a grouping scope whose
        // value is its body.
        match p.left_operand(tape)? {
            Some(callee) => p.parse_call(callee).map(crate::parse::Constructed::Node),
            None => {
                let body = p.parse_sequence()?;
                p.expect_close()?;
                Ok(crate::parse::Constructed::Node(body))
            }
        }
    });

    let record = meta::record(cx.store, meta::TOKEN_TAG, f64::NAN);
    let close = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(r"\)", IdContext::new(close, cx.root_scope));

    (open, close)
}
