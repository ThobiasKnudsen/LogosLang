// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The parsing tape and the driver: the scope's constructor lexing one token
//! at a time and constructing the segment's cells highest parse_rank first
//! (DESIGN ›The scope's constructor is the driver‹). Also the scope stack and
//! name resolution over it; the trie is only the name index.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::binding::Binding;
use crate::dyad::DyadPtr;
use crate::regex_trie::{RegexTrie, RegexTrieError};
use crate::store::Store;
use crate::Core;

mod tape;
pub use tape::*;
mod scopes;
pub use scopes::*;
mod error;
pub use error::*;
mod lex;
pub(crate) use lex::*;
mod driver;
pub use driver::*;
mod declare;
mod operands;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
}

/// A function node's value record, in order: input `record`, return type,
/// body, compiled `callable` (null until compiled), frame size, outer names
/// (DESIGN ›Execution is function application‹).
pub const FN_INPUT: usize = 0;
pub const FN_OUTPUT: usize = 1;
pub const FN_BODY: usize = 2;
/// A `callable` node (`[entry: @exec, convention]`), null until compiled.
pub const FN_BCODE: usize = 3;
/// A `u64` leaf: the frame's byte size, parameters first, then the locals at
/// their offsets; null for none. Both tiers read it on entry.
pub const FN_FRAME: usize = 4;
/// An `array` of the bindings of the outer names the body reads, in
/// first-read order, or null; read at every call (DESIGN ›`own` and `drop`
/// are static‹).
pub const FN_OUTER: usize = 5;
/// A `u64` leaf of [`RECEIVER_READS`] and [`RECEIVER_WRITES`] bits: whether a
/// `share` function's body reads or writes a field of its value; null for neither.
pub const FN_RECEIVER: usize = 6;
pub const RECEIVER_READS: u64 = 1;
pub const RECEIVER_WRITES: u64 = 2;

/// `0` when the slot is null (no parameters and no locals).
///
/// # Safety
/// `fn_node` must be a function node whose value is the seven-slot record
/// `parse_fn` builds.
pub unsafe fn fn_frame_size(fn_node: DyadPtr) -> usize {
    let frame = *((*fn_node).value as *const DyadPtr).add(FN_FRAME);
    if frame.is_null() {
        0
    } else {
        std::ptr::read_unaligned((*frame).value as *const u64) as usize
    }
}

/// Empty for a function that reads none, and for a declaration's placeholder
/// whose value is still being parsed.
///
/// # Safety
/// `fn_node` must be a function node: its value null, or the operands
/// `parse_fn` builds (the early signature included).
pub unsafe fn fn_outer<'a>(fn_node: DyadPtr) -> &'a [DyadPtr] {
    let fields = (*fn_node).value as *const DyadPtr;
    if fields.is_null() {
        return &[];
    }
    let outer = *fields.add(FN_OUTER);
    if outer.is_null() {
        &[]
    } else {
        crate::identities::array::items(outer)
    }
}

/// The [`FN_RECEIVER`] bits; `0` for anything but a built function node.
///
/// # Safety
/// `f` must be a resolved dyad from the store.
pub(crate) unsafe fn fn_receiver(types: &Core, f: DyadPtr) -> u64 {
    if f.is_null() || (*f).ty != types.fn_type || (*f).value.is_null() {
        return 0;
    }
    let leaf = *((*f).value as *const DyadPtr).add(FN_RECEIVER);
    if leaf.is_null() {
        0
    } else {
        std::ptr::read_unaligned((*leaf).value as *const u64)
    }
}

/// The slot words a type body declares into its own scope, in `SlotKind`
/// order (DESIGN ›A type body describes one level‹). `drop` is not here: it is
/// the statement keyword, which `=` takes as the slot's name.
pub const SLOT_NAMES: [&str; 5] = ["parse_rank", "lex_rank", "associativity", "parse", "run"];

/// Each open bracket recurses through `parse_sequence` and `(`'s constructor,
/// so a wall of `(` costs Rust stack like a runaway recursion: sized well
/// under `crate::WORK_STACK_BYTES` and far past anything a person writes.
pub const MAX_BRACKET_DEPTH: usize = 2_000;

/// One of the slots, by `SLOT_NAMES` position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    ParseRank = 0,
    LexRank = 1,
    Associativity = 2,
    Parse = 3,
    Run = 4,
    /// The `drop` keyword left of `=`: no spelling in `SLOT_NAMES`, since the
    /// word is the statement's.
    Drop = 5,
}

impl SlotKind {
    fn of(i: usize) -> Self {
        match i {
            0 => SlotKind::ParseRank,
            1 => SlotKind::LexRank,
            2 => SlotKind::Associativity,
            3 => SlotKind::Parse,
            _ => SlotKind::Run,
        }
    }
}

/// What a `type (…)` body's lines have filled so far: the head the type node
/// takes at the close, its one set of slots, and its values' fields.
struct OpenType {
    scope: DyadPtr,
    /// The five slot words as this definition's own markers, declared into the body's scope.
    slots: [DyadPtr; 5],
    /// The marker a `drop = (…)` line declares; `drop` itself is the core word.
    drop_marker: DyadPtr,
    parse_rank: f64,
    /// From a `lex_rank = …` line; written onto the declaration's binding at
    /// the close.
    lex_rank: Option<f64>,
    assoc: Assoc,
    ctor: DyadPtr,
    /// From a `run = (…)` line: the body lexed once into its cells; a node of
    /// the type constructs it per field-type set.
    run_body: DyadPtr,
    /// The hidden receiver of the body being read, a parse's node or a `drop`'s or
    /// `share` function's value; null otherwise.
    this_param: DyadPtr,
    /// `this_param` is a parse's node, whose fields are reached through `tape[0]`, never bare.
    in_parse: bool,
    /// The parse body's `tape` parameter while `in_parse`.
    tape_param: DyadPtr,
    /// The declaration this body is the value of: inside its parse, its name is `self_type`.
    binding: DyadPtr,
    /// The `share` function being read has read a field of its value.
    read_receiver: bool,
    /// The `share` function being read has written a field of its value, itself or through a
    /// bare call.
    wrote_receiver: bool,
    /// The fields' scope and the fields declared so far: what a body written
    /// below them reaches by name.
    block: (DyadPtr, Vec<DyadPtr>),
    /// From a `share drop = (…)` line: the `fn` over the value an owner's teardown runs.
    instances_drop: DyadPtr,
    /// A field declared `own` over a node's type, whose teardown the type's drop must write.
    owns_node_field: bool,
    /// The type node being defined, its record written at the close: what its name means in its parse.
    self_type: DyadPtr,
}

#[derive(Default)]
struct OpenScope {
    /// The `defer free <place>` nodes the scope's owning bindings inserted,
    /// drained into the body after each statement.
    defers: Vec<DyadPtr>,
    /// The places those teardowns free, once drained: no tail or `return`
    /// leaving the scope may hand one out.
    owned: Vec<DyadPtr>,
    /// Items parsed at depth 0 and not yet run: `drain` runs them when the
    /// pass needs a value, the scope's own run otherwise.
    unrun: Vec<DyadPtr>,
    /// The tape cells and lines a check in this scope has narrowed, each with the type
    /// its read yields.
    narrowed: Vec<(CellKey, DyadPtr)>,
}

/// A `bool` value, a comparison, or a logical operator.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_bool_result(types: &Core, node: DyadPtr) -> bool {
    let node = types.through(node);
    let logos = (*node).ty;
    // An item that ran in the pass is what its expression is.
    if logos == types.ran_ {
        return is_bool_result(types, crate::identities::ran::expr_of(types, node));
    }
    // A sequence's value is its trailing expression's.
    if logos == types.scope {
        return match last_sequence_expr(node) {
            Some(last) => is_bool_result(types, last),
            None => false,
        };
    }
    // `and`/`or` over two booleans is one; over two non-booleans it is a group.
    if logos == types.and_ || logos == types.or_ {
        let (lhs, rhs) = crate::identities::operands(node);
        return is_bool_result(types, lhs) && is_bool_result(types, rhs);
    }
    // A call, or a node of a type with a run, is what its function declares it returns.
    let f = if crate::identities::meta::is_record_type(logos)
        && !crate::identities::meta::run_body_of(logos).is_null()
    {
        crate::identities::run_body::spec_of(node)
    } else {
        logos
    };
    if !f.is_null() && (*f).ty == types.fn_type && !(*f).value.is_null() {
        return *((*f).value as *const DyadPtr).add(FN_OUTPUT) == types.bool_;
    }
    logos == types.bool_
        || logos == types.lt
        || logos == types.gt
        || logos == types.eq
        || logos == types.le
        || logos == types.ge
        || logos == types.ne
        || logos == types.not_
        || logos == types.subset
        || logos == types.tape.is_constructed
}

/// Deliberately no scope unwrapping: a sequence-valued condition may carry
/// effectful non-tail expressions a fold would silently drop.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn bool_literal_value(types: &Core, node: DyadPtr) -> Option<bool> {
    let node = types.through(node);
    if (*node).ty != types.bool_ || (*node).value.is_null() {
        return None;
    }
    Some(std::ptr::read_unaligned((*node).value as *const i32) != 0)
}

/// A cell or line of the tape, by the tape's place, a literal index and the line.
pub(crate) type CellKey = (DyadPtr, i32, crate::identities::tape::Line);

/// `tape[k]:type == T` or `!=` with `T` a number type or `type`, either side first, or
/// `tape[k]:type ⊆ T` with `T` a number type, each under any number of `not`: the cell
/// checked, the type it narrows to, and whether the branch that holds is the `then`.
///
/// # Safety
/// `cond` must be a reduced dyad from the store.
unsafe fn type_check_of(types: &Core, cond: DyadPtr) -> Option<(CellKey, DyadPtr, bool)> {
    let cond = types.through(cond);
    if (*cond).ty == types.not_ {
        let (key, ty, holds) = type_check_of(types, *((*cond).value as *const DyadPtr))?;
        return Some((key, ty, !holds));
    }
    let subset = (*cond).ty == types.subset;
    let eq = subset || (*cond).ty == types.eq;
    if !eq && (*cond).ty != types.ne {
        return None;
    }
    let ops = (*cond).value as *const DyadPtr;
    let (l, r) = (types.through(*ops), types.through(*ops.add(1)));
    let is_read = |n: DyadPtr| (*n).ty == types.tape.cell_type;
    let (read, ty) = if is_read(l) {
        (l, r)
    } else if is_read(r) && !subset {
        (r, l)
    } else {
        return None;
    };
    let number = crate::identities::is_numtype_node(types, ty)
        || crate::identities::meta::is_node_valued(ty, types.fn_type);
    if !number && (subset || ty != types.type_) {
        return None;
    }
    crate::identities::tape::cell_key(types, read).map(|key| (key, ty, eq))
}

/// Trailing comment nodes are prose, not the tail; `None` for a scope with
/// no expression array.
///
/// # Safety
/// `node` must be a valid dyad from the store; a non-null value must be the
/// `[exprs, op, parent]` triple `parse_sequence` fills.
pub(crate) unsafe fn last_sequence_expr(node: DyadPtr) -> Option<DyadPtr> {
    crate::identities::scope::exprs_of(node)?
        .iter()
        .rev()
        .find(|&&e| !crate::identities::numtype::is_comment_type((*e).ty))
        .copied()
}

/// The positions `commit_tail` enumerates: a `return` itself, an `if`'s
/// branches, a sequence's expressions.
///
/// # Safety
/// `node` must be a valid dyad from the store, with the value shapes its
/// type implies.
pub(crate) unsafe fn contains_return(types: &Core, node: DyadPtr) -> bool {
    let logos = (*node).ty;
    if logos == types.return_ {
        return true;
    }
    if logos == types.if_ {
        let p = (*node).value as *const DyadPtr;
        let (then, els) = (*p.add(1), *p.add(2));
        return contains_return(types, then) || (!els.is_null() && contains_return(types, els));
    }
    if logos == types.scope {
        if (*node).value.is_null() {
            return false;
        }
        let arr = *((*node).value as *const DyadPtr);
        return crate::identities::array::items(arr).iter().any(|&e| contains_return(types, e));
    }
    false
}

/// `{type: callee, value: [args…, null]}`: null-terminated so `run` can count
/// the arguments; a nullary call carries a null value.
fn build_call(store: &mut Store, callee: DyadPtr, args: &[DyadPtr]) -> DyadPtr {
    let value = if args.is_empty() {
        std::ptr::null_mut()
    } else {
        let mut ops = args.to_vec();
        ops.push(std::ptr::null_mut());
        store.alloc_operands(&ops)
    };
    store.alloc_raw(callee, value)
}

/// The once-per-run import registry: a file loads once per run, every
/// importer sharing the one loaded section, and a path met again while its
/// own load is in progress is a cycle.
#[derive(Debug, Default)]
pub struct Imports {
    entries: HashMap<PathBuf, ImportState>,
    /// The section scope each loaded file's declarations landed in: a
    /// section's names live for the run, so a `pub` function's body may read
    /// a private sibling from anywhere (DESIGN ›Importing is dropping the text there‹).
    sections: HashSet<DyadPtr>,
}

#[derive(Debug)]
enum ImportState {
    /// Importing it again now is a cycle.
    Loading,
    /// The `pub` names in declaration order, each with the identity it
    /// resolves to inside the file, and the file's tail node (null for a
    /// declaration-only file).
    Loaded { pubs: Vec<(String, DyadPtr)>, tail: DyadPtr },
}

/// The one-pass elaborator: deferred reduction by parse_rank over the explicit
/// tape, not Pratt; operators wait on the tape until parse_rank says to
/// reduce them.
pub struct Parser<'a> {
    /// The sub-parse being driven now; `outer` holds the ones it interrupted, innermost last.
    cx: Context<'a>,
    outer: Vec<Context<'a>>,
    trie: &'a mut RegexTrie,
    types: &'a Core,
    imports: Imports,
    /// The pass's one runtime (DESIGN ›Build and run are one self-directing
    /// pass‹): one runtime keeps the frame arena and the allocation ledger
    /// whole across the pass.
    rt: crate::run::Runtime<'a>,
    /// The lowering table, when the driver attached one: handed to a nested
    /// import's runtime so `f.compile()` at an imported top level works as at
    /// the driver's own.
    lower: Option<&'a crate::compile::LowerTable>,
}

/// Everything a sub-parse (a held `type (…)`, a run body, an `import`) sets aside and puts
/// back: one value, pushed by [`Parser::enter`] and popped by its guard on every path.
struct Context<'a> {
    source: &'a str,
    pos: usize,
    scopes: ScopeStack,
    /// The placeholder of the declaration awaiting its value, or null:
    /// `parse_fn` publishes the signature onto it before the body parses, so
    /// a recursive self-call resolves its types.
    pending_fn: DyadPtr,
    /// The bindings of the declarations whose right side is being driven,
    /// innermost last: what a `lex_rank = …` line writes. A stand-in for a
    /// definition writing a binding field from inside `:=`.
    filling: Vec<DyadPtr>,
    /// Where each of those right sides starts in the source.
    filling_at: Vec<usize>,
    /// The binding of the last declaration that reduced: what a gate word to
    /// its left marks.
    last_declared: DyadPtr,
    /// The binding of the name left of the `.` being constructed, or null.
    member_root: DyadPtr,
    /// The bindings the member read under construction reaches, root first: what `.` writes
    /// onto the cell it places, for the write it may be the target of.
    member_path: Vec<DyadPtr>,
    /// The type and field parameters of the run body being constructed.
    run_fields: Option<(DyadPtr, Vec<DyadPtr>)>,
    /// Open function frames, innermost last: empty at top level, where
    /// declarations get global storage; inside a function each local claims
    /// the next byte offset in the top frame.
    frames: Vec<OpenFn>,
    /// How many deferred-or-repeated bodies enclose the position, where parse
    /// order and run order do not coincide; comptime effects that rebind names
    /// at parse are rejected while non-zero. Comptime-taken `if` branches do not count.
    runtime_depth: u32,
    /// The type definitions open around the position, innermost last; slot
    /// fills and fields write into the top.
    definitions: Vec<OpenType>,
    /// The open scopes, innermost last, the base entry the top level; its
    /// length is the bracket depth.
    open: Vec<OpenScope>,
    /// The folder relative import paths resolve against: the importing file's
    /// own folder during a nested import, else the working directory.
    dir: PathBuf,
    /// Comment cells lifted out of a segment at its boundary, with the offset
    /// each was lexed at, handed out as body items in source order beside the
    /// segment's expression.
    lifted: Vec<(usize, DyadPtr)>,
    /// Body items a constructed segment yielded, in source order, not yet
    /// handed out.
    queued: std::collections::VecDeque<Cell>,
    /// The last `=` built: what the sibling-write rule reads at the next item.
    last_write: LastWrite,
    /// Whether the constructor now running was woken at discovery, its token
    /// just lexed and the source after it unread, rather than at the boundary:
    /// an identity that reads its own bracket reads source only at discovery.
    discovering: bool,
    /// While `.` lexes the member right of a parse's `tape[0]`: the member is
    /// read by its spelling, so a raw-text word it happens to spell is not woken.
    member_asleep: bool,
    /// The identity whose constructor is running, whose lazy reads lex its right side.
    reader: DyadPtr,
    /// The stop mode of the segment being lexed, so a lazy read inside a
    /// constructor stops at the same boundaries the loop would.
    lex_mode: Option<RightSide>,
    /// While an `if` reads a bare branch: the depth of the branch's own scope,
    /// where `else` and `,` end it; a nested bracket reads them as usual.
    else_ends: Option<usize>,
    /// The held run body whose cells the driver is reading now, if any.
    feed: Option<Feed>,
    /// While a held `type (…)` is built at its run, the function frames open
    /// where it was written: a place declared at that depth is the type's, global.
    held_depth: usize,
    /// A cell the branch about to open is checked against a type, with that type.
    narrow_next: Option<(CellKey, DyadPtr)>,
    /// While a `share` line of a type body is read, the frame depth a
    /// `fn` written as the member's value opens at: that `fn` takes the value first.
    member_fn_depth: Option<usize>,
    /// While the initializer of a `share` name parses: the frame depth it
    /// stands at. It runs once, at the definition, so its places are global
    /// and no call's slot is in reach (DESIGN ›Two muts, and the storage partition‹).
    share_init: Option<usize>,
    /// While a type body's field declaration reads its type: a hole there names the
    /// field's type and needs no place, so a type without one, `square_brackets ?`, may stand.
    field_hole: bool,
}

impl<'a> Context<'a> {
    fn top(source: &'a str, scopes: ScopeStack) -> Self {
        Context {
            source,
            pos: 0,
            scopes,
            pending_fn: std::ptr::null_mut(),
            filling: Vec::new(),
            filling_at: Vec::new(),
            last_declared: std::ptr::null_mut(),
            member_root: std::ptr::null_mut(),
            member_path: Vec::new(),
            run_fields: None,
            frames: Vec::new(),
            runtime_depth: 0,
            definitions: Vec::new(),
            open: vec![OpenScope::default()],
            dir: PathBuf::from("."),
            lifted: Vec::new(),
            queued: std::collections::VecDeque::new(),
            last_write: LastWrite::NONE,
            discovering: false,
            member_asleep: false,
            reader: std::ptr::null_mut(),
            lex_mode: None,
            else_ends: None,
            feed: None,
            held_depth: 0,
            narrow_next: None,
            member_fn_depth: None,
            share_init: None,
            field_hole: false,
        }
    }

    /// A sub-parse over `source`: fresh state, keeping only what the interrupted parse
    /// leaves in force across it.
    fn nested(outer: &Self, source: &'a str, scopes: ScopeStack) -> Self {
        Context {
            dir: outer.dir.clone(),
            held_depth: outer.held_depth,
            last_declared: outer.last_declared,
            last_write: outer.last_write,
            reader: outer.reader,
            ..Context::top(source, scopes)
        }
    }
}

/// The last `=` node built, the scope it was built in, and the name and field it fills when
/// its target was a `v.f` the unwritten-fields rule noted (nulls otherwise).
#[derive(Clone, Copy)]
struct LastWrite {
    node: DyadPtr,
    scope: DyadPtr,
    fill: (DyadPtr, DyadPtr),
}

impl LastWrite {
    const NONE: LastWrite = LastWrite {
        node: std::ptr::null_mut(),
        scope: std::ptr::null_mut(),
        fill: (std::ptr::null_mut(), std::ptr::null_mut()),
    };
}

/// Puts back the context a [`Parser::enter`] set aside, by return or by unwinding.
struct Entered<'p, 'a>(&'p mut Parser<'a>);

impl Drop for Entered<'_, '_> {
    fn drop(&mut self) {
        let outer = self.0.outer.pop().expect("pushed by enter");
        self.0.cx = outer;
    }
}

/// One enclosing function body being parsed: parameters claim the frame's
/// first offsets, the body's locals continue after them (DESIGN ›Resolution
/// is one rule‹).
struct OpenFn {
    /// Bytes claimed so far by parameters and frame-relative locals.
    size: usize,
    /// The scope depth the function's barrier begins at: a name declared
    /// below it is from outside the function.
    below: usize,
    /// The bindings of the outer names the body has read so far, first-read
    /// order, each once: the function's `FN_OUTER` list.
    outer: Vec<DyadPtr>,
    /// `open`'s length outside the body: the scopes a `return` leaves are the ones past it.
    open_below: usize,
    /// Every `return` in the body, committed to the result type as the tail is.
    returns: Vec<DyadPtr>,
}

/// A scalar at its own width; anything else an 8-byte container.
///
/// # Safety
/// `logos` must be null or a type node from the store.
unsafe fn field_width(logos: DyadPtr) -> u64 {
    if crate::identities::numtype::is_scalar_type(logos) {
        crate::identities::numtype::of_type_node(logos).bytes() as u64
    } else {
        8
    }
}

impl<'a> Parser<'a> {
    pub fn new(
        source: &'a str,
        store: &'a mut Store,
        trie: &'a mut RegexTrie,
        types: &'a Core,
        scopes: ScopeStack,
    ) -> Self {
        // The parser reaches the store through its runtime, so the one
        // `&mut Store` has one owner.
        let rt = crate::run::Runtime::new(types, store);
        Parser {
            cx: Context::top(source, scopes),
            outer: Vec::new(),
            rt,
            trie,
            types,
            imports: Imports::default(),
            lower: None,
        }
    }

    /// Drive a sub-parse under `cx`; the guard puts the interrupted context back when it drops.
    fn enter(&mut self, cx: Context<'a>) -> Entered<'_, 'a> {
        let outer = std::mem::replace(&mut self.cx, cx);
        self.outer.push(outer);
        Entered(self)
    }

    /// So an imported file's top-level `f.compile()` runs under the nested
    /// pass exactly as under the driver.
    pub fn with_lower(mut self, lower: &'a crate::compile::LowerTable) -> Self {
        self.lower = Some(lower);
        self.rt.set_compiler(lower);
        self
    }

    /// An item that ran in the pass has its value at hand; anything else runs
    /// now. The drivers' one read of an item's value.
    ///
    /// # Safety
    /// `node` must be a valid dyad this parser built into its store.
    pub unsafe fn value_of(&mut self, node: DyadPtr) -> Result<i64, ParseError> {
        if (*node).ty == self.types.ran_ {
            return Ok(crate::identities::ran::value_of(node));
        }
        self.run_on_pass(node).map_err(ParseError::Run)
    }

    /// The root scope's exit: the top level's teardowns, LIFO. The file
    /// driver calls this at program end; a nested scope ran its own at exit.
    pub fn exit(&mut self) -> Result<(), ParseError> {
        for defer_node in self.take_pending_defers().into_iter().rev() {
            // SAFETY: `defer_node` is a `defer` node in the store, which outlives the pass.
            unsafe { crate::identities::run_deferred(&mut self.rt, defer_node) }
                .map_err(ParseError::Run)?;
        }
        Ok(())
    }

    /// The REPL's: a session is one run, so its per-line parsers share one
    /// registry.
    pub fn with_imports(mut self, imports: Imports) -> Self {
        self.imports = imports;
        self
    }

    pub fn take_imports(&mut self) -> Imports {
        std::mem::take(&mut self.imports)
    }

    pub(crate) fn store(&mut self) -> &mut Store {
        self.rt.store
    }

