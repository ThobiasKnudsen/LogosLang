// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Bodies: function bodies over a frame, run bodies, blocks and sequences, and the
//! control flow that reads a body; the fn record's slots and the analysis over a body.

use super::*;
use crate::dyad;
use crate::identities::drop_model::ExitItem;
use crate::identities::flow::{self, Flow};

/// One enclosing function body being parsed: parameters claim the frame's
/// first offsets, the body's locals continue after them (DESIGN ›Resolution
/// is one rule‹).
pub(super) struct OpenFn {
    /// The `fn` node whose call frame this is: what a local's binding names as its frame.
    pub(super) frame: DyadPtr,
    /// Bytes claimed so far by parameters and frame-relative locals.
    pub(super) size: usize,
    /// The scope depth the function's barrier begins at: a name declared
    /// below it is from outside the function.
    pub(super) below: usize,
    /// The bindings of the outer names the body has read so far, first-read
    /// order, each once: the function's `FN_OUTER` list.
    pub(super) outer: Vec<DyadPtr>,
    /// `open`'s length outside the body: the scopes a `return` leaves are the ones past it.
    pub(super) open_below: usize,
    /// Every `return` in the body, committed to the result type as the tail is.
    pub(super) returns: Vec<OpenReturn>,
    /// The result place, `return` read as a name: a binding laid out in the frame the first
    /// time the body reads it, null until then.
    pub(super) result: DyadPtr,
    /// The body is a type's slot, `run`, `parse` or `free`, which has no `->`.
    pub(super) slot_body: bool,
}

/// A `return` in a function body being parsed. Its line may still end names after it, in a
/// part the `return` leaves before: the `return` frees those itself (DESIGN ›A value's
/// teardown runs where its life ends; the ending identity reads the type's `free` slot‹).
pub(super) struct OpenReturn {
    node: DyadPtr,
    /// Where its word stands, for an error the body's close finds in it.
    at: usize,
    /// Whether its operand is made or borrowed, as `handed_ends` reads it.
    made: Option<bool>,
    /// The names the scopes it leaves hold at it, in declaration order; an `if` whose arm
    /// frees one on the `return`'s way out takes it off.
    pub(super) live: Vec<DyadPtr>,
    /// The names its line ended after it.
    ended: Vec<DyadPtr>,
    /// The values `&` landed before it in the line it leaves of a scope it leaves, or in an
    /// argument list around it: no scope's end runs them at the `return`, so it frees them.
    landed: Vec<DyadPtr>,
    /// The innermost scope it is still inside, whose line holding it is closed or not.
    level: usize,
    line_closed: bool,
    /// Where in `ended_log` its line at `level` goes on after it.
    cursor: usize,
}

/// A line parsed in pass order and not yet run.
pub(super) struct UnrunLine {
    pub(super) node: DyadPtr,
    /// Its index in its scope.
    line: usize,
    /// The names it ends that an enclosing scope or an earlier REPL line holds.
    ends: Vec<DyadPtr>,
    /// The nodes whose values it holds to its end.
    plan: Vec<DyadPtr>,
    /// Where each node in it that runs starts.
    pub(super) runs: Vec<usize>,
    /// The values with an end of their own the pass ran early that stand in it.
    stood: Vec<DyadPtr>,
}

impl UnrunLine {
    pub(super) fn new(node: DyadPtr, line: usize) -> Self {
        UnrunLine {
            node,
            line,
            ends: Vec::new(),
            plan: Vec::new(),
            runs: Vec::new(),
            stood: Vec::new(),
        }
    }
}

/// What the construct around a block does with the block's value (›A scope's value is what it
/// evaluates to, and `return` is an optional early exit from the enclosing function‹ [[R207]]).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum BlockValue {
    /// The line the block stands in, which holds it to its end unless something takes it: a
    /// block's or an `if` arm's value passes out to it.
    #[default]
    Used,
    /// A loop's body: a value it gives ends the loop and passes out to the loop's line, and
    /// each pass ends what it made that nothing took.
    Body,
    /// Thrown away, a line held for the run that no value lands from: its last line's end frees
    /// what that line made.
    Thrown,
    /// A `-> void` function's body, none of whose lines may give a value.
    Void,
    /// A function's body, whose value the call hands to the line the call stands in.
    Returned,
    /// An `if` or `while` condition, read once each time it runs: a scope of its own even as
    /// one line, so it ends what it made once it has been read.
    Condition,
}

/// What a bracket's lines are to whatever reads the bracket.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lines {
    /// Lines of their own, the scope valued by its last one.
    Own,
    /// A `[…]`'s items, which go whole to the call that takes them.
    List,
    /// A call's argument list.
    Arguments,
    /// A `{…}`'s lines, the scope itself the value: its last line, which the parse adds, makes
    /// the value of the record its names lay out (›`{ … }` outside text is a scope kept as a
    /// value, read with `.`‹ [[R374]]).
    Kept,
}

/// Where a scope's lines end.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LinesEnd {
    /// At a closer, which the bracket that opened the scope takes.
    Closer,
    /// At the end of the text: a root scope or a quote's `{…}`, where a closer closes nothing.
    TextEnd,
    /// Before the body of the `if` or `while` the scope is the condition of: its one line is
    /// the first complete expression, bracketed or not.
    Condition,
}

#[derive(Default)]
pub(super) struct OpenScope {
    /// What the scope's end runs, in line order: the names it holds, which no tail or
    /// `return` leaving the scope may hand out, and its `defer` lines.
    pub(super) exit: Vec<ExitItem>,
    /// At the top level: what the `exit` items an imported file added hold or defer, which
    /// are the run's and not the line's.
    pub(super) imported: HashSet<DyadPtr>,
    /// The lines closed so far, so the index of the one being parsed.
    pub(super) lines: usize,
    /// Held names a `move` or `free` in the line being parsed ended; the line's close
    /// settles them.
    pub(super) ended: Vec<DyadPtr>,
    /// The comments segments built inside the line being parsed handed to it, in the order
    /// lifted: lines of this scope after that line.
    pub(super) prose: Vec<DyadPtr>,
    /// At the top level: owners a `move` or `free` ended that no open scope holds, which the
    /// top scope's exit does, from an earlier REPL line.
    pub(super) ended_earlier: Vec<DyadPtr>,
    /// Items parsed at depth 0 and not yet run. Where the scope's lines are lines of their own,
    /// `drain` runs them when the pass needs a value, the scope's own run otherwise; where they
    /// are parts of the line a call or a quote's word stands in, that call or word runs them, or
    /// a point where the pass needs a value runs them first.
    pub(super) unrun: Vec<UnrunLine>,
    /// The values the line being parsed has made that nothing took so far, each by the
    /// outermost node it runs as (›A value's teardown runs where its life ends; the ending
    /// identity reads the type's `free` slot‹ [[R57]]).
    made: Vec<DyadPtr>,
    /// Where the line being parsed builds something that runs only when the line runs, one start
    /// per node: what a call the pass runs while it parses must run first (›The pass runs only as
    /// far as it must, in order, and never twice‹ [[R288]]).
    pub(super) runs: Vec<usize>,
    /// The same for the closed lines `unrun` does not hold, such as a condition's.
    closed_runs: Vec<usize>,
    /// The `type ?` boxes read after a part of the line being parsed that runs and cannot run
    /// first, each with where it was read: until the line closes, each stands for itself
    /// (`Parser::unsettled_at`).
    pub(super) unsettled: Vec<(DyadPtr, usize)>,
    /// The values the scope's closed lines made that nothing took: each line's end frees its
    /// own. The scope node's `held` slot once it closes.
    pub(super) held: Vec<DyadPtr>,
    /// The nodes standing for values with an end of their own that the pass ran early in the
    /// line being parsed, or in a scope closed inside it: the line takes them when it closes,
    /// and what nothing took ends where it ends or faults.
    pub(super) stood: Vec<DyadPtr>,
    /// The lines the pass has run, up to which a fault's end ends what they hold.
    ran: usize,
    /// Names this scope holds, or at the top level an earlier REPL line, whose ending line in
    /// a nested scope the pass has run: a fault's end frees them no more.
    reached: Vec<DyadPtr>,
    /// The block node whose `dyads` hold the items, when the scope runs as one
    /// later: a drain leaves its cursor on the frame. None at the top level and in a type body.
    pub(super) scope: Option<DyadPtr>,
    /// A `[…]`: its items go whole to the call that takes them, never pending
    /// (DESIGN ›A bracket goes to the call whole‹).
    pub(super) list: bool,
    /// A call's argument list, which is never the scope its lines stand in: at its close, what
    /// it holds for `&` goes to the scope the call stands in.
    pub(super) arguments: bool,
    /// A quote's parts read so far, its text and its `{…}` alike, which are its lines: they go to
    /// the word that reads the quote, and one the pass ran first stands here for the value it
    /// gave.
    pub(super) quote: Option<Vec<DyadPtr>>,
    /// The names `exit` holds for `&`: values `&` landed here, and those an argument list
    /// read here handed on.
    address_holds: Vec<DyadPtr>,
    /// Where the callee starts whose argument list a call is reading: a `(` right after it
    /// opens that list.
    arguments_after: Option<usize>,
    /// The tape cells and lines a check in this scope has narrowed, each with the type
    /// its read yields.
    pub(super) narrowed: Vec<(CellKey, DyadPtr)>,
    /// A `-> void` function's body, or a `{ … }`, which gives the scope itself: a line that
    /// gives a value is refused, with this.
    gives_none: Option<ParseError>,
    /// The scope's value is the value of the line it stands in, its construct starting the line.
    passes_out: bool,
    /// A name an enclosing scope holds was given out through the line being parsed: the line
    /// must give it on, so a reader that takes the construct the line starts with is refused.
    moves_through: bool,
    /// A line inside the line being parsed gave a name this scope holds, or its address, and
    /// it was not moved: the line may not give it as this scope's value.
    gives_held: bool,
    /// How the line that ended the scope ends, once one did: every line after it is refused.
    ended_by: Option<Flow>,
    /// A program's or an imported file's top level, which a line that gives a value ends.
    top: bool,
    /// Once a line the pass ran gave the scope its value, that line's index: the lines after
    /// never run, nor do those of the scopes open inside it.
    given_at: Option<usize>,
    /// A `( … )` a reader to its left may take as data, line by line, rather than as a scope.
    cell: bool,
    /// In such a cell, the first line refused as a line of a scope, with where it starts: the
    /// refusal stands once the cell's line closes, unless a reader took the lines as data.
    held_back: Option<(usize, ParseError)>,
    /// A `{ … }`'s lines: the value they make.
    pub(super) kept: Option<KeptValue>,
}

/// The value a `{ … }` makes, while its lines are read: the record a `fn` written in it reads its
/// sibling names through, as a `static` function reads its value's fields (›A type body describes
/// one level‹ [[R222]]).
pub(super) struct KeptValue {
    /// The record type, made when a `fn` written here first needs it and laid out when the `}`
    /// closes it; null until then.
    pub(super) ty: DyadPtr,
    /// Each `fn` written here that reads the value, with the names it read them among, in
    /// field order: the record's first fields must still be those names when it is laid out.
    pub(super) readers: Vec<(DyadPtr, Vec<DyadPtr>)>,
    /// How many function frames and definitions are open around its own lines.
    pub(super) depth: (usize, usize),
    /// Its names lie in the program frame, which every call reaches.
    pub(super) in_program_frame: bool,
}

/// What the parse of a line notes on its scope as it goes: the values it made that nothing took,
/// and the comments lifted out of it.
#[derive(Default, Clone)]
pub(super) struct LineNotes {
    made: Vec<DyadPtr>,
    prose: Vec<DyadPtr>,
}

impl crate::start_graph::Carry for LineNotes {
    fn carry(&self, out: &mut crate::start_graph::Out) {
        let LineNotes { made, prose } = self;
        made.carry(out);
        prose.carry(out);
    }

    fn uncarry(inp: &mut crate::start_graph::In) -> Self {
        use crate::start_graph::Carry;
        let made = Carry::uncarry(inp);
        LineNotes { made, prose: Carry::uncarry(inp) }
    }
}

impl OpenScope {
    /// A program's or an imported file's top level (›A scope's value is what it evaluates to, and
    /// `return` is an optional early exit from the enclosing function‹ [[R207]]).
    pub(super) fn top_level() -> Self {
        OpenScope { top: true, ..OpenScope::default() }
    }

    /// How many of the scope's lines completed when its end is reached, as
    /// [`ExitItem::held_at`] reads it: up to the line that gave it its value, else all.
    pub(super) fn reached_lines(&self) -> usize {
        self.given_at.map_or(self.lines, |at| at + 1)
    }

    /// A quote's parts, read by `print`, `abort` or a raise.
    pub(super) fn quote() -> Self {
        OpenScope { quote: Some(Vec::new()), ..OpenScope::default() }
    }

    /// Whether the scope's lines are lines of their own, which run where the scope runs; an
    /// argument list's and a `[…]`'s are parts of the line the call that takes them stands in
    /// (›A bracket goes to the call whole‹ [[R228]]), and a quote's of the line its word stands in.
    pub(super) fn own_lines(&self) -> bool {
        !self.list && !self.arguments && self.quote.is_none()
    }

    /// Line `line`, a part of the line being parsed that the pass ran first, stands for `value` in
    /// its place: among a quote's parts, or in the bracket's scope, which the call that takes it
    /// reads.
    ///
    /// # Safety
    /// `line` must be one of the scope's lines, and `value` a reduced dyad from the store.
    pub(super) unsafe fn stand(&mut self, line: usize, value: DyadPtr) {
        match &mut self.quote {
            Some(parts) => parts[line] = value,
            None => {
                let scope = self.scope.expect("a bracket's lines are lines of its block node");
                // SAFETY: the caller's contract; `scope` is the bracket's block node.
                unsafe { crate::identities::scope::set_item(scope, line, value) };
            }
        }
    }

    /// The values the pass ran early that stand in the scope's lines, closed or not, taken out.
    pub(super) fn take_stood(&mut self) -> Vec<DyadPtr> {
        let mut stood: Vec<DyadPtr> =
            self.unrun.iter_mut().flat_map(|u| std::mem::take(&mut u.stood)).collect();
        stood.append(&mut self.stood);
        stood
    }

    pub(super) fn take_line_notes(&mut self) -> LineNotes {
        LineNotes { made: std::mem::take(&mut self.made), prose: std::mem::take(&mut self.prose) }
    }

