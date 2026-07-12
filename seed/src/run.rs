//! `run` (V1PLAN Phase 4): execute a node.
//!
//! `run` is one primitive with two paths (DESIGN ›Execution is function
//! application‹): read a node's operation (its `type`); if that operation is a
//! *function*, run its `bcode`; otherwise the node is data, read through its
//! type's layout. Interpretation is the body-walk a null-`bcode` function would
//! take.
//!
//! Two homes for machine code, split by leaf vs. compound. The native *leaf*
//! functions (`=`, `return`, the concrete `add_i32`, and the abstract `+` that
//! resolves and delegates to it) have a null value slot, so their Rust
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
use crate::parse::{FN_BCODE, FN_BODY, FN_INPUT};

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
    /// A numeric literal has no exact `i32` value to compute — a non-integer
    /// rational (e.g. `3.14`) or an integer outside `i32` range. Reported instead
    /// of computing a wrong value or crashing.
    UncomputableLiteral,
    /// A call's argument count did not match the callee's parameter count.
    ArityMismatch,
    /// A compiled call had more arguments than the seed's calling convention
    /// supports (v1 calls compiled code with at most three `i32` arguments).
    CompiledArity,
}

/// Call compiled machine code (a `fn(i64…) -> i64`) with `args`, dispatching on
/// arity, since a raw code pointer must be given a concrete function type to call.
/// The calling convention is uniform: every argument and the result is the `i64`
/// bit-container (the compiled body reinterprets them to their real types at the
/// boundary), so this dispatch is independent of the parameter/return types. The seed
/// passes at most three arguments; Cranelift's default convention matches `extern "C"`.
///
/// # Safety
/// `bcode` must point at live machine code of exactly `args.len()` `i64` parameters
/// returning `i64` (as [`crate::compile::compile_fn`] produces).
unsafe fn call_compiled(bcode: DyadPtr, args: &[i64]) -> Result<i64, RunError> {
    let p = bcode as *const u8;
    let r = match args {
        [] => (std::mem::transmute::<*const u8, extern "C" fn() -> i64>(p))(),
        [a] => (std::mem::transmute::<*const u8, extern "C" fn(i64) -> i64>(p))(*a),
        [a, b] => (std::mem::transmute::<*const u8, extern "C" fn(i64, i64) -> i64>(p))(*a, *b),
        [a, b, c] => {
            (std::mem::transmute::<*const u8, extern "C" fn(i64, i64, i64) -> i64>(p))(*a, *b, *c)
        }
        _ => return Err(RunError::CompiledArity),
    };
    Ok(r)
}

/// A running evaluation. Holds the `fn` type (to tell a function application from
/// a data read), the `bcode` table for this run version, and the frame stack.
/// Operand computation rides the Rust call stack (each `run` is a frame); the
/// explicit frame stack holds only per-call *parameter bindings*.
pub struct Runtime<'a> {
    fn_type: DyadPtr,
    /// `rational_number`: a data leaf of this type is molded to its `i32` value
    /// when read, rather than read raw through the generic i32 layout.
    rational: DyadPtr,
    bcode: &'a Bcode,
    /// One activation per in-flight call, each binding the callee's parameter
    /// nodes to their argument values. A parameter reference reads the top frame;
    /// each activation having its own frame is what makes recursion work.
    frames: Vec<HashMap<DyadPtr, i64>>,
}

impl<'a> Runtime<'a> {
    /// A runtime recognizing functions by `fn_type` and running them through
    /// `bcode`, molding `rational` leaves on read, with an empty frame stack.
    pub fn new(fn_type: DyadPtr, rational: DyadPtr, bcode: &'a Bcode) -> Self {
        Runtime { fn_type, rational, bcode, frames: Vec::new() }
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
        // A parameter reference resolves to its bound value in the current frame.
        if let Some(&value) = self.frames.last().and_then(|f| f.get(&node)) {
            return Ok(value);
        }
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
            // Compiled: evaluate the arguments (in the current frame) and call the
            // installed bcode with them. The exec@ is punned into the slot.
            let bcode = *fields.add(FN_BCODE);
            if !bcode.is_null() {
                let args = self.eval_args(op, node)?;
                return call_compiled(bcode, &args);
            }
            // Interpreted: bind the call's arguments to the callee's parameters in a
            // fresh activation frame, walk the body, then drop the frame.
            let frame = self.bind_frame(op, node)?;
            let body = *fields.add(FN_BODY);
            if body.is_null() {
                return Err(RunError::NotRunnable(op));
            }
            self.frames.push(frame);
            let result = self.run(body);
            self.frames.pop();
            result
        } else {
            // `node` is data. A rational literal is molded to its i32 value (a
            // fraction like 3.14 has none: UncomputableLiteral, not a bad read);
            // every other v1 scalar is read as i32 through its layout.
            if (*node).ty == self.rational {
                return crate::identities::rational::mold(node)
                    .map(i64::from)
                    .ok_or(RunError::UncomputableLiteral);
            }
            let slot = (*node).value as *const i32;
            if slot.is_null() {
                return Err(RunError::BadValue);
            }
            Ok(i64::from(std::ptr::read_unaligned(slot)))
        }
    }

    /// Evaluate a call's arguments, in order, in the *current* frame (the caller's),
    /// checking their count against the callee's parameters. The parameter and
    /// argument arrays are both null-terminated (the input struct is
    /// `[scope, param0 …, null]`, the call value `[arg0 …, null]` or null).
    ///
    /// # Safety
    /// `fn_node` must be a valid function node and `call_node` a valid application
    /// of it, both from the store.
    unsafe fn eval_args(
        &mut self,
        fn_node: DyadPtr,
        call_node: DyadPtr,
    ) -> Result<Vec<i64>, RunError> {
        let input = *((*fn_node).value as *const DyadPtr).add(FN_INPUT);
        let params = (*input).value as *const DyadPtr; // [scope, param0 …, null]
        let args = (*call_node).value as *const DyadPtr; // [arg0 …, null] or null

        let mut values = Vec::new();
        let mut i = 0usize;
        loop {
            // Parameters start after the scope at index 0; arguments at index 0.
            let param = if params.is_null() { std::ptr::null_mut() } else { *params.add(i + 1) };
            let arg = if args.is_null() { std::ptr::null_mut() } else { *args.add(i) };
            match (param.is_null(), arg.is_null()) {
                (true, true) => break,           // both exhausted: counts matched
                (false, false) => {
                    values.push(self.run(arg)?);
                    i += 1;
                }
                _ => return Err(RunError::ArityMismatch),
            }
        }
        Ok(values)
    }

    /// Bind a call's evaluated arguments to the callee's parameter nodes, returning
    /// the new frame (an activation the interpreter reads parameters from).
    ///
    /// # Safety
    /// As [`Runtime::eval_args`].
    unsafe fn bind_frame(
        &mut self,
        fn_node: DyadPtr,
        call_node: DyadPtr,
    ) -> Result<HashMap<DyadPtr, i64>, RunError> {
        let values = self.eval_args(fn_node, call_node)?;
        let input = *((*fn_node).value as *const DyadPtr).add(FN_INPUT);
        let params = (*input).value as *const DyadPtr; // [scope, param0 …, null]
        let mut frame = HashMap::new();
        for (i, &value) in values.iter().enumerate() {
            frame.insert(*params.add(i + 1), value);
        }
        Ok(frame)
    }
}
