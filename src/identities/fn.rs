// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `fn`: the type whose values are functions, the record
//! `[input, output, body, bcode, frame, outer]`. The surface parse lives in
//! [`crate::parse::Parser::parse_fn`]; here the identity, its construct, the
//! `->` arrow, and the `compile` member.

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Called before the build context exists, since `=`/`+` reference `fn` as their logos.
pub(super) fn register(store: &mut Store, type_: DyadPtr) -> DyadPtr {
    store.alloc_raw(type_, std::ptr::null_mut())
}

/// Returns the `->` arrow.
pub(super) fn register_syntax(cx: &mut Cx) -> DyadPtr {
    cx.declare("fn", cx.fn_type);
    // The pending `:=` placeholder is claimed so a self-call in the body
    // resolves the published signature.
    cx.metas.insert(cx.fn_type, |p, id, tape| {
        // Claimed only when the literal is the only cell on the tape; elsewhere
        // suppressed around the parse so a deeper literal can still claim it.
        let node = if tape.len() == 1 {
            let declared = p.take_pending_fn();
            // SAFETY: `declared` is null or the placeholder `:=` handed over.
            unsafe { p.parse_fn(id, declared) }?
        } else {
            let suppressed = p.take_pending_fn();
            // SAFETY: a null `declared` writes nothing.
            let node = unsafe { p.parse_fn(id, std::ptr::null_mut()) };
            p.restore_pending_fn(suppressed);
            node?
        };
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });

    // Installed now that the string logos exists for the role names. `bcode`
    // is null until compiled, `frame` null for a function with no locals,
    // `outer` null for one that reads no outer name.
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::READER,
        Assoc::Left,
        &["input", "output", "body", "bcode", "frame", "outer"],
    );
    // SAFETY: `fn_type` was allocated by [`register`] and nothing has read its value slot.
    unsafe {
        (*cx.fn_type).value = record;
    }

    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
    let arrow = cx.store.alloc_raw(cx.type_, record);
    cx.declare("->", arrow);
    arrow
}

/// `f.compile()`: lower `f`'s body into the `code` leaf, minted at parse since
/// minting needs the store. Resolves only after `.` on an fn-typed value, a
/// stand-in for shared-member resolution through the type's scope.
pub(super) fn register_compile(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["function", "code", "op"],
    );
    let compile_ = cx.store.alloc_raw(cx.type_, record);
    let leaf = callable::mint_native(cx.store, cs.callable, compile_run, cs.seed_native);
    (compile_, leaf)
}

/// # Safety
/// `node` must be a compile node `[function, code, op]`.
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
    (*p, *p.add(1))
}

fn compile_run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid compile node with `[function, code]` operands.
    unsafe {
        let (fn_node, code_leaf) = parts(node);
        rt.compile_member(fn_node, code_leaf)?;
        Ok(0)
    }
}
