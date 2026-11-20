// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The copy a place that holds its value takes of a value it would only borrow: a new value made
//! from the old one's fields, each part it owns copied by its own type's copy, a `&` or a plain
//! `@T` kept as the address it is. ›`:=` decides nothing about copy or reference; the type's
//! reading rule does‹ [[R107]]

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::drop_model::{teardown_of, teardown_of_type, Teardown};
use super::read::{carry_of, Carry};
use super::{array, meta, Cx};
use crate::binding::Binding;
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// `copy` is `[value]` under its mint for the type of the value it copies, which it gives. It
/// has no spelling: the parse builds it where a value lands in a place that holds it.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record =
        meta::operand_record(cx, meta::TUPLE_TAG, meta::prec::INERT, Assoc::Left, &["value"]);
    let copy = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `copy` carries the record built above.
    unsafe { meta::mark_own(copy) };
    cx.lower.insert(copy, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (copy, leaf)
}

/// What a value owns beyond its own bytes, which its copy copies too.
#[derive(Debug, Clone, Copy)]
enum Owned {
    /// An owning `@T`: the block, each cell copied by the pointee's copy.
    Block(DyadPtr),
    /// A node a Logos `parse` built, of a type that fills `free`: the node, field by field.
    Node(DyadPtr),
    /// A record kept in its bytes, of a type that fills `free` or a `{ … }` whose entries have
    /// ends: its fields, in place.
    Record(DyadPtr),
    /// A map: its table, each map among its values copied too. The store keeps every table.
    Map(DyadPtr),
    /// A `dyad` entry of a `{ … }`: the node it made, as a `dyad ?` place copies it.
    Dyad,
}

/// Whether a value of `ty` owns memory, so its bytes alone are not its copy.
///
/// # Safety
/// `ty` must be null or a type node from the store.
pub(crate) unsafe fn owns(types: &Core, ty: DyadPtr) -> bool {
    owned(types, ty).is_some()
}

/// Whether a copy of a value of `ty` must be freed by an owner, so only a place that holds its
/// value takes one; a map's tables the store keeps, so any place takes a copy of a map.
///
/// # Safety
/// `ty` must be null or a type node from the store.
pub(crate) unsafe fn needs_owner(types: &Core, ty: DyadPtr) -> bool {
    !matches!(owned(types, ty), None | Some(Owned::Map(_)))
}

/// `None` where a value of `ty` owns no memory, so its bytes are its copy.
///
/// # Safety
/// `ty` must be null or a type node from the store.
unsafe fn owned(types: &Core, ty: DyadPtr) -> Option<Owned> {
    if super::hashmap::is_hashmap(types, ty) {
        return Some(Owned::Map(ty));
    }
    match teardown_of_type(types, ty) {
        Teardown::Block(pointee) => Some(Owned::Block(pointee)),
        Teardown::Node(_) if meta::is_node_valued(ty, types.fn_type) => Some(Owned::Node(ty)),
        Teardown::Node(_) | Teardown::Entries(_) => Some(Owned::Record(ty)),
        // A `dyad ?` place copies the node it holds itself.
        Teardown::Dyad | Teardown::Nothing => None,
    }
}

/// What field `binding` of type `ty` in a value of `owner` owns: an entry of a `{ … }` whatever
/// the name it was held, a `dyad`'s node included (›`{ … }` outside text is a scope kept as a
/// value, read with `.`‹ [[R374]]); any other field holding a node only with `own` on its binding
/// (›A field may be `own t ?`, `t` a type whose body fills `static free`‹ [[R68]]).
///
/// # Safety
/// `owner` must be a record type node, `binding` one of its fields' binding dyads and `ty` that
/// field's declared type, from the store.
unsafe fn field_owned(
    types: &Core,
    owner: DyadPtr,
    binding: DyadPtr,
    ty: DyadPtr,
) -> Option<Owned> {
    if let Teardown::Entries(_) = teardown_of_type(types, owner) {
        return match teardown_of_type(types, ty) {
            Teardown::Dyad => Some(Owned::Dyad),
            _ => owned(types, ty),
        };
    }
    match owned(types, ty)? {
        Owned::Node(_) if !Binding::has_gate(binding, types.own_) => None,
        owned => Some(owned),
    }
}

