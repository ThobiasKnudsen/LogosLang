// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A plain record crosses a call by copy: the caller copies each argument into the
//! callee's argument block at the argument's own width, and a record result comes back
//! through a slot the caller provides. A run-time rational travels the same way, its
//! sixteen bytes the record. DESIGN ›Operands travel on the stack‹. stand-in for #127

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::read::{place_layout, read_kind, Dispatch, Read};
use super::{array, meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, FN_INPUT, FN_OUTPUT};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct ByCopyIds {
    /// `[target, call, type, op]`: a call whose record result, of `type`, is copied into the
    /// name `target`, or into scratch when no name takes it (`target` null).
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
        &["target", "call", "type", "op", "output_type"],
    );
    let result = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(result, lower_result);
    let result_leaf = callable::mint_native(cx.store, cs.callable, run_result, cs.seed_native);
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["expr", "op", "output_type"],
    );
    let out = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(out, lower_out);
    let out_leaf = callable::mint_native(cx.store, cs.callable, run_out, cs.seed_native);
    ByCopyIds { result, result_leaf, out, out_leaf }
}

pub(crate) fn build_result(
    store: &mut Store,
    types: &Core,
    target: DyadPtr,
    call: DyadPtr,
    ty: DyadPtr,
) -> DyadPtr {
    store.alloc_words(types.by_copy.result, &[target, call, ty, types.by_copy.result_leaf, ty])
}

/// Where a `construct` or `result` node puts its value: the name's storage, or scratch
/// of the value's width taken now.
///
/// # Safety
/// `node` must be a `construct` or `result` node from the store.
pub(crate) unsafe fn dest_of(rt: &mut Runtime, node: DyadPtr) -> Result<*mut u8, RunError> {
    let target = *(dyad::value(node) as *const DyadPtr);
    if target.is_null() {
        let ty = super::read::output_type(rt.types(), node);
        let width = place_layout(rt.types(), ty).map_or(8, |(_, w)| w).max(1);
        return Ok(rt.scratch(width));
    }
    rt.place_addr(target).ok_or(RunError::NoActivation)
}

/// [`dest_of`]'s compiled half: a stack slot of this function for scratch.
///
/// # Safety
/// As [`dest_of`].
pub(crate) unsafe fn lower_dest_of(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    let target = *(dyad::value(node) as *const DyadPtr);
    if target.is_null() {
        let ty = super::read::output_type(lw.types(), node);
        let width = place_layout(lw.types(), ty).map_or(8, |(_, w)| w).max(1);
        return Ok(lw.scratch_slot(width));
    }
    lw.place_addr(target)
}

/// # Safety
/// `expr` must be a reduced dyad from the store.
pub(crate) unsafe fn build_out(store: &mut Store, types: &Core, expr: DyadPtr) -> DyadPtr {
    let output = super::read::output_type(types, expr);
    store.alloc_words(types.by_copy.out, &[expr, types.by_copy.out_leaf, output])
}

/// The width a value of `t` is copied at: `Some` for a plain record a `type (…)` body
/// wrote, whose value is its bytes, and for a rational; `None` for everything that
/// travels in one 8-byte container, a native record (a tape handle, a map) among them.
///
/// # Safety
/// `t` must be null or a valid dyad from the store.
pub(crate) unsafe fn record_width(types: &Core, t: DyadPtr) -> Option<usize> {
    let t = super::type_identity_of(types, t)?;
    match place_layout(types, t)? {
        (Read::Aggregate, width) if !meta::record_body_of(t).is_null() => Some(width),
        (Read::Rational, width) => Some(width),
        _ => None,
    }
}

/// The width of `f`'s record result, whose slot address rides the block's first word.
///
/// # Safety
/// `f` must be a `fn` node from the store.
pub(crate) unsafe fn result_width(types: &Core, f: DyadPtr) -> Option<usize> {
    record_width(types, *(dyad::value(f) as *const DyadPtr).add(FN_OUTPUT))
}

/// One parameter's place in the argument block: its first 8-byte word, the record width
/// it is copied at, `None` for a one-word container, and its offset in the call's frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) param: DyadPtr,
    pub(crate) word: usize,
    pub(crate) width: Option<usize>,
    pub(crate) offset: usize,
}

