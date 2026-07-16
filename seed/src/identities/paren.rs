// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `(` and `)`: the matched-paren scope delimiters. `( )` groups a sub-expression
//! and (per DESIGN ›A function's surface‹) opens a scope whose value is what its
//! body returns. These are parse-only markers: they never appear as a node's type
//! after parsing, so they carry no run or compile behaviour. v1 uses them to
//! delimit and group; pushing/popping the scope stack for declarations inside
//! comes with `struct`/`fn`.

use super::{meta, Cx};
use crate::id_context::IdContext;
use crate::parse::Schedule;

/// Register `(` and `)`. The spellings are escaped (`\(`, `\)`) because `(`/`)`
/// are regex metacharacters; escaped, they lex as literal single bytes.
pub(super) fn register(cx: &mut Cx) {
    let record = meta::record(cx.store, meta::TOKEN_TAG, Schedule::Open);
    let open = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(r"\(", IdContext::new(open, cx.root_scope));

    let record = meta::record(cx.store, meta::TOKEN_TAG, Schedule::Close);
    let close = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(r"\)", IdContext::new(close, cx.root_scope));
}
