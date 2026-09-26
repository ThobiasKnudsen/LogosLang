// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A plain record crosses a call by copy: the caller copies each argument into the
//! callee's argument block at the argument's own width, and a record result comes back
//! through a slot the caller provides. DESIGN ›Operands travel on the stack‹. stand-in for #127

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::read::{place_layout, read_kind, Read};
use super::{array, meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, FN_INPUT, FN_OUTPUT};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct ByCopyIds {
    /// `[slot, call, op]`: a call whose record result is copied into `slot`.
    pub result: DyadPtr,
    pub result_leaf: DyadPtr,
    /// `[expr, op]`: the address of a record value's bytes, what a record function hands back.
    pub out: DyadPtr,
    pub out_leaf: DyadPtr,
}

/// Neither has a spelling: the parser builds them around calls and tails of record type.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> ByCopyIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["slot", "call", "op"],
    );
    let result = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(result, lower_result);
    let result_leaf = callable::mint_native(cx.store, cs.callable, run_result, cs.seed_native);
    let record =
        meta::operand_record(cx, meta::TUPLE_TAG, meta::prec::INERT, Assoc::Left, &["expr", "op"]);
    let out = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(out, lower_out);
    let out_leaf = callable::mint_native(cx.store, cs.callable, run_out, cs.seed_native);
    ByCopyIds { result, result_leaf, out, out_leaf }
}

pub(crate) fn build_result(
    store: &mut Store,
    types: &Core,
    slot: DyadPtr,
    call: DyadPtr,
) -> DyadPtr {
    let value = store.alloc_operands(&[slot, call, types.by_copy.result_leaf]);
    store.alloc_raw(types.by_copy.result, value)
}

pub(crate) fn build_out(store: &mut Store, types: &Core, expr: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[expr, types.by_copy.out_leaf]);
    store.alloc_raw(types.by_copy.out, value)
}

/// The width a value of `t` is copied at: `Some` for a plain record a `type (…)` body
/// wrote, whose value is its bytes; `None` for everything that travels in one 8-byte
/// container, a native record (a tape handle, a map) among them.
///
/// # Safety
/// `t` must be null or a valid dyad from the store.
pub(crate) unsafe fn record_width(types: &Core, t: DyadPtr) -> Option<usize> {
    let t = super::type_identity_of(types, t)?;
    match place_layout(types, t)? {
        (Read::Aggregate, width) if !meta::record_body_of(t).is_null() => Some(width),
        _ => None,
    }
}

/// The plain record type `node` yields, if it yields one.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn record_type_of(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    let node = types.through(node);
    let ty = (*node).ty;
    let t = if ty == types.construct_ || ty == types.by_copy.result {
        (**((*node).value as *const DyadPtr)).ty
    } else if read_kind(types, node) == Read::Aggregate {
        ty
    } else {
        return None;
    };
    record_width(types, t).map(|_| t)
}

/// The width of `f`'s record result, whose slot address rides the block's first word.
///
/// # Safety
/// `f` must be a `fn` node from the store.
pub(crate) unsafe fn result_width(types: &Core, f: DyadPtr) -> Option<usize> {
    let fields = (*f).value as *const DyadPtr;
    if fields.is_null() {
        return None;
    }
    record_width(types, *fields.add(FN_OUTPUT))
}

/// One parameter's place in the argument block: its first 8-byte word, and the record
/// width it is copied at, `None` for a one-word container.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) param: DyadPtr,
    pub(crate) word: usize,
    pub(crate) width: Option<usize>,
}

/// `f`'s parameters in order, each at its word in the argument block.
///
/// # Safety
/// `f` must be a `fn` node from the store whose value is its field record.
pub(crate) unsafe fn slots<'a>(types: &'a Core, f: DyadPtr) -> impl Iterator<Item = Slot> + 'a {
    let fields = (*f).value as *const DyadPtr;
    let params = array::items(meta::record_fields_of(*fields.add(FN_INPUT)));
    let mut word = usize::from(result_width(types, f).is_some());
    params.iter().map(move |&param| {
        // SAFETY: each parameter is a dyad from the store.
        let width = unsafe { record_width(types, (*param).ty) };
        let slot = Slot { param, word, width };
        word += width.map_or(1, |w| w.div_ceil(8));
        slot
    })
}

/// The argument block's length in words.
///
/// # Safety
/// As [`slots`].
pub(crate) unsafe fn words(types: &Core, f: DyadPtr) -> usize {
    let first = usize::from(result_width(types, f).is_some());
    slots(types, f).last().map_or(first, |s| s.word + s.width.map_or(1, |w| w.div_ceil(8)))
}

/// Evaluate a record-valued `node` and yield the address of its bytes.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn record_addr(rt: &mut Runtime, node: DyadPtr) -> Result<*mut u8, RunError> {
    let node = rt.through(node);
    let ty = (*node).ty;
    let types = rt.types();
    let (result, construct, aggregate) =
        (types.by_copy.result, types.construct_, read_kind(types, node) == Read::Aggregate);
    if ty == result {
        return rt.run(node).map(|addr| addr as *mut u8);
    }
    let place = if ty == construct {
        rt.run(node)?;
        *((*node).value as *const DyadPtr)
    } else if aggregate {
        node
    } else {
        return Err(RunError::NoWholeRead);
    };
    let addr = rt.place_addr(place).ok_or(RunError::NoActivation)?;
    if addr.is_null() {
        return Err(RunError::Uninitialized);
    }
    Ok(addr)
}

/// [`record_addr`]'s compiled half.
///
/// # Safety
/// As [`record_addr`].
pub(crate) unsafe fn lower_record_addr(
    lw: &mut Lowerer,
    node: DyadPtr,
) -> Result<Value, CompileError> {
    let node = lw.through(node);
    let ty = (*node).ty;
    let types = lw.types();
    let (result, construct, aggregate) =
        (types.by_copy.result, types.construct_, read_kind(types, node) == Read::Aggregate);
    if ty == result {
        return lw.lower(node);
    }
    if ty == construct {
        lw.lower(node)?;
        return lw.place_addr(*((*node).value as *const DyadPtr));
    }
    if aggregate {
        return lw.place_addr(node);
    }
    Err(CompileError::NotLowerable(node))
}

fn run_result(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a result node from `build_result`; its call is a call of a `fn`.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let dest = rt.place_addr(*ops).ok_or(RunError::NoActivation)?;
        let call = *ops.add(1);
        rt.apply_into((*call).ty, call, Some(dest))
    }
}

fn lower_result(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_result`].
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let dest = lw.place_addr(*ops)?;
        let call = *ops.add(1);
        lw.lower_call_into((*call).ty, call, Some(dest))
    }
}

fn run_out(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an out node from `build_out`.
    unsafe { record_addr(rt, *((*node).value as *const DyadPtr)).map(|addr| addr as i64) }
}

fn lower_out(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_out`].
    unsafe { lower_record_addr(lw, *((*node).value as *const DyadPtr)) }
}
