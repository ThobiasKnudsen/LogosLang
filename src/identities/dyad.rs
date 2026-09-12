// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The node cell, and `dyad` the identity that spells it.
//!
//! A dyad is a `type` pointer and a `value` pointer — sixteen bytes — and a
//! dyad's identity is its address (see DESIGN ›A dyad is a type and a
//! value‹): form, matter, and the compound of the two. This is the minimal cell
//! the lexer's trie needs something to point at. Keep additions here
//! deliberate.
//!
//! `dyad` is also a spelled type (DESIGN ›Substrate vocabulary‹: "The cell is
//! the `dyad`, its two slots the `.type` and the `.value`"; sketch
//! `identities/dyad.logos`: `dyad := type (type := @dyad ?, value := @void ?)`).
//! Its constructor builds a cell: `dyad (type, value)` — the construction a
//! Logos-written constructor uses for the node it places, `tape[0] = dyad
//! (scope, body)` (DESIGN ›Feasibility‹: "`dyad (type, value)` construction
//! from Logos"; #60). The cell is store-owned, never a frame instance: a
//! node the graph keeps. Without a bracket, `dyad` stands as its value, the
//! type, so `@dyad` names a pointer to a cell. The dyad *view* — a value of
//! this type whose value is a cell's address, on which `.type` and `.value`
//! read the cell — is spelled `a:dyad` (›The dyad's read surface‹).

use super::{meta, Cx};

/// A node cell: a `type` pointer (`dyad@`) and a `value` pointer (`void@`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Dyad {
    /// The dyad's type — another dyad. Grounds out at the `logos : logos` self-loop.
    pub ty: DyadPtr,
    /// The dyad's value — a type-erased address read through `type`.
    pub value: *mut u8,
}

/// A handle to a node: its address is its id (`dyad@` in the sketch).
pub type DyadPtr = *mut Dyad;

/// Register `dyad`, the cell type, with its constructor
/// ([`crate::parse::Parser::construct_dyad`]): at application parse_rank, so
/// `dyad (…)` reads the bracket to its right and `dyad` alone is the type.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::DYAD_TAG, meta::prec::APPLY);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("dyad", id);
    cx.metas.insert(id, |p, id, tape| p.construct_dyad(id, tape));
    id
}

/// The high-bit tag marking a place node's `value` as a *frame-relative* slot
/// rather than an absolute address. A function-local variable's storage does not
/// exist until the function is called, so its place cannot hold an absolute
/// pointer; its `value` instead encodes its slot in the current activation
/// record. A real heap address never sets bit 63 on the platforms the seed
/// targets (Linux/macOS, x86-64/aarch64, where canonical user-space addresses
/// stay well below 2^47), so the tag is an exact discriminator. Both execution
/// tiers decode it through a single accessor each (`Runtime::place_addr`,
/// `Lowerer::place_addr`), so the tag never escapes those two functions.
///
/// Below the tag the value packs two fields: the owning frame **depth** (bits
/// `DEPTH_SHIFT`..62, which enclosing function the local belongs to) and the
/// byte **offset** within that frame (bits 0..`DEPTH_SHIFT`). The depth is a
/// lexical, parse-time concept: it lets the parser reject a *capture* — a nested
/// function referencing an outer function's local, which v1 does not support (no
/// closures). At run time only the offset is used, against the current
/// activation; the depth is ignored (the runtime call depth is not the lexical
/// nesting depth).
pub const FRAME_TAG: usize = 1 << 63;

/// The bit position of the frame depth, above the byte offset. 48 bits of offset
/// (frames far larger than any real one) and 15 bits of depth (nesting levels).
const DEPTH_SHIFT: u32 = 48;
const OFFSET_MASK: usize = (1 << DEPTH_SHIFT) - 1;

