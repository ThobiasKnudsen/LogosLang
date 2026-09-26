// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The drop model: `alloc`, `own`, `drop`, `free`, and `defer`, one mechanism in one
//! file. `alloc n of T v` yields an owning `@T` to the first of n cells (a pointer type
//! with a non-null destructor), `alloc n` an `@u8` over n bytes; binding it inserts
//! `defer free <place>`, and `own`/`drop` empty the place to null so
//! a pending teardown no-ops. None of the five lower. DESIGN ›Explicit heap, and no implicit
//! destruction‹.

use crate::Core;
use cranelift_codegen::ir::{types, Value};

use super::callable::{self, Callables};
use super::numtype;
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// `alloc` is `[pointee, count, init, op]`; `init` is null in the byte form.
const ALLOC_POINTEE: usize = 0;
const ALLOC_COUNT: usize = 1;
const ALLOC_INIT: usize = 2;
/// `free`/`drop`/`own` are `[place, pointee, op]`.
const TEARDOWN_PLACE: usize = 0;
const TEARDOWN_POINTEE: usize = 1;
/// `defer` is `[inner, op]`.
const DEFER_INNER: usize = 0;

pub(super) struct DropModel {
    pub alloc_: DyadPtr,
    pub own_: DyadPtr,
    pub drop_: DyadPtr,
    pub free_: DyadPtr,
    pub defer_: DyadPtr,
    /// The word between `alloc`'s count and its value; constructs nothing itself.
    pub of_: DyadPtr,
    /// `free`'s run native, also the owning pointer's stored destructor.
    pub teardown_leaf: DyadPtr,
    pub own_leaf: DyadPtr,
    pub drop_leaf: DyadPtr,
    pub alloc_leaf: DyadPtr,
    pub defer_leaf: DyadPtr,
    pub instance_drop_leaf: DyadPtr,
    pub field_free_leaf: DyadPtr,
}

