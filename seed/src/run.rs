// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `run`: execute a node.
//!
//! `run` is one primitive with no tables (DESIGN ›The callable ground is
//! `@exec`‹; issue #44): read a node's operation (its `logos`). A *user*
//! function applies — jump to its installed code or walk its `body`
//! (interpretation is just the null-code path). Everything else consults the
//! node's own *op slot*: the last fixed slot of its logos's record holds the
//! [`callable`](crate::identities::callable) leaf its constructor resolved at
//! parse time (`add_i32` for a `+` node, `if_native` for an `if`), and run
//! jumps to that leaf's entry with the node. No HashMap is consulted anywhere;
//! dispatch flows through the graph, and alternative run versions live where
//! versions live — versioned scopes — not in swapped tables. Identities carry
//! only their shared-member *records* (the reflectable precedence/layout data,
//! see [`crate::identities::meta`]), never code; a node with no code to reach
//! is data, read through its logos's layout. v1 scalar values ride an `i64`
//! bit-container, read and written at their logos's width (see
//! `crate::identities::numtype`).

use crate::synolon::{frame_ref, SynolonPtr};
use crate::parse::{fn_frame_size, FN_BCODE, FN_BODY, FN_INPUT, FN_OUTPUT};

/// The signature of a seed-native shim — what a `seed-native` callable's entry
/// points at. Takes the application node and returns its scalar result,
/// recursing on operands via [`Runtime::run`].
pub type RunFn = fn(&mut Runtime, SynolonPtr) -> Result<i64, RunError>;

/// Why a run failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The operation is a function with neither `bcode` nor a `body` to walk.
    NotRunnable(SynolonPtr),
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
    /// `f.compile()` ran under a runtime with no compiler attached — parse-time
    /// evaluation (a `-> logos` call's body), where compiling would install code
    /// behind the open pass's back.
    CompilerUnavailable,
    /// `f.compile()` failed: the body does not lower (a construct with no
    /// lowering rule, or more parameters than the compiled convention carries).
    /// Carries the rendered [`crate::compile::CompileError`], behind a thin
    /// box so the error enum keeps its one-word payload — `run` recurses
    /// deeply, and every frame carries a `Result` of this logos.
    CompileFailed(Box<String>),
}

/// Call compiled machine code (a `fn(i64…) -> i64`) with `args`, dispatching on
/// arity, since a raw code pointer must be given a concrete function logos to call.
/// The calling convention is uniform: every argument and the result is the `i64`
/// bit-container (the compiled body reinterprets them to their real logos at the
/// boundary), so this dispatch is independent of the parameter/return logos. The seed
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

/// A running evaluation. Holds the `fn` logos (to tell a function application from
/// a data read) and the activation stack. Operand computation rides the Rust
/// call stack (each `run` is a frame); the explicit [`FrameStack`] holds each
/// in-flight interpreted call's frame — its parameters and locals at their
/// parse-assigned byte offsets.
pub struct Runtime {
    fn_type: SynolonPtr,
    /// `rational_number`: a data leaf of this logos is molded to its `i32` value
    /// when read, rather than read raw through the generic i32 layout.
    rational: SynolonPtr,
    /// `defer`: a scope body expression of this logos is not run in the value
    /// pass — [`crate::identities::scope`] holds it for LIFO execution at scope
    /// exit (issue #49). Held here so the sequence native recognizes it.
    defer_type: SynolonPtr,
    /// Live heap allocations (issue #49): `alloc` increments, `free` decrements.
    /// Not a correctness mechanism — the null-place drop flag prevents double
    /// frees — but an observable one, so tests assert a program frees what it
    /// allocates (net zero) rather than leaking.
    live_allocs: usize,
    /// The per-runtime activation stack the frames live in.
    stack: FrameStack,
    /// The base address of each in-flight interpreted call's frame, innermost
    /// last. A frame-relative place reads `base + offset` in the top entry;
    /// each call having its own frame is what makes recursion work.
    activations: Vec<*mut u8>,
    /// What `f.compile()` needs, attached by [`Runtime::with_compiler`]: the
    /// lowering table and the core handles. Absent (the default, and always at
    /// parse-time evaluation), a compile node fails with
    /// [`RunError::CompilerUnavailable`] instead of compiling behind the open
    /// pass's back.
    compiler: Option<CompilerCx>,
}

