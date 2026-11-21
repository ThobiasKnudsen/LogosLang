// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `pub`, `mut`, `immut` and `static`: the gate words over a declaration, each
//! a prefix word (`pub x := 5`, `mut x := i32 ?`, `pub mut x := 5`) that
//! adds itself to the name's binding, first in the set, so the set reads in
//! text order. Unmarked stays private and unwritable, so there is no
//! `private` word to write; `immut` vetoes the one write a field gets by
//! default, its constructor's fill. `static` is read by the type body's own reader,
//! and anywhere else by the declaration's own constructor. `mut alloc …` is the
//! allocation that may be written through.
//! DESIGN ›Read and write are one mechanism across the system‹

use super::{meta, Cx};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError};

/// Returns `pub`, `mut`, `immut`, `static`.
pub(super) fn register(cx: &mut Cx) -> (DyadPtr, DyadPtr, DyadPtr, DyadPtr) {
    let pub_ = word(cx, "pub");
    cx.metas.insert(pub_, construct);
    let mut_ = word(cx, "mut");
    cx.metas.insert(mut_, construct);
    let immut_ = word(cx, "immut");
    cx.metas.insert(immut_, construct);
    let static_ = word(cx, "static");
    cx.metas.insert(static_, outside_block);
    (pub_, mut_, immut_, static_)
}

/// No node is ever typed by a gate word; its identity exists to stand on a
/// binding.
pub(super) fn word(cx: &mut Cx, name: &str) -> DyadPtr {
    let record = meta::token_record(cx, meta::prec::PREFIX, crate::parse::Assoc::Right, &[]);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare(name, id);
    id
}

/// Whether `binding`'s gates grant a read from the scope `from`: its default entry grants one
/// from its own scope and below, along the parent links, and `pub` one from any scope (›A name's
/// gates are one ordered set on its binding, combined as grants and vetoes‹ [[R81]]). A `.` from
/// outside a file, a type or a `{ … }` reaches only its `pub` names so (›Fields are read with a
/// dot outside their scope‹ [[R234]]).
///
/// # Safety
/// `binding` must be a binding dyad from the store; `from` null or a scope node.
pub(crate) unsafe fn grants_read(types: &crate::Core, binding: DyadPtr, from: DyadPtr) -> bool {
    use crate::binding::Binding;
    if Binding::has_gate(binding, types.pub_) {
        return true;
    }
    let own = Binding::read(binding).scope;
    let mut at = from;
    while !at.is_null() {
        if at == own {
            return true;
        }
        at = super::scope::parent_of(at);
    }
    false
}

/// A type body's reader takes the word at a line's start; anywhere else on
/// a body line it stands for nothing.
fn outside_block(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<Constructed, ParseError> {
    if p.in_type_body_lines() {
        return Err(ParseError::StaticMisplaced);
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
    // `mut alloc …` gives a pointer that may be written through; it carries no mark, since
    // no write through `@` is checked yet: stand-in for #33.
    // SAFETY: `inner` is a reduced dyad just parsed.
    if id == types.mut_ && unsafe { super::mint::origin(dyad::ty(inner)) } == types.alloc_ {
        tape.place(inner);
        return Ok(Constructed::Placed);
    }
    // SAFETY: `inner` is a reduced dyad just parsed.
    if unsafe { dyad::ty(inner) } != types.declare_ {
        return Err(ParseError::GateNeedsDeclaration);
    }
    p.gate_declared(id)?;
    tape.place(inner);
    Ok(Constructed::Placed)
}
