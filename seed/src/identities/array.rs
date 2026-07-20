// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `array`: the seed's first array value form — array-of-`dyad@`, the value
//! `[len: u64][data: @dyad]` (16 bytes). The list rides behind one indirection
//! (settled, July 2026: a scope *is* an array, and the dynamic array must not
//! live inline in the scope node's value), so a sequence node holds a single
//! array-typed field and the growable thing is its own value. No spelling and
//! no element operations yet: the surface `array` type, element access, and
//! growth arrive with mutable arrays; today the parser builds one from a
//! complete expression list.

use crate::dyad::DyadPtr;
use crate::store::Store;

use super::{meta, Cx};

/// Byte offset of the element count in an array value.
const LEN_OFF: usize = 0;
/// Byte offset of the data pointer (a run of `dyad@`).
const DATA_OFF: usize = 8;

/// Register the array-of-dyad type: `{ty: type, value -> record}`, the record
/// [`meta::ARRAY_TAG`]-kinded.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::ARRAY_TAG, f64::NAN, crate::parse::Schedule::Operand);
    cx.store.alloc_raw(cx.type_, record)
}

/// Build an array node over `items`: the elements copy into their own stable
/// allocation and the node's value is the `[len, data]` pair.
pub(crate) fn build(store: &mut Store, array_: DyadPtr, items: &[DyadPtr]) -> DyadPtr {
    let data = store.alloc_operands(items);
    let mut bytes = [0u8; 16];
    bytes[LEN_OFF..DATA_OFF].copy_from_slice(&(items.len() as u64).to_ne_bytes());
    bytes[DATA_OFF..].copy_from_slice(&(data as usize).to_ne_bytes());
    let value = store.alloc_bytes(&bytes);
    store.alloc_raw(array_, value)
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

/// The elements of an array node, as a slice.
///
/// # Safety
/// As [`parts`]; the store must outlive the returned slice.
pub(crate) unsafe fn items<'a>(node: DyadPtr) -> &'a [DyadPtr] {
    let (len, data) = parts(node);
    if len == 0 {
        return &[];
    }
    std::slice::from_raw_parts(data, len)
}
