// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `=`: assignment. A function (its type is `fn`), right-associative, binding
//! loosest. Run evaluates the right operand and writes it into the left operand's
//! storage, yielding the value; compile lowers it to a store.

use cranelift_codegen::ir::Value;

use super::numtype::{is_pointer_type, numtype_of_type, of_type_node, write_scalar};
use super::{build_binary, commit_if_literal, is_numtype_node, meta, operands, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `=`: spelling, parse precedence, run bcode, and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::operand_record(cx, meta::TUPLE_TAG, 1.0, Assoc::Right, &["lhs", "rhs"]);
    let id = cx.store.alloc_raw(cx.fn_type, record);
    cx.trie.insert("=", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Infix { build });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs = rhs`, committing an uncommitted literal right side to the target
/// variable's declared numeric type — the typed slot (DESIGN ›committing to a
/// concrete type only when it finally lands in a typed slot‹) — so `a = 10` writes
/// at `a`'s width in both tiers and `a = 5000000000` into an i64 is exact. A
/// literal with no exact value in the target is [`ParseError::UncomputableLiteral`]
/// at parse time; a non-literal right side passes through unchanged.
fn build(
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
    // SAFETY: as above.
    let rhs = unsafe {
        if (*rhs).ty == types.rational {
            let nt = of_type_node((*lhs).ty);
            commit_if_literal(store, rhs, &Operand::Literal, (*lhs).ty, nt)?
        } else {
            rhs
        }
    };
    build_binary(store, types, op, lhs, rhs)
}

/// Run: evaluate the right operand, write it into the left operand's storage at that
/// variable's type width (the store truncates to the width; the reader reads it back
/// the same way), and yield the assigned value.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid application dyad, so its operands are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        let bits = rt.run(rhs)?;
        let slot = (*lhs).value;
        if slot.is_null() {
            return Err(RunError::BadValue);
        }
        write_scalar((*lhs).ty, slot, bits);
        Ok(bits)
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
        let ct = numtype_of_type((*lhs).ty).cranelift_type();
        lw.store(ct, (*lhs).value, v);
        Ok(v)
    }
}
