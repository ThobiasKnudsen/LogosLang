// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Placing: the node a write into a tape cell or a field keeps. When the write runs, its right
//! side is walked once: a tape read becomes the line it names, a read of a field of the parse's
//! own node the node that field holds, and what reads the running call's own frame runs, since
//! that frame is gone when the placed node runs. Everything else stays the graph it was built as.

use super::callable::{self, Callables};
use super::read::{output_type, place_layout, Read};
use super::{meta, Cx};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::reflect::{describe, Shape};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct PlaceIds {
    /// `[value, op, output_type]`: the node a write keeps for `value`, placed each time it runs.
    pub place: DyadPtr,
    pub place_leaf: DyadPtr,
}

pub(super) fn register(cx: &mut Cx, cs: &Callables) -> PlaceIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["value", "op", "output_type"],
    );
    let place = cx.store.alloc_head(cx.type_, record);
    let place_leaf = callable::mint_native(cx.store, cs.callable, run_place, cs.seed_native);
    PlaceIds { place, place_leaf }
}

/// The operand of a write that keeps `value` as a node.
pub(crate) fn build(store: &mut Store, types: &Core, value: DyadPtr) -> DyadPtr {
    let output = super::pointer::at_dyad(store, types);
    store.alloc_words(types.place.place, &[value, types.place.place_leaf, output])
}

/// Where a node stands in the right side: the whole of it, or an operand of a node over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum At {
    Whole,
    Inside,
}

enum Placed {
    Kept,
    New(DyadPtr),
    /// A read of the running call's frame whose value no node holds, so the node over it runs.
    Frame,
}

/// ›A tape read checked against a number type reads as that number‹ [[R229]].
fn run_place(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a place node from `build`, whose value is a reduced dyad.
    unsafe {
        let value = *(dyad::value(node) as *const DyadPtr);
        match place(rt, value, At::Whole)? {
            Placed::Kept => Ok(value as i64),
            Placed::New(placed) => Ok(placed as i64),
            Placed::Frame => Err(RunError::NoWholeRead),
        }
    }
}

/// # Safety
/// `node` must be null or a reduced dyad from the store, and a read of a call frame in it a read
/// of the running call's.
unsafe fn place(rt: &mut Runtime, node: DyadPtr, at: At) -> Result<Placed, RunError> {
    if node.is_null() {
        return Ok(Placed::Kept);
    }
    let types: *const Core = rt.types();
    let types = &*types;
    if let Some(line) = super::tape::named_line(rt, node) {
        let line = line?;
        return Ok(Placed::New(match (at, super::tape::checked_number(types, node)) {
            (At::Inside, Some(ty)) => land(rt, line, ty)?,
            _ => line,
        }));
    }
    // A `.type` read of a line the parse holds reads its tape or its node, and gives a type.
    if super::tape::is_line_type_read(types, node) {
        return run_now(rt, node, at);
    }
    if super::this::is_field_read(types, node) {
        if !super::this::is_fill(types, node) {
            return run_now(rt, node, at);
        }
        let held = super::this::held(rt, node)?;
        return Ok(Placed::New(match (at, super::this::load_type(types, node)) {
            (At::Inside, Some(ty)) => land(rt, held, ty)?,
            _ => held,
        }));
    }
    // A `fn` value's body runs in a frame of its own, and a bracket's lines where it is run.
    if dyad::ty(node) == types.fn_type || super::tape::bracket_kind(types, node).is_some() {
        return Ok(Placed::Kept);
    }
    match types.frame_of(node) {
        Some((crate::binding::Frame::Call(_), _)) => return run_now(rt, node, at),
        Some(_) => return Ok(Placed::Kept),
        None => {}
    }
    let (layout, words) = match describe(types, node) {
        Shape::Call { args, .. } => (Layout::Call, args),
        Shape::Tuple { slots } => (Layout::Tuple, slots.iter().map(|s| s.node).collect()),
        Shape::List { head, tail } => {
            (Layout::List, head.iter().map(|s| s.node).chain(tail).collect())
        }
        _ => return Ok(Placed::Kept),
    };
    let mut placed: Option<Vec<DyadPtr>> = None;
    for (i, &word) in words.iter().enumerate() {
        let new = match place(rt, word, At::Inside)? {
            Placed::Kept => continue,
            Placed::Frame => return run_now(rt, node, at),
            // A bracket handed to a call has its lines run where the call runs.
            Placed::New(new)
                if layout == Layout::Call && super::tape::bracket_kind(types, new).is_some() =>
            {
                super::tape::build_bracket_arg(rt.store(), types, new)
                    .map_err(|e| RunError::Parse(Box::new(e)))?
            }
            Placed::New(new) => new,
        };
        placed.get_or_insert_with(|| words.clone())[i] = new;
    }
    let Some(words) = placed else {
        return Ok(Placed::Kept);
    };
    let copy = rebuild(rt.store(), node, layout, &words);
    // ›Inside a body, a `-> type` call may take an argument known only at run‹ [[R296]].
    if layout == Layout::Call
        && output_type(types, node) == types.type_
        && words.iter().all(|&w| super::is_comptime_arg(types, w))
    {
        return run_now(rt, copy, at);
    }
    Ok(Placed::New(copy))
}

/// # Safety
/// `node` must be a dyad from the store; `ty` a type node.
unsafe fn land(rt: &mut Runtime, node: DyadPtr, ty: DyadPtr) -> Result<DyadPtr, RunError> {
    let types: *const Core = rt.types();
    super::land(rt.store(), &*types, node, ty).map_err(|e| RunError::Parse(Box::new(e)))
}

/// The node runs now, and a node of its value stands in: a number or rational as a constant, a
/// type as itself, and a node as itself, or inside the right side as
/// [`super::tape::node_operand`] hands it on. A value of any other type is [`Placed::Frame`].
///
/// # Safety
/// As [`place`].
unsafe fn run_now(rt: &mut Runtime, node: DyadPtr, at: At) -> Result<Placed, RunError> {
    let types: *const Core = rt.types();
    let types = &*types;
    let ty = output_type(types, node);
    let (read, width) = match place_layout(types, ty) {
        Some((Read::Pointer(p), width)) if p == types.dyad_ => (Read::Container(p), width),
        Some(layout @ (Read::Scalar(_) | Read::Rational | Read::Container(_), _)) => layout,
        _ => return Ok(Placed::Frame),
    };
    let bits = rt.run(node)?;
    let store = rt.store();
    Ok(Placed::New(match read {
        Read::Scalar(_) => store.alloc_blob(ty, &bits.to_ne_bytes()[..width]),
        Read::Rational => {
            store.alloc_blob(ty, std::slice::from_raw_parts(bits as usize as *const u8, width))
        }
        Read::Container(t) if t == types.type_ || at == At::Whole => bits as usize as DyadPtr,
        _ => super::tape::node_operand(rt, bits as usize as DyadPtr),
    }))
}

/// How a node keeps its operand words, as [`describe`] reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// A call's arguments, null-terminated.
    Call,
    /// An operand record's roles, one word each.
    Tuple,
    /// An operand record's roles, then a null-terminated tail.
    List,
}

/// A node of `node`'s type keeping `words` the way `node` keeps its own; stand-in for #101.
fn rebuild(store: &mut Store, node: DyadPtr, layout: Layout, words: &[DyadPtr]) -> DyadPtr {
    // SAFETY: `node` is the dyad `describe` read the words of.
    let ty = unsafe { dyad::ty(node) };
    let mut words = words.to_vec();
    if layout != Layout::Tuple {
        words.push(std::ptr::null_mut());
    }
    store.alloc_words(ty, &words)
}
