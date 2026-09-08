// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `=`: assignment. Beside `:=` on the axis (DESIGN ›The scope's constructor
//! is the driver‹, ruled 8 September 2026): constructed at discovery, it reads
//! the place to its left — the cells since the boundary, constructed to one —
//! and drives its right side to the boundary. Its builder resolves the
//! target's width to a concrete store op — `{type: =, value: [lhs, rhs,
//! store_<logos>]}` — so run jumps through the op slot and writes at the baked
//! width (issue #44); compile lowers it to a store. **`=` returns nothing**:
//! an assignment is an act, not a value, so `a = b = c` is the error of
//! assigning nothing, and an `=` in a value position is the statement-as-value
//! error.

use cranelift_codegen::ir::Value;

use super::numtype::{is_pointer_type, numtype_of_type, of_type_node};
use super::{commit_if_literal, is_numtype_node, meta, operands, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, CoreTypes, ParseError};
use crate::store::Store;

/// Register `=`: spelling, parse precedence, and lowering. The run natives are
/// the per-width store leaves ([`crate::identities::ops`]).
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::DECLARE,
        Assoc::Right,
        &["lhs", "rhs", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("=", id);
    cx.metas.insert(id, construct);
    cx.lower.insert(id, lower);
    id
}

/// `=`'s constructor: at discovery, the place to its left (the cells since
/// the boundary, constructed to one — [`crate::parse::Parser::construct_left`]),
/// then its right side driven to the boundary. Over an unfilled type
/// placeholder, `name = <type>` is the type variable's fill instead
/// ([`crate::parse::Parser::type_fill`]).
fn construct(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, ParseError> {
    // The type variable's fill — `name = <type>` over an unfilled placeholder
    // rebinds the name at parse ([`crate::parse::Parser::type_fill`]).
    let type_var = p.is_type_variable(tape);
    let Some(target) = p.construct_left(tape)? else {
        return Err(ParseError::MissingOperand);
    };
    let value = p.parse_expression()?;
    let node = if let Some(tok) = type_var {
        p.type_fill(tok, value)?
    } else {
        let types = p.types();
        build(p.store(), &types, id, target, value)?
    };
    tape.place(node);
    Ok(crate::parse::Constructed::Placed)
}

/// Build `lhs = rhs`, committing an uncommitted literal right side to the target
/// variable's declared numeric logos — the typed slot (DESIGN ›committing to a
/// concrete logos only when it finally lands in a typed slot‹) — so `a = 10` writes
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
    // The target and the value are stored as they stand (a name is its
    // record, DESIGN ›The dyad's read surface‹); their types are read through
    // the reading rule.
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let (lhs_d, rhs_d) = unsafe { (types.through(lhs), types.through(rhs)) };
    // `t[k] = cell`: a tape slot as the target (#60).
    if unsafe { (*lhs_d).ty } == types.tape.slot {
        return Ok(unsafe { super::tape::build_write(store, types, lhs_d, rhs) });
    }
    if unsafe { (*lhs_d).ty } == types.deref_ {
        return unsafe { super::pointer::build_storeptr(store, types, lhs_d, rhs) };
    }
    // The assignable places in v1 are typed numeric and pointer variables. A
    // comptime (`:=`-bound rational) binding has no machine storage — writing
    // its value slot would corrupt the fraction — and nothing else has storage.
    // SAFETY: as above.
    let (lhs_numeric, lhs_pointer) = unsafe {
        (is_numtype_node(types, (*lhs_d).ty), is_pointer_type((*lhs_d).ty))
    };
    if !lhs_numeric && !lhs_pointer {
        return Err(ParseError::BadAssignTarget);
    }
    // A literal into a pointer would become a wild address.
    // SAFETY: as above.
    if lhs_pointer && unsafe { (*rhs_d).ty } == types.rational {
        return Err(ParseError::TypeMismatch);
    }
    // `=` returns nothing: an assignment as the value assigned is the error.
    if unsafe { (*rhs_d).ty } == types.assign || unsafe { (*rhs_d).ty } == types.storeptr_ {
        return Err(ParseError::StatementAsValue);
    }
    // A literal right side commits to the target's logos (the typed slot); a
    // non-literal one must already BE that logos — no implicit coercion
    // ([`super::check_store_type`]).
    // SAFETY: as above.
    let rhs = unsafe {
        if (*rhs_d).ty == types.rational {
            let nt = of_type_node((*lhs_d).ty);
            commit_if_literal(store, types, rhs, &Operand::Literal, (*lhs_d).ty, nt)?
        } else {
            super::check_store_type(types, (*lhs_d).ty, rhs)?;
            rhs
        }
    };
    // SAFETY: `lhs` is a typed variable checked assignable above.
    let nt = unsafe { of_type_node((*lhs_d).ty) };
    let value = store.alloc_operands(&[lhs, rhs, types.ops.store_leaf(nt)]);
    Ok(store.alloc_raw(op, value))
}

/// Lower: write the right operand into the left operand's place — a promoted
/// frame place defines its register variable, anything else stores to its
/// baked storage. Guards a null storage address, mirroring the interpreter's
/// `BadValue` — without it the compiler would bake a store to address 0 and
/// SIGSEGV at call time where the interpreter cleanly errors, breaking
/// interpreter/JIT parity.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid application dyad, so its operands are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        let lhs = lw.through(lhs);
        if (*lhs).value.is_null() {
            return Err(CompileError::BadValue);
        }
        let v = lw.lower(rhs)?;
        let ct = numtype_of_type((*lhs).ty).cranelift_type();
        lw.write_place(lhs, ct, v);
        Ok(v)
    }
}