    /// Copied out, so a `&mut self` call can follow.
    pub(crate) fn types(&self) -> &'a Core {
        self.types
    }

    /// Inside a function the place is frame-relative, the next offset after
    /// the parameters, its storage per call; at top level an absolute global
    /// blob. The value is a `FRAME_TAG` offset or a real address respectively.
    pub(crate) fn alloc_local(&mut self, ty_node: DyadPtr, width: usize) -> DyadPtr {
        let place = if self.cx.frames.len() <= self.cx.held_depth
            || self.cx.share_init == Some(self.cx.frames.len())
        {
            // Tagged as storage, so a place and a definition's record are
            // told apart everywhere, not only where a frame exists.
            crate::dyad::global_place(self.rt.store.alloc_bytes(&vec![0u8; width]))
        } else {
            let depth = self.cx.frames.len();
            let frame = self.cx.frames.last_mut().unwrap();
            let offset = frame.size;
            frame.size += width;
            crate::dyad::frame_place(depth, offset)
        };
        self.rt.store.alloc_raw(ty_node, place)
    }

    /// A driver's top-level line is complete: see [`ScopeStack::close_item`].
    ///
    /// # Safety
    /// `item` must be a line this parser just returned.
    pub unsafe fn close_item(&mut self, item: DyadPtr) {
        // SAFETY: the pending bindings were minted by this parser's declares.
        unsafe { self.cx.scopes.close_item(self.rt.store, self.types.array_, item) }
    }

    /// The top level's `defer free` nodes, which no `parse_sequence` drained
    /// (the top level is no block); insertion order, the caller reverses.
    pub fn take_pending_defers(&mut self) -> Vec<DyadPtr> {
        match self.cx.open.first_mut() {
            Some(base) => std::mem::take(&mut base.defers),
            None => Vec::new(),
        }
    }

    /// Run everything parsed and not yet run, outermost scope first (DESIGN
    /// ›Build and run are one self-directing pass‹): an executable item becomes
    /// its ran form in place. The lists are taken first, so a nested drain finds nothing.
    pub fn drain(&mut self) -> Result<(), ParseError> {
        let lists: Vec<Vec<DyadPtr>> =
            self.cx.open.iter_mut().map(|s| std::mem::take(&mut s.unrun)).collect();
        for node in lists.into_iter().flatten() {
            // SAFETY: every pending item is a dyad this parser built into its store, which outlives the pass.
            unsafe {
                let ty = (*node).ty;
                // A `[…]` line is a list that goes whole to the call that takes it, which
                // evaluates its lines when it runs (DESIGN ›A bracket goes to the call whole‹).
                if crate::identities::numtype::is_comment_type(ty)
                    || ty == self.types.defer_
                    || ty == self.types.square_brackets
                {
                    continue;
                }
                let bits = self.run_on_pass(node).map_err(ParseError::Run)?;
                if matches!(
                    crate::identities::read::read_kind(self.types, node),
                    crate::identities::read::Read::Executable(_)
                ) {
                    crate::identities::ran::rewrite(self.rt.store, self.types, node, bits);
                }
            }
        }
        Ok(())
    }

    /// The end of the program: the root scope's own run (DESIGN ›The scope's
    /// constructor is the driver‹).
    pub fn finish(&mut self) -> Result<(), ParseError> {
        self.drain()
    }

    /// The REPL parses each line with a fresh `Parser` over one persistent
    /// scope stack.
    pub fn into_scopes(self) -> ScopeStack {
        self.cx.scopes
    }

    /// `( field-list )` as a record node whose value is a `RECORD_TAG` record
    /// holding the scope, the `fields` array and the packed `size_bytes`. Fresh
    /// field names are read raw, which is why the list has its own sub-parse.
    pub fn parse_record(&mut self) -> Result<DyadPtr, ParseError> {
        self.parse_record_taking(None)
    }

    /// `parse_record` with an unnamed field of type `leading` first.
    fn parse_record_taking(&mut self, leading: Option<DyadPtr>) -> Result<DyadPtr, ParseError> {
        let record_logos = self.types.type_;
        let (scope, fields_arr, size_bytes) = self.parse_field_list(false, leading)?;
        let record = crate::identities::meta::record_layout(
            self.rt.store,
            scope,
            fields_arr,
            size_bytes,
            std::ptr::null_mut(),
            crate::identities::meta::prec::APPLY,
            Assoc::Left,
        );
        Ok(self.rt.store.alloc_raw(record_logos, record.cast()))
    }

    /// A `fn`'s parameter list checks each name against every open scope, since
    /// the body reopens the list's scope; a type body's fields against their
    /// siblings alone (`relaxed`), where the list is the body itself, its
    /// slot fills, `share` members and prose among the fields (DESIGN ›A type
    /// body describes one level‹).
    fn parse_field_list(
        &mut self,
        relaxed: bool,
        leading: Option<DyadPtr>,
    ) -> Result<(DyadPtr, DyadPtr, u64), ParseError> {
        if !relaxed {
            self.expect_open()?;
        }
        // Field names are declared into the record's own scope.
        let scope = self.open_scope();

        let mut fields = Vec::new();
        if let Some(ty) = leading {
            fields.push(self.rt.store.alloc_raw(ty, std::ptr::null_mut()));
        }
        let lines = self.field_lines(relaxed, scope, &mut fields);

        self.cx.scopes.pop();
        lines?;
        if !relaxed {
            self.expect_close()?;
        }

        // Fields pack in declaration order at the width rule parameters claim
        // frame offsets by.
        // SAFETY: each field is the dyad just built, its type null or a type node.
        let size_bytes: u64 = fields.iter().map(|&f| unsafe { field_width((*f).ty) }).sum();
        let fields_arr = crate::identities::array::build(self.rt.store, self.types.array_, &fields);
        Ok((scope, fields_arr, size_bytes))
    }

    fn field_lines(
        &mut self,
        relaxed: bool,
        scope: DyadPtr,
        fields: &mut Vec<DyadPtr>,
    ) -> Result<(), ParseError> {
        loop {
            if self.at_close() {
                break;
            }
            if relaxed {
                if self.at_end() {
                    return Err(ParseError::UnclosedBracket);
                }
                if let Some(def) = self.cx.definitions.last_mut() {
                    def.block = (scope, fields.clone());
                }
                // Prose before a slot fill is lifted beside it, as on any line;
                // before a field or member it is read past.
                if self.at_prose() {
                    let at = self.cx.pos;
                    self.skip_prose();
                    if self.at_slot_word() {
                        return Err(ParseError::SlotFillNeedsShare);
                    }
                    if self.at_share_slot() {
                        self.cx.pos = at;
                        self.lift_prose()?;
                        self.lex_spelling();
                        self.slot_line(self.cx.pos)?;
                    }
                    continue;
                }
            }
            let (start, len) = self.lex_spelling().ok_or(ParseError::ExpectedField)?;
            // `self.cx.source` is `&'a str` (Copy), so the slice is independent
            // of the `&mut self` needed below.
            let source = self.cx.source;
            let name = &source[start..start + len];
            let word = self.cx.scopes.resolve(self.trie, name).ok().map(|r| r.identity);
            // One place stored with the type, not a field of the layout; never a
            // parameter, which each call fills (DESIGN ›Two muts, and the storage partition‹).
            if word == Some(self.types.share_) {
                if !relaxed {
                    self.cx.pos = start;
                    return Err(ParseError::ShareMisplaced);
                }
                if self.at_slot_word() {
                    self.slot_line(self.cx.pos)?;
                } else {
                    self.share_member(start)?;
                }
                continue;
            }
            // `mut` or `immut` before a field or parameter gates its binding, as before any name.
            let gate = word.filter(|&w| w == self.types.mut_ || w == self.types.immut_);
            let (start, name) = if gate.is_some() {
                let (start, len) = self.lex_spelling().ok_or(ParseError::ExpectedField)?;
                (start, &source[start..start + len])
            } else {
                (start, name)
            };
            // A literal, `5`, matches a pattern identity and names nothing.
            if relaxed {
                if let Ok(r) = self.cx.scopes.resolve(self.trie, name) {
                    // SAFETY: a resolved binding is a binding dyad from the store.
                    if unsafe { Binding::spelling(r.binding) } != name {
                        self.cx.pos = start;
                        return Err(ParseError::TypeBodyLine);
                    }
                }
            }
            if relaxed && gate.is_none() {
                let declares =
                    matches!(self.peek_token(), Some((id, _)) if id == self.types.declare_tok);
                let def = self.cx.definitions.last().expect("a relaxed field list is a type body");
                if word.is_some_and(|id| id == self.types.drop_ || def.slots.contains(&id)) {
                    self.cx.pos = start;
                    return Err(if declares {
                        ParseError::Resolve(ResolveError::Shadowed(name.to_string()))
                    } else {
                        ParseError::SlotFillNeedsShare
                    });
                }
            }
            // `name := T ?` declares the field's type through the hole `?`
            // built; a bare name leaves the type slot undefined. A default,
            // `size := u64 0`, is a constant: the field's dyad is that value,
            // and each new node starts with it in the field's slot.
            let mut default = std::ptr::null_mut();
            let mut owns_node = false;
            let logos = if self.consume_token(self.types.declare_tok) {
                let outer = std::mem::replace(&mut self.cx.field_hole, relaxed);
                let value = self.parse_expression_cell();
                self.cx.field_hole = outer;
                let cell = value?;
                let value = cell.dyad;
                owns_node = cell.owning;
                if cell.hole {
                    // SAFETY: `value` is the place `?` just built.
                    unsafe { (*value).ty }
                } else {
                    // SAFETY: `value` is a reduced dyad just parsed.
                    let held = unsafe { self.types.through(value) };
                    if held == self.types.unknown {
                        std::ptr::null_mut() // `name := ?`: no type yet
                    } else if relaxed
                        // SAFETY: as above.
                        && unsafe {
                            matches!(
                                crate::identities::read::read_kind(self.types, held),
                                crate::identities::read::Read::Scalar(_)
                            ) && !crate::dyad::is_place((*held).value)
                        }
                    {
                        // SAFETY: as above.
                        let (ty, value) = unsafe { ((*held).ty, (*held).value) };
                        default = value;
                        ty
                    } else {
                        self.cx.pos = start;
                        return Err(ParseError::BadDeclaredType);
                    }
                }
            } else {
                // A bare field name stands alone on its line: `x = 5` in a type
                // body writes no slot `type` declares.
                let alone = self.at_close()
                    || matches!(
                        self.peek_token(),
                        Some((id, _)) if id == self.types.sep_
                    );
                if relaxed && !alone {
                    self.cx.pos = start;
                    return Err(match word {
                        None => ParseError::Resolve(ResolveError::Unknown(name.to_string())),
                        Some(_) => ParseError::TypeBodyLine,
                    });
                }
                std::ptr::null_mut()
            };
            // An `own` parameter consumes its argument, which no call does yet.
            // SAFETY: `logos` is null or a type node.
            let owning = !logos.is_null()
                && unsafe {
                    crate::identities::numtype::is_pointer_type(logos)
                        && !crate::identities::meta::destructor_of(logos).is_null()
                };
            if !relaxed && (owning || owns_node) {
                self.cx.pos = start;
                return Err(ParseError::OwnParameterNotInSeed);
            }
            let field = self.rt.store.alloc_raw(logos, default);
            // The field's name is not stored on the record: declaring it puts
            // a binding in the one name index (DESIGN ›Name resolution is scope-filtered‹).
            let binding = if relaxed {
                // One block, one no-shadowing rule: a field is checked against
                // the `share` members too, which live in the type's own scope.
                let body =
                    self.cx.definitions.last().expect("a relaxed field list is a type body").scope;
                if self.cx.scopes.declared_in(self.trie, name, body).map_err(ParseError::Resolve)? {
                    self.cx.pos = start;
                    return Err(ParseError::Resolve(ResolveError::Shadowed(name.to_string())));
                }
                self.declare_field_name(name, field, start)?
            } else {
                self.declare_name(name, field, start)?
            };
            if let Some(gate) = gate {
                self.add_gate(binding, gate)?;
            }
            if owns_node {
                self.add_gate(binding, self.types.own_)?;
                if let Some(def) = self.cx.definitions.last_mut() {
                    def.owns_node_field = true;
                }
            }
            fields.push(field);
            if !self.consume_separator() {
                break;
            }
        }
        Ok(())
    }

    /// `share name := value` in a type body: one place stored with the type,
    /// declared in the body scope (DESIGN ›Two muts, and the storage
    /// partition‹). The fields and the members stay one scope for no-shadowing.
    fn share_member(&mut self, at: usize) -> Result<(), ParseError> {
        let body = match self.cx.definitions.last() {
            Some(def) => def.scope,
            None => {
                self.cx.pos = at;
                return Err(ParseError::ShareMisplaced);
            }
        };
        if self.consume_token(self.types.declare_tok) {
            self.cx.pos = at;
            return Err(ParseError::ShareNeedsDeclaration);
        }
        let field_scope = self.cx.scopes.pop().expect("the field list's scope is open");
        self.cx.last_declared = std::ptr::null_mut();
        let outer_member = self.cx.member_fn_depth.replace(self.cx.frames.len());
        let items = self.share_line();
        self.cx.member_fn_depth = outer_member;
        self.cx.scopes.push(field_scope);
        // The line's one declaration, a member or a slot fill, with its prose
        // lifted out beside it; anything else is not what `share` marks.
        let mut declared = None;
        for item in items? {
            // SAFETY: `item` is a reduced dyad just parsed.
            let ty = unsafe { (*item).ty };
            if ty == self.types.comment_ {
                // SAFETY: the pending bindings were minted by this parser's declares.
                unsafe { self.cx.scopes.settle_item(body, item) };
            } else if ty == self.types.declare_ && declared.is_none() {
                declared = Some(item);
            } else {
                self.cx.pos = at;
                return Err(ParseError::ShareNeedsDeclaration);
            }
        }
        let Some(item) = declared else {
            self.cx.pos = at;
            return Err(ParseError::ShareNeedsDeclaration);
        };
        {
            // SAFETY: a declaration that fills no slot is `:=`'s, its lhs the binding.
            let name = unsafe { Binding::spelling(crate::identities::declare::binding_of(item)) };
            if self
                .cx
                .scopes
                .declared_in(self.trie, &name, field_scope)
                .map_err(ParseError::Resolve)?
            {
                self.cx.pos = at;
                return Err(ParseError::Resolve(ResolveError::Shadowed(name)));
            }
            let binding = self.cx.last_declared;
            if binding.is_null() {
                self.cx.pos = at;
                return Err(ParseError::ShareNeedsDeclaration);
            }
            self.add_gate(binding, self.types.share_)?;
        }
        // SAFETY: the pending bindings were minted by this parser's declares.
        unsafe { self.cx.scopes.settle_item(body, item) };
        Ok(())
    }

    /// The first `parse_next` lexes and constructs the segment and queues
    /// what it yielded; the rest are taken while queued, before anything
    /// further is lexed, so none is left for the enclosing body's loop.
    fn share_line(&mut self) -> Result<Vec<DyadPtr>, ParseError> {
        let mut items = Vec::new();
        let Some(first) = self.parse_next() else {
            return Ok(items);
        };
        items.push(first?);
        while !self.cx.queued.is_empty() {
            items.push(self.parse_next().expect("a queued item comes out first")?);
        }
        Ok(items)
    }

    /// A slot fill in a type body, `parse = (…)` or `drop = (…)`, parsed over
    /// the body's scope from the slot word at `start`.
    fn slot_line(&mut self, start: usize) -> Result<(), ParseError> {
        let body = self.cx.definitions.last().expect("a slot line is in a type body").scope;
        self.cx.pos = start;
        let field_scope = self.cx.scopes.pop().expect("the field list's scope is open");
        let items = self.share_line();
        self.cx.scopes.push(field_scope);
        let mut filled = false;
        for item in items? {
            // SAFETY: `item` is a reduced dyad just parsed.
            let ty = unsafe { (*item).ty };
            let fill = ty == self.types.declare_ && self.is_slot_fill(item);
            if !(fill && !filled || ty == self.types.comment_) {
                self.cx.pos = start;
                return Err(ParseError::TypeBodyLine);
            }
            filled |= fill;
            // SAFETY: the pending bindings were minted by this parser's declares.
            unsafe { self.cx.scopes.settle_item(body, item) };
        }
        Ok(())
    }

    fn at_end(&mut self) -> bool {
        self.skip_whitespace();
        if self.cx.feed.is_some() {
            return self.feed_index().is_none();
        }
        self.cx.pos >= self.cx.source.len()
    }

    fn at_prose(&mut self) -> bool {
        self.skip_whitespace();
        self.cx.source.as_bytes().get(self.cx.pos) == Some(&b'#')
    }

    /// Past each comment at the cursor, the line form to its newline and the
    /// string form, `# «…»`, past its quote.
    fn skip_prose(&mut self) {
        let source = self.cx.source;
        let bytes = source.as_bytes();
        while self.at_prose() {
            self.cx.pos += 1;
            while matches!(bytes.get(self.cx.pos), Some(b' ' | b'\t')) {
                self.cx.pos += 1;
            }
            if source[self.cx.pos..].starts_with('«') {
                match self.cx.scopes.lex(self.trie, &source[self.cx.pos..]) {
                    Ok(r) => self.cx.pos += r.matched,
                    Err(_) => return,
                }
            } else {
                while bytes.get(self.cx.pos).is_some_and(|&b| b != b'\n') {
                    self.cx.pos += 1;
                }
            }
        }
    }

    /// The comments at the cursor, settled into the body beside the line after them.
    fn lift_prose(&mut self) -> Result<(), ParseError> {
        let body = self.cx.definitions.last().expect("read inside a type body").scope;
        while self.at_prose() {
            self.cx.pos += 1;
            let node = self.comment_after_hash()?;
            // SAFETY: `node` is the comment node just built.
            unsafe { self.cx.scopes.settle_item(body, node) };
        }
        Ok(())
    }

    /// The next words are `share` and a slot word or `drop`; nothing is consumed.
    fn at_share_slot(&mut self) -> bool {
        let at = self.cx.pos;
        let word = self.lex_spelling().and_then(|(start, len)| {
            let source = self.cx.source;
            self.cx.scopes.resolve(self.trie, &source[start..start + len]).ok().map(|r| r.identity)
        });
        let slot = word == Some(self.types.share_) && self.at_slot_word();
        self.cx.pos = at;
        slot
    }

    /// The next word is a slot word or `drop`; nothing is consumed.
    fn at_slot_word(&mut self) -> bool {
        let at = self.cx.pos;
        let word = self.lex_spelling().and_then(|(start, len)| {
            let source = self.cx.source;
            self.cx.scopes.resolve(self.trie, &source[start..start + len]).ok().map(|r| r.identity)
        });
        self.cx.pos = at;
        let def = self.cx.definitions.last().expect("read inside a type body");
        word.is_some_and(|id| id == self.types.drop_ || def.slots.contains(&id))
    }

    /// A slot fill's item declares the definition's own marker.
    fn is_slot_fill(&self, item: DyadPtr) -> bool {
        // SAFETY: `item` is a declare node from the store.
        let declared = unsafe { crate::identities::declare::declared_of(item) };
        self.cx
            .definitions
            .last()
            .is_some_and(|def| def.slots.contains(&declared) || def.drop_marker == declared)
    }

    /// One body for one level (DESIGN ›A type body describes one level‹): its
    /// lines fill the type's slots, declare its values' fields, and declare
    /// its `share` members; every other line must be prose, since nothing runs a type body later.
    pub fn parse_type_body(&mut self, id: DyadPtr) -> Result<DyadPtr, ParseError> {
        if self.cx.runtime_depth > 0 {
            return self.hold_type_body();
        }
        // The declaration whose whole right side is this `type (…)` names it.
        let own_name = match (self.cx.filling.last(), self.cx.filling_at.last()) {
            (Some(&binding), Some(&at))
                if self.cx.source.get(at..self.cx.pos).is_some_and(|t| t.trim() == "type") =>
            {
                binding
            }
            _ => std::ptr::null_mut(),
        };
        self.expect_open()?;
        let depth = self.cx.scopes.depth();
        // The slot words are the fields `type` declares (identities/type.logos):
        // names on the body's lines and nowhere outside them, in a scope of
        // their own so that a member read through the type never finds them.
        // Each is a marker of this definition, declared against its siblings
        // alone, so that a body inside another's shadows its words.
        let words = self.open_scope();
        let slots = SLOT_NAMES.map(|_| {
            let head = crate::identities::meta::record(
                self.rt.store,
                crate::identities::meta::TYPEREC_TAG,
                crate::identities::meta::prec::INERT,
            );
            self.rt.store.alloc_raw(self.types.type_, head)
        });
        for (name, &marker) in SLOT_NAMES.iter().zip(&slots) {
            let binding = self.mint_binding(marker, words, name.as_bytes());
            // SAFETY: `binding` was minted by `Binding::alloc` just above.
            if let Err(e) = unsafe { self.cx.scopes.declare_field(self.trie, name, binding) } {
                self.cx.scopes.pop();
                return Err(ParseError::Resolve(e));
            }
        }
        let head = crate::identities::meta::record(
            self.rt.store,
            crate::identities::meta::TYPEREC_TAG,
            crate::identities::meta::prec::INERT,
        );
        let drop_marker = self.rt.store.alloc_raw(self.types.type_, head);
        let self_type = self.rt.store.alloc_raw(id, std::ptr::null_mut());
        let scope = self.open_scope();
        self.cx.definitions.push(OpenType {
            scope,
            slots,
            drop_marker,
            parse_rank: crate::identities::meta::prec::APPLY,
            lex_rank: None,
            assoc: Assoc::Left,
            ctor: std::ptr::null_mut(),
            run_body: std::ptr::null_mut(),
            this_param: std::ptr::null_mut(),
            in_parse: false,
            tape_param: std::ptr::null_mut(),
            binding: own_name,
            read_receiver: false,
            wrote_receiver: false,
            block: (scope, Vec::new()),
            instances_drop: std::ptr::null_mut(),
            owns_node_field: false,
            self_type,
        });
        // A `fn` literal on a slot's right side must not claim the enclosing
        // declaration's placeholder.
        let suppressed = self.take_pending_fn();
        // The body's declarations are pending until its close: a type is
        // comptime and its members are stored at the definition. Everything
        // before the body runs first, and a failure among its lines is the body's.
        let outer = self.drain();
        self.cx.open.push(OpenScope::default());
        let saved_depth = std::mem::replace(&mut self.cx.runtime_depth, 0);
        let lines = outer.and_then(|()| self.parse_field_list(true, None)).and_then(|layout| {
            self.drain().map(|()| layout).map_err(|e| match e {
                ParseError::Run(r) => {
                    ParseError::TypeBodyFailed(Box::new(crate::report::run_message(&r)))
                }
                other => other,
            })
        });
        self.cx.runtime_depth = saved_depth;
        let defers = self.cx.open.pop().expect("pushed above").defers;
        self.restore_pending_fn(suppressed);
        let def = self.cx.definitions.pop().expect("pushed above");
        while self.cx.scopes.depth() > depth {
            self.cx.scopes.pop();
        }
        let (field_scope, fields, size_bytes) = lines?;
        if !defers.is_empty() {
            return Err(ParseError::DeferInTypeBody);
        }
        self.expect_close()?;
        let layout = crate::identities::meta::record_layout(
            self.rt.store,
            field_scope,
            fields,
            size_bytes,
            scope,
            def.parse_rank,
            def.assoc,
        );
        let node = def.self_type;
        // SAFETY: `node` was minted at the open with no record; nothing reads one until now.
        unsafe { (*node).value = layout.cast() };
        if let Some(rank) = def.lex_rank {
            // The rank is the name's, not the type's: it goes on the binding of
            // the declaration this body is the value of.
            let Some(&binding) = self.cx.filling.last() else {
                return Err(ParseError::LexRankNeedsName);
            };
            // SAFETY: `binding` is a binding dyad `:=` minted before driving its value, live for the whole drive.
            unsafe { Binding::set_lex_rank(binding, rank) };
        }
        if !def.ctor.is_null() {
            // SAFETY: `node` was just built; nothing has read its slot.
            unsafe { crate::identities::meta::install_constructor(node, def.ctor) };
        }
        if !def.run_body.is_null() {
            // SAFETY: `node` was just built; `def.run_body` is the `lex` node the fill built.
            unsafe { crate::identities::meta::install_run_body(node, def.run_body) };
        }
        // A type whose fields own what they point to writes the teardown that frees it
        // (DESIGN ›Explicit heap, and no implicit destruction‹).
        // SAFETY: `fields` is the block's array node, its items field dyads typed null or by a type node.
        let owning_field = unsafe {
            crate::identities::array::items(fields).iter().any(|&f| {
                let ty = (*f).ty;
                !ty.is_null()
                    && crate::identities::numtype::is_pointer_type(ty)
                    && !crate::identities::meta::destructor_of(ty).is_null()
            })
        };
        if (owning_field || def.owns_node_field) && def.instances_drop.is_null() {
            return Err(ParseError::OwningFieldNeedsDrop);
        }
        if !def.instances_drop.is_null() {
            // SAFETY: `node` was just built with a record layout; the drop is the fill's fn.
            unsafe { crate::identities::meta::install_instances_drop(node, def.instances_drop) };
        }
        Ok(node)
    }

    /// A `type (…)` in a body that runs later reads that run's values, so it is
    /// lexed now and built each time it runs (DESIGN ›Deferral is authored‹).
    fn hold_type_body(&mut self) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let (start, len) = self.body_text_extent()?;
        let source = self.cx.source;
        let text = crate::identities::string::build_text(
            self.rt.store,
            types.string_,
            &source.as_bytes()[start - 1..start + len + 1],
        );
        // SAFETY: `text` is the string node just built; its bytes live for the store.
        let held = unsafe { crate::identities::string::text(text) };
        let held = std::str::from_utf8(held).expect("copied from the source text");
        let mut fragment = self.lex_body_fragment(held)?;
        // A slot word of a type around this one names that type's slot; the held body's
        // own are declared when it is built, so its cells wait to be resolved then.
        for (i, cell) in fragment.cells().iter().enumerate() {
            let id = cell.identity(types);
            if !cell.is_fresh() && self.cx.definitions.iter().any(|d| d.slots.contains(&id)) {
                let fresh = self.rt.store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
                fragment.set_dyad(i as isize, fresh);
            }
        }
        let cells = Box::into_raw(Box::new(fragment));
        let scope = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        Ok(crate::identities::held_type::build(
            self.rt.store,
            types,
            text,
            cells,
            scope,
            self.cx.frames.len(),
        ))
    }

    /// Build the held `type (…)` `node` now, inside the run that reached it: its
    /// cells constructed over the scopes open where it was written, its reads
    /// of that function's names reading this call's frame. Everything the
    /// pass had open is set aside and restored.
    ///
    /// # Safety
    /// `node` must be a held type node from the store; the call it was written
    /// in must be the innermost activation.
    unsafe fn construct_held_type(
        &mut self,
        node: DyadPtr,
    ) -> Result<DyadPtr, crate::run::RunError> {
        let (text, cells, scope, depth) = crate::identities::held_type::parts(node);
        let bytes = crate::identities::string::text(text);
        let text: &'a str = std::str::from_utf8(bytes).expect("copied from the source text");
        let mut cells = (*cells).cells();
        for cell in &mut cells {
            if cell.is_fresh() {
                cell.dyad = self.rt.store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
            }
        }
        let mut chain = Vec::new();
        let mut at = scope;
        while !at.is_null() {
            chain.push(at);
            at = crate::identities::scope::parent_of(at);
        }
        let mut nested = ScopeStack::new();
        for &scope in chain.iter().rev() {
            nested.push(scope);
        }
        let base_depth = nested.depth();
        // One frame per function open where the body was written, so a read of
        // that function's names is no capture; nothing is claimed in them.
        let frames = (0..depth)
            .map(|_| OpenFn {
                size: 0,
                below: 0,
                outer: Vec::new(),
                open_below: 0,
                returns: Vec::new(),
            })
            .collect();

        let cx = Context {
            frames,
            held_depth: depth,
            feed: Some(Feed { cells, next: 0, base_depth, in_run_order: true }),
            ..Context::nested(&self.cx, text, nested)
        };
        let type_ = self.types.type_;
        let g = self.enter(cx);
        let built = g.0.parse_type_body(type_);
        let built_pos = g.0.cx.pos;
        drop(g);

        // A type of node values minted while a run type's parse runs is the value that type's
        // node makes, so a bracket taken into its place is built by one; stand-in for #152.
        if let Ok(ty) = built {
            let reader = self.cx.reader;
            if crate::identities::meta::is_record_type(reader)
                && !crate::identities::meta::run_body_of(reader).is_null()
                && crate::identities::meta::is_node_valued(ty, self.types.fn_type)
                && crate::identities::meta::is_record_type(ty)
            {
                // SAFETY: `ty` carries the record its body just built; `reader` is a type node.
                unsafe { crate::identities::meta::install_bracket_builder(ty, reader) };
            }
        }
        built.map_err(|e| {
            crate::run::RunError::MintFailed(Box::new(crate::report::render(
                "type body",
                text,
                built_pos,
                &crate::report::parse_message(&e),
            )))
        })
    }

    /// A use of one of the slot words, or of `drop`; whether the fill reaches
    /// a type being defined is `filling_definition`'s question.
    ///
    /// # Safety
    /// `target` must be a dyad from the store.
    pub(crate) unsafe fn slot_of(&self, target: DyadPtr) -> Option<SlotKind> {
        // The binding of the word's use, or the identity itself when its
        // constructor stood aside (`drop`).
        // SAFETY: `target` is a reduced dyad from the store.
        unsafe {
            let id = if (*target).ty == self.types.binding_ {
                Binding::read(target).dyad
            } else {
                target
            };
            if id == self.types.drop_ {
                return Some(SlotKind::Drop);
            }
            self.cx
                .definitions
                .iter()
                .rev()
                .find_map(|def| def.slots.iter().position(|&m| m == id))
                .map(SlotKind::of)
        }
    }

    /// The current scope is the innermost open definition's own (DESIGN ›The
    /// constructor is a field‹): a `parse_rank = 3` inside a constructor's
    /// body is that function's own business, refused.
    pub(crate) fn filling_definition(&self) -> bool {
        self.cx.definitions.last().is_some_and(|def| self.cx.scopes.current() == Some(def.scope))
    }

    /// A literal molds to f64 without running; a concrete expression runs
    /// now, after everything parsed before it (a rank may read a variable).
    /// Anything else is `NonComptimeRank`.
    fn rank_value(&mut self, value: DyadPtr, read: DyadPtr) -> Result<f64, ParseError> {
        use crate::identities::numtype::NumType;
        // SAFETY: `value` is a reduced dyad from the store.
        let nt = match unsafe { crate::identities::numtype_of(self.types, value) } {
            crate::identities::Operand::Literal => None,
            crate::identities::Operand::Concrete(nt) => Some(nt),
            _ => return Err(ParseError::NonComptimeRank),
        };
        let bits = match nt {
            None => crate::identities::rational::mold_to(read, NumType::F64)
                .ok_or(ParseError::UncomputableLiteral)?,
            Some(_) => {
                self.drain()?;
                // SAFETY: `value` is a reduced dyad from the store.
                unsafe { self.run_on_pass(value) }.map_err(|_| ParseError::NonComptimeRank)?
            }
        };
        Ok(match nt {
            None | Some(NumType::F64) => f64::from_bits(bits as u64),
            Some(NumType::F32) => f64::from(f32::from_bits(bits as u32)),
            Some(_) => bits as f64,
        })
    }

    /// A slot `type` declared is filled with `=` (DESIGN ›The constructor is
    /// a field‹): the parse_rank must be known at the definition; a body slot
    /// takes a bracket, `slot_body_fill`'s; the fill is a silent statement.
    ///
    /// # Safety
    /// `value` must be a dyad from the store.
    pub(crate) unsafe fn slot_fill(
        &mut self,
        kind: SlotKind,
        target: DyadPtr,
        value: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        if matches!(kind, SlotKind::Parse | SlotKind::Run | SlotKind::Drop) {
            return Err(ParseError::SlotNeedsBody(kind));
        }
        // SAFETY: `value` is a reduced dyad from the store.
        let read = unsafe { types.through(value) };
        // Computed before the open definition is borrowed, since computing it
        // may run what stands before it.
        let rank = match kind {
            SlotKind::ParseRank | SlotKind::LexRank => Some(self.rank_value(value, read)?),
            _ => None,
        };
        let def = self.cx.definitions.last_mut().expect("slot_of found an open definition");
        match kind {
            SlotKind::ParseRank => {
                if let Some(rank) = rank {
                    def.parse_rank = rank;
                }
            }
            SlotKind::LexRank => {
                if let Some(rank) = rank {
                    def.lex_rank = Some(rank);
                }
            }
            SlotKind::Associativity => {
                let assoc = if read == types.left_ {
                    Assoc::Left
                } else if read == types.right_ {
                    Assoc::Right
                } else {
                    return Err(ParseError::BadAssociativity);
                };
                def.assoc = assoc;
            }
            SlotKind::Parse | SlotKind::Run | SlotKind::Drop => {
                unreachable!("body slots and `drop` returned above")
            }
        }
        Ok(self.slot_declare(kind, target, value))
    }

    /// The line's item for a slot fill: a declare node over the slot word's
    /// use and the right side, declaring the definition's marker, which is how
    /// the body's close tells a fill from a member declaration.
    fn slot_declare(&mut self, kind: SlotKind, target: DyadPtr, rhs: DyadPtr) -> DyadPtr {
        let types = self.types;
        let def = self.cx.definitions.last().expect("a slot is filled inside a definition");
        let marker = match kind {
            SlotKind::Drop => def.drop_marker,
            kind => def.slots[kind as usize],
        };
        crate::identities::declare::build(
            self.rt.store,
            types.declare_,
            types.ops.declare_,
            target,
            rhs,
            marker,
        )
    }

    /// A slot body read bare, no parameter list (DESIGN ›Execution is function
    /// application‹): `parse` over a hidden `tape`/node record, `run` held as
    /// its lexed tape and constructed per field-type set (DESIGN ›Deferral is authored‹).
    pub(crate) fn slot_body_fill(
        &mut self,
        kind: SlotKind,
        target: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        match kind {
            SlotKind::Parse => {
                let at = self.cx.pos;
                // `tape`, centred on the appearance, and unnamed, the node `tape[0]`
                // reaches: fresh when the type itself appeared, the value when a value did.
                let (input, params) = self.hidden_param_record(
                    &[(Some("tape"), types.tape.parsing_tape), (None, types.dyad_)],
                    at,
                )?;
                let def = self.cx.definitions.last_mut().expect("checked above");
                def.this_param = params[1];
                def.tape_param = params[0];
                def.in_parse = true;
                // SAFETY: `input` was just built, its parameters unplaced; no declaration's placeholder is being filled.
                let f = unsafe {
                    self.fn_over_body(types.fn_type, input, types.void_, std::ptr::null_mut())
                };
                let def = self.cx.definitions.last_mut().expect("checked above");
                def.this_param = std::ptr::null_mut();
                def.in_parse = false;
                def.ctor = f?;
                let f = def.ctor;
                Ok(self.slot_declare(SlotKind::Parse, target, f))
            }
            SlotKind::Run => {
                // The body with its brackets is copied into the store (a
                // source may not outlive the type, the store does) and lexed
                // once, now, against the scopes open here (DESIGN ›Deferral is authored‹).
                let (start, len) = self.body_text_extent()?;
                let source = self.cx.source;
                let text = crate::identities::string::build_text(
                    self.rt.store,
                    types.string_,
                    &source.as_bytes()[start - 1..start + len + 1],
                );
                // SAFETY: `text` is the string node just built; its bytes live for the store.
                let held = unsafe { crate::identities::string::text(text) };
                let held = std::str::from_utf8(held).expect("copied from the source text");
                let fragment = self.lex_body_fragment(held)?;
                let cells = Box::into_raw(Box::new(fragment));
                let body = crate::identities::run_body::build(self.rt.store, types, text, cells);
                self.cx.definitions.last_mut().expect("checked above").run_body = body;
                Ok(self.slot_declare(SlotKind::Run, target, body))
            }
            SlotKind::Drop => {
                // One parameter, unnamed: the value an owner's teardown hands in.
                let at = self.cx.pos;
                let (input, params) = self.hidden_param_record(&[(None, types.dyad_)], at)?;
                let def = self.cx.definitions.last_mut().expect("checked above");
                def.this_param = params[0];
                // SAFETY: `input` was just built, its parameter unplaced; no declaration's placeholder is being filled.
                let f = unsafe {
                    self.fn_over_body(types.fn_type, input, types.void_, std::ptr::null_mut())
                };
                let def = self.cx.definitions.last_mut().expect("checked above");
                def.this_param = std::ptr::null_mut();
                def.instances_drop = f?;
                let f = def.instances_drop;
                Ok(self.slot_declare(SlotKind::Drop, target, f))
            }
            _ => unreachable!("`=` reads a bare body for `parse`, `run` and `drop` only"),
        }
    }

    /// A parameter record no text spelled: each name declared in its own
    /// scope and checked against every open scope, since the body reopens it;
    /// value slots left null for `fn_over_body` to place. `at` is the error position.
    fn hidden_param_record(
        &mut self,
        params: &[(Option<&str>, DyadPtr)],
        at: usize,
    ) -> Result<(DyadPtr, Vec<DyadPtr>), ParseError> {
        let scope = self.open_scope();
        let mut fields = Vec::with_capacity(params.len());
        let mut declared = Ok(());
        for &(name, ty) in params {
            let field = self.rt.store.alloc_raw(ty, std::ptr::null_mut());
            // A parameter with no name is a place the body reaches another
            // way: a receiver, or a run body's field of type `type`.
            if let Some(name) = name {
                declared = declared.and_then(|()| self.declare_name(name, field, at).map(|_| ()));
            }
            fields.push(field);
        }
        self.cx.scopes.pop();
        declared?;
        let fields_arr = crate::identities::array::build(self.rt.store, self.types.array_, &fields);
        // SAFETY: each `ty` is a type node from the store.
        let size_bytes = fields.iter().map(|&f| unsafe { field_width((*f).ty) }).sum();
        let record = crate::identities::meta::record_layout(
            self.rt.store,
            scope,
            fields_arr,
            size_bytes,
            std::ptr::null_mut(),
            crate::identities::meta::prec::APPLY,
            Assoc::Left,
        );
        Ok((self.rt.store.alloc_raw(self.types.type_, record.cast()), fields))
    }

    /// The cell before is an unbuilt `.` or `:`, whose right side is a member's spelling.
    fn after_tight_read(&self, tape: &ParsingTape) -> bool {
        tape.last().is_some_and(|l| {
            !l.constructed && {
                let id = self.cell_identity(l);
                id == self.types.dot_ || id == self.types.colon_
            }
        })
    }

    /// Inside a `drop` or a `share` function a field of the value is named bare (DESIGN
    /// ›There is no `this`‹); fields come first, so an outer name never stands in for one.
    /// Inside a parse the same name is the checked error, the field going through `tape[0]`.
    fn names_own_field(&self, cell: &Cell) -> bool {
        let Some(def) = self.cx.definitions.last() else {
            return false;
        };
        if def.this_param.is_null() {
            return false;
        }
        let mut fields = ScopeStack::new();
        fields.push(def.block.0);
        fields
            .resolve(self.trie, cell.spelling())
            .is_ok_and(|r| r.matched == cell.len && def.block.1.contains(&r.identity))
    }

    /// `tape[0]` inside a parse body: the cell the parse was woken on, holding its node.
    fn is_node_cell(&self, lhs: DyadPtr) -> bool {
        let Some(def) = self.cx.definitions.last().filter(|d| d.in_parse) else {
            return false;
        };
        // SAFETY: `lhs` is a reduced dyad from the store; a slot node carries `[tape, k]`.
        unsafe {
            crate::identities::tape::is_cell_read(self.types, lhs)
                && crate::identities::tape::cell_key(self.types, lhs).is_some_and(|(t, k, l)| {
                    t == def.tape_param && k == 0 && l == crate::identities::tape::Line::Cell
                })
        }
    }

    /// `tape[0]:type = T`: the fresh node placed in the parse's own cell, from where its
    /// fields are written by name (DESIGN ›There is no `this`‹). The seed stamps only the
    /// type being defined.
    ///
    /// # Safety
    /// `target` must be a `tape.cell_type` node from the store; `value` a reduced dyad.
    pub(crate) unsafe fn stamp(
        &mut self,
        target: DyadPtr,
        value: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let ops = (*target).value as *const DyadPtr;
        let cell = crate::identities::tape::build_slot(self.rt.store, types, *ops, *ops.add(1));
        if !self.is_node_cell(cell) || !(*ops.add(2)).is_null() {
            return Err(ParseError::StampOutsideParse);
        }
        let def = self.cx.definitions.last().expect("is_node_cell found a parse body");
        if value != def.self_type {
            return Err(ParseError::StampOtherType);
        }
        let node = def.this_param;
        Ok(crate::identities::tape::build_write(self.rt.store, types, cell, node))
    }

    /// A bare call of a `share` function that works on a value takes the value the calling
    /// body is about (DESIGN ›There is no `this`‹): a `run`'s node, a `drop`'s or `share`
    /// function's own, or in a parse none, so only one that reads no field of its value is
    /// called bare there.
    ///
    /// # Safety
    /// `callee` must be a reduced dyad from the store.
    unsafe fn with_receiver(
        &mut self,
        callee: DyadPtr,
        args: Vec<DyadPtr>,
    ) -> Result<Vec<DyadPtr>, ParseError> {
        let f = self.types.through(callee);
        if !self.takes_this(f) {
            return Ok(args);
        }
        let name = || {
            let spelled = if (*callee).ty == self.types.binding_ {
                Binding::spelling(callee)
            } else {
                String::new()
            };
            Box::new(spelled)
        };
        let bits = fn_receiver(self.types, f);
        let writes = bits & RECEIVER_WRITES != 0;
        let receiver = match self.cx.definitions.last_mut().filter(|d| !d.this_param.is_null()) {
            Some(def) => {
                if def.in_parse && bits & RECEIVER_READS != 0 {
                    return Err(ParseError::ShareFnNeedsValue(name()));
                }
                def.wrote_receiver |= writes && !def.in_parse;
                def.this_param
            }
            None => match &self.cx.run_fields {
                Some((ty, places)) => {
                    crate::identities::this::build_pack(self.rt.store, self.types, *ty, places)
                }
                None => return Err(ParseError::ShareFnNeedsValue(name())),
            },
        };
        let mut with = vec![receiver];
        with.extend(args);
        Ok(with)
    }

    /// The slot of the node being built that the field `f` names: `f`
    /// resolves in the fields' own scope alone, and its index among the fields
    /// declared above is the slot.
    fn this_field(&mut self, name: &str, at: usize) -> Result<DyadPtr, ParseError> {
        let def = self.cx.definitions.last().expect("this_param is set inside a definition");
        let (this, body) = (def.this_param, def.scope);
        let (scope, items) = (def.block.0, def.block.1.clone());
        let mut field_scope = ScopeStack::new();
        field_scope.push(scope);
        let resolved = field_scope.resolve(self.trie, name).ok();
        let field = resolved.as_ref().map(|r| r.identity);
        let index = field.and_then(|f| items.iter().position(|&x| x == f));
        let Some(index) = index else {
            // A `share` member is one place read through every node, so
            // `tape[0].element_type` is the member itself, known here.
            let mut members = ScopeStack::new();
            members.push(body);
            if let Ok(r) = members.resolve(self.trie, name) {
                return Ok(r.identity);
            }
            self.cx.pos = at;
            return Err(ParseError::NoSuchField(Box::new(name.to_string())));
        };
        if let Some(def) = self.cx.definitions.last_mut() {
            def.read_receiver |= !def.in_parse;
        }
        let k = self.scalar_value(crate::identities::numtype::NumType::U64, index as i64);
        let binding = resolved.as_ref().map_or(std::ptr::null_mut(), |r| r.binding);
        // Only the type's own parse fills a field through its default entry.
        let in_parse = self.cx.definitions.last().is_some_and(|d| d.in_parse);
        let fill = if in_parse { self.types.this.fill } else { std::ptr::null_mut() };
        // SAFETY: the field is a declaration dyad, its type null or a type node.
        let node = unsafe {
            crate::identities::this::build_field_read(
                self.rt.store,
                self.types,
                this,
                k,
                (*items[index]).ty,
                binding,
                fill,
            )
        };
        Ok(node)
    }

    /// The run of a node whose type holds its `run` as a lexed body (DESIGN
    /// ›Deferral is authored‹): the function for its field-type set, found on
    /// the type or built now; a field whose type is unknown leaves the slot empty.
    ///
    /// # Safety
    /// `node` must be the node `run_logos_ctor` minted for a type whose
    /// record carries a run body, `[field…, null, spec]`.
    pub(crate) unsafe fn resolve_specialization(
        &mut self,
        node: DyadPtr,
        at: usize,
        spelling: &str,
    ) -> Result<Option<DyadPtr>, ParseError> {
        let ty = (*node).ty;
        let held = crate::identities::meta::run_body_of(ty);
        if held.is_null() {
            return Ok(None);
        }
        let Some(key) = self.field_type_key(ty, node)? else {
            return Ok(None);
        };
        let mut spec = crate::identities::run_body::lookup(held, &key);
        if spec.is_null() {
            spec = self.construct_run_body(ty, held, &key, at, spelling)?;
        }
        crate::identities::run_body::set_spec(node, spec);
        // The node is a call of the function: a use of every outer name the
        // body reads, checked before a comptime node runs it.
        if let Err(e) = self.check_call_reads(spec) {
            self.cx.pos = at;
            return Err(e);
        }
        self.fold_comptime_node(ty, node, spec)
    }

    /// A node every value field of which is a literal is comptime, its
    /// constructor a partial evaluator (DESIGN ›Deferral is authored‹): it runs
    /// now and its literal stands in its place, so the result molds where it lands.
    ///
    /// # Safety
    /// As `resolve_specialization`; `spec` the node's function.
    unsafe fn fold_comptime_node(
        &mut self,
        ty: DyadPtr,
        node: DyadPtr,
        spec: DyadPtr,
    ) -> Result<Option<DyadPtr>, ParseError> {
        use crate::identities::read::{read_kind, Read};
        let types = self.types;
        let fields = crate::identities::array::items(crate::identities::meta::record_fields_of(ty));
        let slots = (*node).value as *const DyadPtr;
        for (i, &field) in fields.iter().enumerate() {
            if (*field).ty == types.type_ {
                continue;
            }
            let slot = types.through(*slots.add(i));
            let comptime = match read_kind(types, slot) {
                Read::Literal => true,
                Read::Scalar(_) => !crate::dyad::is_place((*slot).value),
                Read::Address => {
                    let viewed = (*slot).value as DyadPtr;
                    !viewed.is_null() && (*viewed).ty == types.rational
                }
                _ => false,
            };
            if !comptime {
                return Ok(None);
            }
        }
        let fields = (*spec).value as *const DyadPtr;
        if fields.is_null() {
            return Ok(None);
        }
        let out = *fields.add(FN_OUTPUT);
        let numeric = crate::identities::is_numtype_node(types, out);
        if !numeric && out != types.rational {
            return Ok(None);
        }
        let bits = self
            .run_on_pass(node)
            .map_err(|e| ParseError::ConstructorFailed(Box::new(crate::report::run_message(&e))))?;
        let folded = if numeric {
            self.scalar_value(crate::identities::numtype::of_type_node(out), bits)
        } else {
            bits as DyadPtr
        };
        Ok(Some(folded))
    }

    /// One type per instance field in order (DESIGN ›Execution is function
    /// application‹): a typed field's type, a `type` field's held type, or for
    /// an untyped hole the written value's type. `None` while one is unknown.
    ///
    /// # Safety
    /// As `resolve_specialization`; `ty` the node's type.
    unsafe fn field_type_key(
        &mut self,
        ty: DyadPtr,
        node: DyadPtr,
    ) -> Result<Option<Vec<DyadPtr>>, ParseError> {
        use crate::identities::{numtype_of, Operand};
        let types = self.types;
        let fields = crate::identities::array::items(crate::identities::meta::record_fields_of(ty));
        let slots = (*node).value as *mut DyadPtr;
        let mut key = Vec::with_capacity(fields.len());
        for (i, &field) in fields.iter().enumerate() {
            let declared = (*field).ty;
            let slot = *slots.add(i);
            if slot.is_null() {
                return Ok(None);
            }
            let entry = if declared == types.type_ {
                let held = types.through(slot);
                if !crate::identities::is_type_value(types, held) {
                    return Ok(None);
                }
                held
            } else if !declared.is_null()
                // A field typed as a bracket holds its lines, evaluated as an untyped one's.
                && declared != types.square_brackets
                && declared != types.scope
            {
                if matches!(numtype_of(types, slot), Operand::Literal) {
                    let lit = types.through(slot);
                    if declared == types.rational {
                        *slots.add(i) =
                            crate::identities::rational::box_literal(self.rt.store, types, lit);
                    } else if crate::identities::is_numtype_node(types, declared) {
                        *slots.add(i) = crate::identities::commit_literal_to(
                            self.rt.store,
                            types,
                            lit,
                            declared,
                        )?;
                    }
                }
                declared
            } else if crate::identities::rational::is_rational_value(types, slot) {
                types.rational
            } else if let Some(bracket) = self.bracket_type(slot) {
                if (*slot).ty != types.tape.bracket_arg {
                    *slots.add(i) =
                        crate::identities::tape::build_bracket_arg(self.rt.store, types, slot)?;
                }
                bracket
            } else {
                match numtype_of(types, slot) {
                    Operand::Concrete(nt) => types.numtypes[nt as usize],
                    Operand::Literal => {
                        // The literal's own type, boxed as the value the call passes.
                        let lit = types.through(slot);
                        *slots.add(i) =
                            crate::identities::rational::box_literal(self.rt.store, types, lit);
                        types.rational
                    }
                    Operand::Pointer(_) | Operand::NonNumeric => return Ok(None),
                }
            };
            key.push(entry);
        }
        Ok(Some(key))
    }

    /// The kind of bracket an untyped field holds, written or already wrapped to run per node.
    ///
    /// # Safety
    /// `slot` must be a dyad from the store.
    unsafe fn bracket_type(&self, slot: DyadPtr) -> Option<DyadPtr> {
        let types = self.types;
        let slot = types.through(slot);
        let bracket = if (*slot).ty == types.tape.bracket_arg {
            *((*slot).value as *const DyadPtr)
        } else {
            slot
        };
        let ty = (*bracket).ty;
        (ty == types.scope || ty == types.square_brackets).then_some(ty)
    }

    /// The function a held run body is for one field-type set: the cells lexed
    /// at the definition are constructed now, over the type's own scopes and one
    /// unnamed parameter per field. Entered on the type first, so a recursive use resolves to it.
    ///
    /// # Safety
    /// `ty` must carry a record whose run body is `held`; `key` one type per
    /// instance field.
    unsafe fn construct_run_body(
        &mut self,
        ty: DyadPtr,
        held: DyadPtr,
        key: &[DyadPtr],
        at: usize,
        spelling: &str,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let text = crate::identities::run_body::text_of(held);
        let mut cells = (*crate::identities::run_body::cells_of(held)).cells();
        // A fresh cell's dyad is the placeholder a `:=` in the body fills, so
        // each construction gets its own and the held cells stay as lexed.
        for cell in &mut cells {
            if cell.is_fresh() {
                cell.dyad = self.rt.store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
            }
        }
        let mut chain = Vec::new();
        let mut scope =
            crate::identities::scope::parent_of(crate::identities::meta::record_scope_of(ty));
        while !scope.is_null() {
            chain.push(scope);
            scope = crate::identities::scope::parent_of(scope);
        }
        let mut nested = ScopeStack::new();
        for &scope in chain.iter().rev() {
            nested.push(scope);
        }
        let base_depth = nested.depth();
        let spec = self.rt.store.alloc_raw(types.fn_type, std::ptr::null_mut());
        crate::identities::run_body::insert(self.rt.store, types, held, key, spec);

        let cx = Context {
            feed: Some(Feed { cells, next: 0, base_depth, in_run_order: false }),
            ..Context::nested(&self.cx, text, nested)
        };
        let g = self.enter(cx);
        let inner = g.0.construct_run_body_in(ty, key);
        let inner_pos = g.0.cx.pos;
        drop(g);

        match inner {
            Ok(f) => {
                (*spec).value = (*f).value;
                Ok(spec)
            }
            Err(e) => {
                crate::identities::run_body::remove(self.rt.store, types, held, key);
                self.cx.pos = at;
                Err(ParseError::RunBodyFailed {
                    name: spelling.to_string(),
                    rendered: crate::report::render(
                        &format!("run body of `{spelling}`"),
                        text,
                        inner_pos,
                        &crate::report::parse_message(&e),
                    ),
                })
            }
        }
    }

    /// Over the swapped-in state: the hidden parameter record, the return
    /// type (the type the set holds in the field named `output_type`, or `void`),
    /// and the body read as a function's.
    ///
    /// # Safety
    /// As `construct_run_body`.
    unsafe fn construct_run_body_in(
        &mut self,
        ty: DyadPtr,
        key: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let field_scope = crate::identities::meta::record_scope_of(ty);
        let fields =
            crate::identities::array::items(crate::identities::meta::record_fields_of(ty)).to_vec();
        // The parameters are the fields alone, in order, each under its field's name: a
        // node's operands are its fields, and its call passes exactly those.
        let names: HashMap<DyadPtr, String> = self
            .trie
            .bindings_in(field_scope)
            .into_iter()
            .map(|b| (Binding::read(b).dyad, Binding::spelling(b)))
            .collect();
        let mut params: Vec<(Option<&str>, DyadPtr)> = Vec::with_capacity(fields.len());
        let mut type_fields = Vec::new();
        for (i, &field) in fields.iter().enumerate() {
            let name = names.get(&field).map(String::as_str);
            let ty = if (*field).ty == types.type_ {
                // A field of type `type` names the type this set holds in it, so
                // `output_type 1` reads `i32 1`; its parameter place goes unnamed.
                type_fields.extend(name.map(|n| (n, key[i])));
                params.push((None, types.type_));
                continue;
            } else if key[i] == types.scope || key[i] == types.square_brackets {
                // A bare parameter: the bracket the node's field holds, read by its lines.
                std::ptr::null_mut()
            } else {
                key[i]
            };
            params.push((name, ty));
        }
        let mut scope = ScopeStack::new();
        scope.push(field_scope);
        let output = scope
            .resolve(self.trie, "output_type")
            .ok()
            .and_then(|r| fields.iter().position(|&f| f == r.identity))
            .filter(|&i| (*fields[i]).ty == types.type_)
            .map_or(types.void_, |i| key[i]);
        let (input, places) = self.hidden_param_record(&params, 0)?;
        self.cx.run_fields = Some((ty, places));
        let param_scope = crate::identities::meta::record_scope_of(input);
        self.cx.scopes.push(param_scope);
        let declared =
            type_fields.iter().try_for_each(|&(n, t)| self.declare_name(n, t, 0).map(|_| ()));
        self.cx.scopes.pop();
        declared?;
        self.fn_over_body(types.fn_type, input, output, std::ptr::null_mut())
    }

    /// `(start, len)` of the text inside the `( … )` at the cursor, consumed
    /// with its brackets and constructed by nothing; the closer is found by
    /// lexing token by token, so a bracket inside a quote or a `#` comment is text.
    fn body_text_extent(&mut self) -> Result<(usize, usize), ParseError> {
        self.expect_open()?;
        let source = self.cx.source;
        let bytes = source.as_bytes();
        let start = self.cx.pos;
        let mut depth = 0usize;
        loop {
            self.skip_whitespace();
            if self.cx.pos >= bytes.len() {
                return Err(ParseError::UnclosedBracket);
            }
            if bytes[self.cx.pos] == b'#' {
                self.cx.pos += 1;
                while self.cx.pos < bytes.len() && matches!(bytes[self.cx.pos], b' ' | b'\t') {
                    self.cx.pos += 1;
                }
                // The line form ends at the newline; the string form, `# «…»`,
                // is the quote token the lexer reads next.
                if !source[self.cx.pos..].starts_with('«') {
                    while self.cx.pos < bytes.len() && bytes[self.cx.pos] != b'\n' {
                        self.cx.pos += 1;
                    }
                    continue;
                }
            }
            let r = self
                .cx
                .scopes
                .lex(self.trie, &source[self.cx.pos..])
                .map_err(ParseError::Resolve)?;
            let id = if r.fresh { std::ptr::null_mut() } else { r.identity };
            if id == self.types.open_ || id == self.types.open_sq_ {
                depth += 1;
            } else if id == self.types.close_ || id == self.types.close_sq_ {
                if depth == 0 {
                    let len = self.cx.pos - start;
                    self.cx.pos += r.matched;
                    return Ok((start, len));
                }
                depth -= 1;
            }
            self.cx.pos += r.matched;
        }
    }

    /// `fn ( params ) -> ret ( body )` (DESIGN ›A function's surface‹): the
    /// node is `[input, output, body, bcode, frame, outer]`. `declared` is the
    /// declaration's placeholder the signature publishes onto before the body parses.
    ///
    /// # Safety
    /// `declared` must be null or a placeholder dyad from the store that
    /// nothing has read a value from yet; the early signature is written through it.
    pub unsafe fn parse_fn(
        &mut self,
        fn_type: DyadPtr,
        declared: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        // A `share` function reached through a value takes that value
        // (DESIGN ›The constructor is a field‹): the `fn` takes it first.
        let member = !declared.is_null()
            && self.cx.member_fn_depth == Some(self.cx.frames.len())
            && self.cx.definitions.last().is_some_and(|d| d.this_param.is_null());
        let input = self.parse_record_taking(member.then_some(self.types.dyad_))?;
        self.expect_arrow()?;
        let output = {
            let items = self.drive_until_open(RightSide::ReturnType);
            let out = self.one_of(items?).map(|c| c.dyad).map_err(|e| match e {
                ParseError::Empty => ParseError::ExpectedReturnType,
                e => e,
            })?;
            // A named return type is a use of that name: its binding, read
            // through to the type it names.
            // SAFETY: `out` is a reduced dyad from the store.
            unsafe { self.types.through(out) }
        };
        if !member {
            // SAFETY: `input` was just built by `parse_record`; `declared` is the caller's placeholder.
            return unsafe { self.fn_over_body(fn_type, input, output, declared) };
        }
        // SAFETY: `input` is the record just built, the value its first field.
        let this = unsafe {
            crate::identities::array::items(crate::identities::meta::record_fields_of(input))[0]
        };
        let def = self.cx.definitions.last_mut().expect("checked above");
        def.this_param = this;
        def.read_receiver = false;
        def.wrote_receiver = false;
        // SAFETY: as above.
        let f = unsafe { self.fn_over_body(fn_type, input, output, declared) };
        let def = self.cx.definitions.last_mut().expect("still open");
        def.this_param = std::ptr::null_mut();
        let receiver = (u64::from(def.read_receiver) * RECEIVER_READS)
            | (u64::from(def.wrote_receiver) * RECEIVER_WRITES);
        if let (Ok(&f), true) = (f.as_ref(), receiver != 0) {
            let bytes = self.rt.store.alloc_bytes(&receiver.to_ne_bytes());
            let u64_ty = self.types.numtypes[crate::identities::NumType::U64 as usize];
            let leaf = self.rt.store.alloc_raw(u64_ty, bytes);
            // SAFETY: `f` is the node `fn_over_body` just built over its seven-slot record.
            unsafe { *((*f).value as *mut DyadPtr).add(FN_RECEIVER) = leaf };
        }
        f
    }

    /// The half of `parse_fn` after the signature, shared with a slot body
    /// read bare: the frame opened and the parameters placed in it, the body
    /// parsed deferred with the parameter scope reopened.
    ///
    /// # Safety
    /// `input` must be a record node whose parameters' value slots are still
    /// null; `declared` as for `parse_fn`.
    unsafe fn fn_over_body(
        &mut self,
        fn_type: DyadPtr,
        input: DyadPtr,
        output: DyadPtr,
        declared: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        // A call frame is an instance of its function, so a parameter resolves
        // to a frame slot as a local does (DESIGN ›Resolution is one rule‹). The
        // barrier begins at the current depth, so a lesser depth is outside the function.
        self.cx.frames.push(OpenFn {
            size: 0,
            below: self.cx.scopes.depth(),
            outer: Vec::new(),
            open_below: self.cx.open.len(),
            returns: Vec::new(),
        });
        let depth = self.cx.frames.len();
        // SAFETY: `input` is the record just built; each parameter's value slot is still the null `parse_record` left there.
        unsafe {
            let fields = crate::identities::meta::record_fields_of(input);
            for &param in crate::identities::array::items(fields) {
                let logos = (*param).ty;
                // A parameter's slot is sized by the rule that sizes a local; a
                // bare one holds the 8-byte container.
                let width = if logos.is_null() {
                    8
                } else {
                    crate::identities::read::place_layout(self.types, logos)
                        .map(|(_, w)| w)
                        .unwrap_or(8)
                };
                let frame = self.cx.frames.last_mut().expect("parse_fn just pushed a frame");
                let offset = frame.size;
                frame.size += width;
                (*param).value = crate::dyad::frame_place(depth, offset);
            }
        }

        if !declared.is_null() {
            let early = self.rt.store.alloc_operands(&[
                input,
                output,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ]);
            // SAFETY: `declared` is the just-declared placeholder; nothing has read it, and the fixpoint overwrites it.
            unsafe {
                (*declared).value = early;
            }
        }

        // The body is deferred (it runs at calls), so parse-time rebinding is
        // off inside.
        // SAFETY: `input` is the record just built; its record stores its scope.
        let scope = unsafe { crate::identities::meta::record_scope_of(input) };
        // Names declared outside the body may not be moved or dropped inside;
        // the parameters, in the scope pushed next, may (DESIGN ›A function's surface‹).
        self.cx.scopes.push_barrier();
        self.cx.scopes.push(scope);
        self.cx.runtime_depth += 1;
        let body = self.bracketed_body();
        self.cx.runtime_depth -= 1;
        self.cx.scopes.pop();
        self.cx.scopes.pop_barrier();
        let OpenFn { size: frame_size, outer, returns, .. } =
            self.cx.frames.pop().expect("parse_fn pushed a frame");
        let body = body?;

        // A comptime-rational tail commits to the declared return type here,
        // so `fn () -> i64 (…)` returns i64 rather than the i32 default.
        // SAFETY: `body`/`output` are valid dyads just built.
        let body =
            unsafe { crate::identities::commit_fn_body(self.rt.store, self.types, body, output)? };
        for r in returns {
            // SAFETY: `r` is a `return` node built in this body.
            unsafe { crate::identities::commit_fn_body(self.rt.store, self.types, r, output)? };
        }

        let frame = if frame_size == 0 {
            std::ptr::null_mut()
        } else {
            let bytes = self.rt.store.alloc_bytes(&(frame_size as u64).to_ne_bytes());
            let u64_ty = self.types.numtypes[crate::identities::NumType::U64 as usize];
            self.rt.store.alloc_raw(u64_ty, bytes)
        };
        let outer = if outer.is_empty() {
            std::ptr::null_mut()
        } else {
            crate::identities::array::build(self.rt.store, self.types.array_, &outer)
        };
        let value = self.rt.store.alloc_operands(&[
            input,
            output,
            body,
            std::ptr::null_mut(),
            frame,
            outer,
            std::ptr::null_mut(),
        ]);
        Ok(self.rt.store.alloc_raw(fn_type, value))
    }

    /// A `return` just built: inside a function it leaves every scope open
    /// since the body began, so it may not hand out a place their teardowns
    /// free, nor an owning value; the body's close commits it to the result type.
    ///
    /// # Safety
    /// `node` must be a `return` node `[value, op]` from the store.
    pub(crate) unsafe fn note_return(&mut self, node: DyadPtr) -> Result<(), ParseError> {
        let Some(frame) = self.cx.frames.last() else {
            return Ok(());
        };
        let types = self.types;
        let value = *((*node).value as *const DyadPtr);
        let place = types.through(value);
        if self.cx.open[frame.open_below..].iter().any(|s| s.owned.contains(&place)) {
            return Err(ParseError::OwningEscape);
        }
        if crate::identities::drop_model::is_owning_value(types, value) {
            return Err(ParseError::OwnershipAcrossReturn);
        }
        self.cx.frames.last_mut().expect("checked above").returns.push(node);
        Ok(())
    }

    /// `if cond then` with an optional `else else`: the node is
    /// `[cond, then, else]`, else null when absent, so an else-less `if` is a
    /// statement. Each branch is a bracket or the next expression, and a bare
    /// `else` binds to the nearest `if`; `if` opens no scope.
    pub fn parse_if(&mut self, if_type: DyadPtr) -> Result<DyadPtr, ParseError> {
        let items = self.drive_until_open(RightSide::Condition)?;
        let cond = self.one_of(items)?.dyad;
        let types = self.types;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(types, cond) } {
            return Err(ParseError::NonBoolCondition);
        }

        // A comptime condition resolves the conditional now, in the one pass:
        // an untaken bracket's tokens are dropped unlexed, so nothing inside
        // is resolved, committed or declared, and branches for other comptime types coexist.
        // SAFETY: `cond` is the reduced dyad just parsed.
        if let Some(truth) = unsafe { bool_literal_value(types, cond) } {
            return self.parse_comptime_if(if_type, cond, truth);
        }

        // DESIGN ›A type is a comptime value, resolved in the pass‹, a checked tape cell.
        // SAFETY: `cond` is the reduced dyad just parsed.
        let check = unsafe { type_check_of(types, cond) };

        // A runtime branch may or may not run, so parse-time rebinding is off
        // inside it.
        self.cx.runtime_depth += 1;
        self.cx.narrow_next = check.filter(|&(_, _, holds)| holds).map(|(key, ty, _)| (key, ty));
        let then = self.parse_branch();
        self.cx.narrow_next = None;
        // `else if …` is sugar: the `if` right after `else` becomes the
        // else-branch directly, so a chain nests right-associatively.
        let branches = then.and_then(|then| {
            let els = if self.consume_else() {
                if self.consume_token(self.types.if_) {
                    self.parse_if(if_type)
                } else {
                    self.cx.narrow_next =
                        check.filter(|&(_, _, holds)| !holds).map(|(key, ty, _)| (key, ty));
                    self.parse_branch()
                }
            } else {
                Ok(std::ptr::null_mut())
            };
            els.map(|els| (then, els))
        });
        self.cx.narrow_next = None;
        self.cx.runtime_depth -= 1;
        let (then, els) = branches?;

        // A `!=` check whose branch raises leaves the cell checked for the rest of the scope.
        if let Some((key, ty, false)) = check {
            // SAFETY: `then` is the reduced dyad just parsed.
            if els.is_null() && unsafe { (*types.through(then)).ty } == types.error.error {
                let scope = self.cx.open.last_mut().expect("the `if` stands in an open scope");
                scope.narrowed.push((key, ty));
            }
        }

        let value = self.rt.store.alloc_operands(&[cond, then, els, self.types.ops.if_]);
        Ok(self.rt.store.alloc_raw(if_type, value))
    }

    fn bracketed_body(&mut self) -> Result<DyadPtr, ParseError> {
        self.expect_open()?;
        let body = self.parse_sequence()?;
        self.expect_close()?;
        Ok(body)
    }

    /// A branch of `if`, or a `while` body: a bracket, or the next expression,
    /// which ends before an `else` (DESIGN ›Expressions are self-delimiting‹).
    fn parse_branch(&mut self) -> Result<DyadPtr, ParseError> {
        if self.at_open() {
            return self.bracketed_body();
        }
        // A bare branch is a scope as a bracket is, holding one expression.
        self.skip_whitespace();
        let start = self.cx.pos;
        let was = self.cx.else_ends.replace(self.cx.open.len() + 1);
        let body = self.parse_sequence();
        self.cx.else_ends = was;
        if self.cx.pos == start {
            return Err(ParseError::Empty);
        }
        body
    }

    /// An untaken branch: a bracket is dropped unlexed; a bare one is parsed
    /// and dropped, since only constructing it finds where it ends.
    fn skip_branch(&mut self) -> Result<(), ParseError> {
        if self.at_open() {
            return self.skip_group();
        }
        self.cx.runtime_depth += 1;
        let dead = self.parse_branch();
        self.cx.runtime_depth -= 1;
        dead.map(|_| ())
    }

    /// True with an else: the then-branch is the result and the else-tail is
    /// dropped. False: the then-branch is dropped. An else-less `if` stays a
    /// statement node either way, since it yields unit.
    fn parse_comptime_if(
        &mut self,
        if_type: DyadPtr,
        cond: DyadPtr,
        truth: bool,
    ) -> Result<DyadPtr, ParseError> {
        if truth {
            let then = self.parse_branch()?;
            if self.consume_else() {
                self.skip_else_tail(if_type)?;
                return Ok(then);
            }
            let value = self.rt.store.alloc_operands(&[
                cond,
                then,
                std::ptr::null_mut(),
                self.types.ops.if_,
            ]);
            return Ok(self.rt.store.alloc_raw(if_type, value));
        }
        self.skip_branch()?;
        if self.consume_else() {
            if self.consume_token(self.types.if_) {
                return self.parse_if(if_type);
            }
            return self.parse_branch();
        }
        let value =
            self.rt.store.alloc_operands(&[cond, cond, std::ptr::null_mut(), self.types.ops.if_]);
        Ok(self.rt.store.alloc_raw(if_type, value))
    }

    /// Drop a balanced `( … )` group without parsing it (DESIGN ›a constructor
    /// may splice tokens in or drop upcoming ones before they lex‹): `«…»` text
    /// and `#` prose are skipped opaquely, their parentheses being text.
    fn skip_group(&mut self) -> Result<(), ParseError> {
        /// `pos` must point at `«`; `None` if unterminated.
        fn skip_text(bytes: &[u8], mut pos: usize) -> Option<usize> {
            pos += 2; // the «
            while pos + 1 < bytes.len() {
                if bytes[pos] == 0xC2 && bytes[pos + 1] == 0xBB {
                    return Some(pos + 2);
                }
                pos += 1;
            }
            None
        }
        self.expect_open()?;
        let bytes = self.cx.source.as_bytes();
        let mut depth = 1usize;
        while self.cx.pos < bytes.len() {
            match bytes[self.cx.pos] {
                b'(' => {
                    depth += 1;
                    self.cx.pos += 1;
                }
                b')' => {
                    depth -= 1;
                    self.cx.pos += 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                0xC2 if bytes.get(self.cx.pos + 1) == Some(&0xAB) => {
                    self.cx.pos =
                        skip_text(bytes, self.cx.pos).ok_or(ParseError::UnclosedBracket)?;
                }
                b'#' => {
                    // `#` takes a following «…» string or the rest of the line,
                    // as the comment constructor reads it.
                    self.cx.pos += 1;
                    while self.cx.pos < bytes.len() && matches!(bytes[self.cx.pos], b' ' | b'\t') {
                        self.cx.pos += 1;
                    }
                    if bytes.get(self.cx.pos) == Some(&0xC2)
                        && bytes.get(self.cx.pos + 1) == Some(&0xAB)
                    {
                        self.cx.pos =
                            skip_text(bytes, self.cx.pos).ok_or(ParseError::UnclosedBracket)?;
                    } else {
                        while self.cx.pos < bytes.len() && bytes[self.cx.pos] != b'\n' {
                            self.cx.pos += 1;
                        }
                    }
                }
                _ => self.cx.pos += 1,
            }
        }
        Err(ParseError::UnclosedBracket)
    }

    /// Drop the dead tail after a taken `else`: `if ( cond ) then` links
    /// while further `else`s follow, or the final branch. A bare condition's
    /// end is found only by constructing it, so that link is parsed and dropped.
    fn skip_else_tail(&mut self, if_type: DyadPtr) -> Result<(), ParseError> {
        loop {
            if !self.consume_token(self.types.if_) {
                return self.skip_branch();
            }
            if !self.at_open() {
                self.cx.runtime_depth += 1;
                let dead = self.parse_if(if_type);
                self.cx.runtime_depth -= 1;
                return dead.map(|_| ());
            }
            self.skip_group()?; // ( cond )
            self.skip_branch()?;
            if !self.consume_else() {
                return Ok(());
            }
        }
    }

    /// The node is `{type: not, value: operand}`.
    ///
    /// # Safety
    /// `operand` must be a reduced dyad from the store.
    pub(crate) unsafe fn build_not(
        &mut self,
        not_id: DyadPtr,
        operand: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        if !is_bool_result(types, operand) {
            return Err(ParseError::NonBoolOperands);
        }
        // A bool-literal operand folds now (pure, nothing lost): what keeps a
        // comptime chain comptime.
        if let Some(v) = bool_literal_value(types, operand) {
            return Ok(crate::identities::bool_mod::literal_node(
                self.rt.store,
                self.types.bool_,
                !v,
            ));
        }
        let value = self.rt.store.alloc_operands(&[operand, self.types.ops.not_]);
        Ok(self.rt.store.alloc_raw(not_id, value))
    }

    /// `while cond body`: the node is `[cond, body]`, a statement yielding
    /// unit; the condition and body are read as `if`'s are; the body's value is
    /// thrown away (DESIGN ›a loop body's is thrown away‹); a `return` in it
    /// leaves the enclosing function.
    pub fn parse_while(&mut self, while_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        let items = self.drive_until_open(RightSide::Condition)?;
        let cond = self.one_of(items)?.dyad;
        let types = self.types;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(types, cond) } {
            return Err(ParseError::NonBoolCondition);
        }
        // A repeated body: parse-time rebinding is off inside, and a name
        // declared outside may not be moved or dropped inside.
        self.cx.runtime_depth += 1;
        self.cx.scopes.push_barrier();
        let body = self.parse_branch();
        self.cx.scopes.pop_barrier();
        self.cx.runtime_depth -= 1;
        let body = body?;
        // SAFETY: `body` is the reduced dyad just parsed.
        if self.cx.frames.is_empty() && unsafe { contains_return(types, body) } {
            return Err(ParseError::EarlyReturn);
        }
        let value = self.rt.store.alloc_operands(&[cond, body, self.types.ops.while_]);
        Ok(self.rt.store.alloc_raw(while_id, value))
    }

    /// `for i in a..b ( body )`, with an optional `..step` and optionally no
    /// index (DESIGN ›The scope's constructor is the driver‹): end-exclusive;
    /// the counter is a fresh block-local either way, so both tiers run one node shape.
    pub fn parse_for(&mut self, for_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        let name = self.loop_name()?;
        // The range, constructed by parse_rank, the `..` cells inert
        // delimiters read by position.
        let parts: Vec<(DyadPtr, usize)> = self
            .drive_until_open(RightSide::Range)?
            .into_iter()
            .map(|(c, s)| (c.dyad, s))
            .collect();
        let dotdot = self.types.dotdot_;
        let types = self.types;
        // The `..` cells are uses of that identity: its binding, read through.
        // SAFETY: every item `drive_until_open` returned is a dyad from the store.
        let is_dotdot = |d: &DyadPtr| unsafe { types.through(*d) } == dotdot;
        let (start, end, step) = match parts.as_slice() {
            [(s, _), (d, _), (e, _)] if is_dotdot(d) => (*s, *e, None),
            [(s, _), (d, _), (e, _), (d2, _), (st, _)] if is_dotdot(d) && is_dotdot(d2) => {
                (*s, *e, Some(*st))
            }
            _ => return Err(ParseError::ExpectedRange),
        };

        // Concrete types must match, literals commit, all-literals default to i32.
        let types = self.types;
        // SAFETY: `step` is the reduced dyad just parsed.
        let step_was_literal =
            step.is_some_and(|s| unsafe { (*types.through(s)).ty } == types.rational);
        let mut parts = vec![start, end];
        if let Some(s) = step {
            parts.push(s);
        }
        // SAFETY: `parts` are reduced dyads just parsed.
        let logos =
            unsafe { crate::identities::resolve_loop_parts(self.rt.store, types, &mut parts)? };
        let (start, end) = (parts[0], parts[1]);
        let step = parts.get(2).copied().unwrap_or(std::ptr::null_mut());
        if step_was_literal {
            use crate::identities::numtype;
            // SAFETY: `step` is the committed literal just built; `logos` a numtype node.
            let (bits, nt) = unsafe {
                // A marked place here would mean the literal check above let a
                // variable through, and reading its tagged offset as an address is the crash class the reading rule ends.
                use crate::identities::read::{read_kind, Read};
                if !matches!(read_kind(types, step), Read::Scalar(_))
                    || crate::dyad::is_place((*step).value)
                {
                    return Err(ParseError::BadStep);
                }
                (numtype::read_scalar((*step).ty, (*step).value), numtype::of_type_node(logos))
            };
            if numtype::apply_compare(numtype::CmpOp::Gt, nt, bits, 0) == 0 {
                return Err(ParseError::BadStep);
            }
        }

        // SAFETY: `logos` is a numtype node from resolve_loop_parts.
        let width = unsafe { crate::identities::numtype::of_type_node(logos) }.bytes();
        let var = self.alloc_local(logos, width);
        let parent = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        let scope = crate::identities::scope::mint(self.rt.store, types.scope, parent);
        // A repeated body: a name declared outside may not be moved or dropped
        // inside; the loop variable, declared in the scope pushed next, is inside.
        self.cx.scopes.push_barrier();
        self.cx.scopes.push(scope);
        // Parse-time rebinding is off inside a repeated body, the counter's name included.
        self.cx.runtime_depth += 1;
        let body = match name {
            Some((nstart, nlen)) => {
                let source = self.cx.source;
                self.declare_name(&source[nstart..nstart + nlen], var, nstart)
                    .and_then(|_| self.bracketed_body())
            }
            None => self.bracketed_body(),
        };
        self.cx.runtime_depth -= 1;
        self.cx.scopes.pop();
        self.cx.scopes.pop_barrier();
        let body = body?;
        // SAFETY: `body` is the reduced dyad just parsed.
        if self.cx.frames.is_empty() && unsafe { contains_return(types, body) } {
            return Err(ParseError::EarlyReturn);
        }

        let value =
            self.rt.store.alloc_operands(&[var, start, end, step, body, self.types.ops.for_]);
        Ok(self.rt.store.alloc_raw(for_id, value))
    }

    /// A field of a node reached by path, itself reached (DESIGN ›The dyad's
    /// read surface‹: a node's fields get you more nodes): a scope's `dyads`
    /// and `dyads[k]`, an array's `size`, an operator node's operand by its
    /// role. `None` leaves the read to the rules for any `@dyad` value.
    ///
    /// # Safety
    /// `node` must be a node from the store.
    unsafe fn reached_member(
        &mut self,
        node: DyadPtr,
        name: &str,
        index: Option<usize>,
        bracket: bool,
    ) -> Result<Option<(DyadPtr, usize)>, ParseError> {
        use crate::identities::{array, numtype::NumType, scope};
        let types = self.types;
        let ty = (*node).ty;
        if ty == types.scope && name == "dyads" {
            let arr = scope::dyads(self.rt.store, types.array_, node);
            if arr.is_null() {
                return Err(ParseError::BadReflectRead);
            }
            return match index {
                Some(k) => {
                    let item = *array::items(arr).get(k).ok_or(ParseError::BadReflectRead)?;
                    Ok(Some((self.path_operand(item), 1)))
                }
                // An index known only at run: every read here folds at parse.
                None if bracket => Err(ParseError::BadReflectRead),
                None => Ok(Some((self.reach(arr), 0))),
            };
        }
        if ty == types.array_ && name == "size" {
            let size = array::items(node).len() as i64;
            return Ok(Some((self.scalar_value(NumType::U64, size), 0)));
        }
        Ok(self.node_field(node, name)?.map(|n| (n, 0)))
    }

    /// Field `name` of a node, as [`Parser::path_operand`] yields it: an
    /// operator node's operand by its role, a run type's field in declaration
    /// order. `None` when the node's type defines no such field.
    ///
    /// # Safety
    /// `node` must be a node from the store.
    unsafe fn node_field(
        &mut self,
        node: DyadPtr,
        name: &str,
    ) -> Result<Option<DyadPtr>, ParseError> {
        use crate::identities::{array, meta};
        let ty = (*node).ty;
        let value = (*node).value;
        if ty.is_null() || value.is_null() || crate::dyad::is_place(value) {
            return Ok(None);
        }
        let slot = match meta::kind_of(ty) {
            Some(meta::TUPLE_TAG | meta::LIST_TAG) => (0..meta::arity_of(ty))
                .find(|&i| crate::reflect::text_of(meta::role_of(ty, i)) == name.as_bytes()),
            // `[field…, null, spec]`: the fields are the nodes the constructor filled,
            // for every node a Logos-written `parse` built.
            Some(meta::RECORD_TAG)
                if !meta::run_body_of(ty).is_null() || self.is_logos_ctor(ty) =>
            {
                let mut fields = ScopeStack::new();
                fields.push(meta::record_scope_of(ty));
                fields.resolve(self.trie, name).ok().and_then(|r| {
                    array::items(meta::record_fields_of(ty)).iter().position(|&f| f == r.identity)
                })
            }
            _ => None,
        };
        let Some(i) = slot else {
            return Ok(None);
        };
        let field = *(value as *const DyadPtr).add(i);
        if field.is_null() {
            return Err(ParseError::BadReflectRead);
        }
        Ok(Some(self.path_operand(field)))
    }

    /// Whether `t`'s field `name` is declared of a type a Logos `parse` builds.
    ///
    /// # Safety
    /// `t` must be a record type from the store.
    unsafe fn holds_node(&self, t: DyadPtr, name: &str) -> bool {
        use crate::identities::{array, meta};
        let mut fields = ScopeStack::new();
        fields.push(meta::record_scope_of(t));
        fields.resolve(self.trie, name).ok().is_some_and(|r| {
            array::items(meta::record_fields_of(t)).contains(&r.identity)
                && meta::is_node_valued((*r.identity).ty, self.types.fn_type)
        })
    }

    /// `lhs.f` where `lhs` yields a node of `t` only when the program runs, a place of the
    /// type or a call returning it: the field read through the node's address when it runs,
    /// or a `share` member, a type body's `fn` called with `lhs` as its value.
    ///
    /// # Safety
    /// `lhs` must be a reduced dyad from the store and `t` a type `is_node_valued` accepts.
    unsafe fn run_node_member(
        &mut self,
        lhs: DyadPtr,
        left: Cell,
        t: DyadPtr,
        name: &str,
        call: Option<Vec<DyadPtr>>,
    ) -> Result<Option<(DyadPtr, usize)>, ParseError> {
        use crate::identities::{array, meta, this};
        let types = self.types;
        let mut fields = ScopeStack::new();
        fields.push(meta::record_scope_of(t));
        let items = array::items(meta::record_fields_of(t));
        let found = fields
            .resolve(self.trie, name)
            .ok()
            .and_then(|r| Some((items.iter().position(|&f| f == r.identity)?, r.binding)));
        if let Some((i, binding)) = found {
            let k = self.scalar_value(crate::identities::numtype::NumType::U64, i as i64);
            let ty = (*items[i]).ty;
            let node = this::build_field_read(
                self.rt.store,
                types,
                lhs,
                k,
                ty,
                binding,
                std::ptr::null_mut(),
            );
            return Ok(Some((node, 0)));
        }
        let Some(member) = self.share_member_read(t, name) else {
            return Ok(None);
        };
        if let Some(args) = call {
            if self.takes_this(member) {
                self.check_receiver_write(&left, member)?;
                let mut with_this = vec![lhs];
                with_this.extend(args);
                // The call's result is no member: the path was the member's own.
                self.cx.member_path.clear();
                return self.build_call(member, &with_this).map(|n| Some((n, 1)));
            }
        }
        Ok(Some((member, 0)))
    }

    /// A node reached by path, as a value: itself when reading it runs
    /// nothing (a name, a literal, a place), else its address, so reading a
    /// path never runs code.
    ///
    /// # Safety
    /// `node` must be a node from the store.
    unsafe fn path_operand(&mut self, node: DyadPtr) -> DyadPtr {
        use crate::identities::read::{read_kind, Read};
        let types = self.types;
        let runs = (*node).ty != types.binding_
            && ((*node).ty == types.scope || matches!(read_kind(types, node), Read::Executable(_)));
        if runs {
            self.reach(node)
        } else {
            node
        }
    }

    /// The cell after `for` decides: a spelling followed by `in` is the name;
    /// a fresh spelling not followed by `in` wanted it, since nothing declared
    /// lexes there; anything else begins the range, and the cursor goes back.
    fn loop_name(&mut self) -> Result<Option<(usize, usize)>, ParseError> {
        let at = self.cx.pos;
        let Some((nstart, nlen, fresh)) = self.lex_spelling_fresh() else {
            return Ok(None);
        };
        if self.consume_token(self.types.in_) {
            return Ok(Some((nstart, nlen)));
        }
        if fresh {
            return Err(ParseError::ExpectedIn);
        }
        self.cx.pos = at;
        Ok(None)
    }
}

