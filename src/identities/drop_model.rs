// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The drop model: `alloc`, `own`, `move`, `free`, `defer`, and what a scope's end runs,
//! one mechanism in one file. `alloc n of T v` yields an owning `@T` to the first of n
//! cells (a pointer type with a non-null destructor), `alloc n` an `@u8` over n bytes. A
//! name bound to a value with a teardown is held by its scope, whose end runs the teardown
//! read off the place's type (DESIGN ›A value's teardown runs where its life ends‹).
//! `alloc` and a block's `free` keep a function interpreted.

use crate::Core;
use cranelift_codegen::ir::{types, Value};

use super::callable::{self, Callables};
use super::numtype;
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError, Taken};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// `alloc` is `[pointee, count, init, op]`; `init` is null in the byte form.
const ALLOC_POINTEE: usize = 0;
const ALLOC_COUNT: usize = 1;
const ALLOC_INIT: usize = 2;
/// `free` and `move` are `[place, pointee, op]`.
const TEARDOWN_PLACE: usize = 0;
const TEARDOWN_POINTEE: usize = 1;
/// `defer` is `[inner, op]`.
const DEFER_INNER: usize = 0;

pub(super) struct DropModel {
    pub alloc_: DyadPtr,
    pub own_: DyadPtr,
    pub move_: DyadPtr,
    pub free_: DyadPtr,
    pub defer_: DyadPtr,
    /// The type of a scope's exit items; never spelled.
    pub exit_item: DyadPtr,
    /// The type of an `=`'s right side over a place that holds; never spelled.
    pub displace_: DyadPtr,
    pub displace_leaf: DyadPtr,
    /// The word between `alloc`'s count and its value; constructs nothing itself.
    pub of_: DyadPtr,
    /// The owning pointer's stored destructor.
    pub teardown_leaf: DyadPtr,
    pub move_leaf: DyadPtr,
    pub free_leaf: DyadPtr,
    pub alloc_leaf: DyadPtr,
    pub defer_leaf: DyadPtr,
    pub held_free_leaf: DyadPtr,
    pub instance_free_leaf: DyadPtr,
    pub field_free_leaf: DyadPtr,
    pub value_free_leaf: DyadPtr,
    pub value_release_leaf: DyadPtr,
}

