// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `for i in a..b ( body )`, `for a..b..d ( body )`: the counted loop, index
//! optional, end-exclusive, start/end/step evaluated once; a value its body gives ends the loop
//! and passes out. The node is `[var, start, end, step-or-null, body]`; the surface parse lives
//! in [`crate::parse::Parser::parse_for`].

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::{self, CmpOp};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};

/// `..` registers escaped (`.` is a regex metacharacter); the trie's longest
/// match keeps it distinct from the field-access `.`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr, DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::READER,
        Assoc::Left,
        &["variable", "start", "end", "step", "body", "op"],
    );
    let for_ = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `for_` carries the record built above, which names the body.
    unsafe { meta::install_hands_on(for_, &["body"], &[]) };
    cx.declare("for", for_);
    cx.metas.insert(for_, |p, id, tape| {
        if !p.finds_right_side(tape) {
            return Ok(crate::parse::Constructed::Decline);
        }
        let node = p.parse_for(id, p.starts_line(tape))?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    cx.lower.insert(for_, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);

    let sides = &["lhs", "rhs"];
    let record = meta::token_record(cx, meta::prec::INERT, crate::parse::Assoc::Left, sides);
    let in_ = cx.store.alloc_head(cx.type_, record);
    cx.declare("in", in_);

    let record = meta::token_record(cx, meta::prec::RANGE, crate::parse::Assoc::Left, sides);
    let range = cx.store.alloc_head(cx.type_, record);
    cx.declare("..", range);

    (for_, leaf, in_, range)
}

pub(crate) const BODY: usize = 4;

/// The step is null when unwritten, meaning 1.
///
/// # Safety
/// `node` must be a `for` node built by [`crate::parse::Parser::parse_for`].
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr, DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p, *p.add(1), *p.add(2), *p.add(3), *p.add(BODY))
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

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `for` node; its parts are valid dyads.
    unsafe { run_given(rt, node).map(|given| given.unwrap_or(0)) }
}

/// A named counter lives in its binding's place, read back each pass since the body may
/// write it; a nameless one is this run's own. Each pass runs the body as a line: a value it
/// gives ends the loop and passes out (›A scope's value is what it evaluates to, and `return` is
/// an optional early exit from the enclosing function‹ [[R207]]).
///
/// # Safety
/// `node` must be a `for` node built by [`crate::parse::Parser::parse_for`].
pub(super) unsafe fn run_given(rt: &mut Runtime, node: DyadPtr) -> Result<Option<i64>, RunError> {
    let (var, start, end, step, body) = parts(node);
    let super::Operand::Concrete(nt) = super::operand_of(rt.types(), start) else {
        return Err(RunError::NoWholeRead);
    };
    let counter = |rt: &mut Runtime| rt.place_addr(var).ok_or(RunError::NoActivation);
    let named = !var.is_null();
    let mut v = rt.run(start)?;
    if named {
        numtype::write_scalar_nt(nt, counter(rt)?, v);
    }
    let e = rt.run(end)?;
    let d = if step.is_null() { one_bits(nt) } else { rt.run(step)? };
    if numtype::apply_compare(CmpOp::Gt, nt, d, 0) == 0 {
        return Ok(None);
    }
    let mark = rt.stack_mark();
    loop {
        if named {
            v = numtype::read_scalar_nt(nt, counter(rt)?);
        }
        if numtype::apply_compare(CmpOp::Lt, nt, v, e) == 0 {
            break;
        }
        if let Some(value) = rt.run_line(body)? {
            return Ok(Some(value));
        }
        rt.stack_release(mark);
        // A wrapped counter would satisfy `var < end` again.
        let Some(next) = numtype::checked_add(nt, v, d) else {
            break;
        };
        v = next;
        if named {
            numtype::write_scalar_nt(nt, counter(rt)?, next);
        }
    }
    Ok(None)
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `for` node; its parts are valid dyads.
    lw.lower_as_value(|lw| unsafe { lower_given(lw, node) })
}

/// [`run_given`]'s lowering: a value the body gives ends the loop and passes out.
///
/// # Safety
/// `node` must be a valid `for` node; its parts valid dyads.
pub(super) unsafe fn lower_given(lw: &mut Lowerer, node: DyadPtr) -> Result<(), CompileError> {
    let (var, start, end, step, body) = parts(node);
    lw.lower_for(var, start, end, step, body)
}
