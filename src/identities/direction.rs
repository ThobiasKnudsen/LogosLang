// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `direction`: the type of associativity's values, physically an `i32`. `left` and `right` are
//! shared value nodes typed `direction`, as `true` and `false` are `bool`'s (›Associativity's
//! values are `left` and `right`, the values of the type `direction`‹ [[R216]]).

use super::numtype::NumType;
use super::{meta, Cx};
use crate::dyad::DyadPtr;

/// Returns `direction`, `left` and `right`, in that order.
pub(super) fn register(cx: &mut Cx) -> (DyadPtr, DyadPtr, DyadPtr) {
    let record = meta::record(cx.store, NumType::I32 as u8, meta::prec::INERT);
    let direction = cx.store.alloc_head(cx.type_, record);
    cx.declare("direction", direction);

    let left = cx.store.alloc_blob(direction, &0i32.to_ne_bytes());
    cx.declare("left", left);
    let right = cx.store.alloc_blob(direction, &1i32.to_ne_bytes());
    cx.declare("right", right);

    (direction, left, right)
}
