// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `=`: assignment. Constructed at discovery, it reads the place to its left
//! and drives its right side to the boundary; its builder bakes the target's
//! width into the store leaf in the op slot. `=` returns nothing.
//! DESIGN ›The scope's constructor is the driver‹

use crate::Core;
use cranelift_codegen::ir::Value;

use super::numtype::{is_pointer_type, of_type_node, NumType};
use super::read::{read_kind, Read};
use super::{commit_if_literal, meta, operands, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError};
use crate::store::Store;

/// The run natives are the per-width store leaves ([`crate::identities::ops`]).
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::DECLARE,
        Assoc::Right,
        &["lhs", "rhs", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("=", id);
    cx.metas.insert(id, construct);
    cx.lower.insert(id, lower);
    id
}

fn construct(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, ParseError> {
    let types = p.types();
    // A lone `drop` constructed on its own would read an operand to its
    // right, so `=` takes the keyword's use as the slot's name unconstructed.
    let target = match tape.at(-1) {
        Some(c) if tape.cursor() == 1 && !c.constructed && c.identity(types) == types.drop_ => {
            let record = c.dyad;
            tape.remove(-1);
            record
        }
        _ => match p.construct_left(tape)? {
            Some(target) => target,
            None => return Err(ParseError::MissingOperand),
        },
    };
    // SAFETY: `target` is the reduced dyad of the cell to the left.
    let slot = unsafe { p.slot_of(target) };
    if slot.is_some() && !p.filling_definition() {
        return Err(ParseError::SlotOutsideDefinition);
    }
    if slot == Some(crate::parse::SlotKind::Instance) {
        let node = p.instance_block_fill()?;
        tape.place(node);
        return Ok(crate::parse::Constructed::Placed);
    }
    // A `fn …` right of `parse`/`run` is still read as an expression: stand-in for #133.
    if matches!(slot, Some(crate::parse::SlotKind::Parse | crate::parse::SlotKind::Run))
        && p.at_open()
    {
        let node = p.slot_body_fill(slot.expect("matched above"))?;
        tape.place(node);
        return Ok(crate::parse::Constructed::Placed);
    }
    let value = p.parse_expression()?;
    let node = if let Some(kind) = slot {
        // SAFETY: `value` is the reduced dyad of the right side.
        unsafe { p.slot_fill(kind, value) }?
    } else {
        let types = p.types();
        build(p.store(), types, id, target, value)?
    };
    tape.place(node);
    Ok(crate::parse::Constructed::Placed)
}

/// A literal right side commits to the target's type; a non-literal one must
/// already be it. The op slot gets the store leaf for the target's width.
/// Shared with the declaration's snapshot initializer for `:=`.
pub(super) fn build(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // A name is written only if its record carries `mut`.
    // SAFETY: `lhs` is a reduced dyad from the store; one of the record type is a record.
    unsafe {
        if (*lhs).ty == types.record_ && !crate::record::Record::has_gate(lhs, types.mut_) {
            return Err(ParseError::NotMutable(Box::new(crate::record::Record::spelling(lhs))));
        }
    }
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let (lhs_d, rhs_d) = unsafe { (types.through(lhs), types.through(rhs)) };
    // SAFETY: `through` hands back its argument or the dyad a record names.
    let (lhs_ty, rhs_ty) = unsafe { ((*lhs_d).ty, (*rhs_d).ty) };
    if lhs_ty == types.tape.slot {
        // SAFETY: `lhs_d` is a tape slot node, `rhs` a reduced dyad.
        return Ok(unsafe { super::tape::build_write(store, types, lhs_d, rhs) });
    }
    if lhs_ty == types.tape.is_constructed {
        // SAFETY: `rhs` is a reduced dyad from the store.
        if !unsafe { crate::parse::is_bool_result(types, rhs) } {
            return Err(ParseError::FlagTakesBool);
        }
        // SAFETY: `lhs_d` is a flag slot node, `rhs` a reduced bool dyad.
        return Ok(unsafe { super::tape::build_flag_write(store, types, lhs_d, rhs) });
    }
    if lhs_ty == types.this.slot {
        // SAFETY: `lhs_d` is a `this` field slot node, `rhs` a reduced dyad.
        return Ok(unsafe { super::this::build_write(store, types, lhs_d, rhs) });
    }
    if lhs_ty == types.deref_ {
        // SAFETY: `lhs_d` is a deref node, `rhs` a reduced dyad.
        return unsafe { super::pointer::build_storeptr(store, types, lhs_d, rhs) };
    }
    // A number into a node box would be followed as an address by every later reader.
    // SAFETY: `lhs_d`/`rhs` are reduced dyads from the store.
    let (target, marked) =
        unsafe { (read_kind(types, lhs_d), crate::dyad::is_place((*lhs_d).value)) };
    match target {
        Read::Container(t) if t == types.type_ || t == types.dyad_ => {
            // SAFETY: as above.
            let value_read = unsafe { read_kind(types, rhs) };
            let ok = match value_read {
                Read::Identity => true,
                Read::Container(c) => {
                    if t == types.type_ {
                        c == types.type_
                    } else {
                        !c.is_null()
                    }
                }
                Read::Address => t == types.dyad_,
                _ => false,
            };
            if !ok {
                return Err(ParseError::BadDeclaredType);
            }
            let value = store.alloc_operands(&[lhs, rhs, types.ops.store_leaf(NumType::I64)]);
            return Ok(store.alloc_raw(op, value));
        }
        Read::Container(t) if t == types.rational => {
            // SAFETY: `rhs` is a reduced dyad from the store.
            let value = unsafe { super::rational::rational_operand(store, types, rhs) }
                .ok_or(ParseError::TypeMismatch)?;
            let value = store.alloc_operands(&[lhs, value, types.ops.store_leaf(NumType::I64)]);
            return Ok(store.alloc_raw(op, value));
        }
        Read::Scalar(_) | Read::Pointer(_) if marked => {}
        Read::Literal => {
            // SAFETY: a `Literal` read is a rational node with its fraction blob.
            return Err(ParseError::AssignToLiteral(Box::new(unsafe {
                super::rational::spell(lhs_d)
            })));
        }
        _ => return Err(ParseError::BadAssignTarget),
    }
    // A literal into a pointer would become a wild address.
    // SAFETY: as above.
    let lhs_pointer = unsafe { is_pointer_type((*lhs_d).ty) };
    if lhs_pointer && rhs_ty == types.rational {
        return Err(ParseError::TypeMismatch);
    }
    // `=` returns nothing, so `a = b = c` assigns nothing.
    if rhs_ty == types.assign || rhs_ty == types.storeptr_ {
        return Err(ParseError::StatementAsValue);
    }
    // SAFETY: as above.
    let rhs = unsafe {
        if (*rhs_d).ty == types.rational && !crate::dyad::is_place((*rhs_d).value) {
            let nt = of_type_node((*lhs_d).ty);
            commit_if_literal(store, types, rhs, &Operand::Literal, (*lhs_d).ty, nt)?
        } else {
            super::check_store_type(types, (*lhs_d).ty, rhs)?;
            rhs
        }
    };
    // A non-owning value in an owning place ends in a double free at scope exit.
    // SAFETY: `rhs` is a reduced dyad from the store.
    let rhs_owning = unsafe { super::drop_model::is_owning_value(types, rhs) };
    if super::drop_model::is_owning_place(lhs_d) && !rhs_owning {
        return Err(ParseError::NonOwningIntoOwning);
    }
    // SAFETY: `lhs` is a typed variable checked assignable above.
    let nt = unsafe { of_type_node((*lhs_d).ty) };
    let value = store.alloc_operands(&[lhs, rhs, types.ops.store_leaf(nt)]);
    Ok(store.alloc_raw(op, value))
}

/// Guards a null storage address like the interpreter's `Uninitialized`; the
/// width comes from the node's own store leaf, the one decision read twice.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `=` application `[lhs, rhs, store leaf]`.
    unsafe {
        let (lhs, rhs) = operands(node);
        let leaf = *((*node).value as *const DyadPtr).add(2);
        let Some(nt) = lw.types().ops.store_nt_of(leaf) else {
            return Err(CompileError::Internal("a store node holds a store leaf"));
        };
        let lhs = lw.through(lhs);
        if (*lhs).value.is_null() {
            return Err(CompileError::Uninitialized);
        }
        let v = lw.lower(rhs)?;
        lw.write_place(lhs, nt.cranelift_type(), v)?;
        Ok(v)
    }
}
