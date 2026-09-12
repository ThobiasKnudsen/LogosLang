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
pub use identities::{dyad, record, Core};

/// The stack the seed's work wants under it (#80).
///
/// The parser recurses per open bracket and the interpreter recurses per
/// operand and per call, so a deep Logos program costs Rust stack. The depth
/// guards ([`parse::MAX_BRACKET_DEPTH`], [`run::MAX_CALL_DEPTH`]) turn that
/// into a checked error instead of an abort, but a *limit* is only honest if
/// it means the same thing everywhere: measured on the 8 MiB main thread, a
/// debug build overflowed at a recursion depth near 5,500 and a release build
/// near 22,000, so an unguarded default would fire at whichever number the
/// build profile happened to produce. Anything that runs Logos should hand it
/// a thread of this size — `logos` does, and so does the test that exercises
/// the guards — and then the guards, not the profile, decide.
pub const WORK_STACK_BYTES: usize = 64 * 1024 * 1024;
