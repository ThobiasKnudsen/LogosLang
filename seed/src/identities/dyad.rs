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

/// The high-bit tag marking a place node's `value` as a *frame-relative* byte
/// offset rather than an absolute address. A function-local variable's storage
/// does not exist until the function is called, so its place cannot hold an
/// absolute pointer; its `value` holds the byte offset of its slot inside the
/// current activation record, tagged here. A real heap address never sets bit
/// 63 on the platforms the seed targets (Linux/macOS, x86-64/aarch64, where
/// canonical user-space addresses stay well below 2^47), so the tag is an exact
/// discriminator. Both execution tiers decode it through a single accessor each
/// (`Runtime::place_addr`, `Lowerer::place_addr`), so the tag never escapes
/// those two functions; nothing else reads a place `value` raw.
pub const FRAME_TAG: usize = 1 << 63;

/// Encode a frame-relative place `value` from a byte `offset` into the
/// activation record. See [`FRAME_TAG`].
pub fn frame_place(offset: usize) -> *mut u8 {
    debug_assert!(offset & FRAME_TAG == 0, "a frame offset must leave the tag bit clear");
    std::ptr::without_provenance_mut(FRAME_TAG | offset)
}

/// Decode a place `value`: `Some(offset)` if it is a frame-relative slot,
/// `None` if it is an absolute address (a global/top-level place, or null).
/// See [`FRAME_TAG`].
pub fn frame_offset(value: *mut u8) -> Option<usize> {
    let bits = value as usize;
    (bits & FRAME_TAG != 0).then_some(bits & !FRAME_TAG)
}
