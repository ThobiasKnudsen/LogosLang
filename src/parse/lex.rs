// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Lexing: one token at a time from the source or a held body's feed, and the token
//! cursor the constructors read with.

use super::*;

/// The extent of a token that reads its own: the byte length of the token at the start of
/// the text, its spelling first.
pub type ExtentFn = fn(&str) -> Result<usize, ResolveError>;

/// The one door every lexer takes to the token at the start of `text`: the selection
/// rule's winner, its extent the trie's match unless its record says it reads its own
/// (DESIGN ›Unknown spellings are two pattern identities‹). With `fresh` the
/// fresh-spelling patterns compete too.
pub(crate) fn token(
    scopes: &ScopeStack,
    trie: &RegexTrie,
    text: &str,
    fresh: bool,
) -> Result<Resolved, ResolveError> {
    let mut r = if fresh { scopes.lex(trie, text)? } else { scopes.resolve(trie, text)? };
    // SAFETY: a resolved identity is null or a dyad from the store.
    if let Some(read) = unsafe { crate::identities::meta::extent_reader(r.identity) } {
        r.matched = read(text)?;
    }
    Ok(r)
}

/// The lex step the driver and `lex «…»` share: a fresh spelling is a cell with no node,
/// the pattern's construction done at the lex. `None` at the end of the text; `text` must
/// outlive the cell.
pub(crate) fn lex_token(
    scopes: &ScopeStack,
    trie: &RegexTrie,
    text: &str,
    pos: usize,
) -> Result<Option<(Cell, usize)>, ResolveError> {
    let bytes = text.as_bytes();
    let mut start = pos;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    if start >= bytes.len() {
        return Ok(None);
    }
    let r = token(scopes, trie, &text[start..], true)?;
    // DESIGN ›An unknown spelling stays text on the tape; `:=` makes the binding, and the
    // node comes with the value‹.
    let dyad = if r.fresh { std::ptr::null_mut() } else { r.binding };
    // SAFETY: `text` outlives the cell (the caller's contract); `dyad` is null or the index's binding.
    let cell = unsafe { Cell::lexed(dyad, text, start, r.matched) };
    Ok(Some((cell, start + r.matched)))
}

/// Every token of `text` as an unconstructed cell on a fresh tape centered on
/// its first cell: the fragment `insert` splices (DESIGN ›Text is the quote‹), and a
/// held body lexed once. `text` must outlive the fragment's reads.
pub fn lex_fragment(
    scopes: &ScopeStack,
    trie: &RegexTrie,
    text: &str,
) -> Result<ParsingTape, ResolveError> {
    let mut tape = ParsingTape::new();
    let mut pos = 0;
    while let Some((cell, next)) = lex_token(scopes, trie, text, pos)? {
        tape.push(cell);
        pos = next;
    }
    tape.set_cursor(0);
    Ok(tape)
}

/// The cells of a held run body being constructed, served by the cursor's
/// position in the body's text so a boundary's rewind re-exposes a cell as it
/// does a token; an undeclared spelling resolves at `base_depth` and above only.
pub(super) struct Feed {
    pub(super) cells: Vec<Cell>,
    pub(super) next: usize,
    pub(super) base_depth: usize,
    /// A held `type (…)` builds now, so its parse order is its run order.
    pub(super) in_run_order: bool,
}

