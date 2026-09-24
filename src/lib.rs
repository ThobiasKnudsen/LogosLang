// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The Logos bootstrap seed: the hand-built core identities, the lexing name
//! index, the parser, the interpreter and the compiler, over one node store.
//! `DESIGN.md` says what it builds toward.

pub mod compile;
pub mod identities;
pub mod parse;
pub mod reflect;
pub mod regex_splitting;
pub mod regex_trie;
pub mod report;
pub mod run;
pub mod store;

pub use identities::{binding, dyad, Core};

/// The stack every thread that runs Logos must have. The parser recurses per
/// bracket and the interpreter per call, so the depth guards mean one thing only
/// when the stack under them is always this size (8 MiB overflowed near 5,500 frames).
pub const WORK_STACK_BYTES: usize = 64 * 1024 * 1024;
