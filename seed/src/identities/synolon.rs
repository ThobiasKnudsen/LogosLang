// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The node cell.
//!
//! A synolon is a `logos` pointer and a `hyle` pointer — sixteen bytes — and a
//! synolon's identity is its address (see DESIGN ›A synolon is a logos and a
//! hyle‹): form, matter, and the compound of the two. This is the minimal cell
//! the lexer's trie needs something to point at; Phase 0 fleshes it out (tag
//! bits in the low pointer bits, the `void@`/`exec@`/`synolon@` handle logos).
//! Keep additions here deliberate.

/// A node cell: a `logos` pointer (`synolon@`) and a `hyle` pointer (`void@`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Synolon {
    /// The synolon's logos — another synolon. Grounds out at the `logos : logos` self-loop.
    pub logos: SynolonPtr,
    /// The synolon's hyle — a logos-erased address read through `logos`.
    pub hyle: *mut u8,
}

/// A handle to a node: its address is its id (`synolon@` in the sketch).
pub type SynolonPtr = *mut Synolon;

/// The high-bit tag marking a place node's `hyle` as a *frame-relative* slot
/// rather than an absolute address. A function-local variable's storage does not
/// exist until the function is called, so its place cannot hold an absolute
/// pointer; its `hyle` instead encodes its slot in the current activation
/// record. A real heap address never sets bit 63 on the platforms the seed
/// targets (Linux/macOS, x86-64/aarch64, where canonical user-space addresses
/// stay well below 2^47), so the tag is an exact discriminator. Both execution
/// tiers decode it through a single accessor each (`Runtime::place_addr`,
/// `Lowerer::place_addr`), so the tag never escapes those two functions.
///
/// Below the tag the hyle packs two fields: the owning frame **depth** (bits
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

/// Encode a frame-relative place hyle from its owning frame `depth` (1-based,
/// how many function frames are open) and byte `offset`. See [`FRAME_TAG`].
pub fn frame_place(depth: usize, offset: usize) -> *mut u8 {
    debug_assert!(offset <= OFFSET_MASK, "a frame offset must fit in 48 bits");
    debug_assert!(depth >= 1 && depth << DEPTH_SHIFT < FRAME_TAG, "a frame depth must fit in 15 bits");
    std::ptr::without_provenance_mut(FRAME_TAG | (depth << DEPTH_SHIFT) | offset)
}

/// Decode a place hyle: `Some((depth, offset))` for a frame-relative slot,
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
