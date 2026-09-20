// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A type's held `run` body (#133 slice 8; DESIGN ›Deferral is authored‹,
//! refined 20 September 2026: "the body is held as its lexed tape … and
//! constructed once per field-type set when a node supplies the types, its
//! op slots resolving at that construction as anywhere else; so `^`'s
//! `shared run = (…)` is lexed at the definition and constructed when `x ^ 3`
//! is built, once for i32 operands, kept on the type for every later `^`
//! over i32, the interpreter and the compiler reading the same constructed
//! body").
//!
//! `shared run = (…)` in an instance block is lexed once, at the definition,
//! into a tape fragment ([`crate::parse::Parser::slot_body_fill`]), and the
//! fragment is held on the type in a node of this shape: `{type: run_body,
//! value: [text, cells, specs, op]}`. `text` is the string node the body's
//! text was copied into — its bytes live for the store, and the cells'
//! spellings index them; `cells` is a `u64` holding the fragment's address,
//! a [`ParsingTape`] leaked for the run as a source is; `specs` holds the
//! bodies constructed so far, an array of `[key, fn]` pairs — `key` an array
//! with one type per instance field (for a field of type `type`, the type it
//! held), `fn` the function built from the fragment for that set, its
//! parameters the fields in order. A node of the type points at its set's
//! function through the slot after its operand run's terminator
//! ([`spec_of`]): `[field…, null, fn]`, as `+`'s op slot holds `add_i32`
//! (DESIGN ›Deferral is authored‹: "stores it in the dyad as `+`'s op slot
//! stores `add_i32`"). The parser writes that slot when the node is placed,
//! and again should a field's type arrive later
//! ([`crate::parse::Parser::resolve_specialization`]); a node whose slot is
//! still empty neither runs nor lowers (ruled 20 September 2026).

use super::numtype::NumType;
use super::{array, meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::ParsingTape;
use crate::store::Store;
use crate::Core;

/// The held-body node type.
#[derive(Debug, Clone, Copy)]
pub struct RunBodyIds {
    pub run_body: DyadPtr,
}

/// Register the node type. It has no spelling, and nothing runs it: its op
/// slot stays null, so a read of it is data.
pub(super) fn register(cx: &mut Cx) -> RunBodyIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["text", "cells", "specs", "op"],
    );
    let run_body = cx.store.alloc_raw(cx.type_, record);
    RunBodyIds { run_body }
}

const TEXT: usize = 0;
const CELLS: usize = 1;
const SPECS: usize = 2;

/// Build the held node over `text` (a string node holding the body with its
/// brackets) and `cells`, the fragment lexed from it, which must live for
/// the run.
pub(crate) fn build(
    store: &mut Store,
    types: &Core,
    text: DyadPtr,
    cells: *mut ParsingTape,
) -> DyadPtr {
    let u64_ty = types.numtypes[NumType::U64 as usize];
    let storage = store.alloc_bytes(&(cells as usize as u64).to_ne_bytes());
    let handle = store.alloc_raw(u64_ty, storage);
    let value = store.alloc_operands(&[text, handle, std::ptr::null_mut(), std::ptr::null_mut()]);
    store.alloc_raw(types.run_body.run_body, value)
}

/// The body's text, brackets included.
///
/// # Safety
/// `node` must be a node from [`build`].
pub(crate) unsafe fn text_of<'a>(node: DyadPtr) -> &'a str {
    let ops = (*node).value as *const DyadPtr;
    let bytes = super::string::text(*ops.add(TEXT));
    // The bytes were copied from a `&str` by `build_text`.
    std::str::from_utf8(bytes).unwrap_or("")
}

/// The fragment the body was lexed into.
///
/// # Safety
/// As [`text_of`].
pub(crate) unsafe fn cells_of(node: DyadPtr) -> *mut ParsingTape {
    let ops = (*node).value as *const DyadPtr;
    let handle = *ops.add(CELLS);
    std::ptr::read_unaligned((*handle).value as *const u64) as usize as *mut ParsingTape
}

/// The pairs `[key, fn]` constructed so far.
///
/// # Safety
/// As [`text_of`].
unsafe fn pairs(node: DyadPtr) -> Vec<DyadPtr> {
    let ops = (*node).value as *const DyadPtr;
    let specs = *ops.add(SPECS);
    if specs.is_null() {
        Vec::new()
    } else {
        array::items(specs).to_vec()
    }
}

/// The function built for `key`, or null when none was.
///
/// # Safety
/// As [`text_of`].
pub(crate) unsafe fn lookup(node: DyadPtr, key: &[DyadPtr]) -> DyadPtr {
    for pair in pairs(node) {
        let items = array::items(pair);
        if items.len() == 2 && array::items(items[0]) == key {
            return items[1];
        }
    }
    std::ptr::null_mut()
}

/// Enter `f` as the function for `key`.
///
/// # Safety
/// As [`text_of`]; `key` types from the store, `f` a `fn` node.
pub(crate) unsafe fn insert(
    store: &mut Store,
    types: &Core,
    node: DyadPtr,
    key: &[DyadPtr],
    f: DyadPtr,
) {
    let key_arr = array::build(store, types.array_, key);
    let pair = array::build(store, types.array_, &[key_arr, f]);
    let mut all = pairs(node);
    all.push(pair);
    let ops = (*node).value as *mut DyadPtr;
    *ops.add(SPECS) = array::build(store, types.array_, &all);
}

/// Take the entry for `key` out again — a construction that failed leaves
/// nothing behind.
///
/// # Safety
/// As [`insert`].
pub(crate) unsafe fn remove(store: &mut Store, types: &Core, node: DyadPtr, key: &[DyadPtr]) {
    let kept: Vec<DyadPtr> = pairs(node)
        .into_iter()
        .filter(|&pair| {
            let items = array::items(pair);
            !(items.len() == 2 && array::items(items[0]) == key)
        })
        .collect();
    let ops = (*node).value as *mut DyadPtr;
    *ops.add(SPECS) = if kept.is_empty() {
        std::ptr::null_mut()
    } else {
        array::build(store, types.array_, &kept)
    };
}

/// The slot after a node's operand run: where its set's function goes.
///
/// # Safety
/// `node` must be the node [`crate::parse::Parser::run_logos_ctor`] minted
/// for a type whose record carries a run body: `[field…, null, spec]`.
unsafe fn spec_slot(node: DyadPtr) -> *mut DyadPtr {
    let slots = (*node).value as *mut DyadPtr;
    let mut i = 0;
    while !(*slots.add(i)).is_null() {
        i += 1;
    }
    slots.add(i + 1)
}

/// The function a node of a held-body type runs as — null while no set has
/// been resolved for it.
///
/// # Safety
/// As [`spec_slot`].
pub(crate) unsafe fn spec_of(node: DyadPtr) -> DyadPtr {
    if (*node).value.is_null() {
        return std::ptr::null_mut();
    }
    *spec_slot(node)
}

/// Point `node` at `f`.
///
/// # Safety
/// As [`spec_slot`]; `f` a `fn` node.
pub(crate) unsafe fn set_spec(node: DyadPtr, f: DyadPtr) {
    *spec_slot(node) = f;
}
