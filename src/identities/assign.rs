// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `=`: assignment. Constructed at discovery, it reads the place to its left
//! and drives its right side to the boundary; its builder bakes the target's
//! width into the store leaf in the op slot. `=` returns nothing.
//! DESIGN ›The scope's constructor is the driver‹

use crate::Core;
use cranelift_codegen::ir::Value;

use super::numtype::{is_pointer_type, of_type_node, NumType};
use super::read::{read_kind, Dispatch, Read};
use super::{commit_if_literal, meta, operands, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError, FN_BCODE, FN_BODY, FN_OUTER, FN_OUTPUT};
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
            let binding = c.dyad;
            tape.remove(-1);
            binding
        }
        _ => match p.construct_left(tape)? {
            Some(target) => target,
            None => return Err(ParseError::MissingOperand),
        },
    };
    p.check_path_write(target)?;
    // SAFETY: `target` is the reduced dyad of the cell to the left.
    let slot = unsafe { p.slot_of(target) };
    if slot.is_some() && !p.filling_definition() {
        return Err(ParseError::SlotOutsideDefinition);
    }
    if slot == Some(crate::parse::SlotKind::Fields) {
        let node = p.fields_block_fill(target)?;
        tape.place(node);
        return Ok(crate::parse::Constructed::Placed);
    }
    // Anything but a bracket right of a body slot is `slot_fill`'s checked error.
    if matches!(
        slot,
        Some(
            crate::parse::SlotKind::Parse
                | crate::parse::SlotKind::Run
                | crate::parse::SlotKind::Drop
        )
    ) && p.at_open()
    {
        let node = p.slot_body_fill(slot.expect("matched above"), target)?;
        tape.place(node);
        return Ok(crate::parse::Constructed::Placed);
    }
    let value = p.parse_expression()?;
    let node = if let Some(kind) = slot {
        // SAFETY: `value` is the reduced dyad of the right side.
        unsafe { p.slot_fill(kind, target, value) }?
    } else {
        let types = p.types();
        // An owner takes only what hands ownership over: a borrow stored there would be
        // torn down by two owners.
        // SAFETY: `target` and `value` are reduced dyads from the store.
        if unsafe {
            ((*target).ty == types.binding_ && p.owns_node(target) || p.is_owning_read(target))
                && !super::drop_model::moves_out(types, value)
        } {
            return Err(ParseError::NonOwningIntoOwning);
        }
        let node = build(p.store(), types, id, target, value)?;
        p.note_write(node);
        node
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
    // A name is written only if its binding carries `mut` and no `immut`.
    // SAFETY: `lhs` is a reduced dyad from the store; one of the `binding` type is a binding.
    unsafe {
        if (*lhs).ty == types.binding_ {
            if crate::binding::Binding::has_gate(lhs, types.immut_) {
                return Err(ParseError::Immutable(Box::new(crate::binding::Binding::spelling(
                    lhs,
                ))));
            }
            if !crate::binding::Binding::has_gate(lhs, types.mut_) {
                return Err(ParseError::NotMutable(Box::new(crate::binding::Binding::spelling(
                    lhs,
                ))));
            }
        }
    }
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let (lhs_d, rhs_d) = unsafe { (types.through(lhs), types.through(rhs)) };
    // SAFETY: `through` hands back its argument or the dyad a binding names.
    let (lhs_ty, rhs_ty) = unsafe { ((*lhs_d).ty, (*rhs_d).ty) };
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    if unsafe { super::tape::is_cell_read(types, lhs_d) } {
        // SAFETY: `lhs_d` is a tape cell read, `rhs` a reduced dyad.
        return Ok(unsafe { super::tape::build_write(store, types, lhs_d, rhs) });
    }
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    if unsafe { super::tape::is_line_read(types, lhs_d) } {
        // SAFETY: `lhs_d` is a line read of a tape cell, `rhs` a reduced dyad.
        return Ok(unsafe { super::tape::build_line_write(store, types, lhs_d, rhs) });
    }
    if lhs_ty == types.hashmap.get {
        // SAFETY: `lhs_d` is a get node, `rhs` a reduced dyad.
        return unsafe { super::hashmap::build_put(store, types, lhs_d, rhs) };
    }
    if lhs_ty == types.tape.is_constructed {
        // SAFETY: `rhs` is a reduced dyad from the store.
        if !unsafe { crate::parse::is_bool_result(types, rhs) } {
            return Err(ParseError::FlagTakesBool);
        }
        // SAFETY: `lhs_d` is a flag slot node, `rhs` a reduced bool dyad.
        return Ok(unsafe { super::tape::build_flag_write(store, types, lhs_d, rhs) });
    }
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    if unsafe { super::this::is_field_read(types, lhs_d) } {
        // SAFETY: `lhs_d` is a `this` field read, `rhs` a reduced dyad.
        return unsafe { super::this::build_write(store, types, lhs_d, rhs) };
    }
    if lhs_ty == types.deref_ {
        // SAFETY: `lhs_d` is a deref node, `rhs` a reduced dyad.
        return unsafe { super::pointer::build_storeptr(store, types, lhs_d, rhs) };
    }
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    if let Read::Executable(Dispatch::Call(f)) = unsafe { read_kind(types, lhs_d) } {
        // SAFETY: `lhs_d` is a call of `f`, `rhs` a reduced dyad.
        if let Some(node) = unsafe { build_call_write(store, types, lhs_d, f, rhs) }? {
            return Ok(node);
        }
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
                // SAFETY: as above.
                Read::Executable(_) => {
                    // SAFETY: as above.
                    unsafe {
                        super::hashmap::box_of(types, rhs) == Some(t)
                            || (t == types.type_ && super::yields_type(types, rhs))
                    }
                }
                _ => false,
            };
            if !ok {
                return Err(ParseError::BadDeclaredType);
            }
            let value = store.alloc_operands(&[lhs, rhs, types.ops.store_leaf(NumType::I64)]);
            return Ok(store.alloc_raw(op, value));
        }
        // SAFETY: as above.
        Read::Container(t) if unsafe { meta::is_node_valued(t, types.fn_type) } => {
            // SAFETY: as above.
            if unsafe { super::node_type_of(types, rhs) } != Some(t) {
                return Err(ParseError::TypeMismatch);
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

/// `f(…) = v` where `f`'s body ends in `p@`: a copy of `f` ending in `p` instead finds
/// the address when the write runs, and `v` is stored through it; `None` for any other
/// call. A `return` could hand back a value where the copy owes an address, so a body
/// holding one is no place. DESIGN ›A call that ends in a dereference is a place‹
///
/// # Safety
/// `call` must be a call node of `f`, `rhs` a reduced dyad, both from the store.
unsafe fn build_call_write(
    store: &mut Store,
    types: &Core,
    call: DyadPtr,
    f: DyadPtr,
    rhs: DyadPtr,
) -> Result<Option<DyadPtr>, ParseError> {
    let fields = (*f).value as *const DyadPtr;
    if (*f).ty != types.fn_type || fields.is_null() {
        return Ok(None);
    }
    let body = *fields.add(FN_BODY);
    if body.is_null() || crate::parse::contains_return(types, body) {
        return Ok(None);
    }
    let Some((tail, body)) = address_tail(store, types, body) else {
        return Ok(None);
    };
    let (_, pointee, offset) = super::pointer::deref_parts(tail);
    let mut record: Vec<DyadPtr> = (0..=FN_OUTER).map(|k| *fields.add(k)).collect();
    record[FN_OUTPUT] = super::pointer::make_pointer_type(store, types.type_, pointee);
    record[FN_BODY] = body;
    record[FN_BCODE] = std::ptr::null_mut();
    let record = store.alloc_operands(&record);
    let finder = store.alloc_raw((*f).ty, record);
    let found = store.alloc_raw(finder, (*call).value);
    let place = super::pointer::build_deref(store, types, found, pointee, offset as usize);
    super::pointer::build_storeptr(store, types, place, rhs).map(Some)
}

/// The body's trailing dereference and a copy of the body yielding its address instead;
/// `None` when the body does not end in one. A one-line body is that line itself.
///
/// # Safety
/// `body` must be a function body from the store.
unsafe fn address_tail(
    store: &mut Store,
    types: &Core,
    body: DyadPtr,
) -> Option<(DyadPtr, DyadPtr)> {
    let body = types.through(body);
    if (*body).ty == types.deref_ {
        return Some((body, super::pointer::deref_parts(body).0));
    }
    if (*body).ty != types.scope {
        return None;
    }
    let lines = super::scope::exprs_of(body)?;
    // A teardown runs as the body leaves, so the address would outlive what it frees.
    if lines.iter().any(|&e| (*e).ty == types.defer_) {
        return None;
    }
    let i = lines.iter().rposition(|&e| !super::numtype::is_comment_type((*e).ty))?;
    let tail = types.through(lines[i]);
    if (*tail).ty != types.deref_ {
        return None;
    }
    let mut lines = lines.to_vec();
    lines[i] = super::pointer::deref_parts(tail).0;
    Some((tail, super::scope::with_exprs(store, types.array_, body, &lines)))
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
