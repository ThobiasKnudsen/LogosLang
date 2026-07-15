// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The node cell.
//!
//! A node ("dyad") is a `type` pointer and a `value` pointer — sixteen bytes —
//! and a node's identity is its address (see DESIGN ›A node is a type and a
//! value‹). This is the minimal cell the lexer's trie needs something to point
//! at; Phase 0 fleshes it out (tag bits in the low pointer bits, the
//! `void@`/`exec@`/`dyad@` handle types). Keep additions here deliberate.

/// A node cell: a `type` pointer (`dyad@`) and a `value` pointer (`void@`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Dyad {
    /// The node's type — another node. Grounds out at the `Type : Type` self-loop.
    pub ty: DyadPtr,
    /// The node's value — a type-erased address read through `ty`.
    pub value: *mut u8,
}

/// A handle to a node: its address is its id (`dyad@` in the sketch).
pub type DyadPtr = *mut Dyad;

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

/// Encode a frame-relative place `value` from its owning frame `depth` (1-based,
/// how many function frames are open) and byte `offset`. See [`FRAME_TAG`].
pub fn frame_place(depth: usize, offset: usize) -> *mut u8 {
    debug_assert!(offset <= OFFSET_MASK, "a frame offset must fit in 48 bits");
    debug_assert!(depth >= 1 && depth << DEPTH_SHIFT < FRAME_TAG, "a frame depth must fit in 15 bits");
    std::ptr::without_provenance_mut(FRAME_TAG | (depth << DEPTH_SHIFT) | offset)
}

/// Decode a place `value`: `Some((depth, offset))` for a frame-relative slot,
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
