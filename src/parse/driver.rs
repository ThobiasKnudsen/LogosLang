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

/// A constructor edits the tape in place; `Placed` reports only that it did.
/// `Decline` is "not mine": nothing consumed, nothing touched, and the driver
/// leaves the identity standing as its own value.
pub enum Constructed {
    Placed,
    Decline,
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

fn application(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    p.construct_application(id, tape)
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
        let Some((mut cell, start)) = self.lex_cell()? else {
            return Ok(Some(Boundary::Eof));
        };
        if !cell.constructed && !self.after_tight_read(tape) && self.names_own_field(&cell) {
            if self.cx.definitions.last().is_some_and(|d| d.in_parse) {
                self.cx.pos = start;
                return Err(ParseError::BareFieldInParse(Box::new(cell.spelling().to_string())));
            }
            // Left unbuilt, as a name is, so a value whose type has a parse wakes it: `items[1]`.
            cell.dyad = self.this_field(cell.spelling(), start)?;
        }
        let id = self.cell_identity(&cell);
        if !cell.constructed {
            self.check_unwritten(&cell)?;
            if id == self.types.sep_ {
                self.cx.pos = start;
                return Ok(Some(Boundary::Comma));
            }
            if id == self.types.close_ || id == self.types.close_sq_ {
                self.cx.pos = start;
                return Ok(Some(Boundary::Close));
            }
            if id == self.types.else_ && self.cx.else_ends == Some(self.cx.open.len()) {
                self.cx.pos = start;
                return Ok(Some(Boundary::Else));
            }
            if let Some(mode) = self.cx.lex_mode {
                if mode != RightSide::Condition && id == self.types.open_ && !tape.is_empty() {
                    // In a range a `(` after an unconstructed identity with
                    // a constructor is that identity's (DESIGN ›`X (…)` is one
                    // spelling, and X's constructor decides‹); a return type takes no bracket, so there the first `(` is the body.
                    let owner_pending = mode == RightSide::Range
                        && matches!(tape.last(), Some(l) if !self.is_operand_cell(l));
                    if !owner_pending {
                        self.cx.pos = start;
                        return Ok(Some(Boundary::Open));
                    }
                }
            }
        }
        if self.cx.lex_mode == Some(RightSide::Condition) && self.condition_ends(tape, Some(&cell))
        {
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
                    || self.tight_read_takes(id);
                if prec >= crate::identities::meta::prec::OPEN
                    && !asleep
                    && (!lazy
                        || crate::identities::meta::prec::built_as_lexed(prec)
                        || self.goes_before_reader(id, prec))
                {
                    self.run_ctor(construct, id, tape, true)?;
                }
            }
        }
        if !lazy {
            self.discover_pending(tape)?;
        }
        Ok(None)
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
            for (n, c) in tape.iter() {
                if c.constructed {
                    continue;
                }
                let id = self.cell_identity(c);
                let Some(construct) = self.ctor_of(id) else { continue };
                let prec = self.precedence_of_cell(id);
                if prec < crate::identities::meta::prec::OPEN {
                    continue;
                }
                let reader = prec == crate::identities::meta::prec::READER
                    || prec == crate::identities::meta::prec::DECLARE;
                let asleep = (self.cx.lex_mode == Some(RightSide::ReturnType) && reader)
                    || self.tight_read_takes(id);
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
        Ok(tape.at(offset).copied())
    }

