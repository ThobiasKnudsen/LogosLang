// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `run`: execute a node. One primitive with no tables: a user function
//! applies (a jump to installed code, or a walk of its body); everything else
//! jumps to the callable leaf in its op slot. Scalars ride an `i64` bit-container.
//! DESIGN ›The callable ground is `@exec`‹.

use std::cell::Cell;

use crate::dyad::{frame_ref, Dyad, DyadPtr};
use crate::identities::read::{read_kind, Dispatch, Read};
use crate::parse::{fn_frame_size, FN_BCODE, FN_BODY, FN_INPUT, FN_OUTPUT};

/// What a `seed-native` callable's entry points at: takes the application
/// node, returns its scalar result.
pub type RunFn = fn(&mut Runtime<'_>, DyadPtr) -> Result<i64, RunError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// A function with neither `bcode` nor a `body`.
    NotRunnable(DyadPtr),
    /// `f.compile()` on a value that is not a function.
    NotAFunction(DyadPtr),
    /// An operand record with nothing in its op slot (the tape's path markers).
    NoLeaf,
    /// A frame place read with no call in progress.
    NoActivation,
    /// A place declared and never filled.
    Uninitialized,
    /// A record instance, text or hole read as one value.
    NoWholeRead,
    /// A parameter with no frame slot.
    MalformedFn(DyadPtr),
    /// `lex` handed something other than a string.
    NotText,
    /// `print` failed to write stdout; the I/O error's sentence.
    Output(Box<String>),
    /// `error «…»` ran; its quote's text.
    Raised(Box<String>),
    /// A tape affordance with no tape behind its receiver.
    NoTape,
    /// A tape index the tape does not hold.
    OffTape,
    /// `t[k]:name` on a cell holding no named binding.
    NoName,
    /// `insert` of a null fragment.
    NoFragment,
    /// `this` holds no node, or the node has no slots.
    NoThis,
    /// A negative index.
    BadIndex(i64),
    /// An index at or past the end of what it reads.
    PastEnd { index: i64, size: usize },
    /// A negative cell count in `alloc`.
    BadCount(i64),
    /// A pointer whose pointee is neither scalar nor pointer.
    NotDerefable,
    /// A construction of a type with no field layout.
    NoLayout(DyadPtr),
    /// A sequence node with no expression array.
    EmptyScope,
    /// An address handed to `here`'s reads that is no node of the store.
    NotANode(usize),
    /// A node handed to `here`'s reads that is not a scope.
    NotAScope(DyadPtr),
    /// The allocator refused an `alloc`.
    OutOfMemory,
    /// A teardown of a place whose type carries no destructor.
    NoDestructor(DyadPtr),
    /// A literal with no exact `i32` value: a fraction, or out of range.
    UncomputableLiteral,
    /// A call's argument count differs from the callee's parameter count.
    ArityMismatch,
    /// `f.compile()` under a runtime with no compiler: parse-time evaluation.
    CompilerUnavailable,
    /// `f.compile()` failed; the rendered compile error, boxed so every `run`
    /// frame's `Result` stays one word.
    CompileFailed(Box<String>),
    /// The interpreter panicked under compiled code; the panic's message,
    /// carried back across the machine-code boundary as a checked error.
    Faulted(Box<String>),
    /// Calls nested deeper than [`MAX_CALL_DEPTH`].
    CallDepth,
    /// Not an error: `return X` on its way out to the call it leaves, which
    /// takes the value.
    Return(i64),
    /// A read or write through a pointer holding nothing.
    NullPointer,
    /// `lex «…»` under a runtime with no lexer attached; only the parser attaches one.
    NoLexer,
    /// `lex «…»` met text nothing spells; the resolve error's sentence.
    Lex(Box<String>),
    /// `caller.scope` outside a Logos constructor's run.
    NoCaller,
    /// `caller` read as a value; the seed reads `caller.scope` only.
    CallerSpot,
}

