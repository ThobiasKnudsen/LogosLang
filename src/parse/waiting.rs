// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A segment whose parse needs a type known only when it runs, parsed where it is written as far
//! as it can be, what it built kept, and from there each time it runs, over the nodes and types
//! such values hold then (`identities::waiting`, ›A `dyad ?` place is transparent: a placeholder
//! for a new node of any type‹ [[R293]], ›A type known only when the program runs still works;
//! what needs it is parsed when its line runs‹ [[R377]]). The parse finds where a type is needed
//! ([`ParseError::WaitsForRun`]); a line that reads a `dyad` name, or a `-> dyad` call as an
//! infix's operand or through `.`, is still found by a scan before it is lexed. A `while`
//! condition that read a `dyad` name is held the same way, parsed again on each pass.

use super::*;
use crate::identities::meta::prec;
use crate::identities::read::{read_kind, Read};
use crate::identities::tape::{NodeMember, NodeRead};
use crate::start_graph::{Carry, In, Out};

/// What the scan of a line found: where the first top-level cell whose build reaches a read
/// starts, what stands before it that the parse cannot stop after, and the first word such a
/// line cannot hold.
#[derive(Default)]
struct Found {
    read_at: Option<usize>,
    /// The first top-level identity built as it is lexed that may read past the cell after it:
    /// where the cells built with it start, and whether it is `=`.
    reader_at: Option<(usize, bool)>,
    /// The first `&`, at any depth: the scope the line stands in holds what it lands.
    address_at: Option<usize>,
    refused: Option<(usize, String)>,
    /// How many reads of what a call that gives a `dyad` gives the scan has met, at any depth.
    call_reads: usize,
    /// How many other points where the parse needs a value the scan has met, at any depth
    /// ([`Parser::needs_value`]).
    needs: usize,
    /// Where each bracket that holds such a read or another point opens, at any depth.
    cuts: Vec<usize>,
    /// At the top, the `:=` of a line that declares one name: what follows it is its value.
    declaration: Option<usize>,
}

/// A line that waits: where it ends, where the part left to the run starts, and whether an `=`
/// stands there, the part before it being the place it writes.
pub(super) struct Waits {
    cut: usize,
    writes: bool,
    end: usize,
}

/// The part of a line parsed before the cut: its cells, and what their parse noted for the line.
#[derive(Default, Clone)]
pub(super) struct Kept {
    cells: Vec<Cell>,
    notes: LineNotes,
}

/// A line held for the run, as [`Parser::hold_line`] left it: every cell of its text, lexed
/// against the scopes open where it is written, the part before `cut` parsed there, what a
/// bare field or a bare call there needs to reach the value its body is about: the type
/// definitions open there and a `run` body's fields, and whether the line is the `T ?` a `:=`
/// declares its name with, which lays out a new node of `T`.
#[derive(Default)]
pub(crate) struct HeldLine {
    cells: ParsingTape,
    cut: usize,
    kept: Kept,
    definitions: Vec<OpenType>,
    run_fields: Option<(DyadPtr, Vec<DyadPtr>, DyadPtr)>,
    lays_out: bool,
}

/// The parse where the outermost constructor on a segment's tape in a body starts
/// ([`Parser::ctor_start`]): the tape's cells, which of them is the constructor's, the depths of
/// what a constructor pushes, and what it sets while it reads.
pub(super) struct CtorStart {
    cells: Vec<Cell>,
    own: usize,
    reach: [usize; 19],
    line: [usize; 11],
    lex_mode: Option<super::driver::RightSide>,
    notes: (usize, usize),
    runs: usize,
    flags: Flags,
}

/// What a constructor sets while it reads and puts back once it is built.
struct Flags {
    discovering: bool,
    reader: DyadPtr,
    member_asleep: bool,
    member_root: DyadPtr,
    member_path: Vec<DyadPtr>,
    cut_before_assign: bool,
    field_hole: bool,
    result_type: bool,
    each_pass: bool,
    each_pass_read: bool,
    pending_binding: DyadPtr,
    member_fn_depth: Option<usize>,
    next_passes_out: bool,
    next_block: super::bodies::BlockValue,
    narrow_next: Option<(CellKey, DyadPtr)>,
    last_write: super::LastWrite,
    last_declared: DyadPtr,
    held: DyadPtr,
    handed_tail: Option<(DyadPtr, Vec<DyadPtr>)>,
    cuts: Vec<usize>,
    cuts_end: usize,
}

impl Carry for Kept {
    fn carry(&self, out: &mut Out) {
        let Kept { cells, notes } = self;
        cells.carry(out);
        notes.carry(out);
    }

    fn uncarry(inp: &mut In) -> Self {
        let cells = Carry::uncarry(inp);
        Kept { cells, notes: Carry::uncarry(inp) }
    }
}

impl Carry for HeldLine {
    fn carry(&self, out: &mut Out) {
        let HeldLine { cells, cut, kept, definitions, run_fields, lays_out } = self;
        cells.carry(out);
        cut.carry(out);
        kept.carry(out);
        definitions.carry(out);
        run_fields.carry(out);
        lays_out.carry(out);
    }

    fn uncarry(inp: &mut In) -> Self {
        let cells = Carry::uncarry(inp);
        let cut = Carry::uncarry(inp);
        let kept = Carry::uncarry(inp);
        let definitions = Carry::uncarry(inp);
        let run_fields = Carry::uncarry(inp);
        HeldLine { cells, cut, kept, definitions, run_fields, lays_out: Carry::uncarry(inp) }
    }
}

impl<'a> Parser<'a> {
    /// The line at the cursor, when it waits for the run: a line of a body whose parse needs a
    /// type known only then, outside a `fn` or `type (…)` written in it, and outside a line of a
    /// bracket that waits on its own. The cursor stays. A line that declares one name does not
    /// wait: its value waits on its own ([`Self::waiting_value`]). A line that waits and holds
    /// another `:=`, a `return`, `free` or `move` is refused here: stand-in for #281.
    pub(super) fn waiting_line_end(&mut self) -> Result<Option<Waits>, ParseError> {
        let open = self.cx.open.last();
        if self.cx.runtime_depth == 0 || open.is_none_or(|o| !o.own_lines()) {
            return Ok(None);
        }
        let start = self.cx.pos;
        let mut found = Found::default();
        let scanned = self.scan_level(0, &mut found);
        let end = self.cx.pos;
        self.cx.pos = start;
        // A line that does not scan is left to the parse to report where it fails.
        let Ok(reads) = scanned else { return Ok(None) };
        if !reads {
            return Ok(None);
        }
        if let Some((at, word)) = found.refused {
            self.cx.pos = at;
            return Err(ParseError::WaitingLineHolds(Box::new(word)));
        }
        let read_at = found.read_at.unwrap_or(start);
        if let Some(at) = found.declaration {
            if at < read_at {
                return Ok(None);
            }
            self.cx.pos = at;
            return Err(ParseError::WaitingLineHolds(Box::new(":=".into())));
        }
        // A reader before the read may read past it, so the parse here stops before the reader.
        let (cut, writes) = match found.reader_at {
            Some((at, writes)) if at < read_at => (at, writes),
            _ => (read_at, false),
        };
        if found.address_at.is_some_and(|at| at < cut) {
            return Ok(Some(Waits { cut: start, writes: false, end }));
        }
        Ok(Some(Waits { cut, writes, end }))
    }

