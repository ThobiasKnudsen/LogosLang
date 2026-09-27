// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The node store: an append-only arena of dyads whose addresses never move,
//! since a node's address is its id, and one byte arena for what hangs off them.
//! Nothing but compiled code is freed individually. DESIGN ›The store is keyed by address‹.

use std::collections::HashMap;

use crate::binding::Binding;
use crate::compile::Artifact;
use crate::dyad::{Dyad, DyadPtr};

/// Each chunk is allocated to exactly this capacity and never grown, so its
/// buffer never reallocates and the addresses into it stay stable.
const CHUNK: usize = 4096;

/// One span reserved up front and never grown, so a bump never moves what came
/// before, which every in-place writer relies on.
struct Arena {
    span: Vec<u8>,
    /// The span's write pointer, minted once: a read-only borrow could not mint it later.
    base: *mut u8,
    used: usize,
    allocs: usize,
}

const ARENA_CAP: usize = 1 << 30;

impl Arena {
    fn new() -> Self {
        let mut span = Vec::with_capacity(ARENA_CAP);
        let base = span.as_mut_ptr();
        Arena { span, base, used: 0, allocs: 0 }
    }

    /// `len` zeroed bytes at an 8-aligned offset from the base.
    fn bump(&mut self, len: usize) -> usize {
        let at = self.used;
        let end = at
            .checked_add(len)
            .and_then(|end| end.checked_next_multiple_of(8))
            .filter(|&end| end <= self.span.capacity())
            .unwrap_or_else(|| panic!("the store arena is full ({ARENA_CAP} bytes)"));
        // SAFETY: `at..end` lies inside the reserved capacity, handed out to no one yet.
        unsafe { std::ptr::write_bytes(self.base.add(at), 0, end - at) };
        self.used = end;
        self.allocs += 1;
        at
    }
}

impl Default for Arena {
    fn default() -> Self {
        Arena::new()
    }
}

/// Dyads in fixed-capacity chunks, the byte arena holding operand runs, literal bytes
/// and bindings, plus boxed map tables; every address handed out stays valid for the store's life.
#[derive(Default)]
pub struct Store {
    chunks: Vec<Vec<Dyad>>,
    arena: Arena,
    /// The `hashmap` native's tables, keyed and valued by `i64` bit-containers; boxed
    /// because a table owns heap of its own and its `Drop` must run.
    #[allow(clippy::vec_box)]
    tables: Vec<Box<HashMap<i64, i64>>>,
    /// The compiled code each fn node owns (DESIGN ›The executing primitive has two paths‹).
    artifacts: HashMap<DyadPtr, Artifact>,
    /// Retired while a jump was live; freed when the last jump returns.
    retired: Vec<Artifact>,
    /// Jumps into machine code in flight: a compiled frame is live only under one.
    live_jumps: u32,
}

