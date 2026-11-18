// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The one mint maker: a type whose field is `?` makes one mint per set of field types its parse
//! chooses, built once and kept on the type. A node's type is its mint, and the mint holds what its
//! nodes share: the type they give back, what they run, and its own `share` members.
//! ›A type with a `run` whose field is `?` mints too, so its `run` body is parsed once per mint‹
//! [[R301]]

use super::{array, meta};
use crate::dyad::{self, DyadPtr};
use crate::store::Store;
use crate::Core;

/// The `share` member whose value a Logos-written type's mint keeps as what its nodes give back
/// (›A mint's output type is written in its `share output_type` declaration; a type that does
/// not mint has its parse write it into each node‹ [[R194]]).
pub(crate) const SHARED_FIELD: &str = "output_type";

/// A built-in's mint for the field types `key`, made the first time and kept on `origin`. The
/// parse that chooses it gives the type its nodes give back and the operation they run; the
/// parse choosing the operation is a stand-in for #69.
///
/// # Safety
/// `origin` must carry a `TUPLE_TAG` operand record; `key` and `output_type` null or type nodes,
/// `op` a callable leaf, all from the store.
pub(crate) unsafe fn of(
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

/// `node` made a node of its type's mint for the field types `key`, the mint made the first
/// time. A Logos-written type's mint begins with neither word: its `share output_type` line
/// gives what its nodes give back, and its run is built for it after.
///
/// # Safety
/// `node` must be a node of a type `takes_mint` accepts, or of one of its mints; `key` one type
/// per field, from the store.
pub(crate) unsafe fn stamp(
    store: &mut Store,
    types: &Core,
    node: DyadPtr,
    key: &[DyadPtr],
) -> DyadPtr {
    let origin = origin(dyad::ty(node));
    let mint = match find(origin, key) {
        Some(mint) => mint,
        None => make(store, types, origin, key, std::ptr::null_mut(), std::ptr::null_mut()),
    };
    debug_assert!(
        dyad::ty(node) == origin || dyad::ty(node) == mint,
        "a node's field types choose its mint once"
    );
    dyad::set_ty(node, mint);
    mint
}

/// Whether a node of `ty` takes a mint as its type: `ty` is a Logos-written type with a `?`
/// field, or one of its mints (›A type without a `run` whose field is `?` mints one type per set
/// of field types, as `array` mints one per element type‹ [[R300]]).
///
/// # Safety
/// `ty` must be null or a valid dyad from the store.
pub(crate) unsafe fn takes_mint(ty: DyadPtr) -> bool {
    !ty.is_null()
        && meta::kind_of(ty) == Some(meta::RECORD_TAG)
        && has_untyped_field(array::items(meta::record_fields_of(ty)))
}

/// # Safety
/// `fields` must be field dyads from the store.
pub(crate) unsafe fn has_untyped_field(fields: &[DyadPtr]) -> bool {
    fields.iter().any(|&f| super::hole::type_in(f).is_null())
}

/// # Safety
/// As `of`, for a `TUPLE_TAG` or `RECORD_TAG` origin.
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
/// for‹ [[R279]]). `None` when `t` is no mint, keeps no such field, or its line gave no type.
///
/// # Safety
/// As `is_mint`.
pub(crate) unsafe fn shared_field(t: DyadPtr, name: &str) -> Option<DyadPtr> {
    output_type_of(t).filter(|t| name == SHARED_FIELD && !t.is_null())
}

/// The scope the mint `t`'s own `share` members are declared in; `None` when `t` is no mint or
/// its members are not made yet.
///
/// # Safety
/// As `is_mint`.
pub(crate) unsafe fn shares_of(t: DyadPtr) -> Option<DyadPtr> {
    is_mint(t).then(|| meta::mint_shares_of(t)).filter(|s| !s.is_null())
}

/// Whether `t`, or the type it is a mint of, keeps a `share` member `name` for each mint to
/// make for itself.
///
/// # Safety
/// As `is_mint`.
pub(crate) unsafe fn keeps_share(t: DyadPtr, name: &str) -> bool {
    let origin = origin(t);
    if t.is_null() || meta::kind_of(origin) != Some(meta::RECORD_TAG) {
        return false;
    }
    let kept = meta::kept_shares_of(origin);
    !kept.is_null()
        && array::items(kept)
            .iter()
            .any(|&pair| super::string::text(array::items(pair)[0]) == name.as_bytes())
}

/// What every node of the mint `t` runs; `None` when `t` is no mint.
///
/// # Safety
/// As `is_mint`.
pub(crate) unsafe fn operation_of(t: DyadPtr) -> Option<DyadPtr> {
    is_mint(t).then(|| meta::mint_op_of(t))
}
