// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `while cond body`: the loop, the node `[cond, body]`; a value its body gives ends the loop
//! and passes out. The surface parse lives in [`crate::parse::Parser::parse_while`]; here the
//! identity, run, and lowering.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};

/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::READER,
        Assoc::Left,
        &["condition", "body", "op"],
    );
    let while_ = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `while_` carries the record built above, which names the body.
    unsafe { meta::install_hands_on(while_, &["body"], &[]) };
    cx.declare("while", while_);
    cx.metas.insert(while_, |p, id, tape| {
        let node = p.parse_while(id, p.starts_line(tape))?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    cx.lower.insert(while_, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (while_, leaf)
}

pub(crate) const BODY: usize = 1;

/// # Safety
/// `node` must be a `while` node `[cond, body]`.
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p, *p.add(BODY))
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `while` node with `[cond, body]` operands.
    unsafe { run_given(rt, node).map(|given| given.unwrap_or(0)) }
}

/// Each pass runs the body as a line: a value it gives ends the loop and passes out (›A scope's
/// value is what it evaluates to, and `return` is an optional early exit from the enclosing
/// function‹ [[R207]]).
///
/// # Safety
/// `node` must be a `while` node `[cond, body]`.
pub(super) unsafe fn run_given(rt: &mut Runtime, node: DyadPtr) -> Result<Option<i64>, RunError> {
    let (cond, body) = parts(node);
    let mark = rt.stack_mark();
    while rt.run(cond)? != 0 {
        if let Some(value) = rt.run_line(body)? {
            return Ok(Some(value));
        }
        rt.stack_release(mark);
    }
    Ok(None)
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `while` node with `[cond, body]` operands.
    lw.lower_as_value(|lw| unsafe { lower_given(lw, node) })
}

/// [`run_given`]'s lowering: a value the body gives ends the loop and passes out.
///
/// # Safety
/// `node` must be a valid `while` node with `[cond, body]` operands.
pub(super) unsafe fn lower_given(lw: &mut Lowerer, node: DyadPtr) -> Result<(), CompileError> {
    let (cond, body) = parts(node);
    lw.lower_while(cond, body)
}
