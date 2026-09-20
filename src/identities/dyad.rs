// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The node cell: a `type` pointer and a `value` pointer, its identity its address.
//! `dyad` is also the spelled type whose constructor builds a cell, `dyad (type, value)`;
//! the view `a:dyad` reads a cell's two slots. DESIGN ›A dyad is a type and a value‹.

use super::{meta, Cx};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Dyad {
    pub ty: DyadPtr,
    /// A type-erased address, read through `ty`.
    pub value: *mut u8,
}

/// A node's handle; its address is its identity.
pub type DyadPtr = *mut Dyad;

/// At application rank, so `dyad (…)` reads the bracket to its right and `dyad` alone is the type.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::DYAD_TAG, meta::prec::APPLY);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("dyad", id);
    cx.metas.insert(id, |p, id, tape| p.construct_dyad(id, tape));
    id
}

/// Bit 63 marks a place's `value` as frame-relative: `depth << DEPTH_SHIFT | offset`.
/// A real address never sets bit 63 on the targets (canonical user-space addresses stay
/// under 2^47). The depth is lexical, for the parser's capture check; the runtime uses
/// only the offset. Only `Runtime::place_addr` and `Lowerer::place_addr` decode it.
pub const FRAME_TAG: usize = 1 << 63;

/// 48 bits of offset, 15 bits of depth.
const DEPTH_SHIFT: u32 = 48;
const OFFSET_MASK: usize = (1 << DEPTH_SHIFT) - 1;

/// `depth` is 1-based: how many function frames are open.
pub fn frame_place(depth: usize, offset: usize) -> *mut u8 {
    debug_assert!(offset <= OFFSET_MASK, "a frame offset must fit in 48 bits");
    debug_assert!(
        depth >= 1 && depth << DEPTH_SHIFT < FRAME_TAG,
        "a frame depth must fit in 15 bits"
    );
    std::ptr::without_provenance_mut(FRAME_TAG | (depth << DEPTH_SHIFT) | offset)
}

pub fn frame_ref(value: *mut u8) -> Option<(usize, usize)> {
    let bits = value as usize;
    if bits & FRAME_TAG == 0 {
        None
    } else {
        Some(((bits & !FRAME_TAG) >> DEPTH_SHIFT, bits & OFFSET_MASK))
    }
}

/// Bit 62 marks a place's `value` as global storage, so every place carries a mark and
/// a definition none. Without it a `type` identity and a box holding a type have the
/// same shape, `type` being the one type that is its own type.
pub const GLOBAL_TAG: usize = 1 << 62;

pub fn global_place(addr: *mut u8) -> *mut u8 {
    let bits = addr as usize;
    debug_assert!(bits & (FRAME_TAG | GLOBAL_TAG) == 0, "a real address must carry no place tag");
    std::ptr::without_provenance_mut(GLOBAL_TAG | bits)
}

pub fn global_ref(value: *mut u8) -> Option<*mut u8> {
    let bits = value as usize;
    if bits & GLOBAL_TAG == 0 {
        None
    } else {
        Some(std::ptr::with_exposed_provenance_mut(bits & !GLOBAL_TAG))
    }
}

/// The one question a reader asks before it reads a record.
pub fn is_place(value: *mut u8) -> bool {
    (value as usize) & (FRAME_TAG | GLOBAL_TAG) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_is_marked_and_a_definition_is_not() {
        // A reader can ask of any value whether it is storage or a definition, with no frame open.
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
