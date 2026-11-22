// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `run`: execute a node. One primitive with no tables: a user function
//! applies (a jump to installed code, or a walk of its body); everything else
//! jumps to the callable leaf in its op slot. Scalars ride an `i64` bit-container.
//! DESIGN ›The callable ground is `@exec`‹.

use crate::dyad;
use crate::dyad::{Dyad, DyadPtr};
use crate::identities::by_copy;
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
    /// A record instance or hole read as one value.
    NoWholeRead,
    /// `lex` handed something other than a string.
    NotText,
    /// `print` failed to write stdout; the I/O error's sentence.
    Output(Box<String>),
    /// `abort «…»` ran; its quote's text.
    Aborted(Box<String>),
    /// A raise ran where nothing takes its error: the category as it is declared, `error.X`,
    /// and the message.
    Raised(Box<(String, String)>),
    /// A tape affordance with no tape behind its receiver.
    NoTape,
    /// A tape index the tape does not hold.
    OffTape,
    /// A map read at a key it does not hold, where the value type cannot hold `?`.
    MissingKey,
    /// `m[k]` or `m[k] = v` where `m` holds `?`, no map.
    UnknownMap,
    /// `t[k]:name` on a cell holding no named binding.
    NoName,
    /// A tape cell checked to hold a type holds something else by the time it is read.
    CellNotAType,
    /// A cell, line or field of the parse's node, read as a number type, `bool` or a pointer
    /// while the parse runs, holds no constant of it; the type as the message names it.
    NotAConstant(Box<String>),
    /// A cell or line checked against a type a Logos `parse` builds holds no value of it.
    CellNotANode,
    /// `insert` of a null fragment.
    NoFragment,
    /// The receiver holds no node, or the node has no slots.
    NoThis,
    /// A parse read or wrote a field of `tape[0]` while the cell still held the type.
    FieldBeforeStamp,
    /// `tape[0].type = T` with a `T` that gave no type whose values are nodes when the parse ran.
    StampNotANodeType,
    /// A parse read or wrote a field the type it stamped as it ran does not have.
    NoFieldNamed(Box<String>),
    /// A field of a node its constructor never wrote, by its index among the fields.
    UnfilledField(usize),
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
    /// A node handed to `here`'s reads that is not a scope.
    NotAScope(DyadPtr),
    /// The allocator refused an `alloc`.
    OutOfMemory,
    /// A teardown of a place whose type carries no destructor.
    NoDestructor(DyadPtr),
    /// NaN or an infinity converted into an integer type.
    NoWholeNumber,
    /// Arithmetic on `rational_number` values with no value: a division by zero, or past an
    /// `i64` fraction.
    RationalArithmetic,
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
    /// The run nested deeper than its stack holds: calls, or nodes inside one call.
    TooDeep,
    /// Not an error: `return X` on its way out to the call it leaves, which
    /// takes the value.
    Return(i64),
    /// A read or write through a pointer holding nothing.
    NullPointer,
    /// A read of a union's member while the union holds another, named first, or none.
    UnionHolds(Box<(Option<String>, String)>),
    /// A line built over the node a `dyad` place held found one of another type there when it
    /// ran.
    HeldNodeReplaced,
    /// `lex «…»` under a runtime with no lexer attached; only the parser attaches one.
    NoLexer,
    /// `lex «…»` met text nothing spells; the resolve error's sentence.
    Lex(Box<String>),
    /// `caller.scope` outside a Logos constructor's run.
    NoCaller,
    /// `caller` read as a value; the seed reads `caller.scope` only.
    CallerSpot,
    /// `⊆` over two different types that are not both integer types.
    UnsettledInclusion,
    /// The parse a `tape[k]` read ran to lex on to its cell failed.
    Parse(Box<crate::parse::ParseError>),
    /// A `type (…)` held in a body, or a line that waits for the run, ran where no parser is
    /// running the pass.
    NoParser,
    /// A held `type (…)` failed to build at its call; the rendered parse error.
    MintFailed(Box<String>),
    /// A line that waits for the run failed to parse when it ran; the rendered parse error.
    WaitFailed(Box<String>),
    /// A value the pass ran early, with an end of its own, read again after something took it.
    StoodTaken,
    /// A read of the value a line holds for a node, where no line holds one.
    NotKept,
}

/// What compiled code reaches the run through, its first argument. The fields
/// machine code reads sit first, at the offsets the compiler takes with `offset_of!`.
/// DESIGN ›The calling convention is not part of `@exec`‹.
#[repr(C)]
pub struct Context {
    /// The lowest address the run's stack may reach ([`crate::stack::limit`]): the
    /// interpreter and each compiled prologue fault rather than go under it.
    pub stack_limit: usize,
    /// The program frame's base, the store's arena: a top-level or `static` place is an
    /// offset from it.
    pub root: *mut u8,
    /// How many faults were parked in this run, never reset: compiled code leaves the moment
    /// it reads a count above the one it last saw, a fault its cleanup meets included.
    pub faults: usize,
    /// The checked error a step under compiled code raised; an `extern "C"`
    /// function cannot return it, so the runtime reads it back the moment the
    /// machine code returns.
    pending: Option<RunError>,
    /// The runtime this context belongs to, for a step compiled code hands
    /// back to the seed; set at every jump, since a runtime may move between them.
    runtime: *mut Runtime<'static>,
}

