// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `type`/`logos`: the one classifier, the `logos : logos` self-loop whose
//! layout is the seed's only a-priori knowledge. `type ( body )` defines a
//! type ([`crate::parse::Parser::parse_type_body`]); bare, it is the classifier as a value.
//! DESIGN ›Substrate vocabulary‹

use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::store::Store;

pub(super) fn register_root(store: &mut Store) -> DyadPtr {
    let logos_ = store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: `logos_` was just allocated; make it its own logos.
    unsafe {
        (*logos_).ty = logos_;
    }
    logos_
}

/// Returns `,`, `left` and `right`, in that order.
pub(super) fn register_syntax(cx: &mut Cx) -> (DyadPtr, DyadPtr, DyadPtr) {
    // Spelled here, not in `register_root`: the trie and the root scope did not exist yet.
    // `logos` is a transitional alias of `type` until the `language` identity exists.
    cx.declare("type", cx.type_);
    cx.declare("logos", cx.type_);
    cx.metas.insert(cx.type_, |p, id, tape| {
        // The bracket is read at discovery only; woken at a boundary (`-> type`)
        // the classifier stands as itself.
        if p.reads_own_bracket(tape) {
            let node = p.parse_type_body(id)?;
            tape.place(node);
            return Ok(crate::parse::Constructed::Placed);
        }
        let value = p.stand_as_value(tape, id);
        tape.place(value);
        Ok(crate::parse::Constructed::Placed)
    });

    // `left` and `right` carry a type's own record, not a delimiter's, so each
    // stands as an operand.
    let side = |cx: &mut Cx, name: &str| {
        let record = meta::record(cx.store, meta::TYPEREC_TAG, meta::prec::INERT);
        let id = cx.store.alloc_raw(cx.type_, record);
        cx.declare(name, id);
        id
    };
    let left_ = side(cx, "left");
    let right_ = side(cx, "right");

    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::COMMA);
    let comma = cx.store.alloc_raw(cx.type_, record);
    cx.declare(",", comma);

    (comma, left_, right_)
}