/// The compiler context a runtime carries to serve `f.compile()`. The lower
/// table is held as a raw pointer because the runtime and the `Core` that owns
/// the table live side by side in the driver, with no lifetime to name.
struct CompilerCx {
    /// The lowering table (`Core::lower`). Must outlive the runtime.
    lower: *const crate::compile::LowerTable,
    /// The core handles compilation resolves logos against.
    types: crate::parse::CoreTypes,
}

impl Runtime {
    /// A runtime recognizing functions by `fn_type` (record instances are
    /// recognized by their logos's stored layout record), molding `rational`
    /// leaves on read, with an empty activation stack (its first chunk is
    /// claimed lazily, at the first call that needs a frame). Everything
    /// executable is reached through the graph. No compiler is attached; see
    /// [`Runtime::with_compiler`].
    pub fn new(fn_type: SynolonPtr, rational: SynolonPtr) -> Self {
        Runtime {
            fn_type,
            rational,
            defer_type: std::ptr::null_mut(),
            live_allocs: 0,
            stack: FrameStack::new(),
            activations: Vec::new(),
            compiler: None,
        }
    }

    /// Set the `defer` identity, enabling the scope-exit teardown pass (issue
    /// #49). Left null by [`Runtime::new`] — a null never matches a real node's
    /// logos, so a runtime that never sees `defer` needs no wiring; the file
    /// driver and the drop-model tests set it. A builder, like
    /// [`Runtime::with_compiler`].
    pub fn with_defer_type(mut self, defer_type: SynolonPtr) -> Self {
        self.defer_type = defer_type;
        self
    }

    /// `defer`, so the sequence native ([`crate::identities::scope`]) can hold a
    /// `defer` body expression for scope-exit execution instead of running it.
    pub(crate) fn defer_type(&self) -> SynolonPtr {
        self.defer_type
    }

    /// Note a heap allocation (issue #49): `alloc` calls this after allocating.
    pub(crate) fn note_alloc(&mut self) {
        self.live_allocs += 1;
    }

    /// Note a heap free (issue #49): the teardown calls this after freeing a
    /// non-null pointer (an emptied place is a no-op and never reaches here).
    pub(crate) fn note_free(&mut self) {
        self.live_allocs = self.live_allocs.saturating_sub(1);
    }

    /// Live (allocated-not-yet-freed) heap blocks — zero after a program that
    /// frees everything it allocates. Tests read it to catch leaks.
    pub fn live_allocs(&self) -> usize {
        self.live_allocs
    }

    /// Attach the compiler context, enabling `f.compile()` under this runtime.
    /// `lower` is `Core::lower`; the caller keeps the `Core` (and the store)
    /// alive for the runtime's whole life, which the driver's structure already
    /// guarantees — runtime and engine are siblings dropped together.
    pub fn with_compiler(
        mut self,
        lower: &crate::compile::LowerTable,
        types: crate::parse::CoreTypes,
    ) -> Self {
        self.compiler = Some(CompilerCx { lower, types });
        self
    }

    /// `f.compile()`: lower `fn_node`'s body to machine code and install the
    /// finalized entry into `code_leaf` (the leaf the parser pre-minted), so
    /// the next call jumps instead of walking the body. Already-compiled is a
    /// no-op — the code is installed, the call already jumps.
    ///
    /// # Safety
    /// `fn_node` must be a valid synolon and `code_leaf` a callable value, both
    /// from the store; the compiler context's table and logos must be live.
    pub(crate) unsafe fn compile_member(
        &mut self,
        fn_node: SynolonPtr,
        code_leaf: SynolonPtr,
    ) -> Result<(), RunError> {
        let Some(cx) = &self.compiler else {
            return Err(RunError::CompilerUnavailable);
        };
        if (*fn_node).logos != self.fn_type {
            return Err(RunError::BadValue);
        }
        let fields = (*fn_node).hyle as *const SynolonPtr;
        if fields.is_null() {
            return Err(RunError::NotRunnable(fn_node));
        }
        if !(*fields.add(FN_BCODE)).is_null() {
            return Ok(());
        }
        crate::compile::compile_into(&*cx.lower, cx.types, fn_node, code_leaf)
            .map_err(|e| RunError::CompileFailed(Box::new(crate::report::compile_message(&e))))
    }