thread_local! {
    /// The runtime whose compiled code is running: set around every jump into
    /// machine code so a call back into the interpreter finds it, null outside
    /// one. Saved and restored per jump, so nested jumps each see their own.
    static CURRENT: Cell<*mut Runtime<'static>> = const { Cell::new(std::ptr::null_mut()) };
    /// The checked error an interpreted callee raised under compiled code; an
    /// `extern "C"` function cannot return it, so the runtime reads it back
    /// the moment the machine code returns.
    static PENDING: Cell<Option<RunError>> = const { Cell::new(None) };
}

/// Jump to compiled code of the one signature ([`MachineFn`]).
///
/// # Safety
/// `p` must point at live machine code of that signature.
unsafe fn call_machine(p: *const u8, args: &[i64]) -> i64 {
    let f = std::mem::transmute::<*const u8, MachineFn>(p);
    f(args.as_ptr(), args.len())
}

/// The one signature every compiled function has: the arguments as `i64`
/// bit-containers in a block the caller owns, their count, and the result
/// container; one shape for every arity. DESIGN ›Operands travel on the stack‹.
pub type MachineFn = extern "C" fn(*const i64, usize) -> i64;

/// The jump a compiled caller makes into a callee that is not compiled: the
/// runtime in [`CURRENT`] applies the callee by value. An error or a panic is
/// parked in [`PENDING`] and 0 returned, since neither may cross into machine code.
///
/// # Safety
/// Called only by compiled code the seed emitted: `fn_node` a `fn` node from
/// the store, `argv` holding `argc` containers.
pub unsafe extern "C" fn interpret_call(fn_node: *mut Dyad, argc: usize, argv: *const i64) -> i64 {
    let rt = CURRENT.get();
    if rt.is_null() {
        // Only machine code called with no runtime at all (the test-only
        // `Compiled::call`) can get here; there is nowhere to report to.
        eprintln!("logos: compiled code reached an uncompiled function with no runtime to run it");
        std::process::abort();
    }
    // SAFETY: `rt` was set by the runtime around this very jump and is live for its duration.
    let saved = unsafe { ((*rt).activations.len(), (*rt).stack.mark(), (*rt).constructing) };
    let depth = CALL_DEPTH.with(|d| d.get());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: as above; `argv` holds `argc` containers; `fn_node` is the fn the caller baked.
        unsafe {
            let args = if argc == 0 { &[][..] } else { std::slice::from_raw_parts(argv, argc) };
            (*rt).apply_values(fn_node, args)
        }
    }));
    // The machine code runs on with the 0 and may call back in before the park
    // surfaces, so the interpreter's mid-call state is unwound here.
    let restore = || {
        // SAFETY: as above.
        unsafe {
            (*rt).activations.truncate(saved.0);
            (*rt).stack.release(saved.1);
            (*rt).constructing = saved.2;
        }
        CALL_DEPTH.with(|d| d.set(depth));
    };
    match outcome {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            restore();
            PENDING.set(Some(e));
            0
        }
        Err(panic) => {
            restore();
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown panic".to_string());
            PENDING.set(Some(RunError::Faulted(Box::new(msg))));
            0
        }
    }
}

/// The fault a compiled read or write through a null pointer raises: parked
/// in [`PENDING`] as [`interpret_call`] parks one; the zero is discarded when
/// the runtime reads the park back.
///
/// # Safety
/// Called only by compiled code, on the null arm of a pointer guard.
pub unsafe extern "C" fn park_null_pointer() -> i64 {
    PENDING.set(Some(RunError::NullPointer));
    0
}

/// The fault a compiled prologue raises past [`MAX_CALL_DEPTH`], parked as
/// [`park_null_pointer`]'s is.
///
/// # Safety
/// Called only by compiled code, on the over-limit arm of its prologue.
pub unsafe extern "C" fn park_call_depth() -> i64 {
    PENDING.set(Some(RunError::CallDepth));
    0
}

