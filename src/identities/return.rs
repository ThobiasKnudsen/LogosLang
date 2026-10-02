// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `return`: the node `[value, ends, op]`. Inside a call it leaves the function with the
//! operand's value, from wherever it stands, freeing on its way the names its own line ends
//! after it. Its own line gets nothing from it, inside a function or outside any.
//! DESIGN ›A scope's value is what it evaluates to, and `return` is an optional
//! early exit from the enclosing function‹, ›A value's teardown runs where its life ends;
//! the ending identity reads the type's `free` slot‹

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{array, meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// Null, or an `array` of held places in declaration order.
const ENDS: usize = 1;

/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::RETURN,
        Assoc::Right,
        &["value", "ends", "op"],
    );
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare("return", id);
    cx.metas.insert(id, construct);
    cx.lower.insert(id, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

fn construct(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, ParseError> {
    let operand = p.take_right(tape)?;
    let leaf = p.types().ops.return_;
    let node = p.store().alloc_words(id, &[operand, std::ptr::null_mut(), leaf]);
    // SAFETY: `node` is the `return` node just built.
    unsafe { p.note_return(node) }?;
    tape.place(node);
    Ok(crate::parse::Constructed::Placed)
}

/// # Safety
/// `node` must be a `return` node `[value, ends, op]`.
unsafe fn operand(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr)
}

/// # Safety
/// As [`operand`]; the list's store outlives the slice.
unsafe fn ends_of<'a>(node: DyadPtr) -> &'a [DyadPtr] {
    let arr = *(dyad::value(node) as *const DyadPtr).add(ENDS);
    if arr.is_null() {
        &[]
    } else {
        array::items(arr)
    }
}

/// # Safety
/// `node` must be a `return` node `construct` built, not yet run; `ends` held places.
pub(crate) unsafe fn set_ends(store: &mut Store, types: &Core, node: DyadPtr, ends: &[DyadPtr]) {
    if !ends.is_empty() {
        *(dyad::value(node) as *mut DyadPtr).add(ENDS) = array::build(store, types.array_, ends);
    }
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid return node; its ends are held places.
    unsafe {
        let value = rt.run(operand(node))?;
        if !rt.in_call() {
            return Ok(value);
        }
        for &place in ends_of(node).iter().rev() {
            super::drop_model::end_held(rt, place)?;
        }
        Err(RunError::Return(value))
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid return node; its ends are held places.
    unsafe { lw.lower_return(operand(node), ends_of(node)) }
}
