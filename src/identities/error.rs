// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `error`: the word of error values. `error.X := «description»` declares the category `X`, a
//! name of the scope it stands in, and `error.X «message»` raises it: the node
//! `[parts, op, output_type, category]`, `{…}` read as `print` reads it. Its run ends the run
//! as a fault, the stand-in for #159's error value.
//! ›Error values are in v0.1.0; an error names a category, which is a scoped name‹ [[R143]]

use cranelift_codegen::ir::Value;

use super::binding::Binding;
use super::callable::{self, Callables};
use super::{meta, print, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// The binding record of the category a raise names.
const CATEGORY: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct ErrorIds {
    pub error: DyadPtr,
    pub leaf: DyadPtr,
}

/// At the rank `lex` and `print` have: a word that reads its own right side at discovery.
pub(crate) fn register(cx: &mut Cx, cs: &Callables) -> ErrorIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        crate::parse::Assoc::Left,
        &["parts", "op", "output_type", "category"],
    );
    let error = cx.store.alloc_head(cx.type_, record);
    cx.declare("error", error);
    cx.metas.insert(error, |p, _id, tape| p.construct_error(tape));
    cx.lower.insert(error, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    ErrorIds { error, leaf }
}

/// A raise of `category`, the binding record its declaration made.
pub(crate) fn build(
    store: &mut Store,
    types: &Core,
    parts: &[DyadPtr],
    category: DyadPtr,
) -> DyadPtr {
    let parts = super::array::build(store, types.array_, parts);
    store.alloc_words(types.error.error, &[parts, types.error.leaf, types.void_, category])
}

/// Whether `binding` is a category's record: a name `error` goes by, spelled with its path, so a
/// lex of it wakes `error`'s parse.
///
/// # Safety
/// `binding` must be null or a binding dyad from the store.
pub(crate) unsafe fn is_category(types: &Core, binding: DyadPtr) -> bool {
    !binding.is_null()
        && Binding::read(binding).dyad == types.error.error
        && Binding::spelling(binding).starts_with("error.")
}

/// Whether a node of type `ty` ends the run where it runs: a raise or an `abort`.
pub(crate) fn ends_run(types: &Core, ty: DyadPtr) -> bool {
    ty == types.error.error || ty == types.abort.abort
}

/// # Safety
/// `node` must be a raise `build` made.
unsafe fn fault(node: DyadPtr, message: Vec<u8>) -> RunError {
    let category = *(dyad::value(node) as *const DyadPtr).add(CATEGORY);
    let message = String::from_utf8_lossy(&message).into_owned();
    RunError::Raised(Box::new((Binding::spelling(category), message)))
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    let message = print::render(rt, node)?;
    // SAFETY: `node` is a raise `build` made.
    Err(unsafe { fault(node, message) })
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a raise `build` made; its parts are reduced dyads.
    unsafe { print::lower_shown(lw, node, compiled_raise) }
}

/// # Safety
/// Called only by compiled code, with its context, a raise from the store and a block holding
/// one container per part, in order.
unsafe extern "C" fn compiled_raise(
    ctx: *mut crate::run::Context,
    node: DyadPtr,
    values: *const i64,
) -> i64 {
    crate::run::interpret_step(ctx, |rt| {
        // SAFETY: the caller's contract is `render_compiled`'s and `fault`'s.
        Err(unsafe { fault(node, print::render_compiled(rt, node, values)?) })
    })
}