thread_local! {
    /// The calls in flight across both tiers: the interpreter and each compiled
    /// prologue count in and out, and both refuse the frame past
    /// [`MAX_CALL_DEPTH`]. Per thread, because a run is a thread's.
    pub static CALL_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The address compiled code counts through: the compiling thread's counter,
/// baked at compile time.
pub fn call_depth_ptr() -> *mut usize {
    CALL_DEPTH.with(|d| d.as_ptr())
}

/// How deep interpreted calls may nest before the run faults instead of the
/// Rust stack overflowing. Sized against [`crate::WORK_STACK_BYTES`]: a debug
/// build overflowed near 5,500 frames on 8 MiB, so 10,000 on 64 MiB has room to spare.
pub const MAX_CALL_DEPTH: usize = 10_000;

/// A frame larger than this gets a dedicated chunk of its own size.
const STACK_CHUNK: usize = 64 * 1024;

/// The interpreter's activation stack: a LIFO bump allocation of in-flight
/// frames. Chunked so a live frame's address never moves (a frame lies wholly
/// in one chunk, a chunk never reallocates), which keeps `&local` valid for its call.
struct FrameStack {
    chunks: Vec<Box<[u8]>>,
    /// Index of the chunk the cursor is in. Meaningless while `chunks` is empty.
    chunk: usize,
    /// Byte offset of the next free byte in the current chunk.
    cursor: usize,
}

/// The (chunk, cursor) to restore when a call returns; held on the Rust stack
/// across the body walk.
type StackMark = (usize, usize);

impl FrameStack {
    fn new() -> Self {
        FrameStack { chunks: Vec::new(), chunk: 0, cursor: 0 }
    }

    fn mark(&self) -> StackMark {
        (self.chunk, self.cursor)
    }

    /// Claim `size` zeroed bytes wholly inside one chunk, valid until the
    /// matching release. A zero-size frame returns a dangling, never-read base.
    fn alloc(&mut self, size: usize) -> *mut u8 {
        if size == 0 {
            return std::ptr::NonNull::dangling().as_ptr();
        }
        if self.chunks.is_empty() {
            self.chunks.push(vec![0u8; STACK_CHUNK.max(size)].into_boxed_slice());
            self.chunk = 0;
            self.cursor = 0;
        } else if self.cursor + size > self.chunks[self.chunk].len() {
            // Move to the next chunk, reusing a retained one when it is big enough.
            if self.chunk + 1 >= self.chunks.len() || self.chunks[self.chunk + 1].len() < size {
                self.chunks.truncate(self.chunk + 1);
                self.chunks.push(vec![0u8; STACK_CHUNK.max(size)].into_boxed_slice());
            }
            self.chunk += 1;
            self.cursor = 0;
        }
        // Re-zeroed on reuse: an uninitialized declaration must read the same
        // zero the compiled tier's zeroed slot gives.
        let claim = &mut self.chunks[self.chunk][self.cursor..self.cursor + size];
        claim.fill(0);
        let base = claim.as_mut_ptr();
        self.cursor += size;
        base
    }

    fn release(&mut self, (chunk, cursor): StackMark) {
        self.chunk = chunk;
        self.cursor = cursor;
    }
}

/// A running evaluation. Operand computation rides the Rust call stack; the
/// [`FrameStack`] holds each in-flight interpreted call's frame.
pub struct Runtime<'a> {
    /// The core handles the reading rule reads by.
    types: &'a crate::Core,
    /// `alloc` increments, `free` decrements; tests assert net zero. Not a
    /// correctness mechanism.
    live_allocs: usize,
    stack: FrameStack,
    /// The base address of each in-flight interpreted call's frame, innermost
    /// last; a frame place reads `base + offset` in the top entry.
    activations: Vec<*mut u8>,
    /// Absent at parse-time evaluation, so a compile node fails instead of
    /// installing code behind the open pass's back.
    compiler: Option<&'a crate::compile::LowerTable>,
    /// The parser owns its runtime and reaches the store through it, so the one
    /// `&mut Store` is visible to the borrow checker.
    pub(crate) store: &'a mut crate::store::Store,
    /// Set only inside [`Runtime::lexing`]; absent elsewhere, so `lex` fails
    /// with [`RunError::NoLexer`].
    lexer: Option<Lexer>,
    /// The fragments `lex «…»` built, owned for the runtime's life; boxed because
    /// the handle `lex` yields must stay put as the vector grows.
    #[allow(clippy::vec_box)]
    fragments: Vec<Box<crate::parse::ParsingTape>>,
    /// How many Logos constructors the parser is running through this runtime;
    /// `caller.scope` answers only inside one.
    constructing: u32,
}

