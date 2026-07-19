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

/// The chunk size of the interpreter's activation stack. One chunk carries many
/// ordinary frames; a frame larger than this gets a dedicated chunk of its own
/// size.
const STACK_CHUNK: usize = 64 * 1024;

/// The interpreter's activation stack: one per runtime (per thread), holding
/// every in-flight interpreted call's frame — parameters first, locals after —
/// as a LIFO bump allocation, the software analogue of the machine stack
/// compiled code runs on (DESIGN ›Operands travel on the stack‹). Chunked so
/// that a live frame's address never moves: a frame lives wholly inside one
/// chunk, a chunk never reallocates, and growth adds chunks rather than moving
/// bytes — which is what keeps a `&local` or `&param` address valid for its
/// whole call. Emptied chunks are kept and reused, so steady-state calling
/// allocates nothing.
struct FrameStack {
    chunks: Vec<Box<[u8]>>,
    /// Index of the chunk the cursor is in. Meaningless while `chunks` is empty.
    chunk: usize,
    /// Byte offset of the next free byte in the current chunk.
    cursor: usize,
}

/// A saved stack position: the (chunk, cursor) pair to restore when a call
/// returns. Held on the Rust call stack across the body walk, so the stack
/// needs no side list of frame boundaries.
type StackMark = (usize, usize);

impl FrameStack {
    fn new() -> Self {
        FrameStack { chunks: Vec::new(), chunk: 0, cursor: 0 }
    }

    /// The current position, to be restored with [`FrameStack::release`] when
    /// the frame allocated after it is done.
    fn mark(&self) -> StackMark {
        (self.chunk, self.cursor)
    }

    /// Claim `size` zeroed bytes wholly inside one chunk and return their base.
    /// The base stays valid until the matching [`FrameStack::release`], across
    /// any deeper allocations. A zero-size frame claims nothing and returns a
    /// dangling (never dereferenced) base.
    fn alloc(&mut self, size: usize) -> *mut u8 {
        if size == 0 {
            return std::ptr::NonNull::dangling().as_ptr();
        }
        if self.chunks.is_empty() {
            self.chunks.push(vec![0u8; STACK_CHUNK.max(size)].into_boxed_slice());
            self.chunk = 0;
            self.cursor = 0;
        } else if self.cursor + size > self.chunks[self.chunk].len() {
            // The frame does not fit where the cursor stands: move to the next
            // chunk, reusing a retained one when it is big enough and replacing
            // the tail otherwise (rare: only an oversized frame forces that).
            if self.chunk + 1 >= self.chunks.len() || self.chunks[self.chunk + 1].len() < size {
                self.chunks.truncate(self.chunk + 1);
                self.chunks.push(vec![0u8; STACK_CHUNK.max(size)].into_boxed_slice());
            }
            self.chunk += 1;
            self.cursor = 0;
        }
        let base = unsafe { self.chunks[self.chunk].as_mut_ptr().add(self.cursor) };
        // Chunks are reused after release, so the claim is re-zeroed: a typed
        // declaration with no initializer must read the same zeroed
        // "undefined" the compiled tier's zeroed stack slot gives.
        unsafe { std::ptr::write_bytes(base, 0, size) };
        self.cursor += size;
        base
    }

    /// Pop back to `mark`, releasing every byte claimed after it. The bytes are
    /// dead the moment the call they belonged to returns, exactly like a
    /// machine stack's.
    fn release(&mut self, (chunk, cursor): StackMark) {
        self.chunk = chunk;
        self.cursor = cursor;
    }
}

/// A running evaluation. Holds the `fn` type (to tell a function application from
/// a data read) and the activation stack. Operand computation rides the Rust
/// call stack (each `run` is a frame); the explicit [`FrameStack`] holds each
/// in-flight interpreted call's frame — its parameters and locals at their
/// parse-assigned byte offsets.
pub struct Runtime {
    fn_type: DyadPtr,
    /// `rational_number`: a data leaf of this type is molded to its `i32` value
    /// when read, rather than read raw through the generic i32 layout.
    rational: DyadPtr,
    /// `struct`: an instance of a struct type is not a scalar — its fields are
    /// read through `.` places, never the whole value.
    struct_: DyadPtr,
    /// The per-runtime activation stack the frames live in.
    stack: FrameStack,
    /// The base address of each in-flight interpreted call's frame, innermost
    /// last. A frame-relative place reads `base + offset` in the top entry;
    /// each call having its own frame is what makes recursion work.
    activations: Vec<*mut u8>,
}

