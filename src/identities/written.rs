// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The operations the seed carries written in Logos, `+ * / %` in identities/arithmetic.logos:
//! read at its start, after the other carried files, each taking its name from the native version
//! the seed started with, which then builds only what those files wrote before
//! (›A dependency cycle among core identities is broken with native versions first‹ [[R16]]).

use super::carried;
use crate::dyad::DyadPtr;
use crate::regex_trie::RegexTrie;
use crate::store::Store;
use crate::Core;

const FILE: carried::File = carried::File {
    path: "identities/arithmetic.logos",
    source: include_str!("../../identities/arithmetic.logos"),
};
const NAMES: [&str; 4] = ["+", "*", "/", "%"];

/// An operation written in Logos, and the native version whose name it took.
#[derive(Debug, Clone, Copy)]
pub struct Replaced {
    pub(crate) written: DyadPtr,
    pub(crate) native: DyadPtr,
}

/// Each name of the file with its native version, words only, so the core that holds them is
/// carried in the start graph as its other fields are.
pub type Written = [Replaced; NAMES.len()];

/// What a core holds until the file is read.
pub(super) const fn unread() -> Written {
    let null = std::ptr::null_mut();
    [Replaced { written: null, native: null }; NAMES.len()]
}

/// Reads the file into `section`, the carried files' own, and gives the root each of its names in
/// place of the native version.
///
/// # Panics
/// Where the file does not parse or run, or the root holds no native version of a name it
/// declares: a defect of the seed.
pub(super) fn load(
    store: &mut Store,
    trie: &mut RegexTrie,
    core: &Core,
    section: DyadPtr,
) -> Written {
    let natives = NAMES.map(|name| {
        trie.remove(&regex::escape(name), core.root_scope)
            .unwrap_or_else(|_| panic!("{}: the seed declares no `{name}`", FILE.path))
    });
    carried::read(store, trie, core, section, &FILE);
    let declared = NAMES.map(|name| carried::name_in(trie, section, &FILE, name));
    let published: Vec<(String, DyadPtr)> =
        NAMES.iter().zip(&declared).map(|(name, d)| ((*name).to_string(), d.binding)).collect();
    carried::publish(store, trie, core, &FILE, &published);
    std::array::from_fn(|i| Replaced { written: declared[i].identity, native: natives[i] })
}
