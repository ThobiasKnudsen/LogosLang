// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A `type (…)` written in a body: lexed once where it is written and built each
//! time the node runs, with that run's values, a fresh type per run. Node shape
//! `{type: held_type, value: [text, cells, scope, frames, op]}`. DESIGN ›A type is
//! a comptime value, resolved in the pass‹.

use super::callable::{self, Callables};
use super::numtype::NumType;
use super::{meta, Cx};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::ParsingTape;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct HeldTypeIds {
    pub held_type: DyadPtr,
    pub leaf: DyadPtr,
}

pub(super) fn register(cx: &mut Cx, cs: &Callables) -> HeldTypeIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["text", "cells", "scope", "frames", "op"],
    );
    let held_type = cx.store.alloc_head(cx.type_, record);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    HeldTypeIds { held_type, leaf }
}

/// `text` holds the body with its brackets; `cells` must live for the run; `scope` is the
/// scope open where it is written, `frames` the `fn` nodes open there, outermost first.
pub(crate) fn build(
    store: &mut Store,
    types: &Core,
    text: DyadPtr,
    cells: *mut ParsingTape,
    scope: DyadPtr,
    frames: &[DyadPtr],
) -> DyadPtr {
    let u64_ty = types.numtypes[NumType::U64 as usize];
    let cells = store.alloc_blob(u64_ty, &(cells as usize as u64).to_ne_bytes());
    let frames = if frames.is_empty() {
        std::ptr::null_mut()
    } else {
        super::array::build(store, types.array_, frames)
    };
    store
        .alloc_words(types.held_type.held_type, &[text, cells, scope, frames, types.held_type.leaf])
}

/// The parts `build` stored: text, cells, scope, the open `fn` nodes.
///
/// # Safety
/// `node` must be a node from `build`; the store must outlive the returned slice.
pub(crate) unsafe fn parts<'a>(
    node: DyadPtr,
) -> (DyadPtr, *mut ParsingTape, DyadPtr, &'a [DyadPtr]) {
    let ops = dyad::value(node) as *const DyadPtr;
    let cells = std::ptr::read_unaligned(dyad::value(*ops.add(1)) as *const u64);
    let frames = *ops.add(3);
    let frames = if frames.is_null() { &[][..] } else { super::array::items(frames) };
    (*ops, cells as usize as *mut ParsingTape, *ops.add(2), frames)
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a held type node, whose op slot holds this leaf.
    unsafe { rt.mint_held(node) }
}
