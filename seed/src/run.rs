// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `run`: execute a node.
//!
//! `run` is one primitive with no tables (DESIGN ›The callable ground is
//! `@exec`‹; issue #44): read a node's operation (its `type`). A *user*
//! function applies — jump to its installed code or walk its `body`
//! (interpretation is just the null-code path). Everything else consults the
//! node's own *op slot*: the last fixed slot of its type's record holds the
//! [`callable`](crate::identities::callable) leaf its constructor resolved at
//! parse time (`add_i32` for a `+` node, `if_native` for an `if`), and run
//! jumps to that leaf's entry with the node. No HashMap is consulted anywhere;
//! dispatch flows through the graph, and alternative run versions live where
//! versions live — versioned scopes — not in swapped tables. Identities carry
//! only their shared-member *records* (the reflectable precedence/layout data,
//! see [`crate::identities::meta`]), never code; a node with no code to reach
//! is data, read through its type's layout. v1 scalar values ride an `i64`
//! bit-container, read and written at their type's width (see
//! `crate::identities::numtype`).

use std::collections::HashMap;

use crate::dyad::{frame_ref, DyadPtr};
use crate::parse::{fn_frame_size, FN_BCODE, FN_BODY, FN_INPUT, FN_OUTPUT};

/// The signature of a seed-native shim — what a `seed-native` callable's entry
/// points at. Takes the application node and returns its scalar result,
/// recursing on operands via [`Runtime::run`].
pub type RunFn = fn(&mut Runtime, DyadPtr) -> Result<i64, RunError>;

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
    /// supports (v1 calls compiled code with at most three `i64` bit-container
    /// arguments).
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
/// `p` must point at live machine code of exactly `args.len()` `i64` parameters
/// returning `i64` (as [`crate::compile::compile_fn`] produces).
unsafe fn call_compiled(p: *const u8, args: &[i64]) -> Result<i64, RunError> {
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
/// a data read) and the frame stack. Operand computation rides the Rust call
/// stack (each `run` is a frame); the explicit frame stack holds only per-call
/// *parameter bindings*.
pub struct Runtime {
    fn_type: DyadPtr,
    /// `rational_number`: a data leaf of this type is molded to its `i32` value
    /// when read, rather than read raw through the generic i32 layout.
    rational: DyadPtr,
    /// `struct`: an instance of a struct type is not a scalar — its fields are
    /// read through `.` places, never the whole value.
    struct_: DyadPtr,
    /// One activation per in-flight call, each binding the callee's parameter
    /// nodes to their argument values. A parameter reference reads the top frame;
    /// each activation having its own frame is what makes recursion work.
    frames: Vec<HashMap<DyadPtr, i64>>,
    /// One activation *record* per in-flight interpreted call: a byte buffer
    /// holding the call's frame-relative locals at their offsets (sized from the
    /// callee's `FN_FRAME`). A local place reads/writes `base + offset` in the
    /// top record, so each call's locals are private — the interpreter half of
    /// per-call activation records. Each buffer is a `Box<[u8]>`, so its data
    /// pointer is stable even as this `Vec` grows under nested calls, which keeps
    /// a `&local` address valid for the whole call. Empty for a call whose callee
    /// has no locals.
    activations: Vec<Box<[u8]>>,
}

impl Runtime {
    /// A runtime recognizing functions by `fn_type` and struct instances by
    /// `struct_`, molding `rational` leaves on read, with an empty frame stack.
    /// Everything executable is reached through the graph.
    pub fn new(fn_type: DyadPtr, rational: DyadPtr, struct_: DyadPtr) -> Self {
        Runtime { fn_type, rational, struct_, frames: Vec::new(), activations: Vec::new() }
    }

    /// The machine address a place node denotes: an absolute pointer for a
    /// global/top-level place, or `activation_base + offset` for a frame-relative
    /// local of the call in progress (the top activation record). This is the one
    /// place the interpreter decodes the frame tag (see [`crate::dyad::FRAME_TAG`]);
    /// every read, write, and address-of of a local goes through it.
    ///
    /// `None` is a frame-relative place with *no call in progress*: its storage
    /// does not exist. Ordinary execution never sees this (a frame place is only
    /// built inside a function body, which only runs under a call), but parse-time
    /// evaluation does — a `-> type` call whose argument touches an enclosing
    /// function's local runs before any activation exists — and every caller maps
    /// it to a clean [`RunError::BadValue`], which the comptime path reports as
    /// not-comptime-known.
    ///
    /// # Safety
    /// `node` must be a valid place node.
    pub(crate) unsafe fn place_addr(&mut self, node: DyadPtr) -> Option<*mut u8> {
        match frame_ref((*node).value) {
            // Only the offset matters at run time — the local is in the call in
            // progress (the top record); the depth is a parse-time capture guard.
            Some((_, off)) => {
                let base = self.activations.last_mut()?.as_mut_ptr();
                Some(base.add(off))
            }
            None => Some((*node).value),
        }
    }

    /// Run `node`: read its operation (its `type`). If the operation is a
    /// function (its own type is `fn`), apply it — jump to its installed code
    /// or walk its `body`. Otherwise consult the node's op slot: a resolved
    /// application jumps to the callable leaf its constructor stored there;
    /// anything without one is data, read through its type's layout.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store (address = id). `run`
    /// dereferences it, its operation, and (for functions) the operands or body
    /// they reach. If the operation has installed code, the compiled artifact
    /// that owns that machine code must still be alive (see
    /// [`crate::compile::compile_fn`]).
    pub unsafe fn run(&mut self, node: DyadPtr) -> Result<i64, RunError> {
        // A parameter reference resolves to its bound value in the current frame.
        if let Some(&value) = self.frames.last().and_then(|f| f.get(&node)) {
            return Ok(value);
        }
        let op = (*node).ty;
        if (*op).ty == self.fn_type {
            // A user function's value is `[input, output, body, bcode]`.
            let fields = (*op).value as *const DyadPtr;
            if fields.is_null() {
                return Err(RunError::NotRunnable(op));
            }
            // Compiled: evaluate the arguments (in the current frame) and call
            // the installed code — a callable node carrying the finalized entry
            // under the container convention (issue #44).
            let bcode = *fields.add(FN_BCODE);
            if !bcode.is_null() {
                let args = self.eval_args(op, node)?;
                let entry = crate::identities::callable::entry_of(bcode);
                return call_compiled(entry as *const u8, &args);
            }
            // Interpreted: bind the call's arguments to the callee's parameters in
            // a fresh activation frame, allocate the call's activation record (its
            // frame-relative locals, zeroed, sized from `FN_FRAME`), walk the body,
            // then drop both. `bind_frame` evaluates the arguments in the *caller's*
            // frame, before the callee's activation is pushed.
            let frame = self.bind_frame(op, node)?;
            let body = *fields.add(FN_BODY);
            if body.is_null() {
                return Err(RunError::NotRunnable(op));
            }
            self.activations.push(vec![0u8; fn_frame_size(op)].into_boxed_slice());
            self.frames.push(frame);
            let result = self.run(body);
            self.frames.pop();
            self.activations.pop();
            // A `-> void` function runs its body for effect and yields unit (0 bits),
            // matching the compiled void fn's `return 0`, so both tiers agree.
            if crate::identities::numtype::is_void_type(*fields.add(FN_OUTPUT)) {
                result.map(|_| 0)
            } else {
                result
            }
        } else {
            // A fn literal is an inert declaration statement (its work happened at
            // parse) and yields unit, the same precedent as `-> void`. Checked
            // before the op-slot read below: a *compiled* fn literal's fourth slot
            // holds its callable, and evaluating the declaration must not jump to it.
            if op == self.fn_type {
                return Ok(0);
            }
            // A type node standing as a value carries its identity AS its value: its
            // bits are its own address. So a `-> type` function, or an `if` that
            // yields a type, returns the type it produced (roadmap #30), and `x := i32`
            // still binds a name to one. The `Type : Type` root is the store's one
            // self-typed node, so `op == (*op).ty` recognizes every type node (numeric
            // types, the root, `bool`, `void`, pointer types); `op == self.struct_` is
            // a struct *type* node — a struct *instance* has `op ==` its struct type,
            // not `struct_`, and is handled by the lower branch.
            if op == self.struct_ || op == (*op).ty {
                return Ok(node as i64);
            }
            // `node` is data or a migrated application. A rational literal is
            // molded to its i32 value (a fraction like 3.14 has none:
            // UncomputableLiteral, not a bad read).
            if (*node).ty == self.rational {
                return crate::identities::rational::mold(node)
                    .map(i64::from)
                    .ok_or(RunError::UncomputableLiteral);
            }
            // A struct instance is not a scalar (its type's value is a field
            // list, not a record — it must not reach the op-slot read below);
            // its fields are read through `.` places.
            if (*op).ty == self.struct_ {
                return Err(RunError::BadValue);
            }
            // The op slot (issue #44): a migrated node's last fixed slot holds
            // its resolved callable leaf — read the leaf, jump to its entry
            // with the node. Dispatch flows through the node, not a table; the
            // identity carries only its record.
            if let Some(idx) = crate::identities::meta::op_slot_of(op) {
                let slots = (*node).value as *const DyadPtr;
                if !slots.is_null() {
                    let leaf = *slots.add(idx);
                    if !leaf.is_null() && crate::identities::callable::is_callable(leaf) {
                        // SAFETY: a seed-native callable's entry is a `RunFn`
                        // shim address, minted only by the registration loops.
                        let entry = std::mem::transmute::<usize, RunFn>(
                            crate::identities::callable::entry_of(leaf),
                        );
                        return entry(self, node);
                    }
                }
            }
            // Prose is data, invisible to value flow: a comment node forced
            // directly yields unit, off its graph tag (no run entry exists).
            if crate::identities::numtype::is_comment_type((*node).ty) {
                return Ok(0);
            }
            // The text substance (a string node) and unit have no scalar to
            // read; refuse rather than reinterpret their bytes.
            if !crate::identities::numtype::is_scalar_type((*node).ty) {
                return Err(RunError::BadValue);
            }
            let slot = self.place_addr(node).ok_or(RunError::BadValue)?;
            if slot.is_null() {
                return Err(RunError::BadValue);
            }
            // Read the scalar at its type's width into the i64 bit-container.
            Ok(crate::identities::numtype::read_scalar((*node).ty, slot))
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
        let params = (*input).value as *const DyadPtr; // [scope, decl0 …, null]
        let mut frame = HashMap::new();
        for (i, &value) in values.iter().enumerate() {
            // Each entry is the parameter's declare node; the frame binds the
            // declared parameter itself, which is what body references resolve to.
            let param = crate::identities::declare::declared_of(*params.add(i + 1));
            frame.insert(param, value);
        }
        Ok(frame)
    }
}