pub(super) fn register(cx: &mut Cx, cs: &Callables) -> DropModel {
    // The owning pointer's destructor slot points at it; `free` reaches it through the slot.
    let teardown_leaf = callable::mint_native(cx.store, cs.callable, run_teardown, cs.seed_native);

    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
    let of_ = cx.store.alloc_head(cx.type_, record);
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

    // `own` stands only in a type position, `own @T ?`: the word makes the hole an owning one.
    let own_ = super::gate::word(cx, "own");
    cx.metas.insert(own_, |p, _id, tape| {
        let Some(&hole) = tape.at(1) else {
            return Err(ParseError::MissingOperand);
        };
        if !hole.hole {
            return Err(ParseError::OwnOutsideType);
        }
        tape.remove(1);
        let owning = own_hole(p, hole.dyad)?;
        tape.place(hole.dyad);
        let cell = tape.at_mut(0).expect("placed above");
        cell.hole = true;
        cell.owning = owning;
        Ok(crate::parse::Constructed::Placed)
    });

    let move_ =
        keyword(cx, "move", meta::prec::PREFIX, &["place", "pointee", "op"], |p, _id, tape| {
            let (place, ended) = match p.place_operand_cell(tape)? {
                Taken::Place(place, ended) => (place, ended),
                Taken::Value(_, at) | Taken::Hole(at) => {
                    return Err(p.fail_at(at, ParseError::MoveOfValue))
                }
            };
            let types = p.types();
            // SAFETY: `place` is a resolved dyad whose type is a valid type node.
            if unsafe { super::this::is_plain(types, types.type_of(place)) } {
                return Err(ParseError::RecordMoveNotInSeed);
            }
            // A name moves whatever it holds, a borrowed node's name excepted: it holds none.
            // A field or cell moves only what it owns, and is emptied: stand-in for #66.
            // SAFETY: as above, and `ended` holds the binding the resolver returned for it.
            let (owner, node_valued) = unsafe {
                (
                    ended.as_ref().is_some_and(|e| p.owns_node(e.binding))
                        || p.is_owning_read(place)
                        || is_owning_place(types, place),
                    meta::is_node_valued(types.type_of(place), types.fn_type),
                )
            };
            if !owner && node_valued && ended.is_some() {
                return Err(ParseError::MoveOfBorrow);
            }
            if !owner && ended.is_none() {
                return Err(ParseError::MoveOfUnownedPath);
            }
            // SAFETY: as above.
            let node = unsafe { build_move(p.store(), types, place) };
            tape.place(node);
            if let Some(ended) = ended {
                p.mark_dead(ended, node);
            }
            Ok(crate::parse::Constructed::Placed)
        });
    cx.lower.insert(move_, lower_move);
    let move_leaf = callable::mint_native(cx.store, cs.callable, run_move, cs.seed_native);

    // A place: a name that holds its value, an owning field or an owning cell gets its
    // teardown, and a name, field path or cell whose type fills no `free` the inert `free`. A
    // value no name holds runs, then its type's `free`. `free = …` never reaches here: `=`
    // constructs first and takes the lone `free` as the slot's name.
    let free_ =
        keyword(cx, "free", meta::prec::PREFIX, &["place", "pointee", "op"], |p, _id, tape| {
            let (place, ended) = match p.place_operand_cell(tape)? {
                Taken::Place(place, ended) => (place, ended),
                Taken::Hole(at) => return Err(p.fail_at(at, ParseError::FreeOfHole)),
                Taken::Value(value, at) => {
                    let types = p.types();
                    // SAFETY: `value` is a reduced dyad from the store.
                    let Some(teardown) = (unsafe { teardown_of(types, value) }) else {
                        return Err(p.fail_at(at, ParseError::FreeOfUntypedValue));
                    };
                    let node = build_value_free(p.store(), types, value, teardown);
                    tape.place(node);
                    return Ok(crate::parse::Constructed::Placed);
                }
            };
            // SAFETY: `ended` holds the binding the resolver returned.
            let holder = ended.as_ref().is_some_and(|e| unsafe { p.name_holds(e.binding) });
            // SAFETY: `place` is a reduced dyad from the store.
            let node = unsafe { build_place_free(p, place, holder) }?;
            tape.place(node);
            if let Some(ended) = ended {
                p.mark_dead(ended, node);
            }
            Ok(crate::parse::Constructed::Placed)
        });
    cx.lower.insert(free_, lower_free);
    let free_leaf = callable::mint_native(cx.store, cs.callable, run_free, cs.seed_native);

    // Its run native is a no-op: a scope's end runs the inner, never the defer node.
    let defer_ = keyword(cx, "defer", meta::prec::READER, &["inner", "op"], |p, _id, tape| {
        let inner = p.parse_expression()?;
        let types = p.types();
        let node = build_defer(p.store(), types, inner);
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    let defer_leaf = callable::mint_native(cx.store, cs.callable, run_defer_noop, cs.seed_native);

    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["what", "from", "end", "op"],
    );
    let exit_item = cx.store.alloc_head(cx.type_, record);

    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["value", "free", "op"],
    );
    let displace_ = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(displace_, lower_displace);
    let displace_leaf = callable::mint_native(cx.store, cs.callable, run_displace, cs.seed_native);

    let held_free_leaf =
        callable::mint_native(cx.store, cs.callable, run_held_free, cs.seed_native);
    let instance_free_leaf =
        callable::mint_native(cx.store, cs.callable, run_instance_free, cs.seed_native);
    let field_free_leaf =
        callable::mint_native(cx.store, cs.callable, run_field_free, cs.seed_native);
    let value_free_leaf =
        callable::mint_native(cx.store, cs.callable, run_value_free, cs.seed_native);
    let value_release_leaf =
        callable::mint_native(cx.store, cs.callable, run_value_release, cs.seed_native);

    DropModel {
        alloc_,
        own_,
        move_,
        free_,
        defer_,
        exit_item,
        displace_,
        displace_leaf,
        of_,
        teardown_leaf,
        move_leaf,
        free_leaf,
        alloc_leaf,
        defer_leaf,
        held_free_leaf,
        instance_free_leaf,
        field_free_leaf,
        value_free_leaf,
        value_release_leaf,
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
    let id = cx.store.alloc_head(cx.type_, record);
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
        Some(init) if unsafe { types.is_hole(init) } => unsafe { super::hole::type_in(init) },
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
    // `T ?`, the valueless marker, names the cells' type and fills them with nothing.
    // SAFETY: `init` is a reduced dyad just parsed.
    let init = init.filter(|&i| unsafe { !types.is_hole(i) }).unwrap_or(std::ptr::null_mut());
    Ok(store.alloc_words(types.alloc_, &[pointee, count, init, types.ops.alloc_]))
}

/// `free` over an owning pointer reached through a field or cell: `[place, pointee, op]`.
///
/// # Safety
/// `place` must be a reduced dyad from the store whose type is an owning pointer type.
unsafe fn build_block_free(store: &mut Store, types: &Core, place: DyadPtr) -> DyadPtr {
    let pointee = numtype::pointee_of(types.type_of(place));
    store.alloc_words(types.free_, &[place, pointee, types.ops.free_])
}

/// A pointer type whose `destructor` slot is set, as opposed to a borrow or a plain
/// value. `place` must be a reduced dyad from the store.
pub(crate) fn is_owning_place(types: &Core, place: DyadPtr) -> bool {
    // SAFETY: `place` is a reduced dyad; its type is a valid type node.
    unsafe {
        let logos = types.type_of(place);
        !logos.is_null() && numtype::is_pointer_type(logos) && !meta::destructor_of(logos).is_null()
    }
}

/// `own @T ?`: the hole `?` built for a pointer type becomes a place of an owning pointer
/// type, what a field or name declared with it holds (DESIGN ›Memory and concurrency‹).
/// `own t ?`, `t` a type whose body fills `free`: the hole becomes an owning one (`true`),
/// and the field or name declared with it owns the node written into it.
fn own_hole(p: &mut crate::parse::Parser, hole: DyadPtr) -> Result<bool, ParseError> {
    let types = p.types();
    // SAFETY: `hole` is the place `?` just built; its type is a type node.
    unsafe {
        let ty = super::hole::type_in(hole);
        if meta::is_node_valued(ty, types.fn_type) && !meta::instances_free_of(ty).is_null() {
            return Ok(true);
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
        super::hole::set_type(hole, owning);
    }
    Ok(false)
}

/// `move` of a place: `[place, type, op]`, `type` the moved value's, from which the name
/// that takes it learns whether it holds it.
///
/// # Safety
/// `place` must be a reduced dyad from the store.
pub(crate) unsafe fn build_move(store: &mut Store, types: &Core, place: DyadPtr) -> DyadPtr {
    let ty = super::node_type_of(types, place).unwrap_or(types.type_of(place));
    store.alloc_words(types.move_, &[place, ty, types.ops.move_])
}

/// What `free` of the place runs: a name that holds its value (`holder`) its teardown, an
/// owning field or cell the teardown of what it holds, emptying it, and anything else the
/// inert `free`.
///
/// # Safety
/// `place` must be a reduced dyad from the store.
pub(crate) unsafe fn build_place_free(
    p: &mut crate::parse::Parser,
    place: DyadPtr,
    holder: bool,
) -> Result<DyadPtr, ParseError> {
    let types = p.types();
    Ok(if let Some(pointee) = owning_field_pointee(types, place) {
        build_field_free(p.store(), types, place, pointee)
    } else if holder {
        build_held_free(p.store(), types, place)
    } else if is_owning_place(types, place) {
        build_block_free(p.store(), types, place)
    } else if p.is_owning_read(place) {
        // An owning field reads a node of a type whose body fills `free`.
        let free = meta::instances_free_of(
            super::node_type_of(types, place).unwrap_or(types.type_of(place)),
        );
        build_instance_free(p.store(), types, place, free)
    } else if let Some(free) = cell_free(types, place) {
        build_instance_free(p.store(), types, place, free)
    } else {
        build_inert_free(p.store(), types, place)
    })
}

/// `=` over a place that holds its value, a name or an owning field: the free of the value it
/// displaces, `None` over any other place.
///
/// # Safety
/// `target` must be the reduced dyad `=` writes.
pub(crate) unsafe fn displaced_free(
    p: &mut crate::parse::Parser,
    target: DyadPtr,
) -> Result<Option<DyadPtr>, ParseError> {
    let types = p.types();
    let named = dyad::ty(target) == types.binding_ && p.name_holds(target);
    if !named && owning_field_pointee(types, target).is_none() && !p.is_owning_read(target) {
        return Ok(None);
    }
    build_place_free(p, target, named).map(Some)
}

/// The right side of an `=` over a place that holds: `[value, free, op]` builds the value,
/// runs the free of the value it displaces, and yields the value for the write.
pub(crate) fn build_displace(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    free: DyadPtr,
) -> DyadPtr {
    store.alloc_words(types.displace_, &[value, free, types.ops.displace_])
}

fn run_displace(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[value, free, op]` node from `build_displace`.
    unsafe {
        let slots = dyad::value(node) as *const DyadPtr;
        let value = rt.run(*slots)?;
        rt.run(*slots.add(1))?;
        Ok(value)
    }
}

fn lower_displace(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as `run_displace`.
    unsafe {
        let slots = dyad::value(node) as *const DyadPtr;
        let value = lw.lower(*slots)?;
        lw.lower(*slots.add(1))?;
        Ok(value)
    }
}

/// `free a` where the name `a` holds its value: `[place, null, op]`, the teardown the scope's
/// end would have run.
fn build_held_free(store: &mut Store, types: &Core, place: DyadPtr) -> DyadPtr {
    store.alloc_words(types.free_, &[place, std::ptr::null_mut(), types.ops.held_free_])
}

/// `free` of an owning field's node or a cell's: `[place, free, op]`, the instances' `free`
/// run over the node the slot holds, the slot emptied first.
pub(crate) fn build_instance_free(
    store: &mut Store,
    types: &Core,
    place: DyadPtr,
    free: DyadPtr,
) -> DyadPtr {
    store.alloc_words(types.free_, &[place, free, types.ops.instance_free_])
}

/// The instances' `free` of the node a dereference `p@` reads, when its type fills one: the
/// cell holds that node's address, and freeing the cell frees the node. DESIGN ›A value
/// owns what its elements hold and frees it‹.
///
/// # Safety
/// `place` must be a reduced dyad from the store.
unsafe fn cell_free(types: &Core, place: DyadPtr) -> Option<DyadPtr> {
    if dyad::ty(place) != types.deref_ {
        return None;
    }
    let (_, pointee, _) = super::pointer::deref_parts(place);
    if !meta::is_node_valued(pointee, types.fn_type) {
        return None;
    }
    let free = meta::instances_free_of(pointee);
    (!free.is_null()).then_some(free)
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
    store.alloc_words(types.free_, &[read, pointee, types.ops.field_free_])
}

/// `free` of a value no name holds: `[value, free, op]` runs the value, then `free` over
/// the node it makes, a null `free` nothing more; `[value, pointee, op]` frees the block
/// an owning pointer value holds.
fn build_value_free(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    teardown: Teardown,
) -> DyadPtr {
    let (what, op) = match teardown {
        Teardown::Nothing => (std::ptr::null_mut(), types.ops.value_free_),
        Teardown::Node(free) => (free, types.ops.value_free_),
        Teardown::Block(pointee) => (pointee, types.ops.value_release_),
    };
    store.alloc_words(types.free_, &[value, what, op])
}

/// `[place, null, op]`: the null pointee marks nothing to free; the run still reaches the
/// place, the code that finds a cell included. A name's end was done at parse, where it
/// became dead.
fn build_inert_free(store: &mut Store, types: &Core, place: DyadPtr) -> DyadPtr {
    store.alloc_words(types.free_, &[place, std::ptr::null_mut(), types.ops.free_])
}

/// A node's `free` runs in the seed. A cell's or field's free empties a slot no frame holds,
/// so a function holding one stays interpreted: stand-in for #164.
fn lower_free(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a `free` node `[place, pointee, op]`, `[place, free, op]` or
    // `[value, free, op]`.
    let (place, pointee, op) = unsafe {
        let slots = dyad::value(node) as *const DyadPtr;
        (*slots, *slots.add(TEARDOWN_POINTEE), *slots.add(2))
    };
    if op == lw.types().ops.value_free_ {
        // SAFETY: the value is a reduced dyad from the store.
        let this = unsafe { lw.lower(place) }?;
        if pointee.is_null() {
            return Ok(lw.const_i32(0));
        }
        let free = lw.node_addr(pointee);
        return Ok(lw.call_seed(compiled_instance_free as *const () as usize, &[this, free]));
    }
    if op == lw.types().ops.held_free_ {
        // SAFETY: `place` is a name the binding site made an owner.
        unsafe { lower_end_held(lw, place) }?;
        return Ok(lw.const_i32(0));
    }
    if !pointee.is_null() {
        return Err(CompileError::NotLowerable(node));
    }
    // SAFETY: `place` is a reduced dyad from the store; a deref node's parts are valid.
    unsafe {
        if dyad::ty(place) == lw.types().deref_ {
            let addr = lw.lower(super::pointer::deref_parts(place).0)?;
            return lw.guard_non_null(addr, types::I32, |s| Ok(s.const_i32(0)));
        }
        if super::this::is_field_read(lw.types(), place) {
            return Err(CompileError::NotLowerable(node));
        }
    }
    Ok(lw.const_i32(0))
}

/// # Safety
/// Called only by compiled code, with its context and `free` the instances' `free` a node
/// free was built over.
unsafe extern "C" fn compiled_instance_free(
    ctx: *mut crate::run::Context,
    this: i64,
    free: DyadPtr,
) -> i64 {
    if this == 0 {
        return 0;
    }
    crate::run::interpret_call(ctx, free, 1, &this)
}

/// A place with no frame, a cell or a field's slot, would be read as its own bytes, so the
/// function declines to compile: stand-in for #164.
///
/// # Safety
/// `place` must be a reduced dyad from the store.
unsafe fn require_frame_place(
    lw: &Lowerer,
    node: DyadPtr,
    place: DyadPtr,
) -> Result<(), CompileError> {
    if lw.types().is_storage(place) {
        Ok(())
    } else {
        Err(CompileError::NotLowerable(node))
    }
}

/// The move reads the place; the parse ended its name, so nothing is written.
fn lower_move(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a `move` node `[place, type, op]`.
    let place = unsafe { *(dyad::value(node) as *const DyadPtr).add(TEARDOWN_PLACE) };
    // SAFETY: `place` is a reduced dyad from the store.
    unsafe { require_frame_place(lw, node, place) }?;
    // SAFETY: `place` is a frame place, read by its type.
    unsafe { lw.lower(place) }
}

pub(crate) fn build_defer(store: &mut Store, types: &Core, inner: DyadPtr) -> DyadPtr {
    store.alloc_words(types.defer_, &[inner, types.ops.defer_])
}

/// # Safety
/// `node` must be a `defer` node from `build_defer`.
pub(crate) unsafe fn deferred_inner_of(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr).add(DEFER_INNER)
}

/// One thing a scope's end runs: an authored `defer`, or a name the scope holds, whose
/// value's teardown runs. It is held once `from` of the scope's lines completed and until
/// line `end`, the one that moved or freed the name, when there is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExitItem {
    pub what: DyadPtr,
    pub from: usize,
    pub end: Option<usize>,
}

/// An exit item node is `[what, from, end, op]`, `from` and `end` `u64` leaves, `end` null
/// while nothing ended the name, `op` null: nothing runs the node itself.
const ITEM_FROM: usize = 1;
const ITEM_END: usize = 2;

impl ExitItem {
    /// Whether the scope's end, reached after `reached` of its lines completed, runs it.
    pub(crate) fn held_at(&self, reached: usize) -> bool {
        self.from <= reached && self.end.is_none_or(|end| end > reached)
    }

    /// # Safety
    /// `what` must be a `defer` node or a place laid out in the frame `rt` runs.
    pub(crate) unsafe fn run(&self, rt: &mut Runtime) -> Result<(), RunError> {
        if dyad::ty(self.what) == rt.types().defer_ {
            rt.run(deferred_inner_of(self.what)).map(|_| ())
        } else {
            end_held(rt, self.what)
        }
    }

    pub(crate) fn build(&self, store: &mut Store, types: &Core) -> DyadPtr {
        let u64_ty = types.numtypes[numtype::NumType::U64 as usize];
        let from = store.alloc_blob(u64_ty, &(self.from as u64).to_ne_bytes());
        let end = self.end.map_or(std::ptr::null_mut(), |end| {
            store.alloc_blob(u64_ty, &(end as u64).to_ne_bytes())
        });
        store.alloc_words(types.exit_item, &[self.what, from, end, std::ptr::null_mut()])
    }
}

/// Whether two places are one: the same name, or two names laid out over the same bytes, as
/// an imported `pub` name and the file's own are.
///
/// # Safety
/// `a` and `b` must be dyads from the store.
pub(crate) unsafe fn same_place(types: &Core, a: DyadPtr, b: DyadPtr) -> bool {
    a == b || types.frame_of(a).is_some_and(|at| types.frame_of(b) == Some(at))
}

/// The place `&x` lends when `value` is one; a scope's end that holds it frees the value
/// under the address.
///
/// # Safety
/// `value` must be a reduced dyad from the store.
pub(crate) unsafe fn lent_place(types: &Core, value: DyadPtr) -> Option<DyadPtr> {
    let value = types.through(value);
    (dyad::ty(value) == types.addr_).then(|| types.through(*(dyad::value(value) as *const DyadPtr)))
}

/// A later REPL line moved or freed the name the item holds: the session's end frees it no
/// more.
///
/// # Safety
/// `node` must be an exit item node [`ExitItem::build`] made.
pub(crate) unsafe fn end_exit_item(store: &mut Store, types: &Core, node: DyadPtr) {
    let u64_ty = types.numtypes[numtype::NumType::U64 as usize];
    let end = store.alloc_blob(u64_ty, &0u64.to_ne_bytes());
    *(dyad::value(node) as *mut DyadPtr).add(ITEM_END) = end;
}

/// # Safety
/// `node` must be an exit item node [`ExitItem::build`] made.
pub(crate) unsafe fn exit_item_of(node: DyadPtr) -> ExitItem {
    let slots = dyad::value(node) as *const DyadPtr;
    let leaf = |at: usize| {
        let leaf = *slots.add(at);
        (!leaf.is_null())
            .then(|| std::ptr::read_unaligned(dyad::value(leaf) as *const u64) as usize)
    };
    ExitItem {
        what: *slots,
        from: leaf(ITEM_FROM).expect("an exit item holds from where"),
        end: leaf(ITEM_END),
    }
}

/// The one teardown of the value a held place holds, the `free` slot of the place's type run
/// over it: an owning pointer's destructor frees its block, a type that fills `free` runs it
/// over the value, a node by its address and a plain record by its bytes' address, as a
/// `share` function takes its receiver. A place that holds no node, never written, frees
/// nothing.
///
/// # Safety
/// `place` must be a place the binding site made an owner, laid out in the frame `rt` runs.
pub(crate) unsafe fn end_held(rt: &mut Runtime, place: DyadPtr) -> Result<(), RunError> {
    let ty = rt.types().type_of(place);
    let dtor = meta::destructor_of(ty);
    if !dtor.is_null() {
        let entry = std::mem::transmute::<usize, crate::run::RunFn>(callable::entry_of(dtor));
        return entry(rt, place).map(|_| ());
    }
    let slot = rt.place_addr(place).ok_or(RunError::NoActivation)?;
    if slot.is_null() {
        return Err(RunError::Uninitialized);
    }
    let held = if super::this::is_plain(rt.types(), ty) {
        slot as i64
    } else {
        std::ptr::read_unaligned(slot as *const i64)
    };
    if held != 0 {
        rt.apply_values(meta::instances_free_of(ty), &[held])?;
    }
    Ok(())
}

/// [`end_held`] compiled: a type's `free` is called over the value the place holds; a block's
/// free keeps the function interpreted.
///
/// # Safety
/// As `end_held`.
pub(crate) unsafe fn lower_end_held(lw: &mut Lowerer, place: DyadPtr) -> Result<(), CompileError> {
    if is_owning_place(lw.types(), place) {
        return Err(CompileError::NotLowerable(place));
    }
    let ty = lw.types().type_of(place);
    let this = if super::this::is_plain(lw.types(), ty) {
        lw.place_addr(place)?
    } else {
        lw.read_place(place, types::I64)?
    };
    let free = lw.node_addr(meta::instances_free_of(ty));
    lw.call_seed(compiled_instance_free as *const () as usize, &[this, free]);
    Ok(())
}

/// What a bound owning pointer points at, so the binding site can mint its owning
/// `@pointee` type; a scope whose tail is one propagates through.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn owning_pointee_of(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    let node = types.through(node);
    let logos = dyad::ty(node);
    if logos == types.alloc_ {
        Some(*(dyad::value(node) as *const DyadPtr).add(ALLOC_POINTEE))
    } else if logos == types.move_ {
        let ty = *(dyad::value(node) as *const DyadPtr).add(TEARDOWN_POINTEE);
        (!ty.is_null() && numtype::is_pointer_type(ty) && !meta::destructor_of(ty).is_null())
            .then(|| numtype::pointee_of(ty))
    } else if let Some(output) = moving_call_output(types, node) {
        numtype::is_pointer_type(output).then(|| numtype::pointee_of(output))
    } else if logos == types.scope {
        // A block that yields an owning value moves ownership to the binder.
        crate::parse::last_sequence_expr(node).and_then(|tail| owning_pointee_of(types, tail))
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
    if dyad::ty(f) != types.fn_type {
        return None;
    }
    let fields = dyad::value(f) as *const DyadPtr;
    let body = *fields.add(crate::parse::FN_BODY);
    (!body.is_null() && moves_out_within(types, body, 1))
        .then(|| *fields.add(crate::parse::FN_OUTPUT))
}

/// Whether binding `node` makes the binder an owner: a value just constructed (`alloc`, a
/// type's own `parse` placing a call on its fresh node), a move (`move a`), or a block or
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
    let logos = dyad::ty(node);
    if logos == types.alloc_
        || logos == types.move_
        || logos == types.this.copy
        || logos == types.construct_
    {
        return true;
    }
    // A record call's result moves out when its callee's value does; a record handed on by
    // its bytes' address, when a name that held it or what it copies does.
    if logos == types.by_copy.result {
        let call = *(dyad::value(node) as *const DyadPtr).add(1);
        return moves_out_within(types, call, depth + 1);
    }
    if logos == types.by_copy.out {
        let inner = types.through(*(dyad::value(node) as *const DyadPtr));
        return (dyad::ty(inner) == types.binding_
            && crate::binding::Binding::has_gate(inner, types.own_))
            || moves_out_within(types, inner, depth + 1);
    }
    if logos == types.scope {
        return crate::parse::last_sequence_expr(node)
            .is_some_and(|tail| moves_out_within(types, tail, depth + 1));
    }
    if let super::read::Read::Executable(super::read::Dispatch::Call(f)) =
        super::read::read_kind(types, node)
    {
        // A call a type's own `parse` placed on its fresh node, yielding that node.
        let args = dyad::value(node) as *const DyadPtr;
        if !(*args).is_null() && dyad::ty(*args) == types.this.copy {
            let template = *(dyad::value(*args) as *const DyadPtr);
            let fields = dyad::value(f) as *const DyadPtr;
            return *fields.add(crate::parse::FN_OUTPUT) == dyad::ty(template);
        }
        if dyad::ty(f) == types.fn_type {
            let body = *(dyad::value(f) as *const DyadPtr).add(crate::parse::FN_BODY);
            return !body.is_null() && moves_out_within(types, body, depth + 1);
        }
    }
    false
}

/// Whether a value owns a block: binding it mints an owning place its scope holds.
///
/// # Safety
/// As `owning_pointee_of`.
pub(crate) unsafe fn is_owning_value(types: &Core, node: DyadPtr) -> bool {
    owning_pointee_of(types, node).is_some()
}

/// What ends the life of a value a name or a `free` holds (DESIGN ›Holding is decided at the
/// binding site, parameters included‹).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Teardown {
    /// A borrow, or a value whose type fills no `free`.
    Nothing,
    /// A node just made or moved, of a type that fills `free`: that `free`.
    Node(DyadPtr),
    /// An owning pointer to this pointee type: its block is freed.
    Block(DyadPtr),
}

/// `None` for an `if` with an `else` whose arms carry a teardown, whose value's type shows
/// only when it runs: stand-in for #82.
///
/// # Safety
/// `value` must be a reduced dyad from the store.
pub(crate) unsafe fn teardown_of(types: &Core, value: DyadPtr) -> Option<Teardown> {
    if let Some(pointee) = owning_pointee_of(types, value) {
        return Some(Teardown::Block(pointee));
    }
    // A node, or a plain record in its bytes.
    let typed = super::node_type_of(types, value).or_else(|| {
        super::by_copy::record_type_of(types, value).filter(|&t| meta::is_record_type(t))
    });
    if let Some(t) = typed {
        let free = meta::instances_free_of(t);
        let owned = !free.is_null() && moves_out(types, value);
        return Some(if owned { Teardown::Node(free) } else { Teardown::Nothing });
    }
    let node = types.through(value);
    if dyad::ty(node) == types.scope {
        return crate::parse::last_sequence_expr(node)
            .map_or(Some(Teardown::Nothing), |tail| teardown_of(types, tail));
    }
    if dyad::ty(node) == types.if_ {
        let (_, then, els) = super::if_mod::branches(node);
        // An `if` with no `else` yields unit whichever way it goes.
        if els.is_null() {
            return Some(Teardown::Nothing);
        }
        let plain = |arm: DyadPtr| teardown_of(types, arm) == Some(Teardown::Nothing);
        return (plain(then) && plain(els)).then_some(Teardown::Nothing);
    }
    Some(Teardown::Nothing)
}

/// The runtime notes the live allocation so leaks and double frees are observable.
fn run_alloc(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `alloc` node `[pointee, count, init, op]` from the store.
    unsafe {
        let slots = dyad::value(node) as *const DyadPtr;
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

/// The owning pointer's destructor, run over the place that holds the pointer: frees the
/// block once, a null pointer freeing nothing. A field or cell is emptied, so the free of
/// what holds it finds nothing there; a name is left as it is, its end the parse's.
fn run_teardown(rt: &mut Runtime, place: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `place` is a place of an owning pointer type.
    unsafe {
        let slot = rt.place_addr(place).ok_or(RunError::NoActivation)?;
        if slot.is_null() {
            return Err(RunError::Uninitialized);
        }
        let ptr = std::ptr::read_unaligned(slot as *const i64) as u64 as *mut u8;
        if ptr.is_null() {
            return Ok(0);
        }
        // Tests observe teardown order (LIFO) by the value each freed block held.
        #[cfg(test)]
        {
            let pointee = numtype::pointee_of(rt.types().type_of(place));
            let held = numtype::read_scalar(pointee, ptr);
            FREE_LOG.with(|log| log.borrow_mut().push(held));
        }
        heap_free(ptr);
        rt.note_free();
        if dyad::ty(place) != rt.types().binding_ {
            std::ptr::write_unaligned(slot as *mut i64, 0);
        }
        Ok(0)
    }
}

/// The teardown flows through the reserved `destructor` slot. A null destructor here is a
/// malformed node: `build_teardown` demanded an owning place at parse. The inert form reaches
/// its place and frees nothing (DESIGN ›A value owns what its elements hold and frees it‹).
fn run_free(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `free` node; in the owning form its place's type carries a
    // destructor whose entry is a `RunFn` over the place.
    unsafe {
        let slots = dyad::value(node) as *const DyadPtr;
        let place = *slots.add(TEARDOWN_PLACE);
        if (*slots.add(TEARDOWN_POINTEE)).is_null() {
            reached_slot(rt, place)?;
            return Ok(0);
        }
        let dtor = meta::destructor_of(rt.types().type_of(place));
        if dtor.is_null() || !callable::is_callable(dtor) {
            return Err(RunError::NoDestructor(place));
        }
        let entry = std::mem::transmute::<usize, crate::run::RunFn>(callable::entry_of(dtor));
        entry(rt, place)
    }
}

/// `free a` where the name holds its value: the teardown its scope's end would have run.
fn run_held_free(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[place, null, op]` node from `build_held_free`.
    unsafe { end_held(rt, *(dyad::value(node) as *const DyadPtr).add(TEARDOWN_PLACE)) }?;
    Ok(0)
}

/// The slot is emptied before the body runs, so the free of what holds it finds nothing.
fn run_instance_free(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[place, free, op]` node from `build_instance_free`; the slot
    // holds a node's address or the null an earlier free or move left.
    unsafe {
        let slots = dyad::value(node) as *const DyadPtr;
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
    // SAFETY: `node` is a `[field read, pointee, op]` node from `build_field_free`, over an
    // owning pointer field.
    unsafe {
        let read = *(dyad::value(node) as *const DyadPtr).add(TEARDOWN_PLACE);
        let Some(bytes) = super::this::pointer_bytes(rt, read)? else {
            return Ok(0);
        };
        let bytes = bytes as *mut i64;
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

/// A value no name holds: it runs, and a node it made is freed by its type's `free`.
fn run_value_free(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[value, free, op]` node from `build_value_free`; `free` is null or
    // the instances' `free` of the node the value makes.
    unsafe {
        let slots = dyad::value(node) as *const DyadPtr;
        let this = rt.run(*slots.add(TEARDOWN_PLACE))?;
        let free = *slots.add(TEARDOWN_POINTEE);
        if !free.is_null() && this != 0 {
            rt.apply_values(free, &[this])?;
        }
        Ok(0)
    }
}

/// A value no name holds that owns a block: it runs, and the block is freed.
fn run_value_release(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[value, pointee, op]` node from `build_value_free`; the value
    // yields a block from `heap_alloc` or null.
    unsafe {
        let ptr = rt.run(*(dyad::value(node) as *const DyadPtr).add(TEARDOWN_PLACE))?;
        let ptr = ptr as u64 as *mut u8;
        if !ptr.is_null() {
            heap_free(ptr);
            rt.note_free();
        }
        Ok(0)
    }
}

/// Where an owned value's address lies: a name's place, the cell a dereference reaches, or
/// the slot of an owning field, which holds the node itself.
///
/// # Safety
/// `place` must be the place operand of a `move` or `free` node.
unsafe fn owned_slot(rt: &mut Runtime, place: DyadPtr) -> Result<*mut u8, RunError> {
    match reached_slot(rt, place)? {
        Some(slot) => Ok(slot),
        None => rt.place_addr(place).ok_or(RunError::NoActivation),
    }
}

/// The cell a dereference reaches or the field a field read reaches, found by running the code
/// that leads there; `None` for a place a frame holds, found by its offset alone.
///
/// # Safety
/// As `owned_slot`.
unsafe fn reached_slot(rt: &mut Runtime, place: DyadPtr) -> Result<Option<*mut u8>, RunError> {
    if dyad::ty(place) == rt.types().deref_ {
        return super::pointer::deref_addr(rt, place).map(Some);
    }
    if super::this::is_field_read(rt.types(), place) {
        return super::this::field_addr(rt, place).map(Some);
    }
    Ok(None)
}

/// A move reads the value; the parse ended a moved name. A field moved out is emptied, so its
/// owner's free skips it: stand-in for #66.
fn run_move(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `move` node `[place, type, op]` from the store; a field or cell it
    // reaches is an owning one, eight bytes wide.
    unsafe {
        let place = *(dyad::value(node) as *const DyadPtr).add(TEARDOWN_PLACE);
        let value = rt.run(place)?;
        if dyad::ty(place) != rt.types().binding_ {
            let slot = owned_slot(rt, place)?;
            std::ptr::write_unaligned(slot as *mut i64, 0);
        }
        Ok(value)
    }
}

/// A scope's end runs the inner, never this node; reaching it directly means a defer stood
/// outside any scope.
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
        let mut p = Parser::new(src, &mut store, &mut trie, types, scopes).with_lower(&core.lower);
        let root = p.parse_sequence().expect("parse");
        let mut rt = p.into_runtime();
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

        // `=` frees the block it displaces, after building the right side, before the write.
        assert_eq!(run("mut a := alloc 1 of i32 5, a = alloc 1 of i32 6, a@"), (6, 0));
        assert_eq!(free_log(), vec![5, 6]);
        assert_eq!(run("mut a := alloc 1 of i32 5, b := alloc 1 of i32 6, a = move b, a@"), (6, 0));
        assert_eq!(free_log(), vec![5, 6]);
        // The right side may read what it displaces.
        assert_eq!(run("mut a := alloc 1 of i32 5, a = alloc 1 of i32 (a@ + 1), a@"), (6, 0));
        // An owner never written frees nothing when the first value lands.
        assert_eq!(
            run("mut a := own @i32 ?, a = alloc 1 of i32 5, a = alloc 1 of i32 7, a@"),
            (7, 0)
        );
        assert_eq!(free_log(), vec![5, 7]);
    }

    fn free_log() -> Vec<i64> {
        FREE_LOG.with(|l| l.borrow().clone())
    }

    /// Parse and run `src`, which must fault; returns the fault and the still-live blocks.
    fn fault(src: &str) -> (RunError, usize) {
        FREE_LOG.with(|l| l.borrow_mut().clear());
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = &core;
        let mut p = Parser::new(src, &mut store, &mut trie, types, scopes).with_lower(&core.lower);
        let root = p.parse_sequence().expect("parse");
        let mut rt = p.into_runtime();
        // SAFETY: `root` is the scope just parsed into `store`, which outlives `rt`.
        let fault = unsafe { rt.run(root) }.expect_err("expected a fault");
        (fault, rt.live_allocs())
    }

    #[test]
    fn a_fault_ends_what_every_scope_it_leaves_holds() {
        let f = "f := fn () -> i32 ( p := alloc 1 of i32 7, \
                 ( q := alloc 1 of i32 8, error «stop» ), 0 ),\n";
        let (e, live) = fault(&format!("{f}f()"));
        assert!(matches!(e, RunError::Raised(_)), "{e:?}");
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![8, 7], "the inner scope's first");
        // Only what was declared before the fault ends.
        let (_, live) = fault("p := alloc 1 of i32 7,\nerror «stop»,\nq := alloc 1 of i32 8,\n1");
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![7]);
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
    fn a_return_ends_what_the_scope_holds_at_its_line() {
        let f = "f := fn (c := i32 ?) -> i32 ( a := alloc 1 of i32 5, \
                 if (c == 1) (return 1), free a, 2 ),\n";
        assert_eq!(run(&format!("{f}f(1) + f(0)")), (3, 0));
        assert_eq!(free_log(), vec![5, 5], "freed once on each path");
        // A name declared after the return is not held there yet.
        let g = "g := fn (c := i32 ?) -> i32 ( if (c == 1) (return 1), \
                 a := alloc 1 of i32 5, a@ ),\n";
        assert_eq!(run(&format!("{g}g(1) + g(0)")), (6, 0));
        assert_eq!(free_log(), vec![5]);
    }

    #[test]
    fn the_address_of_a_held_name_does_not_leave_its_scope() {
        for src in [
            "p := ( c := alloc 1 of i32 7, &c ),\n1",
            "f := fn () -> @@i32 ( c := alloc 1 of i32 7, &c )",
            "f := fn (n := i32 ?) -> i32 ( c := alloc 1 of i32 7, if (n > 2) (return &c), 0 )",
        ] {
            assert_eq!(parse_err(src), ParseError::AddressOfHeld, "{src}");
        }
    }

    #[test]
    fn alloc_inside_a_function_frees_when_the_call_returns() {
        let (v, live) = run("main := fn () -> i32 ( p := alloc 1 of i32 42, p@ ),\nmain()");
        assert_eq!(v, 42);
        assert_eq!(live, 0, "the call's frame scope frees its alloc");
    }

    #[test]
    fn early_free_does_not_double_free() {
        let (v, live) = run("a := alloc 1 of i32 3,\nfree a,\n99");
        assert_eq!(v, 99);
        assert_eq!(live, 0, "free frees once; the scope's end holds it no more");
        assert_eq!(free_log(), vec![3], "exactly one free happened");
    }

    #[test]
    fn move_moves_ownership_and_the_source_scope_frees_nothing() {
        let (v, live) = run("a := alloc 1 of i32 7,\nb := move a,\nb@");
        assert_eq!(v, 7);
        assert_eq!(live, 0, "the moved pointer is freed once, through b");
        assert_eq!(free_log(), vec![7], "move does not double-free the source");
    }

    #[test]
    fn move_moves_the_value_of_any_name() {
        // A scalar is copied at its width, a borrowed pointer stays a borrow; the name ends.
        assert_eq!(run("a := i32 5,\nb := move a,\nb"), (5, 0));
        assert_eq!(run("a := i64 7,\nb := move a,\nb + 1"), (8, 0));
        assert_eq!(run("x := i32 3,\nq := &x,\nr := move q,\nr@"), (3, 0));
        assert_eq!(
            run("f := fn (n := i32 ?) -> i32 ( m := move n, m + 1 ),\nf.compile(),\nf(4)"),
            (5, 0)
        );
        assert_eq!(
            parse_err("a := i32 5,\nb := move a,\na"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
        // A field moves only what it owns.
        assert_eq!(
            parse_err("p := type ( x := i32 ?, y := i32 ? ),\nq := p (1, 2),\nb := move q.x"),
            ParseError::MoveOfUnownedPath
        );
    }

    #[test]
    fn move_out_of_an_inner_block_frees_at_the_outer_owner() {
        let (v, live) = run("b := ( a := alloc 1 of i32 8, move a ),\nb@");
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
    fn a_freed_name_takes_no_later_free() {
        // A later `free a` is a use of a dead name, refused before anything runs.
        assert_eq!(
            parse_err("a := alloc 1 of i32 4,\nfree a,\nfree a,\n1"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_dead_name_may_be_redeclared_after_move() {
        // LIFO: the new `a` (9) frees before `b` (7).
        let (v, live) =
            run("a := alloc 1 of i32 7,\nb := move a,\na := alloc 1 of i32 9,\na@ + b@");
        assert_eq!(v, 16);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![9, 7], "a fresh place; the old one held up to the move");
    }

    #[test]
    fn a_read_after_move_is_refused() {
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\nb := move a,\na@"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_write_after_move_is_refused() {
        assert_eq!(
            parse_err("mut a := alloc 1 of i32 7,\nb := move a,\na = b"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_pass_after_free_is_refused_in_the_same_line() {
        // While the line is still parsing the entry's `end` is the `free` node itself.
        assert_eq!(
            parse_err(
                "h := fn (x := i32 ?, p := @i32 ?) -> i32 ( x ),\na := alloc 1 of i32 1,\nh(free a, a)"
            ),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_move_inside_a_nested_block_ends_the_outer_name_after_the_block() {
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\nc := i32 1,\nif (c == 1) ( b := move a, b@ ),\na@"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
        // On both paths the old `a` ends at the `if`: moved into `b`, freed at the arm's end,
        // or freed in the arm the else-less `if` gains.
        for c in [0, 1] {
            let (v, live) = run(&format!(
                "a := alloc 1 of i32 7,\nc := i32 {c},\nif (c == 1) ( b := move a, b@ ),\n\
                 a := alloc 1 of i32 9,\na@"
            ));
            assert_eq!((v, live), (9, 0), "c = {c}");
            assert_eq!(free_log(), vec![7, 9], "c = {c}: the old a at the if, the new at the end");
        }
    }

    #[test]
    fn an_if_frees_what_one_arm_ended_at_the_end_of_the_other() {
        for c in [0, 1] {
            for body in [
                "if (c == 1) ( b := move a, b@ )",
                "if (c == 1) ( free a ) else ( 2 )",
                "if (c == 1) ( 2 ) else ( free a )",
                "if (c == 1) ( 2 ) else if (c == 3) ( 4 ) else ( b := move a, b@ )",
                "( if (c == 1) ( free a ), 5 )",
            ] {
                let src = format!("a := alloc 1 of i32 7,\nc := i32 {c},\n{body},\n1");
                assert_eq!(run(&src), (1, 0), "c = {c}: {body}");
                assert_eq!(free_log(), vec![7], "c = {c}: {body}: freed once");
            }
        }
    }

    #[test]
    fn a_branch_the_pass_drops_ends_no_name() {
        // A bare untaken branch is parsed only to find where it ends; it never runs.
        for src in [
            "a := alloc 1 of i32 7,\nif true (1) else move a,\na@",
            "a := alloc 1 of i32 7,\nif false free a,\na@",
        ] {
            assert_eq!(run(src), (7, 0), "{src}");
            assert_eq!(free_log(), vec![7], "{src}: freed once, at the end");
        }
    }

    #[test]
    fn a_return_before_a_move_frees_what_it_would_have_moved() {
        // The arm that does not move frees on its way out; the moving arm holds the name up to
        // the line that moves it.
        let f = "f := fn (c := i32 ?) -> i32 ( a := alloc 1 of i32 7, \
                 if (c > 0) ( if (c == 2) (return 9), b := move a, b@ ) else ( return 2 ), 3 ),\n";
        assert_eq!(run(&format!("{f}f(0) + f(1) + f(2)")), (14, 0));
        assert_eq!(free_log(), vec![7, 7, 7]);
    }

    #[test]
    fn a_move_of_an_outer_name_inside_a_loop_body_is_refused() {
        // The next pass would read a dead name.
        assert_eq!(
            parse_err(
                "a := alloc 1 of i32 7,\nmut c := i32 1,\nwhile (c == 1) ( b := move a, c = 0 )"
            ),
            ParseError::MoveOfOuterName
        );
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\nfor i in 0..2 ( b := move a )"),
            ParseError::MoveOfOuterName
        );
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\nfor i in 0..2 ( free a )"),
            ParseError::MoveOfOuterName
        );
    }

    #[test]
    fn free_ends_the_name_of_a_plain_value() {
        let (v, live) = run("n := i32 5,\nfree n,\nn := i32 6,\nn");
        assert_eq!(v, 6);
        assert_eq!(live, 0);
        assert_eq!(
            parse_err("n := i32 5,\nfree n,\nn + 1"),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
    }

    #[test]
    fn a_freed_parameter_is_reusable_in_its_body_and_still_compiles() {
        // A parameter is same-level with the body; the inert free lowers to unit, so the
        // function compiles.
        let (v, live) =
            run("f := fn (n := i32 ?) -> i32 ( free n, n := i32 4, n ),\nf.compile(),\nf(1)");
        assert_eq!(v, 4);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_move_of_an_outer_name_inside_a_fn_body_is_refused() {
        // A function may own only what its parameters hand it.
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\nf := fn () -> i32 ( b := move a, b@ )"),
            ParseError::MoveOfOuterName
        );
    }

    /// A value whose owning field holds an array, in the array's shape: `bag ()` is a node
    /// whose run makes a new `bagged`, and `bag` with no bracket is `bagged`.
    const BAG: &str = "import ./identities/array.logos, t := array i32, \
        bagged := type ( \
            mut items := own t ?, \
            share free = ( free items ), \
            share parse_rank = dyad.parse_rank, \
            share parse = ( tape.is_constructed[0] = true ) ), \
        bag := type ( \
            output_type := type ?, \
            share run = ( mut v := output_type ?, v.items = array i32 [4, 5, 6], move v ), \
            share parse_rank = dyad.parse_rank, \
            share associativity = left, \
            share parse = ( \
                if tape[1]:type == scope ( \
                    tape[0]:type = bag, tape[0].output_type = bagged, tape.remove(1) \
                ) else ( tape[0] = bagged ), \
                tape.is_constructed[0] = true \
            ) ),\n";

    #[test]
    fn an_owning_field_frees_its_array_once_through_the_owner_s_free() {
        for (tail, want) in [
            ("b := bag (), b.items[1]", 5),
            ("b := bag (), free b, 2", 2),
            ("f := fn () -> i32 ( b := bag (), b.items[2] ), f() + f()", 12),
            ("l := array bagged [bag (), bag ()], l[1].items[0]", 4),
            ("b := bag (), l := array bagged [move b], l[0].items.size", 3),
            ("mut a := own t ?, a = array i32 [7], a[0]", 7),
        ] {
            assert_eq!(run(&format!("{BAG}{tail}")), (want, 0), "{tail}");
        }
        // `=` into an owned field frees the array it displaces.
        for tail in [
            "b := bag (), x := array i32 [1], b.items = move x, b.items[0]",
            "b := bag (), b.items = array i32 [1], b.items[0]",
            "mut a := own t ?, a = array i32 [5], a = array i32 [1], a[0]",
            "mut a := array i32 [5], a = array i32 [a[0] - 4], a[0]",
        ] {
            assert_eq!(run(&format!("{BAG}{tail}")), (1, 0), "{tail}");
        }
        // The count sees the array: a free that leaves the field alone leaks its one block.
        let forgetful = BAG.replace("share free = ( free items )", "share free = ( 0 )");
        assert_eq!(run(&format!("{forgetful}b := bag (), 1")), (1, 1));
    }

    /// The scope `src` parses to, and the items of its exit.
    fn exit_items(
        store: &mut Store,
        trie: &mut RegexTrie,
        core: &Core,
        src: &str,
    ) -> Vec<ExitItem> {
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let scope = {
            let mut p = Parser::new(src, store, trie, core, scopes);
            p.parse_sequence().expect("parse")
        };
        // SAFETY: `scope` is the sequence node just parsed; its exit holds exit items.
        unsafe {
            let exit = super::super::scope::exit_of(scope);
            assert!(!exit.is_null(), "{src}: the scope's end runs something");
            super::super::array::items(exit).iter().map(|&i| exit_item_of(i)).collect()
        }
    }

    #[test]
    fn a_scope_s_exit_is_reflectable_graph_structure() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let src = "a := alloc 1 of i32 5,\ndefer free (alloc 1 of i32 6),\nfree a,\n0";
        let items = exit_items(&mut store, &mut trie, &core, src);
        // Nothing is inserted into the lines: the scope holds `a` until `free a` and
        // defers what its `defer` line wrote.
        assert_eq!(items.len(), 2, "{items:?}");
        assert_eq!((items[0].from, items[0].end), (1, Some(2)), "held from line 0 to line 2");
        // SAFETY: the items' `what` are dyads from the store.
        unsafe {
            assert_eq!(dyad::ty(items[0].what), core.binding_, "a held name");
            assert_eq!(dyad::ty(items[1].what), core.defer_, "an authored defer");
        }
        assert_eq!((items[1].from, items[1].end), (2, None));
    }

    #[test]
    fn owning_pointer_carries_a_destructor_but_a_borrow_does_not() {
        // Owning-ness rides the node `alloc` mints, not `@T`.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let items = exit_items(&mut store, &mut trie, &core, "a := alloc 1 of i32 5,\na@");
        let a = items[0].what;
        // SAFETY: `a` is the held name's binding, laid out over an owning pointer type.
        unsafe {
            assert!(numtype::is_pointer_type(core.type_of(a)), "a is a pointer place");
            assert!(
                !meta::destructor_of(core.type_of(a)).is_null(),
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
    fn own_names_a_state_and_drop_is_no_word() {
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\nb := own a,\nb@"),
            ParseError::OwnOutsideType
        );
        assert_eq!(parse_err("own.arity"), ParseError::BadReflectRead, "a gate word, as `pub`");
        assert_eq!(
            parse_err("a := alloc 1 of i32 7,\ndrop a,\n1"),
            ParseError::Resolve(ResolveError::Unknown("drop".into()))
        );
        assert_eq!(
            parse_err("t := type ( a := i32 ?, share drop = ( 0 ) ),\n1"),
            ParseError::NoSuchSlot("drop".into())
        );
        assert_eq!(
            run("import ./identities/array.logos, mut x := array i32 [1], y := move x, free y, 0"),
            (0, 0)
        );
    }

    #[test]
    fn free_of_a_value_runs_it_then_frees_what_it_owns() {
        assert_eq!(run("free (alloc 1 of i32 5),\n1"), (1, 0));
        assert_eq!(run("alloc 1 of i32 5,\n1"), (1, 1), "the same value no word takes leaks");
        assert_eq!(run("mk := fn () -> @i32 ( alloc 1 of i32 7 ),\nfree (mk()),\n1"), (1, 0));
        assert!(matches!(
            try_run("n := i32 0 - 1,\nfree (alloc n of i32 1),\n1"),
            Err(RunError::BadCount(-1))
        ));
        let counted = "mut n := i32 0,\nf := fn () -> i32 ( n = n + 1, n ),\n";
        for tail in [
            "free (f()),\nn",
            "free f(),\nn",
            "free (if (1 == 1) (f()) else (2)),\nn",
            "( defer free (f()), 2 ),\nn",
            "g := fn () -> i32 ( free (f()), 1 ),\ng.compile(),\ng(),\nn",
        ] {
            assert_eq!(run(&format!("{counted}{tail}")), (1, 0), "{tail}");
        }
        // A borrow is not the value's owner, so nothing is freed twice.
        assert_eq!(
            run("p := alloc 1 of i32 3,\nid := fn (q := @i32 ?) -> @i32 ( q ),\nfree (id(p)),\np@"),
            (3, 0)
        );
        assert_eq!(free_log(), vec![3], "freed once, by its owner");
        for tail in ["b := bag (),\nfree (bag ()),\n1", "free (bag ()),\n1"] {
            assert_eq!(run(&format!("{BAG}{tail}")), (1, 0), "{tail}");
        }
    }

    #[test]
    fn a_hole_is_no_value_and_move_takes_only_a_place() {
        for src in [
            "free (own @i32 ?),\n1",
            "free (@i32 ?),\n1",
            "free ?,\n1",
            "g := fn () -> i32 ( free (own @i32 ?), 1 )",
            "( defer free (own @i32 ?), 2 ),\n1",
        ] {
            assert_eq!(parse_err(src), ParseError::FreeOfHole, "{src}");
        }
        for src in [
            "f := fn () -> i32 ( 7 ),\nb := move (f())",
            "b := move 5",
            "b := move (alloc 1 of i32 5)",
            "b := move (own @i32 ?)",
            "b := move ?",
            "a := alloc 2 of i32 1,\nb := move (a + 1)",
        ] {
            assert_eq!(parse_err(src), ParseError::MoveOfValue, "{src}");
        }
        assert_eq!(
            parse_err(
                "c := i32 1,\nfree (if (c == 1) (alloc 1 of i32 5) else (alloc 1 of i32 6)),\n1"
            ),
            ParseError::FreeOfUntypedValue
        );
        // With no `else` the value is unit, known at parse.
        assert_eq!(run("c := i32 1,\nfree (if (c == 1) (alloc 1 of i32 5)),\n1").0, 1);
    }

    #[test]
    fn a_name_the_run_starts_with_is_not_ended_by_a_program() {
        for src in ["free i32,\n1", "b := move i32", "g := fn () -> i32 ( free i32, 1 )"] {
            assert_eq!(
                parse_err(src),
                ParseError::EndsPrimordialName(Box::new("i32".into())),
                "{src}"
            );
        }
        assert_eq!(run("t := i32,\nfree t,\nx := i32 4,\nx"), (4, 0));
    }

    #[test]
    fn a_returned_owned_place_is_refused() {
        // The scope's end would free it on the way out and hand back a freed pointer.
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
            "mk := fn () -> @i32 ( p := alloc 1 of i32 7, move p )",
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
                "mut n := i32 0,\nclimb := fn () -> i32 ( n = n + 1, n ),\nclimb(),\nfree n,\nclimb()"
            ),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        assert_eq!(
            parse_err(
                "mut n := i32 0,\nclimb := fn () -> i32 ( n ),\nfree n,\nmut n := i32 10,\nclimb()"
            ),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        let (v, live) =
            run("mut n := i32 0,\nclimb := fn () -> i32 ( n = n + 1, n ),\nclimb(),\nfree n,\n\
             mut n := i32 10,\nclimb2 := fn () -> i32 ( n = n + 1, n ),\nclimb2()");
        assert_eq!(v, 11);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_call_reading_a_freed_owning_pointer_is_refused() {
        // A freed name's place may hold anything, here a freed block.
        assert_eq!(
            parse_err("p := alloc 1 of i32 5,\nf := fn () -> i32 ( p@ ),\nfree p,\nf()"),
            ParseError::Resolve(ResolveError::Dead("p".into()))
        );
        let (v, live) = run(
            "p := alloc 1 of i32 5,\nf := fn (q := @i32 ?) -> i32 ( q@ ),\nr := alloc 1 of i32 4,\nfree p,\nf(r)",
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
                 free n,\ng()"
            ),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        assert_eq!(
            parse_err(
                "n := i32 1,\nouter := fn () -> i32 ( inner := fn () -> i32 ( n ), inner() ),\n\
                 free n,\nouter()"
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
        // Whether the body is constructed after the free or was built before it.
        assert_eq!(
            parse_err(&format!("{POW}free n,\n2 ^ 3")),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        assert_eq!(
            parse_err(&format!("{POW}2 ^ 3,\nfree n,\n2 ^ 3")),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
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
        let mut p = Parser::new(src, &mut store, &mut trie, types, scopes).with_lower(&core.lower);
        let root = p.parse_sequence().expect("parse");
        let mut rt = p.into_runtime();
        // SAFETY: `root` is the script just parsed into `store`.
        let result = unsafe { rt.run(root) };
        assert!(
            matches!(result, Err(RunError::CompileFailed(_))),
            "compiling a heap function declines (deopt), got {result:?}"
        );
    }
}
