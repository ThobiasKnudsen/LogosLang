// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `=`: assignment. Constructed at discovery, it reads the place to its left
//! and drives its right side to the boundary. Its node over a frame's place is `[mint, lhs,
//! rhs]`, the mint `=`'s for the place's type, holding the store leaf for how that type moves.
//! `=` returns nothing. Beside it, the one mover every whole-value write runs.
//! ›`=` sits beside `:=`, and returns nothing‹ [[R277]]

use crate::Core;
use cranelift_codegen::ir::Value;

use super::numtype::{read_scalar_nt, write_scalar_nt, NumType};
use super::read::{
    call_place, carry_of, is_lex_rank_field, place_of, read_kind, Carry, Place, Read,
};
use super::{meta, mint, operands, Cx};
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
        &["lhs", "rhs"],
    );
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare("=", id);
    cx.metas.insert(id, construct);
    cx.lower.insert(id, lower);
    id
}

/// The place an `=` at the center of `tape` writes, read before its right side: the cells before
/// it built to one and removed, with the slot it fills where it names one. A write into any other
/// place is checked against the gates that grant it here, since whether a place may be written
/// needs nothing from the right side.
pub(crate) fn read_target(
    p: &mut crate::parse::Parser,
    tape: &mut crate::parse::ParsingTape,
) -> Result<(crate::parse::Cell, Option<crate::parse::SlotKind>), ParseError> {
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
    // SAFETY: `target` holds the reduced dyad of the cell to the left.
    let slot = unsafe { p.slot_of(target.dyad) };
    match slot {
        Some(_) if !p.filling_definition() => return Err(ParseError::SlotOutsideDefinition),
        Some(_) => {}
        None => p.check_write(&target)?,
    }
    Ok((target, slot))
}