impl Context {
    fn new() -> Self {
        Context {
            stack_limit: crate::stack::limit(),
            root: std::ptr::null_mut(),
            faults: 0,
            pending: None,
            runtime: std::ptr::null_mut(),
        }
    }
}

/// The runtime whose machine code is running, for a step it hands back to the seed.
///
/// # Safety
/// `ctx` must be the context a running artifact was called with.
pub(crate) unsafe fn runtime_of(ctx: *mut Context) -> *mut Runtime<'static> {
    (*ctx).runtime
}

/// Jump to compiled code of the one signature ([`MachineFn`]).
///
/// # Safety
/// `p` must point at live machine code of that signature; `ctx` must be the
/// context of the runtime making the jump.
unsafe fn call_machine(p: *const u8, ctx: *mut Context, args: &[i64]) -> i64 {
    let f = std::mem::transmute::<*const u8, MachineFn>(p);
    f(ctx, args.as_ptr(), args.len())
}

/// The one signature every compiled function has: the run's context, the argument
/// block the caller owns, its length in 8-byte words, and the result container; one
/// shape for every arity. DESIGN ›Operands travel on the stack‹.
pub type MachineFn = unsafe extern "C" fn(*mut Context, *const i64, usize) -> i64;

/// The jump a compiled caller makes into a callee that is not compiled: the
/// context's runtime applies the callee by value.
///
/// # Safety
/// Called only by compiled code the seed emitted, with the context it was
/// called with: `fn_node` a `fn` node from the store, `argv` its argument
/// block of `argc` words.
pub unsafe extern "C" fn interpret_call(
    ctx: *mut Context,
    fn_node: *mut Dyad,
    argc: usize,
    argv: *const i64,
) -> i64 {
    // SAFETY: `argv` holds `argc` words; `fn_node` is the fn the caller baked.
    interpret_step(ctx, |rt| unsafe {
        let args = if argc == 0 { &[][..] } else { std::slice::from_raw_parts(argv, argc) };
        rt.apply_values(fn_node, args)
    })
}

/// A step compiled code hands the interpreter. An error or a panic is parked in the context
/// and 0 returned, since neither may cross into machine code.
///
/// # Safety
/// Called only from a function compiled code called, with the context it was called with.
pub(crate) unsafe fn interpret_step(
    ctx: *mut Context,
    step: impl FnOnce(&mut Runtime) -> Result<i64, RunError>,
) -> i64 {
    let rt = runtime_of(ctx);
    // SAFETY: `rt` is the runtime that made this very jump, live for its duration.
    let saved = unsafe { ((*rt).activations.len(), (*rt).stack.mark(), (*rt).constructing) };
    // SAFETY: as above.
    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| step(unsafe { &mut *rt })));
    // The machine code's way out calls back in to end what it holds before the park
    // surfaces, so the interpreter's mid-call state is unwound here.
    let restore = move || {
        // SAFETY: as above.
        unsafe {
            (*rt).activations.truncate(saved.0);
            (*rt).stack.release(saved.1);
            (*rt).constructing = saved.2;
        }
    };
    match outcome {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            restore();
            // SAFETY: `ctx` is the live runtime's context.
            unsafe { park(ctx, e) };
            0
        }
        Err(panic) => {
            restore();
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown panic".to_string());
            // SAFETY: as above.
            unsafe { park(ctx, RunError::Faulted(Box::new(msg))) };
            0
        }
    }
}

/// The fault a compiled read or write through a null pointer raises: parked
/// in the context as [`interpret_call`] parks one; the code leaves before the zero is read.
///
/// # Safety
/// Called only by compiled code, on the null arm of a pointer guard, with the
/// context it was called with.
pub unsafe extern "C" fn park_null_pointer(ctx: *mut Context) -> i64 {
    park(ctx, RunError::NullPointer);
    0
}

/// A float converted into an integer type in compiled code, by the interpreter's own
/// [`apply_cast`](crate::identities::numtype::apply_cast); NaN or an infinity is parked as the
/// fault the interpreter raises.
///
/// # Safety
/// Called only by compiled code, with the context it was called with; `from` and `to` are
/// number type tags.
pub unsafe extern "C" fn convert_float_to_int(
    ctx: *mut Context,
    v: i64,
    from: i64,
    to: i64,
) -> i64 {
    use crate::identities::numtype::{apply_cast, NumType};
    match apply_cast(NumType::from_tag(from as u8), NumType::from_tag(to as u8), v) {
        Some(bits) => bits,
        None => {
            park(ctx, RunError::NoWholeNumber);
            0
        }
    }
}

/// Every fault is counted, so the machine code that reads the count leaves; the first one
/// parked stays the one shown, a later one being met by that code's cleanup.
///
/// # Safety
/// `ctx` must be the context of the runtime whose machine code is running.
unsafe fn park(ctx: *mut Context, e: RunError) {
    (*ctx).faults += 1;
    (*ctx).pending.get_or_insert(e);
}

/// The fault a compiled prologue raises under the stack limit, parked as
/// [`park_null_pointer`]'s is.
///
/// # Safety
/// As [`park_null_pointer`], on the over-limit arm of a prologue.
pub unsafe extern "C" fn park_too_deep(ctx: *mut Context) -> i64 {
    park(ctx, RunError::TooDeep);
    0
}

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

/// The (chunk, cursor) to restore when a call or a line is done; held on the Rust stack
/// across the body walk.
pub(crate) type StackMark = (usize, usize);

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

