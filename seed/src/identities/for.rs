// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `for i in a..b ( body )` and `for i in a..b..d ( body )`: the counted loop,
//! with the surface the old prototype settled (examples/*.logos there). The
//! range is **end-exclusive** (`0..10` runs 0 through 9) with an optional step
//! `d` (default 1), and start/end/step are evaluated **once**, before the loop.
//! The loop variable is a fresh block-local of the range's resolved numeric type
//! (endpoints resolve like a binary operator's operands: concrete types must
//! match, literals commit, all-literals default to i32). A non-positive step
//! runs zero iterations — the guard both tiers emit — and a *literal* step must
//! be positive at parse. Like `while`, `for` is a statement yielding unit.
//!
//! Node: `{ty: for, value: [var, start, end, step-or-null, body]}`. The surface
//! parse lives in [`crate::parse::Parser::parse_for`]; here we register the
//! identity, the structural `in` and `..` tokens it consumes, and the run and
//! lowering. Deferred, deliberately: ranges as first-class values, multi-variable
//! and `in`-less forms, and the prototype's `gpu` loops.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::{self, ArithOp, CmpOp};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Schedule};
use crate::run::{RunError, Runtime};

/// Register `for` (the loop keyword, its run native, and its lowering) plus the
/// structural `in` and `..` tokens the parser consumes. `..` registers escaped
/// (`.` is a regex metacharacter); the trie's longest-match keeps it distinct
/// from the field-access `.` and from a rational's fractional part.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr, DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        0.0,
        Assoc::Left,
        Schedule::For,
        &["variable", "start", "end", "step", "body", "op"],
    );
    let for_ = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("for", IdContext::new(for_, cx.root_scope));
    cx.metas.insert(for_, |p, id, _tape| p.parse_for(id).map(crate::parse::Constructed::Node));
    cx.lower.insert(for_, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);

    let record = meta::record(cx.store, meta::TOKEN_TAG, Schedule::In);
    let in_ = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("in", IdContext::new(in_, cx.root_scope));

    let record = meta::record(cx.store, meta::TOKEN_TAG, Schedule::DotDot);
    let range = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(r"\.\.", IdContext::new(range, cx.root_scope));

    (for_, leaf, in_, range)
}

/// The `(var, start, end, step, body)` operands of a `for` node (step null when
/// unwritten, meaning 1).
///
/// # Safety
/// `node` must be a `for` node built by [`crate::parse::Parser::parse_for`].
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr, DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
    (*p, *p.add(1), *p.add(2), *p.add(3), *p.add(4))
}

/// The default step's bit-container: 1 in the loop type (1.0 for floats).
fn one_bits(nt: numtype::NumType) -> i64 {
    use numtype::NumType::*;
    match nt {
        F32 => i64::from(1.0f32.to_bits()),
        F64 => 1.0f64.to_bits() as i64,
        _ => 1,
    }
}

/// Run: evaluate start (written to the variable), end, and step once; then
/// rerun the body and increment while `var < end`. A non-positive step runs
/// zero iterations. Yields unit.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `for` node; its parts are valid dyads.
    unsafe {
        let (var, start, end, step, body) = parts(node);
        let nt = numtype::of_type_node((*var).ty);
        let s = rt.run(start)?;
        let var_ty = (*var).ty;
        numtype::write_scalar(var_ty, rt.place_addr(var).ok_or(RunError::BadValue)?, s);
        let e = rt.run(end)?;
        let d = if step.is_null() { one_bits(nt) } else { rt.run(step)? };
        if numtype::apply_compare(CmpOp::Gt, nt, d, 0) == 0 {
            return Ok(0);
        }
        loop {
            let v = numtype::read_scalar(var_ty, rt.place_addr(var).ok_or(RunError::BadValue)?);
            if numtype::apply_compare(CmpOp::Lt, nt, v, e) == 0 {
                break;
            }
            rt.run(body)?;
            let next = numtype::apply_arith(ArithOp::Add, nt, v, d);
            numtype::write_scalar(var_ty, rt.place_addr(var).ok_or(RunError::BadValue)?, next);
        }
        Ok(0)
    }
}

/// Lower: see [`Lowerer::lower_for`].
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `for` node; its parts are valid dyads.
    unsafe {
        let (var, start, end, step, body) = parts(node);
        lw.lower_for(var, start, end, step, body)
    }
}