    /// Where the line at the cursor reads what a call that gives a `dyad` gives, or holds another
    /// point where the parse needs a value ([`Self::needs_value`]), inside a bracket, where the
    /// pass does so as it parses it: each bracket that holds such a point, at any depth, before
    /// whose parse what stands before it in its line runs ([`Self::cut_at`]). A line a scan
    /// already covered keeps its cuts. The cursor stays.
    pub(super) fn note_cuts(&mut self) {
        if !self.in_pass_order() || self.cx.pos < self.cx.cuts_end {
            return;
        }
        let start = self.cx.pos;
        let mut found = Found::default();
        let scanned = self.scan_level(0, &mut found);
        self.cx.cuts_end = self.cx.pos.max(start + 1);
        self.cx.pos = start;
        // A line that does not scan is left to the parse to report where it fails.
        self.cx.cuts = if scanned.is_ok() { found.cuts } else { Vec::new() };
    }

    /// Before the bracket at `at` is parsed into `tape`, where the line's scan found it holds a
    /// point where the parse needs a value: what stands before it runs, in order (›The
    /// pass runs only as far as it must, in order, and never twice‹ [[R288]]). Where something
    /// before it cannot run first, nothing does, and the read refuses.
    pub(super) fn cut_at(&mut self, tape: &mut ParsingTape, at: usize) -> Result<(), ParseError> {
        if self.cx.each_pass || !self.in_pass_order() {
            return Ok(());
        }
        let Some(k) = self.cx.cuts.iter().position(|&c| c == at) else { return Ok(()) };
        self.cx.cuts.swap_remove(k);
        self.run_before(tape, at).map(|_| ())
    }

    /// The `while` condition at the cursor, where it reads what a call that gives a `dyad` gives:
    /// where it ends, before its body's `(`. Such a condition is held before it is parsed, so the
    /// call runs only when the condition does (›The pass runs only as far as it must, in order,
    /// and never twice‹ [[R288]]). The scan sees a call written bare, inside a bracket too, not
    /// one read through a bracket around it or a member. The cursor stays.
    pub(super) fn condition_reads_call(&mut self) -> Option<usize> {
        let start = self.cx.pos;
        let end = self.scan_condition();
        self.cx.pos = start;
        end.ok().flatten()
    }

    /// [`Self::condition_reads_call`], the cursor left where the scan stopped.
    fn scan_condition(&mut self) -> Result<Option<usize>, ParseError> {
        let types = self.types;
        let mut found = Found::default();
        let mut prev: Option<DyadPtr> = None;
        let (mut callee, mut after_call): (Option<bool>, bool) = (None, false);
        loop {
            self.skip_whitespace();
            let at = self.cx.pos;
            if at >= self.cx.source.len() {
                return Ok(None);
            }
            let r = self.token_at(at, true)?;
            let id = r.identity;
            if id == types.sep_ || self.is_closer(id) || id == types.else_ {
                return Ok(None);
            }
            self.cx.pos = at + r.matched;
            if self.is_opener(id) {
                if id == types.open_ && self.opens_body(prev) {
                    return Ok((found.call_reads > 0).then_some(at));
                }
                self.scan_level(1, &mut found)?;
                let call_operand = callee.take();
                after_call = call_operand == Some(false);
                if call_operand == Some(true) {
                    found.call_reads += 1;
                }
                prev = Some(types.open_);
                continue;
            }
            if std::mem::take(&mut after_call) && (self.is_value_infix(id) || id == types.dot_) {
                found.call_reads += 1;
            }
            // SAFETY: a resolved token's identity is a dyad from the store.
            callee = (prev != Some(types.dot_) && unsafe { self.gives_dyad(id) })
                .then(|| prev.is_some_and(|p| self.is_value_infix(p)));
            prev = Some(id);
        }
    }

    /// Whether a `(` after `prev` in a condition opens its body: not where the condition is still
    /// owed a value, after an operator or at its start, nor after a function, whose call it is.
    fn opens_body(&self, prev: Option<DyadPtr>) -> bool {
        let Some(prev) = prev else { return false };
        if prev == self.types.open_ {
            return true;
        }
        // SAFETY: a resolved token's identity is a dyad from the store.
        let function = unsafe { !prev.is_null() && crate::dyad::ty(prev) == self.types.fn_type };
        let rank = self.precedence_of_cell(prev);
        let operator =
            self.ctor_of(prev).is_some() && (rank < prec::OPEN || prec::waits_for_reads(rank));
        !function && !operator
    }

