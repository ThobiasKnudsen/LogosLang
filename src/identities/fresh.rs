// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The two fresh-spelling patterns, a word and a symbol run: identities of the
//! language start that lex a spelling the trie does not otherwise know, ranked
//! below every declared spelling so they match only where nothing declared does;
//! and `unknown_name`, the type of the node each makes, which holds the spelling.
//! DESIGN ›The scope's constructor is the driver‹, ›Unknown spellings are two pattern
//! identities‹ [[R252]]

use super::numtype::STRING_TAG;
use super::{meta, Cx};
use crate::dyad::DyadPtr;

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

/// `unknown_name` holds text as `string` and `regex` do, `[len][bytes]`, and has no
/// constructor, so the name standing alone is the type.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, STRING_TAG, meta::prec::INERT);
    let unknown_name = cx.store.alloc_head(cx.type_, record);
    cx.declare("unknown_name", unknown_name);
    let mut mint = |pattern: &str| {
        let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
        let id = cx.store.alloc_head(cx.type_, record);
        let entry = cx.declare_pattern(pattern, id);
        // SAFETY: `entry` is the binding dyad `declare_pattern` just minted.
        unsafe { crate::binding::Binding::set_lex_rank(entry, FRESH_LEX_RANK) };
    };
    mint(WORD);
    mint(SYMBOL);
    unknown_name
}