/// One frame in flight: the program's, then each interpreted call's, innermost last.
/// DESIGN ›The pass runs only as far as it must, in order, and never twice‹: the frame keeps
/// the cursor of each scope it has started running.
struct Activation {
    /// The frame's base: a place reads `base + offset`.
    base: *mut u8,
    /// The [`frame_record`] of the `fn` this frame is a call of; null for the program's.
    record: DyadPtr,
    cursors: Vec<(DyadPtr, Cursor)>,
}

/// How far the pass ran a scope's lines in a frame: its run starts at line `at`, or, where line
/// `at` gave the scope its value, ends there with that value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Cursor {
    pub at: usize,
    pub given: Option<i64>,
}

impl Activation {
    fn new(base: *mut u8, record: DyadPtr) -> Self {
        Activation { base, record, cursors: Vec::new() }
    }
}

/// The frames in flight above `depth`, set aside by [`Runtime::set_aside_frames`].
pub(crate) struct SetAside {
    depth: usize,
    frames: Vec<Activation>,
}

/// The record a call of the `fn` node `f` is an instance of: its input record, which each
/// `fn` parses for itself and a copy of `f` shares. ›A scope lays out its declarations; a
/// use reaches the offset through its binding‹ [[R294]].
///
/// # Safety
/// `f` must be a `fn` node from the store.
pub(crate) unsafe fn frame_record(f: DyadPtr) -> DyadPtr {
    *(dyad::value(f) as *const DyadPtr).add(FN_INPUT)
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
    /// The program frame first, then each in-flight interpreted call's, innermost last.
    activations: Vec<Activation>,
    /// Absent at parse-time evaluation, so a compile node fails instead of
    /// installing code behind the open pass's back.
    compiler: Option<&'a crate::compile::LowerTable>,
    /// The parser owns its runtime and reaches the store through it, so the one
    /// `&mut Store` is visible to the borrow checker.
    pub(crate) store: &'a mut crate::store::Store,
    /// Set only inside [`Runtime::hosting`]; absent elsewhere, so `lex` fails
    /// with [`RunError::NoLexer`].
    lexer: Option<Lexer>,
    /// The fragments `lex «…»` built, owned for the runtime's life; boxed because
    /// the handle `lex` yields must stay put as the vector grows.
    #[allow(clippy::vec_box)]
    fragments: Vec<Box<crate::parse::ParsingTape>>,
    /// How many Logos constructors the parser is running through this runtime;
    /// `caller.scope` answers only inside one.
    constructing: u32,
    /// The driver's tape the innermost running Logos constructor was handed: a read
    /// past its frontier lexes on demand (DESIGN ›The scope's constructor is the driver‹).
    ctor_tape: Option<*mut crate::parse::ParsingTape>,
    /// The fresh node a type's own `parse` is running over, whose placed calls take a copy
    /// of it made each time they run, and whether `tape[0].type = T` has stamped it yet.
    fresh_this: Option<(DyadPtr, bool)>,
    /// The nodes whose values the line being run holds to its end, one list per scope or pass
    /// line being run, innermost last: a scope's `held` slot, or the pass's line's.
    plans: Vec<*const [DyadPtr]>,
    /// What each line being run made that nothing took, each node with the value it gave,
    /// innermost line's last; a line's end frees its own.
    line_values: Vec<(DyadPtr, i64)>,
    /// The values with an end of their own the pass ran early that nothing has taken yet, each
    /// as the node standing in its place (`identities::ran`).
    stood: Vec<DyadPtr>,
    /// The brackets whose lines a refusal waits on, until a reader takes them as data.
    unread_lines: Vec<DyadPtr>,
    /// Handed to every jump into machine code.
    pub(crate) ctx: Context,
}

/// What `lex «…»` lexes against. Raw, because the parser owns both and the
/// runtime inside it; set only by [`Runtime::hosting`], which restores the one
/// before it when its call returns or unwinds, and every read is inside such a call.
#[derive(Clone, Copy)]
struct Lexer {
    scopes: std::ptr::NonNull<crate::parse::ScopeStack>,
    trie: std::ptr::NonNull<crate::regex_trie::RegexTrie>,
    host: Option<Host>,
}

/// The parser running the pass, erased so the runtime needs no lifetime of it, and
/// its two ways back in.
#[derive(Clone, Copy)]
pub(crate) struct Host {
    pub(crate) parser: *mut (),
    /// Lexes a tape on until it holds the cell `k` right of its center, or a boundary comes first.
    pub(crate) lex_on: unsafe fn(
        *mut (),
        *mut crate::parse::ParsingTape,
        isize,
    ) -> Result<(), crate::parse::ParseError>,
    /// Builds a held `type (…)` when its node runs, yielding the new type's address.
    /// DESIGN ›A type is a comptime value, resolved in the pass‹.
    pub(crate) mint: unsafe fn(*mut (), DyadPtr) -> Result<i64, RunError>,
    /// Constructs the two cells `T v`, the type unbuilt and the value built, as the driver
    /// constructs a segment, yielding the one node they come to.
    pub(crate) construct: unsafe fn(*mut (), DyadPtr, DyadPtr) -> Result<i64, RunError>,
    /// Parses a line that waits for the run (`identities::waiting`) over the nodes its `dyad`
    /// names hold now, yielding the node to run.
    pub(crate) parse_waiting: unsafe fn(*mut (), DyadPtr) -> Result<i64, RunError>,
    /// Reads the cell `k` right of a tape's center, for itself, its type or what it gives, as the
    /// parse reads what an operation takes, yielding the node it stands for.
    pub(crate) read_tape: unsafe fn(
        *mut (),
        *mut crate::parse::ParsingTape,
        isize,
        crate::parse::TapeRead,
    ) -> Result<i64, RunError>,
}

