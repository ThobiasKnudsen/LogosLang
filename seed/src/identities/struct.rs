// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `struct`: the type whose constructor derives a layout from a `( field-list )`.
//! Written `struct ( name : type, … | bare name )`; the field list *is* a
//! function's parameter list (DESIGN ›A function's surface‹), so the same parse
//! serves both. A struct type node stores the layout its definition derived
//! (issue #47): its value is a [`meta::STRUCT_TAG`] record holding the
//! field-name scope, the `fields` array node over the field declarations, and
//! the packed `size_bytes` — filled at definition, where the layout locks
//! (DESIGN ›a type whose constructor derives the layout automatically —
//! reading the field declarations in its scope and filling `fields` and
//! `size_bytes`‹). Field names are not stored on the struct: they enter the
//! shared name index and resolve by open-scope filtering (a per-struct names
//! store is recorded as rejected).
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

/// Register `struct` and the field-list punctuation (`:`, `,`) it consumes,
/// returning all three handles.
pub(super) fn register(cx: &mut Cx) -> (DyadPtr, DyadPtr, DyadPtr) {
    // A struct definition's value is `[scope, field…, null]`: one fixed slot,
    // then the variadic field list.
    let record =
        meta::operand_record(cx, meta::LIST_TAG, f64::NAN, Assoc::Left, Schedule::Struct, &["scope"]);
    let struct_ = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("struct", IdContext::new(struct_, cx.root_scope));
    cx.metas
        .insert(struct_, |p, id, _tape| p.parse_struct(id).map(crate::parse::Constructed::Node));

    let record = meta::record(cx.store, meta::TOKEN_TAG, f64::NAN, Schedule::Colon);
    let colon = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(":", IdContext::new(colon, cx.root_scope));

    let record = meta::record(cx.store, meta::TOKEN_TAG, f64::NAN, Schedule::Separator);
    let comma = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(",", IdContext::new(comma, cx.root_scope));

    (struct_, colon, comma)
}
