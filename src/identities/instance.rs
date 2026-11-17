// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Record instances: construction (`point(3, 4)`, the type applied to its field values)
//! and field resolution (`p.x`). An instance is bytes laid out from the field declarations
//! in order, made into the name that takes it or into scratch; a field is a place at its
//! byte offset inside the record's storage, so every scalar read, write and lowering path
//! serves it unchanged.

use crate::binding::Binding;
use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::read::{carry_of, Carry};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::ParseError;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Where a construct node's arguments begin.
const ARGS: usize = 4;

/// The handles: `construct` and its leaf, the `field` place, `.`, `square_brackets`, `[`, `]`.
pub(super) struct InstanceIds {
    pub construct: DyadPtr,
    pub construct_leaf: DyadPtr,
    pub field: DyadPtr,
    pub dot: DyadPtr,
    pub square_brackets: DyadPtr,
    pub open_sq: DyadPtr,
    pub close_sq: DyadPtr,
}

/// `construct` and `field` have no spelling: the parser builds the one from a record-typed
/// callee and the other from `p.x` over a record's storage.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> InstanceIds {
    let record = meta::operand_record(
        cx,
        meta::LIST_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["target", "type", "op", "output_type"],
    );
    let construct = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(construct, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);

    // A field of a record's storage: read through two bindings, the record's and the field's,
    // its address the record's plus the field's offset; no leaf, since it is storage, not a step.
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["record", "field", "op"],
    );
    let field = cx.store.alloc_head(cx.type_, record);

    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::TIGHT);
    let dot = cx.store.alloc_head(cx.type_, record);
    cx.declare(".", dot);
    // `.` reads its member's spelling off the cell to its right, and a `[i]` or `()` cell after
    // that where the read takes one.
    cx.metas.insert(dot, |p, _id, tape| p.construct_field_access(tape));

    // `[` is `(` in square brackets: it parses its interior as any bracket's into a
    // `square_brackets` cell the reads after `.` consume, or, after a tape, into the element read.
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::OPEN);
    let open_sq = cx.store.alloc_head(cx.type_, record);
    cx.declare("[", open_sq);
    cx.metas.insert(open_sq, |p, _id, tape| p.construct_index(tape));
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
    let close_sq = cx.store.alloc_head(cx.type_, record);
    cx.declare("]", close_sq);
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["dyads", "op", "output_type"],
    );
    let square_brackets = cx.store.alloc_head(cx.type_, record);
    cx.declare("square_brackets", square_brackets);

    InstanceIds { construct, construct_leaf: leaf, field, dot, square_brackets, open_sq, close_sq }
}

/// `p.x`: the place of field `x` inside the record's storage `record`, `binding` the field's.
pub(crate) fn build_field(
    store: &mut Store,
    types: &Core,
    record: DyadPtr,
    binding: DyadPtr,
) -> DyadPtr {
    store.alloc_words(types.field_, &[record, binding, std::ptr::null_mut()])
}

/// The record's storage and the field's binding a `field` place was built over.
///
/// # Safety
/// `node` must be a `field` node from `build_field`.
pub(crate) unsafe fn field_parts(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p, *p.add(1))
}

/// Each field of a value of `ty` with its offset in the value, the one its binding carries.
///
/// # Safety
/// `ty` must be a record type node from the store.
pub(crate) unsafe fn field_offsets<'a>(ty: DyadPtr) -> impl Iterator<Item = (DyadPtr, usize)> + 'a {
    let fields = super::array::items(meta::record_fields_of(ty));
    let bindings = super::array::items(meta::record_field_bindings_of(ty));
    fields.iter().zip(bindings).map(|(&field, &binding)| {
        // SAFETY: the record keeps a binding per field, minted when its field was declared.
        (field, unsafe { Binding::read(binding) }.offset)
    })
}

/// Where each field of a value of `ty` lies and how a whole value of its type moves into it;
/// `None` where a field's type moves none, as a `?` field's.
///
/// # Safety
/// `ty` must be a record type node from the store.
unsafe fn places(types: &Core, ty: DyadPtr) -> Option<Vec<(usize, Carry)>> {
    field_offsets(ty)
        .map(|(field, offset)| Some((offset, carry_of(types, super::hole::type_in(field))?)))
        .collect()
}

/// `[target, type, op, output_type, args…, null]`: the value is made into the name `target`
/// when one takes it (`:=` writes its binding there), else into scratch; the node yields its
/// address.
///
/// # Safety
/// `record_logos` must be a record type node and `args` reduced dyads, all from the store.
pub(crate) unsafe fn build_ctor(
    store: &mut Store,
    types: &Core,
    construct: DyadPtr,
    record_logos: DyadPtr,
    args: &[DyadPtr],
) -> Result<DyadPtr, ParseError> {
    let places = places(types, record_logos).ok_or(ParseError::UnsupportedOperands)?;
    if args.len() != places.len() {
        return Err(ParseError::CtorArity);
    }
    let mut ops = Vec::with_capacity(ARGS + args.len() + 1);
    ops.push(std::ptr::null_mut());
    ops.push(record_logos);
    ops.push(types.ops.construct_);
    ops.push(record_logos);
    for (&arg, &field) in args.iter().zip(super::array::items(meta::record_fields_of(record_logos)))
    {
        ops.push(super::land(store, types, arg, super::hole::type_in(field))?);
    }
    ops.push(std::ptr::null_mut());
    Ok(store.alloc_words(construct, &ops))
}

/// The arguments run first, then the bytes are taken and filled, so an argument's own
/// scratch never sits inside them; yields the instance's address.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a construct node from `build_ctor`.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let ty = *ops.add(1);
        let places = places(rt.types(), ty).ok_or(RunError::NoLayout(ty))?;
        let mut taken = Vec::with_capacity(places.len());
        for (i, &(_, carry)) in places.iter().enumerate() {
            taken.push(super::assign::take(rt, carry, *ops.add(ARGS + i))?);
        }
        let blob = super::by_copy::dest_of(rt, node)?;
        for (&(offset, carry), &value) in places.iter().zip(&taken) {
            super::assign::put(carry, blob.add(offset), value);
        }
        Ok(blob as i64)
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a construct node from `build_ctor`.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let ty = *ops.add(1);
        let places = places(lw.types(), ty).ok_or(CompileError::NoLayout(ty))?;
        let base = super::by_copy::lower_dest_of(lw, node)?;
        for (i, &(offset, carry)) in places.iter().enumerate() {
            let value = super::assign::lower_take(lw, carry, *ops.add(ARGS + i))?;
            super::assign::lower_put(lw, carry, base, offset as i64, value);
        }
        Ok(base)
    }
}
