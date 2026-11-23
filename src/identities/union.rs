// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `union ( a := T, b := U )`: a type whose value holds one of its members at a time. A value
//! is `[tag: TAG_BYTES][room of the largest member]`; the tag is 0 while it holds none, else
//! the held member's place among the members, counted from 1. A member read `v.a` is a
//! dereference of a `member` node `{type: member, value: [base, union, tag, access, op,
//! output_type]}`, which runs to the room's address: a write sets the tag, ending the member
//! held before, and every other use checks it. `v holds a` is an `asks` node `[base, union, tag,
//! op, output_type]`, the tag compared. ›A union holds one of its members at a time: `union (
//! a := T, b := U )`‹ [[R379]]

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::NumType;
use super::{array, hole, meta, pointer, Cx};
use crate::binding::Binding;
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct UnionIds {
    /// The former, spelled `union`: what `type_mint` keeps each union under, and what a
    /// union's record names as the former that made it.
    pub union: DyadPtr,
    pub member: DyadPtr,
    pub leaf: DyadPtr,
    /// The word `holds`, and the node it builds.
    pub holds: DyadPtr,
    pub asks: DyadPtr,
    pub asks_leaf: DyadPtr,
    /// A new value of a union, holding one member a value fills.
    pub fill: DyadPtr,
    pub fill_leaf: DyadPtr,
    /// `[expr, op, output_type]`: the member `expr` reads, taken out of a union its line made,
    /// and made for whoever takes it.
    pub take: DyadPtr,
    pub take_leaf: DyadPtr,
}

/// What a member node does with the member it reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// Reads it, checking the tag.
    Read = 0,
    /// Writes it, setting the tag.
    Write = 1,
    /// Reads it out of a union made for this read alone, checking the tag, and leaves the union
    /// holding none, so the union's end frees nothing and the member is its taker's (›A last
    /// value moves out‹ [[R66]]).
    Take = 2,
}

/// The tag's width in a value.
pub(crate) const TAG_BYTES: usize = 1;
/// The most members the tag tells apart, 0 saying none is held.
pub(crate) const MOST_MEMBERS: usize = (u64::MAX >> (64 - 8 * TAG_BYTES)) as usize;

pub(super) fn register(cx: &mut Cx, cs: &Callables) -> UnionIds {
    let record = meta::record(cx.store, meta::TYPEREC_TAG, meta::prec::READER);
    let union = cx.store.alloc_head(cx.type_, record);
    cx.declare("union", union);
    cx.metas.insert(union, |p, _id, tape| p.construct_union(tape));

    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["base", "union", "tag", "access", "op", "output_type"],
    );
    let member = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(member, lower_member);
    let leaf = callable::mint_native(cx.store, cs.callable, run_member, cs.seed_native);

    // `holds` reads the value to its left and a member's name to its right; it gives a `bool`,
    // so it is built with the comparisons (›Every identity has a `parse_rank`, and `(` works in
    // two steps‹ [[R259]]).
    let sides = &["lhs", "rhs"];
    let record = meta::token_record(cx, meta::prec::COMPARE, crate::parse::Assoc::Left, sides);
    let holds = cx.store.alloc_head(cx.type_, record);
    cx.declare("holds", holds);
    cx.metas.insert(holds, |p, _id, tape| p.construct_holds(tape));
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["base", "union", "tag", "op", "output_type"],
    );
    let asks = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(asks, lower_holds);
    let asks_leaf = callable::mint_native(cx.store, cs.callable, run_holds, cs.seed_native);

    // `T!` reads the type to its left as `.` reads its cell.
    let record = meta::token_record(cx, meta::prec::TIGHT, crate::parse::Assoc::Left, &["lhs"]);
    let fallible = cx.store.alloc_head(cx.type_, record);
    cx.declare("!", fallible);
    cx.metas.insert(fallible, |p, _id, tape| p.construct_fallible(tape));

    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["value", "union", "tag", "op", "output_type"],
    );
    let fill = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `fill` carries the record just built, whose roles name `value`.
    unsafe {
        meta::mark_own(fill);
        meta::install_hands_on(fill, &["value"], &[]);
    }
    cx.lower.insert(fill, lower_fill);
    let fill_leaf = callable::mint_native(cx.store, cs.callable, run_fill, cs.seed_native);

    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["expr", "op", "output_type"],
    );
    let take = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `take` carries the record just built.
    unsafe { meta::mark_own(take) };
    cx.lower.insert(take, lower_take);
    let take_leaf = callable::mint_native(cx.store, cs.callable, run_take, cs.seed_native);
    UnionIds { union, member, leaf, holds, asks, asks_leaf, fill, fill_leaf, take, take_leaf }
}

