// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `=`: assignment. Constructed at discovery, it reads the place to its left
//! and drives its right side to the boundary; its builder bakes the target's
//! width into the store leaf in the op slot. `=` returns nothing.
//! DESIGN ›The scope's constructor is the driver‹

use crate::Core;
use cranelift_codegen::ir::Value;

use super::numtype::{is_pointer_type, of_type_node, NumType};
use super::read::{call_place, place_of, read_kind, Place, Read};
use super::{commit_if_literal, meta, operands, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
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
    let id = cx.store.alloc_head(cx.type_, record);
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
    // A lone `free` constructed on its own would read an operand to its
    // right, so `=` takes the keyword's use as the slot's name unconstructed.
    let target = match tape.at(-1) {
        Some(c) if tape.cursor() == 1 && !c.constructed && c.identity(types) == types.free_ => {
            let name = *c;
            tape.remove(-1);
            name
        }
        _ => match p.construct_left(tape)? {
            Some(target) => target,
            None => return Err(ParseError::MissingOperand),
        },
    };
    p.check_path_write(&target)?;
    let fill = target.target;
    let target = target.dyad;
    // SAFETY: `target` is the reduced dyad of the cell to the left.
    let slot = unsafe { p.slot_of(target) };
    if slot.is_some() && !p.filling_definition() {
        return Err(ParseError::SlotOutsideDefinition);
    }
    // Anything but a bracket right of a body slot is `slot_fill`'s checked error.
    if matches!(
        slot,
        Some(
            crate::parse::SlotKind::Parse
                | crate::parse::SlotKind::Run
                | crate::parse::SlotKind::Free
        )
    ) && p.at_open()
    {
        let node = p.slot_body_fill(slot.expect("matched above"), target)?;
        tape.place(node);
        return Ok(crate::parse::Constructed::Placed);
    }
    let value = p.parse_expression()?;
    // SAFETY: `target` is the reduced dyad of the cell to the left.
    if unsafe { dyad::ty(target) } == types.tape.cell_type {
        // SAFETY: `target` is a cell type read, `value` a reduced dyad.
        let node = unsafe { p.stamp(target, value) }?;
        tape.place(node);
        return Ok(crate::parse::Constructed::Placed);
    }
    // SAFETY: `target` and `value` are reduced dyads from the store.
    let value = unsafe { p.bracket_into(target, value) }?;
    let node = if let Some(kind) = slot {
        // SAFETY: `value` is the reduced dyad of the right side.
        unsafe { p.slot_fill(kind, target, value) }?
    } else {
        let types = p.types();
        // An owner takes only what hands ownership over: a borrow stored there would be
        // torn down by two owners.
        // SAFETY: `target` and `value` are reduced dyads from the store.
        if unsafe {
            (dyad::ty(target) == types.binding_ && p.owns_node(target) || p.is_owning_read(target))
                && !super::drop_model::moves_out(types, value)
        } {
            return Err(ParseError::NonOwningIntoOwning);
        }
        let node = build(p.store(), types, id, target, value)?;
        // SAFETY: `target` is the reduced dyad of the cell to the left; `node` the write just
        // built over it.
        unsafe {
            if let Some(free) = super::drop_model::displaced_free(p, target)? {
                displace(p.store(), types, node, free);
            }
            p.note_receiver_write(target);
        }
        p.note_write(node, fill);
        node
    };
    tape.place(node);
    Ok(crate::parse::Constructed::Placed)
}

