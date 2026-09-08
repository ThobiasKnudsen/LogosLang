// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A `record`: one per declared name, the trie entry itself — the dyad the name
//! denotes, the scope it was declared in, its range of life inside that scope,
//! and its gate set (DESIGN ›`mut` is a gate on the record‹ and ›Name
//! resolution is scope-filtered‹, ruled 7 September 2026; `id_context` renamed
//! `record` the same day). Not the *shared-member record* a type value
//! holds (`meta`): that is a type's metadata, this is a name's.
//!
//! A single spelling can denote different identities in different scopes, so the
//! name index stores a *list* of `record`s per spelling (see [`crate::regex_trie`]).
//! Resolution keeps the candidate whose `scope` is currently open *and* whose
//! range covers the point of use, and, because shadowing is disallowed (a name
//! may not be redeclared while another declaration of it is live), exactly one
//! candidate survives, or none for a genuine out-of-scope use. Two survivors is
//! impossible under that rule and signals a corrupt index (see DESIGN ›Name
//! resolution is scope-filtered‹).
//!
//! One record per *name*, never one per identity: `x := i32` binds a second
//! name to i32's own dyad, and a record shared through the dyad would have let
//! `x := pub i32` gate i32 itself. Per-name records keep `x:gate` x's and
//! `i32:gate` i32's, with both `:dyad` at one cell.
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
//!
//! The gate set: v0.1.0 has no gates (DESIGN ›Feasibility and effort‹, ruled
//! 4 September 2026), so `gate` is null on every record until gates land
//! (issue #33). The slot exists so the one lookup that answers reachable and
//! live is the one that will answer permitted.

use crate::dyad::DyadPtr;

/// One candidate for a spelling: the dyad it denotes, the scope it was
/// declared in, its range of life within that scope's body, and its gate set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    /// The dyad this name denotes (`a:dyad`).
    pub dyad: DyadPtr,
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
    /// The name's gate set; null while the name has none, which in v0.1.0 is
    /// always.
    pub gate: DyadPtr,
}

impl Record {
    /// A new, live, ungated `record` pairing `dyad` with its declaring `scope`.
    pub fn new(dyad: DyadPtr, scope: DyadPtr) -> Self {
        Record {
            dyad,
            scope,
            start: std::ptr::null_mut(),
            end: std::ptr::null_mut(),
            gate: std::ptr::null_mut(),
        }
    }

    /// Whether an `own` or `drop` has ended this name's life.
    pub fn is_dead(&self) -> bool {
        !self.end.is_null()
    }
}