/// Puts back the lexer a [`Runtime::hosting`] call found, by return or by unwinding.
struct Detach<'r, 'a>(&'r mut Runtime<'a>, Option<Lexer>);

impl Drop for Detach<'_, '_> {
    fn drop(&mut self) {
        self.0.lexer = self.1;
    }
}

impl<'a> Runtime<'a> {
    /// No compiler attached; see [`Runtime::with_compiler`].
    pub fn new(types: &'a crate::Core, store: &'a mut crate::store::Store) -> Self {
        Runtime {
            types,
            live_allocs: 0,
            stack: FrameStack::new(),
            activations: vec![Activation::new(store.arena_base(), std::ptr::null_mut())],
            compiler: None,
            store,
            lexer: None,
            fragments: Vec::new(),
            constructing: 0,
            ctor_tape: None,
            fresh_this: None,
            plans: Vec::new(),
            line_values: Vec::new(),
            stood: Vec::new(),
            unread_lines: Vec::new(),
            ctx: Context::new(),
        }
    }

    /// A runtime for one call into Logos, as the seed makes many of: its frames start in the
    /// chunk the store keeps between them, which [`Runtime::leave_stack`] gives back.
    pub(crate) fn for_a_call(types: &'a crate::Core, store: &'a mut crate::store::Store) -> Self {
        let mut rt = Self::new(types, store);
        rt.stack.chunks.extend(rt.store.take_frame_chunk());
        rt
    }

    /// Leaves this runtime's first frame chunk with the store; no frame may be in flight.
    pub(crate) fn leave_stack(&mut self) {
        if let Some(chunk) = self.stack.chunks.drain(..).next() {
            self.store.keep_frame_chunk(chunk);
        }
    }

    /// The lines of `bracket` are refused as a scope's unless a reader takes them as data
    /// before the line holding it ends (›A scope's value is what it evaluates to, and `return`
    /// is an optional early exit from the enclosing function‹ [[R207]]).
    pub(crate) fn hold_back_lines(&mut self, bracket: DyadPtr) {
        self.unread_lines.push(bracket);
    }

    /// A reader took the lines of `bracket` as data.
    pub(crate) fn note_lines_read(&mut self, bracket: DyadPtr) {
        self.unread_lines.retain(|&b| b != bracket);
    }

    /// Whether no reader took the lines of `bracket`, held back by [`Self::hold_back_lines`];
    /// nothing waits on them after.
    pub(crate) fn take_unread_lines(&mut self, bracket: DyadPtr) -> bool {
        let before = self.unread_lines.len();
        self.unread_lines.retain(|&b| b != bracket);
        self.unread_lines.len() != before
    }

    /// From here until [`Self::pop_plan`], a node of `plan` that runs gives a value the line
    /// being run holds to its end.
    pub(crate) fn push_plan(&mut self, plan: &[DyadPtr]) {
        self.plans.push(plan);
    }

    pub(crate) fn pop_plan(&mut self) {
        self.plans.pop();
    }

    /// Where the values the line about to run makes begin.
    pub(crate) fn line_mark(&self) -> usize {
        self.line_values.len()
    }

    /// The values made since `mark`, in the order they were made, which their line now ends.
    pub(crate) fn take_line_values(&mut self, mark: usize) -> Vec<(DyadPtr, i64)> {
        self.line_values.split_off(mark)
    }

    /// `node` stands for a value with an end of its own the pass ran early, which nothing has
    /// taken yet.
    pub(crate) fn stand(&mut self, node: DyadPtr) {
        self.stood.push(node);
    }

    /// Whether nothing has taken the value `node` stands for yet.
    pub(crate) fn stands(&self, node: DyadPtr) -> bool {
        self.stood.contains(&node)
    }

    /// Something takes the value `node` stands for: `false` where something took it already.
    pub(crate) fn take_stood(&mut self, node: DyadPtr) -> bool {
        match self.stood.iter().rposition(|&n| n == node) {
            Some(at) => {
                self.stood.remove(at);
                true
            }
            None => false,
        }
    }

    /// How many frames are in flight, the program's included.
    pub(crate) fn frame_depth(&self) -> usize {
        self.activations.len()
    }

    /// Sets aside the frames calls pushed above `depth`, so the frame in flight at `depth` is the
    /// innermost again until [`Runtime::restore_frames`] puts them back.
    pub(crate) fn set_aside_frames(&mut self, depth: usize) -> SetAside {
        SetAside { depth, frames: self.activations.split_off(depth) }
    }

    /// Puts back the frames [`Runtime::set_aside_frames`] set aside, above the frame in flight
    /// where they were set aside.
    pub(crate) fn restore_frames(&mut self, aside: &mut SetAside) {
        self.activations.truncate(aside.depth);
        self.activations.append(&mut aside.frames);
    }

    /// The innermost frame: the program's outside any call.
    fn frame(&mut self) -> &mut Activation {
        self.activations.last_mut().expect("the program frame is never popped")
    }

