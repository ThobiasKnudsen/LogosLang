// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! What waits for the run to parse it, since a type its parse needs is known only then: a line of
//! a body that reads a `dyad` value or reads after a type a name or a call gives only then, or a
//! `while` condition that reads a `dyad` name, parsed where it is written up to that read and
//! from there each time it runs, over the nodes and types such values hold then (›A `dyad ?`
//! place is transparent: a placeholder for a new node of any type‹ [[R293]], ›A type known only
//! when the program runs still works; what needs it is parsed when its line runs‹ [[R377]]).

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{array, meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::{self, DyadPtr};
use crate::parse::{Assoc, HeldLine};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct WaitingIds {
    /// `[text, cells, scope, frames, into, op, output_type, own]`: `into` is the type the line's
    /// value lands in, null for a line whose value nothing takes; `own` the `own` word where it
    /// lands in a new node, a `dyad`.
    pub line: DyadPtr,
    pub line_leaf: DyadPtr,
}

pub(super) fn register(cx: &mut Cx, cs: &Callables) -> WaitingIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["text", "cells", "scope", "frames", "into", "op", "output_type", "own"],
    );
    let line = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(line, lower_line);
    let line_leaf = callable::mint_native(cx.store, cs.callable, run_line, cs.seed_native);
    WaitingIds { line, line_leaf }
}

/// Compiled code has baked the decisions a type's fields drive, so it cannot parse a line when it
/// runs (›A type known only when the program runs still works; what needs it is parsed when its
/// line runs‹ [[R377]]).
fn lower_line(_: &mut Lowerer, _: DyadPtr) -> Result<Value, CompileError> {
    Err(CompileError::ParsedAtRun)
}

const INTO: usize = 4;
const LINE_OUTPUT: usize = 6;
const OWN: usize = 7;

/// `text` holds the line; `line` must live for the run; `scope` is the scope open where it is
/// written, `frames` the `fn` nodes open there, outermost first.
pub(crate) fn build_line(
    store: &mut Store,
    types: &Core,
    text: DyadPtr,
    line: *mut HeldLine,
    scope: DyadPtr,
    frames: &[DyadPtr],
) -> DyadPtr {
    let u64_ty = types.numtypes[super::NumType::U64 as usize];
    let cells = store.alloc_blob(u64_ty, &(line as usize as u64).to_ne_bytes());
    let frames = if frames.is_empty() {
        std::ptr::null_mut()
    } else {
        array::build(store, types.array_, frames)
    };
    let none = std::ptr::null_mut();
    let ops = [text, cells, scope, frames, none, types.waiting.line_leaf, types.void_, none];
    store.alloc_words(types.waiting.line, &ops)
}

/// The line `node` gives a value of `into`, which its parse at run lands there: in a new node
/// for a `dyad`, which is made for whoever takes it.
///
/// # Safety
/// `node` must be a node from [`build_line`] that has not run.
pub(crate) unsafe fn set_into(types: &Core, node: DyadPtr, into: DyadPtr) {
    let ops = dyad::value(node) as *mut DyadPtr;
    *ops.add(INTO) = into;
    *ops.add(LINE_OUTPUT) = into;
    *ops.add(OWN) = if into == types.dyad_ { types.own_ } else { std::ptr::null_mut() };
}

/// Where `value` ends in a line held for the run, itself, a block's last line or an `if`'s arm,
/// that line lands its value in `into` when it runs. Whether such a line gives a value is known
/// only when it runs, so where a value is needed the line that ends the block gives it. Whether
/// there was one.
///
/// # Safety
/// `value` must be null or a reduced dyad from the store, parsed just now; `into` a type node.
pub(crate) unsafe fn lands_in(types: &Core, value: DyadPtr, into: DyadPtr) -> bool {
    if value.is_null() {
        return false;
    }
    let ty = dyad::ty(value);
    if ty == types.waiting.line {
        set_into(types, value, into);
        return true;
    }
    if ty == types.scope {
        let lines = super::scope::exprs_of(value).unwrap_or_default();
        let last = lines.iter().rev().find(|&&line| !super::flow::is_prose(types, line));
        return last.is_some_and(|&line| lands_in(types, line, into));
    }
    if ty == types.if_ {
        let (then, els) = super::if_mod::arm_slots(value);
        return lands_in(types, *then, into) | lands_in(types, *els, into);
    }
    false
}

/// The parts [`build_line`] stored: text, line, scope, the open `fn` nodes, `into`.
///
/// # Safety
/// `node` must be a node from `build_line`; the store must outlive the returned slice.
pub(crate) unsafe fn line_parts<'a>(
    node: DyadPtr,
) -> (DyadPtr, *mut HeldLine, DyadPtr, &'a [DyadPtr], DyadPtr) {
    let ops = dyad::value(node) as *const DyadPtr;
    let line = std::ptr::read_unaligned(dyad::value(*ops.add(1)) as *const u64);
    let frames = *ops.add(3);
    let frames = if frames.is_null() { &[][..] } else { array::items(frames) };
    (*ops, line as usize as *mut HeldLine, *ops.add(2), frames, *ops.add(INTO))
}

fn run_line(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a node from `build_line`, whose op slot holds this leaf.
    unsafe {
        let built = rt.parse_waiting(node)?;
        rt.run(built)
    }
}
