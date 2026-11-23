// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Conversion, written (`i64 (a)`, `i32 lossy (x)`, `@i32 &a`) or not: one shared `convert`
//! identity for every conversion between number types and between pointer types, the node
//! `[operand]` under its mint for the source and target types, number types or `rational_number`
//! as the source for a rational value. A pointer keeps its address; only its type changes. A
//! float written into an integer type is the checked conversion, which gives the integer type's
//! `T!`.

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::{apply_cast, is_pointer_type, of_type_node};
use super::{meta, mint, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// No spelling: a conversion is built from the `logos(value)` surface.
/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record =
        meta::operand_record(cx, meta::TUPLE_TAG, meta::prec::INERT, Assoc::Left, &["operand"]);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(id, lower);
    cx.choosers.insert(id, choose);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

/// The written conversion that may find no value of its target, read as the `T!` it gives,
/// `[operand, op, output_type, from]`: `output_type` the target's `T!`, `from` the operand's type.
/// Returns `(identity, leaf)`.
pub(super) fn register_checked(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let roles = ["operand", "op", "output_type", "from"];
    let record = meta::operand_record(cx, meta::TUPLE_TAG, meta::prec::INERT, Assoc::Left, &roles);
    let id = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `id` carries the record just built.
    unsafe { meta::mark_own(id) };
    cx.lower.insert(id, lower_checked);
    let leaf = callable::mint_native(cx.store, cs.callable, run_checked, cs.seed_native);
    (id, leaf)
}

/// Whether a value of `from` converted into `to` may have no value of `to`: a float, which may be
/// no number (NaN) or infinite, into an integer type (›A checked error is a fault: the task that
/// hit it is cancelled‹ [[R141]]).
///
/// # Safety
/// `from` and `to` must be null or type nodes from the store.
pub(crate) unsafe fn may_fail(types: &Core, from: DyadPtr, to: DyadPtr) -> bool {
    let from = super::gate::under(types.through(from));
    let number = |t| super::is_numtype_node(types, t);
    number(from) && of_type_node(from).is_float() && number(to) && !of_type_node(to).is_float()
}

/// The written conversion of `operand`, of the float type `from`, into the integer type whose
/// `T!` is `fallible`: `fallible`'s `value` where the float has a whole number, else an error of
/// `error.no_whole_number` in its `err`.
///
/// # Safety
/// `operand` must be a reduced dyad giving a value of `from`, `from` a float type and
/// `fallible` the `T!` of an integer type, all from the store.
pub(crate) unsafe fn build_checked(
    store: &mut Store,
    types: &Core,
    operand: DyadPtr,
    from: DyadPtr,
    fallible: DyadPtr,
) -> DyadPtr {
    let from = super::gate::under(types.through(from));
    store.alloc_words(types.checked_convert, &[operand, types.checked_convert_leaf, fallible, from])
}

/// `(operand, union, from)`.
///
/// # Safety
/// `node` must be a node [`build_checked`] built.
unsafe fn checked_parts(node: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr) {
    let words = dyad::value(node) as *const DyadPtr;
    (*words, *words.add(2), *words.add(3))
}

/// The member of `node`'s union that `v`, the operand's value, fills, and the member's value.
///
/// # Safety
/// `node` must be a node [`build_checked`] built and `v` a value of its operand's type.
unsafe fn checked_member(
    rt: &mut Runtime,
    node: DyadPtr,
    v: i64,
) -> Result<(DyadPtr, i64), RunError> {
    let (_, union, from) = checked_parts(node);
    let (value, err) = super::union::fallible_members(rt.types(), union)
        .expect("a `T!` has a `value` and an `err`");
    let to = of_type_node(super::hole::type_in(value));
    match apply_cast(of_type_node(from), to, v) {
        Some(whole) => Ok((value, whole)),
        None => Ok((err, super::error::no_whole_number(rt, node)?)),
    }
}

fn run_checked(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a checked conversion, whose operand gives a value of its float type.
    unsafe {
        let (operand, union, _) = checked_parts(node);
        let v = rt.run(operand)?;
        let (member, taken) = checked_member(rt, node, v)?;
        let at = rt.scratch(meta::record_size_of(union) as usize);
        super::union::put(rt.types(), union, member, at, taken);
        Ok(at as i64)
    }
}

fn lower_checked(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_checked`].
    unsafe {
        let (operand, union, from) = checked_parts(node);
        let v = lw.lower(operand)?;
        let v = lw.widen(v, of_type_node(from));
        let at = lw.scratch_slot(meta::record_size_of(union) as usize);
        let node = lw.node_addr(node);
        lw.call_seed(compiled_checked as *const () as usize, &[node, v, at])?;
        Ok(at)
    }
}

/// # Safety
/// Called only by compiled code, with its context, a checked conversion `lower_checked` baked,
/// its operand's value in its `i64` container and room for a value of its union.
unsafe extern "C" fn compiled_checked(
    ctx: *mut crate::run::Context,
    node: DyadPtr,
    v: i64,
    at: i64,
) -> i64 {
    crate::run::interpret_step(ctx, |rt| {
        // SAFETY: the caller's contract.
        unsafe {
            let (member, taken) = checked_member(rt, node, v)?;
            let union = checked_parts(node).1;
            super::union::put(rt.types(), union, member, at as usize as *mut u8, taken);
        }
        Ok(0)
    })
}

/// # Safety
/// `operand` must be a reduced dyad, `from` and `to` type nodes, all from the store.
pub(crate) unsafe fn build_convert(
    store: &mut Store,
    types: &Core,
    operand: DyadPtr,
    from: DyadPtr,
    to: DyadPtr,
) -> DyadPtr {
    mint::node_of_any(store, types, types.convert, &[from, to], &[operand])
}

/// What `convert`'s mint over the field types `key`, the source and the target, gives back, the
/// target, and runs; stand-in for #462.
///
/// # Safety
/// `key` must hold type nodes from the store.
unsafe fn choose(_: &mut Store, types: &Core, key: &[DyadPtr]) -> Option<mint::Made> {
    let &[_, to] = key else {
        return None;
    };
    Some(mint::Made { output_type: to, op: types.ops.convert_ })
}

/// `(operand, from, to)`.
///
/// # Safety
/// `node` must be a conversion node [`build_convert`] built.
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr) {
    let operand = *(dyad::value(node) as *const DyadPtr);
    match mint::key_of(dyad::ty(node)) {
        Some(&[from, to]) => (operand, from, to),
        _ => unreachable!("a conversion is typed by its mint for its source and target"),
    }
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid conversion node, so its operand runs to a value of `from`.
    unsafe {
        let (operand, from, to) = parts(node);
        let v = rt.run(operand)?;
        converted(rt.types(), from, to, v)
    }
}

/// `v`, a value of `from`, as the value of `to` it converts to: a pointer keeps its address, a
/// number is cast.
///
/// # Safety
/// `from` and `to` must be type nodes a conversion is built between, `v` a value of `from`; a
/// rational value is the address of its sixteen bytes.
pub(crate) unsafe fn converted(
    types: &Core,
    from: DyadPtr,
    to: DyadPtr,
    v: i64,
) -> Result<i64, RunError> {
    if is_pointer_type(to) {
        return Ok(v);
    }
    let to = of_type_node(to);
    if from == types.rational {
        let p = v as usize as *const u8;
        if p.is_null() {
            return Err(RunError::Uninitialized);
        }
        let (num, den) = super::rational::read_at(p);
        if den == 0 {
            return Err(RunError::Uninitialized);
        }
        return Ok(super::rational::cast_pair(num, den, to));
    }
    apply_cast(of_type_node(from), to, v).ok_or(RunError::NoWholeNumber)
}

/// A rational value is interpreted only, as every operator over one is.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid conversion node.
    unsafe {
        let (operand, from, to) = parts(node);
        if from == lw.types().rational {
            return Err(CompileError::NotLowerable(node));
        }
        let v = lw.lower(operand)?;
        if is_pointer_type(to) {
            return Ok(v);
        }
        lw.emit_cast(of_type_node(from), of_type_node(to), v)
    }
}