/// `node` writes a place that holds its value: its right side becomes the node that builds it
/// and then frees the value it displaces, so the write comes last (DESIGN ›`=` sits beside
/// `:=`, and returns nothing‹). A name's write holds its right side second, a field's third.
///
/// # Safety
/// `node` must be an `=` or a field write just built; `free` a free node.
unsafe fn displace(store: &mut Store, types: &Core, node: DyadPtr, free: DyadPtr) {
    let at = if dyad::ty(node) == types.assign { 1 } else { 2 };
    let slot = (dyad::value(node) as *mut DyadPtr).add(at);
    *slot = super::drop_model::build_displace(store, types, *slot, free);
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
    // A binding's fields are read; `lex_rank` is the one a program sets after the declaration
    // (DESIGN ›`lex_rank` belongs to the binding, not the type‹).
    // SAFETY: `lhs` is a reduced dyad from the store.
    if let Some(field) = unsafe { types.binding_field_of(lhs) } {
        // SAFETY: `field` is a binding dyad from the store.
        if unsafe { crate::binding::Binding::spelling(field) } != "lex_rank" {
            return Err(ParseError::BadAssignTarget);
        }
    }
    // A name is written only if its binding carries `mut` and no `immut`.
    // SAFETY: `lhs` is a reduced dyad from the store; one of the `binding` type is a binding.
    unsafe {
        if dyad::ty(lhs) == types.binding_ {
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
    build_store(store, types, op, lhs, rhs)
}

/// [`build`] past the gates: the declaration's own first write into the storage it laid out.
pub(super) fn build_store(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `rhs` is a reduced dyad from the store.
    unsafe { super::read::value_type(types, rhs) }?;
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let (lhs_d, rhs_d) = unsafe { (types.through(lhs), types.through(rhs)) };
    // SAFETY: `through` hands back its argument, the dyad a binding names, or the storage.
    let (lhs_ty, rhs_ty) = unsafe { (dyad::ty(lhs_d), dyad::ty(rhs_d)) };
    // SAFETY: as above.
    let lhs_type = unsafe { types.type_of(lhs_d) };
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    if unsafe { super::tape::is_cell_read(types, lhs_d) } {
        // SAFETY: `lhs_d` is a tape cell read, `rhs` a reduced dyad.
        return Ok(unsafe { super::tape::build_write(store, types, lhs_d, rhs) });
    }
    if lhs_ty == types.hashmap.get {
        // SAFETY: `lhs_d` is a get node, `rhs` a reduced dyad.
        return unsafe { super::hashmap::build_put(store, types, lhs_d, rhs) };
    }
    if lhs_ty == types.tape.is_constructed {
        // SAFETY: `rhs` is a reduced dyad from the store.
        if unsafe { super::read::output_type(types, rhs) } != types.bool_ {
            return Err(ParseError::FlagTakesBool);
        }
        // SAFETY: `lhs_d` is a flag slot node, `rhs` a reduced bool dyad.
        return Ok(unsafe { super::tape::build_flag_write(store, types, lhs_d, rhs) });
    }
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    if unsafe { super::this::is_field_read(types, lhs_d) } {
        // SAFETY: `lhs_d` is a field read, `rhs` a reduced dyad.
        return unsafe { super::this::build_write(store, types, lhs_d, rhs) };
    }
    if lhs_ty == types.deref_ {
        // SAFETY: `lhs_d` is a deref node, `rhs` a reduced dyad.
        return unsafe { super::pointer::build_storeptr(store, types, lhs_d, rhs) };
    }
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    if let Some(Place::Call(call)) = unsafe { place_of(types, lhs_d) } {
        // SAFETY: `call` came from `place_of`, `rhs` is a reduced dyad.
        return unsafe {
            let place = call_place(store, types, call);
            super::pointer::build_storeptr(store, types, place, rhs)
        };
    }
    // SAFETY: `lhs_d`/`rhs` are reduced dyads from the store.
    let (target, marked) = unsafe { (read_kind(types, lhs_d), types.is_storage(lhs_d)) };
    match target {
        Read::Container(t) if t == types.type_ || t == types.dyad_ => {
            // SAFETY: as above.
            if !unsafe { box_takes(types, t, rhs) } {
                return Err(ParseError::BadDeclaredType);
            }
            return Ok(store.alloc_words(op, &[lhs, rhs, types.ops.store_leaf(NumType::I64)]));
        }
        // SAFETY: as above.
        Read::Container(t) if unsafe { meta::is_node_valued(t, types.fn_type) } => {
            // SAFETY: as above.
            if unsafe { super::node_output(types, rhs) } != Some(t) {
                return Err(ParseError::TypeMismatch);
            }
            return Ok(store.alloc_words(op, &[lhs, rhs, types.ops.store_leaf(NumType::I64)]));
        }
        Read::Rational => {
            // SAFETY: `rhs` is a reduced dyad from the store.
            let fits = unsafe {
                super::rational::is_rational_value(types, rhs)
                    || matches!(super::operand_of(types, rhs), super::Operand::Literal)
            };
            if !fits {
                return Err(ParseError::TypeMismatch);
            }
            return Ok(store.alloc_words(op, &[lhs, rhs, types.ops.rational_store]));
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
    let lhs_pointer = unsafe { is_pointer_type(lhs_type) };
    if lhs_pointer && rhs_ty == types.rational {
        return Err(ParseError::TypeMismatch);
    }
    // SAFETY: as above.
    let rhs = unsafe {
        if dyad::ty(rhs_d) == types.rational {
            let nt = of_type_node(lhs_type);
            commit_if_literal(store, types, rhs, &Operand::Literal, lhs_type, nt)?
        } else {
            super::check_store_type(types, lhs_type, rhs)?;
            rhs
        }
    };
    // A non-owning value in an owning place ends in a double free at scope exit.
    // SAFETY: `rhs` is a reduced dyad from the store.
    let rhs_owning = unsafe { super::drop_model::is_owning_value(types, rhs) };
    if super::drop_model::is_owning_place(types, lhs_d) && !rhs_owning {
        return Err(ParseError::NonOwningIntoOwning);
    }
    // SAFETY: `lhs` is a typed variable checked assignable above.
    let nt = unsafe { of_type_node(lhs_type) };
    Ok(store.alloc_words(op, &[lhs, rhs, types.ops.store_leaf(nt)]))
}

/// Whether a `t` box, `type` or `dyad`, takes `value`: a type, a name whose place holds one,
/// or into a `dyad` box a node. A number would be followed as a node's address by every later
/// reader. DESIGN ›A `dyad ?` place is transparent: a placeholder for a new node of any type‹.
///
/// # Safety
/// `value` must be a reduced dyad from the store.
pub(crate) unsafe fn box_takes(types: &Core, t: DyadPtr, value: DyadPtr) -> bool {
    match read_kind(types, value) {
        Read::Identity => true,
        Read::Container(c) => {
            if t == types.type_ {
                c == types.type_
            } else {
                !c.is_null()
            }
        }
        Read::Address => t == types.dyad_,
        Read::Executable(_) => super::read::output_type(types, value) == t,
        _ => false,
    }
}

/// Guards a null storage address like the interpreter's `Uninitialized`; the
/// width comes from the node's own store leaf, the one decision read twice.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `=` application `[lhs, rhs, store leaf]`.
    unsafe {
        let (lhs, rhs) = operands(node);
        let leaf = *(dyad::value(node) as *const DyadPtr).add(2);
        let Some(nt) = lw.types().ops.store_nt_of(leaf) else {
            return Err(CompileError::Internal("a store node holds a store leaf"));
        };
        let lhs = lw.through(lhs);
        if lw.types().is_hole(lhs) {
            return Err(CompileError::Uninitialized);
        }
        let v = lw.lower(rhs)?;
        lw.write_place(lhs, nt.cranelift_type(), v)?;
        Ok(v)
    }
}
