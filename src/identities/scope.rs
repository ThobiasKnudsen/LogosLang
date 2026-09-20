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
//! scope, each record/parameter-list scope, and each block are typed `scope` —
//! and, for a multi-expression block, the *sequence node* itself. A scope *is*
//! an array (settled, July 2026): its value is `[exprs, op, parent]` — an
//! [`array`](super::array) node holding the expression list behind one
//! indirection (never inline in the node), the sequence native's leaf in the
//! op slot — run in order and yielding the trailing expression (DESIGN ›A
//! scope's value is what it evaluates to‹) — and, since 15 September 2026
//! (#123), **the enclosing scope**: "A scope node carries its enclosing
//! scope: the parent link, null at the arche, and only it … the open-scope
//! membership set is a cache over the link" (DESIGN ›Meta-navigation‹). The
//! link is set when the scope is minted ([`mint`]), before anything inside
//! it is parsed, so a constructor running inside walks up over settled
//! structure; the expression list and the leaf are filled when the block
//! closes ([`fill`]). A record/parameter scope keeps its first two slots
//! null. The root scope and a type's member scope are minted with no value
//! at all and enclose nothing. The grant-bearing `gate` role (visibility and
//! access; DESIGN ›Read and write are one mechanism‹) is deferred. `scope` is
//! created internally by the parser, never written in source, so it needs no
//! spelling.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{array, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Create the `scope` logos (its own type is `logos`) and return it. Called before
/// the build context exists, since the root scope is itself typed `scope`.
pub(super) fn register(store: &mut Store, type_: DyadPtr) -> DyadPtr {
    store.alloc_raw(type_, std::ptr::null_mut())
}

/// Register `scope`'s executable behaviour: the sequence native as a callable
/// leaf (a sequence node references it from its op slot) and the lowering.
/// Done after the build context exists. Returns the leaf.
pub(super) fn register_exec(cx: &mut Cx, scope_: DyadPtr, cs: &Callables) -> DyadPtr {
    cx.lower.insert(scope_, lower);
    callable::mint_native(cx.store, cs.callable, run, cs.seed_native)
}

/// The slots of a scope node's value, `[exprs, op, parent]`.
const EXPRS: usize = 0;
const OP: usize = 1;
const PARENT: usize = 2;

/// Mint a scope node carrying its enclosing scope (`parent`, null at the
/// arche) and nothing else yet: the block's membership key while it parses,
/// and the sequence node itself once [`fill`] gives it its expressions.
pub(crate) fn mint(store: &mut Store, scope_ty: DyadPtr, parent: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[std::ptr::null_mut(), std::ptr::null_mut(), parent]);
    store.alloc_raw(scope_ty, value)
}

/// Make a minted scope the sequence node of `exprs`, an [`array`](super::array)
/// node, run by the leaf `op`.
///
/// # Safety
/// `node` must be a scope [`mint`] built; nothing else may hold its slots.
pub(crate) unsafe fn fill(node: DyadPtr, exprs: DyadPtr, op: DyadPtr) {
    let slots = (*node).value as *mut DyadPtr;
    *slots.add(EXPRS) = exprs;
    *slots.add(OP) = op;
}

/// The expression array node of a scope, or null for one that is no
/// sequence — a record or parameter scope, or one minted with no value.
///
/// # Safety
/// `node` must be a scope node from the store.
pub(crate) unsafe fn exprs_array(node: DyadPtr) -> DyadPtr {
    if (*node).value.is_null() {
        return std::ptr::null_mut();
    }
    *((*node).value as *const DyadPtr).add(EXPRS)
}

/// The expressions of a sequence node, or `None` for a scope that is no
/// sequence.
///
/// # Safety
/// `node` must be a scope node from the store; the store must outlive the
/// returned slice.
pub(crate) unsafe fn exprs_of<'a>(node: DyadPtr) -> Option<&'a [DyadPtr]> {
    let arr = exprs_array(node);
    if arr.is_null() {
        None
    } else {
        Some(array::items(arr))
    }
}

/// The enclosing scope of a scope node, the parent link (#123): null at the
/// arche, and on a scope minted with no value (the root, a type's member
/// scope).
///
/// # Safety
/// `node` must be a scope node from the store.
pub(crate) unsafe fn parent_of(node: DyadPtr) -> DyadPtr {
    if (*node).value.is_null() {
        return std::ptr::null_mut();
    }
    *((*node).value as *const DyadPtr).add(PARENT)
}

/// Run: each expression in order, for effect; the trailing one's value is the
/// sequence's. A `defer` (issue #49) is not run in this pass — it is held and its
/// inner run LIFO at scope exit, the scope's own teardown machinery (DESIGN
/// ›Explicit heap‹). A scope with no expression array is not runnable data.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a sequence node whose first slot is its expression
    // array (as `Parser::parse_sequence` builds; it never builds it empty).
    unsafe {
        let Some(exprs) = exprs_of(node) else {
            return Err(RunError::BadValue);
        };
        let defer_ty = rt.types().defer_;
        let mut last = 0i64;
        let mut defers: Vec<DyadPtr> = Vec::new();
        for &expr in exprs {
            let logos = (*expr).ty;
            // A comment node is prose — reflectable structure invisible to value
            // flow: never run, never the tail.
            if super::numtype::is_comment_type(logos) {
                continue;
            }
            // A `defer`: held, not run — its teardown fires at scope exit, LIFO.
            if logos == defer_ty {
                defers.push(expr);
                continue;
            }
            last = rt.run(expr)?;
        }
        // Scope exit: run the held teardowns in reverse (LIFO), so teardown order
        // reverses construction order (a `defer free` over an emptied place is
        // the sanctioned no-op inside its own run). No unwinding in v1: a body
        // error above skips the teardowns, a known limitation of the seed.
        for &d in defers.iter().rev() {
            rt.run(super::drop_model::deferred_inner_of(d))?;
        }
        Ok(last)
    }
}

/// Lower: each expression in order (non-tail values fall dead; effects remain),
/// yielding the trailing expression's value.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run`].
    unsafe {
        let Some(exprs) = exprs_of(node) else {
            return Err(CompileError::BadValue);
        };
        let mut last = None;
        for &expr in exprs {
            // Prose is not lowered; see [`run`].
            if !super::numtype::is_comment_type((*expr).ty) {
                last = Some(lw.lower(expr)?);
            }
        }
        last.ok_or(CompileError::BadValue)
    }
}
