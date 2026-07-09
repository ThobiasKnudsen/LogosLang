//! `+`: the *abstract* addition operator. It is not itself a machine addition;
//! it is an abstraction over all of them (DESIGN ›the operator is a higher-level
//! identity … which concrete machine operation runs, which `+`, is resolved from
//! the operand types‹). At parse time `+` looks at its operands' types and resolves
//! to a concrete op (`add_i32` today; `add_f32`, `add_u64`, `add_i32_imm` later),
//! which it stores in its own value so the choice is conserved for reflection.
//!
//! A `+` node is therefore `{ty: +, value: [lhs, rhs, concrete]}`: it stays
//! reflectable *as* `+`, its operands are the first two fields, and the resolved
//! concrete op is the third. Run and compile delegate to that concrete op — the
//! actual arithmetic lives in [`crate::identities::add`], never on `+`.

use cranelift_codegen::ir::Value;

use super::Cx;
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// The index, in a `+` node's value struct, of the resolved concrete op.
const PLUS_CONCRETE: usize = 2;

/// Register `+`: spelling and parse precedence (left-associative, binding tighter
/// than `=`), plus its resolve-and-delegate run and lowering. `+` carries no
/// arithmetic of its own; `add_i32` (and future concrete ops) do.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("+", IdContext::new(id, cx.root_scope));
    cx.metas
        .insert(id, Construct::Infix { precedence: 2.0, assoc: Assoc::Left, build });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs + rhs`: resolve the concrete addition op from the operand types and
/// store it as the node's third operand, giving `{ty: +, value: [lhs, rhs, op]}`.
/// The seed has one numeric machine type, so numeric operands resolve to `add_i32`;
/// non-numeric operands leave `+` unresolved ([`ParseError::UnsupportedOperands`]).
fn build(
    store: &mut Store,
    types: &CoreTypes,
    plus: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store; reading their type is safe.
    let resolvable = unsafe { numeric(types, plus, lhs) && numeric(types, plus, rhs) };
    if !resolvable {
        return Err(ParseError::UnsupportedOperands);
    }
    let concrete = types.add_i32;
    let value = store.alloc_operands(&[lhs, rhs, concrete]);
    Ok(store.alloc_raw(plus, value))
}

/// Whether `node` produces a number `+` can add: an `i32`, a `rational` literal
/// (which molds to i32), or another `+` (whose result is numeric). One numeric
/// machine type today; this widens as `f32`/`u64`/… arrive.
///
/// # Safety
/// `node` must be a valid dyad from the store.
unsafe fn numeric(types: &CoreTypes, plus: DyadPtr, node: DyadPtr) -> bool {
    let ty = (*node).ty;
    ty == types.i32_ || ty == types.rational || ty == plus
}

/// The concrete op a `+` node resolved to (its third operand).
///
/// # Safety
/// `node` must be a `+` node built by [`build`], with a `[lhs, rhs, concrete]` value.
unsafe fn concrete_op(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr).add(PLUS_CONCRETE)
}

/// Run: delegate to the resolved concrete op, which reads this node's operands.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `+` application carrying its resolved concrete op.
    unsafe { rt.run_native(concrete_op(node), node) }
}

/// Lower: delegate to the resolved concrete op's lowering.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `+` application carrying its resolved concrete op.
    unsafe { lw.lower_op(concrete_op(node), node) }
}
