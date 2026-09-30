// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `scope`: the type of a scope node, the parser's membership marker and, for
//! a block, the sequence node itself: `[exprs, op, parent, exit]`, the parent link
//! set at [`mint`], the expressions (the `dyads` field) pushed as each line
//! completes, the op set at [`fill`] when the block closes, and the exit, what the
//! scope's end runs, set when it closes.
//! DESIGN ›Meta-navigation walks the graph; the scope stack is the graph's own spine‹

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{array, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Constructed;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Called before the build context exists, since the root scope is itself typed `scope`.
pub(super) fn register(store: &mut Store, type_: DyadPtr) -> DyadPtr {
    store.alloc_leaf(type_)
}

/// Returns the sequence leaf a sequence node references from its op slot.
/// DESIGN ›The scope's constructor is the driver‹: `scope (…)` is `(…)`; with
/// no bracket to its right, `scope` stands as the type.
pub(super) fn register_exec(cx: &mut Cx, scope_: DyadPtr, cs: &Callables) -> DyadPtr {
    cx.declare("scope", scope_);
    cx.metas.insert(scope_, |p, id, tape| {
        if p.reads_own_bracket(tape) {
            p.expect_open()?;
            let body = p.parse_sequence()?;
            p.expect_close()?;
            tape.place_bracket(body);
        } else {
            let value = p.stand_as_value(tape, id);
            tape.place(value);
        }
        Ok(Constructed::Placed)
    });
    cx.lower.insert(scope_, lower);
    callable::mint_native(cx.store, cs.callable, run, cs.seed_native)
}

/// The slots of a scope node's value, `[exprs, op, parent, exit]`.
const EXPRS: usize = 0;
const OP: usize = 1;
const PARENT: usize = 2;
/// Null, or an `array` of exit items (`drop_model::exit_item`) in line order.
const EXIT: usize = 3;

/// The block's membership key while it parses, and the sequence node once
/// [`fill`] gives it its expressions; `parent` is null at the arche. A scope that
/// holds names alone (the root, a type's member scope) is minted with no parent.
pub(crate) fn mint(store: &mut Store, scope_ty: DyadPtr, parent: DyadPtr) -> DyadPtr {
    let none = std::ptr::null_mut();
    store.alloc_words(scope_ty, &[none, none, parent, none])
}

/// The block closed: it runs as a sequence over its `dyads`.
///
/// # Safety
/// `node` must be a scope [`mint`] built; nothing else may hold its slots.
pub(crate) unsafe fn fill(node: DyadPtr, op: DyadPtr) {
    *(dyad::value(node) as *mut DyadPtr).add(OP) = op;
}

/// The scope's `dyads`, made empty on first read.
///
/// # Safety
/// `node` must be a scope or `square_brackets` node from the store.
pub(crate) unsafe fn dyads(store: &mut Store, array_ty: DyadPtr, node: DyadPtr) -> DyadPtr {
    let slot = (dyad::value(node) as *mut DyadPtr).add(EXPRS);
    if (*slot).is_null() {
        *slot = array::build(store, array_ty, &[]);
    }
    *slot
}

/// A line of the scope is complete: it joins `dyads` while the scope is still
/// open, so a read inside it sees the lines above.
///
/// # Safety
/// `node` must be a scope node from the store; `item` a reduced dyad.
pub(crate) unsafe fn push_item(store: &mut Store, array_ty: DyadPtr, node: DyadPtr, item: DyadPtr) {
    let arr = dyads(store, array_ty, node);
    if !arr.is_null() {
        array::push(store, arr, item);
    }
}

/// Null for a scope that is no sequence yet: a record or parameter scope, one whose
/// lines nobody has read.
///
/// # Safety
/// `node` must be a scope or `square_brackets` node from the store.
pub(crate) unsafe fn exprs_array(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr).add(EXPRS)
}

/// # Safety
/// `node` must be a scope or `square_brackets` node from the store; the store must
/// outlive the returned slice.
pub(crate) unsafe fn exprs_of<'a>(node: DyadPtr) -> Option<&'a [DyadPtr]> {
    let arr = exprs_array(node);
    if arr.is_null() {
        None
    } else {
        Some(array::items(arr))
    }
}