/// The bytes a parameter or local of `t` takes in a call's frame: its type's own layout,
/// or the eight-byte container every other value travels in; a bare parameter's too.
///
/// # Safety
/// `t` must be null or a valid dyad from the store.
pub(crate) unsafe fn param_width(types: &Core, t: DyadPtr) -> usize {
    if t.is_null() {
        8
    } else {
        place_layout(types, t).map_or(8, |(_, width)| width)
    }
}

/// `f`'s parameters in order, each at its word in the argument block and its offset in the
/// frame: the parameters come first, packed by `param_width`, as the parser laid them out.
///
/// # Safety
/// `f` must be a `fn` node from the store whose value is its field record.
pub(crate) unsafe fn slots<'a>(types: &'a Core, f: DyadPtr) -> impl Iterator<Item = Slot> + 'a {
    let fields = dyad::value(f) as *const DyadPtr;
    let params = array::items(meta::record_fields_of(*fields.add(FN_INPUT)));
    let mut word = usize::from(result_width(types, f).is_some());
    let mut offset = 0;
    params.iter().map(move |&param| {
        // SAFETY: each parameter is a dyad from the store.
        let (width, own) = unsafe {
            let ty = super::hole::type_in(param);
            (record_width(types, ty), param_width(types, ty))
        };
        let slot = Slot { param, word, width, offset };
        word += width.map_or(1, |w| w.div_ceil(8));
        offset += own;
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
    let ty = dyad::ty(node);
    let types = rt.types();
    let (result, out, construct) = (types.by_copy.result, types.by_copy.out, types.construct_);
    let kind = read_kind(types, node);
    // Each yields the address of the bytes it made.
    if ty == result || ty == out || ty == construct {
        return rt.run(node).map(|addr| addr as usize as *mut u8);
    }
    let place = match kind {
        Read::Aggregate | Read::Rational => node,
        // A literal's bytes are a rational value's.
        Read::Literal => return Ok(dyad::value(node)),
        _ => return Err(RunError::NoWholeRead),
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
    let ty = dyad::ty(node);
    let types = lw.types();
    let (result, out, construct) = (types.by_copy.result, types.by_copy.out, types.construct_);
    let kind = read_kind(types, node);
    if ty == result || ty == out || ty == construct {
        return lw.lower(node);
    }
    match kind {
        Read::Aggregate | Read::Rational => lw.place_addr(node),
        // A literal is source: its bytes' address is a constant.
        Read::Literal => Ok(lw.const_i64(dyad::value(node) as usize as i64)),
        _ => Err(CompileError::NotLowerable(node)),
    }
}

fn run_result(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a result node from `build_result`: a call of a `fn`, a rational
    // arithmetic node, or a literal, run into the name's storage or scratch.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let dest = dest_of(rt, node)?;
        let call = *ops.add(1);
        match read_kind(rt.types(), call) {
            Read::Executable(Dispatch::Call(f)) => rt.apply_into(f, call, Some(dest)),
            Read::Executable(Dispatch::Leaf(_)) => {
                super::rational::run_into(rt, call, dest)?;
                Ok(dest as i64)
            }
            Read::Literal => {
                std::ptr::copy_nonoverlapping(dyad::value(call), dest, 16);
                Ok(dest as i64)
            }
            _ => Err(RunError::NoWholeRead),
        }
    }
}

fn lower_result(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_result`]; a rational step is interpreted only.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let dest = lower_dest_of(lw, node)?;
        let call = *ops.add(1);
        match read_kind(lw.types(), call) {
            Read::Executable(Dispatch::Call(f)) => lw.lower_call_into(f, call, Some(dest)),
            _ => Err(CompileError::NotLowerable(node)),
        }
    }
}

fn run_out(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an out node from `build_out`.
    unsafe { record_addr(rt, *(dyad::value(node) as *const DyadPtr)).map(|addr| addr as i64) }
}

fn lower_out(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_out`].
    unsafe { lower_record_addr(lw, *(dyad::value(node) as *const DyadPtr)) }
}