    /// One level of the line: at the top the line itself, up to its boundary, left unconsumed;
    /// below it the lines of a bracket, up to its closer, consumed. Whether a line of this level
    /// reads a value whose type is known only at run for the line around it: an assignment gives
    /// nothing (›`=` sits beside `:=`, and returns nothing‹ [[R277]]), so one in a bracket waits
    /// on its own, when the parse reaches it. A `:=` on a line that reads is refused, and at the
    /// top a `:=` at all, but the one that declares the line's name.
    fn scan_level(&mut self, depth: usize, found: &mut Found) -> Result<bool, ParseError> {
        self.check_stack()?;
        let types = self.types;
        let top = depth == 0;
        let bare_branch = top && self.cx.else_ends == Some(self.cx.open.len());
        let (mut any, mut line_reads, mut assigns) = (false, false, false);
        let mut declares: Option<usize> = None;
        let (mut prev, mut before_prev): (Option<DyadPtr>, Option<DyadPtr>) = (None, None);
        // The type of the place the name or field path scanned last ends at, and the two tokens
        // before that path.
        let mut reached: Option<DyadPtr> = None;
        let mut path_start: (Option<DyadPtr>, Option<DyadPtr>) = (None, None);
        // After a `fn` or `type`, not a member's name, the rest of the level is a body read where
        // it is built.
        let mut skipping = false;
        // A `-> dyad` function just scanned, with whether an infix stands before it; and whether
        // the bracket just closed was its call's.
        let (mut callee, mut after_call): (Option<bool>, bool) = (None, false);
        // A member just scanned whose call is a call for a type.
        let mut type_member = false;
        // A read of a tape node just scanned, `tape[k]` or `tape[k].dyads[i]`, or the `.type` of
        // one; and the `.dyads` of one. Each with where its cells start.
        let mut node_read: Option<(NodeRead, usize)> = None;
        let mut lines: Option<usize> = None;
        // At the top, the tokens before the first `:=` that are no gate word.
        let mut lead = 0;
        // At the top: where the cells built together with the token just scanned start, and
        // where those of the last call that gives a `dyad` start.
        let (mut chain, mut call_chain) = (0, 0);
        loop {
            self.skip_whitespace();
            let at = self.cx.pos;
            let ends_here = at >= self.cx.source.len();
            let r = if ends_here { None } else { Some(self.token_at(at, true)?) };
            let id = r.as_ref().map(|r| r.identity);
            let boundary = ends_here
                || id.is_some_and(|id| {
                    id == types.sep_ || self.is_closer(id) || (bare_branch && id == types.else_)
                });
            if boundary {
                if line_reads || top {
                    if let Some(at) = declares.take() {
                        found.refused.get_or_insert((at, ":=".into()));
                    }
                }
                any |= line_reads && (top || !assigns);
                if top {
                    return Ok(any);
                }
                let (Some(r), Some(id)) = (r, id) else { return Err(ParseError::UnclosedBracket) };
                self.cx.pos = at + r.matched;
                if self.is_closer(id) {
                    return Ok(any);
                }
                (line_reads, assigns, declares, prev, before_prev, skipping) =
                    (false, false, None, None, None, false);
                (callee, after_call, type_member) = (None, false, false);
                (reached, path_start) = (None, (None, None));
                (node_read, lines) = (None, None);
                continue;
            }
            let (Some(r), Some(id)) = (r, id) else { unreachable!("a boundary at the end") };
            self.cx.pos = at + r.matched;
            let leads = id != types.declare_tok && !self.is_gate_word(id);
            if top && leads && found.declaration.is_none() && declares.is_none() {
                lead += 1;
            }
            let member_call = std::mem::take(&mut type_member);
            if top {
                if !self.joins_chain(prev, id) {
                    chain = at;
                }
                if self.reads_past_next(id) {
                    found.reader_at.get_or_insert((chain, id == types.assign));
                }
            }
            let (after_node, after_lines) = (node_read.take(), lines.take());
            if self.is_opener(id) {
                if member_call && id == types.open_ {
                    found.needs += 1;
                }
                let on_tape = reached == Some(types.tape.parsing_tape);
                // Where the cells of a tape read this bracket indexes start.
                let indexes = match (id == types.open_sq_ && !skipping, on_tape) {
                    (false, _) => None,
                    (true, true) => Some(chain),
                    (true, false) => after_lines,
                };
                let own_tape = on_tape && prev.is_some_and(|tape| self.is_own_tape(tape));
                let reads_before = found.call_reads + found.needs;
                let inner = if skipping {
                    self.skip_bracket(id).map(|_| false)?
                } else {
                    self.scan_level(depth + 1, found)?
                };
                if found.call_reads + found.needs > reads_before {
                    found.cuts.push(at);
                }
                let call_operand = callee.take();
                after_call = call_operand == Some(false);
                if after_call {
                    call_chain = chain;
                }
                if call_operand == Some(true) {
                    found.call_reads += 1;
                }
                if inner || call_operand == Some(true) {
                    line_reads = true;
                    if top {
                        found.read_at.get_or_insert(chain);
                    }
                }
                (before_prev, prev) = (prev, Some(types.open_));
                reached = None;
                if let Some(from) = indexes {
                    let index = &self.cx.source[at + r.matched..self.cx.pos];
                    let own = own_tape && Self::is_index_zero(index);
                    node_read = Some((NodeRead::Node { own }, from));
                }
                continue;
            }
            if skipping {
                continue;
            }
            // A member of a tape node that the read does not answer where it is written is read
            // from the node the tape holds when the line runs.
            let spelling = &self.cx.source[at..at + r.matched];
            if id == types.dot_ {
                (node_read, lines) = (after_node, after_lines);
            } else if let Some((read, from)) = after_node.filter(|_| prev == Some(types.dot_)) {
                match crate::identities::tape::node_member(read, spelling) {
                    NodeMember::Type => node_read = Some((NodeRead::Type, from)),
                    NodeMember::Lines => lines = Some(from),
                    NodeMember::Held => {
                        line_reads = true;
                        if top {
                            found.read_at.get_or_insert(from);
                        }
                    }
                    NodeMember::Own => {}
                }
            }
            if prev != Some(types.dot_) && id != types.dot_ {
                path_start = (prev, before_prev);
            }
            // SAFETY: a resolved token's identity is a dyad from the store.
            reached = unsafe {
                self.place_reached(prev, reached, id, &self.cx.source[at..at + r.matched])
            };
            if (id == types.fn_type || id == types.type_) && prev != Some(types.dot_) {
                skipping = true;
                continue;
            }
            // `logos file «…»` reads the file as Logos where it is written.
            if id == types.logos
                && prev != Some(types.dot_)
                && self.next_identity() == Some(types.file)
            {
                found.needs += 1;
                (before_prev, prev) = (prev, Some(id));
                continue;
            }
            if self.ranked(id, prec::ADDRESS) {
                found.address_at.get_or_insert(at);
            }
            // A call that gives a `dyad`, read by an infix on either side or by a `.` after it.
            if std::mem::take(&mut after_call) && (self.is_value_infix(id) || id == types.dot_) {
                line_reads = true;
                found.call_reads += 1;
                if top {
                    found.read_at.get_or_insert(call_chain);
                }
            }
            // SAFETY: a resolved token's identity is a dyad from the store.
            callee = (prev != Some(types.dot_) && unsafe { self.gives_dyad(id) })
                .then(|| prev.is_some_and(|p| self.is_value_infix(p)));
            if id == types.return_ || id == types.free_ || id == types.move_ {
                let word = self.cx.source[at..at + r.matched].to_string();
                found.refused.get_or_insert((at, word));
            }
            if id == types.declare_tok {
                if top && lead == 1 && found.declaration.is_none() && declares.is_none() {
                    found.declaration = Some(at);
                } else {
                    declares.get_or_insert(at);
                }
            }
            // SAFETY: a resolved token's identity is a dyad from the store, and so is the one
            // before the `.`.
            unsafe {
                if prev != Some(types.dot_) && self.needs_value(id) {
                    found.needs += 1;
                }
                type_member = prev == Some(types.dot_)
                    && before_prev.is_some_and(|owner| {
                        self.member_gives_type(owner, &self.cx.source[at..at + r.matched])
                    });
            }
            assigns |= id == types.assign;
            let dyad_read = id != types.dot_ && reached == Some(types.dyad_);
            let (before_path, target) = path_start;
            // SAFETY: a resolved token's identity is a dyad from the store.
            let target_holds_dyad = target
                .is_some_and(|t| unsafe { read_kind(types, t) == Read::Container(types.dyad_) });
            if dyad_read && !self.reads_the_box(before_path, target_holds_dyad, top, bare_branch) {
                line_reads = true;
                if top {
                    found.read_at.get_or_insert(chain);
                }
            }
            (before_prev, prev) = (prev, Some(id));
        }
    }

    /// Whether the parse needs a value where `id` stands, outside a member read: a box read, a
    /// call for a type, a constructor written in Logos and an import (›The pass runs only as far
    /// as it must, in order, and never twice‹ [[R288]]).
    ///
    /// # Safety
    /// `id` must be null or a resolved dyad from the store.
    unsafe fn needs_value(&self, id: DyadPtr) -> bool {
        self.is_box(id)
            || self.returns_type(id)
            || self.is_logos_ctor(id)
            || id == self.types.import_
    }

