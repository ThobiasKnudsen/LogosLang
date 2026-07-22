// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Struct instances: construction (`point(3, 4)`, the logos applied to its field
//! values — the same constructor doctrine casts use) and field resolution
//! (`p.x`).
//!
//! An instance is `{logos: point, value -> bytes}` — parse-allocated storage laid
//! out from the field declarations in order, exactly a numeric variable grown
//! wide. Construction is the runtime half: a `construct` node
//! `{logos: construct, value: [instance, arg0 … argN, null]}` that evaluates each
//! argument and writes it to its field's offset, yielding unit (a statement).
//! Field access is the parse-time half, per DESIGN ›Resolution is one rule‹: the
//! declaration found decides — a field resolves to a *place*, the byte offset
//! inside the instance's hyle area — so `p.x` becomes an ordinary numeric node
//! `{logos: i32, value -> blob + offset}` and every existing read, write, and
//! lowering path just works. Fields are numeric-only in v1 (nested records
//! arrive with the richer layout) and writable by default, like today's
//! variables; the immutable-by-default flip arrives with `mut` for both at once.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::{self, NumType};
use super::{commit_if_literal, meta, numtype_of, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::synolon::SynolonPtr;
use crate::id_context::IdContext;
use crate::parse::{CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register the instance machinery: the `construct` identity (no spelling; the
/// parser builds these from a record-typed callee) with its native leaf and
/// lowering, and the `.` field-access token (parse-only; access nodes are plain
/// data). Returns `(identity, leaf, . token)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (SynolonPtr, SynolonPtr, SynolonPtr) {
    let record = meta::operand_record(
        cx,
        meta::LIST_TAG,
        f64::NAN,
        crate::parse::Assoc::Left,
        &["instance", "op"],
    );
    let construct = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(construct, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);

    // Escaped, because `.` is a regex metacharacter (as `\(` and `\)` are).
    let record = meta::record(cx.store, meta::TOKEN_TAG, f64::INFINITY);
    let dot = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(r"\.", IdContext::new(dot, cx.root_scope));
    cx.metas.insert(dot, |p, _id, tape| {
        // The model's `tape[-1]`: the completed operand left of the cursor.
        let Some(left) = p.left_operand(tape)? else {
            return Err(crate::parse::ParseError::MissingOperand);
        };
        // SAFETY: `left` is a reduced synolon off the tape.
        let node = unsafe { p.parse_field_access(left) }?;
        tape.remove(-1); // the consumed left
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });

    (construct, leaf, dot)
}

/// The layout a record logos stores from its field declarations (DESIGN ›a logos
/// whose constructor derives the layout automatically‹, issue #47): each field
/// with its numeric logos and byte offset, in declaration order, plus the total
/// size — the offsets walked from the stored `fields` array, the size matching
/// the stored `size_bytes`. Fields must be numeric or pointer-typed (8 bytes)
/// in v1 ([`ParseError::UnsupportedOperands`] otherwise — there is no nested
/// layout yet).
///
/// # Safety
/// `record_logos` must be a record logos node from the store (its value a
/// [`meta::RECORD_TAG`] record).
pub(crate) unsafe fn layout(
    record_logos: SynolonPtr,
) -> Result<(Vec<(SynolonPtr, NumType, usize)>, usize), ParseError> {
    // A field's logos must be a *logos node* (its own logos is `logos`, reachable as
    // the record logos's logos's logos — the fixed point): that excludes a nested
    // record definition and a value node standing in logos position, whose value
    // bytes would otherwise be misread as a width tag.
    let type_root = (*(*record_logos).logos).logos;
    let mut fields = Vec::new();
    let mut offset = 0usize;
    for &field in super::array::items(meta::record_fields_of(record_logos)) {
        let fty = (*field).logos;
        if fty.is_null() || (*fty).logos != type_root || !numtype::is_scalar_type(fty) {
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

/// Build a construction `point(args)`: derive the layout, check the argument
/// count ([`ParseError::CtorArity`]) and each argument's logos against its field
/// (a literal commits to the field's logos; a concrete mismatch is
/// [`ParseError::TypeMismatch`]), and return the construct node over the
/// pre-minted `instance` place. The caller mints `instance` (via
/// [`layout`] for its size), frame-relative inside a function so each call fills
/// its own copy, or an absolute blob at top level. The instance rides at operand
/// 0; the construct is a re-run initializer.
///
/// # Safety
/// `record_logos` must be a record logos node, `instance` a place of that logos
/// sized to [`layout`], and `args` reduced synolons, all from the store.
pub(crate) unsafe fn build_ctor(
    store: &mut Store,
    types: &CoreTypes,
    construct: SynolonPtr,
    record_logos: SynolonPtr,
    instance: SynolonPtr,
    args: &[SynolonPtr],
) -> Result<SynolonPtr, ParseError> {
    let (fields, _) = layout(record_logos)?;
    if args.len() != fields.len() {
        return Err(ParseError::CtorArity);
    }
    let mut ops = Vec::with_capacity(args.len() + 3);
    ops.push(instance);
    ops.push(types.ops.construct_);
    for (&arg, &(field, nt, _)) in args.iter().zip(&fields) {
        let fty = (*field).logos;
        let field_ptr = numtype::is_pointer_type(fty);
        let arg = match numtype_of(types, arg) {
            Operand::Literal => {
                if field_ptr {
                    // A literal into a pointer field would be a wild address.
                    return Err(ParseError::TypeMismatch);
                }
                commit_if_literal(store, arg, &Operand::Literal, fty, nt)?
            }
            Operand::Pointer(pointee) => {
                if !field_ptr || numtype::pointee_of(fty) != pointee {
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

/// Run: evaluate each argument and write it to its field's offset in the
/// instance's storage; construction is a statement and yields unit.
fn run(rt: &mut Runtime, node: SynolonPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a construct node as [`build_ctor`] lays it out.
    unsafe {
        let ops = (*node).hyle as *const SynolonPtr;
        let instance = *ops;
        let (fields, _) = layout((*instance).logos).map_err(|_| RunError::BadValue)?;
        // The arguments follow the two fixed head slots (instance, op).
        for (i, &(field, _, offset)) in fields.iter().enumerate() {
            let bits = rt.run(*ops.add(i + 2))?;
            let blob = rt.place_addr(instance).ok_or(RunError::BadValue)?;
            numtype::write_scalar((*field).logos, blob.add(offset), bits);
        }
        Ok(0)
    }
}

/// Lower: each argument stores to its field's baked address; yields unit.
fn lower(lw: &mut Lowerer, node: SynolonPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a construct node as [`build_ctor`] lays it out.
    unsafe {
        let ops = (*node).hyle as *const SynolonPtr;
        let instance = *ops;
        let (fields, _) = layout((*instance).logos).map_err(|_| CompileError::BadValue)?;
        // The instance's base address, resolved once (baked absolute, or a frame
        // `stack_addr`); each field stores at its byte offset from it.
        let base = lw.place_addr(instance);
        // The arguments follow the two fixed head slots (instance, op).
        for (i, &(_, nt, offset)) in fields.iter().enumerate() {
            let v = lw.lower(*ops.add(i + 2))?;
            lw.store_at(nt.cranelift_type(), base, offset as i64, v);
        }
        Ok(lw.const_i32(0))
    }
}