    /// The machine address a place node denotes: an absolute pointer for a
    /// global/top-level place, or `frame_base + offset` for a frame-relative
    /// parameter or local of the call in progress (the top frame). This is the
    /// one place the interpreter decodes the frame tag (see
    /// [`crate::synolon::FRAME_TAG`]); every read, write, and address-of of a
    /// parameter or local goes through it.
    ///
    /// `None` is a frame-relative place with *no call in progress*: its storage
    /// does not exist. Ordinary execution never sees this (a frame place is only
    /// built inside a function, which only runs under a call), but parse-time
    /// evaluation does — a `-> logos` call whose argument touches an enclosing
    /// function's local runs before any activation exists — and every caller maps
    /// it to a clean [`RunError::BadValue`], which the comptime path reports as
    /// not-comptime-known.
    ///
    /// # Safety
    /// `node` must be a valid place node.
    pub(crate) unsafe fn place_addr(&mut self, node: SynolonPtr) -> Option<*mut u8> {
        match frame_ref((*node).hyle) {
            // Only the offset matters at run time — the place is in the call in
            // progress (the top frame); the depth is a parse-time capture guard.
            Some((_, off)) => {
                let base = *self.activations.last()?;
                Some(base.add(off))
            }
            None => Some((*node).hyle),
        }
    }

