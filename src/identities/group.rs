// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A group: `and`/`or` over two non-booleans, `[lhs, rhs, null]`. Every operator applied
//! to a group applies to each member, and the results join with the group's connective,
//! which collapses to a boolean once the members are booleans.
//! DESIGN ›The proof layer‹

use super::{and, or, ran};
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
    let node = ran::expr_of(types, types.through(node));
    if node.is_null() || ((*node).ty != types.and_ && (*node).ty != types.or_) {
        return None;
    }
    let p = (*node).value as *const DyadPtr;
    (*p.add(2)).is_null().then(|| ((*node).ty, *p, *p.add(1)))
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
    if connective == types.and_ {
        and::build(store, types, connective, a, b)
    } else {
        or::build(store, types, connective, a, b)
    }
}
