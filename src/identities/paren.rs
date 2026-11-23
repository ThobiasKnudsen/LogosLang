// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `(` and `)`: the scope delimiters. `(` builds a group, never a call: `X (…)`
//! is X's constructor's decision. Neither is ever a node's type after parsing,
//! so they carry no run or compile behaviour.
//! DESIGN ›The scope's constructor is the driver‹

use super::{meta, Cx};
use crate::dyad::DyadPtr;

pub(super) fn register(cx: &mut Cx) -> (DyadPtr, DyadPtr) {
    let record = meta::token_record(cx, meta::prec::OPEN, crate::parse::Assoc::Left, &[]);
    let open = cx.store.alloc_head(cx.type_, record);
    cx.declare("(", open);
    cx.metas.insert(open, |p, _id, tape| {
        let body = p.parse_paren_cell(tape)?;
        p.expect_close()?;
        // A group gives what its line gives, a line held for the run giving it in a `dyad`.
        // SAFETY: `body` holds a reduced dyad just parsed.
        unsafe { super::waiting::lands_in(p.types(), body.dyad, p.types().dyad_) };
        tape.place_bracket_cell(body);
        Ok(crate::parse::Constructed::Placed)
    });

    let record = meta::token_record(cx, meta::prec::INERT, crate::parse::Assoc::Left, &[]);
    let close = cx.store.alloc_head(cx.type_, record);
    cx.declare(")", close);

    (open, close)
}