/// What `lex «…»` lexes against. Raw, because the parser owns both and the
/// runtime inside it; set only by [`Runtime::lexing`], which clears them when
/// its call returns or unwinds, and every read is inside such a call.
#[derive(Clone, Copy)]
struct Lexer {
    scopes: std::ptr::NonNull<crate::parse::ScopeStack>,
    trie: std::ptr::NonNull<crate::regex_trie::RegexTrie>,
}

/// Clears the lexer when a [`Runtime::lexing`] call ends, by return or by unwinding.
struct Detach<'r, 'a>(&'r mut Runtime<'a>);

impl Drop for Detach<'_, '_> {
    fn drop(&mut self) {
        self.0.lexer = None;
    }
}

impl<'a> Runtime<'a> {
    /// No compiler attached; see [`Runtime::with_compiler`].
    pub fn new(types: &'a crate::Core, store: &'a mut crate::store::Store) -> Self {
        Runtime {
            types,
            live_allocs: 0,
            stack: FrameStack::new(),
            activations: Vec::new(),
            compiler: None,
            store,
            lexer: None,
            fragments: Vec::new(),
            constructing: 0,
        }
    }

    /// Run `f` with the parser's scopes and index attached for exactly this call.
    pub(crate) fn lexing<R>(
        &mut self,
        scopes: &crate::parse::ScopeStack,
        trie: &crate::regex_trie::RegexTrie,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.lexer = Some(Lexer {
            scopes: std::ptr::NonNull::from(scopes),
            trie: std::ptr::NonNull::from(trie),
        });
        let guard = Detach(self);
        f(guard.0)
    }

    /// Until the matching [`Runtime::leave_constructor`], `caller.scope` answers.
    pub(crate) fn enter_constructor(&mut self) {
        self.constructing += 1;
    }

    pub(crate) fn leave_constructor(&mut self) {
        self.constructing -= 1;
    }

    /// The scope open at the pass's position, for `caller.scope` inside a
    /// constructor's run; [`RunError::NoCaller`] elsewhere. DESIGN ›Meta-navigation‹.
    pub(crate) fn pass_scope(&self) -> Result<DyadPtr, RunError> {
        let Some(lexer) = self.lexer else {
            return Err(RunError::NoCaller);
        };
        if self.constructing == 0 {
            return Err(RunError::NoCaller);
        }
        // SAFETY: `lexer` is set only inside `lexing`, whose borrows outlive this call.
        let scopes = unsafe { lexer.scopes.as_ref() };
        Ok(scopes.current().unwrap_or(std::ptr::null_mut()))
    }

    /// Lex `text` against the attached scopes and index into a fresh fragment
    /// owned by this runtime; the handle is the `parsing_tape` value `insert` splices.
    pub(crate) fn lex(&mut self, text: &str) -> Result<*mut crate::parse::ParsingTape, RunError> {
        let Some(lexer) = self.lexer else {
            return Err(RunError::NoLexer);
        };
        // SAFETY: `lexer` is set only inside `lexing`, whose borrows outlive this call.
        let (scopes, trie) = unsafe { (lexer.scopes.as_ref(), lexer.trie.as_ref()) };
        let tape = crate::parse::lex_fragment(scopes, trie, self.store, text)
            .map_err(|e| RunError::Lex(Box::new(crate::report::resolve_message(&e))))?;
        let mut tape = Box::new(tape);
        let handle: *mut crate::parse::ParsingTape = &mut *tape;
        self.fragments.push(tape);
        Ok(handle)
    }

