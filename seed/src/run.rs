//! `run` (V1PLAN Phase 4): execute a node.
//!
//! `run` is one primitive with two paths (see DESIGN ›Execution is function
//! application‹): if the operation has `bcode`, jump to it; if not, walk the
//! `body` directly. Interpretation is that null-`bcode` body-walk, not a separate
//! function. `run` therefore depends on `compile`: the leaves it bottoms out in
//! must already be executable. In this seed the core primitives are hand-built
//! natives installed as their `bcode` (see `crate::core`), so "compiling the
//! leaves" is that installation; `run` jumps to those natives, and each recurses
//! on its operands through [`Runtime::run`].
//!
//! v1 scalar values are `i64` (holding the `i32` smoke test). The null-`bcode`
//! body-walk for compound user functions is the next increment; core primitives
//! all carry `bcode`.

use std::collections::HashMap;

use crate::dyad::DyadPtr;

/// A compiled operation: native machine code the seed provides directly. Takes
/// the application node and returns its scalar result, recursing on operands via
/// the [`Runtime`].
pub type RunFn = fn(&mut Runtime, DyadPtr) -> Result<i64, RunError>;

/// Why a run failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The node's operation has neither `bcode` nor a walkable body. (The
    /// body-walk path is not built yet, so this covers any non-primitive node.)
    NotRunnable(DyadPtr),
    /// A literal's stored bytes could not be read as a value.
    BadValue,
}

/// The `bcode` table: each operation identity to its compiled native. A
/// self-hosted Logos keeps `bcode` on the `fn` node itself; the seed holds the
/// core primitives' natives here.
pub type Bcode = HashMap<DyadPtr, RunFn>;

/// A running evaluation. Holds only the `bcode` table `run` dispatches through;
/// the tree-walk carries operands on the native Rust call stack (each `run` is a
/// frame), so no explicit operand stack is materialized here. DESIGN's stack
/// calling convention — where `run`/`compile` take no arguments and operands ride
/// an explicit stack — is the compiled tier's target, not the interpreter's.
pub struct Runtime<'a> {
    bcode: &'a Bcode,
}

impl<'a> Runtime<'a> {
    /// A runtime dispatching through `bcode`.
    pub fn new(bcode: &'a Bcode) -> Self {
        Runtime { bcode }
    }

    /// Run `node`: read its operation (its `type`) and jump to that operation's
    /// `bcode`. The null-`bcode` case is where the body-walk will go; until then
    /// a node whose operation has no native is [`RunError::NotRunnable`].
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store (address = id). `run`
    /// dereferences it and, transitively, the operands each native reads.
    pub unsafe fn run(&mut self, node: DyadPtr) -> Result<i64, RunError> {
        let op = (*node).ty;
        match self.bcode.get(&op).copied() {
            Some(native) => native(self, node),
            None => Err(RunError::NotRunnable(op)),
        }
    }
}