    /// The pass ran the scope's lines up to `cursor`; the scope's own run starts there.
    pub(crate) fn set_cursor(&mut self, scope: DyadPtr, cursor: Cursor) {
        let cursors = &mut self.frame().cursors;
        match cursors.iter_mut().find(|(s, _)| *s == scope) {
            Some(entry) => entry.1 = cursor,
            None => cursors.push((scope, cursor)),
        }
    }

    /// Where the scope's run starts in this frame: line 0 for a scope the pass never touched.
    /// Taken, since a scope the pass started runs once.
    pub(crate) fn take_cursor(&mut self, scope: DyadPtr) -> Cursor {
        let cursors = &mut self.frame().cursors;
        match cursors.iter().position(|(s, _)| *s == scope) {
            Some(i) => cursors.swap_remove(i).1,
            None => Cursor::default(),
        }
    }

    /// Run `f` with the parser's scopes and index attached for exactly this call.
    /// With `host`, the parser that lexes a constructor's tape on and builds a held `type (…)`.
    pub(crate) fn hosting<R>(
        &mut self,
        scopes: &crate::parse::ScopeStack,
        trie: &crate::regex_trie::RegexTrie,
        host: Option<Host>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let before = self.lexer.replace(Lexer {
            scopes: std::ptr::NonNull::from(scopes),
            trie: std::ptr::NonNull::from(trie),
            host,
        });
        let guard = Detach(self, before);
        f(guard.0)
    }

    /// Lexes `tape` on to the cell `k` when it is the tape a running constructor was
    /// handed; any other tape, and a read the tape already holds, is left as it stands.
    ///
    /// # Safety
    /// `tape` must be a live `ParsingTape`.
    pub(crate) unsafe fn reach(
        &mut self,
        tape: *mut crate::parse::ParsingTape,
        k: isize,
    ) -> Result<(), RunError> {
        if k <= 0 || self.ctor_tape != Some(tape) || (*tape).at(k).is_some() {
            return Ok(());
        }
        let Some(host) = self.lexer.and_then(|l| l.host) else { return Ok(()) };
        // SAFETY: the host is the parser that owns this runtime and is running it, set by
        // `hosting` for exactly this call; `ctor_tape` is the tape it handed the running
        // constructor, alive until that constructor's run returns.
        (host.lex_on)(host.parser, tape, k).map_err(|e| RunError::Parse(Box::new(e)))
    }

    /// What the cell `k` of `tape` stands for, read by the parser running the pass as `read`
    /// says, when `tape` is the one the running constructor was handed; `None` for any other.
    ///
    /// # Safety
    /// `tape` must be a live `ParsingTape`.
    pub(crate) unsafe fn read_on_tape(
        &mut self,
        tape: *mut crate::parse::ParsingTape,
        k: isize,
        read: crate::parse::TapeRead,
    ) -> Result<Option<DyadPtr>, RunError> {
        if self.ctor_tape != Some(tape) {
            return Ok(None);
        }
        let Some(host) = self.lexer.and_then(|l| l.host) else { return Ok(None) };
        // SAFETY: as `reach`.
        (host.read_tape)(host.parser, tape, k, read).map(|p| Some(p as usize as DyadPtr))
    }

    /// Construct `ty` applied to `value` through the parser running the pass.
    ///
    /// # Safety
    /// `ty` must be a type node and `value` a dyad, both from the store.
    pub(crate) unsafe fn construct_on_pass(
        &mut self,
        ty: DyadPtr,
        value: DyadPtr,
    ) -> Result<i64, RunError> {
        let Some(host) = self.lexer.and_then(|l| l.host) else {
            return Err(RunError::NoParser);
        };
        // SAFETY: as `mint_held`.
        (host.construct)(host.parser, ty, value)
    }

    /// Build the held `type (…)` `node` through the parser running the pass.
    ///
    /// # Safety
    /// `node` must be a held type node from the store.
    pub(crate) unsafe fn mint_held(&mut self, node: DyadPtr) -> Result<i64, RunError> {
        let Some(host) = self.lexer.and_then(|l| l.host) else {
            return Err(RunError::NoParser);
        };
        // SAFETY: the host is the parser that owns this runtime and is running it,
        // set by `hosting` for exactly this call; nothing reads the runtime through
        // `self` until the parser hands it back.
        (host.mint)(host.parser, node)
    }

    /// Parse `node`, a line that waits for the run, through the parser running the pass.
    ///
    /// # Safety
    /// `node` must be a node `identities::waiting` built.
    pub(crate) unsafe fn parse_waiting(&mut self, node: DyadPtr) -> Result<DyadPtr, RunError> {
        let Some(host) = self.lexer.and_then(|l| l.host) else {
            return Err(RunError::NoParser);
        };
        // SAFETY: as `mint_held`.
        (host.parse_waiting)(host.parser, node).map(|p| p as usize as DyadPtr)
    }

    /// Until the matching [`Runtime::leave_constructor`], `caller.scope` answers and a
    /// read of `tape` past its frontier lexes on. Hands back the tape it replaces.
    pub(crate) fn enter_constructor(
        &mut self,
        tape: *mut crate::parse::ParsingTape,
    ) -> Option<*mut crate::parse::ParsingTape> {
        self.constructing += 1;
        self.ctor_tape.replace(tape)
    }

    /// Starts the node unstamped and hands back the one it replaces, with its stamp.
    pub(crate) fn set_fresh_this(&mut self, this: Option<DyadPtr>) -> Option<(DyadPtr, bool)> {
        std::mem::replace(&mut self.fresh_this, this.map(|t| (t, false)))
    }