    pub(crate) fn store(&mut self) -> &mut crate::store::Store {
        self.store
    }

    pub(crate) fn note_alloc(&mut self) {
        self.live_allocs += 1;
    }

    /// Called after freeing a non-null pointer; an emptied place never reaches here.
    pub(crate) fn note_free(&mut self) {
        self.live_allocs = self.live_allocs.saturating_sub(1);
    }

    /// Zero after a program that frees everything it allocates.
    pub fn live_allocs(&self) -> usize {
        self.live_allocs
    }

    /// Enable `f.compile()` under this runtime.
    pub fn with_compiler(mut self, lower: &'a crate::compile::LowerTable) -> Self {
        self.compiler = Some(lower);
        self
    }

    pub(crate) fn set_compiler(&mut self, lower: &'a crate::compile::LowerTable) {
        self.compiler = Some(lower);
    }

    /// `f.compile()`: lower `fn_node`'s body and install the entry into
    /// `code_leaf`, the leaf the parser pre-minted. Compiling again lifts a
    /// jump into the interpreter left by a callee that had no code the first time.
    ///
    /// # Safety
    /// `fn_node` must be a valid dyad and `code_leaf` a callable value, both
    /// from the store.
    pub(crate) unsafe fn compile_member(
        &mut self,
        fn_node: DyadPtr,
        code_leaf: DyadPtr,
    ) -> Result<(), RunError> {
        let Some(lower) = self.compiler else {
            return Err(RunError::CompilerUnavailable);
        };
        if (*fn_node).ty != self.types.fn_type {
            return Err(RunError::NotAFunction(fn_node));
        }
        let fields = (*fn_node).value as *const DyadPtr;
        if fields.is_null() {
            return Err(RunError::NotRunnable(fn_node));
        }
        crate::compile::compile_into(lower, self.types, fn_node, code_leaf)
            .map_err(|e| RunError::CompileFailed(Box::new(crate::report::compile_message(&e))))
    }

    /// The core handles, for a native's run to ask the reading rule as `run` does.
    pub(crate) fn types(&self) -> &crate::Core {
        self.types
    }

    /// Whether an interpreted call is in flight: what a `return` leaves.
    pub(crate) fn in_call(&self) -> bool {
        !self.activations.is_empty()
    }

    /// A binding operand yields the dyad it names. DESIGN ›The dyad's read surface‹.
    ///
    /// # Safety
    /// `p` must be null or a valid dyad from the store.
    pub(crate) unsafe fn through(&self, p: DyadPtr) -> DyadPtr {
        crate::binding::through(self.types.binding_, p)
    }

    /// The machine address a place denotes: absolute for a global, `frame base
    /// + offset` in the top frame for a parameter or local. The one place the
    /// interpreter decodes the frame tag. `None`: a frame place with no call in
    /// progress (parse-time evaluation), which callers map to [`RunError::NoActivation`].
    ///
    /// # Safety
    /// `node` must be a valid place node.
    pub(crate) unsafe fn place_addr(&mut self, node: DyadPtr) -> Option<*mut u8> {
        let node = self.through(node);
        match frame_ref((*node).value) {
            // The depth is a parse-time capture guard; only the offset matters here.
            Some((_, off)) => {
                let base = *self.activations.last()?;
                Some(base.add(off))
            }
            // Global storage carries its own tag; an untagged value is a literal's blob.
            None => crate::dyad::global_ref((*node).value).or(Some((*node).value)),
        }
    }

