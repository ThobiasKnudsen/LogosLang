// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `regex «…»`: the reader that turns a quote into a recognizer, which `:=`
//! enters into the trie as written, so `regex «[0-9]+[kK]» := type (…)`
//! declares a spelling no bare name could. The pattern is compiled at the read.
//! DESIGN ›The scope's constructor is the driver‹

use super::numtype::STRING_TAG;
use super::{meta, Cx};
use crate::dyad::DyadPtr;

/// A text-shaped record at the rank of a word that consumes raw text to its
/// right at discovery, the rank `import` has.
pub(crate) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, STRING_TAG, meta::prec::IMPORT);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("regex", id);
    cx.metas.insert(id, |p, _id, tape| p.construct_regex(tape));
    id
}
