// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Operands: the reading rule a constructor takes its cells through, the left side of
//! `=`, a prefix word's operand, and the capture and liveness checks on a name's use.

use super::*;
use crate::dyad;

impl<'a> Parser<'a> {
    /// `fn`'s constructor claims it so a recursive self-call inside the body
    /// resolves the published signature.
    pub(crate) fn take_pending_binding(&mut self) -> DyadPtr {
        std::mem::replace(&mut self.cx.pending_binding, std::ptr::null_mut())
    }

    /// `fn`'s constructor suppresses the handoff around a literal that does
    /// not open its expression, so a grouped literal deeper in the same
    /// declaration can still claim it.
    pub(crate) fn restore_pending_binding(&mut self, pending: DyadPtr) {
        self.cx.pending_binding = pending;
    }

    /// A reduced dyad, or a token that does not extend: a resolved operand or
    /// a fresh name in waiting. A pending extender token is not an operand.
    pub(crate) fn is_operand_cell(&self, cell: &Cell) -> bool {
        match cell {
            c if c.constructed => true,
            // A resolved token is an operand only when nothing would construct
            // it and it is not a bare delimiter, which no operator takes.
            c => {
                let id = self.cell_identity(c);
                self.ctor_of(id).is_none() && !self.is_delimiter(id)
            }
        }
    }

    /// An identity whose record is a parse-only token, of which no value exists: `)`, `,`, `->`,
    /// `else`, `in`, `..`, read by the constructs that spell them, and words such as `lossy`.
    /// Never an operand, and never a type.
    pub(super) fn is_delimiter(&self, id: DyadPtr) -> bool {
        // SAFETY: `id` is null or a resolved identity from the store.
        unsafe {
            !id.is_null()
                && dyad::ty(id) == self.types.type_
                && crate::identities::meta::kind_of(id) == Some(crate::identities::meta::TOKEN_TAG)
        }
    }

    /// A comment `#` built: value rules read through it (›Text literals are plain values; `#`
    /// is the one comment constructor‹ [[R160]]).
    pub(super) fn is_comment_cell(&self, cell: &Cell) -> bool {
        // SAFETY: a cell's dyad is null or a dyad from the store.
        cell.constructed
            && !cell.dyad.is_null()
            && unsafe { dyad::ty(cell.dyad) } == self.types.comment_
    }

    /// Whether an `if` or `while` condition ends before `next` (DESIGN
    /// ›Expressions are self-delimiting‹): it is the first complete expression,
    /// so it ends once complete at a token that does not read to its left. It is
    /// complete when it ends in a value, or in identities that could still read
    /// a right side but stand right of a comparison, where they are the compared
    /// value; those still take a value word (`x == i32 1`), and a `(` there is
    /// the body. The identity left of a `(` decides whose it is (DESIGN ›`X (…)`
    /// is one spelling‹): a value with a constructor is applied to it, so
    /// `1 == f(x)` is complete only after the call, while a type is the compared value.
    /// A comment is read through, on the tape and next, so `# «a» true` is complete only at
    /// `true` and `5 * 2 # «c» == 10` only after `10`.
    /// `next` is `None` for a `(` a bracket reader peeks at unlexed.
    pub(super) fn condition_ends(&self, tape: &ParsingTape, next: Option<&Cell>) -> bool {
        let id = next.map_or(self.types.open_, |c| self.cell_identity(c));
        let open = id == self.types.open_;
        if id == self.types.hash_ || next.is_some_and(|c| self.is_comment_cell(c)) {
            return false;
        }
        if !open && next.is_some_and(|c| !c.constructed) && self.reads_left(id) {
            return false;
        }
        let cells: Vec<Cell> =
            tape.iter().map(|(_, c)| *c).filter(|c| !self.is_comment_cell(c)).collect();
        let Some(last) = cells.last() else { return false };
        // A member is its spelling, never the identity it spells: `x.type` ends a value;
        // stand-in for #236.
        let member = cells.len() > 1 && self.is_unbuilt_tight_read(&cells[cells.len() - 2]);
        if member || self.ends_value(last) {
            return true;
        }
        if open && self.applied_to_bracket(last) {
            return false;
        }
        // A type by its rank, stand-in for #286.
        let readers = cells
            .iter()
            .rev()
            .take_while(|c| {
                let id = self.cell_identity(c);
                !c.constructed
                    && !self.ends_value(c)
                    && self.precedence_of_cell(id) >= crate::identities::meta::prec::CONVERT
                    && !self.is_delimiter(id)
            })
            .count();
        let Some(before) = cells.len().checked_sub(readers + 1).map(|k| cells[k]) else {
            return false;
        };
        readers > 0
            && !before.constructed
            && self.compares(self.cell_identity(&before))
            && (open || next.is_some_and(|c| !self.is_value_word(c, id)))
    }

    /// A value, not a type, that has a constructor: a fn value or an instance,
    /// which takes a `(` after it as its call.
    fn applied_to_bracket(&self, cell: &Cell) -> bool {
        let id = self.cell_identity(cell);
        // SAFETY: `id` is null or a resolved identity from the store.
        !cell.constructed
            && !id.is_null()
            && unsafe { dyad::ty(id) } != self.types.type_
            && self.ctor_of(id).is_some()
    }

