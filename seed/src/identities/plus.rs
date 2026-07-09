//! `+`: i32 addition. A function (its type is `fn`), left-associative, binding
//! tighter than `=`. Parse folds it as an infix binary, run sums its operands,
//! compile lowers it to `iadd`.

use cranelift_codegen::ir::Value;

use super::{build_binary, operands, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct};
use crate::run::{RunError, Runtime};

/// Register `+`: spelling, parse precedence, run bcode, and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("+", IdContext::new(id, cx.root_scope));
    cx.metas
        .insert(id, Construct::Infix { precedence: 2.0, assoc: Assoc::Left, build: build_binary });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Run: sum both operands. v1 scalars are `i32`, and the JIT lowers `+` to a
/// 32-bit `iadd` that wraps on overflow, so the interpreter must compute in `i32`
/// with the same wrapping — otherwise a sum past `i32::MAX` would diverge from the
/// compiled result and break the interpreter-is-the-oracle guarantee.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid application dyad, so its operands are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = rt.run(lhs)? as i32;
        let r = rt.run(rhs)? as i32;
        Ok(i64::from(l.wrapping_add(r)))
    }
}

/// Lower: `iadd` over the lowered operands.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid application dyad, so its operands are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = lw.lower(lhs)?;
        let r = lw.lower(rhs)?;
        Ok(lw.add(l, r))
    }
}
