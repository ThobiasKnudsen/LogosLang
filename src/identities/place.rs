// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Placing: the node a write into a tape cell or a field keeps. When the write runs, its right
//! side is walked once, each node placed as its type's record says, as its run is: a read of a
//! line or field the parse holds becomes that node, and what needs the running call runs, since
//! that call is gone when the placed node runs. A node whose type says nothing is placed operand
//! by operand, and stays the graph it was built as where none of them changes.

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

/// The operand of a write that keeps `value` as a node. A line held for the run is parsed only
/// where it runs, so it is no node to keep: the write waits with it, and is parsed whole when it
/// runs (›A type known only when the program runs still works; what needs it is parsed when its
/// line runs‹ [[R377]]).
///
/// # Safety
/// `value` must be null or a reduced dyad from the store.
pub(crate) unsafe fn build(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
) -> Result<DyadPtr, crate::parse::ParseError> {
    if !value.is_null() && dyad::ty(value) == types.waiting.line {
        return Err(crate::parse::ParseError::WaitsForRun(None));
    }
    let output = super::pointer::at_dyad(store, types);
    Ok(store.alloc_words(types.place.place, &[value, types.place.place_leaf, output]))
}

/// What a parse's placed right side makes of a node of one type, held by the type's record
/// (›A tape read checked against a number type reads as that number‹ [[R229]]).
pub(crate) type PlaceFn = unsafe fn(&mut Runtime, DyadPtr, At) -> Result<Placed, RunError>;

/// Where a node stands in the right side: the whole of it, or an operand of a node over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum At {
    Whole,
    Inside,
}

pub(crate) enum Placed {
    Kept,
    New(DyadPtr),
    /// It needs the running call and gives nothing a node can hold: the node over it runs.
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
    match meta::placing_of(dyad::ty(node)) {
        Some(placing) => placing(rt, node, at),
        None => {
            let operands = operands(rt, node, at)?;
            finish(rt, node, at, operands)
        }
    }
}

/// A node's operands, each placed.
pub(crate) enum Operands {
    /// None changes.
    Same,
    /// One needs the running call, so the node ran: what stands for it.
    Ran(Placed),
    /// The node's words, the changed ones placed anew.
    New(Words),
}

pub(crate) struct Words {
    layout: Layout,
    words: Vec<DyadPtr>,
}

impl Words {
    /// The first operand, placed.
    pub(crate) fn first(&self) -> DyadPtr {
        self.words[0]
    }
}

/// # Safety
/// As [`place`].
pub(crate) unsafe fn operands(
    rt: &mut Runtime,
    node: DyadPtr,
    at: At,
) -> Result<Operands, RunError> {
    let types: *const Core = rt.types();
    let types = &*types;
    let (layout, words) = match describe(types, node) {
        Shape::Call { args, .. } => (Layout::Call, args),
        Shape::Tuple { slots } => (Layout::Tuple, slots.iter().map(|s| s.node).collect()),
        Shape::List { head, tail } => {
            (Layout::List, head.iter().map(|s| s.node).chain(tail).collect())
        }
        _ => return Ok(Operands::Same),
    };
    let mut placed: Option<Vec<DyadPtr>> = None;
    for (i, &word) in words.iter().enumerate() {
        let new = match place(rt, word, At::Inside)? {
            Placed::Kept => continue,
            Placed::Frame => return Ok(Operands::Ran(run_now(rt, node, at)?)),
            // A bracket handed to a call has its lines run where the call runs.
            Placed::New(new)
                if layout == Layout::Call && super::tape::bracket_kind(types, new).is_some() =>
            {
                super::tape::build_bracket_arg(rt.store(), types, new)
            }
            Placed::New(new) => new,
        };
        placed.get_or_insert_with(|| words.clone())[i] = new;
    }
    Ok(match placed {
        Some(words) => Operands::New(Words { layout, words }),
        None => Operands::Same,
    })
}

/// The node [`operands`] leaves: itself, what stands for it where it ran, or a new node of its
/// type over the placed words.
///
/// # Safety
/// As [`place`]; `operands` what [`operands`] gave of `node`.
pub(crate) unsafe fn finish(
    rt: &mut Runtime,
    node: DyadPtr,
    at: At,
    operands: Operands,
) -> Result<Placed, RunError> {
    let Words { layout, words } = match operands {
        Operands::Same => return Ok(Placed::Kept),
        Operands::Ran(placed) => return Ok(placed),
        Operands::New(words) => words,
    };
    let types: *const Core = rt.types();
    let types = &*types;
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

/// A node whose lines run where it runs, a `fn` value's in a frame of its own and a bracket's
/// where the call it is handed to runs, is kept whole.
///
/// # Safety
/// As [`place`].
pub(crate) unsafe fn whole(_rt: &mut Runtime, _node: DyadPtr, _at: At) -> Result<Placed, RunError> {
    Ok(Placed::Kept)
}

/// A name runs where its bytes lie in the running call's frame, and is kept where they outlive it.
///
/// # Safety
/// As [`place`].
pub(crate) unsafe fn by_frame(rt: &mut Runtime, node: DyadPtr, at: At) -> Result<Placed, RunError> {
    match rt.types().frame_of(node) {
        Some((crate::binding::Frame::Call(_), _)) => run_now(rt, node, at),
        _ => Ok(Placed::Kept),
    }
}

/// # Safety
/// `node` must be a dyad from the store; `ty` a type node.
pub(crate) unsafe fn land(
    rt: &mut Runtime,
    node: DyadPtr,
    ty: DyadPtr,
) -> Result<DyadPtr, RunError> {
    let types: *const Core = rt.types();
    super::land(rt.store(), &*types, node, ty).map_err(|e| RunError::Parse(Box::new(e)))
}

/// The placing of a node that needs the running call: it runs now, and a node of its value
/// stands in: a number or rational as a constant, a type as itself, and a node as itself, or
/// inside the right side as [`super::tape::node_operand`] hands it on. A value of any other type,
/// and a node that gives none, is [`Placed::Frame`], and never runs here.
///
/// # Safety
/// As [`place`].
pub(crate) unsafe fn run_now(rt: &mut Runtime, node: DyadPtr, at: At) -> Result<Placed, RunError> {
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
    // SAFETY: `ty` is a type node; a record whose node `describe` reads slots of is not plain, so
    // the node is laid out with the words after its fields.
    unsafe {
        if meta::is_record_type(ty) {
            words.extend_from_slice(super::run_body::tail_of(node));
        }
    }
    store.alloc_words(ty, &words)
}
