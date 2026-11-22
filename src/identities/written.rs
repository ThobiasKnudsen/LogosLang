// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The operations the seed carries written in Logos, `+ * / %` in identities/arithmetic.logos:
//! read at its start, after `type_mint`'s file, each taking its name from the native version the
//! seed started with, which then builds only what the seed's own files wrote before
//! (›Bootstrapping‹ [[R16]]). The native versions: stand-in for #469.

use super::type_mint;
use crate::dyad::DyadPtr;
use crate::parse::ScopeStack;
use crate::regex_trie::RegexTrie;
use crate::store::Store;
use crate::Core;

/// Each file with the names it declares for the root.
const FILES: [(&str, &str, &[&str]); 1] = [(
    "identities/arithmetic.logos",
    include_str!("../../identities/arithmetic.logos"),
    &["+", "*", "/", "%"],
)];

/// An operation written in Logos, and the native version whose name it took.
#[derive(Debug, Clone, Copy)]
pub struct Replaced {
    pub(crate) written: DyadPtr,
    pub(crate) native: DyadPtr,
    /// The scope the file's own lines stand in.
    pub(crate) file: DyadPtr,
}

/// Reads every file in its order, each in a scope of its own under the root.
///
/// # Panics
/// Where a file does not parse or run, or the root holds no native version of a name it declares:
/// a defect of the seed.
pub(super) fn load(store: &mut Store, trie: &mut RegexTrie, core: &Core) -> Vec<Replaced> {
    let mut replaced = Vec::new();
    for &(path, source, names) in &FILES {
        let natives: Vec<DyadPtr> = names
            .iter()
            .map(|name| {
                trie.remove(&regex::escape(name), core.root_scope)
                    .unwrap_or_else(|_| panic!("{path}: the seed declares no `{name}`"))
            })
            .collect();
        let file = type_mint::read_file(store, trie, core, path, source);
        let mut scopes = ScopeStack::new();
        scopes.push(file);
        for (&name, native) in names.iter().zip(natives) {
            let declared =
                scopes.resolve(trie, name).unwrap_or_else(|_| panic!("{path}: no `{name}`"));
            type_mint::publish(store, trie, core, path, name, declared.binding);
            replaced.push(Replaced { written: declared.identity, native, file });
        }
    }
    replaced
}
