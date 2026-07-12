//! `or`: short-circuiting logical disjunction over `bool`s. Both operands must be
//! `bool` (comparisons, `bool` values, or nested logical results); the result is a
//! `bool`. It short-circuits: when the left operand is true the right is not
//! evaluated, so run branches and compile lowers a two-way branch (`if a then true
//! else b`, [`crate::compile::Lowerer::lower_or`]). It binds loosest of the logical
//! operators (looser than `and`), just above `=`.

use cranelift_codegen::ir::Value;

use super::{operands, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{is_bool_result, Assoc, Construct, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `or`: spelling and parse precedence (logical, left-associative, looser
/// than `and`), plus its short-circuiting run and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("or", IdContext::new(id, cx.root_scope));
    cx.metas
        .insert(id, Construct::Infix { precedence: 1.1, assoc: Assoc::Left, build });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs or rhs` as `{ty: or, value: [lhs, rhs]}`, requiring both operands to be
/// `bool` ([`ParseError::NonBoolOperands`]).
fn build(
    store: &mut Store,
    types: &CoreTypes,
    or: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store; reading their type is safe.
    if !unsafe { is_bool_result(types, lhs) && is_bool_result(types, rhs) } {
        return Err(ParseError::NonBoolOperands);
    }
    let value = store.alloc_operands(&[lhs, rhs]);
    Ok(store.alloc_raw(or, value))
}

/// Run: short-circuit — the result is `true` (without running the right operand)
/// when the left is true, else the right operand's value.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `or` application; its two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        if rt.run(lhs)? != 0 {
            Ok(1)
        } else {
            rt.run(rhs)
        }
    }
}

/// Lower: a short-circuiting two-way branch (`if a then true else b`).
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `or` application; its two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        lw.lower_or(lhs, rhs)
    }
}
