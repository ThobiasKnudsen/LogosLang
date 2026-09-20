// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The two fresh-spelling patterns, a word and a symbol run: identities of the
//! language start that lex a spelling the trie does not otherwise know, ranked
//! below every declared spelling so they match only where nothing declared does.
//! DESIGN ›The scope's constructor is the driver‹

use super::{meta, Cx};

/// Below the `0` every declared spelling reads by default.
pub(crate) const FRESH_LEX_RANK: f64 = -1.0;

pub(crate) const WORD: &str = "[A-Za-z_][A-Za-z0-9_]*";

/// Everything that is not a word character, whitespace, a bracket, the
/// separator, a quote mark, or the comment mark.
pub(crate) const SYMBOL: &str = r"[^A-Za-z0-9_\s()\[\],«»#]+";

/// How the lexer tells a fresh run from a declared spelling, by the key the index matched.
pub(crate) fn is_fresh_key(key: &str) -> bool {
    key == WORD || key == SYMBOL
}

pub(super) fn register(cx: &mut Cx) {
    let mut mint = |pattern: &str| {
        let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
        let id = cx.store.alloc_raw(cx.type_, record);
        let entry = cx.declare(pattern, id);
        // SAFETY: `entry` is the record dyad `declare` just minted.
        unsafe { crate::record::Record::set_lex_rank(entry, FRESH_LEX_RANK) };
    };
    mint(WORD);
    mint(SYMBOL);
}
