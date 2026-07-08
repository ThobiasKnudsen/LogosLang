//! `run` (V1PLAN Phase 4): execute a node.
//!
//! `run` is one primitive with two paths (DESIGN ›Execution is function
//! application‹): read a node's operation (its `type`); if that operation is a
//! *function*, run its `bcode`; otherwise the node is data, read through its
//! type's layout. Interpretation is the body-walk a null-`bcode` function would
//! take.
//!
//! Two homes for machine code, split by leaf vs. compound. The native *leaf*
//! functions (`=`, `+`, `return`) have a null value slot, so their Rust
//! implementations live in a table the runtime holds, keyed by identity; a new run
//! *version* is a new table, not a graph rewrite. A *user* function instead carries
//! its own compiled `bcode` per instance, on the node (the `FN_BCODE` field), null
//! until [`crate::compile::compile_fn`] installs the `exec@`.
//!
//! So `run` resolves a function in three steps: a leaf native in the table runs
//! directly; otherwise, if the node has installed `bcode`, jump to it; otherwise
//! interpret by walking the `body`. Interpretation is just the null-`bcode` path.
//! v1 scalar values are `i32` widened to `i64`.

use std::collections::HashMap;

use crate::dyad::DyadPtr;
use crate::parse::{FN_BCODE, FN_BODY};

/// A function's implementation for one run version. Takes the application node
/// and returns its scalar result, recursing on operands via [`Runtime::run`].
pub type RunFn = fn(&mut Runtime, DyadPtr) -> Result<i64, RunError>;

/// A run version: the implementation of each function identity, keyed by
/// identity. Swapping this table swaps the interpreter.
pub type Bcode = HashMap<DyadPtr, RunFn>;

/// Why a run failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The operation is a function with neither `bcode` nor a `body` to walk.
    NotRunnable(DyadPtr),
    /// A data node had no storage to read.
    BadValue,
}

/// A running evaluation. Holds the `fn` type (to tell a function application from
/// a data read) and the `bcode` table for this run version. Operands ride the
/// Rust call stack (each `run` is a frame); no explicit operand stack.
pub struct Runtime<'a> {
    fn_type: DyadPtr,
    bcode: &'a Bcode,
}

impl<'a> Runtime<'a> {
    /// A runtime recognizing functions by `fn_type` and running them through
    /// `bcode`.
    pub fn new(fn_type: DyadPtr, bcode: &'a Bcode) -> Self {
        Runtime { fn_type, bcode }
    }

    /// Run `node`: read its operation (its `type`). If the operation is a function
    /// (its own type is `fn`): a leaf native in the table runs directly; otherwise
    /// jump to the node's installed `bcode` if present, else walk its `body`. If the
    /// operation is not a function, read the node's scalar value through its layout.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store (address = id). `run`
    /// dereferences it, its operation, and (for functions) the operands or body
    /// they reach. If the operation has installed `bcode`, the compiled artifact
    /// that owns that machine code must still be alive (see
    /// [`crate::compile::compile_fn`]).
    pub unsafe fn run(&mut self, node: DyadPtr) -> Result<i64, RunError> {
        let op = (*node).ty;
        if (*op).ty == self.fn_type {
            // A leaf native (`=`, `+`, `return`) runs from the table directly.
            if let Some(native) = self.bcode.get(&op).copied() {
                return native(self, node);
            }
            // A user function's value is `[input, output, body, bcode]`.
            let fields = (*op).value as *const DyadPtr;
            if fields.is_null() {
                return Err(RunError::NotRunnable(op));
            }
            // Compiled: jump to the installed bcode, a nullary `extern "C" fn() ->
            // i32` (the exec@ punned into the pointer-sized slot).
            let bcode = *fields.add(FN_BCODE);
            if !bcode.is_null() {
                let compiled: extern "C" fn() -> i32 = std::mem::transmute(bcode);
                return Ok(i64::from(compiled()));
            }
            // Interpreted: walk the body.
            let body = *fields.add(FN_BODY);
            if body.is_null() {
                return Err(RunError::NotRunnable(op));
            }
            self.run(body)
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
