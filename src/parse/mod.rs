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
mod fields;
mod operands;
mod types;
pub use types::*;

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

/// Each open bracket recurses through `parse_sequence` and `(`'s constructor,
/// so a wall of `(` costs Rust stack like a runaway recursion: sized well
/// under `crate::WORK_STACK_BYTES` and far past anything a person writes.
pub const MAX_BRACKET_DEPTH: usize = 2_000;

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

impl<'a> Parser<'a> {
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
