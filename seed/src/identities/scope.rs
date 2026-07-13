// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `scope`: the type of a scope node, the graph's own spine (DESIGN
//! ›Meta-navigation walks the graph; the scope stack is the graph's own spine‹).
//! A scope is a node whose type is `scope`; entering one is descending into its
//! subtree, leaving it is ascending, and name resolution walks up the chain of
//! open scopes.
//!
//! A scope node is both the parser's membership marker — the
//! [`ScopeStack`](crate::parse::ScopeStack) keys on its address, so the root
//! scope, each struct/parameter-list scope, and each block are typed `scope` —
//! and, for a multi-expression block, the *sequence node* itself: its value is
//! the null-terminated expression list `[expr0 … exprN, null]`, run in order and
//! yielding the trailing expression (DESIGN ›A scope's value is what it evaluates
//! to‹). A struct/parameter scope keeps an undefined value. The grant-bearing
//! `gate` role (visibility and access; DESIGN ›Read and write are one mechanism‹)
//! is deferred. `scope` is created internally by the parser, never written in
//! source, so it needs no spelling.

use cranelift_codegen::ir::Value;

use super::Cx;
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Create the `scope` type (its own type is `type`) and return it. Called before
/// the build context exists, since the root scope is itself typed `scope`.
pub(super) fn register(store: &mut Store, type_: DyadPtr) -> DyadPtr {
    store.alloc_raw(type_, std::ptr::null_mut())
}

/// Register `scope`'s executable behaviour: a sequence node runs its expressions
/// in order and yields the trailing one. Done after the build context exists.
pub(super) fn register_exec(cx: &mut Cx, scope_: DyadPtr) {
    cx.bcode.insert(scope_, run);
    cx.lower.insert(scope_, lower);
}

/// Run: each expression in order, for effect; the trailing one's value is the
/// sequence's. A scope with no expression list is not runnable data.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a sequence node whose value is a null-terminated list of
    // valid dyads (as `Parser::parse_sequence` builds; it never builds it empty).
    unsafe {
        let p = (*node).value as *const DyadPtr;
        if p.is_null() {
            return Err(RunError::BadValue);
        }
        let mut last = 0i64;
        let mut i = 0;
        while !(*p.add(i)).is_null() {
            last = rt.run(*p.add(i))?;
            i += 1;
        }
        Ok(last)
    }
}

/// Lower: each expression in order (non-tail values fall dead; effects remain),
/// yielding the trailing expression's value.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run`].
    unsafe {
        let p = (*node).value as *const DyadPtr;
        if p.is_null() {
            return Err(CompileError::BadValue);
        }
        let mut last = None;
        let mut i = 0;
        while !(*p.add(i)).is_null() {
            last = Some(lw.lower(*p.add(i))?);
            i += 1;
        }
        last.ok_or(CompileError::BadValue)
    }
}
