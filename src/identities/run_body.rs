// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A type's held `run` body: the `run` slot's body is lexed once at the definition into a
//! tape fragment and constructed once per field-type set when a node supplies the types.
//! Node shape `{type: run_body, value: [text, cells, specs, op]}`; `specs` is an array
//! of `[key, fn]` pairs, `key` one type per instance field. DESIGN ›Deferral is authored‹.

use super::numtype::NumType;
use super::{array, meta, Cx};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::ParsingTape;
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct RunBodyIds {
    pub run_body: DyadPtr,
}

/// No spelling, and nothing runs it: its op slot stays null, so a read of it is data.
pub(super) fn register(cx: &mut Cx) -> RunBodyIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["text", "cells", "specs", "op"],
    );
    let run_body = cx.store.alloc_head(cx.type_, record);
    RunBodyIds { run_body }
}

const TEXT: usize = 0;
const CELLS: usize = 1;
const SPECS: usize = 2;

/// `text` holds the body with its brackets; `cells` must live for the run.
pub(crate) fn build(
    store: &mut Store,
    types: &Core,
    text: DyadPtr,
    cells: *mut ParsingTape,
) -> DyadPtr {
    let u64_ty = types.numtypes[NumType::U64 as usize];
    let handle = store.alloc_blob(u64_ty, &(cells as usize as u64).to_ne_bytes());
    store.alloc_words(
        types.run_body.run_body,
        &[text, handle, std::ptr::null_mut(), std::ptr::null_mut()],
    )
}

/// The body's text, brackets included.
///
/// # Safety
/// `node` must be a node from `build`.
pub(crate) unsafe fn text_of<'a>(node: DyadPtr) -> &'a str {
    let ops = dyad::value(node) as *const DyadPtr;
    let bytes = super::string::text(*ops.add(TEXT));
    // The bytes were copied from a `&str` by `build_text`.
    std::str::from_utf8(bytes).unwrap_or("")
}

/// # Safety
/// As `text_of`.
pub(crate) unsafe fn cells_of(node: DyadPtr) -> *mut ParsingTape {
    let ops = dyad::value(node) as *const DyadPtr;
    let handle = *ops.add(CELLS);
    std::ptr::read_unaligned(dyad::value(handle) as *const u64) as usize as *mut ParsingTape
}

/// The `[key, fn]` pairs constructed so far.
///
/// # Safety
/// As `text_of`.
unsafe fn pairs(node: DyadPtr) -> Vec<DyadPtr> {
    let ops = dyad::value(node) as *const DyadPtr;
    let specs = *ops.add(SPECS);
    if specs.is_null() {
        Vec::new()
    } else {
        array::items(specs).to_vec()
    }
}

/// Null when no function was built for `key`.
///
/// # Safety
/// As `text_of`.
pub(crate) unsafe fn lookup(node: DyadPtr, key: &[DyadPtr]) -> DyadPtr {
    for pair in pairs(node) {
        let items = array::items(pair);
        if items.len() == 2 && array::items(items[0]) == key {
            return items[1];
        }
    }
    std::ptr::null_mut()
}

/// # Safety
/// As `text_of`; `key` types from the store, `f` a `fn` node.
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
    let ops = dyad::value(node) as *mut DyadPtr;
    *ops.add(SPECS) = array::build(store, types.array_, &all);
}

/// A construction that failed leaves nothing behind.
///
/// # Safety
/// As `insert`.
pub(crate) unsafe fn remove(store: &mut Store, types: &Core, node: DyadPtr, key: &[DyadPtr]) {
    let kept: Vec<DyadPtr> = pairs(node)
        .into_iter()
        .filter(|&pair| {
            let items = array::items(pair);
            !(items.len() == 2 && array::items(items[0]) == key)
        })
        .collect();
    let ops = dyad::value(node) as *mut DyadPtr;
    *ops.add(SPECS) = if kept.is_empty() {
        std::ptr::null_mut()
    } else {
        array::build(store, types.array_, &kept)
    };
}

