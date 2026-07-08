//! `run` (V1PLAN Phase 4): execute a node.
//!
//! `run` is one primitive with two paths (DESIGN ›Execution is function
//! application‹): read a node's operation (its `type`); if that operation is a
//! *function*, jump to its `bcode`; otherwise the node is data, read through its
//! type's layout. Interpretation is the body-walk a null-`bcode` function would
//! take.
//!
//! The `bcode` lives **on each function node**, not in a side table: a core
//! primitive is a function whose `value` is its native code, so `run` reads it
//! straight off the graph. (The compiler keeps its own primitive->Cranelift
//! table instead, since a lowering rule is the compiler's knowledge, not the
//! primitive's; see `crate::compile`.)
//!
//! v1 scalar values are `i32` widened to `i64`; compound user functions with a
//! walkable `body` are a later increment.

use crate::dyad::DyadPtr;

/// A function's native code (its `bcode`). The seed stores one as a function
/// node's `value`; [`Runtime::run`] transmutes that back and jumps to it.
pub type RunFn = fn(&mut Runtime, DyadPtr) -> Result<i64, RunError>;

/// Why a run failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// A data node had no storage to read.
    BadValue,
}

/// A running evaluation. Carries the `fn` type so `run` can tell a function
/// application (jump to `bcode`) from a data read. Operands ride the Rust call
/// stack (each `run` is a frame); no explicit operand stack is materialized.
pub struct Runtime {
    fn_type: DyadPtr,
}

impl Runtime {
    /// A runtime that recognizes functions by `fn_type` (the type whose values
    /// are functions).
    pub fn new(fn_type: DyadPtr) -> Self {
        Runtime { fn_type }
    }

    /// Run `node`: read its operation (its `type`). If the operation is itself a
    /// function (its own type is `fn`), jump to the `bcode` stored as its
    /// `value`; otherwise read the node's scalar value through its layout.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store (address = id). `run`
    /// dereferences it, its operation, and (for functions) the operands the
    /// `bcode` reads, and transmutes the operation's `value` to native code.
    pub unsafe fn run(&mut self, node: DyadPtr) -> Result<i64, RunError> {
        let op = (*node).ty;
        if (*op).ty == self.fn_type {
            // `op` is a function: its `value` is its bcode; jump to it.
            let bcode = std::mem::transmute::<*mut u8, RunFn>((*op).value);
            bcode(self, node)
        } else {
            // `node` is data: read its scalar (i32 in v1) through its layout.
            let slot = (*node).value as *const i32;
            if slot.is_null() {
                return Err(RunError::BadValue);
            }
            Ok(i64::from(std::ptr::read_unaligned(slot)))
        }
    }
}
