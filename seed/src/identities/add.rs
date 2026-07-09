//! Concrete addition operations. `+` is an *abstract* operator (DESIGN ›which
//! concrete machine operation runs, which `+`, is resolved from the operand
//! types‹); the actual machine-level additions live here, one per operand type:
//! `add_i32` today, `add_f32` / `add_u64` / `add_i32_imm` later. `+` resolves to
//! one of these from its operands and stores it in its own value (see
//! [`crate::identities::plus`]); run and compile execute the concrete op, never `+`.
//!
//! A concrete op has no source spelling (it is never written, only resolved to),
//! so it registers only a run native and a compile lowering, keyed by its identity.
//! It reads the operands from the `+` node it is dispatched over — the operand
//! struct's first two fields — so it is agnostic to that node's own operation.

use cranelift_codegen::ir::Value;

use super::{operands, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};

/// Register `add_i32`: the run native and compile lowering, no spelling. Returns
/// the identity so `+` can resolve to it and the parser can hold it in `CoreTypes`.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Run: sum both operands in wrapping `i32` (matching the JIT's `iadd`), so the
/// interpreter stays the compiler's oracle across overflow.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `+` application; its first two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = rt.run(lhs)? as i32;
        let r = rt.run(rhs)? as i32;
        Ok(i64::from(l.wrapping_add(r)))
    }
}

/// Lower: `iadd` over the lowered operands.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `+` application; its first two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = lw.lower(lhs)?;
        let r = lw.lower(rhs)?;
        Ok(lw.add(l, r))
    }
}
