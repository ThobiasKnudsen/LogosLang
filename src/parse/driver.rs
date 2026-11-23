// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The segment driver: lex a segment whole, construct it highest parse_rank first, and
//! hand out the items; which constructor a cell wakes and how a Logos constructor runs.

use super::*;
use crate::dyad;

/// Where a segment stopped, none consumed by the lexing step.
pub(crate) enum Boundary {
    Comma,
    Close,
    Eof,
    /// Where a right-side read stops: its caller's bracket, or the start of
    /// an `if`'s bare branch.
    Open,
    /// The `else` that ends an `if`'s bare branch.
    Else,
}

/// What a right-side read is for, which decides whose a `(` inside it is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RightSide {
    /// `for`'s range: a bracket after a pending identity is that identity's.
    Range,
    /// The first bracket is the body, and an identity that reads its own
    /// bracket is not woken.
    ReturnType,
    /// `if`'s and `while`'s: ends at the first complete expression; see `condition_ends`.
    Condition,
}

/// A constructor edits the tape in place; `Placed` reports only that it did, and `Gave` that its
/// cell holds the value an expression gives, a call's result or what a member holds, which the
/// driver may hand on as a name holding it. `Decline` is "not mine": nothing consumed, nothing
/// touched, and the driver leaves the identity standing as its own value.
pub enum Constructed {
    Placed,
    Gave,
    Decline,
}

/// A segment constructed: its items in order, and the comments lifted out of it, each list in
/// the order lifted: `before` stood left of every cell, `after` the rest.
pub(super) struct Built {
    pub(super) items: Vec<(Cell, usize)>,
    pub(super) before: Vec<DyadPtr>,
    pub(super) after: Vec<DyadPtr>,
}

/// Every identity's parse-time constructor: it reads its span from the
/// cursor cell, its context from `tape.at(-1)` and `tape.at(1)`, and edits
/// the tape in place. The driver decides only *when* constructors run.
pub type ConstructFn =
    fn(&mut Parser, DyadPtr, &mut ParsingTape) -> Result<Constructed, ParseError>;

/// The function in the identity's constructor slot, run over the tape.
fn logos_constructor(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    if let Some(native) = p.types.native_of(id) {
        if let Some(outcome) = p.around_written(id, native, tape)? {
            return Ok(outcome);
        }
    }
    // SAFETY: `id` is the identity whose slot `construct_of` just read.
    let f = unsafe { crate::identities::meta::constructor_of(id) };
    // SAFETY: `f` is the fn the type's constructor slot holds, `id` the type.
    unsafe { p.run_logos_ctor(f, id, tape) }
}

/// The `parse` of the type of the value standing in the cell, run with
/// `tape[0]` that value (DESIGN ›A type body describes one level‹).
fn instances_constructor(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    let Some(t) = p.awake_type(id) else {
        return Ok(Constructed::Decline);
    };
    // SAFETY: `awake_type` found the slot filled on this record type.
    let f = unsafe { crate::identities::meta::constructor_of(t) };
    // An instance's parse runs over the instance, never over a type's fresh node.
    let outer = p.rt.set_fresh_this(None);
    // SAFETY: `f` is the fn the type's parse slot holds, `id` a node its parse built or a place of that type.
    let out = unsafe { p.run_logos_body(f, id, tape) };
    p.rt.restore_fresh_this(outer);
    out
}

/// The constructor a function's signature gives its values, the call defaults' `parse`: a
/// function before a bracket is called with it (›`fn` is not a primitive: `fn (…) -> T` is a
/// function's type, and its body the value‹ [[R199]]). The signature standing there itself, read
/// as `f.input`, makes a value of its parameters, as a record type does.
pub(crate) fn call_parse(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    // SAFETY: `id` is a resolved dyad from the store.
    let build = match unsafe { p.identity_head(id) } {
        Some(_) => Parser::build_instance,
        None => Parser::build_fn_call,
    };
    p.construct_application(id, tape, build)
}

/// The constructor `type` gives a record type whose body fills no `parse`: `T (…)` makes a value
/// of `T` from the bracket's values, positional in order (›`X (…)` is one spelling, and X's
/// constructor decides what the bracket is‹ [[R270]], ›Plain data and hand-written types are one
/// identity, `type`‹ [[R209]]).
pub(crate) fn derived_parse(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    p.construct_application(id, tape, Parser::build_instance)
}

impl<'a> Parser<'a> {
    /// Every cell up to the next `,`, `)`, or the end of input, none consumed,
    /// constructing at discovery each cell whose identity sits at or above `(`
    /// on the axis (DESIGN ›The scope's constructor is the driver‹).
    pub(super) fn lex_segment(&mut self, tape: &mut ParsingTape) -> Result<Boundary, ParseError> {
        self.lex_segment_until(tape, None)
    }

    /// `lex_segment`, optionally stopping before a `(` that follows at least
    /// one cell: the right side an identity reads up to its body bracket. The
    /// mode is kept on the parser for the lazy reads a constructor makes meanwhile.
    fn lex_segment_until(
        &mut self,
        tape: &mut ParsingTape,
        stop_at_open: Option<RightSide>,
    ) -> Result<Boundary, ParseError> {
        let was = std::mem::replace(&mut self.cx.lex_mode, stop_at_open);
        let result = loop {
            match self.lex_next(tape, false) {
                Ok(None) => {}
                Ok(Some(boundary)) => {
                    tape.sealed = true;
                    break Ok(boundary);
                }
                Err(e) => break Err(e),
            }
        };
        self.cx.lex_mode = was;
        result
    }

    /// One step of the scope's loop (DESIGN ›The scope's constructor is the
    /// driver‹): a boundary token is left unconsumed and returned. A cell lexed
    /// `lazy`, for a constructor's read, arrives unbuilt for that reader to decide,
    /// unless building it is lexing it: a bracket, a literal, raw text.
    fn lex_next(
        &mut self,
        tape: &mut ParsingTape,
        lazy: bool,
    ) -> Result<Option<Boundary>, ParseError> {
        let Some((mut cell, start)) = self.lex_cell(tape)? else {
            return Ok(Some(Boundary::Eof));
        };
        if !cell.constructed && !self.after_tight_read(tape) {
            if self.reads_result_place(&cell) {
                cell.dyad = self.result_place(start)?;
            } else {
                let field = self.names_own_field(&cell);
                if field.is_err() {
                    self.cx.pos = start;
                }
                if field? {
                    // Left unbuilt, as a name is, so a value whose type has a parse wakes it:
                    // `items[1]`.
                    cell.dyad = self.this_field(cell.spelling(), start)?;
                }
            }
        }
        let id = self.cell_identity(&cell);
        if !cell.constructed {
            self.check_unwritten(&cell)?;
            if let Some(boundary) = self.boundary_of(id) {
                self.cx.pos = start;
                return Ok(Some(boundary));
            }
            // A return type takes no bracket, so there the first `(` is the body.
            if self.cx.lex_mode == Some(RightSide::ReturnType)
                && id == self.types.open_
                && !tape.is_empty()
            {
                self.cx.pos = start;
                return Ok(Some(Boundary::Open));
            }
        }
        if self.reads_first_node() && self.condition_ends(tape, Some(&cell)) {
            self.cx.pos = start;
            return Ok(Some(Boundary::Open));
        }
        tape.push(cell);
        if !cell.constructed {
            if let Some(construct) = self.ctor_of(id) {
                let prec = self.precedence_of_cell(id);
                // A right-side read stops before its caller's bracket, so an
                // identity that reads its own bracket (`type`, `fn`) is not woken
                // there; a reader followed by a tight read never wakes (DESIGN ›Text is the quote‹).
                let reader = prec == crate::identities::meta::prec::READER
                    || prec == crate::identities::meta::prec::DECLARE;
                let asleep = (self.cx.lex_mode == Some(RightSide::ReturnType) && reader)
                    || self.cx.member_asleep
                    || self.tight_read_takes(id)
                    || self.waits_for_reads(id, tape.at(1), tape.at(2).is_some());
                if prec >= crate::identities::meta::prec::OPEN
                    && !asleep
                    && (!lazy
                        || crate::identities::meta::prec::built_as_lexed(prec)
                        || self.goes_before_reader(id, prec))
                {
                    let opener = self.is_opener(id);
                    if opener && !lazy && !self.reads_element(tape) {
                        self.skip_bracket(id)?;
                        tape.at_mut(0).expect("pushed above").held = Some(self.cx.pos);
                    } else {
                        if opener {
                            self.cut_at(tape, cell.start)?;
                        }
                        self.run_ctor(construct, id, tape, true)?;
                    }
                }
            }
        }
        if !lazy {
            self.discover_pending(tape)?;
        }
        Ok(None)
    }

