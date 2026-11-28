// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A line whose parse needs a type known only when it runs: one that reads a `dyad` name, or a
//! `-> dyad` call as an infix's operand or through `.`, one that reads a value, a `?` or a field
//! after a type a name or a call gives only then, and one that writes a name whose type its own
//! line gave when it ran. Found before it is lexed, parsed where it
//! is written up to that read or a reader before it, and from there each time it runs, over the
//! nodes and types such values hold then (`identities::waiting`, ›A `dyad ?` place is
//! transparent: a placeholder for a new node of any type‹ [[R293]], ›A type known only when the
//! program runs still works; what needs it is parsed when its line runs‹ [[R377]]). A `while`
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
    /// How many tokens and lines inside brackets the scan has met that the pass may not know as
    /// an argument of a call for a type: anything but one type or one literal alone on its line.
    unknown_now: usize,
    /// At the top, where a `?` after a type known only at run ends.
    typed_hole_end: Option<usize>,
    /// At the top, where the line starts when it is one type known only at run, with nothing
    /// after it.
    bare_type: Option<usize>,
}

/// A line that waits: where it ends, where the part left to the run starts, whether an `=`
/// stands there, the part before it being the place it writes, and whether the line is the place
/// of a type known only at run that a `:=` declares its name with: `T ?`, or `T` alone.
pub(super) struct Waits {
    cut: usize,
    writes: bool,
    end: usize,
    hole: bool,
}

/// The part of a line parsed before the cut: its cells, and what their parse noted for the line.
#[derive(Default, Clone)]
pub(super) struct Kept {
    cells: Vec<Cell>,
    notes: LineNotes,
}

