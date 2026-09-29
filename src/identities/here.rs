// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `here` and `caller`: where a line is written, and where it was called from.
//! `here.scope` folds to the scope open at the appearance; `caller.scope`
//! answers, inside a constructor, the scope of the appearance being built;
//! a scope's `.back` is the scope it stands in.
//! DESIGN ›Meta-navigation walks the graph; the scope stack is the graph's own spine‹

use super::callable::{self, Callables};
use super::{meta, scope, Cx};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// The two spelled identities, the `caller.scope` and `.back` nodes, and their run leaves.
#[derive(Debug, Clone, Copy)]
pub struct HereIds {
    pub here: DyadPtr,
    pub here_leaf: DyadPtr,
    pub caller: DyadPtr,
    pub caller_leaf: DyadPtr,
    /// `caller.scope`: never spelled, built by `.` over a `caller` node.
    pub caller_scope: DyadPtr,
    pub caller_scope_leaf: DyadPtr,
    /// A scope's `.back`: never spelled, built by `.` over a scope address.
    pub back: DyadPtr,
    pub back_leaf: DyadPtr,
}

/// `here` and `caller` at a literal's rank, nullary words constructed at
/// discovery; `caller.scope` and `.back` inert, since no spelling reaches them.
pub(crate) fn register(cx: &mut Cx, cs: &Callables) -> HereIds {
    let word = |cx: &mut Cx, rank: f64, roles: &[&str]| {
        let record =
            meta::operand_record(cx, meta::TUPLE_TAG, rank, crate::parse::Assoc::Left, roles);
        cx.store.alloc_head(cx.type_, record)
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
    let back = word(cx, meta::prec::INERT, &["scope", "op"]);
    let back_leaf = callable::mint_native(cx.store, cs.callable, run_back, cs.seed_native);
    HereIds {
        here,
        here_leaf,
        caller,
        caller_leaf,
        caller_scope,
        caller_scope_leaf,
        back,
        back_leaf,
    }
}

/// `[scope, op]`, `scope` the scope open at the appearance.
pub(crate) fn build_here(store: &mut Store, types: &Core, scope: DyadPtr) -> DyadPtr {
    store.alloc_words(types.here.here, &[scope, types.here.here_leaf])
}

pub(crate) fn build_caller(store: &mut Store, types: &Core) -> DyadPtr {
    store.alloc_words(types.here.caller, &[types.here.caller_leaf])
}

pub(crate) fn build_caller_scope(store: &mut Store, types: &Core) -> DyadPtr {
    store.alloc_words(types.here.caller_scope, &[types.here.caller_scope_leaf])
}

/// `[scope, op]`, `scope` what stands left of the `.`, read for its address
/// when the node runs.
pub(crate) fn build_back(store: &mut Store, types: &Core, of: DyadPtr) -> DyadPtr {
    store.alloc_words(types.here.back, &[of, types.here.back_leaf])
}

/// # Safety
/// `node` must be a `here` node from the store.
pub(crate) unsafe fn scope_of_here(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr)
}

/// Whether `.back` on `node` reads a parent link: an `@dyad` value, or a
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
    if dyad::ty(d) == h.caller_scope || dyad::ty(d) == h.back {
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

fn run_back(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `back` node from the store; its operand is a reduced dyad.
    unsafe {
        let of = *(dyad::value(node) as *const DyadPtr);
        let addr = rt.run(of)?;
        if addr == 0 {
            return Err(RunError::NullPointer);
        }
        let s = addr as usize as DyadPtr;
        let types = rt.types();
        if dyad::ty(s) == types.here.here {
            return Ok(scope_of_here(s) as usize as i64);
        }
        if dyad::ty(s) != types.scope {
            return Err(RunError::NotAScope(s));
        }
        Ok(scope::parent_of(s) as usize as i64)
    }
}
