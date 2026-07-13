// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `bool`: the type of a boolean value. A data type (its own type is `type`), like
//! [`crate::identities::i32`], but its values are just 0/1. Comparisons (`<`, …)
//! produce `bool`, and `if` requires a `bool` condition (checked at parse time).
//! DESIGN has no `bool` yet; this is the seed's minimal one.
//!
//! `true`/`false` are the two literals: shared value nodes typed `bool`, each
//! carrying an `i32` 0/1 in its storage. They have no parse `Construct` (they
//! resolve to their node and push as an operand); `run`'s generic data path reads
//! the `i32`, and the `bool` lowering bakes it as a constant.

use cranelift_codegen::ir::Value;

use super::Cx;
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;

/// Register `bool`: its type spelling and lowering, plus the `true`/`false`
/// literal nodes with their spellings. Returns the `bool` type identity so the
/// parser can hold it in `CoreTypes` (a comparison result is `bool`; `if`'s
/// condition must be one).
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let bool_ = cx.store.alloc_raw(cx.type_, std::ptr::null_mut());
    cx.trie.insert("bool", IdContext::new(bool_, cx.root_scope));
    cx.lower.insert(bool_, lower);

    let true_ = literal(cx, bool_, 1);
    cx.trie.insert("true", IdContext::new(true_, cx.root_scope));
    let false_ = literal(cx, bool_, 0);
    cx.trie.insert("false", IdContext::new(false_, cx.root_scope));

    bool_
}

/// A `bool` literal node carrying `v` (0 or 1) as an `i32` in its storage.
fn literal(cx: &mut Cx, bool_: DyadPtr, v: i32) -> DyadPtr {
    let value = cx.store.alloc_bytes(&v.to_ne_bytes());
    cx.store.alloc_raw(bool_, value)
}

/// Build a `bool` value node for `v`, physically the `i32` 0/1. Used to fold a
/// comparison of two comptime rationals into a literal at parse time.
pub(crate) fn literal_node(store: &mut crate::store::Store, bool_: DyadPtr, v: bool) -> DyadPtr {
    let value = store.alloc_bytes(&i32::from(v).to_ne_bytes());
    store.alloc_raw(bool_, value)
}

/// Lower a `bool` value to its `i32` 0/1 constant, read from its storage. Guards a
/// null address, mirroring the interpreter's `BadValue`.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    let addr = unsafe { (*node).value };
    if addr.is_null() {
        return Err(CompileError::BadValue);
    }
    // SAFETY: a non-null `bool` storage holds an `i32` 0/1 (see `literal`).
    let v = unsafe { std::ptr::read_unaligned(addr as *const i32) };
    Ok(lw.const_i32(v))
}
