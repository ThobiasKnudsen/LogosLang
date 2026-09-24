// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `if cond then` with an optional `else else`, each branch a bracket or the
//! next expression: the node is `[cond, then, else]`, the else slot null when absent. The surface parse
//! lives in [`crate::parse::Parser::parse_if`]; here the identity, its run
//! native and lowering, and the `else` token.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};

const IF_COND: usize = 0;
const IF_THEN: usize = 1;
const IF_ELSE: usize = 2;

/// Returns `(identity, leaf, else token)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::READER,
        Assoc::Left,
        &["condition", "then", "else", "op"],
    );
    let if_ = cx.store.alloc_raw(cx.type_, record);
    cx.declare("if", if_);
    cx.metas.insert(if_, |p, id, tape| {
        let node = p.parse_if(id)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    cx.lower.insert(if_, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);

    // `else` is a parse-only token between the branches, not a function.
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
    let else_ = cx.store.alloc_raw(cx.type_, record);
    cx.declare("else", else_);

    (if_, leaf, else_)
}

/// # Safety
/// `node` must be an `if` node `[cond, then, else]`.
unsafe fn branches(node: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
    (*p.add(IF_COND), *p.add(IF_THEN), *p.add(IF_ELSE))
}

/// An else-less `if` yields unit either way, matching the compiled merge.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `if` node with `[cond, then, else]` operands.
    unsafe {
        let (cond, then, els) = branches(node);
        if els.is_null() {
            if rt.run(cond)? != 0 {
                rt.run(then)?;
            }
            return Ok(0);
        }
        if rt.run(cond)? != 0 {
            rt.run(then)
        } else {
            rt.run(els)
        }
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `if` node with `[cond, then, else]` operands.
    unsafe {
        let (cond, then, els) = branches(node);
        if els.is_null() {
            return lw.lower_if_stmt(cond, then);
        }
        lw.lower_if(cond, then, els)
    }
}
