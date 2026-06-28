//! Lexing: the single string/regex -> identity index.
//!
//! `regex_splitting` breaks a regex pattern into the literal-prefix paths and
//! residual regex segments the trie stores; `trie` is the hybrid regex-trie
//! itself. Both are faithful ports of the Zig originals, with PCRE2 replaced by
//! the Rust `regex` crate (see the module docs in `trie`).

pub mod regex_splitting;
pub mod trie;

pub use trie::{MatchResult, RegexTrie, RegexTrieError};
