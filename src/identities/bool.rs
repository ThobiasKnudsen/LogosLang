// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `bool`: the type of a boolean value, physically an `i32` 0/1. `true` and
//! `false` are shared value nodes typed `bool` with no parse construct.

use super::numtype::NumType;
use super::{meta, Cx};
use crate::dyad::DyadPtr;

pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    // The record carries the I32 width kind; bool-ness lives in the identity itself.
    let record = meta::record(cx.store, NumType::I32 as u8, meta::prec::INERT);
    let bool_ = cx.store.alloc_raw(cx.type_, record);
    cx.declare("bool", bool_);

    let true_ = literal(cx, bool_, 1);
    cx.declare("true", true_);
    let false_ = literal(cx, bool_, 0);
    cx.declare("false", false_);

    bool_
}

fn literal(cx: &mut Cx, bool_: DyadPtr, v: i32) -> DyadPtr {
    let value = cx.store.alloc_bytes(&v.to_ne_bytes());
    cx.store.alloc_raw(bool_, value)
}

pub(crate) fn literal_node(store: &mut crate::store::Store, bool_: DyadPtr, v: bool) -> DyadPtr {
    let value = store.alloc_bytes(&i32::from(v).to_ne_bytes());
    store.alloc_raw(bool_, value)
}
