// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `type`: the one classifier, the `type : type` self-loop whose layout is the seed's only
//! a-priori knowledge. `type ( body )` defines a type
//! ([`crate::parse::Parser::parse_type_body`]), `type T` holds the type `T`
//! ([`crate::parse::Parser::construct_type`]); bare, it is the classifier as a value.
//! ›The ground is one identity, spelled `type`; `logos` is a language‹ [[R134]]

use super::{meta, Cx};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::store::Store;

pub(super) fn register_root(store: &mut Store) -> DyadPtr {
    let type_ = store.alloc_leaf(std::ptr::null_mut());
    // SAFETY: `type_` was just allocated; make it its own type.
    unsafe {
        dyad::set_ty(type_, type_);
    }
    type_
}

/// Returns `,`.
pub(super) fn register_syntax(cx: &mut Cx) -> DyadPtr {
    // Spelled here, not in `register_root`: the trie and the root scope did not exist yet.
    cx.declare("type", cx.type_);
    cx.metas.insert(cx.type_, |p, id, tape| p.construct_type(id, tape));

    let record = meta::token_record(cx, meta::prec::COMMA, crate::parse::Assoc::Left, &[]);
    let comma = cx.store.alloc_head(cx.type_, record);
    cx.declare(",", comma);

    comma
}
