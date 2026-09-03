// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! An `id_context`: an identity paired with the scope it was declared in, and
//! its range of life inside that scope.
//!
//! A single spelling can denote different identities in different scopes, so the
//! name index stores a *list* of `id_context`s per spelling (see [`crate::regex_trie`]).
//! Resolution keeps the candidate whose `scope` is currently open *and* whose
//! range covers the point of use, and, because shadowing is disallowed (a name
//! may not be redeclared while another declaration of it is live), exactly one
//! candidate survives, or none for a genuine out-of-scope use. Two survivors is
//! impossible under that rule and signals a corrupt index (see DESIGN ›Name
//! resolution is scope-filtered‹).
//!
//! The `scope` stores the *enclosing scope* rather than the declaration node
//! because a dyad has no parent pointer: keying by scope makes membership an
//! O(1) test against the set of open scopes during elaboration.
//!
//! The range (DESIGN ›Name resolution is scope-filtered‹, ruled 3 September
//! 2026): `start` is the item of the declaring scope's body that declares the
//! name, and `end` the item holding the `own` or `drop` that makes it dead —
//! null while the name is alive, i.e. to the scope's end. While that item is
//! still being parsed `end` holds the `own`/`drop` node itself, so a later use
//! in the same line already fails; the parser settles both to the body item
//! once the item is complete. A dead entry stays indexed: the range is what
//! reflection reads, and a fresh declaration of the same spelling in the same
//! scope sits beside it. During elaboration the point of use is always the
//! frontier, so liveness reduces to `end` being null; the position comparison
//! is for resolution from a later context, which nothing performs yet.

use crate::dyad::DyadPtr;

/// One candidate for a spelling: the identity it denotes, the scope it was
/// declared in, and its range of life within that scope's body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdContext {
    /// The declared identity this spelling denotes.
    pub identity: DyadPtr,
    /// The enclosing scope the declaration lives in. Whether this scope is open
    /// decides whether the candidate is live.
    pub scope: DyadPtr,
    /// The body item of `scope` that declares the name; null until the parser
    /// settles it (and at top level, which has no body array).
    pub start: DyadPtr,
    /// The body item of `scope` holding the `own` or `drop` that made the name
    /// dead — the node itself while that item is still parsing — or null while
    /// the name is alive.
    pub end: DyadPtr,
}

impl IdContext {
    /// A new, live `id_context` pairing `identity` with its declaring `scope`.
    pub fn new(identity: DyadPtr, scope: DyadPtr) -> Self {
        IdContext { identity, scope, start: std::ptr::null_mut(), end: std::ptr::null_mut() }
    }

    /// Whether an `own` or `drop` has ended this name's life.
    pub fn is_dead(&self) -> bool {
        !self.end.is_null()
    }
}
