// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `scope`: the type of a scope node, the parser's membership marker and, for
//! a block, the sequence node itself: `[exprs, op, parent, exit, held, flow, kept_as]`, the
//! parent link set at [`mint`], the expressions (the `dyads` field) pushed as each line
//! completes, the op set at [`fill`] when the block closes, and the exit, what the
//! scope's end runs, and what its lines' ends free, set when it closes.
//! DESIGN ›Meta-navigation walks the graph; the scope stack is the graph's own spine‹

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::drop_model::ExitItem;
use super::flow::Flow;
use super::{array, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Constructed;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// Called before the build context exists, since the root scope is itself typed `scope`.
pub(super) fn register(store: &mut Store, type_: DyadPtr) -> DyadPtr {
    store.alloc_leaf(type_)
}

/// Returns the sequence leaf a sequence node references from its op slot.
/// DESIGN ›The scope's constructor is the driver‹: `scope (…)` is `(…)`; with
/// no bracket to its right, `scope` stands as the type.
pub(super) fn register_exec(cx: &mut Cx, scope_: DyadPtr, cs: &Callables) -> DyadPtr {
    cx.declare("scope", scope_);
    cx.metas.insert(scope_, |p, _id, tape| {
        if !p.reads_own_bracket(tape) {
            return Ok(Constructed::Decline);
        }
        p.expect_open()?;
        let body = p.parse_sequence()?;
        p.expect_close()?;
        tape.place_bracket(body);
        Ok(Constructed::Placed)
    });
    cx.lower.insert(scope_, lower);
    cx.placings.insert(scope_, super::place::whole);
    callable::mint_native(cx.store, cs.callable, run, cs.seed_native)
}

/// The slots of a scope node's value, `[exprs, op, parent, exit, held, flow, kept_as]`.
const EXPRS: usize = 0;
const OP: usize = 1;
const PARENT: usize = 2;
/// Null, or an `array` of exit items (`drop_model::exit_item`) in line order.
const EXIT: usize = 3;
/// Null, or an `array` of the nodes whose values the scope's lines made and nothing took: the
/// end of the line a value was made in frees it (›A value's teardown runs where its life ends;
/// the ending identity reads the type's `free` slot‹ [[R57]]).
const HELD: usize = 4;
/// How a closed block's lines end, once a walk has read it: null before, else a word,
/// `1 + gives + 2 * falls`, never an address.
const FLOW: usize = 5;
/// Null, or for the block of a `{ … }` the record its value is of, which other `{ … }` of the same
/// shape share (›`{ … }` outside text is a scope kept as a value, read with `.`‹ [[R374]]).
const KEPT_AS: usize = 6;
pub(crate) const SLOTS: usize = KEPT_AS + 1;

/// The block's membership key while it parses, and the sequence node once
/// [`fill`] gives it its expressions; `parent` is null at the arche. A scope that
/// holds names alone (the root, a type's member scope) is minted with no parent.
pub(crate) fn mint(store: &mut Store, scope_ty: DyadPtr, parent: DyadPtr) -> DyadPtr {
    let none = std::ptr::null_mut();
    let mut slots = [none; SLOTS];
    slots[PARENT] = parent;
    store.alloc_words(scope_ty, &slots)
}

/// The record the block `node` is kept as a value of, when it is a `{ … }`'s; null otherwise.
///
/// # Safety
/// `node` must be a scope node from the store.
pub(crate) unsafe fn kept_as(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr).add(KEPT_AS)
}

/// The block `node` is a `{ … }`'s, kept as a value of the record `ty`.
///
/// # Safety
/// `node` must be a scope node from the store and `ty` a record type node.
pub(crate) unsafe fn set_kept_as(node: DyadPtr, ty: DyadPtr) {
    *(dyad::value(node) as *mut DyadPtr).add(KEPT_AS) = ty;
}

/// How the closed block `node`'s lines end, if a walk has read it.
///
/// # Safety
/// `node` must be a scope node from the store.
pub(crate) unsafe fn kept_flow(node: DyadPtr) -> Option<Flow> {
    let word = *(dyad::value(node) as *const DyadPtr).add(FLOW) as usize;
    (word != 0).then(|| Flow { gives: (word - 1) & 1 != 0, falls: (word - 1) & 2 != 0 })
}