    pub(super) fn add_line_notes(&mut self, notes: LineNotes) {
        self.made.extend(notes.made);
        self.prose.extend(notes.prose);
    }
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
/// first-read order, or null; read at every call (DESIGN ›`move` and `free`
/// are static‹).
pub const FN_OUTER: usize = 5;

/// A `u64` leaf of [`RECEIVER_READS`] and [`RECEIVER_WRITES`] bits: whether a
/// `static` function's body reads or writes a field of its value; null for neither.
pub const FN_RECEIVER: usize = 6;

pub const FN_SLOTS: usize = FN_RECEIVER + 1;

/// The fields' names in slot order, as reflection reads them.
pub const FN_FIELDS: [&str; FN_SLOTS] =
    ["input", "output_type", "body", "bcode", "frame", "outer", "receiver"];

/// Refuses a value a function hands on through a pointer result, `made` as `handed_ends` reads
/// it, that the result does not say: a borrow under an owning `@T`, or, where the result is
/// written (`written`, the source after `->`), a value made under a plain one. A `T!` result is
/// read as its `T`. A result of any other type owns what it gets either way (›A last value moves
/// out‹ [[R66]]). No `written` is a `run`.
///
/// # Safety
/// `output` must be the result type node of the function whose body closes.
unsafe fn check_handed(
    types: &Core,
    output: DyadPtr,
    written: Option<&str>,
    made: Option<bool>,
) -> Result<(), ParseError> {
    let fallible = crate::identities::union::fallible_members(types, output);
    let output = fallible.map_or(output, |(value, _)| crate::identities::hole::type_in(value));
    if !crate::identities::numtype::is_pointer_type(output) {
        return Ok(());
    }
    let owns = !crate::identities::meta::destructor_of(output).is_null();
    match (made, written) {
        (Some(false), Some(_)) if owns => Err(ParseError::OwnResultBorrows),
        (Some(false), None) if owns => Err(ParseError::RunReturnBorrows),
        (Some(true), Some(text)) if !owns => {
            let owning = match text.trim().strip_suffix('!').filter(|_| fallible.is_some()) {
                Some(value) => format!("(own {})!", value.trim()),
                None => format!("own {text}"),
            };
            Err(ParseError::HandsOnMade(owning))
        }
        _ => Ok(()),
    }
}

pub const RECEIVER_READS: u64 = 1;

pub const RECEIVER_WRITES: u64 = 2;

/// `0` when the slot is null (no parameters and no locals).
///
/// # Safety
/// `fn_node` must be a function node whose value is the `FN_SLOTS`-word record
/// `parse_fn` builds.
pub unsafe fn fn_frame_size(fn_node: DyadPtr) -> usize {
    let frame = *(dyad::value(fn_node) as *const DyadPtr).add(FN_FRAME);
    if frame.is_null() {
        0
    } else {
        std::ptr::read_unaligned(dyad::value(frame) as *const u64) as usize
    }
}

/// Empty for a function that reads none, and for one whose body is still being parsed.
///
/// # Safety
/// `fn_node` must be a function node: the operands `parse_fn` builds, the early
/// signature included.
pub unsafe fn fn_outer<'a>(fn_node: DyadPtr) -> &'a [DyadPtr] {
    let outer = *(dyad::value(fn_node) as *const DyadPtr).add(FN_OUTER);
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
    if f.is_null() || dyad::ty(f) != types.fn_type {
        return 0;
    }
    let leaf = *(dyad::value(f) as *const DyadPtr).add(FN_RECEIVER);
    if leaf.is_null() {
        0
    } else {
        std::ptr::read_unaligned(dyad::value(leaf) as *const u64)
    }
}

/// Deliberately no scope unwrapping: a sequence-valued condition may carry
/// effectful non-tail expressions a fold would silently drop.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn bool_literal_value(types: &Core, node: DyadPtr) -> Option<bool> {
    let node = types.through(node);
    if dyad::ty(node) != types.bool_ {
        return None;
    }
    Some(std::ptr::read_unaligned(dyad::value(node) as *const i32) != 0)
}

/// The line a condition's value is read from at parse: past each block whose other lines are
/// comments and whose end runs nothing, a comment being void (›Text literals are plain values;
/// `#` is the one comment constructor‹ [[R160]]); a block with any other line stays itself, as
/// a fold would drop what that line runs.
///
/// # Safety
/// `cond` must be a reduced dyad from the store.
unsafe fn condition_value(types: &Core, mut cond: DyadPtr) -> DyadPtr {
    use crate::identities::{numtype::is_comment_type, scope};
    while dyad::ty(cond) == types.scope
        && scope::exit_items(cond).is_empty()
        && scope::held_of(cond).is_empty()
    {
        let Some(lines) = scope::exprs_of(cond) else { break };
        let mut values = lines.iter().filter(|&&l| !is_comment_type(dyad::ty(l)));
        match (values.next(), values.next()) {
            (Some(&only), None) => cond = only,
            _ => break,
        }
    }
    cond
}

/// A cell or line of the tape, by the tape's place, a literal index and the line.
pub(crate) type CellKey = (DyadPtr, i32, crate::identities::tape::Line);

/// `tape[k].type == T` or `!=` with `T` a number type or `type`, either side first, or
/// `tape[k].type ⊆ T` with `T` a number type, each under any number of `not`, or the same of
/// `tape[0].f.type` for a `dyad` field `f`: the cell or field checked, the type it narrows to, and
/// whether the branch that holds is the `then`.
///
/// # Safety
/// `cond` must be a reduced dyad from the store.
unsafe fn type_check_of(types: &Core, cond: DyadPtr) -> Option<(CellKey, DyadPtr, bool)> {
    let cond = types.through(cond);
    if dyad::ty(cond) == types.not_ {
        let (key, ty, holds) = type_check_of(types, *(dyad::value(cond) as *const DyadPtr))?;
        return Some((key, ty, !holds));
    }
    let subset = dyad::ty(cond) == types.subset;
    let eq = subset || crate::identities::mint::origin(dyad::ty(cond)) == types.eq;
    if !eq && crate::identities::mint::origin(dyad::ty(cond)) != types.ne {
        return None;
    }
    let ops = dyad::value(cond) as *const DyadPtr;
    let (l, r) = (types.through(*ops), types.through(*ops.add(1)));
    let is_read = |n: DyadPtr| crate::identities::tape::is_line_type_read(types, n);
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
    crate::identities::tape::type_read_key(types, read).map(|key| (key, ty, eq))
}

/// `{type: callee, value: [args…, null]}`: null-terminated so `run` can count
/// the arguments; a nullary call is a leaf, its one zero word the terminator.
pub(crate) fn build_call(store: &mut Store, callee: DyadPtr, args: &[DyadPtr]) -> DyadPtr {
    if args.is_empty() {
        return store.alloc_leaf(callee);
    }
    let mut ops = args.to_vec();
    ops.push(std::ptr::null_mut());
    store.alloc_words(callee, &ops)
}

/// The words of a null-terminated run, the terminator left out: a call's arguments as
/// `build_call` laid them, a list operand's tail.
///
/// # Safety
/// `p` must point at a null-terminated run of node words.
pub unsafe fn null_terminated<'a>(p: *const DyadPtr) -> &'a [DyadPtr] {
    let n = (0..).take_while(|&i| !(*p.add(i)).is_null()).count();
    std::slice::from_raw_parts(p, n)
}