    /// Run `node`: read its operation (its `logos`). If the operation is a
    /// function (its own logos is `fn`), apply it — jump to its installed code
    /// or walk its `body`. Otherwise consult the node's op slot: a resolved
    /// application jumps to the callable leaf its constructor stored there;
    /// anything without one is data, read through its logos's layout.
    ///
    /// # Safety
    /// `node` must be a valid synolon from the store (address = id). `run`
    /// dereferences it, its operation, and (for functions) the operands or body
    /// they reach. If the operation has installed code, the compiled artifact
    /// that owns that machine code must still be alive (see
    /// [`crate::compile::compile_fn`]).
    pub unsafe fn run(&mut self, node: SynolonPtr) -> Result<i64, RunError> {
        let op = (*node).logos;
        // A bare parameter (`fn (a)`) has no declared logos; its frame slot holds
        // the full i64 bit-container the call bound. Checked before anything
        // reads through the null logos.
        if op.is_null() {
            return self.read_container(node);
        }
        if (*op).logos == self.fn_type {
            // A user function's value is `[input, output, body, bcode, frame]`.
            let fields = (*op).hyle as *const SynolonPtr;
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
            // A logos node standing as a value carries its identity AS its value: its
            // bits are its own address. So a `-> logos` function, or an `if` that
            // yields a logos, returns the logos it produced (roadmap #30), and `x := i32`
            // still binds a name to one. The `logos : logos` root is the store's one
            // self-classified node, so `op == (*op).logos` recognizes every logos node
            // (numeric logos, the root, `bool`, `void`, pointer and record logos — a
            // record *instance* has `op ==` its record logos, not the root, and is
            // handled by the lower branch). A frame place *classified* by a logos
            // (`t : logos`, a logos-valued parameter) is not a logos standing as a
            // value: its slot holds the bound logos's address, read as the container.
            if op == (*op).logos {
                if frame_ref((*node).hyle).is_some() {
                    return self.read_container(node);
                }
                return Ok(node as i64);
            }
            // `node` is data or a migrated application. A rational literal is
            // molded to its i32 value (a fraction like 3.14 has none:
            // UncomputableLiteral, not a bad read).
            if (*node).logos == self.rational {
                return crate::identities::rational::mold(node)
                    .map(i64::from)
                    .ok_or(RunError::UncomputableLiteral);
            }
            // A record instance is not a scalar; its fields are read through
            // `.` places, never the whole value, so it must not reach the
            // op-slot read below.
            if crate::identities::meta::is_record_type(op) {
                return Err(RunError::BadValue);
            }
            // The op slot (issue #44): a migrated node's last fixed slot holds
            // its resolved callable leaf — read the leaf, jump to its entry
            // with the node. Dispatch flows through the node, not a table; the
            // identity carries only its record.
            if let Some(idx) = crate::identities::meta::op_slot_of(op) {
                let slots = (*node).hyle as *const SynolonPtr;
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
            if crate::identities::numtype::is_comment_type((*node).logos) {
                return Ok(0);
            }
            // The text substance (a string node) and unit have no scalar to
            // read; refuse rather than reinterpret their bytes — except a frame
            // place, a parameter slot of a non-scalar declared logos, which
            // holds the container its call bound.
            if !crate::identities::numtype::is_scalar_type((*node).logos) {
                if frame_ref((*node).hyle).is_some() {
                    return self.read_container(node);
                }
                return Err(RunError::BadValue);
            }
            let slot = self.place_addr(node).ok_or(RunError::BadValue)?;
            if slot.is_null() {
                return Err(RunError::BadValue);
            }
            // Read the scalar at its logos's width into the i64 bit-container.
            Ok(crate::identities::numtype::read_scalar((*node).logos, slot))
        }
    }

    /// Read a frame place's full 8-byte slot as the raw i64 bit-container — how
    /// a parameter of no declared scalar width (a bare `name`, a logos-valued
    /// parameter) is stored and read. Not a frame place, or no call in
    /// progress: [`RunError::BadValue`].
    ///
    /// # Safety
    /// `node` must be a valid synolon from the store; a frame-tagged one must carry
    /// an offset its function's frame size covers.
    unsafe fn read_container(&mut self, node: SynolonPtr) -> Result<i64, RunError> {
        if frame_ref((*node).hyle).is_none() {
            return Err(RunError::BadValue);
        }
        let slot = self.place_addr(node).ok_or(RunError::BadValue)?;
        Ok(std::ptr::read_unaligned(slot as *const i64))
    }

    /// Evaluate a compiled call's arguments, in order, in the *current* frame
    /// (the caller's), checking their count against the callee's parameters. The
    /// parameter and argument arrays are both null-terminated (the input record
    /// is `[scope, param0 …, null]`, the call value `[arg0 …, null]` or null).
    /// Returns the bit-container values and the arity; more than the seed's
    /// three compiled arguments is [`RunError::CompiledArity`].
    ///
    /// # Safety
    /// `fn_node` must be a valid function node and `call_node` a valid application
    /// of it, both from the store.
    unsafe fn eval_args_compiled(
        &mut self,
        fn_node: SynolonPtr,
        call_node: SynolonPtr,
    ) -> Result<([i64; 3], usize), RunError> {
        let input = *((*fn_node).hyle as *const SynolonPtr).add(FN_INPUT);
        let params =
            crate::identities::array::items(crate::identities::meta::record_fields_of(input));
        let args = (*call_node).hyle as *const SynolonPtr; // [arg0 …, null] or null

        let mut values = [0i64; 3];
        let mut i = 0usize;
        loop {
            let param = params.get(i).copied().unwrap_or(std::ptr::null_mut());
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
    /// A scalar-typed parameter stores at its logos's width, exactly as a local
    /// of that logos would; any other (a bare `name`, a logos-valued parameter)
    /// stores the full i64 bit-container. Arity is checked against the callee's
    /// parameters as the walk pairs them.
    ///
    /// # Safety
    /// As [`Runtime::eval_args_compiled`]; `base` must be a frame allocation of
    /// the callee's `FN_FRAME` size, which covers every parameter slot the
    /// parser assigned.
    unsafe fn bind_args(
        &mut self,
        fn_node: SynolonPtr,
        call_node: SynolonPtr,
        base: *mut u8,
    ) -> Result<(), RunError> {
        let input = *((*fn_node).hyle as *const SynolonPtr).add(FN_INPUT);
        let params =
            crate::identities::array::items(crate::identities::meta::record_fields_of(input));
        let args = (*call_node).hyle as *const SynolonPtr; // [arg0 …, null] or null

        let mut i = 0usize;
        loop {
            let param = params.get(i).copied().unwrap_or(std::ptr::null_mut());
            let arg = if args.is_null() { std::ptr::null_mut() } else { *args.add(i) };
            match (param.is_null(), arg.is_null()) {
                (true, true) => break, // both exhausted: counts matched
                (false, false) => {
                    let bits = self.run(arg)?;
                    // A parameter without a parse-assigned slot is a malformed
                    // function node (the parser always assigns one).
                    let Some((_, off)) = frame_ref((*param).hyle) else {
                        return Err(RunError::BadValue);
                    };
                    let slot = base.add(off);
                    let logos = (*param).logos;
                    if crate::identities::numtype::is_scalar_place_type(logos) {
                        crate::identities::numtype::write_scalar(logos, slot, bits);
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