    /// At the boundary: comment cells lifted out, then the unconstructed cells
    /// highest parse_rank first, associativity breaking ties, no lookahead;
    /// what remains is read as operands. Returns the cells in order with their offsets.
    pub(super) fn construct_segment(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Vec<(Cell, usize)>, ParseError> {
        let comment_ = self.types.comment_;
        let prose: Vec<(usize, usize, DyadPtr)> = tape
            .iter()
            // SAFETY: a constructed cell holds a node from the store.
            .filter(|(_, c)| c.constructed && unsafe { dyad::ty(c.dyad) } == comment_)
            .map(|(n, c)| (n, c.start, c.dyad))
            .collect();
        for (n, start, d) in prose {
            self.cx.lifted.push((start, d));
            tape.center_on(n);
            tape.remove(0);
        }
        loop {
            let mut best: Option<(usize, f64, ConstructFn, DyadPtr)> = None;
            for (n, c) in tape.iter() {
                if c.constructed {
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
                // A bracket no identity claimed, standing before another cell, is an
                // expression like any other: its value wakes its type's `parse`.
                let last = tape.iter().last().map(|(n, _)| n);
                let woken = tape.iter().find(|&(n, c)| {
                    Some(n) != last && c.bracket && self.is_awake_instance(self.cell_identity(c))
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
        Ok(items)
    }

    /// The cells up to the next `(`, the right side an identity reads before
    /// its bracket: a condition, a range, a return type (DESIGN ›The scope's
    /// constructor is the driver‹). A `(` standing first is part of the read; a later one is the bracket.
    pub(super) fn drive_until_open(
        &mut self,
        mode: RightSide,
    ) -> Result<Vec<(Cell, usize)>, ParseError> {
        let mut tape = ParsingTape::new();
        self.lex_segment_until(&mut tape, Some(mode))?;
        self.construct_segment(&mut tape)
    }

    /// One segment, lexed to the next `,`, `)`, or end of input (left
    /// unconsumed) and constructed to exactly one cell: what a discovery-time
    /// constructor drives, and the REPL's line.
    pub fn parse_expression(&mut self) -> Result<DyadPtr, ParseError> {
        self.parse_expression_cell().map(|c| c.dyad)
    }

    /// [`Self::parse_expression`] with the cell's facts, for the reader that consumes them.
    pub(crate) fn parse_expression_cell(&mut self) -> Result<Cell, ParseError> {
        let mut tape = ParsingTape::new();
        self.lex_segment(&mut tape)?;
        let items = self.construct_segment(&mut tape)?;
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

    /// Usually `Trailing`; where the first leftover is a box holding a node,
    /// the real reason is that the box could not say what it holds (a deferred
    /// body), so `a 5` never juxtaposed.
    pub(super) fn leftover_error(&self, first: DyadPtr) -> ParseError {
        // SAFETY: `first` is a constructed node from the store.
        let node_box = unsafe {
            let d = self.types.through(first);
            let t = self.types.type_of(d);
            !d.is_null()
                && ((t == self.types.type_ || t == self.types.dyad_) && self.types.is_storage(d)
                    || !crate::identities::is_type_value(self.types, d)
                        && crate::identities::yields_type(self.types, d))
        };
        if node_box {
            ParseError::TypeKnownOnlyAtRun
        } else {
            ParseError::Trailing
        }
    }

    /// `None` for an inert cell (a value, a delimiter). The identity's own
    /// slot first; failing that, its type's shared instance constructor,
    /// application (DESIGN ›The constructor is a field‹).
    pub(super) fn ctor_of(&self, id: DyadPtr) -> Option<ConstructFn> {
        // SAFETY: `id` is a resolved dyad from the store.
        unsafe {
            if !id.is_null() && dyad::ty(id) == self.types.fn_type {
                return Some(application);
            }
            if self.is_awake_instance(id) {
                return Some(instances_constructor);
            }
            let id = self.identity_head(id)?;
            // A record type runs the constructor its body filled, or the
            // derived one: application, the instance construction.
            if crate::identities::meta::is_record_type(id) {
                return self.construct_of(id).or(Some(application));
            }
            self.construct_of(id)
        }
    }

    /// `id` when it is an identity carrying a record, or `None` for null, a
    /// value, an instance, a place: the one test the dispatch readers share.
    ///
    /// # Safety
    /// `id` must be null or a dyad from the store.
    pub(super) unsafe fn identity_head(&self, id: DyadPtr) -> Option<DyadPtr> {
        if id.is_null()
            || dyad::ty(id) != self.types.type_
            || crate::identities::meta::kind_of(id).is_none()
        {
            None
        } else {
            Some(id)
        }
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
            let wakes = |t: DyadPtr| meta::is_record_type(t) && !meta::constructor_of(t).is_null();
            // A node of a type with a run stands for the value its run yields.
            if wakes(ty) && meta::run_body_of(ty).is_null() {
                return Some(ty);
            }
            if ty == self.types.fn_type || ty == self.types.type_ {
                return None;
            }
            crate::identities::node_type_of(self.types, id).filter(|&t| wakes(t))
        }
    }

    /// The place of `id` on the one axis (its record's parse_rank), or
    /// `prec::APPLY` for a cell that carries no record.
    pub(super) fn precedence_of_cell(&self, id: DyadPtr) -> f64 {
        if let Some(t) = self.awake_type(id) {
            // SAFETY: `awake_type` found `t` a record type.
            return unsafe { crate::identities::meta::parse_rank_of(t) };
        }
        // SAFETY: `id` is null or a resolved dyad from the store.
        unsafe {
            match self.identity_head(id) {
                Some(id) => crate::identities::meta::parse_rank_of(id),
                None => crate::identities::meta::prec::APPLY,
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
            match self.identity_head(id) {
                Some(id) => crate::identities::meta::assoc_of(id),
                None => Assoc::Left,
            }
        }
    }

    /// Settle `construct`'s outcome on the cell at the cursor, judged by its
    /// handle whatever the center is after (DESIGN ›The scope's constructor is
    /// the driver‹): constructed, gone, handed on, standing as its value, or the checked error.
    fn run_ctor(
        &mut self,
        construct: ConstructFn,
        id: DyadPtr,
        tape: &mut ParsingTape,
        discovery: bool,
    ) -> Result<(), ParseError> {
        let own = tape.mark();
        let edits = tape.edits();
        let logos = self.is_logos_ctor(id);
        // Dispatching the cell's identity is the body's read of its name: a
        // callee, an operator, a keyword alike.
        if let Some(c) = tape.at(0) {
            let binding = c.binding(self.types);
            self.note_outer_read(binding);
        }
        let was = std::mem::replace(&mut self.cx.discovering, discovery);
        let reader = std::mem::replace(&mut self.cx.reader, id);
        let outcome = construct(self, id, tape);
        self.cx.reader = reader;
        self.cx.discovering = was;
        let outcome = outcome?;
        // Removed itself: the splice is the outcome.
        let Some(cell) = tape.cell_at_mark(own).copied() else {
            return Ok(());
        };
        tape.restore(own);
        // The driver's rule (DESIGN ›Execution is function application‹): flag
        // true, done; flag false and another identity, its turn; flag false and
        // the same identity, the checked error. A Rust constructor declines with `Decline` or an untouched frontier.
        let same = self.cell_identity(&cell) == id;
        if cell.constructed {
            let instance = same && self.is_awake_instance(id);
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
                // SAFETY: the node is the one `run_logos_ctor` minted for `id`, `[field…, null, spec]`.
                let folded =
                    unsafe { self.resolve_specialization(cell.dyad, cell.start, &spelling)? };
                // A comptime node folded: its literal stands in the cell, the node it came
                // from beside it for `.`.
                if let Some(lit) = folded {
                    let origin = cell.dyad;
                    tape.place(lit);
                    tape.at_mut(0).expect("placed above").origin = origin;
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
            // A call that yields a node whose type has a `parse` wakes it as a name does,
            // in the driver's next turn; a node a parse built runs, and is never parsed again,
            // though the value its run yields is read by its own type's parse.
            // SAFETY: `built` saw a node from the store.
            let reparsed =
                built && self.awake_type(cell.dyad) == Some(unsafe { dyad::ty(cell.dyad) });
            if !same && !reparsed && !cell.bracket && self.is_awake_instance(cell.dyad) {
                tape.set_constructed(0, false);
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
        let declined = matches!(outcome, Constructed::Decline) || (!logos && tape.edits() == edits);
        if !declined {
            self.cx.pos = cell.start;
            return Err(ParseError::CellLeftUnconstructed(Box::new(cell.spelling().to_string())));
        }
        // The identity stands as its own value: the use of its name, its binding.
        let value = self.stand_as_value(tape, id);
        tape.place(value);
        Ok(())
    }

    /// A `share` function of a type body, whose first parameter is the value it works on.
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
        // SAFETY: `id` is a resolved dyad from the store; the slot is read only where a record head exists.
        self.is_awake_instance(id)
            || unsafe {
                !id.is_null()
                    && dyad::ty(id) == self.types.type_
                    && crate::identities::meta::kind_of(id).is_some()
                    && crate::identities::meta::is_record_type(id)
                    && {
                        let leaf = crate::identities::meta::constructor_of(id);
                        !leaf.is_null() && dyad::ty(leaf) == self.types.fn_type
                    }
            }
    }

    /// An identity a cell may be handed on to: a function value, or a type
    /// node with a record head.
    fn is_identity_node(&self, d: DyadPtr) -> bool {
        // SAFETY: `d` is null or a resolved dyad from the store.
        unsafe {
            !d.is_null()
                && (dyad::ty(d) == self.types.fn_type
                    || (dyad::ty(d) == self.types.type_
                        && crate::identities::meta::kind_of(d).is_some()))
        }
    }

    /// The one sequencing step, shared by `parse_sequence`, the drivers and a
    /// type body: a comment node or one expression, an optional `,` after it
    /// consumed. `None` at the sequence's end. Where parse order is run order the item is left pending.
    pub fn parse_next(&mut self) -> Option<Result<DyadPtr, ParseError>> {
        self.next_cell().map(|item| item.map(|c| c.dyad))
    }

    /// [`Self::parse_next`] as the cell, its facts with it.
    pub(super) fn next_cell(&mut self) -> Option<Result<Cell, ParseError>> {
        loop {
            // What the last segment yielded, its expression and the prose
            // lifted out of it, goes out first.
            if let Some(item) = self.cx.queued.pop_front() {
                // Where parse order is run order the item is pending: it runs
                // when the pass needs a value or when its scope runs. A list's
                // items go whole to the call that takes them.
                if self.cx.runtime_depth == 0 {
                    let open = self.cx.open.last_mut().expect("the root scope is open");
                    if !open.list {
                        open.unrun.push(item.dyad);
                    }
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
            let mut tape = ParsingTape::new();
            let boundary = match self.lex_segment(&mut tape) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let mut items = match self.construct_segment(&mut tape) {
                Ok(items) => items,
                Err(e) => {
                    self.cx.lifted.clear();
                    return Some(Err(e));
                }
            };
            // The `,` is consumed once the segment before it is constructed; a
            // `,` where nothing stands is purely for the reader.
            if matches!(boundary, Boundary::Comma) && !bare_branch {
                self.consume_separator();
            }
            if items.len() > 1 {
                self.cx.pos = items[1].1;
                self.cx.lifted.clear();
                return Some(Err(ParseError::Trailing));
            }
            if let Some(&(item, _)) = items.first() {
                // SAFETY: `item` is a reduced dyad just constructed.
                unsafe { self.fill_if_sibling_write(item.dyad) };
            }
            let mut ordered: Vec<(usize, Cell)> = std::mem::take(&mut self.cx.lifted)
                .into_iter()
                // SAFETY: a lifted comment cell holds a node from the store.
                .map(|(start, d)| (start, unsafe { Cell::built(d) }))
                .collect();
            if let Some((item, start)) = items.pop() {
                ordered.push((start, item));
            }
            ordered.sort_by_key(|&(start, _)| start);
            self.cx.queued.extend(ordered.into_iter().map(|(_, n)| n));
        }
    }

    /// The constructor a `fn`, a record type, or a numeric type runs for the
    /// bracket to its right (DESIGN ›`X (…)` is one spelling, and X's
    /// constructor decides what the bracket is‹); without a `(` directly ahead
    /// the identity stands as its own value.
    pub(crate) fn construct_application(
        &mut self,
        id: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // A type still being defined has no record yet: its name stands as itself, the
        // bracket to its right not in reach.
        // SAFETY: `id` is a reduced dyad from the store.
        let unfinished = unsafe {
            let node = self.types.through(id);
            dyad::ty(node) == self.types.type_ && dyad::head(node).is_null()
        };
        if unfinished {
            let value = self.stand_as_value(tape, id);
            tape.place(value);
            return Ok(Constructed::Placed);
        }
        // The bracket, lexed on demand: application runs at discovery.
        if let Some(scope) = self.cell_at(tape, 1)?.filter(|c| c.is_bracket()).map(|c| c.dyad) {
            // SAFETY: `scope` is the bracket's node from the store.
            let args = unsafe { self.args_of(scope) };
            // SAFETY: `id` is the resolved callee, a reduced dyad.
            let args = unsafe { self.with_receiver(id, args) }?;
            // SAFETY: `id` is the resolved callee and `args` reduced dyads.
            let node = unsafe { self.build_call(id, &args) }?;
            tape.remove(1);
            tape.place(node);
        } else {
            let value = self.stand_as_value(tape, id);
            tape.place(value);
        }
        Ok(Constructed::Placed)
    }

    /// The scope's expressions in order (prose and teardown structure aside),
    /// a single expression standing alone as itself, an empty `()` as none
    /// (DESIGN ›A function's surface‹).
    ///
    /// # Safety
    /// `scope` must be a node from the store.
    pub(crate) unsafe fn args_of(&self, scope: DyadPtr) -> Vec<DyadPtr> {
        if dyad::ty(scope) != self.types.scope {
            return vec![scope];
        }
        let Some(items) = crate::identities::scope::exprs_of(scope) else {
            return vec![scope];
        };
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
            Constructed::Placed => Ok(tape.cell(0).filter(|c| c.constructed).map(|c| c.dyad)),
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
        // A constructor runs now, by nature; what stands before it runs first.
        self.drain()?;
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
        // Inside the call, `caller.scope` reads the pass's position.
        let outer = self.rt.enter_constructor(tape);
        // SAFETY: `call` was just built into the store; the tape and the store are reached only through the natives until `run` returns.
        let out = unsafe { self.run_on_pass(call) };
        self.rt.leave_constructor(outer);
        match out {
            Ok(_) => Ok(Constructed::Placed),
            // A cell lexed on demand failed to parse: its own error, where it stands.
            Err(crate::run::RunError::Parse(e)) => Err(*e),
            Err(e) => Err(ParseError::ConstructorFailed(Box::new(crate::report::run_message(&e)))),
        }
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
        parser.cell_at(tape, k).map(|_| ())
    }

    /// With the lexer attached: a `lex «…»` inside what runs resolves against
    /// the scopes open now and the index (DESIGN ›Text is the quote‹). The
    /// parser reads neither while the runtime runs.
    ///
    /// # Safety
    /// `node` must be a valid dyad this parser built into its store.
    pub(super) unsafe fn run_on_pass(
        &mut self,
        node: DyadPtr,
    ) -> Result<i64, crate::run::RunError> {
        let host = crate::run::Host {
            parser: (self as *mut Self).cast(),
            lex_on: Self::lex_on,
            mint: Self::mint_host,
            construct: Self::construct_host,
        };
        // SAFETY: `node` is a valid dyad in the store (the caller's contract).
        self.rt.hosting(&self.cx.scopes, self.trie, Some(host), |rt| unsafe { rt.run(node) })
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
        let host = crate::run::Host {
            parser: (self as *mut Self).cast(),
            lex_on: Self::lex_on,
            mint: Self::mint_host,
            construct: Self::construct_host,
        };
        // SAFETY: the caller's contract.
        self.rt.hosting(&self.cx.scopes, self.trie, Some(host), |rt| unsafe {
            rt.apply_into(f, node, Some(dest))
        })
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
        let mut tape = ParsingTape::new();
        for cell in cells {
            tape.push(cell);
        }
        tape.center = tape.head;
        tape.sealed = true;
        self.construct_segment(&mut tape).and_then(|items| self.one_of(items)).map(|c| c.dyad)
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