impl Runtime {
    /// A runtime recognizing functions by `fn_type` and struct instances by
    /// `struct_`, molding `rational` leaves on read, with an empty activation
    /// stack (its first chunk is claimed lazily, at the first call that needs
    /// a frame). Everything executable is reached through the graph.
    pub fn new(fn_type: DyadPtr, rational: DyadPtr, struct_: DyadPtr) -> Self {
        Runtime { fn_type, rational, struct_, stack: FrameStack::new(), activations: Vec::new() }
    }

    /// The machine address a place node denotes: an absolute pointer for a
    /// global/top-level place, or `frame_base + offset` for a frame-relative
    /// parameter or local of the call in progress (the top frame). This is the
    /// one place the interpreter decodes the frame tag (see
    /// [`crate::dyad::FRAME_TAG`]); every read, write, and address-of of a
    /// parameter or local goes through it.
    ///
    /// `None` is a frame-relative place with *no call in progress*: its storage
    /// does not exist. Ordinary execution never sees this (a frame place is only
    /// built inside a function, which only runs under a call), but parse-time
    /// evaluation does — a `-> type` call whose argument touches an enclosing
    /// function's local runs before any activation exists — and every caller maps
    /// it to a clean [`RunError::BadValue`], which the comptime path reports as
    /// not-comptime-known.
    ///
    /// # Safety
    /// `node` must be a valid place node.
    pub(crate) unsafe fn place_addr(&mut self, node: DyadPtr) -> Option<*mut u8> {
        match frame_ref((*node).value) {
            // Only the offset matters at run time — the place is in the call in
            // progress (the top frame); the depth is a parse-time capture guard.
            Some((_, off)) => {
                let base = *self.activations.last()?;
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
        let op = (*node).ty;
        // A bare parameter (`fn (a)`) has no declared type; its frame slot holds
        // the full i64 bit-container the call bound. Checked before anything
        // reads through the null type.
        if op.is_null() {
            return self.read_container(node);
        }
        if (*op).ty == self.fn_type {
            // A user function's value is `[input, output, body, bcode, frame]`.
            let fields = (*op).value as *const DyadPtr;
            if fields.is_null() {
                return Err(RunError::NotRunnable(op));
            }
            // Compiled: evaluate the arguments (in the current frame) and call
            // the installed code — a callable node carrying the finalized entry
            // under the container convention (issue #44).
            let bcode = *fields.add(FN_BCODE);
            if !bcode.is_null() {
                let (args, arity) = self.eval_args_compiled(op, node)?;
                let entry = crate::identities::callable::entry_of(bcode);
                return call_compiled(entry as *const u8, &args[..arity]);
            }
            let body = *fields.add(FN_BODY);
            if body.is_null() {
                return Err(RunError::NotRunnable(op));
            }
            // Interpreted: claim the callee's zeroed frame from the activation
            // stack, evaluate each argument in the *caller's* frame and write it
            // into the callee's parameter slot — the caller placing the operands
            // on the stack for the callee to read, the ordinary calling
            // convention (DESIGN ›Operands travel on the stack‹) — then make the
            // frame current, walk the body, and pop both again. The stack mark
            // rides this Rust frame, so unwinding on an argument error releases
            // the claim without ever having pushed the activation.
            let mark = self.stack.mark();
            let base = self.stack.alloc(fn_frame_size(op));
            if let Err(e) = self.bind_args(op, node, base) {
                self.stack.release(mark);
                return Err(e);
            }
            self.activations.push(base);
            let result = self.run(body);
            self.activations.pop();
            self.stack.release(mark);
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
            // not `struct_`, and is handled by the lower branch. A frame place
            // *typed* by a type (`t : type`, a type-valued parameter) is not a
            // type standing as a value: its slot holds the bound type's address,
            // read as the container.
            if op == self.struct_ || op == (*op).ty {
                if frame_ref((*node).value).is_some() {
                    return self.read_container(node);
                }
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
            // read; refuse rather than reinterpret their bytes — except a frame
            // place, a parameter slot of a non-scalar declared type, which
            // holds the container its call bound.
            if !crate::identities::numtype::is_scalar_type((*node).ty) {
                if frame_ref((*node).value).is_some() {
                    return self.read_container(node);
                }
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

    /// Read a frame place's full 8-byte slot as the raw i64 bit-container — how
    /// a parameter of no declared scalar width (a bare `name`, a type-valued
    /// parameter) is stored and read. Not a frame place, or no call in
    /// progress: [`RunError::BadValue`].
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store; a frame-tagged one must carry
    /// an offset its function's frame size covers.
    unsafe fn read_container(&mut self, node: DyadPtr) -> Result<i64, RunError> {
        if frame_ref((*node).value).is_none() {
            return Err(RunError::BadValue);
        }
        let slot = self.place_addr(node).ok_or(RunError::BadValue)?;
        Ok(std::ptr::read_unaligned(slot as *const i64))
    }

    /// Evaluate a compiled call's arguments, in order, in the *current* frame
    /// (the caller's), checking their count against the callee's parameters. The
    /// parameter and argument arrays are both null-terminated (the input struct
    /// is `[scope, param0 …, null]`, the call value `[arg0 …, null]` or null).
    /// Returns the bit-container values and the arity; more than the seed's
    /// three compiled arguments is [`RunError::CompiledArity`].
    ///
    /// # Safety
    /// `fn_node` must be a valid function node and `call_node` a valid application
    /// of it, both from the store.
    unsafe fn eval_args_compiled(
        &mut self,
        fn_node: DyadPtr,
        call_node: DyadPtr,
    ) -> Result<([i64; 3], usize), RunError> {
        let input = *((*fn_node).value as *const DyadPtr).add(FN_INPUT);
        let params = (*input).value as *const DyadPtr; // [scope, param0 …, null]
        let args = (*call_node).value as *const DyadPtr; // [arg0 …, null] or null

        let mut values = [0i64; 3];
        let mut i = 0usize;
        loop {
            // Parameters start after the scope at index 0; arguments at index 0.
            let param = if params.is_null() { std::ptr::null_mut() } else { *params.add(i + 1) };
            let arg = if args.is_null() { std::ptr::null_mut() } else { *args.add(i) };
            match (param.is_null(), arg.is_null()) {
                (true, true) => break, // both exhausted: counts matched
                (false, false) => {
                    if i == values.len() {
                        return Err(RunError::CompiledArity);
                    }
                    values[i] = self.run(arg)?;
                    i += 1;
                }
                _ => return Err(RunError::ArityMismatch),
            }
        }
        Ok((values, i))
    }

    /// Evaluate an interpreted call's arguments, in order, in the *current*
    /// frame (the caller's), writing each into the callee's parameter slot in
    /// the fresh frame at `base` — the caller's side of the calling convention.
    /// A scalar-typed parameter stores at its type's width, exactly as a local
    /// of that type would; any other (a bare `name`, a type-valued parameter)
    /// stores the full i64 bit-container. Arity is checked against the callee's
    /// parameters as the walk pairs them.
    ///
    /// # Safety
    /// As [`Runtime::eval_args_compiled`]; `base` must be a frame allocation of
    /// the callee's `FN_FRAME` size, which covers every parameter slot the
    /// parser assigned.
    unsafe fn bind_args(
        &mut self,
        fn_node: DyadPtr,
        call_node: DyadPtr,
        base: *mut u8,
    ) -> Result<(), RunError> {
        let input = *((*fn_node).value as *const DyadPtr).add(FN_INPUT);
        let params = (*input).value as *const DyadPtr; // [scope, param0 …, null]
        let args = (*call_node).value as *const DyadPtr; // [arg0 …, null] or null

        let mut i = 0usize;
        loop {
            let param = if params.is_null() { std::ptr::null_mut() } else { *params.add(i + 1) };
            let arg = if args.is_null() { std::ptr::null_mut() } else { *args.add(i) };
            match (param.is_null(), arg.is_null()) {
                (true, true) => break, // both exhausted: counts matched
                (false, false) => {
                    let bits = self.run(arg)?;
                    // A parameter without a parse-assigned slot is a malformed
                    // function node (the parser always assigns one).
                    let Some((_, off)) = frame_ref((*param).value) else {
                        return Err(RunError::BadValue);
                    };
                    let slot = base.add(off);
                    let ty = (*param).ty;
                    if !ty.is_null() && crate::identities::numtype::is_scalar_type(ty) {
                        crate::identities::numtype::write_scalar(ty, slot, bits);
                    } else {
                        std::ptr::write_unaligned(slot as *mut i64, bits);
                    }
                    i += 1;
                }
                _ => return Err(RunError::ArityMismatch),
            }
        }
        Ok(())
    }
}
