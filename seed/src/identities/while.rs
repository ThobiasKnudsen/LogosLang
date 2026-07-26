// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `while ( cond ) ( body )`: the loop statement. `while` is a function (its own
//! logos is `fn`), like `if`; its node is `{logos: while, value: [cond, body]}`. The
//! `bool` condition is re-evaluated before each iteration and the body reruns for
//! its effect, its value discarded each time (DESIGN ›a loop body's is thrown
//! away‹). The loop yields unit (0 bits): it is a statement, so value positions
//! reject it at parse time ([`crate::parse::ParseError::StatementAsValue`]).
//!
//! The surface parse lives in [`crate::parse::Parser::parse_while`]; here we
//! register the identity, its run native, and its lowering.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::synolon::SynolonPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc};
use crate::run::{RunError, Runtime};

/// Register `while`: spelling, its `While` schedule, native leaf, and
/// lowering. Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (SynolonPtr, SynolonPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        f64::NAN,
        Assoc::Left,
        &["condition", "body", "op"],
    );
    let while_ = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("while", IdContext::new(while_, cx.root_scope));
    cx.metas.insert(while_, |p, id, tape| {
        let node = p.parse_while(id)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    cx.lower.insert(while_, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (while_, leaf)
}

/// The `(cond, body)` operands of a `while` node.
///
/// # Safety
/// `node` must be a `while` node built by [`crate::parse::Parser::parse_while`],
/// with a `[cond, body]` value.
unsafe fn parts(node: SynolonPtr) -> (SynolonPtr, SynolonPtr) {
    let p = (*node).hyle as *const SynolonPtr;
    (*p, *p.add(1))
}

/// Run: re-evaluate the condition before each iteration (non-zero is true,
/// matching the compiled `brif`), running the body for its effect; yield unit.
fn run(rt: &mut Runtime, node: SynolonPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `while` node with `[cond, body]` operands.
    unsafe {
        let (cond, body) = parts(node);
        while rt.run(cond)? != 0 {
            rt.run(body)?;
        }
        Ok(0)
    }
}

/// Lower: a loop of header (condition), body, and exit blocks; see
/// [`Lowerer::lower_while`].
fn lower(lw: &mut Lowerer, node: SynolonPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `while` node with `[cond, body]` operands.
    unsafe {
        let (cond, body) = parts(node);
        lw.lower_while(cond, body)
    }
}
