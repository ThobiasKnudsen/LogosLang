// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `fn`: the type whose values are functions. `run` recognizes a function by its
//! type being this.
//!
//! Surface syntax (DESIGN ›A function's surface‹): `fn ( params ) -> ret ( body )`,
//! an anonymous literal, e.g. `fn () -> i32 ( return 40 + 2 )`. The parameter list
//! *is* a `struct` (step 2's field list); the return type after `->` is mandatory;
//! the body is a `( )` scope parsed with the parameters open. The parse lives in
//! [`crate::parse::Parser::parse_fn`]; here we only register the identity, its
//! `Fn` construct, and the `->` arrow it consumes.
//!
//! A `fn` instance's value is its `[input, output, body, bcode, frame]` struct
//! (the params, the return type, the reflectable body, the compiled machine code
//! — null until [`crate::compile::compile_fn`] installs it — and the
//! activation-record byte size, null for a function with no locals). `run` jumps
//! to an installed `bcode` and walks `body` otherwise; only the leaf natives
//! (`=`, `+`, `if`, …) keep their machine code in the run version's table
//! instead (see `crate::run`).

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Schedule};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Create the `fn` type (its own type is `type`) and return it. Called before the
/// build context exists, since `=`/`+` reference `fn` as their type.
pub(super) fn register(store: &mut Store, type_: DyadPtr) -> DyadPtr {
    store.alloc_raw(type_, std::ptr::null_mut())
}

/// Register `fn`'s surface syntax: the `fn` spelling and its `Fn` construct, plus
/// the `->` return arrow, whose handle is returned. Done after the build context
/// exists so it can add to the parser's table.
pub(super) fn register_syntax(cx: &mut Cx) -> DyadPtr {
    cx.trie.insert("fn", IdContext::new(cx.fn_type, cx.root_scope));
    // `fn`'s constructor claims the pending declaration placeholder (the driver
    // suppresses it when the literal does not open a (sub-)expression), so a
    // recursive self-call inside the body resolves the published signature.
    cx.metas.insert(cx.fn_type, |p, id, _tape| {
        let declared = p.take_pending_fn();
        p.parse_fn(id, declared).map(crate::parse::Constructed::Node)
    });

    // `fn`'s own record, installed now that the string type exists for the role
    // names: an fn value is the five fixed slots `[input, output, body, bcode,
    // frame]` — the params, the return type, the reflectable body, the compiled
    // callable (null until compiled), and the activation-record byte size (null
    // for a function with no locals).
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        f64::NAN,
        Assoc::Left,
        Schedule::Fn,
        &["input", "output", "body", "bcode", "frame"],
    );
    // SAFETY: `fn_type` was allocated by [`register`] and nothing has read its
    // (previously null) value slot.
    unsafe {
        (*cx.fn_type).value = record;
    }

    // `->` separates a fn's parameter list from its return type.
    let record = meta::record(cx.store, meta::TOKEN_TAG, f64::NAN, Schedule::Arrow);
    let arrow = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("->", IdContext::new(arrow, cx.root_scope));
    arrow
}

/// Register `compile`, the `fn` type's shared member (DESIGN ›Execution is
/// function application‹: "The `fn` type carries two shared functions:
/// `compile` … and `run`"; `run` is calling). `f.compile()` lowers `f`'s body
/// to machine code and installs it, so the next call jumps instead of walking
/// the body — explicit direction in the one pass (DESIGN ›Build and run are
/// one self-directing pass‹). No spelling enters the trie: `compile` resolves
/// only after `.` on an fn-typed value ([`crate::parse::Parser::parse_field_access`]),
/// the seed's stand-in for shared-member resolution through the type's scope.
/// A statement yielding unit, like `while`; value positions reject it.
///
/// The node is `{ty: compile, value -> [function, code, op]}`: the target fn,
/// the callable leaf pre-minted (entry zero) at parse for the compile to patch
/// (minting needs the store, which run does not hold), and the run leaf.
/// Returns `(identity, leaf)`.
pub(super) fn register_compile(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        f64::NAN,
        Assoc::Left,
        Schedule::Operand,
        &["function", "code", "op"],
    );
    let compile_ = cx.store.alloc_raw(cx.type_, record);
    let leaf = callable::mint_native(cx.store, cs.callable, compile_run, cs.seed_native);
    (compile_, leaf)
}

/// The `(function, code)` operands of a compile node.
///
/// # Safety
/// `node` must be a compile node as `parse_field_access` builds it, with a
/// `[function, code, op]` value.
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
    (*p, *p.add(1))
}

/// Run: compile the target function's body and install the entry into the
/// pre-minted leaf, through the runtime's compiler context; yield unit.
fn compile_run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid compile node with `[function, code]` operands.
    unsafe {
        let (fn_node, code_leaf) = parts(node);
        rt.compile_member(fn_node, code_leaf)?;
        Ok(0)
    }
}
