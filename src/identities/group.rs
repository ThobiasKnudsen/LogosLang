// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A group: `and`/`or` over two non-booleans, `[lhs, rhs]` typed by the connective itself, where
//! two booleans are typed by its mint. Every operator applied to a group applies to each member,
//! and the results join with the group's connective, which collapses to a boolean once the
//! members are booleans. ›`and`/`or` on non-booleans build a group; every operator on a group
//! applies to each member‹ [[R37]]

use super::binary;
use super::numtype::LogicOp;
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::ParseError;
use crate::store::Store;
use crate::Core;

pub(crate) type BuildFn =
    fn(&mut Store, &Core, DyadPtr, DyadPtr, DyadPtr) -> Result<DyadPtr, ParseError>;

/// `(connective, lhs, rhs)` when `node` is a group.
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
pub(crate) unsafe fn members(types: &Core, node: DyadPtr) -> Option<(DyadPtr, DyadPtr, DyadPtr)> {
    let node = types.through(node);
    if node.is_null() || (dyad::ty(node) != types.and_ && dyad::ty(node) != types.or_) {
        return None;
    }
    let p = dyad::value(node) as *const DyadPtr;
    Some((dyad::ty(node), *p, *p.add(1)))
}

/// The connective joins; any other operator over a group applies to each member.
pub(crate) fn apply(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
    build: BuildFn,
) -> Result<DyadPtr, ParseError> {
    if op == types.and_ || op == types.or_ {
        return build(store, types, op, lhs, rhs);
    }
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    if let Some((connective, a, b)) = unsafe { members(types, lhs) } {
        let a = apply(store, types, op, a, rhs, build)?;
        let b = apply(store, types, op, b, rhs, build)?;
        return join(store, types, connective, a, b);
    }
    // SAFETY: as above.
    if let Some((connective, a, b)) = unsafe { members(types, rhs) } {
        let a = apply(store, types, op, lhs, a, build)?;
        let b = apply(store, types, op, lhs, b, build)?;
        return join(store, types, connective, a, b);
    }
    build(store, types, op, lhs, rhs)
}

/// The connective's own constructor: two booleans collapse, two non-booleans stay a group.
pub(crate) fn join(
    store: &mut Store,
    types: &Core,
    connective: DyadPtr,
    a: DyadPtr,
    b: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let l = if connective == types.and_ { LogicOp::And } else { LogicOp::Or };
    // SAFETY: the members and the results built over them are reduced dyads from the store.
    unsafe { binary::build_logic(store, types, connective, l, a, b) }
}
