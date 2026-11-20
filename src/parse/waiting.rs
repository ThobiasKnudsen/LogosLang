// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A line whose value's type is known only when it runs, since it reads a `dyad` name, or a
//! `-> dyad` call as an infix's operand or through `.`: found before it is lexed, parsed where it
//! is written up to that read or a reader before it, and from there each time it runs, over the
//! nodes such values hold then (`identities::waiting`, ›A `dyad ?` place is transparent: a
//! placeholder for a new node of any type‹ [[R293]]). A `while` condition that read a `dyad` name
//! is held the same way, parsed again on each pass.

use super::*;
use crate::identities::meta::prec;
use crate::identities::read::{read_kind, Read};

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
    /// Where each bracket that holds such a read opens, at any depth.
    cuts: Vec<usize>,
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
/// against the scopes open where it is written, the part before `cut` parsed there, and what a
/// bare field or a bare call there needs to reach the value its body is about: the type
/// definitions open there and a `run` body's fields.
pub(crate) struct HeldLine {
    cells: ParsingTape,
    cut: usize,
    kept: Kept,
    definitions: Vec<OpenType>,
    run_fields: Option<(DyadPtr, Vec<DyadPtr>)>,
}

impl<'a> Parser<'a> {
    /// The line at the cursor, when it waits for the run: a line of a body that reads a `dyad`
    /// value for its value, outside a `fn` or `type (…)` written in it, and outside a line of a
    /// bracket that waits on its own. The cursor stays. A line that waits and holds `:=`,
    /// `return`, `free` or `move` is refused here: stand-in for #281.
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
        let Ok(true) = scanned else { return Ok(None) };
        if let Some((at, word)) = found.refused {
            self.cx.pos = at;
            return Err(ParseError::WaitingLineHolds(Box::new(word)));
        }
        let read_at = found.read_at.unwrap_or(start);
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

    /// Where the line at the cursor reads what a call that gives a `dyad` gives inside a bracket,
    /// where the pass runs that call as it parses it: each bracket that holds such a read, at any
    /// depth, before whose parse what stands before it in its line runs ([`Self::cut_at`]). A line
    /// a scan already covered keeps its cuts. The cursor stays.
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
    /// read of what a call that gives a `dyad` gives: what stands before it runs, in order (›The
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
        let operator = self.ctor_of(prev).is_some() && self.precedence_of_cell(prev) < prec::OPEN;
        !function && !operator
    }

    /// One level of the line: at the top the line itself, up to its boundary, left unconsumed;
    /// below it the lines of a bracket, up to its closer, consumed. Whether a line of this level
    /// reads a `dyad` value for the line around it: an assignment gives nothing (›`=` sits beside
    /// `:=`, and returns nothing‹ [[R277]]), so one in a bracket waits on its own, when the parse
    /// reaches it. A `:=` on a line that reads is refused, and at the top a `:=` at all.
    fn scan_level(&mut self, depth: usize, found: &mut Found) -> Result<bool, ParseError> {
        self.check_depth(depth)?;
        let types = self.types;
        let top = depth == 0;
        let bare_branch = top && self.cx.else_ends == Some(self.cx.open.len());
        let (mut any, mut line_reads, mut assigns) = (false, false, false);
        let mut declares: Option<usize> = None;
        let (mut prev, mut before_prev): (Option<DyadPtr>, Option<DyadPtr>) = (None, None);
        // After a `fn` or `type`, not a member's name, the rest of the level is a body read where
        // it is built.
        let mut skipping = false;
        // A `-> dyad` function just scanned, with whether an infix stands before it; and whether
        // the bracket just closed was its call's.
        let (mut callee, mut after_call): (Option<bool>, bool) = (None, false);
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
                (callee, after_call) = (None, false);
                continue;
            }
            let (Some(r), Some(id)) = (r, id) else { unreachable!("a boundary at the end") };
            self.cx.pos = at + r.matched;
            if top {
                if !self.joins_chain(prev, id) {
                    chain = at;
                }
                if self.reads_past_next(id) {
                    found.reader_at.get_or_insert((chain, id == types.assign));
                }
            }
            if self.is_opener(id) {
                let calls_before = found.call_reads;
                let inner = if skipping {
                    self.skip_bracket(id, depth + 1).map(|_| false)?
                } else {
                    self.scan_level(depth + 1, found)?
                };
                if found.call_reads > calls_before {
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
                continue;
            }
            if skipping {
                continue;
            }
            if (id == types.fn_type || id == types.type_) && prev != Some(types.dot_) {
                skipping = true;
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
                declares.get_or_insert(at);
            }
            assigns |= id == types.assign;
            // SAFETY: a resolved token's identity is a dyad from the store.
            let dyad_read = prev != Some(types.dot_)
                && unsafe { read_kind(types, id) } == Read::Container(types.dyad_);
            // SAFETY: as above.
            let target_holds_dyad = before_prev
                .is_some_and(|t| unsafe { read_kind(types, t) == Read::Container(types.dyad_) });
            if dyad_read && !self.reads_the_box(prev, target_holds_dyad, top, bare_branch) {
                line_reads = true;
                if top {
                    found.read_at.get_or_insert(chain);
                }
            }
            (before_prev, prev) = (prev, Some(id));
        }
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

    /// Whether the `dyad` name just scanned, after `prev`, stands for its place rather than the
    /// node it holds: what `=` writes, `&` points at, `free` and `move` end, and a value taken
    /// whole by a `:=`, a `return`, an `=` into another `dyad` place or the line itself.
    fn reads_the_box(
        &mut self,
        prev: Option<DyadPtr>,
        target_holds_dyad: bool,
        top: bool,
        bare_branch: bool,
    ) -> bool {
        let types = self.types;
        let next = self.peek_token().map(|(id, _)| id);
        if next == Some(types.assign) {
            return true;
        }
        if prev.is_some_and(|p| {
            p == types.free_ || p == types.move_ || self.precedence_of_cell(p) == prec::ADDRESS
        }) {
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
    /// use of them is, and parsed from the cut each time it runs.
    pub(super) fn hold_line(&mut self, waits: Waits) -> Result<Cell, ParseError> {
        let types = self.types;
        let start = self.cx.pos;
        let end = start + self.cx.source[start..waits.end].trim_end().len();
        let kept = self.parse_up_to(&waits)?;
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
        };
        let line = Box::into_raw(Box::new(line));
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
        // SAFETY: `node` was just built in the store.
        Ok(unsafe { Cell::built(node) })
    }

    /// Whether `cells[k]`, a cell of a held line, names a field of the value its body is about,
    /// so it is no outer name. What the line's parse notes on the open definition is noted here,
    /// since the body's callers and its later lines read it before the line first runs: a bare
    /// field read, or written where `=` follows it, and a bare call of a `static` function that
    /// writes mark a `static` function's receiver (›A `static` function writes the value it is
    /// called on, and respects `mut`‹ [[R239]]); in a parse, a stamp, `.type =`, has the node's
    /// later fields read by name, since the type it stamps is known only at run, and a bare field
    /// is refused, as on any line of a parse.
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
            return Ok(false);
        }
        let field = self.names_own_field(cell);
        if field && in_parse {
            return Err(ParseError::BareFieldInParse(Box::new(cell.spelling().to_string())));
        }
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
        if let Some(def) = self.cx.definitions.last_mut().filter(|d| !d.this_param.is_null()) {
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
        let g = self.enter(cx);
        let built = g.0.parse_sequence();
        let built_pos = g.0.cx.pos;
        drop(g);
        let built = built.and_then(|line| {
            if into.is_null() {
                return Ok(line);
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
