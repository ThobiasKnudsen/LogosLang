// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `callable`: the type whose values are the complete jump information — an
//! `@exec` entry plus the convention identity the jump follows (DESIGN ›The
//! callable ground is `@exec`‹; issue #44, settled July 2026).
//!
//! Every exec leaf is a `callable` value: the concrete machine operations
//! (`add_i32`, `lt_f64`, …), the statement natives (`if_native`,
//! `scope_native`, …), and a compiled function's code. Identities carry *no*
//! code — their shared-member records stay pure parse/layout metadata — so the
//! only path from a runnable node to machine code is through the callable leaf
//! stored in the node's own value (its op slot). Arity is deliberately no part
//! of a callable: the caller derives it — the seed-native convention *implies*
//! two containers (runtime context and node), and the container convention
//! reads its count from the signature on `fn` — so nothing of the caller's
//! knowledge rides inside the callee's value (the `bcode {entry, arity}`
//! wrapper stays rejected).
//!
//! Conventions are identities, not an enum: declared metadata a backend renders
//! per target, open-ended (new backends mint new ones). The seed registers two:
//! `seed-native` (a Rust `RunFn` shim taking the runtime context and the node)
//! and `container-i64` (compiled artifacts taking uniform `i64` bit-containers,
//! as `run::call_compiled` jumps to).
//!
//! Value layout (16 bytes, native-endian): `[entry: usize][convention: dyad@]`.

use crate::dyad::DyadPtr;
use crate::run::RunFn;
use crate::store::Store;

use super::{meta, string, Cx};

/// Byte offset of the `@exec` entry in a callable value.
const ENTRY_OFF: usize = 0;
/// Byte offset of the convention identity.
const CONVENTION_OFF: usize = 8;

/// The callable machinery's identities: the `callable` type, the `convention`
/// type, and the conventions the seed mints values under.
pub(crate) struct Callables {
    /// The `callable` type; every exec leaf's `ty`.
    pub callable: DyadPtr,
    /// The `convention` type; the identities below are its values.
    pub convention: DyadPtr,
    /// `seed-native`: a Rust shim `fn(&mut Runtime, DyadPtr) -> Result<i64, RunError>`.
    pub seed_native: DyadPtr,
    /// `container-i64`: compiled code taking uniform `i64` bit-containers.
    pub container_i64: DyadPtr,
    /// `seed-parse`: a Rust parse shim whose exact signature is selected by the
    /// identity's schedule byte (an Atom, Prefix, or Infix build) — the
    /// convention of every constructor slot's leaf until self-hosting ports
    /// constructors to Logos source.
    pub seed_parse: DyadPtr,
}

/// Register the `callable` and `convention` types and the two seed conventions.
/// Neither type has a spelling: callables are minted by registration loops and
/// compilation, never written in source (the same as `convert`).
pub(super) fn register(cx: &mut Cx) -> Callables {
    let record = meta::record(cx.store, meta::CALLABLE_TAG, crate::parse::Schedule::Operand);
    let callable = cx.store.alloc_raw(cx.type_, record);

    let record = meta::record(cx.store, meta::CONVENTION_TAG, crate::parse::Schedule::Operand);
    let convention = cx.store.alloc_raw(cx.type_, record);

    let seed_native = mint_convention(cx, convention, b"seed-native");
    let container_i64 = mint_convention(cx, convention, b"container-i64");
    let seed_parse = mint_convention(cx, convention, b"seed-parse");

    Callables { callable, convention, seed_native, container_i64, seed_parse }
}

/// Mint a convention identity: `{ty: convention, value -> name string node}`.
fn mint_convention(cx: &mut Cx, convention: DyadPtr, name: &[u8]) -> DyadPtr {
    let text = string::build_text(cx.store, cx.string_, name);
    cx.store.alloc_raw(convention, text.cast())
}

/// Mint a callable leaf: `{ty: callable, value -> [entry, convention]}`. The one
/// licensed mint in the seed — `entry` must be the address of code the given
/// convention can actually jump to (a Rust `RunFn` shim under `seed-native`, a
/// finalized JIT function under `container-i64`).
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

/// Mint a seed-native leaf from its Rust shim — the fn-pointer-to-address cast
/// done once, honestly, at the one place addresses enter the graph.
pub(crate) fn mint_native(
    store: &mut Store,
    callable: DyadPtr,
    entry: RunFn,
    convention: DyadPtr,
) -> DyadPtr {
    mint(store, callable, entry as usize, convention)
}

/// Whether `node` is a callable leaf, read from the graph alone: its type's
/// record kind is [`meta::CALLABLE_TAG`].
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_callable(node: DyadPtr) -> bool {
    let ty = (*node).ty;
    !ty.is_null() && meta::kind_of(ty) == Some(meta::CALLABLE_TAG)
}

/// The `@exec` entry of a callable leaf.
///
/// # Safety
/// `leaf` must be a callable value ([`is_callable`]).
pub(crate) unsafe fn entry_of(leaf: DyadPtr) -> usize {
    std::ptr::read_unaligned((*leaf).value.add(ENTRY_OFF) as *const usize)
}

/// The convention identity of a callable leaf.
///
/// # Safety
/// As [`entry_of`].
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
            assert_eq!((*core.conv_container_i64).ty, core.convention_);
            assert_eq!(
                crate::reflect::text_of((*core.conv_seed_native).value.cast()),
                b"seed-native"
            );
            assert_eq!(
                crate::reflect::text_of((*core.conv_container_i64).value.cast()),
                b"container-i64"
            );
            // The types carry their record kinds, readable from the graph alone.
            assert_eq!(meta::kind_of(core.callable_), Some(meta::CALLABLE_TAG));
            assert_eq!(meta::kind_of(core.convention_), Some(meta::CONVENTION_TAG));
            // A non-callable node is told apart by the same graph read.
            assert!(!is_callable(core.conv_seed_native));
        }
    }
}
