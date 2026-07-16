// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `=`: assignment. A parse-time constructor, right-associative, binding
//! loosest. Its builder resolves the target's width to a concrete store op —
//! `{ty: =, value: [lhs, rhs, store_<type>]}` — so run jumps through the op
//! slot and writes at the baked width (issue #44); compile lowers it to a
//! store. The stored value is yielded.

use cranelift_codegen::ir::Value;

use super::numtype::{is_pointer_type, numtype_of_type, of_type_node};
use super::{commit_if_literal, is_numtype_node, meta, operands, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError, Schedule};
use crate::store::Store;

/// Register `=`: spelling, parse precedence, and lowering. The run natives are
/// the per-width store leaves ([`crate::identities::ops`]).
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        1.0,
        Assoc::Right,
        Schedule::Infix,
        &["lhs", "rhs", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("=", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Infix { build });
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs = rhs`, committing an uncommitted literal right side to the target
/// variable's declared numeric type — the typed slot (DESIGN ›committing to a
/// concrete type only when it finally lands in a typed slot‹) — so `a = 10` writes
/// at `a`'s width in both tiers and `a = 5000000000` into an i64 is exact. A
/// literal with no exact value in the target is [`ParseError::UncomputableLiteral`]
/// at parse time; a non-literal right side passes through unchanged. The op slot
/// gets the store leaf for the target's width (a pointer target stores as its
/// 8-byte address).
///
/// Exposed to siblings so a declaration's snapshot initializer
/// ([`super::build_scalar_init`]) can reuse the `place = value` store for `:=`.
pub(super) fn build(
    store: &mut Store,
    types: &CoreTypes,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // A store through a pointer — `p@ = v`, `p@.x = v` — rebuilds as a storeptr
    // node with its own run/lower, resolved here at parse time.
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    if unsafe { (*lhs).ty } == types.deref_ {
        return unsafe { super::pointer::build_storeptr(store, types, lhs, rhs) };
    }
    // The assignable places in v1 are typed numeric and pointer variables. A
    // comptime (`:=`-bound rational) binding has no machine storage — writing
    // its value slot would corrupt the fraction — and nothing else has storage.
    // SAFETY: as above.
    let (lhs_numeric, lhs_pointer) = unsafe {
        (is_numtype_node(types, (*lhs).ty), is_pointer_type((*lhs).ty))
    };
    if !lhs_numeric && !lhs_pointer {
        return Err(ParseError::BadAssignTarget);
    }
    // A literal into a pointer would become a wild address.
    // SAFETY: as above.
    if lhs_pointer && unsafe { (*rhs).ty } == types.rational {
        return Err(ParseError::TypeMismatch);
    }
    // A literal right side commits to the target's type (the typed slot); a
    // non-literal one must already BE that type — no implicit coercion
    // ([`super::check_store_type`]).
    // SAFETY: as above.
    let rhs = unsafe {
        if (*rhs).ty == types.rational {
            let nt = of_type_node((*lhs).ty);
            commit_if_literal(store, rhs, &Operand::Literal, (*lhs).ty, nt)?
        } else {
            super::check_store_type(types, (*lhs).ty, rhs)?;
            rhs
        }
    };
    // SAFETY: `lhs` is a typed variable checked assignable above.
    let nt = unsafe { of_type_node((*lhs).ty) };
    let value = store.alloc_operands(&[lhs, rhs, types.ops.store_leaf(nt)]);
    Ok(store.alloc_raw(op, value))
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
        let ct = numtype_of_type((*lhs).ty).cranelift_type();
        let addr = lw.place_addr(lhs);
        lw.store_at(ct, addr, 0, v);
        Ok(v)
    }
}
