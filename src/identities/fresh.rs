// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The two fresh-spelling patterns: the identities of the language start that
//! recognize a spelling the trie does not otherwise know (#110; DESIGN ›The
//! scope's constructor is the driver‹, ruled 10 September 2026: "the two
//! fresh-spelling patterns — a word, `[A-Za-z_][A-Za-z0-9_]*`, and a symbol
//! run, `[^A-Za-z0-9_\s()\[\],«»#]+` — are identities of the language start
//! beside the five names … each constructing the fresh null-slotted dyad").
//!
//! Both carry a `lex_rank` below the default every declared spelling reads, so
//! they match only where nothing declared does: `@` beats the run `@@`, a
//! declared `a` beats the run `a`, and the word pattern alone reaches past `a`
//! to `ab2` (›A language is a section with an authored start‹: unknown
//! spellings "may stand only on the left of `:=`"). Their construction — a
//! fresh dyad with both slots undefined, its spelling kept in the cell's span
//! — happens when the cell is lexed ([`crate::parse::ScopeStack::lex`] names
//! the winner, `Parser::lex_cell` mints the dyad), the same act a constructor
//! slot would perform at discovery; the slot stays empty until the driver keys
//! fresh cells by type rather than by a null dyad. Anything the trie can hold
//! is thereby nameable; a spelling that is itself a pattern enters through
//! `regex «…» := type (…)` (#114).

use super::{meta, Cx};

/// The rank both patterns carry: below the `0` every declared spelling reads
/// by default.
pub(crate) const FRESH_LEX_RANK: f64 = -1.0;

/// The word pattern, in the trie's regex syntax.
pub(crate) const WORD: &str = "[A-Za-z_][A-Za-z0-9_]*";

/// The symbol-run pattern: everything that is not a word character,
/// whitespace, a bracket, the separator, a quote mark, or the comment mark.
pub(crate) const SYMBOL: &str = r"[^A-Za-z0-9_\s()\[\],«»#]+";

/// Whether `key` is one of the two patterns: how the lexer tells a fresh
/// run from a declared spelling, by the key the index matched (the pattern is
/// its own name, so no handle is needed and a bare scope stack knows it too).
pub(crate) fn is_fresh_key(key: &str) -> bool {
    key == WORD || key == SYMBOL
}

/// Register both patterns at the root.
pub(super) fn register(cx: &mut Cx) {
    let mut mint = |pattern: &str| {
        let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
        let id = cx.store.alloc_raw(cx.type_, record);
        // SAFETY: `id` was just allocated above with the record `record`.
        unsafe { meta::set_lex_rank(id, FRESH_LEX_RANK) };
        cx.declare(pattern, id);
    };
    mint(WORD);
    mint(SYMBOL);
}