    /// The boundary an unbuilt token ends its segment at: a `,`, a closer, or the `else` that ends
    /// an `if`'s bare branch.
    pub(super) fn boundary_of(&self, id: DyadPtr) -> Option<Boundary> {
        if id == self.types.sep_ {
            Some(Boundary::Comma)
        } else if self.is_closer(id) {
            Some(Boundary::Close)
        } else if id == self.types.else_ && self.cx.else_ends == Some(self.cx.open.len()) {
            Some(Boundary::Else)
        } else {
            None
        }
    }

    /// A cell a reader's lazy read lexes that would construct before the reader at the
    /// boundary: the same rank, both associating right, so the right one goes first
    /// (DESIGN ›A reader's read builds an equal right-associative cell first‹).
    fn goes_before_reader(&self, id: DyadPtr, prec: f64) -> bool {
        let reader = self.cx.reader;
        !reader.is_null()
            && self.precedence_of_cell(reader) == prec
            && self.assoc_of_cell(reader) == Assoc::Right
            && self.assoc_of_cell(id) == Assoc::Right
    }

    /// Construct at discovery what a lazy read lexed and its reader left on the
    /// tape, in tape order, before the loop lexes further; each runs with the
    /// center on its own cell.
    fn discover_pending(&mut self, tape: &mut ParsingTape) -> Result<(), ParseError> {
        let mark = tape.mark();
        loop {
            let mut found = None;
            let cells: Vec<(usize, Cell)> = tape.iter().map(|(n, c)| (n, *c)).collect();
            for (i, &(n, c)) in cells.iter().enumerate() {
                if c.constructed || c.held.is_some() {
                    continue;
                }
                let id = self.cell_identity(&c);
                let Some(construct) = self.ctor_of(id) else { continue };
                let prec = self.precedence_of_cell(id);
                if prec < crate::identities::meta::prec::OPEN {
                    continue;
                }
                let reader = prec == crate::identities::meta::prec::READER
                    || prec == crate::identities::meta::prec::DECLARE;
                let side = cells.get(i + 1).map(|(_, c)| c);
                let asleep = (self.cx.lex_mode == Some(RightSide::ReturnType) && reader)
                    || self.tight_read_takes(id)
                    || self.waits_for_reads(id, side, cells.len() > i + 2);
                if asleep {
                    continue;
                }
                found = Some((n, construct, id));
                break;
            }
            let Some((n, construct, id)) = found else { break };
            tape.center_on(n);
            if let Err(e) = self.run_ctor(construct, id, tape, true) {
                tape.restore(mark);
                return Err(e);
            }
        }
        tape.restore(mark);
        Ok(())
    }

    /// Whether `id`, built at discovery below the reads after a node (`&`), waits there: until
    /// `side`, the cell to its right, is lexed and built and no read after a node follows it, a
    /// bracket or a word of their ranks, so `&p.x` takes `p.x` (›Every identity has a
    /// `parse_rank`, and `(` works in two steps‹ [[R259]]). `lexed_after`: a cell already stands
    /// after `side`. A held bracket or a word still to be built there it takes at the boundary.
    fn waits_for_reads(&mut self, id: DyadPtr, side: Option<&Cell>, lexed_after: bool) -> bool {
        use crate::identities::meta::prec;
        if !prec::waits_for_reads(self.precedence_of_cell(id)) {
            return false;
        }
        let Some(side) = side else { return true };
        if side.held.is_some() || !self.is_operand_cell(side) {
            return true;
        }
        !lexed_after
            && self.peek_token().is_some_and(|(n, _)| {
                self.is_opener(n)
                    || (self.ctor_of(n).is_some()
                        && prec::reads_after_node(self.precedence_of_cell(n)))
            })
    }

    /// The cell `offset` links right of the center, lexed on demand (DESIGN
    /// ›The scope's constructor is the driver‹): what lets `:`, `.` and `@`
    /// construct at discovery. `None` when a boundary comes first; negative offsets read the tape as it stands.
    pub(crate) fn cell_at(
        &mut self,
        tape: &mut ParsingTape,
        offset: isize,
    ) -> Result<Option<Cell>, ParseError> {
        if offset <= 0 || tape.sealed {
            return Ok(tape.at(offset).copied());
        }
        let mark = tape.mark();
        let edits = tape.edits();
        // `lex_next` pushes at the end and moves the center there, so the
        // center is put back before each look at `offset`.
        while tape.at(offset).is_none() {
            match self.lex_next(tape, true) {
                Ok(None) => tape.restore(mark),
                Ok(Some(_)) => break,
                Err(e) => {
                    tape.restore(mark);
                    return Err(e);
                }
            }
        }
        tape.restore(mark);
        tape.lexed_on(edits);
        Ok(tape.at(offset).copied())
    }

