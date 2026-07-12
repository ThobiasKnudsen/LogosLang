//! `+`: addition. `+` is a *higher-level identity* describing how to read its
//! operands (DESIGN ›which concrete machine operation runs is resolved from the
//! operand types‹): its node is `{ty: +, value: [lhs, rhs, type]}`, where the
//! resolved operand type is stored in the value slot. Run and compile read that
//! stored type and switch on it (see [`crate::identities::numtype`]), so one `+`
//! identity serves every numeric type.

use cranelift_codegen::ir::Value;

use super::numtype::{eval_arith, ArithOp};
use super::{is_numeric, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `+`: spelling, parse precedence (left-associative, binding tighter than
/// `=`), and its type-switched run and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("+", IdContext::new(id, cx.root_scope));
    cx.metas
        .insert(id, Construct::Infix { precedence: 2.0, assoc: Assoc::Left, build });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs + rhs`: resolve the operand type and store it as the node's third
/// operand, giving `{ty: +, value: [lhs, rhs, type]}`. The seed has one machine
/// numeric type today, so numeric operands resolve to `i32`; non-numeric operands
/// leave `+` unresolved ([`ParseError::UnsupportedOperands`]).
fn build(
    store: &mut Store,
    types: &CoreTypes,
    plus: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store; reading their type is safe.
    if !unsafe { is_numeric(types, lhs) && is_numeric(types, rhs) } {
        return Err(ParseError::UnsupportedOperands);
    }
    let value = store.alloc_operands(&[lhs, rhs, types.i32_]);
    Ok(store.alloc_raw(plus, value))
}

/// Run: add in the stored operand type.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `+` application `[lhs, rhs, type]`.
    unsafe { eval_arith(rt, node, ArithOp::Add) }
}

/// Lower: emit the machine addition for the stored operand type.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `+` application `[lhs, rhs, type]`.
    unsafe { lw.lower_arith(node, ArithOp::Add) }
}
