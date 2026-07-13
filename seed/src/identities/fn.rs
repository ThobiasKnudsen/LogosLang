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
//! A `fn` instance's value is its `[input, output, body, bcode]` struct (the
//! params, the return type, the reflectable body, and the compiled machine code,
//! null until [`crate::compile::compile_fn`] installs it). `run` jumps to an
//! installed `bcode` and walks `body` otherwise; only the leaf natives (`=`, `+`,
//! `if`, …) keep their machine code in the run version's table instead (see
//! `crate::run`).

use super::Cx;
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::Construct;
use crate::store::Store;

/// Create the `fn` type (its own type is `type`) and return it. Called before the
/// build context exists, since `=`/`+` reference `fn` as their type.
pub(super) fn register(store: &mut Store, type_: DyadPtr) -> DyadPtr {
    store.alloc_raw(type_, std::ptr::null_mut())
}

/// Register `fn`'s surface syntax: the `fn` spelling and its `Fn` construct, plus
/// the `->` return arrow. Done after the build context exists so it can add to the
/// parser's table.
pub(super) fn register_syntax(cx: &mut Cx) {
    cx.trie.insert("fn", IdContext::new(cx.fn_type, cx.root_scope));
    cx.metas.insert(cx.fn_type, Construct::Fn);

    // `->` separates a fn's parameter list from its return type.
    let arrow = cx.store.alloc_raw(cx.type_, std::ptr::null_mut());
    cx.trie.insert("->", IdContext::new(arrow, cx.root_scope));
    cx.metas.insert(arrow, Construct::Arrow);
}
