// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `=`: assignment. Constructed at discovery, it reads the place to its left
//! and drives its right side to the boundary; its builder bakes how the place's type
//! moves into the store leaf in the op slot. `=` returns nothing. Beside it, the one
//! mover every whole-value write runs. DESIGN ›The scope's constructor is the driver‹

use crate::Core;
use cranelift_codegen::ir::Value;

use super::numtype::{read_scalar_nt, write_scalar_nt, NumType};
use super::read::{
    call_place, carry_of, is_lex_rank_field, place_of, read_kind, Carry, Place, Read,
};
use super::{meta, operands, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError};
use crate::run::{RunError, Runtime};
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
    // An `if` whose arms give two types gives one where a typed place lands each arm.
    // SAFETY: `rhs` is a reduced dyad from the store.
    let arms_differ = match unsafe { super::read::value_type(types, rhs) } {
        Ok(_) => false,
        Err(ParseError::ArmsDiffer) => true,
        Err(e) => return Err(e),
    };
    // SAFETY: `lhs` is a reduced dyad from the store.
    let lhs_d = unsafe { types.through(lhs) };
    // SAFETY: `through` hands back its argument, the dyad a binding names, or the storage.
    let lhs_ty = unsafe { dyad::ty(lhs_d) };
    // SAFETY: as above.
    let lhs_type = unsafe { types.type_of(lhs_d) };
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    let place = unsafe { place_of(types, lhs_d) };
    // SAFETY: as above.
    if place.is_none() && !unsafe { is_lex_rank_field(types, lhs_d) } {
        // SAFETY: as above; a `Literal` read is a rational node with its fraction blob.
        return Err(unsafe {
            if read_kind(types, lhs_d) == Read::Literal {
                ParseError::AssignToLiteral(Box::new(super::rational::spell(lhs_d)))
            } else {
                ParseError::BadAssignTarget
            }
        });
    }
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    if unsafe { super::tape::is_cell_read(types, lhs_d) } {
        if arms_differ {
            return Err(ParseError::ArmsDiffer);
        }
        // SAFETY: `lhs_d` is a tape cell read, `rhs` a reduced dyad.
        return Ok(unsafe { super::tape::build_write(store, types, lhs_d, rhs) });
    }
    if lhs_ty == types.hashmap.get {
        // SAFETY: `lhs_d` is a get node, `rhs` a reduced dyad.
        return unsafe { super::hashmap::build_put(store, types, lhs_d, rhs) };
    }
    if lhs_ty == types.tape.is_constructed {
        if arms_differ {
            return Err(ParseError::ArmsDiffer);
        }
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
    if let Some(Place::Call(call)) = place {
        // SAFETY: `call` came from `place_of`, `rhs` is a reduced dyad.
        return unsafe {
            let deref = call_place(store, types, call);
            super::pointer::build_storeptr(store, types, deref, rhs)
        };
    }
    // Left: a frame's place, written at its offset when its type moves whole.
    // SAFETY: `lhs_d` is a reduced dyad from the store; `lhs_type` is null or a type node.
    let carry = unsafe { types.is_storage(lhs_d).then(|| carry_of(types, lhs_type)) }.flatten();
    let Some(carry) = carry else {
        return Err(ParseError::BadAssignTarget);
    };
    // SAFETY: `rhs` is a reduced dyad from the store.
    let rhs = unsafe { super::land(store, types, rhs, lhs_type) }?;
    // A non-owning value in an owning place ends in a double free at scope exit.
    // SAFETY: `rhs` is a reduced dyad from the store.
    let rhs_owning = unsafe { super::drop_model::is_owning_value(types, rhs) };
    if super::drop_model::is_owning_place(types, lhs_d) && !rhs_owning {
        return Err(ParseError::NonOwningIntoOwning);
    }
    Ok(store.alloc_words(op, &[lhs, rhs, types.ops.put_leaf(carry)]))
}

/// What `value` hands a place of `carry`: the word it runs to, or the address of its bytes.
///
/// # Safety
/// `value` must be a reduced dyad from the store whose type moves as `carry`.
pub(crate) unsafe fn take(rt: &mut Runtime, carry: Carry, value: DyadPtr) -> Result<i64, RunError> {
    match carry {
        Carry::Word(_) => rt.run(value),
        Carry::Bytes(_) => super::by_copy::record_addr(rt, value).map(|addr| addr as i64),
    }
}