/// A line held for the run, as [`Parser::hold_line`] left it: every cell of its text, lexed
/// against the scopes open where it is written, the part before `cut` parsed there, what a
/// field or a call through `self` there needs to reach the value its body is about: the type
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
    /// another `:=`, a `return`, `free` or `move` is refused here: stand-in for #281. With
    /// `declares`, the line is the value of a `:=`, which makes a place of a type with nothing
    /// after it, so one known only at run waits as `T ?` does (›`?` is one value, the unknown;
    /// `key := T ?` declares without a value‹ [[R103]]).
    pub(super) fn waiting_line_end(&mut self, declares: bool) -> Result<Option<Waits>, ParseError> {
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
        let bare_type = found.bare_type.filter(|_| declares);
        if !reads && bare_type.is_none() {
            return Ok(None);
        }
        if let Some((at, word)) = found.refused {
            self.cx.pos = at;
            return Err(ParseError::WaitingLineHolds(Box::new(word)));
        }
        if let Some(cut) = bare_type {
            return Ok(Some(Waits { cut, writes: false, end, hole: true }));
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
        let hole =
            found.typed_hole_end == Some(start + self.cx.source[start..end].trim_end().len());
        if found.address_at.is_some_and(|at| at < cut) {
            return Ok(Some(Waits { cut: start, writes: false, end, hole }));
        }
        Ok(Some(Waits { cut, writes, end, hole }))
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
            let r = self.scanned_through_self(at, r)?;
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
        // A function just scanned whose call is a call for a type.
        let mut type_callee = false;
        // A type known only when the line runs, just scanned: a name holding a type, or a call
        // for a type over an argument the pass may not know; where the cells built with it start.
        let mut run_type: Option<usize> = None;
        // A name just scanned whose type its line gave when it ran, with an `=` next.
        let mut run_write = false;
        // A read of a tape node just scanned, `tape[k]` or `tape[k].dyads[i]`, or the `.type` of
        // one; and the `.dyads` of one. Each with where its cells start.
        let mut node_read: Option<(NodeRead, usize)> = None;
        let mut lines: Option<usize> = None;
        // At the top, the tokens before the first `:=` that are no gate word; in a bracket, the
        // tokens of the line being scanned.
        let (mut lead, mut line_tokens) = (0, 0);
        // At the top: where the cells built together with the token just scanned start, and
        // where those of the last call that gives a `dyad` start.
        let (mut chain, mut call_chain) = (0, 0);
        // At the top, where the line's first token starts.
        let mut line_start: Option<usize> = None;
        loop {
            self.skip_whitespace();
            let at = self.cx.pos;
            let ends_here = at >= self.cx.source.len();
            let r = if ends_here { None } else { Some(self.token_at(at, true)?) };
            let r = r.map(|r| self.scanned_through_self(at, r)).transpose()?;
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
                    found.bare_type = run_type.filter(|_| !skipping && run_type == line_start);
                    return Ok(any);
                }
                if line_tokens > 1 {
                    found.unknown_now += 1;
                }
                let (Some(r), Some(id)) = (r, id) else { return Err(ParseError::UnclosedBracket) };
                self.cx.pos = at + r.matched;
                if self.is_closer(id) {
                    return Ok(any);
                }
                (line_reads, assigns, declares, prev, before_prev, skipping) =
                    (false, false, None, None, None, false);
                (callee, after_call, type_member) = (None, false, false);
                (type_callee, run_type, run_write, line_tokens) = (false, None, false, 0);
                (reached, path_start) = (None, (None, None));
                (node_read, lines) = (None, None);
                continue;
            }
            let (Some(r), Some(id)) = (r, id) else { unreachable!("a boundary at the end") };
            self.cx.pos = at + r.matched;
            line_tokens += 1;
            if top {
                line_start.get_or_insert(at);
            }
            let leads = id != types.declare_tok && !self.is_gate_word(id);
            if top && leads && found.declaration.is_none() && declares.is_none() {
                lead += 1;
            }
            let member_call = std::mem::take(&mut type_member);
            let type_call = std::mem::take(&mut type_callee) || member_call;
            if top {
                if !self.joins_chain(prev, id) {
                    chain = at;
                }
                if self.reads_past_next(id) {
                    found.reader_at.get_or_insert((chain, id == types.assign));
                }
            }
            // What is written into a place whose type its line gave when it ran converts into
            // that type, so the line waits from its `=`, the place read where it is written.
            if std::mem::take(&mut run_write) {
                line_reads = true;
                if top {
                    found.read_at.get_or_insert(at + r.matched);
                }
            }
            // A value, a `?` or a field read after a type known only at run needs that type.
            if let Some(from) = run_type.take().filter(|_| !skipping && self.uses_type(id)) {
                line_reads = true;
                if top {
                    found.read_at.get_or_insert(from);
                    if id == types.unknown {
                        found.typed_hole_end = Some(at + r.matched);
                    }
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
                let unknown_before = found.unknown_now;
                let inner = if skipping {
                    self.skip_bracket(id).map(|_| false)?
                } else {
                    self.scan_level(depth + 1, found)?
                };
                if found.call_reads + found.needs > reads_before {
                    found.cuts.push(at);
                }
                if type_call && id == types.open_ && found.unknown_now > unknown_before {
                    run_type = Some(chain);
                }
                // A bracket among a call's arguments is no argument the pass knows as written.
                found.unknown_now += 1;
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
            if !self.known_as_written(id) {
                found.unknown_now += 1;
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
                if prev != Some(types.dot_) {
                    type_callee = self.returns_type(id);
                    if read_kind(types, id) == Read::Container(types.type_) {
                        run_type = Some(chain);
                    }
                    run_write = crate::dyad::ty(id) == types.binding_
                        && Binding::read(id).typed_at_run
                        && self.peek_token().is_some_and(|(next, _)| next == types.assign);
                }
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

    /// Whether the pass knows the value `id` gives where it is written, as an argument of a call
    /// for a type: a type or a literal (`identities::is_comptime_arg`).
    fn known_as_written(&self, id: DyadPtr) -> bool {
        // SAFETY: a resolved token's identity is null or a dyad from the store.
        self.ranked(id, prec::LITERAL)
            || unsafe { !id.is_null() && crate::identities::is_type_value(self.types, id) }
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

    /// The line at the cursor, cut where `waits` says: the part before the cut parsed here, and
    /// the line held as it is written, lexed against the scopes open here, its names checked as a
    /// use of them is, and parsed from the cut each time it runs; with `lays_out`, the `T ?` that
    /// lays out a name's node.
    pub(super) fn hold_line(&mut self, waits: Waits, lays_out: bool) -> Result<Cell, ParseError> {
        let types = self.types;
        let start = self.cx.pos;
        let end = start + self.cx.source[start..waits.end].trim_end().len();
        let kept = self.parse_up_to(&waits)?;
        let place = if waits.writes { kept.cells.last().copied() } else { None };
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
            cut: waits.cut - start,
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
    /// line first runs: a field read through `self`, or written where `=` follows it, and a call
    /// through `self` of a `static` function that writes mark a `static` function's receiver
    /// (›A `static` function writes the value it is called on, and respects `mut`‹ [[R239]]); in
    /// a parse, a stamp, `.type =`, has the node's later fields read by name, since the type it
    /// stamps is known only at run. A member named bare is refused where
    /// [`Parser::refuse_bare_member`] refuses it, and a field where [`Parser::own_field`] does.
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
        let member = self.held_member(cells, k);
        let (cell, next) = match &member {
            Some(name) => (name, cells.get(k + 3).map(|c| self.cell_identity(c))),
            None => {
                self.refuse_bare_member(cell, next)?;
                (cell, next)
            }
        };
        let field = self.own_field(cell)?;
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
        Ok(field || member.is_some())
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
    pub(super) fn start_from_kept(&mut self, tape: &mut ParsingTape) {
        let Some(kept) = self.cx.kept.take() else { return };
        for cell in kept.cells {
            if cell.constructed {
                self.note_runs(&cell);
            }
            tape.push(cell);
        }
        let open = self.cx.open.last_mut().expect("a line is read in an open scope");
        open.add_line_notes(kept.notes);
    }

    /// The value a `:=` declares its name with, when it waits for the run: held as a line of its
    /// own, whose value lands in a new node, a `dyad`, which the name holds, since its type is
    /// known only then (›A `type ?` place is a box, written any number of times‹ [[R291]], its
    /// Q72 line), with whether that value is a `?`, which lays the node out, unwritten, when the
    /// line runs (›A type known only when the program runs still works; what needs it is parsed
    /// when its line runs‹ [[R377]]).
    pub(super) fn waiting_value(&mut self) -> Result<Option<(Cell, bool)>, ParseError> {
        let Some(waits) = self.waiting_line_end(true)? else { return Ok(None) };
        let hole = waits.hole;
        let held = self.hold_line(waits, hole)?;
        // SAFETY: `held` holds the line `hold_line` just built, which has not run.
        unsafe { crate::identities::waiting::set_into(self.types, held.dyad, self.types.dyad_) };
        Ok(Some((held, hole)))
    }

    /// The `while` condition begun at `start` and ended at the cursor, which read a `dyad` name:
    /// the name stood for the node it held when the condition was parsed, so the condition is held
    /// as written and parsed again each time it runs, its value landing in `bool`.
    pub(super) fn hold_condition(&mut self, start: usize) -> Result<DyadPtr, ParseError> {
        let end = self.cx.pos;
        self.cx.pos = start;
        let held = self.hold_line(Waits { cut: start, writes: false, end, hole: false }, false)?;
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
                result: std::ptr::null_mut(),
                slot_body: false,
                gives_at: None,
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