    /// Apply `f` to the call node `node`, `{type: _, value: [args…, null]}`:
    /// a node typed by a function, or by a type whose `code` is `f`, both
    /// arrive here. DESIGN ›Execution is function application‹.
    ///
    /// # Safety
    /// `f` must be a `fn` node from the store and `node` a node whose value
    /// is a null-terminated operand run (or null for a nullary call).
    pub unsafe fn apply(&mut self, f: DyadPtr, node: DyadPtr) -> Result<i64, RunError> {
        let values = self.eval_args(f, node)?;
        self.apply_values(f, &values)
    }

    /// Apply `f` to arguments already in their `i64` containers: jump to the
    /// installed entry, or claim the callee's zeroed frame, write the
    /// containers into its parameter slots, walk the body, and pop both again.
    ///
    /// # Safety
    /// `f` must be a `fn` node from the store; `values` one container per
    /// parameter.
    pub unsafe fn apply_values(&mut self, f: DyadPtr, values: &[i64]) -> Result<i64, RunError> {
        let fields = (*f).value as *const DyadPtr;
        if fields.is_null() {
            return Err(RunError::NotRunnable(f));
        }
        let bcode = *fields.add(FN_BCODE);
        if !bcode.is_null() {
            let entry = crate::identities::callable::entry_of(bcode);
            return self.call_compiled(entry as *const u8, values);
        }
        let body = *fields.add(FN_BODY);
        if body.is_null() {
            return Err(RunError::NotRunnable(f));
        }
        // Past the limit the run faults rather than the Rust stack overflowing.
        if CALL_DEPTH.with(|d| d.get()) >= MAX_CALL_DEPTH {
            return Err(RunError::CallDepth);
        }
        let mark = self.stack.mark();
        let base = self.stack.alloc(fn_frame_size(f));
        if let Err(e) = Self::bind_values(f, base, values) {
            self.stack.release(mark);
            return Err(e);
        }
        CALL_DEPTH.with(|d| d.set(d.get() + 1));
        self.activations.push(base);
        let result = match self.run(body) {
            Err(RunError::Return(v)) => Ok(v),
            other => other,
        };
        self.activations.pop();
        CALL_DEPTH.with(|d| d.set(d.get() - 1));
        self.stack.release(mark);
        // A `-> void` body yields unit, matching the compiled tier's `return 0`.
        if crate::identities::numtype::is_void_type(*fields.add(FN_OUTPUT)) {
            result.map(|_| 0)
        } else {
            result
        }
    }

