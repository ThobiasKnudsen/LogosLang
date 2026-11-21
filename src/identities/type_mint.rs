// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `type_mint`, the one keeper of every mint, written in Logos: the seed carries
//! identities/type_mint.logos and reads it into the root scope before any program, so the name
//! stands wherever a type mints. ›Every type is minted by its contents through `type_mint`; the
//! name that holds a `type ( … )` gives it its identity‹ [[R383]]

use crate::parse::{LinesEnd, Parser, ScopeStack};
use crate::regex_trie::RegexTrie;
use crate::store::Store;
use crate::Core;

const PATH: &str = "identities/type_mint.logos";
const SOURCE: &str = include_str!("../../identities/type_mint.logos");

/// Reads the file as lines of the root scope, so its names are ambient.
///
/// # Panics
/// Where the file the seed was built with does not parse or run: a defect of the seed.
pub(super) fn load(store: &mut Store, trie: &mut RegexTrie, core: &Core) {
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let mut p = Parser::new(SOURCE, store, trie, core, scopes);
    let read = p.read_lines(LinesEnd::TextEnd).and_then(|_| p.finish());
    if let Err(e) = read {
        let message = crate::report::parse_message(&e);
        panic!("{}", crate::report::render(PATH, SOURCE, p.offset(), &message));
    }
    p.hand_exit();
}