/// The member holes `value` and `err` of `ty` where `ty` is `T!`, `union ( value := T, err :=
/// error )`; `None` for every other type.
///
/// # Safety
/// `ty` must be null or a type node from the store.
pub(crate) unsafe fn fallible_members(types: &Core, ty: DyadPtr) -> Option<(DyadPtr, DyadPtr)> {
    if !is_union(types, ty) {
        return None;
    }
    let members = array::items(meta::record_fields_of(ty));
    let bindings = array::items(meta::record_field_bindings_of(ty));
    let [value, err] = members[..] else {
        return None;
    };
    let named = bindings.iter().map(|&b| Binding::spelling(b)).eq(["value", "err"]);
    (named && super::error::is_value(types, hole::type_in(err))).then_some((value, err))
}

/// What a function that gives `ty` hands back for `value`, one of the values its body gives: where
/// `ty` is `T!` and `value` gives no `T!`, a new `T!` whose `value` it fills, landed in `T` first
/// (›Target: errors are values (`T!`), handled by `if`, passed on by `try`…‹ [[R140]]); `value`
/// itself otherwise.
///
/// # Safety
/// `ty` must be a type node and `value` a reduced dyad from the store.
pub(crate) unsafe fn filled(
    store: &mut Store,
    types: &Core,
    ty: DyadPtr,
    value: DyadPtr,
) -> Result<DyadPtr, crate::parse::ParseError> {
    let Some((member, _)) = fallible_members(types, ty) else {
        return Ok(value);
    };
    if super::read::output_type(types, value) == ty {
        return Ok(value);
    }
    let t = hole::type_in(member);
    let value = super::land_held(store, types, value, t, super::dyad_place::Holder::Owner)?;
    Ok(build_fill(store, types, value, ty, member))
}

/// The `value` member of `ty` where `ty` is `void!`, whose functions fill it where their body
/// ends, the body giving nothing.
///
/// # Safety
/// `ty` must be null or a type node from the store.
pub(crate) unsafe fn void_value_of(types: &Core, ty: DyadPtr) -> Option<DyadPtr> {
    let (value, _) = fallible_members(types, ty)?;
    (hole::type_in(value) == types.void_).then_some(value)
}

/// `body`, which gives nothing, with one more line, which makes a value of `union` holding
/// `member`, a `void` member: what a `void!` function gives where its body ends. A body of one
/// line that holds nothing is the line itself, so it becomes the first line of a scope below
/// `parent`.
///
/// # Safety
/// `body` must be a closed sequence or a reduced dyad from the store, `union` a union type,
/// `member` one of its member holes, of type `void`, and `parent` a scope node.
pub(crate) unsafe fn end_filled(
    store: &mut Store,
    types: &Core,
    body: DyadPtr,
    [union, member]: [DyadPtr; 2],
    parent: DyadPtr,
) -> DyadPtr {
    use super::scope;
    let filled = build_fill(store, types, std::ptr::null_mut(), union, member);
    if dyad::ty(body) == types.scope {
        let mut lines = scope::exprs_of(body).unwrap_or_default().to_vec();
        lines.push(filled);
        return scope::with_exprs(store, types, body, &lines);
    }
    let sequence = scope::mint(store, types.scope, parent);
    for line in [body, filled] {
        scope::push_item(store, types.array_, sequence, line);
    }
    scope::fill(types, sequence);
    sequence
}

/// A new value of `union` holding `member`, which `value` fills where it runs.
///
/// # Safety
/// `union` must be a union type, `member` one of its member holes and `value` a reduced dyad
/// that gives the member's type.
pub(crate) unsafe fn build_fill(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    union: DyadPtr,
    member: DyadPtr,
) -> DyadPtr {
    let tag = tag_of(store, types, union, member);
    store.alloc_words(types.union.fill, &[value, union, tag, types.union.fill_leaf, union])
}

/// What a fill node fills, the union it makes, and how the member's value moves: `None` for a
/// `void` member, which holds nothing, its value null or a node that gives nothing.
///
/// # Safety
/// `node` must be a fill node from [`build_fill`].
unsafe fn fill_parts(
    types: &Core,
    node: DyadPtr,
) -> (DyadPtr, DyadPtr, Option<super::read::Carry>) {
    let ops = dyad::value(node) as *const DyadPtr;
    let union = *ops.add(1);
    let members = array::items(meta::record_fields_of(union));
    let ty = hole::type_in(members[tag_in(node) as usize - 1]);
    let carry = super::read::carry_of(types, ty);
    debug_assert!(carry.is_some() || ty == types.void_, "a union's member moves whole");
    (*ops, union, carry)
}