    /// At the boundary: the unconstructed cells built highest parse_rank first,
    /// associativity breaking ties, no lookahead. Before each, the comment cells
    /// built so far are lifted out, one a constructor built here included; what
    /// remains is read as operands.
    pub(super) fn construct_segment(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Built, ParseError> {
        let (mut before, mut after) = (Vec::new(), Vec::new());
        loop {
            let mut leads = true;
            let mut prose = Vec::new();
            for (n, c) in tape.iter() {
                if self.is_comment_cell(c) {
                    prose.push((n, c.dyad, leads));
                } else {
                    leads = false;
                }
            }
            for (n, d, leads) in prose {
                if leads {
                    before.push(d);
                } else {
                    after.push(d);
                }
                tape.center_on(n);
                tape.remove(0);
            }
            let mut best: Option<(usize, f64, ConstructFn, DyadPtr)> = None;
            for (n, c) in tape.iter() {
                if c.constructed || c.held.is_some() {
                    continue;
                }
                let id = self.cell_identity(c);
                let Some(construct) = self.ctor_of(id) else { continue };
                let prec = self.precedence_of_cell(id);
                let right = self.assoc_of_cell(id) == Assoc::Right;
                let better = match best {
                    None => true,
                    Some((_, bp, _, _)) => prec > bp || (prec == bp && right),
                };
                if better {
                    best = Some((n, prec, construct, id));
                }
            }
            let Some((n, _, construct, id)) = best else {
                // A bracket no word took is built at the segment's end, in written order.
                if let Some(n) = tape.first_held(usize::MAX) {
                    tape.center_on(n);
                    self.take_held(tape, 0)?;
                    continue;
                }
                // A bracket no identity claimed, standing before another cell, gives its value
                // as any expression does, a type included.
                let last = tape.iter().last().map(|(n, _)| n);
                let woken = tape.iter().find(|&(n, c)| {
                    Some(n) != last && c.bracket && self.wakes(self.cell_identity(c), true)
                });
                let Some((n, _)) = woken else { break };
                tape.center_on(n);
                let Some(cell) = tape.at_mut(0) else { break };
                cell.bracket = false;
                cell.constructed = false;
                continue;
            };
            tape.center_on(n);
            self.run_ctor(construct, id, tape, false)?;
        }
        let cells: Vec<Cell> = tape.iter().map(|(_, c)| *c).collect();
        let mut items = Vec::with_capacity(cells.len());
        for cell in cells {
            // The cell leaves the tape as the operand it reduces to, its facts with it.
            let mut reduced = cell;
            reduced.dyad = self.as_operand(cell)?;
            reduced.constructed = true;
            items.push((reduced, cell.start));
        }
        Ok(Built { items, before, after })
    }

    /// A segment built inside a line hands its comments to that line: lines of the scope the
    /// line stands in, after it (›A scope's `dyads` fill as its lines complete, and hold
    /// everything‹ [[R272]]).
    pub(super) fn hand_to_line(&mut self, built: Built) -> Vec<(Cell, usize)> {
        let open = self.cx.open.last_mut().expect("a line is read in an open scope");
        open.prose.extend(built.before);
        open.prose.extend(built.after);
        built.items
    }

    /// The cells up to the next `(`, the right side an identity reads before
    /// its bracket: a range, a return type (DESIGN ›The scope's constructor is the driver‹).
    /// A `(` standing first is part of the read; a later one is the bracket.
    pub(super) fn drive_until_open(
        &mut self,
        mode: RightSide,
    ) -> Result<Vec<(Cell, usize)>, ParseError> {
        let built = self.drive_segment(mode)?;
        Ok(self.hand_to_line(built))
    }

    /// [`Self::drive_until_open`] with the segment's comments, for the reader whose lines they are.
    pub(super) fn drive_segment(&mut self, mode: RightSide) -> Result<Built, ParseError> {
        let mut tape = ParsingTape::new();
        self.lex_segment_until(&mut tape, Some(mode))?;
        self.construct_segment(&mut tape)
    }

    /// One segment, lexed to the next `,`, `)`, or end of input (left
    /// unconsumed) and constructed to exactly one cell: what a discovery-time
    /// constructor drives.
    pub fn parse_expression(&mut self) -> Result<DyadPtr, ParseError> {
        self.parse_expression_cell().map(|c| c.dyad)
    }

    /// [`Self::parse_expression`] with the cell's facts, for the reader that consumes them.
    pub(crate) fn parse_expression_cell(&mut self) -> Result<Cell, ParseError> {
        let mut tape = ParsingTape::new();
        self.lex_segment(&mut tape)?;
        let built = self.construct_segment(&mut tape)?;
        let items = self.hand_to_line(built);
        self.one_of(items)
    }

    /// None is an empty expression, more than one the leftover cell, reported
    /// at the second.
    pub(super) fn one_of(&mut self, items: Vec<(Cell, usize)>) -> Result<Cell, ParseError> {
        match items.len() {
            0 => Err(ParseError::Empty),
            1 => Ok(items[0].0),
            _ => {
                self.cx.pos = items[1].1;
                Err(self.leftover_error(items[0].0.dyad))
            }
        }
    }

    /// Usually `Trailing`; where the first leftover names a type only running gives, the real
    /// reason is that it could not say which type it is, so `a 5` never juxtaposed.
    pub(super) fn leftover_error(&mut self, first: DyadPtr) -> ParseError {
        match self.type_known_now(first) {
            Err(e) if e.type_given_by_running() => e,
            _ => ParseError::Trailing,
        }
    }

    /// `None` for an inert cell (a value, a delimiter): an identity's constructor slot, or the
    /// `parse` of the type of the value standing in the cell (›The constructors drive the tape‹
    /// [[R211]], ›No climbing: a type without a parse leaves its values inert‹ [[R214]]).
    pub(super) fn ctor_of(&self, id: DyadPtr) -> Option<ConstructFn> {
        if self.is_awake_instance(id) {
            return Some(instances_constructor);
        }
        // SAFETY: `id` is a resolved dyad from the store.
        self.construct_of(unsafe { self.dispatch_record(id) }?)
    }

    /// A gated type as the type its gates stand over and its gate mint, built by that type's
    /// constructor and standing with the gates, as a gate word over it leaves it (›Gates are type
    /// nodes: `x := mut i32 5`; the record holds no gates‹ [[R381]]); any other `id` as itself,
    /// with no gates.
    fn gated_type_parts(&self, id: DyadPtr) -> (DyadPtr, DyadPtr) {
        use crate::identities::read::{read_kind, Read};
        // SAFETY: `id` is null or a resolved dyad from the store.
        unsafe {
            if read_kind(self.types, id) == Read::Identity {
                crate::identities::gate::parts(id)
            } else {
                (id, std::ptr::null_mut())
            }
        }
    }

    /// The record whose slots decide a cell: an identity's own, or a function's signature.
    ///
    /// # Safety
    /// `id` must be null or a dyad from the store.
    unsafe fn dispatch_record(&self, id: DyadPtr) -> Option<DyadPtr> {
        self.identity_head(id).or_else(|| self.signature_of(id))
    }

    /// A function value's signature, the record of its parameters, which the value holds where
    /// its type would; stand-in for #474.
    ///
    /// # Safety
    /// `id` must be null or a dyad from the store.
    unsafe fn signature_of(&self, id: DyadPtr) -> Option<DyadPtr> {
        if id.is_null() || dyad::ty(id) != self.types.fn_type {
            return None;
        }
        let record = *(dyad::value(id) as *const DyadPtr).add(super::FN_INPUT);
        (!record.is_null()).then_some(record)
    }

    /// `id` when it is an identity carrying a record, the type under its gates for a gated type,
    /// or `None` for null, a value, an instance, a place: the one test the dispatch readers share.
    ///
    /// # Safety
    /// `id` must be null or a dyad from the store.
    pub(super) unsafe fn identity_head(&self, id: DyadPtr) -> Option<DyadPtr> {
        let (id, _) = self.gated_type_parts(id);
        if id.is_null() || crate::identities::meta::kind_of(id).is_none() {
            None
        } else {
            Some(id)
        }
    }

    /// Whether `value`, placed on the tape, stands unconstructed for the driver's next turn, as a
    /// name holding it does (›A name of a type with an instances' `parse` wakes it, as the
    /// instance does, and so does any expression of the type‹ [[R230]]): an instance whose type
    /// fills `parse`, or a type with a constructor that an expression `gave`.
    fn wakes(&self, value: DyadPtr, gave: bool) -> bool {
        // SAFETY: `value` is null or a reduced dyad from the store.
        self.is_awake_instance(value)
            || (gave
                && unsafe { self.identity_head(value) }.is_some()
                && self.ctor_of(value).is_some())
    }

    /// A node its type's `parse` built, standing on the tape, or any expression known at
    /// parse to yield one (a name, a parameter, a call's result), whose type fills `parse`.
    /// DESIGN ›A type body describes one level‹.
    fn is_awake_instance(&self, id: DyadPtr) -> bool {
        self.awake_type(id).is_some()
    }

    /// The type whose `parse` the value in the cell wakes.
    fn awake_type(&self, id: DyadPtr) -> Option<DyadPtr> {
        use crate::identities::meta;
        // SAFETY: `id` is null or a resolved dyad from the store; the trio is read only on a record type.
        unsafe {
            if id.is_null() {
                return None;
            }
            let ty = dyad::ty(id);
            let fn_type = self.types.fn_type;
            let wakes = |t: DyadPtr| {
                meta::is_record_type(t) && !meta::written_parse_of(t, fn_type).is_null()
            };
            // A node of a type with a run stands for the value its run yields.
            if wakes(ty) && meta::run_body_of(ty).is_null() {
                return Some(ty);
            }
            if ty == self.types.fn_type || ty == self.types.type_ {
                return None;
            }
            crate::identities::node_output(self.types, id).filter(|&t| wakes(t))
        }
    }

    /// The place of `id` on the one axis, its record's `parse_rank`, or `prec::INERT` for a cell
    /// that wakes nothing.
    pub(super) fn precedence_of_cell(&self, id: DyadPtr) -> f64 {
        if let Some(t) = self.awake_type(id) {
            // SAFETY: `awake_type` found `t` a record type.
            return unsafe { crate::identities::meta::parse_rank_of(t) };
        }
        // SAFETY: `id` is null or a resolved dyad from the store.
        unsafe {
            match self.dispatch_record(id) {
                Some(id) => crate::identities::meta::parse_rank_of(id),
                None => crate::identities::meta::prec::INERT,
            }
        }
    }

    /// Its record's, or its type's for a value running its type's parse.
    fn assoc_of_cell(&self, id: DyadPtr) -> Assoc {
        if let Some(t) = self.awake_type(id) {
            // SAFETY: `awake_type` found `t` a record type.
            return unsafe { crate::identities::meta::assoc_of(t) };
        }
        // SAFETY: `id` is null or a resolved dyad from the store.
        unsafe {
            match self.dispatch_record(id) {
                Some(id) => crate::identities::meta::assoc_of(id),
                None => Assoc::Left,
            }
        }
    }

    /// Settle `construct`'s outcome on the cell at the cursor, judged by its
    /// handle whatever the center is after (DESIGN ›The scope's constructor is
    /// the driver‹): constructed, gone, handed on, standing as its value, or the checked error.
    pub(super) fn run_ctor(
        &mut self,
        construct: ConstructFn,
        id: DyadPtr,
        tape: &mut ParsingTape,
        discovery: bool,
    ) -> Result<(), ParseError> {
        // A word built as it is lexed reads what is written before it, so the brackets held there
        // are built first, in order; at the boundary a word takes the nearest node on each side,
        // and a bracket only the one to its left.
        if discovery {
            let at = tape.at(0).map_or(usize::MAX, |c| c.start);
            self.take_held_before(tape, at)?;
        } else {
            self.take_held(tape, -1)?;
            if !self.is_opener(id) {
                self.take_held(tape, 1)?;
            }
        }
        let own = tape.mark();
        let edits = tape.edits();
        let taken = tape.taken_mark();
        let (built_as, gates) = self.gated_type_parts(id);
        let logos = self.is_logos_ctor(built_as);
        // Dispatching the cell's identity is the body's read of its name: a
        // callee, an operator, a keyword alike.
        if let Some(c) = tape.at(0) {
            let binding = c.binding(self.types);
            self.note_outer_read(binding);
        }
        let was = std::mem::replace(&mut self.cx.discovering, discovery);
        let reader = std::mem::replace(&mut self.cx.reader, built_as);
        let outcome = construct(self, built_as, tape);
        self.cx.reader = reader;
        self.cx.discovering = was;
        let outcome = outcome?;
        let taken = tape.taken_since(taken);
        // Removed itself: the splice is the outcome.
        let Some(mut cell) = tape.cell_at_mark(own).copied() else {
            return Ok(());
        };
        tape.restore(own);
        self.runs_moved_to(&taken, cell.start);
        // The driver's rule (DESIGN ›Execution is function application‹): flag
        // true, done; flag false and another identity, its turn; flag false and
        // the same identity, the checked error. Any constructor, built-in or written in Logos,
        // declines with `Decline` or an untouched frontier (›A constructor's outcome is read off
        // its own cell; no holding, no re-invocation‹ [[R261]]).
        let same = self.cell_identity(&cell) == id;
        if cell.constructed {
            let instance = same && self.is_awake_instance(built_as);
            if same && !instance {
                // The flag set on the untouched cell: the identity stands as
                // its own value.
                return Ok(());
            }
            // A Logos-written constructor placed its node, or an instance's
            // parse let it stand: the node's run is the function built for its field-type set.
            // SAFETY: a constructed cell's dyad is null or a node from the store.
            let record = !cell.dyad.is_null()
                && unsafe { crate::identities::meta::is_record_type(dyad::ty(cell.dyad)) };
            // A node a parse placed, of any type, as opposed to a name or a call's result.
            // SAFETY: `record` saw a node from the store.
            let built = logos && record && unsafe { !self.types.is_storage(cell.dyad) };
            if logos && record && (instance || built) {
                let spelling = cell.spelling().to_string();
                // SAFETY: the node is the one `run_logos_ctor` minted for `id`, or its mint's.
                let folded =
                    unsafe { self.resolve_specialization(cell.dyad, cell.start, &spelling)? };
                if let Some(lit) = folded {
                    let lit = self.gate_built(gates, lit, false, cell.start)?;
                    tape.place(lit);
                    return Ok(());
                }
            }
            // A node that runs a function's body is a use of every outer name
            // that body reads, checked where the node comes to exist, whichever
            // constructor built it; the error points at the construct's own token.
            if !cell.dyad.is_null() {
                // SAFETY: a constructed cell holds a node from the store.
                let callee = unsafe { dyad::ty(cell.dyad) };
                // SAFETY: `callee` is the resolved identity of the cell.
                if let Err(e) = unsafe { self.check_call_reads(callee) } {
                    self.cx.pos = cell.start;
                    return Err(e);
                }
            }
            let ran = self.run_type_where_built(tape)?.is_some();
            if ran {
                cell = *tape.at(0).expect("the cell stands at its mark");
            }
            // A node a parse built runs, and is never parsed again, though the value its run
            // yields is read by its own type's parse; a bracket waits for the segment's end.
            // SAFETY: `built` saw a node from the store.
            let reparsed =
                !ran && built && self.awake_type(cell.dyad) == Some(unsafe { dyad::ty(cell.dyad) });
            let gave = ran || matches!(outcome, Constructed::Gave);
            // A value a gate word read whole has had its turn.
            // SAFETY: a constructed cell's dyad is null or a node from the store.
            let gated = !gave
                && (!gates.is_null()
                    || unsafe { crate::identities::gate::is_value(self.types, cell.dyad) });
            if !same && !reparsed && !gated && !cell.bracket && self.wakes(cell.dyad, gave) {
                tape.set_constructed(0, false);
            }
            self.note_made(cell.dyad);
            self.note_runs(&cell);
            if !gates.is_null() {
                let built = self.gate_built(gates, cell.dyad, cell.type_value, cell.start)?;
                tape.at_mut(0).expect("the cell stands at its mark").dyad = built;
            }
            return Ok(());
        }
        if !same {
            // Handed on to another identity, which the driver constructs as
            // its own or reads as the use of its name; a value that is no identity has no turn to take.
            let next = self.cell_identity(&cell);
            if self.is_identity_node(next) || self.is_awake_instance(next) {
                return Ok(());
            }
            self.cx.pos = cell.start;
            return Err(ParseError::CellLeftUnconstructed(Box::new(cell.spelling().to_string())));
        }
        let declined = matches!(outcome, Constructed::Decline) || tape.edits() == edits;
        if !declined {
            self.cx.pos = cell.start;
            return Err(ParseError::CellLeftUnconstructed(Box::new(cell.spelling().to_string())));
        }
        // The identity stands as its own value: the use of its name, its binding; a gated type,
        // `mut i32` with nothing after it, is that gated type.
        let value = self.stand_as_value(tape, id);
        tape.place(value);
        Ok(())
    }

    /// `built`, the node a gated type's constructor built, with that type's `gates`: `mut pt(1, 2)`
    /// gives the node `pt(1, 2)` built gated `mut`. Null gates, or no node, give `built` as it is.
    fn gate_built(
        &mut self,
        gates: DyadPtr,
        built: DyadPtr,
        held: bool,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        if gates.is_null() || built.is_null() {
            return Ok(built);
        }
        // SAFETY: `gates` is a gate mint, and `built` a reduced dyad a constructor placed.
        let gated =
            unsafe { crate::identities::gate::give(self.rt.store, self.types, gates, built, held) };
        gated.map_err(|e| self.fail_at(at, e))
    }

    /// The bracket held at `k` of `tape`, built as the word beside it takes it: its lines are read
    /// from after its opener, and the lexer goes on from where it stood.
    pub(super) fn take_held(&mut self, tape: &mut ParsingTape, k: isize) -> Result<(), ParseError> {
        let Some(&cell) = tape.at(k) else { return Ok(()) };
        if cell.held.is_none() {
            return Ok(());
        }
        let id = self.cell_identity(&cell);
        let construct = self.ctor_of(id).expect("a held bracket's opener has a constructor");
        let center = tape.mark();
        let handle = tape.handle_at(k).expect("read above");
        tape.at_mut(k).expect("read above").held = None;
        tape.center_on(handle);
        let frontier = std::mem::replace(&mut self.cx.pos, cell.end());
        self.cut_at(tape, cell.start)?;
        self.run_ctor(construct, id, tape, false)?;
        self.ended_after(tape, cell.start)?;
        self.cx.pos = frontier;
        tape.restore(center);
        Ok(())
    }

    /// A name lexed on `tape` after `from`, where a bracket was built late: one the bracket moved
    /// or freed is dead there, as it would be had the bracket been built before it was lexed.
    fn ended_after(&mut self, tape: &ParsingTape, from: usize) -> Result<(), ParseError> {
        for (_, c) in tape.iter() {
            let binding = c.binding(self.types);
            // SAFETY: a lexed binding cell holds a binding dyad from the store.
            let ended = !binding.is_null()
                && c.start > from
                && unsafe { Binding::read(binding).is_dead() || Binding::has_ended_path(binding) };
            if ended {
                self.cx.pos = c.start;
                let name = c.spelling().to_string();
                return Err(ParseError::Resolve(ResolveError::Dead(name)));
            }
        }
        Ok(())
    }

    /// Every bracket held on `tape` that starts before `at`, built in written order.
    pub(super) fn take_held_before(
        &mut self,
        tape: &mut ParsingTape,
        at: usize,
    ) -> Result<(), ParseError> {
        let center = tape.mark();
        while let Some(n) = tape.first_held(at) {
            tape.center_on(n);
            let taken = self.take_held(tape, 0);
            tape.restore(center);
            taken?;
        }
        Ok(())
    }

    /// A `static` function of a type body, whose first parameter is the value it works on.
    ///
    /// # Safety
    /// `f` must be a resolved dyad from the store.
    pub(super) unsafe fn takes_this(&self, f: DyadPtr) -> bool {
        use crate::identities::{array, meta};
        if f.is_null() || dyad::ty(f) != self.types.fn_type {
            return false;
        }
        let input = *(dyad::value(f) as *const DyadPtr);
        if input.is_null() || !meta::is_record_type(input) {
            return false;
        }
        let Some(&first) = array::items(meta::record_fields_of(input)).first() else {
            return false;
        };
        // The receiver is the one parameter no name declares: no named binding of the
        // parameter scope is laid out at the frame's first offset.
        let scope = meta::record_scope_of(input);
        crate::identities::hole::type_in(first) == self.types.dyad_
            && !self.trie.bindings_in(scope).iter().any(|&b| {
                let b = Binding::read(b);
                b.frame == f && b.offset == 0
            })
    }

    /// A `fn` node in the constructor slot: judged by the flag alone.
    pub(super) fn is_logos_ctor(&self, id: DyadPtr) -> bool {
        use crate::identities::meta;
        self.is_awake_instance(id)
            // SAFETY: `id` is a resolved dyad from the store; the slot is read only where a record head exists.
            || unsafe {
                meta::is_record_type(id)
                    && !meta::written_parse_of(id, self.types.fn_type).is_null()
            }
    }

    /// An identity a cell may be handed on to: a function value, or a type
    /// node with a record head.
    pub(super) fn is_identity_node(&self, d: DyadPtr) -> bool {
        // SAFETY: `d` is null or a resolved dyad from the store.
        unsafe {
            !d.is_null()
                && (dyad::ty(d) == self.types.fn_type
                    || crate::identities::meta::kind_of(d).is_some())
        }
    }

    /// The one sequencing step, shared by `parse_sequence`, the drivers and a
    /// type body: a comment node or one expression, an optional `,` after it
    /// consumed. `None` at the sequence's end. Where parse order is run order the item is left pending.
    pub fn parse_next(&mut self) -> Option<Result<DyadPtr, ParseError>> {
        self.next_cell().map(|item| item.map(|c| c.dyad))
    }

    /// Whether the cell at `tape`'s center starts the line being read: it stands on the line's own
    /// tape, with nothing but comments to its left.
    pub(crate) fn starts_line(&self, tape: &ParsingTape) -> bool {
        std::ptr::eq(tape, self.cx.line_tape)
            && (1..)
                .map_while(|k| tape.at(-k))
                // SAFETY: a cell on the tape holds a dyad from the store.
                .all(|c| unsafe { crate::identities::numtype::is_comment_type(dyad::ty(c.dyad)) })
    }

    /// [`Self::parse_next`] as the cell, its facts with it.
    pub(super) fn next_cell(&mut self) -> Option<Result<Cell, ParseError>> {
        loop {
            // What the last segment yielded, its expression and the prose
            // lifted out of it, goes out first.
            if let Some(item) = self.cx.queued.pop_front() {
                // Where parse order is run order the item is pending: it runs
                // when the pass needs a value, or when its scope or the call
                // that takes it runs. A loop's condition runs on every pass instead.
                if self.cx.runtime_depth == 0 && !self.cx.each_pass {
                    let open = self.cx.open.last_mut().expect("the root scope is open");
                    open.unrun.push(UnrunLine::new(item.dyad, open.lines));
                }
                return Some(Ok(item));
            }
            self.skip_whitespace();
            let bare_branch = self.cx.else_ends == Some(self.cx.open.len());
            if self.cx.pos >= self.cx.source.len()
                || self.at_close()
                || (bare_branch && self.at_branch_end())
            {
                return None;
            }
            let waits = self
                .waiting_line_end(false)
                .and_then(|end| end.map(|e| self.hold_line(e, false)).transpose());
            match waits {
                Ok(Some(held)) => {
                    // SAFETY: `held` holds the line `hold_line` just built.
                    unsafe { self.fill_if_sibling_write(held.dyad) };
                    if !bare_branch {
                        self.consume_separator();
                    }
                    self.cx.queued.push_back(held);
                    continue;
                }
                Ok(None) => {}
                Err(e) => return Some(Err(e)),
            }
            self.note_cuts();
            let mut tape = ParsingTape::new();
            self.start_from_kept(&mut tape);
            let boundary = match self.lex_segment(&mut tape) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            self.cx.line_tape = &tape;
            let Built { items, before, after } = match self.construct_segment(&mut tape) {
                Ok(built) => built,
                Err(e) => return Some(Err(e)),
            };
            // The `,` is consumed once the segment before it is constructed; a
            // `,` where nothing stands is purely for the reader.
            if matches!(boundary, Boundary::Comma) && !bare_branch {
                self.consume_separator();
            }
            if items.is_empty() {
                // A line its constructors took whole runs nothing.
                self.cx.open.last_mut().expect("a line is read in an open scope").runs.clear();
            }
            if items.len() > 1 {
                self.cx.pos = items[1].1;
                return Some(Err(self.leftover_error(items[0].0.dyad)));
            }
            if let Some(&(item, _)) = items.first() {
                // SAFETY: `item` is a reduced dyad just constructed.
                unsafe { self.fill_if_sibling_write(item.dyad) };
            }
            let open = self.cx.open.last_mut().expect("a line is read in an open scope");
            let handed = std::mem::take(&mut open.prose);
            // SAFETY: a lifted comment cell holds a node from the store.
            let prose = |d: DyadPtr| unsafe { Cell::built(d) };
            self.cx.queued.extend(before.into_iter().map(prose));
            self.cx.queued.extend(items.into_iter().map(|(item, _)| item));
            self.cx.queued.extend(handed.into_iter().chain(after).map(prose));
        }
    }

    /// The bracket to the right of `id`, read and handed to `build` with `id`: what a function
    /// value's call and a record type's derived constructor share (›`X (…)` is one spelling, and
    /// X's constructor decides what the bracket is‹ [[R270]]); without a `(` directly ahead the
    /// identity stands as its own value.
    pub(crate) fn construct_application(
        &mut self,
        id: DyadPtr,
        tape: &mut ParsingTape,
        build: unsafe fn(&mut Self, DyadPtr, &[DyadPtr]) -> Result<DyadPtr, ParseError>,
    ) -> Result<Constructed, ParseError> {
        // A type still being defined has no record yet: its name stands as itself, the
        // bracket to its right not in reach.
        // SAFETY: `id` is a reduced dyad from the store.
        let unfinished = unsafe {
            let node = self.types.through(id);
            dyad::ty(node) == self.types.type_ && dyad::head(node).is_null()
        };
        if unfinished {
            return Ok(Constructed::Decline);
        }
        // The bracket, lexed on demand: application runs at discovery.
        if let Some(scope) =
            self.read_arguments(tape, 1)?.filter(|c| c.is_bracket()).map(|c| c.dyad)
        {
            // SAFETY: `scope` is the bracket's node from the store.
            let args = unsafe { self.args_of(scope) };
            let cell = *tape.at(0).expect("the callee's own cell");
            // SAFETY: `id` is the resolved callee, a reduced dyad.
            let args = unsafe { self.with_receiver(id, &cell, args) }?;
            // A call for a type runs where it is built, so what stands before it runs first.
            // SAFETY: `id` is the resolved callee.
            if self.in_pass_order() && unsafe { self.returns_type(id) } {
                self.run_first(tape, cell.start)?;
            }
            // SAFETY: `id` is the resolved callee and `args` reduced dyads.
            let node = unsafe { build(self, id, &args) }?;
            tape.remove(1);
            tape.place(node);
            return Ok(Constructed::Gave);
        }
        Ok(Constructed::Decline)
    }

    /// The scope's expressions in order (prose and teardown structure aside),
    /// a single expression standing alone as itself, an empty `()` as none
    /// (DESIGN ›A function's surface‹).
    ///
    /// # Safety
    /// `scope` must be a node from the store.
    pub(crate) unsafe fn args_of(&mut self, scope: DyadPtr) -> Vec<DyadPtr> {
        if dyad::ty(scope) != self.types.scope {
            return vec![scope];
        }
        let Some(items) = crate::identities::scope::exprs_of(scope) else {
            return vec![scope];
        };
        self.rt.note_lines_read(scope);
        let defer_ = self.types.defer_;
        items
            .iter()
            .copied()
            .filter(|&e| {
                !crate::identities::numtype::is_comment_type(dyad::ty(e)) && dyad::ty(e) != defer_
            })
            .collect()
    }

    /// `id`'s constructor over a fresh single-token tape, for a construct
    /// invoked from inside another constructor. `Ok(None)` when `id` has no
    /// constructor or it declined.
    pub(super) fn construct_leaf(
        &mut self,
        id: DyadPtr,
        start: usize,
        len: usize,
    ) -> Result<Option<DyadPtr>, ParseError> {
        let Some(construct) = self.construct_of(id) else {
            return Ok(None);
        };
        let mut tape = ParsingTape::new();
        // SAFETY: `self.cx.source` outlives the parse; `id` came from the index.
        tape.push(unsafe { Cell::lexed(id, self.cx.source, start, len) });
        match construct(self, id, &mut tape)? {
            Constructed::Placed | Constructed::Gave => {
                Ok(tape.cell(0).filter(|c| c.constructed).map(|c| c.dyad))
            }
            Constructed::Decline => Ok(None),
        }
    }

    /// `id`'s constructor over the bracket a reader already read after `id`'s spelling, on a fresh
    /// tape of the two: `cell` holds `id` as written, `start` and `len` its spelling. `Ok(None)`
    /// where `id` has no constructor or it declined.
    pub(super) fn construct_applied(
        &mut self,
        id: DyadPtr,
        cell: DyadPtr,
        (start, len): (usize, usize),
        bracket: Cell,
    ) -> Result<Option<DyadPtr>, ParseError> {
        let Some(construct) = self.ctor_of(id) else {
            return Ok(None);
        };
        let mut tape = ParsingTape::new();
        // SAFETY: `self.cx.source` outlives the parse; `cell` is a dyad from the store.
        tape.push(unsafe { Cell::lexed(cell, self.cx.source, start, len) });
        let own = tape.mark();
        tape.push(bracket);
        tape.restore(own);
        match construct(self, id, &mut tape)? {
            Constructed::Placed | Constructed::Gave => {
                Ok(tape.cell(0).filter(|c| c.constructed).map(|c| c.dyad))
            }
            Constructed::Decline => Ok(None),
        }
    }

    /// Decoded from the constructor-slot leaf: dispatch flows through the
    /// graph, no table anywhere. `None` for an undefined constructor.
    fn construct_of(&self, id: DyadPtr) -> Option<ConstructFn> {
        // SAFETY: a constructor leaf is minted from a `ConstructFn` under `seed-parse`, checked before the transmute.
        unsafe {
            let leaf = crate::identities::meta::constructor_of(id);
            if leaf.is_null() {
                return None;
            }
            if dyad::ty(leaf) == self.types.fn_type {
                return Some(logos_constructor);
            }
            // A leaf under another convention would be jumped to with the wrong signature.
            assert_eq!(
                crate::identities::callable::convention_of(leaf),
                self.types.conv_seed_parse,
                "a constructor leaf must carry the seed-parse convention"
            );
            let entry = crate::identities::callable::entry_of(leaf);
            Some(std::mem::transmute::<usize, ConstructFn>(entry))
        }
    }

    /// An appearance of X runs X's constructor field (DESIGN ›The constructor
    /// is a field‹): the function takes the tape by value, its handle, so the
    /// argument is the driver's own tape; a run error is `ConstructorFailed`.
    ///
    /// # Safety
    /// `f` must be a `fn` node and `owner` a type node, both from the store;
    /// the tape's cells must hold dyads from the store.
    pub(crate) unsafe fn run_logos_ctor(
        &mut self,
        f: DyadPtr,
        owner: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // SAFETY: `owner` is the record type whose slot holds `f`.
        let this = unsafe { crate::identities::this::empty_node(self.rt.store, owner) };
        let outer = self.rt.set_fresh_this(Some(this));
        // SAFETY: as this function's own contract; the node was just built.
        let out = unsafe { self.run_logos_body(f, this, tape) };
        self.rt.restore_fresh_this(outer);
        out
    }

    /// What the operation `written`, written in Logos, has the seed do around its parse, where
    /// both its operands stand built: over an `and` or `or` group it applies to each member and
    /// the results join (›A conjecture states a boolean, and calling it yields the other side of a
    /// fact‹ [[R31]]), and a pointer to its left steps through `native`, the version it replaced
    /// (stand-in for #462). `None` where its own parse runs.
    fn around_written(
        &mut self,
        written: DyadPtr,
        native: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Option<Constructed>, ParseError> {
        use crate::identities::{group, operand_of, Operand};
        let types = self.types;
        // Its parse reads the right side this way too.
        self.cell_at(tape, 1)?;
        let (Some(l), Some(r)) = (self.operand_now(tape, -1), self.operand_now(tape, 1)) else {
            return Ok(None);
        };
        // SAFETY: a built cell holds a reduced dyad from the store.
        if matches!(unsafe { operand_of(types, l) }, Operand::Pointer(_)) {
            let construct = self.construct_of(native).expect("a native version has a constructor");
            return construct(self, native, tape).map(Some);
        }
        for (k, side) in [(-1, l), (1, r)] {
            // SAFETY: as above.
            let Some((connective, a, b)) = (unsafe { group::members(types, side) }) else {
                continue;
            };
            let a = self.written_over(written, tape, k, a)?;
            let b = self.written_over(written, tape, k, b)?;
            let joined = group::join(self.rt.store, types, connective, a, b)?;
            tape.reduce_here(joined);
            return Ok(Some(Constructed::Placed));
        }
        Ok(None)
    }

    /// What the operand at `k` gives as an infix reads it, a name its binding, read without the
    /// checks the read makes; `None` where no operand stands there.
    fn operand_now(&self, tape: &ParsingTape, k: isize) -> Option<DyadPtr> {
        let cell = tape.at(k)?;
        if cell.constructed {
            return Some(cell.dyad);
        }
        if !self.is_operand_cell(cell) || cell.is_fresh(self.types) {
            return None;
        }
        let binding = cell.binding(self.types);
        Some(if binding.is_null() { self.cell_identity(cell) } else { binding })
    }

    /// The node `written`'s parse builds over `tape`'s operands, the one at `k` replaced by
    /// `member`.
    fn written_over(
        &mut self,
        written: DyadPtr,
        tape: &ParsingTape,
        k: isize,
        member: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let mut over = ParsingTape::new();
        for i in -1..=1 {
            let mut cell = *tape.at(i).expect("an operation stands between its operands");
            if i == k {
                cell.over(member);
            }
            over.push(cell);
        }
        over.set_cursor(1);
        let start = over.at(0).map_or(self.cx.pos, |c| c.start);
        self.run_ctor(logos_constructor, written, &mut over, self.cx.discovering)?;
        match over.at(0) {
            Some(c) if c.constructed => Ok(c.dyad),
            _ => {
                self.cx.pos = start;
                Err(ParseError::CellLeftUnconstructed(Box::new(
                    tape.spelling(0).unwrap_or("").to_string(),
                )))
            }
        }
    }

    /// A `parse` body run over the tape with `this` as its node, `tape[0]`.
    ///
    /// # Safety
    /// `f` must be a `fn` node over the hidden `tape`/node record and `this`
    /// a node from the store; the tape's cells must hold dyads from the store.
    unsafe fn run_logos_body(
        &mut self,
        f: DyadPtr,
        this: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // A constructor runs now, by nature, so what stands before it runs first, but its left
        // operand, which it reads as written (›How a Logos constructor builds its node‹ [[R362]]).
        let from = tape.at(-1).or(tape.at(0)).map_or(self.cx.pos, |c| c.start);
        self.run_first(tape, from)?;
        // Its run is a call: a use of every outer name its body reads.
        if let Err(e) = self.check_call_reads(f) {
            if let Some(c) = tape.at(0) {
                self.cx.pos = c.start;
            }
            return Err(e);
        }
        let handle = self.scalar_value(
            crate::identities::numtype::NumType::U64,
            tape as *mut ParsingTape as usize as i64,
        );
        let this_arg =
            self.scalar_value(crate::identities::numtype::NumType::U64, this as usize as i64);
        let call = build_call(self.rt.store, f, &[handle, this_arg]);
        // A `parse` gives `void!`, whose value lands in `given` (›Target: a constructor returns
        // `void!`, so a user's syntax error is a recoverable value‹ [[R142]]).
        // SAFETY: `f` is a `fn` node, its signature written.
        let output = unsafe { *(dyad::value(f) as *const DyadPtr).add(super::FN_OUTPUT) };
        // SAFETY: `output` is a type node from the store.
        let fallible = unsafe { crate::identities::union::void_value_of(self.types, output) };
        let width = fallible.map_or(0, |_| {
            // SAFETY: as above; `void!` is a union, laid out.
            unsafe { crate::identities::meta::record_size_of(output) as usize }
        });
        let mut given = vec![0u64; width.div_ceil(8)];
        // Inside the call, `caller.scope` reads the pass's position.
        let outer = self.rt.enter_constructor(tape);
        self.tape_hosts
            .push(super::Handed { outer: self.outer.len(), frames: self.rt.frame_depth() });
        // SAFETY: `call` was just built into the store; the tape and the store are reached only
        // through the natives until `run` returns; `given` holds a value of `output`.
        let out = unsafe {
            match fallible {
                Some(_) => self.run_on_pass_into(f, call, given.as_mut_ptr().cast()),
                None => self.run_on_pass(call),
            }
        };
        self.tape_hosts.pop();
        self.rt.leave_constructor(outer);
        // The tape lives for the call alone: once it returns, the handle names none, so the
        // store keeps no address of the parser's stack.
        // SAFETY: `handle` is the `u64` node just built, its value eight bytes.
        unsafe { std::ptr::write_unaligned(crate::dyad::value(handle) as *mut u64, 0) };
        match out {
            // SAFETY: `given` holds the `void!` the call gave.
            Ok(_) if fallible.is_some() => unsafe {
                self.read_given(output, given.as_ptr().cast())
            },
            Ok(_) => Ok(Constructed::Placed),
            // A cell lexed on demand failed to parse: its own error, where it stands.
            Err(crate::run::RunError::Parse(e)) => Err(*e),
            Err(e) => Err(ParseError::ConstructorFailed(Box::new(crate::report::run_message(&e)))),
        }
    }

    /// The `void!` a `parse` gave at `at`: its error, the diagnostic the driver records, shown as
    /// a raise nobody handles is, then ended as error.logos's `hops`.
    ///
    /// # Safety
    /// `at` must hold a value of `fallible`, a `void!`.
    unsafe fn read_given(
        &mut self,
        fallible: DyadPtr,
        at: *const u8,
    ) -> Result<Constructed, ParseError> {
        use crate::identities::{error, union};
        let types = self.types;
        let (_, err) = union::fallible_members(types, fallible).expect("a `void!` is a `T!`");
        let Some((_, room)) = union::held_member(fallible, at).filter(|&(m, _)| m == err) else {
            return Ok(Constructed::Placed);
        };
        let value = std::ptr::read_unaligned(room as *const i64);
        let shown: Result<_, crate::run::RunError> = self.hosted(|rt| {
            let shown = error::handed_on(rt, value as DyadPtr);
            crate::identities::drop_model::end_of_type(rt, types.error.value, value)?;
            shown
        });
        let message = crate::report::run_message(&shown.unwrap_or_else(|e| e));
        Err(ParseError::ConstructorFailed(Box::new(message)))
    }

    /// A Logos constructor's read past the frontier: the driver lexes on, as for a
    /// built-in reader (DESIGN ›The scope's constructor is the driver‹).
    ///
    /// # Safety
    /// `parser` must be the parser hosting the run, and `tape` the tape it handed the
    /// running constructor; both outlive the constructor's run, which is the only caller.
    unsafe fn lex_on(parser: *mut (), tape: *mut ParsingTape, k: isize) -> Result<(), ParseError> {
        // SAFETY: as this function's own contract.
        let (parser, tape) = unsafe { (&mut *(parser as *mut Self), &mut *tape) };
        parser.on_tape_host(|p| p.cell_at(tape, k).map(|_| ()))
    }

    /// A Logos constructor's `tape[k]`, `tape[k].type` or `tape[k].gives` ([`Parser::read_tape`]).
    ///
    /// # Safety
    /// As `lex_on`.
    unsafe fn read_tape_host(
        parser: *mut (),
        tape: *mut ParsingTape,
        k: isize,
        read: TapeRead,
    ) -> Result<i64, crate::run::RunError> {
        // SAFETY: as this function's own contract.
        let (parser, tape) = unsafe { (&mut *(parser as *mut Self), &mut *tape) };
        let node = parser.on_tape_host(|p| p.read_tape(tape, k, read));
        node.map(|d| d as i64).map_err(|e| crate::run::RunError::Parse(Box::new(e)))
    }

    /// `f` in the sub-parse and the frame the running constructor's tape was handed out in: a
    /// read made while a line of the constructor is parsed as it runs reads the tape's source,
    /// not that line's, and a place of the tape's line, a local of the call it runs in, is read
    /// in that call's frame, not the constructor's.
    fn on_tape_host<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        match self.tape_hosts.last().copied() {
            Some(handed) => f(&mut *TapeHost::new(self, handed).parser),
            None => f(self),
        }
    }

    /// `f` over the runtime with this parser as its host: the lexer is attached, so a
    /// `lex «…»` inside what runs resolves against the scopes open now and the index (DESIGN
    /// ›Text is the quote‹), and what runs reaches the parser for a held `type (…)`, a
    /// constructor, a waiting line and a read of a constructor's tape. The parser reads neither
    /// while the runtime runs.
    pub(super) fn hosted<R>(&mut self, f: impl FnOnce(&mut crate::run::Runtime) -> R) -> R {
        let host = crate::run::Host {
            parser: (self as *mut Self).cast(),
            lex_on: Self::lex_on,
            mint: Self::mint_host,
            construct: Self::construct_host,
            parse_waiting: Self::parse_waiting_host,
            read_tape: Self::read_tape_host,
            construct_tape: Self::construct_tape_host,
        };
        self.rt.hosting(&self.cx.scopes, self.trie, Some(host), f)
    }

    /// `node` run on the pass, hosted ([`Parser::hosted`]).
    ///
    /// # Safety
    /// `node` must be a valid dyad this parser built into its store.
    pub(super) unsafe fn run_on_pass(
        &mut self,
        node: DyadPtr,
    ) -> Result<i64, crate::run::RunError> {
        // SAFETY: `node` is a valid dyad in the store (the caller's contract).
        self.hosted(|rt| unsafe { rt.run(node) })
    }

    /// [`Parser::run_on_pass`] for a line of a scope: the value it gives the scope, `None`
    /// where it goes on without one.
    ///
    /// # Safety
    /// As [`Parser::run_on_pass`].
    pub(super) unsafe fn run_line_on_pass(
        &mut self,
        node: DyadPtr,
    ) -> Result<Option<i64>, crate::run::RunError> {
        // SAFETY: `node` is a valid dyad in the store (the caller's contract).
        self.hosted(|rt| unsafe { rt.run_line(node) })
    }

    /// [`Parser::run_on_pass`] for a call whose record result is copied into `dest`.
    ///
    /// # Safety
    /// `f` must be a `fn` node and `node` an application of it, both from the store;
    /// `dest` a place of the result's width.
    pub(super) unsafe fn run_on_pass_into(
        &mut self,
        f: DyadPtr,
        node: DyadPtr,
        dest: *mut u8,
    ) -> Result<i64, crate::run::RunError> {
        // SAFETY: the caller's contract.
        self.hosted(|rt| unsafe { rt.apply_into(f, node, Some(dest)) })
    }

    /// The runtime's way back into the parser running it: build a held `type (…)`.
    ///
    /// # Safety
    /// `parser` is the `Parser` whose `run_on_pass` is on the stack, its runtime
    /// inside the run that reached `node`, a held type node; the runtime is not
    /// read through the run's own reference until this returns.
    unsafe fn mint_host(parser: *mut (), node: DyadPtr) -> Result<i64, crate::run::RunError> {
        let p = &mut *parser.cast::<Self>();
        p.construct_held_type(node).map(|ty| ty as i64)
    }

    /// The runtime's way back into the parser running it: parse what waits for the run.
    ///
    /// # Safety
    /// As `mint_host`; `node` a node `identities::waiting` built.
    unsafe fn parse_waiting_host(
        parser: *mut (),
        node: DyadPtr,
    ) -> Result<i64, crate::run::RunError> {
        let p = &mut *parser.cast::<Self>();
        p.parse_waiting(node).map(|d| d as i64)
    }

    /// The runtime's way back into the parser running it: construct `ty` applied to `value`
    /// on a tape of their two cells, as the driver constructs a segment (DESIGN ›The
    /// scope's constructor is the driver‹).
    ///
    /// # Safety
    /// As `mint_host`; `ty` a type node and `value` a dyad from the store.
    unsafe fn construct_host(
        parser: *mut (),
        ty: DyadPtr,
        value: DyadPtr,
    ) -> Result<i64, crate::run::RunError> {
        let p = &mut *parser.cast::<Self>();
        let types = p.types;
        let bracket = dyad::ty(value) == types.scope || dyad::ty(value) == types.square_brackets;
        let builds = bracket
            && crate::identities::meta::is_record_type(ty)
            && !crate::identities::meta::bracket_builder_of(ty).is_null();
        let built = if builds {
            p.bracket_node(ty, value)
        } else {
            let mut head = Cell::built(ty);
            head.constructed = false;
            let mut arg = Cell::built(value);
            arg.bracket = bracket;
            p.construct_cells(vec![head, arg])
        };
        built.map(|d| d as i64).map_err(|e| crate::run::RunError::Parse(Box::new(e)))
    }

    fn construct_cells(&mut self, cells: Vec<Cell>) -> Result<DyadPtr, ParseError> {
        // These cells stand on no line: what they run, the node that asked for them runs.
        let depth = self.cx.open.len() - 1;
        let runs = std::mem::take(&mut self.cx.open[depth].runs);
        let built = self.construct_line(cells);
        if let Some(open) = self.cx.open.get_mut(depth) {
            open.runs = runs;
        }
        built.map(|c| c.dyad)
    }

    /// `cells` built as one line that ends after them, as the driver builds a segment: the one
    /// node they give, or the error a line giving none or more than one is.
    fn construct_line(&mut self, cells: Vec<Cell>) -> Result<Cell, ParseError> {
        let mut tape = ParsingTape::new();
        for cell in cells {
            tape.push(cell);
        }
        tape.center = tape.head;
        tape.sealed = true;
        let built = self.construct_segment(&mut tape)?;
        let items = self.hand_to_line(built);
        self.one_of(items)
    }

    /// A Logos constructor's `tape.construct(from, to)` ([`Parser::construct_range`]).
    ///
    /// # Safety
    /// As `lex_on`.
    unsafe fn construct_tape_host(
        parser: *mut (),
        tape: *mut ParsingTape,
        from: isize,
        to: isize,
    ) -> Result<(), crate::run::RunError> {
        // SAFETY: as this function's own contract.
        let (parser, tape) = unsafe { (&mut *(parser as *mut Self), &mut *tape) };
        let built = parser.on_tape_host(|p| p.construct_range(tape, from, to));
        built.map_err(|e| crate::run::RunError::Parse(Box::new(e)))
    }

    /// The nodes `from` up to, not including, `to` right of `tape`'s center, lexed already, built
    /// as one line that ends there, each by its own constructor; the one node they give stands
    /// at `from` (›A `tape[k]` read past the lexed frontier lexes on demand for every
    /// constructor‹ [[R231]]). They stand on the line being parsed, so what they run, it runs.
    pub(super) fn construct_range(
        &mut self,
        tape: &mut ParsingTape,
        from: isize,
        to: isize,
    ) -> Result<(), ParseError> {
        let cells: Vec<Cell> = (from..to).map_while(|k| tape.in_reach(k).copied()).collect();
        let taken = cells.len();
        let built = self.construct_line(cells)?;
        for _ in 1..taken {
            tape.remove(from + 1);
        }
        let placed = tape.set_cell(from, built);
        debug_assert!(placed, "a line of at least one node began at `from`");
        Ok(())
    }

    /// A square bracket taken into a place of a type of node values: the node of the run
    /// type that builds one, over the bracket; any other right side as it stands;
    /// stand-in for #152.
    ///
    /// # Safety
    /// `target` and `value` must be reduced dyads from the store.
    pub(crate) unsafe fn bracket_into(
        &mut self,
        target: DyadPtr,
        value: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        use crate::identities::read::{read_kind, Read};
        let types = self.types;
        if dyad::ty(types.through(value)) != types.square_brackets {
            return Ok(value);
        }
        match read_kind(types, types.through(target)) {
            Read::Container(t)
                if crate::identities::meta::is_record_type(t)
                    && !crate::identities::meta::bracket_builder_of(t).is_null() =>
            {
                self.bracket_node(t, types.through(value))
            }
            _ => Ok(value),
        }
    }

    /// stand-in for #152
    ///
    /// # Safety
    /// `ty` must be a record type with a bracket builder installed and `bracket` a bracket
    /// node from the store.
    unsafe fn bracket_node(
        &mut self,
        ty: DyadPtr,
        bracket: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let builder = crate::identities::meta::bracket_builder_of(ty);
        let element = self.bracket_element_type(ty).ok_or(ParseError::BadDeclaredType)?;
        let mut head = Cell::built(builder);
        head.constructed = false;
        let mut arg = Cell::built(bracket);
        arg.bracket = true;
        self.construct_cells(vec![head, Cell::built(element), arg])
    }
}
