// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Operands: the reading rule a constructor takes its cells through, the left side of
//! `=`, a prefix word's operand, and the capture and liveness checks on a name's use.

use super::*;
use crate::dyad;

/// What a point where the parse needs a value finds before it in its line, once the pass has
/// tried to run it first.
pub(super) enum Before {
    /// Everything before it has run.
    Ran,
    /// A part that runs and cannot run first yet stands in the line being parsed of the open
    /// scope at this depth, and nothing ran.
    Waits(usize),
}

/// Who takes the operands [`Parser::operands`] reads, which decides what a side that is no value
/// is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Taker {
    /// A built-in operation, which refuses it where it stands, saying why.
    Operation,
    /// A parse written in Logos, handed it to decide itself (›A `tape[k]` read past the lexed
    /// frontier lexes on demand for every constructor‹ [[R231]]): a spelling no live name has as
    /// its `unknown_name` node, a side that gives nothing as the tape's `void` node.
    Parse,
}

/// What a running constructor reads of a cell of its tape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TapeRead {
    /// The cell, `tape[k]`: a name in it read as every reader of an operand reads one.
    Cell,
    /// The node whose own type `tape[k].type` is: the name read as for `Cell`, a `dyad` read
    /// through to the node it holds.
    Type,
    /// The node whose type `tape[k].gives` is what it gives: the cell, a bracket too, read as an
    /// operation reads its operand.
    Gives,
}

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

    /// A reduced dyad, or a token that does not extend: a resolved operand, a bracket held for
    /// the word that takes it, or a fresh name in waiting. A pending extender token is not an
    /// operand.
    pub(crate) fn is_operand_cell(&self, cell: &Cell) -> bool {
        match cell {
            c if c.constructed || c.held.is_some() => true,
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

    /// Whether the side read now is one that `if`, `while` or `for` takes its first node from,
    /// building its arm after it: a condition, or a `for` range (›Every identity has a
    /// `parse_rank`, and `(` works in two steps‹ [[R259]]).
    pub(super) fn reads_first_node(&self) -> bool {
        matches!(self.cx.lex_mode, Some(RightSide::Condition | RightSide::Range))
    }

    /// Whether an `if` or `while` condition, or a `for` range, ends before `next` (DESIGN
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
        let by_rank = |rank: f64| {
            use crate::identities::meta::prec;
            rank == prec::ONE_RANK || rank > prec::POINTER_TYPE
        };
        let readers = cells
            .iter()
            .rev()
            .take_while(|c| {
                let id = self.cell_identity(c);
                !c.constructed
                    && !self.ends_value(c)
                    && by_rank(self.precedence_of_cell(id))
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

    /// An infix ranked among the comparisons: its operands may be any value, types included, and
    /// a type standing right of it is a whole operand, where right of `+` it must still read its
    /// bracket. The rank is a stand-in for #137: whether the result is `bool` is known only once
    /// built.
    fn compares(&self, id: DyadPtr) -> bool {
        use crate::identities::meta::prec;
        self.ctor_of(id).is_some()
            && self.reads_left(id)
            && self.precedence_of_cell(id) == prec::COMPARE
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

    /// Whether a word that reads its own right side finds one to consume (›`parse_rank` and
    /// `associativity` are data, and the only triggers‹ [[R215]]): woken at discovery and at the
    /// frontier, the one moment it may read source, with a token at the cursor that is no
    /// boundary, no operator that reads to its left, and no `(` that is the body of a complete
    /// `if` or `while` condition. Where it finds none it declines, and stands as its own value (›A
    /// constructor's outcome is read off its own cell; no holding, no re-invocation‹ [[R261]]).
    pub(crate) fn finds_right_side(&mut self, tape: &ParsingTape) -> bool {
        if !self.cx.discovering || tape.at(1).is_some() {
            return false;
        }
        match self.peek_start() {
            None => false,
            Some(None) => true,
            Some(Some(id)) if id == self.types.open_ => {
                !(self.reads_first_node() && self.condition_ends(tape, None))
            }
            Some(Some(id)) => {
                let operator = self.ctor_of(id).is_some() && self.reads_left(id);
                self.boundary_of(id).is_none() && !operator
            }
        }
    }

    /// Whether a bracket reader takes the `(` at the cursor: a right side it finds that opens with
    /// its bracket.
    pub(crate) fn reads_own_bracket(&mut self, tape: &ParsingTape) -> bool {
        self.finds_right_side(tape) && self.at_open()
    }

    /// An operand, or `?`, which reads only to its left.
    fn ends_value(&self, cell: &Cell) -> bool {
        self.is_operand_cell(cell)
            || (!cell.constructed && self.cell_identity(cell) == self.types.unknown)
    }

    /// Whether `id` continues what stands to its left, as its record says: an infix operator's
    /// first operand, a token's first side (`..`, `.`, `in`) or a type's first field is `lhs`.
    /// A quote, a `(`, a gate word or `else` names none, so it starts what follows it.
    pub(super) fn reads_left(&self, id: DyadPtr) -> bool {
        use crate::identities::meta;
        // An operation written in Logos reads to its left as the native version whose name it
        // took: stand-in for #500.
        if let Some(native) = self.types.native_of(id) {
            return self.reads_left(native);
        }
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

    /// A literal, a plain name, a data type, or a gate word, which gates the value after it: what
    /// a type standing to its left may take as its value.
    fn is_value_word(&self, cell: &Cell, id: DyadPtr) -> bool {
        use crate::identities::meta;
        if cell.constructed || self.is_operand_cell(cell) || self.is_gate_word(id) {
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

    /// `value` where `=` or a call lands it: a box the pass has filled with a type stands for that
    /// type, as `?` reads it (›A `type ?` place is a box, written any number of times‹ [[R291]]);
    /// a box holding any other node was read through where it was lexed, and any other value
    /// stands as it is.
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
    /// to its left, so a box the pass has filled, `type` or `dyad`, names what it holds; a box
    /// read after a part of its line that cannot run first yet is refused there; a place
    /// or call whose type is known only at run is the checked error; anything else lands in `type`
    /// as `=` lands a value in a `type ?` place, so a value that is no type is refused
    /// (›A `type ?` place is a box, written any number of times‹ [[R291]]).
    pub(crate) fn type_known_now(&mut self, operand: DyadPtr) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        // SAFETY: `operand` is a reduced dyad from the store.
        let place = unsafe { types.through(operand) };
        if let Some(at) = self.unsettled_at(place) {
            return Err(self.fail_at(at, ParseError::RunsBeforeUnbuilt));
        }
        let read = self.settled_type(place);
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
        Ok(self.operands(tape, [-1, 1], Taker::Operation)?.map(|[lhs, rhs]| (lhs, rhs)))
    }

    /// The operands at `ks` of `tape`, read as every operation reads what it takes, built in or
    /// written in Logos: each side a name live here (›A dead name takes nothing until `:=`
    /// redeclares it‹ [[R72]]) giving a value refused nowhere (`read::value_type`), and a `dyad`
    /// read through to the node it gives ([`Self::read_through_value`]). Every side is checked
    /// before any is read through, so nothing runs before a side is refused. `Ok(None)` when a
    /// side is structurally missing.
    fn operands<const N: usize>(
        &mut self,
        tape: &mut ParsingTape,
        ks: [isize; N],
        taker: Taker,
    ) -> Result<Option<[DyadPtr; N]>, ParseError> {
        let types = self.types;
        let mut cells = Vec::with_capacity(N);
        for k in ks {
            match self.side_at(tape, k)? {
                Some(cell) => cells.push(cell),
                None => return Ok(None),
            }
        }
        let mut names = Vec::with_capacity(N);
        for &cell in &cells {
            names.push(self.name_read(cell, taker)?);
        }
        let mut gives = Vec::with_capacity(N);
        for (&name, cell) in names.iter().zip(&cells) {
            // SAFETY: `name` is a reduced dyad from the store.
            gives.push(match unsafe { crate::identities::read::value_type(types, name) } {
                Ok(_) => true,
                Err(e) if taker == Taker::Parse && crate::identities::read::gives_nothing(&e) => {
                    false
                }
                Err(e) => return Err(self.fail_at(cell.start, e)),
            });
        }
        let mut read = [std::ptr::null_mut(); N];
        for (i, k) in ks.into_iter().enumerate() {
            read[i] = if gives[i] {
                self.read_through_value(tape, k, names[i])?
            } else {
                types.tape.nothing
            };
        }
        // A side the pass ran while a later side was read stands for the value its run gave.
        for i in 0..N.saturating_sub(1) {
            if let Some(ran) = tape.at(ks[i]).filter(|ran| ran.dyad != cells[i].dyad) {
                read[i] = ran.dyad;
            }
        }
        Ok(Some(read))
    }

    /// The cell an operation takes at `k` of `tape`: a word there not built yet is started first
    /// and builds what it takes, and a bracket held there is built (›Every identity has a
    /// `parse_rank`, and `(` works in two steps‹ [[R259]]). `None` where no operand stands within
    /// reach.
    pub(crate) fn side_at(
        &mut self,
        tape: &mut ParsingTape,
        k: isize,
    ) -> Result<Option<Cell>, ParseError> {
        loop {
            if tape.in_reach(k).is_some() {
                self.take_held(tape, k)?;
            }
            let Some(&cell) = tape.in_reach(k) else { return Ok(None) };
            if self.is_operand_cell(&cell) {
                return Ok(Some(cell));
            }
            let id = self.cell_identity(&cell);
            let Some(construct) = self.ctor_of(id) else { return Ok(None) };
            let handle = tape.handle_at(k).expect("read above");
            let taker = tape.taking();
            tape.center_on(handle);
            let started = self.run_ctor(construct, id, tape, false);
            tape.restore(taker);
            if let Some(taker) = taker {
                tape.took(taker);
            }
            started?;
        }
    }

    /// The read of a name every reader of an operand makes ([`Self::as_operand`]), a dead one
    /// refused; a parse is handed a spelling no live name has as its `unknown_name` node
    /// (›Unknown spellings are two pattern identities‹ [[R252]]).
    fn name_read(&mut self, cell: Cell, taker: Taker) -> Result<DyadPtr, ParseError> {
        let unknown = taker == Taker::Parse
            && cell.is_fresh(self.types)
            && matches!(self.resolve_fresh(cell.spelling()), Err(e) if !matches!(e, ResolveError::Dead(_)));
        if unknown {
            return Ok(cell.dyad);
        }
        self.as_operand(cell)
    }

    /// What a running constructor reads at `k` of its tape: the tape's `void` node past its reach
    /// (›A `tape[k]` read past the lexed frontier lexes on demand for every constructor‹
    /// [[R231]]); `tape[k].gives` by the whole read an operation makes of its operand, which
    /// builds an identity there still to be built first (›`.gives` reads the type a node gives
    /// when it runs, on any node; `.type` stays the node's own type‹ [[R387]]); otherwise a cell
    /// that is no operand, an identity still to be built, as it stands, and so a bracket, whose
    /// lines it reads, and any other cell as an operation takes its operand, `tape[k]` by its name
    /// read alone and `tape[k].type` read through a `dyad` to the node it holds.
    pub(crate) fn read_tape(
        &mut self,
        tape: &mut ParsingTape,
        k: isize,
        read: TapeRead,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let Some(cell) = tape.in_reach(k).copied() else { return Ok(types.tape.nothing) };
        // SAFETY: a lexed or built cell holds a dyad from the store.
        let bracket = unsafe { crate::identities::tape::bracket_kind(types, cell.dyad) }.is_some();
        if read != TapeRead::Gives && (!self.is_operand_cell(&cell) || bracket) {
            return Ok(cell.dyad);
        }
        match read {
            TapeRead::Cell => self.name_read(cell, Taker::Parse),
            TapeRead::Type => {
                let name = self.name_read(cell, Taker::Parse)?;
                self.read_through_value(tape, k, name)
            }
            TapeRead::Gives => {
                Ok(self.operands(tape, [k], Taker::Parse)?.map_or(cell.dyad, |[operand]| operand))
            }
        }
    }

    /// `operand`, the cell at `k` of `tape`, read through when it gives a `dyad` no name holds,
    /// where its own type is read, as an infix's operand and by `.type`: the node it gives stands
    /// in its place (›A `dyad ?` place is transparent: a placeholder for a new node of any
    /// type‹ [[R293]]). Where that node's type is known where the operand is written, it is read
    /// at that type each time the line runs (`dyad_place::read_through`, which every landing in a
    /// typed place reads it by too). An entry of a `{ … }` a field path reaches is read as a
    /// `dyad` name is where the lexer meets it ([`Self::held_now`]). A call whose node's type is
    /// known only at run runs now, where the pass has reached its line (the top level, or a
    /// body's line parsed when it runs), after what stands before it ([`Self::run_before`]), and
    /// its cell stands for the node it gave, as a `dyad` name read there does; a body's line
    /// holding either waits for the run (`waiting_line_end`), and so does a `while` condition
    /// holding one (`hold_condition`).
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
            if self.in_pass_order() && self.is_dyad_place(operand) {
                self.run_first(tape, at)?;
                let Some(node) = self.held_now(operand)? else { return Ok(operand) };
                self.cx.each_pass_read |= self.cx.each_pass;
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
        self.run_first(tape, at)?;
        let plan = self.line_made();
        let depth = self.cx.open.len() - 1;
        // SAFETY: as above.
        let node = unsafe { self.run_early(depth, at, operand, &plan)? };
        if let Some(handle) = tape.handle_at(k) {
            tape.stand_as_ran(handle, node);
            self.ran_through(tape, handle);
        }
        Ok(node)
    }

    /// A point at `at` where the parse needs a value: what stands before it runs first
    /// ([`Self::ran_first`]), or the point is refused, with nothing run, where something before
    /// it cannot.
    pub(super) fn run_first(
        &mut self,
        tape: &mut ParsingTape,
        at: usize,
    ) -> Result<(), ParseError> {
        match self.ran_first(tape, at)? {
            Before::Ran => Ok(()),
            Before::Waits(_) => Err(self.fail_at(at, ParseError::RunsBeforeUnbuilt)),
        }
    }

    /// A `type ?` box, `id`, read at `at`: what stands before it runs first, as for every box
    /// read. Where a part of its line before it cannot run first, nothing runs, and until that
    /// line closes the box stands for itself, as in a body: read as a value it reads its place
    /// when the line runs, and a use of the type it holds is refused ([`Self::type_known_now`]).
    pub(super) fn read_type_box(
        &mut self,
        tape: &mut ParsingTape,
        id: DyadPtr,
        at: usize,
    ) -> Result<(), ParseError> {
        if let Before::Waits(depth) = self.ran_first(tape, at)? {
            self.cx.open[depth].unsettled.push((id, at));
        }
        Ok(())
    }

    /// Where `place`, a `type ?` box, was last read after a part of its line that cannot run
    /// first, while that line is still being parsed: the type it holds there is known only once
    /// that part has run (›A `type ?` place is a box, written any number of times‹ [[R291]]).
    pub(super) fn unsettled_at(&self, place: DyadPtr) -> Option<usize> {
        let reads = self.cx.open.iter().flat_map(|o| &o.unsettled);
        reads.filter(|&&(read, _)| read == place).map(|&(_, at)| at).max()
    }

    /// What stands before `at` runs first ([`Self::run_before`]); where the line does not run in
    /// the pass, a `while` condition's or a body's, only the lines before it do.
    pub(super) fn ran_first(
        &mut self,
        tape: &mut ParsingTape,
        at: usize,
    ) -> Result<Before, ParseError> {
        if self.cx.each_pass || !self.in_pass_order() {
            self.drain()?;
            return Ok(Before::Ran);
        }
        self.run_before(tape, at)
    }

    /// Everything parsed before `at` that has not run, run first, in order (›The pass runs only
    /// as far as it must, in order, and never twice‹ [[R288]]): the lines before the one being
    /// parsed, then the part of it before `at`: the earlier lines of each argument list or `[…]`
    /// still open, the earlier parts of each quote still open, and what stands on `tape`, each
    /// of which then stands for the value its run gave. Nothing runs where that line holds
    /// something before `at` that runs and cannot run first: what stands before a branch a
    /// construct reads, or a node whose operation is not built yet.
    pub(super) fn run_before(
        &mut self,
        tape: &mut ParsingTape,
        at: usize,
    ) -> Result<Before, ParseError> {
        self.take_held_before(tape, at)?;
        let standing = self.standing_runs(tape, at);
        let covered = |s: usize| standing.iter().any(|&(_, from, to)| from <= s && s < to);
        let blocked =
            self.cx.open.iter().position(|o| o.runs.iter().any(|&s| s < at && !covered(s)));
        if let Some(depth) = blocked {
            return Ok(Before::Waits(depth));
        }
        self.drain()?;
        let depth = self.cx.open.len() - 1;
        for (handle, from, _) in standing {
            let node = self.as_operand(*tape.cell_of(handle))?;
            let plan = self.line_made();
            // SAFETY: a standing cell holds a node this parser built into its store, or names a
            // binding the resolver found.
            let value = unsafe { self.run_early(depth, from, node, &plan)? };
            self.note_made(value);
            tape.stand_as_ran(handle, value);
            self.ran_through(tape, handle);
        }
        Ok(Before::Ran)
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
    /// the tape its right side is read on (`assign::read_target`).
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

    /// Line `line` of the bracket or quote open at `depth`, a part of the line being parsed, runs
    /// before what the pass needs at the cursor ([`Self::run_early`]), and the value it gave
    /// stands in its place, which the call that takes the bracket, or the word that reads the
    /// quote, reads. The value lives on in its place, so the call holds it or the line's end ends
    /// it (›Holding is decided at the binding site, parameters included‹ [[R61]]).
    ///
    /// # Safety
    /// `node` must be line `line` of the scope open at `depth`, and `plan` the nodes whose values
    /// the line holds to its end.
    pub(super) unsafe fn run_part(
        &mut self,
        depth: usize,
        line: usize,
        node: DyadPtr,
        plan: &[DyadPtr],
    ) -> Result<(), ParseError> {
        use crate::identities::drop_model::{teardown_of, Teardown};
        let value = self.run_early(depth, self.cx.pos, node, plan)?;
        let open = &mut self.cx.open[depth];
        open.stand(line, value);
        open.held.retain(|m| !plan.contains(m));
        if teardown_of(self.types, value) != Teardown::Nothing {
            open.held.push(value);
        }
        Ok(())
    }

    /// `node`, a part of the line the scope open at `depth` is parsing, runs early, giving what
    /// stands for it ([`Self::ran_value`]); the line holds what else its run made
    /// ([`Self::run_held`]).
    ///
    /// # Safety
    /// `node` must be a reduced dyad this parser built into its store, and `plan` the nodes whose
    /// values the line holds to its end.
    unsafe fn run_early(
        &mut self,
        depth: usize,
        at: usize,
        node: DyadPtr,
        plan: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        // SAFETY: the caller's contract.
        self.run_held(depth, node, plan, |p| unsafe { p.ran_value(at, node) })
    }

    /// `run`, a run the pass makes of `node` before the line the scope open at `depth` is
    /// parsing runs: an early part of it, a `static` value or an `immediate` operand (›The pass
    /// runs only as far as it must, in order, and never twice‹ [[R288]]). The value of each node
    /// of `plan` that runs, but for what `node`'s own value passes through, is the line's, which
    /// holds it to its end, last made first, as if the line had run it: each stands as a value
    /// the pass ran early (›Holding is decided at the binding site, parameters included‹
    /// [[R61]]). A fault leaves the line, so what the run made ends before the fault shows.
    ///
    /// # Safety
    /// `node` must be a reduced dyad this parser built into its store, and `plan` the nodes whose
    /// values the line holds to its end.
    pub(super) unsafe fn run_held<T>(
        &mut self,
        depth: usize,
        node: DyadPtr,
        plan: &[DyadPtr],
        run: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        use crate::identities::drop_model::{end_line, handed_through};
        use crate::identities::{ran, read};
        let types = self.types;
        let own = handed_through(types, node);
        let ends: Vec<DyadPtr> = plan.iter().copied().filter(|m| !own.contains(m)).collect();
        let mark = self.rt.line_mark();
        let before = self.cx.open[depth].stood.len();
        self.rt.push_plan(&ends);
        let value = run(self);
        self.rt.pop_plan();
        let value = match value {
            Ok(value) => value,
            Err(fault) => {
                // SAFETY: what the run made since `mark`, still live. The fault stays the error
                // shown.
                let _ = end_line(&mut self.rt, mark, Ok(()));
                return Err(fault);
            }
        };
        let made: Vec<DyadPtr> = self
            .rt
            .take_line_values(mark)
            .into_iter()
            .map(|(made, word)| {
                let out = read::output_type(types, made);
                let stood = ran::build(self.rt.store, types, word, true, out);
                self.rt.stand(stood);
                stood
            })
            .collect();
        let stood = &mut self.cx.open[depth].stood;
        let first = before.min(stood.len());
        stood.splice(first..first, made);
        Ok(value)
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
        use crate::identities::drop_model::{teardown_of, teardown_of_type, Teardown};
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
            if stood == held {
                return Ok(stood);
            }
            dyad_place::release(&mut self.rt, held as i64);
            // A value with an end of its own moved into the store's copy, which then stands as
            // one its line made.
            let ty = dyad::ty(stood);
            return match read::carry_of(types, ty) {
                Some(carry) if teardown_of_type(types, ty) != Teardown::Nothing => {
                    Ok(self.stand_ran(ran::word_in(carry, stood), ty))
                }
                _ => Ok(stood),
            };
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
                    dyad_place::release(&mut self.rt, held as i64);
                }
                (Some(stood), ran::word_in(carry, stood))
            }
        };
        self.take(node);
        match stood {
            Some(stood) if !ends => Ok(stood),
            _ if ends => Ok(self.stand_ran(word, out)),
            _ => Ok(ran::build(self.rt.store, types, word, false, out)),
        }
    }

    /// A `ran` node of `out` giving `word`, a value with an end of its own the pass ran early,
    /// which what takes it, or its line's end, ends.
    fn stand_ran(&mut self, word: i64, out: DyadPtr) -> DyadPtr {
        let ran = crate::identities::ran::build(self.rt.store, self.types, word, true, out);
        self.rt.stand(ran);
        self.cx.open.last_mut().expect("a line is read in an open scope").stood.push(ran);
        ran
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
        self.take_held(tape, 1)?;
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
            if self.ends_outer_name(scope, binding) {
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
            cell_teardown, freeing_holder, is_owning_place, path_borrows,
        };
        use crate::identities::this::{field_path, FieldPath};
        let (at, held) = (cell.start, self.settled_type(cell.root));
        // SAFETY: `place` is a reduced dyad from the store.
        let owns = unsafe {
            self.is_owning_read(place)
                || is_owning_place(self.types, place)
                || cell_teardown(self.types, place).is_some()
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
            if self.ends_outer_name(Binding::read(binding).scope, binding) {
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
    /// prefix `-`): its side, read as an operation reads one, so a word there not built yet is
    /// built first, as `move p` in `return move p`.
    pub(crate) fn take_right(&mut self, tape: &mut ParsingTape) -> Result<DyadPtr, ParseError> {
        let Some(cell) = self.side_at(tape, 1)? else {
            return Err(ParseError::MissingOperand);
        };
        let node = self.as_operand(cell)?;
        tape.remove(1);
        Ok(node)
    }

    /// What a type or a gate word at the center takes from its right, the cell at `k`: a built
    /// value or a bare name, what is built at discovery (`.`, `@`, a call, a bracket) finished
    /// before it, `?`, and a word of its own rank, built before it since each associates right.
    /// `None` where it stands alone: nothing there, or a word of a later rank still to be built
    /// (`+`, `not`, a negating `-`), which it does not start, so `i32 -m` is written `i32 (-m)`
    /// (›Every identity has a `parse_rank`, and `(` works in two steps‹ [[R259]]).
    pub(crate) fn type_operand(
        &mut self,
        tape: &mut ParsingTape,
        k: isize,
    ) -> Result<Option<Cell>, ParseError> {
        // A `[ … ]` is a list, which only `array` takes as its own.
        let list = tape
            .in_reach(k)
            .is_some_and(|c| c.held.is_some() && self.cell_identity(c) == self.types.open_sq_);
        if list {
            return Ok(None);
        }
        let Some(mut cell) = self.cell_at(tape, k)? else { return Ok(None) };
        // A call's result whose type has a parse of its own stands unbuilt, woken as that type's
        // word, though the call built it: `type arithmetic.operation(«+», …)`.
        let types = self.types;
        if !cell.constructed
            && cell.held.is_none()
            && cell.binding(types).is_null()
            && !cell.is_fresh(types)
            && self.wakes(cell.dyad, true)
        {
            cell.constructed = true;
            return Ok(Some(cell));
        }
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
        match self.types.frame_of(node) {
            Some((crate::binding::Frame::Call(f), _)) => self.reach_frame(f),
            _ => Ok(()),
        }
    }

    /// Whether the line being read reaches the frame of the function `f`: only from `f`'s own
    /// body, in a call of it, which a value made once at its definition is not.
    pub(super) fn reach_frame(&self, f: DyadPtr) -> Result<(), ParseError> {
        if let Some(at) = self.cx.once_at {
            if self.cx.frames[..at].iter().any(|open| open.frame == f) {
                return Err(ParseError::OnceReadsUnmade);
            }
        }
        if self.cx.frames.last().is_none_or(|open| open.frame != f) {
            return Err(ParseError::CapturedLocal);
        }
        Ok(())
    }

    /// Whether a `move` or `free` here of `binding`, which `scope` holds, ends a name declared
    /// outside the loop or function body around it, so the next pass or call would find it dead:
    /// a name of a scope opened before that body, or a file's name, whose place the file keeps
    /// for the run however an `import` in the body set it there.
    pub(super) fn ends_outer_name(&self, scope: DyadPtr, binding: DyadPtr) -> bool {
        if self.cx.scopes.crosses_barrier(scope) {
            return true;
        }
        // SAFETY: a non-null `binding` is a binding dyad from the store, and so is its origin.
        let file = !binding.is_null()
            && self
                .imports
                .sections
                .contains(&unsafe { Binding::read(Binding::origin_of(binding)) }.scope);
        file && self.cx.scopes.in_barrier()
    }

    /// A value made once (`static`, `immediate`) runs at parse, where a name of the
    /// loop or branch around it has no value yet.
    pub(super) fn check_made(&self, binding: DyadPtr) -> Result<(), ParseError> {
        // A field named bare has no binding.
        if self.cx.once_at.is_none() || binding.is_null() {
            return Ok(());
        }
        // SAFETY: a non-null `binding` is the binding dyad the resolver returned for the name.
        if unsafe { Binding::read(binding) }.unmade {
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
            // A `static` name is one place for the run, reachable wherever its function is, and so
            // is a name of a `{ … }` that holds no bytes, which reads through its value.
            // SAFETY: as above; a binding's scope is a scope node from the store.
            let is_static = unsafe {
                crate::identities::gate::name_has(self.types, binding, self.types.static_)
                    || !fields.is_storage()
                        && !fields.scope.is_null()
                        && crate::identities::instance::kept_line(self.types, fields.scope)
                            .is_some()
            };
            if !is_static
                && !self.cx.scopes.is_open(fields.scope)
                && !self.outer.iter().any(|c| c.scopes.is_open(fields.scope))
                && !self.imports.sections.contains(&fields.scope)
                && fields.scope != self.types.carried
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
