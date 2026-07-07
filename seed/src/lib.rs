//! The Logos bootstrap seed.
//!
//! See `V1PLAN.md` for the build order and `DESIGN.md` for the rationale.
//! The first subsystem built here is the lexer's regex-trie (`lex`), a port of
//! `regex_trie.zig` / `regex_splitting.zig` from the `zig_pivot` branch.

pub mod core;
pub mod dyad;
pub mod id_context;
pub mod lex;
pub mod parse;
pub mod run;
pub mod store;