/// The store's size, counted by hand: malloc's own per-block overhead is left out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreStats {
    pub cells: usize,
    pub cell_bytes: usize,
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
            arena: Arena::new(),
            tables: Vec::new(),
            artifacts: HashMap::new(),
            retired: Vec::new(),
            live_jumps: 0,
        }
    }

    /// `fn_node` now owns `artifact`; the one it owned before is retired.
    pub(crate) fn install_artifact(&mut self, fn_node: DyadPtr, artifact: Artifact) {
        if let Some(old) = self.artifacts.insert(fn_node, artifact) {
            self.retire(old);
        }
    }

    pub(crate) fn retire_artifact(&mut self, fn_node: DyadPtr) {
        if let Some(old) = self.artifacts.remove(&fn_node) {
            self.retire(old);
        }
    }

    /// Freed now, or once the last live jump returns: code is never pulled from
    /// under a running frame.
    fn retire(&mut self, artifact: Artifact) {
        if self.live_jumps == 0 {
            drop(artifact);
        } else {
            self.retired.push(artifact);
        }
    }

    pub(crate) fn enter_jump(&mut self) {
        self.live_jumps += 1;
    }

    pub(crate) fn leave_jump(&mut self) {
        self.live_jumps -= 1;
        if self.live_jumps == 0 {
            self.retired.clear();
        }
    }

    /// Owned, plus retired but not yet freed.
    pub fn live_artifacts(&self) -> usize {
        self.artifacts.len() + self.retired.len()
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
        self.alloc(Dyad::new(ty, value))
    }

    /// An operand run (`dyad@` fields) as a `void@`, in the arena. A write pointer:
    /// callers patch it in place.
    pub fn alloc_operands(&mut self, fields: &[DyadPtr]) -> *mut u8 {
        let at = self.arena.bump(std::mem::size_of_val(fields));
        let ptr = self.arena_at(at) as *mut DyadPtr;
        // SAFETY: the run was just bumped, 8-aligned and `fields.len()` words long.
        unsafe { std::ptr::copy_nonoverlapping(fields.as_ptr(), ptr, fields.len()) };
        ptr as *mut u8
    }

    pub fn alloc_binding(&mut self, rec: Binding) -> *mut Binding {
        debug_assert!(std::mem::align_of::<Binding>() <= 8, "the arena bumps to 8");
        let at = self.arena.bump(std::mem::size_of::<Binding>());
        let ptr = self.arena_at(at) as *mut Binding;
        // SAFETY: just bumped at the record's size and alignment; `Binding` has no `Drop`.
        unsafe { ptr.write(rec) };
        ptr
    }

    /// A write pointer, as `alloc_operands`'s is: an `=` writes through it.
    pub fn alloc_bytes(&mut self, bytes: &[u8]) -> *mut u8 {
        let at = self.arena.bump(bytes.len());
        let ptr = self.arena_at(at);
        // SAFETY: just bumped at `bytes.len()`.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
        ptr
    }

    /// `width` zeroed bytes in the arena: the offset a place carries (`dyad::arena_place`).
    pub fn arena_alloc(&mut self, width: usize) -> usize {
        self.arena.bump(width)
    }

    pub fn arena_base(&self) -> *mut u8 {
        self.arena.base
    }

    /// The address an arena offset denotes, stable for the store's life.
    pub fn arena_at(&self, offset: usize) -> *mut u8 {
        debug_assert!(offset <= self.arena.used, "an arena offset is one the arena handed out");
        // SAFETY: `offset` lies inside the one reserved span.
        unsafe { self.arena.base.add(offset) }
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
        for table in &self.tables {
            // hashbrown: a 16-byte entry and a control byte per bucket.
            boxed_bytes += size_of::<Box<HashMap<i64, i64>>>()
                + size_of::<HashMap<i64, i64>>()
                + table.capacity() * (size_of::<(i64, i64)>() + 1);
        }
        StoreStats {
            cells,
            cell_bytes: cells * size_of::<Dyad>(),
            arena_bytes: self.arena.used,
            arena_allocs: self.arena.allocs,
            boxed_bytes,
            boxed_allocs: self.tables.len(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)] // a test reads the nodes it built a line above
mod tests {
    use super::*;
    use crate::dyad;

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
            assert_eq!(dyad::value(first), tag(1));
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
    fn arena_offsets_are_aligned_zeroed_and_stable() {
        let mut s = Store::new();
        let a = s.arena_alloc(3);
        let b = s.arena_alloc(16);
        assert_eq!((a, b), (0, 8));
        unsafe { *s.arena_at(a) = 7 };
        for _ in 0..1000 {
            s.arena_alloc(64);
        }
        unsafe {
            assert_eq!(*s.arena_at(a), 7);
            assert_eq!(*s.arena_at(b), 0);
        }
        assert!(!s.contains(s.arena_at(b).cast()), "an arena address is no node");
        let st = s.stats();
        assert_eq!((st.arena_allocs, st.arena_bytes), (1002, 24 + 1000 * 64));
    }

    #[test]
    fn stats_count_cells_the_arena_and_boxed_tables() {
        let mut s = Store::new();
        assert_eq!(s.stats(), StoreStats::default());
        s.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
        s.alloc_operands(&[tag(1) as DyadPtr, tag(2) as DyadPtr]);
        s.alloc_bytes(b"abc");
        s.alloc_table();
        let st = s.stats();
        assert_eq!((st.cells, st.cell_bytes), (1, 16));
        // A two-word run, then three bytes rounded to a word.
        assert_eq!((st.arena_allocs, st.arena_bytes), (2, 16 + 8));
        assert_eq!(st.boxed_allocs, 1);
        assert_eq!(st.total_bytes(), 16 + 24 + st.boxed_bytes);
        assert_eq!(st.since(&st), StoreStats::default());
    }

    #[test]
    fn self_typed_node_round_trips() {
        // The `logos : logos` self-loop the core builds.
        let mut s = Store::new();
        let n = s.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
        unsafe {
            dyad::set_ty(n, n);
            assert_eq!(dyad::ty(n), n);
        }
    }
}