pub(super) fn register(cx: &mut Cx, cs: &Callables) -> DropModel {
    // Minted once: `free`'s op leaf and the owning pointer's destructor slot both point
    // at it, so `drop` reaches the same code as an inserted `free`.
    let teardown_leaf = callable::mint_native(cx.store, cs.callable, run_teardown, cs.seed_native);

    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
    let of_ = cx.store.alloc_raw(cx.type_, record);
    cx.declare("of", of_);

    let alloc_ = keyword(
        cx,
        "alloc",
        meta::prec::PREFIX,
        &["pointee", "count", "init", "op"],
        |p, _id, tape| {
            let of_ = p.types().of_;
            if p.marker_right(tape, of_) {
                return Err(ParseError::MissingOperand);
            }
            let count = p.take_right(tape)?;
            let init = if p.marker_right(tape, of_) {
                tape.remove(1);
                Some(p.take_right(tape)?)
            } else {
                None
            };
            let types = p.types();
            let node = build_alloc(p.store(), types, count, init)?;
            tape.place(node);
            Ok(crate::parse::Constructed::Placed)
        },
    );
    let alloc_leaf = callable::mint_native(cx.store, cs.callable, run_alloc, cs.seed_native);

    let own_ =
        keyword(cx, "own", meta::prec::PREFIX, &["place", "pointee", "op"], |p, _id, tape| {
            let (place, ended) = p.place_operand_cell(tape, true)?;
            // In a type position, `own @T ?`, the word makes the hole an owning one.
            if p.is_hole(place) {
                own_hole(p, place)?;
                tape.place(place);
                return Ok(crate::parse::Constructed::Placed);
            }
            let types = p.types();
            // SAFETY: `place` is a resolved dyad whose type is a valid type node, and
            // `ended` holds the binding the resolver returned for it.
            let (owner, node_valued, ty) = unsafe {
                (
                    ended.as_ref().is_some_and(|e| p.owns_node(e.binding))
                        || p.is_owning_read(place),
                    meta::is_node_valued((*place).ty, types.fn_type),
                    super::node_type_of(types, place).unwrap_or((*place).ty),
                )
            };
            let node = if owner {
                build_instance_own(p.store(), types, place, ty)
            } else if node_valued && ended.is_some() {
                return Err(ParseError::MoveOfBorrow);
            } else {
                build_teardown(p.store(), types, types.own_, place, true)?
            };
            tape.place(node);
            if let Some(ended) = ended {
                p.mark_dead(ended, node);
            }
            Ok(crate::parse::Constructed::Placed)
        });
    cx.lower.insert(own_, lower_own);
    let own_leaf = callable::mint_native(cx.store, cs.callable, run_own, cs.seed_native);

    // Any identity may be dropped: an owning place gets the teardown node, anything else
    // an inert `drop` node whose work is the parse-time dead mark. `drop = …` never
    // reaches here: `=` constructs first and takes the lone `drop` as the slot's name.
    let drop_ =
        keyword(cx, "drop", meta::prec::PREFIX, &["place", "pointee", "op"], |p, _id, tape| {
            let (place, ended) = p.place_operand_cell(tape, true)?;
            let types = p.types();
            let node = if is_owning_place(place) {
                build_teardown(p.store(), types, types.drop_, place, true)?
            // SAFETY: `ended` holds the binding the resolver returned.
            } else if ended.as_ref().is_some_and(|e| unsafe { p.owns_node(e.binding) })
                || p.is_owning_read(place)
            {
                // SAFETY: an owner's place holds, and an owning field reads, a node of a type
                // whose body fills `drop`.
                let drop = unsafe {
                    meta::instances_drop_of(
                        super::node_type_of(types, place).unwrap_or((*place).ty),
                    )
                };
                build_instance_drop(p.store(), types, place, drop)
            // SAFETY: `place` is a reduced dyad from the store.
            } else if let Some(drop) = unsafe { cell_drop(types, place) } {
                build_instance_drop(p.store(), types, place, drop)
            } else {
                build_inert_drop(p.store(), types, place)
            };
            tape.place(node);
            if let Some(ended) = ended {
                p.mark_dead(ended, node);
            }
            Ok(crate::parse::Constructed::Placed)
        });
    cx.lower.insert(drop_, lower_drop);
    let drop_leaf = callable::mint_native(cx.store, cs.callable, run_drop, cs.seed_native);

    // `free` demands an owning place too: freeing a borrow would hand a stack or global
    // address to the allocator.
    let free_ =
        keyword(cx, "free", meta::prec::PREFIX, &["place", "pointee", "op"], |p, _id, tape| {
            // The raw teardown verb leaves the name alive: only `own`/`drop` end it.
            let (place, _) = p.place_operand_cell(tape, false)?;
            let types = p.types();
            // SAFETY: `place` is a reduced dyad from the store.
            let node = match unsafe { owning_field_pointee(types, place) } {
                Some(pointee) => build_field_free(p.store(), types, place, pointee),
                None => build_teardown(p.store(), types, types.free_, place, true)?,
            };
            tape.place(node);
            Ok(crate::parse::Constructed::Placed)
        });

    // Its run native is a no-op: the scope machinery runs the inner, never the defer node.
    let defer_ = keyword(cx, "defer", meta::prec::READER, &["inner", "op"], |p, _id, tape| {
        let inner = p.parse_expression()?;
        let types = p.types();
        let node = build_defer(p.store(), types, inner);
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    let defer_leaf = callable::mint_native(cx.store, cs.callable, run_defer_noop, cs.seed_native);

    let instance_drop_leaf =
        callable::mint_native(cx.store, cs.callable, run_instance_drop, cs.seed_native);
    let field_free_leaf =
        callable::mint_native(cx.store, cs.callable, run_field_free, cs.seed_native);

    DropModel {
        alloc_,
        own_,
        drop_,
        free_,
        defer_,
        of_,
        teardown_leaf,
        own_leaf,
        drop_leaf,
        alloc_leaf,
        defer_leaf,
        instance_drop_leaf,
        field_free_leaf,
    }
}

fn keyword(
    cx: &mut Cx,
    spelling: &str,
    parse_rank: f64,
    roles: &[&str],
    construct: crate::parse::ConstructFn,
) -> DyadPtr {
    let record = meta::operand_record(cx, meta::TUPLE_TAG, parse_rank, Assoc::Right, roles);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare(spelling, id);
    cx.metas.insert(id, construct);
    id
}

/// A block is `[byte count as u64, pad][cells…]`, the header `HEADER` bytes and the
/// block aligned to it, which is at or above every scalar width: the span's size lives
/// before its first cell, where `free` reads it back.
const HEADER: usize = 16;

fn block_layout(total: usize) -> Option<std::alloc::Layout> {
    std::alloc::Layout::from_size_align(HEADER.checked_add(total)?, HEADER).ok()
}

/// Null when the allocator refuses; the cells start zeroed.
///
/// # Safety
/// The block is freed exactly once by `heap_free` with the returned address.
unsafe fn heap_alloc(total: usize) -> *mut u8 {
    let Some(layout) = block_layout(total) else {
        return std::ptr::null_mut();
    };
    let base = std::alloc::alloc_zeroed(layout);
    if base.is_null() {
        return base;
    }
    (base as *mut u64).write(total as u64);
    base.add(HEADER)
}

/// # Safety
/// `ptr` must be a live address from `heap_alloc`.
unsafe fn heap_free(ptr: *mut u8) {
    let base = ptr.sub(HEADER);
    let total = (base as *const u64).read() as usize;
    let layout = block_layout(total).expect("a live block was laid out once already");
    std::alloc::dealloc(base, layout);
}

/// The pointee is the value's own type (`alloc 2 of i32 5` allocates two `i32`), `u8` with
/// no value; the count must be an integer. A value of a type built by a Logos `parse` takes
/// a cell of its address; any other non-scalar value is rejected.
pub(super) fn build_alloc(
    store: &mut Store,
    types: &Core,
    count: DyadPtr,
    init: Option<DyadPtr>,
) -> Result<DyadPtr, ParseError> {
    use crate::identities::Operand;
    // SAFETY: `count` and `init` are reduced dyads just parsed.
    unsafe {
        match crate::identities::numtype_of(types, count) {
            Operand::Literal => {}
            Operand::Concrete(nt) if !nt.is_float() => {}
            _ => return Err(ParseError::UnsupportedOperands),
        }
    }
    let pointee = match init {
        None => types.numtypes[numtype::NumType::U8 as usize],
        // SAFETY: as above.
        Some(init) => match unsafe { crate::identities::numtype_of(types, init) } {
            Operand::Concrete(_) | Operand::Pointer(_) => {
                // SAFETY: as above; the operand is scalar or pointer, what `scalar_binding_type` takes.
                unsafe { crate::identities::scalar_binding_type(store, types, init).0 }
            }
            // SAFETY: as above.
            _ => unsafe { crate::identities::node_type_of(types, init) }
                .ok_or(ParseError::UnsupportedOperands)?,
        },
    };
    let init = init.unwrap_or(std::ptr::null_mut());
    let value = store.alloc_operands(&[pointee, count, init, types.ops.alloc_]);
    Ok(store.alloc_raw(types.alloc_, value))
}

/// When `require_owning`, the place must carry a non-null destructor: a borrow or a
/// plain value cannot be moved or dropped. `place` must be a reduced dyad from the store.
pub(crate) fn build_teardown(
    store: &mut Store,
    types: &Core,
    op_id: DyadPtr,
    place: DyadPtr,
    require_owning: bool,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `place` is a reduced dyad; its type is a valid type node.
    let logos = unsafe { (*place).ty };
    // SAFETY: `logos` is a type node from the store (above).
    if unsafe { !numtype::is_pointer_type(logos) } {
        return Err(ParseError::BadAssignTarget);
    }
    // SAFETY: a pointer type carries a record, which the destructor slot is in.
    if require_owning && unsafe { meta::destructor_of(logos).is_null() } {
        return Err(ParseError::BadAssignTarget);
    }
    // SAFETY: as above: a pointer type's record holds its pointee.
    let pointee = unsafe { numtype::pointee_of(logos) };
    let leaf = if op_id == types.own_ {
        types.ops.own_
    } else if op_id == types.drop_ {
        types.ops.drop_
    } else {
        types.ops.teardown_
    };
    let value = store.alloc_operands(&[place, pointee, leaf]);
    Ok(store.alloc_raw(op_id, value))
}

/// A pointer type whose `destructor` slot is set, as opposed to a borrow or a plain
/// value. `place` must be a reduced dyad from the store.
pub(crate) fn is_owning_place(place: DyadPtr) -> bool {
    // SAFETY: `place` is a reduced dyad; its type is a valid type node.
    unsafe {
        let logos = (*place).ty;
        numtype::is_pointer_type(logos) && !meta::destructor_of(logos).is_null()
    }
}

/// `own @T ?`: the hole `?` built for a pointer type becomes a place of an owning pointer
/// type, what a field or name declared with it holds (DESIGN ›Memory and concurrency‹).
/// `own t ?`, `t` a type whose body fills `drop`: the hole is marked, and
/// the field or name declared with it owns the node written into it.
fn own_hole(p: &mut crate::parse::Parser, hole: DyadPtr) -> Result<(), ParseError> {
    let types = p.types();
    // SAFETY: `hole` is the place `?` just built; its type is a type node.
    unsafe {
        let ty = (*hole).ty;
        if meta::is_node_valued(ty, types.fn_type) && !meta::instances_drop_of(ty).is_null() {
            p.mark_owning_hole(hole);
            return Ok(());
        }
        if !numtype::is_pointer_type(ty) || !meta::destructor_of(ty).is_null() {
            return Err(ParseError::OwnNeedsPointer);
        }
        let owning = super::pointer::make_owning_pointer_type(
            p.store(),
            types.type_,
            numtype::pointee_of(ty),
            types.ops.teardown_,
        );
        (*hole).ty = owning;
    }
    Ok(())
}

/// `own a` where `a` owns a node: the move reads the node's address and empties the place,
/// as over an owning pointer; the pointee slot holds the node's type.
pub(crate) fn build_instance_own(
    store: &mut Store,
    types: &Core,
    place: DyadPtr,
    ty: DyadPtr,
) -> DyadPtr {
    let value = store.alloc_operands(&[place, ty, types.ops.own_]);
    store.alloc_raw(types.own_, value)
}

/// `drop a` where `a` owns a node: `[place, drop, op]`, the instances' `drop` run over the
/// node the place holds, the place emptied first.
pub(crate) fn build_instance_drop(
    store: &mut Store,
    types: &Core,
    place: DyadPtr,
    drop: DyadPtr,
) -> DyadPtr {
    let value = store.alloc_operands(&[place, drop, types.ops.instance_drop_]);
    store.alloc_raw(types.drop_, value)
}

/// The instances' `drop` of the node a dereference `p@` reads, when its type fills one: the
/// cell holds that node's address, and dropping the cell drops the node. DESIGN ›Explicit
/// heap, and no implicit destruction‹.
///
/// # Safety
/// `place` must be a reduced dyad from the store.
unsafe fn cell_drop(types: &Core, place: DyadPtr) -> Option<DyadPtr> {
    if (*place).ty != types.deref_ {
        return None;
    }
    let (_, pointee, _) = super::pointer::deref_parts(place);
    if !meta::is_node_valued(pointee, types.fn_type) {
        return None;
    }
    let drop = meta::instances_drop_of(pointee);
    (!drop.is_null()).then_some(drop)
}

/// The pointee of a field read where the field is declared `own @T ?`; `None` for any
/// other operand.
///
/// # Safety
/// `place` must be a reduced dyad from the store.
unsafe fn owning_field_pointee(types: &Core, place: DyadPtr) -> Option<DyadPtr> {
    let ty = super::this::load_type(types, place)?;
    (numtype::is_pointer_type(ty) && !meta::destructor_of(ty).is_null())
        .then(|| numtype::pointee_of(ty))
}

/// `free` of an owning field: `[field read, pointee, op]`.
fn build_field_free(store: &mut Store, types: &Core, read: DyadPtr, pointee: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[read, pointee, types.ops.field_free_]);
    store.alloc_raw(types.free_, value)
}

/// `[place, null, op]`: the null pointee marks nothing to run or free. Its work was done
/// at parse, where the name became dead; it stands in the body for reflection.
fn build_inert_drop(store: &mut Store, types: &Core, place: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[place, std::ptr::null_mut(), types.ops.drop_]);
    store.alloc_raw(types.drop_, value)
}

