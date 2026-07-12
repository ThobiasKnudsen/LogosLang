//! `!=`: the *abstract* inequality comparison, an abstraction over all machine
//! comparisons (mirrors [`crate::identities::lt`]). At parse time it resolves from
//! its operands' types to a concrete op (`ne_i32` today), which it stores as its
//! third operand, so `!=` stays reflectable while run and compile delegate to that
//! concrete op ([`crate::identities::cmp`]). Its result is a `bool`; inequality binds
//! at equality precedence (looser than the relational operators, tighter than `=`).

use cranelift_codegen::ir::Value;

use super::{is_numeric, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// The index, in a `!=` node's value struct, of the resolved concrete op.
const NE_CONCRETE: usize = 2;

/// Register `!=`: spelling and parse precedence (equality, left-associative), plus
/// its resolve-and-delegate run and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("!=", IdContext::new(id, cx.root_scope));
    cx.metas
        .insert(id, Construct::Infix { precedence: 1.4, assoc: Assoc::Left, build });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs != rhs`: resolve the concrete comparison op from the operand types and
/// store it as the node's third operand, giving `{ty: !=, value: [lhs, rhs, op]}`.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    ne: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store; reading their type is safe.
    let resolvable = unsafe { is_numeric(types, lhs) && is_numeric(types, rhs) };
    if !resolvable {
        return Err(ParseError::UnsupportedOperands);
    }
    let concrete = types.ne_i32;
    let value = store.alloc_operands(&[lhs, rhs, concrete]);
    Ok(store.alloc_raw(ne, value))
}

/// The concrete op a `!=` node resolved to (its third operand).
///
/// # Safety
/// `node` must be a `!=` node built by [`build`], with a `[lhs, rhs, concrete]` value.
unsafe fn concrete_op(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr).add(NE_CONCRETE)
}

/// Run: delegate to the resolved concrete op, which reads this node's operands.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `!=` application carrying its resolved concrete op.
    unsafe { rt.run_native(concrete_op(node), node) }
}

/// Lower: delegate to the resolved concrete op's lowering.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `!=` application carrying its resolved concrete op.
    unsafe { lw.lower_op(concrete_op(node), node) }
}