/// Write what [`take`] handed over into the place at `dest`, the type's width of bytes.
///
/// # Safety
/// `dest` must be writable for `carry.width()` bytes; `taken` what [`take`] gave for `carry`.
pub(crate) unsafe fn put(carry: Carry, dest: *mut u8, taken: i64) {
    match carry {
        Carry::Word(nt) => write_scalar_nt(nt, dest, taken),
        // `v = v` copies a place onto itself.
        Carry::Bytes(width) => std::ptr::copy(taken as usize as *const u8, dest, width),
    }
}

/// What a whole read of the place at `at` hands over, as [`take`] does.
///
/// # Safety
/// `at` must be readable for `carry.width()` bytes.
pub(crate) unsafe fn load(carry: Carry, at: *mut u8) -> i64 {
    match carry {
        Carry::Word(nt) => read_scalar_nt(nt, at),
        Carry::Bytes(_) => at as i64,
    }
}

/// [`take`]'s compiled half.
///
/// # Safety
/// As [`take`].
pub(crate) unsafe fn lower_take(
    lw: &mut Lowerer,
    carry: Carry,
    value: DyadPtr,
) -> Result<Value, CompileError> {
    match carry {
        Carry::Word(_) => lw.lower(value),
        Carry::Bytes(_) => super::by_copy::lower_record_addr(lw, value),
    }
}

/// [`put`]'s compiled half, `offset` bytes past `dest`.
pub(crate) fn lower_put(lw: &mut Lowerer, carry: Carry, dest: Value, offset: i64, taken: Value) {
    match carry {
        Carry::Word(nt) => lw.store_at(nt.cranelift_type(), dest, offset, taken),
        Carry::Bytes(width) => {
            let dest = lw.offset_addr(dest, offset);
            lw.copy_bytes(dest, taken, width);
        }
    }
}

/// [`load`]'s compiled half, `offset` bytes past `at`.
pub(crate) fn lower_load(lw: &mut Lowerer, carry: Carry, at: Value, offset: i64) -> Value {
    match carry {
        Carry::Word(nt) => lw.load_at(nt.cranelift_type(), at, offset),
        Carry::Bytes(_) => lw.offset_addr(at, offset),
    }
}

/// The machine type what [`lower_take`] and [`lower_load`] hand over is held at.
pub(crate) fn handed_type(carry: Carry) -> cranelift_codegen::ir::Type {
    match carry {
        Carry::Word(nt) => nt.cranelift_type(),
        Carry::Bytes(_) => NumType::U64.cranelift_type(),
    }
}

/// Whether a `t` box, `type` or `dyad`, takes `value`: a type, a name whose place holds one,
/// or into a `dyad` box any node, named or not. A number would be followed as a node's address
/// by every later reader. DESIGN ›A `dyad ?` place is transparent: a placeholder for a new node
/// of any type‹.
///
/// # Safety
/// `value` must be a reduced dyad from the store.
pub(crate) unsafe fn box_takes(types: &Core, t: DyadPtr, value: DyadPtr) -> bool {
    let any_node = t == types.dyad_;
    // A value made when the call runs, a run's own or a record's place, is a node's address too.
    if any_node && [types.this.pack, types.by_copy.out].contains(&dyad::ty(value)) {
        return true;
    }
    match read_kind(types, value) {
        Read::Identity => true,
        Read::Container(c) => {
            if any_node {
                !c.is_null()
            } else {
                c == types.type_
            }
        }
        Read::Address => any_node,
        Read::Executable(_) if super::read::output_type(types, value) == t => true,
        _ => any_node && super::node_output(types, value).is_some(),
    }
}

/// Guards a null storage address like the interpreter's `Uninitialized`; what moves is what
/// the place's type moves as, the decision the leaf was picked by.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `=` application `[lhs, rhs, store leaf]` over a storage place.
    unsafe {
        let (lhs, rhs) = operands(node);
        let lhs = lw.through(lhs);
        if lw.types().is_hole(lhs) {
            return Err(CompileError::Uninitialized);
        }
        let Some(carry) = carry_of(lw.types(), lw.types().type_of(lhs)) else {
            return Err(CompileError::Internal("a store node's place moves whole"));
        };
        let v = lower_take(lw, carry, rhs)?;
        match carry {
            // A word may live in a register the place was promoted to.
            Carry::Word(nt) => lw.write_place(lhs, nt.cranelift_type(), v)?,
            Carry::Bytes(_) => {
                let dest = lw.place_addr(lhs)?;
                lower_put(lw, carry, dest, 0, v);
            }
        }
        Ok(v)
    }
}
