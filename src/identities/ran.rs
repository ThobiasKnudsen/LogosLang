// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `ran`: an item that has run in the pass, kept in the graph with its result:
//! `[expr, value, op]`, rewritten in place so every holder of the item's
//! address sees the ran form and the scope's run reads it instead of running it again.
//! DESIGN ›Build and run are one self-directing pass‹

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};

const EXPR: usize = 0;
const VALUE: usize = 1;

/// Returns `(identity, leaf)`; the identity has no spelling.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["expr", "value", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    cx.lower.insert(id, lower);
    (id, leaf)
}

/// The item a ran node holds, or `node` itself for anything else: the hop for
/// readers that ask what an expression is, not what its value reads as.
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
pub unsafe fn expr_of(types: &Core, node: DyadPtr) -> DyadPtr {
    if node.is_null() || dyad::ty(node) != types.ran_ {
        return node;
    }
    *(dyad::value(node) as *const DyadPtr).add(EXPR)
}

/// # Safety
/// `node` must be a ran node as [`build`] lays it out.
pub unsafe fn value_of(node: DyadPtr) -> i64 {
    let cell = *(dyad::value(node) as *const DyadPtr).add(VALUE);
    std::ptr::read_unaligned(dyad::value(cell) as *const i64)
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a ran node; its value slot is the cell, a scalar node.
    unsafe {
        let cell = *(dyad::value(node) as *const DyadPtr).add(VALUE);
        rt.run(cell)
    }
}

/// The cell read on both tiers: a scope compiled after the pass reads the
/// result it kept.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run`].
    unsafe {
        let cell = *(dyad::value(node) as *const DyadPtr).add(VALUE);
        lw.lower(cell)
    }
}
