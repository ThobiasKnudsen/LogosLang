// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The files written in Logos that the seed carries and reads at every start, before any program.
//! All are read in one section under the root, as an imported file is read, so a program sees only
//! the names the root is given (›Importing is dropping the text there, wrapped in its own scope‹
//! [[R306]]).

use crate::dyad::DyadPtr;
use crate::parse::{LinesEnd, Parser, Resolved, ScopeStack};
use crate::regex_trie::RegexTrie;
use crate::store::Store;
use crate::Core;

/// A file the seed carries: its path in the repository, which a report names, and its text.
pub(super) struct File {
    pub(super) path: &'static str,
    pub(super) source: &'static str,
}

/// The section the carried files' lines stand in, whose names live for the run.
pub(super) fn section(store: &mut Store, core: &Core) -> DyadPtr {
    super::scope::mint(store, core.scope, core.root_scope)
}

/// Reads `file` into `section`, after the files read there before it.
///
/// # Panics
/// Where the file the seed was built with does not parse or run: a defect of the seed.
pub(super) fn read(
    store: &mut Store,
    trie: &mut RegexTrie,
    core: &Core,
    section: DyadPtr,
    file: &File,
) {
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    scopes.push(section);
    let mut p = Parser::new(file.source, store, trie, core, scopes);
    let read = p.read_lines(LinesEnd::TextEnd).and_then(|_| p.finish());
    if let Err(e) = read {
        let message = crate::report::parse_message(&e);
        panic!("{}", crate::report::render(file.path, file.source, p.offset(), &message));
    }
    p.hand_exit();
}

/// What `name` names in `scope`.
///
/// # Panics
/// Where `scope` declares no `name`: a defect of the file the seed was built with.
pub(super) fn name_in(trie: &RegexTrie, scope: DyadPtr, file: &File, name: &str) -> Resolved {
    let mut names = ScopeStack::new();
    names.push(scope);
    names.resolve(trie, name).unwrap_or_else(|_| panic!("{}: no `{name}`", file.path))
}

/// What the member `name` of the type whose body is `body` names.
///
/// # Panics
/// Where `body` declares no member `name`: a defect of the file the seed was built with.
pub(super) fn member_in(trie: &RegexTrie, body: DyadPtr, file: &File, name: &str) -> Resolved {
    let mut members = ScopeStack::new();
    members.push(body);
    members.resolve_member(trie, name).unwrap_or_else(|_| panic!("{}: no `{name}`", file.path))
}

/// Gives the root `names`, each with the binding a carried file declared it by.
///
/// # Panics
/// Where the root refuses a name: a defect of the seed.
pub(super) fn publish(
    store: &mut Store,
    trie: &mut RegexTrie,
    core: &Core,
    file: &File,
    names: &[(String, DyadPtr)],
) {
    let mut root = ScopeStack::new();
    root.push(core.root_scope);
    Parser::new("", store, trie, core, root)
        .publish(names)
        .unwrap_or_else(|e| panic!("{}: {}", file.path, crate::report::parse_message(&e)));
}