/// Starts the union value at `at`: the tag of the member a fill node fills, and an empty room.
///
/// # Safety
/// `node` must be a fill node from [`build_fill`], `at` writable for its union's width.
unsafe fn start(node: DyadPtr, at: *mut u8) {
    let union = *(dyad::value(node) as *const DyadPtr).add(1);
    std::ptr::write_bytes(at, 0, meta::record_size_of(union) as usize);
    std::ptr::copy_nonoverlapping(tag_in(node).to_le_bytes().as_ptr(), at, TAG_BYTES);
}

fn run_fill(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a fill node, whose value gives its member's type.
    unsafe {
        let (value, union, carry) = fill_parts(rt.types(), node);
        let taken = match carry {
            Some(carry) => Some((carry, super::assign::take(rt, carry, value)?)),
            None if !value.is_null() => rt.run(value).map(|_| None)?,
            None => None,
        };
        let at = rt.scratch(meta::record_size_of(union) as usize);
        start(node, at);
        if let Some((carry, taken)) = taken {
            super::assign::put(carry, at.add(TAG_BYTES), taken);
        }
        Ok(at as i64)
    }
}

fn lower_fill(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_fill`].
    unsafe {
        let (value, union, carry) = fill_parts(lw.types(), node);
        let taken = match carry {
            Some(carry) => Some((carry, super::assign::lower_take(lw, carry, value)?)),
            None if !value.is_null() => lw.lower(value).map(|_| None)?,
            None => None,
        };
        let at = lw.scratch_slot(meta::record_size_of(union) as usize);
        let node = lw.node_addr(node);
        lw.call_seed(compiled_start as *const () as usize, &[node, at])?;
        if let Some((carry, taken)) = taken {
            super::assign::lower_put(lw, carry, at, TAG_BYTES as i64, taken);
        }
        Ok(at)
    }
}

/// # Safety
/// Called only by compiled code, with its context, a fill node `lower_fill` baked and the address
/// of room for a value of its union.
unsafe extern "C" fn compiled_start(ctx: *mut crate::run::Context, node: DyadPtr, at: i64) -> i64 {
    crate::run::interpret_step(ctx, |_| {
        // SAFETY: the caller's contract.
        unsafe { start(node, at as usize as *mut u8) };
        Ok(0)
    })
}

/// Whether `t` is a union type.
///
/// # Safety
/// `t` must be null or a type node from the store.
pub(crate) unsafe fn is_union(types: &Core, t: DyadPtr) -> bool {
    !t.is_null() && meta::is_record_type(t) && meta::mint_of(t) == types.union.union
}

/// Every member's place in a value of the union `ty`, the room after the tag, written onto the
/// member's binding; the tag and the room of the largest member, the union's `size_bytes`, and
/// the widest member's alignment, the union's, as C aligns a union.
///
/// # Safety
/// `ty` must be a type node from the store, `members` its member holes and `bindings` their
/// bindings in the same order.
pub(crate) unsafe fn lay_out(
    types: &Core,
    ty: DyadPtr,
    members: &[DyadPtr],
    bindings: &[DyadPtr],
) -> meta::Extent {
    let (mut room, mut align) = (0, 1);
    for (&member, &binding) in members.iter().zip(bindings) {
        Binding::set_field_offset(binding, ty, TAG_BYTES);
        let t = hole::type_in(member);
        if t == types.void_ {
            continue;
        }
        room = room.max(super::by_copy::place_width(types, t));
        align = align.max(super::read::place_align(types, t));
    }
    meta::Extent { size_bytes: (TAG_BYTES + room) as u64, align }
}

/// The place of the member `member` of the union value `value`, of the union type `union`: a
/// dereference of the room's address, which is checked each time it runs. `value` is a place,
/// whose address is taken, or a value nothing names, which runs to the address of its bytes.
/// `access` says what the read does with the member; a take of a member whose values have no
/// end is a read, there being nothing to give up, and any other is no place but the value made
/// for whoever takes it. A `void` member is no place: its read checks the tag and gives nothing.
///
/// # Safety
/// `value` must be a reduced dyad from the store whose type is `union`, a union type, and
/// `member` one of its member holes; with [`Access::Take`], `value` must be a value made for this
/// read alone.
pub(crate) unsafe fn build_member(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    union: DyadPtr,
    member: DyadPtr,
    access: Access,
) -> DyadPtr {
    use super::drop_model::{teardown_of_type, Teardown};
    let base = base_of(store, types, value, union);
    let tag = tag_of(store, types, union, member);
    let member_type = hole::type_in(member);
    let ends = teardown_of_type(types, member_type) != Teardown::Nothing;
    let access = if access == Access::Take && !ends { Access::Read } else { access };
    let u64_ty = types.numtypes[NumType::U64 as usize];
    let word = store.alloc_blob(u64_ty, &(access as u64).to_ne_bytes());
    if member_type == types.void_ {
        let ops = [base, union, tag, word, types.union.leaf, types.void_];
        return store.alloc_words(types.union.member, &ops);
    }
    // The room is written as the member is, by its gates.
    let room_type = pointer::make_pointer_type(store, types, hole::gated_type_in(member));
    let ops = [base, union, tag, word, types.union.leaf, room_type];
    let room = store.alloc_words(types.union.member, &ops);
    let read = pointer::build_deref(store, types, room, member_type, 0);
    if access == Access::Take {
        store.alloc_words(types.union.take, &[read, types.union.take_leaf, member_type])
    } else {
        read
    }
}

/// `value holds member`: whether the union value `value`, of the union type `union`, holds the
/// member `member`, a `bool` the tag gives.
///
/// # Safety
/// As [`build_member`].
pub(crate) unsafe fn build_holds(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    union: DyadPtr,
    member: DyadPtr,
) -> DyadPtr {
    let base = base_of(store, types, value, union);
    let tag = tag_of(store, types, union, member);
    let ops = [base, union, tag, types.union.asks_leaf, types.bool_];
    store.alloc_words(types.union.asks, &ops)
}

/// What runs to the address of the union value `value`: the address of a place, or a value
/// nothing names, which runs to the address of its bytes.
///
/// # Safety
/// As [`build_member`].
unsafe fn base_of(store: &mut Store, types: &Core, value: DyadPtr, union: DyadPtr) -> DyadPtr {
    if types.is_storage(value) || dyad::ty(value) == types.deref_ {
        pointer::build_address(store, types, types.addr_, value, std::ptr::null_mut(), union)
    } else {
        value
    }
}

/// The tag that says `union` holds `member`, a `u64` literal.
///
/// # Safety
/// `union` must be a union type and `member` one of its member holes.
unsafe fn tag_of(store: &mut Store, types: &Core, union: DyadPtr, member: DyadPtr) -> DyadPtr {
    let members = array::items(meta::record_fields_of(union));
    let at = members.iter().position(|&m| m == member).expect("a member of this union");
    store.alloc_blob(types.numtypes[NumType::U64 as usize], &(at as u64 + 1).to_ne_bytes())
}

/// The place among its members of the member the union value at `at` holds, counted from 0;
/// `None` while it holds none.
///
/// # Safety
/// `at` must be the address of a union value.
pub(crate) unsafe fn held(at: *const u8) -> Option<usize> {
    (tag_at(at) as usize).checked_sub(1)
}

/// The member hole the union value at `at`, of the union type `union`, holds, and the address of
/// its room; `None` while it holds none.
///
/// # Safety
/// `at` must be the address of a value of `union`, a union type.
pub(crate) unsafe fn held_member(union: DyadPtr, at: *const u8) -> Option<(DyadPtr, *const u8)> {
    let members = array::items(meta::record_fields_of(union));
    Some((*members.get(held(at)?)?, at.add(TAG_BYTES)))
}

/// # Safety
/// `node` must be a member node from [`build_member`].
unsafe fn member_parts(node: DyadPtr) -> (DyadPtr, u64, Access) {
    let ops = dyad::value(node) as *const DyadPtr;
    let access = match std::ptr::read_unaligned(dyad::value(*ops.add(3)) as *const u64) {
        0 => Access::Read,
        1 => Access::Write,
        _ => Access::Take,
    };
    (*ops.add(1), tag_in(node), access)
}

/// The tag a member node or an `asks` node carries; both lay out their base, union and tag first.
///
/// # Safety
/// `node` must be a node from [`build_member`] or [`build_holds`].
unsafe fn tag_in(node: DyadPtr) -> u64 {
    let ops = dyad::value(node) as *const DyadPtr;
    std::ptr::read_unaligned(dyad::value(*ops.add(2)) as *const u64)
}

/// # Safety
/// As [`tag_in`].
unsafe fn base_in(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr)
}

/// The tag of the union value at `at`.
///
/// # Safety
/// `at` must be the address of a union value.
unsafe fn tag_at(at: *const u8) -> u64 {
    let mut bytes = [0u8; 8];
    std::ptr::copy_nonoverlapping(at, bytes.as_mut_ptr(), TAG_BYTES);
    u64::from_le_bytes(bytes)
}

/// The room of the union value at `at`, for the member `node` reads: its tag set by a write,
/// which ends the member held before it and empties the room, checked by every other use, and
/// set to hold none by a take. The one step both tiers take.
///
/// # Safety
/// `node` must be a member node from [`build_member`], `at` the address of a value of its union.
unsafe fn enter(rt: &mut Runtime, node: DyadPtr, at: *mut u8) -> Result<i64, RunError> {
    let (union, tag, access) = member_parts(node);
    if at.is_null() {
        return Err(RunError::NullPointer);
    }
    if access == Access::Write {
        if tag_at(at) != tag {
            super::drop_model::end_of_type(rt, union, at as i64)?;
            let room = meta::record_size_of(union) as usize - TAG_BYTES;
            std::ptr::write_bytes(at.add(TAG_BYTES), 0, room);
            std::ptr::copy_nonoverlapping(tag.to_le_bytes().as_ptr(), at, TAG_BYTES);
        }
    } else if tag_at(at) != tag {
        let bindings = array::items(meta::record_field_bindings_of(union));
        let name = |tag: u64| bindings.get(tag as usize - 1).map(|&b| Binding::spelling(b));
        let held = Some(tag_at(at)).filter(|&held| held != 0).and_then(name);
        let read = name(tag).expect("a member node reads a member of its union");
        return Err(RunError::UnionHolds(Box::new((held, read))));
    } else if access == Access::Take {
        std::ptr::write_bytes(at, 0, TAG_BYTES);
    }
    Ok(at.add(TAG_BYTES) as i64)
}

fn run_member(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a member node, whose base runs to the address of a value of its union.
    unsafe {
        let at = rt.run(base_in(node))?;
        enter(rt, node, at as usize as *mut u8)
    }
}

/// Whether the union value at `at` holds the member whose tag the `asks` node `node` carries: the
/// one step both tiers take.
///
/// # Safety
/// `node` must be an `asks` node from [`build_holds`], `at` null or the address of a value of its
/// union.
unsafe fn asks(node: DyadPtr, at: *const u8) -> Result<i64, RunError> {
    if at.is_null() {
        return Err(RunError::NullPointer);
    }
    Ok(i64::from(tag_at(at) == tag_in(node)))
}

fn run_holds(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `asks` node, whose base runs to the address of a value of its union.
    unsafe {
        let at = rt.run(base_in(node))?;
        asks(node, at as usize as *const u8)
    }
}

fn lower_holds(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_holds`].
    unsafe {
        let at = lw.lower(base_in(node))?;
        let node = lw.node_addr(node);
        let held = lw.call_seed(compiled_holds as *const () as usize, &[node, at])?;
        let yes = lw.const_i64(1);
        Ok(lw.icmp_eq(held, yes))
    }
}

/// # Safety
/// Called only by compiled code, with its context, an `asks` node `lower_holds` baked and the
/// address of a value of its union.
unsafe extern "C" fn compiled_holds(ctx: *mut crate::run::Context, node: DyadPtr, at: i64) -> i64 {
    crate::run::interpret_step(ctx, |_| asks(node, at as usize as *const u8))
}

fn lower_member(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_member`].
    unsafe {
        let at = lw.lower(base_in(node))?;
        let node = lw.node_addr(node);
        lw.call_seed(compiled_enter as *const () as usize, &[node, at])
    }
}

/// # Safety
/// Called only by compiled code, with its context, a member node `lower_member` baked and the
/// address of a value of its union.
unsafe extern "C" fn compiled_enter(ctx: *mut crate::run::Context, node: DyadPtr, at: i64) -> i64 {
    crate::run::interpret_step(ctx, |rt| enter(rt, node, at as usize as *mut u8))
}

/// # Safety
/// `node` must be a take node from [`build_member`].
unsafe fn taken_read(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr)
}

fn run_take(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a take node, whose first word reads the member its union gives up.
    unsafe { rt.run(taken_read(node)) }
}

fn lower_take(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_take`].
    unsafe { lw.lower(taken_read(node)) }
}
