// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `try`: over a `T!`, its `value`, and over one that holds `err`, a `return` from the function,
//! whose own `T!` it fills with that error, one hop longer; at the top level, the error handed to
//! the command line, which stops the run. The node is `[operand, check, value, op,
//! output_type]`, run in that order: `operand`, a value the line makes and keeps, null where the
//! `T!` is a place; `check`, `if v holds err ( return … )`; and `value`, `v.value`, each over the
//! kept value or the place. ›Target: errors are values (`T!`), handled by `if`, passed on by
//! `try`…‹ [[R140]]

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{flow, meta, union, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::{self, DyadPtr};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct TryIds {
    /// The word, and the type of the node it builds.
    pub try_: DyadPtr,
    pub leaf: DyadPtr,
    /// A read of the value its line holds for a node that ran in it before, `[node, op,
    /// output_type]`.
    pub kept: DyadPtr,
    pub kept_leaf: DyadPtr,
}

/// At the rank of `return` (›Every identity has a `parse_rank`, and `(` works in two steps‹
/// [[R259]]).
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> TryIds {
    let roles = &["operand", "check", "value", "op", "output_type"];
    let try_ = super::drop_model::keyword(cx, "try", meta::prec::RETURN, roles, |p, _id, tape| {
        p.construct_try(tape)
    });
    // SAFETY: `try_` carries the record `keyword` built, whose roles name `value`.
    unsafe { meta::install_hands_on(try_, &["value"], &[]) };
    cx.lower.insert(try_, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["node", "op", "output_type"],
    );
    let kept = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(kept, lower_kept);
    let kept_leaf = callable::mint_native(cx.store, cs.callable, run_kept, cs.seed_native);
    TryIds { try_, leaf, kept, kept_leaf }
}

/// `try` over `operand`, a value of the union `fallible` whose members are `value` and `err`,
/// inside a function that gives `output`, whose `err` member is `output_err`, or at the top level
/// where `receiver` is `None`: the `try` node, and the `return` it leaves the function by, for the
/// parse to note as a written one. A `kept` operand is a value its line makes and holds, which
/// runs once and is read again where the line holds it, and whose `value` the `try` takes out of
/// it; any other is a place, read where it is needed.
///
/// # Safety
/// `operand` must be a reduced dyad of the type `fallible`, a `T!` whose member holes are `value`
/// and `err`; `output` a `T!` whose `err` member hole is `output_err`.
pub(crate) unsafe fn build(
    store: &mut Store,
    types: &Core,
    operand: DyadPtr,
    [fallible, value, err]: [DyadPtr; 3],
    receiver: Option<(DyadPtr, DyadPtr)>,
    kept: bool,
) -> (DyadPtr, Option<DyadPtr>) {
    let none = std::ptr::null_mut();
    // Built first, so the hop it adds can name it as its location; its parts are written below.
    let node = store.alloc_words(types.try_.try_, &[none; 5]);
    let (first, read) = if kept {
        let words = [operand, types.try_.kept_leaf, fallible];
        (operand, store.alloc_words(types.try_.kept, &words))
    } else {
        (none, operand)
    };
    // `passed` makes the longer error of its own and the command line only reads it, so the error
    // is read and the union frees it.
    let error = union::build_member(store, types, read, fallible, err, union::Access::Read);
    let (leave, ret) = match receiver {
        Some((output, output_err)) => {
            let passing = super::error::build_passing(store, types, error, node);
            let filled = union::build_fill(store, types, passing, output, output_err);
            let ret = store.alloc_words(types.return_, &[filled, none, types.ops.return_]);
            (ret, Some(ret))
        }
        None => (super::error::build_handed(store, types, error), None),
    };
    let holds = union::build_holds(store, types, read, fallible, err);
    let check = super::if_mod::build(store, types, holds, leave, none);
    let access = if kept { union::Access::Take } else { union::Access::Read };
    let given = union::build_member(store, types, read, fallible, value, access);
    let parts = [first, check, given, types.try_.leaf, super::hole::type_in(value)];
    std::ptr::copy_nonoverlapping(parts.as_ptr(), dyad::value(node) as *mut DyadPtr, parts.len());
    (node, ret)
}

/// # Safety
/// `node` must be a `try` node [`build`] made.
unsafe fn parts(node: DyadPtr) -> [DyadPtr; 3] {
    let ops = dyad::value(node) as *const DyadPtr;
    [*ops, *ops.add(1), *ops.add(2)]
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `try` node, its parts reduced dyads, the first null where it reads a
    // place.
    unsafe {
        let [operand, check, value] = parts(node);
        if !operand.is_null() {
            rt.run(operand)?;
        }
        rt.run_line(check)?;
        rt.run(value)
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run`].
    unsafe {
        let [operand, check, value] = parts(node);
        if !operand.is_null() {
            lw.lower(operand)?;
        }
        flow::lower_line(lw, check)?;
        lw.lower(value)
    }
}

/// # Safety
/// `node` must be a kept node [`build`] made.
unsafe fn kept_of(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr)
}

fn run_kept(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a kept node, whose first word is a node from the store.
    unsafe { rt.kept_value(kept_of(node)) }.ok_or(RunError::NotKept)
}

fn lower_kept(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_kept`].
    unsafe { lw.kept_value(kept_of(node)) }
        .ok_or(CompileError::Internal("a value its line holds is read where no line holds it"))
}