    /// An infix ranked among the comparisons, below the range and above `not`:
    /// its operands may be any value, types included, and a type standing
    /// right of it is a whole operand, where right of `+` it must still read its bracket.
    /// The rank is a stand-in for #137: whether the result is `bool` is known only once built.
    fn compares(&self, id: DyadPtr) -> bool {
        use crate::identities::meta::prec;
        let rank = self.precedence_of_cell(id);
        self.ctor_of(id).is_some() && self.reads_left(id) && prec::NOT < rank && rank < prec::RANGE
    }

    /// An infix that reads a value on each side, `+` or `==`, whose operands `binary_operands`
    /// reads through. By its rank, stand-in for #286.
    pub(super) fn is_value_infix(&self, id: DyadPtr) -> bool {
        use crate::identities::meta::prec;
        let rank = self.precedence_of_cell(id);
        self.ctor_of(id).is_some()
            && self.reads_left(id)
            && !self.is_delimiter(id)
            && (prec::OR..=prec::MULTIPLICATIVE).contains(&rank)
    }

    /// Whether a bracket reader takes the `(` at the cursor: only when woken
    /// at discovery, the one moment it may read source, and not where the `(`
    /// ends a complete `if` or `while` condition, being the body.
    pub(crate) fn reads_own_bracket(&mut self, tape: &ParsingTape) -> bool {
        self.cx.discovering
            && self.at_open()
            && !(self.cx.lex_mode == Some(RightSide::Condition) && self.condition_ends(tape, None))
    }

    /// An operand, or `?`, which reads only to its left.
    fn ends_value(&self, cell: &Cell) -> bool {
        self.is_operand_cell(cell)
            || (!cell.constructed && self.cell_identity(cell) == self.types.unknown)
    }

    /// Whether `id` continues what stands to its left, as its record says: an infix operator's
    /// first operand, a token's first side (`..`, `.`, `in`) or a type's first field is `lhs`.
    /// A quote, a `(`, a gate word or `else` names none, so it starts what follows it.
    fn reads_left(&self, id: DyadPtr) -> bool {
        use crate::identities::meta;
        // SAFETY: `id` is null or a resolved identity; roles are read only off an operand or
        // token record, fields only off a record type.
        unsafe {
            self.identity_head(id).is_some_and(|h| match meta::kind_of(h) {
                Some(meta::RECORD_TAG) => {
                    let fields = meta::record_fields_of(h);
                    let first = if fields.is_null() {
                        None
                    } else {
                        crate::identities::array::items(fields).first().copied()
                    };
                    let mut scope = ScopeStack::new();
                    scope.push(meta::record_scope_of(h));
                    first.is_some_and(|f| {
                        scope.resolve(self.trie, "lhs").is_ok_and(|r| r.identity == f)
                    })
                }
                _ => meta::takes_lhs(h),
            })
        }
    }

    /// A literal, a plain name, or a data type: what a type standing to its
    /// left may take as its value.
    fn is_value_word(&self, cell: &Cell, id: DyadPtr) -> bool {
        use crate::identities::meta;
        if cell.constructed || self.is_operand_cell(cell) {
            return true;
        }
        if self.precedence_of_cell(id) == meta::prec::LITERAL {
            return true;
        }
        // SAFETY: `id` is null or a resolved identity from the store.
        unsafe {
            self.identity_head(id).is_some_and(|h| {
                !matches!(
                    meta::kind_of(h),
                    Some(meta::TUPLE_TAG | meta::LIST_TAG | meta::TOKEN_TAG)
                )
            })
        }
    }

    /// The one seam every constructor goes through: a reduced dyad passes; a
    /// resolved token yields the name's binding (DESIGN ›The dyad's read
    /// surface‹); a fresh-name token re-resolves its span for the precise error.
    pub(crate) fn as_operand(&mut self, cell: Cell) -> Result<DyadPtr, ParseError> {
        match cell {
            c if c.constructed => Ok(c.dyad),
            c => {
                let (id, binding) = if c.is_fresh(self.types) {
                    match self.resolve_fresh(c.spelling()) {
                        Ok(r) => (r.identity, r.binding),
                        Err(e) => {
                            self.cx.pos = c.start;
                            return Err(ParseError::Resolve(e));
                        }
                    }
                } else {
                    (self.cell_identity(&c), c.binding(self.types))
                };
                // SAFETY: `id` is a resolved dyad from the store.
                unsafe { self.check_capture(id)? };
                self.check_made(binding)?;
                self.note_outer_read(binding);
                Ok(if binding.is_null() { id } else { binding })
            }
        }
    }

    /// Whether the next token is a tight read (`:` or `.`) that takes a
    /// right-side reader's own cell before the reader wakes (DESIGN ›Text is
    /// the quote‹): the reader sleeps, and the read finds it at the boundary.
    pub(super) fn tight_read_takes(&mut self, id: DyadPtr) -> bool {
        if self.precedence_of_cell(id) != crate::identities::meta::prec::READER {
            return false;
        }
        matches!(self.peek_token(), Some((n, _)) if n == self.types.colon_ || n == self.types.dot_)
    }

