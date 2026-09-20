// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `pub`: the first gate, a prefix word over a declaration, `pub x := 5`, that
//! fills the declare node's gate slot. Unmarked stays private, so there is no
//! `private` word to write.
//! DESIGN ›Read and write are one mechanism across the system‹

use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError};

/// No node is ever typed by `pub`; its identity exists to be named in a
/// declare node's gate slot.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record_assoc(
        cx.store,
        meta::TOKEN_TAG,
        meta::prec::PREFIX,
        crate::parse::Assoc::Right,
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("pub", id);
    cx.metas.insert(id, construct);
    id
}

/// Anything but a declaration to the right is a parse error, not a silent
/// no-op: a gate that marked nothing would be a lie in the source.
fn construct(
    p: &mut crate::parse::Parser,
    _id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<Constructed, ParseError> {
    let inner = p.take_right(tape)?;
    let types = p.types();
    // SAFETY: `inner` is a reduced dyad just parsed.
    unsafe {
        if (*inner).ty != types.declare_ {
            return Err(ParseError::GateNeedsDeclaration);
        }
        if !super::declare::gate_of(inner).is_null() {
            return Err(ParseError::DoubleGate);
        }
        super::declare::set_gate(inner, types.pub_);
    }
    tape.place(inner);
    Ok(Constructed::Placed)
}
