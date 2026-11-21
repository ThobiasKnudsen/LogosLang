// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The built-in operations' mints: one per set of field types their parse chooses, made once by
//! the operation's chooser and kept by `type_mint`, and the one builder of their nodes. A
//! node's type is its mint, and the mint holds what its nodes share: the type they give back,
//! what they run and the field types it was made for. A Logos-written type mints through a
//! function of its own. ›An operation mints through a chooser, one mint per set of operand
//! types, so each mint's `run` body is parsed once‹ [[R301]]

use super::{array, meta};
use crate::dyad::DyadPtr;
use crate::store::Store;
use crate::Core;

/// The member that holds what a type's nodes give back (›A mint's output type is written in its
/// `static output_type` declaration; a type that does not mint has its parse write it into each
/// node‹ [[R194]]).
pub(crate) const STATIC_FIELD: &str = "output_type";

/// The member that names a built-in operation's chooser, `+.mint(i32, i32)` (›An operation is
/// defined for its operand types by its mint, named through the operation's chooser‹ [[R312]]).
pub(crate) const MEMBER: &str = "mint";

/// What every node of one mint gives back and runs.
pub(crate) struct Made {
    pub output_type: DyadPtr,
    pub op: DyadPtr,
}

/// A built-in operation's chooser: what its mint for the field types `key` gives back and runs,
/// or `None` where the operation has no mint for them.
pub(crate) type ChooseFn = unsafe fn(&mut Store, &Core, &[DyadPtr]) -> Option<Made>;

/// A node of the built-in operation `origin`, its fields `words` in the order of `origin`'s roles,
/// typed by `origin`'s mint for the field types `key`: the one way a built-in operation's node is
/// built. Refused where `origin`'s chooser has no mint for `key`.
///
/// # Safety
/// `origin` must carry a `TUPLE_TAG` operand record of `words.len()` roles; `key` type nodes or
/// null and `words` dyads, all from the store.
pub(crate) unsafe fn node(
    store: &mut Store,
    types: &Core,
    origin: DyadPtr,
    key: &[DyadPtr],
    words: &[DyadPtr],
) -> Result<DyadPtr, crate::parse::ParseError> {
    debug_assert_eq!(meta::arity_of(origin), words.len(), "a mint's node holds its fields alone");
    let mint =
        of(store, types, origin, key).ok_or(crate::parse::ParseError::UnsupportedOperands)?;
    Ok(store.alloc_words(mint, words))
}

/// As [`node`], for an operation whose chooser makes a mint for every key of `key`'s length.
///
/// # Panics
/// Where `origin`'s chooser refuses `key` all the same: a chooser out of step with its parse.
///
/// # Safety
/// As [`node`].
pub(crate) unsafe fn node_of_any(
    store: &mut Store,
    types: &Core,
    origin: DyadPtr,
    key: &[DyadPtr],
    words: &[DyadPtr],
) -> DyadPtr {
    node(store, types, origin, key, words)
        .expect("the operation's chooser makes a mint for every key of this length")
}

/// `origin`'s mint for the field types `key`, made by its chooser the first time and kept by
/// `type_mint` under `origin` and `key`; `None` where `origin` has no chooser or its chooser no
/// mint for `key`.
///
/// # Safety
/// `origin` must be a valid dyad and `key` type nodes or null, all from the store.
pub(crate) unsafe fn of(
    store: &mut Store,
    types: &Core,
    origin: DyadPtr,
    key: &[DyadPtr],
) -> Option<DyadPtr> {
    let chooser = meta::chooser(origin)?;
    super::type_mint::mint_of(store, types, origin, key, |store| {
        let made = chooser(store, types, key)?;
        let key = array::build(store, types.array_, key);
        let record = meta::mint_record(store, origin, made.output_type, made.op, key);
        Some(store.alloc_head(types.type_, record))
    })
}

/// # Safety
/// `t` must be null or a valid dyad from the store.
unsafe fn is_mint(t: DyadPtr) -> bool {
    // A built-in operation's mint is a run of operands; a map type is a record `hashmap` mints.
    !t.is_null() && meta::kind_of(t) == Some(meta::TUPLE_TAG) && !meta::mint_of(t).is_null()
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

/// The field types the mint `t` was made for; `None` when `t` is no mint.
///
/// # Safety
/// As `is_mint`.
pub(crate) unsafe fn key_of<'a>(t: DyadPtr) -> Option<&'a [DyadPtr]> {
    is_mint(t).then(|| array::items(meta::mint_key_of(t)))
}
