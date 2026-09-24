// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Record instances: construction (`point(3, 4)`, the type applied to its field values)
//! and field resolution (`p.x`). An instance is parse-allocated storage laid out from
//! the field declarations in order; a field resolves to a place at its byte offset, so
//! every scalar read, write and lowering path serves it unchanged.

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::{self, NumType};
use super::{commit_if_literal, meta, numtype_of, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::ParseError;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// `construct` has no spelling: the parser builds it from a record-typed callee.
/// Returns the `construct` identity, its leaf, `.`, `square_brackets`, `[`, `]`.
pub(super) fn register(
    cx: &mut Cx,
    cs: &Callables,
) -> (DyadPtr, DyadPtr, DyadPtr, DyadPtr, DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::LIST_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["instance", "op"],
    );
    let construct = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(construct, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);

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
        &["key", "op"],
    );
    let square_brackets = cx.store.alloc_raw(cx.type_, record);
    cx.declare("square_brackets", square_brackets);

    (construct, leaf, dot, square_brackets, open_sq, close_sq)
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
    let type_root = (*(*record_logos).ty).ty;
    let mut fields = Vec::new();
    let mut offset = 0usize;
    for &field in super::array::items(meta::record_fields_of(record_logos)) {
        let fty = (*field).ty;
        if fty.is_null() || (*fty).ty != type_root || !numtype::is_scalar_type(fty) {
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

/// The caller mints `instance` sized by `layout`, frame-relative inside a function or
/// absolute at top level; it rides at operand 0 and the construct is a re-run initializer.
///
/// # Safety
/// `record_logos` must be a record type node, `instance` a place of that type sized to
/// `layout`, and `args` reduced dyads, all from the store.
pub(crate) unsafe fn build_ctor(
    store: &mut Store,
    types: &Core,
    construct: DyadPtr,
    record_logos: DyadPtr,
    instance: DyadPtr,
    args: &[DyadPtr],
) -> Result<DyadPtr, ParseError> {
    let (fields, _) = layout(record_logos)?;
    if args.len() != fields.len() {
        return Err(ParseError::CtorArity);
    }
    let mut ops = Vec::with_capacity(args.len() + 3);
    ops.push(instance);
    ops.push(types.ops.construct_);
    for (&arg, &(field, nt, _)) in args.iter().zip(&fields) {
        let fty = (*field).ty;
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

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a construct node from `build_ctor`.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let instance = *ops;
        let (fields, _) = layout((*instance).ty).map_err(|_| RunError::NoLayout((*instance).ty))?;
        // The arguments follow the two fixed head slots (instance, op).
        for (i, &(field, _, offset)) in fields.iter().enumerate() {
            let bits = rt.run(*ops.add(i + 2))?;
            let blob = rt.place_addr(instance).ok_or(RunError::NoActivation)?;
            numtype::write_scalar((*field).ty, blob.add(offset), bits);
        }
        Ok(0)
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a construct node from `build_ctor`.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let instance = *ops;
        let (fields, _) =
            layout((*instance).ty).map_err(|_| CompileError::NoLayout((*instance).ty))?;
        let base = lw.place_addr(instance)?;
        // The arguments follow the two fixed head slots (instance, op).
        for (i, &(_, nt, offset)) in fields.iter().enumerate() {
            let v = lw.lower(*ops.add(i + 2))?;
            lw.store_at(nt.cranelift_type(), base, offset as i64, v);
        }
        Ok(lw.const_i32(0))
    }
}
