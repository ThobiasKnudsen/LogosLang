// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The node store: an append-only arena of dyads whose addresses never move,
//! since a node's address is its id. Nothing is freed individually.
//! DESIGN ›The store is keyed by address‹.

use crate::binding::Binding;
use crate::dyad::{Dyad, DyadPtr};

/// Each chunk is allocated to exactly this capacity and never grown, so its
/// buffer never reallocates and the addresses into it stay stable.
const CHUNK: usize = 4096;

/// Dyads in fixed-capacity chunks, plus boxed side blobs (operand runs, literal
/// bytes, bindings); every address handed out stays valid for the store's life.
#[derive(Default)]
pub struct Store {
    chunks: Vec<Vec<Dyad>>,
    operands: Vec<Box<[DyadPtr]>>,
    blobs: Vec<Box<[u8]>>,
    /// Boxed: a binding's address is handed out and must survive the vector's growth.
    #[allow(clippy::vec_box)]
    bindings: Vec<Box<Binding>>,
}

impl Store {
    pub fn new() -> Self {
        Store { chunks: Vec::new(), operands: Vec::new(), blobs: Vec::new(), bindings: Vec::new() }
    }

    pub fn alloc(&mut self, dyad: Dyad) -> DyadPtr {
        let need_chunk = match self.chunks.last() {
            Some(c) => c.len() == CHUNK,
            None => true,
        };
        if need_chunk {
            self.chunks.push(Vec::with_capacity(CHUNK));
        }
        let chunk = self.chunks.last_mut().unwrap();
        debug_assert!(chunk.len() < chunk.capacity(), "chunk must not reallocate");
        chunk.push(dyad);
        chunk.last_mut().unwrap() as *mut Dyad
    }

    /// Whether `ptr` is an address this store handed out, on a `Dyad` boundary:
    /// bits that arrive from a run must become a checked error, never a dereference.
    pub fn contains(&self, ptr: DyadPtr) -> bool {
        if ptr.is_null() {
            return false;
        }
        self.chunks.iter().any(|chunk| {
            let start = chunk.as_ptr();
            // SAFETY: `start` and `start + len` bound one allocation.
            let end = unsafe { start.add(chunk.len()) };
            let p = ptr.cast_const();
            p >= start
                && p < end
                && (p as usize - start as usize).is_multiple_of(std::mem::size_of::<Dyad>())
        })
    }

    pub fn alloc_raw(&mut self, ty: DyadPtr, value: *mut u8) -> DyadPtr {
        self.alloc(Dyad { ty, value })
    }

    /// An operand run (`dyad@` fields) as a `void@`. A write pointer: callers
    /// patch it in place, so it is minted from `as_mut_ptr`, never `as_ptr`,
    /// whose read-only provenance would make the later write UB.
    pub fn alloc_operands(&mut self, fields: &[DyadPtr]) -> *mut u8 {
        let mut boxed: Box<[DyadPtr]> = fields.into();
        let ptr = boxed.as_mut_ptr() as *mut u8;
        self.operands.push(boxed);
        ptr
    }

    pub fn alloc_binding(&mut self, rec: Binding) -> *mut Binding {
        let mut boxed = Box::new(rec);
        let ptr: *mut Binding = &mut *boxed;
        self.bindings.push(boxed);
        ptr
    }

    /// A write pointer, minted as `alloc_operands`'s is: an `=` writes through it.
    pub fn alloc_bytes(&mut self, bytes: &[u8]) -> *mut u8 {
        let mut boxed: Box<[u8]> = bytes.into();
        let ptr = boxed.as_mut_ptr();
        self.blobs.push(boxed);
        ptr
    }

    /// Read-only: the pointers derive from a shared borrow.
    pub fn iter(&self) -> impl Iterator<Item = DyadPtr> + '_ {
        self.chunks.iter().flat_map(|c| c.iter().map(|d| d as *const Dyad as DyadPtr))
    }

    pub fn len(&self) -> usize {
        self.chunks.iter().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)] // a test reads the nodes it built a line above
mod tests {
    use super::*;

    /// A sentinel `value` bit-pattern, never dereferenced.
    fn tag(n: usize) -> *mut u8 {
        std::ptr::without_provenance_mut(n)
    }

    #[test]
    fn distinct_allocations_have_distinct_addresses() {
        let mut s = Store::new();
        let a = s.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
        let b = s.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
        assert_ne!(a, b);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn addresses_are_stable_across_chunk_growth() {
        let mut s = Store::new();
        let first = s.alloc_raw(std::ptr::null_mut(), tag(1));
        for i in 0..(CHUNK * 2) {
            s.alloc_raw(std::ptr::null_mut(), tag(i));
        }
        unsafe {
            assert_eq!((*first).value, tag(1));
        }
        assert_eq!(s.len(), CHUNK * 2 + 1);
    }

    #[test]
    fn operands_round_trip_and_stay_stable() {
        let mut s = Store::new();
        let (lhs, rhs) = (tag(1) as DyadPtr, tag(2) as DyadPtr);
        let ops = s.alloc_operands(&[lhs, rhs]);
        for _ in 0..1000 {
            s.alloc_operands(&[tag(9) as DyadPtr]);
        }
        unsafe {
            let p = ops as *const DyadPtr;
            assert_eq!(*p, lhs);
            assert_eq!(*p.add(1), rhs);
        }
    }

    #[test]
    fn literal_bytes_round_trip() {
        let mut s = Store::new();
        let p = s.alloc_bytes(b"123");
        unsafe {
            assert_eq!(std::slice::from_raw_parts(p, 3), b"123");
        }
    }

    #[test]
    fn self_typed_node_round_trips() {
        // The `logos : logos` self-loop the core builds.
        let mut s = Store::new();
        let n = s.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
        unsafe {
            (*n).ty = n;
            assert_eq!((*n).ty, n);
        }
    }
}