fn construct(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, ParseError> {
    let types = p.types();
    let (place, slot) = read_target(p, tape)?;
    let target = place.dyad;
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
    let value = p.held_type_or(value);
    // SAFETY: `target` is the reduced dyad of the cell to the left.
    let into_cell = unsafe { dyad::ty(target) } == types.tape.cell_type;
    let value = if into_cell {
        value
    } else {
        // SAFETY: `target` and `value` are reduced dyads from the store.
        unsafe { p.bracket_into(target, value) }?
    };
    // A place takes a copy of a value it would only borrow, one an owner must free only where
    // the place holds its value (›`:=` decides nothing about copy or reference; the type's
    // reading rule does‹ [[R107]]).
    // SAFETY: `target` and `value` are reduced dyads from the store.
    let value = unsafe {
        let copies = || match super::read::value_type(types, value) {
            Ok(ty) => {
                !super::copy::needs_owner(types, ty) || super::drop_model::holds_place(p, target)
            }
            Err(_) => false,
        };
        if !into_cell && slot.is_none() && copies() {
            super::copy::taken(p.store(), types, value)
        } else {
            value
        }
    };
    // What `=` writes reaches a place, which holds it or not; the line's end never frees it
    // (›Holding is decided at the binding site, parameters included‹ [[R61]]).
    p.take(value);
    if into_cell {
        // SAFETY: `target` is a cell type read, `value` a reduced dyad.
        let node = unsafe { p.stamp(target, value) }?;
        tape.place(node);
        return Ok(crate::parse::Constructed::Placed);
    }
    let node = if let Some(kind) = slot {
        // SAFETY: `value` is the reduced dyad of the right side.
        unsafe { p.slot_fill(kind, target, value) }?
    } else {
        let types = p.types();
        // A value whose type owns nothing, a `&` or a plain `@T`, has no copy an owner could
        // free.
        // SAFETY: `target` and `value` are reduced dyads from the store.
        if unsafe {
            (dyad::ty(target) == types.binding_ && p.owns_node(target) || p.is_owning_read(target))
                && super::drop_model::teardown_of(types, value)
                    == super::drop_model::Teardown::Nothing
        } {
            return Err(ParseError::NonOwningIntoOwning);
        }
        let node = build_store(p.store(), types, id, target, value)?;
        // SAFETY: `target` is the reduced dyad of the cell to the left; `node` the write just
        // built over it.
        unsafe {
            if let Some(free) = super::drop_model::displaced_free(p, target)? {
                displace(p.store(), types, node, free);
            }
            p.note_receiver_write(target);
        }
        p.note_write(node, &place);
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
    let at = if mint::origin(dyad::ty(node)) == types.assign { 1 } else { 2 };
    let slot = (dyad::value(node) as *mut DyadPtr).add(at);
    *slot = super::drop_model::build_displace(store, types, *slot, free);
}

/// A literal right side commits to the target's type; a non-literal one must
/// already be it. Shared with the declaration's first write, so a name's first write and
/// every later one over a place of one type are nodes of one mint. The gates were checked
/// where `=` read its place ([`read_target`]); a declaration's first write has none to meet.
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
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    let place = unsafe { place_of(types, lhs_d) };
    // SAFETY: as above.
    if place.is_none() && !unsafe { is_lex_rank_field(types, lhs_d) } {
        return Err(if lhs_ty == types.rational {
            // SAFETY: as above; a node of `rational_number` holds its fraction's bytes.
            let spelled = unsafe { super::rational::spell(lhs_d) };
            ParseError::AssignToLiteral(Box::new(spelled))
        } else {
            ParseError::BadAssignTarget
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
    if unsafe { super::this::is_named(types, lhs_d) } {
        // SAFETY: `lhs_d` is a named field read, `rhs` a reduced dyad.
        return unsafe { super::this::build_named_write(store, types, lhs_d, rhs) };
    }
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    if unsafe { super::this::is_field_read(types, lhs_d) } {
        // SAFETY: `lhs_d` is a field read, `rhs` a reduced dyad.
        return unsafe { super::this::build_write(store, types, lhs_d, rhs) };
    }
    // Left: a place with an address, a frame's or a dereference's cell, a call place's
    // included, written through it whole.
    let lhs_d = match place {
        // SAFETY: `call` came from `place_of`.
        Some(Place::Call(call)) => unsafe { call_place(store, types, call) },
        _ => lhs_d,
    };
    // SAFETY: `lhs_d` is a reduced dyad from the store.
    let (through_pointer, place_type) =
        unsafe { (dyad::ty(lhs_d) == types.deref_, super::read::output_type(types, lhs_d)) };
    // SAFETY: as above; `place_type` is null or a type node.
    let carry = unsafe {
        (through_pointer || types.is_storage(lhs_d)).then(|| carry_of(types, place_type))
    }
    .flatten();
    let Some(carry) = carry else {
        return Err(ParseError::BadAssignTarget);
    };
    // SAFETY: `place_type` is the type node `carry` was read from.
    let held_record = unsafe {
        super::this::is_plain(types, place_type) && !meta::instances_free_of(place_type).is_null()
    };
    // stand-in for #240: no free runs over a value displaced through a pointer.
    if through_pointer && held_record {
        return Err(ParseError::HeldRecordThroughPointerNotInSeed);
    }
    // SAFETY: `rhs` is a reduced dyad from the store.
    let rhs = unsafe {
        let line = types.through(rhs);
        let into = if dyad::ty(line) == types.tape.cell_dyad_at {
            super::tape::build_cell_into(store, types, line, place_type)
        } else {
            None
        };
        match into {
            Some(into) => into,
            None if !through_pointer
                && dyad::ty(lhs_d) == types.binding_
                && super::drop_model::holds_dyad(types, lhs_d) =>
            {
                super::land_held(store, types, rhs, place_type)?
            }
            None => super::land(store, types, rhs, place_type)?,
        }
    };
    // A value that owns nothing here, in an owning place, is freed by an owner that never had it.
    // SAFETY: `rhs` is a reduced dyad from the store.
    let rhs_ends = unsafe { super::drop_model::teardown_of(types, rhs) };
    if super::drop_model::is_owning_place(types, lhs_d)
        && rhs_ends == super::drop_model::Teardown::Nothing
    {
        return Err(ParseError::NonOwningIntoOwning);
    }
    // A frame's place stays the name, which lifting a `?`'s read veto reads; a cell is held as
    // its dereference, which both tiers write through.
    let written = if through_pointer { lhs_d } else { lhs };
    let leaf = types.ops.put_leaf(carry);
    // SAFETY: `op` is `=`, an operand record of two roles; `place_type` is the type node `carry`
    // was read from.
    Ok(unsafe { mint::node(store, types, op, &[place_type], types.void_, leaf, &[written, rhs]) })
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
        _ => any_node && super::node_value_output(types, value).is_some(),
    }
}

/// Guards a null storage address like the interpreter's `Uninitialized`; what moves is what
/// the place's type moves as, the decision the leaf was picked by.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a node of one of `=`'s mints, `[lhs, rhs]` over a place with an address.
    unsafe {
        let (lhs, rhs) = operands(node);
        let lhs = lw.through(lhs);
        if lw.types().is_hole(lhs) {
            return Err(CompileError::Uninitialized);
        }
        let Some(carry) = carry_of(lw.types(), super::read::output_type(lw.types(), lhs)) else {
            return Err(CompileError::Internal("a store node's place moves whole"));
        };
        let v = lower_take(lw, carry, rhs)?;
        // A frame's word may live in a register the place was promoted to, which has no address.
        if let (Carry::Word(nt), true) = (carry, lw.types().is_storage(lhs)) {
            lw.write_place(lhs, nt.cranelift_type(), v)?;
            return Ok(v);
        }
        super::pointer::lower_at_place(lw, lhs, handed_type(carry), |s, dest| {
            lower_put(s, carry, dest, 0, v);
            Ok(v)
        })
    }
}
