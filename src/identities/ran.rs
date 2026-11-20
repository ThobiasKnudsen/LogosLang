// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A value the pass ran early, standing in its place in the line being parsed: a run of it gives
//! what the value's run gave, and one with an end of its own the call that holds it or the line's
//! end ends, once (›The pass runs only as far as it must, in order, and never twice‹ [[R288]],
//! ›Holding is decided at the binding site, parameters included‹ [[R61]]).

use super::callable::{self, Callables};
use super::read::Carry;
use super::{meta, Cx};
use crate::dyad::{self, DyadPtr};
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct RanIds {
    /// `[word, op, output_type, own]`: `word` holds, as a `u64`, what the value's run gave; `own`
    /// the `own` word where the value has an end of its own, made where it ran.
    pub node: DyadPtr,
    pub leaf: DyadPtr,
}

/// No spelling: the pass builds one where it ran a value early.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> RanIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["word", "op", "output_type", "own"],
    );
    let node = cx.store.alloc_head(cx.type_, record);
    let leaf = callable::mint_native(cx.store, cs.callable, run_ran, cs.seed_native);
    RanIds { node, leaf }
}

/// The value of type `out` whose run gave `word`, standing where it was written; `ends` where
/// it has an end of its own.
pub(crate) fn build(
    store: &mut Store,
    types: &Core,
    word: i64,
    ends: bool,
    out: DyadPtr,
) -> DyadPtr {
    let u64_ty = types.numtypes[super::NumType::U64 as usize];
    let word = store.alloc_blob(u64_ty, &word.to_ne_bytes());
    let own = if ends { types.own_ } else { std::ptr::null_mut() };
    store.alloc_words(types.ran.node, &[word, types.ran.leaf, out, own])
}

const OWN: usize = 3;

/// What a run of a value that moves as `carry` gives, read out of `node`, a node holding it.
///
/// # Safety
/// `node` must hold a value of a type that moves as `carry`.
pub(crate) unsafe fn word_in(carry: Carry, node: DyadPtr) -> i64 {
    super::assign::load(carry, dyad::value(node))
}

/// What the value's run gave.
///
/// # Safety
/// `node` must be a node from [`build`].
pub(crate) unsafe fn word(node: DyadPtr) -> i64 {
    let word = *(dyad::value(node) as *const DyadPtr);
    std::ptr::read_unaligned(dyad::value(word) as *const i64)
}

/// Whether the value has an end of its own.
///
/// # Safety
/// `node` must be a node from [`build`].
pub(crate) unsafe fn ends(node: DyadPtr) -> bool {
    !(*(dyad::value(node) as *const DyadPtr).add(OWN)).is_null()
}

fn run_ran(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a node from `build`, whose op slot holds this leaf.
    unsafe {
        if ends(node) && !rt.take_stood(node) {
            return Err(RunError::StoodTaken);
        }
        Ok(word(node))
    }
}
