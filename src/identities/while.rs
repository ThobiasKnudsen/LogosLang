// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `while cond body`: the loop statement, the node `[cond, body]`,
//! yielding unit. The surface parse lives in
//! [`crate::parse::Parser::parse_while`]; here the identity, run, and lowering.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
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
    let while_ = cx.store.alloc_raw(cx.type_, record);
    cx.declare("while", while_);
    cx.metas.insert(while_, |p, id, tape| {
        let node = p.parse_while(id)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    cx.lower.insert(while_, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (while_, leaf)
}

/// # Safety
/// `node` must be a `while` node `[cond, body]`.
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
    (*p, *p.add(1))
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `while` node with `[cond, body]` operands.
    unsafe {
        let (cond, body) = parts(node);
        while rt.run(cond)? != 0 {
            rt.run(body)?;
        }
        Ok(0)
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `while` node with `[cond, body]` operands.
    unsafe {
        let (cond, body) = parts(node);
        lw.lower_while(cond, body)
    }
}
