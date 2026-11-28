// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A `type (…)` written in a body: lexed once where it is written and built each
//! time the node runs, with that run's values. Node shape
//! `{type: held_type, value: [text, cells, scope, frames, name, reads, walk, op]}`. DESIGN ›A
//! type is a comptime value, resolved in the pass‹.

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
        &["text", "cells", "scope", "frames", "name", "reads", "walk", "op", "output_type"],
    );
    let held_type = cx.store.alloc_head(cx.type_, record);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    HeldTypeIds { held_type, leaf }
}

/// What the walk where the body is written found in the code that runs when it is constructed
/// (›Every type is minted by its contents through `type_mint`; the name that holds a `type ( … )`
/// gives it its identity‹ [[R383]]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Construction<'a> {
    /// A read the walk cannot follow: the body is built at every reach.
    Unseen,
    /// The body is built once for each set of values of `reads`, the names declared outside it
    /// that its construction reads and that can change. Where `effect_end` is some, the lines
    /// that end at or before that byte of the text have an effect and run at every reach.
    Keyed { reads: &'a [DyadPtr], effect_end: Option<usize> },
}

/// The `walk` word: `0` for [`Construction::Unseen`], `1` for a keyed body with no effect, and
/// `2 + end` for one whose lines with an effect end at `end`.
fn walk_word(construction: &Construction) -> u64 {
    match construction {
        Construction::Unseen => 0,
        Construction::Keyed { effect_end: None, .. } => 1,
        Construction::Keyed { effect_end: Some(end), .. } => 2 + *end as u64,
    }
}

/// `text` holds the body with its brackets; `cells` must live for the run; `scope` is the
/// scope open where it is written, `frames` the `fn` nodes open there, outermost first, and
/// `name` the binding of the name its declaration gives the type, or null. The name is kept by
/// its address, as `cells` is: the node names it and never reads it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build(
    store: &mut Store,
    types: &Core,
    text: DyadPtr,
    cells: *mut ParsingTape,
    scope: DyadPtr,
    frames: &[DyadPtr],
    name: DyadPtr,
    construction: &Construction,
) -> DyadPtr {
    let u64_ty = types.numtypes[NumType::U64 as usize];
    let cells = store.alloc_blob(u64_ty, &(cells as usize as u64).to_ne_bytes());
    let array = |store: &mut Store, items: &[DyadPtr]| {
        if items.is_empty() {
            std::ptr::null_mut()
        } else {
            super::array::build(store, types.array_, items)
        }
    };
    let frames = array(store, frames);
    let reads = match construction {
        Construction::Keyed { reads, .. } => array(store, reads),
        Construction::Unseen => std::ptr::null_mut(),
    };
    let walk = store.alloc_blob(u64_ty, &walk_word(construction).to_ne_bytes());
    let name = store.alloc_blob(u64_ty, &(name as usize as u64).to_ne_bytes());
    let ops = [text, cells, scope, frames, name, reads, walk, types.held_type.leaf, types.type_];
    store.alloc_words(types.held_type.held_type, &ops)
}

/// What `build` stored.
pub(crate) struct Parts<'a> {
    pub(crate) text: DyadPtr,
    pub(crate) cells: *mut ParsingTape,
    pub(crate) scope: DyadPtr,
    pub(crate) frames: &'a [DyadPtr],
    pub(crate) name: DyadPtr,
    pub(crate) construction: Construction<'a>,
}

/// # Safety
/// `node` must be a node from `build`; the store must outlive the returned slice.
pub(crate) unsafe fn parts<'a>(node: DyadPtr) -> Parts<'a> {
    let ops = dyad::value(node) as *const DyadPtr;
    let word = |k: usize| std::ptr::read_unaligned(dyad::value(*ops.add(k)) as *const u64);
    let items = |k: usize| {
        let array = *ops.add(k);
        if array.is_null() {
            &[][..]
        } else {
            super::array::items(array)
        }
    };
    let construction = match word(6) {
        0 => Construction::Unseen,
        1 => Construction::Keyed { reads: items(5), effect_end: None },
        end => Construction::Keyed { reads: items(5), effect_end: Some((end - 2) as usize) },
    };
    Parts {
        text: *ops,
        cells: word(1) as usize as *mut ParsingTape,
        scope: *ops.add(2),
        frames: items(3),
        name: word(4) as usize as DyadPtr,
        construction,
    }
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a held type node, whose op slot holds this leaf.
    unsafe { rt.mint_held(node) }
}
