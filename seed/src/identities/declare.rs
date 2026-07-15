// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `:=`: the declaration operator, and the `declare` node it builds. `name :=
//! value` binds a *fresh* name to a value in the current scope, declaring the
//! name *before* the value is elaborated so the value can refer to it — a
//! function can name itself and recurse.
//!
//! The binding happens at parse time (the driver,
//! [`crate::parse::Parser::parse_expression`], performs it when it sees a fresh
//! name followed by `:=`), but the declaration is *real graph structure*, not a
//! parse-time vapor: the expression becomes
//! `{ty: declare, value: [name, declared, op]}` — the spelling as a string node
//! (the human-stable half of the nominal identity; DESIGN ›A declaration has a
//! stable identity across edits‹), the declared binding, and the native leaf.
//! Anything downstream — the sequence runner, the REPL's echo policy, a
//! renderer — can see that a declaration happened by looking at the node,
//! never by re-reading source text.
//!
//! Running a declaration runs its initializer for effect and yields unit: for
//! a plain value the work already happened at parse (storage allocated, bytes
//! written) and the run is a harmless read; a construction re-fills its
//! instance each evaluation; a declared fn or struct is inert. A declaration
//! is a statement — value positions reject it.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// The index of the declared binding (or the construction initializer) in a
/// declare node's value; the name string node sits at 0.
const DECL_DECLARED: usize = 1;

/// Register the `:=` token (the driver dispatches on its `Construct::Declare`;
/// the trie longest-matches `:=` over the field-list `:`) and the `declare`
/// identity its expressions are typed by, with its native leaf and lowering.
/// Returns `(declare identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::record(cx.store, meta::TOKEN_TAG);
    let token = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(":=", IdContext::new(token, cx.root_scope));
    cx.metas.insert(token, Construct::Declare);

    let record =
        meta::operand_record(cx, meta::TUPLE_TAG, 0.0, Assoc::Left, &["name", "declared", "op"]);
    let declare = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(declare, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (declare, leaf)
}

/// Build a declare node `{ty: declare, value: [name, declared, op]}`.
pub(crate) fn build(
    store: &mut Store,
    declare: DyadPtr,
    op: DyadPtr,
    name: DyadPtr,
    declared: DyadPtr,
) -> DyadPtr {
    let value = store.alloc_operands(&[name, declared, op]);
    store.alloc_raw(declare, value)
}

/// The declared binding (or construction initializer) of a declare node.
///
/// # Safety
/// `node` must be a declare node as [`build`] lays it out.
pub(crate) unsafe fn declared_of(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr).add(DECL_DECLARED)
}

/// Run: evaluate the initializer for its effect (a construction fills its
/// instance; a plain binding's read is harmless; a fn or struct is inert) and
/// yield unit — a declaration is a statement.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid declare node; its declared slot is a valid dyad.
    unsafe {
        rt.run(declared_of(node))?;
    }
    Ok(0)
}

/// Lower: the initializer lowers for its effect (a construction emits its
/// stores; a dead load is cleaned up by the backend), yielding unit.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run`].
    unsafe {
        lw.lower(declared_of(node))?;
    }
    Ok(lw.const_i32(0))
}
