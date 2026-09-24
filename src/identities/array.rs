// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `array`: array-of-`dyad@`, the value `[len: u64][data: @dyad]` (16 bytes).
//! The list rides behind one indirection, so a scope node holds the growable
//! thing as a single field.

use crate::dyad::DyadPtr;
use crate::store::Store;

use super::{meta, Cx};

/// Byte offset of the element count in an array value.
const LEN_OFF: usize = 0;
/// Byte offset of the data pointer (a run of `dyad@`).
const DATA_OFF: usize = 8;

pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::ARRAY_TAG, meta::prec::APPLY);
    cx.store.alloc_raw(cx.type_, record)
}

pub(crate) fn build(store: &mut Store, array_: DyadPtr, items: &[DyadPtr]) -> DyadPtr {
    let data = store.alloc_operands(items);
    let mut bytes = [0u8; 16];
    bytes[LEN_OFF..DATA_OFF].copy_from_slice(&(items.len() as u64).to_ne_bytes());
    bytes[DATA_OFF..].copy_from_slice(&(data as usize).to_ne_bytes());
    let value = store.alloc_bytes(&bytes);
    store.alloc_raw(array_, value)
}

/// Append `item` in place, so whoever holds the node sees the new length. The
/// data run doubles when full; its capacity is implied by the length: none
/// at zero, else the next power of two, at least four.
///
/// # Safety
/// `node` must be an array node that [`build`] made empty and only `push` grew.
pub(crate) unsafe fn push(store: &mut Store, node: DyadPtr, item: DyadPtr) {
    let (len, data) = parts(node);
    let full = len == 0 || (len >= 4 && len.is_power_of_two());
    let data = if full {
        let mut grown = vec![std::ptr::null_mut(); (len + 1).next_power_of_two().max(4)];
        grown[..len].copy_from_slice(items(node));
        let grown = store.alloc_operands(&grown) as *mut DyadPtr;
        std::ptr::write_unaligned((*node).value.add(DATA_OFF) as *mut *mut DyadPtr, grown);
        grown
    } else {
        data as *mut DyadPtr
    };
    *data.add(len) = item;
    std::ptr::write_unaligned((*node).value.add(LEN_OFF) as *mut u64, (len + 1) as u64);
}

/// The `(len, data)` of an array node.
///
/// # Safety
/// `node` must be an array node as [`build`] lays it out.
pub(crate) unsafe fn parts(node: DyadPtr) -> (usize, *const DyadPtr) {
    let v = (*node).value;
    let len = std::ptr::read_unaligned(v.add(LEN_OFF) as *const u64) as usize;
    let data = std::ptr::read_unaligned(v.add(DATA_OFF) as *const *const DyadPtr);
    (len, data)
}

/// # Safety
/// As [`parts`]; the store must outlive the returned slice.
pub(crate) unsafe fn items<'a>(node: DyadPtr) -> &'a [DyadPtr] {
    let (len, data) = parts(node);
    if len == 0 {
        return &[];
    }
    std::slice::from_raw_parts(data, len)
}
