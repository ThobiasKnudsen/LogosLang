// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The comment node: `{type: comment, value -> string node}` — prose as reflectable
//! graph structure, per the settled design (DESIGN ›Text literals are plain
//! values; `#` is the one comment constructor‹). Built by the parser's
//! statement-level `#` ([`crate::parse::Parser::parse_comment`]) from either a
//! `«…»` string or raw text to the end of the line; the substance is the string
//! node in the value slot. A comment is invisible to value flow — the sequence
//! machinery neither runs it nor lets it be a tail — and yields unit if anything
//! forces it. Mid-expression `#`s remain trivia in the seed
//! ([`Parser::skip_trivia`](crate::parse::Parser)); the full constructor form
//! arrives at self-hosting.

use cranelift_codegen::ir::Value;

use super::numtype::COMMENT_TAG;
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;

/// Register the `comment` logos: its [`COMMENT_TAG`] node — no spelling; the
/// parser builds comment nodes from `#` — plus a unit-valued lowering as a
/// backstop should one ever be forced directly. Prose is *data*, not behaviour:
/// the interpreter's data path yields unit for a comment node off its graph tag
/// (see [`crate::run`]), so no run entry exists anywhere.
pub(crate) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, COMMENT_TAG, meta::prec::INERT);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(id, lower);

    // `#` is the one comment constructor, an identity constructed at
    // discovery like a literal (DESIGN ›Text literals are plain values; `#`
    // is the one comment constructor‹): it consumes a following `«…»` or the
    // raw text to the end of the line and leaves the comment node as its
    // cell, which the boundary lifts into the body — void-valued, invisible
    // to value flow (#59 step 3).
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::LITERAL);
    let hash = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("#", IdContext::new(hash, cx.root_scope));
    cx.metas.insert(hash, |p, _id, tape| p.construct_comment(tape));
    id
}

/// Lower: prose yields unit.
fn lower(lw: &mut Lowerer, _node: DyadPtr) -> Result<Value, CompileError> {
    Ok(lw.const_i32(0))
}
