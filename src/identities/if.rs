// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `if cond then` with an optional `else else`, each branch a bracket or the
//! next expression: the node is `[cond, then, else, then_ends, else_ends, condition_ends, op]`,
//! the else slot null when absent, each arm's `_ends` the names it frees at its end because
//! the other arm moved or freed them, and `condition_ends` both, freed by a condition that
//! leaves before either arm runs (DESIGN ›`move` and `free` are static: the parse marks the
//! name dead‹, rule 2). The surface parse lives in [`crate::parse::Parser::parse_if`]; here
//! the identity, its run native and lowering, and the `else` token.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{array, meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

const IF_COND: usize = 0;
const IF_THEN: usize = 1;
const IF_ELSE: usize = 2;
/// Null, or an `array` of held places in declaration order.
const IF_THEN_ENDS: usize = 3;
const IF_ELSE_ENDS: usize = 4;
const IF_CONDITION_ENDS: usize = 5;

/// Returns `(identity, leaf, else token)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::READER,
        Assoc::Left,
        &["condition", "then", "else", "then_ends", "else_ends", "condition_ends", "op"],
    );
    let if_ = cx.store.alloc_head(cx.type_, record);
    cx.declare("if", if_);
    cx.metas.insert(if_, |p, id, tape| {
        let node = p.parse_if(id)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    cx.lower.insert(if_, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);

    // `else` is a parse-only token between the branches, not a function.
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
    let else_ = cx.store.alloc_head(cx.type_, record);
    cx.declare("else", else_);

    (if_, leaf, else_)
}

/// `els` null for an else-less `if`; no arm frees anything yet.
pub(crate) fn build(
    store: &mut Store,
    types: &Core,
    cond: DyadPtr,
    then: DyadPtr,
    els: DyadPtr,
) -> DyadPtr {
    let none = std::ptr::null_mut();
    store.alloc_words(types.if_, &[cond, then, els, none, none, none, types.ops.if_])
}

/// What an `if` gives back: what its arms hand on where every arm gives a value of one type and
/// every arm makes its value or every arm borrows one, else `void`. DESIGN ›`if` reads its own
/// right side‹, ›A node's output type is per node, and its parse writes it‹.
///
/// # Safety
/// `node` must be an `if` node [`build`] made.
pub(crate) unsafe fn output_type(types: &Core, node: DyadPtr) -> DyadPtr {
    value_type(types, node).unwrap_or(types.void_)
}

/// The type an `if` gives where its value is used, or why it gives none: it needs an `else`,
/// every arm must give a value, the arms one type, and every arm make its value or every arm
/// borrow one. DESIGN ›`if` reads its own right side‹.
///
/// # Safety
/// `node` must be an `if` node [`build`] made.
pub(crate) unsafe fn value_type(types: &Core, node: DyadPtr) -> Result<DyadPtr, ParseError> {
    read(types, node).map(|v| v.handed)
}

/// An `if`'s value as one walk over its arms reads it, each nested `if` read once, so a chain
/// of `else if` arms reads in time in step with its length (DESIGN ›Reading a program takes
/// time in step with its size; no walk runs once per path‹).
#[derive(Clone, Copy)]
struct ArmsRead {
    /// The arms' one type before a plain number is handed on, so a plain number stays a
    /// `rational_number` beside an arm of a number type.
    ty: DyadPtr,
    /// What the then arm hands on.
    handed: DyadPtr,
    /// Whether every arm makes its value (`Some(true)`) or borrows it, as
    /// [`super::drop_model::handed_ends`] reads it.
    made: Option<bool>,
}

/// # Safety
/// `node` must be an `if` node [`build`] made.
unsafe fn read(types: &Core, node: DyadPtr) -> Result<ArmsRead, ParseError> {
    let (_, then, els) = branches(node);
    if els.is_null() {
        return Err(ParseError::MissingElse);
    }
    // An arm that gives nothing makes the `if` give nothing, whichever arm it is, so a sibling
    // arm's two types are no use of a value.
    let mut values = Vec::with_capacity(2);
    for arm in [then, els] {
        match arm_value(types, arm) {
            Ok(v) => values.push(v),
            Err(ParseError::StatementAsValue) => return Err(ParseError::ArmGivesNothing),
            Err(ParseError::ArmsDiffer) => {}
            Err(e) => return Err(e),
        }
    }
    match values[..] {
        [a, b] if super::same_type(a.ty, b.ty) => {
            let made = super::drop_model::arms_end_alike(a.made, b.made)?;
            Ok(ArmsRead { made, ..a })
        }
        _ => Err(ParseError::ArmsDiffer),
    }
}

/// An arm read through a block to its last line and through a nested `if` to its arms.
///
/// # Safety
/// `arm` must be a reduced dyad from the store.
unsafe fn arm_value(types: &Core, arm: DyadPtr) -> Result<ArmsRead, ParseError> {
    if dyad::ty(arm) == types.scope {
        return match crate::parse::last_sequence_expr(arm) {
            Some(last) => arm_value(types, last),
            None => Err(ParseError::StatementAsValue),
        };
    }
    if dyad::ty(arm) == types.if_ {
        return read(types, arm);
    }
    let ty = super::read::value_type(types, arm)?;
    let made = super::drop_model::handed_ends(types, arm)?;
    Ok(ArmsRead { ty, handed: super::read::handed_on(types, arm), made })
}

/// # Safety
/// `node` must be an `if` node [`build`] made.
pub(crate) unsafe fn branches(node: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p.add(IF_COND), *p.add(IF_THEN), *p.add(IF_ELSE))
}