/// Keeps how `node`'s lines end, once it is closed, so no walk reads them again (›Reading a
/// program takes time in step with its size; no walk runs once per path‹ [[R170]]).
///
/// # Safety
/// `node` must be a scope node from the store, and `flow` how its lines end.
pub(crate) unsafe fn keep_flow(node: DyadPtr, flow: Flow) {
    let slots = dyad::value(node) as *mut DyadPtr;
    if (*slots.add(OP)).is_null() {
        return;
    }
    let word = 1 + usize::from(flow.gives) + 2 * usize::from(flow.falls);
    *slots.add(FLOW) = std::ptr::without_provenance_mut(word);
}

/// The block closed: it runs as a sequence over its `dyads`.
///
/// # Safety
/// `node` must be a scope [`mint`] built; nothing else may hold its slots.
pub(crate) unsafe fn fill(types: &Core, node: DyadPtr) {
    *(dyad::value(node) as *mut DyadPtr).add(OP) = types.ops.scope_;
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

/// Line `k` of the scope is `item` from here.
///
/// # Safety
/// `node` must be a scope or `square_brackets` node from the store holding more than `k` lines;
/// `item` a reduced dyad.
pub(crate) unsafe fn set_item(node: DyadPtr, k: usize, item: DyadPtr) {
    let (len, lines) = array::parts(exprs_array(node));
    debug_assert!(k < len, "line {k} of a scope with {len}");
    *(lines as *mut DyadPtr).add(k) = item;
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
    types: &Core,
    node: DyadPtr,
    exprs: &[DyadPtr],
) -> DyadPtr {
    let slots = dyad::value(node) as *const DyadPtr;
    let lines = array::build(store, types.array_, exprs);
    let mut words = [std::ptr::null_mut(); SLOTS];
    words[EXPRS] = lines;
    for k in [OP, PARENT, EXIT, HELD] {
        words[k] = *slots.add(k);
    }
    store.alloc_words(dyad::ty(node), &words)
}

/// The nodes whose values the scope's lines free at their ends.
///
/// # Safety
/// `node` must be a scope node from the store; the store must outlive the returned slice.
pub(crate) unsafe fn held_of<'a>(node: DyadPtr) -> &'a [DyadPtr] {
    let held = *(dyad::value(node) as *const DyadPtr).add(HELD);
    if held.is_null() {
        &[]
    } else {
        array::items(held)
    }
}

/// Appends `nodes` to what the scope's lines free at their ends, the array made on first use.
///
/// # Safety
/// `node` must be a scope node from the store; `nodes` reduced dyads.
pub(crate) unsafe fn extend_held(
    store: &mut Store,
    array_ty: DyadPtr,
    node: DyadPtr,
    nodes: &[DyadPtr],
) {
    extend_slot(store, array_ty, node, HELD, nodes);
}

/// Appends `items` to the array in the scope's slot `at`, made on first use.
///
/// # Safety
/// `node` must be a scope node from the store and `at` one of its array slots.
unsafe fn extend_slot(
    store: &mut Store,
    array_ty: DyadPtr,
    node: DyadPtr,
    at: usize,
    items: &[DyadPtr],
) {
    if items.is_empty() {
        return;
    }
    let slot = (dyad::value(node) as *mut DyadPtr).add(at);
    // Empty, so `push` knows its capacity.
    if (*slot).is_null() {
        *slot = array::build(store, array_ty, &[]);
    }
    for &item in items {
        array::push(store, *slot, item);
    }
}

/// # Safety
/// `node` must be a scope node from the store; the store must outlive the returned slice.
pub(crate) unsafe fn exit_items<'a>(node: DyadPtr) -> &'a [DyadPtr] {
    let exit = *(dyad::value(node) as *const DyadPtr).add(EXIT);
    if exit.is_null() {
        &[]
    } else {
        array::items(exit)
    }
}

/// The scope's exit is `items` from here.
///
/// # Safety
/// `node` must be a scope node from the store; `items` exit items.
pub(crate) unsafe fn set_exit(
    store: &mut Store,
    array_ty: DyadPtr,
    node: DyadPtr,
    items: &[DyadPtr],
) {
    *(dyad::value(node) as *mut DyadPtr).add(EXIT) = array::build(store, array_ty, items);
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
    extend_slot(store, array_ty, node, EXIT, items);
}

/// The scope's end, reached after `reached` of its lines completed: every exit item held
/// there runs, last first, an authored `defer` its expression and a held name its
/// value's teardown (DESIGN ›A value's teardown runs where its life ends; the ending
/// identity reads the type's `free` slot‹ [[R57]]). Compiled, `Lowerer::end_to`.
///
/// # Safety
/// `node` must be a scope node from the store, its places laid out in the frame `rt` runs.
pub(crate) unsafe fn run_exit(
    rt: &mut Runtime,
    node: DyadPtr,
    reached: usize,
) -> Result<(), RunError> {
    run_exit_items(rt, node, |_, item| item.held_at(reached))
}

/// The scope's end after its line `failed` faulted: what [`run_exit`] would run there, and what
/// that line ended and did not reach (DESIGN ›`free` and `move` end a name; no drop flag‹).
///
/// # Safety
/// As [`run_exit`].
unsafe fn run_exit_after_fault(
    rt: &mut Runtime,
    node: DyadPtr,
    failed: usize,
) -> Result<(), RunError> {
    run_exit_items(rt, node, |types, item| item.held_at_fault(types, failed))
}

/// A fault stops the end. A `defer` line's values end with it, when the exit runs it.
///
/// # Safety
/// As [`run_exit`].
unsafe fn run_exit_items(
    rt: &mut Runtime,
    node: DyadPtr,
    runs: impl Fn(&crate::Core, &ExitItem) -> bool,
) -> Result<(), RunError> {
    rt.push_plan(held_of(node));
    let ran = run_held_exit_items(rt, node, runs);
    rt.pop_plan();
    ran
}

/// [`run_exit_items`] with the scope's plan in force.
///
/// # Safety
/// As [`run_exit`].
unsafe fn run_held_exit_items(
    rt: &mut Runtime,
    node: DyadPtr,
    runs: impl Fn(&crate::Core, &ExitItem) -> bool,
) -> Result<(), RunError> {
    for &item in exit_items(node).iter().rev() {
        let item = super::drop_model::exit_item_of(item);
        if runs(rt.types(), &item) {
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
    unsafe { run_given(rt, node).map(|given| given.unwrap_or(0)) }
}

/// The scope run as a line: the value its first line that gives one gave, its lines after that
/// never run, or `None` where every line went on without one (›A scope's value is what it
/// evaluates to, and `return` is an optional early exit from the enclosing function‹ [[R207]]).
///
/// # Safety
/// `node` must be a sequence node from the store.
pub(super) unsafe fn run_given(rt: &mut Runtime, node: DyadPtr) -> Result<Option<i64>, RunError> {
    let Some(exprs) = exprs_of(node) else {
        return Err(RunError::EmptyScope);
    };
    rt.push_plan(held_of(node));
    let ran = run_lines(rt, node, exprs);
    rt.pop_plan();
    ran
}

/// The scope's lines from its cursor, each ended as it completes, then its end.
///
/// # Safety
/// `node` must be a sequence node from the store and `exprs` its lines.
unsafe fn run_lines(
    rt: &mut Runtime,
    node: DyadPtr,
    exprs: &[DyadPtr],
) -> Result<Option<i64>, RunError> {
    let start = rt.take_cursor(node);
    // The pass ran the line that gave the value in this frame already.
    if let Some(value) = start.given {
        run_exit(rt, node, start.at + 1)?;
        return Ok(Some(value));
    }
    for (i, &expr) in exprs.iter().enumerate().skip(start.at) {
        // A line's scratch is freed with the line; a value given is handed on with its own.
        let mark = rt.stack_mark();
        let held = rt.line_mark();
        let ran = rt.run_line(expr);
        match super::drop_model::end_line(rt, held, ran) {
            Ok(None) => rt.stack_release(mark),
            // The line that gave the value completed and ends the scope.
            Ok(Some(value)) => {
                run_exit(rt, node, i + 1)?;
                return Ok(Some(value));
            }
            // A `return` leaves through this scope: what it holds at line `i` ends.
            Err(RunError::Return(v)) => {
                run_exit(rt, node, i)?;
                return Err(RunError::Return(v));
            }
            // A fault ends the scope from line `i`; the fault is the error shown, whatever
            // its cleanup meets (DESIGN ›A checked error is a fault: the task that hit it is
            // cancelled‹).
            Err(fault) => {
                let _ = run_exit_after_fault(rt, node, i);
                return Err(fault);
            }
        }
    }
    run_exit(rt, node, exprs.len())?;
    Ok(None)
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run`].
    unsafe {
        let Some(exprs) = exprs_of(node) else {
            return Err(CompileError::EmptyScope);
        };
        lw.lower_scope(node, exprs)?.ok_or(CompileError::EmptyScope)
    }
}

/// [`run_given`]'s lowering: the value a line gives passes out.
///
/// # Safety
/// As [`run`].
pub(super) unsafe fn lower_given(lw: &mut Lowerer, node: DyadPtr) -> Result<(), CompileError> {
    let Some(exprs) = exprs_of(node) else {
        return Err(CompileError::EmptyScope);
    };
    lw.lower_scope_given(node, exprs).map(|_| ())
}
