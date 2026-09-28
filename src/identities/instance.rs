// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Record instances: construction (`point(3, 4)`, the type applied to its field values)
//! and field resolution (`p.x`). An instance is bytes laid out from the field declarations
//! in order, made into the name that takes it or into scratch; a field is a place at its
//! byte offset inside the record's storage, so every scalar read, write and lowering path
//! serves it unchanged.

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::{self, NumType};
use super::{commit_if_literal, meta, numtype_of, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::ParseError;
use crate::run::{RunError, Runtime};
use crate::store::Store;

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
        &["target", "type", "op"],
    );
    let construct = cx.store.alloc_raw(cx.type_, record);
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
    let field = cx.store.alloc_raw(cx.type_, record);

    // Escaped, because `.` is a regex metacharacter.
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::TIGHT);
    let dot = cx.store.alloc_raw(cx.type_, record);
    cx.declare(r"\.", dot);
    // `.` reads its member's spelling off the cell to its right, and a `[i]` or `()` cell after
    // that where the read takes one.
    cx.metas.insert(dot, |p, _id, tape| p.construct_field_access(tape));

    // `[` is `(` in square brackets: it parses its interior as any bracket's into a
    // `square_brackets` cell the reads after `.` consume, or, after a tape, into the element read.
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::OPEN);
    let open_sq = cx.store.alloc_raw(cx.type_, record);
    cx.declare(r"\[", open_sq);
    cx.metas.insert(open_sq, |p, _id, tape| p.construct_index(tape));
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
    let close_sq = cx.store.alloc_raw(cx.type_, record);
    cx.declare(r"\]", close_sq);
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["dyads", "op"],
    );
    let square_brackets = cx.store.alloc_raw(cx.type_, record);
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
    let value = store.alloc_operands(&[record, binding, std::ptr::null_mut()]);
    store.alloc_raw(types.field_, value)
}

/// The record's storage and the field's binding a `field` place was built over.
///
/// # Safety
/// `node` must be a `field` node from `build_field`.
pub(crate) unsafe fn field_parts(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p, *p.add(1))
}

/// `(type node, width tag, byte offset)` per field, and the total size.
pub(crate) type FieldLayout = (Vec<(DyadPtr, NumType, usize)>, usize);

/// Each field with its numeric type and byte offset, in declaration order, plus the
/// total size. Fields must be numeric or pointer-typed; there is no nested layout.
///
/// # Safety
/// `record_logos` must be a record type node from the store.
pub(crate) unsafe fn layout(record_logos: DyadPtr) -> Result<FieldLayout, ParseError> {
    // A field's type must be a type node: that excludes a nested record definition and a
    // value node standing in type position, whose bytes would be misread as a width tag.
    let type_root = dyad::ty(dyad::ty(record_logos));
    let mut fields = Vec::new();
    let mut offset = 0usize;
    for &field in super::array::items(meta::record_fields_of(record_logos)) {
        let fty = dyad::ty(field);
        if fty.is_null() || dyad::ty(fty) != type_root || !numtype::is_scalar_type(fty) {
            return Err(ParseError::UnsupportedOperands);
        }
        let nt = numtype::of_type_node(fty);
        fields.push((field, nt, offset));
        offset += nt.bytes();
    }
    debug_assert_eq!(
        offset as u64,
        meta::record_size_of(record_logos),
        "the walked layout matches the size stored at definition"
    );
    Ok((fields, offset))
}

/// `[target, type, op, args…, null]`: the value is made into the name `target` when one
/// takes it (`:=` writes its binding there), else into scratch; the node yields its address.
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
    let (fields, _) = layout(record_logos)?;
    if args.len() != fields.len() {
        return Err(ParseError::CtorArity);
    }
    let mut ops = Vec::with_capacity(args.len() + 4);
    ops.push(std::ptr::null_mut());
    ops.push(record_logos);
    ops.push(types.ops.construct_);
    for (&arg, &(field, nt, _)) in args.iter().zip(&fields) {
        let fty = dyad::ty(field);
        let field_read = super::read::place_layout(types, fty);
        let field_ptr = matches!(field_read, Some((super::read::Read::Pointer(_), _)));
        let arg = match numtype_of(types, arg) {
            Operand::Literal => {
                if field_ptr {
                    // A literal into a pointer field would be a wild address.
                    return Err(ParseError::TypeMismatch);
                }
                commit_if_literal(store, types, arg, &Operand::Literal, fty, nt)?
            }
            Operand::Pointer(pointee) => {
                // Pointees compare as types, not nodes.
                if !matches!(field_read, Some((super::read::Read::Pointer(fp), _)) if super::pointee_types_match(fp, pointee))
                {
                    return Err(ParseError::TypeMismatch);
                }
                arg
            }
            Operand::Concrete(a_nt) if !field_ptr && a_nt == nt => arg,
            Operand::Concrete(_) => return Err(ParseError::TypeMismatch),
            Operand::NonNumeric => return Err(ParseError::UnsupportedOperands),
        };
        ops.push(arg);
    }
    ops.push(std::ptr::null_mut());
    let value = store.alloc_operands(&ops);
    Ok(store.alloc_raw(construct, value))
}

/// The arguments run first, then the bytes are taken and filled, so an argument's own
/// scratch never sits inside them; yields the instance's address.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a construct node from `build_ctor`.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let ty = *ops.add(1);
        let (fields, _) = layout(ty).map_err(|_| RunError::NoLayout(ty))?;
        // The arguments follow the three fixed head slots (target, type, op).
        let mut bits = Vec::with_capacity(fields.len());
        for i in 0..fields.len() {
            bits.push(rt.run(*ops.add(i + 3))?);
        }
        let blob = super::by_copy::dest_of(rt, node)?;
        for (&(field, _, offset), &b) in fields.iter().zip(&bits) {
            numtype::write_scalar(dyad::ty(field), blob.add(offset), b);
        }
        Ok(blob as i64)
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a construct node from `build_ctor`.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let ty = *ops.add(1);
        let (fields, _) = layout(ty).map_err(|_| CompileError::NoLayout(ty))?;
        let base = super::by_copy::lower_dest_of(lw, node)?;
        // The arguments follow the three fixed head slots (target, type, op).
        for (i, &(_, nt, offset)) in fields.iter().enumerate() {
            let v = lw.lower(*ops.add(i + 3))?;
            lw.store_at(nt.cranelift_type(), base, offset as i64, v);
        }
        Ok(base)
    }
}
