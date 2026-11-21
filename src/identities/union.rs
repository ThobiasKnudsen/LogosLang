// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `union ( a := T, b := U )`: a type whose value holds one of its members at a time. A value
//! is `[tag: TAG_BYTES][room of the largest member]`; the tag is 0 while it holds none, else
//! the held member's place among the members, counted from 1. A member read `v.a` is a
//! dereference of a `member` node `{type: member, value: [base, union, tag, writes, op,
//! output_type]}`, which runs to the room's address: a write sets the tag, every other use
//! checks it. ›A union holds one of its members at a time: `union ( a := T, b := U )`‹ [[R379]]

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
        &["base", "union", "tag", "writes", "op", "output_type"],
    );
    let member = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(member, lower_member);
    let leaf = callable::mint_native(cx.store, cs.callable, run_member, cs.seed_native);
    UnionIds { union, member, leaf }
}

/// Whether `t` is a union type.
///
/// # Safety
/// `t` must be null or a type node from the store.
pub(crate) unsafe fn is_union(types: &Core, t: DyadPtr) -> bool {
    !t.is_null() && meta::is_record_type(t) && meta::mint_of(t) == types.union.union
}

/// Every member's place in a value of the union `ty`, the room after the tag, written onto the
/// member's binding; the tag and the room of the largest member, the union's `size_bytes`.
///
/// # Safety
/// `ty` must be a type node from the store, `members` its member holes and `bindings` their
/// bindings in the same order.
pub(crate) unsafe fn lay_out(
    types: &Core,
    ty: DyadPtr,
    members: &[DyadPtr],
    bindings: &[DyadPtr],
) -> u64 {
    let mut room = 0;
    for (&member, &binding) in members.iter().zip(bindings) {
        Binding::set_field_offset(binding, ty, TAG_BYTES);
        room = room.max(super::by_copy::place_width(types, hole::type_in(member)));
    }
    (TAG_BYTES + room) as u64
}

/// The place of the member `member` of the union value `value`, of the union type `union`: a
/// dereference of the room's address, which is checked each time it runs. `value` is a place,
/// whose address is taken, or a value nothing names, which runs to the address of its bytes.
/// With `writes`, the read is the left side of `=`, which makes the union hold the member.
///
/// # Safety
/// `value` must be a reduced dyad from the store whose type is `union`, a union type, and
/// `member` one of its member holes.
pub(crate) unsafe fn build_member(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    union: DyadPtr,
    member: DyadPtr,
    writes: bool,
) -> DyadPtr {
    let members = array::items(meta::record_fields_of(union));
    let at = members.iter().position(|&m| m == member).expect("a member of this union");
    let is_place = types.is_storage(value) || dyad::ty(value) == types.deref_;
    let base = if is_place {
        pointer::build_address(store, types, types.addr_, value, std::ptr::null_mut(), union)
    } else {
        value
    };
    let u64_ty = types.numtypes[NumType::U64 as usize];
    let tag = store.alloc_blob(u64_ty, &(at as u64 + 1).to_ne_bytes());
    let writes = store.alloc_blob(u64_ty, &u64::from(writes).to_ne_bytes());
    let member_type = hole::type_in(member);
    // The room is written as the member is, by its gates.
    let room_type = pointer::make_pointer_type(store, types, hole::gated_type_in(member));
    let ops = [base, union, tag, writes, types.union.leaf, room_type];
    let room = store.alloc_words(types.union.member, &ops);
    pointer::build_deref(store, types, room, member_type, 0)
}

/// # Safety
/// `node` must be a member node from [`build_member`].
unsafe fn member_parts(node: DyadPtr) -> (DyadPtr, DyadPtr, u64, bool) {
    let ops = dyad::value(node) as *const DyadPtr;
    let word = |k: usize| std::ptr::read_unaligned(dyad::value(*ops.add(k)) as *const u64);
    (*ops, *ops.add(1), word(2), word(3) != 0)
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
/// checked by every other use. The one step both tiers take.
///
/// # Safety
/// `node` must be a member node from [`build_member`], `at` the address of a value of its union.
unsafe fn enter(node: DyadPtr, at: *mut u8) -> Result<i64, RunError> {
    let (_, union, tag, writes) = member_parts(node);
    if at.is_null() {
        return Err(RunError::NullPointer);
    }
    if writes {
        std::ptr::copy_nonoverlapping(tag.to_le_bytes().as_ptr(), at, TAG_BYTES);
    } else if tag_at(at) != tag {
        let bindings = array::items(meta::record_field_bindings_of(union));
        let name = |tag: u64| bindings.get(tag as usize - 1).map(|&b| Binding::spelling(b));
        let held = Some(tag_at(at)).filter(|&held| held != 0).and_then(name);
        let read = name(tag).expect("a member node reads a member of its union");
        return Err(RunError::UnionHolds(Box::new((held, read))));
    }
    Ok(at.add(TAG_BYTES) as i64)
}

fn run_member(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a member node, whose base runs to the address of a value of its union.
    unsafe {
        let at = rt.run(member_parts(node).0)?;
        enter(node, at as usize as *mut u8)
    }
}

fn lower_member(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_member`].
    unsafe {
        let at = lw.lower(member_parts(node).0)?;
        let node = lw.node_addr(node);
        lw.call_seed(compiled_enter as *const () as usize, &[node, at])
    }
}

/// # Safety
/// Called only by compiled code, with its context, a member node `lower_member` baked and the
/// address of a value of its union.
unsafe extern "C" fn compiled_enter(ctx: *mut crate::run::Context, node: DyadPtr, at: i64) -> i64 {
    crate::run::interpret_step(ctx, |_| enter(node, at as usize as *mut u8))
}