    /// The segment since the last boundary constructed to exactly one operand
    /// and removed from the tape: the place `=` reads before it drives its
    /// right side. `None` when nothing stands to the left.
    pub(crate) fn construct_left(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Option<Cell>, ParseError> {
        let n = tape.cursor();
        if n == 0 {
            return Ok(None);
        }
        let mut left = ParsingTape::new();
        for (_, c) in tape.iter().take(n) {
            left.push(*c);
        }
        // Lexed to the `=`: a read past it is no part of the place.
        left.sealed = true;
        let built = self.construct_segment(&mut left)?;
        let items = self.hand_to_line(built);
        let target = match items.as_slice() {
            [(one, _)] => *one,
            [] => return Ok(None),
            [(first, _), (_, start), ..] => {
                let e = self.leftover_error(first.dyad);
                self.cx.pos = *start;
                return Err(e);
            }
        };
        for _ in 0..n {
            tape.remove(-1);
        }
        Ok(Some(target))
    }

    /// What a constructor places when it "stands as its own value" (DESIGN
    /// ›The scope's constructor is the driver‹): the use of the name, its
    /// binding, when the cell was lexed; the bare identity for a minted token.
    pub(crate) fn stand_as_value(&self, tape: &ParsingTape, id: DyadPtr) -> DyadPtr {
        match tape.at(0).map(|c| c.binding(self.types)) {
            Some(binding) if !binding.is_null() => binding,
            _ => id,
        }
    }

    /// `as_operand` read through the reading rule, for the constructors that
    /// inspect or bind the value itself rather than store a use of it.
    pub(crate) fn operand_dyad(&mut self, cell: Cell) -> Result<DyadPtr, ParseError> {
        let p = self.as_operand(cell)?;
        // SAFETY: `p` is a dyad from the store.
        let d = unsafe { self.types.through(p) };
        // A box the pass has already filled reads as what it holds; an
        // assignment's target does not come through here, so `a = f64` still writes the box.
        Ok(self.settled_type(d))
    }

    /// `value` where `=` lands it: a box the pass has filled with a type stands for that type, as
    /// `?` reads it (›A `type ?` place is a box, written any number of times‹ [[R291]]); a box
    /// holding any other node was read through where it was lexed, and any other value stands
    /// as it is.
    pub(crate) fn held_type_or(&self, value: DyadPtr) -> DyadPtr {
        // SAFETY: `value` is a reduced dyad from the store.
        let place = unsafe { self.types.through(value) };
        let held = self.settled_type(place);
        // SAFETY: `held` is `place` or the node the place holds now.
        match unsafe { crate::identities::type_identity_of(self.types, held) } {
            Some(t) if held != place => t,
            _ => value,
        }
    }

    /// The type identity `operand` names, where a type is needed now: read as `?` reads the type
    /// to its left, so a box the pass has filled, `type` or `dyad`, names what it holds; a place
    /// or call whose type is known only at run is the checked error; anything else lands in `type`
    /// as `=` lands a value in a `type ?` place, so a value that is no type is refused
    /// (›A `type ?` place is a box, written any number of times‹ [[R291]]).
    pub(crate) fn type_known_now(&mut self, operand: DyadPtr) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        // SAFETY: `operand` is a reduced dyad from the store.
        let read = self.settled_type(unsafe { types.through(operand) });
        // A name with no node yet names no type.
        if read.is_null() {
            return Err(ParseError::BadDeclaredType);
        }
        // SAFETY: `read` is a reduced dyad from the store, and `type` a type node.
        unsafe {
            if crate::identities::dyad_place::known_only_at_run(types, read) {
                return Err(ParseError::TypeKnownOnlyAtRun);
            }
            let landed = crate::identities::land(self.rt.store, types, read, types.type_)?;
            crate::identities::type_identity_of(types, landed).ok_or(ParseError::TypeKnownOnlyAtRun)
        }
    }

    /// A tight extender's left context (`.`'s value, `@`'s pointer, `(`'s
    /// callee); `None` when the construct opens fresh.
    pub(crate) fn left_operand(
        &mut self,
        tape: &ParsingTape,
    ) -> Result<Option<DyadPtr>, ParseError> {
        match tape.at(-1) {
            Some(&cell) if self.is_operand_cell(&cell) => self.as_operand(cell).map(Some),
            _ => Ok(None),
        }
    }

