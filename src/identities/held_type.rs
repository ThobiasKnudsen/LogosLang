// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A `type (…)` written in a body: lexed once where it is written and built each
//! time the node runs, with that run's values, a fresh type per run. Node shape
//! `{type: held_type, value: [text, cells, scope, depth, op]}`. DESIGN ›A type is
//! a comptime value, resolved in the pass‹.

use super::callable::{self, Callables};
use super::numtype::NumType;
use super::{meta, Cx};
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
        &["text", "cells", "scope", "depth", "op"],
    );
    let held_type = cx.store.alloc_raw(cx.type_, record);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    HeldTypeIds { held_type, leaf }
}

/// `text` holds the body with its brackets; `cells` must live for the run;
/// `scope` is the scope open where it is written, `depth` the function frames open there.
pub(crate) fn build(
    store: &mut Store,
    types: &Core,
    text: DyadPtr,
    cells: *mut ParsingTape,
    scope: DyadPtr,
    depth: usize,
) -> DyadPtr {
    let u64_ty = types.numtypes[NumType::U64 as usize];
    let mut word = |n: u64| {
        let storage = store.alloc_bytes(&n.to_ne_bytes());
        store.alloc_raw(u64_ty, storage)
    };
    let cells = word(cells as usize as u64);
    let depth = word(depth as u64);
    let value = store.alloc_operands(&[text, cells, scope, depth, types.held_type.leaf]);
    store.alloc_raw(types.held_type.held_type, value)
}

/// The parts `build` stored: text, cells, scope, depth.
///
/// # Safety
/// `node` must be a node from `build`.
pub(crate) unsafe fn parts(node: DyadPtr) -> (DyadPtr, *mut ParsingTape, DyadPtr, usize) {
    let ops = (*node).value as *const DyadPtr;
    let word = |p: DyadPtr| std::ptr::read_unaligned((*p).value as *const u64);
    (*ops, word(*ops.add(1)) as usize as *mut ParsingTape, *ops.add(2), word(*ops.add(3)) as usize)
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a held type node, whose op slot holds this leaf.
    unsafe { rt.mint_held(node) }
}