/// A closed sequence with `node`'s op and parent running `exprs` instead.
///
/// # Safety
/// `node` must be a closed sequence node from the store; `exprs` reduced dyads.
pub(crate) unsafe fn with_exprs(
    store: &mut Store,
    array_ty: DyadPtr,
    node: DyadPtr,
    exprs: &[DyadPtr],
) -> DyadPtr {
    let slots = dyad::value(node) as *const DyadPtr;
    let lines = array::build(store, array_ty, exprs);
    store
        .alloc_words(dyad::ty(node), &[lines, *slots.add(OP), *slots.add(PARENT), *slots.add(EXIT)])
}

/// Null for a scope whose end runs nothing.
///
/// # Safety
/// `node` must be a scope node from the store.
pub(crate) unsafe fn exit_of(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr).add(EXIT)
}

/// Appends `items` to the scope's exit, the array made on first use.
///
/// # Safety
/// `node` must be a scope node from the store; `items` exit items.
pub(crate) unsafe fn extend_exit(
    store: &mut Store,
    array_ty: DyadPtr,
    node: DyadPtr,
    items: &[DyadPtr],
) {
    if items.is_empty() {
        return;
    }
    let slot = (dyad::value(node) as *mut DyadPtr).add(EXIT);
    if (*slot).is_null() {
        *slot = array::build(store, array_ty, items);
        return;
    }
    for &item in items {
        array::push(store, *slot, item);
    }
}

/// The scope's end, reached after `reached` of its lines completed: every exit item held
/// there runs, last first, an authored `defer` its expression and a held name its
/// value's teardown (DESIGN ›A value's teardown runs where its life ends; the ending
/// identity reads the type's `free` slot‹).
///
/// # Safety
/// `node` must be a scope node from the store, its places laid out in the frame `rt` runs.
pub(crate) unsafe fn run_exit(
    rt: &mut Runtime,
    node: DyadPtr,
    reached: usize,
) -> Result<(), RunError> {
    let exit = exit_of(node);
    if exit.is_null() {
        return Ok(());
    }
    for &item in array::items(exit).iter().rev() {
        let item = super::drop_model::exit_item_of(item);
        if item.held_at(reached) {
            item.run(rt)?;
        }
    }
    Ok(())
}

/// Null at the arche.
///
/// # Safety
/// `node` must be a scope node from the store.
pub(crate) unsafe fn parent_of(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr).add(PARENT)
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a sequence node; its first slot is its expression array.
    unsafe {
        let Some(exprs) = exprs_of(node) else {
            return Err(RunError::EmptyScope);
        };
        let defer_ty = rt.types().defer_;
        let start = rt.take_cursor(node);
        let mut last = 0i64;
        let tail = exprs.iter().rposition(|&e| {
            !super::numtype::is_comment_type(dyad::ty(e)) && dyad::ty(e) != defer_ty
        });
        for (i, &expr) in exprs.iter().enumerate() {
            let logos = dyad::ty(expr);
            // Prose never runs; a `defer` runs at the end, from the exit.
            if super::numtype::is_comment_type(logos) || logos == defer_ty {
                continue;
            }
            // Before the cursor: the pass ran it in this frame already.
            if i < start {
                continue;
            }
            // A line's scratch is freed with the line; the tail's is the value handed on.
            let mark = rt.stack_mark();
            match rt.run(expr) {
                Ok(v) => {
                    last = v;
                    if tail != Some(i) {
                        rt.stack_release(mark);
                    }
                }
                // A `return` leaves through this scope: what it holds at line `i` ends.
                Err(RunError::Return(v)) => {
                    run_exit(rt, node, i)?;
                    return Err(RunError::Return(v));
                }
                // A fault ends the scope as its end would, from line `i`; the fault is the error
                // shown, whatever its cleanup meets (DESIGN ›A checked error is a fault: the
                // task that hit it is cancelled‹).
                Err(fault) => {
                    let _ = run_exit(rt, node, i);
                    return Err(fault);
                }
            }
        }
        run_exit(rt, node, exprs.len())?;
        Ok(last)
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run`].
    unsafe {
        let Some(exprs) = exprs_of(node) else {
            return Err(CompileError::EmptyScope);
        };
        let lines: Vec<DyadPtr> = exprs
            .iter()
            .copied()
            .filter(|&e| !super::numtype::is_comment_type(dyad::ty(e)))
            .collect();
        let exit = exit_of(node);
        let exit = if exit.is_null() { &[][..] } else { array::items(exit) };
        lw.lower_with_teardowns(&lines, exit)?.ok_or(CompileError::EmptyScope)
    }
}