/// The inert form is unit; a node's drop empties the place and hands the node to the seed,
/// which runs the instances' `drop`; the owning pointer's form has no lowering, so the
/// function declines to compile and stays interpreted.
fn lower_drop(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a `drop` node `[place, pointee, op]` or `[place, drop, op]`.
    let (place, pointee, op) = unsafe {
        let slots = (*node).value as *const DyadPtr;
        (*slots, *slots.add(TEARDOWN_POINTEE), *slots.add(2))
    };
    if pointee.is_null() {
        return Ok(lw.const_i32(0));
    }
    if op != lw.types().ops.instance_drop_ {
        return Err(CompileError::NotLowerable(node));
    }
    // SAFETY: `place` is the node place the binding site minted, read at its container width.
    unsafe {
        let this = lw.read_place(place, types::I64)?;
        let empty = lw.const_i64(0);
        lw.write_place(place, types::I64, empty)?;
        let drop = lw.const_i64(pointee as i64);
        Ok(lw.call_seed(compiled_instance_drop as *const () as usize, &[this, drop]))
    }
}

/// # Safety
/// Called only by compiled code, with `drop` the instances' `drop` a node drop was built over.
unsafe extern "C" fn compiled_instance_drop(this: i64, drop: DyadPtr) -> i64 {
    if this == 0 {
        return 0;
    }
    crate::run::interpret_call(drop, 1, &this)
}

/// The move reads the place and empties it, so the pending teardown over it finds nothing.
fn lower_own(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is an `own` node `[place, pointee, op]`; the place holds an address.
    unsafe {
        let place = *((*node).value as *const DyadPtr).add(TEARDOWN_PLACE);
        let held = lw.read_place(place, types::I64)?;
        let empty = lw.const_i64(0);
        lw.write_place(place, types::I64, empty)?;
        Ok(held)
    }
}

pub(crate) fn build_defer(store: &mut Store, types: &Core, inner: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[inner, types.ops.defer_]);
    store.alloc_raw(types.defer_, value)
}

/// # Safety
/// `node` must be a `defer` node from `build_defer`.
pub(crate) unsafe fn deferred_inner_of(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr).add(DEFER_INNER)
}

