//! Concrete comparison operations (mirrors [`crate::identities::add`]). The abstract
//! comparison operators (`<`, `>`, `==`, `<=`, `>=`, `!=`) are DESIGN's higher-level
//! identities; the machine-level comparisons live here, one per operand type
//! (`*_i32` today, `*_f32`/… later). Each abstract operator resolves to one of these
//! from its operands and stores it (see [`crate::identities::lt`] and its siblings);
//! run and compile execute the concrete op.
//!
//! A comparison yields the `i32` 0/1 both tiers compute in (its *type* is `bool`,
//! carried by the abstract operator; the machine value is a first-class i32). A
//! concrete op has no source spelling, so it registers only a run native and a
//! lowering, keyed by its identity. The six ops share one run/lower skeleton, each
//! parameterized by its comparison (a Rust operator on the interpreter, an `icmp`
//! condition on the JIT).

use cranelift_codegen::ir::Value;

use super::{operands, Cx};
use crate::compile::{CompileError, LowerFn, Lowerer};
use crate::dyad::DyadPtr;
use crate::run::{RunError, RunFn, Runtime};

/// The concrete i32 comparison ops the abstract operators resolve to.
pub(super) struct Concrete {
    pub lt_i32: DyadPtr,
    pub gt_i32: DyadPtr,
    pub eq_i32: DyadPtr,
    pub le_i32: DyadPtr,
    pub ge_i32: DyadPtr,
    pub ne_i32: DyadPtr,
}

/// Register every concrete i32 comparison (run native + lowering, no spelling).
/// Returns their identities so the abstract operators can resolve to them and the
/// parser can hold them in `CoreTypes`.
pub(super) fn register(cx: &mut Cx) -> Concrete {
    Concrete {
        lt_i32: op(cx, run_lt, lower_lt),
        gt_i32: op(cx, run_gt, lower_gt),
        eq_i32: op(cx, run_eq, lower_eq),
        le_i32: op(cx, run_le, lower_le),
        ge_i32: op(cx, run_ge, lower_ge),
        ne_i32: op(cx, run_ne, lower_ne),
    }
}

/// Register one concrete op with its run native and lowering.
fn op(cx: &mut Cx, run: RunFn, lower: LowerFn) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Run a comparison: evaluate both operands as `i32` and apply `op`, yielding the
/// i32 0/1 (matching the JIT's `icmp` zero-extended).
fn cmp_run<F: Fn(i32, i32) -> bool>(
    rt: &mut Runtime,
    node: DyadPtr,
    op: F,
) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid comparison application; its first two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = rt.run(lhs)? as i32;
        let r = rt.run(rhs)? as i32;
        Ok(i64::from(op(l, r) as i32))
    }
}

/// Lower a comparison: lower both operands and emit its `icmp` (zero-extended to i32).
fn cmp_lower<F: Fn(&mut Lowerer, Value, Value) -> Value>(
    lw: &mut Lowerer,
    node: DyadPtr,
    emit: F,
) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid comparison application; its first two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = lw.lower(lhs)?;
        let r = lw.lower(rhs)?;
        Ok(emit(lw, l, r))
    }
}

fn run_lt(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    cmp_run(rt, node, |l, r| l < r)
}
fn run_gt(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    cmp_run(rt, node, |l, r| l > r)
}
fn run_eq(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    cmp_run(rt, node, |l, r| l == r)
}
fn run_le(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    cmp_run(rt, node, |l, r| l <= r)
}
fn run_ge(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    cmp_run(rt, node, |l, r| l >= r)
}
fn run_ne(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    cmp_run(rt, node, |l, r| l != r)
}

fn lower_lt(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    cmp_lower(lw, node, |lw, l, r| lw.icmp_slt(l, r))
}
fn lower_gt(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    cmp_lower(lw, node, |lw, l, r| lw.icmp_sgt(l, r))
}
fn lower_eq(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    cmp_lower(lw, node, |lw, l, r| lw.icmp_eq(l, r))
}
fn lower_le(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    cmp_lower(lw, node, |lw, l, r| lw.icmp_sle(l, r))
}
fn lower_ge(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    cmp_lower(lw, node, |lw, l, r| lw.icmp_sge(l, r))
}
fn lower_ne(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    cmp_lower(lw, node, |lw, l, r| lw.icmp_ne(l, r))
}