/// Each field of a value of the record type `ty`, with its binding and declared type.
///
/// # Safety
/// `ty` must be a record type node from the store.
unsafe fn fields<'a>(ty: DyadPtr) -> impl Iterator<Item = (DyadPtr, DyadPtr)> + 'a {
    let fields = array::items(meta::record_fields_of(ty));
    let bindings = array::items(meta::record_field_bindings_of(ty));
    // SAFETY: each field is a hole node from the store.
    bindings.iter().zip(fields).map(|(&b, &field)| (b, unsafe { super::hole::type_in(field) }))
}

/// What a place that holds its value takes of `value`: `value` itself when it is made or moved
/// there, or owns no memory; otherwise its copy, which the place owns (›A filled `static free`
/// is the value's teardown; one name owns each value‹ [[R65]]).
///
/// # Safety
/// `value` must be a reduced dyad from the store.
pub(crate) unsafe fn taken(store: &mut Store, types: &Core, value: DyadPtr) -> DyadPtr {
    let ty = super::read::output_type(types, value);
    if owned(types, ty).is_none() || teardown_of(types, value) != Teardown::Nothing {
        return value;
    }
    super::mint::node(store, types, types.copy_, &[ty], ty, types.ops.copy_, &[value])
}

/// The place at `at` holds a value of `ty` copied bit for bit; each part the value owns becomes a
/// copy of its own, a NULL owner staying NULL.
///
/// # Safety
/// `at` must hold a whole value of `ty`, a type node from the store, laid out as its places are.
pub(crate) unsafe fn deepen(rt: &mut Runtime, ty: DyadPtr, at: *mut u8) -> Result<(), RunError> {
    match owned(rt.types(), ty) {
        Some(what) => deepen_as(rt, what, at),
        None => Ok(()),
    }
}

/// [`deepen`] of a value at `at` that owns `what`.
///
/// # Safety
/// As [`deepen`], the value at `at` owning `what`.
unsafe fn deepen_as(rt: &mut Runtime, what: Owned, at: *mut u8) -> Result<(), RunError> {
    match what {
        Owned::Block(pointee) => {
            let block = std::ptr::read_unaligned(at as *const u64) as usize as *mut u8;
            if !block.is_null() {
                let copy = copy_block(rt, pointee, block)?;
                std::ptr::write_unaligned(at as *mut u64, copy as u64);
            }
        }
        Owned::Node(t) => {
            let node = std::ptr::read_unaligned(at as *const DyadPtr);
            if !node.is_null() {
                let copy = copy_node(rt, t, node)?;
                std::ptr::write_unaligned(at as *mut DyadPtr, copy);
            }
        }
        Owned::Record(t) => {
            for (binding, field_ty) in fields(t) {
                if let Some(what) = field_owned(rt.types(), t, binding, field_ty) {
                    deepen_as(rt, what, at.add(Binding::read(binding).offset))?;
                }
            }
        }
        Owned::Map(t) => {
            let types: *const Core = rt.types();
            super::hashmap::deepen(rt.store(), &*types, t, at);
        }
        Owned::Dyad => {
            let held = std::ptr::read_unaligned(at as *const usize) as DyadPtr;
            let copy = super::dyad_place::copy(rt, held)?;
            std::ptr::write_unaligned(at as *mut i64, copy);
        }
    }
    Ok(())
}

/// A new block of `block`'s length holding its bytes, each cell then deepened as `pointee`'s.
///
/// # Safety
/// `block` must be a live block from `alloc` whose cells hold values of `pointee`.
unsafe fn copy_block(
    rt: &mut Runtime,
    pointee: DyadPtr,
    block: *mut u8,
) -> Result<*mut u8, RunError> {
    let total = super::drop_model::block_size(block);
    let copy = super::drop_model::heap_alloc(total);
    if copy.is_null() {
        return Err(RunError::OutOfMemory);
    }
    std::ptr::copy_nonoverlapping(block, copy, total);
    rt.note_alloc();
    if owned(rt.types(), pointee).is_some() {
        let width = carry_of(rt.types(), pointee).ok_or(RunError::NoLayout(pointee))?.width();
        for k in 0..total / width {
            deepen(rt, pointee, copy.add(k * width))?;
        }
    }
    Ok(copy)
}

