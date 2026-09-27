// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The node store: an append-only arena of dyads whose addresses never move,
//! since a node's address is its id. Nothing is freed individually.
//! DESIGN ›The store is keyed by address‹.

use std::collections::HashMap;

use crate::binding::Binding;
use crate::dyad::{Dyad, DyadPtr};

/// Each chunk is allocated to exactly this capacity and never grown, so its
/// buffer never reallocates and the addresses into it stay stable.
const CHUNK: usize = 4096;

/// Dyads in fixed-capacity chunks, plus boxed side blobs (operand runs, literal
/// bytes, bindings, map tables); every address handed out stays valid for the store's life.
#[derive(Default)]
pub struct Store {
    chunks: Vec<Vec<Dyad>>,
    operands: Vec<Box<[DyadPtr]>>,
    blobs: Vec<Box<[u8]>>,
    /// Boxed: a binding's address is handed out and must survive the vector's growth.
    #[allow(clippy::vec_box)]
    bindings: Vec<Box<Binding>>,
    /// The `hashmap` native's tables, keyed and valued by `i64` bit-containers; boxed
    /// for the same reason as `bindings`.
    #[allow(clippy::vec_box)]
    tables: Vec<Box<HashMap<i64, i64>>>,
}

/// The store's size, counted by hand: malloc's own per-block overhead is left out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreStats {
    pub cells: usize,
    pub cell_bytes: usize,
    /// The inline arena's; zero until it exists.
    pub arena_bytes: usize,
    pub arena_allocs: usize,
    /// Each boxed side blob's payload plus its box's slot in the store's vector.
    pub boxed_bytes: usize,
    pub boxed_allocs: usize,
}

impl StoreStats {
    pub fn total_bytes(&self) -> usize {
        self.cell_bytes + self.arena_bytes + self.boxed_bytes
    }

    /// What grew between two readings; the store is append-only, so nothing shrinks.
    pub fn since(&self, earlier: &StoreStats) -> StoreStats {
        StoreStats {
            cells: self.cells - earlier.cells,
            cell_bytes: self.cell_bytes - earlier.cell_bytes,
            arena_bytes: self.arena_bytes - earlier.arena_bytes,
            arena_allocs: self.arena_allocs - earlier.arena_allocs,
            boxed_bytes: self.boxed_bytes - earlier.boxed_bytes,
            boxed_allocs: self.boxed_allocs - earlier.boxed_allocs,
        }
    }
}

impl std::fmt::Display for StoreStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cells={} cell_bytes={} arena_bytes={} arena_allocs={} boxed_bytes={} boxed_allocs={} total_bytes={}",
            self.cells,
            self.cell_bytes,
            self.arena_bytes,
            self.arena_allocs,
            self.boxed_bytes,
            self.boxed_allocs,
            self.total_bytes()
        )
    }
}

impl Store {
    pub fn new() -> Self {
        Store {
            chunks: Vec::new(),
            operands: Vec::new(),
            blobs: Vec::new(),
            bindings: Vec::new(),
            tables: Vec::new(),
        }
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

    pub fn alloc_table(&mut self) -> *mut HashMap<i64, i64> {
        let mut boxed = Box::default();
        let ptr: *mut HashMap<i64, i64> = &mut *boxed;
        self.tables.push(boxed);
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

    pub fn stats(&self) -> StoreStats {
        use std::mem::size_of;
        let cells = self.len();
        let mut boxed_bytes = 0;
        for run in &self.operands {
            boxed_bytes += size_of::<Box<[DyadPtr]>>() + run.len() * size_of::<DyadPtr>();
        }
        for blob in &self.blobs {
            boxed_bytes += size_of::<Box<[u8]>>() + blob.len();
        }
        boxed_bytes += self.bindings.len() * (size_of::<Box<Binding>>() + size_of::<Binding>());
        for table in &self.tables {
            // hashbrown: a 16-byte entry and a control byte per bucket.
            boxed_bytes += size_of::<Box<HashMap<i64, i64>>>()
                + size_of::<HashMap<i64, i64>>()
                + table.capacity() * (size_of::<(i64, i64)>() + 1);
        }
        StoreStats {
            cells,
            cell_bytes: cells * size_of::<Dyad>(),
            arena_bytes: 0,
            arena_allocs: 0,
            boxed_bytes,
            boxed_allocs: self.operands.len()
                + self.blobs.len()
                + self.bindings.len()
                + self.tables.len(),
        }
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
    fn stats_count_cells_and_boxed_side_blobs() {
        let mut s = Store::new();
        assert_eq!(s.stats(), StoreStats::default());
        s.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
        s.alloc_operands(&[tag(1) as DyadPtr, tag(2) as DyadPtr]);
        s.alloc_bytes(b"abc");
        let st = s.stats();
        assert_eq!((st.cells, st.cell_bytes), (1, 16));
        assert_eq!(st.boxed_allocs, 2);
        // Two fat-pointer slots, a two-word run, three bytes.
        assert_eq!(st.boxed_bytes, 16 + 16 + 16 + 3);
        assert_eq!(st.total_bytes(), 16 + st.boxed_bytes);
        assert_eq!(st.since(&st), StoreStats::default());
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
