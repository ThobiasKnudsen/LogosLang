// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `scope`: the type of a scope node, the graph's own spine (DESIGN
//! ›Meta-navigation walks the graph; the scope stack is the graph's own spine‹).
//! A scope is a node whose type is `scope`; entering one is descending into its
//! subtree, leaving it is ascending, and name resolution walks up the chain of
//! open scopes.
//!
//! v1 uses a scope only as an addressable membership marker: the parser's
//! [`ScopeStack`](crate::parse::ScopeStack) keys on its address, so the root scope
//! and each struct/parameter-list scope are typed `scope` and carry an undefined
//! value. Its grant-bearing `gate` role (visibility and access; DESIGN ›Read and
//! write are one mechanism‹) is deferred, so nothing reads the value slot yet.
//! `scope` is created internally by the parser, never written in source, so it
//! needs no spelling, run, or compile behaviour.

use crate::dyad::DyadPtr;
use crate::store::Store;

/// Create the `scope` type (its own type is `type`) and return it. Called before
/// the build context exists, since the root scope is itself typed `scope`.
pub(super) fn register(store: &mut Store, type_: DyadPtr) -> DyadPtr {
    store.alloc_raw(type_, std::ptr::null_mut())
}
