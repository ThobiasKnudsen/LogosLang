//! The node store: an append-only arena that hands out stable addresses for
//! dyads. A node's address is its id (DESIGN ›The store is keyed by address‹),
//! so an allocated dyad must never move. The arena is a list of fixed-capacity
//! chunks: allocation bumps within the newest chunk and starts a fresh chunk
//! when it fills, so every address stays valid for the store's lifetime. v1 does
//! not free individual nodes (structural deletion tombstones in the graph, not
//! the store; see V1PLAN §Mutability & deopt).

use crate::dyad::{Dyad, DyadPtr};

/// Dyads per chunk. Each chunk is a `Vec` allocated to exactly this capacity and
/// never grown past it, so its heap buffer never reallocates and the addresses
/// into it stay stable even as new chunks are appended.
const CHUNK: usize = 4096;

/// An append-only arena of dyads with stable addresses.
#[derive(Default)]
pub struct Store {
    chunks: Vec<Vec<Dyad>>,
}

impl Store {
    /// A fresh, empty store.
    pub fn new() -> Self {
        Store { chunks: Vec::new() }
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
