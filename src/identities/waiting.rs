// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! What waits for the run to parse it, since its value's type is known only then: a line of a
//! body that reads a `dyad` value, or a `while` condition that reads a `dyad` name, parsed where
//! it is written up to that read and from there each time it runs, over the nodes such values
//! hold then (›A `dyad ?` place is transparent: a placeholder for a new node of any type‹
//! [[R293]]).

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
    pub line_leaf: DyadPtr,
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
    let line_leaf = callable::mint_native(cx.store, cs.callable, run_line, cs.seed_native);
    WaitingIds { line, line_leaf }
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

/// The line `node` gives a value of `into`, which its parse at run lands there.
///
/// # Safety
/// `node` must be a node from [`build_line`] that has not run.
pub(crate) unsafe fn set_into(node: DyadPtr, into: DyadPtr) {
    let ops = dyad::value(node) as *mut DyadPtr;
    *ops.add(INTO) = into;
    *ops.add(LINE_OUTPUT) = into;
}

/// Where `value`, a quote's `{…}`, gives the value of a line held for the run, that line gives a
/// `dyad`: the node its value lands in when it runs, since its type is known only then. Whether
/// it did.
///
/// # Safety
/// `value` must be a reduced dyad from the store, parsed just now.
pub(crate) unsafe fn give_dyad(types: &Core, value: DyadPtr) -> bool {
    let tail = super::trailing_expr(types, value);
    if tail.is_null() || dyad::ty(tail) != types.waiting.line {
        return false;
    }
    set_into(tail, types.dyad_);
    true
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
