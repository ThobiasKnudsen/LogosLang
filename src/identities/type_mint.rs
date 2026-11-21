// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `type_mint`, the one keeper of every mint, written in Logos: the seed carries
//! identities/type_mint.logos and reads it into the root scope before any program, so the name
//! stands wherever a type mints. ›Every type is minted by its contents through `type_mint`; the
//! name that holds a `type ( … )` gives it its identity‹ [[R383]]

use super::meta;
use crate::dyad::{self, DyadPtr};
use crate::parse::{LinesEnd, Parser, ScopeStack};
use crate::regex_trie::RegexTrie;
use crate::store::Store;
use crate::Core;

const PATH: &str = "identities/type_mint.logos";
const SOURCE: &str = include_str!("../../identities/type_mint.logos");
const NAME: &str = "type_mint";

/// `type_mint`'s functions, which the formers written in the seed call.
#[derive(Debug, Clone, Copy)]
pub struct Members {
    pub(crate) step: DyadPtr,
    pub(crate) step_name: DyadPtr,
    pub(crate) step_word: DyadPtr,
    pub(crate) kept: DyadPtr,
    pub(crate) keep: DyadPtr,
}

impl Members {
    /// What a core holds until the file is read.
    pub(super) const fn unread() -> Self {
        let null = std::ptr::null_mut();
        Self { step: null, step_name: null, step_word: null, kept: null, keep: null }
    }

    pub(crate) fn is_read(&self) -> bool {
        !self.step.is_null()
    }
}

/// Reads the file as lines of the root scope, so its names are ambient.
///
/// # Panics
/// Where the file the seed was built with does not parse or run, or lacks a function the seed
/// calls: a defect of the seed.
pub(super) fn load(store: &mut Store, trie: &mut RegexTrie, core: &Core) -> Members {
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let mut p = Parser::new(SOURCE, store, trie, core, scopes);
    let read = p.read_lines(LinesEnd::TextEnd).and_then(|_| p.finish());
    if let Err(e) = read {
        let message = crate::report::parse_message(&e);
        panic!("{}", crate::report::render(PATH, SOURCE, p.offset(), &message));
    }
    p.hand_exit();
    let function = |scope: DyadPtr, name: &str| {
        let mut members = ScopeStack::new();
        members.push(scope);
        members
            .resolve(trie, name)
            .map(|r| r.identity)
            .unwrap_or_else(|_| panic!("{PATH}: no `{name}`"))
    };
    let keeper = function(core.root_scope, NAME);
    // SAFETY: the file's one line declared `type_mint` as a type with a body.
    let body = unsafe { meta::record_body_of(keeper) };
    let member = |name: &str| {
        let f = function(body, name);
        // SAFETY: `f` is the dyad a name of the body holds.
        assert!(unsafe { dyad::ty(f) } == core.fn_type, "{PATH}: `{name}` is no function");
        f
    };
    Members {
        step: member("step"),
        step_name: member("step_name"),
        step_word: member("step_word"),
        kept: member("kept"),
        keep: member("keep"),
    }
}