    /// An infix construct's operands at reduction, each one that gives a value; `Ok(None)`
    /// when either side is structurally missing, so the caller declines and the driver
    /// shifts the token.
    pub(crate) fn binary_operands(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Option<(DyadPtr, DyadPtr)>, ParseError> {
        let (Some(&l), Some(&r)) = (tape.at(-1), tape.at(1)) else {
            return Ok(None);
        };
        if !self.is_operand_cell(&l) || !self.is_operand_cell(&r) {
            return Ok(None);
        }
        let (lhs, rhs) = (self.as_operand(l)?, self.as_operand(r)?);
        for (cell, operand) in [(l, lhs), (r, rhs)] {
            // SAFETY: `operand` is a reduced dyad from the store.
            if let Err(e) = unsafe { crate::identities::read::value_type(self.types, operand) } {
                return Err(self.fail_at(cell.start, e));
            }
        }
        let lhs = self.read_through_value(tape, -1, lhs)?;
        let rhs = self.read_through_value(tape, 1, rhs)?;
        // A left side the pass ran stands for the value its run gave.
        let lhs = match tape.at(-1) {
            Some(ran) if ran.dyad != l.dyad => ran.dyad,
            _ => lhs,
        };
        Ok(Some((lhs, rhs)))
    }

    /// `operand`, the cell at `k` of `tape`, read through when it gives a `dyad` no name holds,
    /// where its own type is read, as an infix's operand and by `.type`: the node it gives stands
    /// in its place (›A `dyad ?` place is transparent: a placeholder for a new node of any
    /// type‹ [[R293]]). Where that node's type is known where the operand is written, it is read
    /// at that type each time the line runs (`dyad_place::read_through`, which every landing in a
    /// typed place reads it by too). A call whose node's type is known only at run runs now,
    /// where the pass has reached its line (the top level, or a body's line parsed when it runs),
    /// after what stands before it ([`Self::run_before`]), and its cell stands for the node it
    /// gave, as a `dyad` name read there does; a body's line holding one waits for the run
    /// (`waiting_line_end`), and so does a `while` condition holding one (`hold_condition`).
    pub(super) fn read_through_value(
        &mut self,
        tape: &mut ParsingTape,
        k: isize,
        operand: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        use crate::identities::{dyad_place, read};
        let types = self.types;
        let at = tape.at(k).map_or(self.cx.pos, |c| c.start);
        // SAFETY: `operand` is a reduced dyad from the store.
        unsafe {
            if read::output_type(types, operand) != types.dyad_ {
                return Ok(operand);
            }
            let unknown = std::ptr::null_mut();
            let read = dyad_place::read_through(self.rt.store, types, operand, unknown, false);
            if let Some(node) = read {
                return Ok(node);
            }
            let call = matches!(
                read::read_kind(types, operand),
                read::Read::Executable(read::Dispatch::Call(_))
            );
            if !call || !self.in_pass_order() {
                return Ok(operand);
            }
        }
        // A condition the scan did not find holding the call is parsed again on every pass, so
        // this run would be one too many: stand-in for a scan that reads through a bracket and a
        // member's call.
        if self.cx.each_pass {
            return Err(self.fail_at(at, ParseError::DyadReadEachPass));
        }
        if !self.run_before(tape, at)? {
            return Err(self.fail_at(at, ParseError::DyadCallAfterUnbuilt));
        }
        // SAFETY: as above.
        let node = unsafe { self.ran_value(at, operand)? };
        if let Some(handle) = tape.handle_at(k) {
            tape.stand_as_ran(handle, node);
            self.ran_through(tape, handle);
        }
        Ok(node)
    }

    /// Everything parsed before `at` that has not run, run first, in order (›The pass runs only
    /// as far as it must, in order, and never twice‹ [[R288]]): the lines before the one being
    /// parsed, then the part of it before `at`: the earlier lines of each argument list or `[…]`
    /// still open, and what stands on `tape`, each of which then stands for the value its run
    /// gave. `false`, with nothing run, where that line holds something before `at` that runs
    /// and cannot run first: a part of a quote still being built, what stands before a branch a
    /// construct reads, or a node whose operation is not built yet.
    pub(super) fn run_before(
        &mut self,
        tape: &mut ParsingTape,
        at: usize,
    ) -> Result<bool, ParseError> {
        let standing = self.standing_runs(tape, at);
        let covered = |s: usize| standing.iter().any(|&(_, from, to)| from <= s && s < to);
        if self.cx.open.iter().flat_map(|o| &o.runs).any(|&s| s < at && !covered(s)) {
            return Ok(false);
        }
        self.drain()?;
        for (handle, from, _) in standing {
            let node = self.as_operand(*tape.cell_of(handle))?;
            // SAFETY: a standing cell holds a node this parser built into its store, or names a
            // binding the resolver found.
            let value = unsafe { self.ran_value(from, node)? };
            self.note_made(value);
            tape.stand_as_ran(handle, value);
            self.ran_through(tape, handle);
        }
        Ok(true)
    }

    /// The cells on `tape` before `at` that the line runs when it runs, each with its handle and
    /// the source it spans, up to where the next cell starts.
    fn standing_runs(&self, tape: &ParsingTape, at: usize) -> Vec<(usize, usize, usize)> {
        let cells: Vec<(usize, Cell)> = tape.iter().map(|(n, c)| (n, *c)).collect();
        let ends = cells.iter().skip(1).map(|(_, c)| c.start).chain(std::iter::once(usize::MAX));
        cells
            .iter()
            .zip(ends)
            .filter(|((_, c), _)| c.start < at && self.runs_standing(c))
            .map(|(&(n, c), end)| (n, c.start, end))
            .collect()
    }

    /// Whether the line runs `cell` when it runs: a node that runs, a bracket's scope, which runs
    /// its lines, and a read of a place holding a number or a pointer, which a call after it may
    /// write. A node whose operation is not built yet is none of them. An `=`'s target is not on
    /// the tape its right side is read on (`construct_left`).
    fn runs_standing(&self, cell: &Cell) -> bool {
        use crate::identities::read::{read_kind, Dispatch, Read};
        let types = self.types;
        let node = if cell.constructed { cell.dyad } else { cell.binding(types) };
        if node.is_null() {
            return false;
        }
        // SAFETY: a constructed cell holds a node from the store, and a binding the resolver
        // found is a binding dyad.
        unsafe {
            if dyad::ty(node) == types.scope {
                return true;
            }
            match read_kind(types, node) {
                Read::Executable(Dispatch::Call(_) | Dispatch::Leaf(_)) => true,
                Read::Scalar(_) | Read::Pointer(_) => types.storage_type(node).is_some(),
                _ => false,
            }
        }
    }

    /// Whether a part of the line being parsed, a closed line of an argument list or `[…]`, runs
    /// before a call the pass runs after it: it holds a node that runs, or the line runs it.
    pub(super) fn runs_first(&self, part: &UnrunLine) -> bool {
        // SAFETY: a part is a reduced dyad this parser built into its store.
        !part.runs.is_empty() || self.runs_standing(&unsafe { Cell::built(part.node) })
    }

    /// Line `line` of the bracket `scope` open at `depth`, a part of the line being parsed, runs
    /// before what the pass needs at the cursor, and the value it gave stands in its place, which
    /// the call that takes the bracket reads; what else the part made and nothing took ends with
    /// it. The value itself lives on in its place, so the call holds it or the line's end ends it
    /// (›Holding is decided at the binding site, parameters included‹ [[R61]]).
    ///
    /// # Safety
    /// `node` must be line `line` of `scope`, and `plan` the nodes whose values the line holds to
    /// its end.
    pub(super) unsafe fn run_part(
        &mut self,
        depth: usize,
        scope: DyadPtr,
        line: usize,
        node: DyadPtr,
        plan: &[DyadPtr],
    ) -> Result<(), ParseError> {
        use crate::identities::drop_model::{end_line, handed_through, teardown_of, Teardown};
        let own = handed_through(self.types, node);
        let ends: Vec<DyadPtr> = plan.iter().copied().filter(|m| !own.contains(m)).collect();
        let held = self.rt.line_mark();
        self.rt.push_plan(&ends);
        let value = self.ran_value(self.cx.pos, node);
        self.rt.pop_plan();
        // SAFETY: what the part made since `held` its run gave, still live.
        let ended = end_line(&mut self.rt, held, Ok(0));
        let value = value?;
        ended.map_err(ParseError::Run)?;
        crate::identities::scope::set_item(scope, line, value);
        let open = &mut self.cx.open[depth];
        open.held.retain(|m| !plan.contains(m));
        if teardown_of(self.types, value) != Teardown::Nothing {
            open.held.push(value);
        }
        Ok(())
    }

    /// The cell `handle` names ran: what its line had noted as running inside it, up to where the
    /// next cell starts, has run.
    fn ran_through(&mut self, tape: &ParsingTape, handle: usize) {
        let from = tape.cell_of(handle).start;
        let to = tape.next_start(handle).unwrap_or(usize::MAX);
        let open = self.cx.open.last_mut().expect("a line is read in an open scope");
        open.runs.retain(|&s| !(from <= s && s < to));
    }

    /// What stands for `node` once the pass has run it here, holding what its run gave: the node
    /// or the type a `-> dyad` or `-> type` call gave, as itself, a node the run made copied into
    /// the store and freed; a `[…]` as itself, each of its items that runs standing for the
    /// value it gave; a value whose type keeps it in its bytes, in a new node of its type the
    /// store holds. Any other value, text among them, and one with an end of its own, stands as
    /// a `ran` node of its type, which gives what the run gave, so what reads it reads what read
    /// the node it ran; what takes it, or the line's end, ends it (›Holding is decided at the
    /// binding site, parameters included‹ [[R61]]). A statement is refused at `at`, with nothing
    /// run.
    ///
    /// # Safety
    /// `node` must be a reduced dyad this parser built into its store.
    unsafe fn ran_value(&mut self, at: usize, node: DyadPtr) -> Result<DyadPtr, ParseError> {
        use crate::identities::drop_model::{teardown_of, Teardown};
        use crate::identities::read::{self, place_layout, Read};
        use crate::identities::{dyad_place, ran};
        let types = self.types;
        if dyad::ty(node) == types.square_brackets {
            self.ran_items(at, node)?;
            return Ok(node);
        }
        let out = read::value_type(types, node).map_err(|e| self.fail_at(at, e))?;
        if out == types.dyad_ || out == types.type_ {
            let held = self.run_on_pass(node).map_err(ParseError::Run)? as usize as DyadPtr;
            if held.is_null() {
                return Err(self.fail_at(at, ParseError::Run(crate::run::RunError::NullPointer)));
            }
            self.take(node);
            if crate::identities::type_identity_of(types, held).is_some() {
                return Ok(held);
            }
            let stood = dyad_place::stable(self.rt.store, types, held);
            if stood != held {
                dyad_place::end(&mut self.rt, held as i64);
            }
            return Ok(stood);
        }
        let ends = teardown_of(types, node) != Teardown::Nothing;
        let layout = place_layout(types, out).map(|(r, _)| r);
        let carry = read::carry_of(types, out);
        let (stood, word) = match (layout, carry) {
            (Some(Read::Container(_)), _) | (_, None) => {
                (None, self.run_on_pass(node).map_err(ParseError::Run)?)
            }
            (_, Some(carry)) => {
                let run = dyad_place::build_new_node(self.rt.store, types, node, out);
                let held = self.run_on_pass(run).map_err(ParseError::Run)? as usize as DyadPtr;
                let stood = dyad_place::stable(self.rt.store, types, held);
                if stood != held {
                    dyad_place::end(&mut self.rt, held as i64);
                }
                (Some(stood), ran::word_in(carry, stood))
            }
        };
        self.take(node);
        match stood {
            Some(stood) if !ends => Ok(stood),
            _ => {
                let ran = ran::build(self.rt.store, types, word, ends, out);
                if ends {
                    self.rt.stand(ran);
                    self.cx
                        .open
                        .last_mut()
                        .expect("a line is read in an open scope")
                        .stood
                        .push(ran);
                }
                Ok(ran)
            }
        }
    }

    /// Each item of the `[…]` `list` that runs runs, in order, and stands in its place for the
    /// value it gave, which the call that takes the list reads.
    ///
    /// # Safety
    /// `list` must be a closed `[…]` this parser built into its store.
    unsafe fn ran_items(&mut self, at: usize, list: DyadPtr) -> Result<(), ParseError> {
        use crate::identities::scope;
        let items = scope::exprs_of(list).map_or_else(Vec::new, <[_]>::to_vec);
        for (i, item) in items.into_iter().enumerate() {
            if self.item_runs(item) {
                let value = self.ran_value(at, item)?;
                scope::set_item(list, i, value);
            }
        }
        Ok(())
    }

    /// Whether `item`, a line of a closed `[…]`, holds something that runs.
    ///
    /// # Safety
    /// `item` must be a reduced dyad this parser built into its store.
    unsafe fn item_runs(&self, item: DyadPtr) -> bool {
        if dyad::ty(item) != self.types.square_brackets {
            return self.runs_standing(&Cell::built(item));
        }
        let items = crate::identities::scope::exprs_of(item).unwrap_or(&[]);
        items.iter().any(|&i| self.item_runs(i))
    }

    /// `e` with its caret at `at`, for a constructor refusing an operand it read.
    pub(crate) fn fail_at(&mut self, at: usize, e: ParseError) -> ParseError {
        self.cx.pos = at;
        e
    }

    /// The operand of `move`/`free`: a place as the node it is reached through
    /// (`read::place_node`), never an `addr`, a value no name holds, or a hole. A bare
    /// name or a path `name.field` comes back with the `Ended` the caller hands to
    /// `mark_dead`; a name from outside a loop or `fn` body is refused, and so is one the run
    /// starts with.
    pub(crate) fn place_operand_cell(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Taken, ParseError> {
        let Some(&cell) = tape.at(1) else {
            return Err(ParseError::MissingOperand);
        };
        // A name whose instances' `parse` woke it stands constructed and still holds its binding.
        // SAFETY: a cell's dyad is null or a dyad from the store.
        let woke = cell.constructed
            && !cell.dyad.is_null()
            && unsafe { dyad::ty(cell.dyad) } == self.types.binding_;
        // A field named bare reaches its place as a path does.
        // SAFETY: as above.
        let field = !cell.constructed
            && !cell.dyad.is_null()
            && unsafe { crate::identities::this::is_field_read(self.types, cell.dyad) };
        let taken = if cell.constructed && !woke || field {
            // SAFETY: as above.
            if unsafe { self.types.is_hole(cell.dyad) } {
                Taken::Hole(cell.start)
            } else {
                // SAFETY: as above.
                match unsafe {
                    crate::identities::read::place_node(self.rt.store, self.types, cell.dyad)
                } {
                    Some(place) => Taken::Place(place, self.ended_path(place, &cell)?),
                    None => Taken::Value(cell.dyad, cell.start),
                }
            }
        } else {
            let (identity, binding, scope) = if woke {
                // SAFETY: checked above to be a binding dyad.
                let b = unsafe { Binding::read(cell.dyad) };
                // SAFETY: as above.
                (unsafe { b.names(cell.dyad) }, cell.dyad, b.scope)
            } else {
                let r = match self.cx.scopes.resolve(self.trie, cell.spelling()) {
                    Ok(r) => r,
                    Err(e) => {
                        self.cx.pos = cell.start;
                        return Err(ParseError::Resolve(e));
                    }
                };
                if self.ctor_of(r.identity).is_some() {
                    return Err(ParseError::MissingOperand);
                }
                (r.identity, r.binding, r.scope)
            };
            if identity == self.types.unknown {
                tape.remove(1);
                return Ok(Taken::Hole(cell.start));
            }
            // Every section reads the arche's names, an import included, so ending one
            // here would end it there: stand-in for #35.
            if scope == self.types.root_scope {
                self.cx.pos = cell.start;
                return Err(ParseError::EndsPrimordialName(Box::new(cell.spelling().into())));
            }
            if self.cx.scopes.crosses_barrier(scope) {
                self.cx.pos = cell.start;
                return Err(ParseError::MoveOfOuterName);
            }
            self.check_made(binding)?;
            self.note_outer_read(binding);
            Taken::Place(identity, Some(Ended::name(binding)))
        };
        if let Taken::Place(node, _) | Taken::Value(node, _) = taken {
            // SAFETY: `node` is a resolved dyad from the store.
            unsafe {
                self.check_capture(node)?;
            }
        }
        tape.remove(1);
        Ok(taken)
    }

    /// The field path `place` is, from a name through fields at any depth, which a `move` or
    /// `free` here ends as it would the bare name (DESIGN ›`move` and `free` take a field path
    /// too‹); `None` for a place that is no field, and for a field of the value a type's `free`
    /// or `parse` body works on. A field the parse cannot follow back to a name that holds it
    /// is refused where `cell`, the operand's, stands: through a pointer, an element, a value
    /// no name holds or a borrow, or of a `static` function's own value. A cell that holds a value
    /// with an end is refused where something it is reached through only borrows it.
    fn ended_path(&mut self, place: DyadPtr, cell: &Cell) -> Result<Option<Ended>, ParseError> {
        use crate::identities::drop_model::{
            cell_free, freeing_holder, is_owning_place, path_borrows,
        };
        use crate::identities::this::{field_path, FieldPath};
        let (at, held) = (cell.start, self.settled_type(cell.root));
        // SAFETY: `place` is a reduced dyad from the store.
        let owns = unsafe {
            self.is_owning_read(place)
                || is_owning_place(self.types, place)
                || cell_free(self.types, place).is_some()
        };
        // SAFETY: `place` is a reduced dyad from the store, the cell's root null or a binding.
        let (binding, path) = match unsafe { field_path(self.types, place, cell.root, held) } {
            // A field of a node the parse holds is read as the node its slot holds.
            FieldPath::NoField if !cell.target.0.is_null() => (cell.target.0, vec![cell.target.1]),
            FieldPath::NoField => {
                // Judged as written: a call's place lies in its callee, reached through a parameter.
                // SAFETY: the cell's dyad is a reduced dyad from the store.
                if owns && unsafe { self.reached_through_borrow(cell.dyad) } {
                    return Err(self.fail_at(at, ParseError::EndThroughBorrow));
                }
                return Ok(None);
            }
            FieldPath::Unfollowed => return Err(self.fail_at(at, ParseError::UnfollowedPath)),
            FieldPath::From(binding, path) => (binding, path),
        };
        match self.own_value_borrowed(binding) {
            Some(false) => return Ok(None),
            Some(true) => return Err(self.fail_at(at, ParseError::UnfollowedPath)),
            None => {}
        }
        // SAFETY: `binding` and the path's fields are binding dyads from the store.
        unsafe {
            if self.cx.scopes.crosses_barrier(Binding::read(binding).scope) {
                return Err(self.fail_at(at, ParseError::MoveOfOuterName));
            }
            if owns && path_borrows(self.types, binding, &path) {
                return Err(self.fail_at(at, ParseError::UnfollowedPath));
            }
            if let Some(k) = freeing_holder(self.types, binding, &path) {
                let spelled = |n: usize| {
                    let steps = std::iter::once(binding).chain(path[..n].iter().copied());
                    steps.map(|b| Binding::spelling(b)).collect::<Vec<_>>().join(".")
                };
                let spelled = (spelled(path.len()), spelled(k));
                return Err(self.fail_at(at, ParseError::EndUnderFree(Box::new(spelled))));
            }
        }
        Ok(Some(Ended { binding, path }))
    }

    /// Whether `place` is reached through something that only borrows it
    /// (`drop_model::only_borrows`): a cell through its pointer, past the steps `p + k`, and
    /// whatever that pointer is reached through in turn; a field path through its name and fields.
    ///
    /// # Safety
    /// `place` must be a reduced dyad from the store.
    unsafe fn reached_through_borrow(&self, place: DyadPtr) -> bool {
        use crate::identities::drop_model::{only_borrows, path_borrows};
        use crate::identities::this::{field_path, FieldPath};
        let types = self.types;
        if dyad::ty(place) == types.deref_ {
            let (pointer, _, _) = crate::identities::pointer::deref_parts(place);
            let pointer = crate::identities::binary::step_base(types, pointer);
            return only_borrows(types, pointer) || self.reached_through_borrow(pointer);
        }
        let null = std::ptr::null_mut();
        let FieldPath::From(name, path) = field_path(types, place, null, null) else {
            return false;
        };
        self.own_value_borrowed(name).unwrap_or_else(|| path_borrows(types, name, &path))
    }

    /// Whether the value the body being defined works on, when `name` is it, is borrowed there:
    /// a `static` function's own value is, the one a type's own `free` or `parse` body ends or
    /// builds is not. `None` for any other name.
    fn own_value_borrowed(&self, name: DyadPtr) -> Option<bool> {
        let def = self.cx.definitions.last().filter(|d| d.this_param == name)?;
        Some(!(def.in_parse || def.in_free))
    }

    /// The one read every prefix constructor makes (`return x`, `not x`, a
    /// prefix `-`).
    pub(crate) fn take_right(&mut self, tape: &mut ParsingTape) -> Result<DyadPtr, ParseError> {
        let Some(&cell) = tape.at(1) else {
            return Err(ParseError::MissingOperand);
        };
        if !self.is_operand_cell(&cell) {
            return Err(ParseError::MissingOperand);
        }
        let node = self.as_operand(cell)?;
        tape.remove(1);
        Ok(node)
    }

    /// What a type at the center takes from its right, the cell at `k` (›A type's `parse` decides
    /// what converts into it; unwritten, a value converts only into a type that holds it
    /// exactly‹ [[R155]]): a built value or a bare name, what is built at discovery (`.`, `@`, a
    /// call, an `if`) finished before it. `None` where the type stands as itself: nothing there,
    /// or a cell still to be built that is no value of its own (`+`, `->`).
    pub(crate) fn type_operand(
        &mut self,
        tape: &mut ParsingTape,
        k: isize,
    ) -> Result<Option<Cell>, ParseError> {
        let Some(cell) = self.cell_at(tape, k)? else { return Ok(None) };
        Ok(self.is_operand_cell(&cell).then_some(cell))
    }

    /// Whether the cell right of the center is the unconstructed marker word `id`.
    pub(crate) fn marker_right(&self, tape: &ParsingTape, id: DyadPtr) -> bool {
        tape.at(1).is_some_and(|cell| !cell.constructed && self.cell_identity(cell) == id)
    }

    /// Reject a capture: a name of an enclosing function's frame, not the innermost open
    /// one. v1 has no closures; reading one would resolve against the wrong activation
    /// record at run time. A `static` initializer runs at the definition, where no call of
    /// the functions open around it is live.
    ///
    /// # Safety
    /// `node` must be a resolved dyad from the store.
    pub(super) unsafe fn check_capture(&self, node: DyadPtr) -> Result<(), ParseError> {
        let node = self.types.through(node);
        if let Some((crate::binding::Frame::Call(f), _)) = self.types.frame_of(node) {
            if let Some(at) = self.cx.once_at {
                if self.cx.frames[..at].iter().any(|open| open.frame == f) {
                    return Err(ParseError::OnceReadsUnmade);
                }
            }
            if self.cx.frames.last().is_none_or(|open| open.frame != f) {
                return Err(ParseError::CapturedLocal);
            }
        }
        Ok(())
    }

    /// A value made once (`static`, `immediate`) runs at parse, where a name of the
    /// loop or branch around it has no value yet.
    pub(super) fn check_made(&self, binding: DyadPtr) -> Result<(), ParseError> {
        // SAFETY: `binding` is the binding dyad the resolver returned for the name.
        if self.cx.once_at.is_some() && unsafe { Binding::read(binding) }.unmade {
            return Err(ParseError::OnceReadsUnmade);
        }
        Ok(())
    }

    /// For every open function whose barrier the binding's scope lies below,
    /// the read is of an outer name and the binding joins its `FN_OUTER` list
    /// once; a scope on no stack (a section's) lies below every function.
    pub(super) fn note_outer_read(&mut self, binding: DyadPtr) {
        if binding.is_null() || self.cx.frames.is_empty() {
            return;
        }
        // SAFETY: a non-null binding came from the resolver or a function's list: a binding dyad.
        let scope = unsafe { Binding::read(binding).scope };
        let depth = self.cx.scopes.position(scope).unwrap_or(0);
        for frame in &mut self.cx.frames {
            if depth < frame.below && !frame.outer.contains(&binding) {
                frame.outer.push(binding);
            }
        }
    }

    /// A call is a use of every outer name the callee's body reads (DESIGN
    /// ›`move` and `free` are static‹): each listed binding is checked as a bare
    /// use here would be, and joins the lists of the functions being parsed.
    ///
    /// # Safety
    /// `callee` must be null or a dyad from the store.
    pub(crate) unsafe fn check_call_reads(&mut self, callee: DyadPtr) -> Result<(), ParseError> {
        if callee.is_null() {
            return Ok(());
        }
        // SAFETY: `callee` is a dyad from the store.
        if unsafe { dyad::ty(callee) } != self.types.fn_type {
            return Ok(());
        }
        // SAFETY: `callee` is a function node; its list holds binding dyads.
        let outer = unsafe { fn_outer(callee) };
        for &binding in outer {
            // SAFETY: a function's outer list holds binding dyads from the store.
            let fields = unsafe { Binding::read(binding) };
            let name = || {
                if fields.name.is_null() {
                    String::new()
                } else {
                    // SAFETY: a binding's `name` is a string node.
                    String::from_utf8_lossy(unsafe { crate::identities::string::text(fields.name) })
                        .into_owned()
                }
            };
            // A `static` name is one place for the run, reachable wherever its function is.
            // SAFETY: as above.
            let is_static = unsafe { Binding::has_gate(binding, self.types.static_) };
            if !is_static
                && !self.cx.scopes.is_open(fields.scope)
                && !self.outer.iter().any(|c| c.scopes.is_open(fields.scope))
                && !self.imports.sections.contains(&fields.scope)
            {
                return Err(ParseError::Resolve(ResolveError::OutOfScope(name())));
            }
            // The list names names, not paths: a name with a path ended is refused whole.
            // SAFETY: as above.
            if fields.is_dead() || unsafe { Binding::has_ended_path(binding) } {
                return Err(ParseError::Resolve(ResolveError::Dead(name())));
            }
            self.note_outer_read(binding);
        }
        Ok(())
    }
}