    pub(crate) fn restore_fresh_this(&mut self, outer: Option<(DyadPtr, bool)>) {
        self.fresh_this = outer;
    }

    pub(crate) fn fresh_this(&self) -> Option<DyadPtr> {
        self.fresh_this.map(|(t, _)| t)
    }

    /// The fresh node placed on the tape: from here its fields are the node's.
    pub(crate) fn stamp(&mut self, placed: DyadPtr) {
        if let Some((t, stamped)) = &mut self.fresh_this {
            *stamped |= *t == placed;
        }
    }

    /// `d` is the fresh node, not yet placed: its cell still holds the type.
    pub(crate) fn unstamped(&self, d: DyadPtr) -> bool {
        self.fresh_this == Some((d, false))
    }

    /// The parse placed `node`, a new node of a type it chose as it ran, in place of its fresh
    /// node: from here its fields are this node's.
    pub(crate) fn restamp(&mut self, node: DyadPtr) {
        if let Some(fresh) = &mut self.fresh_this {
            *fresh = (node, true);
        }
    }

    /// The index the attached lexer resolves names against.
    pub(crate) fn index(&self) -> Option<&crate::regex_trie::RegexTrie> {
        // SAFETY: `lexer` is set only inside `hosting`, whose borrows outlive this call.
        self.lexer.map(|lexer| unsafe { lexer.trie.as_ref() })
    }

    /// Takes the tape [`Runtime::enter_constructor`] handed back.
    pub(crate) fn leave_constructor(&mut self, outer: Option<*mut crate::parse::ParsingTape>) {
        self.constructing -= 1;
        self.ctor_tape = outer;
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
        // SAFETY: `lexer` is set only inside `hosting`, whose borrows outlive this call.
        let scopes = unsafe { lexer.scopes.as_ref() };
        Ok(scopes.current().unwrap_or(std::ptr::null_mut()))
    }

    /// Lex `text` against the attached scopes and index into a fresh fragment
    /// owned by this runtime; the handle is the `parsing_tape` value `insert` splices.
    pub(crate) fn lex(&mut self, text: &str) -> Result<*mut crate::parse::ParsingTape, RunError> {
        let Some(lexer) = self.lexer else {
            return Err(RunError::NoLexer);
        };
        // SAFETY: `lexer` is set only inside `hosting`, whose borrows outlive this call.
        let (scopes, trie) = unsafe { (lexer.scopes.as_ref(), lexer.trie.as_ref()) };
        let tape = crate::parse::lex_fragment(scopes, trie, self.store, self.types, text)
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

    /// Called after freeing a non-null pointer; a place holding nothing never reaches here.
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
        if dyad::ty(fn_node) != self.types.fn_type {
            return Err(RunError::NotAFunction(fn_node));
        }
        crate::compile::compile_into(self.store, lower, self.types, fn_node, code_leaf)
            .map_err(|e| RunError::CompileFailed(Box::new(crate::report::compile_message(&e))))
    }

    /// Deoptimize `fn_node`: null its `bcode`, zero its leaf's entry so a compiled
    /// caller takes the interpreter jump, and retire its artifact. The entry every
    /// structural write of a compiled body must go through.
    ///
    /// # Safety
    /// `fn_node` must be a valid `fn` node from the store.
    pub unsafe fn deopt(&mut self, fn_node: DyadPtr) {
        let bcode_slot = (dyad::value(fn_node) as *mut DyadPtr).add(FN_BCODE);
        let leaf = *bcode_slot;
        if !leaf.is_null() {
            crate::identities::callable::install_entry(leaf, 0);
            *bcode_slot = std::ptr::null_mut();
        }
        self.store.retire_artifact(fn_node);
    }

    /// The core handles, for a native's run to ask the reading rule as `run` does.
    pub(crate) fn types(&self) -> &crate::Core {
        self.types
    }

    /// Whether an interpreted call is in flight: what a `return` leaves.
    pub(crate) fn in_call(&self) -> bool {
        self.activations.len() > 1
    }

    /// A binding operand yields the dyad it names. DESIGN ›The dyad's read surface‹.
    ///
    /// # Safety
    /// `p` must be null or a valid dyad from the store.
    pub(crate) unsafe fn through(&self, p: DyadPtr) -> DyadPtr {
        self.types.through(p)
    }

    /// The machine address a place denotes: its offset from the program frame's base or
    /// from the innermost call's, for storage reached through a binding; a literal's own
    /// bytes otherwise. `None`: a call-frame place read where the innermost call is not of
    /// its function, as in parse-time evaluation or a node another call runs, which
    /// callers map to [`RunError::NoActivation`].
    ///
    /// # Safety
    /// `node` must be a valid place node.
    pub(crate) unsafe fn place_addr(&self, node: DyadPtr) -> Option<*mut u8> {
        let node = self.through(node);
        match self.types.frame_of(node) {
            Some((crate::binding::Frame::Root, off)) => Some(self.store.arena_at(off)),
            Some((crate::binding::Frame::Call(f), off)) => {
                let top = self.activations.last().expect("the program frame is never popped");
                (top.record == frame_record(f)).then(|| top.base.add(off))
            }
            // A literal's own bytes.
            None => Some(dyad::value(node)),
        }
    }

    /// `width` zeroed bytes for a result nobody named, live until the line that made it
    /// is released (DESIGN ›A scope lays out its declarations…‹).
    pub(crate) fn scratch(&mut self, width: usize) -> *mut u8 {
        self.stack.alloc(width)
    }

