// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The node store: an append-only arena that hands out stable addresses for
//! dyads. A node's address is its id (DESIGN ›The store is keyed by address‹),
//! so an allocated dyad must never move. The arena is a list of fixed-capacity
//! chunks: allocation bumps within the newest chunk and starts a fresh chunk
//! when it fills, so every address stays valid for the store's lifetime. v1 does
//! not free individual nodes (structural deletion tombstones in the graph, not
//! the store).

use crate::dyad::{Dyad, DyadPtr};

/// Dyads per chunk. Each chunk is a `Vec` allocated to exactly this capacity and
/// never grown past it, so its heap buffer never reallocates and the addresses
/// into it stay stable even as new chunks are appended.
const CHUNK: usize = 4096;

/// An append-only arena of dyads with stable addresses, plus side arenas for the
/// variable-width blobs a node's `value` points at: operand structs (a run of
/// `dyad@` fields) and literal bytes. Each blob is boxed, so its heap address is
/// stable, and the keeper `Vec` holds ownership for the store's lifetime.
#[derive(Default)]
pub struct Store {
    chunks: Vec<Vec<Dyad>>,
    operands: Vec<Box<[DyadPtr]>>,
    blobs: Vec<Box<[u8]>>,
}

impl Store {
    /// A fresh, empty store.
    pub fn new() -> Self {
        Store { chunks: Vec::new(), operands: Vec::new(), blobs: Vec::new() }
    }

    /// Store `dyad` and return its stable address (its id).
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

    /// Store a dyad with the given `ty` and `value` fields and return its address.
    pub fn alloc_raw(&mut self, ty: DyadPtr, value: *mut u8) -> DyadPtr {
        self.alloc(Dyad { ty, value })
    }

    /// Store an operand struct (a run of `dyad@` fields, e.g. a binary op's
    /// `{lhs, rhs}`) and return a `void@` to it. Read back by casting to
    /// `*const DyadPtr` and indexing. The returned pointer is 8-aligned, and is a
    /// *write* pointer: callers patch it in place (e.g. `compile_fn` installs the
    /// `bcode` into a fn value's trailing slot), so it must be derived from a
    /// mutable borrow (`as_mut_ptr`) rather than `as_ptr`, which would carry
    /// read-only provenance and make the later write UB under Stacked/Tree Borrows.
    pub fn alloc_operands(&mut self, fields: &[DyadPtr]) -> *mut u8 {
        let mut boxed: Box<[DyadPtr]> = fields.into();
        let ptr = boxed.as_mut_ptr() as *mut u8;
        self.operands.push(boxed);
        ptr
    }

    /// Store literal bytes (e.g. a numeric literal's digits, or a variable's
    /// storage) and return a `void@` to them. Length is the caller's to track.
    /// The pointer is a *write* pointer (an `=` assignment writes a variable's
    /// storage through it), so it is minted from a mutable borrow — see
    /// [`Self::alloc_operands`] for why `as_mut_ptr` and not `as_ptr`.
    pub fn alloc_bytes(&mut self, bytes: &[u8]) -> *mut u8 {
        let mut boxed: Box<[u8]> = bytes.into();
        let ptr = boxed.as_mut_ptr();
        self.blobs.push(boxed);
        ptr
    }

    /// Total number of dyads stored.
    pub fn len(&self) -> usize {
        self.chunks.iter().map(Vec::len).sum()
    }

    /// True if no dyads have been stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sentinel `value` bit-pattern (never dereferenced), used to prove a
    /// stored dyad's bytes survive later allocations.
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
        // Allocate well past a chunk boundary to force new chunks.
        for i in 0..(CHUNK * 2) {
            s.alloc_raw(std::ptr::null_mut(), tag(i));
        }
        // The first address is still valid and unchanged.
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
        // Churn other allocations to force the keeper Vec to grow.
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
        // A node can be its own type: the `Type : Type` self-loop core.rs builds.
        let mut s = Store::new();
        let n = s.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
        unsafe {
            (*n).ty = n;
            assert_eq!((*n).ty, n);
        }
    }
}
