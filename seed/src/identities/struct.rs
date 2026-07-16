// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `struct`: the type whose constructor derives a layout from a `( field-list )`.
//! Written `struct ( name : type, … | bare name )`; the field list *is* a
//! function's parameter list (DESIGN ›A function's surface‹), so the same parse
//! serves both. v1 builds the minimal carrier: the node is
//! `{ty: struct, value -> [scope, field0 … fieldN, null]}`, with each field a `:`
//! declaration dyad and its name declared in the struct's own scope. The sketch's
//! richer value (`names` hashtable, `fields` array, `size_bytes`) arrives once
//! instances need a real layout.
//!
//! The field-list parse lives in [`crate::parse::Parser::parse_struct`] because it
//! needs the parser's tape, scope stack, and reentrant expression parse; here we
//! only register the identity and the field-list punctuation `:` and `,` it
//! consumes. `struct` is a type (its own type is `type`), like `dyad := struct (…)`
//! in the sketch.

use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Schedule};

/// Register `struct` and the field-list punctuation (`:`, `,`) it consumes.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    // A struct definition's value is `[scope, field…, null]`: one fixed slot,
    // then the variadic field list.
    let record =
        meta::operand_record(cx, meta::LIST_TAG, 0.0, Assoc::Left, Schedule::Struct, &["scope"]);
    let struct_ = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("struct", IdContext::new(struct_, cx.root_scope));

    let record = meta::record(cx.store, meta::TOKEN_TAG, Schedule::Colon);
    let colon = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(":", IdContext::new(colon, cx.root_scope));

    let record = meta::record(cx.store, meta::TOKEN_TAG, Schedule::Separator);
    let comma = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(",", IdContext::new(comma, cx.root_scope));

    struct_
}
