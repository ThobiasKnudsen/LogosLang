// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `pub`, `mut`, `immut` and `shared`: the gate words, each a prefix word
//! over a declaration (`pub x := 5`, `mut x := i32 ?`, `pub mut x := 5`) that
//! adds itself to the name's binding, first in the set, so the set reads in
//! text order. Unmarked stays private and unwritable, so there is no
//! `private` word to write; `immut` vetoes the one write a field gets by
//! default, its constructor's fill. `shared` is read by the fields block's own reader,
//! and in a function body the declaration's own constructor reads it.
//! DESIGN ›Read and write are one mechanism across the system‹

use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError};

/// No node is ever typed by a gate word; its identity exists to stand on a
/// binding. Returns `pub`, `mut`, `immut`, `shared`.
pub(super) fn register(cx: &mut Cx) -> (DyadPtr, DyadPtr, DyadPtr, DyadPtr) {
    let word = |cx: &mut Cx, name: &str| {
        let record = meta::record_assoc(
            cx.store,
            meta::TOKEN_TAG,
            meta::prec::PREFIX,
            crate::parse::Assoc::Right,
        );
        let id = cx.store.alloc_raw(cx.type_, record);
        cx.declare(name, id);
        id
    };
    let pub_ = word(cx, "pub");
    cx.metas.insert(pub_, construct);
    let mut_ = word(cx, "mut");
    cx.metas.insert(mut_, construct);
    let immut_ = word(cx, "immut");
    cx.metas.insert(immut_, construct);
    let shared_ = word(cx, "shared");
    cx.metas.insert(shared_, outside_block);
    (pub_, mut_, immut_, shared_)
}

/// A fields block's reader takes the word itself; in a function body it
/// gates a name as the other words do; anywhere else it stands for nothing.
fn outside_block(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<Constructed, ParseError> {
    if !p.in_fn_body() {
        return Err(ParseError::SharedOutsideFieldsBlock);
    }
    construct(p, id, tape)
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
