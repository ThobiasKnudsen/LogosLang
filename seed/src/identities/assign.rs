//! `=`: assignment. A function (its type is `fn`), right-associative, binding
//! loosest. Run evaluates the right operand and writes it into the left operand's
//! storage, yielding the value; compile lowers it to a store.

use cranelift_codegen::ir::Value;

use super::{build_binary, operands, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct};
use crate::run::{RunError, Runtime};

/// Register `=`: spelling, parse precedence, run bcode, and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("=", IdContext::new(id, cx.root_scope));
    cx.metas
        .insert(id, Construct::Infix { precedence: 1.0, assoc: Assoc::Right, build: build_binary });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Run: evaluate the right operand, write it into the left operand's i32 storage,
/// and yield the assigned value. The stored `i32` is what the expression yields —
/// returning the untruncated i64 would make `(a = X)` evaluate to a different
/// number than `a` now holds when `X` is outside i32 range.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid application dyad, so its operands are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        let value = rt.run(rhs)?;
        let slot = (*lhs).value as *mut i32;
        if slot.is_null() {
            return Err(RunError::BadValue);
        }
        let stored = value as i32;
        std::ptr::write_unaligned(slot, stored);
        Ok(i64::from(stored))
    }
}

/// Lower: store the right operand into the left operand's baked storage. Guards a
/// null storage address, mirroring the interpreter's `BadValue` — without it the
/// compiler would bake a store to address 0 and SIGSEGV at call time where the
/// interpreter cleanly errors, breaking interpreter/JIT parity.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid application dyad, so its operands are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        if (*lhs).value.is_null() {
            return Err(CompileError::BadValue);
        }
        let v = lw.lower(rhs)?;
        lw.store_i32((*lhs).value, v);
        Ok(v)
    }
}