/// The place an inserted `defer free <place>` frees; the escape check compares a scope's
/// tail against these.
///
/// # Safety
/// `defer_node` must be a `defer` node over a teardown, as the binding site builds.
pub(crate) unsafe fn teardown_place_of(defer_node: DyadPtr) -> DyadPtr {
    let inner = deferred_inner_of(defer_node);
    *((*inner).value as *const DyadPtr).add(TEARDOWN_PLACE)
}

/// What a bound owning pointer points at, so the binding site can mint its owning
/// `@pointee` type; a scope whose tail is one propagates through.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn owning_pointee_of(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    let node = types.through(node);
    let logos = (*node).ty;
    if logos == types.alloc_ {
        Some(*((*node).value as *const DyadPtr).add(ALLOC_POINTEE))
    } else if logos == types.own_ {
        // A move of a node carries the node's type there, and owns no pointer.
        let pointee = *((*node).value as *const DyadPtr).add(TEARDOWN_POINTEE);
        (!meta::is_node_valued(pointee, types.fn_type)).then_some(pointee)
    } else if let Some(output) = moving_call_output(types, node) {
        numtype::is_pointer_type(output).then(|| numtype::pointee_of(output))
    } else if logos == types.scope {
        // A block that yields an owning value moves ownership to the binder.
        crate::parse::last_sequence_expr(node).and_then(|tail| owning_pointee_of(types, tail))
    } else if logos == types.ran_ {
        // An item that ran in the pass owns what its expression owns.
        owning_pointee_of(types, super::ran::expr_of(types, node))
    } else {
        None
    }
}

/// A call whose callee's last value moves out: the callee's declared result.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
unsafe fn moving_call_output(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    use super::read::{read_kind, Dispatch, Read};
    let Read::Executable(Dispatch::Call(f)) = read_kind(types, node) else {
        return None;
    };
    if (*f).ty != types.fn_type || (*f).value.is_null() {
        return None;
    }
    let fields = (*f).value as *const DyadPtr;
    let body = *fields.add(crate::parse::FN_BODY);
    (!body.is_null() && moves_out_within(types, body, 1))
        .then(|| *fields.add(crate::parse::FN_OUTPUT))
}

/// Whether binding `node` makes the binder an owner: a value just constructed (`alloc`, a
/// type's own `parse` placing a call on its fresh node), a move (`own a`), or a block or
/// call whose last value is one of these. A name is a borrow. DESIGN ›Memory and
/// concurrency‹.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn moves_out(types: &Core, node: DyadPtr) -> bool {
    moves_out_within(types, node, 0)
}

/// A recursive function's body calls itself; past this depth the answer is no.
const MOVES_OUT_DEPTH: usize = 32;

/// # Safety
/// As `moves_out`.
unsafe fn moves_out_within(types: &Core, node: DyadPtr, depth: usize) -> bool {
    if depth > MOVES_OUT_DEPTH {
        return false;
    }
    let node = types.through(node);
    let logos = (*node).ty;
    if logos == types.alloc_ || logos == types.own_ || logos == types.this.copy {
        return true;
    }
    if logos == types.scope {
        return crate::parse::last_sequence_expr(node)
            .is_some_and(|tail| moves_out_within(types, tail, depth + 1));
    }
    if logos == types.ran_ {
        return moves_out_within(types, super::ran::expr_of(types, node), depth + 1);
    }
    if let super::read::Read::Executable(super::read::Dispatch::Call(f)) =
        super::read::read_kind(types, node)
    {
        // A call a type's own `parse` placed on its fresh node, yielding that node.
        let args = (*node).value as *const DyadPtr;
        if !args.is_null() && !(*args).is_null() && (**args).ty == types.this.copy {
            let template = *((**args).value as *const DyadPtr);
            let fields = (*f).value as *const DyadPtr;
            return !fields.is_null() && *fields.add(crate::parse::FN_OUTPUT) == (*template).ty;
        }
        if (*f).ty == types.fn_type && !(*f).value.is_null() {
            let body = *((*f).value as *const DyadPtr).add(crate::parse::FN_BODY);
            return !body.is_null() && moves_out_within(types, body, depth + 1);
        }
    }
    false
}

/// The binding-site test: whether `a := <node>` mints an owning place and inserts `defer free a`.
///
/// # Safety
/// As `owning_pointee_of`.
pub(crate) unsafe fn is_owning_value(types: &Core, node: DyadPtr) -> bool {
    owning_pointee_of(types, node).is_some()
}

/// The runtime notes the live allocation so leaks and double frees are observable.
fn run_alloc(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `alloc` node `[pointee, count, init, op]` from the store.
    unsafe {
        let slots = (*node).value as *const DyadPtr;
        let pointee = *slots.add(ALLOC_POINTEE);
        let count = rt.run(*slots.add(ALLOC_COUNT))?;
        if count < 0 {
            return Err(RunError::BadCount(count));
        }
        let init = *slots.add(ALLOC_INIT);
        let bits = if init.is_null() { None } else { Some(rt.run(init)?) };
        let nt = super::read::cell_numtype(rt.types(), pointee).ok_or(RunError::NotDerefable)?;
        let width = nt.bytes();
        let count = count as usize;
        let Some(total) = count.checked_mul(width) else {
            return Err(RunError::OutOfMemory);
        };
        let mem = heap_alloc(total);
        if mem.is_null() {
            return Err(RunError::OutOfMemory);
        }
        if let Some(bits) = bits {
            for k in 0..count {
                numtype::write_scalar_nt(nt, mem.add(k * width), bits);
            }
        }
        rt.note_alloc();
        Ok(mem as i64)
    }
}

