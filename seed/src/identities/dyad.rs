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
