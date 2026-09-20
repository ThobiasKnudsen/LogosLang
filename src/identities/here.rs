// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `here` and `caller`: where a line is written, and where it was called from
//! (#123; DESIGN ›Meta-navigation walks the graph; the scope stack is the
//! graph's own spine‹, ruled 15 September 2026: "`here` is the spot a line
//! stands at and `here.scope` the scope that spot is in, `here.scope.scope`
//! the one above, and so up to the arche, whose `.scope` is null; `caller` is
//! the same for the call — the node a function was called from, and for a
//! constructor the appearance of its identity … so inside a constructor
//! `caller.scope` is the scope the tape belongs to, the use site, and
//! `here.scope` the constructor's own body, the definition site").
//!
//! Both are ordinary identities held by the arche. `here` is placed at
//! discovery with the scope open at its appearance in its first slot, and
//! `here.scope` folds to that scope as an `@dyad` address value, the form
//! `x:scope` already takes (›The dyad's read surface‹, seed note). `caller`
//! reads the pass: `caller.scope` becomes a node that, when a constructor
//! runs it, answers the scope open at the appearance being constructed
//! ([`Runtime::pass_scope`]); anywhere else it is the checked error, the
//! seed's stand-in for the per-call read of an ordinary function. A scope's
//! own `.scope` — on any `@dyad` value: `here.scope.scope`,
//! `caller.scope.scope`, `x:scope.scope` — is a [`build_scope_of`] node
//! reading the parent link when it runs: null at the arche,
//! [`RunError::NullPointer`] past it, [`RunError::BadValue`] over a node that
//! is no scope. Nothing here lowers.

use super::callable::{self, Callables};
use super::{meta, scope, Cx};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// The handles: the two spelled identities, the two nodes `.scope` builds,
/// and their run leaves.
#[derive(Debug, Clone, Copy)]
pub struct HereIds {
    pub here: DyadPtr,
    pub here_leaf: DyadPtr,
    pub caller: DyadPtr,
    pub caller_leaf: DyadPtr,
    /// `caller.scope`: never spelled, built by `.` over a `caller` node.
    pub caller_scope: DyadPtr,
    pub caller_scope_leaf: DyadPtr,
    /// A scope's `.scope`: never spelled, built by `.` over a scope address.
    pub scope_of: DyadPtr,
    pub scope_of_leaf: DyadPtr,
}

/// Register the four: `here` and `caller` at a literal's rank — nullary
/// words constructed at discovery — and the two `.scope` nodes inert, since
/// no spelling reaches them.
pub(crate) fn register(cx: &mut Cx, cs: &Callables) -> HereIds {
    let word = |cx: &mut Cx, rank: f64, roles: &[&str]| {
        let record =
            meta::operand_record(cx, meta::TUPLE_TAG, rank, crate::parse::Assoc::Left, roles);
        cx.store.alloc_raw(cx.type_, record)
    };
    let here = word(cx, meta::prec::LITERAL, &["scope", "op"]);
    cx.declare("here", here);
    cx.metas.insert(here, |p, _id, tape| p.construct_here(tape));
    let here_leaf = callable::mint_native(cx.store, cs.callable, run_here, cs.seed_native);
    let caller = word(cx, meta::prec::LITERAL, &["op"]);
    cx.declare("caller", caller);
    cx.metas.insert(caller, |p, _id, tape| p.construct_caller(tape));
    let caller_leaf = callable::mint_native(cx.store, cs.callable, run_caller, cs.seed_native);
    let caller_scope = word(cx, meta::prec::INERT, &["op"]);
    let caller_scope_leaf =
        callable::mint_native(cx.store, cs.callable, run_caller_scope, cs.seed_native);
    let scope_of = word(cx, meta::prec::INERT, &["scope", "op"]);
    let scope_of_leaf = callable::mint_native(cx.store, cs.callable, run_scope_of, cs.seed_native);
    HereIds {
        here,
        here_leaf,
        caller,
        caller_leaf,
        caller_scope,
        caller_scope_leaf,
        scope_of,
        scope_of_leaf,
    }
}

/// The node `here` places: `[scope, op]`, `scope` the scope open at its
/// appearance.
pub(crate) fn build_here(store: &mut Store, types: &Core, scope: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[scope, types.here.here_leaf]);
    store.alloc_raw(types.here.here, value)
}

/// The node `caller` places: `[op]`.
pub(crate) fn build_caller(store: &mut Store, types: &Core) -> DyadPtr {
    let value = store.alloc_operands(&[types.here.caller_leaf]);
    store.alloc_raw(types.here.caller, value)
}

/// The node `caller.scope` builds: `[op]`, answering the pass's position
/// when a constructor runs it.
pub(crate) fn build_caller_scope(store: &mut Store, types: &Core) -> DyadPtr {
    let value = store.alloc_operands(&[types.here.caller_scope_leaf]);
    store.alloc_raw(types.here.caller_scope, value)
}

/// The node a scope address's `.scope` builds: `[scope, op]`, `scope` what
/// stands left of the `.`, read for its address when the node runs.
pub(crate) fn build_scope_of(store: &mut Store, types: &Core, of: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[of, types.here.scope_of_leaf]);
    store.alloc_raw(types.here.scope_of, value)
}

/// The scope slot of a `here` node.
///
/// # Safety
/// `node` must be a `here` node from the store.
pub(crate) unsafe fn scope_of_here(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr)
}

/// Whether `node` stands as a scope's address, so that `.scope` on it reads
/// the parent link: an `@dyad` value — the address literal `x:scope` and
/// `here.scope` yield, or a pointer place holding one — or a node whose run
/// yields one, `caller.scope` and another `.scope`.
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
pub(crate) unsafe fn yields_scope_address(types: &Core, node: DyadPtr) -> bool {
    let d = types.through(node);
    if d.is_null() {
        return false;
    }
    let h = types.here;
    if (*d).ty == h.caller_scope || (*d).ty == h.scope_of {
        return true;
    }
    matches!(super::read::read_kind(types, d), super::read::Read::Pointer(p) if p == types.dyad_)
}

/// Run `here`: the spot, its own address.
fn run_here(_rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    Ok(node as usize as i64)
}

/// Run a bare `caller`: the appearance's spot has no value form in the seed.
fn run_caller(_rt: &mut Runtime, _node: DyadPtr) -> Result<i64, RunError> {
    Err(RunError::CallerSpot)
}

/// Run `caller.scope`: the scope open at the appearance a constructor is
/// building.
fn run_caller_scope(rt: &mut Runtime, _node: DyadPtr) -> Result<i64, RunError> {
    rt.pass_scope().map(|s| s as usize as i64)
}

/// Run a scope's `.scope`: read the operand for its address, which must be a
/// node of the store — a scope, whose parent link is the answer, or a `here`
/// spot, whose scope is — and never a dereference of bits that are no node.
fn run_scope_of(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `scope_of` node from the store; its operand is a
    // reduced dyad.
    unsafe {
        let of = *((*node).value as *const DyadPtr);
        let addr = rt.run(of)?;
        if addr == 0 {
            return Err(RunError::NullPointer);
        }
        let s = addr as usize as DyadPtr;
        if !rt.store().contains(s) {
            return Err(RunError::BadValue);
        }
        let types = rt.types();
        if (*s).ty == types.here.here {
            return Ok(scope_of_here(s) as usize as i64);
        }
        if (*s).ty != types.scope {
            return Err(RunError::BadValue);
        }
        Ok(scope::parent_of(s) as usize as i64)
    }
}
