// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `is`, membership: `x is c` is the call `c.contains(x)`, a collection being a type with that
//! member (›`x is c` is membership‹ [[R281]]); and the collections identities/collections.logos
//! writes, which the seed reads at its start.

use super::{callable, carried, dyad, gate, meta, numtype, Cx};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, FN_BCODE, FN_BODY};
use crate::regex_trie::RegexTrie;
use crate::run::Context;
use crate::store::Store;
use crate::Core;

/// The member `is` calls on its right side.
pub(crate) const CONTAINS: &str = "contains";

const FILE: carried::File = carried::File {
    path: "identities/collections.logos",
    source: include_str!("../../identities/collections.logos"),
};
const NAMES: [&str; 2] = ["number", "pointer"];

/// `is` at the rank of `==`. Built in Rust, since a Logos parse cannot yet build the call
/// `c.contains(x)`: stand-in for #561.
pub(super) fn register(cx: &mut Cx) {
    let record = meta::token_record(cx, meta::prec::COMPARE, Assoc::Left, &["lhs", "rhs"]);
    let is = cx.store.alloc_head(cx.type_, record);
    cx.declare("is", is);
    cx.metas.insert(is, |p, _id, tape| p.construct_membership(tape));
}

/// Reads identities/collections.logos into `section`, the carried files' own, gives `pointer`'s
/// `contains` its machine-code body, and gives the root the collections.
///
/// # Panics
/// Where the file the seed was built with does not parse or run: a defect of the seed.
pub(super) fn load(store: &mut Store, trie: &mut RegexTrie, core: &Core, section: DyadPtr) {
    carried::read(store, trie, core, section, &FILE);
    let pointer = carried::name_in(trie, section, &FILE, "pointer").identity;
    // SAFETY: the file declared `pointer` as a type with a body, and `contains` a function in it.
    unsafe {
        let contains = carried::member_in(trie, meta::record_body_of(pointer), &FILE, CONTAINS);
        let f = contains.identity;
        assert!(dyad::ty(f) == core.fn_type, "{}: `contains` is no function", FILE.path);
        let slots = dyad::value(f) as *mut DyadPtr;
        let entry = pointer_contains as *const () as usize;
        *slots.add(FN_BCODE) = callable::mint(store, core.callable_, entry, core.conv_container);
        *slots.add(FN_BODY) = std::ptr::null_mut();
    }
    let published: Vec<(String, DyadPtr)> = NAMES
        .iter()
        .map(|&name| (name.to_string(), carried::name_in(trie, section, &FILE, name).binding))
        .collect();
    carried::publish(store, trie, core, &FILE, &published);
}

/// `pointer.contains(t)`: whether `t`, under its gates, is a pointer type. Native, since Logos
/// cannot ask a type what it was made from yet: stand-in for #549.
///
/// # Safety
/// `argv` must hold `argc` words, the last of them the type node the call hands `t`.
unsafe extern "C" fn pointer_contains(_: *mut Context, argv: *const i64, argc: usize) -> i64 {
    let t = *argv.add(argc - 1) as DyadPtr;
    i64::from(numtype::is_pointer_type(gate::under(t)))
}
