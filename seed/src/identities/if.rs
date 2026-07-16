// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `if ( cond ) ( then )` with an optional `else ( else )`: the conditional. `if`
//! is a function (its own type is `fn`), like the operators; its node is
//! `{ty: if, value: [cond, then, else]}`, the else slot null when absent. The
//! condition must be a `bool` (checked at parse time); run evaluates only the taken
//! branch, compile emits a two-way branch merging to a single value (DESIGN ›A
//! scope's value is what it evaluates to‹). With both branches an `if` is a value;
//! else-less it is a statement yielding unit (0 bits), and value positions reject
//! it at parse time ([`crate::parse::ParseError::MissingElse`]).
//!
//! The surface parse lives in [`crate::parse::Parser::parse_if`] (it drives three
//! bracketed sub-parses and the `else` keyword); here we register the identity, its
//! run native and lowering, and the `else` token the parser consumes.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, Schedule};
use crate::run::{RunError, Runtime};

/// The index of the condition in an `if` node's value struct.
const IF_COND: usize = 0;
/// The index of the then-branch.
const IF_THEN: usize = 1;
/// The index of the else-branch.
const IF_ELSE: usize = 2;

/// Register `if` (the conditional keyword, its native leaf, and its lowering)
/// and the `else` token the parser consumes between the branches. Returns
/// `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        0.0,
        Assoc::Left,
        Schedule::If,
        &["condition", "then", "else", "op"],
    );
    let if_ = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("if", IdContext::new(if_, cx.root_scope));
    cx.metas.insert(if_, Construct::Keyword(|p, id, _left| p.parse_if(id)));
    cx.lower.insert(if_, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);

    // `else` is a parse-only token between the branches, not a function.
    let record = meta::record(cx.store, meta::TOKEN_TAG, Schedule::Else);
    let else_ = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("else", IdContext::new(else_, cx.root_scope));

    (if_, leaf)
}

/// The `(cond, then, else)` operands of an `if` node (the else null when absent).
///
/// # Safety
/// `node` must be an `if` node built by [`crate::parse::Parser::parse_if`], with a
/// `[cond, then, else]` value.
unsafe fn branches(node: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
    (*p.add(IF_COND), *p.add(IF_THEN), *p.add(IF_ELSE))
}

/// Run: evaluate the condition, then run only the taken branch (a non-zero condition
/// is true, matching the compiled `brif`). An else-less `if` runs its then-branch
/// for its effect when taken and yields unit (0 bits) either way, matching the
/// compiled merge.
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

/// Lower: a two-way branch on the condition, each arm lowering its branch and
/// jumping to a merge block whose parameter is the `if`'s value; an else-less `if`
/// lowers as a unit-valued statement ([`Lowerer::lower_if_stmt`]).
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
