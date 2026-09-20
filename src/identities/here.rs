// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `here` and `caller`: where a line is written, and where it was called from.
//! `here.scope` folds to the scope open at the appearance; `caller.scope`
//! answers, inside a constructor, the scope of the appearance being built.
//! DESIGN ›Meta-navigation walks the graph; the scope stack is the graph's own spine‹

use super::callable::{self, Callables};
use super::{meta, scope, Cx};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// The two spelled identities, the two nodes `.scope` builds, and their run leaves.
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

/// `here` and `caller` at a literal's rank, nullary words constructed at
/// discovery; the two `.scope` nodes inert, since no spelling reaches them.
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

/// `[scope, op]`, `scope` the scope open at the appearance.
pub(crate) fn build_here(store: &mut Store, types: &Core, scope: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[scope, types.here.here_leaf]);
    store.alloc_raw(types.here.here, value)
}

pub(crate) fn build_caller(store: &mut Store, types: &Core) -> DyadPtr {
    let value = store.alloc_operands(&[types.here.caller_leaf]);
    store.alloc_raw(types.here.caller, value)
}

pub(crate) fn build_caller_scope(store: &mut Store, types: &Core) -> DyadPtr {
    let value = store.alloc_operands(&[types.here.caller_scope_leaf]);
    store.alloc_raw(types.here.caller_scope, value)
}

/// `[scope, op]`, `scope` what stands left of the `.`, read for its address
/// when the node runs.
pub(crate) fn build_scope_of(store: &mut Store, types: &Core, of: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[of, types.here.scope_of_leaf]);
    store.alloc_raw(types.here.scope_of, value)
}

/// # Safety
/// `node` must be a `here` node from the store.
pub(crate) unsafe fn scope_of_here(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr)
}

/// Whether `.scope` on `node` reads a parent link: an `@dyad` value, or a
/// node whose run yields one.
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

/// A `here` value is the spot's own address.
fn run_here(_rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    Ok(node as usize as i64)
}

/// A bare `caller` has no value form in the seed.
fn run_caller(_rt: &mut Runtime, _node: DyadPtr) -> Result<i64, RunError> {
    Err(RunError::CallerSpot)
}

fn run_caller_scope(rt: &mut Runtime, _node: DyadPtr) -> Result<i64, RunError> {
    rt.pass_scope().map(|s| s as usize as i64)
}

/// The operand's address must be a node of the store, never a dereference of
/// bits that are no node.
fn run_scope_of(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `scope_of` node from the store; its operand is a reduced dyad.
    unsafe {
        let of = *((*node).value as *const DyadPtr);
        let addr = rt.run(of)?;
        if addr == 0 {
            return Err(RunError::NullPointer);
        }
        let s = addr as usize as DyadPtr;
        if !rt.store().contains(s) {
            return Err(RunError::NotANode(addr as usize));
        }
        let types = rt.types();
        if (*s).ty == types.here.here {
            return Ok(scope_of_here(s) as usize as i64);
        }
        if (*s).ty != types.scope {
            return Err(RunError::NotAScope(s));
        }
        Ok(scope::parent_of(s) as usize as i64)
    }
}
