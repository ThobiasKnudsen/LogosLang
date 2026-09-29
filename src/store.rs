// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The node store: one byte arena holding every node as a block, the type word first and
//! the value bytes after it, and what hangs off the nodes: a list that grows, a type's
//! record, the program frame. Addresses never move, since a node's address is its id.
//! Nothing but compiled code is freed individually. DESIGN ›The store is keyed by address‹.

use std::collections::HashMap;

use crate::binding::Binding;
use crate::compile::Artifact;
use crate::dyad::{Dyad, DyadPtr};

/// One span reserved up front and never grown, so a bump never moves what came
/// before, which every in-place writer relies on.
struct Arena {
    span: Vec<u8>,
    /// The span's write pointer, minted once: a read-only borrow could not mint it later.
    base: *mut u8,
    used: usize,
}

const ARENA_CAP: usize = 1 << 30;

impl Arena {
    fn new() -> Self {
        let mut span = Vec::with_capacity(ARENA_CAP);
        let base = span.as_mut_ptr();
        Arena { span, base, used: 0 }
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
        at
    }
}

impl Default for Arena {
    fn default() -> Self {
        Arena::new()
    }
}

/// The nodes and everything that hangs off them in one arena, plus boxed map tables;
/// every address handed out stays valid for the store's life.
#[derive(Default)]
pub struct Store {
    arena: Arena,
    nodes: usize,
    /// The nodes' blocks, type word included.
    node_bytes: usize,
    /// Bumps that are no node: operand lists, records, literal bytes, the program frame.
    hung: usize,
    #[cfg(test)]
    node_list: Vec<DyadPtr>,
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
    pub nodes: usize,
    /// The nodes' blocks: the type word and the value bytes of each.
    pub node_bytes: usize,
    /// What hangs off the nodes.
    pub arena_bytes: usize,
    pub arena_allocs: usize,
    /// Each boxed side blob's payload plus its box's slot in the store's vector.
    pub boxed_bytes: usize,
    pub boxed_allocs: usize,
}

impl StoreStats {
    pub fn total_bytes(&self) -> usize {
        self.node_bytes + self.arena_bytes + self.boxed_bytes
    }

