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

/// Run: sum both operands.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid application dyad, so its operands are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        Ok(rt.run(lhs)? + rt.run(rhs)?)
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
