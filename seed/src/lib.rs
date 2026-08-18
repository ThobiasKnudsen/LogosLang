// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The Logos bootstrap seed.
//!
//! See `DESIGN.md` and `language_sketch.logos` for what the seed builds toward.
//! The seed's hand-built core identities live in `identities` (one file each);
//! the phase engines are the lexing name index (`regex_trie`), `parse`, `run`,
//! and `compile`, over the `store`.

pub mod compile;
pub mod identities;
pub mod parse;
pub mod reflect;
pub mod regex_splitting;
pub mod regex_trie;
pub mod report;
pub mod run;
pub mod store;

// The node cell and name-resolution pairing are core identities, but the rest of
// the crate reaches them by these short paths.
pub use identities::{dyad, id_context, Core};
