// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Lexing: one token at a time from the source or a held body's feed, and the token
//! cursor the constructors read with.

use std::ops::Range;

use super::*;

/// The `{…}` scopes a token opened, each the byte range of an interior.
pub type Opened = Vec<Range<usize>>;

/// What a token that reads its own extent read: its byte length, its spelling first, and
/// the scopes it opened, both from the token's start.
pub struct Extent {
    pub len: usize,
    pub scopes: Opened,
}

pub type ExtentFn = fn(&str) -> Result<Extent, ResolveError>;

/// The one door every lexer takes to the token at the start of `text`: the selection
/// rule's winner, its extent the trie's match unless its record says it reads its own
/// (DESIGN ›Unknown spellings are two pattern identities‹), with the scopes such a token
/// opened. With `fresh` the fresh-spelling patterns compete too, and `left` is what stands
/// left of `text`.
pub(crate) fn token(
    scopes: &ScopeStack,
    trie: &RegexTrie,
    text: &str,
    fresh: bool,
    left: Left,
) -> Result<(Resolved, Opened), ResolveError> {
    let mut r = if fresh { scopes.lex(trie, text, left)? } else { scopes.resolve(trie, text)? };
    // SAFETY: a resolved identity is null or a dyad from the store.
    let Some(read) = (unsafe { crate::identities::meta::extent_reader(r.identity) }) else {
        return Ok((r, Vec::new()));
    };
    let extent = read(text)?;
    r.matched = extent.len;
    Ok((r, extent.scopes))
}

/// The lex step the driver and `lex «…»` share: a fresh spelling lexes to a node of type
/// `unknown_name` holding it, built into `store`, the pattern's construction done at the
/// lex. The cell comes with the `{…}` scopes it opened, as ranges of `text`. `None` at the
/// end of the text; `text` must outlive the cell. `before` is the cells lexed left of it on
/// its tape, nearest first.
pub(crate) fn lex_token<'c>(
    scopes: &ScopeStack,
    trie: &RegexTrie,
    store: &mut Store,
    types: &Core,
    text: &str,
    pos: usize,
    before: impl IntoIterator<Item = &'c Cell>,
) -> Result<Option<(Cell, Opened)>, ResolveError> {
    let bytes = text.as_bytes();
    let mut start = pos;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    if start >= bytes.len() {
        return Ok(None);
    }
    let mut before = before.into_iter().peekable();
    let after_dot =
        before.peek().is_some_and(|c| !c.constructed && c.identity(types) == types.dot_);
    let left = if after_dot {
        Left::Member
    } else if !value_ends(types, before) {
        Left::Nothing
    } else if start > 0 && bytes[start - 1].is_ascii_whitespace() {
        Left::SpacedValue
    } else {
        Left::Value
    };
    let (r, opened) = token(scopes, trie, &text[start..], true, left)?;
    let dyad = if r.fresh {
        crate::identities::fresh::node(store, types, &text[start..start + r.matched])
    } else {
        r.binding
    };
    // SAFETY: `text` outlives the cell (the caller's contract); `dyad` is the index's
    // binding or the node just built.
    let cell = unsafe { Cell::lexed(dyad, text, start, r.matched) };
    Ok(Some((cell, opened.into_iter().map(|s| start + s.start..start + s.end).collect())))
}

/// Whether a value ends where `before` ends, the cells left of a token, nearest first: a built
/// cell, a held bracket, a closer, a literal, `?`, a name, a function or a type that takes a value
/// does; an opener, `,`, an operator, a prefix word or a type whose `parse` is written in Logos
/// does not, and a tight read leaves what stands left of it (`x@`). What tells apart the two
/// identities `-` spells (›`-` negates where a space stands before it and none after, or no value
/// stands to its left; any other `-` subtracts‹ [[R157]]).
fn value_ends<'c>(types: &Core, before: impl IntoIterator<Item = &'c Cell>) -> bool {
    use crate::identities::meta;
    for cell in before {
        let id = cell.identity(types);
        let closer = types.brackets().into_iter().any(|(_, close)| close == id);
        if cell.constructed || cell.held.is_some() || id == types.unknown || closer {
            return true;
        }
        // SAFETY: a lexed cell's identity is a dyad from the store; `kind_of` turns away one
        // that carries no record, and a record holds its rank and its constructor.
        let (kind, rank, parse) = unsafe {
            match (!id.is_null()).then(|| meta::kind_of(id)).flatten() {
                Some(kind) => {
                    (kind, meta::parse_rank_of(id), meta::written_parse_of(id, types.fn_type))
                }
                None => return true,
            }
        };
        if rank == meta::prec::TIGHT {
            continue;
        }
        return match kind {
            meta::TUPLE_TAG | meta::LIST_TAG | meta::TOKEN_TAG => rank == meta::prec::LITERAL,
            meta::RECORD_TAG => parse.is_null(),
            _ => true,
        };
    }
    false
}

