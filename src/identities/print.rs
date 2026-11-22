// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `print «…»`: the output word. The node `[parts, op]` writes its quote
//! and a newline to stdout each time it runs, and yields unit: `parts` holds
//! the text runs and the `{…}` expressions, each run and shown as echo shows it.
//! DESIGN ›The command line is Logos source‹

use std::io::Write;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;
use cranelift_codegen::ir::Value;

#[derive(Debug, Clone, Copy)]
pub struct PrintIds {
    pub print: DyadPtr,
    pub leaf: DyadPtr,
}

/// At the rank `lex` and `import` have: a word that consumes raw text to its
/// right at discovery.
pub(crate) fn register(cx: &mut Cx, cs: &Callables) -> PrintIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        crate::parse::Assoc::Left,
        &["parts", "op"],
    );
    let print = cx.store.alloc_head(cx.type_, record);
    cx.declare("print", print);
    cx.metas.insert(print, |p, _id, tape| p.construct_print(tape));
    cx.lower.insert(print, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    PrintIds { print, leaf }
}

pub(crate) fn build(store: &mut Store, types: &Core, parts: &[DyadPtr]) -> DyadPtr {
    let parts = super::array::build(store, types.array_, parts);
    store.alloc_words(types.print.print, &[parts, types.print.leaf])
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    write_line(render(rt, node)?)
}

fn write_line(mut line: Vec<u8>) -> Result<i64, RunError> {
    line.push(b'\n');
    std::io::stdout()
        .lock()
        .write_all(&line)
        .map_err(|e| RunError::Output(Box::new(e.to_string())))?;
    Ok(0)
}

/// The text of a node that shows a quote (`print`, `abort`, a raise), each `{…}` run here.
pub(crate) fn render(rt: &mut Runtime, node: DyadPtr) -> Result<Vec<u8>, RunError> {
    let types: *const Core = rt.types();
    // SAFETY: `node` is a node whose first slot is its parts; the runtime borrows the core,
    // so running a part leaves it where it is.
    unsafe { compose(&*types, node, |part| rt.run(part)) }
}

/// The text of such a node in compiled code, which ran each part itself: `values` holds one
/// container per part, in order.
///
/// # Safety
/// `node` must be a node whose first slot is its parts, `values` a block of one container per
/// part.
pub(crate) unsafe fn render_compiled(
    rt: &Runtime,
    node: DyadPtr,
    values: *const i64,
) -> Result<Vec<u8>, RunError> {
    let mut next = values;
    compose(rt.types(), node, |_| {
        let bits = *next;
        next = next.add(1);
        Ok(bits)
    })
}

/// The text of a node that shows a quote on either tier: each part, a text run or a `{…}`,
/// shown as the echo shows the container `value` gives for it, in order.
///
/// # Safety
/// `node` must be a node whose first slot is its parts.
unsafe fn compose(
    types: &Core,
    node: DyadPtr,
    mut value: impl FnMut(DyadPtr) -> Result<i64, RunError>,
) -> Result<Vec<u8>, RunError> {
    let mut line = Vec::new();
    for &part in parts_of(node) {
        let bits = value(part)?;
        line.extend_from_slice(super::display_value(types, part, bits)?.as_bytes());
    }
    Ok(line)
}

/// The text runs and the `{…}` expressions, in the order written: the array `build` made.
///
/// # Safety
/// `node` must be a node whose first slot is its parts.
unsafe fn parts_of<'a>(node: DyadPtr) -> &'a [DyadPtr] {
    super::array::items(*(dyad::value(node) as *const DyadPtr))
}

/// What compiled code calls with a node that shows a quote and the block of its parts' values.
pub(crate) type ShowEntry =
    unsafe extern "C" fn(*mut crate::run::Context, DyadPtr, *const i64) -> i64;

/// Each part runs in the compiled frame, its value in a block; the text is made, and shown or
/// raised, by calling back into the seed at `entry`.
///
/// # Safety
/// `node` must be a node whose first slot is its parts, each a reduced dyad.
pub(crate) unsafe fn lower_shown(
    lw: &mut Lowerer,
    node: DyadPtr,
    entry: ShowEntry,
) -> Result<Value, CompileError> {
    let values = lower_parts(lw, node)?;
    let node = lw.node_addr(node);
    lw.call_seed(entry as usize, &[node, values])?;
    Ok(lw.const_i32(0))
}

/// The block of one container per part of such a node, each part run in order, which
/// [`render_compiled`] reads.
///
/// # Safety
/// `node` must be a node whose first slot is its parts, each a reduced dyad.
pub(crate) unsafe fn lower_parts(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    let mut values = Vec::new();
    for &part in parts_of(node) {
        let v = lw.lower(part)?;
        values.push(lw.container(part, v));
    }
    Ok(lw.spill(&values))
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a `print` node `build` made; its parts are reduced dyads.
    unsafe { lower_shown(lw, node, compiled_print) }
}

/// # Safety
/// Called only by compiled code, with its context, a `print` node from the store and a block
/// holding one container per part, in order.
unsafe extern "C" fn compiled_print(
    ctx: *mut crate::run::Context,
    node: DyadPtr,
    values: *const i64,
) -> i64 {
    // SAFETY: the caller's contract is `render_compiled`'s.
    crate::run::interpret_step(ctx, |rt| write_line(unsafe { render_compiled(rt, node, values) }?))
}