impl<'a> Parser<'a> {
    /// The function a held run body is for one field-type set: the cells lexed
    /// at the definition are constructed now, over the type's own scopes and one
    /// unnamed parameter per field. Entered on the type first, so a recursive use resolves to it.
    ///
    /// # Safety
    /// `ty` must carry a record whose run body is `held`; `key` one type per
    /// instance field.
    pub(super) unsafe fn construct_run_body(
        &mut self,
        ty: DyadPtr,
        held: DyadPtr,
        key: &[DyadPtr],
        at: usize,
        spelling: &str,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let text = crate::identities::run_body::text_of(held);
        let cells = (*crate::identities::run_body::cells_of(held)).held_cells();
        // The type's own scope over what encloses its fields', whose names the run's parameters
        // are.
        let mut chain = vec![crate::identities::meta::record_body_of(ty)];
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
        let spec = self.rt.store.alloc_words(types.fn_type, &[std::ptr::null_mut(); FN_SLOTS]);
        crate::identities::run_body::insert(self.rt.store, types, held, key, spec);

        let cx = Context {
            feed: Some(Feed { cells, next: 0, base_depth, in_run_order: false }),
            ..Context::nested(&self.cx, text, nested)
        };
        let g = self.enter(cx);
        let inner = g.0.construct_run_body_in(ty, key, spec);
        let inner_pos = g.0.cx.pos;
        drop(g);

        match inner {
            Ok(f) => {
                // A run that is one instruction over the fields is that instruction, kept on the
                // mint, and its nodes jump to it.
                if let Some(instruction) = crate::identities::instruction::named_by(types, f) {
                    crate::identities::meta::keep_instruction(ty, instruction);
                }
                Ok(f)
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
    /// type (the type the set holds in the field `output_type`, else the type's
    /// `static output_type`, else `void`), and the body read as a function's, filling `spec`.
    ///
    /// # Safety
    /// As `construct_run_body`; `spec` the `fn` node minted for the set.
    unsafe fn construct_run_body_in(
        &mut self,
        ty: DyadPtr,
        key: &[DyadPtr],
        spec: DyadPtr,
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
        let output_field = self.output_field(ty);
        let mut params: Vec<(Option<&str>, DyadPtr)> = Vec::with_capacity(fields.len());
        let mut output_name = None;
        for (i, &field) in fields.iter().enumerate() {
            let name = names.get(&field).map(String::as_str);
            let ty = if output_field == Some(i) {
                // The field names the type this node's run gives, so `output_type 1` reads
                // `i32 1`; its parameter place goes unnamed.
                output_name = name;
                params.push((None, types.type_));
                continue;
            } else if key[i] == types.scope || key[i] == types.square_brackets {
                // A bracket parameter, typeless: the bracket the node's field holds, read by
                // its lines.
                std::ptr::null_mut()
            } else {
                key[i]
            };
            params.push((name, ty));
        }
        let static_output = self.declared_output_type(ty);
        let output = output_field.map(|i| key[i]).or(static_output).unwrap_or(types.void_);
        let (input, places) = self.hidden_param_record(&params, 0)?;
        self.cx.run_fields = Some((ty, places.clone(), spec));
        let param_scope = crate::identities::meta::record_scope_of(input);
        self.cx.scopes.push(param_scope);
        let mut names_now = places.clone();
        let declared = match (output_name, output_field) {
            (Some(n), Some(i)) => self.declare_name(n, key[i], 0).map(|b| names_now[i] = b),
            _ => Ok(()),
        };
        let declared = declared.and_then(|()| match static_output {
            Some(t) => self.declare_output_type(ty, t),
            None => Ok(()),
        });
        self.cx.scopes.pop();
        declared?;
        // The body was lexed where it is written, among the fields: a cell bound to a field now
        // names the parameter that stands for it.
        let field_bindings =
            crate::identities::array::items(crate::identities::meta::record_field_bindings_of(ty));
        if let Some(feed) = self.cx.feed.as_mut() {
            for cell in &mut feed.cells {
                if let Some(i) = field_bindings.iter().position(|&f| f == cell.dyad) {
                    cell.dyad = names_now[i];
                }
            }
        }
        self.fn_over_body(types.fn_type, input, &places, output, None, spec, std::ptr::null_mut())
    }

    /// Which of `ty`'s fields is its `type` field `output_type`, where each node's parse writes the
    /// type its run gives (›A mint's output type is written in its `static output_type`
    /// declaration; a type that does not mint has its parse write it into each node‹ [[R194]]).
    ///
    /// # Safety
    /// `ty` must carry a `RECORD_TAG` record.
    pub(super) unsafe fn output_field(&self, ty: DyadPtr) -> Option<usize> {
        let fields = crate::identities::array::items(crate::identities::meta::record_fields_of(ty));
        let mut scope = ScopeStack::new();
        scope.push(crate::identities::meta::record_scope_of(ty));
        scope
            .resolve(self.trie, crate::identities::mint::STATIC_FIELD)
            .ok()
            .and_then(|r| fields.iter().position(|&f| f == r.identity))
            .filter(|&i| crate::identities::hole::type_in(fields[i]) == self.types.type_)
    }

    /// The type `ty`'s own `static output_type` holds, what every node of it gives (›A mint's
    /// output type is written in its `static output_type` declaration; a type that does not mint
    /// has its parse write it into each node‹ [[R194]]); `None` when it declares none or the
    /// member holds no type.
    ///
    /// # Safety
    /// `ty` must be a record type from the store.
    unsafe fn declared_output_type(&self, ty: DyadPtr) -> Option<DyadPtr> {
        use crate::identities::mint::STATIC_FIELD;
        let body = crate::identities::meta::record_body_of(ty);
        if body.is_null() {
            return None;
        }
        let mut members = ScopeStack::new();
        members.push(body);
        let found = members.resolve(self.trie, STATIC_FIELD).ok()?;
        let member = found.binding;
        let own = !member.is_null()
            && found.matched == STATIC_FIELD.len()
            && Binding::read(member).scope == body
            && crate::identities::gate::name_has(self.types, member, self.types.static_);
        if !own {
            return None;
        }
        let held = self.settled_type(self.types.through(member));
        crate::identities::type_identity_of(self.types, held)
    }

    /// The type's `static output_type`, declared over the member as a field of type `type` is,
    /// so `output_type lossy (lhs)` reads `f64 lossy (lhs)` in the run built for a type whose
    /// line gave `f64`; each cell of the held body the definition bound to the member now names
    /// it.
    ///
    /// # Safety
    /// `mint` must be a type with a body; `output` a type node.
    unsafe fn declare_output_type(
        &mut self,
        mint: DyadPtr,
        output: DyadPtr,
    ) -> Result<(), ParseError> {
        use crate::identities::mint::STATIC_FIELD;
        let binding = self.declare_field_name(STATIC_FIELD, output, 0)?;
        let mut members = ScopeStack::new();
        members.push(crate::identities::meta::record_body_of(mint));
        if let (Ok(origin), Some(feed)) =
            (members.resolve(self.trie, STATIC_FIELD), self.cx.feed.as_mut())
        {
            for cell in feed.cells.iter_mut().filter(|c| c.dyad == origin.binding) {
                cell.dyad = binding;
            }
        }
        Ok(())
    }

    /// `(start, len)` of the text inside the `( … )` at the cursor, consumed
    /// with its brackets and constructed by nothing: an untaken branch dropped unlexed, a
    /// body held to be lexed later.
    pub(super) fn body_text_extent(&mut self) -> Result<(usize, usize), ParseError> {
        self.expect_open()?;
        let start = self.cx.pos;
        let close = self.skip_bracket(self.types.open_)?;
        Ok((start, close - start))
    }

    /// `fn ( params ) -> ret ( body )` (DESIGN ›A function's surface‹): the
    /// node is `[input, output, body, bcode, frame, outer]`. `binding` is the
    /// declaration's, pointed at the node before the body parses, or null.
    ///
    /// # Safety
    /// `binding` must be null or a binding dyad from the store.
    pub unsafe fn parse_fn(
        &mut self,
        fn_type: DyadPtr,
        binding: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        // A `static` function reached through a value takes that value
        // (DESIGN ›The constructor is a field‹): the `fn` takes it first. So does a `fn` written
        // in a `{ … }`, whose sibling names are the value's.
        let kept = self.kept_entry(binding);
        let member = kept.is_some()
            || !binding.is_null()
                && self.cx.member_fn_depth == Some(self.cx.frames.len())
                && self.cx.definitions.last().is_some_and(|d| d.this_param.is_null());
        let (input, params) = self.parse_record_taking(member.then_some(self.types.dyad_))?;
        self.expect_arrow()?;
        self.skip_whitespace();
        let start = self.cx.pos;
        let output = {
            let was = std::mem::replace(&mut self.cx.result_type, true);
            let items = self.drive_until_open(RightSide::ReturnType);
            self.cx.result_type = was;
            let out = self.one_of(items?).map_err(|e| match e {
                ParseError::Empty => ParseError::ExpectedReturnType,
                e => e,
            })?;
            // The function is built here, so its result type is needed now (›`fn` is not a
            // primitive: a function is a type in this same shape‹ [[R199]]).
            self.operand_dyad(out)
                .and_then(|d| self.type_known_now(d))
                .map_err(|e| self.fail_at(start, e))?
        };
        let source = self.cx.source;
        // The source after `->`, as an error names the type.
        let result = source[start..self.cx.pos].trim_end();
        if !member {
            // SAFETY: `input` was just built by `parse_record`; `binding` is the caller's.
            return unsafe {
                self.fn_over_body(
                    fn_type,
                    input,
                    &params,
                    output,
                    Some(result),
                    std::ptr::null_mut(),
                    binding,
                )
            };
        }
        // The value is the first parameter, the one no name declares.
        let this = params[0];
        let in_kept = kept.is_some();
        self.cx.definitions.extend(kept);
        let def = self.cx.definitions.last_mut().expect("checked above");
        def.this_param = this;
        def.read_receiver = false;
        def.wrote_receiver = false;
        // SAFETY: as above.
        let f = unsafe {
            self.fn_over_body(
                fn_type,
                input,
                &params,
                output,
                Some(result),
                std::ptr::null_mut(),
                binding,
            )
        };
        let def = self.cx.definitions.last_mut().expect("still open");
        def.this_param = std::ptr::null_mut();
        let receiver = (u64::from(def.read_receiver) * RECEIVER_READS)
            | (u64::from(def.wrote_receiver) * RECEIVER_WRITES);
        if in_kept {
            let def = self.cx.definitions.pop().expect("pushed above");
            if receiver != 0 {
                let open = self.cx.open.last_mut().expect("the `{ … }` is open");
                let kept = open.kept.as_mut().expect("the `fn` is written in a `{ … }`");
                kept.readers.push((binding, def.block.1));
            }
        }
        if let (Ok(&f), true) = (f.as_ref(), receiver != 0) {
            let u64_ty = self.types.numtypes[crate::identities::NumType::U64 as usize];
            let leaf = self.rt.store.alloc_blob(u64_ty, &receiver.to_ne_bytes());
            // SAFETY: `f` is the node `fn_over_body` just built over its `FN_SLOTS` words.
            unsafe { *(dyad::value(f) as *mut DyadPtr).add(FN_RECEIVER) = leaf };
        }
        f
    }

    /// The `{ … }` a `fn` declared as `binding` is written in, as the definition its body is read
    /// in (›`{ … }` outside text is a scope kept as a value, read with `.`‹ [[R374]]): the innermost
    /// open scope is that `{ … }`'s own.
    fn kept_entry(&mut self, binding: DyadPtr) -> Option<OpenType> {
        let scope = self.cx.open.last()?.scope?;
        if binding.is_null() || self.cx.scopes.current() != Some(scope) {
            return None;
        }
        let types = self.types;
        let kept = self.cx.open.last_mut()?.kept.as_mut()?;
        if kept.ty.is_null() {
            kept.ty = self.rt.store.alloc_leaf(types.type_);
        }
        let ty = kept.ty;
        // SAFETY: `scope` is the open `{ … }`'s block node, its closed lines reduced dyads.
        let names = unsafe { crate::identities::instance::kept_names(types, scope) };
        // SAFETY: a kept name is a binding dyad from the store.
        let fields = names.into_iter().filter(|&name| unsafe { types.is_storage(name) });
        Some(OpenType::kept(scope, fields.collect(), ty))
    }

    /// The `{ … }` whose block is `scope`, while its lines are read.
    pub(super) fn open_kept(&self, scope: DyadPtr) -> Option<&KeptValue> {
        self.cx.open.iter().rev().find(|o| o.scope == Some(scope))?.kept.as_ref()
    }

    /// The half of `parse_fn` after the signature, shared with a slot body read bare: the
    /// frame opened on the `fn` node, the parameters laid out in it on their bindings, the
    /// body parsed deferred with the parameter scope reopened. The node is `spec`, a run
    /// body's node minted for its set, or fresh; its signature is on it, and `binding`
    /// points at it, before the body parses, so a recursive self-call resolves its types.
    /// `result` is the source after `->`, `None` for a type's `run` body, which writes none.
    ///
    /// # Safety
    /// `input` must be a record node and `params` its fields' bindings in order; `spec`
    /// null or a `fn` node minted with `FN_SLOTS` null words that nothing has filled;
    /// `binding` null or a binding dyad from the store.
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn fn_over_body(
        &mut self,
        fn_type: DyadPtr,
        input: DyadPtr,
        params: &[DyadPtr],
        output: DyadPtr,
        result: Option<&str>,
        spec: DyadPtr,
        binding: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let node = if spec.is_null() {
            let mut early = [std::ptr::null_mut(); FN_SLOTS];
            early[FN_INPUT] = input;
            early[FN_OUTPUT] = output;
            self.rt.store.alloc_words(fn_type, &early)
        } else {
            // SAFETY: `spec` holds `FN_SLOTS` words (the caller's contract).
            unsafe {
                let slots = dyad::value(spec) as *mut DyadPtr;
                *slots.add(FN_INPUT) = input;
                *slots.add(FN_OUTPUT) = output;
            }
            spec
        };
        if !binding.is_null() {
            // SAFETY: `binding` is a binding dyad from the store (the caller's contract).
            unsafe { Binding::set_dyad(binding, node) };
        }
        // A call frame is an instance of its function, so a parameter resolves
        // to a frame slot as a local does (DESIGN ›Resolution is one rule‹). The
        // barrier begins at the current depth, so a lesser depth is outside the function.
        self.cx.frames.push(OpenFn {
            frame: node,
            size: 0,
            below: self.cx.scopes.depth(),
            outer: Vec::new(),
            open_below: self.cx.open.len(),
            returns: Vec::new(),
            result: std::ptr::null_mut(),
            slot_body: result.is_none(),
        });
        // SAFETY: `input` is the record just built, `params` its bindings.
        unsafe {
            let fields = crate::identities::meta::record_fields_of(input);
            for (&param, &binding) in crate::identities::array::items(fields).iter().zip(params) {
                // Sized by the rule the argument block lays them out by (`by_copy::slots`). The
                // name's bytes are of the parameter's type as declared, its gate node with it.
                let logos = crate::identities::hole::type_in(param);
                let width = crate::identities::by_copy::place_width(self.types, logos);
                let gated = crate::identities::hole::gated_type_in(param);
                let frame = self.cx.frames.last_mut().expect("pushed above");
                Binding::lay_out(binding, gated, node, frame.size);
                frame.size += width;
            }
        }

        // The body is deferred (it runs at calls), so parse-time rebinding is
        // off inside.
        // SAFETY: `input` is the record just built; its record stores its scope.
        let scope = unsafe { crate::identities::meta::record_scope_of(input) };
        // Names declared outside the body may not be moved or freed inside;
        // the parameters, in the scope pushed next, may (DESIGN ›A function's surface‹).
        self.cx.scopes.push_barrier();
        self.cx.scopes.push(scope);
        self.cx.runtime_depth += 1;
        // A `void!` body gives nothing, as a `void` one, and its end fills `value`.
        // SAFETY: `output` is a type node from the store.
        let filled_at_end = unsafe { crate::identities::union::void_value_of(self.types, output) };
        let gives_nothing = output == self.types.void_ || filled_at_end.is_some();
        let value = if gives_nothing { BlockValue::Void } else { BlockValue::Returned };
        let body = self.body_as(value);
        self.cx.runtime_depth -= 1;
        self.cx.scopes.pop();
        self.cx.scopes.pop_barrier();
        let OpenFn { size: frame_size, outer, returns, .. } =
            self.cx.frames.pop().expect("parse_fn pushed a frame");
        let body = body?;
        // The value its lines give is the call's, so an `if` among them is a used one.
        let made = if gives_nothing {
            None
        } else {
            // SAFETY: `body` is the valid dyad just built.
            unsafe { crate::identities::drop_model::handed_ends(self.types, body) }?
        };
        let body = match filled_at_end {
            // SAFETY: `body` is the body just built, giving nothing; `value` is the `void` member
            // of `output`, and `scope` the parameters' scope, the body's.
            Some(value) => unsafe {
                let filled = [output, value];
                crate::identities::union::end_filled(self.rt.store, self.types, body, filled, scope)
            },
            None => body,
        };

        // SAFETY: `body`/`output` are valid dyads just built.
        let body =
            unsafe { crate::identities::commit_fn_body(self.rt.store, self.types, body, output)? };
        // A type's `run` has no `->` to write `own` in, so over a pointer result its close writes
        // it: stand-in for P128.
        // SAFETY: `output` is the type the node was minted with, from the store.
        let plain_pointer = unsafe {
            crate::identities::numtype::is_pointer_type(output)
                && crate::identities::meta::destructor_of(output).is_null()
        };
        let result_type = if result.is_none() && made == Some(true) && plain_pointer {
            // SAFETY: as above.
            let owned = unsafe {
                crate::identities::drop_model::own_result(self.rt.store, self.types, output)
            }?;
            // SAFETY: `node`'s value is the `FN_SLOTS`-word record written above.
            unsafe { *(dyad::value(node) as *mut DyadPtr).add(FN_OUTPUT) = owned };
            owned
        } else {
            output
        };
        // The last value, once its type is, and every `return`'s operand, which a `return` makes
        // the call's value, are checked against the result type a call reads.
        // SAFETY: `result_type` is the result type node the function carries.
        unsafe { check_handed(self.types, result_type, result, made) }?;
        for r in &returns {
            // SAFETY: as above.
            unsafe { check_handed(self.types, result_type, result, r.made) }
                .map_err(|e| self.fail_at(r.at, e))?;
        }
        for r in returns {
            let ends: Vec<DyadPtr> = r
                .live
                .iter()
                .copied()
                .filter(|name| r.ended.contains(name))
                .chain(r.landed.iter().copied())
                .collect();
            // SAFETY: `r.node` is a `return` node built in this body, not yet run; `ends` are
            // places the scopes it leaves hold.
            unsafe {
                crate::identities::commit_fn_body(self.rt.store, self.types, r.node, output)?;
                crate::identities::return_mod::set_ends(self.rt.store, self.types, r.node, &ends);
            }
        }

        let frame = if frame_size == 0 {
            std::ptr::null_mut()
        } else {
            let u64_ty = self.types.numtypes[crate::identities::NumType::U64 as usize];
            self.rt.store.alloc_blob(u64_ty, &(frame_size as u64).to_ne_bytes())
        };
        let outer = if outer.is_empty() {
            std::ptr::null_mut()
        } else {
            crate::identities::array::build(self.rt.store, self.types.array_, &outer)
        };
        // SAFETY: `node`'s value is the `FN_SLOTS`-word record written above, nothing else's.
        unsafe {
            let slots = dyad::value(node) as *mut DyadPtr;
            *slots.add(FN_BODY) = body;
            *slots.add(FN_FRAME) = frame;
            *slots.add(FN_OUTER) = outer;
        }
        Ok(node)
    }

    /// A `return` just built, its word at `at`, written only inside a function: it leaves
    /// every scope open since the body began, so a name one of them holds moves out, as the
    /// body's last line does, and the address of one may not leave; the body's close checks
    /// what it hands on against the result and commits it to the result type.
    ///
    /// # Safety
    /// `node` must be a `return` node `[value, op]` from the store.
    pub(crate) unsafe fn note_return(
        &mut self,
        node: DyadPtr,
        at: usize,
    ) -> Result<(), ParseError> {
        let Some(frame) = self.cx.frames.last() else {
            return Err(self.fail_at(at, ParseError::ReturnOutsideFunction));
        };
        let open_below = frame.open_below;
        let types = self.types;
        let place = types.through(*(dyad::value(node) as *const DyadPtr));
        let left = |p: DyadPtr| self.holder_of(p).is_some_and(|holder| holder >= open_below);
        if crate::identities::drop_model::lent_place(types, place).is_some_and(left) {
            return Err(ParseError::AddressOfHeld);
        }
        if left(place) {
            self.return_moves_out(node, place)?;
        }
        // Whether the operand is owned is read from one arm, so every arm must agree first.
        let operand = *(dyad::value(node) as *const DyadPtr);
        let made = crate::identities::drop_model::handed_ends(types, operand)?;
        // What a `return` hands on is the caller's, never its line's.
        self.take(operand);
        let live = self.cx.open[open_below..]
            .iter()
            .flat_map(|s| &s.exit)
            .filter(|item| item.end.is_none())
            .map(|item| item.what)
            .collect();
        let landed = self.cx.open[open_below..]
            .iter()
            .flat_map(|s| {
                let in_line = |h: &ExitItem| s.arguments || h.from == s.lines + 1;
                s.exit.iter().filter(move |h| {
                    h.end.is_none() && in_line(h) && s.address_holds.contains(&h.what)
                })
            })
            .map(|h| h.what)
            .collect();
        let open = OpenReturn {
            node,
            at,
            made,
            live,
            ended: Vec::new(),
            landed,
            level: self.cx.open.len() - 1,
            line_closed: false,
            cursor: self.cx.ended_log.len(),
        };
        self.cx.frames.last_mut().expect("checked above").returns.push(open);
        Ok(())
    }

    /// What the `return` at the center of `tape` leaves with: the value to its right, or where none
    /// stands, what the result place holds (›A scope's value is what it evaluates to, and `return`
    /// is an optional early exit from the enclosing function‹ [[R207]]).
    pub(crate) fn return_operand(&mut self, tape: &mut ParsingTape) -> Result<DyadPtr, ParseError> {
        if self.side_at(tape, 1)?.is_some() {
            return self.take_right(tape);
        }
        let mut cell = *tape.at(0).expect("the word's own cell");
        cell.dyad = self.result_place(cell.start)?;
        self.check_unwritten(&cell)?;
        Ok(cell.dyad)
    }

    /// `return a` of a name a scope the `return` leaves holds: the operand becomes `move a`, and
    /// `a` is dead from this line, as a written `move a` makes it (›A last value moves out‹
    /// [[R66]]).
    ///
    /// # Safety
    /// `node` must be a `return` node `[value, op]` from the store; `binding` the name's
    /// binding, held by an open scope.
    unsafe fn return_moves_out(
        &mut self,
        node: DyadPtr,
        binding: DyadPtr,
    ) -> Result<(), ParseError> {
        if self.ends_outer_name(Binding::read(binding).scope, binding) {
            return Err(ParseError::MoveOfOuterName);
        }
        let moved = crate::identities::drop_model::build_move(self.rt.store, self.types, binding);
        *(dyad::value(node) as *mut DyadPtr) = moved;
        self.mark_dead(Ended::name(binding), moved);
        Ok(())
    }

    /// The returns of the function being parsed from index `from`; none outside one.
    fn returns_from(&mut self, from: usize) -> &mut [OpenReturn] {
        self.cx.frames.last_mut().map(|f| &mut f.returns[from..]).unwrap_or_default()
    }

    fn returns_len(&self) -> usize {
        self.cx.frames.last().map_or(0, |f| f.returns.len())
    }

    /// A `defer`'s body. It runs as its scope ends, once the way out is chosen and with nothing
    /// to take a value, so a `return` in it, at any depth, and a body that gives a value are
    /// refused where they are written (›A scope's value is what it evaluates to, and `return` is
    /// an optional early exit from the enclosing function‹ [[R207]]), as a raise and a `try` are,
    /// whose error nobody there receives.
    pub(crate) fn parse_deferred(&mut self) -> Result<DyadPtr, ParseError> {
        let from = self.returns_len();
        self.cx.deferring.push(self.cx.frames.len());
        let body = self.parse_expression_cell();
        self.cx.deferring.pop();
        let body = body?;
        if let Some(at) = self.returns_from(from).first().map(|r| r.at) {
            return Err(self.fail_at(at, ParseError::ReturnInDefer));
        }
        // SAFETY: `body` is the reduced dyad just parsed.
        if unsafe { flow::of(self.types, body.dyad) }.gives {
            return Err(self.fail_at(body.start, ParseError::DeferGives));
        }
        Ok(body.dyad)
    }

    /// `if cond then` with an optional `else else`: the node is
    /// `[cond, then, else]`, else null when absent, so an else-less `if` is a
    /// statement. Each branch is a bracket or the next expression, and a bare
    /// `else` binds to the nearest `if`.
    pub fn parse_if(&mut self, if_type: DyadPtr, alone: bool) -> Result<DyadPtr, ParseError> {
        let returns_start = self.returns_len();
        let cond = self.parse_condition()?;
        let types = self.types;

        // A comptime condition resolves the conditional now, in the one pass:
        // an untaken bracket's tokens are dropped unlexed, so nothing inside
        // is resolved, committed or declared, and branches for other comptime types coexist.
        // SAFETY: `cond` is the reduced dyad just parsed.
        let value = unsafe { condition_value(types, cond) };
        // SAFETY: `value` is `cond` or one of its lines.
        if let Some(truth) = unsafe { bool_literal_value(types, value) } {
            return self.parse_comptime_if(if_type, cond, truth, alone);
        }

        // DESIGN ›A type is a comptime value, resolved in the pass‹, a checked tape cell.
        // SAFETY: as above.
        let check = unsafe { type_check_of(types, value) };

        // A runtime branch may or may not run, so parse-time rebinding is off
        // inside it.
        self.cx.runtime_depth += 1;
        self.cx.narrow_next = check.filter(|&(_, _, holds)| holds).map(|(key, ty, _)| (key, ty));
        let (if_depth, log_start) = (self.cx.open.len(), self.cx.ended_log.len());
        let returns_then = self.returns_len();
        let ends_mark = self.cx.scopes.ends_mark();
        let mut ended_by_then = Vec::new();
        self.cx.next_passes_out = alone;
        let then = self.parse_branch();
        self.cx.handed_tail = None;
        let (log_then, returns_else) = (self.cx.ended_log.len(), self.returns_len());
        self.cx.narrow_next = None;
        // `else if …` is sugar: the `if` right after `else` becomes the
        // else-branch directly, so a chain nests right-associatively.
        let branches = then.and_then(|then| {
            let els = if self.consume_else() {
                // SAFETY: the journal holds the bindings this parse ended.
                ended_by_then = unsafe { self.cx.scopes.undo_ends(ends_mark) };
                if self.consume_token(self.types.if_) {
                    self.parse_if(if_type, alone)
                } else {
                    self.cx.narrow_next =
                        check.filter(|&(_, _, holds)| !holds).map(|(key, ty, _)| (key, ty));
                    self.cx.next_passes_out = alone;
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
        // What the then arm ended is dead after the `if`, as what the else arm ended is.
        for (name, node) in ended_by_then {
            // SAFETY: `name` is a binding dyad the journal held.
            if !unsafe { Binding::read(name) }.is_dead() {
                // SAFETY: as above.
                unsafe { self.cx.scopes.mark_dead(name, node) };
            }
        }

        // A `!=` check whose branch always leaves, as a raise does even where it hands back a
        // `T!`, leaves the cell checked for the rest of the scope (›A tape read checked against a
        // number type reads as that number‹ [[R229]]).
        if let Some((key, ty, false)) = check {
            // SAFETY: `then` is the reduced dyad just parsed.
            if els.is_null() && unsafe { flow::of(types, then) }.ends_scope() {
                let scope = self.cx.open.last_mut().expect("the `if` stands in an open scope");
                scope.narrowed.push((key, ty));
            }
        }

        // A held name an arm moved or freed ends on every path: an arm that did not end it
        // frees it at its end, an else-less `if` in the arm it gains, and a condition that
        // leaves before either arm runs frees it too (DESIGN ›`move` and `free` are static: the
        // parse marks the name dead‹, rule 2).
        let log_end = self.cx.ended_log.len();
        let then_ended = self.ended_outside(log_start..log_then, if_depth);
        let else_ended = self.ended_outside(log_then..log_end, if_depth);
        let not_in = |ended: &[DyadPtr], other: &[DyadPtr]| -> Vec<DyadPtr> {
            other.iter().copied().filter(|name| !ended.contains(name)).collect()
        };
        let then_ends = not_in(&then_ended, &else_ended);
        let else_ends = not_in(&else_ended, &then_ended);
        let condition_ends = self.ended_outside(log_start..log_end, if_depth);
        // A `return` in the condition or the then arm leaves through the `if`, which frees
        // these on its way, though its line may end them after it, in the else arm. An else
        // arm's `return` stands after the then arm in the log, so its line never ends them.
        let returns = self.returns_from(returns_start);
        let (in_condition, in_then) = returns.split_at_mut(returns_then - returns_start);
        for r in in_condition {
            r.live.retain(|name| !condition_ends.contains(name));
        }
        for r in &mut in_then[..returns_else - returns_then] {
            r.live.retain(|name| !then_ends.contains(name));
        }
        let node = crate::identities::if_mod::build(self.rt.store, types, cond, then, els);
        // SAFETY: `node` was just built; the ends are places the enclosing scopes hold.
        unsafe {
            crate::identities::if_mod::set_ends(
                self.rt.store,
                types,
                node,
                [&then_ends, &else_ends, &condition_ends],
            )
        };
        Ok(node)
    }

    /// The held names the ended log's `range` ended that a scope outside depth `inside` holds, in
    /// the order they were declared, each once.
    fn ended_outside(&self, range: std::ops::Range<usize>, inside: usize) -> Vec<DyadPtr> {
        let mut names: Vec<(usize, usize, DyadPtr)> = self.cx.ended_log[range]
            .iter()
            .filter(|&&(_, holder)| holder < inside)
            .map(|&(name, holder)| {
                // SAFETY: held places and `name` are dyads from the store.
                let at =
                    self.cx.open[holder].exit.iter().position(|h| unsafe { self.holds(h, name) });
                (holder, at.unwrap_or(usize::MAX), name)
            })
            .collect();
        names.sort_by_key(|&(holder, at, _)| (holder, at));
        let mut once: Vec<DyadPtr> = Vec::with_capacity(names.len());
        for (_, _, place) in names {
            if !once.contains(&place) {
                once.push(place);
            }
        }
        once
    }

    fn bracketed_body(&mut self) -> Result<DyadPtr, ParseError> {
        self.expect_open()?;
        let body = self.parse_sequence()?;
        self.expect_close()?;
        Ok(body)
    }

    /// An `if` or `while` condition: a scope of its own whether or not it is bracketed, so what it
    /// makes that nothing takes, and what `&` holds there, ends once it has been read (›A call
    /// stays a call in a condition, and `while` reads as `if`‹ [[R266]]).
    fn parse_condition(&mut self) -> Result<DyadPtr, ParseError> {
        self.cx.next_block = BlockValue::Condition;
        let cond = self.parse_block(false, LinesEnd::Condition);
        self.cx.next_block = BlockValue::Used;
        let cond = cond?.1.dyad;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if unsafe { crate::identities::read::output_type(self.types, cond) } != self.types.bool_ {
            return Err(ParseError::NonBoolCondition);
        }
        Ok(cond)
    }

    /// [`Self::bracketed_body`] for a body whose value the construct around it does `value` with.
    fn body_as(&mut self, value: BlockValue) -> Result<DyadPtr, ParseError> {
        self.cx.next_block = value;
        let body = self.bracketed_body();
        self.cx.next_block = BlockValue::Used;
        body
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
            return self.body_text_extent().map(|_| ());
        }
        self.cx.runtime_depth += 1;
        self.cx.dropping += 1;
        let dead = self.parse_branch();
        self.cx.dropping -= 1;
        self.cx.runtime_depth -= 1;
        dead.map(|_| ())
    }

    /// True with an else: the then-branch is the result and the else-tail is
    /// dropped. False: the then-branch is dropped. An else-less `if` stays an `if`
    /// node either way, its untaken arm none.
    fn parse_comptime_if(
        &mut self,
        if_type: DyadPtr,
        cond: DyadPtr,
        truth: bool,
        alone: bool,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let none = std::ptr::null_mut();
        if truth {
            self.cx.next_passes_out = alone;
            let then = self.parse_branch()?;
            self.cx.handed_tail = None;
            if self.consume_else() {
                self.skip_else_tail(if_type)?;
                return Ok(then);
            }
            return Ok(crate::identities::if_mod::build(self.rt.store, types, cond, then, none));
        }
        self.skip_branch()?;
        if self.consume_else() {
            if self.consume_token(self.types.if_) {
                return self.parse_if(if_type, alone);
            }
            self.cx.next_passes_out = alone;
            return self.parse_branch();
        }
        Ok(crate::identities::if_mod::build(self.rt.store, types, cond, none, none))
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
                self.cx.dropping += 1;
                let dead = self.parse_if(if_type, false);
                self.cx.dropping -= 1;
                self.cx.runtime_depth -= 1;
                return dead.map(|_| ());
            }
            self.body_text_extent()?; // ( cond )
            self.skip_branch()?;
            if !self.consume_else() {
                return Ok(());
            }
        }
    }

    /// The node is `[operand, op, bool]`.
    ///
    /// # Safety
    /// `operand` must be a reduced dyad from the store.
    pub(crate) unsafe fn build_not(
        &mut self,
        not_id: DyadPtr,
        operand: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        if crate::identities::read::output_type(types, operand) != types.bool_ {
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
        Ok(self.rt.store.alloc_words(not_id, &[operand, types.ops.not_, types.bool_]))
    }

    /// `while cond body`: the node is `[cond, body]`; the condition and body are read as `if`'s
    /// are; a value the body gives ends the loop and passes out (›A scope's value is what it
    /// evaluates to, and `return` is an optional early exit from the enclosing function‹
    /// [[R207]]); a `return` in it leaves the enclosing function.
    pub fn parse_while(&mut self, while_id: DyadPtr, alone: bool) -> Result<DyadPtr, ParseError> {
        // The condition runs every pass too, so a name declared outside may not be moved or
        // freed in it either.
        self.cx.scopes.push_barrier();
        let body = self.parse_while_parts(alone);
        self.cx.scopes.pop_barrier();
        let (cond, body) = body?;
        Ok(self.rt.store.alloc_words(while_id, &[cond, body, self.types.ops.while_]))
    }

    fn parse_while_parts(&mut self, alone: bool) -> Result<(DyadPtr, DyadPtr), ParseError> {
        self.skip_whitespace();
        let start = self.cx.pos;
        let held_end = if self.in_pass_order() { self.condition_reads_call() } else { None };
        let cond = if let Some(end) = held_end {
            self.cx.pos = end;
            self.hold_condition(start)?
        } else {
            let made = self.cx.open.last().map(|o| o.made.clone());
            let was = std::mem::replace(&mut self.cx.each_pass, true);
            let was_read = std::mem::replace(&mut self.cx.each_pass_read, false);
            let cond = self.parse_condition();
            self.cx.each_pass = was;
            let read = std::mem::replace(&mut self.cx.each_pass_read, was_read);
            let cond = cond?;
            // The condition stood for the nodes its `dyad` names held as it was parsed, and runs
            // again each pass: it is parsed again each time (›A `dyad ?` place is transparent: a
            // placeholder for a new node of any type‹ [[R293]]), and what this parse made never
            // runs.
            if read {
                if let (Some(open), Some(made)) = (self.cx.open.last_mut(), made) {
                    open.made = made;
                }
                self.hold_condition(start)?
            } else {
                cond
            }
        };
        // A repeated body: parse-time rebinding is off inside.
        self.cx.runtime_depth += 1;
        let body = self.loop_body(alone);
        self.cx.runtime_depth -= 1;
        Ok((cond, body?))
    }

    /// The body of `while` or `for`, a bracket or the next expression, as an `if` arm is read.
    fn loop_body(&mut self, alone: bool) -> Result<DyadPtr, ParseError> {
        self.cx.next_block = BlockValue::Body;
        self.cx.next_passes_out = alone;
        let body = self.parse_branch();
        self.cx.next_block = BlockValue::Used;
        body
    }

    /// `for i in a..b ( body )`, with an optional `..step` and optionally no
    /// index (DESIGN ›The scope's constructor is the driver‹): end-exclusive;
    /// the counter is a fresh block-local either way, so both tiers run one node shape.
    pub fn parse_for(&mut self, for_id: DyadPtr, alone: bool) -> Result<DyadPtr, ParseError> {
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
            step.is_some_and(|s| unsafe { dyad::ty(types.through(s)) } == types.rational);
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
                // Storage here would mean the literal check above let a variable through.
                use crate::identities::read::{read_kind, Read};
                if !matches!(read_kind(types, step), Read::Scalar(_)) || types.is_storage(step) {
                    return Err(ParseError::BadStep);
                }
                (
                    numtype::read_scalar(dyad::ty(step), dyad::value(step)),
                    numtype::of_type_node(logos),
                )
            };
            if numtype::apply_compare(numtype::CmpOp::Gt, nt, bits, 0) == 0 {
                return Err(ParseError::BadStep);
            }
        }

        // SAFETY: `logos` is a numtype node from resolve_loop_parts.
        let width = unsafe { crate::identities::numtype::of_type_node(logos) }.bytes();
        let parent = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        let scope = crate::identities::scope::mint(self.rt.store, types.scope, parent);
        // A repeated body: a name declared outside may not be moved or freed
        // inside; the loop variable, declared in the scope pushed next, is inside.
        self.cx.scopes.push_barrier();
        self.cx.scopes.push(scope);
        // Parse-time rebinding is off inside a repeated body, the counter's name included.
        self.cx.runtime_depth += 1;
        // A named counter is a name of the enclosing frame; a nameless one is the run's own.
        let mut var = std::ptr::null_mut();
        let body = match name {
            Some((nstart, nlen)) => {
                let source = self.cx.source;
                self.declare_name(&source[nstart..nstart + nlen], logos, nstart).and_then(
                    |binding| {
                        // SAFETY: `binding` was just declared; `logos` is a type node.
                        var = unsafe { self.place_for(binding, logos, width) };
                        self.loop_body(alone)
                    },
                )
            }
            None => self.loop_body(alone),
        };
        self.cx.runtime_depth -= 1;
        self.cx.scopes.pop();
        self.cx.scopes.pop_barrier();
        let body = body?;

        let ops = [var, start, end, step, body, types.ops.for_];
        Ok(self.rt.store.alloc_words(for_id, &ops))
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

    /// What a bracket's values go through before the constructor that took the bracket builds
    /// its node: a held type is built, a group `a | b` spreads the build over its two members, each
    /// value's type is read, and a value made here is held by the parameter it lands in (›Holding
    /// is decided at the binding site, parameters included‹ [[R61]]).
    ///
    /// # Safety
    /// `callee` and every `arg` must be dyads from the store, and `build` must take values that
    /// are.
    unsafe fn over_arguments<F>(
        &mut self,
        callee: DyadPtr,
        args: &[DyadPtr],
        build: &mut F,
    ) -> Result<DyadPtr, ParseError>
    where
        F: FnMut(&mut Self, &[DyadPtr]) -> Result<DyadPtr, ParseError>,
    {
        let mut args: Vec<DyadPtr> = args.iter().map(|&a| self.held_type_or(a)).collect();
        for i in 0..args.len() {
            // SAFETY: `args` are reduced dyads from the store.
            if let Some((connective, a, b)) = crate::identities::group::members(self.types, args[i])
            {
                args[i] = a;
                let a = self.over_arguments(callee, &args, build)?;
                args[i] = b;
                let b = self.over_arguments(callee, &args, build)?;
                return crate::identities::group::join(self.rt.store, self.types, connective, a, b);
            }
        }
        for (i, &arg) in args.iter().enumerate() {
            // An `if` whose arms give two types gives one where its parameter's type lands each
            // arm.
            // SAFETY: `args` are reduced dyads just parsed.
            match unsafe { crate::identities::read::value_type(self.types, arg) } {
                Ok(_) | Err(ParseError::ArmsDiffer) => {}
                Err(e) => return Err(e),
            }
            // A `dyad` is held only where it lands as itself, being read through elsewhere.
            // SAFETY: as above; `callee` is a resolved dyad from the store.
            unsafe {
                use crate::identities::drop_model::{param_holds, teardown_of, Teardown};
                if !matches!(teardown_of(self.types, arg), Teardown::Nothing | Teardown::Dyad)
                    && !param_holds(self.types, callee, i)
                {
                    return Err(ParseError::UnboundOwningValue);
                }
            }
        }
        build(self, &args)
    }

    /// `T (…)` for a number type `T`: the written conversion, losing what `T` cannot hold where
    /// `lossy` is set, as `i32 lossy (x)` (›A type's `parse` decides what converts into it;
    /// unwritten, a value converts only into a type that holds it exactly‹ [[R155]]).
    ///
    /// # Safety
    /// `ty` must be a number type node and every `arg` a dyad from the store.
    pub(crate) unsafe fn build_conversion(
        &mut self,
        ty: DyadPtr,
        args: &[DyadPtr],
        lossy: bool,
    ) -> Result<DyadPtr, ParseError> {
        self.over_arguments(ty, args, &mut |p, args| {
            if let &[value] = args {
                // SAFETY: as below.
                if let Some(checked) = unsafe { p.checked_conversion(ty, value, lossy) }? {
                    return Ok(checked);
                }
            }
            // SAFETY: `ty` is a number type node; `args` are reduced dyads.
            unsafe { crate::identities::build_cast(p.rt.store, p.types, ty, args, lossy) }
        })
    }

    /// The written conversion of `value` into the number type `ty` where it may have no value of
    /// `ty`, a float of a `lossy` type into an integer type: it gives `ty!`, whose `err` holds an
    /// error of `error.no_whole_number` where the float is no number (NaN) or infinite (›A checked
    /// error is a fault: the task that hit it is cancelled‹ [[R141]]). `None` for every other
    /// conversion.
    ///
    /// # Safety
    /// `ty` must be a number type node and `value` a reduced dyad from the store.
    pub(crate) unsafe fn checked_conversion(
        &mut self,
        ty: DyadPtr,
        value: DyadPtr,
        lossy: bool,
    ) -> Result<Option<DyadPtr>, ParseError> {
        use crate::identities::{convert, dyad_place, gate, read};
        let types = self.types;
        let lossy = lossy || gate::is_lossy(types, value);
        let value =
            dyad_place::read_through(self.rt.store, types, value, ty, lossy).unwrap_or(value);
        let from = read::output_type(types, value);
        if !lossy || !convert::may_fail(types, from, ty) {
            return Ok(None);
        }
        let fallible = self.fallible_of(ty, self.cx.pos)?;
        Ok(Some(convert::build_checked(self.rt.store, types, value, from, fallible)))
    }

    /// `T (…)` for a record type `T` whose body fills no `parse`: a value of `T` from the
    /// bracket's values, positional in order (›`X (…)` is one spelling, and X's constructor
    /// decides what the bracket is‹ [[R270]]).
    ///
    /// # Safety
    /// `ty` must be a record type node and every `arg` a dyad from the store.
    pub(crate) unsafe fn build_instance(
        &mut self,
        ty: DyadPtr,
        args: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        self.over_arguments(ty, args, &mut |p, args| {
            // SAFETY: `ty` is a record type node; `args` are reduced dyads from the store.
            unsafe {
                if !crate::identities::meta::run_body_of(ty).is_null() {
                    return Err(ParseError::RunTypeApplied);
                }
                crate::identities::instance::build_ctor(
                    p.rt.store,
                    p.types,
                    p.types.construct_,
                    ty,
                    args,
                    crate::identities::dyad_place::Holder::Nobody,
                )
            }
        })
    }

    /// `f (…)` for a function `f`: the call. A record result is copied out where the call runs,
    /// and a call for a type resolves now, in the pass, where it can.
    ///
    /// # Safety
    /// `callee` and every `arg` must be dyads from the store.
    pub(crate) unsafe fn build_fn_call(
        &mut self,
        callee: DyadPtr,
        args: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        self.over_arguments(callee, args, &mut |p, args| {
            // SAFETY: `callee` and `args` are reduced dyads from the store.
            unsafe { p.call_over(callee, args.to_vec()) }
        })
    }

    /// # Safety
    /// As [`Self::build_fn_call`].
    unsafe fn call_over(
        &mut self,
        callee: DyadPtr,
        mut args: Vec<DyadPtr>,
    ) -> Result<DyadPtr, ParseError> {
        // Each uncommitted literal argument commits to its parameter's declared type.
        let types = self.types;
        // SAFETY: `callee` and `args` are reduced dyads from the store.
        unsafe {
            use crate::identities::drop_model::{build_hold, param_holds, teardown_of, Teardown};
            let written = args.clone();
            crate::identities::commit_call_args(self.rt.store, types, callee, &mut args)?;
            for (i, arg) in args.iter_mut().enumerate() {
                // What the argument lands as, a new node a `dyad ?` parameter takes a value
                // in included, which takes the value written there with it.
                if teardown_of(types, *arg) == Teardown::Nothing {
                    continue;
                }
                if !param_holds(types, callee, i) {
                    return Err(ParseError::UnboundOwningValue);
                }
                self.take(written[i]);
                self.take(*arg);
                *arg = build_hold(self.rt.store, types, *arg);
            }
        }
        let call = build_call(self.rt.store, callee, &args);
        // SAFETY: `callee` is a reduced dyad; a `fn` node holds `FN_SLOTS` words.
        let record_out = unsafe {
            if dyad::ty(callee) == types.fn_type {
                let out = *(dyad::value(callee) as *const DyadPtr).add(FN_OUTPUT);
                crate::identities::read::carry_of(types, out)
                    .and_then(crate::identities::read::Carry::bytes)
                    .map(|w| (out, w))
            } else {
                None
            }
        };
        if let Some((out, _)) = record_out {
            // The record result is copied out where the call runs: into the name that
            // takes it, else scratch.
            return Ok(crate::identities::by_copy::build_result(
                self.rt.store,
                types,
                std::ptr::null_mut(),
                call,
                out,
            ));
        }
        // A type-returning call resolves now, at comptime, so the result
        // flows as an ordinary type value; the outer-name check runs first,
        // since a body that reads a dead name must not run.
        // SAFETY: `callee` is a reduced dyad.
        if unsafe { self.returns_type(callee) } {
            self.check_call_reads(callee)?;
            // A call that waits for the run gives a type value then.
            if !self.runs_in_pass(&args) {
                return Ok(call);
            }
            // SAFETY: `call` was just built over reduced dyads.
            unsafe { self.eval_type_call(call) }
        } else {
            Ok(call)
        }
    }

    /// Mint a scope node carrying its enclosing scope (DESIGN ›Meta-navigation‹)
    /// and push it; the link is set before anything inside is parsed, so a
    /// constructor running inside walks up over settled structure.
    pub(super) fn open_scope(&mut self) -> DyadPtr {
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
        self.parse_block(false, LinesEnd::Closer).map(|(_, value)| value)
    }

    /// The lines of the open scope, each closed as it completes, read to the scope's end
    /// (DESIGN ›The scope's constructor is the driver‹). Returns the scope's tail, its last line
    /// that is not prose, with where that line ended; none when there is no such line.
    pub fn read_lines(&mut self, end: LinesEnd) -> Result<Option<(Cell, usize)>, ParseError> {
        if end == LinesEnd::Condition {
            let Built { items, before, after } = self.drive_segment(RightSide::Condition)?;
            let line = self.one_of(items)?;
            let open = self.cx.open.last_mut().expect("the condition's scope is open");
            let handed = std::mem::take(&mut open.prose);
            let lines = before.into_iter().chain([line.dyad]).chain(handed).chain(after);
            for item in lines {
                // SAFETY: `item` is the line just parsed or a comment lifted out of it.
                unsafe { self.close_line(item) };
            }
            return Ok(Some((line, self.cx.pos)));
        }
        let mut tail = None;
        loop {
            let held_back = self.cx.held_back.len();
            let Some(item) = self.next_cell() else {
                break;
            };
            let mut line = item?;
            // SAFETY: a line is a reduced dyad from the store.
            let prose = unsafe { crate::identities::numtype::is_comment_type(dyad::ty(line.dyad)) };
            if !prose {
                self.check_line_runs(&mut line, held_back)?;
            }
            // SAFETY: `line` is the line `next_cell` just returned.
            unsafe { self.close_line(line.dyad) };
            if !prose {
                tail = Some((line, self.cx.pos));
            }
        }
        if end == LinesEnd::TextEnd {
            if let Some((_, len)) = self.peek_token().filter(|&(id, _)| self.is_closer(id)) {
                let closer = &self.cx.source[self.cx.pos..self.cx.pos + len];
                return Err(ParseError::StrayCloser(Box::new(closer.to_string())));
            }
        }
        Ok(tail)
    }

    /// A line of the innermost scope, read: where the scope's lines are its own, a line after one
    /// that always ends the scope never runs and is refused, as is a line that gives a value in
    /// a `-> void` body; at a program's or a file's top level, where nothing lands the value a
    /// line gives, the nodes that give it give one type (›A scope's value is what it evaluates
    /// to, and `return` is an optional early exit from the enclosing function‹ [[R207]]).
    fn check_line_runs(&mut self, line: &mut Cell, held_back: usize) -> Result<(), ParseError> {
        let open = self.cx.open.last().expect("a line is read inside an open scope");
        if !open.own_lines() {
            return Ok(());
        }
        // The cells this line holds that no reader took as data are scopes, refused as such.
        let mut unread = None;
        for (cell, start, e) in self.cx.held_back.split_off(held_back) {
            if self.rt.take_unread_lines(cell) {
                unread.get_or_insert((start, e));
            }
        }
        if let Some((start, e)) = unread {
            return Err(self.fail_at(start, e));
        }
        let open = self.cx.open.last_mut().expect("checked above");
        let refused = match open.ended_by {
            Some(Flow { gives: true, .. }) => Some(ParseError::LineAfterValue),
            Some(_) => Some(ParseError::LineAfterLeave),
            None => None,
        };
        if let Some(e) = refused {
            if open.cell {
                open.held_back.get_or_insert((line.start, e));
                return Ok(());
            }
            return Err(self.fail_at(line.start, e));
        }
        let open = self.cx.open.last().expect("checked above");
        // SAFETY: a line is a reduced dyad from the store.
        let flow = unsafe { flow::of(self.types, line.dyad) };
        if let Some(e) = open.gives_none.clone().filter(|_| flow.gives) {
            return Err(self.fail_at(line.start, e));
        }
        let open = self.cx.open.last_mut().expect("checked above");
        // SAFETY: as above.
        let passes = unsafe { flow::passes_value(self.types, line.dyad) };
        if std::mem::take(&mut open.moves_through) && !passes {
            return Err(self.fail_at(line.start, ParseError::HeldTakenOnTheWay));
        }
        if std::mem::take(&mut open.gives_held) && flow.gives {
            // SAFETY: as above.
            if let Some(e) = unsafe { self.gives_held(line.dyad) } {
                return Err(self.fail_at(line.start, e));
            }
        }
        if flow.gives && !passes {
            self.give_held_out(line).map_err(|e| self.fail_at(line.start, e))?;
        }
        let open = self.cx.open.last().expect("checked above");
        if flow.gives && open.top {
            // SAFETY: as above.
            if let Err(e) = unsafe { flow::check_line_value(self.types, line.dyad) } {
                return Err(self.fail_at(line.start, e));
            }
        }
        if flow.ends_scope() {
            self.cx.open.last_mut().expect("checked above").ended_by = Some(flow);
        }
        Ok(())
    }

    /// Why `line`, a line of the innermost scope, may not give a name the scope holds as the
    /// scope's value where it was not moved there: the scope's end frees it under the value.
    ///
    /// # Safety
    /// `line` must be a reduced dyad from the store.
    unsafe fn gives_held(&self, line: DyadPtr) -> Option<ParseError> {
        let types = self.types;
        let open = self.cx.open.last().expect("a line is read inside an open scope");
        let held = |p: DyadPtr| open.exit.iter().any(|h| self.holds(h, p));
        let mut refused = None;
        let mut at = line;
        let _ = flow::walk(types, &mut at, &mut |slot: *mut DyadPtr| {
            let place = types.through(*slot);
            if crate::identities::drop_model::lent_place(types, place).is_some_and(held) {
                refused.get_or_insert(ParseError::AddressOfHeld);
            } else if held(place) {
                refused.get_or_insert(ParseError::HeldGivenUnmoved);
            }
            Ok(())
        });
        refused
    }

    /// A line that gives as its value a name a scope holds, out of each scope it leaves as that
    /// scope's value, up to the holder: the name moves out, as a written `move` moves it, so the
    /// holder's end frees it only on the paths that go on (›A last value moves out‹ [[R66]]).
    /// The address of such a name is refused. At the top level nothing takes the value.
    fn give_held_out(&mut self, line: &mut Cell) -> Result<(), ParseError> {
        let types = self.types;
        // SAFETY: a line is a reduced dyad from the store.
        let place = unsafe { types.through(line.dyad) };
        let reach = self.cx.open.iter().rposition(|o| !o.passes_out).unwrap_or(0);
        // SAFETY: `place` is a dyad from the store.
        let lent = unsafe { crate::identities::drop_model::lent_place(types, place) };
        let holders = [lent, Some(place)]
            .map(|p| p.and_then(|p| self.holder_of(p)).filter(|&h| !self.cx.open[h].top));
        // A holder the value does not leave as its value is told to check the line it gives in:
        // a reader to the left of a construct takes the value, and only that is a borrow.
        for &h in holders.iter().flatten().filter(|&&h| h < reach) {
            self.cx.open[h].gives_held = true;
        }
        let [lent, held] = holders.map(|h| h.filter(|&h| h >= reach));
        if lent.is_some() {
            return Err(ParseError::AddressOfHeld);
        }
        // SAFETY: as above.
        if unsafe { dyad::ty(place) } != types.binding_ {
            return Ok(());
        }
        let Some(holder) = held else {
            return Ok(());
        };
        // SAFETY: `place` is a binding an open scope holds.
        if self.ends_outer_name(unsafe { Binding::read(place) }.scope, place) {
            return Err(ParseError::MoveOfOuterName);
        }
        // SAFETY: as above.
        let moved =
            unsafe { crate::identities::drop_model::build_move(self.rt.store, types, place) };
        let inner = self.cx.open.len() - 1;
        if holder == inner {
            // The line gives the scope its value, so no line after it reads the name: the scope
            // holds it up to this line, and a line held to be parsed when it runs still finds it.
            let open = &self.cx.open[inner];
            // SAFETY: held places and `place` are dyads from the store.
            let at = open.exit.iter().position(|h| unsafe { self.holds(h, place) });
            let open = &mut self.cx.open[inner];
            if let Some(at) = at {
                open.exit[at].end = Some(open.lines);
            }
        } else {
            self.mark_dead(Ended::name(place), moved);
            for open in &mut self.cx.open[holder..inner] {
                open.moves_through = true;
            }
        }
        let open = &mut self.cx.open[inner];
        if let Some(pending) = open.unrun.last_mut().filter(|u| u.node == line.dyad) {
            pending.node = moved;
        }
        line.dyad = moved;
        Ok(())
    }

    /// Each bracket the parse opens, skips or scans recurses, so a wall of `(` costs stack
    /// like a runaway recursion; once the stack is spent the parse is the checked error.
    pub(super) fn check_stack(&self) -> Result<(), ParseError> {
        if crate::stack::exhausted(self.rt.ctx.stack_limit) {
            return Err(ParseError::TooDeep);
        }
        Ok(())
    }

    /// [`Parser::parse_sequence`], with the scope the block opened, whose `dyads` hold
    /// every line whatever the value collapsed to.
    pub(super) fn parse_block(
        &mut self,
        list: bool,
        end: LinesEnd,
    ) -> Result<(DyadPtr, Cell), ParseError> {
        self.parse_block_as(if list { Lines::List } else { Lines::Own }, false, end)
    }

    /// The lines of a `(` just lexed, `tape`'s center: a call's argument list when the call
    /// reading it marked its left neighbour as the callee ([`Self::read_arguments`]), else a
    /// cell a reader may yet take as data.
    pub(crate) fn parse_paren_cell(&mut self, tape: &ParsingTape) -> Result<Cell, ParseError> {
        let left = tape.at(-1).map(|c| c.start);
        let arguments =
            left.is_some() && self.cx.open.last().is_some_and(|o| o.arguments_after == left);
        let lines = if arguments { Lines::Arguments } else { Lines::Own };
        self.cx.next_passes_out = self.starts_line(tape);
        self.parse_block_as(lines, true, LinesEnd::Closer).map(|(_, value)| value)
    }

    /// The lines of a `{` just lexed, the scope kept as a value.
    pub(super) fn parse_kept_block(&mut self) -> Result<(DyadPtr, Cell), ParseError> {
        self.parse_block_as(Lines::Kept, false, LinesEnd::Closer)
    }

    /// The cell `offset` right of the center, read by a call whose callee stands just left of
    /// it: a `(` lexed there is the call's argument list.
    pub(crate) fn read_arguments(
        &mut self,
        tape: &mut ParsingTape,
        offset: isize,
    ) -> Result<Option<Cell>, ParseError> {
        let callee = tape.at(offset - 1).map(|c| c.start);
        let was =
            self.cx.open.last_mut().map(|o| std::mem::replace(&mut o.arguments_after, callee));
        let cell = self.cell_at(tape, offset);
        if let (Some(open), Some(was)) = (self.cx.open.last_mut(), was) {
            open.arguments_after = was;
        }
        cell
    }

    fn parse_block_as(
        &mut self,
        lines: Lines,
        cell: bool,
        end: LinesEnd,
    ) -> Result<(DyadPtr, Cell), ParseError> {
        self.check_stack()?;
        let value = std::mem::take(&mut self.cx.next_block);
        let scope = self.open_scope();
        let narrowed = self.cx.narrow_next.take().into_iter().collect();
        let gives_none = match (value, lines) {
            (_, Lines::Kept) => Some(ParseError::KeptScopeGives),
            (BlockValue::Void, _) => Some(ParseError::VoidBodyGives),
            _ => None,
        };
        let (list, arguments) = (lines == Lines::List, lines == Lines::Arguments);
        let passes_out = std::mem::take(&mut self.cx.next_passes_out)
            && matches!(value, BlockValue::Used | BlockValue::Body)
            && lines == Lines::Own;
        let kept = (lines == Lines::Kept).then(|| KeptValue {
            ty: std::ptr::null_mut(),
            readers: Vec::new(),
            depth: (self.cx.frames.len(), self.cx.definitions.len()),
            in_program_frame: self.in_program_frame(),
        });
        let open = OpenScope {
            narrowed,
            scope: Some(scope),
            list,
            arguments,
            gives_none,
            passes_out,
            cell,
            kept,
            ..OpenScope::default()
        };
        self.cx.open.push(open);
        let array_ = self.types.array_;
        let read = self.read_lines(end).and_then(|tail| match lines {
            // SAFETY: `scope` is the block just read, still open.
            Lines::Kept => unsafe { self.close_kept(scope) }.map(Some),
            _ => Ok(tail),
        });
        let tail = match read {
            Ok(tail) => tail,
            Err(e) => {
                self.cx.scopes.pop();
                let open = self.cx.open.pop().expect("pushed above");
                self.end_after_fault(&open);
                return Err(e);
            }
        };
        self.cx.scopes.pop();
        // SAFETY: `scope` was minted by `open_scope` with a value, so it has an
        // array; the store outlives the slice and nothing pushes to it below.
        let exprs = unsafe {
            crate::identities::scope::dyads(self.rt.store, array_, scope);
            crate::identities::scope::exprs_of(scope).expect("a block scope has dyads")
        };
        // What the block parsed and did not run runs when the block runs;
        // what it did run stands in the body as its result.
        let mut closed = self.cx.open.pop().expect("pushed above");
        if let Some((start, e)) = closed.held_back.take() {
            self.rt.hold_back_lines(scope);
            self.cx.held_back.push((scope, start, e));
        }
        self.hand_on_notes(&mut closed);
        if closed.arguments {
            self.hand_on_address_holds(&mut closed);
        }
        let own = closed.own_lines();
        // An argument list's values, and what a `[…]`'s items make beside the items themselves,
        // which go whole to the callee, are made in the line the call stands in.
        if !own {
            if closed.list {
                for &item in exprs {
                    // SAFETY: the list's items are reduced dyads just parsed.
                    let path =
                        unsafe { crate::identities::drop_model::handed_through(self.types, item) };
                    closed.held.retain(|m| !path.contains(m));
                }
            }
            self.hand_on_held(&mut closed);
        }
        // Prose and a `defer` (it runs at exit, never as the tail) are
        // invisible to value flow.
        let defer_ = self.types.defer_;
        // SAFETY: `exprs` are reduced dyads just parsed/built.
        let is_value = |e: DyadPtr| unsafe {
            !crate::identities::numtype::is_comment_type(dyad::ty(e)) && dyad::ty(e) != defer_
        };
        let values = exprs.iter().filter(|&&e| is_value(e)).count();
        let mut handed = Vec::new();
        if values > 0 {
            let types = self.types;
            // An argument list's or a `[…]`'s last item that is a place it holds moves out to
            // the call; its address may not leave. A block's own lines did this as each closed.
            if !own {
                let tail = exprs.iter().rposition(|&e| is_value(e)).expect("values >= 1");
                // SAFETY: `exprs[tail]` is a reduced dyad just parsed.
                let place = unsafe { types.through(exprs[tail]) };
                // SAFETY: held places and `place` are dyads from the store.
                let held_at =
                    |p: DyadPtr| closed.exit.iter().position(|h| unsafe { self.holds(h, p) });
                // SAFETY: as above.
                let lent = unsafe { crate::identities::drop_model::lent_place(types, place) };
                if lent.and_then(held_at).is_some() {
                    self.end_after_fault(&closed);
                    return Err(ParseError::AddressOfHeld);
                }
                if let Some(at) = held_at(place) {
                    closed.exit[at].end = Some(tail);
                    // SAFETY: `place` is a place this scope's binding site laid out, and `tail`
                    // indexes the scope's own lines, which nothing else reads yet.
                    unsafe {
                        let moved =
                            crate::identities::drop_model::build_move(self.rt.store, types, place);
                        crate::identities::scope::set_item(scope, tail, moved);
                    }
                }
            }
            // What the lines that give the block its value give is the block's own, handed on
            // to whatever takes the block's value, unless the construct around it throws it
            // away (›A scope's value is what it evaluates to, and `return` is an optional early
            // exit from the enclosing function‹ [[R207]]).
            if value != BlockValue::Thrown && value != BlockValue::Void && own {
                // SAFETY: `scope` is the block just read, its lines reduced dyads.
                let path = unsafe { crate::identities::drop_model::handed_through(types, scope) };
                handed = closed.held.iter().copied().filter(|m| path.contains(m)).collect();
                closed.held.retain(|m| !path.contains(m));
            }
        }
        let used =
            matches!(value, BlockValue::Used | BlockValue::Body | BlockValue::Condition) && own;
        // One line, and nothing for the end to run: the line stands as itself, so a `.` reads
        // the line's node (›`x.type` reads the type word of the node a name or an expression
        // stands for‹ [[R279]]), and what it holds to its end the line the block stands in
        // holds, unless that line runs it again.
        let ends_run = closed.exit.iter().any(ExitItem::held_somewhere);
        let lends = value == BlockValue::Used && own;
        if values > 0 && exprs.len() == 1 && !ends_run && (closed.held.is_empty() || lends) {
            if let Some(open) = self.cx.open.last_mut() {
                open.made.append(&mut closed.held);
            }
            let line = tail.map(|(cell, _)| cell).filter(|c| c.dyad == exprs[0]);
            // SAFETY: `exprs` are reduced dyads from the store.
            let cell = line.unwrap_or_else(|| unsafe { Cell::built(exprs[0]) });
            if used {
                self.hand_on_block(cell.dyad, handed);
            }
            return Ok((scope, cell));
        }
        let exit: Vec<DyadPtr> =
            closed.exit.iter().map(|h| h.build(self.rt.store, self.types)).collect();
        // SAFETY: `scope` was minted by `open_scope` above and is unaliased.
        let cell = unsafe {
            crate::identities::scope::fill(self.types, scope);
            crate::identities::scope::extend_exit(self.rt.store, array_, scope, &exit);
            crate::identities::scope::extend_held(self.rt.store, array_, scope, &closed.held);
            Cell::built(scope)
        };
        if used {
            self.hand_on_block(scope, handed);
        }
        Ok((scope, cell))
    }

    /// `closed`, a scope just closed inside the line being parsed: what it runs when that line
    /// runs, and the values the pass ran early in it, go to the scope open around it, and a
    /// `return` inside it stands in that scope's line from here.
    pub(super) fn hand_on_notes(&mut self, closed: &mut OpenScope) {
        if let Some(open) = self.cx.open.last_mut() {
            open.runs.append(&mut closed.runs);
            open.runs.append(&mut closed.closed_runs);
            open.runs.extend(closed.unrun.iter().flat_map(|u| &u.runs));
            open.stood.append(&mut closed.take_stood());
        }
        self.close_returns(self.cx.open.len());
    }

    /// `closed`, whose lines are parts of the line being parsed, closed: the values they made
    /// that nothing took are made in that line.
    pub(super) fn hand_on_held(&mut self, closed: &mut OpenScope) {
        if let Some(open) = self.cx.open.last_mut() {
            open.made.retain(|m| !closed.held.contains(m));
            open.made.append(&mut closed.held);
        }
    }

    /// `{ … }`'s last line: the value of the record its names lay out, made from them; its tail.
    /// A name the scope holds moves into the value, whose end ends it (›`{ … }` outside text is a
    /// scope kept as a value, read with `.`‹ [[R374]]).
    ///
    /// # Safety
    /// `scope` must be the innermost open scope, the block whose lines were just read.
    unsafe fn close_kept(&mut self, scope: DyadPtr) -> Result<(Cell, usize), ParseError> {
        let open = self.cx.open.len() - 1;
        let kept = self.cx.open[open].kept.take().expect("a `{ … }`'s lines make its value");
        let (record, names) = self.kept_record(scope, kept)?;
        crate::identities::scope::set_kept_as(scope, record);
        let types = self.types;
        let made_at = self.cx.open[open].lines;
        let mut entries = Vec::with_capacity(names.len());
        for name in names {
            let held = self.cx.open[open].exit.iter().position(|h| self.holds(h, name));
            entries.push(match held {
                Some(at) => {
                    self.cx.open[open].exit[at].end = Some(made_at);
                    crate::identities::drop_model::build_move(self.rt.store, types, name)
                }
                None => name,
            });
        }
        let value = crate::identities::instance::build_ctor(
            self.rt.store,
            types,
            types.construct_,
            record,
            &entries,
            crate::identities::dyad_place::Holder::Owner,
        )?;
        self.close_line(value);
        Ok((Cell::built(value), self.cx.pos))
    }

    /// A block closed whose value is `value`, its last line's or itself: the line it stands in
    /// holds the value to its end unless something takes it, and `handed` are the values its
    /// last line made that it handed on as its own.
    fn hand_on_block(&mut self, value: DyadPtr, handed: Vec<DyadPtr>) {
        self.note_made(value);
        self.cx.handed_tail = Some((value, handed));
    }

    /// A line of the innermost scope is complete: the names it declared or ended settle on
    /// it (see [`ScopeStack::close_item`]), and a `defer` line joins the exit. A held name the
    /// line ended is held up to it; one an enclosing scope holds, this scope holds from its
    /// start up to it, so a `return` or fault before the line frees it (DESIGN ›`move` and
    /// `free` are static: the parse marks the name dead‹), and the pass that runs the line
    /// tells the holder. A comment is a line of the scope too, but none of that is its own: a
    /// comment that leads an expression closes before it, and leaves it all to that line.
    ///
    /// # Safety
    /// `item` must be a line this parser just returned.
    unsafe fn close_line(&mut self, item: DyadPtr) {
        // SAFETY: `item` is a reduced dyad from the store.
        if unsafe { crate::identities::numtype::is_comment_type(dyad::ty(item)) } {
            let scope = self.cx.scopes.current().expect("a line closes inside an open scope");
            // SAFETY: `scope` is the open scope's node, `item` a reduced dyad.
            unsafe {
                crate::identities::scope::push_item(self.rt.store, self.types.array_, scope, item)
            };
            self.cx.open.last_mut().expect("a line closes inside an open scope").lines += 1;
            return;
        }
        // SAFETY: the pending bindings were minted by this parser's declares.
        unsafe { self.cx.scopes.close_item(self.rt.store, self.types.array_, item) };
        // SAFETY: the caller's contract.
        unsafe { self.settle_line(item) };
    }

    /// What the line `item` of the innermost scope noted as it was parsed settles on it, a line
    /// of a scope node or a part of a quote: the names it ended, a `defer`, what it made and runs,
    /// and the values the pass ran early in it.
    ///
    /// # Safety
    /// `item` must be a line this parser just returned, or a part of the quote being read.
    pub(super) unsafe fn settle_line(&mut self, item: DyadPtr) {
        let depth = self.cx.open.len() - 1;
        let line = self.cx.open[depth].lines;
        if let Some(frame) = self.cx.frames.last_mut() {
            for r in frame.returns.iter_mut().filter(|r| r.level == depth && !r.line_closed) {
                r.ended.extend(self.cx.ended_log[r.cursor..].iter().map(|&(name, _)| name));
                r.line_closed = true;
            }
        }
        for name in std::mem::take(&mut self.cx.open[depth].ended) {
            let open = &self.cx.open[depth];
            // SAFETY: held places and `name` are dyads from the store.
            if let Some(at) = open.exit.iter().position(|h| unsafe { self.holds(h, name) }) {
                self.cx.open[depth].exit[at].end = Some(line);
                continue;
            }
            let open = &mut self.cx.open[depth];
            if line > 0 {
                let held = ExitItem { what: name, from: 0, end: Some(line) };
                open.exit.insert(0, held);
            }
            if let Some(unrun) = open.unrun.last_mut().filter(|u| u.line == line) {
                unrun.ends.push(name);
            }
        }
        // SAFETY: `item` is a reduced dyad from the store.
        if unsafe { dyad::ty(item) } == self.types.defer_ {
            let deferred = ExitItem { what: item, from: line + 1, end: None };
            self.cx.open[depth].exit.push(deferred);
        }
        // What the line made and nothing took is the line's to free at its end.
        let open = &mut self.cx.open[depth];
        open.unsettled.clear();
        let made = std::mem::take(&mut open.made);
        let runs = std::mem::take(&mut open.runs);
        match open.unrun.last_mut().filter(|u| u.line == line) {
            Some(unrun) => {
                unrun.plan.extend_from_slice(&made);
                unrun.runs = runs;
                unrun.stood = std::mem::take(&mut open.stood);
            }
            None => open.closed_runs.extend(runs),
        }
        open.held.extend(made);
        self.cx.open[depth].lines += 1;
    }

    /// `value` was just made: the line being parsed holds it to its end unless something takes
    /// it (›Holding is decided at the binding site, parameters included‹ [[R61]]), and what it
    /// wraps and hands on is held through it alone.
    pub(super) fn note_made(&mut self, value: DyadPtr) {
        use crate::identities::drop_model::{handed_through, teardown_of, Teardown};
        if value.is_null() || self.cx.dropping > 0 {
            return;
        }
        let types = self.types;
        // SAFETY: `value` is a reduced dyad from the store.
        let path = unsafe {
            if teardown_of(types, value) == Teardown::Nothing {
                return;
            }
            handed_through(types, value)
        };
        let open = self.cx.open.last_mut().expect("a value is made in an open scope");
        open.made.retain(|m| !path.contains(m));
        open.made.push(path[0]);
    }

    /// What the line being parsed has made so far that nothing took.
    pub(super) fn line_made(&self) -> Vec<DyadPtr> {
        self.cx.open.last().map_or_else(Vec::new, |o| o.made.clone())
    }

    /// What the line being parsed made, that nothing took, since [`Self::line_made`] gave
    /// `before`, taken out of the line's plan: a run the pass makes of it now
    /// ([`Self::run_held`]) is the run that holds it.
    pub(super) fn take_made_since(&mut self, before: &[DyadPtr]) -> Vec<DyadPtr> {
        let Some(open) = self.cx.open.last_mut() else {
            return Vec::new();
        };
        let (since, kept) = open.made.iter().partition(|m| !before.contains(m));
        open.made = kept;
        since
    }

    /// `cell` was just built where the pass runs a line once it has parsed it: a node that runs,
    /// a call or an operator, runs only then. A node that runs nothing holds nothing that runs, so
    /// what the line noted inside it, the cells it took among it, left the line.
    pub(super) fn note_runs(&mut self, cell: &Cell) {
        use crate::identities::read::{read_kind, Read};
        if !self.in_pass_order() {
            return;
        }
        let open = self.cx.open.last_mut().expect("a node is built in an open scope");
        // SAFETY: a constructed cell holds null or a node from the store.
        if matches!(unsafe { read_kind(self.types, cell.dyad) }, Read::Executable(_)) {
            open.runs.push(cell.start);
        } else {
            open.runs.retain(|&s| !holds_offset((cell.start, cell.end()), s));
        }
    }

    /// A constructor took `taken` off the tape into its own cell, which starts at `to`, each with
    /// where the cell after it started: what the line noted as running inside them, from where
    /// each starts to there, runs where that cell stands, or not at all.
    pub(super) fn runs_moved_to(&mut self, taken: &[(Cell, usize)], to: usize) {
        let source = self.cx.source;
        let spans: Vec<(usize, usize)> = taken
            .iter()
            .filter_map(|(c, until)| c.span_in(source).map(|(start, end)| (start, end.max(*until))))
            .collect();
        if spans.is_empty() {
            return;
        }
        let open = self.cx.open.last_mut().expect("a node is built in an open scope");
        for s in open.runs.iter_mut().filter(|s| spans.iter().any(|&span| holds_offset(span, **s)))
        {
            *s = to;
        }
    }

    /// What the line being parsed noted as running from `from` up to `at` runs inside the node
    /// built at `at`, where that node runs it: an `=`'s place, which it reaches after its right
    /// side.
    pub(crate) fn runs_inside(&mut self, from: usize, at: usize) {
        let open = self.cx.open.last_mut().expect("a node is built in an open scope");
        open.runs.retain(|&s| s < from || s >= at);
    }

    /// Where the pass runs each line once it has parsed it: the top level, or a held line
    /// parsed when it runs, outside every deferred body.
    pub(super) fn in_pass_order(&self) -> bool {
        self.cx.feed.as_ref().is_none_or(|f| f.in_run_order) && self.cx.runtime_depth == 0
    }

    /// Whether a part whose value the parse needs to go on reading runs now, in the pass, its
    /// value standing in for it (›The pass runs only as far as it must, in order, and never twice‹
    /// [[R288]]): outside a body it does, and inside one when everything it reads is known while
    /// the program is parsed, each of `reads` a type, a literal or the no value a call through a
    /// type works on; a part that reads a parameter or a local waits for the run. A file read as
    /// Logos reads only its address, written as plain text, so it always runs in the pass and asks
    /// nothing here. `immediate` and a `static` initializer ask for the pass where the parse would
    /// not need it ([`Self::parse_in_pass`]).
    pub(super) fn runs_in_pass(&self, reads: &[DyadPtr]) -> bool {
        self.cx.runtime_depth == 0
            // SAFETY: each read is a reduced dyad from the store.
            || reads.iter().all(|&r| unsafe { crate::identities::is_comptime_arg(self.types, r) })
    }

    /// `parse`, a part the author asks the pass to run once where it is written, an `immediate`
    /// operand or a `static` initializer: every part of it runs in the pass, and a read of a name
    /// that only the run gives a value is refused (`once_at`).
    pub(super) fn parse_in_pass<T>(&mut self, parse: impl FnOnce(&mut Self) -> T) -> T {
        let once_at = self.cx.once_at.replace(self.cx.frames.len());
        let depth = std::mem::replace(&mut self.cx.runtime_depth, 0);
        let parsed = parse(self);
        self.cx.once_at = once_at;
        self.cx.runtime_depth = depth;
        parsed
    }

    /// Something took `value`, a name, a parameter, `free`, `&`, a `return` or the scope it is
    /// the last value of: no line's end frees it, nor anything it hands on.
    pub(crate) fn take(&mut self, value: DyadPtr) {
        // SAFETY: `value` is a reduced dyad from the store.
        let path = unsafe { crate::identities::drop_model::handed_through(self.types, value) };
        for open in &mut self.cx.open {
            open.made.retain(|m| !path.contains(m));
        }
    }

    /// A call's argument list closed: the names it holds for `&` are held from here by the scope
    /// the call stands in, whose end frees them (DESIGN ›Holding is decided at the binding site,
    /// parameters included‹ [[R61]]).
    fn hand_on_address_holds(&mut self, list: &mut OpenScope) {
        let held: Vec<DyadPtr> = list
            .address_holds
            .iter()
            .copied()
            .filter(|&name| list.exit.iter().any(|h| h.what == name && h.end.is_none()))
            .collect();
        list.exit.retain(|h| !held.contains(&h.what));
        for name in held {
            self.hold(name);
            self.note_address_hold(name);
        }
    }

    /// `name`, which `&` landed a value in, is held for `&` by the scope that holds it.
    pub(super) fn note_address_hold(&mut self, name: DyadPtr) {
        if let Some(at) = self.holder_of(name) {
            self.cx.open[at].address_holds.push(name);
        }
    }

    /// The block at `depth` closed: a `return` inside it stands in the enclosing scope's line
    /// from here, which may end names after the block.
    fn close_returns(&mut self, depth: usize) {
        let log_len = self.cx.ended_log.len();
        for r in self.returns_from(0).iter_mut().filter(|r| r.level == depth) {
            r.level = depth - 1;
            r.line_closed = false;
            r.cursor = log_len;
        }
    }

    /// Run everything parsed and not yet run, outermost scope first, and hand back the line
    /// that gave the program its value, with the value, its values kept on the stack until
    /// [`Self::end_tail`] (DESIGN ›The pass runs only as far as it must, in order, and never
    /// twice‹). A block's cursor moves to its last line, so its own run starts after it. The
    /// lists are taken first, so a nested drain finds nothing. A scope's lines count as run as
    /// they go, prose, `defer` and `[…]` lines included. The closed lines of an open argument
    /// list or `[…]`, and the parts of an open quote, are parts of the line being parsed: each
    /// that runs runs too, then standing in its place for the value it gave, which the call that
    /// takes the bracket, or the word that reads the quote, reads. A line that gives its block,
    /// the program or a file its value ends it there: the lines after it never run, nor those of
    /// the scopes open inside it (›A scope's value is what it evaluates to, and `return` is an
    /// optional early exit from the enclosing function‹ [[R207]]).
    pub fn drain(&mut self) -> Result<Option<(DyadPtr, i64)>, ParseError> {
        let lists: Vec<_> = self
            .cx
            .open
            .iter_mut()
            .map(|s| {
                s.closed_runs.clear();
                (s.scope, s.own_lines(), std::mem::take(&mut s.unrun))
            })
            .collect();
        let mut items = Vec::new();
        for (depth, (scope, own, list)) in lists.into_iter().enumerate() {
            if list.is_empty() {
                continue;
            }
            let given = self.cx.open[depth].given_at.is_some();
            if let Some(scope) = scope.filter(|_| own && !given) {
                // SAFETY: `scope` is the block `open_scope` minted; the pending items are the
                // last of its `dyads`, the item being parsed not yet among them.
                let lines =
                    unsafe { crate::identities::scope::exprs_of(scope) }.map_or(0, <[_]>::len);
                self.rt.set_cursor(scope, crate::run::Cursor { at: lines, given: None });
            }
            let part = !own;
            for unrun in list {
                let runs = if own {
                    // A `[…]` line is a list that goes whole to the call that takes it, which
                    // evaluates its lines when it runs (DESIGN ›A bracket goes to the call whole‹).
                    // SAFETY: every pending item is a dyad this parser built into its store.
                    unsafe {
                        let ty = dyad::ty(unrun.node);
                        !crate::identities::numtype::is_comment_type(ty)
                            && ty != self.types.defer_
                            && ty != self.types.square_brackets
                    }
                } else {
                    self.runs_first(&unrun)
                };
                items.push((unrun, runs, depth, part));
            }
        }
        for (unrun, runs, depth, part) in items {
            let UnrunLine { node, line, ends, plan, stood, .. } = unrun;
            if self.cx.open[..=depth].iter().any(|s| s.given_at.is_some()) {
                // SAFETY: each stood node is one `ran_value` built, live until something takes
                // it; the line never runs to take it.
                unsafe { crate::identities::drop_model::end_stood(&mut self.rt, &stood) }
                    .map_err(ParseError::Run)?;
                continue;
            }
            if part || !runs {
                // A part stands in the line its call or word stands in, which has not ended.
                self.cx.open[depth].stood.extend(stood.iter().copied());
            }
            if runs {
                // The holder frees the names the line ends no more, even when the line
                // faults, as a scope's run counts its ending line (`ExitItem::held_at`).
                for name in ends {
                    let at = self.holder_of(name).unwrap_or(0);
                    self.cx.open[at].reached.push(name);
                }
            }
            if runs && part {
                // SAFETY: `node` is line `line` of the bracket or quote open at `depth`.
                unsafe { self.run_part(depth, line, node, &plan) }?;
            } else if runs {
                // SAFETY: `node` is line `line` of the scope open at `depth`, its `plan` and
                // `stood` its own.
                unsafe { self.run_pending_line(depth, line, node, &plan, &stood) }?;
            }
            self.cx.open[depth].ran = line + 1;
        }
        Ok(self.cx.program_value)
    }

    /// A pending line of the scope open at `depth` runs, its scratch going with it. A value it
    /// gives ends the scope there: a block's run gives it, the program shows it, and a file drops
    /// it (›Importing is dropping the text there, wrapped in its own scope‹ [[R306]]). A type
    /// body's lines are no scope's lines, and end as they did.
    ///
    /// # Safety
    /// `node` must be line `line` of the scope open at `depth`, a dyad this parser built into its
    /// store, `plan` the nodes whose values it holds to its end and `stood` the values the pass
    /// ran early that stand in it.
    unsafe fn run_pending_line(
        &mut self,
        depth: usize,
        line: usize,
        node: DyadPtr,
        plan: &[DyadPtr],
        stood: &[DyadPtr],
    ) -> Result<(), ParseError> {
        use crate::identities::drop_model::{end_line, end_line_giving, end_stood};
        let mark = self.rt.stack_mark();
        let held = self.rt.line_mark();
        self.rt.push_plan(plan);
        // SAFETY: the caller's contract.
        let ran = unsafe { self.run_line_on_pass(node) };
        self.rt.pop_plan();
        let open = &self.cx.open[depth];
        let program = depth == 0 && open.top;
        let given = match ran {
            Ok(Some(value)) if open.top || open.scope.is_some() => value,
            ran => {
                // SAFETY: what the line made since `held` its run gave, still live; each stood
                // node is one `ran_value` built, live until something takes it.
                let (ended, left_over) =
                    unsafe { (end_line(&mut self.rt, held, ran), end_stood(&mut self.rt, stood)) };
                ended.and(left_over).map_err(ParseError::Run)?;
                self.rt.stack_release(mark);
                return Ok(());
            }
        };
        self.cx.open[depth].given_at = Some(line);
        if program {
            // The program's value is shown before its line ends.
            self.cx.tail_values = Some(held);
            self.cx.open[depth].stood.extend_from_slice(stood);
            self.cx.program_value = Some((node, given));
            return Ok(());
        }
        let Some(scope) = self.cx.open[depth].scope else {
            // A file's value is dropped with its line.
            // SAFETY: as above.
            let (ended, left_over) =
                unsafe { (end_line(&mut self.rt, held, Ok(())), end_stood(&mut self.rt, stood)) };
            ended.and(left_over).map_err(ParseError::Run)?;
            self.rt.stack_release(mark);
            return Ok(());
        };
        // The block hands the value on when it runs; what the value passes through is its own.
        // SAFETY: `node` is a reduced dyad from the store; the values since `held` are live, and
        // each stood node is one `ran_value` built.
        unsafe {
            let path = crate::identities::drop_model::handed_through(self.types, node);
            let left: Vec<DyadPtr> = stood.iter().copied().filter(|s| !path.contains(s)).collect();
            let ended = end_line_giving(&mut self.rt, held, &path);
            ended.and_then(|()| end_stood(&mut self.rt, &left)).map_err(ParseError::Run)?;
        }
        self.rt.set_cursor(scope, crate::run::Cursor { at: line, given: Some(given) });
        Ok(())
    }

    /// The program's last line ends once its echo has read its value: what it made that nothing
    /// took ends now.
    pub fn end_tail(&mut self) -> Result<(), ParseError> {
        use crate::identities::drop_model::{end_line, end_stood};
        let ended = match self.cx.tail_values.take() {
            // SAFETY: the values since `held` are the last line's, live until now.
            Some(held) => unsafe { end_line(&mut self.rt, held, Ok(0)) }.map(|_| ()),
            None => Ok(()),
        };
        let stood: Vec<DyadPtr> = self.cx.open.iter_mut().flat_map(OpenScope::take_stood).collect();
        // SAFETY: each stood node is one `ran_value` built, its value live until something
        // takes it.
        let left = unsafe { end_stood(&mut self.rt, &stood) };
        ended.and(left).map_err(ParseError::Run)
    }

    /// A fault or a failed parse unwinds the parse of an open scope: what the lines the pass
    /// ran hold and defer ends, and the fault stays the error shown (DESIGN ›A checked error is a fault:
    /// the task that hit it is cancelled‹). The pass knows which lines ran, so the scope that
    /// holds a name alone decides: a name held from the scope's start is an enclosing
    /// scope's, and one a nested line the pass ran ended is not held.
    pub(super) fn end_after_fault(&mut self, open: &OpenScope) {
        let stood: Vec<DyadPtr> =
            open.unrun.iter().flat_map(|u| &u.stood).chain(&open.stood).copied().collect();
        // SAFETY: each stood node is one `ran_value` built, its value live until something
        // takes it. The fault stays the error shown.
        let _ = unsafe { crate::identities::drop_model::end_stood(&mut self.rt, &stood) };
        let ends: Vec<ExitItem> = open
            .exit
            .iter()
            .rev()
            // SAFETY: held places and reached names are dyads from the store.
            .filter(|item| item.from > 0 && unsafe { self.held_after_fault(open, item) })
            .copied()
            .collect();
        self.rt.push_plan(&open.held);
        for item in ends {
            // SAFETY: the pass laid the scope's places out in the frame `self.rt` runs.
            if unsafe { item.run(&mut self.rt) }.is_err() {
                break;
            }
        }
        self.rt.pop_plan();
    }

    /// Whether `item` is still held where `open`'s pass stopped on a fault or a failed parse:
    /// held at that line, and not ended by a nested line the pass ran.
    ///
    /// # Safety
    /// `item`'s place and `open`'s reached names must be dyads from the store.
    unsafe fn held_after_fault(&self, open: &OpenScope, item: &ExitItem) -> bool {
        item.held_at_fault(self.types, open.ran)
            && !open.reached.iter().any(|&n| self.holds(item, n))
    }

    /// `immediate x` (DESIGN ›`immediate x` runs the expression to its right as soon as
    /// it is parsed, and stands as its value‹): the operand is read as a `static` value is,
    /// once at the definition, runs after everything before it, and its value is the cell.
    pub(crate) fn construct_immediate(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let before = self.line_made();
        let operand = self.parse_in_pass(Self::parse_expression);
        // SAFETY: the operand was just parsed into this parser's store.
        let value = unsafe { self.immediate_value(operand?, &before) }?;
        tape.place(value);
        Ok(Constructed::Placed)
    }

    /// What an `immediate` operand stands as: itself when nothing runs, else what the
    /// graph can hold of its result: a number, a bool, a type or a node. Its line holds what else
    /// the run made ([`Self::run_held`]); `before` is [`Self::line_made`] before the operand was
    /// parsed.
    ///
    /// # Safety
    /// `node` must be a reduced dyad this parser built into its store.
    unsafe fn immediate_value(
        &mut self,
        node: DyadPtr,
        before: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        use crate::identities::numtype::NumType;
        use crate::identities::read::{read_kind, Dispatch, Read};
        use crate::identities::{operand_of, Operand};
        let types = self.types;
        let kind = read_kind(types, node);
        let place = types.is_storage(node);
        if !matches!(kind, Read::Executable(_)) && !place {
            return Ok(node);
        }
        if let Read::Executable(Dispatch::Call(f)) = kind {
            if self.returns_type(f) {
                return self.eval_type_call(node);
            }
        }
        let bool_ = crate::identities::read::output_type(types, node) == types.bool_;
        let nt = match operand_of(types, node) {
            Operand::Concrete(nt) => nt,
            Operand::Literal => NumType::I32,
            _ if bool_ => NumType::I32,
            _ => return Err(ParseError::ImmediateNotHeld),
        };
        self.drain()?;
        let plan = self.take_made_since(before);
        let depth = self.cx.open.len() - 1;
        let bits =
            self.run_held(depth, node, &plan, |p| p.run_on_pass(node).map_err(ParseError::Run))?;
        if bool_ {
            let bool_ty = types.bool_;
            return Ok(crate::identities::bool_mod::literal_node(
                self.rt.store,
                bool_ty,
                bits != 0,
            ));
        }
        Ok(self.scalar_value(nt, bits))
    }

    /// The end of the program: the root scope's own run (DESIGN ›The scope's
    /// constructor is the driver‹), yielding the tail it ran, whose line ends at
    /// [`Self::end_tail`].
    pub fn finish(&mut self) -> Result<Option<(DyadPtr, i64)>, ParseError> {
        self.drain()
    }

    /// Whether `node`, a line of the program, gives a value the pass never ran: a `[…]` line,
    /// which goes whole to a call that takes it (›A bracket goes to the call whole‹ [[R228]]).
    ///
    /// # Safety
    /// `node` must be a reduced dyad from the store.
    pub unsafe fn gives_unrun(&self, node: DyadPtr) -> bool {
        dyad::ty(node) == self.types.square_brackets && flow::of(self.types, node).gives
    }

    /// The program's end: what the program holds and defers, handed to the top scope,
    /// runs after every top-level line. The file driver calls this at program end; a
    /// nested scope ran its own at its end.
    pub fn exit(&mut self) -> Result<(), ParseError> {
        let reached = self.cx.open[0].reached_lines();
        let top = self.hand_exit();
        // SAFETY: `top` is the top scope, its places in the program frame `self.rt` runs.
        unsafe { crate::identities::scope::run_exit(&mut self.rt, top, reached) }
            .map_err(ParseError::Run)
    }

    /// The program's end after a fault or a failed parse, as a nested scope's
    /// ([`Self::end_after_fault`]).
    pub fn exit_after_fault(&mut self) {
        let top = std::mem::take(&mut self.cx.open[0]);
        self.end_after_fault(&top);
    }

    /// What the program holds and defers so far joins the top scope's exit, where the
    /// REPL keeps a session's across its lines, and a name this parse ended that an earlier
    /// line's item holds is held no longer; returns the top scope.
    pub fn hand_exit(&mut self) -> DyadPtr {
        use crate::identities::scope;
        self.end_earlier_holds();
        let top = self.cx.scopes.current().expect("the top scope is open");
        // A line that gave the program its value ended it: the lines after never ran, so what
        // they declare never was and what they end is still held.
        let reached = self.cx.open[0].given_at.map(|at| at + 1);
        let items: Vec<DyadPtr> = std::mem::take(&mut self.cx.open[0].exit)
            .into_iter()
            .filter(|h| reached.is_none_or(|r| h.from <= r))
            .map(|h| ExitItem { end: h.end.filter(|&end| reached.is_none_or(|r| end < r)), ..h })
            .map(|h| h.build(self.rt.store, self.types))
            .collect();
        let held = std::mem::take(&mut self.cx.open[0].held);
        // SAFETY: `top` is a scope node the driver minted; `items` exit items just built, `held`
        // the top level's lines' made values.
        unsafe {
            scope::extend_exit(self.rt.store, self.types.array_, top, &items);
            scope::extend_held(self.rt.store, self.types.array_, top, &held);
        }
        top
    }

    /// What the files a failed REPL line imported still hold, and what they defer, leaves the
    /// line's end and joins `session`'s, as a kept line's does: a file loads once per run, and
    /// the run is the session (DESIGN ›Importing is dropping the text there, wrapped in its own
    /// scope‹).
    ///
    /// # Safety
    /// `session` must be the scope node the driver minted for the session.
    pub unsafe fn hand_imported_exit(&mut self, session: DyadPtr) {
        let open = &mut self.cx.open[0];
        let imported = std::mem::take(&mut open.imported);
        let (files, own): (Vec<ExitItem>, Vec<ExitItem>) =
            std::mem::take(&mut open.exit).into_iter().partition(|h| imported.contains(&h.what));
        open.exit = own;
        // A file's holds start at its import, which ran at parse whatever line the pass reached;
        // whether the line's own `free` or `move` of one ended it is read as the fault's end reads
        // the line's own holds.
        let top = &self.cx.open[0];
        let held: Vec<ExitItem> = files
            .into_iter()
            // SAFETY: the files' places and the reached names are dyads from the store.
            .filter(|&item| unsafe { self.held_after_fault(top, &ExitItem { from: 0, ..item }) })
            .map(|item| ExitItem { end: None, ..item })
            .collect();
        let items: Vec<DyadPtr> = held.iter().map(|h| h.build(self.rt.store, self.types)).collect();
        // SAFETY: the caller's contract; `items` are exit items just built.
        unsafe {
            crate::identities::scope::extend_exit(self.rt.store, self.types.array_, session, &items)
        };
    }

    /// The names an earlier REPL line holds that this parse ended: the top scope's end frees
    /// them no more. Returns them.
    pub fn end_earlier_holds(&mut self) -> Vec<DyadPtr> {
        use crate::identities::{drop_model, scope};
        let top = self.cx.scopes.current().expect("the top scope is open");
        let ended = std::mem::take(&mut self.cx.open[0].ended_earlier);
        // SAFETY: `top` is a scope node the driver minted, its exit null or exit items.
        unsafe {
            for &place in &ended {
                self.split_earlier_hold(top, place);
            }
            for &node in scope::exit_items(top) {
                let item = drop_model::exit_item_of(node);
                if ended.iter().any(|&name| self.holds(&item, name)) {
                    drop_model::end_exit_item(self.rt.store, self.types, node);
                }
            }
        }
        ended
    }

    /// The session `top` holds the `{ … }` value whose entry `place` is, held whole by an earlier
    /// REPL line: that hold becomes one hold per entry with an end, where it stood, as a scope
    /// still open splits its own (›`move` and `free` take a field path too‹ [[R75]]). Nothing
    /// for a name, or an entry already held on its own.
    ///
    /// # Safety
    /// `top` must be the session's scope node, its exit null or exit items; `place` a binding or a
    /// `field` place from the store.
    unsafe fn split_earlier_hold(&mut self, top: DyadPtr, place: DyadPtr) {
        use crate::identities::{drop_model, instance, scope};
        let types = self.types;
        if dyad::ty(place) != types.field_ {
            return;
        }
        let held = |p: &Self, place: DyadPtr| {
            scope::exit_items(top)
                .iter()
                // SAFETY: `top`'s exit holds exit item nodes, their places dyads from the store.
                .position(|&node| unsafe { p.holds(&drop_model::exit_item_of(node), place) })
        };
        if held(self, place).is_some() {
            return;
        }
        let record = instance::field_parts(place).0;
        self.split_earlier_hold(top, record);
        let Some(at) = held(self, record) else {
            return;
        };
        let Some(entries) = drop_model::held_entries(self.rt.store, types, record) else {
            return;
        };
        let items = scope::exit_items(top);
        let from = drop_model::exit_item_of(items[at]).from;
        let mut nodes = items[..at].to_vec();
        for what in entries {
            nodes.push(ExitItem { what, from, end: None }.build(self.rt.store, types));
        }
        nodes.extend_from_slice(&items[at + 1..]);
        scope::set_exit(self.rt.store, types.array_, top, &nodes);
    }

    /// [`Self::end_earlier_holds`] for a line that failed: only the names whose ending line the
    /// pass ran, and whose `free` or `move` it reached there (DESIGN ›`free` and `move` end a
    /// name; no drop flag‹).
    pub fn end_failed_earlier_holds(&mut self) -> Vec<DyadPtr> {
        use crate::identities::drop_model::ended_at_run;
        let rt = &self.rt;
        let top = &mut self.cx.open[0];
        let reached = &top.reached;
        // SAFETY: the ended names are places an earlier line laid out in the program frame.
        top.ended_earlier
            .retain(|&name| reached.contains(&name) && unsafe { ended_at_run(rt, name) });
        self.end_earlier_holds()
    }
}

/// Whether the source `start..end` holds `offset`; its start always does, so a cell of no length
/// holds the notes moved to it.
fn holds_offset((start, end): (usize, usize), offset: usize) -> bool {
    offset == start || (start <= offset && offset < end)
}
