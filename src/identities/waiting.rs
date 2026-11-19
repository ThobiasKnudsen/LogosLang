// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! What waits for the run to parse it, since its value's type is known only then: a line of a
//! body that reads a `dyad` value, or a `while` condition that reads a `dyad` name, parsed where
//! it is written up to that read and from there each time it runs, over the nodes such values
//! hold then; and a body's last value read from a `dyad` place into the declared result (›A
//! `dyad ?` place is transparent: a placeholder for a new node of any type‹ [[R293]]).

use super::callable::{self, Callables};
use super::{array, meta, Cx};
use crate::dyad::{self, DyadPtr};
use crate::parse::{Assoc, HeldLine};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct WaitingIds {
    /// `[text, cells, scope, frames, into, op, output_type]`: `into` is the type the line's
    /// value lands in, null for a line whose value nothing takes.
    pub line: DyadPtr,
    /// `[value, into, ends, op, output_type]`: `value` gives the node a `dyad` place holds, and
    /// `ends` is non-null where it moved out of the place, so the read frees it.
    pub tail: DyadPtr,
    pub line_leaf: DyadPtr,
    pub tail_leaf: DyadPtr,
}

pub(super) fn register(cx: &mut Cx, cs: &Callables) -> WaitingIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["text", "cells", "scope", "frames", "into", "op", "output_type"],
    );
    let line = cx.store.alloc_head(cx.type_, record);
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["value", "into", "ends", "op", "output_type"],
    );
    let tail = cx.store.alloc_head(cx.type_, record);
    let line_leaf = callable::mint_native(cx.store, cs.callable, run_line, cs.seed_native);
    let tail_leaf = callable::mint_native(cx.store, cs.callable, run_tail, cs.seed_native);
    WaitingIds { line, tail, line_leaf, tail_leaf }
}

const INTO: usize = 4;
const LINE_OUTPUT: usize = 6;

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
    let ops =
        [text, cells, scope, frames, std::ptr::null_mut(), types.waiting.line_leaf, types.void_];
    store.alloc_words(types.waiting.line, &ops)
}

/// `value` read into `into` when it runs; a node that `moved` out of its place is freed once read.
pub(crate) fn build_tail(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    into: DyadPtr,
    moved: bool,
) -> DyadPtr {
    let ends = if moved { types.dyad_ } else { std::ptr::null_mut() };
    store.alloc_words(types.waiting.tail, &[value, into, ends, types.waiting.tail_leaf, into])
}

/// The line `node` gives a value of `into`, which its parse at run lands there.
///
/// # Safety
/// `node` must be a node from [`build_line`] that has not run.
pub(crate) unsafe fn set_into(node: DyadPtr, into: DyadPtr) {
    let ops = dyad::value(node) as *mut DyadPtr;
    *ops.add(INTO) = into;
    *ops.add(LINE_OUTPUT) = into;
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

/// The type a node from [`build_tail`] reads its place's node into.
///
/// # Safety
/// `node` must be a node from `build_tail`.
pub(crate) unsafe fn tail_into(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr).add(1)
}

fn run_line(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a node from `build_line`, whose op slot holds this leaf.
    unsafe {
        let built = rt.parse_waiting(node, std::ptr::null_mut())?;
        rt.run(built)
    }
}

fn run_tail(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a node from `build_tail`; a `dyad` value is a node's address or 0.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let held = rt.run(*ops)? as usize as DyadPtr;
        if held.is_null() {
            return Err(RunError::NullPointer);
        }
        // What the read builds stands on the store's copy of `held`, so `held` may go first.
        let built = rt.parse_waiting(node, held);
        if !(*ops.add(2)).is_null() {
            super::dyad_place::end(rt, held as i64);
        }
        rt.run(built?)
    }
}