    /// The point a line's scratch is given back to.
    pub(crate) fn stack_mark(&self) -> StackMark {
        self.stack.mark()
    }

    pub(crate) fn stack_release(&mut self, mark: StackMark) {
        self.stack.release(mark);
    }

    /// Apply `f` to the call node `node`, `{type: _, value: [args…, null]}`:
    /// a node typed by a function, or by a type whose `code` is `f`, both
    /// arrive here. DESIGN ›Execution is function application‹.
    ///
    /// # Safety
    /// `f` must be a `fn` node from the store and `node` a node whose value
    /// is a null-terminated operand run (or null for a nullary call).
    pub unsafe fn apply(&mut self, f: DyadPtr, node: DyadPtr) -> Result<i64, RunError> {
        self.apply_into(f, node, None)
    }

    /// [`Runtime::apply`] with the slot a record result is copied into; a call of a
    /// function whose result is a record has one, and yields its address.
    ///
    /// # Safety
    /// As [`Runtime::apply`]; `dest` must be a place of the result's width.
    pub(crate) unsafe fn apply_into(
        &mut self,
        f: DyadPtr,
        node: DyadPtr,
        dest: Option<*mut u8>,
    ) -> Result<i64, RunError> {
        let (mut values, mut filled) = (Vec::new(), 0);
        let result = self
            .eval_args(f, node, dest, &mut values, &mut filled)
            .and_then(|()| self.apply_values(f, &values));
        // A fault in a later argument or in the callee ends what the call holds as its end would.
        let ended =
            crate::identities::drop_model::end_held_args(self, f, node, &mut values, filled);
        let value = result?;
        ended.map(|()| value)
    }

    /// Apply `f` to its argument block (the words `by_copy::slots` lays out): jump to
    /// the installed entry, or claim the callee's zeroed frame, copy the arguments into
    /// its parameter slots, walk the body, and pop both again.
    ///
    /// # Safety
    /// `f` must be a `fn` node from the store; `values` its argument block, whose first
    /// word is a live result slot's address when the result is a record.
    pub unsafe fn apply_values(&mut self, f: DyadPtr, values: &[i64]) -> Result<i64, RunError> {
        let fields = dyad::value(f) as *const DyadPtr;
        let bcode = *fields.add(FN_BCODE);
        if !bcode.is_null() {
            let entry = crate::identities::callable::entry_of(bcode);
            return self.call_compiled(entry as *const u8, values);
        }
        let body = *fields.add(FN_BODY);
        if body.is_null() {
            return Err(RunError::NotRunnable(f));
        }
        let mark = self.stack.mark();
        let base = self.stack.alloc(fn_frame_size(f));
        if let Err(e) = self.bind_values(f, base, values) {
            self.stack.release(mark);
            return Err(e);
        }
        self.activations.push(Activation::new(base, frame_record(f)));
        let mut result = match self.run(body) {
            Err(RunError::Return(v)) => Ok(v),
            other => other,
        };
        // The body handed back its record's address in this frame, which is still live.
        if let (Some(width), Ok(src)) = (by_copy::result_width(self.types, f), &result) {
            let dest = values[0];
            std::ptr::copy(*src as *const u8, dest as *mut u8, width);
            result = Ok(dest);
        }
        self.activations.pop();
        self.stack.release(mark);
        // A `-> void` body yields unit, matching the compiled tier's `return 0`.
        if crate::identities::numtype::is_void_type(*fields.add(FN_OUTPUT)) {
            result.map(|_| 0)
        } else {
            result
        }
    }

    /// Jump to machine code with this runtime's context, and read back the
    /// error this jump's code parked when it returns (0 is a value). A fault
    /// parked before the jump, by the compiled code whose cleanup or interpreted
    /// call made it, is set aside and put back: it stays the first.
    ///
    /// # Safety
    /// `entry` must be live machine code of the one compiled signature
    /// ([`MachineFn`]).
    pub(crate) unsafe fn call_compiled(
        &mut self,
        entry: *const u8,
        args: &[i64],
    ) -> Result<i64, RunError> {
        // The lifetime is erased at the machine-code boundary and restored
        // below before the borrow it came from ends.
        let this: *mut Runtime<'static> = (self as *mut Runtime<'a>).cast();
        (*this).ctx.runtime = this;
        (*this).ctx.root = (*this).store.arena_base();
        let outer = (*this).ctx.pending.take();
        (*this).store.enter_jump();
        let r = call_machine(entry, std::ptr::addr_of_mut!((*this).ctx), args);
        self.store.leave_jump();
        let own = std::mem::replace(&mut self.ctx.pending, outer);
        match own {
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
        // Every recursion of the run passes here, a call's body and an `if`'s arm alike.
        if crate::stack::exhausted(self.ctx.stack_limit) {
            return Err(RunError::TooDeep);
        }
        let node = self.through(node);
        let value = self.dispatch(node)?;
        if self.planned(node) {
            self.line_values.push((node, value));
        }
        Ok(value)
    }

    /// Run `node` as a line of a scope: the value it gives the scope, `None` where it goes on
    /// without one ([`crate::identities::flow::run_line`]).
    ///
    /// # Safety
    /// As [`Runtime::run`].
    pub(crate) unsafe fn run_line(&mut self, node: DyadPtr) -> Result<Option<i64>, RunError> {
        crate::identities::flow::run_line(self, node)
    }

