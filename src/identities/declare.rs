// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `:=`, the declaration operator, and the `declare` node it builds:
//! `[lhs, rhs, declared, op]`, the binding, the right side as written, and
//! what the seed made of it, real graph structure, so anything downstream
//! sees that a declaration happened without re-reading source. Running one
//! runs `declared` for effect and yields unit.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

const DECL_LHS: usize = 0;
const DECL_RHS: usize = 1;
/// The declared binding, construction initializer or snapshot the run executes.
const DECL_DECLARED: usize = 2;

/// The trie longest-matches `:=` over the binding read `:`.
/// Returns `(declare identity, leaf, := token)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr, DyadPtr) {
    let sides = &["lhs", "rhs"];
    let record = meta::token_record(cx, meta::prec::DECLARE, crate::parse::Assoc::Left, sides);
    let token = cx.store.alloc_head(cx.type_, record);
    cx.declare(":=", token);
    cx.metas.insert(token, |p, _id, tape| p.construct_decl(tape));

    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["lhs", "rhs", "declared", "op"],
    );
    let declare = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(declare, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (declare, leaf, token)
}

pub(crate) fn build(
    store: &mut Store,
    types: &Core,
    lhs: DyadPtr,
    rhs: DyadPtr,
    declared: DyadPtr,
) -> DyadPtr {
    store.alloc_words(types.declare_, &[lhs, rhs, declared, types.ops.declare_])
}

/// # Safety
/// `node` must be a declare node as [`build`] lays it out.
pub(crate) unsafe fn binding_of(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr).add(DECL_LHS)
}

/// # Safety
/// `node` must be a declare node as [`build`] lays it out.
pub(crate) unsafe fn rhs_of(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr).add(DECL_RHS)
}

/// # Safety
/// `node` must be a declare node as [`build`] lays it out.
pub(crate) unsafe fn declared_of(node: DyadPtr) -> DyadPtr {
    *(dyad::value(node) as *const DyadPtr).add(DECL_DECLARED)
}

/// # Safety
/// `node` must be a declare node as [`build`] lays it out.
pub(crate) unsafe fn set_declared(node: DyadPtr, declared: DyadPtr) {
    *(dyad::value(node) as *mut DyadPtr).add(DECL_DECLARED) = declared;
}

/// Whether a declaration has nothing to run: the storage a hole laid out, `x := T ?`, which
/// nothing initializes, or a node with no run, which is data and was built whole at parse.
///
/// # Safety
/// `declared` must be a valid dyad from the store.
unsafe fn runs_nothing(types: &Core, declared: DyadPtr) -> bool {
    types.is_storage(declared)
        || super::read::read_kind(types, declared) == super::read::Read::Aggregate
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid declare node; its declared slot is a valid dyad.
    unsafe {
        let declared = declared_of(node);
        if !runs_nothing(rt.types(), declared) {
            rt.run(declared)?;
        }
    }
    Ok(0)
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run`].
    unsafe {
        let declared = declared_of(node);
        if !runs_nothing(lw.types(), declared) {
            lw.lower(declared)?;
        }
    }
    Ok(lw.const_i32(0))
}
