// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `lossy`: takes the value to its right and makes a `lossy` value of it, the node `[value]`,
//! which the number type to its left takes and converts, losing what it cannot hold. It
//! exists only there, so no `lossy` value reaches the graph.
//! ›A type's `parse` decides what converts into it; unwritten, a value converts only into a type
//! that holds it exactly‹ [[R155]]

use super::{meta, Cx};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, Cell, Constructed, ParseError, Parser, ParsingTape};
use crate::Core;

/// At the number types' rank, associating right as they do, so in `i32 lossy f64 2.5` the
/// `f64` is built first, then `lossy`, then `i32` (›A parse is ranked by what it takes from its
/// right‹ [[R116]]).
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::token_record(cx, meta::prec::CONVERT, Assoc::Right, &["lhs", "rhs"]);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare("lossy", id);
    cx.metas.insert(id, construct);
    id
}

fn construct(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    let at = tape.at(0).map_or(0, |c| c.start);
    let types = p.types();
    let typed = tape.at(-1).copied().is_some_and(|c| {
        // SAFETY: a cell's identity is null or a resolved dyad from the store.
        !c.constructed && unsafe { super::is_numtype_node(types, p.cell_identity(&c)) }
    });
    let operand = if typed { p.type_operand(tape, 1)? } else { None };
    let Some(operand) = operand else {
        return Err(p.fail_at(at, ParseError::LossyOutOfPlace));
    };
    let value = p.as_operand(operand)?;
    tape.remove(1);
    let node = p.store().alloc_words(id, &[value]);
    tape.place(node);
    Ok(Constructed::Placed)
}

/// What the `lossy` value standing in `cell` took from its right, a bracket as it stood there;
/// `None` for any other cell.
pub(crate) fn taken(types: &Core, cell: &Cell) -> Option<DyadPtr> {
    // SAFETY: a constructed cell holds a node from the store, and a `lossy` node's one word is
    // the value its constructor took.
    unsafe {
        (cell.constructed && !cell.dyad.is_null() && dyad::ty(cell.dyad) == types.lossy)
            .then(|| *(dyad::value(cell.dyad) as *const DyadPtr))
    }
}