impl<'a> Parser<'a> {
    /// The next token of the source as a tape cell with its offset, or `None`
    /// at the end; a spelling the trie does not know becomes a fresh-name
    /// cell, declared by a following `:=` or reported at the boundary.
    pub(super) fn lex_cell(&mut self) -> Result<Option<(Cell, usize)>, ParseError> {
        let in_run_order = match &self.cx.feed {
            Some(feed) => feed.in_run_order,
            None => true,
        };
        let mut cell = if self.cx.feed.is_some() {
            let Some(cell) = self.feed_take() else { return Ok(None) };
            cell
        } else {
            self.skip_whitespace();
            let source = self.cx.source;
            let start = self.cx.pos;
            let Some((cell, next)) = lex_token(&self.cx.scopes, self.trie, source, start)
                .map_err(|e| self.lex_error(start, e))?
            else {
                return Ok(None);
            };
            self.cx.pos = next;
            if self.unbuilt(&cell) {
                self.cx.pos = cell.start;
                return Err(ParseError::Resolve(ResolveError::Unbuilt(
                    cell.spelling().to_string(),
                )));
            }
            cell
        };
        // A box is read by the pass wherever it stands as an operand, and the
        // read is honest only if everything parsed before it has run; so where
        // parse order is run order, its cell is the point the pass runs to.
        if in_run_order && self.cx.runtime_depth == 0 && !cell.is_fresh() {
            // SAFETY: the binding the trie resolved is a dyad from the store.
            let is_box = unsafe {
                matches!(
                    crate::identities::read::read_kind(self.types, cell.identity(self.types)),
                    crate::identities::read::Read::Container(t) if !t.is_null()
                )
            };
            if is_box {
                self.drain()?;
            }
            // The running call's slot is that call's value (DESIGN ›A type is a
            // comptime value‹): a use of it in the held body is the type it holds.
            let id = cell.identity(self.types);
            if self.is_live_call_slot(id) {
                let held = self.settled_type(id);
                if held != id {
                    cell.dyad = held;
                }
            }
        }
        Ok(Some((cell, cell.start)))
    }

    /// A slot of the call a held `type (…)` is being built in, which is live: a name of the
    /// innermost function open where the body was written, with no function opened since.
    fn is_live_call_slot(&self, id: DyadPtr) -> bool {
        // SAFETY: `id` is null or a resolved dyad from the store.
        matches!(unsafe { self.types.frame_of(id) },
            Some((crate::binding::Frame::Call(f), _))
                if self.cx.held_depth > 0
                    && self.cx.frames.len() == self.cx.held_depth
                    && self.cx.frames.last().is_some_and(|open| open.frame == f))
    }

    /// A place holding a type denotes the type it holds, since lexing a box's
    /// cell first ran everything before it (DESIGN ›A type is a comptime
    /// value‹); in a deferred body, or a frame slot, the place denotes itself,
    /// except a slot of the call a held `type (…)` is being built in, which is live.
    pub(super) fn settled_type(&self, id: DyadPtr) -> DyadPtr {
        if self.cx.runtime_depth > 0 {
            return id;
        }
        // SAFETY: `id` is null or a resolved dyad from the store; the read is of a place this parser allocated, eight bytes wide.
        unsafe {
            if id.is_null() {
                return id;
            }
            // `type ?` and `dyad ?` both store a node's address, so both are
            // read the same way.
            let ty = match crate::identities::read::read_kind(self.types, id) {
                crate::identities::read::Read::Container(t) if !t.is_null() => t,
                _ => return id,
            };
            use crate::binding::Frame;
            let addr = match self.types.frame_of(id) {
                Some((Frame::Root, off)) => self.rt.store.arena_at(off),
                Some((Frame::Call(_), _)) if self.is_live_call_slot(id) => {
                    match self.rt.place_addr(id) {
                        Some(addr) => addr,
                        None => return id,
                    }
                }
                Some((Frame::Call(_), _)) | None => return id,
            };
            let held = std::ptr::read_unaligned(addr as *const DyadPtr);
            if held.is_null() {
                return id;
            }
            // A `type ?` box may hold only an identity; a `dyad ?` box holds anything.
            if ty == self.types.type_
                && crate::identities::type_identity_of(self.types, held).is_none()
            {
                return id;
            }
            held
        }
    }

    /// `Cell::identity` through `settled_type`: what the driver dispatches on.
    pub(super) fn cell_identity(&self, cell: &Cell) -> DyadPtr {
        self.settled_type(cell.identity(self.types))
    }

    /// The held cell whose spelling starts at the cursor, if any; the running
    /// index is corrected either way, since a boundary rewinds the cursor.
    pub(super) fn feed_index(&mut self) -> Option<usize> {
        self.skip_whitespace();
        let pos = self.cx.pos;
        let feed = self.cx.feed.as_mut()?;
        while feed.next > 0 && feed.cells[feed.next - 1].start >= pos {
            feed.next -= 1;
        }
        while feed.next < feed.cells.len() && feed.cells[feed.next].start < pos {
            feed.next += 1;
        }
        (feed.next < feed.cells.len() && feed.cells[feed.next].start == pos).then_some(feed.next)
    }

