//! `not (x)`: logical negation of a `bool`. Its operand must be a `bool`; the result
//! is a `bool`. It takes a parenthesized operand (like `if`'s condition), which keeps
//! its binding unambiguous without a unary-precedence rule: `not (a) and b` is
//! `(not a) and b`. The node is `{ty: not, value: operand}` (a single operand, as
//! [`crate::identities::return`]'s).
//!
//! The surface parse lives in [`crate::parse::Parser::parse_not`]; here we register
//! the identity, its run native, and its lowering. Run yields `1` when the operand
//! is false, else `0`; compile lowers it as `operand == 0`.

use cranelift_codegen::ir::Value;

use super::Cx;
use crate::compile::{CompileError, Lowerer};
use crate::dyad::{Dyad, DyadPtr};
use crate::id_context::IdContext;
use crate::parse::Construct;
use crate::run::{RunError, Runtime};

/// Register `not`: spelling, the parenthesized-operand construct, run native, and
/// lowering. Returns the identity.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("not", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Not);
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// The single operand of a `not` node (its `value`).
///
/// # Safety
/// `node` must be a `not` node built by [`crate::parse::Parser::parse_not`].
unsafe fn operand(node: DyadPtr) -> DyadPtr {
    (*node).value.cast::<Dyad>()
}

/// Run: `1` when the operand is false (0), else `0`.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `not` node; its `value` is its operand.
    unsafe { Ok(i64::from((rt.run(operand(node))? == 0) as i32)) }
}

/// Lower: `operand == 0`, yielding the i32 0/1.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `not` node; its `value` is its operand.
    unsafe {
        let a = lw.lower(operand(node))?;
        let zero = lw.const_i32(0);
        Ok(lw.icmp_eq(a, zero))
    }
}
