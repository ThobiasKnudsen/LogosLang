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

use super::numtype::{is_pointer_type, of_type_node, NumType};
use super::read::{read_kind, Read};
use super::{commit_if_literal, meta, operands, Cx, Operand};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, CoreTypes, ParseError};
use crate::store::Store;

/// Register `=`: spelling, parse_rank, and lowering. The run natives are
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
/// then its right side driven to the boundary. Over one of the six slots of
/// the type being defined, the slot's fill
/// ([`crate::parse::Parser::slot_fill`], #61).
fn construct(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, ParseError> {
    let Some(target) = p.construct_left(tape)? else {
        return Err(ParseError::MissingOperand);
    };
    let slot = p.slot_of(target);
    let value = p.parse_expression()?;
    let node = if let Some(kind) = slot {
        p.slot_fill(kind, value)?
    } else {
        let types = p.types();
        build(p.store(), &types, id, target, value)?
    };
    // A node-valued box's store is replayed at parse, so a `( )` block settles
    // its boxes exactly as the top level does (see
    // [`crate::parse::Parser::settle_box_store`]).
    p.settle_box_store(node);
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
    // `t[k] = cell`: a tape slot as the target (#60); `t[k]:dyad.type = T`:
    // the cell's type as the target, the retype (#61).
    if unsafe { (*lhs_d).ty } == types.tape.slot {
        return Ok(unsafe { super::tape::build_write(store, types, lhs_d, rhs) });
    }
    if unsafe { (*lhs_d).ty } == types.tape.cell_type {
        return Ok(unsafe { super::tape::build_retype(store, types, lhs_d, rhs) });
    }
    if unsafe { (*lhs_d).ty } == types.deref_ {
        return unsafe { super::pointer::build_storeptr(store, types, lhs_d, rhs) };
    }
    // What the target is, by the reading rule (#82). A scalar place — a numeric
    // or pointer variable, marked as storage — takes a value at its width. A
    // box holding a node address takes a node: a `type ?` box a type value
    // (an identity, or another type box), a `dyad ?` box anything whose run
    // yields a node — a type, a box, a view — since what it holds is asked
    // afterwards, `a:dyad.type == type`. A number into either would leave
    // bits every later reader follows as an address. Nothing else has storage
    // an `=` may write: a comptime binding has none, a literal's untagged
    // storage is not a place (writing `i32 5 = 3` into it used to be
    // accepted), a bare parameter's container is not assignable, and a record
    // instance is written by field (#115).
    // SAFETY: `lhs_d`/`rhs` are reduced dyads from the store.
    let (target, marked) =
        unsafe { (read_kind(types, lhs_d), crate::dyad::is_place((*lhs_d).value)) };
    match target {
        Read::Container(t) if t == types.type_ || t == types.dyad_ => {
            // SAFETY: as above.
            let value_read = unsafe { read_kind(types, rhs) };
            let ok = match value_read {
                Read::Identity => true,
                Read::Container(c) => {
                    if t == types.type_ {
                        c == types.type_
                    } else {
                        !c.is_null()
                    }
                }
                Read::Address => t == types.dyad_,
                _ => false,
            };
            if !ok {
                return Err(ParseError::BadDeclaredType);
            }
            let value = store.alloc_operands(&[lhs, rhs, types.ops.store_leaf(NumType::I64)]);
            return Ok(store.alloc_raw(op, value));
        }
        Read::Scalar(_) | Read::Pointer(_) if marked => {}
        _ => return Err(ParseError::BadAssignTarget),
    }
    // A literal into a pointer would become a wild address.
    // SAFETY: as above.
    let lhs_pointer = unsafe { is_pointer_type((*lhs_d).ty) };
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
    // An owning place holds the only pointer to a block its teardown frees, so
    // what is written into it must own too: a borrow leaves the teardown
    // freeing memory the store owns, and a bare owning name leaves two places
    // whose teardowns free one block (#79). Both aborted the process with a
    // double free at scope exit; both are the checked error here.
    // SAFETY: `lhs_d`/`rhs` are reduced dyads from the store.
    if super::drop_model::is_owning_place(lhs_d)
        && !unsafe { super::drop_model::is_owning_value(types, rhs) }
    {
        return Err(ParseError::NonOwningIntoOwning);
    }
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
///
/// The width comes from the node's own op slot — the store leaf [`build`]
/// chose — exactly as the interpreter's `store_run<NT>` reads it. It used to
/// be re-derived from the target's logos, which for a `type ?` or `dyad ?` box
/// has no numeric tag: `numtype_of_type` reached `NumType::from_tag(18)` and
/// panicked, so a compiled body assigning into such a box took the process
/// down where the interpreter wrote eight bytes (#82, asymmetry 6). One
/// decision, made at build, read twice.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `=` application `[lhs, rhs, store leaf]`, so
    // its operands and its op slot are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        let leaf = *((*node).value as *const DyadPtr).add(2);
        let Some(nt) = lw.types().ops.store_nt_of(leaf) else {
            return Err(CompileError::BadValue);
        };
        let lhs = lw.through(lhs);
        if (*lhs).value.is_null() {
            return Err(CompileError::BadValue);
        }
        let v = lw.lower(rhs)?;
        lw.write_place(lhs, nt.cranelift_type(), v)?;
        Ok(v)
    }
}