    /// Under a held body's construction only the body's own names count, at
    /// the feed's base depth and above: a name declared after the body, in a
    /// scope it is written in, is not one it could see.
    pub(super) fn resolve_fresh(&self, spelling: &str) -> Result<Resolved, ResolveError> {
        let r = self.cx.scopes.resolve(self.trie, spelling)?;
        if let Some(feed) = &self.cx.feed {
            if !self.cx.scopes.position(r.scope).is_some_and(|p| p >= feed.base_depth) {
                return Err(ResolveError::Unknown(spelling.to_string()));
            }
        }
        Ok(r)
    }

    /// An unresolved cell stays fresh for the boundary to report.
    fn feed_resolve(&self, cell: &mut Cell) {
        if !cell.is_fresh() {
            return;
        }
        if let Ok(r) = self.resolve_fresh(cell.spelling()) {
            if r.matched == cell.len {
                cell.dyad = r.binding;
                if self.unbuilt(cell) {
                    cell.dyad = std::ptr::null_mut();
                }
            }
        }
    }

    /// A name whose `:=` is still driving its value: its binding has no node yet and is
    /// no storage, so a use of it would read nothing.
    fn unbuilt(&self, cell: &Cell) -> bool {
        let binding = cell.binding(self.types);
        if binding.is_null() {
            return false;
        }
        // SAFETY: a cell's binding is a binding dyad from the store.
        let b = unsafe { Binding::read(binding) };
        b.dyad.is_null() && b.frame.is_null()
    }

    fn feed_take(&mut self) -> Option<Cell> {
        let i = self.feed_index()?;
        let mut cell = self.cx.feed.as_ref()?.cells[i];
        self.cx.feed.as_mut()?.next = i + 1;
        self.cx.pos = cell.end();
        self.feed_resolve(&mut cell);
        Some(cell)
    }

    fn feed_peek(&mut self) -> Option<(DyadPtr, usize)> {
        let i = self.feed_index()?;
        let mut cell = self.cx.feed.as_ref()?.cells[i];
        self.feed_resolve(&mut cell);
        if cell.is_fresh() {
            return None;
        }
        Some((cell.identity(self.types), cell.len))
    }

    /// [`token`] at `pos` in the source.
    pub(super) fn token_at(&mut self, pos: usize, fresh: bool) -> Result<Resolved, ParseError> {
        let source = self.cx.source;
        token(&self.cx.scopes, self.trie, &source[pos..], fresh).map_err(|e| self.lex_error(pos, e))
    }

    /// A token that did not lex: an unclosed `«` or `{` is reported where it stands, `base`
    /// being where the failed lookup's text began.
    pub(super) fn lex_error(&mut self, base: usize, e: ResolveError) -> ParseError {
        if let ResolveError::Unclosed { at, .. } = e {
            self.cx.pos = base + at;
        }
        ParseError::Resolve(e)
    }

    /// Never past a `#`: the sequence parser peeks at a statement-level `#`
    /// itself.
    pub(super) fn skip_whitespace(&mut self) {
        let bytes = self.cx.source.as_bytes();
        while self.cx.pos < bytes.len() && bytes[self.cx.pos].is_ascii_whitespace() {
            self.cx.pos += 1;
        }
    }

    /// After a parse error, the stuck point `crate::report` renders; after a
    /// success, where consumption stopped, so a caller can check for trailing
    /// input.
    pub fn offset(&self) -> usize {
        self.cx.pos
    }

    /// `None` at end of input or when nothing resolves.
    pub(super) fn peek_token(&mut self) -> Option<(DyadPtr, usize)> {
        if self.cx.feed.is_some() {
            return self.feed_peek();
        }
        self.skip_whitespace();
        let source = self.cx.source;
        if self.cx.pos >= source.len() {
            return None;
        }
        let r = token(&self.cx.scopes, self.trie, &source[self.cx.pos..], false).ok()?;
        Some((r.identity, r.matched))
    }