    /// Whether `id` names the tape of the parse body being read, whose cell 0 holds the node the
    /// parse builds.
    fn is_own_tape(&self, id: DyadPtr) -> bool {
        let Some(def) = self.cx.definitions.last().filter(|d| d.in_parse) else { return false };
        // SAFETY: a resolved token's identity is null or a dyad from the store.
        !id.is_null() && unsafe { self.types.through(id) } == def.tape_param
    }

    /// Whether `index`, the text of a `[…]` from after its opener through its closer, is the
    /// literal `0`. An index 0 written any other way reads as another cell's here, so its line
    /// waits, and the parse at run then reads the parse's own cell as it does where written.
    fn is_index_zero(index: &str) -> bool {
        index.strip_suffix(']').is_some_and(|inner| inner.trim() == "0")
    }

    /// Whether `id`, after a type, reads that type: a value it takes, a `?`, or a `.` before a
    /// field, never an operator, a `=` or a `:=` (›A type known only when the program runs still
    /// works; what needs it is parsed when its line runs‹ [[R377]]).
    fn uses_type(&self, id: DyadPtr) -> bool {
        id == self.types.dot_
            || id == self.types.unknown
            || self.is_opener(id)
            || self.ctor_of(id).is_none()
            || self.ranked(id, prec::LITERAL)
    }

    /// Whether `id` has a constructor at parse_rank `rank`.
    fn ranked(&self, id: DyadPtr, rank: f64) -> bool {
        self.ctor_of(id).is_some() && self.precedence_of_cell(id) == rank
    }

    /// Whether `id`, after `prev` at the top of a line, is built with the cells before it: a
    /// tight read with the cell it reads, a bracket with the value it follows.
    fn joins_chain(&self, prev: Option<DyadPtr>, id: DyadPtr) -> bool {
        let Some(prev) = prev else { return false };
        let tight = |id| self.ranked(id, prec::TIGHT);
        tight(id) || tight(prev) || (self.is_opener(id) && !self.ends_chain(prev))
    }

    /// An identity no bracket after it belongs to: a delimiter, an operator built at the
    /// boundary, or one that reads past the next cell.
    fn ends_chain(&self, id: DyadPtr) -> bool {
        self.is_delimiter(id)
            || self.reads_past_next(id)
            || (self.ctor_of(id).is_some() && self.precedence_of_cell(id) < prec::OPEN)
    }

    /// An identity built as it is lexed that may read past the cell after it: a declaration, a
    /// reader, a type's own constructor. A tight read and a call read the one cell after them.
    fn reads_past_next(&self, id: DyadPtr) -> bool {
        let rank = self.precedence_of_cell(id);
        // SAFETY: a resolved token's identity is a dyad from the store.
        let call = unsafe { !id.is_null() && crate::dyad::ty(id) == self.types.fn_type };
        self.ctor_of(id).is_some()
            && rank >= prec::OPEN
            && !prec::built_as_lexed(rank)
            && rank != prec::TIGHT
            && !call
    }

    /// The type of the place the token `id`, spelled `spelling`, after `prev`, ends a name or a
    /// field path at: a name's own, or after a `.` the field's of the record the path reached so
    /// far, `reached`, so an entry of a `{ … }` reads as a name does (›`{ … }` outside text is a
    /// scope kept as a value, read with `.`‹ [[R374]]). A `.` keeps what the path reached;
    /// `None` for any other token.
    ///
    /// # Safety
    /// `id` must be a resolved dyad from the store, and `reached`, where it is one, a type node.
    unsafe fn place_reached(
        &self,
        prev: Option<DyadPtr>,
        reached: Option<DyadPtr>,
        id: DyadPtr,
        spelling: &str,
    ) -> Option<DyadPtr> {
        let types = self.types;
        if id == types.dot_ {
            return reached;
        }
        if prev == Some(types.dot_) {
            return self.field_type(reached?, spelling);
        }
        if read_kind(types, id) == Read::Container(types.dyad_) {
            return Some(types.dyad_);
        }
        types.storage_type(id)
    }

    /// Whether the `dyad` name or field path just scanned, after `prev`, stands for its place
    /// rather than the node it holds: what `=` writes, `&` points at, `free` and `move` end, and
    /// a value taken whole by a `:=`, a `return`, an `=` into another `dyad` place or the line
    /// itself.
    fn reads_the_box(
        &mut self,
        prev: Option<DyadPtr>,
        target_holds_dyad: bool,
        top: bool,
        bare_branch: bool,
    ) -> bool {
        let types = self.types;
        let next = self.peek_token().map(|(id, _)| id);
        if self.takes_the_place(prev, next) {
            return true;
        }
        let at_end = self.cx.pos >= self.cx.source.len()
            || next.is_some_and(|n| {
                n == types.sep_ || self.is_closer(n) || (bare_branch && n == types.else_)
            });
        at_end
            && match prev {
                None => top,
                Some(p) => {
                    p == types.declare_tok
                        || p == types.return_
                        || (p == types.assign && target_holds_dyad)
                }
            }
    }

    /// Whether `id` is a function whose result is a `dyad`, so the type of what its call gives
    /// is known only at run.
    ///
    /// # Safety
    /// `id` must be null or a dyad from the store.
    unsafe fn gives_dyad(&self, id: DyadPtr) -> bool {
        !id.is_null()
            && crate::dyad::ty(id) == self.types.fn_type
            && *(crate::dyad::value(id) as *const DyadPtr).add(FN_OUTPUT) == self.types.dyad_
    }

    /// Whether a part of a segment parsed here can wait for the run: inside a body, where the
    /// program's run reaches the line later.
    pub(super) fn waits_here(&self) -> bool {
        self.cx.runtime_depth > 0
    }

    /// Whether `cell` gives a type the pass does not know: a node whose output is a type and
    /// that is none, or a `type ?` box, which a body reads when it runs.
    pub(super) fn gives_type_at_run(&self, cell: &Cell) -> bool {
        let types = self.types;
        let d = cell.dyad;
        if d.is_null() || cell.type_value {
            return false;
        }
        // SAFETY: a cell holds null or a resolved dyad from the store.
        unsafe {
            if read_kind(types, d) == Read::Container(types.type_) {
                return true;
            }
            cell.constructed && self.runs_for_type(d)
        }
    }

    /// Whether `node`, built, gives a type only by running.
    ///
    /// # Safety
    /// `node` must be a reduced dyad from the store.
    pub(super) unsafe fn runs_for_type(&self, node: DyadPtr) -> bool {
        let types = self.types;
        !types.is_storage(node)
            && !types.is_hole(types.through(node))
            && crate::identities::read::output_type(types, node) == types.type_
            && crate::identities::type_identity_of(types, node).is_none()
    }

    /// Whether `next`, after a type known only at run, uses it, so that what follows needs it
    /// while it is parsed (›A type known only when the program runs still works; what needs it
    /// is parsed when its line runs‹ [[R377]]).
    fn used_after(&self, prev: Option<DyadPtr>, next: Option<DyadPtr>) -> bool {
        next.is_some_and(|n| {
            self.boundary_of(n).is_none() && !self.is_closer(n) && self.uses_type(n)
        }) && !self.takes_the_place(prev, next)
    }

