//! `==`: equality. Like `<` (see [`crate::identities::lt`]), it stores its resolved
//! operand type in the value slot and run/compile switch on it; result is `bool`.
//! Equality binds looser than the relational operators; the trie longest-matches `==`
//! over `=`.

use cranelift_codegen::ir::Value;

use super::numtype::{eval_compare, CmpOp};
use super::{is_numeric, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `==`: spelling, precedence (equality, left-associative), and its
/// type-switched run and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("==", IdContext::new(id, cx.root_scope));
    cx.metas
        .insert(id, Construct::Infix { precedence: 1.4, assoc: Assoc::Left, build });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs == rhs`: resolve the operand type and store it as the third operand.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    eq: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store; reading their type is safe.
    if !unsafe { is_numeric(types, lhs) && is_numeric(types, rhs) } {
        return Err(ParseError::UnsupportedOperands);
    }
    let value = store.alloc_operands(&[lhs, rhs, types.i32_]);
    Ok(store.alloc_raw(eq, value))
}

/// Run: compare in the stored operand type.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `==` application `[lhs, rhs, type]`.
    unsafe { eval_compare(rt, node, CmpOp::Eq) }
}

/// Lower: emit the machine comparison for the stored operand type.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `==` application `[lhs, rhs, type]`.
    unsafe { lw.lower_compare(node, CmpOp::Eq) }
}
