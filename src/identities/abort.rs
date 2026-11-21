// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `abort «…»`: the fault word. The node `[parts, op, output_type]` never returns: running it
//! ends the run with its quote's text as the reason, `{…}` read as `print` reads it.
//! ›There is no bare `error «…»`; `abort «…»` stops the program; `alloc` returns `@T!`‹ [[R145]]

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, print, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct AbortIds {
    pub abort: DyadPtr,
    pub leaf: DyadPtr,
}

/// At the rank `print` has: a word that consumes raw text to its right at discovery.
pub(crate) fn register(cx: &mut Cx, cs: &Callables) -> AbortIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        crate::parse::Assoc::Left,
        &["parts", "op", "output_type"],
    );
    let abort = cx.store.alloc_head(cx.type_, record);
    cx.declare("abort", abort);
    cx.metas.insert(abort, |p, _id, tape| p.construct_abort(tape));
    cx.lower.insert(abort, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    AbortIds { abort, leaf }
}

pub(crate) fn build(store: &mut Store, types: &Core, parts: &[DyadPtr]) -> DyadPtr {
    let parts = super::array::build(store, types.array_, parts);
    store.alloc_words(types.abort.abort, &[parts, types.abort.leaf, types.void_])
}

fn fault(message: Vec<u8>) -> RunError {
    RunError::Aborted(Box::new(String::from_utf8_lossy(&message).into_owned()))
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    Err(fault(print::render(rt, node)?))
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is an `abort` node `build` made; its parts are reduced dyads.
    unsafe { print::lower_shown(lw, node, compiled_abort) }
}

/// # Safety
/// Called only by compiled code, with its context, an `abort` node from the store and a block
/// holding one container per part, in order.
unsafe extern "C" fn compiled_abort(
    ctx: *mut crate::run::Context,
    node: DyadPtr,
    values: *const i64,
) -> i64 {
    crate::run::interpret_step(ctx, |rt| {
        // SAFETY: the caller's contract is `render_compiled`'s.
        let message = unsafe { print::render_compiled(rt, node, values) }?;
        Err(fault(message))
    })
}