    /// A segment at its end, a built node that gives a type known only at run standing before a
    /// cell that uses it, which that type would build (`T v`) at a parse_rank known only then, so
    /// the order the segment's cells are built in is too: the segment waits for the run, every
    /// cell kept as built, and is built on from them when it runs ([`Self::hold_segment`]).
    /// `false` where nothing waits.
    pub(super) fn waits_at_end(&mut self, tape: &mut ParsingTape) -> Result<bool, ParseError> {
        if !self.waits_here() || tape.segment.running != 0 || !tape.segment.plain {
            return Ok(false);
        }
        let Some(end) = tape.segment.ends_at.filter(|_| tape.sealed) else { return Ok(false) };
        let cells: Vec<Cell> = tape.iter().map(|(_, c)| *c).collect();
        let ids: Vec<DyadPtr> = cells.iter().map(|c| self.cell_identity(c)).collect();
        let waits = (1..cells.len()).any(|k| {
            let prev = k.checked_sub(2).map(|p| ids[p]);
            self.gives_type_at_run(&cells[k - 1]) && self.used_after(prev, Some(ids[k]))
        });
        if !waits {
            return Ok(false);
        }
        self.hold_segment(tape, cells, end, None)?;
        Ok(true)
    }

    /// `cell`, a name about to be pushed on `tape`, where it holds a type known only at run and
    /// what follows it uses that type: the segment waits from the name, which is read when the
    /// line runs. Inside a constructor's read, that constructor's segment waits instead
    /// ([`ParseError::WaitsForRun`]). `false` where nothing waits.
    pub(super) fn waits_for_named_type(
        &mut self,
        tape: &mut ParsingTape,
        cell: &Cell,
    ) -> Result<bool, ParseError> {
        if !self.waits_here() || self.after_tight_read(tape) || !self.gives_type_at_run(cell) {
            return Ok(false);
        }
        let prev = tape.last().map(|c| self.cell_identity(c));
        // A read that stops before its caller's bracket ends at a `(`, which is no use of the name.
        let next =
            self.next_identity().filter(|&n| self.cx.lex_mode.is_none() || !self.is_opener(n));
        if !self.used_after(prev, next) {
            return Ok(false);
        }
        if tape.segment.running != 0 || !tape.segment.plain {
            self.cx.pos = cell.start;
            return Err(ParseError::WaitsForRun(None));
        }
        let kept = tape.iter().map(|(_, c)| *c).collect();
        self.hold_segment(tape, kept, cell.start, None)?;
        Ok(true)
    }

    /// The parse where the outermost constructor on `tape`, at its center, starts, the brackets it
    /// reads before it built, where its segment may wait for the run: in a body, on a segment's
    /// tape.
    pub(super) fn ctor_start(&self, tape: &ParsingTape) -> Option<CtorStart> {
        if !self.waits_here() || !tape.segment.plain {
            return None;
        }
        let own = tape.mark()?;
        let cells: Vec<(usize, Cell)> = tape.iter().map(|(n, c)| (n, *c)).collect();
        let own = cells.iter().position(|&(n, _)| n == own)?;
        let cx = &self.cx;
        let open = cx.open.last()?;
        Some(CtorStart {
            cells: cells.into_iter().map(|(_, c)| c).collect(),
            own,
            reach: self.ctor_reach(),
            line: open.line_reach(),
            lex_mode: cx.lex_mode,
            notes: open.line_notes_mark(),
            runs: open.runs.len(),
            flags: Flags {
                discovering: cx.discovering,
                reader: cx.reader,
                member_asleep: cx.member_asleep,
                member_root: cx.member_root,
                member_path: cx.member_path.clone(),
                cut_before_assign: cx.cut_before_assign,
                field_hole: cx.field_hole,
                result_type: cx.result_type,
                each_pass: cx.each_pass,
                each_pass_read: cx.each_pass_read,
                pending_binding: cx.pending_binding,
                member_fn_depth: cx.member_fn_depth,
                next_passes_out: cx.next_passes_out,
                next_block: cx.next_block,
                narrow_next: cx.narrow_next,
                last_write: cx.last_write,
                last_declared: cx.last_declared,
                held: cx.held,
                handed_tail: cx.handed_tail.clone(),
                cuts: cx.cuts.clone(),
                cuts_end: cx.cuts_end,
            },
        })
    }

    /// The depths of every stack a constructor pushes onto while it reads, and of the names it
    /// declares and ends.
    fn ctor_reach(&self) -> [usize; 19] {
        let cx = &self.cx;
        let at = |o: Option<usize>| o.map_or(0, |d| d + 1);
        [
            cx.open.len(),
            cx.frames.len(),
            cx.definitions.len(),
            cx.filling.len(),
            cx.filling_at.len(),
            cx.deferring.len(),
            cx.scopes.depth(),
            cx.scopes.ends_mark(),
            cx.runtime_depth as usize,
            usize::from(cx.feed.is_some()),
            usize::from(cx.kept.is_some()),
            usize::from(cx.run_fields.is_some()),
            at(cx.else_ends),
            at(cx.once_at),
            cx.held_depth,
            cx.dropping,
            cx.ended_log.len(),
            cx.queued.len(),
            cx.held_back.len(),
        ]
    }

    /// The parse put back as `before` found it, where the constructor that failed left every
    /// stack and name as it found them: `false` where it did not, and the failure stands.
    fn back_to(&mut self, before: &mut CtorStart) -> bool {
        let same = self.ctor_reach() == before.reach
            && self.cx.lex_mode == before.lex_mode
            && self.cx.open.last().is_some_and(|o| o.line_reach() == before.line);
        if !same {
            return false;
        }
        let open = self.cx.open.last_mut().expect("compared above");
        open.line_notes_since(before.notes);
        open.runs.truncate(before.runs);
        let f = &mut before.flags;
        let cx = &mut self.cx;
        cx.discovering = f.discovering;
        cx.reader = f.reader;
        cx.member_asleep = f.member_asleep;
        cx.member_root = f.member_root;
        cx.member_path = std::mem::take(&mut f.member_path);
        cx.cut_before_assign = f.cut_before_assign;
        cx.field_hole = f.field_hole;
        cx.result_type = f.result_type;
        cx.each_pass = f.each_pass;
        cx.each_pass_read = f.each_pass_read;
        cx.pending_binding = f.pending_binding;
        cx.member_fn_depth = f.member_fn_depth;
        cx.next_passes_out = f.next_passes_out;
        cx.next_block = f.next_block;
        cx.narrow_next = f.narrow_next;
        cx.last_write = f.last_write;
        cx.last_declared = f.last_declared;
        cx.held = f.held;
        cx.handed_tail = f.handed_tail.take();
        cx.cuts = std::mem::take(&mut f.cuts);
        cx.cuts_end = f.cuts_end;
        true
    }

