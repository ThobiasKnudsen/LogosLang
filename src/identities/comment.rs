// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The comment node: `{type: comment, value -> string node}`, prose as
//! reflectable structure, invisible to value flow. Built by `#` from a `«…»`
//! string or raw text to the end of the line.
//! DESIGN ›Text literals are plain values; `#` is the one comment constructor‹

use super::numtype::COMMENT_TAG;
use super::{meta, Cx};
use crate::dyad::DyadPtr;

/// The `comment` logos has no spelling and no run entry: the interpreter's data
/// path yields unit for it off its tag. `#` is the constructor.
pub(crate) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, COMMENT_TAG, meta::prec::INERT);
    let id = cx.store.alloc_raw(cx.type_, record);

    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::LITERAL);
    let hash = cx.store.alloc_raw(cx.type_, record);
    cx.declare("#", hash);
    cx.metas.insert(hash, |p, _id, tape| p.construct_comment(tape));
    id
}
