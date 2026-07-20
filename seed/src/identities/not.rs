// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `not (x)`: logical negation of a `bool`. Its operand must be a `bool`; the result
//! is a `bool`. It takes a parenthesized operand (like `if`'s condition), which keeps
//! its binding unambiguous without a unary-precedence rule: `not (a) and b` is
//! `(not a) and b`. The node is `{ty: not, value: [operand, op]}` — the punned
//! single-operand form widened so the node references its native leaf like every
//! other runnable (issue #44).
//!
//! The surface parse lives in [`crate::parse::Parser::parse_not`]; here we register
//! the identity, its native leaf, and its lowering. Run yields `1` when the operand
//! is false, else `0`; compile lowers it as `operand == 0`.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Schedule};
use crate::run::{RunError, Runtime};

/// Register `not`: spelling, the parenthesized-operand construct, native leaf,
/// and lowering. Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        0.0,
        Assoc::Left,
        Schedule::Not,
        &["operand", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("not", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, |p, id, _tape| p.parse_not(id).map(crate::parse::Constructed::Node));
    cx.lower.insert(id, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

/// The single operand of a `not` node (its first slot).
///
/// # Safety
/// `node` must be a `not` node `[operand, op]` built by
/// [`crate::parse::Parser::parse_not`].
unsafe fn operand(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr)
}

/// Run: `1` when the operand is false (0), else `0`.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `not` node; its first slot is its operand.
    unsafe { Ok(i64::from((rt.run(operand(node))? == 0) as i32)) }
}

/// Lower: `operand == 0`, yielding the i32 0/1.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `not` node; its first slot is its operand.
    unsafe {
        let a = lw.lower(operand(node))?;
        let zero = lw.const_i32(0);
        Ok(lw.icmp_eq(a, zero))
    }
}