/// Encode a frame-relative place value from its owning frame `depth` (1-based,
/// how many function frames are open) and byte `offset`. See [`FRAME_TAG`].
pub fn frame_place(depth: usize, offset: usize) -> *mut u8 {
    debug_assert!(offset <= OFFSET_MASK, "a frame offset must fit in 48 bits");
    debug_assert!(
        depth >= 1 && depth << DEPTH_SHIFT < FRAME_TAG,
        "a frame depth must fit in 15 bits"
    );
    std::ptr::without_provenance_mut(FRAME_TAG | (depth << DEPTH_SHIFT) | offset)
}

/// Decode a place value: `Some((depth, offset))` for a frame-relative slot,
/// `None` for an absolute address (a global/top-level place, or null). See
/// [`FRAME_TAG`].
pub fn frame_ref(value: *mut u8) -> Option<(usize, usize)> {
    let bits = value as usize;
    if bits & FRAME_TAG == 0 {
        None
    } else {
        Some(((bits & !FRAME_TAG) >> DEPTH_SHIFT, bits & OFFSET_MASK))
    }
}

/// The tag marking a place node's `value` as *global* storage: an absolute
/// address, but an address of a box rather than of a definition.
///
/// [`FRAME_TAG`] marks the places that live in a call. This marks the rest, so
/// that **every** place carries a mark and a definition carries none. Without
/// it the two are only distinguishable where a frame exists, which is why a
/// type-valued place worked as a parameter and nowhere else: `i32` the
/// identity is `{type: type, value: <its record>}` and a top-level box holding
/// a type is `{type: type, value: <8 bytes>}`, the same shape, both untagged.
/// `type` is the only logos with that problem, being the only one that is its
/// own logos — for every other `T`, an identity says `type` in its type slot
/// and a box says `T`.
///
/// Bit 62, below the frame tag and far above any canonical user-space address
/// (which stays under 2^47 on the platforms the seed targets), so the three
/// cases — frame place, global place, definition — are exactly discriminated.
pub const GLOBAL_TAG: usize = 1 << 62;

/// Mark an absolute address as global *storage*. See [`GLOBAL_TAG`].
pub fn global_place(addr: *mut u8) -> *mut u8 {
    let bits = addr as usize;
    debug_assert!(bits & (FRAME_TAG | GLOBAL_TAG) == 0, "a real address must carry no place tag");
    std::ptr::without_provenance_mut(GLOBAL_TAG | bits)
}

/// Decode a global place: `Some(addr)` with the tag stripped, `None` for
/// anything else (a frame place, a definition, null). See [`GLOBAL_TAG`].
pub fn global_ref(value: *mut u8) -> Option<*mut u8> {
    let bits = value as usize;
    if bits & GLOBAL_TAG == 0 {
        None
    } else {
        Some(std::ptr::with_exposed_provenance_mut(bits & !GLOBAL_TAG))
    }
}

/// Whether `value` is a place at all — a frame slot or global storage — as
/// against a definition's record or null. The one question a reader of an
/// identity has to ask before it reads a record ([`super::meta::kind_of`]).
pub fn is_place(value: *mut u8) -> bool {
    (value as usize) & (FRAME_TAG | GLOBAL_TAG) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_is_marked_and_a_definition_is_not() {
        // The invariant the whole tag scheme exists for: a reader can ask of
        // any `value` whether it is storage or a definition's record, without
        // needing to know whether a frame happens to be open. Before the
        // global tag that question was only answerable inside a function,
        // which is why a type-valued place worked as a parameter and nowhere
        // else (#75): `i32` the identity and a top-level box holding a type
        // are both `{type: type, value: <an address>}`.
        let mut storage = [0u8; 8];
        let real: *mut u8 = storage.as_mut_ptr();
        assert!(!is_place(real), "a plain address is a definition's record");

        let global = global_place(real);
        assert!(is_place(global));
        assert_eq!(global_ref(global), Some(real), "the tag comes off exactly");
        assert!(frame_ref(global).is_none(), "a global place is not a frame place");

        let frame = frame_place(2, 24);
        assert!(is_place(frame));
        assert_eq!(frame_ref(frame), Some((2, 24)));
        assert!(global_ref(frame).is_none(), "a frame place is not a global place");

        assert!(!is_place(std::ptr::null_mut()), "null is no place");
    }
}
