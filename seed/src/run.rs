//! `run` (V1PLAN Phase 4): execute a node.
//!
//! `run` is one primitive with two paths (DESIGN ›Execution is function
//! application‹): read a node's operation (its `type`); if that operation is a
//! *function*, run its `bcode`; otherwise the node is data, read through its
//! type's layout. Interpretation is the body-walk a null-`bcode` function would
//! take.
//!
//! The `bcode` for the core functions lives in a table the runtime holds, not
//! baked on each node. Keeping it in `run` (and the lowering in `compile`) means
//! a new run or compile *version* is a new table over the same identities, not a
//! rewrite of the graph. A node marks *that* it is a function (its type is `fn`);
//! the table supplies *which* implementation this run version uses.
//!
//! A function with a null `bcode` (no entry in this version's table) is
//! interpreted by walking its `body` (the node its `value` points at). Primitives
//! carry `bcode`, so only compound functions take that path. v1 scalar values are
//! `i32` widened to `i64`.

use std::collections::HashMap;

use crate::dyad::DyadPtr;
use crate::parse::FN_BODY;

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

    /// Run `node`: read its operation (its `type`). If the operation is a
    /// function (its own type is `fn`), run its `bcode` if this version has one,
    /// else walk its `body`; if the operation is not a function, read the node's
    /// scalar value through its layout.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store (address = id). `run`
    /// dereferences it, its operation, and (for functions) the operands or body
    /// they reach.
    pub unsafe fn run(&mut self, node: DyadPtr) -> Result<i64, RunError> {
        let op = (*node).ty;
        if (*op).ty == self.fn_type {
            // `op` is a function: run its bcode if present, else walk its body.
            match self.bcode.get(&op).copied() {
                Some(bcode) => bcode(self, node),
                None => {
                    // Null bcode: interpret by walking the function's `body`, the
                    // third field of its `[input, output, body]` value struct
                    // (built by `Parser::parse_fn`). A function with no value struct
                    // or a null body is not runnable.
                    let fields = (*op).value as *const DyadPtr;
                    if fields.is_null() {
                        return Err(RunError::NotRunnable(op));
                    }
                    let body = *fields.add(FN_BODY);
                    if body.is_null() {
                        return Err(RunError::NotRunnable(op));
                    }
                    self.run(body)
                }
            }
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
