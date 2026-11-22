// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The Logos bootstrap seed: the hand-built core identities, the lexing name
//! index, the parser, the interpreter and the compiler, over one node store.
//! The project's rules say what it builds toward.

pub mod compile;
pub mod identities;
pub mod parse;
pub mod reflect;
pub mod regex_splitting;
pub mod regex_trie;
pub mod report;
pub mod run;
pub mod stack;
pub mod start_graph;
pub mod store;

pub use identities::{binding, dyad, Core};