    /// Jump to machine code with this runtime standing by in [`CURRENT`], and
    /// read back any error parked in [`PENDING`] when it returns (0 is a value).
    ///
    /// # Safety
    /// `entry` must be live machine code of the one compiled signature
    /// ([`MachineFn`]).
    unsafe fn call_compiled(&mut self, entry: *const u8, args: &[i64]) -> Result<i64, RunError> {
        // The lifetime is erased at the machine-code boundary and restored
        // below before the borrow it came from ends.
        let this: *mut Runtime<'static> = (self as *mut Runtime<'a>).cast();
        let prev = CURRENT.replace(this);
        // A park left by machine code that ran with no runtime to report to
        // (the test-only `Compiled::call`) must not surface as this call's.
        PENDING.set(None);
        let r = call_machine(entry, args);
        CURRENT.set(prev);
        match PENDING.take() {
            Some(e) => Err(e),
            None => Ok(r),
        }
    }

    /// Run `node`: ask the reading rule what it is and do that one thing. An
    /// executable node is dispatched; everything else is data read the way its
    /// type says. The compiler asks the same question in the same order.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store. If its operation has
    /// installed code, the artifact owning that machine code must still be alive.
    pub unsafe fn run(&mut self, node: DyadPtr) -> Result<i64, RunError> {
        let node = self.through(node);
        match read_kind(self.types, node) {
            Read::Executable(Dispatch::Call(f)) => self.apply(f, node),
            Read::Executable(Dispatch::Leaf(leaf)) => {
                // SAFETY: a seed-native callable's entry is a `RunFn` shim
                // address, minted only by the registration loops.
                let entry = std::mem::transmute::<usize, RunFn>(
                    crate::identities::callable::entry_of(leaf),
                );
                entry(self, node)
            }
            Read::Executable(Dispatch::None) => Err(RunError::NoLeaf),
            Read::Unit => Ok(0),
            // An identity's value is its own address.
            Read::Identity => Ok(node as i64),
            // A view's value is the viewed node's address.
            Read::Address => Ok((*node).value as i64),
            Read::Container(_) => self.read_container(node),
            Read::Literal => crate::identities::rational::mold(node)
                .map(i64::from)
                .ok_or(RunError::UncomputableLiteral),
            Read::Scalar(_) | Read::Pointer(_) => {
                let slot = self.place_addr(node).ok_or(RunError::NoActivation)?;
                if slot.is_null() {
                    return Err(RunError::Uninitialized);
                }
                Ok(crate::identities::numtype::read_scalar((*node).ty, slot))
            }
            Read::Aggregate | Read::Opaque | Read::Undefined => Err(RunError::NoWholeRead),
        }
    }

    /// A place's full 8-byte slot as the raw container: how a binding of no
    /// declared scalar width is stored, in a frame or at top level alike.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store; a frame-tagged one must carry
    /// an offset its function's frame size covers.
    unsafe fn read_container(&mut self, node: DyadPtr) -> Result<i64, RunError> {
        let node = self.through(node);
        let slot = self.place_addr(node).ok_or(RunError::NoActivation)?;
        Ok(std::ptr::read_unaligned(slot as *const i64))
    }

    /// Evaluate a call's arguments in the caller's frame into their containers;
    /// the parameter and the argument arrays are both null-terminated.
    ///
    /// # Safety
    /// `fn_node` must be a valid function node and `call_node` a valid
    /// application of it, both from the store.
    unsafe fn eval_args(
        &mut self,
        fn_node: DyadPtr,
        call_node: DyadPtr,
    ) -> Result<Vec<i64>, RunError> {
        let input = *((*fn_node).value as *const DyadPtr).add(FN_INPUT);
        let params =
            crate::identities::array::items(crate::identities::meta::record_fields_of(input));
        let args = (*call_node).value as *const DyadPtr; // [arg0 …, null] or null
        let mut values = Vec::with_capacity(params.len());
        let mut i = 0usize;
        loop {
            let param = params.get(i).copied().unwrap_or(std::ptr::null_mut());
            let arg = if args.is_null() { std::ptr::null_mut() } else { *args.add(i) };
            match (param.is_null(), arg.is_null()) {
                (true, true) => break, // both exhausted: counts matched
                (false, false) => {
                    values.push(self.run(arg)?);
                    i += 1;
                }
                _ => return Err(RunError::ArityMismatch),
            }
        }
        Ok(values)
    }

    /// Write `values` into the parameter slots of the fresh frame at `base`: a
    /// scalar parameter at its type's width, any other as the full container.
    ///
    /// # Safety
    /// `fn_node` must be a valid function node; `base` a frame allocation of
    /// its frame size, which covers every parameter slot the parser assigned.
    unsafe fn bind_values(fn_node: DyadPtr, base: *mut u8, values: &[i64]) -> Result<(), RunError> {
        let input = *((*fn_node).value as *const DyadPtr).add(FN_INPUT);
        let params =
            crate::identities::array::items(crate::identities::meta::record_fields_of(input));
        if params.len() != values.len() {
            return Err(RunError::ArityMismatch);
        }
        for (&param, &bits) in params.iter().zip(values) {
            let Some((_, off)) = frame_ref((*param).value) else {
                return Err(RunError::MalformedFn(fn_node));
            };
            let slot = base.add(off);
            let ty = (*param).ty;
            if crate::identities::numtype::is_scalar_type(ty) {
                crate::identities::numtype::write_scalar(ty, slot, bits);
            } else {
                std::ptr::write_unaligned(slot as *mut i64, bits);
            }
        }
        Ok(())
    }
}