/// A new node of `ty` holding `node`'s fields, each owned one a copy of its own: a node as its
/// slot holds it, an owning pointer as a new node of the field's type holding the new block's
/// address, as a field write stores one.
///
/// # Safety
/// `node` must be a node of `ty`, a type a Logos `parse` builds, laid out as its fresh nodes are.
unsafe fn copy_node(rt: &mut Runtime, ty: DyadPtr, node: DyadPtr) -> Result<DyadPtr, RunError> {
    let copy = super::this::copy_of(rt.store(), node);
    let slots = dyad::value(copy) as *mut DyadPtr;
    for (k, (binding, field_ty)) in fields(ty).enumerate() {
        let held = *slots.add(k);
        if held.is_null() {
            continue;
        }
        match field_owned(rt.types(), ty, binding, field_ty) {
            Some(Owned::Node(t)) => *slots.add(k) = copy_node(rt, t, held)?,
            Some(Owned::Block(_)) => {
                let mut word = std::ptr::read_unaligned(dyad::value(held) as *const u64);
                deepen(rt, field_ty, (&mut word as *mut u64).cast())?;
                *slots.add(k) = rt.store().alloc_blob(field_ty, &word.to_ne_bytes());
            }
            // A field kept in bytes keeps the node it was written from, run at each read.
            Some(Owned::Record(_) | Owned::Map(_) | Owned::Dyad) | None => {}
        }
    }
    Ok(copy)
}

/// The copy of a value that is one word: an owning pointer, or a node's address.
///
/// # Safety
/// `word` must be a whole value of `ty`, a type node from the store.
unsafe fn copy_word(rt: &mut Runtime, ty: DyadPtr, word: i64) -> Result<i64, RunError> {
    let mut word = word;
    deepen(rt, ty, (&mut word as *mut i64).cast())?;
    Ok(word)
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `copy` node `[value]` under its mint for the value's type.
    unsafe {
        let value = *(dyad::value(node) as *const DyadPtr);
        let ty = super::read::output_type(rt.types(), node);
        let carry = carry_of(rt.types(), ty).ok_or(RunError::NoLayout(ty))?;
        let taken = super::assign::take(rt, carry, value)?;
        match carry {
            Carry::Word(_) => copy_word(rt, ty, taken),
            Carry::Bytes(width) => {
                let copy = rt.scratch(width);
                std::ptr::copy_nonoverlapping(taken as usize as *const u8, copy, width);
                deepen(rt, ty, copy)?;
                Ok(copy as i64)
            }
        }
    }
}

/// A record's copy is made in a stack slot of the function, as a record result's scratch is;
/// the parts it owns are copied by the seed.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run`].
    unsafe {
        let value = *(dyad::value(node) as *const DyadPtr);
        let ty = super::read::output_type(lw.types(), node);
        let carry = carry_of(lw.types(), ty).ok_or(CompileError::NotLowerable(node))?;
        let taken = super::assign::lower_take(lw, carry, value)?;
        let ty = lw.node_addr(ty);
        match carry {
            Carry::Word(_) => lw.call_seed(compiled_copy_word as *const () as usize, &[ty, taken]),
            Carry::Bytes(width) => {
                let copy = lw.scratch_slot(width);
                lw.copy_bytes(copy, taken, width);
                lw.call_seed(compiled_deepen as *const () as usize, &[ty, copy])?;
                Ok(copy)
            }
        }
    }
}

/// # Safety
/// Called only by compiled code, with its context, a type `lower` baked and a whole value of it.
unsafe extern "C" fn compiled_copy_word(
    ctx: *mut crate::run::Context,
    ty: DyadPtr,
    word: i64,
) -> i64 {
    crate::run::interpret_step(ctx, |rt| copy_word(rt, ty, word))
}

/// # Safety
/// Called only by compiled code, with its context, a type `lower` baked and the address of a
/// record of it copied bit for bit.
unsafe extern "C" fn compiled_deepen(ctx: *mut crate::run::Context, ty: DyadPtr, at: i64) -> i64 {
    crate::run::interpret_step(ctx, |rt| deepen(rt, ty, at as usize as *mut u8).map(|()| 0))
}