    /// What grew between two readings; the store is append-only, so nothing shrinks.
    pub fn since(&self, earlier: &StoreStats) -> StoreStats {
        StoreStats {
            nodes: self.nodes - earlier.nodes,
            node_bytes: self.node_bytes - earlier.node_bytes,
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
            "nodes={} node_bytes={} arena_bytes={} arena_allocs={} boxed_bytes={} boxed_allocs={} total_bytes={}",
            self.nodes,
            self.node_bytes,
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
            arena: Arena::new(),
            nodes: 0,
            node_bytes: 0,
            hung: 0,
            #[cfg(test)]
            node_list: Vec::new(),
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

    /// One block: the type word, then `payload_len` zeroed value bytes, at least one word
    /// so that a node with nothing to hold still owns the word its value would be. Returns
    /// the node and its value bytes.
    fn node_block(&mut self, ty: DyadPtr, payload_len: usize) -> (DyadPtr, *mut u8) {
        let len = std::mem::size_of::<Dyad>() + payload_len.max(8);
        let at = self.arena.bump(len);
        let node = self.arena_at(at) as DyadPtr;
        // SAFETY: just bumped at `len`, zeroed and 8-aligned; the type word is the block's first.
        unsafe { node.write(Dyad { ty }) };
        self.nodes += 1;
        self.node_bytes += self.arena.used - at;
        #[cfg(test)]
        self.node_list.push(node);
        // SAFETY: the value bytes follow the type word inside the block.
        (node, unsafe { crate::dyad::value(node) })
    }

    /// A node whose value is the operand words `words`, fixed at birth
    /// (DESIGN ›Operands sit inline after the type word; a growing list stays behind a pointer‹).
    pub fn alloc_words(&mut self, ty: DyadPtr, words: &[DyadPtr]) -> DyadPtr {
        let (node, value) = self.node_block(ty, std::mem::size_of_val(words));
        // SAFETY: the block holds `words.len()` words, 8-aligned.
        unsafe {
            std::ptr::copy_nonoverlapping(words.as_ptr(), value as *mut DyadPtr, words.len())
        };
        node
    }

    /// A node whose value is the bytes `bytes`, fixed at birth: a literal, a text.
    pub fn alloc_blob(&mut self, ty: DyadPtr, bytes: &[u8]) -> DyadPtr {
        let (node, value) = self.node_block(ty, bytes.len());
        // SAFETY: the block holds `bytes.len()` bytes.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), value, bytes.len()) };
        node
    }

    /// A `binding` node: its value is the record `rec`, fixed at birth.
    pub fn alloc_binding(&mut self, ty: DyadPtr, rec: Binding) -> DyadPtr {
        debug_assert!(std::mem::align_of::<Binding>() <= 8, "the arena bumps to 8");
        let (node, value) = self.node_block(ty, std::mem::size_of::<Binding>());
        // SAFETY: the block holds one record at its size and alignment; `Binding` has no `Drop`.
        unsafe { (value as *mut Binding).write(rec) };
        node
    }

    /// A node whose value is one address: a type's record, another node, a table. What
    /// grows or is written after the node's birth lives behind this word.
    #[allow(clippy::not_unsafe_ptr_arg_deref)] // `ptr` is stored, never dereferenced
    pub fn alloc_head(&mut self, ty: DyadPtr, ptr: *mut u8) -> DyadPtr {
        let (node, value) = self.node_block(ty, std::mem::size_of::<*mut u8>());
        // SAFETY: the block holds one word.
        unsafe { (value as *mut *mut u8).write(ptr) };
        node
    }

    /// A node with no value: a marker, a bare identity. Its one word is zero.
    pub fn alloc_leaf(&mut self, ty: DyadPtr) -> DyadPtr {
        self.node_block(ty, 0).0
    }

    /// An operand run (`dyad@` fields) as a `void@`, hung off a node whose list grows.
    /// A write pointer: callers patch it in place.
    pub fn alloc_operands(&mut self, fields: &[DyadPtr]) -> *mut u8 {
        let at = self.arena.bump(std::mem::size_of_val(fields));
        self.hung += 1;
        let ptr = self.arena_at(at) as *mut DyadPtr;
        // SAFETY: the run was just bumped, 8-aligned and `fields.len()` words long.
        unsafe { std::ptr::copy_nonoverlapping(fields.as_ptr(), ptr, fields.len()) };
        ptr as *mut u8
    }

    /// Bytes hung off a node: a type's record. A write pointer, as `alloc_operands`'s is.
    pub fn alloc_bytes(&mut self, bytes: &[u8]) -> *mut u8 {
        let at = self.arena.bump(bytes.len());
        self.hung += 1;
        let ptr = self.arena_at(at);
        // SAFETY: just bumped at `bytes.len()`.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
        ptr
    }

    /// `width` zeroed bytes in the arena: the offset a program-frame name is laid out at.
    pub fn arena_alloc(&mut self, width: usize) -> usize {
        self.hung += 1;
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

    /// Every node, in birth order: a test's walk, since a block's size is known only to its type.
    #[cfg(test)]
    pub fn iter(&self) -> impl Iterator<Item = DyadPtr> + '_ {
        self.node_list.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.nodes
    }

    pub fn is_empty(&self) -> bool {
        self.nodes == 0
    }

    pub fn stats(&self) -> StoreStats {
        use std::mem::size_of;
        let mut boxed_bytes = 0;
        for table in &self.tables {
            // hashbrown: a 16-byte entry and a control byte per bucket.
            boxed_bytes += size_of::<Box<HashMap<i64, i64>>>()
                + size_of::<HashMap<i64, i64>>()
                + table.capacity() * (size_of::<(i64, i64)>() + 1);
        }
        StoreStats {
            nodes: self.nodes,
            node_bytes: self.node_bytes,
            arena_bytes: self.arena.used - self.node_bytes,
            arena_allocs: self.hung,
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

    /// A sentinel bit-pattern, never dereferenced.
    fn tag(n: usize) -> *mut u8 {
        std::ptr::without_provenance_mut(n)
    }

    #[test]
    fn distinct_allocations_have_distinct_addresses() {
        let mut s = Store::new();
        let a = s.alloc_leaf(std::ptr::null_mut());
        let b = s.alloc_leaf(std::ptr::null_mut());
        assert_ne!(a, b);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn a_node_s_value_is_inline_after_its_type_word() {
        let mut s = Store::new();
        let (lhs, rhs) = (tag(1) as DyadPtr, tag(2) as DyadPtr);
        let words = s.alloc_words(tag(9) as DyadPtr, &[lhs, rhs]);
        let blob = s.alloc_blob(std::ptr::null_mut(), b"123");
        let head = s.alloc_head(std::ptr::null_mut(), tag(7));
        let leaf = s.alloc_leaf(std::ptr::null_mut());
        unsafe {
            assert_eq!(dyad::ty(words), tag(9) as DyadPtr);
            assert_eq!(dyad::value(words), (words as *mut u8).add(8));
            let p = dyad::value(words) as *const DyadPtr;
            assert_eq!((*p, *p.add(1)), (lhs, rhs));
            assert_eq!(std::slice::from_raw_parts(dyad::value(blob), 3), b"123");
            assert_eq!(dyad::head(head), tag(7));
            assert!(dyad::head(leaf).is_null(), "a leaf owns one zero word");
        }
        let st = s.stats();
        assert_eq!((st.nodes, st.node_bytes), (4, 24 + 16 + 16 + 16));
    }

    #[test]
    fn addresses_are_stable_across_growth() {
        let mut s = Store::new();
        let first = s.alloc_blob(std::ptr::null_mut(), &[1, 2, 3]);
        for i in 0..10_000 {
            s.alloc_head(std::ptr::null_mut(), tag(i));
        }
        unsafe {
            assert_eq!(std::slice::from_raw_parts(dyad::value(first), 3), &[1, 2, 3]);
        }
        assert_eq!(s.len(), 10_001);
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
        let st = s.stats();
        assert_eq!((st.arena_allocs, st.arena_bytes), (1002, 24 + 1000 * 64));
    }

    #[test]
    fn stats_count_nodes_the_rest_of_the_arena_and_boxed_tables() {
        let mut s = Store::new();
        assert_eq!(s.stats(), StoreStats::default());
        s.alloc_leaf(std::ptr::null_mut());
        s.alloc_operands(&[tag(1) as DyadPtr, tag(2) as DyadPtr]);
        s.alloc_bytes(b"abc");
        s.alloc_table();
        let st = s.stats();
        assert_eq!((st.nodes, st.node_bytes), (1, 16));
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
        let n = s.alloc_leaf(std::ptr::null_mut());
        unsafe {
            dyad::set_ty(n, n);
            assert_eq!(dyad::ty(n), n);
        }
    }
}
