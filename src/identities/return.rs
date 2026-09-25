// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `return`: the node `[value, op]`. Inside a call it leaves the function with
//! the operand's value, from wherever it stands; outside any function it is a
//! scope's tail and yields the value.
//! DESIGN ›A scope's value is what it evaluates to, and `return` is an optional
//! early exit from the enclosing function‹

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError};
use crate::run::{RunError, Runtime};

/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::RETURN,
        Assoc::Right,
        &["value", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
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
    let types = p.types();
    let value = p.store().alloc_operands(&[operand, types.ops.return_]);
    let node = p.store().alloc_raw(id, value);
    // SAFETY: `node` is the `return` node just built.
    unsafe { p.note_return(node) }?;
    tape.place(node);
    Ok(crate::parse::Constructed::Placed)
}

/// # Safety
/// `node` must be a `return` node `[value, op]`.
unsafe fn operand(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr)
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid return node; its first slot is its operand.
    let value = unsafe { rt.run(operand(node)) }?;
    if rt.in_call() {
        Err(RunError::Return(value))
    } else {
        Ok(value)
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid return node; its first slot is its operand.
    unsafe { lw.lower_return(operand(node)) }
}