    /// The constructor that `before` saw start on `tape` found that its segment waits for the run
    /// ([`ParseError::WaitsForRun`]): the segment is held from the tape as it stood then, from
    /// where that constructor's cell starts, keeping what stood before it, or, where it was built
    /// at the segment's end with nothing more to lex, from that end, keeping every cell. `place` is
    /// the place an `=` there writes. Where the constructor left the parse otherwise than it
    /// found it, the failure stands.
    pub(super) fn wait_from(
        &mut self,
        tape: &mut ParsingTape,
        mut before: CtorStart,
        discovery: bool,
        failed: ParseError,
    ) -> Result<(), ParseError> {
        let ParseError::WaitsForRun(place) = failed else { return Err(failed) };
        if !self.back_to(&mut before) {
            return Err(ParseError::WaitsForRun(place));
        }
        let own = before.cells[before.own];
        let (kept, cut) = match tape.segment.ends_at {
            Some(end) if !discovery && tape.sealed => (before.cells, end),
            _ => (before.cells[..before.own].to_vec(), own.start),
        };
        self.hold_segment(tape, kept, cut, place.map(|p| *p))
    }

    /// The segment on `tape`, held for the run from `cut` (›A `dyad ?` place is transparent: a
    /// placeholder for a new node of any type‹ [[R293]], its Q-61 line): `kept` is what it lexed
    /// and built before the cut, which runs when it runs, and the rest of its text, to its
    /// boundary, is parsed then. What its line noted since the segment began goes with it. A
    /// segment that is a whole line of its scope gives what that line gives; any other gives a
    /// `dyad`, as a `-> dyad` call does. The tape is left holding the held node alone.
    fn hold_segment(
        &mut self,
        tape: &mut ParsingTape,
        kept: Vec<Cell>,
        cut: usize,
        place: Option<Cell>,
    ) -> Result<(), ParseError> {
        let types = self.types;
        let (from, mark) = tape.segment.from.expect("a segment notes where it began");
        let (boundary, hole) = self.segment_end(from)?;
        let end = from + self.cx.source[from..boundary].trim_end().len();
        self.cx.pos = cut;
        self.refuse_held_words(end, 0)?;
        let open = self.cx.open.last_mut().expect("a segment is read in an open scope");
        let notes = open.line_notes_since(mark);
        open.runs.retain(|&s| !(from..end).contains(&s));
        let (mut start, mut cut, mut kept, mut gates) = (from, cut, kept, std::ptr::null_mut());
        let value = !tape.segment.line;
        if value {
            // The gate words that open a value gate the place it fills, where it is written.
            self.cx.pos = from;
            let (words, after) = self.gates_ahead();
            for gate in words {
                // SAFETY: `gates` is null or a gate mint, and `gate` a gate word.
                let joined =
                    unsafe { crate::identities::gate::joined(self.rt.store, types, gates, gate) };
                gates = joined.ok_or_else(|| self.fail_at(from, ParseError::DoubleGate))?;
            }
            kept.retain(|c| c.start >= after);
            (start, cut) = (after, cut.max(after));
        }
        // What was lexed and not built is built when the line runs: a name in it is read here as
        // a use of it, and it holds no word the run cannot check where it is written (stand-in
        // for #281).
        for cell in kept.iter().filter(|c| !c.constructed) {
            if cell.is_fresh(types) {
                self.as_operand(*cell)?;
            }
            self.cx.pos = cell.start;
            self.refuse_held_words(cell.end(), 0)?;
        }
        let kept = Kept { cells: kept, notes };
        let lays_out = value && hole;
        let mut held = self.hold_text(start, end, cut, kept, place, lays_out)?;
        if value {
            // SAFETY: `held` holds the line `hold_text` just built, which has not run.
            unsafe { crate::identities::waiting::set_into(types, held.dyad, types.dyad_) };
        }
        (held.start, held.len, held.gates) = (from, end - from, gates);
        tape.hold_whole(held);
        tape.segment.held_to = Some(boundary);
        self.cx.pos = boundary;
        Ok(())
    }

    /// Where the segment that starts at `from` ends: at its `,`, the closer that
    /// ends it, the `else` that ends a bare branch, or the end of the text, brackets read whole;
    /// with whether its last word is `?`, so it makes a place with no value (›`?` is one value,
    /// the unknown; `key := T ?` declares without a value‹ [[R103]]).
    fn segment_end(&mut self, from: usize) -> Result<(usize, bool), ParseError> {
        let pos = std::mem::replace(&mut self.cx.pos, from);
        let mut hole = false;
        let end = loop {
            self.skip_whitespace();
            let at = self.cx.pos;
            if at >= self.cx.source.len() {
                break Ok((at, hole));
            }
            let r = match self.token_at(at, true) {
                Ok(r) => r,
                Err(e) => break Err(e),
            };
            if self.boundary_of(r.identity).is_some() {
                break Ok((at, hole));
            }
            hole = r.identity == self.types.unknown;
            self.cx.pos = at + r.matched;
            if self.is_opener(r.identity) {
                if let Err(e) = self.skip_bracket(r.identity) {
                    break Err(e);
                }
            }
        };
        self.cx.pos = pos;
        end
    }

    /// The part of a segment left to the run, from the cursor to `end` at the top (`depth` 0) or
    /// to its closer inside a bracket, refused where it declares a name, returns, or ends a
    /// name, which a part parsed only when it runs cannot do yet: stand-in for #281. A `fn` or
    /// `type` body is read where it is built, so the rest of its level is not looked at.
    fn refuse_held_words(&mut self, end: usize, depth: usize) -> Result<(), ParseError> {
        self.check_stack()?;
        let types = self.types;
        let (mut prev, mut skipping): (Option<DyadPtr>, bool) = (None, false);
        loop {
            self.skip_whitespace();
            let at = self.cx.pos;
            if (depth == 0 && at >= end) || at >= self.cx.source.len() {
                return Ok(());
            }
            let r = self.token_at(at, true)?;
            let id = r.identity;
            self.cx.pos = at + r.matched;
            if depth > 0 && self.is_closer(id) {
                return Ok(());
            }
            if id == types.sep_ {
                (prev, skipping) = (None, false);
                continue;
            }
            if self.is_opener(id) {
                if skipping {
                    self.skip_bracket(id)?;
                } else {
                    self.refuse_held_words(end, depth + 1)?;
                }
                prev = Some(types.open_);
                continue;
            }
            if prev != Some(types.dot_) {
                if id == types.fn_type || id == types.type_ {
                    skipping = true;
                }
                let refused = [types.return_, types.free_, types.move_, types.declare_tok];
                if !skipping && refused.contains(&id) {
                    self.cx.pos = at;
                    let word = self.cx.source[at..at + r.matched].to_string();
                    return Err(ParseError::WaitingLineHolds(Box::new(word)));
                }
            }
            prev = Some(id);
        }
    }

