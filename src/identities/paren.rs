// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `(` and `)`: the scope delimiters. `(` builds a group, never a call: `X (…)`
//! is X's constructor's decision. Neither is ever a node's type after parsing,
//! so they carry no run or compile behaviour.
//! DESIGN ›The scope's constructor is the driver‹

use super::{meta, Cx};
use crate::dyad::DyadPtr;

/// Escaped: `(` and `)` are regex metacharacters.
pub(super) fn register(cx: &mut Cx) -> (DyadPtr, DyadPtr) {
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::OPEN);
    let open = cx.store.alloc_raw(cx.type_, record);
    cx.declare(r"\(", open);
    cx.metas.insert(open, |p, _id, tape| {
        let body = p.parse_sequence()?;
        p.expect_close()?;
        tape.place_bracket(body);
        Ok(crate::parse::Constructed::Placed)
    });

    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
    let close = cx.store.alloc_raw(cx.type_, record);
    cx.declare(r"\)", close);

    (open, close)
}
