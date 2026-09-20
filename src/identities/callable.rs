// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `callable`, the type whose values are the complete jump information: an `@exec`
//! entry plus the convention identity the jump follows. Every exec leaf is one, and the
//! only path from a runnable node to machine code is the leaf in its op slot. Arity is
//! no part of a callable; the caller derives it. Value layout, 16 bytes: `[entry:
//! usize][convention: dyad@]`.

use crate::dyad::DyadPtr;
use crate::run::RunFn;
use crate::store::Store;

use super::{meta, string, Cx};

const ENTRY_OFF: usize = 0;
const CONVENTION_OFF: usize = 8;

pub(crate) struct Callables {
    pub callable: DyadPtr,
    pub convention: DyadPtr,
    /// A Rust shim `fn(&mut Runtime, DyadPtr) -> Result<i64, RunError>`.
    pub seed_native: DyadPtr,
    /// Compiled code taking `(argv, argc)` over i64 containers.
    pub container_i64: DyadPtr,
    /// The one constructor signature, `ConstructFn`.
    pub seed_parse: DyadPtr,
}

/// Neither type has a spelling: callables are minted by registration loops and compilation,
/// never written in source.
pub(super) fn register(cx: &mut Cx) -> Callables {
    let record = meta::record(cx.store, meta::CALLABLE_TAG, meta::prec::INERT);
    let callable = cx.store.alloc_raw(cx.type_, record);

    let record = meta::record(cx.store, meta::CONVENTION_TAG, meta::prec::INERT);
    let convention = cx.store.alloc_raw(cx.type_, record);

    let seed_native = mint_convention(cx, convention, b"seed-native");
    let container_i64 = mint_convention(cx, convention, b"container-i64");
    let seed_parse = mint_convention(cx, convention, b"seed-parse");

    Callables { callable, convention, seed_native, container_i64, seed_parse }
}

/// `{type: convention, value -> name string node}`.
fn mint_convention(cx: &mut Cx, convention: DyadPtr, name: &[u8]) -> DyadPtr {
    let text = string::build_text(cx.store, cx.string_, name);
    cx.store.alloc_raw(convention, text.cast())
}

/// The one licensed mint: `entry` must be the address of code the convention can jump to.
pub(crate) fn mint(
    store: &mut Store,
    callable: DyadPtr,
    entry: usize,
    convention: DyadPtr,
) -> DyadPtr {
    let mut bytes = [0u8; 16];
    bytes[ENTRY_OFF..CONVENTION_OFF].copy_from_slice(&entry.to_ne_bytes());
    bytes[CONVENTION_OFF..].copy_from_slice(&(convention as usize).to_ne_bytes());
    let value = store.alloc_bytes(&bytes);
    store.alloc_raw(callable, value)
}

/// The fn-pointer-to-address cast, done once where addresses enter the graph.
pub(crate) fn mint_native(
    store: &mut Store,
    callable: DyadPtr,
    entry: RunFn,
    convention: DyadPtr,
) -> DyadPtr {
    mint(store, callable, entry as usize, convention)
}

/// For a leaf minted with a zero entry: `f.compile()` mints its leaf at parse and patches
/// the entry in at run time.
///
/// # Safety
/// `leaf` must be a callable value (`is_callable`) from the store.
pub(crate) unsafe fn install_entry(leaf: DyadPtr, entry: usize) {
    std::ptr::write_unaligned((*leaf).value.add(ENTRY_OFF) as *mut usize, entry);
}

/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_callable(node: DyadPtr) -> bool {
    let logos = (*node).ty;
    !logos.is_null() && meta::kind_of(logos) == Some(meta::CALLABLE_TAG)
}

/// # Safety
/// `leaf` must be a callable value (`is_callable`).
pub(crate) unsafe fn entry_of(leaf: DyadPtr) -> usize {
    std::ptr::read_unaligned((*leaf).value.add(ENTRY_OFF) as *const usize)
}

/// # Safety
/// As `entry_of`.
pub(crate) unsafe fn convention_of(leaf: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*leaf).value.add(CONVENTION_OFF) as *const DyadPtr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identities::Core;
    use crate::regex_trie::RegexTrie;

    #[test]
    fn a_minted_callable_round_trips_entry_and_convention() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        fn probe() {}
        let entry = probe as fn() as usize;
        let leaf = mint(&mut store, core.callable_, entry, core.conv_seed_native);
        // SAFETY: `leaf` was just minted; the core handles are valid identities.
        unsafe {
            assert!(is_callable(leaf));
            assert_eq!(entry_of(leaf), entry);
            assert_eq!(convention_of(leaf), core.conv_seed_native);
        }
    }

    #[test]
    fn conventions_are_named_identities_of_the_convention_type() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // SAFETY: the handles are identities Core::build just allocated.
        unsafe {
            assert_eq!((*core.conv_seed_native).ty, core.convention_);
            assert_eq!((*core.conv_container).ty, core.convention_);
            assert_eq!(
                crate::reflect::text_of((*core.conv_seed_native).value.cast()),
                b"seed-native"
            );
            assert_eq!(
                crate::reflect::text_of((*core.conv_container).value.cast()),
                b"container-i64"
            );
            assert_eq!(meta::kind_of(core.callable_), Some(meta::CALLABLE_TAG));
            assert_eq!(meta::kind_of(core.convention_), Some(meta::CONVENTION_TAG));
            assert!(!is_callable(core.conv_seed_native));
        }
    }
}