/// Every token of `text` as an unconstructed cell on a fresh tape centered on
/// its first cell: the fragment `insert` splices (DESIGN ›Text is the quote‹), and a
/// held body lexed once. The cells of the `{…}` scopes its quotes opened are lexed with
/// them, beside the frontier. `text` must outlive the fragment's reads.
pub fn lex_fragment(
    scopes: &ScopeStack,
    trie: &RegexTrie,
    store: &mut Store,
    types: &Core,
    text: &str,
) -> Result<ParsingTape, ResolveError> {
    let mut tape = ParsingTape::new();
    let mut pos = 0;
    while let Some((cell, opened)) =
        lex_token(scopes, trie, store, types, text, pos, tape.iter_back())?
    {
        tape.push(cell);
        pos = cell.end();
        lex_scopes(scopes, trie, store, types, text, opened, &mut tape.inner)?;
    }
    tape.set_cursor(0);
    Ok(tape)
}

/// The cells of each scope, a scope inside one included; `text` is cut at the scope's end,
/// so its lexing stops there.
fn lex_scopes(
    scopes: &ScopeStack,
    trie: &RegexTrie,
    store: &mut Store,
    types: &Core,
    text: &str,
    opened: Opened,
    into: &mut Vec<Cell>,
) -> Result<(), ResolveError> {
    for scope in opened {
        let mut pos = scope.start;
        let mut own: Vec<Cell> = Vec::new();
        while let Some((cell, deeper)) =
            lex_token(scopes, trie, store, types, &text[..scope.end], pos, own.iter().rev())?
        {
            into.push(cell);
            own.push(cell);
            pos = cell.end();
            lex_scopes(scopes, trie, store, types, text, deeper, into)?;
        }
    }
    Ok(())
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
    /// cell, declared by a following `:=` or reported at the boundary. `tape` holds the
    /// cells lexed left of it.
    pub(super) fn lex_cell(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Option<(Cell, usize)>, ParseError> {
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
            // The scopes a quote opened are lexed by whoever builds them, from the source.
            let lexed = lex_token(
                &self.cx.scopes,
                self.trie,
                self.rt.store,
                self.types,
                source,
                start,
                tape.iter_back(),
            );
            let Some((cell, _)) = lexed.map_err(|e| self.lex_error(start, e))? else {
                return Ok(None);
            };
            self.cx.pos = cell.end();
            if self.unbuilt(&cell) {
                self.cx.pos = cell.start;
                return Err(ParseError::Resolve(ResolveError::Unbuilt(
                    cell.spelling().to_string(),
                )));
            }
            cell
        };
        // A place holding a node is read by the pass wherever it stands as an operand, and the
        // read is honest only if everything parsed before it has run; so where parse order is
        // run order, its cell is the point the pass runs to. A box, `type ?` or `dyad ?`, is
        // read for the identity it holds, so the part of its own line before it runs first too
        // (›The pass runs only as far as it must, in order, and never twice‹ [[R288]]).
        if in_run_order && self.cx.runtime_depth == 0 {
            use crate::identities::read::{read_kind, Read};
            let id = cell.identity(self.types);
            // SAFETY: the binding the trie resolved, or the fresh spelling's node, is a dyad
            // from the store.
            let kind = unsafe { read_kind(self.types, id) };
            let dyad_box = kind == Read::Container(self.types.dyad_);
            if dyad_box {
                self.run_first(tape, cell.start)?;
            } else if self.is_box(id) {
                self.read_type_box(tape, id, cell.start)?;
            } else if matches!(kind, Read::Container(t) if !t.is_null()) {
                self.drain()?;
            }
            if dyad_box {
                if let Some(held) = self.read_through(id)? {
                    self.cx.each_pass_read |= self.cx.each_pass;
                    cell.dyad = held;
                }
            } else if self.is_live_call_slot(id) {
                // The running call's slot is that call's value (DESIGN ›A type is a
                // comptime value‹): a use of it in the held body is the type it holds.
                let held = self.settled_type(id);
                if held != id {
                    cell.dyad = held;
                }
            }
        }
        Ok(Some((cell, cell.start)))
    }

    /// Whether `id` names a box, a `type ?` or `dyad ?` place, read for the identity it holds
    /// (›A `type ?` place is a box, written any number of times‹ [[R291]]).
    pub(super) fn is_box(&self, id: DyadPtr) -> bool {
        use crate::identities::read::{read_kind, Read};
        // SAFETY: `id` is null or a resolved dyad from the store.
        match unsafe { read_kind(self.types, id) } {
            Read::Container(t) => t == self.types.type_ || t == self.types.dyad_,
            _ => false,
        }
    }

    /// What a read of `id`, a `dyad` name, stands for once the pass has run everything before
    /// it ([`Self::held_now`]); a name `=` writes keeps its place, and so does one `.` reads a
    /// member of, which reads it through the place.
    fn read_through(&mut self, id: DyadPtr) -> Result<Option<DyadPtr>, ParseError> {
        let types = self.types;
        if matches!(self.peek_token(), Some((t, _)) if t == types.assign || t == types.dot_) {
            return Ok(None);
        }
        self.held_now(id)
    }

    /// The place of the `dyad` name `cell` spells, where the lexer read it through to the node it
    /// holds ([`Self::read_through`]), so the cell holds that node and no binding; `None` for any
    /// other cell.
    pub(super) fn name_read_through(&self, cell: &Cell) -> Option<DyadPtr> {
        let types = self.types;
        if cell.constructed || cell.is_fresh(types) || !cell.binding(types).is_null() {
            return None;
        }
        let named = self.cx.scopes.resolve(self.trie, cell.spelling()).ok()?;
        if named.matched != cell.len || named.binding.is_null() {
            return None;
        }
        // SAFETY: a resolved name's binding is a dyad from the store.
        let place = unsafe { types.through(named.binding) };
        // SAFETY: `place` is the dyad the binding names, or the binding itself.
        (place != cell.dyad && unsafe { self.is_dyad_place(place) }).then_some(place)
    }

    /// Whether `place` is a `dyad` place the pass reads where it runs its line: a name, or an
    /// entry of a `{ … }` reached by a field path from a name or from the value a body is about,
    /// so that reading it runs nothing but the read.
    ///
    /// # Safety
    /// `place` must be null or a reduced dyad from the store.
    pub(super) unsafe fn is_dyad_place(&self, place: DyadPtr) -> bool {
        use crate::identities::{read, this};
        let types = self.types;
        if types.is_storage(place) {
            return read::read_kind(types, place) == read::Read::Container(types.dyad_);
        }
        this::is_field_read(types, place)
            && read::output_type(types, place) == types.dyad_
            && this::reached_by_name(types, place)
    }

    /// What a read of `place`, a `dyad` place ([`Self::is_dyad_place`]), stands for where the
    /// pass runs its line, once everything before the read has run: the node it holds now (›A
    /// `dyad ?` place is transparent: a placeholder for a new node of any type‹ [[R293]]). A name
    /// and an entry of a `{ … }` read alike, the entry through the value it is reached by or
    /// named bare in a `fn` entry (›`{ … }` outside text is a scope kept as a value, read with
    /// `.`‹ [[R374]]). `None` where it holds nothing or a type, which is read by dispatching on
    /// it, the name staying the box (›A block settles its boxes as the top level does‹
    /// [[R295]]), and for any other operand.
    pub(super) fn held_now(&mut self, place: DyadPtr) -> Result<Option<DyadPtr>, ParseError> {
        let types = self.types;
        // SAFETY: `place` is a reduced dyad from the store.
        if !self.in_pass_order() || !unsafe { self.is_dyad_place(place) } {
            return Ok(None);
        }
        // SAFETY: as above.
        let held = if unsafe { types.is_storage(place) } {
            self.settled_type(place)
        } else {
            // SAFETY: `place` is a field read reached by names alone, so running it runs only
            // reads, of the value the call running this line is about.
            unsafe { crate::identities::this::held(&mut self.rt, place) }
                .map_err(ParseError::Run)?
        };
        // SAFETY: `held` is `place` or the live node the place holds now.
        if held == place || unsafe { crate::identities::type_identity_of(types, held) }.is_some() {
            return Ok(None);
        }
        // SAFETY: `held` is the live node the place holds now.
        Ok(Some(unsafe { crate::identities::dyad_place::stable(self.rt.store, types, held) }))
    }

    /// A slot of the call a held `type (…)` is being built in, which is live: a name of the
    /// innermost function open where the body was written, with no function opened since.
    fn is_live_call_slot(&self, id: DyadPtr) -> bool {
        // SAFETY: `id` is null or a resolved dyad from the store.
        let Some((crate::binding::Frame::Call(f), _)) = (unsafe { self.types.frame_of(id) }) else {
            return false;
        };
        self.cx.held_depth > 0
            && self.cx.frames.len() == self.cx.held_depth
            && self.cx.frames.last().is_some_and(|open| open.frame == f)
            // A field read is reached through its record's name.
            // SAFETY: a dyad with a frame is a binding or a field read.
            && unsafe {
                crate::dyad::ty(id) != self.types.binding_ || crate::binding::Binding::is_named(id)
            }
    }

    /// A place holding a type denotes the type it holds, since lexing a box's
    /// cell first ran everything before it (DESIGN ›A type is a comptime
    /// value‹); in a deferred body, or a frame slot, the place denotes itself,
    /// except a slot of the call a held `type (…)` is being built in, which is live; so does
    /// a box read after a part of its line that cannot run first yet.
    pub(super) fn settled_type(&self, id: DyadPtr) -> DyadPtr {
        if self.cx.runtime_depth > 0 || self.unsettled_at(id).is_some() {
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
    pub(crate) fn cell_identity(&self, cell: &Cell) -> DyadPtr {
        self.settled_type(cell.identity(self.types))
    }

    /// The held cell whose spelling starts at the cursor, if any, and ends within the source, which
    /// a parse cut short ends early as it does for the lexer; the running index is corrected
    /// either way, since a boundary rewinds the cursor.
    pub(super) fn feed_index(&mut self) -> Option<usize> {
        self.skip_whitespace();
        let (pos, end) = (self.cx.pos, self.cx.source.len());
        let feed = self.cx.feed.as_mut()?;
        while feed.next > 0 && feed.cells[feed.next - 1].start >= pos {
            feed.next -= 1;
        }
        while feed.next < feed.cells.len() && feed.cells[feed.next].start < pos {
            feed.next += 1;
        }
        let cell = feed.cells.get(feed.next)?;
        (cell.start == pos && cell.end() <= end).then_some(feed.next)
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

    /// A held cell is resolved where it is built when nothing was live for it where it was lexed,
    /// or when it lexed to the word of a body around it: the body it is built in declares its
    /// own then, which is the innermost. An unresolved cell stays as it was lexed, a fresh one
    /// for the boundary to report.
    fn feed_resolve(&self, cell: &mut Cell) {
        let binding = cell.binding(self.types);
        // SAFETY: a lexed cell's binding is null or a binding dyad from the store.
        let outer_word = !binding.is_null() && unsafe { Binding::read(binding) }.body_word;
        if !cell.is_fresh(self.types) && !outer_word {
            return;
        }
        if let Ok(r) = self.resolve_fresh(cell.spelling()) {
            if r.matched == cell.len {
                let mut bound = *cell;
                bound.dyad = r.binding;
                if !self.unbuilt(&bound) {
                    *cell = bound;
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
        if cell.is_fresh(self.types) {
            return None;
        }
        Some((cell.identity(self.types), cell.len))
    }

    /// [`token`] at `pos` in the source.
    pub(super) fn token_at(&mut self, pos: usize, fresh: bool) -> Result<Resolved, ParseError> {
        let source = self.cx.source;
        token(&self.cx.scopes, self.trie, &source[pos..], fresh, Left::Value)
            .map(|(r, _)| r)
            .map_err(|e| self.lex_error(pos, e))
    }

    /// A token that did not lex: an unclosed `«` or `{` is reported where it stands, `base`
    /// being where the failed lookup's text began.
    pub(super) fn lex_error(&mut self, base: usize, e: ResolveError) -> ParseError {
        if let ResolveError::Unmatched { at, .. } = e {
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
        let (r, _) =
            token(&self.cx.scopes, self.trie, &source[self.cx.pos..], false, Left::Value).ok()?;
        Some((r.identity, r.matched))
    }

    /// The identity of the token after the cursor, `None` at the end or where nothing resolves;
    /// at the end of a held line cut before its `=`, that `=`.
    pub(super) fn next_identity(&mut self) -> Option<DyadPtr> {
        match self.peek_token() {
            Some((id, _)) => Some(id),
            None if self.cx.cut_before_assign && self.cx.pos >= self.cx.source.len() => {
                Some(self.types.assign)
            }
            None => None,
        }
    }

    /// The token at the cursor read as the start of what follows a word, no value to its left, so
    /// a `-` there negates: `None` at the end, `Some(None)` for a spelling no identity has.
    pub(super) fn peek_start(&mut self) -> Option<Option<DyadPtr>> {
        if self.cx.feed.is_some() {
            let i = self.feed_index()?;
            let mut cell = self.cx.feed.as_ref()?.cells[i];
            self.feed_resolve(&mut cell);
            return Some((!cell.is_fresh(self.types)).then(|| cell.identity(self.types)));
        }
        self.skip_whitespace();
        let source = self.cx.source;
        let rest = source.get(self.cx.pos..).filter(|rest| !rest.is_empty())?;
        let resolved = token(&self.cx.scopes, self.trie, rest, true, Left::Nothing);
        Some(resolved.ok().filter(|(r, _)| !r.fresh).map(|(r, _)| r.identity))
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

    /// A closer ends the body, and [`Parser::take_closer`] checks it is its opener's.
    pub(super) fn at_close(&mut self) -> bool {
        matches!(self.peek_token(), Some((id, _)) if self.is_closer(id))
    }

    /// The closer that ends `opener`'s bracket, built, held or skipped alike (DESIGN ›Every
    /// identity has a `parse_rank`, and `(` works in two steps‹).
    fn closer_of(&self, opener: DyadPtr) -> Option<DyadPtr> {
        self.types.brackets().into_iter().find(|&(open, _)| open == opener).map(|(_, close)| close)
    }

    pub(super) fn is_closer(&self, id: DyadPtr) -> bool {
        self.types.brackets().into_iter().any(|(_, close)| close == id)
    }

    pub(super) fn is_opener(&self, id: DyadPtr) -> bool {
        self.closer_of(id).is_some()
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
        fn no_spelling(text: &str) -> bool {
            matches!(
                text.as_bytes().first(),
                Some(b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b'#') | None
            ) || text.starts_with('«')
        }
        if self.cx.feed.is_some() {
            let i = self.feed_index()?;
            if no_spelling(self.cx.feed.as_ref()?.cells[i].spelling()) {
                return None;
            }
            let cell = self.feed_take()?;
            return Some((cell.start, cell.len, cell.is_fresh(self.types)));
        }
        self.skip_whitespace();
        let source = self.cx.source;
        let start = self.cx.pos;
        let rest = source.get(start..)?;
        if no_spelling(rest) {
            return None;
        }
        let left = if self.cx.member_asleep { Left::Member } else { Left::Value };
        let (r, _) = token(&self.cx.scopes, self.trie, rest, true, left).ok()?;
        self.cx.pos = start + r.matched;
        Some((start, r.matched, r.fresh))
    }

    pub(crate) fn expect_close(&mut self) -> Result<(), ParseError> {
        self.take_closer(self.types.open_)
    }

    /// Where every bracket ends, built or not: the closer of `opener` at the cursor is
    /// consumed; a closer of another bracket, or the end of the text, is `UnclosedBracket`
    /// with the cursor left on it.
    pub(super) fn take_closer(&mut self, opener: DyadPtr) -> Result<(), ParseError> {
        let closer = self.closer_of(opener);
        if self.cx.feed.is_some() {
            return if closer.is_some_and(|c| self.consume_token(c)) {
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
        if Some(r.identity) == closer {
            self.cx.pos = start + r.matched;
            Ok(())
        } else {
            Err(ParseError::UnclosedBracket)
        }
    }

    /// The bracket `opener` opened just behind the cursor, read past its closer and built
    /// by nothing: token by token through the one door, a quote or a `#` comment being one
    /// token and an inner bracket read the same way. Gives where the closer stood.
    pub(super) fn skip_bracket(&mut self, opener: DyadPtr) -> Result<usize, ParseError> {
        self.check_stack()?;
        loop {
            self.skip_whitespace();
            let at = self.cx.pos;
            if at >= self.cx.source.len() {
                return Err(ParseError::UnclosedBracket);
            }
            let r = self.token_at(at, true)?;
            let id = r.identity;
            if self.is_closer(id) {
                return self.take_closer(opener).map(|()| at);
            }
            self.cx.pos += r.matched;
            if self.closer_of(id).is_some() {
                self.skip_bracket(id)?;
            }
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
