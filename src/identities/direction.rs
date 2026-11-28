// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `direction`: the type of associativity's values, physically an `i32`. `left` and `right` are
//! shared value nodes typed `direction`, as `true` and `false` are `bool`'s (›Associativity's
//! values are `left` and `right`, the values of the type `direction`‹ [[R216]]).

use super::numtype::{self, NumType};
use super::{meta, Core, Cx};
use crate::dyad::{self, DyadPtr};
use crate::parse::Assoc;

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

/// The side of the `direction` value whose bits a run gave, `None` for one neither `left` nor
/// `right` holds.
///
/// # Safety
/// `types` must hold `left` and `right` as `register` made them.
pub(crate) unsafe fn side(types: &Core, bits: i64) -> Option<Assoc> {
    // SAFETY: `left` and `right` are `i32` blobs of `direction`.
    let held =
        |value: DyadPtr| unsafe { numtype::read_scalar_nt(NumType::I32, dyad::value(value)) };
    let bits = bits as i32 as i64;
    if bits == held(types.left_) {
        Some(Assoc::Left)
    } else if bits == held(types.right_) {
        Some(Assoc::Right)
    } else {
        None
    }
}