    /// The line at the cursor, cut where `waits` says: the part before the cut parsed here, and
    /// the line held as it is written, lexed against the scopes open here, its names checked as a
    /// use of them is, and parsed from the cut each time it runs.
    pub(super) fn hold_line(&mut self, waits: Waits) -> Result<Cell, ParseError> {
        let start = self.cx.pos;
        let end = start + self.cx.source[start..waits.end].trim_end().len();
        let kept = self.parse_up_to(&waits)?;
        let place = if waits.writes { kept.cells.last().copied() } else { None };
        self.hold_text(start, end, waits.cut, kept, place, false)
    }

    /// The text from `start` to `end` held for the run, as [`Self::hold_line`] says, `kept` being
    /// its part before `cut`, parsed here, and `place`, where one is given, the place its `=`
    /// writes. The cursor ends at `end`.
    fn hold_text(
        &mut self,
        start: usize,
        end: usize,
        cut: usize,
        kept: Kept,
        place: Option<Cell>,
        lays_out: bool,
    ) -> Result<Cell, ParseError> {
        let types = self.types;
        let text = crate::identities::string::build_text(
            self.rt.store,
            types.string_,
            &self.cx.source.as_bytes()[start..end],
        );
        // SAFETY: `text` is the string node just built; its bytes live for the store.
        let held = unsafe { crate::identities::string::text(text) };
        let held = std::str::from_utf8(held).expect("copied from the source text");
        let fragment = lex_fragment(&self.cx.scopes, self.trie, self.rt.store, types, held)
            .map_err(|e| self.lex_error(start, e))?;
        let held_cells = fragment.held_cells();
        for (k, cell) in held_cells.iter().enumerate() {
            self.cx.pos = start + cell.start;
            let binding = cell.binding(types);
            if self.note_held_use(&held_cells, k)? || binding.is_null() {
                continue;
            }
            // SAFETY: a resolved cell's identity is a dyad from the store.
            unsafe { self.check_capture(cell.identity(types))? };
            self.check_made(binding)?;
            self.cx.pos = start + cell.end();
            self.check_held_read(binding, start + cell.start)?;
            self.note_outer_read(binding);
        }
        self.cx.pos = end;
        // SAFETY: `held` is the store's copy of the line, which holds each kept cell's spelling
        // `start` bytes before the source did.
        let cells = kept.cells.into_iter().map(|c| unsafe { c.moved_to(held, start) }).collect();
        let kept = Kept { cells, ..kept };
        let line = HeldLine {
            cells: fragment,
            cut: cut - start,
            kept,
            definitions: self.cx.definitions.clone(),
            run_fields: self.cx.run_fields.clone(),
            lays_out,
        };
        let line = self.rt.store.keep_line(line);
        let scope = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        let frames: Vec<DyadPtr> = self.cx.frames.iter().map(|f| f.frame).collect();
        let node = crate::identities::waiting::build_line(
            self.rt.store,
            types,
            text,
            line,
            scope,
            &frames,
        );
        if let Some(place) = place {
            self.note_write(node, &place);
        }
        // SAFETY: `node` was just built in the store.
        Ok(unsafe { Cell::built(node) })
    }

    /// Whether `cells[k]`, a cell of a held line, is no outer name: a member spelled after `.` or
    /// `:`, or a field of the value its body is about. What the line's parse notes on the open
    /// definition is noted here, since the body's callers and its later lines read it before the
    /// line first runs: a bare
    /// field read, or written where `=` follows it, and a bare call of a `static` function that
    /// writes mark a `static` function's receiver (›A `static` function writes the value it is
    /// called on, and respects `mut`‹ [[R239]]); in a parse, a stamp, `.type =`, has the node's
    /// later fields read by name, since the type it stamps is known only at run. A bare field is
    /// refused where [`Parser::names_own_field`] refuses it.
    fn note_held_use(&mut self, cells: &[Cell], k: usize) -> Result<bool, ParseError> {
        let types = self.types;
        let cell = &cells[k];
        if cell.constructed {
            return Ok(false);
        }
        let next = cells.get(k + 1).map(|c| self.cell_identity(c));
        let in_parse = self.cx.definitions.last().is_some_and(|d| d.in_parse);
        if k.checked_sub(1).is_some_and(|j| self.is_unbuilt_tight_read(&cells[j])) {
            if in_parse && cell.spelling() == "type" && next == Some(types.assign) {
                self.cx.definitions.last_mut().expect("a parse is open").stamps_at_run = true;
            }
            return Ok(true);
        }
        let field = self.names_own_field(cell)?;
        let bits = if field {
            RECEIVER_READS | (u64::from(next == Some(types.assign)) * RECEIVER_WRITES)
        } else if next == Some(types.open_) {
            // SAFETY: a resolved cell's identity is null or a dyad from the store.
            unsafe {
                let f = types.through(self.cell_identity(cell));
                if self.takes_this(f) {
                    fn_receiver(types, f)
                } else {
                    0
                }
            }
        } else {
            0
        };
        if let Some(def) = self.about_value_mut() {
            def.note_receiver_use(bits);
        }
        Ok(field)
    }

    /// The line at the cursor parsed up to the cut, the read the run supplies or a reader before
    /// it: lexed, and built as far as building a cell as it is lexed goes (›A `dyad ?` place is
    /// transparent: a placeholder for a new node of any type‹ [[R293]], its Q-61 line); a name
    /// nothing declared is reported here, as at the line's end. Before an `=`, what was lexed is
    /// the place it writes, read here as `=` reads it, so a write its gates refuse is refused
    /// where it is written (›A function is built once; a `fn` or `run` body is built as far as
    /// it can be where it is written‹ [[R299]]). What that parse noted for the line goes with
    /// the cells. The cursor ends at the cut.
    fn parse_up_to(&mut self, waits: &Waits) -> Result<Kept, ParseError> {
        let cut = waits.cut;
        if cut <= self.cx.pos {
            return Ok(Kept::default());
        }
        let open = self.cx.open.last_mut().expect("a line is read in an open scope");
        let before = open.take_line_notes();
        let source = self.cx.source;
        self.cx.source = &source[..cut];
        let was = std::mem::replace(&mut self.cx.cut_before_assign, waits.writes);
        let mut tape = ParsingTape::new();
        let parsed = self.lex_segment(&mut tape).and_then(|boundary| {
            debug_assert!(
                matches!(boundary, Boundary::Eof),
                "the cut stands before the line's end"
            );
            if waits.writes {
                tape.set_cursor(tape.len());
                let (place, _) = crate::identities::assign::read_target(self, &mut tape)?;
                tape.push(place);
            }
            Ok(())
        });
        self.cx.source = source;
        self.cx.cut_before_assign = was;
        let open = self.cx.open.last_mut().expect("a line is read in an open scope");
        let notes = open.take_line_notes();
        open.add_line_notes(before);
        parsed?;
        let cells: Vec<Cell> = tape.iter().map(|(_, c)| *c).collect();
        for &cell in cells.iter().filter(|c| c.is_fresh(self.types)) {
            self.as_operand(cell)?;
        }
        Ok(Kept { cells, notes })
    }

