// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `not`: negation of a `bool`, the node `[operand, op]`. Run yields `1` when
//! the operand is false; compile lowers it as `operand == 0`.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};

/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::NOT,
        Assoc::Right,
        &["operand", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("not", id);
    cx.metas.insert(id, |p, id, tape| {
        let operand = p.take_right(tape)?;
        // SAFETY: `operand` is the constructed cell just taken off the tape.
        let node = unsafe { p.build_not(id, operand) }?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    cx.lower.insert(id, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

/// # Safety
/// `node` must be a `not` node `[operand, op]`.
unsafe fn operand(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr)
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `not` node; its first slot is its operand.
    unsafe { Ok(i64::from((rt.run(operand(node))? == 0) as i32)) }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `not` node; its first slot is its operand.
    unsafe {
        let a = lw.lower(operand(node))?;
        let zero = lw.const_i32(0);
        Ok(lw.icmp_eq(a, zero))
    }
}
