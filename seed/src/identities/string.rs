// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `string` and its `«…»` literal: the text substance (DESIGN ›Text literals are
//! plain values; `#` is the one comment constructor‹). A string literal is always
//! a plain value — never position-dependently a comment — and in this seed it is
//! *inert*: nothing consumes a string yet (no parameters, returns, casts, or
//! operations), so it exists as reflectable structure, above all as the substance
//! of comment nodes (see [`crate::identities::comment`]). The `string` *name* and
//! runtime reads arrive with the machinery that can hold them; until then the
//! interpreter refuses to read a string as a scalar (`RunError::BadValue`).
//!
//! Storage: the value points at `[len: u64][bytes]`, the native-endian length
//! then the UTF-8 text. The type node self-describes via
//! [`STRING_TAG`](crate::identities::numtype::STRING_TAG) in its own value slot,
//! so run and compile recognize string-typed data without a handle.

use super::numtype::STRING_TAG;
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Constructed, ParseError, ParsingTape, Parser, Schedule};
use crate::store::Store;

/// Register `string`: its [`STRING_TAG`] type node and the `«…»` literal pattern
/// (no escapes yet, so a `»` cannot occur inside the text; unanchored, like the
/// rational pattern, so the lexer longest-matches a prefix).
pub(crate) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, STRING_TAG, Schedule::Atom);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("«[^»]*»", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, construct);
    id
}

/// The literal's constructor: read the `«…»` span off the cursor token and
/// build the string node (the guillemets are two bytes each in UTF-8).
fn construct(p: &mut Parser, id: DyadPtr, tape: &mut ParsingTape) -> Result<Constructed, ParseError> {
    let t = tape.own_token().ok_or(ParseError::BadLiteral)?;
    let span = &p.source()[t.start..t.start + t.len];
    let inner = &span.as_bytes()[2..span.len() - 2];
    let node = build_text(p.store(), id, inner);
    Ok(Constructed::Node(node))
}

/// Build a string node `{ty: string, value -> [len, bytes]}` from raw text.
pub(crate) fn build_text(store: &mut Store, string_ty: DyadPtr, text: &[u8]) -> DyadPtr {
    let mut blob = Vec::with_capacity(8 + text.len());
    blob.extend_from_slice(&(text.len() as u64).to_ne_bytes());
    blob.extend_from_slice(text);
    let value = store.alloc_bytes(&blob);
    store.alloc_raw(string_ty, value)
}

/// The text of a string node — the reflection accessor (a comment's substance
/// and a record's role names are read through this; see [`crate::reflect`]).
///
/// # Safety
/// `node` must be a string node built by [`build_text`] (its value the
/// `[len, bytes]` blob), and the slice must not outlive the store.
pub(crate) unsafe fn text<'a>(node: DyadPtr) -> &'a [u8] {
    let p = (*node).value;
    let len = std::ptr::read_unaligned(p as *const u64) as usize;
    std::slice::from_raw_parts(p.add(8), len)
}