    /// A held line's first segment starts from the part parsed where it was written, whose nodes
    /// run when the line runs.
    pub(super) fn start_from_kept(&mut self, tape: &mut ParsingTape) -> Result<(), ParseError> {
        let Some(kept) = self.cx.kept.take() else { return Ok(()) };
        for cell in kept.cells {
            tape.push(cell);
        }
        let open = self.cx.open.last_mut().expect("a line is read in an open scope");
        open.add_line_notes(kept.notes);
        // A kept node that gives a type runs where it stands, as where it is built
        // (›A type known only when the program runs still works; what needs it is parsed when
        // its line runs‹ [[R377]]).
        let built: Vec<usize> =
            tape.iter().filter(|(_, c)| c.constructed).map(|(n, _)| n).collect();
        let end = tape.mark();
        for n in built {
            tape.center_on(n);
            if let Some(value) = self.run_type_where_built(tape, true)? {
                // The type it gave reads the value after it with its parse.
                if self.wakes(value, true) {
                    tape.set_constructed(0, false);
                }
            }
            let cell = *tape.cell_of(n);
            self.note_runs(&cell);
        }
        tape.restore(end);
        Ok(())
    }

    /// The value a `:=` declares its name with, when it waits for the run: held as a line of its
    /// own, whose value lands in a new node, a `dyad`, which the name holds, since its type is
    /// known only then (›A `type ?` place is a box, written any number of times‹ [[R291]], its
    /// Q72 line).
    pub(super) fn waiting_value(&mut self) -> Result<Option<Cell>, ParseError> {
        let Some(waits) = self.waiting_line_end()? else { return Ok(None) };
        let held = self.hold_line(waits)?;
        // SAFETY: `held` holds the line `hold_line` just built, which has not run.
        unsafe { crate::identities::waiting::set_into(self.types, held.dyad, self.types.dyad_) };
        Ok(Some(held))
    }

    /// Where `node` is a part of a segment held for the run: whether it lays out a place when it
    /// runs, so a name it declares holds no value until a write fills it.
    ///
    /// # Safety
    /// `node` must be null or a reduced dyad from the store.
    pub(super) unsafe fn held_lays_out(&self, node: DyadPtr) -> Option<bool> {
        if node.is_null() || crate::dyad::ty(node) != self.types.waiting.line {
            return None;
        }
        let (_, line, _, _, _) = crate::identities::waiting::line_parts(node);
        Some((*line).lays_out)
    }

    /// The value of a `:=`, `cell`, read from `start` to the cursor, where it gives a type known
    /// only when the program runs and nothing stands after it: the name is declared a place of
    /// that type (›`?` is one value, the unknown; `key := T ?` declares without a value‹
    /// [[R103]]), laid out when its line runs, so the value is held for the run, kept as built.
    /// `notes` is how many values and comments its line had noted before it. `None` for any
    /// other value.
    pub(super) fn hold_bare_type(
        &mut self,
        start: usize,
        notes: (usize, usize),
        cell: Cell,
    ) -> Result<Option<Cell>, ParseError> {
        if !self.waits_here() || cell.hole || cell.type_value || !self.gives_type_at_run(&cell) {
            return Ok(None);
        }
        let types = self.types;
        let at = self.cx.pos;
        let end = start + self.cx.source[start..at].trim_end().len();
        let open = self.cx.open.last_mut().expect("a value is read in an open scope");
        let notes = open.line_notes_since(notes);
        open.runs.retain(|&s| !(start..end).contains(&s));
        let kept = Kept { cells: vec![cell], notes };
        let mut held = self.hold_text(start, end, end, kept, None, true)?;
        // SAFETY: `held` holds the line `hold_text` just built, which has not run.
        unsafe { crate::identities::waiting::set_into(types, held.dyad, types.dyad_) };
        (held.start, held.len, held.gates) = (start, end - start, cell.gates);
        self.cx.pos = at;
        Ok(Some(held))
    }

    /// The `while` condition begun at `start` and ended at the cursor, which read a `dyad` name:
    /// the name stood for the node it held when the condition was parsed, so the condition is held
    /// as written and parsed again each time it runs, its value landing in `bool`.
    pub(super) fn hold_condition(&mut self, start: usize) -> Result<DyadPtr, ParseError> {
        let end = self.cx.pos;
        self.cx.pos = start;
        let held = self.hold_line(Waits { cut: start, writes: false, end })?;
        self.cx.pos = end;
        // SAFETY: `held` holds the line `hold_line` just built, which has not run.
        unsafe { crate::identities::waiting::set_into(self.types, held.dyad, self.types.bool_) };
        Ok(held.dyad)
    }

    /// Parse `node`, which waits for the run, inside the run that reached it: a held line over
    /// the scopes open where it was written, from its cut, its `dyad` names standing for the nodes
    /// they hold now. What it gives lands in its `into`.
    ///
    /// # Safety
    /// `node` must be a node `identities::waiting` built; the call it was written in must be the
    /// innermost activation.
    pub(super) unsafe fn parse_waiting(
        &mut self,
        node: DyadPtr,
    ) -> Result<DyadPtr, crate::run::RunError> {
        let types = self.types;
        let (text, line, scope, open_fns, into) = crate::identities::waiting::line_parts(node);
        let bytes = crate::identities::string::text(text);
        let text: &'a str = std::str::from_utf8(bytes).expect("copied from the source text");
        let line = &*line;
        let cells = line.cells.held_cells();
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
        // One frame per function open where the line was written, so a read of their names is
        // no capture and a name declared in the line is laid out in the program frame.
        let depth = open_fns.len();
        let frames = open_fns
            .iter()
            .map(|&f| OpenFn {
                frame: f,
                size: 0,
                below: 0,
                outer: Vec::new(),
                open_below: 0,
                returns: Vec::new(),
            })
            .collect();
        let cx = Context {
            pos: line.cut,
            frames,
            held_depth: depth,
            feed: Some(Feed { cells, next: 0, base_depth, in_run_order: true }),
            kept: Some(line.kept.clone()),
            definitions: line.definitions.clone(),
            run_fields: line.run_fields.clone(),
            next_block: if into.is_null() { BlockValue::Thrown } else { BlockValue::Returned },
            ..Context::nested(&self.cx, text, nested)
        };
        let lays_out = line.lays_out;
        let g = self.enter(cx);
        // A type with nothing after it is the place `T ?` makes of it.
        let built = g.0.parse_sequence().and_then(|line| {
            if !lays_out || types.is_hole(types.through(line)) {
                return Ok(line);
            }
            // The gates of a type known only here grant nothing: the parse checked every write.
            Ok(g.0.empty_place_of(line)?.map_or(line, |(place, _)| place))
        });
        let built_pos = g.0.cx.pos;
        drop(g);
        let built = built.and_then(|line| {
            if into.is_null() {
                return Ok(line);
            }
            if lays_out && types.is_hole(types.through(line)) {
                let hole = types.through(line);
                return crate::identities::dyad_place::lay_out(self.rt.store, types, hole);
            }
            crate::identities::commit_fn_body(self.rt.store, types, line, into)
        });
        built.map_err(|e| {
            crate::run::RunError::WaitFailed(Box::new(crate::report::render(
                "held line",
                text,
                built_pos,
                &crate::report::parse_message(&e),
            )))
        })
    }
}
