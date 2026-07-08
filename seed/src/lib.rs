//! The Logos bootstrap seed.
//!
//! See `V1PLAN.md` for the build order and `DESIGN.md` for the rationale. The
//! seed's hand-built core identities live in `identities` (one file each); the
//! phase engines are `lex`, `parse`, `run`, and `compile`, over the `store`.

pub mod compile;
pub mod identities;
pub mod parse;
pub mod regex_splitting;
pub mod regex_trie;
pub mod run;
pub mod store;

// The node cell and name-resolution pairing are core identities, but the rest of
// the crate reaches them by these short paths.
pub use identities::{dyad, id_context, Core};
