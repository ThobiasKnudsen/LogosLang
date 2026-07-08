//! An `id_context`: an identity paired with the scope it was declared in.
//!
//! A single spelling can denote different identities in different scopes, so the
//! name index stores a *list* of `id_context`s per spelling (see [`crate::lex::trie`]).
//! Resolution keeps the candidate whose `scope` is currently open and, because
//! shadowing is disallowed (a name may not be redeclared while another
//! declaration of it is live), exactly one candidate survives, or none for a
//! genuine out-of-scope use. Two survivors is impossible under that rule and
//! signals a corrupt index (see DESIGN ›Name resolution is scope-filtered‹).
//!
//! The `scope` stores the *enclosing scope* rather than the declaration node
//! because a dyad has no parent pointer: keying by scope makes membership an
//! O(1) test against the set of open scopes during elaboration.

use crate::dyad::DyadPtr;

/// One candidate for a spelling: the identity it denotes and the scope it was
/// declared in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdContext {
    /// The declared identity this spelling denotes.
    pub identity: DyadPtr,
    /// The enclosing scope the declaration lives in. Whether this scope is open
    /// decides whether the candidate is live.
    pub scope: DyadPtr,
}

impl IdContext {
    /// A new `id_context` pairing `identity` with its declaring `scope`.
    pub fn new(identity: DyadPtr, scope: DyadPtr) -> Self {
        IdContext { identity, scope }
    }
}