/// A null pointer (an emptied place) is the sanctioned no-op; after freeing, the place is
/// nulled so a second teardown over it also no-ops.
fn run_teardown(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[place, pointee, op]` teardown node from the store.
    unsafe {
        let slots = (*node).value as *const DyadPtr;
        let place = *slots.add(TEARDOWN_PLACE);
        let slot = rt.place_addr(place).ok_or(RunError::NoActivation)?;
        if slot.is_null() {
            return Err(RunError::Uninitialized);
        }
        let ptr = std::ptr::read_unaligned(slot as *const i64) as u64 as *mut u8;
        if ptr.is_null() {
            return Ok(0); // emptied place: the sanctioned no-op
        }
        // Tests observe teardown order (LIFO) by the value each freed block held.
        #[cfg(test)]
        let held = numtype::read_scalar(*slots.add(TEARDOWN_POINTEE), ptr);
        #[cfg(test)]
        FREE_LOG.with(|log| log.borrow_mut().push(held));
        heap_free(ptr);
        rt.note_free();
        std::ptr::write_unaligned(slot as *mut i64, 0);
        Ok(0)
    }
}

/// The teardown flows through the reserved `destructor` slot. A null destructor here is a
/// malformed node: `build_teardown` demanded an owning place at parse.
fn run_drop(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `drop` node; in the owning form its place's type carries a
    // destructor whose entry is a `RunFn` over this node's layout.
    unsafe {
        let slots = (*node).value as *const DyadPtr;
        if (*slots.add(TEARDOWN_POINTEE)).is_null() {
            return Ok(0);
        }
        let place = *slots.add(TEARDOWN_PLACE);
        let dtor = meta::destructor_of((*place).ty);
        if dtor.is_null() || !callable::is_callable(dtor) {
            return Err(RunError::NoDestructor(place));
        }
        let entry = std::mem::transmute::<usize, crate::run::RunFn>(callable::entry_of(dtor));
        entry(rt, node)
    }
}

/// The place is emptied before the body runs, so a second teardown over it finds nothing.
fn run_instance_drop(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[place, drop, op]` node from `build_instance_drop`; the place
    // holds a node's address or the null an earlier teardown or move left.
    unsafe {
        let slots = (*node).value as *const DyadPtr;
        let place = *slots.add(TEARDOWN_PLACE);
        let slot = owned_slot(rt, place)?;
        if slot.is_null() {
            return Err(RunError::Uninitialized);
        }
        let this = std::ptr::read_unaligned(slot as *const i64);
        if this == 0 {
            return Ok(0);
        }
        std::ptr::write_unaligned(slot as *mut i64, 0);
        rt.apply_values(*slots.add(TEARDOWN_POINTEE), &[this])?;
        Ok(0)
    }
}