    pub(super) fn consume_token(&mut self, id: DyadPtr) -> bool {
        match self.peek_token() {
            Some((t, matched)) if t == id => {
                self.cx.pos += matched;
                true
            }
            _ => false,
        }
    }

    pub(crate) fn expect_open(&mut self) -> Result<(), ParseError> {
        if self.consume_token(self.types.open_) {
            Ok(())
        } else {
            Err(ParseError::ExpectedOpen)
        }
    }

    pub(crate) fn at_open(&mut self) -> bool {
        matches!(self.peek_token(), Some((id, _)) if id == self.types.open_)
    }

    /// The `,` or `else` after an `if`'s bare branch, left to the enclosing scope and the `if`.
    pub(super) fn at_branch_end(&mut self) -> bool {
        matches!(self.peek_token(), Some((id, _)) if id == self.types.sep_ || id == self.types.else_)
    }

    pub(super) fn consume_separator(&mut self) -> bool {
        self.consume_token(self.types.sep_)
    }

    /// `)` or `]`: a closer ends the body, and the opener's own expect-helper
    /// checks it is the matching one.
    pub(super) fn at_close(&mut self) -> bool {
        matches!(self.peek_token(), Some((id, _)) if id == self.types.close_ || id == self.types.close_sq_)
    }

    /// One spelling at the cursor, a declared name or a fresh run, never a
    /// bracket, the separator, a quote or the comment mark: a fresh name is
    /// not yet in the index, and a member resolves in its owner's scope, not here.
    pub(super) fn lex_spelling(&mut self) -> Option<(usize, usize)> {
        self.lex_spelling_fresh().map(|(start, len, _)| (start, len))
    }

    /// `lex_spelling` keeping whether the spelling is fresh: what a position
    /// that may hold a declaration or a use, `for`'s first cell, decides by.
    pub(super) fn lex_spelling_fresh(&mut self) -> Option<(usize, usize, bool)> {
        if self.cx.feed.is_some() {
            let i = self.feed_index()?;
            let spelling = self.cx.feed.as_ref()?.cells[i].spelling();
            let no_spelling = matches!(
                spelling.as_bytes().first(),
                Some(b'(' | b')' | b'[' | b']' | b',' | b'#') | None
            ) || spelling.starts_with('«');
            if no_spelling {
                return None;
            }
            let cell = self.feed_take()?;
            return Some((cell.start, cell.len, cell.is_fresh()));
        }
        self.skip_whitespace();
        let source = self.cx.source;
        let start = self.cx.pos;
        let rest = source.get(start..)?;
        if matches!(rest.as_bytes().first(), Some(b'(' | b')' | b'[' | b']' | b',' | b'#') | None)
            || rest.starts_with('«')
        {
            return None;
        }
        let r = self.cx.scopes.lex(self.trie, rest).ok()?;
        self.cx.pos = start + r.matched;
        Some((start, r.matched, r.fresh))
    }

    pub(crate) fn expect_close(&mut self) -> Result<(), ParseError> {
        if self.cx.feed.is_some() {
            return if self.consume_token(self.types.close_) {
                Ok(())
            } else {
                Err(ParseError::UnclosedBracket)
            };
        }
        self.skip_whitespace();
        let source = self.cx.source;
        if self.cx.pos >= source.len() {
            return Err(ParseError::UnclosedBracket);
        }
        let start = self.cx.pos;
        let r = self.token_at(start, false)?;
        if r.identity == self.types.close_ {
            self.cx.pos = start + r.matched;
            Ok(())
        } else {
            Err(ParseError::UnclosedBracket)
        }
    }

    /// A `)` there is the mismatched closer, the same error.
    pub(crate) fn expect_close_sq(&mut self) -> Result<(), ParseError> {
        if self.consume_token(self.types.close_sq_) {
            Ok(())
        } else {
            Err(ParseError::UnclosedBracket)
        }
    }

    pub(super) fn consume_else(&mut self) -> bool {
        self.consume_token(self.types.else_)
    }

    pub(super) fn expect_arrow(&mut self) -> Result<(), ParseError> {
        if self.consume_token(self.types.arrow_) {
            Ok(())
        } else {
            Err(ParseError::ExpectedArrow)
        }
    }
}
