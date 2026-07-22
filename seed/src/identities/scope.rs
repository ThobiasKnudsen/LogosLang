// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `scope`: the logos of a scope node, the graph's own spine (DESIGN
//! ›Meta-navigation walks the graph; the scope stack is the graph's own spine‹).
//! A scope is a node whose logos is `scope`; entering one is descending into its
//! subtree, leaving it is ascending, and name resolution walks up the chain of
//! open scopes.
//!
//! A scope node is both the parser's membership marker — the
//! [`ScopeStack`](crate::parse::ScopeStack) keys on its address, so the root
//! scope, each record/parameter-list scope, and each block are typed `scope` —
//! and, for a multi-expression block, the *sequence node* itself. A scope *is*
//! an array (settled, July 2026): its value is `[exprs, op]` — an
//! [`array`](super::array) node holding the expression list behind one
//! indirection (never inline in the node), and the sequence native's leaf in
//! the op slot — run in order and yielding the trailing expression (DESIGN ›A
//! scope's value is what it evaluates to‹). A record/parameter scope keeps an
//! undefined value. The grant-bearing `gate` role (visibility and access;
//! DESIGN ›Read and write are one mechanism‹) is deferred. `scope` is created
//! internally by the parser, never written in source, so it needs no spelling.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{array, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::synolon::SynolonPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Create the `scope` logos (its own logos is `logos`) and return it. Called before
/// the build context exists, since the root scope is itself typed `scope`.
pub(super) fn register(store: &mut Store, type_: SynolonPtr) -> SynolonPtr {
    store.alloc_raw(type_, std::ptr::null_mut())
}

/// Register `scope`'s executable behaviour: the sequence native as a callable
/// leaf (a sequence node references it from its op slot) and the lowering.
/// Done after the build context exists. Returns the leaf.
pub(super) fn register_exec(cx: &mut Cx, scope_: SynolonPtr, cs: &Callables) -> SynolonPtr {
    cx.lower.insert(scope_, lower);
    callable::mint_native(cx.store, cs.callable, run, cs.seed_native)
}

/// The expression array of a sequence node (`value` is `[exprs, op]`).
///
/// # Safety
/// `node` must be a sequence node as `Parser::parse_sequence` builds it, with a
/// non-null value; the store must outlive the returned slice.
unsafe fn exprs<'a>(node: SynolonPtr) -> &'a [SynolonPtr] {
    let arr = *((*node).hyle as *const SynolonPtr);
    array::items(arr)
}

/// Run: each expression in order, for effect; the trailing one's value is the
/// sequence's. A scope with no expression array is not runnable data.
fn run(rt: &mut Runtime, node: SynolonPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a sequence node whose first slot is its expression
    // array (as `Parser::parse_sequence` builds; it never builds it empty).
    unsafe {
        if (*node).hyle.is_null() {
            return Err(RunError::BadValue);
        }
        let mut last = 0i64;
        for &expr in exprs(node) {
            // A comment node is prose — reflectable structure invisible to value
            // flow: never run, never the tail.
            if !super::numtype::is_comment_type((*expr).logos) {
                last = rt.run(expr)?;
            }
        }
        Ok(last)
    }
}

/// Lower: each expression in order (non-tail values fall dead; effects remain),
/// yielding the trailing expression's value.
fn lower(lw: &mut Lowerer, node: SynolonPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run`].
    unsafe {
        if (*node).hyle.is_null() {
            return Err(CompileError::BadValue);
        }
        let mut last = None;
        for &expr in exprs(node) {
            // Prose is not lowered; see [`run`].
            if !super::numtype::is_comment_type((*expr).logos) {
                last = Some(lw.lower(expr)?);
            }
        }
        last.ok_or(CompileError::BadValue)
    }
}