/// An unwritten field or an emptied one frees nothing; after freeing, the field holds null.
fn run_field_free(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[field read, pointee, op]` node from `build_field_free`; the node
    // a filled slot holds keeps the pointer in its eight value bytes.
    unsafe {
        let read = *((*node).value as *const DyadPtr).add(TEARDOWN_PLACE);
        let Some(held) = super::this::field_node(rt, read)? else {
            return Ok(0);
        };
        let bytes = (*held).value as *mut i64;
        let ptr = std::ptr::read_unaligned(bytes) as u64 as *mut u8;
        if ptr.is_null() {
            return Ok(0);
        }
        heap_free(ptr);
        rt.note_free();
        std::ptr::write_unaligned(bytes, 0);
        Ok(0)
    }
}

/// Where an owned value's address lies: a name's place, the cell a dereference reaches, or
/// the slot of an owning field, which holds the node itself.
///
/// # Safety
/// `place` must be the place operand of an `own` or `drop` node.
unsafe fn owned_slot(rt: &mut Runtime, place: DyadPtr) -> Result<*mut u8, RunError> {
    if (*place).ty == rt.types().deref_ {
        return super::pointer::deref_addr(rt, place);
    }
    if super::this::is_field_read(rt.types(), place) {
        return super::this::slot_addr(rt, place);
    }
    rt.place_addr(place).ok_or(RunError::NoActivation)
}

/// A move: the moved-from place's pending `defer free` then no-ops.
fn run_own(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `own` node `[place, pointee, op]` from the store.
    unsafe {
        let place = *((*node).value as *const DyadPtr).add(TEARDOWN_PLACE);
        let slot = owned_slot(rt, place)?;
        if slot.is_null() {
            return Err(RunError::Uninitialized);
        }
        let ptr = std::ptr::read_unaligned(slot as *const i64);
        std::ptr::write_unaligned(slot as *mut i64, 0);
        Ok(ptr)
    }
}

/// The scope machinery and the top-level drain run the inner, never this node; reaching
/// it directly means a defer stood outside any scope.
fn run_defer_noop(_rt: &mut Runtime, _node: DyadPtr) -> Result<i64, RunError> {
    Ok(0)
}

// Test-only log of the value each freed block held, in teardown order.
#[cfg(test)]
thread_local! {
    static FREE_LOG: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identities::Core;
    use crate::parse::{Parser, ResolveError, ScopeStack};
    use crate::regex_trie::RegexTrie;

    /// Parse and run `src`; returns the tail value and the count of still-live heap blocks.
    fn run(src: &str) -> (i64, usize) {
        try_run(src).expect("run")
    }

    fn try_run(src: &str) -> Result<(i64, usize), RunError> {
        FREE_LOG.with(|l| l.borrow_mut().clear());
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = &core;
        let root = {
            let mut p =
                Parser::new(src, &mut store, &mut trie, types, scopes).with_lower(&core.lower);
            p.parse_sequence().expect("parse")
        };
        let mut rt = Runtime::new(&core, &mut store).with_compiler(&core.lower);
        // SAFETY: `root` is the scope just parsed into `store`, which outlives `rt`.
        let bits = unsafe { rt.run(root) }?;
        Ok((bits, rt.live_allocs()))
    }

    #[test]
    fn an_owning_place_takes_only_an_owning_value() {
        assert_eq!(
            parse_err("x := i32 1, mut a := alloc 1 of i32 5, a = &x"),
            ParseError::NonOwningIntoOwning
        );
        assert_eq!(
            parse_err("mut a := alloc 1 of i32 5, b := alloc 1 of i32 6, a = b"),
            ParseError::NonOwningIntoOwning
        );
        assert_eq!(run("x := i32 1, mut p := &x, p = &x, p@").0, 1);
        assert_eq!(run("mut a := alloc 1 of i32 5, a@ = 9, a@"), (9, 0));

        // The displaced block is not freed yet; the leak is pinned as a number.
        assert_eq!(run("mut a := alloc 1 of i32 5, a = alloc 1 of i32 6, a@"), (6, 1));
        assert_eq!(run("mut a := alloc 1 of i32 5, b := alloc 1 of i32 6, a = own b, a@"), (6, 1));
    }

    fn free_log() -> Vec<i64> {
        FREE_LOG.with(|l| l.borrow().clone())
    }

    /// The parse error `src` raises; for the fail-closed paths.
    fn parse_err(src: &str) -> ParseError {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = &core;
        let mut p = Parser::new(src, &mut store, &mut trie, types, scopes);
        p.parse_sequence().expect_err("expected a parse error")
    }

    #[test]
    fn alloc_counts_its_cells_and_fills_them_from_the_value() {
        assert_eq!(run("n := i32 3,\na := alloc n of i32 7,\na@"), (7, 0));
        assert_eq!(run("a := alloc 2 of i32 ?,\na@ = 4,\na@"), (4, 0));
        assert_eq!(run("a := alloc 0 of i32 1,\n9"), (9, 0));
    }

    #[test]
    fn alloc_of_a_bare_count_is_that_many_zeroed_bytes() {
        assert_eq!(run("a := alloc 8,\na@"), (0, 0));
        assert_eq!(run("mut a := alloc 8,\na@ = 200,\na@"), (200, 0));
        // The count-less spelling is gone: `i32 2` is a count, so this is two bytes.
        assert_eq!(run("a := alloc i32 2,\na@"), (0, 0));
    }

    #[test]
    fn alloc_needs_an_integer_count_and_a_value_after_of() {
        assert_eq!(parse_err("a := alloc f64 2 of i32 1"), ParseError::UnsupportedOperands);
        assert_eq!(parse_err("a := alloc 2 of"), ParseError::MissingOperand);
        assert_eq!(parse_err("a := alloc of i32 1"), ParseError::MissingOperand);
        assert!(matches!(
            try_run("n := i32 0 - 1,\na := alloc n of i32 1,\n1"),
            Err(RunError::BadCount(-1))
        ));
    }

    #[test]
    fn alloc_reads_back_and_frees_at_scope_exit() {
        let (v, live) = run("a := alloc 1 of i32 5,\na@");
        assert_eq!(v, 5);
        assert_eq!(live, 0, "scope exit frees the allocation");
    }

    #[test]
    fn an_early_return_runs_the_teardowns_of_every_scope_it_leaves() {
        let f = "f := fn (n := i32 ?) -> i32 ( p := alloc 1 of i32 42, \
                 ( q := alloc 1 of i32 1, for i in 0..n ( if (i == 2) (return p@ + q@) ) ), 0 ),\n";
        assert_eq!(run(&format!("{f}f(5)")), (43, 0));
        assert_eq!(run(&format!("{f}f(1)")), (0, 0));
    }

    #[test]
    fn alloc_inside_a_function_frees_when_the_call_returns() {
        let (v, live) = run("main := fn () -> i32 ( p := alloc 1 of i32 42, p@ ),\nmain()");
        assert_eq!(v, 42);
        assert_eq!(live, 0, "the call's frame scope frees its alloc");
    }

    #[test]
    fn early_drop_does_not_double_free() {
        let (v, live) = run("a := alloc 1 of i32 3,\ndrop a,\n99");
        assert_eq!(v, 99);
        assert_eq!(live, 0, "drop frees once; the deferred free no-ops");
        assert_eq!(free_log(), vec![3], "exactly one free happened");
    }

    #[test]
    fn own_moves_ownership_and_the_source_scope_frees_nothing() {
        let (v, live) = run("a := alloc 1 of i32 7,\nb := own a,\nb@");
        assert_eq!(v, 7);
        assert_eq!(live, 0, "the moved pointer is freed once, through b");
        assert_eq!(free_log(), vec![7], "own does not double-free the source");
    }

    #[test]
    fn own_out_of_an_inner_block_frees_at_the_outer_owner() {
        let (v, live) = run("b := ( a := alloc 1 of i32 8, own a ),\nb@");
        assert_eq!(v, 8);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![8], "freed once, at the outer owner");
    }

    #[test]
    fn teardown_runs_lifo() {
        let (_v, live) = run("a := alloc 1 of i32 10,\nb := alloc 1 of i32 20,\n0");
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![20, 10], "LIFO := last ? allocated frees first");
    }

    #[test]
    fn a_dropped_name_takes_no_later_free() {
        // A later `free a` is a use of a dead name, refused before anything runs.
        assert_eq!(
            parse_err("a := alloc 1 of i32 4,\ndrop a,\nfree a,\n1"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_dead_name_may_be_redeclared_after_own() {
        // LIFO: the new `a` (9) frees before `b` (7).
        let (v, live) = run("a := alloc 1 of i32 7,\nb := own a,\na := alloc 1 of i32 9,\na@ + b@");
        assert_eq!(v, 16);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![9, 7], "fresh place, old teardown a no-op");
    }

    #[test]
    fn a_read_after_own_is_refused() {
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\nb := own a,\na@"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_write_after_own_is_refused() {
        assert_eq!(
            parse_err("mut a := alloc 1 of i32 7,\nb := own a,\na = b"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_pass_after_drop_is_refused_in_the_same_line() {
        // While the line is still parsing the entry's `end` is the `drop` node itself.
        assert_eq!(
            parse_err(
                "h := fn (x := i32 ?, p := @i32 ?) -> i32 ( x ),\na := alloc 1 of i32 1,\nh(drop a, a)"
            ),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_move_inside_a_nested_block_ends_the_outer_name_after_the_block() {
        // Maybe-moved is moved for the name; the run-time null decides whether its teardown fires.
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\nc := i32 1,\nif (c == 1) ( b := own a, b@ ),\na@"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
        let (v, live) =
            run("a := alloc 1 of i32 7,\nc := i32 1,\nif (c == 1) ( b := own a, b@ ),\na := alloc 1 of i32 9,\na@");
        assert_eq!(v, 9);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![7, 9], "b frees at the if's exit, the new a at the end");
    }

    #[test]
    fn a_move_of_an_outer_name_inside_a_loop_body_is_refused() {
        // The next pass would read a dead name.
        assert_eq!(
            parse_err(
                "a := alloc 1 of i32 7,\nmut c := i32 1,\nwhile (c == 1) ( b := own a, c = 0 )"
            ),
            ParseError::OwnOfOuterName
        );
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\nfor i in 0..2 ( b := own a )"),
            ParseError::OwnOfOuterName
        );
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\nfor i in 0..2 ( drop a )"),
            ParseError::OwnOfOuterName
        );
    }

    #[test]
    fn drop_frees_the_name_of_a_plain_value() {
        let (v, live) = run("n := i32 5,\ndrop n,\nn := i32 6,\nn");
        assert_eq!(v, 6);
        assert_eq!(live, 0);
        assert_eq!(
            parse_err("n := i32 5,\ndrop n,\nn + 1"),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
    }

    #[test]
    fn a_dropped_parameter_is_reusable_in_its_body_and_still_compiles() {
        // A parameter is same-level with the body; the inert drop lowers to unit, so the
        // function compiles.
        let (v, live) =
            run("f := fn (n := i32 ?) -> i32 ( drop n, n := i32 4, n ),\nf.compile(),\nf(1)");
        assert_eq!(v, 4);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_move_of_an_outer_name_inside_a_fn_body_is_refused() {
        // A function may own only what its parameters hand it.
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\nf := fn () -> i32 ( b := own a, b@ )"),
            ParseError::OwnOfOuterName
        );
    }

    /// A value whose owning field holds an array, in the array's shape: `bag ()` is a node
    /// whose run makes a new `bagged`, and `bag` with no bracket is `bagged`.
    const BAG: &str = "import ./identities/array.logos, t := array i32, \
        bagged := type ( \
            mut items := own t ?, \
            share drop = ( drop items ), \
            share parse_rank = dyad.parse_rank, \
            share parse = ( tape.is_constructed[0] = true ) ), \
        bag := type ( \
            output_type := type ?, \
            share run = ( mut v := output_type ?, v.items = array i32 [4, 5, 6], own v ), \
            share parse_rank = dyad.parse_rank, \
            share associativity = left, \
            share parse = ( \
                if tape[1]:type == scope ( \
                    tape[0]:type = bag, tape[0].output_type = bagged, tape.remove(1) \
                ) else ( tape[0] = bagged ), \
                tape.is_constructed[0] = true \
            ) ),\n";

    #[test]
    fn an_owning_field_frees_its_array_once_through_the_owner_s_drop() {
        for (tail, want) in [
            ("b := bag (), b.items[1]", 5),
            ("b := bag (), drop b, 2", 2),
            ("f := fn () -> i32 ( b := bag (), b.items[2] ), f() + f()", 12),
            ("l := array bagged [bag (), bag ()], l[1].items[0]", 4),
            ("b := bag (), l := array bagged [own b], l[0].items.size", 3),
            ("b := bag (), x := array i32 [1], b.items = own x, b.items[0]", 1),
            ("mut a := own t ?, a = array i32 [7], a[0]", 7),
        ] {
            assert_eq!(run(&format!("{BAG}{tail}")), (want, 0), "{tail}");
        }
        // The count sees the array: a drop that leaves the field alone leaks its one block.
        let forgetful = BAG.replace("share drop = ( drop items )", "share drop = ( 0 )");
        assert_eq!(run(&format!("{forgetful}b := bag (), 1")), (1, 1));
    }

    #[test]
    fn the_inserted_defer_is_reflectable_graph_structure() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = &core;
        let scope = {
            let mut p =
                Parser::new("a := alloc 1 of i32 5,\n0", &mut store, &mut trie, types, scopes);
            p.parse_sequence().expect("parse")
        };
        // SAFETY: `scope` is a sequence node; its body is an array of exprs.
        unsafe {
            let arr = *((*scope).value as *const DyadPtr);
            let exprs = crate::identities::array::items(arr);
            let defer = exprs.iter().find(|&&e| (*e).ty == core.defer_);
            assert!(defer.is_some(), "an inserted defer node is in the scope body");
            let inner = deferred_inner_of(*defer.unwrap());
            assert_eq!((*inner).ty, core.free_, "it defers a free");
            let _ = crate::reflect::describe(types, *defer.unwrap());
        }
    }

    #[test]
    fn owning_pointer_carries_a_destructor_but_a_borrow_does_not() {
        // Owning-ness rides the node `alloc` mints, not `@T`.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = &core;
        let scope = {
            let mut p =
                Parser::new("a := alloc 1 of i32 5,\na@", &mut store, &mut trie, types, scopes);
            p.parse_sequence().expect("parse")
        };
        // SAFETY: the scope body holds the inserted `defer free a`, whose place slot is `a`.
        unsafe {
            let arr = *((*scope).value as *const DyadPtr);
            let exprs = crate::identities::array::items(arr);
            let defer = *exprs
                .iter()
                .find(|&&e| (*e).ty == core.defer_)
                .expect("the binding inserted a defer");
            let a = teardown_place_of(defer);
            assert!(numtype::is_pointer_type((*a).ty), "a is a pointer place");
            assert!(
                !meta::destructor_of((*a).ty).is_null(),
                "an owning pointer's logos carries the destructor"
            );
        }
    }

    #[test]
    fn an_unbound_owning_temporary_is_rejected_not_leaked() {
        // An owning value handed straight to a call has no name to hang its `free` on.
        assert_eq!(
            parse_err("f := fn (p := @i32 ?) -> i32 ( p@ ),\nf(alloc 1 of i32 5)"),
            ParseError::UnboundOwningValue
        );
    }

    #[test]
    fn an_own_hole_frees_what_is_written_into_it() {
        assert_eq!(run("mut a := own @i32 ?,\na = alloc 1 of i32 5,\na@"), (5, 0));
        assert_eq!(free_log(), vec![5]);
        assert_eq!(run("mut a := own @i32 ?,\n1"), (1, 0), "an unwritten owner frees nothing");
        assert_eq!(
            parse_err("mut a := own @i32 ?,\nb := alloc 1 of i32 6,\na = b"),
            ParseError::NonOwningIntoOwning
        );
        assert_eq!(parse_err("a := own i32 ?"), ParseError::OwnNeedsPointer);
        assert_eq!(
            parse_err("f := fn (p := own @i32 ?) -> i32 ( p@ )"),
            ParseError::OwnParameterNotInSeed
        );
    }

    #[test]
    fn a_returned_owned_place_is_refused() {
        // The scope's `defer free` would run on the way out and hand back a freed pointer.
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( p := alloc 1 of i32 7, return p ),\nmk()"),
            ParseError::OwningEscape
        );
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( return alloc 1 of i32 7 ),\nmk()"),
            ParseError::OwnershipAcrossReturn
        );
    }

    #[test]
    fn a_function_s_last_value_moves_ownership_to_the_caller() {
        for mk in [
            "mk := fn () -> @i32 ( p := alloc 1 of i32 7, p )",
            "mk := fn () -> @i32 ( p := alloc 1 of i32 7, own p )",
            "mk := fn () -> @i32 ( alloc 1 of i32 7 )",
        ] {
            assert_eq!(run(&format!("{mk},\nm := mk(),\nm@")), (7, 0), "{mk}");
            assert_eq!(free_log(), vec![7], "{mk}: freed once, by the caller's owner");
        }
        // A block's last value moves to its binder the same way.
        assert_eq!(run("b := ( a := alloc 1 of i32 8, a ),\nb@"), (8, 0));
        assert_eq!(free_log(), vec![8]);
        // A moved-out value no name takes is not freed yet; the leak is pinned as a number.
        assert_eq!(run("mk := fn () -> @i32 ( alloc 1 of i32 7 ),\nmk(),\n1"), (1, 1));
        // A call's result is owned only when the callee's last value moves out.
        assert_eq!(
            run("id := fn (q := @i32 ?) -> @i32 ( q ),\np := alloc 1 of i32 3,\nr := id(p),\nr@"),
            (3, 0)
        );
        assert_eq!(free_log(), vec![3], "the borrow `r` frees nothing");
    }

    #[test]
    fn a_call_is_a_use_of_the_outer_names_the_body_reads() {
        // Calling a function is a use of each outer name its body reads, at the call.
        assert_eq!(
            parse_err(
                "mut n := i32 0,\nclimb := fn () -> i32 ( n = n + 1, n ),\nclimb(),\ndrop n,\nclimb()"
            ),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        assert_eq!(
            parse_err(
                "mut n := i32 0,\nclimb := fn () -> i32 ( n ),\ndrop n,\nmut n := i32 10,\nclimb()"
            ),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        let (v, live) =
            run("mut n := i32 0,\nclimb := fn () -> i32 ( n = n + 1, n ),\nclimb(),\ndrop n,\n\
             mut n := i32 10,\nclimb2 := fn () -> i32 ( n = n + 1, n ),\nclimb2()");
        assert_eq!(v, 11);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_call_reading_a_dropped_owning_pointer_is_refused() {
        // A dropped name's place may hold anything, here a freed block.
        assert_eq!(
            parse_err("p := alloc 1 of i32 5,\nf := fn () -> i32 ( p@ ),\ndrop p,\nf()"),
            ParseError::Resolve(ResolveError::Dead("p".into()))
        );
        let (v, live) = run(
            "p := alloc 1 of i32 5,\nf := fn (q := @i32 ?) -> i32 ( q@ ),\nr := alloc 1 of i32 4,\ndrop p,\nf(r)",
        );
        assert_eq!(v, 4);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_call_is_a_use_of_what_its_callees_read() {
        // A callee's outer names join the caller's list.
        assert_eq!(
            parse_err(
                "n := i32 0,\nclimb := fn () -> i32 ( n ),\ng := fn () -> i32 ( climb() ),\n\
                 drop n,\ng()"
            ),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        assert_eq!(
            parse_err(
                "n := i32 1,\nouter := fn () -> i32 ( inner := fn () -> i32 ( n ), inner() ),\n\
                 drop n,\nouter()"
            ),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
    }

    #[test]
    fn a_node_of_a_run_type_is_a_use_of_every_name_its_body_reads() {
        const POW: &str = "n := i32 2,\n\
            ^ := type (\n\
                a := i32 ?, b := i32 ?, output_type := type ?, share run = ( a * b * n ),\n\
                share parse_rank = *.parse_rank + 1,\n\
                share associativity = right,\n\
                share parse = (\n\
                    tape[0]:type = ^, tape[0].a = tape[-1],\n\
                    tape[0].b = tape[1],\n\
                    tape[0].output_type = i32,\n\
                    ,\n\
                    tape.is_constructed[0] = true,\n\
                    tape.remove(1),\n\
                    tape.remove(-1)\n\
                )\n\
            ),\n";
        let (v, live) = run(&format!("{POW}2 ^ 3"));
        assert_eq!(v, 12);
        assert_eq!(live, 0);
        // Whether the body is constructed after the drop or was built before it.
        assert_eq!(
            parse_err(&format!("{POW}drop n,\n2 ^ 3")),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        assert_eq!(
            parse_err(&format!("{POW}2 ^ 3,\ndrop n,\n2 ^ 3")),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
    }

    #[test]
    fn own_hands_ownership_to_an_enclosing_binder() {
        let (v, live) = run("b := ( a := alloc 1 of i32 8, own a ),\nb@");
        assert_eq!(v, 8);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![8], "freed once, by the outer owner");
    }

    #[test]
    fn a_borrow_may_still_be_handed_out_as_a_scope_value() {
        // Only places the scope itself frees are owned; a borrow carries no destructor.
        let (v, live) = run("x := i32 9,\nr := ( &x ),\nr@");
        assert_eq!(v, 9);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_loop_body_frees_every_iteration() {
        let (_v, live) =
            run("mut i := i32 0,\nwhile (i < 3) ( p := alloc 1 of i32 5, i = i + 1 ),\ni");
        assert_eq!(live, 0, "no allocation outlives its iteration");
        assert_eq!(free_log().len(), 3, "one free per iteration");
    }

    #[test]
    fn the_compiled_tier_reads_heap_memory_identically() {
        let (v, live) = run(
            "f := fn (p := @i32 ?) -> i32 ( p@ + 1 ),\na := alloc 1 of i32 41,\nb := f(a),\nf.compile(),\nc := f(a),\nb + c",
        );
        assert_eq!(v, 84, "interpreted and compiled reads agree");
        assert_eq!(live, 0);
    }

    #[test]
    fn a_function_with_a_heap_path_declines_to_compile() {
        // Heap paths have no lowering, so the function stays interpreted, never miscompiled.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = &core;
        let src = "main := fn () -> i32 ( p := alloc 1 of i32 5, p@ ),\nmain.compile()";
        let root = {
            let mut p =
                Parser::new(src, &mut store, &mut trie, types, scopes).with_lower(&core.lower);
            p.parse_sequence().expect("parse")
        };
        let mut rt = Runtime::new(&core, &mut store).with_compiler(&core.lower);
        // SAFETY: `root` is the script just parsed into `store`.
        let result = unsafe { rt.run(root) };
        assert!(
            matches!(result, Err(RunError::CompileFailed(_))),
            "compiling a heap function declines (deopt), got {result:?}"
        );
    }
}
