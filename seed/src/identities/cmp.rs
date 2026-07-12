//! Concrete comparison operations (mirrors [`crate::identities::add`]). `<` (and
//! its future siblings `>`, `==`, …) are *abstract* operators; the machine-level
//! comparisons live here, one per operand type: `lt_i32` today. `<` resolves to one
//! of these from its operands and stores it (see [`crate::identities::lt`]); run and
//! compile execute the concrete op.
//!
//! A comparison yields the `i32` 0/1 both tiers compute in (its *type* is `bool`,
//! carried by the abstract `<`; the machine value is a first-class i32). A concrete
//! op has no source spelling, so it registers only a run native and a lowering.

use cranelift_codegen::ir::Value;

use super::{operands, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};

/// Register `lt_i32`: the run native and compile lowering, no spelling. Returns the
/// identity so `<` can resolve to it and the parser can hold it in `CoreTypes`.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Run: `1` if the left operand is signed-less-than the right, else `0`, as an i32
/// (matching the JIT's `icmp slt` zero-extended).
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `<` application; its first two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = rt.run(lhs)? as i32;
        let r = rt.run(rhs)? as i32;
        Ok(i64::from((l < r) as i32))
    }
}

/// Lower: `icmp slt` over the lowered operands, zero-extended to the i32 0/1.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `<` application; its first two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = lw.lower(lhs)?;
        let r = lw.lower(rhs)?;
        Ok(lw.icmp_slt(l, r))
    }
}