/// The slot after a node's operand run, where its set's function goes.
///
/// # Safety
/// `node` must be a node minted for a type whose record carries a run body:
/// `[field…, null, spec]`.
unsafe fn spec_slot(node: DyadPtr) -> *mut DyadPtr {
    tail_start(node).add(1)
}

/// # Safety
/// `node` must be a node of a record that is not plain, laid out as `this::empty_node` lays one
/// out: `[field…]`, then the words [`TAIL_LEN`] counts.
unsafe fn tail_start(node: DyadPtr) -> *mut DyadPtr {
    // Counted by the type, not scanned to the first null: an unwritten field is null too.
    let n_fields = array::items(meta::record_fields_of(dyad::ty(node))).len();
    (dyad::value(node) as *mut DyadPtr).add(n_fields)
}

/// The words `node` keeps after its fields.
///
/// # Safety
/// As `tail_start`.
pub(crate) unsafe fn tail_of<'a>(node: DyadPtr) -> &'a [DyadPtr] {
    std::slice::from_raw_parts(tail_start(node), TAIL_LEN)
}

/// The words a node of a Logos-written type holds after its fields: the run's terminator, then
/// the slot for its set's function (›Operands sit inline after the type word; a growing list
/// stays behind a pointer‹ [[R153]]).
pub(crate) const TAIL_LEN: usize = 2;

/// The function the node runs; null while no set has been resolved for the node.
///
/// # Safety
/// As `spec_slot`.
pub(crate) unsafe fn spec_of(node: DyadPtr) -> DyadPtr {
    *spec_slot(node)
}

/// One type per field of `node`, a node of the Logos-written type `ty`, in order: a field's
/// declared type, `type` included, but the type the field `output` holds, the type the node's run
/// gives, and the bracket a field typed as a bracket holds (›A function is built once; a `fn` or
/// `run` body is built as far as it can be where it is written‹ [[R299]]). `None` while one is
/// unknown.
///
/// # Safety
/// `ty` must carry a `RECORD_TAG` record and `node` be `[field…]` of it; `output` one of its
/// `type` fields.
pub(crate) unsafe fn field_types(
    types: &Core,
    ty: DyadPtr,
    node: DyadPtr,
    output: Option<usize>,
) -> Result<Option<Vec<DyadPtr>>, crate::parse::ParseError> {
    let fields = array::items(meta::record_fields_of(ty));
    let slots = dyad::value(node) as *const DyadPtr;
    let mut key = Vec::with_capacity(fields.len());
    for (i, &field) in fields.iter().enumerate() {
        let declared = super::hole::type_in(field);
        let slot = *slots.add(i);
        if slot.is_null() {
            return Ok(None);
        }
        let entry = if output == Some(i) {
            types.through(slot)
        } else if declared != types.square_brackets && declared != types.scope {
            declared
        } else if let Some(bracket) = super::tape::bracket_kind(types, slot) {
            bracket
        } else {
            super::read::value_type(types, slot)?
        };
        let known =
            if output == Some(i) { super::is_type_value(types, entry) } else { !entry.is_null() };
        if !known {
            return Ok(None);
        }
        key.push(entry);
    }
    Ok(Some(key))
}

/// The first field of `node` its constructor left null, when `node` is a node of a type
/// with a run body; such a node has no function to run.
///
/// # Safety
/// `node` must be a valid dyad from the store with a non-null type.
pub(crate) unsafe fn unfilled_field(node: DyadPtr) -> Option<usize> {
    let ty = dyad::ty(node);
    let slots = dyad::value(node) as *const DyadPtr;
    if meta::kind_of(ty) != Some(meta::RECORD_TAG) || meta::run_body_of(ty).is_null() {
        return None;
    }
    let n_fields = array::items(meta::record_fields_of(ty)).len();
    (0..n_fields).find(|&i| (*slots.add(i)).is_null())
}

/// # Safety
/// As `spec_slot`; `f` a `fn` node.
pub(crate) unsafe fn set_spec(node: DyadPtr, f: DyadPtr) {
    *spec_slot(node) = f;
}
