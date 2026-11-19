// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The built-in operations' chooser: one mint per set of field types their parse chooses, built
//! once and kept on the type, and the one builder of their nodes. A node's type is its mint, and
//! the mint holds what its nodes share: the type they give back and what they run. A
//! Logos-written type mints through a function of its own. ›An operation mints through a chooser,
//! one mint per set of operand types, so each mint's `run` body is parsed once‹ [[R301]]

use super::{array, meta};
use crate::dyad::DyadPtr;
use crate::store::Store;
use crate::Core;

/// The member that holds what a type's nodes give back (›A mint's output type is written in its
/// `static output_type` declaration; a type that does not mint has its parse write it into each
/// node‹ [[R194]]).
pub(crate) const STATIC_FIELD: &str = "output_type";

/// A node of the built-in operation `origin`, its fields `words` in the order of `origin`'s roles,
/// typed by `origin`'s mint for the field types `key`: the one way a built-in operation's node is
/// built. The parse that chooses the mint gives the type its nodes give back and the operation
/// they run; the parse choosing the operation is a stand-in for #69.
///
/// # Safety
/// `origin` must carry a `TUPLE_TAG` operand record of `words.len()` roles; `key` and
/// `output_type` null or type nodes, `op` a callable leaf, `words` dyads, all from the store.
pub(crate) unsafe fn node(
    store: &mut Store,
    types: &Core,
    origin: DyadPtr,
    key: &[DyadPtr],
    output_type: DyadPtr,
    op: DyadPtr,
    words: &[DyadPtr],
) -> DyadPtr {
    debug_assert_eq!(meta::arity_of(origin), words.len(), "a mint's node holds its fields alone");
    let mint = of(store, types, origin, key, output_type, op);
    store.alloc_words(mint, words)
}

/// `origin`'s mint for the field types `key`, made the first time and kept on `origin`.
///
/// # Safety
/// As [`node`].
unsafe fn of(
    store: &mut Store,
    types: &Core,
    origin: DyadPtr,
    key: &[DyadPtr],
    output_type: DyadPtr,
    op: DyadPtr,
) -> DyadPtr {
    if let Some(mint) = find(origin, key) {
        debug_assert!(
            meta::mint_output_of(mint) == output_type && meta::mint_op_of(mint) == op,
            "one set of field types gives one output type and one operation"
        );
        return mint;
    }
    make(store, types, origin, key, output_type, op)
}

/// # Safety
/// As [`node`].
unsafe fn make(
    store: &mut Store,
    types: &Core,
    origin: DyadPtr,
    key: &[DyadPtr],
    output_type: DyadPtr,
    op: DyadPtr,
) -> DyadPtr {
    let key = array::build(store, types.array_, key);
    let record = meta::mint_record(store, origin, output_type, op);
    let mint = store.alloc_head(types.type_, record);
    let mut made = meta::mints_of(origin);
    if made.is_null() {
        made = array::build(store, types.array_, &[]);
        meta::install_mints(origin, made);
    }
    let pair = array::build(store, types.array_, &[key, mint]);
    array::push(store, made, pair);
    mint
}

/// # Safety
/// `origin` must carry a record whose memo only `of` built and grew.
unsafe fn find(origin: DyadPtr, key: &[DyadPtr]) -> Option<DyadPtr> {
    let made = meta::mints_of(origin);
    if made.is_null() {
        return None;
    }
    array::items(made).iter().find_map(|&pair| match array::items(pair) {
        &[made_for, mint] if array::items(made_for) == key => Some(mint),
        _ => None,
    })
}

/// # Safety
/// `t` must be null or a valid dyad from the store.
unsafe fn is_mint(t: DyadPtr) -> bool {
    !t.is_null() && meta::kind_of(t).is_some() && !meta::mint_of(t).is_null()
}

/// The type `t` is a mint of; `t` itself when it is no mint.
///
/// # Safety
/// As `is_mint`.
pub(crate) unsafe fn origin(t: DyadPtr) -> DyadPtr {
    if is_mint(t) {
        meta::mint_of(t)
    } else {
        t
    }
}

/// The type every node of the mint `t` gives back; `None` when `t` is no mint.
///
/// # Safety
/// As `is_mint`.
pub(crate) unsafe fn output_type_of(t: DyadPtr) -> Option<DyadPtr> {
    is_mint(t).then(|| meta::mint_output_of(t))
}

/// The field `name` of a node of the mint `t` that lives on the mint: `output_type`, the one a
/// program reads there (›`x.type` reads the type word of the node a name or an expression stands
/// for‹ [[R279]]). `None` when `t` is no mint or keeps no such field.
///
/// # Safety
/// As `is_mint`.
pub(crate) unsafe fn static_field(t: DyadPtr, name: &str) -> Option<DyadPtr> {
    output_type_of(t).filter(|t| name == STATIC_FIELD && !t.is_null())
}

/// What every node of the mint `t` runs; `None` when `t` is no mint.
///
/// # Safety
/// As `is_mint`.
pub(crate) unsafe fn operation_of(t: DyadPtr) -> Option<DyadPtr> {
    is_mint(t).then(|| meta::mint_op_of(t))
}
