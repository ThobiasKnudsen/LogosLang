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
//! The definition body's parse lives in
//! [`crate::parse::Parser::parse_type_body`] because it needs the parser's
//! tape, scope stack, and reentrant expression parse (DESIGN ›The constructor
//! is a field‹, #61): a body is a scope whose bare lines fill the four slots
//! `type` declares for every type it builds — `precedence`, `associativity`,
//! `constructor`, `destructor`, filled with `=` — or declare its own members,
//! and whose `instance (…)` block holds the per-instance fields. Here we only
//! create the root, attach its constructor, and register what the body
//! consumes: `,`, `instance`, the two associativity values `left` and
//! `right` (identities of type `type`, like a keyword, ruled 9 September
//! 2026), and the four slot markers.

use crate::parse::SLOT_NAMES;

use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::store::Store;

/// Create the `logos : logos` root and return it.
pub(super) fn register_root(store: &mut Store) -> DyadPtr {
    let logos_ = store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: `logos_` was just allocated; make it its own logos.
    unsafe {
        (*logos_).ty = logos_;
    }
    logos_
}

/// Spell the root, attach its constructor, and register what a definition
/// body consumes: `,`, `instance`, `left`, `right`, and the four slot
/// markers, returned in that order.
pub(super) fn register_syntax(cx: &mut Cx) -> (DyadPtr, DyadPtr, DyadPtr, DyadPtr, [DyadPtr; 4]) {
    // The spelling: `type` resolves to the root as a first-class value (DESIGN
    // ›Substrate vocabulary‹, ruled 4 September 2026: `type` is the ground and
    // the definition keyword, `logos` names the language). `logos` stays a
    // transitional alias of the same identity until the `language` identity
    // exists and can give `logos (…)` its ruled meaning. The inserts wait
    // until here, because `register_root` builds the fixed point before the
    // trie and `root_scope` exist.
    cx.declare("type", cx.type_);
    cx.declare("logos", cx.type_);
    // The constructor: a following `( body )` defines a type; anything else
    // declines the right and the constructor "yields its own dyad as-is" —
    // the classifier as a value (DESIGN ›Expressions are self-delimiting‹),
    // exactly the numeric logos' shape.
    cx.metas.insert(cx.type_, |p, id, tape| {
        // The definition bracket is read at discovery only (DESIGN: `type`
        // "reads its own bracket … constructed at discovery, before the
        // bracket is lexed"); woken at a boundary — the `-> type` of a return
        // logos — the classifier stands as its own value.
        if p.discovering() && p.at_open() {
            let node = p.parse_type_body(id)?;
            tape.place(node);
            return Ok(crate::parse::Constructed::Placed);
        }
        let value = p.stand_as_value(tape, id);
        tape.place(value);
        Ok(crate::parse::Constructed::Placed)
    });

    // `instance (…)`: the per-instance fields of the type being defined,
    // read at discovery like every bracket reader (the word itself recorded
    // as open, 2 September 2026).
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::READER);
    let instance_ = cx.store.alloc_raw(cx.type_, record);
    cx.declare("instance", instance_);
    cx.metas.insert(instance_, |p, id, tape| p.construct_instance_block(id, tape));

    // `left` and `right`: associativity's two values, identities of type
    // `type` with nothing behind them — a type's own record, not a
    // delimiter's, so each stands as an operand (`^.associativity == right`).
    let side = |cx: &mut Cx, name: &str| {
        let record = meta::record(cx.store, meta::TYPEREC_TAG, meta::prec::INERT);
        let id = cx.store.alloc_raw(cx.type_, record);
        cx.declare(name, id);
        id
    };
    let left_ = side(cx, "left");
    let right_ = side(cx, "right");

    // The four slot markers: what the slot names denote inside a body
    // (declared there per definition), never spelled outside one. Both slots
    // undefined, so a read or an ordinary `=` on them is refused at parse.
    let slots = SLOT_NAMES.map(|_| cx.store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut()));

    // `:` is no declaration operator (DESIGN ›Declarations are immutable by
    // default‹, ruled 2 September 2026): `key := T ?` is the valueless form,
    // and the seed's `:` stood in only until the driver converged (#59).
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::COMMA);
    let comma = cx.store.alloc_raw(cx.type_, record);
    cx.declare(",", comma);

    (comma, instance_, left_, right_, slots)
}
