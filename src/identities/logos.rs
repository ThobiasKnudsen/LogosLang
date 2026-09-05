// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `logos`: the one classifier — the `logos : logos` self-loop, the one node
//! whose logos is itself, the fixed point whose layout is the seed's only
//! a-priori knowledge. Since the merge of the former `logos`/`record`/`language`
//! identities (DESIGN ›Substrate vocabulary‹, ruled July 2026) the root also
//! carries the record path: `logos ( field-list )` derives a record layout —
//! the field list *is* a function's parameter list (DESIGN ›A function's
//! surface‹), so the same parse serves both — while a bare `logos` yields the
//! classifier itself as a value, the same declines-the-right shape a numeric
//! logos's constructor has. A record logos node stores the layout its
//! definition derived (issue #47): its value is a [`meta::RECORD_TAG`] record
//! holding the field-name scope, the `fields` array node over the field
//! declarations, and the packed `size_bytes` — filled at definition, where the
//! layout locks. Field names are not stored on the record: they enter the
//! shared name index and resolve by open-scope filtering (a per-record names
//! store is recorded as rejected).
//!
//! The field-list parse lives in [`crate::parse::Parser::parse_record`] because
//! it needs the parser's tape, scope stack, and reentrant expression parse;
//! here we only create the root, attach its constructor, and register the
//! field-list punctuation `:` and `,` it consumes.

use super::{meta, Cx};
use crate::id_context::IdContext;
use crate::store::Store;
use crate::dyad::DyadPtr;

/// Create the `logos : logos` root and return it.
pub(super) fn register_root(store: &mut Store) -> DyadPtr {
    let logos_ = store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: `logos_` was just allocated; make it its own logos.
    unsafe {
        (*logos_).ty = logos_;
    }
    logos_
}

/// Spell the root, attach its merged constructor, and register the field-list
/// punctuation (`:`, `,`) the record path consumes, returning the two
/// punctuation handles.
pub(super) fn register_syntax(cx: &mut Cx) -> (DyadPtr, DyadPtr) {
    // The spelling: `type` resolves to the root as a first-class value (DESIGN
    // ›Substrate vocabulary‹, ruled 4 September 2026: `type` is the ground and
    // the definition keyword, `logos` names the language). `logos` stays a
    // transitional alias of the same identity until the `language` identity
    // exists and can give `logos (…)` its ruled meaning. The inserts wait
    // until here, because `register_root` builds the fixed point before the
    // trie and `root_scope` exist.
    cx.trie.insert("type", IdContext::new(cx.type_, cx.root_scope));
    cx.trie.insert("logos", IdContext::new(cx.type_, cx.root_scope));
    // The merged constructor: a following `( field-list )` builds a record
    // logos; anything else declines the right and the constructor "yields its
    // own dyad as-is" — the classifier as a value (DESIGN ›Expressions are
    // self-delimiting‹), exactly the numeric logos' shape.
    cx.metas.insert(cx.type_, |p, id, tape| {
        if p.at_open() {
            let node = p.parse_record(id)?;
            tape.place(node);
            return Ok(crate::parse::Constructed::Placed);
        }
        tape.place(id);
        Ok(crate::parse::Constructed::Placed)
    });

    // `:` is a tight extender: its constructor declares a fresh name token to
    // its left (`name : logos`) and declines anywhere else, staying a bare
    // delimiter for the field lists the record parse consumes itself.
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::DECLARE);
    let colon = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(":", IdContext::new(colon, cx.root_scope));
    cx.metas.insert(colon, |p, _id, tape| p.construct_typed_decl(tape));

    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::COMMA);
    let comma = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(",", IdContext::new(comma, cx.root_scope));

    (colon, comma)
}
