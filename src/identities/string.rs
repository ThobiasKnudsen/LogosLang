// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `string` and its `«…»` literal: a plain value, inert in the seed (nothing
//! consumes one yet). Storage: `[len: u64][bytes]`, the native-endian length
//! then the UTF-8 text; the type node self-describes via [`STRING_TAG`].

use super::numtype::STRING_TAG;
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError, Parser, ParsingTape};
use crate::store::Store;

/// Fills the record into the type node the build minted first (every record's
/// `name` is a string node, so the type exists before the first declaration).
/// The literal has no escapes yet, so a `»` cannot occur inside the text.
pub(crate) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, STRING_TAG, meta::prec::LITERAL);
    let id = cx.string_;
    // SAFETY: `id` is the string type node the build allocated, its value null until now.
    unsafe { (*id).value = record };
    cx.declare("«[^»]*»", id);
    cx.metas.insert(id, construct);
    id
}

/// The guillemets are two bytes each in UTF-8.
fn construct(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    let span = tape.own_text().ok_or(ParseError::BadLiteral)?;
    let inner = &span.as_bytes()[2..span.len() - 2];
    let node = build_text(p.store(), id, inner);
    tape.place(node);
    Ok(Constructed::Placed)
}

pub(crate) fn build_text(store: &mut Store, string_ty: DyadPtr, text: &[u8]) -> DyadPtr {
    let mut blob = Vec::with_capacity(8 + text.len());
    blob.extend_from_slice(&(text.len() as u64).to_ne_bytes());
    blob.extend_from_slice(text);
    let value = store.alloc_bytes(&blob);
    store.alloc_raw(string_ty, value)
}

/// # Safety
/// `node` must be a string node built by [`build_text`] (its value the
/// `[len, bytes]` blob), and the slice must not outlive the store.
pub(crate) unsafe fn text<'a>(node: DyadPtr) -> &'a [u8] {
    let p = (*node).value;
    let len = std::ptr::read_unaligned(p as *const u64) as usize;
    std::slice::from_raw_parts(p.add(8), len)
}
