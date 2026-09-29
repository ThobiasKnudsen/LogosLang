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
                c.is_fresh() || (self.ctor_of(id).is_none() && !self.is_delimiter(id))
            }
        }
    }

    /// An identity whose record is a parse-only token with no constructor
    /// (`)`, `,`, `->`, `else`, `in`, `..`): read by the constructs that
    /// spell them, never an operand.
    fn is_delimiter(&self, id: DyadPtr) -> bool {
        // SAFETY: `id` is null or a resolved identity from the store.
        unsafe {
            !id.is_null()
                && dyad::ty(id) == self.types.type_
                && crate::identities::meta::kind_of(id) == Some(crate::identities::meta::TOKEN_TAG)
        }
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
    /// `next` is `None` for a `(` a bracket reader peeks at unlexed.
    pub(super) fn condition_ends(&self, tape: &ParsingTape, next: Option<&Cell>) -> bool {
        let id = next.map_or(self.types.open_, |c| self.cell_identity(c));
        let open = id == self.types.open_;
        if !open && next.is_some_and(|c| !c.constructed) && self.reads_left(id) {
            return false;
        }
        let cells: Vec<Cell> = tape.iter().map(|(_, c)| *c).collect();
        let Some(last) = cells.last() else { return false };
        if self.ends_value(last) {
            return true;
        }
        if open && self.applied_to_bracket(last) {
            return false;
        }
        let readers = cells
            .iter()
            .rev()
            .take_while(|c| {
                !c.constructed
                    && !self.ends_value(c)
                    && self.precedence_of_cell(self.cell_identity(c))
                        >= crate::identities::meta::prec::OPEN
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

    /// An infix operator, a tight read, an index, or a token a construct
    /// spells between its parts; `else` ends a branch instead. An infix is
    /// what its record says: its first operand, or a type's first field, is `lhs`.
    fn reads_left(&self, id: DyadPtr) -> bool {
        use crate::identities::meta;
        if id == self.types.else_ {
            return false;
        }
        // SAFETY: `id` is null or a resolved identity; roles are read only off an operand
        // record, fields only off a record type.
        self.is_delimiter(id)
            || unsafe {
                self.identity_head(id).is_some_and(|h| match meta::kind_of(h) {
                    Some(meta::TUPLE_TAG | meta::LIST_TAG) => {
                        meta::arity_of(h) > 0
                            && crate::reflect::text_of(meta::role_of(h, 0)) == b"lhs"
                    }
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
                    _ => false,
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
                let (id, binding) = if c.is_fresh() {
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
        let items = self.construct_segment(&mut left)?;
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

    /// An infix construct's operands at reduction; `Ok(None)` when either
    /// side is structurally missing, so the caller declines and the driver
    /// shifts the token.
    pub(crate) fn binary_operands(
        &mut self,
        tape: &ParsingTape,
    ) -> Result<Option<(DyadPtr, DyadPtr)>, ParseError> {
        let (Some(&l), Some(&r)) = (tape.at(-1), tape.at(1)) else {
            return Ok(None);
        };
        if !self.is_operand_cell(&l) || !self.is_operand_cell(&r) {
            return Ok(None);
        }
        Ok(Some((self.as_operand(l)?, self.as_operand(r)?)))
    }

    /// `e` with its caret at `at`, for a constructor refusing an operand it read.
    pub(crate) fn fail_at(&mut self, at: usize, e: ParseError) -> ParseError {
        self.cx.pos = at;
        e
    }

    /// The operand of `move`/`free`: a place as the node it is reached through
    /// (`read::place_node`), never an `addr`, a value no name holds, or a hole. A bare
    /// name comes back with the `Ended` the caller hands to `mark_dead`; a name from
    /// outside a loop or `fn` body is refused, and so is one the run starts with.
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
                    Some(place) => Taken::Place(place, None),
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
            Taken::Place(identity, Some(Ended { binding }))
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

    /// Whether the cell right of the center is the unconstructed marker word `id`.
    pub(crate) fn marker_right(&self, tape: &ParsingTape, id: DyadPtr) -> bool {
        tape.at(1).is_some_and(|cell| !cell.constructed && self.cell_identity(cell) == id)
    }

    /// Reject a capture: a name of an enclosing function's frame, not the innermost open
    /// one. v1 has no closures; reading one would resolve against the wrong activation
    /// record at run time. A `share` initializer runs at the definition, where no call of
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

    /// A value made once (`share`, `immediate`) runs at parse, where a name of the
    /// loop or branch around it has no value yet.
    fn check_made(&self, binding: DyadPtr) -> Result<(), ParseError> {
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
            // A `share` name is one place for the run, reachable wherever its function is.
            // SAFETY: as above.
            let shared = unsafe { Binding::has_gate(binding, self.types.share_) };
            if !shared
                && !self.cx.scopes.is_open(fields.scope)
                && !self.outer.iter().any(|c| c.scopes.is_open(fields.scope))
                && !self.imports.sections.contains(&fields.scope)
            {
                return Err(ParseError::Resolve(ResolveError::OutOfScope(name())));
            }
            if fields.is_dead() {
                return Err(ParseError::Resolve(ResolveError::Dead(name())));
            }
            self.note_outer_read(binding);
        }
        Ok(())
    }
}
