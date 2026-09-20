// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `pub` and `mut`: the gate words, each a prefix word over a declaration
//! (`pub x := 5`, `mut x := i32 ?`, `pub mut x := 5`) that adds itself to the
//! name's record. Unmarked stays private and unwritable, so there is no
//! `private` word to write.
//! DESIGN ›Read and write are one mechanism across the system‹

use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError};

/// No node is ever typed by a gate word; its identity exists to stand on a
/// record. Returns `pub`, `mut`.
pub(super) fn register(cx: &mut Cx) -> (DyadPtr, DyadPtr) {
    let word = |cx: &mut Cx, name: &str| {
        let record = meta::record_assoc(
            cx.store,
            meta::TOKEN_TAG,
            meta::prec::PREFIX,
            crate::parse::Assoc::Right,
        );
        let id = cx.store.alloc_raw(cx.type_, record);
        cx.declare(name, id);
        cx.metas.insert(id, construct);
        id
    };
    (word(cx, "pub"), word(cx, "mut"))
}

/// Anything but a declaration to the right is a parse error, not a silent
/// no-op: a gate that marked nothing would be a lie in the source.
fn construct(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<Constructed, ParseError> {
    let inner = p.take_right(tape)?;
    let types = p.types();
    // SAFETY: `inner` is a reduced dyad just parsed.
    if unsafe { (*inner).ty } != types.declare_ {
        return Err(ParseError::GateNeedsDeclaration);
    }
    p.gate_declared(id)?;
    tape.place(inner);
    Ok(Constructed::Placed)
}