/// The member right of `.`: the name's span in the source, and the `[i]` or `(…)` cell
/// some reads take after it.
#[derive(Clone)]
struct Member {
    start: usize,
    len: usize,
    index: Option<usize>,
    key: Option<DyadPtr>,
    call: Option<Vec<DyadPtr>>,
}

impl<'a> Parser<'a> {
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn field_access_each(
        &mut self,
        lhs: DyadPtr,
        left: Cell,
        member: Member,
    ) -> Result<(DyadPtr, usize), ParseError> {
        let Some((connective, a, b)) = crate::identities::group::members(self.types, lhs) else {
            return self.field_access(lhs, left, member);
        };
        // A member reads as `.`'s left operand does, through its name.
        let (a, b) =
            (self.settled_type(self.types.through(a)), self.settled_type(self.types.through(b)));
        let (a, consumed) = self.field_access_each(a, Cell::built(a), member.clone())?;
        let (b, _) = self.field_access_each(b, Cell::built(b), member)?;
        let joined = crate::identities::group::join(self.rt.store, self.types, connective, a, b)?;
        Ok((joined, consumed))
    }

    /// `lhs.name` to a place: a numeric node over the instance's storage at
    /// the field's byte offset (DESIGN ›Resolution is one rule‹). The field
    /// name resolves in the record type's own scope alone.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn field_access(
        &mut self,
        lhs: DyadPtr,
        left: Cell,
        member: Member,
    ) -> Result<(DyadPtr, usize), ParseError> {
        let Member { start: nstart, len: nlen, index, key, call } = member;
        let unit_call = matches!(call, Some(ref a) if a.is_empty());
        // The member name is read as its raw spelling: a field is dot-only,
        // so what the driver resolved it to is beside the point. `index` and
        // `unit_call` are the reads that take one more cell; the count consumed is returned.
        {
            // `source` is `&'a str` (Copy), independent of the `&mut self` the
            // member reads then need.
            let source = self.cx.source;
            let name = &source[nstart..nstart + nlen];
            // `tape[0].f` inside a parse body: a field of the node the parse builds or reads
            // (DESIGN ›There is no `this`‹); `.dyads` is the cell's own.
            if name != "dyads" && self.is_node_cell(lhs) {
                let node = self.cx.definitions.last().expect("a parse body is open").this_param;
                let member = self.this_field(name, nstart)?;
                // `tape[0].f(…)`, a `share` function: called on that node.
                if let Some(args) = call {
                    if self.takes_this(member) {
                        let mut with_node = vec![node];
                        with_node.extend(args);
                        return self.build_call(member, &with_node).map(|n| (n, 1));
                    }
                }
                return Ok((member, 0));
            }
            // A scope cell's lines, read when the constructor runs.
            {
                use crate::identities::tape;
                let (types, store) = (self.types, &mut *self.rt.store);
                if (*lhs).ty == types.tape.slot && name == "dyads" {
                    let Some(i) = key else {
                        return Ok((tape::build_cell_dyads(store, types, lhs), 0));
                    };
                    let line = tape::build_cell_dyad_at(store, types, lhs, i);
                    return Ok((self.narrowed_read(line), 1));
                }
                if (*lhs).ty == types.tape.cell_dyads && name == "size" {
                    return Ok((tape::build_cell_dyads_size(store, types, lhs), 0));
                }
                // A bare parameter holding a bracket a call was handed: its lines, read as a
                // tape cell's are, when the body runs.
                if (*lhs).ty.is_null() && crate::dyad::is_place((*lhs).value) && name == "dyads" {
                    let Some(i) = key else {
                        return Ok((tape::build_bracket_dyads(store, types, lhs), 0));
                    };
                    let line = tape::build_bracket_line(store, types, lhs, i);
                    return Ok((self.narrowed_read(line), 1));
                }
            }
            // A field holding a node a Logos `parse` built is read when the code runs, even
            // through a node the parse knows: the node it holds may be replaced before then.
            if let Some(t) = crate::identities::node_type_of(self.types, lhs) {
                if self.holds_node(t, name) {
                    if let Some(read) = self.run_node_member(lhs, left, t, name, None)? {
                        return Ok(read);
                    }
                }
            }
            if let Some(node) = self.known_node(lhs, self.cx.member_root) {
                if let Some(read) = self.reached_member(node, name, index, key.is_some())? {
                    return Ok(read);
                }
            }
            if crate::identities::read::read_kind(self.types, lhs)
                != crate::identities::read::Read::Node
            {
                if let Some(t) = crate::identities::node_type_of(self.types, lhs) {
                    if let Some(read) = self.run_node_member(lhs, left, t, name, call.clone())? {
                        return Ok(read);
                    }
                }
            }
            // `(2 ^ 3).lhs`: the node a comptime value was folded from keeps its fields.
            let lhs = if left.origin.is_null() { lhs } else { left.origin };
            if (*lhs).ty == self.types.dyad_ {
                return self.view_member(lhs, name).map(|n| (n, 0));
            }
            // `here.scope`, `caller.scope` (DESIGN ›Meta-navigation‹): `here`
            // knows its scope at the appearance, so the read folds; `caller`
            // reads the pass, so its `.scope` is a node run inside a constructor.
            let h = self.types.here;
            if (*lhs).ty == h.here || (*lhs).ty == h.caller {
                if name != "scope" {
                    return Err(ParseError::BadReflectRead);
                }
                let node = if (*lhs).ty == h.here {
                    self.reach(crate::identities::here::scope_of_here(lhs))
                } else {
                    crate::identities::here::build_caller_scope(self.rt.store, self.types)
                };
                return Ok((node, 0));
            }
            // A scope's link up is `.back`; `.scope` is the scope of a spot, a call or a name.
            if name == "back" && crate::identities::here::yields_scope_address(self.types, lhs) {
                let node = crate::identities::here::build_back(self.rt.store, self.types, lhs);
                return Ok((node, 0));
            }
            if crate::identities::is_type_value(self.types, lhs) {
                let n = self.logos_member(lhs, name, index)?;
                return Ok((n, usize::from(name == "roles")));
            }
            // A place holding a type: its fields are the identity's, which
            // nobody knows until the program runs (DESIGN ›A type is a comptime value‹).
            if matches!(
                crate::identities::read::read_kind(self.types, lhs),
                crate::identities::read::Read::Container(t) if t == self.types.type_ || t == self.types.dyad_
            ) || crate::identities::yields_type(self.types, lhs)
            {
                return Err(ParseError::TypeKnownOnlyAtRun);
            }
            if name == "type" {
                return Err(ParseError::TypeIsColonRead);
            }
            // An operator node's slots are the fields its own type defines:
            // `.operands[i]` fetches one, no view involved; a null slot is the checked error until `?`.
            if name == "operands"
                && !(*lhs).ty.is_null()
                && matches!(
                    crate::identities::meta::kind_of((*lhs).ty),
                    Some(crate::identities::meta::TUPLE_TAG | crate::identities::meta::LIST_TAG)
                )
            {
                let i = index.ok_or(ParseError::ExpectedIndexBracket)?;
                if i >= crate::identities::meta::arity_of((*lhs).ty) || (*lhs).value.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                let ops = (*lhs).value as *const DyadPtr;
                let operand = *ops.add(i);
                if operand.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                return Ok((operand, 1));
            }
            if let Some(field) = self.node_field(lhs, name)? {
                return Ok((field, 0));
            }
            // `.compile` is the fn type's shared member (DESIGN ›Execution is
            // function application‹); the name-compare stands in for resolution
            // through the type's scope. The leaf is minted now, entry zero; the run patches it in.
            if &self.cx.source[nstart..nstart + nlen] == "compile"
                && (*lhs).ty == self.types.fn_type
            {
                if !unit_call {
                    return Err(ParseError::ExpectedOpen);
                }
                let code = crate::identities::callable::mint(
                    self.rt.store,
                    self.types.callable_,
                    0,
                    self.types.conv_container,
                );
                let value = self.rt.store.alloc_operands(&[lhs, code, self.types.ops.compile_]);
                return Ok((self.rt.store.alloc_raw(self.types.compile_, value), 1));
            }
        }
        // A tape's natives, members of `parsing_tape`'s scope that are not
        // laid-out fields, are built as a call with the receiver's address first.
        if let Some(recv) = crate::identities::tape::receiver_addr(self.rt.store, self.types, lhs) {
            let name = &self.cx.source[nstart..nstart + nlen];
            if let Some((op, leaf)) = crate::identities::tape::member(&self.types.tape, name) {
                let types = self.types;
                if op == types.tape.is_constructed || op == types.tape.spelling {
                    let k = key.ok_or(ParseError::ExpectedIndexBracket)?;
                    let node = crate::identities::tape::build_member(
                        self.rt.store,
                        types,
                        recv,
                        op,
                        leaf,
                        &[k],
                    )?;
                    return Ok((node, 1));
                }
                let args = call.ok_or(ParseError::ExpectedOpen)?;
                if op == types.tape.insert || op == types.tape.remove || op == types.tape.recenter {
                    self.forget_narrowed();
                }
                let node = crate::identities::tape::build_member(
                    self.rt.store,
                    types,
                    recv,
                    op,
                    leaf,
                    &args,
                )?;
                return Ok((node, 1));
            }
        }
        // Through a record pointer, `p@.x` folds the field offset into the deref.
        if (*lhs).ty == self.types.deref_ {
            let (ptr_expr, pointee, base_off) = crate::identities::pointer::deref_parts(lhs);
            if pointee.is_null() || !crate::identities::meta::is_record_type(pointee) {
                return Err(ParseError::UnsupportedOperands);
            }
            let (field, offset, _) = match self.resolve_field(pointee, nstart, nlen) {
                Ok(found) => found,
                Err(e) => {
                    let name = &self.cx.source[nstart..nstart + nlen];
                    return self.share_member_read(pointee, name).map(|n| (n, 0)).ok_or(e);
                }
            };
            let types = self.types;
            return Ok((
                crate::identities::pointer::build_deref(
                    self.rt.store,
                    types,
                    ptr_expr,
                    (*field).ty,
                    base_off as usize + offset,
                ),
                0,
            ));
        }
        // The access is a place, its offset folded into the instance's own
        // place now; `wrapping_add` keeps a frame-tagged value a valid tagged offset.
        let record_logos = (*lhs).ty;
        if record_logos.is_null()
            || !crate::identities::meta::is_record_type(record_logos)
            || (*lhs).value.is_null()
        {
            return Err(ParseError::UnsupportedOperands);
        }
        let (field, offset, binding) = match self.resolve_field(record_logos, nstart, nlen) {
            Ok(found) => found,
            Err(e) => {
                let name = &self.cx.source[nstart..nstart + nlen];
                let Some(member) = self.share_member_read(record_logos, name) else {
                    return Err(e);
                };
                if let Some(args) = call {
                    if self.takes_this(member) {
                        self.check_receiver_write(&left, member)?;
                        // A record laid out in bytes is handed on as its own place, any
                        // other value by a `dyad` view, as a `dyad ?` parameter takes a node.
                        let view = if crate::dyad::is_place((*lhs).value) {
                            crate::identities::instance::layout(record_logos)?;
                            let types = self.types;
                            crate::identities::this::build_on_record(
                                self.rt.store,
                                types,
                                record_logos,
                                lhs,
                            )
                        } else {
                            self.rt.store.alloc_raw(self.types.dyad_, lhs.cast())
                        };
                        let mut with_this = vec![view];
                        with_this.extend(args);
                        self.cx.member_path.clear();
                        return self.build_call(member, &with_this).map(|n| (n, 1));
                    }
                }
                return Ok((member, 0));
            }
        };
        let addr = (*lhs).value.wrapping_add(offset);
        let node = self.rt.store.alloc_raw((*field).ty, addr);
        // Every binding along the path grants a write into the place: the
        // name the path starts at, then each field.
        let mut path = self.path_of(&left);
        if path.is_empty() && !self.cx.member_root.is_null() {
            path.push(self.cx.member_root);
        }
        path.push(binding);
        self.cx.member_path = path;
        Ok((node, 0))
    }

    /// `.`'s constructor: the member read of `tape[-1]` named by the cell to
    /// the right, plus the `[i]` or `()` cell some reads take (see
    /// [`Parser::field_access`]); every consumed cell is spliced out.
    pub(crate) fn construct_field_access(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // The left is read as it stands: an identity's fields are read off the
        // token before its own constructor wakes (DESIGN ›Text is the quote‹),
        // so a callable to the left is the identity itself, not a call in waiting.
        let Some(left) = tape.at(-1).copied() else {
            return Err(ParseError::MissingOperand);
        };
        let (lhs, root) = (self.operand_dyad(left)?, left.binding(self.types));
        // The member is read by its spelling at the offset it was lexed at, so a
        // keyword there (`.type`) reads as a name.
        let this_read = self.is_node_cell(lhs);
        self.cx.member_asleep = this_read;
        let m = self.cell_at(tape, 1);
        self.cx.member_asleep = false;
        let Some(m) = m? else {
            return Err(ParseError::ExpectedField);
        };
        let mstart = m.start;
        let save = self.cx.pos;
        self.cx.pos = mstart;
        let member = self.lex_spelling();
        self.cx.pos = save;
        let Some((nstart, nlen)) = member else {
            return Err(ParseError::ExpectedField);
        };
        // The optional `[i]` or `(…)` after the member, lexed on demand; a
        // bracket is the member's argument list.
        self.cell_at(tape, 2)?;
        let index = self.index_at(tape, 2);
        let bracket = tape.at(2).filter(|c| c.is_bracket()).map(|c| c.dyad);
        // SAFETY: a bracket cell is a node from the store.
        let call = bracket.map(|d| unsafe { self.args_of(d) });
        let key = self.index_node_at(tape, 2);
        let writes = index.is_none()
            && key.is_none()
            && bracket.is_none()
            && tape
                .at(2)
                .is_some_and(|c| !c.constructed && self.cell_identity(c) == self.types.assign);
        let source = self.cx.source;
        // A name its value's parse woke stands constructed and still holds its binding.
        let woke = tape.at(-1).filter(|c| c.constructed && !c.dyad.is_null()).map(|c| c.dyad);
        // SAFETY: a constructed cell's dyad is a dyad from the store.
        let named = woke.filter(|&d| unsafe { (*d).ty } == self.types.binding_).unwrap_or(root);
        // SAFETY: `named` is null or a binding dyad; `lhs` a reduced dyad off the tape.
        let fills = unsafe {
            self.check_field_unwritten(named, lhs, &source[nstart..nstart + nlen], writes)
        };
        if let Err(e) = fills {
            self.cx.pos = nstart;
            return Err(e);
        }
        self.cx.member_root = root;
        self.cx.member_path.clear();
        let member = Member { start: nstart, len: nlen, index, key, call };
        // SAFETY: `lhs` is a reduced dyad off the tape.
        let access = unsafe { self.field_access_each(lhs, left, member) };
        self.cx.member_root = std::ptr::null_mut();
        let path = std::mem::take(&mut self.cx.member_path);
        let (node, consumed) = access?;
        for _ in 0..(1 + consumed) {
            tape.remove(1);
        }
        tape.remove(-1);
        // A type identity a parse's `tape[0].f` folded to stands as its own
        // unconstructed cell, so it reads its right side as anywhere else.
        // SAFETY: `node` is a node from the store.
        let folded_type = this_read
            && unsafe {
                (*node).ty == self.types.type_ && crate::identities::meta::kind_of(node).is_some()
            };
        if folded_type {
            tape.set_dyad(0, node);
        } else {
            let path = if path.is_empty() {
                std::ptr::null_mut()
            } else {
                crate::identities::array::build(self.rt.store, self.types.array_, &path)
            };
            tape.place(node);
            let cell = tape.at_mut(0).expect("placed above");
            cell.path = path;
            if let Ok(Some(fill)) = fills {
                cell.target = fill;
            }
        }
        Ok(Constructed::Placed)
    }

    /// The comptime index a `[…]` cell carries when its interior is a
    /// non-negative literal: what the reflection reads fold at parse.
    fn index_at(&self, tape: &ParsingTape, offset: isize) -> Option<usize> {
        let key = self.index_node_at(tape, offset)?;
        // SAFETY: the interior is a node from the store.
        unsafe {
            if (*key).ty != self.types.rational {
                return None;
            }
            let i = crate::identities::rational::mold(key)?;
            if i < 0 {
                None
            } else {
                Some(i as usize)
            }
        }
    }

    /// The one value a `[…]` cell's `dyads` hold, prose aside: any expression, for the
    /// reads whose natives run it.
    fn index_node_at(&self, tape: &ParsingTape, offset: isize) -> Option<DyadPtr> {
        let c = tape.at(offset)?;
        if !c.constructed {
            return None;
        }
        let d = c.dyad;
        // SAFETY: a dyad cell is a node from the store; a `square_brackets` node's value
        // holds its `dyads` first, as a scope's does.
        unsafe {
            if (*d).ty != self.types.square_brackets {
                return None;
            }
            let defer_ = self.types.defer_;
            let mut values = crate::identities::scope::exprs_of(d)?.iter().copied().filter(|&e| {
                !crate::identities::numtype::is_comment_type((*e).ty) && (*e).ty != defer_
            });
            let key = values.next()?;
            values.next().is_none().then_some(key)
        }
    }

    /// `[` is `(` in square brackets: the interior parsed as any bracket's,
    /// closed by its `]`. It lands a `square_brackets` cell (DESIGN ›The
    /// scope's constructor is the driver‹), or right after a tape value the element read
    /// `t[k]`, folded here since a tape value has no constructor of its own yet.
    pub(crate) fn construct_index(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let (scope, key) = self.parse_block()?;
        let key = key.dyad;
        self.expect_close_sq()?;
        // An index right after a tape value is the element read, a slot node
        // the identity to its left owns; after a `.` it stays a passive cell.
        if let Some(left) = tape.at(-1).copied() {
            // A fresh spelling to the left is a member name after `.`, never a tape.
            if !left.is_fresh() && self.is_operand_cell(&left) {
                let lhs = self.operand_dyad(left)?;
                let types = self.types;
                // SAFETY: `lhs` is a reduced dyad from the store.
                let recv =
                    unsafe { crate::identities::tape::receiver_addr(self.rt.store, types, lhs) };
                if let Some(recv) = recv {
                    let slot = crate::identities::tape::build_slot(self.rt.store, types, recv, key);
                    // SAFETY: `slot` is the slot node just built.
                    let node = unsafe { self.narrowed_read(slot) };
                    tape.remove(-1);
                    tape.place(node);
                    return Ok(Constructed::Placed);
                }
                // SAFETY: `lhs` and `key` are reduced dyads from the store.
                let get = unsafe {
                    crate::identities::hashmap::build_get(self.rt.store, types, lhs, key)
                }?;
                if let Some(node) = get {
                    tape.remove(-1);
                    tape.place(node);
                    return Ok(Constructed::Placed);
                }
            }
        }
        // SAFETY: `scope` is the block `parse_block` minted, closed.
        let dyads =
            unsafe { crate::identities::scope::dyads(self.rt.store, self.types.array_, scope) };
        let value = self.rt.store.alloc_operands(&[dyads, std::ptr::null_mut()]);
        let node = self.rt.store.alloc_raw(self.types.square_brackets, value);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Against `record_logos`'s own scope alone: an enclosing binding of the
    /// same spelling can never shadow or double a field.
    ///
    /// # Safety
    /// `record_logos` must be a record type node from the store.
    unsafe fn resolve_field(
        &mut self,
        record_logos: DyadPtr,
        nstart: usize,
        nlen: usize,
    ) -> Result<(DyadPtr, usize, DyadPtr), ParseError> {
        let source = self.cx.source;
        let name = &source[nstart..nstart + nlen];
        let mut field_scope = ScopeStack::new();
        field_scope.push(crate::identities::meta::record_scope_of(record_logos));
        let resolved = field_scope.resolve(self.trie, name).map_err(ParseError::Resolve)?;
        let field = resolved.identity;
        let (fields, _) = crate::identities::instance::layout(record_logos)?;
        let (_, _, offset) = fields
            .iter()
            .copied()
            .find(|&(f, _, _)| f == field)
            .ok_or(ParseError::ExpectedField)?;
        Ok((field, offset, resolved.binding))
    }

    /// A function whose declared return type is `type`: it yields a type,
    /// resolved at comptime.
    ///
    /// # Safety
    /// `callee` must be a resolved dyad from the store.
    unsafe fn returns_type(&self, callee: DyadPtr) -> bool {
        if callee.is_null() || (*callee).ty != self.types.fn_type {
            return false;
        }
        let fields = (*callee).value as *const DyadPtr;
        !fields.is_null() && *fields.add(FN_OUTPUT) == self.types.type_
    }

    /// The call runs on the pass and the result bits are the produced type
    /// node's address (DESIGN ›Build and run are one self-directing pass‹); a
    /// run failure or a non-type result is `NonComptimeTypeCall`.
    ///
    /// # Safety
    /// `call` must be a reduced call node from the store.
    unsafe fn eval_type_call(&mut self, call: DyadPtr) -> Result<DyadPtr, ParseError> {
        // What stands before the call runs first, so the call reads committed state.
        self.drain()?;
        let bits = self.run_on_pass(call).map_err(|e| match e {
            crate::run::RunError::MintFailed(_) => ParseError::Run(e),
            _ => ParseError::NonComptimeTypeCall,
        })?;
        let node = bits as usize as DyadPtr;
        // The bits are read as a node address, so they must be one: bits that
        // were never a node are the checked error, never a dereference.
        if !self.rt.store.contains(node) {
            return Err(ParseError::NonComptimeTypeCall);
        }
        if crate::identities::is_type_value(self.types, node) {
            Ok(node)
        } else {
            Err(ParseError::NonComptimeTypeCall)
        }
    }

    /// The lhs's static type must be a pointer type: a pointer variable or
    /// `&x`, a pointer field place, or another deref whose pointee is a pointer.
    ///
    /// # Safety
    /// `lhs` must be a reduced dyad from the store.
    pub(crate) unsafe fn build_deref(&mut self, lhs: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The pointer expression is stored as it stands (a use of a name is
        // its binding); its type is read through the reading rule.
        let read = self.types.through(lhs);
        let ptr_ty = if (*read).ty == self.types.deref_ {
            crate::identities::pointer::deref_parts(read).1
        } else {
            (*read).ty
        };
        let pointee = if ptr_ty == self.types.plus || ptr_ty == self.types.minus {
            // A pointer step, `(p + k)@`: its pointee is the stepped pointer's.
            match crate::identities::numtype_of(self.types, read) {
                crate::identities::Operand::Pointer(pointee) => pointee,
                _ => return Err(ParseError::UnsupportedOperands),
            }
        } else {
            // The pointee rides on the reading rule's answer.
            let Some((crate::identities::read::Read::Pointer(pointee), _)) =
                crate::identities::read::place_layout(self.types, ptr_ty)
            else {
                return Err(ParseError::UnsupportedOperands);
            };
            pointee
        };
        let types = self.types;
        Ok(crate::identities::pointer::build_deref(self.rt.store, types, lhs, pointee, 0))
    }

    /// `&`'s constructor: an `addr` node that resolves the place's address at
    /// run/lower time, so a frame-relative local yields a per-activation
    /// address. A comptime binding has no storage.
    pub(crate) fn construct_address_of(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let Some(&cell) = tape.at(1) else {
            return Err(ParseError::BadAddressOf);
        };
        // Keywords, operators, literals: not places.
        if !cell.constructed
            && !cell.is_fresh()
            && self.ctor_of(self.cell_identity(&cell)).is_some()
        {
            return Err(ParseError::BadAddressOf);
        }
        let node = self.operand_dyad(cell)?;
        // SAFETY: `node` is a resolved dyad from the store.
        let addr = unsafe {
            // A place the reading rule can name, marked as storage: a literal's
            // blob is not one, nor a node of a code-carrying type (an operand run, no fields).
            use crate::identities::read::{read_kind, Read};
            let placed = matches!(
                read_kind(self.types, node),
                Read::Scalar(_) | Read::Pointer(_) | Read::Aggregate
            ) && crate::dyad::is_place((*node).value);
            if !placed {
                return Err(ParseError::BadAddressOf);
            }
            self.check_capture(node)?;
            crate::identities::pointer::build_addr(self.rt.store, self.types, node)
        };
        tape.remove(1);
        tape.place(addr);
        Ok(Constructed::Placed)
    }

    /// Every further `@` cell to the right, then the base type cell, any type
    /// identity (DESIGN ›Pointer types are prefix `@T`‹), built into the
    /// pointer type. What a place of `@T` reads as never depends on `T`.
    pub(crate) fn construct_pointer_type(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let mut depth = 1usize;
        while matches!(self.cell_at(tape, 1)?, Some(c) if !c.constructed && self.cell_identity(&c) == self.types.at_)
        {
            tape.remove(1);
            depth += 1;
        }
        let Some(cell) = self.cell_at(tape, 1)? else {
            return Err(ParseError::UnsupportedOperands);
        };
        let base = self.operand_dyad(cell)?;
        // SAFETY: `base` is a resolved dyad from the store.
        if !unsafe { crate::identities::is_type_value(self.types, base) } {
            return Err(ParseError::UnsupportedOperands);
        }
        tape.remove(1);
        let mut logos = base;
        for _ in 0..depth {
            // SAFETY: `logos` is the base type node, or the pointer type minted in the previous round.
            logos = unsafe {
                crate::identities::pointer::make_pointer_type(
                    self.rt.store,
                    self.types.type_,
                    logos,
                )
            };
        }
        tape.place(logos);
        Ok(Constructed::Placed)
    }

    /// The callee's own constructor's work (DESIGN ›`X (…)` is one spelling,
    /// and X's constructor decides what the bracket is‹): a numeric callee is
    /// a conversion, a record type constructs an instance, a type-returning callee resolves now.
    ///
    /// # Safety
    /// `callee` and every `arg` must be dyads from the store.
    pub(crate) unsafe fn build_call(
        &mut self,
        callee: DyadPtr,
        args: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        let mut args = args.to_vec();
        for i in 0..args.len() {
            // SAFETY: `args` are reduced dyads from the store.
            if let Some((connective, a, b)) = crate::identities::group::members(self.types, args[i])
            {
                args[i] = a;
                let a = self.build_call(callee, &args)?;
                args[i] = b;
                let b = self.build_call(callee, &args)?;
                return crate::identities::group::join(self.rt.store, self.types, connective, a, b);
            }
        }
        // Fail-closed until ownership-gated parameters let the callee declare
        // that it takes the value.
        for &arg in &args {
            // SAFETY: `args` are reduced dyads just parsed.
            if unsafe { crate::identities::drop_model::is_owning_value(self.types, arg) } {
                return Err(ParseError::UnboundOwningValue);
            }
        }
        // SAFETY: `callee` is a resolved dyad from the store; a record type carries the record `run_body_of` reads.
        let (is_numtype, is_record) = unsafe {
            (
                crate::identities::is_numtype_node(self.types, callee),
                crate::identities::meta::is_record_type(callee),
            )
        };
        if is_numtype {
            // SAFETY: `callee` is a numtype node; `args` are reduced dyads.
            unsafe { crate::identities::build_cast(self.rt.store, self.types, callee, &args) }
        } else if is_record {
            // SAFETY: `callee` is a record type node.
            if unsafe { !crate::identities::meta::run_body_of(callee).is_null() } {
                return Err(ParseError::RunTypeApplied);
            }
            // A record type applied to its field values constructs an
            // instance, like `i32(a)`.
            let types = self.types;
            // SAFETY: `callee` is a record type node; `args` are reduced dyads from the store.
            unsafe {
                // A per-call local sized from the record layout, so a recursive
                // call fills its own copy.
                let (_, size) = crate::identities::instance::layout(callee)?;
                let instance = self.alloc_local(callee, size.max(1));
                crate::identities::instance::build_ctor(
                    self.rt.store,
                    types,
                    types.construct_,
                    callee,
                    instance,
                    &args,
                )
            }
        } else {
            // Each uncommitted literal argument commits to its parameter's
            // declared type; an unbound callee has no signature yet and commits nothing.
            let types = self.types;
            let mut args = args;
            // SAFETY: `callee` and `args` are reduced dyads from the store.
            unsafe {
                crate::identities::commit_call_args(self.rt.store, types, callee, &mut args)?;
            }
            let call = build_call(self.rt.store, callee, &args);
            // SAFETY: `callee` is a reduced dyad; a `fn` callee's value is its field record or null.
            let record_out = unsafe {
                if (*callee).ty == types.fn_type && !(*callee).value.is_null() {
                    let out = *((*callee).value as *const DyadPtr).add(FN_OUTPUT);
                    crate::identities::by_copy::record_width(types, out).map(|w| (out, w))
                } else {
                    None
                }
            };
            if let Some((out, width)) = record_out {
                // The caller provides the slot the record result is copied into.
                let slot = self.alloc_local(out, width);
                return Ok(crate::identities::by_copy::build_result(
                    self.rt.store,
                    types,
                    slot,
                    call,
                ));
            }
            // A type-returning call resolves now, at comptime, so the result
            // flows as an ordinary type value; the outer-name check runs first,
            // since a body that reads a dead name must not run.
            // SAFETY: `callee` is a reduced dyad.
            if unsafe { self.returns_type(callee) } {
                self.check_call_reads(callee)?;
                // Inside a body an argument known only at run leaves the call
                // to run with it, its result a type value at run.
                let comptime = args.iter().all(|&a| {
                    // SAFETY: `args` are reduced dyads from the store.
                    unsafe { crate::identities::is_comptime_arg(types, a) }
                });
                if self.cx.runtime_depth > 0 && !comptime {
                    return Ok(call);
                }
                // SAFETY: `call` was just built over reduced dyads.
                unsafe { self.eval_type_call(call) }
            } else {
                Ok(call)
            }
        }
    }

    /// Mint a scope node carrying its enclosing scope (DESIGN ›Meta-navigation‹)
    /// and push it; the link is set before anything inside is parsed, so a
    /// constructor running inside walks up over settled structure.
    fn open_scope(&mut self) -> DyadPtr {
        let parent = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        let scope = crate::identities::scope::mint(self.rt.store, self.types.scope, parent);
        self.cx.scopes.push(scope);
        scope
    }

    /// One expression is returned as itself; several become a sequence node
    /// `[expr0 … exprN, null]` yielding the trailing one, itself the scope
    /// its declarations live in (DESIGN ›A scope's value is what it evaluates to‹).
    pub fn parse_sequence(&mut self) -> Result<DyadPtr, ParseError> {
        self.parse_sequence_cell().map(|c| c.dyad)
    }

    /// [`Self::parse_sequence`] as the cell, its facts with it.
    pub(crate) fn parse_sequence_cell(&mut self) -> Result<Cell, ParseError> {
        self.parse_block().map(|(_, value)| value)
    }

    /// [`Parser::parse_sequence`], with the scope the block opened, whose `dyads` hold
    /// every line whatever the value collapsed to.
    fn parse_block(&mut self) -> Result<(DyadPtr, Cell), ParseError> {
        // `open` has one entry per open scope, so its length is the nesting
        // depth; past the limit the parse is the checked error, not a Rust stack overflow.
        if self.cx.open.len() >= MAX_BRACKET_DEPTH {
            return Err(ParseError::TooDeep);
        }
        let scope = self.open_scope();
        let narrowed = self.cx.narrow_next.take().into_iter().collect();
        self.cx.open.push(OpenScope { narrowed, ..OpenScope::default() });
        let array_ = self.types.array_;
        // SAFETY: null is a legal cell dyad.
        let mut last = unsafe { Cell::built(std::ptr::null_mut()) };
        while let Some(item) = self.next_cell() {
            last = match item {
                Ok(cell) => cell,
                Err(e) => {
                    self.cx.scopes.pop();
                    self.cx.open.pop();
                    return Err(e);
                }
            };
            let item = last.dyad;
            // SAFETY: the pending bindings were minted by this parser's declares;
            // `scope` is the innermost open scope, minted by `open_scope` above.
            unsafe { self.cx.scopes.close_item(self.rt.store, array_, item) };
            // A binding's `defer free <place>` is drained right after its
            // statement, so the defer sits at its source position, the right
            // LIFO rank among other statements' defers.
            let depth = self.cx.open.len() - 1;
            if !self.cx.open[depth].defers.is_empty() {
                let drained = std::mem::take(&mut self.cx.open[depth].defers);
                for &d in &drained {
                    // SAFETY: `d` is a `defer free <place>` node the binding site
                    // just built; `scope` a scope `open_scope` minted.
                    unsafe {
                        let place = crate::identities::drop_model::teardown_place_of(d);
                        self.cx.open[depth].owned.push(place);
                        crate::identities::scope::push_item(self.rt.store, array_, scope, d);
                    }
                }
            }
        }
        self.cx.scopes.pop();
        // SAFETY: `scope` was minted by `open_scope` with a value, so it has an
        // array; the store outlives the slice and nothing pushes to it below.
        let exprs = unsafe {
            crate::identities::scope::dyads(self.rt.store, array_, scope);
            crate::identities::scope::exprs_of(scope).expect("a block scope has dyads")
        };
        // What the block parsed and did not run runs when the block runs;
        // what it did run stands in the body as its result.
        let owned_here = self.cx.open.pop().expect("pushed above").owned;
        // Prose and a `defer` (it runs at exit, never as the tail) are
        // invisible to value flow.
        let defer_ = self.types.defer_;
        // SAFETY: `exprs` are reduced dyads just parsed/built.
        let is_value = |e: DyadPtr| unsafe {
            !crate::identities::numtype::is_comment_type((*e).ty) && (*e).ty != defer_
        };
        let values = exprs.iter().filter(|&&e| is_value(e)).count();
        match (values, exprs.len()) {
            // An empty `( )`, or a bracket holding only prose: the scope node
            // with nothing to run, yielding unit.
            (0, _) => {
                // SAFETY: `scope` was minted by `open_scope` above and is unaliased.
                let cell = unsafe {
                    crate::identities::scope::fill(scope, self.types.ops.scope_);
                    Cell::built(scope)
                };
                Ok((scope, cell))
            }
            (_, 1) => {
                // SAFETY: `exprs` are reduced dyads from the store.
                let expr = unsafe { Cell::built(exprs[0]) };
                Ok((scope, if last.dyad == exprs[0] { last } else { expr }))
            }
            _ => {
                // Outside a function a `return` before the tail has nothing to leave.
                let types = self.types;
                let tail = exprs.iter().rposition(|&e| is_value(e)).expect("values >= 1");
                for (i, &e) in exprs.iter().enumerate() {
                    if i != tail
                        && self.cx.frames.is_empty()
                        // SAFETY: `e` is a reduced dyad just parsed.
                        && unsafe { contains_return(types, e) }
                    {
                        return Err(ParseError::EarlyReturn);
                    }
                }
                // A last value that is a place this scope owns moves out to
                // whoever takes the value, so the teardown on the way out finds
                // it empty; a `return` of one would hand out freed memory. Only
                // places this scope frees count; an enclosing scope's owning place is an ordinary borrow.
                // SAFETY: `exprs[tail]` is a reduced dyad just parsed.
                let (tail_value, returned) = unsafe {
                    let t = exprs[tail];
                    // `return x` yields `x`, so the escape rides its operand.
                    if (*t).ty == types.return_ && !(*t).value.is_null() {
                        (*((*t).value as *const DyadPtr), true)
                    } else {
                        (t, false)
                    }
                };
                // SAFETY: `tail_value` is a reduced dyad from the store.
                let place = unsafe { types.through(tail_value) };
                if owned_here.contains(&place) {
                    if returned {
                        return Err(ParseError::OwningEscape);
                    }
                    // SAFETY: `place` is a place this scope's binding site minted, and
                    // `tail` indexes the scope's own lines, which nothing else reads yet.
                    unsafe {
                        let moved = if crate::identities::drop_model::is_owning_place(place) {
                            crate::identities::drop_model::build_teardown(
                                self.rt.store,
                                types,
                                types.own_,
                                place,
                                true,
                            )?
                        } else {
                            crate::identities::drop_model::build_instance_own(
                                self.rt.store,
                                types,
                                place,
                                (*place).ty,
                            )
                        };
                        let (_, lines) = crate::identities::array::parts(
                            crate::identities::scope::exprs_array(scope),
                        );
                        *(lines as *mut DyadPtr).add(tail) = moved;
                    }
                }
                // SAFETY: `scope` was minted by `open_scope` above and is unaliased.
                let cell = unsafe {
                    crate::identities::scope::fill(scope, self.types.ops.scope_);
                    Cell::built(scope)
                };
                Ok((scope, cell))
            }
        }
    }

    /// `#` followed by a `«…»` string or raw text to the end of the line
    /// (DESIGN ›Text literals are plain values; `#` is the one comment constructor‹).
    pub(crate) fn construct_comment(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let node = self.comment_after_hash()?;
        tape.place(node);
        Ok(Constructed::Placed)
    }

    fn comment_after_hash(&mut self) -> Result<DyadPtr, ParseError> {
        let bytes = self.cx.source.as_bytes();
        // Spaces (not the newline) may separate `#` from its text.
        while self.cx.pos < bytes.len() && matches!(bytes[self.cx.pos], b' ' | b'\t') {
            self.cx.pos += 1;
        }
        let source = self.cx.source;
        let text_node = if source[self.cx.pos..].starts_with('«') {
            // `# «…»`: the string form ends at the `»`, not the line.
            let r = self
                .cx
                .scopes
                .resolve(self.trie, &source[self.cx.pos..])
                .map_err(ParseError::Resolve)?;
            let start = self.cx.pos;
            self.cx.pos += r.matched;
            self.construct_leaf(r.identity, start, r.matched)?.ok_or(ParseError::BadLiteral)?
        } else {
            let start = self.cx.pos;
            while self.cx.pos < bytes.len() && bytes[self.cx.pos] != b'\n' {
                self.cx.pos += 1;
            }
            let text = source[start..self.cx.pos].trim_end();
            crate::identities::string::build_text(
                self.rt.store,
                self.types.string_,
                text.as_bytes(),
            )
        };
        Ok(self.rt.store.alloc_raw(self.types.comment_, text_node.cast()))
    }

    /// Consume the path token (raw text up to whitespace or `,`, or a `«…»`
    /// string), load the file, and place `{type: import, value: [path, tail,
    /// op]}`. The load happens here, once per run; the node's run only re-yields the tail.
    pub(crate) fn construct_import(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // The load is a comptime effect: inside a deferred-or-repeated body
        // parse order and run order do not coincide.
        if self.cx.runtime_depth != 0 {
            return Err(ParseError::ImportInRuntimeBody);
        }
        self.skip_whitespace();
        let source = self.cx.source;
        let start = self.cx.pos;
        let path_text: String = if source[self.cx.pos..].starts_with('«') {
            let r = self
                .cx
                .scopes
                .resolve(self.trie, &source[self.cx.pos..])
                .map_err(ParseError::Resolve)?;
            let s = self.cx.pos;
            self.cx.pos += r.matched;
            let node =
                self.construct_leaf(r.identity, s, r.matched)?.ok_or(ParseError::ExpectedPath)?;
            // SAFETY: the leaf just built is a string node.
            String::from_utf8_lossy(unsafe { crate::identities::string::text(node) }).into_owned()
        } else {
            let bytes = source.as_bytes();
            while self.cx.pos < bytes.len()
                && !bytes[self.cx.pos].is_ascii_whitespace()
                && bytes[self.cx.pos] != b','
            {
                self.cx.pos += 1;
            }
            source[start..self.cx.pos].to_string()
        };
        if path_text.is_empty() {
            self.cx.pos = start;
            return Err(ParseError::ExpectedPath);
        }
        let tail = self.import_file(&path_text)?;
        let types = self.types;
        let path_node = crate::identities::string::build_text(
            self.rt.store,
            types.string_,
            path_text.as_bytes(),
        );
        let value = self.rt.store.alloc_operands(&[path_node, tail, types.ops.import_]);
        let node = self.rt.store.alloc_raw(types.import_, value);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Read the `«…»` to the right at discovery and place a `regex` node
    /// holding the pattern's bytes (DESIGN ›The scope's constructor is the
    /// driver‹). Compiled here, so a bad pattern errors at its quote, since the trie compiles lazily.
    pub(crate) fn construct_regex(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        self.skip_whitespace();
        let source = self.cx.source;
        let start = self.cx.pos;
        if !source[start..].starts_with('«') {
            return Err(ParseError::ExpectedPattern);
        }
        let r = self.cx.scopes.resolve(self.trie, &source[start..]).map_err(ParseError::Resolve)?;
        self.cx.pos += r.matched;
        let quote = self
            .construct_leaf(r.identity, start, r.matched)?
            .ok_or(ParseError::ExpectedPattern)?;
        // SAFETY: the leaf just built is a string node.
        let pattern = unsafe { crate::identities::string::text(quote) }.to_vec();
        let text = String::from_utf8_lossy(&pattern);
        let bad = if pattern.is_empty() {
            Some("a pattern must not be empty".to_string())
        } else {
            // The engine's report ends in its one-line reason; the lines
            // before repeat the pattern with a caret.
            regex::bytes::Regex::new(&format!("^(?:{text})")).err().map(|e| {
                e.to_string()
                    .lines()
                    .last()
                    .unwrap_or("")
                    .trim()
                    .trim_start_matches("error: ")
                    .to_string()
            })
        };
        if let Some(reason) = bad {
            self.cx.pos = start;
            return Err(ParseError::BadPattern(reason));
        }
        let node =
            crate::identities::string::build_text(self.rt.store, self.types.regex_, &pattern);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Read the `«…»` to the right at discovery and place the node that lexes
    /// it when it runs (DESIGN ›Text is the quote‹): the cells are made at
    /// each run, against the scopes open then.
    pub(crate) fn construct_lex(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let quote = self.quote_after("lex")?;
        let node = crate::identities::lex::build(self.rt.store, self.types, quote);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    pub(crate) fn construct_print(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let parts = self.interpolated_quote("print")?;
        let node = crate::identities::print::build(self.rt.store, self.types, &parts);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    pub(crate) fn construct_error(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let parts = self.interpolated_quote("error")?;
        let node = crate::identities::error::build(self.rt.store, self.types, &parts);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// The `«…»` to the right of `print` or `error`, read into its parts.
    fn interpolated_quote(&mut self, word: &'static str) -> Result<Vec<DyadPtr>, ParseError> {
        let quote = self.quote_after(word)?;
        let end = self.cx.pos;
        // SAFETY: `quote` is the string node the `«…»` constructor just built.
        let len = unsafe { crate::identities::string::text(quote) }.len();
        // The literal has no escapes, so its text is the source between the guillemets.
        let inner = end - 2 - len;
        let parts = self.quote_parts(inner, inner + len)?;
        self.cx.pos = end;
        Ok(parts)
    }

    /// A `print` or `error` quote's text runs and `{…}` expressions, each
    /// expression parsed here, in the scope the word appears in; `\{` and `\}`
    /// are the braces as text.
    fn quote_parts(&mut self, from: usize, to: usize) -> Result<Vec<DyadPtr>, ParseError> {
        let source = self.cx.source;
        let bytes = source.as_bytes();
        let mut parts = Vec::new();
        let mut text = Vec::new();
        let mut i = from;
        while i < to {
            match bytes[i] {
                b'\\' if i + 1 < to && matches!(bytes[i + 1], b'{' | b'}') => {
                    text.push(bytes[i + 1]);
                    i += 2;
                }
                b'}' => {
                    self.cx.pos = i;
                    return Err(ParseError::StrayInterpolationClose);
                }
                b'{' => {
                    let Some(close) = source[i + 1..to].find('}').map(|k| i + 1 + k) else {
                        self.cx.pos = i;
                        return Err(ParseError::UnclosedInterpolation);
                    };
                    if !text.is_empty() {
                        let t = crate::identities::string::build_text(
                            self.rt.store,
                            self.types.string_,
                            &text,
                        );
                        parts.push(t);
                        text.clear();
                    }
                    parts.push(self.parse_within(i + 1, close)?);
                    i = close + 1;
                }
                b => {
                    text.push(b);
                    i += 1;
                }
            }
        }
        if !text.is_empty() || parts.is_empty() {
            parts.push(crate::identities::string::build_text(
                self.rt.store,
                self.types.string_,
                &text,
            ));
        }
        Ok(parts)
    }

    /// One expression over `source[from..to]` alone: the source is cut at `to`
    /// so the segment ends there, and offsets stay those of the whole line.
    fn parse_within(&mut self, from: usize, to: usize) -> Result<DyadPtr, ParseError> {
        let whole = self.cx.source;
        self.cx.source = &whole[..to];
        self.cx.pos = from;
        let parsed = self.parse_expression().and_then(|expr| {
            self.skip_whitespace();
            if self.cx.pos < to {
                Err(ParseError::Trailing)
            } else {
                Ok(expr)
            }
        });
        self.cx.source = whole;
        parsed
    }

    /// The `«…»` string node to the right of a raw-text word, consumed at discovery.
    fn quote_after(&mut self, word: &'static str) -> Result<DyadPtr, ParseError> {
        self.skip_whitespace();
        let source = self.cx.source;
        let start = self.cx.pos;
        if !source[start..].starts_with('«') {
            return Err(ParseError::ExpectedQuote(word));
        }
        let r = self.cx.scopes.resolve(self.trie, &source[start..]).map_err(ParseError::Resolve)?;
        self.cx.pos += r.matched;
        self.construct_leaf(r.identity, start, r.matched)?.ok_or(ParseError::ExpectedQuote(word))
    }

    /// The node placed at discovery carries the scope open at its appearance
    /// (DESIGN ›Meta-navigation‹): inside a constructor's body the definition
    /// site; the use site is `caller.scope`.
    pub(crate) fn construct_here(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let scope = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        let node = crate::identities::here::build_here(self.rt.store, self.types, scope);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// The node whose `.scope` reads the pass's position when a constructor
    /// runs it (DESIGN ›Meta-navigation‹).
    pub(crate) fn construct_caller(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let node = crate::identities::here::build_caller(self.rt.store, self.types);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// On a first load the file runs in its own section, a fresh stack of the
    /// root plus a section scope, so it sees ambient names and its own imports
    /// only (DESIGN ›Importing is dropping the text there‹); its `pub` names then land here.
    fn import_file(&mut self, path_text: &str) -> Result<DyadPtr, ParseError> {
        let joined = self.cx.dir.join(path_text);
        let canon = joined
            .canonicalize()
            .map_err(|e| ParseError::ImportRead(format!("{}: {e}", joined.display())))?;
        match self.imports.entries.get(&canon) {
            Some(ImportState::Loading) => {
                return Err(ParseError::ImportCycle(path_text.to_string()))
            }
            Some(ImportState::Loaded { pubs, tail }) => {
                let (pubs, tail) = (pubs.clone(), *tail);
                self.publish(&pubs)?;
                return Ok(tail);
            }
            None => {}
        }
        let text = std::fs::read_to_string(&canon)
            .map_err(|e| ParseError::ImportRead(format!("{}: {e}", joined.display())))?;
        // Sources are process-lived: every span and every later report
        // indexes into its file's text.
        let text: &'static str = Box::leak(text.into_boxed_str());
        self.imports.entries.insert(canon.clone(), ImportState::Loading);

        // A fresh stack of the root (ambient names) plus a fresh scope node
        // the file's declarations land in.
        let root = *self.cx.scopes.open.first().expect("an import site has an open root scope");
        let mut nested = ScopeStack::new();
        nested.push(root);
        let section = nested.push_section(self.rt.store, self.types.scope);
        self.imports.sections.insert(section);

        let cx = Context {
            dir: canon.parent().map(Into::into).unwrap_or_else(|| PathBuf::from(".")),
            ..Context::nested(&self.cx, text, nested)
        };
        let g = self.enter(cx);
        // An import is text dropped in place (DESIGN ›Importing is dropping the text there‹):
        // it shares the importer's bracket stack, so its `drain` runs the items in source order.
        g.0.cx.open = std::mem::take(&mut g.0.outer.last_mut().expect("pushed by enter").open);
        let inner = g.0.run_imported();
        let inner_pos = g.0.cx.pos;
        g.0.outer.last_mut().expect("pushed by enter").open = std::mem::take(&mut g.0.cx.open);
        drop(g);

        match inner {
            Err(message) => {
                // Remove the Loading entry so a later attempt (a REPL retry)
                // reports the real failure, not a phantom cycle.
                self.imports.entries.remove(&canon);
                Err(ParseError::ImportFailed {
                    path: path_text.to_string(),
                    rendered: crate::report::render(path_text, text, inner_pos, &message),
                })
            }
            Ok((pubs, tail)) => {
                self.imports
                    .entries
                    .insert(canon, ImportState::Loaded { pubs: pubs.clone(), tail });
                self.publish(&pubs)?;
                Ok(tail)
            }
        }
    }

    /// The nested pass over an imported file: each statement pending, the
    /// file run at its end, collecting the `pub` (name, identity) pairs and
    /// the tail node. On failure the message is returned with `offset` at the stuck point.
    fn run_imported(&mut self) -> Result<(Vec<(String, DyadPtr)>, DyadPtr), String> {
        let mut pubs = Vec::new();
        let mut tail = std::ptr::null_mut();
        while let Some(item) = self.parse_next() {
            let node = item.map_err(|e| crate::report::parse_message(&e))?;
            // SAFETY: `node` is the line `parse_next` just returned.
            unsafe { self.close_item(node) };
            // SAFETY: `node` was just parsed into the store, which outlives the pass.
            unsafe {
                if (*node).ty != self.types.comment_ {
                    tail = node;
                }
                if (*node).ty == self.types.declare_ {
                    let name = Binding::spelling(crate::identities::declare::binding_of(node));
                    let resolved = self
                        .cx
                        .scopes
                        .resolve(self.trie, &name)
                        .map_err(|_| format!("name `{name}` did not stay resolvable"))?;
                    if Binding::has_gate(resolved.binding, self.types.pub_) {
                        pubs.push((name, resolved.identity));
                    }
                }
            }
        }
        // A stray `)` ends the loop without being consumed, as in the drivers.
        if !self.cx.source[self.cx.pos..].trim_start().is_empty() {
            return Err("unexpected `)` — no scope is open here".to_string());
        }
        self.drain().map_err(|e| crate::report::parse_message(&e))?;
        Ok((pubs, tail))
    }

    /// Pub-only exposure, the ordinary visibility rule: a collision with a
    /// live name is the ordinary shadowing error. Idempotent where the name
    /// already resolves to the same identity.
    fn publish(&mut self, pubs: &[(String, DyadPtr)]) -> Result<(), ParseError> {
        // A collision is the import's: reported where the import stands.
        let at = self.cx.pos;
        for (name, identity) in pubs {
            if let Ok(r) = self.cx.scopes.resolve(self.trie, name) {
                if r.identity == *identity {
                    continue;
                }
            }
            self.declare_name(name, *identity, at)?;
        }
        Ok(())
    }

    /// `:`'s constructor (DESIGN ›The dyad's read surface‹): a field of the
    /// binding to the left, the cell taken as it stands, never through the
    /// reading rule; the member is the raw spelling at the cell to the right.
    pub(crate) fn construct_binding_read(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let lhs = match tape.at(-1).copied() {
            Some(cell) => self.as_operand(cell)?,
            None => return Err(ParseError::MissingOperand),
        };
        let Some(m) = self.cell_at(tape, 1)? else {
            return Err(ParseError::ExpectedField);
        };
        let save = self.cx.pos;
        self.cx.pos = m.start;
        let member = self.lex_spelling();
        self.cx.pos = save;
        let Some((nstart, nlen)) = member else {
            return Err(ParseError::ExpectedField);
        };
        // SAFETY: `lhs` is a dyad off the tape.
        let node = unsafe { self.binding_read_each(lhs, nstart, nlen)? };
        tape.remove(1);
        tape.remove(-1);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// A group's read is each member's; a name reads its own binding, never what it holds.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn binding_read_each(
        &mut self,
        lhs: DyadPtr,
        nstart: usize,
        nlen: usize,
    ) -> Result<DyadPtr, ParseError> {
        let group = if (*lhs).ty == self.types.binding_ {
            None
        } else {
            crate::identities::group::members(self.types, lhs)
        };
        let Some((connective, a, b)) = group else {
            return self.binding_read(lhs, nstart, nlen);
        };
        let a = self.binding_read_each(a, nstart, nlen)?;
        let b = self.binding_read_each(b, nstart, nlen)?;
        crate::identities::group::join(self.rt.store, self.types, connective, a, b)
    }

    /// On a binding, `type` is the type of the dyad the name stands for,
    /// `start` the declaring line once it is complete, and any other field the
    /// instance-field read in `binding`'s own scope; on a constructed node
    /// `type` is its own and the path answers the rest (DESIGN ›Meta-navigation‹),
    /// `start`/`end` null.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn binding_read(
        &mut self,
        lhs: DyadPtr,
        nstart: usize,
        nlen: usize,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let source = self.cx.source;
        let name = &source[nstart..nstart + nlen];
        // The binding's `dyad` field is storage, not a spelling.
        if name == "dyad" || name == "value" {
            return Err(ParseError::CellNotReachable);
        }
        // A name answers from its own binding; a node reached by path from itself.
        let lhs = if (*lhs).ty == types.binding_ {
            lhs
        } else {
            self.known_node(lhs, std::ptr::null_mut()).unwrap_or(lhs)
        };
        // Read when the constructor runs.
        if (*lhs).ty == types.tape.cell_dyad_at && name == "type" {
            return Ok(crate::identities::tape::build_cell_type(self.rt.store, types, lhs));
        }
        if crate::identities::tape::is_cell_read(types, lhs) {
            if name == "type" {
                return Ok(crate::identities::tape::build_cell_type(self.rt.store, types, lhs));
            }
            // `t[k]:name`: the spelling of the binding the cell holds.
            if name == "name" {
                return Ok(crate::identities::tape::build_slot_name(self.rt.store, types, lhs));
            }
            return Err(ParseError::ExpectedField);
        }
        if (*lhs).ty == types.binding_ {
            if name == "type" {
                // Through a settled box: the type of what the name holds.
                let cell = self.settled_type(Binding::read(lhs).dyad);
                if cell.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                return Ok((*cell).ty);
            }
            // `b:start`: the declaring line, once it is complete, is a node
            // like any other (DESIGN ›The dyad's read surface‹).
            let start = Binding::read(lhs).start;
            if name == "start" && !start.is_null() {
                return Ok(self.path_operand(start));
            }
            let (field, offset, _) = self.resolve_field(types.binding_, nstart, nlen)?;
            let addr = (*lhs).value.wrapping_add(offset);
            // `a:name`: a `string`-typed place over the slot, marked as one so
            // the reading rule sees the container it is (no place of type `string` exists in a layout).
            if name == "name" {
                let place = crate::dyad::global_place(addr);
                return Ok(self.rt.store.alloc_raw(types.string_, place));
            }
            // `a:lex_rank`: an `f64` place a user may write, so it carries the
            // place mark `=` asks for.
            if name == "lex_rank" {
                let place = crate::dyad::global_place(addr);
                return Ok(self.rt.store.alloc_raw((*field).ty, place));
            }
            return Ok(self.rt.store.alloc_raw((*field).ty, addr));
        }
        // `tape[0].f:type` is the field's declared type, not the type of the read that reaches it;
        // an untyped field's type is the written value's, unknown until the constructor runs.
        if crate::identities::this::is_field_read(types, lhs) && name == "type" {
            let declared = if crate::identities::this::is_fill(types, lhs) {
                (*Binding::read(crate::identities::this::field_binding_of(types, lhs)).dyad).ty
            } else {
                std::ptr::null_mut()
            };
            if declared.is_null() {
                return Err(ParseError::BadReflectRead);
            }
            return Ok(declared);
        }
        let value = match name {
            "type" => return Ok((*lhs).ty),
            "scope" => self.cx.scopes.current().unwrap_or(std::ptr::null_mut()),
            "start" | "end" | "gate" => std::ptr::null_mut(),
            _ => {
                // Not a field of a binding: the same error a binding read gives.
                self.resolve_field(types.binding_, nstart, nlen)?;
                return Err(ParseError::ExpectedField);
            }
        };
        Ok(self.address_value(types.dyad_, value))
    }

    /// A pointer-typed literal with its own eight bytes of storage, read at
    /// run time like any pointer variable.
    fn address_value(&mut self, pointee: DyadPtr, addr: DyadPtr) -> DyadPtr {
        crate::identities::pointer::address_value(self.rt.store, self.types, pointee, addr)
    }

    /// `node` reached by path: as a value its address, so using it runs nothing;
    /// the reads after it fold through [`Parser::known_node`].
    fn reach(&mut self, node: DyadPtr) -> DyadPtr {
        self.address_value(self.types.dyad_, node)
    }

    /// The node an `@dyad` value stands for when the parse knows it: an
    /// address no `=` writes, or a name `root` no `=` can move, followed through
    /// its declaring line to the right side as written (DESIGN ›The dyad's read
    /// surface‹: a binding gets you a dyad or a node).
    ///
    /// # Safety
    /// `value` must be a dyad from the store; `root` null or a binding dyad.
    unsafe fn known_node(&self, value: DyadPtr, root: DyadPtr) -> Option<DyadPtr> {
        use crate::identities::{declare, read};
        let types = self.types;
        if !root.is_null() {
            if Binding::has_gate(root, types.mut_) {
                return None;
            }
            let start = Binding::read(root).start;
            if start.is_null()
                || (*start).ty != types.declare_
                || declare::binding_of(start) != root
            {
                return None;
            }
            let rhs = declare::rhs_of(start);
            let next = if (*rhs).ty == types.binding_ { rhs } else { std::ptr::null_mut() };
            return self.known_node(types.through(rhs), next);
        }
        let stored = (*value).value;
        if stored.is_null()
            || crate::dyad::is_place(stored)
            || !matches!(read::read_kind(types, value), read::Read::Pointer(p) if p == types.dyad_)
        {
            return None;
        }
        let node = *(stored as *const DyadPtr);
        (!node.is_null() && self.rt.store.contains(node)).then_some(node)
    }

    /// Exactly the cell's two fields, `.type` and `.value` (DESIGN ›The dyad's
    /// read surface‹); nothing else reads through the view, and nothing here writes.
    ///
    /// # Safety
    /// `view` must be a node of type `dyad`.
    unsafe fn view_member(&mut self, view: DyadPtr, name: &str) -> Result<DyadPtr, ParseError> {
        let viewed = (*view).value as DyadPtr;
        if viewed.is_null() {
            return Err(ParseError::BadReflectRead);
        }
        match name {
            "type" => Ok((*viewed).ty),
            // The raw address as a u64; the `@void` spelling waits for pointer-value plumbing.
            "value" => Ok(self.scalar_value(
                crate::identities::numtype::NumType::U64,
                (*viewed).value as usize as i64,
            )),
            _ => Err(ParseError::BadReflectRead),
        }
    }

    /// The shared metadata stored once per type: `.arity`, `.roles[i]`,
    /// `.parse_rank`, `.associativity`, `.parse`, `.run`, a `share` member,
    /// `.size_bytes`, `.scope`; a null slot is the honest undefined and errors until `?` exists.
    ///
    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn logos_member(
        &mut self,
        logos: DyadPtr,
        name: &str,
        index: Option<usize>,
    ) -> Result<DyadPtr, ParseError> {
        use crate::identities::meta;
        use crate::identities::numtype::NumType;
        if meta::kind_of(logos).is_none() {
            return Err(ParseError::BadReflectRead);
        }
        let operand_kind = matches!(meta::kind_of(logos), Some(meta::TUPLE_TAG | meta::LIST_TAG));
        match name {
            "arity" if operand_kind => {
                Ok(self.scalar_value(NumType::I64, meta::arity_of(logos) as i64))
            }
            "roles" if operand_kind => {
                let i = index.ok_or(ParseError::ExpectedIndexBracket)?;
                if i >= meta::arity_of(logos) {
                    return Err(ParseError::BadReflectRead);
                }
                Ok(meta::role_of(logos, i))
            }
            "parse_rank" => {
                Ok(self.scalar_value(NumType::F64, meta::parse_rank_of(logos).to_bits() as i64))
            }
            // `lex_rank` is the name's binding's, not the type's, so it falls to
            // the unknown-member error below. Associativity's values are the identities `left` and `right`.
            "associativity" => Ok(match meta::assoc_of(logos) {
                Assoc::Left => self.types.left_,
                Assoc::Right => self.types.right_,
            }),
            // The Logos function a body filled the slot with, or the view of a native leaf.
            "parse" => {
                let c = meta::constructor_of(logos);
                if c.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                if (*c).ty == self.types.fn_type {
                    return Ok(c);
                }
                Ok(self.rt.store.alloc_raw(self.types.dyad_, c as *mut u8))
            }
            // The held body is no function until a node's field types construct
            // it, so it is read through a view, as `.parse` reads a native leaf.
            "run" => {
                let held = meta::run_body_of(logos);
                if held.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                Ok(self.rt.store.alloc_raw(self.types.dyad_, held as *mut u8))
            }
            "size_bytes" if meta::is_record_type(logos) => {
                Ok(self.scalar_value(NumType::U64, meta::record_size_of(logos) as i64))
            }
            "size_bytes" => match crate::identities::read::place_layout(self.types, logos) {
                Some((
                    crate::identities::read::Read::Scalar(_)
                    | crate::identities::read::Read::Pointer(_),
                    width,
                )) => Ok(self.scalar_value(NumType::U64, width as i64)),
                _ => Err(ParseError::BadReflectRead),
            },
            "scope" if meta::is_record_type(logos) => Ok(self
                .rt
                .store
                .alloc_raw(self.types.dyad_, meta::record_scope_of(logos) as *mut u8)),
            "type" => Err(ParseError::TypeIsColonRead),
            _ if self.share_member_of(logos, name).is_some() => {
                Ok(self.share_member_read(logos, name).expect("found just above"))
            }
            _ if self.per_node_field_of(logos, name) => {
                Err(ParseError::PerNodeThroughType(name.to_string()))
            }
            _ => Err(ParseError::BadReflectRead),
        }
    }

    /// A `share` member of `logos`, one place stored with the type, read
    /// through the type or a node alike: the member's own gate decides a
    /// write, since the place is no node's.
    ///
    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn share_member_read(&mut self, logos: DyadPtr, name: &str) -> Option<DyadPtr> {
        let (identity, binding) = self.share_member_of(logos, name)?;
        self.cx.member_path = vec![binding];
        Some(identity)
    }

    /// The element type a type of node values built from a bracket holds; stand-in for #152.
    ///
    /// # Safety
    /// `ty` must be a type identity node from the store.
    unsafe fn bracket_element_type(&self, ty: DyadPtr) -> Option<DyadPtr> {
        let (identity, _) = self.share_member_of(ty, "element_type")?;
        let element = self.types.through(identity);
        crate::identities::is_type_value(self.types, element).then_some(element)
    }

    /// The identity and binding of `logos`'s `share` member `name`, declared
    /// in the definition body's scope.
    ///
    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn share_member_of(&self, logos: DyadPtr, name: &str) -> Option<(DyadPtr, DyadPtr)> {
        use crate::identities::meta;
        if !meta::is_record_type(logos) {
            return None;
        }
        let body = meta::record_body_of(logos);
        if body.is_null() {
            return None;
        }
        let mut members = ScopeStack::new();
        members.push(body);
        let r = members.resolve(self.trie, name).ok()?;
        Some((r.identity, r.binding))
    }

    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn per_node_field_of(&self, logos: DyadPtr, name: &str) -> bool {
        use crate::identities::meta;
        if !meta::is_record_type(logos) {
            return false;
        }
        let mut fields = ScopeStack::new();
        fields.push(meta::record_scope_of(logos));
        fields.resolve(self.trie, name).is_ok()
    }

    /// Fresh storage holding `bits` at `nt`'s width: the reflection counts
    /// and measures are ordinary typed values, comparable with literals.
    fn scalar_value(&mut self, nt: crate::identities::numtype::NumType, bits: i64) -> DyadPtr {
        let ty = self.types.numtypes[nt as usize];
        let width = nt.bytes();
        let bytes = bits.to_ne_bytes();
        let storage = self.rt.store.alloc_bytes(&bytes[..width]);
        self.rt.store.alloc_raw(ty, storage)
    }
}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)] // a test reads the nodes it built a line above
mod tests {
    use super::*;

    /// A binding dyad for `identity`, leaked like the dyads below.
    fn rec(identity: DyadPtr) -> DyadPtr {
        rec_in(identity, std::ptr::null_mut())
    }

    fn rec_in(identity: DyadPtr, scope: DyadPtr) -> DyadPtr {
        let fields = Box::into_raw(Box::new(Binding::new(identity, scope, std::ptr::null_mut())));
        Box::into_raw(Box::new(crate::dyad::Dyad {
            ty: std::ptr::null_mut(),
            value: fields as *mut u8,
        }))
    }

    /// The fields behind a binding dyad.
    fn f(binding: DyadPtr) -> Binding {
        // SAFETY: only `rec`-built dyads reach the trie in these tests.
        unsafe { Binding::read(binding) }
    }

    /// A distinct sentinel address per tag, never dereferenced.
    fn dyad(tag: usize) -> DyadPtr {
        std::ptr::without_provenance_mut(tag)
    }

    fn dyad_cells(tags: &[usize]) -> Vec<Cell> {
        tags.iter().map(|&t| unsafe { Cell::built(dyad(t)) }).collect()
    }

    #[test]
    fn offset_indexing_is_cursor_relative() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12, 13]));
        t.set_cursor(2);
        assert_eq!(t.at(0).unwrap().dyad, dyad(12));
        assert_eq!(t.at(-1).unwrap().dyad, dyad(11));
        assert_eq!(t.at(1).unwrap().dyad, dyad(13));
        assert_eq!(t.at(-2).unwrap().dyad, dyad(10));
        assert!(t.at(2).is_none());
        assert!(t.at(-3).is_none());
        assert_eq!(t.cursor(), 2);
    }

    #[test]
    fn insert_left_keeps_cursor_on_same_cell() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1);
        t.insert(0, unsafe { Cell::built(dyad(99)) });
        assert_eq!(t.at(0).unwrap().dyad, dyad(11));
        assert_eq!(t.at(-1).unwrap().dyad, dyad(99));
        assert_eq!(t.len(), 4);
        assert_eq!(t.cursor(), 2);
    }

    #[test]
    fn insert_right_leaves_cursor() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1);
        t.insert(1, unsafe { Cell::built(dyad(99)) });
        assert_eq!(t.at(0).unwrap().dyad, dyad(11));
        assert_eq!(t.at(1).unwrap().dyad, dyad(99));
        assert_eq!(t.at(2).unwrap().dyad, dyad(12));
    }

    #[test]
    fn remove_left_keeps_cursor_on_same_cell() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(2);
        let gone = t.remove(-1);
        assert_eq!(gone.unwrap().dyad, dyad(11));
        assert_eq!(t.at(0).unwrap().dyad, dyad(12));
        assert_eq!(t.at(-1).unwrap().dyad, dyad(10));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn removing_the_center_moves_it_to_the_next_cell_or_past_the_end() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1);
        t.remove(0);
        assert_eq!(t.at(0).unwrap().dyad, dyad(12));
        t.remove(0);
        assert!(t.at(0).is_none(), "past the end");
        assert_eq!(t.at(-1).unwrap().dyad, dyad(10), "the tail is still behind the center");
        assert_eq!(t.cursor(), t.len());
        t.insert(1, unsafe { Cell::built(dyad(7)) });
        assert_eq!(t.last().unwrap().dyad, dyad(7));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn unconstructed_and_constructed_cells_coexist() {
        // Told apart by the tape's own flag, never by the dyad.
        let mut t = ParsingTape::new();
        t.insert(0, unsafe { Cell::lexed(dyad(3), "abc", 0, 3) });
        t.insert(1, unsafe { Cell::built(dyad(7)) });
        assert_eq!(t.is_constructed(0), Some(false));
        assert_eq!(t.is_constructed(1), Some(true));
        assert_eq!(t.own_text(), Some("abc"));
        assert_eq!(t.spelling(1), Some(""), "a built cell answers the empty text");
        assert_eq!(t.at(1).unwrap().dyad, dyad(7));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn a_cell_is_rewritable_in_place_until_placed() {
        let mut t = ParsingTape::from_cells(vec![
            unsafe { Cell::lexed(dyad(1), "a b c", 0, 1) },
            unsafe { Cell::lexed(dyad(2), "a b c", 2, 1) },
            unsafe { Cell::lexed(dyad(3), "a b c", 4, 1) },
        ]);
        t.set_cursor(1);
        t.at_mut(1).unwrap().len = 2;
        assert_eq!(t.at(1).unwrap().len, 2);
        t.reduce_here(dyad(9));
        assert_eq!(t.len(), 1);
        let c = *t.at(0).unwrap();
        assert!(c.constructed);
        assert_eq!((c.dyad, c.start, c.len), (dyad(9), 0, 6));
        t.recenter(0);
        assert_eq!(t.cursor(), 0);
    }

    /// Parse one expression and run it on a runtime with the lexer attached.
    fn go(
        src: &str,
        store: &mut crate::store::Store,
        trie: &mut crate::regex_trie::RegexTrie,
        types: &Core,
        scopes: ScopeStack,
    ) -> (i64, ScopeStack) {
        let mut p = Parser::new(src, store, trie, types, scopes);
        let node = p.parse_expression().unwrap_or_else(|e| panic!("{src}: {e:?}"));
        let scopes = p.into_scopes();
        let mut rt = crate::run::Runtime::new(types, store);
        // SAFETY: `node` was just parsed into the store.
        let v = rt.hosting(&scopes, trie, None, |rt| unsafe { rt.run(node) });
        (v.unwrap_or_else(|e| panic!("{src}: {e:?}")), scopes)
    }

    #[test]
    fn a_scope_carries_its_enclosing_scope() {
        let mut store = crate::store::Store::new();
        let mut trie = crate::regex_trie::RegexTrie::new();
        let core = crate::identities::Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let mut p = Parser::new("(1, 2, (3, 4))", &mut store, &mut trie, types, scopes);
        let outer = p.parse_expression().unwrap();
        // SAFETY: the nodes were just parsed into the store.
        unsafe {
            use crate::identities::scope::{exprs_of, parent_of};
            assert_eq!((*outer).ty, types.scope);
            let inner = exprs_of(outer).expect("a sequence")[2];
            assert_eq!((*inner).ty, types.scope);
            assert_eq!(parent_of(inner), outer);
            assert_eq!(parent_of(outer), core.root_scope);
            assert!(parent_of(core.root_scope).is_null(), "the arche encloses nothing");
        }
    }

    #[test]
    fn here_is_where_the_line_is_written() {
        let mut store = crate::store::Store::new();
        let mut trie = crate::regex_trie::RegexTrie::new();
        let core = crate::identities::Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let (root, scopes) = go("here.scope", &mut store, &mut trie, types, scopes);
        assert_eq!(root as usize, core.root_scope as usize);
        let (above, scopes) = go("here.scope.back", &mut store, &mut trie, types, scopes);
        assert_eq!(above, 0, "the arche has no enclosing scope");
        // Past the arche is the checked error, never a dereference.
        let mut p = Parser::new("here.scope.back.back", &mut store, &mut trie, types, scopes);
        let node = p.parse_expression().unwrap();
        let scopes = p.into_scopes();
        let mut rt = crate::run::Runtime::new(types, &mut store);
        // SAFETY: `node` was just parsed into the store.
        let err = rt.hosting(&scopes, &trie, None, |rt| unsafe { rt.run(node) }).unwrap_err();
        assert_eq!(err, crate::run::RunError::NullPointer);
        // `caller.scope` outside a constructor is the checked error.
        let mut p = Parser::new("caller.scope", &mut store, &mut trie, types, scopes);
        let node = p.parse_expression().unwrap();
        let scopes = p.into_scopes();
        let mut rt = crate::run::Runtime::new(types, &mut store);
        // SAFETY: `node` was just parsed into the store.
        let err = rt.hosting(&scopes, &trie, None, |rt| unsafe { rt.run(node) }).unwrap_err();
        assert_eq!(err, crate::run::RunError::NoCaller);
    }

    #[test]
    fn lex_hands_back_the_tape_unconstructed() {
        use crate::identities::Core;
        use crate::regex_trie::RegexTrie;
        use crate::store::Store;

        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let plus = lex_fragment(&scopes, &trie, &mut store, "+").unwrap();
        assert_eq!(plus.len(), 1);
        let cell = *plus.at(0).unwrap();
        assert!(!cell.constructed);
        assert_eq!(cell.identity(types), types.plus, "the cell points at `+`'s binding");
        assert!(!cell.binding(types).is_null());
        assert_eq!(plus.spelling(0), Some("+"));

        let group = lex_fragment(&scopes, &trie, &mut store, "(a, b)").unwrap();
        let cells = group.cells();
        assert_eq!(cells.len(), 5, "a group is its tokens, the bracket not woken");
        assert!(cells.iter().all(|c| !c.constructed));
        assert_eq!(cells[0].identity(types), types.open_);
        assert_eq!(cells[2].identity(types), types.sep_);
        assert_eq!(cells[4].identity(types), types.close_);
        assert!(cells[1].is_fresh() && cells[3].is_fresh(), "names nothing declared are fresh");
        assert_eq!(cells[1].spelling(), "a");
        assert_eq!(cells[3].spelling(), "b");
        assert_eq!(group.cursor(), 0, "the fragment is centered on its first cell");

        let five = lex_fragment(&scopes, &trie, &mut store, " 5 ").unwrap();
        assert_eq!(five.len(), 1);
        assert_eq!(five.at(0).unwrap().identity(types), types.rational);
        assert_eq!(five.spelling(0), Some("5"), "`lex «5»` carries «5»");

        let none = lex_fragment(&scopes, &trie, &mut store, "  ").unwrap();
        assert!(none.is_empty());
        assert!(
            lex_fragment(&scopes, &trie, &mut store, "«").is_err(),
            "nothing spells a lone quote mark"
        );
    }

    #[test]
    fn a_handle_outlives_the_center_and_counts_edits() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1);
        let own = t.mark();
        let edits = t.edits();
        t.recenter(1);
        assert_eq!(t.cell_at_mark(own).map(|c| c.dyad), Some(dyad(11)));
        assert_eq!(t.edits(), edits, "moving the center is not an edit");
        t.restore(own);
        assert_eq!(t.at(0).unwrap().dyad, dyad(11));
        t.set_dyad(0, dyad(7));
        assert!(t.cell_at_mark(own).unwrap().constructed, "a write keeps the flag");
        assert_eq!(t.cell_at_mark(own).unwrap().dyad, dyad(7));
        assert_eq!(t.edits(), edits + 1, "a write is an edit");
        t.set_constructed(0, false);
        assert!(!t.cell_at_mark(own).unwrap().constructed, "the flag is its own write");
        assert_eq!(t.edits(), edits + 2, "a flag write is an edit");
        t.insert(1, unsafe { Cell::built(dyad(8)) });
        assert_eq!(t.edits(), edits + 3, "a splice is an edit");
        t.remove(0);
        assert!(t.cell_at_mark(own).is_none(), "a removed cell is gone by its handle");
        assert_eq!(t.edits(), edits + 4);
    }

    #[test]
    fn a_fragment_splices_in_order_with_its_spellings() {
        let frag = vec![unsafe { Cell::lexed(dyad(1), "x y", 0, 1) }, unsafe {
            Cell::lexed(dyad(2), "x y", 2, 1)
        }];
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11]));
        t.splice(1, frag.clone());
        let ids: Vec<_> = t.cells().iter().map(|c| c.dyad).collect();
        assert_eq!(ids, vec![dyad(10), dyad(1), dyad(2), dyad(11)]);
        assert_eq!(t.spelling(1), Some("x"));
        assert_eq!(t.spelling(2), Some("y"));
        assert_eq!(t.cursor(), 0, "the center keeps pointing at the same cell");

        let mut t = ParsingTape::from_cells(dyad_cells(&[10]));
        t.splice(0, frag.clone());
        let ids: Vec<_> = t.cells().iter().map(|c| c.dyad).collect();
        assert_eq!(ids, vec![dyad(1), dyad(2), dyad(10)]);

        let mut empty = ParsingTape::new();
        empty.splice(3, frag.clone());
        assert_eq!(empty.len(), 2);
        assert_eq!(empty.cursor(), 0, "on an empty tape the first cell becomes the center");

        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11]));
        let own = t.cells();
        t.splice(2, own);
        let ids: Vec<_> = t.cells().iter().map(|c| c.dyad).collect();
        assert_eq!(ids, vec![dyad(10), dyad(11), dyad(10), dyad(11)]);
    }

    #[test]
    fn a_logos_constructor_runs_when_its_identity_appears() {
        // A postfix `squared` whose constructor is written in Logos, one above `*`'s parse_rank.
        use crate::identities::Core;
        use crate::regex_trie::RegexTrie;
        use crate::store::Store;

        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let (_, s) =
            go("sq := fn (a := i32 ?) -> i32 ( a * a )", &mut store, &mut trie, types, scopes);
        let (_, s) = go(
            "squared := type ( \
                a := i32 ?, output_type := type ?, share run = ( sq(a) ), \
                share parse_rank = *.parse_rank + 1, \
                share parse = ( tape[0]:type = squared, tape[0].a = tape[-1], tape[0].output_type = i32, \
                          tape.is_constructed[0] = true, tape.remove(-1) ) )",
            &mut store,
            &mut trie,
            types,
            s,
        );

        let (_, s) = go("x := i32 5", &mut store, &mut trie, types, s);
        let (v, s) = go("x squared", &mut store, &mut trie, types, s);
        assert_eq!(v, 25);
        let (v, s) = go("x squared + 1", &mut store, &mut trie, types, s);
        assert_eq!(v, 26, "squared binds tighter than +");
        let (v, s) = go("2 * x squared", &mut store, &mut trie, types, s);
        assert_eq!(v, 50, "and tighter than *");
        let (_, s) =
            go("f := fn (y := i32 ?) -> i32 ( y squared + 1 )", &mut store, &mut trie, types, s);
        let (v, _s) = go("f(3)", &mut store, &mut trie, types, s);
        assert_eq!(v, 10, "the node a constructor built calls like any other");
    }

    #[test]
    fn a_constructor_writes_a_cell_from_logos() {
        // The write and the flag are two lines: `t[0] = g` repoints, `t.is_constructed[0] = true` marks.
        use crate::binding::Binding;
        use crate::identities::Core;
        use crate::regex_trie::RegexTrie;
        use crate::store::Store;

        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let tape = {
            let mut tape = ParsingTape::new();
            let mut p = Parser::new("a + b", &mut store, &mut trie, types, scopes);
            p.lex_segment(&mut tape).unwrap();
            scopes = p.into_scopes();
            tape.set_cursor(1);
            Box::into_raw(Box::new(tape))
        };
        // SAFETY: `tape` is a live box the natives write through.
        unsafe {
            let plus = (*tape).at(0).unwrap().dyad;
            let storage = store.alloc_bytes(&(tape as usize as u64).to_ne_bytes());
            // Marked as storage, as the parser marks every place it allocates.
            let t = store.alloc_raw(core.tape.parsing_tape, crate::dyad::global_place(storage));
            let rec = Binding::alloc(
                &mut store,
                core.binding_,
                Binding::new(t, core.root_scope, std::ptr::null_mut()),
            );
            scopes.declare(&mut trie, "t", rec).unwrap();

            let (v, s) = go("t[0]:type", &mut store, &mut trie, types, scopes);
            assert_eq!(v as DyadPtr, core.type_, "an identity's type is the root");

            let (_, s) = go(
                "g := fn (p := i32 ?, q := i32 ?) -> i32 ( p + q )",
                &mut store,
                &mut trie,
                types,
                s,
            );
            let (_, s) = go("t[0] = g", &mut store, &mut trie, types, s);
            let c = *(*tape).at(0).unwrap();
            assert!(!c.constructed, "a write never sets the flag");
            assert_ne!(c.dyad, plus, "the pointer is replaced");
            assert_eq!(
                types.through(c.dyad),
                types.through(s.resolve(&trie, "g").unwrap().binding),
                "the cell now names g"
            );
            let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
            assert_eq!(v, 0);
            let (_, s) = go("t.is_constructed[0] = true", &mut store, &mut trie, types, s);
            assert!((*tape).at(0).unwrap().constructed, "the flag is the constructor's line");
            let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
            assert_eq!(v, 1);

            let (_, s) = go("t.remove(1)", &mut store, &mut trie, types, s);
            let (_, _s) = go("t.remove(-1)", &mut store, &mut trie, types, s);
            assert_eq!((*tape).len(), 1);

            drop(Box::from_raw(tape));
        }
    }

    #[test]
    fn the_tape_affordances_are_reachable_from_logos() {
        use crate::binding::Binding;
        use crate::identities::Core;
        use crate::regex_trie::RegexTrie;
        use crate::store::Store;

        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let tape = {
            let mut tape = ParsingTape::new();
            let mut p = Parser::new("a + b", &mut store, &mut trie, types, scopes);
            p.lex_segment(&mut tape).unwrap();
            scopes = p.into_scopes();
            tape.set_cursor(0);
            Box::into_raw(Box::new(tape))
        };
        // SAFETY: `tape` is a live box; the natives and the checks below read through the same handle.
        unsafe {
            assert_eq!((*tape).len(), 3);
            let plus = (*tape).at(1).unwrap().dyad;
            assert!(!(*tape).at(1).unwrap().constructed, "a lexed cell is unconstructed");

            let storage = store.alloc_bytes(&(tape as usize as u64).to_ne_bytes());
            // Marked as storage, as the parser marks every place it allocates.
            let t = store.alloc_raw(core.tape.parsing_tape, crate::dyad::global_place(storage));
            let rec = Binding::alloc(
                &mut store,
                core.binding_,
                Binding::new(t, core.root_scope, std::ptr::null_mut()),
            );
            scopes.declare(&mut trie, "t", rec).unwrap();

            let (v, s) = go("t.is_constructed[1]", &mut store, &mut trie, types, scopes);
            assert_eq!(v, 0);
            let (v, s) = go("t.spelling[1]", &mut store, &mut trie, types, s);
            assert_eq!(crate::identities::string::text(v as DyadPtr), b"+");
            let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
            assert_eq!(crate::identities::string::text(v as DyadPtr), b"a");
            let (v, s) = go("t[1]:name", &mut store, &mut trie, types, s);
            assert_eq!(crate::identities::string::text(v as DyadPtr), b"+");
            let (v, s) = go("t.remove(1)", &mut store, &mut trie, types, s);
            assert_eq!(v as DyadPtr, plus, "remove yields the cell it took");
            assert_eq!((*tape).len(), 2);

            let (a_rec, b_rec) = ((*tape).at(0).unwrap().dyad, (*tape).at(1).unwrap().dyad);
            let (_, s) = go("i := i64 1", &mut store, &mut trie, types, s);
            let (v, s) = go("t[i]", &mut store, &mut trie, types, s);
            assert_eq!(v as DyadPtr, b_rec, "an index may be a name");
            let (_, s) = go("t.recenter(1)", &mut store, &mut trie, types, s);
            let (v, s) = go("t[-1]", &mut store, &mut trie, types, s);
            assert_eq!(v as DyadPtr, a_rec, "a negative index reads the left context");
            let (v, s) = go("t[i - 2]", &mut store, &mut trie, types, s);
            assert_eq!(v as DyadPtr, a_rec, "an index may be an expression");
            let (_, s) = go("t.recenter(-1)", &mut store, &mut trie, types, s);

            let (_, s) = go("t[0] = dyad (i32, 7)", &mut store, &mut trie, types, s);
            let c0 = *(*tape).at(0).unwrap();
            assert!(!c0.constructed, "a write replaces the pointer and nothing more");
            assert_eq!((*c0.dyad).ty, core.i32_);
            let (v, s) = go("t[0]", &mut store, &mut trie, types, s);
            assert_eq!(v as DyadPtr, c0.dyad, "the element read yields the cell");
            let (_, s) = go("t.is_constructed[0] = true", &mut store, &mut trie, types, s);
            let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
            assert_eq!(v, 1, "the flag is the constructor's own write");
            let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
            assert_eq!(
                crate::identities::string::text(v as DyadPtr),
                b"a",
                "the text stays after the cell is constructed"
            );

            let (_, s) = go("t.insert(0, lex «9»)", &mut store, &mut trie, types, s);
            assert_eq!((*tape).len(), 3);
            assert_eq!((*tape).at(0).unwrap().dyad, c0.dyad, "the center stays on its cell");
            let (_, s) = go("t.recenter(-1)", &mut store, &mut trie, types, s);
            let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
            assert_eq!(v, 0, "a spliced cell is unconstructed");
            let (v, s) = go("t[0]", &mut store, &mut trie, types, s);
            assert_eq!((*(v as DyadPtr)).ty, core.binding_, "the spliced cell is a binding");
            assert_eq!(types.through(v as DyadPtr), core.rational, "the number pattern's");
            let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
            assert_eq!(
                crate::identities::string::text(v as DyadPtr),
                b"9",
                "a spliced cell keeps the text it was lexed from"
            );
            let (_, s) = go("t[0] = dyad (i32, 9)", &mut store, &mut trie, types, s);
            let (v, s) = go("t[0]", &mut store, &mut trie, types, s);
            assert_eq!((*(v as DyadPtr)).ty, core.i32_, "the written cell, now the center");
            assert_ne!(v as DyadPtr, c0.dyad);
            let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
            assert_eq!(crate::identities::string::text(v as DyadPtr), b"9");
            let mut p = Parser::new("t.insert(0, dyad (i32, 9))", &mut store, &mut trie, types, s);
            assert_eq!(p.parse_expression().unwrap_err(), ParseError::InsertTakesTape);
            let s = p.into_scopes();

            // A use of a name handed in is its binding; the flag is untouched.
            let (_, s) = go("t[0] = t", &mut store, &mut trie, types, s);
            assert!(!(*tape).at(0).unwrap().constructed);
            assert_eq!((*tape).at(0).unwrap().dyad, rec);

            // Through a function taking the tape as a pointer parameter.
            let (_, s) = go(
                "g := fn (p := @parsing_tape ?) -> void ( p@.remove(0) )",
                &mut store,
                &mut trie,
                types,
                s,
            );
            let (_, _s) = go("g(&t)", &mut store, &mut trie, types, s);
            assert_eq!((*tape).len(), 2);

            drop(Box::from_raw(tape));
        }
    }

    #[test]
    fn resolves_a_name_declared_in_an_open_scope() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        let id = dyad(1);
        unsafe { scopes.declare(&mut trie, "a", rec(id)) }.unwrap();
        assert_eq!(scopes.resolve(&trie, "a").unwrap().identity, id);
    }

    #[test]
    fn same_name_in_sibling_scopes_resolves_the_open_one() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let (outer, inner) = (dyad(100), dyad(101));

        scopes.push(outer);
        unsafe { scopes.declare(&mut trie, "x", rec(dyad(1))) }.unwrap();
        scopes.pop();

        scopes.push(inner);
        unsafe { scopes.declare(&mut trie, "x", rec(dyad(2))) }.unwrap();
        assert_eq!(scopes.resolve(&trie, "x").unwrap().identity, dyad(2));

        scopes.pop();
        scopes.push(outer);
        assert_eq!(scopes.resolve(&trie, "x").unwrap().identity, dyad(1));
    }

    #[test]
    fn out_of_scope_is_distinct_from_unknown() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        unsafe { scopes.declare(&mut trie, "y", rec(dyad(1))) }.unwrap();
        scopes.pop();

        assert_eq!(scopes.resolve(&trie, "y"), Err(ResolveError::OutOfScope("y".into())));
        assert_eq!(scopes.resolve(&trie, "nope"), Err(ResolveError::Unknown("nope".into())));
    }

    #[test]
    fn shadowing_is_rejected() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let (outer, inner) = (dyad(100), dyad(101));

        scopes.push(outer);
        unsafe { scopes.declare(&mut trie, "a", rec(dyad(1))) }.unwrap();
        assert_eq!(
            unsafe { scopes.declare(&mut trie, "a", rec(dyad(2))) },
            Err(ResolveError::Shadowed("a".into()))
        );
        scopes.push(inner);
        assert_eq!(
            unsafe { scopes.declare(&mut trie, "a", rec(dyad(3))) },
            Err(ResolveError::Shadowed("a".into()))
        );
    }

    #[test]
    fn rollback_undoes_journalled_declarations() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        unsafe { scopes.declare(&mut trie, "keep", rec(dyad(1))) }.unwrap();
        scopes.commit();
        unsafe { scopes.declare(&mut trie, "gone", rec(dyad(2))) }.unwrap();

        scopes.rollback(&mut trie);
        assert_eq!(scopes.resolve(&trie, "keep").unwrap().identity, dyad(1));
        assert_eq!(scopes.resolve(&trie, "gone"), Err(ResolveError::Unknown("gone".into())));
        unsafe { scopes.declare(&mut trie, "gone", rec(dyad(3))) }.unwrap();
        assert_eq!(scopes.resolve(&trie, "gone").unwrap().identity, dyad(3));
    }

    #[test]
    fn rebind_points_a_spelling_at_the_original_identity() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        let alias = rec(dyad(1));
        unsafe { scopes.declare(&mut trie, "alias", alias) }.unwrap();
        unsafe { scopes.rebind(alias, dyad(2)) };
        assert_eq!(scopes.resolve(&trie, "alias").unwrap().identity, dyad(2));
        // The declare's journal entry still covers the rebound binding.
        scopes.rollback(&mut trie);
        assert_eq!(scopes.resolve(&trie, "alias"), Err(ResolveError::Unknown("alias".into())));
    }

    #[test]
    fn truncate_restores_a_known_depth() {
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        scopes.push(dyad(101));
        scopes.push(dyad(102));
        scopes.truncate(1);
        assert_eq!(scopes.depth(), 1);
        assert_eq!(scopes.current(), Some(dyad(100)));
        assert!(!scopes.is_open(dyad(101)));
    }

    #[test]
    fn a_dead_name_resolves_as_dead_and_may_be_redeclared() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let scope = dyad(100);
        scopes.push(scope);
        let a1 = rec(dyad(1));
        unsafe { scopes.declare(&mut trie, "a", a1) }.unwrap();
        unsafe { scopes.mark_dead(a1, dyad(50)) };

        assert_eq!(scopes.resolve(&trie, "a"), Err(ResolveError::Dead("a".into())));
        unsafe { scopes.declare(&mut trie, "a", rec(dyad(2))) }.unwrap();
        assert_eq!(scopes.resolve(&trie, "a").unwrap().identity, dyad(2));
        // The dead entry is still indexed: its range is what reflection reads.
        let m = trie.get("a").unwrap();
        assert_eq!(m.bindings.len(), 2);
        assert!(m.bindings.iter().any(|&c| f(c).dyad == dyad(1) && f(c).end == dyad(50)));
    }

    #[test]
    fn rollback_restores_a_dead_mark() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let scope = dyad(100);
        scopes.push(scope);
        let a1 = rec(dyad(1));
        unsafe { scopes.declare(&mut trie, "a", a1) }.unwrap();
        scopes.commit();

        unsafe { scopes.mark_dead(a1, dyad(50)) };
        unsafe { scopes.declare(&mut trie, "a", rec(dyad(2))) }.unwrap();
        scopes.rollback(&mut trie);

        assert_eq!(scopes.resolve(&trie, "a").unwrap().identity, dyad(1));
        assert_eq!(trie.get("a").unwrap().bindings.len(), 1);
    }

    #[test]
    fn two_spellings_of_equal_rank_matching_the_same_length_are_a_tie() {
        // Two patterns entered as the core enters its own (raw keys).
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let sc = dyad(9);
        scopes.push(sc);
        trie.insert("a[0-9]", rec_in(dyad(1), sc));
        trie.insert("[a-z]1", rec_in(dyad(2), sc));
        assert_eq!(scopes.resolve(&trie, "a1"), Err(ResolveError::Tied));
        assert_eq!(scopes.resolve(&trie, "a2").unwrap().identity, dyad(1));
        assert_eq!(scopes.resolve(&trie, "b1").unwrap().identity, dyad(2));
        // A longer match at equal rank wins, as `:=` wins over `:`.
        trie.insert("a1x", rec_in(dyad(3), sc));
        assert_eq!(scopes.resolve(&trie, "a1x").unwrap().identity, dyad(3));
    }

    #[test]
    fn settle_patches_start_and_end_to_the_body_item() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let scope = dyad(100);
        scopes.push(scope);
        let a1 = rec(dyad(1));
        unsafe { scopes.declare(&mut trie, "a", a1) }.unwrap();
        let a = |trie: &RegexTrie| f(trie.get("a").unwrap().bindings[0]);
        assert!(a(&trie).start.is_null());

        unsafe { scopes.settle_item(scope, dyad(10)) };
        assert_eq!(a(&trie).start, dyad(10));
        assert!(a(&trie).end.is_null());

        unsafe { scopes.mark_dead(a1, dyad(50)) };
        assert_eq!(a(&trie).end, dyad(50), "provisional: the own/drop node");
        unsafe { scopes.settle_item(scope, dyad(11)) };
        assert_eq!(a(&trie).end, dyad(11), "settled: the body item");
        assert_eq!(a(&trie).start, dyad(10), "start untouched by the end's settle");

        // A rebind keeps the range, and the pending endpoint holds the binding.
        let b_rec = rec(dyad(3));
        unsafe { scopes.declare(&mut trie, "b", b_rec) }.unwrap();
        unsafe { scopes.rebind(b_rec, dyad(4)) };
        unsafe { scopes.settle_item(scope, dyad(12)) };
        let b = f(trie.get("b").unwrap().bindings[0]);
        assert_eq!((b.dyad, b.start), (dyad(4), dyad(12)));
    }

    #[test]
    fn a_barrier_between_a_name_and_the_current_scope_is_detected() {
        let mut scopes = ScopeStack::new();
        let (outer, body, inner) = (dyad(100), dyad(101), dyad(102));
        scopes.push(outer);
        scopes.push_barrier();
        scopes.push(body);
        scopes.push(inner);
        assert!(scopes.crosses_barrier(outer));
        assert!(!scopes.crosses_barrier(body));
        assert!(!scopes.crosses_barrier(inner));
        scopes.pop();
        scopes.pop();
        scopes.pop_barrier();
        assert!(!scopes.crosses_barrier(outer));
    }

    #[test]
    fn truncating_past_a_body_drops_its_barrier() {
        // Or every later top-level `own`/`drop` is refused for the rest of the session.
        let mut scopes = ScopeStack::new();
        let (outer, body) = (dyad(100), dyad(101));
        scopes.push(outer);
        scopes.push_barrier();
        scopes.push(body);
        assert!(scopes.crosses_barrier(outer));
        scopes.truncate(1);
        assert!(!scopes.crosses_barrier(outer), "the closed body's barrier is gone");
    }

    #[test]
    fn two_live_bindings_resolve_to_the_innermost_scope() {
        let mut trie = RegexTrie::new();
        let (a, b) = (dyad(100), dyad(101));
        trie.insert("z", rec_in(dyad(1), a));
        trie.insert("z", rec_in(dyad(2), b));

        let mut scopes = ScopeStack::new();
        scopes.push(a);
        scopes.push(b);
        assert_eq!(scopes.resolve(&trie, "z").map(|r| r.identity), Ok(dyad(2)));
        scopes.pop();
        assert_eq!(scopes.resolve(&trie, "z").map(|r| r.identity), Ok(dyad(1)));
    }
}