    /// [`Runtime::run`] for a block, `if` or loop run as a line by `run`.
    ///
    /// # Safety
    /// As [`Runtime::run`]; `run` must be the line run of `node`'s type.
    pub(crate) unsafe fn run_given(
        &mut self,
        node: DyadPtr,
        run: unsafe fn(&mut Runtime, DyadPtr) -> Result<Option<i64>, RunError>,
    ) -> Result<Option<i64>, RunError> {
        if crate::stack::exhausted(self.ctx.stack_limit) {
            return Err(RunError::TooDeep);
        }
        let given = run(self, node)?;
        if let Some(value) = given.filter(|_| self.planned(node)) {
            self.line_values.push((node, value));
        }
        Ok(given)
    }

    /// Whether the line being run holds the value `node` gives to its end, which ends it.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store.
    pub(crate) unsafe fn planned(&self, node: DyadPtr) -> bool {
        let node = self.through(node);
        // SAFETY: a plan is a scope's `held` array in the store, or the pass's line's list,
        // live until its pop.
        self.plans.last().is_some_and(|&plan| (*plan).contains(&node))
    }

    /// The value a line being run holds for `node`, which ran in it before: read again there
    /// without running twice. `None` where no line holds one.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store.
    pub(crate) unsafe fn kept_value(&self, node: DyadPtr) -> Option<i64> {
        let node = self.through(node);
        self.line_values.iter().rev().find(|&&(made, _)| made == node).map(|&(_, value)| value)
    }

    /// [`Runtime::run`]'s one thing, for `node` read through.
    ///
    /// # Safety
    /// As [`Runtime::run`].
    unsafe fn dispatch(&mut self, node: DyadPtr) -> Result<i64, RunError> {
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
            Read::Executable(Dispatch::None) => {
                Err(crate::identities::run_body::unfilled_field(node)
                    .map_or(RunError::NoLeaf, RunError::UnfilledField))
            }
            Read::Unit => Ok(0),
            // An identity's value is its own address.
            Read::Identity | Read::Node => Ok(node as i64),
            // A view's value is the viewed node's address.
            Read::Address => Ok(dyad::head(node) as i64),
            // A rational value travels as the address of its sixteen bytes.
            Read::Rational => Ok(self.place_addr(node).ok_or(RunError::NoActivation)? as i64),
            Read::Container(_) | Read::Behind(_) => self.read_container(node),
            Read::Scalar(_) | Read::Pointer(_) => {
                let slot = self.place_addr(node).ok_or(RunError::NoActivation)?;
                if slot.is_null() {
                    return Err(RunError::Uninitialized);
                }
                Ok(crate::identities::numtype::read_scalar(self.types.type_of(node), slot))
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

    /// Evaluate a call's arguments in the caller's frame into its argument block `values`: a
    /// record copied at its width, anything else in its container. `filled` counts the
    /// arguments in it, so a failing one leaves the count of those before it.
    ///
    /// # Safety
    /// `fn_node` must be a valid function node and `call_node` a valid
    /// application of it, both from the store; `dest` as for [`Runtime::apply_into`].
    unsafe fn eval_args(
        &mut self,
        fn_node: DyadPtr,
        call_node: DyadPtr,
        dest: Option<*mut u8>,
        values: &mut Vec<i64>,
        filled: &mut usize,
    ) -> Result<(), RunError> {
        let args = crate::parse::null_terminated(dyad::value(call_node) as *const DyadPtr);
        let types = self.types;
        if args.len() != by_copy::slots(types, fn_node).count() {
            return Err(RunError::ArityMismatch);
        }
        *values = vec![0i64; by_copy::words(types, fn_node)];
        if by_copy::result_width(types, fn_node).is_some() {
            values[0] = dest.ok_or(RunError::NoWholeRead)? as i64;
        }
        for (i, slot) in by_copy::slots(types, fn_node).enumerate() {
            let arg = args[i];
            match slot.width {
                Some(width) => {
                    let src = by_copy::record_addr(self, arg)?;
                    let block = values.as_mut_ptr().add(slot.word).cast::<u8>();
                    std::ptr::copy_nonoverlapping(src, block, width);
                }
                None => values[slot.word] = self.run(arg)?,
            }
            *filled = i + 1;
        }
        Ok(())
    }

    /// Copy the argument block into the parameter slots of the fresh frame at `base`: a
    /// record at its width, a scalar at its type's width, any other as the full container.
    ///
    /// # Safety
    /// `fn_node` must be a valid function node; `base` a frame allocation of
    /// its frame size, which covers every parameter slot the parser assigned.
    unsafe fn bind_values(
        &self,
        fn_node: DyadPtr,
        base: *mut u8,
        values: &[i64],
    ) -> Result<(), RunError> {
        if values.len() != by_copy::words(self.types, fn_node) {
            return Err(RunError::ArityMismatch);
        }
        for slot in by_copy::slots(self.types, fn_node) {
            let dst = base.add(slot.offset);
            let ty = crate::identities::hole::type_in(slot.param);
            let at = values.as_ptr().add(slot.word);
            match slot.width {
                Some(width) => std::ptr::copy_nonoverlapping(at.cast(), dst, width),
                None if crate::identities::numtype::is_scalar_type(ty) => {
                    crate::identities::numtype::write_scalar(ty, dst, *at)
                }
                None => std::ptr::write_unaligned(dst as *mut i64, *at),
            }
        }
        Ok(())
    }
}