/// # Safety
/// As [`branches`]; the list's store outlives the slice.
unsafe fn ends_at<'a>(node: DyadPtr, at: usize) -> &'a [DyadPtr] {
    let arr = *(dyad::value(node) as *const DyadPtr).add(at);
    if arr.is_null() {
        &[]
    } else {
        array::items(arr)
    }
}

/// The places each arm frees at its end, the then arm's first; an else-less `if` gains the
/// arm that frees the second list.
///
/// # Safety
/// As [`ends_at`].
pub(crate) unsafe fn arm_ends<'a>(node: DyadPtr) -> (&'a [DyadPtr], &'a [DyadPtr]) {
    (ends_at(node, IF_THEN_ENDS), ends_at(node, IF_ELSE_ENDS))
}

/// # Safety
/// `node` must be an `if` node [`build`] made, not yet run; the ends held places.
pub(crate) unsafe fn set_ends(
    store: &mut Store,
    types: &Core,
    node: DyadPtr,
    [then_ends, else_ends, condition_ends]: [&[DyadPtr]; 3],
) {
    let p = dyad::value(node) as *mut DyadPtr;
    for (at, ends) in
        [(IF_THEN_ENDS, then_ends), (IF_ELSE_ENDS, else_ends), (IF_CONDITION_ENDS, condition_ends)]
    {
        if !ends.is_empty() {
            *p.add(at) = array::build(store, types.array_, ends);
        }
    }
}

/// An else-less `if` yields unit either way, matching the compiled merge. The arm that ran
/// frees, at its end or on a `return` or fault through it, what the other arm moved or freed,
/// and a condition that leaves frees what either arm would have.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `if` node [`build`] made; its ends are held places.
    unsafe {
        let (cond, then, els) = branches(node);
        let (then_ends, else_ends) = arm_ends(node);
        let truth = match rt.run(cond) {
            Ok(truth) => truth,
            Err(left) => return end_after(rt, Err(left), ends_at(node, IF_CONDITION_ENDS)),
        };
        let (arm, ends) = if truth != 0 { (then, then_ends) } else { (els, else_ends) };
        let value = if arm.is_null() { Ok(0) } else { rt.run(arm) };
        let value = end_after(rt, value, ends)?;
        Ok(if els.is_null() { 0 } else { value })
    }
}

/// Frees `ends`, last declared first, once `value` is known; a fault stays the error shown,
/// whatever that cleanup meets.
///
/// # Safety
/// `ends` must be held places laid out in the frame `rt` runs.
unsafe fn end_after(
    rt: &mut Runtime,
    value: Result<i64, RunError>,
    ends: &[DyadPtr],
) -> Result<i64, RunError> {
    let ended = ends.iter().rev().try_for_each(|&place| super::drop_model::end_held(rt, place));
    match (value, ended) {
        (Err(fault), _) if !matches!(fault, RunError::Return(_)) => Err(fault),
        (_, Err(e)) => Err(e),
        (value, Ok(())) => value,
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is an `if` node [`build`] made.
    unsafe {
        let (cond, then, els) = branches(node);
        let (then_ends, else_ends) = arm_ends(node);
        if els.is_null() {
            return lw.lower_if_stmt(cond, then, then_ends, else_ends);
        }
        lw.lower_if(cond, [(then, then_ends), (els, else_ends)])
    }
}
