// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `type`: the `Type : Type` self-loop, the one node whose type is itself, the
//! fixed point whose layout is the seed's only a-priori knowledge.

use crate::dyad::DyadPtr;
use crate::store::Store;

/// Create the `Type : Type` root and return it.
pub(super) fn register_root(store: &mut Store) -> DyadPtr {
    let type_ = store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: `type_` was just allocated; make it its own type.
    unsafe {
        (*type_).ty = type_;
    }
    type_
}
