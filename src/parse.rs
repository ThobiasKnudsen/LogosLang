// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The parsing tape and the driver: the scope's constructor lexing one token
//! at a time and constructing the segment's cells highest parse_rank first
//! (DESIGN ›The scope's constructor is the driver‹). Also the scope stack and
//! name resolution over it; the trie is only the name index.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::binding::Binding;
use crate::dyad::DyadPtr;
use crate::regex_trie::{RegexTrie, RegexTrieError};
use crate::store::Store;
use crate::Core;

/// One cell of the tape: a dyad pointer (a binding, a fresh dyad, or the built
/// node) plus the tape's own two facts, the flag and the lexed spelling
/// (DESIGN ›The scope's constructor is the driver‹).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The binding, the fresh dyad, or the node.
    pub dyad: DyadPtr,
    pub constructed: bool,
    /// The constructed cell is the group a `(` landed, so the identity to its
    /// left may claim it (`X (…)` is X's decision); goes when the seed stops
    /// unwrapping a one-expression group to its expression.
    pub bracket: bool,
    /// The text `start..start + len` indexes; `None` for a cell nothing lexed.
    /// It must outlive every read of the cell.
    text: Option<std::ptr::NonNull<str>>,
    /// Byte offset of the spelling in its text (0 when never lexed).
    pub start: usize,
    /// Byte length of that spelling.
    pub len: usize,
}

impl Cell {
    /// # Safety
    /// `text` must outlive every read of the cell's spelling; `dyad` must be
    /// null or a dyad from the store.
    pub unsafe fn lexed(dyad: DyadPtr, text: &str, start: usize, len: usize) -> Self {
        Cell {
            dyad,
            constructed: false,
            bracket: false,
            text: Some(std::ptr::NonNull::from(text)),
            start,
            len,
        }
    }

    /// # Safety
    /// `dyad` must be null or a dyad from the store.
    pub unsafe fn built(dyad: DyadPtr) -> Self {
        Cell { dyad, constructed: true, bracket: false, text: None, start: 0, len: 0 }
    }

    /// `tape.spelling[k]`: the empty text for a cell nothing lexed, so a
    /// constructor can test for a lexed cell with one read and no fault.
    pub fn spelling(&self) -> &str {
        match self.text {
            // SAFETY: `lexed` took a `&str` that outlives the cell.
            Some(p) => unsafe { p.as_ref() }.get(self.start..self.end()).unwrap_or(""),
            None => "",
        }
    }

    /// One past the cell's last byte.
    pub fn end(&self) -> usize {
        self.start + self.len
    }

    /// The binding read through to its dyad, a fresh dyad or a node itself.
    pub fn identity(&self, types: &Core) -> DyadPtr {
        // SAFETY: a cell's dyad is null or a dyad from the store.
        unsafe { types.through(self.dyad) }
    }

    /// Null when the cell holds no binding (a fresh spelling, a minted
    /// identity, a constructed node).
    pub fn binding(&self, types: &Core) -> DyadPtr {
        // SAFETY: a cell's dyad is null or a dyad from the store.
        if !self.constructed && !self.dyad.is_null() && unsafe { (*self.dyad).ty } == types.binding_
        {
            self.dyad
        } else {
            std::ptr::null_mut()
        }
    }

    /// An unconstructed cell whose fresh dyad has both slots null.
    pub fn is_fresh(&self) -> bool {
        // SAFETY: a cell's dyad is null or a dyad from the store.
        !self.constructed && !self.dyad.is_null() && unsafe { (*self.dyad).ty }.is_null()
    }

    /// The finished group `(` leaves, which the identity to its left may
    /// claim; a *named* scope value is a binding, so `f s` is no call.
    pub fn is_bracket(&self) -> bool {
        self.constructed && self.bracket
    }
}

#[derive(Debug, Clone, Copy)]
struct Node {
    cell: Cell,
    prev: Option<usize>,
    next: Option<usize>,
    /// A removed node stays in the arena, unlinked, so a handle to it can
    /// still say that its cell is gone.
    linked: bool,
}

/// The working frontier of a scope: a doubly linked list of cells with a
/// movable center, `tape[0]` (DESIGN ›The scope's constructor is the driver‹).
/// Nodes live in an arena so handles stay valid across splices.
#[derive(Debug, Default)]
pub struct ParsingTape {
    nodes: Vec<Node>,
    head: Option<usize>,
    tail: Option<usize>,
    /// `None` is the one-past-end position.
    center: Option<usize>,
    len: usize,
    /// How many times the frontier was edited: what tells a decline from a
    /// constructor that edited the tape and left its own cell unconstructed.
    edits: u64,
}

impl ParsingTape {
    pub fn new() -> Self {
        ParsingTape { nodes: Vec::new(), head: None, tail: None, center: None, len: 0, edits: 0 }
    }

    pub fn edits(&self) -> u64 {
        self.edits
    }

    /// `None` when the cell at `mark` has been removed since: how the driver
    /// judges a constructor's outcome on the construct's own cell, whatever
    /// the center is after the call.
    pub fn cell_at_mark(&self, mark: Option<usize>) -> Option<&Cell> {
        let n = mark?;
        self.nodes[n].linked.then(|| &self.nodes[n].cell)
    }

    /// A native's `tape[k] = …`: the pointer alone; the flag and the spelling
    /// stay. `false` off the tape.
    pub fn set_dyad(&mut self, offset: isize, dyad: DyadPtr) -> bool {
        let Some(c) = self.at_mut(offset) else {
            return false;
        };
        c.dyad = dyad;
        c.bracket = false;
        self.edits += 1;
        true
    }

    /// A native's `tape.is_constructed[k] = flag`. `false` off the tape.
    pub fn set_constructed(&mut self, offset: isize, flag: bool) -> bool {
        let Some(c) = self.at_mut(offset) else {
            return false;
        };
        c.constructed = flag;
        self.edits += 1;
        true
    }

    /// A tape over `cells`, center on the first.
    #[cfg(test)]
    pub fn from_cells(cells: Vec<Cell>) -> Self {
        let mut t = ParsingTape::new();
        for c in cells {
            t.push(c);
        }
        t.center = t.head;
        t
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Past the end (no center), negative offsets walk back from the tail.
    fn node_at(&self, offset: isize) -> Option<usize> {
        let (mut n, mut steps) = match self.center {
            Some(c) => (c, offset),
            None => {
                if offset >= 0 {
                    return None;
                }
                (self.tail?, offset + 1)
            }
        };
        while steps > 0 {
            n = self.nodes[n].next?;
            steps -= 1;
        }
        while steps < 0 {
            n = self.nodes[n].prev?;
            steps += 1;
        }
        Some(n)
    }

    fn node_abs(&self, i: usize) -> Option<usize> {
        let mut n = self.head?;
        for _ in 0..i {
            n = self.nodes[n].next?;
        }
        Some(n)
    }

    pub fn at(&self, offset: isize) -> Option<&Cell> {
        self.node_at(offset).map(|n| &self.nodes[n].cell)
    }

    pub fn at_mut(&mut self, offset: isize) -> Option<&mut Cell> {
        self.node_at(offset).map(move |n| &mut self.nodes[n].cell)
    }

    /// Walks from the head: a loop over the tape uses `iter` instead.
    pub fn cell(&self, i: usize) -> Option<&Cell> {
        self.node_abs(i).map(|n| &self.nodes[n].cell)
    }

    /// Each cell with the handle of its node, which `center_on` takes: one
    /// walk of the list.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (usize, &Cell)> + '_ {
        std::iter::successors(self.head, move |&n| self.nodes[n].next)
            .map(move |n| (n, &self.nodes[n].cell))
    }

    /// `handle` is one `iter` handed out for a cell still on the tape.
    pub(crate) fn center_on(&mut self, handle: usize) {
        debug_assert!(self.nodes[handle].linked, "a handle names a cell on the tape");
        self.center = Some(handle);
    }

    pub fn last(&self) -> Option<&Cell> {
        self.tail.map(|n| &self.nodes[n].cell)
    }

    pub fn start_of(&self, i: usize) -> usize {
        self.cell(i).map(|c| c.start).unwrap_or(0)
    }

    /// The center's absolute index; `len` for the one-past-end position.
    pub fn cursor(&self) -> usize {
        let Some(c) = self.center else { return self.len };
        let mut i = 0;
        let mut n = self.head;
        while let Some(k) = n {
            if k == c {
                return i;
            }
            i += 1;
            n = self.nodes[k].next;
        }
        self.len
    }

    /// `i` at `len` or beyond puts the center past the end.
    pub fn set_cursor(&mut self, i: usize) {
        self.center = self.node_abs(i);
    }

    /// The re-centered view a constructor hands to another (`tape.recenter(k)`).
    pub fn recenter(&mut self, offset: isize) {
        self.center = self.node_at(offset);
    }

    pub fn is_constructed(&self, offset: isize) -> Option<bool> {
        self.at(offset).map(|c| c.constructed)
    }

    pub fn spelling(&self, offset: isize) -> Option<&str> {
        self.at(offset).map(Cell::spelling)
    }

    /// A copy of every cell in tape order: the snapshot `splice` takes, so a
    /// tape may be spliced into itself.
    pub fn cells(&self) -> Vec<Cell> {
        let mut out = Vec::with_capacity(self.len);
        let mut n = self.head;
        while let Some(k) = n {
            out.push(self.nodes[k].cell);
            n = self.nodes[k].next;
        }
        out
    }

    /// The center as a handle, to be put back after a lazy read moved it.
    pub fn mark(&self) -> Option<usize> {
        self.center
    }

    pub fn restore(&mut self, mark: Option<usize>) {
        self.center = mark;
    }

    fn alloc(&mut self, cell: Cell) -> usize {
        self.nodes.push(Node { cell, prev: None, next: None, linked: false });
        self.nodes.len() - 1
    }

    fn link_before(&mut self, at: Option<usize>, n: usize) {
        match at {
            Some(a) => {
                let p = self.nodes[a].prev;
                self.nodes[n].prev = p;
                self.nodes[n].next = Some(a);
                self.nodes[a].prev = Some(n);
                match p {
                    Some(p) => self.nodes[p].next = Some(n),
                    None => self.head = Some(n),
                }
            }
            None => {
                self.nodes[n].prev = self.tail;
                self.nodes[n].next = None;
                match self.tail {
                    Some(t) => self.nodes[t].next = Some(n),
                    None => self.head = Some(n),
                }
                self.tail = Some(n);
            }
        }
        self.nodes[n].linked = true;
        self.len += 1;
        self.edits += 1;
    }

    /// The node an insertion at `offset` lands before: the head past the left
    /// end, the end (`None`) past the right end.
    fn anchor(&self, offset: isize) -> Option<usize> {
        if self.center.is_none() && offset >= 0 {
            return None;
        }
        match self.node_at(offset) {
            Some(a) => Some(a),
            None if offset < 0 => self.head,
            None => None,
        }
    }

    /// Lands before the cell at `offset`, so `insert(0, ..)` is just left of
    /// the center and `insert(1, ..)` just right; the center keeps its cell.
    pub fn insert(&mut self, offset: isize, cell: Cell) {
        self.splice(offset, vec![cell]);
    }

    /// `tape.insert(k, lex «…»)`: the cells keep their flags and spellings; on
    /// an empty tape the first becomes the center (DESIGN ›Text is the quote‹).
    pub fn splice(&mut self, offset: isize, cells: Vec<Cell>) {
        let empty = self.head.is_none();
        let at = self.anchor(offset);
        for cell in cells {
            let n = self.alloc(cell);
            self.link_before(at, n);
            if empty && self.center.is_none() {
                self.center = Some(n);
            }
        }
    }

    /// Removing the center moves it to the next cell (past the end if none).
    pub fn remove(&mut self, offset: isize) -> Option<Cell> {
        let n = self.node_at(offset)?;
        let Node { cell, prev, next, .. } = self.nodes[n];
        match prev {
            Some(p) => self.nodes[p].next = next,
            None => self.head = next,
        }
        match next {
            Some(x) => self.nodes[x].prev = prev,
            None => self.tail = prev,
        }
        if self.center == Some(n) {
            self.center = next;
        }
        self.nodes[n].prev = None;
        self.nodes[n].next = None;
        self.nodes[n].linked = false;
        self.len -= 1;
        self.edits += 1;
        Some(cell)
    }

    /// Append `cell` and move the center to it: the driver's lex step.
    pub fn push(&mut self, cell: Cell) {
        let n = self.alloc(cell);
        self.link_before(None, n);
        self.center = Some(n);
    }

    /// The center cell's text while unconstructed: how an atom constructor
    /// reaches its matched text.
    pub fn own_text(&self) -> Option<&str> {
        self.at(0).filter(|c| !c.constructed).map(Cell::spelling)
    }

    /// The edit nearly every constructor ends with (`tape[0] = dyad (…)`);
    /// the span stays.
    pub fn place(&mut self, dyad: DyadPtr) {
        let cell = self.at_mut(0).expect("the construct's cell is at the center");
        cell.dyad = dyad;
        cell.constructed = true;
        cell.bracket = false;
        self.edits += 1;
    }

    pub fn place_bracket(&mut self, dyad: DyadPtr) {
        self.place(dyad);
        self.at_mut(0).expect("placed above").bracket = true;
    }

    /// An infix constructor's splice: `tape[-1]`, the center and `tape[+1]`
    /// become one constructed cell spanning all three.
    pub fn reduce_here(&mut self, dyad: DyadPtr) {
        let (Some(left), Some(right)) = (self.at(-1).copied(), self.at(1).copied()) else {
            panic!("an infix reduces between two operands");
        };
        self.remove(-1);
        self.remove(1);
        let cell = self.at_mut(0).expect("the center survives its neighbours");
        cell.dyad = dyad;
        cell.constructed = true;
        cell.bracket = false;
        cell.start = left.start;
        cell.len = right.end().saturating_sub(left.start);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// The spelling matched only a fresh-spelling pattern; only
    /// `ScopeStack::lex` ever answers `true`.
    pub fresh: bool,
    /// Bytes consumed from the start of the input.
    pub matched: usize,
    /// What a use of the name points at (DESIGN ›The dyad's read surface‹).
    pub binding: DyadPtr,
    /// The binding's dyad.
    pub identity: DyadPtr,
    /// The scope the winning declaration was made in: what a rebind that
    /// completes it must target.
    pub scope: DyadPtr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// Not in the name index at all; carries the leading word at the position.
    Unknown(String),
    /// Known, but no declaration of it is in an open scope.
    OutOfScope(String),
    /// Declaring it would shadow a declaration still live in an open scope.
    Shadowed(String),
    /// Declared in an open scope but made dead by an `own` or `drop`; only
    /// `:=` may follow (DESIGN ›Name resolution is scope-filtered‹).
    Dead(String),
    /// Two spellings of equal `lex_rank` match the same length: an
    /// inconsistency in the definitions, never a pick by declaration order.
    Tied,
    /// The name index itself rejected the lookup.
    Index(RegexTrieError),
}

/// The reader's best guess at what the writer meant as one name: the leading
/// word run, or the first character.
fn unknown_spelling(text: &str) -> String {
    let word = text.split(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("");
    if word.is_empty() {
        text.chars().next().map(String::from).unwrap_or_default()
    } else {
        word.to_string()
    }
}

/// One act on the name index since the last commit, undone newest-first by
/// rollback.
#[derive(Debug)]
enum Journal {
    /// Rollback removes the live entry.
    Declared { name: String, scope: DyadPtr },
    /// Rollback restores the `end` the binding had before.
    Ended { binding: DyadPtr, prev_end: DyadPtr },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    Start,
    End,
}

/// A range endpoint waiting for the body item that carries it: the entry holds
/// a provisional value until the declaring scope appends the finished item.
#[derive(Debug)]
struct Pending {
    /// Its own `scope` says which scope's next item settles it.
    binding: DyadPtr,
    endpoint: Endpoint,
}

/// A bare name an `own` or `drop` is about to make dead, handed back so the
/// keyword's constructor can mark it dead with the node it built.
#[derive(Debug)]
pub(crate) struct Ended {
    pub(crate) binding: DyadPtr,
}

/// The open scopes with an O(1) membership set: a cache over the parent link
/// every scope node carries (DESIGN ›Meta-navigation‹). Resolution keeps the
/// one live candidate (DESIGN ›Name resolution is scope-filtered‹).
#[derive(Debug)]
pub struct ScopeStack {
    open: Vec<DyadPtr>,
    set: HashSet<DyadPtr>,
    /// The REPL's undo log: a failed line rolls its declarations and dead
    /// marks back, so a typo never burns a name for the session.
    journal: Vec<Journal>,
    pending: Vec<Pending>,
    /// Stack depths where a body that runs again or later begins; `own`/`drop`
    /// of a name declared below one is refused (DESIGN ›Memory and concurrency‹).
    barriers: Vec<usize>,
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

impl ScopeStack {
    pub fn new() -> Self {
        ScopeStack {
            open: Vec::new(),
            set: HashSet::new(),
            journal: Vec::new(),
            pending: Vec::new(),
            barriers: Vec::new(),
        }
    }

    pub fn push(&mut self, scope: DyadPtr) {
        self.open.push(scope);
        self.set.insert(scope);
    }

    /// Open a section under the innermost open scope: its parent link is that
    /// scope, so a walk up from a body constructed inside it reaches the root.
    pub fn push_section(&mut self, store: &mut Store, scope_ty: DyadPtr) -> DyadPtr {
        let parent = *self.open.last().expect("a section opens under an open scope");
        let section = crate::identities::scope::mint(store, scope_ty, parent);
        self.push(section);
        section
    }

    /// Endpoints still pending for the scope can no longer settle and are
    /// dropped.
    pub fn pop(&mut self) -> Option<DyadPtr> {
        let s = self.open.pop()?;
        self.set.remove(&s);
        // SAFETY: every pending binding is a binding dyad from the store.
        self.pending.retain(|p| unsafe { Binding::read(p.binding).scope } != s);
        Some(s)
    }

    /// A body that runs again or later begins with the next scope pushed.
    pub fn push_barrier(&mut self) {
        self.barriers.push(self.open.len());
    }

    pub fn pop_barrier(&mut self) {
        self.barriers.pop();
    }

    /// A barrier began after `scope` was pushed, so `own`/`drop` of a name
    /// from it is refused here.
    pub fn crosses_barrier(&self, scope: DyadPtr) -> bool {
        let Some(idx) = self.position(scope) else {
            return false;
        };
        self.barriers.iter().any(|&b| b > idx)
    }

    pub fn position(&self, scope: DyadPtr) -> Option<usize> {
        self.open.iter().position(|&s| s == scope)
    }

    pub fn current(&self) -> Option<DyadPtr> {
        self.open.last().copied()
    }

    pub fn is_open(&self, scope: DyadPtr) -> bool {
        self.set.contains(&scope)
    }

    pub fn depth(&self) -> usize {
        self.open.len()
    }

    /// The REPL's recovery after an error skipped the balancing pops. The
    /// barriers of the bodies closed this way go too, or they would refuse
    /// `own`/`drop` for the rest of the session.
    pub fn truncate(&mut self, depth: usize) {
        while self.open.len() > depth {
            self.pop();
        }
        // A barrier lives exactly while the body scope it was pushed for is open.
        self.barriers.retain(|&b| b < depth);
    }

    /// A kept line: the undo log goes, and an endpoint still pending is
    /// dropped (the REPL settles the line's first, at `close_item`).
    pub fn commit(&mut self) {
        self.journal.clear();
        self.pending.clear();
    }

    /// A declaration is removed by spelling *and* declaring scope, so outer
    /// declarations of the same spelling stay.
    pub fn rollback(&mut self, trie: &mut RegexTrie) {
        while let Some(act) = self.journal.pop() {
            match act {
                Journal::Declared { name, scope } => {
                    // A failed removal means the entry was already pruned.
                    let _ = trie.remove(&name, scope);
                }
                Journal::Ended { binding, prev_end } => {
                    // SAFETY: a journalled binding is a binding dyad from the store.
                    unsafe { Binding::set_end(binding, prev_end) };
                }
            }
        }
        self.pending.clear();
    }

    pub fn resolve(&self, trie: &RegexTrie, name: &str) -> Result<Resolved, ResolveError> {
        self.select(trie, name, false)
    }

    /// As `resolve`, but the fresh-spelling patterns compete too: a spelling
    /// nothing declared answers with `fresh` set and the run's length.
    pub fn lex(&self, trie: &RegexTrie, text: &str) -> Result<Resolved, ResolveError> {
        self.select(trie, text, true)
    }

    /// The one selection rule (DESIGN ›The scope's constructor is the driver‹):
    /// a match ending between two word characters is no candidate; of the
    /// rest, highest `lex_rank` first, longest match at equal rank.
    fn select(
        &self,
        trie: &RegexTrie,
        text: &str,
        include_fresh: bool,
    ) -> Result<Resolved, ResolveError> {
        if text.is_empty() {
            return Err(ResolveError::Unknown(String::new()));
        }
        let matches = trie.get_all_matches(text).map_err(ResolveError::Index)?;
        let bytes = text.as_bytes();
        let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        // SAFETY: every pointer the trie stores is a binding dyad from the store.
        let fields = |r: DyadPtr| unsafe { Binding::read(r) };
        // One candidate per binding, at its longest match: an optional tail
        // reports every end, an alternation holds the binding on each path.
        let mut cands: Vec<(DyadPtr, usize, bool)> = Vec::new();
        let mut why_none: Option<ResolveError> = None;
        for m in &matches {
            let cuts_word =
                m.matched < bytes.len() && word(bytes[m.matched - 1]) && word(bytes[m.matched]);
            let fresh = crate::identities::fresh::is_fresh_key(m.regex_key);
            if m.matched == 0 || cuts_word || (fresh && !include_fresh) {
                continue;
            }
            // At the frontier "range covers the point" is exactly "not dead".
            // Two live bindings of one spelling are a field's or a slot word's,
            // declared against their siblings alone: the innermost open
            // scope's wins (DESIGN ›The constructor is a field‹).
            let binding = m
                .bindings
                .iter()
                .copied()
                .filter(|&r| self.is_open(fields(r).scope) && !fields(r).is_dead())
                .max_by_key(|&r| self.position(fields(r).scope));
            let Some(binding) = binding else {
                let spelling = text[..m.matched].to_string();
                if m.bindings.iter().any(|&r| self.is_open(fields(r).scope)) {
                    why_none = Some(ResolveError::Dead(spelling));
                } else if why_none.is_none() {
                    why_none = Some(ResolveError::OutOfScope(spelling));
                }
                continue;
            };
            match cands.iter_mut().find(|(r, _, _)| *r == binding) {
                Some(e) => e.1 = e.1.max(m.matched),
                None => cands.push((binding, m.matched, fresh)),
            }
        }
        let mut best: Option<(f64, usize, Resolved)> = None;
        let mut tied = false;
        for (binding, matched, fresh) in cands {
            let f = fields(binding);
            // Off its binding: a second name for one identity ranks on its own.
            let rank = f.lex_rank;
            let better = match &best {
                None => true,
                Some((r, n, _)) => rank > *r || (rank == *r && matched > *n),
            };
            if better {
                let r = Resolved { fresh, matched, binding, identity: f.dyad, scope: f.scope };
                best = Some((rank, matched, r));
                tied = false;
            } else if matches!(&best, Some((r, n, _)) if rank == *r && matched == *n) {
                tied = true;
            }
        }
        match best {
            Some(_) if tied => Err(ResolveError::Tied),
            Some((_, _, r)) => Ok(r),
            None => Err(why_none.unwrap_or_else(|| ResolveError::Unknown(unknown_spelling(text)))),
        }
    }

    /// No-shadowing: `Shadowed` if `name` is live in an open scope; a dead
    /// name is free to redeclare, a fresh entry beside the dead one.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; this writes its `scope`.
    pub unsafe fn declare(
        &mut self,
        trie: &mut RegexTrie,
        name: &str,
        binding: DyadPtr,
    ) -> Result<(), ResolveError> {
        let scope = self.current().expect("declare needs an open scope");
        match self.resolve(trie, name) {
            Ok(_) => return Err(ResolveError::Shadowed(name.to_string())),
            Err(ResolveError::OutOfScope(_) | ResolveError::Unknown(_) | ResolveError::Dead(_)) => {
            }
            Err(e) => return Err(e),
        }
        // The spelling enters the index as a literal key, never as a pattern.
        let key = regex::escape(name);
        // SAFETY: `binding` is a binding dyad from the store, built for this name.
        unsafe { Binding::set_scope(binding, scope) };
        trie.insert(&key, binding);
        self.journal.push(Journal::Declared { name: key, scope });
        self.pending.push(Pending { binding, endpoint: Endpoint::Start });
        Ok(())
    }

    /// The key enters the index as the recognizer it is, never escaped; the
    /// one check is that this very key is not live in an open scope. Two
    /// patterns that can match the same length at equal rank are caught only
    /// when text hits both (`Tied`): deciding it here needs automaton
    /// intersection.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; this writes its `scope`.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store ([`Binding::alloc`]); this
    /// writes its `scope` field through the pointer.
    pub unsafe fn declare_pattern(
        &mut self,
        trie: &mut RegexTrie,
        key: &str,
        binding: DyadPtr,
    ) -> Result<(), ResolveError> {
        let scope = self.current().expect("declare needs an open scope");
        if let Some(bindings) = trie.bindings_for_key(key) {
            // SAFETY: every pointer the trie stores is a binding dyad from the store.
            let fields = |r: DyadPtr| unsafe { Binding::read(r) };
            if bindings.iter().any(|&r| self.is_open(fields(r).scope) && !fields(r).is_dead()) {
                return Err(ResolveError::Shadowed(key.to_string()));
            }
        }
        // SAFETY: `binding` is a binding dyad from the store, built for this pattern.
        unsafe { Binding::set_scope(binding, scope) };
        trie.insert(key, binding);
        self.journal.push(Journal::Declared { name: key.to_string(), scope });
        self.pending.push(Pending { binding, endpoint: Endpoint::Start });
        Ok(())
    }

    /// `node` is the provisional `end` until `settle_item` replaces it with
    /// the body item (DESIGN ›Memory and concurrency‹).
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; this writes its `end`.
    pub unsafe fn mark_dead(&mut self, binding: DyadPtr, node: DyadPtr) {
        // SAFETY: `binding` is a binding dyad the trie returned.
        let prev_end = unsafe { Binding::replace_end(binding, node) };
        self.journal.push(Journal::Ended { binding, prev_end });
        self.pending.push(Pending { binding, endpoint: Endpoint::End });
    }

    /// Every endpoint pending for `scope` now points at `item`, the line as a
    /// whole: an `own` inside an `if` body ends the outer name at the `if`
    /// (DESIGN ›Name resolution is scope-filtered‹).
    ///
    /// # Safety
    /// Every pending binding must be a binding dyad from the store; `item` is
    /// stored, never read.
    pub unsafe fn settle_item(&mut self, scope: DyadPtr, item: DyadPtr) {
        let mut i = 0;
        while i < self.pending.len() {
            let binding = self.pending[i].binding;
            // SAFETY: every pending binding is a binding dyad from the store.
            if unsafe { Binding::read(binding).scope } != scope {
                i += 1;
                continue;
            }
            let p = self.pending.swap_remove(i);
            // SAFETY: every pending binding is a binding dyad from the store.
            unsafe {
                match p.endpoint {
                    Endpoint::Start => Binding::set_start(binding, item),
                    Endpoint::End => Binding::set_end(binding, item),
                }
            }
        }
    }

    /// `item` is a complete line of the innermost scope: the names it declared
    /// or ended settle on it, and it joins the scope's `dyads`.
    ///
    /// # Safety
    /// As [`ScopeStack::settle_item`]; `array_ty` the `array` identity.
    pub unsafe fn close_item(&mut self, store: &mut Store, array_ty: DyadPtr, item: DyadPtr) {
        let scope = self.current().expect("a line closes inside an open scope");
        // SAFETY: the caller's contract.
        unsafe {
            self.settle_item(scope, item);
            crate::identities::scope::push_item(store, array_ty, scope, item);
        }
    }

    /// Checked against its siblings alone (DESIGN ›The constructor is a
    /// field‹): a live binding of the spelling in an enclosing scope stands
    /// beside it, both live until this scope closes.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; this writes its `scope`.
    pub unsafe fn declare_field(
        &mut self,
        trie: &mut RegexTrie,
        name: &str,
        binding: DyadPtr,
    ) -> Result<(), ResolveError> {
        let scope = self.current().expect("declare needs an open scope");
        if self.declared_in(trie, name, scope)? {
            return Err(ResolveError::Shadowed(name.to_string()));
        }
        // The spelling enters the index as a literal key, never as a pattern.
        let key = regex::escape(name);
        // SAFETY: `binding` is a binding dyad from the store, built for this name.
        unsafe { Binding::set_scope(binding, scope) };
        trie.insert(&key, binding);
        self.journal.push(Journal::Declared { name: key, scope });
        self.pending.push(Pending { binding, endpoint: Endpoint::Start });
        Ok(())
    }

    /// The sibling check, run too across the two halves of a `fields = (…)`
    /// block, whose `shared` members live in the type's own scope and whose
    /// fields live in the list's (DESIGN ›The constructor is a field‹).
    pub fn declared_in(
        &self,
        trie: &RegexTrie,
        name: &str,
        scope: DyadPtr,
    ) -> Result<bool, ResolveError> {
        match trie.get(name) {
            Ok(m) if m.matched == name.len() => {
                // SAFETY: every pointer the trie stores is a binding dyad.
                let fields = |r: DyadPtr| unsafe { Binding::read(r) };
                Ok(m.bindings.iter().any(|&r| fields(r).scope == scope && !fields(r).is_dead()))
            }
            Ok(_) | Err(RegexTrieError::NodeNotFound) => Ok(false),
            Err(e) => Err(ResolveError::Index(e)),
        }
    }

    /// The name becomes another spelling of an existing identity (a type), so
    /// pointer-identity checks see the original; the binding's range, journal
    /// entry and pending endpoint are untouched.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; this writes its `dyad`.
    pub unsafe fn rebind(&mut self, binding: DyadPtr, identity: DyadPtr) {
        // SAFETY: `binding` is a binding dyad from the store.
        unsafe { Binding::set_dyad(binding, identity) };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
}

/// A function node's value record, in order: input `record`, return type,
/// body, compiled `callable` (null until compiled), frame size, outer names
/// (DESIGN ›Execution is function application‹).
pub const FN_INPUT: usize = 0;
pub const FN_OUTPUT: usize = 1;
pub const FN_BODY: usize = 2;
/// A `callable` node (`[entry: @exec, convention]`), null until compiled.
pub const FN_BCODE: usize = 3;
/// A `u64` leaf: the frame's byte size, parameters first, then the locals at
/// their offsets; null for none. Both tiers read it on entry.
pub const FN_FRAME: usize = 4;
/// An `array` of the bindings of the outer names the body reads, in
/// first-read order, or null; read at every call (DESIGN ›`own` and `drop`
/// are static‹).
pub const FN_OUTER: usize = 5;

/// `0` when the slot is null (no parameters and no locals).
///
/// # Safety
/// `fn_node` must be a function node whose value is the six-slot record
/// `parse_fn` builds.
pub unsafe fn fn_frame_size(fn_node: DyadPtr) -> usize {
    let frame = *((*fn_node).value as *const DyadPtr).add(FN_FRAME);
    if frame.is_null() {
        0
    } else {
        std::ptr::read_unaligned((*frame).value as *const u64) as usize
    }
}

/// Empty for a function that reads none, and for a declaration's placeholder
/// whose value is still being parsed.
///
/// # Safety
/// `fn_node` must be a function node: its value null, or the operands
/// `parse_fn` builds (the early signature included).
pub unsafe fn fn_outer<'a>(fn_node: DyadPtr) -> &'a [DyadPtr] {
    let fields = (*fn_node).value as *const DyadPtr;
    if fields.is_null() {
        return &[];
    }
    let outer = *fields.add(FN_OUTER);
    if outer.is_null() {
        &[]
    } else {
        crate::identities::array::items(outer)
    }
}

/// The slot words a type body declares into its own scope, in `SlotKind`
/// order (DESIGN ›The constructor is a field‹). `drop` is not here: it is the
/// statement keyword, which `=` takes as the slot's name.
pub const SLOT_NAMES: [&str; 6] =
    ["parse_rank", "lex_rank", "associativity", "parse", "run", "fields"];

/// Each open bracket recurses through `parse_sequence` and `(`'s constructor,
/// so a wall of `(` costs Rust stack like a runaway recursion: sized well
/// under `crate::WORK_STACK_BYTES` and far past anything a person writes.
pub const MAX_BRACKET_DEPTH: usize = 2_000;

/// What a line of a `type (…)` body is (DESIGN ›The constructor is a field‹).
#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyLine {
    /// A comment.
    Prose,
    /// A member of the type's own, or a slot fill.
    Declare,
    Other,
}

/// One of the slots, by `SLOT_NAMES` position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    ParseRank = 0,
    LexRank = 1,
    Associativity = 2,
    Parse = 3,
    Run = 4,
    Fields = 5,
    /// The `drop` keyword left of `=`: no spelling in `SLOT_NAMES`, since the
    /// word is the statement's.
    Drop = 6,
}

impl SlotKind {
    fn of(i: usize) -> Self {
        match i {
            0 => SlotKind::ParseRank,
            1 => SlotKind::LexRank,
            2 => SlotKind::Associativity,
            3 => SlotKind::Parse,
            4 => SlotKind::Run,
            _ => SlotKind::Fields,
        }
    }
}

/// What a `type (…)` body's lines have filled so far: the head the type node
/// takes at the close, the constructor, and the fields block's layout.
struct OpenType {
    scope: DyadPtr,
    /// The six slot words as this definition's own markers, declared into the body's scope.
    slots: [DyadPtr; 6],
    /// The marker a `shared drop = (…)` line declares; `drop` itself is the core word.
    drop_marker: DyadPtr,
    parse_rank: f64,
    /// From a `lex_rank = …` line; written onto the declaration's binding at
    /// the close.
    lex_rank: Option<f64>,
    assoc: Assoc,
    ctor: DyadPtr,
    /// From a `shared run = (…)` line: the body lexed once into its cells; a
    /// node of the type constructs it per field-type set.
    run_body: DyadPtr,
    instance: Option<(DyadPtr, DyadPtr, u64)>,
    /// The hidden `this` parameter of the `parse` body being read; null
    /// otherwise.
    this_param: DyadPtr,
    /// True while a `shared` line of the fields block is being parsed: a
    /// slot fill there is the instances' slot, on a bare line the type's own.
    in_block: bool,
    /// The fields block's scope and the fields declared so far, while it is
    /// open: what `this.f` reaches in a body written inside it.
    block: Option<(DyadPtr, Vec<DyadPtr>)>,
    /// The instances' parse trio, from `shared parse`, `shared parse_rank`
    /// and `shared associativity` lines.
    instances_ctor: DyadPtr,
    instances_rank: f64,
    instances_assoc: Assoc,
}

/// A constructor edits the tape in place; `Placed` reports only that it did.
/// `Decline` is "not mine": nothing consumed, nothing touched, and the driver
/// leaves the identity standing as its own value.
pub enum Constructed {
    Placed,
    Decline,
}

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

/// The instances' `parse` of the node standing in the cell, run with `this`
/// bound to that node (DESIGN ›The constructor is a field‹).
fn instances_constructor(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    // SAFETY: `ctor_of` chose this for a node whose record type fills the slot.
    let f = unsafe { crate::identities::meta::instances_parse_of((*id).ty) };
    // SAFETY: `f` is the fn the fields block's slot holds, `id` a node its type's parse built.
    unsafe { p.run_logos_body(f, id, tape) }
}

fn application(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    p.construct_application(id, tape)
}

/// Every identity's parse-time constructor: it reads its span from the
/// cursor cell, its context from `tape.at(-1)` and `tape.at(1)`, and edits
/// the tape in place. The driver decides only *when* constructors run.
pub type ConstructFn =
    fn(&mut Parser, DyadPtr, &mut ParsingTape) -> Result<Constructed, ParseError>;

#[derive(Default)]
struct OpenScope {
    /// The `defer free <place>` nodes the scope's owning bindings inserted,
    /// drained into the body after each statement.
    defers: Vec<DyadPtr>,
    /// The places those teardowns free, once drained: no tail or `return`
    /// leaving the scope may hand one out.
    owned: Vec<DyadPtr>,
    /// Items parsed at depth 0 and not yet run: `drain` runs them when the
    /// pass needs a value, the scope's own run otherwise.
    unrun: Vec<DyadPtr>,
    /// The tape cells, by the tape's place and a literal index, a check in this
    /// scope has shown to hold a type.
    narrowed: Vec<(DyadPtr, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Name resolution failed; the spelling is in the reason.
    Resolve(ResolveError),
    /// A Logos-written constructor failed while running; carries the
    /// rendered run error.
    ConstructorFailed(Box<String>),
    /// An item the pass ran failed (DESIGN ›Build and run are one
    /// self-directing pass‹).
    Run(crate::run::RunError),
    /// A line of a `type (…)` body that neither fills a slot, lays out its
    /// instances, nor is prose.
    TypeBodyLine,
    /// A second `fields = (…)` block in one type body.
    DoubleFields,
    /// A bare `:=` line in a type body: members are declared inside
    /// `fields = (…)`.
    MemberOutsideFieldsBlock,
    /// `shared` outside a `fields = (…)` block.
    SharedOutsideFieldsBlock,
    /// `shared` not followed by a `name := value` declaration.
    SharedNeedsDeclaration,
    /// A slot word left of `=` where no type is being defined.
    SlotOutsideDefinition,
    /// A bare `run = …` line, or `t.run`: `run` is a slot of the instances alone.
    NoOwnRun,
    /// `t.x` where `x` is a member of `t`'s fields block, read `t.fields.x`.
    MemberThroughFields(String),
    /// `t.fields.x` where `x` is a place per node, not stored with the type.
    PerNodeThroughType(String),
    /// `shared lex_rank` or a nested `fields` in the block.
    FieldsSlotNotInSeed,
    /// A `drop = …` fill, on a bare line or in the block.
    DropSlotNotInSeed,
    /// A slot word inside the fields block without `shared`.
    FieldsSlotNeedsShared,
    /// `shared parse = (…)` in a type with no `parse` of its own, whose
    /// instances the seed builds by application, without the slots `this.f` reads.
    InstancesParseNeedsOwnParse,
    /// A binding in a type body inserted a teardown, which no scope exit runs.
    DeferInTypeBody,
    /// A type body's own declaration failed while running at the definition;
    /// carries the rendered run error.
    TypeBodyFailed(Box<String>),
    /// Scopes nested deeper than `MAX_BRACKET_DEPTH`.
    TooDeep,
    /// The pass needed a type identity and found a place that will only hold
    /// one when the program runs (DESIGN ›A type is a comptime value‹).
    TypeKnownOnlyAtRun,
    /// `=` into an owning place whose right side does not own: two teardowns
    /// over one block, or one over memory the store owns.
    NonOwningIntoOwning,
    /// `dyad (T, v)` for a `T` whose values are bytes at a width the seed
    /// cannot fill from a value node.
    BadDyadType,
    /// `parse_rank = …` whose value is not a number known at the definition.
    NonComptimeRank,
    /// `associativity = …` with something other than `left` or `right`.
    BadAssociativity,
    /// `lex_rank = …` in a type body that is not a declaration's value: the
    /// rank is the name's, and here there is no name.
    LexRankNeedsName,
    /// `parse = …` or `shared run = …` with anything but a bracket on its right.
    SlotNeedsBody(SlotKind),
    /// A type with a `run` applied to arguments, `sq(3)`: only its `parse`
    /// fills a node's fields and output, so the call form builds no node.
    RunTypeApplied,
    /// A held run body could not be constructed for a field-type set; carries
    /// the type's spelling at the use and the failure rendered against the
    /// body's text.
    RunBodyFailed { name: String, rendered: String },
    /// `this.f` in a parse body of a type with no `fields = (…)` block.
    ThisNeedsFieldsBlock,
    /// `this.f` naming no field of the fields block; carries `f`.
    ThisFieldUnknown(Box<String>),
    /// `tape.is_constructed[k] = v` with a `v` that is no bool.
    FlagTakesBool,
    /// An operator lacked a reduced operand on one side.
    MissingOperand,
    /// The tape did not reduce to a single dyad (a dangling operator or operand).
    Trailing,
    /// The input held no expression.
    Empty,
    /// A numeric literal's digits did not parse.
    BadLiteral,
    /// An opener whose closer never came, or a mismatched one (`)` for `[`).
    UnclosedBracket,
    /// A construct that requires a `(` (a `record`/parameter list) was not
    /// followed by one.
    ExpectedOpen,
    /// A field list expected a field name where it found neither a name nor `)`.
    ExpectedField,
    /// A fn signature's parameter list was not followed by `->`.
    ExpectedArrow,
    /// A fn signature's `->` was not followed by a return type.
    ExpectedReturnType,
    /// An abstract operator could not resolve a concrete machine op for its
    /// operand types.
    UnsupportedOperands,
    /// An `if` or `while` condition was not a `bool`.
    NonBoolCondition,
    /// An `if` without an `else` where a value is required: with no false
    /// branch it yields unit.
    MissingElse,
    /// A logical operator applied to a non-`bool` operand.
    NonBoolOperands,
    /// A binary operator's operands were two different concrete numeric
    /// types: cross-type arithmetic needs an explicit cast.
    TypeMismatch,
    /// A number literal had no exact value in the type it was committed to.
    UncomputableLiteral,
    /// A `return` before the tail with no function around it to leave.
    EarlyReturn,
    /// A unit-valued statement (a `while` loop) stood where a value is required.
    StatementAsValue,
    /// An assignment target that is not a typed numeric variable: a comptime
    /// binding has no machine storage to write.
    BadAssignTarget,
    /// The target is a bare literal: `x := 5` binds the name to the number
    /// itself, so there is no storage for `x = 6` to write (DESIGN ›Numbers‹).
    /// Carries the literal's spelling.
    AssignToLiteral(Box<String>),
    /// A gate word (`pub`) not followed by a declaration: a gate fills a
    /// declare node's gate slot.
    GateNeedsDeclaration,
    /// A declaration was gated twice (`pub pub x := …`).
    DoubleGate,
    /// `x = …` on a name not declared `mut`; carries the name.
    NotMutable(Box<String>),
    /// A write into a name or field declared `immut`; carries the name.
    Immutable(Box<String>),
    /// A read of a name declared `T ?` before a sibling write filled it; carries the name.
    Unwritten(Box<String>),
    /// An `import` was not followed by a path token.
    ExpectedPath,
    /// A `regex` was not followed by a `«…»` quote.
    ExpectedPattern,
    /// A `lex`, `print` or `error` was not followed by a `«…»` quote; carries the word.
    ExpectedQuote(&'static str),
    /// A `{` in a `print` quote with no `}` after it.
    UnclosedInterpolation,
    /// A `}` in a `print` quote with no `{` before it.
    StrayInterpolationClose,
    /// `tape.insert(k, …)` was handed something that is not a tape: `insert`
    /// splices a tape into a tape (DESIGN ›Text is the quote‹).
    InsertTakesTape,
    /// `hashmap` not followed by `K -> V`, or a key or value type whose values are
    /// not one word read by value.
    HashmapShape,
    /// A constructor edited the tape and returned with its own cell still
    /// unconstructed and still its own identity: neither a decline nor a
    /// construction. Carries the cell's spelling.
    CellLeftUnconstructed(Box<String>),
    /// The pattern a `regex «…»` quotes does not compile, or is empty;
    /// reported at the definition, since the index compiles a branch only on
    /// first lookup.
    BadPattern(String),
    /// An `import` inside a deferred-or-repeated body: the load happens once,
    /// at parse, so `import` belongs where parse order and run order coincide.
    ImportInRuntimeBody,
    /// The imported file could not be read: the joined path and the OS error.
    ImportRead(String),
    /// The named file is already loading: the import graph must be a DAG.
    ImportCycle(String),
    ImportFailed {
        /// The path as written at the import site.
        path: String,
        /// The fully rendered inner report (file:line:col, caret and all).
        rendered: String,
    },
    /// A reflection read that does not fit the node's type, an unknown member
    /// on a view, or a read whose honest answer is undefined (a null
    /// constructor slot).
    BadReflectRead,
    /// A collection member (`.operands`, `.roles`) without its `[index]`: the
    /// bare collection as a first-class value waits for the array type.
    ExpectedIndexBracket,
    /// `.type` on something that is not a dyad: a value's type is never one of
    /// its own fields; `:` reads it, `x:type`.
    TypeIsColonRead,
    /// `:dyad` or `:value`: nothing reaches a value's cell as a whole.
    CellNotReachable,
    /// `⊆` over two different types that are not both integer types.
    UnsettledInclusion,
    /// A record construction's argument count did not match its field count.
    CtorArity,
    /// A `for` followed by a fresh spelling and then not by `in`: a fresh
    /// spelling can only be the loop variable.
    ExpectedIn,
    /// A `for`'s range was malformed: a missing `..`, or a range part that is
    /// not a primary (a bare full expression would consume the body's `(`).
    ExpectedRange,
    /// A `for`'s literal step was not positive: with the end-exclusive
    /// condition it could never terminate.
    BadStep,
    /// An `&` of something without storage to point at.
    BadAddressOf,
    /// A numeric conversion `logos(value)` with not exactly one operand, or a
    /// non-numeric one.
    BadCast,
    /// A declaration's type position, or a type variable's fill, held
    /// something that does not evaluate to a type.
    BadDeclaredType,
    /// A typed declaration of a non-numeric type: the declared-type storage
    /// for those is not in the seed yet.
    NonNumericDeclaredType,
    /// A `-> logos` call could not be resolved at parse time: its arguments
    /// were not comptime-known, or it did not yield a type.
    NonComptimeTypeCall,
    /// A nested function reached a local or parameter of an enclosing
    /// function: a closure capture, which v1 does not support; it would read
    /// the wrong frame at run time.
    CapturedLocal,
    /// An owning value stood where no name binds it: the teardown attaches at
    /// the binding site (DESIGN ›Explicit heap, and no implicit destruction‹),
    /// so it would leak. Fail-closed until ownership-gated parameters.
    UnboundOwningValue,
    /// A scope's value is a bare owning place: the inserted `defer free` runs
    /// at exit, so the value handed out is already freed; `own x` is how
    /// ownership leaves a scope.
    OwningEscape,
    /// A function body hands ownership out through its return: a plain `@T`
    /// carries no destructor, so the caller could not know it owes a `free`.
    /// Fail-closed until a return type can declare the transfer.
    OwnershipAcrossReturn,
    /// An `own` or `drop` inside a loop or `fn` body names a place declared
    /// outside it: the loop would read a dead name on its next pass, and a
    /// function may own only what its parameters hand it (DESIGN ›Memory and concurrency‹).
    OwnOfOuterName,
}

/// A `bool` value, a comparison, or a logical operator.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_bool_result(types: &Core, node: DyadPtr) -> bool {
    let node = types.through(node);
    let logos = (*node).ty;
    // An item that ran in the pass is what its expression is.
    if logos == types.ran_ {
        return is_bool_result(types, crate::identities::ran::expr_of(types, node));
    }
    // A sequence's value is its trailing expression's.
    if logos == types.scope {
        return match last_sequence_expr(node) {
            Some(last) => is_bool_result(types, last),
            None => false,
        };
    }
    // `and`/`or` over two booleans is one; over two non-booleans it is a group.
    if logos == types.and_ || logos == types.or_ {
        let (lhs, rhs) = crate::identities::operands(node);
        return is_bool_result(types, lhs) && is_bool_result(types, rhs);
    }
    // A call, or a node of a type with a run, is what its function declares it returns.
    let f = if crate::identities::meta::is_record_type(logos)
        && !crate::identities::meta::run_body_of(logos).is_null()
    {
        crate::identities::run_body::spec_of(node)
    } else {
        logos
    };
    if !f.is_null() && (*f).ty == types.fn_type && !(*f).value.is_null() {
        return *((*f).value as *const DyadPtr).add(FN_OUTPUT) == types.bool_;
    }
    logos == types.bool_
        || logos == types.lt
        || logos == types.gt
        || logos == types.eq
        || logos == types.le
        || logos == types.ge
        || logos == types.ne
        || logos == types.not_
        || logos == types.subset
        || logos == types.tape.is_constructed
}

/// Deliberately no scope unwrapping: a sequence-valued condition may carry
/// effectful non-tail expressions a fold would silently drop.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn bool_literal_value(types: &Core, node: DyadPtr) -> Option<bool> {
    let node = types.through(node);
    if (*node).ty != types.bool_ || (*node).value.is_null() {
        return None;
    }
    Some(std::ptr::read_unaligned((*node).value as *const i32) != 0)
}

/// `tape[k]:type == type` or `!=`, either side first: the cell checked, and whether
/// the test is `==`.
///
/// # Safety
/// `cond` must be a reduced dyad from the store.
unsafe fn type_check_of(types: &Core, cond: DyadPtr) -> Option<((DyadPtr, i32), bool)> {
    let cond = types.through(cond);
    let eq = (*cond).ty == types.eq;
    if !eq && (*cond).ty != types.ne {
        return None;
    }
    let ops = (*cond).value as *const DyadPtr;
    let (l, r) = (types.through(*ops), types.through(*ops.add(1)));
    let read = if r == types.type_ {
        l
    } else if l == types.type_ {
        r
    } else {
        return None;
    };
    if (*read).ty != types.tape.cell_type {
        return None;
    }
    crate::identities::tape::cell_key(types, read).map(|key| (key, eq))
}

/// Trailing comment nodes are prose, not the tail; `None` for a scope with
/// no expression array.
///
/// # Safety
/// `node` must be a valid dyad from the store; a non-null value must be the
/// `[exprs, op, parent]` triple `parse_sequence` fills.
pub(crate) unsafe fn last_sequence_expr(node: DyadPtr) -> Option<DyadPtr> {
    crate::identities::scope::exprs_of(node)?
        .iter()
        .rev()
        .find(|&&e| !crate::identities::numtype::is_comment_type((*e).ty))
        .copied()
}

/// The positions `commit_tail` enumerates: a `return` itself, an `if`'s
/// branches, a sequence's expressions.
///
/// # Safety
/// `node` must be a valid dyad from the store, with the value shapes its
/// type implies.
unsafe fn contains_return(types: &Core, node: DyadPtr) -> bool {
    let logos = (*node).ty;
    if logos == types.return_ {
        return true;
    }
    if logos == types.if_ {
        let p = (*node).value as *const DyadPtr;
        let (then, els) = (*p.add(1), *p.add(2));
        return contains_return(types, then) || (!els.is_null() && contains_return(types, els));
    }
    if logos == types.scope {
        if (*node).value.is_null() {
            return false;
        }
        let arr = *((*node).value as *const DyadPtr);
        return crate::identities::array::items(arr).iter().any(|&e| contains_return(types, e));
    }
    false
}

/// `{type: callee, value: [args…, null]}`: null-terminated so `run` can count
/// the arguments; a nullary call carries a null value.
fn build_call(store: &mut Store, callee: DyadPtr, args: &[DyadPtr]) -> DyadPtr {
    let value = if args.is_empty() {
        std::ptr::null_mut()
    } else {
        let mut ops = args.to_vec();
        ops.push(std::ptr::null_mut());
        store.alloc_operands(&ops)
    };
    store.alloc_raw(callee, value)
}

/// The once-per-run import registry: a file loads once per run, every
/// importer sharing the one loaded section, and a path met again while its
/// own load is in progress is a cycle.
#[derive(Debug, Default)]
pub struct Imports {
    entries: HashMap<PathBuf, ImportState>,
    /// The section scope each loaded file's declarations landed in: a
    /// section's names live for the run, so a `pub` function's body may read
    /// a private sibling from anywhere (DESIGN ›Importing is dropping the text there‹).
    sections: HashSet<DyadPtr>,
}

#[derive(Debug)]
enum ImportState {
    /// Importing it again now is a cycle.
    Loading,
    /// The `pub` names in declaration order, each with the identity it
    /// resolves to inside the file, and the file's tail node (null for a
    /// declaration-only file).
    Loaded { pubs: Vec<(String, DyadPtr)>, tail: DyadPtr },
}

/// The one-pass elaborator: deferred reduction by parse_rank over the explicit
/// tape, not Pratt; operators wait on the tape until parse_rank says to
/// reduce them.
pub struct Parser<'a> {
    source: &'a str,
    pos: usize,
    scopes: ScopeStack,
    trie: &'a mut RegexTrie,
    types: &'a Core,
    /// The placeholder of the declaration awaiting its value, or null:
    /// `parse_fn` publishes the signature onto it before the body parses, so
    /// a recursive self-call resolves its types.
    pending_fn: DyadPtr,
    /// The bindings of the declarations whose right side is being driven,
    /// innermost last: what a `lex_rank = …` line writes. A stand-in for a
    /// definition writing a binding field from inside `:=`.
    filling: Vec<DyadPtr>,
    /// The binding of the last declaration that reduced: what a gate word to
    /// its left marks.
    last_declared: DyadPtr,
    /// The bindings along the path a field place was reached by, root first:
    /// what a write into it must be granted by.
    paths: HashMap<DyadPtr, Vec<DyadPtr>>,
    /// Each view a `t.fields` read built, to its type: the step a following
    /// `.x` reads the block through.
    fields_views: HashMap<DyadPtr, DyadPtr>,
    /// Each value a run type's node folded to at parse, and that node, whose
    /// fields a `.` on the value still reads.
    unfolded: HashMap<DyadPtr, DyadPtr>,
    /// The binding of the name left of the `.` being constructed, or null.
    member_root: DyadPtr,
    /// The field binding behind each `this.f` slot a parse body built: the
    /// constructor's fill, granted by default and vetoed by `immut`.
    fills: HashMap<DyadPtr, DyadPtr>,
    /// Open function frames, innermost last: empty at top level, where
    /// declarations get global storage; inside a function each local claims
    /// the next byte offset in the top frame.
    frames: Vec<OpenFn>,
    /// How many deferred-or-repeated bodies enclose the position, where parse
    /// order and run order do not coincide; comptime effects that rebind names
    /// at parse are rejected while non-zero. Comptime-taken `if` branches do not count.
    runtime_depth: u32,
    /// The type definitions open around the position, innermost last; slot
    /// fills and the `fields = (…)` block write into the top.
    definitions: Vec<OpenType>,
    /// The open scopes, innermost last, the base entry the top level; its
    /// length is the bracket depth.
    open: Vec<OpenScope>,
    /// The folder relative import paths resolve against: the importing file's
    /// own folder during a nested import, else the working directory.
    dir: PathBuf,
    imports: Imports,
    /// Comment cells lifted out of a segment at its boundary, with the offset
    /// each was lexed at, handed out as body items in source order beside the
    /// segment's expression.
    lifted: Vec<(usize, DyadPtr)>,
    /// Body items a constructed segment yielded, in source order, not yet
    /// handed out.
    queued: std::collections::VecDeque<DyadPtr>,
    /// The valueless places `?` built: a `:=` binds its name straight to such
    /// a place, no snapshot and no initializer.
    holes: HashSet<DyadPtr>,
    /// The last `=` node built and the scope it was built in.
    last_write: (DyadPtr, DyadPtr),
    /// Whether the constructor now running was woken at discovery, its token
    /// just lexed and the source after it unread, rather than at the boundary:
    /// an identity that reads its own bracket reads source only at discovery.
    discovering: bool,
    /// While `.` lexes the member right of a parse body's `this`: the member is
    /// read by its spelling, so a raw-text word it happens to spell is not woken.
    member_asleep: bool,
    /// The stop mode of the segment being lexed, so a lazy read inside a
    /// constructor stops at the same boundaries the loop would.
    lex_mode: Option<RightSide>,
    /// While an `if` reads a bare branch: the depth of the branch's own scope,
    /// where `else` and `,` end it; a nested bracket reads them as usual.
    else_ends: Option<usize>,
    /// The pass's one runtime (DESIGN ›Build and run are one self-directing
    /// pass‹): one runtime keeps the frame arena and the allocation ledger
    /// whole across the pass.
    rt: crate::run::Runtime<'a>,
    /// The lowering table, when the driver attached one: handed to a nested
    /// import's runtime so `f.compile()` at an imported top level works as at
    /// the driver's own.
    lower: Option<&'a crate::compile::LowerTable>,
    /// The held run body whose cells the driver is reading now, if any.
    feed: Option<Feed>,
    /// The construction of a held run body in progress: what `this.f` means
    /// there.
    run_body: Option<RunBodyCx>,
    /// While a held `type (…)` is built at its run, the function frames open
    /// where it was written: a place declared at that depth is the type's, global.
    held_depth: usize,
    /// A cell the branch about to open is checked to hold a type.
    narrow_next: Option<(DyadPtr, i32)>,
    /// While a `shared` line of a fields block is read, the frame depth a
    /// `fn` written as the member's value opens at: that `fn` takes `this`.
    member_fn_depth: Option<usize>,
}

/// The cells of a held run body being constructed, served by the cursor's
/// position in the body's text so a boundary's rewind re-exposes a cell as it
/// does a token; an undeclared spelling resolves at `base_depth` and above only.
struct Feed {
    cells: Vec<Cell>,
    next: usize,
    base_depth: usize,
    /// A held `type (…)` builds now, so its parse order is its run order.
    in_run_order: bool,
}

/// What `this.f` means inside a held run body under construction: the field's
/// parameter place, or for a field of type `type` the type this field-type
/// set holds in it.
struct RunBodyCx {
    this_param: DyadPtr,
    field_scope: DyadPtr,
    fields: Vec<DyadPtr>,
    places: Vec<DyadPtr>,
    key: Vec<DyadPtr>,
}

/// The lex step the driver and `lex «…»` share: a fresh spelling mints its
/// dyad with both slots null into `store`, the pattern's construction done at
/// the lex. `None` at the end of the text; `text` must outlive the cell.
pub(crate) fn lex_token(
    scopes: &ScopeStack,
    trie: &RegexTrie,
    store: &mut Store,
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
    let r = scopes.lex(trie, &text[start..])?;
    let dyad = if r.fresh {
        store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut())
    } else {
        r.binding
    };
    // SAFETY: `text` outlives the cell (the caller's contract); `dyad` is the index's binding or a fresh dyad.
    let cell = unsafe { Cell::lexed(dyad, text, start, r.matched) };
    Ok(Some((cell, start + r.matched)))
}

/// Every token of `text` as an unconstructed cell on a fresh tape centered on
/// its first cell: the fragment `insert` splices (DESIGN ›Text is the quote‹).
/// `text` must outlive the fragment's reads.
pub(crate) fn lex_fragment(
    scopes: &ScopeStack,
    trie: &RegexTrie,
    store: &mut Store,
    text: &str,
) -> Result<ParsingTape, ResolveError> {
    let mut tape = ParsingTape::new();
    let mut pos = 0;
    while let Some((cell, next)) = lex_token(scopes, trie, store, text, pos)? {
        tape.push(cell);
        pos = next;
    }
    tape.set_cursor(0);
    Ok(tape)
}

/// One enclosing function body being parsed: parameters claim the frame's
/// first offsets, the body's locals continue after them (DESIGN ›Resolution
/// is one rule‹).
struct OpenFn {
    /// Bytes claimed so far by parameters and frame-relative locals.
    size: usize,
    /// The scope depth the function's barrier begins at: a name declared
    /// below it is from outside the function.
    below: usize,
    /// The bindings of the outer names the body has read so far, first-read
    /// order, each once: the function's `FN_OUTER` list.
    outer: Vec<DyadPtr>,
    /// `open`'s length outside the body: the scopes a `return` leaves are the ones past it.
    open_below: usize,
    /// Every `return` in the body, committed to the result type as the tail is.
    returns: Vec<DyadPtr>,
}

/// A scalar at its own width; anything else the 8-byte container the call
/// convention passes.
///
/// # Safety
/// `logos` must be null or a type node from the store.
unsafe fn field_width(logos: DyadPtr) -> u64 {
    if crate::identities::numtype::is_scalar_type(logos) {
        crate::identities::numtype::of_type_node(logos).bytes() as u64
    } else {
        8
    }
}

impl<'a> Parser<'a> {
    pub fn new(
        source: &'a str,
        store: &'a mut Store,
        trie: &'a mut RegexTrie,
        types: &'a Core,
        scopes: ScopeStack,
    ) -> Self {
        // The parser reaches the store through its runtime, so the one
        // `&mut Store` has one owner.
        let rt = crate::run::Runtime::new(types, store);
        Parser {
            source,
            pos: 0,
            scopes,
            rt,
            trie,
            types,
            pending_fn: std::ptr::null_mut(),
            filling: Vec::new(),
            last_declared: std::ptr::null_mut(),
            paths: HashMap::new(),
            fields_views: HashMap::new(),
            unfolded: HashMap::new(),
            member_root: std::ptr::null_mut(),
            fills: HashMap::new(),
            lifted: Vec::new(),
            queued: std::collections::VecDeque::new(),
            discovering: false,
            member_asleep: false,
            lex_mode: None,
            else_ends: None,
            holes: HashSet::new(),
            last_write: (std::ptr::null_mut(), std::ptr::null_mut()),
            frames: Vec::new(),
            runtime_depth: 0,
            definitions: Vec::new(),
            open: vec![OpenScope::default()],
            dir: PathBuf::from("."),
            imports: Imports::default(),
            lower: None,
            feed: None,
            run_body: None,
            held_depth: 0,
            narrow_next: None,
            member_fn_depth: None,
        }
    }

    /// So an imported file's top-level `f.compile()` runs under the nested
    /// pass exactly as under the driver.
    pub fn with_lower(mut self, lower: &'a crate::compile::LowerTable) -> Self {
        self.lower = Some(lower);
        self.rt.set_compiler(lower);
        self
    }

    /// An item that ran in the pass has its value at hand; anything else runs
    /// now. The drivers' one read of an item's value.
    ///
    /// # Safety
    /// `node` must be a valid dyad this parser built into its store.
    pub unsafe fn value_of(&mut self, node: DyadPtr) -> Result<i64, ParseError> {
        if (*node).ty == self.types.ran_ {
            return Ok(crate::identities::ran::value_of(node));
        }
        self.run_on_pass(node).map_err(ParseError::Run)
    }

    /// With the lexer attached: a `lex «…»` inside what runs resolves against
    /// the scopes open now and the index (DESIGN ›Text is the quote‹). The
    /// parser reads neither while the runtime runs.
    ///
    /// # Safety
    /// `node` must be a valid dyad this parser built into its store.
    unsafe fn run_on_pass(&mut self, node: DyadPtr) -> Result<i64, crate::run::RunError> {
        let host = crate::run::Host {
            parser: (self as *mut Self).cast(),
            lex_on: Self::lex_on,
            mint: Self::mint_host,
        };
        // SAFETY: `node` is a valid dyad in the store (the caller's contract).
        self.rt.hosting(&self.scopes, self.trie, Some(host), |rt| unsafe { rt.run(node) })
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

    /// The root scope's exit: the top level's teardowns, LIFO. The file
    /// driver calls this at program end; a nested scope ran its own at exit.
    pub fn exit(&mut self) -> Result<(), ParseError> {
        for defer_node in self.take_pending_defers().into_iter().rev() {
            // SAFETY: `defer_node` is a `defer` node in the store, which outlives the pass.
            unsafe { crate::identities::run_deferred(&mut self.rt, defer_node) }
                .map_err(ParseError::Run)?;
        }
        Ok(())
    }

    /// The REPL's: a session is one run, so its per-line parsers share one
    /// registry.
    pub fn with_imports(mut self, imports: Imports) -> Self {
        self.imports = imports;
        self
    }

    pub fn take_imports(&mut self) -> Imports {
        std::mem::take(&mut self.imports)
    }

    pub(crate) fn store(&mut self) -> &mut Store {
        self.rt.store
    }

    /// Copied out, so a `&mut self` call can follow.
    pub(crate) fn types(&self) -> &'a Core {
        self.types
    }

    /// Inside a function the place is frame-relative, the next offset after
    /// the parameters, its storage per call; at top level an absolute global
    /// blob. The value is a `FRAME_TAG` offset or a real address respectively.
    pub(crate) fn alloc_local(&mut self, ty_node: DyadPtr, width: usize) -> DyadPtr {
        let place = if self.frames.len() <= self.held_depth {
            // Tagged as storage, so a place and a definition's record are
            // told apart everywhere, not only where a frame exists.
            crate::dyad::global_place(self.rt.store.alloc_bytes(&vec![0u8; width]))
        } else {
            let depth = self.frames.len();
            let frame = self.frames.last_mut().unwrap();
            let offset = frame.size;
            frame.size += width;
            crate::dyad::frame_place(depth, offset)
        };
        self.rt.store.alloc_raw(ty_node, place)
    }

    /// Reject a capture: a frame-relative place of an enclosing function's
    /// frame. v1 has no closures; reading one would resolve against the wrong
    /// activation record at run time.
    ///
    /// # Safety
    /// `node` must be a resolved dyad from the store.
    unsafe fn check_capture(&self, node: DyadPtr) -> Result<(), ParseError> {
        let node = self.types.through(node);
        if let Some((depth, _)) = crate::dyad::frame_ref((*node).value) {
            if depth != self.frames.len() {
                return Err(ParseError::CapturedLocal);
            }
        }
        Ok(())
    }

    /// For every open function whose barrier the binding's scope lies below,
    /// the read is of an outer name and the binding joins its `FN_OUTER` list
    /// once; a scope on no stack (a section's) lies below every function.
    fn note_outer_read(&mut self, binding: DyadPtr) {
        if binding.is_null() || self.frames.is_empty() {
            return;
        }
        // SAFETY: a non-null binding came from the resolver or a function's list: a binding dyad.
        let scope = unsafe { Binding::read(binding).scope };
        let depth = self.scopes.position(scope).unwrap_or(0);
        for frame in &mut self.frames {
            if depth < frame.below && !frame.outer.contains(&binding) {
                frame.outer.push(binding);
            }
        }
    }

    /// A call is a use of every outer name the callee's body reads (DESIGN
    /// ›`own` and `drop` are static‹): each listed binding is checked as a bare
    /// use here would be, and joins the lists of the functions being parsed.
    ///
    /// # Safety
    /// `callee` must be null or a dyad from the store.
    pub(crate) unsafe fn check_call_reads(&mut self, callee: DyadPtr) -> Result<(), ParseError> {
        if callee.is_null() {
            return Ok(());
        }
        // SAFETY: `callee` is a dyad from the store.
        if unsafe { (*callee).ty } != self.types.fn_type {
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
            if !self.scopes.is_open(fields.scope) && !self.imports.sections.contains(&fields.scope)
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

    /// Never past a `#`: the sequence parser peeks at a statement-level `#`
    /// itself.
    fn skip_whitespace(&mut self) {
        let bytes = self.source.as_bytes();
        while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// After a parse error, the stuck point `crate::report` renders; after a
    /// success, where consumption stopped, so a caller can check for trailing
    /// input.
    pub fn offset(&self) -> usize {
        self.pos
    }

    /// A driver's top-level line is complete: see [`ScopeStack::close_item`].
    ///
    /// # Safety
    /// `item` must be a line this parser just returned.
    pub unsafe fn close_item(&mut self, item: DyadPtr) {
        // SAFETY: the pending bindings were minted by this parser's declares.
        unsafe { self.scopes.close_item(self.rt.store, self.types.array_, item) }
    }

    /// The top level's `defer free` nodes, which no `parse_sequence` drained
    /// (the top level is no block); insertion order, the caller reverses.
    pub fn take_pending_defers(&mut self) -> Vec<DyadPtr> {
        match self.open.first_mut() {
            Some(base) => std::mem::take(&mut base.defers),
            None => Vec::new(),
        }
    }

    /// Run everything parsed and not yet run, outermost scope first (DESIGN
    /// ›Build and run are one self-directing pass‹): an executable item becomes
    /// its ran form in place. The lists are taken first, so a nested drain finds nothing.
    pub fn drain(&mut self) -> Result<(), ParseError> {
        let lists: Vec<Vec<DyadPtr>> =
            self.open.iter_mut().map(|s| std::mem::take(&mut s.unrun)).collect();
        for node in lists.into_iter().flatten() {
            // SAFETY: every pending item is a dyad this parser built into its store, which outlives the pass.
            unsafe {
                let ty = (*node).ty;
                if crate::identities::numtype::is_comment_type(ty) || ty == self.types.defer_ {
                    continue;
                }
                let bits = self.run_on_pass(node).map_err(ParseError::Run)?;
                if matches!(
                    crate::identities::read::read_kind(self.types, node),
                    crate::identities::read::Read::Executable(_)
                ) {
                    crate::identities::ran::rewrite(self.rt.store, self.types, node, bits);
                }
            }
        }
        Ok(())
    }

    /// The end of the program: the root scope's own run (DESIGN ›The scope's
    /// constructor is the driver‹).
    pub fn finish(&mut self) -> Result<(), ParseError> {
        self.drain()
    }

    /// The REPL parses each line with a fresh `Parser` over one persistent
    /// scope stack.
    pub fn into_scopes(self) -> ScopeStack {
        self.scopes
    }

    pub(crate) fn expect_close(&mut self) -> Result<(), ParseError> {
        if self.feed.is_some() {
            return if self.consume_token(self.types.close_) {
                Ok(())
            } else {
                Err(ParseError::UnclosedBracket)
            };
        }
        self.skip_whitespace();
        let source = self.source;
        if self.pos >= source.len() {
            return Err(ParseError::UnclosedBracket);
        }
        let start = self.pos;
        let r = self.scopes.resolve(self.trie, &source[start..]).map_err(ParseError::Resolve)?;
        if r.identity == self.types.close_ {
            self.pos = start + r.matched;
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

    /// The constructor a `fn`, a record type, or a numeric type runs for the
    /// bracket to its right (DESIGN ›`X (…)` is one spelling, and X's
    /// constructor decides what the bracket is‹); without a `(` directly ahead
    /// the identity stands as its own value.
    pub(crate) fn construct_application(
        &mut self,
        id: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // The bracket, lexed on demand: application runs at discovery.
        if let Some(scope) = self.cell_at(tape, 1)?.filter(|c| c.is_bracket()).map(|c| c.dyad) {
            // SAFETY: `scope` is the bracket's node from the store.
            let args = unsafe { self.args_of(scope) };
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
        if (*scope).ty != self.types.scope {
            return vec![scope];
        }
        let Some(items) = crate::identities::scope::exprs_of(scope) else {
            return vec![scope];
        };
        let defer_ = self.types.defer_;
        items
            .iter()
            .copied()
            .filter(|&e| !crate::identities::numtype::is_comment_type((*e).ty) && (*e).ty != defer_)
            .collect()
    }

    /// `id`'s constructor over a fresh single-token tape, for a construct
    /// invoked from inside another constructor. `Ok(None)` when `id` has no
    /// constructor or it declined.
    fn construct_leaf(
        &mut self,
        id: DyadPtr,
        start: usize,
        len: usize,
    ) -> Result<Option<DyadPtr>, ParseError> {
        let Some(construct) = self.construct_of(id) else {
            return Ok(None);
        };
        let mut tape = ParsingTape::new();
        // SAFETY: `self.source` outlives the parse; `id` came from the index.
        tape.push(unsafe { Cell::lexed(id, self.source, start, len) });
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
            if (*leaf).ty == self.types.fn_type {
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
        // `this`: a fresh node of `owner` per run, one slot per field, null or
        // the field's default, filled by name; then the run's terminator and
        // the slot for the node's field-type set.
        // SAFETY: `owner` is the record type whose slot holds `f`; its fields are dyads.
        let mut slots: Vec<DyadPtr> = unsafe {
            crate::identities::array::items(crate::identities::meta::record_fields_of(owner))
                .iter()
                .map(|&field| if (*field).value.is_null() { std::ptr::null_mut() } else { field })
                .collect()
        };
        slots.extend([std::ptr::null_mut(); 2]);
        let run = self.rt.store.alloc_operands(&slots);
        let this = self.rt.store.alloc_raw(owner, run);
        // SAFETY: as this function's own contract; `this` was just built.
        unsafe { self.run_logos_body(f, this, tape) }
    }

    /// A `parse` body run over the tape with `this` bound to `this`.
    ///
    /// # Safety
    /// `f` must be a `fn` node over the hidden `tape`/`this` record and `this`
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
                self.pos = c.start;
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

    /// `dyad (T, v)` builds a store-owned cell of type `T` (DESIGN ›Feasibility‹):
    /// a numeric `T` takes a literal `v` committed to its width, any other `T`
    /// takes `v` as the node the value points at. Without a bracket, `dyad` stands as its value.
    pub(crate) fn construct_dyad(
        &mut self,
        id: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let types = self.types;
        let Some(bracket) = self.cell_at(tape, 1)?.filter(|c| c.is_bracket()) else {
            let value = self.stand_as_value(tape, id);
            tape.place(value);
            return Ok(Constructed::Placed);
        };
        // SAFETY: the bracket cell holds a node from the store; its arguments are reduced dyads.
        let cell = unsafe {
            let args = self.args_of(bracket.dyad);
            let [ty, value] = args[..] else {
                return Err(ParseError::CtorArity);
            };
            let ty = types.through(ty);
            if !crate::identities::is_type_value(types, ty) {
                return Err(ParseError::BadDeclaredType);
            }
            let read = types.through(value);
            // What a cell of `ty` holds is the reading rule's answer for a
            // place of `ty`: a scalar is storage at its width.
            let scalar = matches!(
                crate::identities::read::place_layout(types, ty),
                Some((crate::identities::read::Read::Scalar(_), _))
            );
            if scalar && ty != types.bool_ {
                if (*read).ty != types.rational {
                    return Err(ParseError::UnsupportedOperands);
                }
                crate::identities::commit_literal_to(self.rt.store, types, read, ty)?
            } else if scalar {
                // `bool` is physically an i32 0/1 in storage; only `true` and
                // `false` have bits at parse.
                if (*read).ty != types.bool_ || (*read).value.is_null() {
                    return Err(ParseError::UnsupportedOperands);
                }
                let bits = std::ptr::read_unaligned((*read).value as *const i32);
                let storage = self.rt.store.alloc_bytes(&bits.to_ne_bytes());
                self.rt.store.alloc_raw(types.bool_, storage)
            } else {
                // No other type has a whole value a cell can hold from a
                // node: refused rather than guessed.
                return Err(ParseError::BadDyadType);
            }
        };
        tape.remove(1);
        tape.place(cell);
        Ok(Constructed::Placed)
    }

    /// `?`'s constructor (DESIGN ›Declarations are immutable by default‹): the
    /// one unknown standing as its value, or with a type to its left that
    /// type's valueless place, the cells to the left consumed.
    pub(crate) fn construct_hole(
        &mut self,
        id: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let types = self.types;
        let base = match tape.at(-1).copied() {
            Some(cell) => {
                // A fresh name to the left is not a type; leave it for the
                // boundary's own report. An operator still waiting for its
                // turn (`x != ?`) is no type either.
                let left = self.cell_identity(&cell);
                let waiting = !cell.constructed
                    && !cell.is_fresh()
                    && self.ctor_of(left).is_some()
                    && self.precedence_of_cell(left) < crate::identities::meta::prec::HOLE;
                if cell.is_fresh() || waiting {
                    None
                } else {
                    // A box the pass has already filled declares with the
                    // type it holds.
                    let d = self.operand_dyad(cell)?;
                    let d = self.settled_type(d);
                    // SAFETY: `d` is a resolved dyad from the store.
                    unsafe {
                        // A place holding a type cannot say what a hole's
                        // layout is (DESIGN ›A type is a comptime value‹).
                        match crate::identities::read::read_kind(types, d) {
                            crate::identities::read::Read::Identity => Some(d),
                            crate::identities::read::Read::Container(t)
                                if t == types.type_ || t == types.dyad_ =>
                            {
                                return Err(ParseError::TypeKnownOnlyAtRun);
                            }
                            _ if crate::identities::yields_type(types, d) => {
                                return Err(ParseError::TypeKnownOnlyAtRun);
                            }
                            _ => None,
                        }
                    }
                }
            }
            None => None,
        };
        let node = match base {
            None => self.stand_as_value(tape, id),
            Some(t) => {
                // The width comes from the same reading rule a read of the
                // place consults, so allocation and read cannot disagree; a
                // `bool` place is refused since its literals cannot yet be stored into one.
                // SAFETY: `t` is a type node from the store.
                let place = unsafe {
                    if t == types.bool_ {
                        return Err(ParseError::NonNumericDeclaredType);
                    }
                    let Some((_, width)) = crate::identities::read::place_layout(types, t) else {
                        return Err(ParseError::NonNumericDeclaredType);
                    };
                    self.alloc_local(t, width)
                };
                tape.remove(-1);
                self.holes.insert(place);
                place
            }
        };
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// `fn`'s constructor claims it so a recursive self-call inside the body
    /// resolves the published signature.
    pub(crate) fn take_pending_fn(&mut self) -> DyadPtr {
        std::mem::replace(&mut self.pending_fn, std::ptr::null_mut())
    }

    /// `fn`'s constructor suppresses the handoff around a literal that does
    /// not open its expression, so a grouped literal deeper in the same
    /// declaration can still claim it.
    pub(crate) fn restore_pending_fn(&mut self, pending: DyadPtr) {
        self.pending_fn = pending;
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
                && (*id).ty == self.types.type_
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
    fn condition_ends(&self, tape: &ParsingTape, next: Option<&Cell>) -> bool {
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
            && unsafe { (*id).ty } != self.types.type_
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
        self.discovering
            && self.at_open()
            && !(self.lex_mode == Some(RightSide::Condition) && self.condition_ends(tape, None))
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
                            self.pos = c.start;
                            return Err(ParseError::Resolve(e));
                        }
                    }
                } else {
                    (self.cell_identity(&c), c.binding(self.types))
                };
                // SAFETY: `id` is a resolved dyad from the store.
                unsafe { self.check_capture(id)? };
                self.note_outer_read(binding);
                Ok(if binding.is_null() { id } else { binding })
            }
        }
    }

    /// Whether the next token is a tight read (`:` or `.`) that takes a
    /// right-side reader's own cell before the reader wakes (DESIGN ›Text is
    /// the quote‹): the reader sleeps, and the read finds it at the boundary.
    fn tight_read_takes(&mut self, id: DyadPtr) -> bool {
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
    ) -> Result<Option<DyadPtr>, ParseError> {
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
                let e = self.leftover_error(*first);
                self.pos = *start;
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

    /// `None` at end of input or when nothing resolves.
    fn peek_token(&mut self) -> Option<(DyadPtr, usize)> {
        if self.feed.is_some() {
            return self.feed_peek();
        }
        self.skip_whitespace();
        let source = self.source;
        if self.pos >= source.len() {
            return None;
        }
        let r = self.scopes.resolve(self.trie, &source[self.pos..]).ok()?;
        Some((r.identity, r.matched))
    }

    fn consume_token(&mut self, id: DyadPtr) -> bool {
        match self.peek_token() {
            Some((t, matched)) if t == id => {
                self.pos += matched;
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
    fn at_branch_end(&mut self) -> bool {
        matches!(self.peek_token(), Some((id, _)) if id == self.types.sep_ || id == self.types.else_)
    }

    fn consume_separator(&mut self) -> bool {
        self.consume_token(self.types.sep_)
    }

    /// `)` or `]`: a closer ends the body, and the opener's own expect-helper
    /// checks it is the matching one.
    fn at_close(&mut self) -> bool {
        matches!(self.peek_token(), Some((id, _)) if id == self.types.close_ || id == self.types.close_sq_)
    }

    /// One spelling at the cursor, a declared name or a fresh run, never a
    /// bracket, the separator, a quote or the comment mark: a fresh name is
    /// not yet in the index, and a member resolves in its owner's scope, not here.
    fn lex_spelling(&mut self) -> Option<(usize, usize)> {
        self.lex_spelling_fresh().map(|(start, len, _)| (start, len))
    }

    /// `lex_spelling` keeping whether the spelling is fresh: what a position
    /// that may hold a declaration or a use, `for`'s first cell, decides by.
    fn lex_spelling_fresh(&mut self) -> Option<(usize, usize, bool)> {
        if self.feed.is_some() {
            let i = self.feed_index()?;
            let spelling = self.feed.as_ref()?.cells[i].spelling();
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
        let source = self.source;
        let start = self.pos;
        let rest = source.get(start..)?;
        if matches!(rest.as_bytes().first(), Some(b'(' | b')' | b'[' | b']' | b',' | b'#') | None)
            || rest.starts_with('«')
        {
            return None;
        }
        let r = self.scopes.lex(self.trie, rest).ok()?;
        self.pos = start + r.matched;
        Some((start, r.matched, r.fresh))
    }

    /// `( field-list )` as a record node whose value is a `RECORD_TAG` record
    /// holding the scope, the `fields` array and the packed `size_bytes`. Fresh
    /// field names are read raw, which is why the list has its own sub-parse.
    pub fn parse_record(&mut self) -> Result<DyadPtr, ParseError> {
        self.parse_record_taking(None)
    }

    /// `parse_record` with a field no text spells declared first.
    fn parse_record_taking(
        &mut self,
        leading: Option<(&str, DyadPtr)>,
    ) -> Result<DyadPtr, ParseError> {
        let record_logos = self.types.type_;
        let (scope, fields_arr, size_bytes) = self.parse_field_list(false, leading)?;
        let record = crate::identities::meta::record_layout(
            self.rt.store,
            scope,
            fields_arr,
            size_bytes,
            std::ptr::null_mut(),
            crate::identities::meta::prec::APPLY,
            Assoc::Left,
        );
        Ok(self.rt.store.alloc_raw(record_logos, record.cast()))
    }

    /// A `fn`'s parameter list checks each name against every open scope, since
    /// the body reopens the list's scope; a `fields = (…)` block's fields
    /// against their siblings alone (`relaxed`; DESIGN ›The constructor is a field‹).
    fn parse_field_list(
        &mut self,
        relaxed: bool,
        leading: Option<(&str, DyadPtr)>,
    ) -> Result<(DyadPtr, DyadPtr, u64), ParseError> {
        let at = self.pos;
        self.expect_open()?;
        // Field names are declared into the record's own scope.
        let scope = self.open_scope();

        let mut fields = Vec::new();
        if let Some((name, ty)) = leading {
            let field = self.rt.store.alloc_raw(ty, std::ptr::null_mut());
            self.declare_name(name, field, at)?;
            fields.push(field);
        }
        loop {
            if self.at_close() {
                break;
            }
            let (start, len) = self.lex_spelling().ok_or(ParseError::ExpectedField)?;
            // `self.source` is `&'a str` (Copy), so the slice is independent
            // of the `&mut self` needed below.
            let source = self.source;
            let name = &source[start..start + len];
            let word = self.scopes.resolve(self.trie, name).ok().map(|r| r.identity);
            // One place stored with the type, not a field of the layout; legal
            // in a `fields = (…)` block alone (DESIGN ›Two muts, and the storage partition‹).
            if word == Some(self.types.shared_) {
                if !relaxed {
                    self.pos = start;
                    return Err(ParseError::SharedOutsideFieldsBlock);
                }
                if let Some(def) = self.definitions.last_mut() {
                    def.block = Some((scope, fields.clone()));
                }
                self.shared_member(start)?;
                continue;
            }
            // `mut` or `immut` before a field or parameter gates its binding, as before any name.
            let gate = word.filter(|&w| w == self.types.mut_ || w == self.types.immut_);
            let (start, name) = if gate.is_some() {
                let (start, len) = self.lex_spelling().ok_or(ParseError::ExpectedField)?;
                (start, &source[start..start + len])
            } else {
                (start, name)
            };
            // A slot fill inside the block is written `shared run = (…)`: an
            // unmarked one would be a per-instance default, not in the seed.
            if relaxed {
                let def = self.definitions.last().expect("a relaxed field list is a fields block");
                let id = self.scopes.resolve(self.trie, name).ok().map(|r| r.identity);
                if id.is_some_and(|id| id == self.types.drop_ || def.slots.contains(&id)) {
                    self.pos = start;
                    return Err(ParseError::FieldsSlotNeedsShared);
                }
            }
            // `name := T ?` declares the field's type through the hole `?`
            // built; a bare name leaves the type slot undefined. A default,
            // `size := u64 0`, is a constant: the field's dyad is that value,
            // and each new node starts with it in the field's slot.
            let mut default = std::ptr::null_mut();
            let logos = if self.consume_token(self.types.declare_tok) {
                let value = self.parse_expression()?;
                if self.holes.remove(&value) {
                    // SAFETY: `value` is the place `?` just built.
                    unsafe { (*value).ty }
                } else {
                    // SAFETY: `value` is a reduced dyad just parsed.
                    let held = unsafe { self.types.through(value) };
                    if held == self.types.unknown {
                        std::ptr::null_mut() // `name := ?`: no type yet
                    } else if relaxed
                        // SAFETY: as above.
                        && unsafe {
                            matches!(
                                crate::identities::read::read_kind(self.types, held),
                                crate::identities::read::Read::Scalar(_)
                            ) && !crate::dyad::is_place((*held).value)
                        }
                    {
                        // SAFETY: as above.
                        let (ty, value) = unsafe { ((*held).ty, (*held).value) };
                        default = value;
                        ty
                    } else {
                        self.pos = start;
                        return Err(ParseError::BadDeclaredType);
                    }
                }
            } else {
                std::ptr::null_mut()
            };
            let field = self.rt.store.alloc_raw(logos, default);
            // The field's name is not stored on the record: declaring it puts
            // a binding in the one name index (DESIGN ›Name resolution is scope-filtered‹).
            let binding = if relaxed {
                // One block, one no-shadowing rule: a field is checked against
                // the `shared` members too, which live in the type's own scope.
                let body =
                    self.definitions.last().expect("a relaxed field list is a fields block").scope;
                if self.scopes.declared_in(self.trie, name, body).map_err(ParseError::Resolve)? {
                    self.pos = start;
                    return Err(ParseError::Resolve(ResolveError::Shadowed(name.to_string())));
                }
                self.declare_field_name(name, field, start)?
            } else {
                self.declare_name(name, field, start)?
            };
            if let Some(gate) = gate {
                self.add_gate(binding, gate)?;
            }
            fields.push(field);
            if !self.consume_separator() {
                break;
            }
        }

        self.scopes.pop();
        self.expect_close()?;

        // Fields pack in declaration order at the width rule parameters claim
        // frame offsets by.
        // SAFETY: each field is the dyad just built, its type null or a type node.
        let size_bytes: u64 = fields.iter().map(|&f| unsafe { field_width((*f).ty) }).sum();
        let fields_arr = crate::identities::array::build(self.rt.store, self.types.array_, &fields);
        Ok((scope, fields_arr, size_bytes))
    }

    /// `shared name := value` in a `fields = (…)` block: one place stored
    /// with the type, declared in the body scope (DESIGN ›Two muts, and the
    /// storage partition‹). The block stays one scope for no-shadowing.
    fn shared_member(&mut self, at: usize) -> Result<(), ParseError> {
        let body = match self.definitions.last() {
            Some(def) => def.scope,
            None => {
                self.pos = at;
                return Err(ParseError::SharedOutsideFieldsBlock);
            }
        };
        if self.consume_token(self.types.declare_tok) {
            self.pos = at;
            return Err(ParseError::SharedNeedsDeclaration);
        }
        let field_scope = self.scopes.pop().expect("the field list's scope is open");
        self.definitions.last_mut().expect("checked above").in_block = true;
        self.last_declared = std::ptr::null_mut();
        let outer_member = self.member_fn_depth.replace(self.frames.len());
        let items = self.shared_line();
        self.member_fn_depth = outer_member;
        self.definitions.last_mut().expect("checked above").in_block = false;
        self.scopes.push(field_scope);
        // The line's one declaration, a member or a slot fill, with its prose
        // lifted out beside it; anything else is not what `shared` marks.
        let mut declared = None;
        for item in items? {
            // SAFETY: `item` is a reduced dyad just parsed.
            let ty = unsafe { (*item).ty };
            if ty == self.types.comment_ {
                // SAFETY: the pending bindings were minted by this parser's declares.
                unsafe { self.scopes.settle_item(body, item) };
            } else if ty == self.types.declare_ && declared.is_none() {
                declared = Some(item);
            } else {
                self.pos = at;
                return Err(ParseError::SharedNeedsDeclaration);
            }
        }
        let Some(item) = declared else {
            self.pos = at;
            return Err(ParseError::SharedNeedsDeclaration);
        };
        if !self.is_slot_fill(item) {
            // SAFETY: a declaration that fills no slot is `:=`'s, its lhs the binding.
            let name = unsafe { Binding::spelling(crate::identities::declare::binding_of(item)) };
            if self
                .scopes
                .declared_in(self.trie, &name, field_scope)
                .map_err(ParseError::Resolve)?
            {
                self.pos = at;
                return Err(ParseError::Resolve(ResolveError::Shadowed(name)));
            }
            let binding = self.last_declared;
            if binding.is_null() {
                self.pos = at;
                return Err(ParseError::SharedNeedsDeclaration);
            }
            self.add_gate(binding, self.types.shared_)?;
        }
        // SAFETY: the pending bindings were minted by this parser's declares.
        unsafe { self.scopes.settle_item(body, item) };
        Ok(())
    }

    /// The first `parse_next` lexes and constructs the segment and queues
    /// what it yielded; the rest are taken while queued, before anything
    /// further is lexed, so none is left for the enclosing body's loop.
    fn shared_line(&mut self) -> Result<Vec<DyadPtr>, ParseError> {
        let mut items = Vec::new();
        let Some(first) = self.parse_next() else {
            return Ok(items);
        };
        items.push(first?);
        while !self.queued.is_empty() {
            items.push(self.parse_next().expect("a queued item comes out first")?);
        }
        Ok(items)
    }

    /// A slot fill's item declares the definition's own marker.
    fn is_slot_fill(&self, item: DyadPtr) -> bool {
        // SAFETY: `item` is a declare node from the store.
        let declared = unsafe { crate::identities::declare::declared_of(item) };
        self.definitions
            .last()
            .is_some_and(|def| def.slots.contains(&declared) || def.drop_marker == declared)
    }

    /// An ordinary scope whose bare lines fill the type's own slots and whose
    /// `fields = (…)` block holds what lives on instances (DESIGN ›The
    /// constructor is a field‹); every other line must be prose, since nothing runs a type body later.
    pub fn parse_type_body(&mut self, id: DyadPtr) -> Result<DyadPtr, ParseError> {
        if self.runtime_depth > 0 {
            return self.hold_type_body();
        }
        self.expect_open()?;
        // The slot words are the fields `type` declares (identities/type.logos):
        // names on the body's lines and nowhere outside them, in a scope of
        // their own so that a member read through the type never finds them.
        // Each is a marker of this definition, declared against its siblings
        // alone, so that a body inside another's shadows its words.
        let words = self.open_scope();
        let slots = SLOT_NAMES.map(|_| {
            let head = crate::identities::meta::record(
                self.rt.store,
                crate::identities::meta::TYPEREC_TAG,
                crate::identities::meta::prec::INERT,
            );
            self.rt.store.alloc_raw(self.types.type_, head)
        });
        for (name, &marker) in SLOT_NAMES.iter().zip(&slots) {
            let binding = self.mint_binding(marker, words, name.as_bytes());
            // SAFETY: `binding` was minted by `Binding::alloc` just above.
            unsafe { self.scopes.declare_field(self.trie, name, binding) }
                .map_err(ParseError::Resolve)?;
        }
        let head = crate::identities::meta::record(
            self.rt.store,
            crate::identities::meta::TYPEREC_TAG,
            crate::identities::meta::prec::INERT,
        );
        let drop_marker = self.rt.store.alloc_raw(self.types.type_, head);
        let scope = self.open_scope();
        self.definitions.push(OpenType {
            scope,
            slots,
            drop_marker,
            parse_rank: crate::identities::meta::prec::APPLY,
            lex_rank: None,
            assoc: Assoc::Left,
            ctor: std::ptr::null_mut(),
            run_body: std::ptr::null_mut(),
            instance: None,
            this_param: std::ptr::null_mut(),
            in_block: false,
            block: None,
            instances_ctor: std::ptr::null_mut(),
            instances_rank: crate::identities::meta::prec::APPLY,
            instances_assoc: Assoc::Left,
        });
        // A `fn` literal on a slot's right side must not claim the enclosing
        // declaration's placeholder.
        let suppressed = self.take_pending_fn();
        // The body's declarations are pending until its close: a type is
        // comptime and its members are stored at the definition. Everything
        // before the body runs first, and a failure among its lines is the body's.
        let outer = self.drain();
        self.open.push(OpenScope::default());
        let saved_depth = std::mem::replace(&mut self.runtime_depth, 0);
        let lines = outer.and_then(|()| self.type_body_lines(scope)).and_then(|()| {
            self.drain().map_err(|e| match e {
                ParseError::Run(r) => {
                    ParseError::TypeBodyFailed(Box::new(crate::report::run_message(&r)))
                }
                other => other,
            })
        });
        self.runtime_depth = saved_depth;
        let defers = self.open.pop().expect("pushed above").defers;
        self.restore_pending_fn(suppressed);
        let def = self.definitions.pop().expect("pushed above");
        self.scopes.pop();
        self.scopes.pop();
        lines?;
        if !defers.is_empty() {
            return Err(ParseError::DeferInTypeBody);
        }
        self.expect_close()?;
        let (field_scope, fields, size_bytes) = match def.instance {
            Some(instance) => instance,
            None => {
                let field_scope = self.rt.store.alloc_raw(self.types.scope, std::ptr::null_mut());
                let fields = crate::identities::array::build(self.rt.store, self.types.array_, &[]);
                (field_scope, fields, 0)
            }
        };
        let layout = crate::identities::meta::record_layout(
            self.rt.store,
            field_scope,
            fields,
            size_bytes,
            scope,
            def.parse_rank,
            def.assoc,
        );
        let node = self.rt.store.alloc_raw(id, layout.cast());
        if let Some(rank) = def.lex_rank {
            // The rank is the name's, not the type's: it goes on the binding of
            // the declaration this body is the value of.
            let Some(&binding) = self.filling.last() else {
                return Err(ParseError::LexRankNeedsName);
            };
            // SAFETY: `binding` is a binding dyad `:=` minted before driving its value, live for the whole drive.
            unsafe { Binding::set_lex_rank(binding, rank) };
        }
        if !def.ctor.is_null() {
            // SAFETY: `node` was just built; nothing has read its slot.
            unsafe { crate::identities::meta::install_constructor(node, def.ctor) };
        }
        if !def.run_body.is_null() {
            // SAFETY: `node` was just built; `def.run_body` is the `lex` node the fill built.
            unsafe { crate::identities::meta::install_run_body(node, def.run_body) };
        }
        if !def.instances_ctor.is_null() && def.ctor.is_null() {
            return Err(ParseError::InstancesParseNeedsOwnParse);
        }
        // SAFETY: `node` was just built with a record layout; the ctor is null or the fill's fn.
        unsafe {
            crate::identities::meta::install_instances_parse(
                node,
                def.instances_ctor,
                def.instances_rank,
                def.instances_assoc,
            )
        };
        Ok(node)
    }

    /// A `type (…)` in a body that runs later reads that run's values, so it is
    /// lexed now and built each time it runs (DESIGN ›Deferral is authored‹).
    fn hold_type_body(&mut self) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let (start, len) = self.body_text_extent()?;
        let source = self.source;
        let text = crate::identities::string::build_text(
            self.rt.store,
            types.string_,
            &source.as_bytes()[start - 1..start + len + 1],
        );
        // SAFETY: `text` is the string node just built; its bytes live for the store.
        let held = unsafe { crate::identities::string::text(text) };
        let held = std::str::from_utf8(held).expect("copied from the source text");
        let fragment = self.lex_body_fragment(held)?;
        let cells = Box::into_raw(Box::new(fragment));
        let scope = self.scopes.current().unwrap_or(std::ptr::null_mut());
        Ok(crate::identities::held_type::build(
            self.rt.store,
            types,
            text,
            cells,
            scope,
            self.frames.len(),
        ))
    }

    /// Build the held `type (…)` `node` now, inside the run that reached it: its
    /// cells constructed over the scopes open where it was written, its reads
    /// of that function's names reading this call's frame. Everything the
    /// pass had open is set aside and restored.
    ///
    /// # Safety
    /// `node` must be a held type node from the store; the call it was written
    /// in must be the innermost activation.
    unsafe fn construct_held_type(
        &mut self,
        node: DyadPtr,
    ) -> Result<DyadPtr, crate::run::RunError> {
        let (text, cells, scope, depth) = crate::identities::held_type::parts(node);
        let bytes = crate::identities::string::text(text);
        let text: &'a str = std::str::from_utf8(bytes).expect("copied from the source text");
        let mut cells = (*cells).cells();
        for cell in &mut cells {
            if cell.is_fresh() {
                cell.dyad = self.rt.store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
            }
        }
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
        // One frame per function open where the body was written, so a read of
        // that function's names is no capture; nothing is claimed in them.
        let frames = (0..depth)
            .map(|_| OpenFn {
                size: 0,
                below: 0,
                outer: Vec::new(),
                open_below: 0,
                returns: Vec::new(),
            })
            .collect();

        let saved_source = std::mem::replace(&mut self.source, text);
        let saved_pos = std::mem::replace(&mut self.pos, 0);
        let saved_scopes = std::mem::replace(&mut self.scopes, nested);
        let saved_frames = std::mem::replace(&mut self.frames, frames);
        let saved_held_depth = std::mem::replace(&mut self.held_depth, depth);
        let saved_definitions = std::mem::take(&mut self.definitions);
        let saved_open = std::mem::replace(&mut self.open, vec![OpenScope::default()]);
        let saved_filling = std::mem::take(&mut self.filling);
        let saved_pending_fn = std::mem::replace(&mut self.pending_fn, std::ptr::null_mut());
        let saved_runtime_depth = std::mem::replace(&mut self.runtime_depth, 0);
        let saved_lex_mode = self.lex_mode.take();
        let saved_else_ends = self.else_ends.take();
        let saved_lifted = std::mem::take(&mut self.lifted);
        let saved_queued = std::mem::take(&mut self.queued);
        let saved_discovering = std::mem::replace(&mut self.discovering, false);
        let saved_run_body = self.run_body.take();
        let saved_feed = self.feed.replace(Feed { cells, next: 0, base_depth, in_run_order: true });

        let built = self.parse_type_body(self.types.type_);
        let built_pos = self.pos;

        self.source = saved_source;
        self.pos = saved_pos;
        self.scopes = saved_scopes;
        self.frames = saved_frames;
        self.held_depth = saved_held_depth;
        self.definitions = saved_definitions;
        self.open = saved_open;
        self.filling = saved_filling;
        self.pending_fn = saved_pending_fn;
        self.runtime_depth = saved_runtime_depth;
        self.lex_mode = saved_lex_mode;
        self.else_ends = saved_else_ends;
        self.lifted = saved_lifted;
        self.queued = saved_queued;
        self.discovering = saved_discovering;
        self.run_body = saved_run_body;
        self.feed = saved_feed;

        built.map_err(|e| {
            crate::run::RunError::MintFailed(Box::new(crate::report::render(
                "type body",
                text,
                built_pos,
                &crate::report::parse_message(&e),
            )))
        })
    }

    /// Each line settled as the body item of the names it declared and
    /// checked to be a kind a body holds.
    fn type_body_lines(&mut self, scope: DyadPtr) -> Result<(), ParseError> {
        // The lines are pending as they parse and run at the body's close;
        // this loop only checks what each line is.
        while let Some(item) = self.parse_next() {
            let item = item?;
            // SAFETY: the pending bindings were minted by this parser's declares.
            unsafe { self.scopes.settle_item(scope, item) };
            // SAFETY: `item` is a reduced dyad just parsed.
            let kind = unsafe {
                let ty = (*item).ty;
                if crate::identities::numtype::is_comment_type(ty) {
                    BodyLine::Prose
                } else if ty == self.types.declare_ {
                    BodyLine::Declare
                } else {
                    BodyLine::Other
                }
            };
            match kind {
                // A bare `fields` line stands as the marker and declares nothing.
                BodyLine::Other => return Err(ParseError::TypeBodyLine),
                BodyLine::Declare if !self.is_slot_fill(item) => {
                    return Err(ParseError::MemberOutsideFieldsBlock);
                }
                BodyLine::Prose | BodyLine::Declare => {}
            }
        }
        Ok(())
    }

    /// The block of per-instance fields, parsed as a field list checked
    /// against its siblings alone; `=` calls this before reading its right
    /// side, since the bracket is a field list, not an expression.
    pub(crate) fn fields_block_fill(&mut self, target: DyadPtr) -> Result<DyadPtr, ParseError> {
        let def = self.definitions.last().expect("slot_of found an open definition");
        // `shared fields = (…)` inside the block is the instances' own slot,
        // not in the seed; the enclosing block is stored only when its list
        // closes, so the double-block check below would not see it.
        if def.in_block {
            return Err(ParseError::FieldsSlotNotInSeed);
        }
        if def.instance.is_some() {
            return Err(ParseError::DoubleFields);
        }
        let instance = self.parse_field_list(true, None)?;
        self.definitions.last_mut().expect("checked above").instance = Some(instance);
        Ok(self.slot_declare(SlotKind::Fields, target, instance.1))
    }

    /// A use of one of the slot words, or of `drop`; whether the fill reaches
    /// a type being defined is `filling_definition`'s question.
    ///
    /// # Safety
    /// `target` must be a dyad from the store.
    pub(crate) unsafe fn slot_of(&self, target: DyadPtr) -> Option<SlotKind> {
        // The binding of the word's use, or the identity itself when its
        // constructor stood aside (`drop`).
        // SAFETY: `target` is a reduced dyad from the store.
        unsafe {
            let id = if (*target).ty == self.types.binding_ {
                Binding::read(target).dyad
            } else {
                target
            };
            if id == self.types.drop_ {
                return Some(SlotKind::Drop);
            }
            self.definitions
                .iter()
                .rev()
                .find_map(|def| def.slots.iter().position(|&m| m == id))
                .map(SlotKind::of)
        }
    }

    /// The current scope is the innermost open definition's own (DESIGN ›The
    /// constructor is a field‹): a `parse_rank = 3` inside a constructor's
    /// body is that function's own business, refused.
    pub(crate) fn filling_definition(&self) -> bool {
        self.definitions.last().is_some_and(|def| self.scopes.current() == Some(def.scope))
    }

    /// A literal molds to f64 without running; a concrete expression runs
    /// now, after everything parsed before it (a rank may read a variable).
    /// Anything else is `NonComptimeRank`.
    fn rank_value(&mut self, value: DyadPtr, read: DyadPtr) -> Result<f64, ParseError> {
        use crate::identities::numtype::NumType;
        // SAFETY: `value` is a reduced dyad from the store.
        let nt = match unsafe { crate::identities::numtype_of(self.types, value) } {
            crate::identities::Operand::Literal => None,
            crate::identities::Operand::Concrete(nt) => Some(nt),
            _ => return Err(ParseError::NonComptimeRank),
        };
        let bits = match nt {
            None => crate::identities::rational::mold_to(read, NumType::F64)
                .ok_or(ParseError::UncomputableLiteral)?,
            Some(_) => {
                self.drain()?;
                // SAFETY: `value` is a reduced dyad from the store.
                unsafe { self.run_on_pass(value) }.map_err(|_| ParseError::NonComptimeRank)?
            }
        };
        Ok(match nt {
            None | Some(NumType::F64) => f64::from_bits(bits as u64),
            Some(NumType::F32) => f64::from(f32::from_bits(bits as u32)),
            Some(_) => bits as f64,
        })
    }

    /// A slot `type` declared is filled with `=` (DESIGN ›The constructor is
    /// a field‹): the parse_rank must be known at the definition; a body slot
    /// takes a bracket, `slot_body_fill`'s; the fill is a silent statement.
    ///
    /// # Safety
    /// `value` must be a dyad from the store.
    pub(crate) unsafe fn slot_fill(
        &mut self,
        kind: SlotKind,
        target: DyadPtr,
        value: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        // Inside the fields block the slots are the instances', and a type has
        // no run of its own; the instances' drop, and a type's own drop on a
        // bare line, are not in the seed yet (stand-in for #133).
        let in_block = self.definitions.last().expect("slot_of found an open definition").in_block;
        match kind {
            SlotKind::Drop => return Err(ParseError::DropSlotNotInSeed),
            SlotKind::Run if !in_block => return Err(ParseError::NoOwnRun),
            SlotKind::Parse | SlotKind::Run => return Err(ParseError::SlotNeedsBody(kind)),
            SlotKind::LexRank if in_block => return Err(ParseError::FieldsSlotNotInSeed),
            _ => {}
        }
        // SAFETY: `value` is a reduced dyad from the store.
        let read = unsafe { types.through(value) };
        // Computed before the open definition is borrowed, since computing it
        // may run what stands before it.
        let rank = match kind {
            SlotKind::ParseRank | SlotKind::LexRank => Some(self.rank_value(value, read)?),
            _ => None,
        };
        let def = self.definitions.last_mut().expect("slot_of found an open definition");
        match kind {
            SlotKind::ParseRank => {
                if let Some(rank) = rank {
                    if in_block {
                        def.instances_rank = rank;
                    } else {
                        def.parse_rank = rank;
                    }
                }
            }
            SlotKind::LexRank => {
                if let Some(rank) = rank {
                    def.lex_rank = Some(rank);
                }
            }
            SlotKind::Associativity => {
                let assoc = if read == types.left_ {
                    Assoc::Left
                } else if read == types.right_ {
                    Assoc::Right
                } else {
                    return Err(ParseError::BadAssociativity);
                };
                if in_block {
                    def.instances_assoc = assoc;
                } else {
                    def.assoc = assoc;
                }
            }
            SlotKind::Parse | SlotKind::Run | SlotKind::Fields | SlotKind::Drop => {
                unreachable!(
                    "body slots and `drop` returned above; `fields` is routed to fields_block_fill by `=`"
                )
            }
        }
        Ok(self.slot_declare(kind, target, value))
    }

    /// The line's item for a slot fill: a declare node over the slot word's
    /// use and the right side, declaring the definition's marker, which is how
    /// the body's close tells a fill from a member declaration.
    fn slot_declare(&mut self, kind: SlotKind, target: DyadPtr, rhs: DyadPtr) -> DyadPtr {
        let types = self.types;
        let def = self.definitions.last().expect("a slot is filled inside a definition");
        let marker = match kind {
            SlotKind::Drop => def.drop_marker,
            kind => def.slots[kind as usize],
        };
        crate::identities::declare::build(
            self.rt.store,
            types.declare_,
            types.ops.declare_,
            target,
            rhs,
            marker,
        )
    }

    /// A slot body read bare, no parameter list (DESIGN ›Execution is function
    /// application‹): `parse` over a hidden `tape`/`this` record, `run` held as
    /// its lexed tape and constructed per field-type set (DESIGN ›Deferral is authored‹).
    pub(crate) fn slot_body_fill(
        &mut self,
        kind: SlotKind,
        target: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let in_block = self.definitions.last().expect("slot_of found an open definition").in_block;
        match kind {
            SlotKind::Parse => {
                let at = self.pos;
                // The two names `parse` declares into its body: `tape`, centred
                // on the appearance, and `this`, the fresh node on a bare line,
                // the instance that appeared inside the fields block.
                let (input, params) = self.hidden_param_record(
                    &[(Some("tape"), types.tape.parsing_tape), (Some("this"), types.dyad_)],
                    at,
                )?;
                let def = self.definitions.last_mut().expect("checked above");
                def.this_param = params[1];
                // SAFETY: `input` was just built, its parameters unplaced; no declaration's placeholder is being filled.
                let f = unsafe {
                    self.fn_over_body(types.fn_type, input, types.void_, std::ptr::null_mut())
                };
                let def = self.definitions.last_mut().expect("checked above");
                def.this_param = std::ptr::null_mut();
                let f = f?;
                if in_block {
                    def.instances_ctor = f;
                } else {
                    def.ctor = f;
                }
                Ok(self.slot_declare(SlotKind::Parse, target, f))
            }
            SlotKind::Run if !in_block => Err(ParseError::NoOwnRun),
            SlotKind::Run => {
                // The body with its brackets is copied into the store (a
                // source may not outlive the type, the store does) and lexed
                // once, now, against the scopes open here (DESIGN ›Deferral is authored‹).
                let (start, len) = self.body_text_extent()?;
                let source = self.source;
                let text = crate::identities::string::build_text(
                    self.rt.store,
                    types.string_,
                    &source.as_bytes()[start - 1..start + len + 1],
                );
                // SAFETY: `text` is the string node just built; its bytes live for the store.
                let held = unsafe { crate::identities::string::text(text) };
                let held = std::str::from_utf8(held).expect("copied from the source text");
                let fragment = self.lex_body_fragment(held)?;
                let cells = Box::into_raw(Box::new(fragment));
                let body = crate::identities::run_body::build(self.rt.store, types, text, cells);
                self.definitions.last_mut().expect("checked above").run_body = body;
                Ok(self.slot_declare(SlotKind::Run, target, body))
            }
            SlotKind::Drop if in_block => {
                // Held as its text and never built or run: the instances' drop is
                // not in the seed yet (stand-in for #133).
                let (start, len) = self.body_text_extent()?;
                let source = self.source;
                let text = crate::identities::string::build_text(
                    self.rt.store,
                    types.string_,
                    &source.as_bytes()[start - 1..start + len + 1],
                );
                Ok(self.slot_declare(SlotKind::Drop, target, text))
            }
            SlotKind::Drop => Err(ParseError::DropSlotNotInSeed),
            _ => unreachable!("`=` reads a bare body for `parse`, `run` and `drop` only"),
        }
    }

    /// A parameter record no text spelled: each name declared in its own
    /// scope and checked against every open scope, since the body reopens it;
    /// value slots left null for `fn_over_body` to place. `at` is the error position.
    fn hidden_param_record(
        &mut self,
        params: &[(Option<&str>, DyadPtr)],
        at: usize,
    ) -> Result<(DyadPtr, Vec<DyadPtr>), ParseError> {
        let scope = self.open_scope();
        let mut fields = Vec::with_capacity(params.len());
        let mut declared = Ok(());
        for &(name, ty) in params {
            let field = self.rt.store.alloc_raw(ty, std::ptr::null_mut());
            // A parameter with no name is a place the body reaches another
            // way: a run body's field, through `this.f`.
            if let Some(name) = name {
                declared = declared.and_then(|()| self.declare_name(name, field, at).map(|_| ()));
            }
            fields.push(field);
        }
        self.scopes.pop();
        declared?;
        let fields_arr = crate::identities::array::build(self.rt.store, self.types.array_, &fields);
        // SAFETY: each `ty` is a type node from the store.
        let size_bytes = fields.iter().map(|&f| unsafe { field_width((*f).ty) }).sum();
        let record = crate::identities::meta::record_layout(
            self.rt.store,
            scope,
            fields_arr,
            size_bytes,
            std::ptr::null_mut(),
            crate::identities::meta::prec::APPLY,
            Assoc::Left,
        );
        Ok((self.rt.store.alloc_raw(self.types.type_, record.cast()), fields))
    }

    /// The slot of the node being built that the instance field `f` names:
    /// `f` resolves in the block's own scope alone, and its index among the
    /// fields is the slot.
    fn this_field(&mut self, name: &str, at: usize) -> Result<DyadPtr, ParseError> {
        let def = self.definitions.last().expect("this_param is set inside a definition");
        let (this, body) = (def.this_param, def.scope);
        let (scope, items) = match (def.instance, &def.block) {
            (Some((scope, fields, _)), _) => {
                // SAFETY: `fields` is the block's array node, its items the field dyads.
                (scope, unsafe { crate::identities::array::items(fields) }.to_vec())
            }
            (None, Some((scope, fields))) => (*scope, fields.clone()),
            (None, None) => {
                self.pos = at;
                return Err(ParseError::ThisNeedsFieldsBlock);
            }
        };
        let mut field_scope = ScopeStack::new();
        field_scope.push(scope);
        let resolved = field_scope.resolve(self.trie, name).ok();
        let field = resolved.as_ref().map(|r| r.identity);
        let index = field.and_then(|f| items.iter().position(|&x| x == f));
        let Some(index) = index else {
            // A `shared` member is one place read through every node, so
            // `this.element_type` is the member itself, known here.
            let mut members = ScopeStack::new();
            members.push(body);
            if let Ok(r) = members.resolve(self.trie, name) {
                return Ok(r.identity);
            }
            self.pos = at;
            return Err(ParseError::ThisFieldUnknown(Box::new(name.to_string())));
        };
        let k = self.scalar_value(crate::identities::numtype::NumType::U64, index as i64);
        let types = self.types;
        // SAFETY: the field is a declaration dyad, its type null or a type node.
        let declared = unsafe { (*items[index]).ty };
        // SAFETY: as above.
        let data = !declared.is_null()
            && matches!(
                unsafe { crate::identities::read::place_layout(types, declared) },
                Some((
                    crate::identities::read::Read::Scalar(_)
                        | crate::identities::read::Read::Pointer(_),
                    _
                ))
            );
        let node = if data {
            crate::identities::this::build_load(self.rt.store, types, this, k, declared)
        } else {
            crate::identities::this::build_slot(self.rt.store, types, this, k)
        };
        if let Some(r) = resolved {
            self.fills.insert(node, r.binding);
        }
        Ok(node)
    }

    /// The field's parameter place, the frame place the node's operand is
    /// evaluated into (DESIGN ›Execution is function application‹), or for a
    /// field of type `type` the type this set holds in it, so `this.output 1` reads `i32 1`.
    fn run_body_field(&mut self, name: &str, at: usize) -> Result<DyadPtr, ParseError> {
        let (field_scope, fields, places, key) = {
            let rb = self.run_body.as_ref().expect("this_param is set while a run body is built");
            (rb.field_scope, rb.fields.clone(), rb.places.clone(), rb.key.clone())
        };
        let mut scope = ScopeStack::new();
        scope.push(field_scope);
        let index = scope
            .resolve(self.trie, name)
            .ok()
            .and_then(|r| fields.iter().position(|&f| f == r.identity));
        let Some(i) = index else {
            self.pos = at;
            return Err(ParseError::ThisFieldUnknown(Box::new(name.to_string())));
        };
        // SAFETY: the fields are the fields block's declaration dyads.
        let is_type = unsafe { (*fields[i]).ty } == self.types.type_;
        Ok(if is_type { key[i] } else { places[i] })
    }

    /// The run of a node whose type holds its `run` as a lexed body (DESIGN
    /// ›Deferral is authored‹): the function for its field-type set, found on
    /// the type or built now; a field whose type is unknown leaves the slot empty.
    ///
    /// # Safety
    /// `node` must be the node `run_logos_ctor` minted for a type whose
    /// record carries a run body, `[field…, null, spec]`.
    pub(crate) unsafe fn resolve_specialization(
        &mut self,
        node: DyadPtr,
        at: usize,
        spelling: &str,
    ) -> Result<Option<DyadPtr>, ParseError> {
        let ty = (*node).ty;
        let held = crate::identities::meta::run_body_of(ty);
        if held.is_null() {
            return Ok(None);
        }
        let Some(key) = self.field_type_key(ty, node)? else {
            return Ok(None);
        };
        let mut spec = crate::identities::run_body::lookup(held, &key);
        if spec.is_null() {
            spec = self.construct_run_body(ty, held, &key, at, spelling)?;
        }
        crate::identities::run_body::set_spec(node, spec);
        // The node is a call of the function: a use of every outer name the
        // body reads, checked before a comptime node runs it.
        if let Err(e) = self.check_call_reads(spec) {
            self.pos = at;
            return Err(e);
        }
        self.fold_comptime_node(ty, node, spec)
    }

    /// A node every value field of which is a literal is comptime, its
    /// constructor a partial evaluator (DESIGN ›Deferral is authored‹): it runs
    /// now and its literal stands in its place, so the result molds where it lands.
    ///
    /// # Safety
    /// As `resolve_specialization`; `spec` the node's function.
    unsafe fn fold_comptime_node(
        &mut self,
        ty: DyadPtr,
        node: DyadPtr,
        spec: DyadPtr,
    ) -> Result<Option<DyadPtr>, ParseError> {
        use crate::identities::read::{read_kind, Read};
        let types = self.types;
        let fields = crate::identities::array::items(crate::identities::meta::record_fields_of(ty));
        let slots = (*node).value as *const DyadPtr;
        for (i, &field) in fields.iter().enumerate() {
            if (*field).ty == types.type_ {
                continue;
            }
            let slot = types.through(*slots.add(i));
            let comptime = match read_kind(types, slot) {
                Read::Literal => true,
                Read::Scalar(_) => !crate::dyad::is_place((*slot).value),
                Read::Address => {
                    let viewed = (*slot).value as DyadPtr;
                    !viewed.is_null() && (*viewed).ty == types.rational
                }
                _ => false,
            };
            if !comptime {
                return Ok(None);
            }
        }
        let fields = (*spec).value as *const DyadPtr;
        if fields.is_null() {
            return Ok(None);
        }
        let out = *fields.add(FN_OUTPUT);
        let numeric = crate::identities::is_numtype_node(types, out);
        if !numeric && out != types.rational {
            return Ok(None);
        }
        let bits = self
            .run_on_pass(node)
            .map_err(|e| ParseError::ConstructorFailed(Box::new(crate::report::run_message(&e))))?;
        let folded = if numeric {
            self.scalar_value(crate::identities::numtype::of_type_node(out), bits)
        } else {
            bits as DyadPtr
        };
        self.unfolded.insert(folded, node);
        Ok(Some(folded))
    }

    /// One type per instance field in order (DESIGN ›Execution is function
    /// application‹): a typed field's type, a `type` field's held type, or for
    /// an untyped hole the written value's type. `None` while one is unknown.
    ///
    /// # Safety
    /// As `resolve_specialization`; `ty` the node's type.
    unsafe fn field_type_key(
        &mut self,
        ty: DyadPtr,
        node: DyadPtr,
    ) -> Result<Option<Vec<DyadPtr>>, ParseError> {
        use crate::identities::{numtype_of, Operand};
        let types = self.types;
        let fields = crate::identities::array::items(crate::identities::meta::record_fields_of(ty));
        let slots = (*node).value as *mut DyadPtr;
        let mut key = Vec::with_capacity(fields.len());
        for (i, &field) in fields.iter().enumerate() {
            let declared = (*field).ty;
            let slot = *slots.add(i);
            if slot.is_null() {
                return Ok(None);
            }
            let entry = if declared == types.type_ {
                let held = types.through(slot);
                if !crate::identities::is_type_value(types, held) {
                    return Ok(None);
                }
                held
            } else if !declared.is_null() {
                if matches!(numtype_of(types, slot), Operand::Literal) {
                    let lit = types.through(slot);
                    if declared == types.rational {
                        *slots.add(i) =
                            crate::identities::rational::box_literal(self.rt.store, types, lit);
                    } else if crate::identities::is_numtype_node(types, declared) {
                        *slots.add(i) = crate::identities::commit_literal_to(
                            self.rt.store,
                            types,
                            lit,
                            declared,
                        )?;
                    }
                }
                declared
            } else if crate::identities::rational::is_rational_value(types, slot) {
                types.rational
            } else {
                match numtype_of(types, slot) {
                    Operand::Concrete(nt) => types.numtypes[nt as usize],
                    Operand::Literal => {
                        // The literal's own type, boxed as the value the call passes.
                        let lit = types.through(slot);
                        *slots.add(i) =
                            crate::identities::rational::box_literal(self.rt.store, types, lit);
                        types.rational
                    }
                    Operand::Pointer(_) | Operand::NonNumeric => return Ok(None),
                }
            };
            key.push(entry);
        }
        Ok(Some(key))
    }

    /// The function a held run body is for one field-type set: the cells lexed
    /// at the definition are constructed now, over the type's own scopes and one
    /// unnamed parameter per field. Entered on the type first, so a recursive use resolves to it.
    ///
    /// # Safety
    /// `ty` must carry a record whose run body is `held`; `key` one type per
    /// instance field.
    unsafe fn construct_run_body(
        &mut self,
        ty: DyadPtr,
        held: DyadPtr,
        key: &[DyadPtr],
        at: usize,
        spelling: &str,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let text = crate::identities::run_body::text_of(held);
        let mut cells = (*crate::identities::run_body::cells_of(held)).cells();
        // A fresh cell's dyad is the placeholder a `:=` in the body fills, so
        // each construction gets its own and the held cells stay as lexed.
        for cell in &mut cells {
            if cell.is_fresh() {
                cell.dyad = self.rt.store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
            }
        }
        let mut chain = Vec::new();
        let mut scope =
            crate::identities::scope::parent_of(crate::identities::meta::record_scope_of(ty));
        while !scope.is_null() {
            chain.push(scope);
            scope = crate::identities::scope::parent_of(scope);
        }
        let mut nested = ScopeStack::new();
        for &scope in chain.iter().rev() {
            nested.push(scope);
        }
        let base_depth = nested.depth();
        let spec = self.rt.store.alloc_raw(types.fn_type, std::ptr::null_mut());
        crate::identities::run_body::insert(self.rt.store, types, held, key, spec);

        let saved_source = std::mem::replace(&mut self.source, text);
        let saved_pos = std::mem::replace(&mut self.pos, 0);
        let saved_scopes = std::mem::replace(&mut self.scopes, nested);
        let saved_frames = std::mem::take(&mut self.frames);
        let saved_definitions = std::mem::take(&mut self.definitions);
        let saved_pending_fn = std::mem::replace(&mut self.pending_fn, std::ptr::null_mut());
        let saved_runtime_depth = std::mem::replace(&mut self.runtime_depth, 0);
        let saved_lex_mode = self.lex_mode.take();
        let saved_else_ends = self.else_ends.take();
        let saved_lifted = std::mem::take(&mut self.lifted);
        let saved_queued = std::mem::take(&mut self.queued);
        let saved_discovering = std::mem::replace(&mut self.discovering, false);
        let saved_run_body = self.run_body.take();
        let saved_feed =
            self.feed.replace(Feed { cells, next: 0, base_depth, in_run_order: false });

        let inner = self.construct_run_body_in(ty, key);
        let inner_pos = self.pos;

        self.source = saved_source;
        self.pos = saved_pos;
        self.scopes = saved_scopes;
        self.frames = saved_frames;
        self.definitions = saved_definitions;
        self.pending_fn = saved_pending_fn;
        self.runtime_depth = saved_runtime_depth;
        self.lex_mode = saved_lex_mode;
        self.else_ends = saved_else_ends;
        self.lifted = saved_lifted;
        self.queued = saved_queued;
        self.discovering = saved_discovering;
        self.run_body = saved_run_body;
        self.feed = saved_feed;

        match inner {
            Ok(f) => {
                (*spec).value = (*f).value;
                Ok(spec)
            }
            Err(e) => {
                crate::identities::run_body::remove(self.rt.store, types, held, key);
                self.pos = at;
                Err(ParseError::RunBodyFailed {
                    name: spelling.to_string(),
                    rendered: crate::report::render(
                        &format!("run body of `{spelling}`"),
                        text,
                        inner_pos,
                        &crate::report::parse_message(&e),
                    ),
                })
            }
        }
    }

    /// Over the swapped-in state: the hidden parameter record, the return
    /// type (the type the set holds in the field named `output`, or `void`),
    /// and the body read as a function's.
    ///
    /// # Safety
    /// As `construct_run_body`.
    unsafe fn construct_run_body_in(
        &mut self,
        ty: DyadPtr,
        key: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let field_scope = crate::identities::meta::record_scope_of(ty);
        let fields =
            crate::identities::array::items(crate::identities::meta::record_fields_of(ty)).to_vec();
        // The parameters are the fields alone, in order, unnamed: a node's
        // operands are its fields, and its call passes exactly those.
        let mut params: Vec<(Option<&str>, DyadPtr)> = Vec::with_capacity(fields.len());
        for (i, &field) in fields.iter().enumerate() {
            let ty = if (*field).ty == types.type_ { types.type_ } else { key[i] };
            params.push((None, ty));
        }
        let mut scope = ScopeStack::new();
        scope.push(field_scope);
        let output = scope
            .resolve(self.trie, "output")
            .ok()
            .and_then(|r| fields.iter().position(|&f| f == r.identity))
            .filter(|&i| (*fields[i]).ty == types.type_)
            .map_or(types.void_, |i| key[i]);
        let (input, places) = self.hidden_param_record(&params, 0)?;
        // `this` is a name of the body, not a parameter: a marker the `.`
        // constructor recognizes, declared in the parameter scope the body reopens.
        let this = self.rt.store.alloc_raw(types.dyad_, std::ptr::null_mut());
        let param_scope = crate::identities::meta::record_scope_of(input);
        self.scopes.push(param_scope);
        let declared = self.declare_name("this", this, 0);
        self.scopes.pop();
        declared?;
        self.run_body =
            Some(RunBodyCx { this_param: this, field_scope, fields, places, key: key.to_vec() });
        self.fn_over_body(types.fn_type, input, output, std::ptr::null_mut())
    }

    /// `(start, len)` of the text inside the `( … )` at the cursor, consumed
    /// with its brackets and constructed by nothing; the closer is found by
    /// lexing token by token, so a bracket inside a quote or a `#` comment is text.
    fn body_text_extent(&mut self) -> Result<(usize, usize), ParseError> {
        self.expect_open()?;
        let source = self.source;
        let bytes = source.as_bytes();
        let start = self.pos;
        let mut depth = 0usize;
        loop {
            self.skip_whitespace();
            if self.pos >= bytes.len() {
                return Err(ParseError::UnclosedBracket);
            }
            if bytes[self.pos] == b'#' {
                self.pos += 1;
                while self.pos < bytes.len() && matches!(bytes[self.pos], b' ' | b'\t') {
                    self.pos += 1;
                }
                // The line form ends at the newline; the string form, `# «…»`,
                // is the quote token the lexer reads next.
                if !source[self.pos..].starts_with('«') {
                    while self.pos < bytes.len() && bytes[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                    continue;
                }
            }
            let r = self.scopes.lex(self.trie, &source[self.pos..]).map_err(ParseError::Resolve)?;
            let id = if r.fresh { std::ptr::null_mut() } else { r.identity };
            if id == self.types.open_ || id == self.types.open_sq_ {
                depth += 1;
            } else if id == self.types.close_ || id == self.types.close_sq_ {
                if depth == 0 {
                    let len = self.pos - start;
                    self.pos += r.matched;
                    return Ok((start, len));
                }
                depth -= 1;
            }
            self.pos += r.matched;
        }
    }

    /// `fn ( params ) -> ret ( body )` (DESIGN ›A function's surface‹): the
    /// node is `[input, output, body, bcode, frame, outer]`. `declared` is the
    /// declaration's placeholder the signature publishes onto before the body parses.
    ///
    /// # Safety
    /// `declared` must be null or a placeholder dyad from the store that
    /// nothing has read a value from yet; the early signature is written through it.
    pub unsafe fn parse_fn(
        &mut self,
        fn_type: DyadPtr,
        declared: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        // A body in the fields block reached through an instance binds `this`
        // to it (DESIGN ›The constructor is a field‹): the member's `fn` takes it first.
        let member = !declared.is_null()
            && self.member_fn_depth == Some(self.frames.len())
            && self.definitions.last().is_some_and(|d| d.this_param.is_null());
        let input = self.parse_record_taking(member.then_some(("this", self.types.dyad_)))?;
        self.expect_arrow()?;
        let output = {
            let items = self.drive_until_open(RightSide::ReturnType)?;
            let out = self.one_of(items).map_err(|e| match e {
                ParseError::Empty => ParseError::ExpectedReturnType,
                e => e,
            })?;
            // A named return type is a use of that name: its binding, read
            // through to the type it names.
            // SAFETY: `out` is a reduced dyad from the store.
            unsafe { self.types.through(out) }
        };
        if !member {
            // SAFETY: `input` was just built by `parse_record`; `declared` is the caller's placeholder.
            return unsafe { self.fn_over_body(fn_type, input, output, declared) };
        }
        // SAFETY: `input` is the record just built, `this` its first field.
        let this = unsafe {
            crate::identities::array::items(crate::identities::meta::record_fields_of(input))[0]
        };
        self.definitions.last_mut().expect("checked above").this_param = this;
        // SAFETY: as above.
        let f = unsafe { self.fn_over_body(fn_type, input, output, declared) };
        if let Some(def) = self.definitions.last_mut() {
            def.this_param = std::ptr::null_mut();
        }
        f
    }

    /// The half of `parse_fn` after the signature, shared with a slot body
    /// read bare: the frame opened and the parameters placed in it, the body
    /// parsed deferred with the parameter scope reopened.
    ///
    /// # Safety
    /// `input` must be a record node whose parameters' value slots are still
    /// null; `declared` as for `parse_fn`.
    unsafe fn fn_over_body(
        &mut self,
        fn_type: DyadPtr,
        input: DyadPtr,
        output: DyadPtr,
        declared: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        // A call frame is an instance of its function, so a parameter resolves
        // to a frame slot as a local does (DESIGN ›Resolution is one rule‹). The
        // barrier begins at the current depth, so a lesser depth is outside the function.
        self.frames.push(OpenFn {
            size: 0,
            below: self.scopes.depth(),
            outer: Vec::new(),
            open_below: self.open.len(),
            returns: Vec::new(),
        });
        let depth = self.frames.len();
        // SAFETY: `input` is the record just built; each parameter's value slot is still the null `parse_record` left there.
        unsafe {
            let fields = crate::identities::meta::record_fields_of(input);
            for &param in crate::identities::array::items(fields) {
                let logos = (*param).ty;
                // A parameter's slot is sized by the rule that sizes a local; a
                // bare one holds the 8-byte container. A record parameter gets
                // its layout's width though the call binds only the container (stand-in for #115).
                let width = if logos.is_null() {
                    8
                } else {
                    crate::identities::read::place_layout(self.types, logos)
                        .map(|(_, w)| w)
                        .unwrap_or(8)
                };
                let frame = self.frames.last_mut().expect("parse_fn just pushed a frame");
                let offset = frame.size;
                frame.size += width;
                (*param).value = crate::dyad::frame_place(depth, offset);
            }
        }

        if !declared.is_null() {
            let early = self.rt.store.alloc_operands(&[
                input,
                output,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ]);
            // SAFETY: `declared` is the just-declared placeholder; nothing has read it, and the fixpoint overwrites it.
            unsafe {
                (*declared).value = early;
            }
        }

        // The body is deferred (it runs at calls), so parse-time rebinding is
        // off inside.
        // SAFETY: `input` is the record just built; its record stores its scope.
        let scope = unsafe { crate::identities::meta::record_scope_of(input) };
        // Names declared outside the body may not be moved or dropped inside;
        // the parameters, in the scope pushed next, may (DESIGN ›A function's surface‹).
        self.scopes.push_barrier();
        self.scopes.push(scope);
        self.runtime_depth += 1;
        self.expect_open()?;
        let body = self.parse_sequence()?;
        self.expect_close()?;
        self.runtime_depth -= 1;
        self.scopes.pop();
        self.scopes.pop_barrier();
        // SAFETY: `body` is the reduced dyad just parsed.
        if unsafe { crate::identities::drop_model::is_owning_value(self.types, body) } {
            return Err(ParseError::OwnershipAcrossReturn);
        }
        let OpenFn { size: frame_size, outer, returns, .. } =
            self.frames.pop().expect("parse_fn pushed a frame");

        // A comptime-rational tail commits to the declared return type here,
        // so `fn () -> i64 (…)` returns i64 rather than the i32 default.
        // SAFETY: `body`/`output` are valid dyads just built.
        let body =
            unsafe { crate::identities::commit_fn_body(self.rt.store, self.types, body, output)? };
        for r in returns {
            // SAFETY: `r` is a `return` node built in this body.
            unsafe { crate::identities::commit_fn_body(self.rt.store, self.types, r, output)? };
        }

        let frame = if frame_size == 0 {
            std::ptr::null_mut()
        } else {
            let bytes = self.rt.store.alloc_bytes(&(frame_size as u64).to_ne_bytes());
            let u64_ty = self.types.numtypes[crate::identities::NumType::U64 as usize];
            self.rt.store.alloc_raw(u64_ty, bytes)
        };
        let outer = if outer.is_empty() {
            std::ptr::null_mut()
        } else {
            crate::identities::array::build(self.rt.store, self.types.array_, &outer)
        };
        let value = self.rt.store.alloc_operands(&[
            input,
            output,
            body,
            std::ptr::null_mut(),
            frame,
            outer,
        ]);
        Ok(self.rt.store.alloc_raw(fn_type, value))
    }

    /// A `return` just built: inside a function it leaves every scope open
    /// since the body began, so it may not hand out a place their teardowns
    /// free, nor an owning value; the body's close commits it to the result type.
    ///
    /// # Safety
    /// `node` must be a `return` node `[value, op]` from the store.
    pub(crate) unsafe fn note_return(&mut self, node: DyadPtr) -> Result<(), ParseError> {
        let Some(frame) = self.frames.last() else {
            return Ok(());
        };
        let types = self.types;
        let value = *((*node).value as *const DyadPtr);
        let place = types.through(value);
        if self.open[frame.open_below..].iter().any(|s| s.owned.contains(&place)) {
            return Err(ParseError::OwningEscape);
        }
        if crate::identities::drop_model::is_owning_value(types, value) {
            return Err(ParseError::OwnershipAcrossReturn);
        }
        self.frames.last_mut().expect("checked above").returns.push(node);
        Ok(())
    }

    /// `if cond then` with an optional `else else`: the node is
    /// `[cond, then, else]`, else null when absent, so an else-less `if` is a
    /// statement. Each branch is a bracket or the next expression, and a bare
    /// `else` binds to the nearest `if`; `if` opens no scope.
    pub fn parse_if(&mut self, if_type: DyadPtr) -> Result<DyadPtr, ParseError> {
        let items = self.drive_until_open(RightSide::Condition)?;
        let cond = self.one_of(items)?;
        let types = self.types;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(types, cond) } {
            return Err(ParseError::NonBoolCondition);
        }

        // A comptime condition resolves the conditional now, in the one pass:
        // an untaken bracket's tokens are dropped unlexed, so nothing inside
        // is resolved, committed or declared, and branches for other comptime types coexist.
        // SAFETY: `cond` is the reduced dyad just parsed.
        if let Some(truth) = unsafe { bool_literal_value(types, cond) } {
            return self.parse_comptime_if(if_type, cond, truth);
        }

        // DESIGN ›A type is a comptime value, resolved in the pass‹, a checked tape cell.
        // SAFETY: `cond` is the reduced dyad just parsed.
        let check = unsafe { type_check_of(types, cond) };

        // A runtime branch may or may not run, so parse-time rebinding is off
        // inside it.
        self.runtime_depth += 1;
        self.narrow_next = check.filter(|&(_, eq)| eq).map(|(key, _)| key);
        let then = self.parse_branch();
        self.narrow_next = None;
        let then = then?;

        // `else if …` is sugar: the `if` right after `else` becomes the
        // else-branch directly, so a chain nests right-associatively.
        let els = if self.consume_else() {
            if self.consume_token(self.types.if_) {
                self.parse_if(if_type)?
            } else {
                self.narrow_next = check.filter(|&(_, eq)| !eq).map(|(key, _)| key);
                self.parse_branch()?
            }
        } else {
            std::ptr::null_mut()
        };
        self.narrow_next = None;
        self.runtime_depth -= 1;

        // A `!=` check whose branch raises leaves the cell checked for the rest of the scope.
        if let Some((key, false)) = check {
            // SAFETY: `then` is the reduced dyad just parsed.
            if els.is_null() && unsafe { (*types.through(then)).ty } == types.error.error {
                self.open.last_mut().expect("the `if` stands in an open scope").narrowed.push(key);
            }
        }

        let value = self.rt.store.alloc_operands(&[cond, then, els, self.types.ops.if_]);
        Ok(self.rt.store.alloc_raw(if_type, value))
    }

    /// A branch of `if`, or a `while` body: a bracket, or the next expression,
    /// which ends before an `else` (DESIGN ›Expressions are self-delimiting‹).
    fn parse_branch(&mut self) -> Result<DyadPtr, ParseError> {
        if self.at_open() {
            self.expect_open()?;
            let body = self.parse_sequence()?;
            self.expect_close()?;
            return Ok(body);
        }
        // A bare branch is a scope as a bracket is, holding one expression.
        self.skip_whitespace();
        let start = self.pos;
        let was = self.else_ends.replace(self.open.len() + 1);
        let body = self.parse_sequence();
        self.else_ends = was;
        if self.pos == start {
            return Err(ParseError::Empty);
        }
        body
    }

    /// An untaken branch: a bracket is dropped unlexed; a bare one is parsed
    /// and dropped, since only constructing it finds where it ends.
    fn skip_branch(&mut self) -> Result<(), ParseError> {
        if self.at_open() {
            return self.skip_group();
        }
        self.runtime_depth += 1;
        let dead = self.parse_branch();
        self.runtime_depth -= 1;
        dead.map(|_| ())
    }

    /// True with an else: the then-branch is the result and the else-tail is
    /// dropped. False: the then-branch is dropped. An else-less `if` stays a
    /// statement node either way, since it yields unit.
    fn parse_comptime_if(
        &mut self,
        if_type: DyadPtr,
        cond: DyadPtr,
        truth: bool,
    ) -> Result<DyadPtr, ParseError> {
        if truth {
            let then = self.parse_branch()?;
            if self.consume_else() {
                self.skip_else_tail(if_type)?;
                return Ok(then);
            }
            let value = self.rt.store.alloc_operands(&[
                cond,
                then,
                std::ptr::null_mut(),
                self.types.ops.if_,
            ]);
            return Ok(self.rt.store.alloc_raw(if_type, value));
        }
        self.skip_branch()?;
        if self.consume_else() {
            if self.consume_token(self.types.if_) {
                return self.parse_if(if_type);
            }
            return self.parse_branch();
        }
        let value =
            self.rt.store.alloc_operands(&[cond, cond, std::ptr::null_mut(), self.types.ops.if_]);
        Ok(self.rt.store.alloc_raw(if_type, value))
    }

    /// Drop a balanced `( … )` group without parsing it (DESIGN ›a constructor
    /// may splice tokens in or drop upcoming ones before they lex‹): `«…»` text
    /// and `#` prose are skipped opaquely, their parentheses being text.
    fn skip_group(&mut self) -> Result<(), ParseError> {
        /// `pos` must point at `«`; `None` if unterminated.
        fn skip_text(bytes: &[u8], mut pos: usize) -> Option<usize> {
            pos += 2; // the «
            while pos + 1 < bytes.len() {
                if bytes[pos] == 0xC2 && bytes[pos + 1] == 0xBB {
                    return Some(pos + 2);
                }
                pos += 1;
            }
            None
        }
        self.expect_open()?;
        let bytes = self.source.as_bytes();
        let mut depth = 1usize;
        while self.pos < bytes.len() {
            match bytes[self.pos] {
                b'(' => {
                    depth += 1;
                    self.pos += 1;
                }
                b')' => {
                    depth -= 1;
                    self.pos += 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                0xC2 if bytes.get(self.pos + 1) == Some(&0xAB) => {
                    self.pos = skip_text(bytes, self.pos).ok_or(ParseError::UnclosedBracket)?;
                }
                b'#' => {
                    // `#` takes a following «…» string or the rest of the line,
                    // as the comment constructor reads it.
                    self.pos += 1;
                    while self.pos < bytes.len() && matches!(bytes[self.pos], b' ' | b'\t') {
                        self.pos += 1;
                    }
                    if bytes.get(self.pos) == Some(&0xC2) && bytes.get(self.pos + 1) == Some(&0xAB)
                    {
                        self.pos = skip_text(bytes, self.pos).ok_or(ParseError::UnclosedBracket)?;
                    } else {
                        while self.pos < bytes.len() && bytes[self.pos] != b'\n' {
                            self.pos += 1;
                        }
                    }
                }
                _ => self.pos += 1,
            }
        }
        Err(ParseError::UnclosedBracket)
    }

    /// Drop the dead tail after a taken `else`: `if ( cond ) then` links
    /// while further `else`s follow, or the final branch. A bare condition's
    /// end is found only by constructing it, so that link is parsed and dropped.
    fn skip_else_tail(&mut self, if_type: DyadPtr) -> Result<(), ParseError> {
        loop {
            if !self.consume_token(self.types.if_) {
                return self.skip_branch();
            }
            if !self.at_open() {
                self.runtime_depth += 1;
                let dead = self.parse_if(if_type);
                self.runtime_depth -= 1;
                return dead.map(|_| ());
            }
            self.skip_group()?; // ( cond )
            self.skip_branch()?;
            if !self.consume_else() {
                return Ok(());
            }
        }
    }

    /// The node is `{type: not, value: operand}`.
    ///
    /// # Safety
    /// `operand` must be a reduced dyad from the store.
    pub(crate) unsafe fn build_not(
        &mut self,
        not_id: DyadPtr,
        operand: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        if !is_bool_result(types, operand) {
            return Err(ParseError::NonBoolOperands);
        }
        // A bool-literal operand folds now (pure, nothing lost): what keeps a
        // comptime chain comptime.
        if let Some(v) = bool_literal_value(types, operand) {
            return Ok(crate::identities::bool_mod::literal_node(
                self.rt.store,
                self.types.bool_,
                !v,
            ));
        }
        let value = self.rt.store.alloc_operands(&[operand, self.types.ops.not_]);
        Ok(self.rt.store.alloc_raw(not_id, value))
    }

    /// `while cond body`: the node is `[cond, body]`, a statement yielding
    /// unit; the condition and body are read as `if`'s are; the body's value is
    /// thrown away (DESIGN ›a loop body's is thrown away‹); a `return` in it
    /// leaves the enclosing function.
    pub fn parse_while(&mut self, while_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        let items = self.drive_until_open(RightSide::Condition)?;
        let cond = self.one_of(items)?;
        let types = self.types;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(types, cond) } {
            return Err(ParseError::NonBoolCondition);
        }
        // A repeated body: parse-time rebinding is off inside, and a name
        // declared outside may not be moved or dropped inside.
        self.runtime_depth += 1;
        self.scopes.push_barrier();
        let body = self.parse_branch();
        self.scopes.pop_barrier();
        self.runtime_depth -= 1;
        let body = body?;
        // SAFETY: `body` is the reduced dyad just parsed.
        if self.frames.is_empty() && unsafe { contains_return(types, body) } {
            return Err(ParseError::EarlyReturn);
        }
        let value = self.rt.store.alloc_operands(&[cond, body, self.types.ops.while_]);
        Ok(self.rt.store.alloc_raw(while_id, value))
    }

    /// `for i in a..b ( body )`, with an optional `..step` and optionally no
    /// index (DESIGN ›The scope's constructor is the driver‹): end-exclusive;
    /// the counter is a fresh block-local either way, so both tiers run one node shape.
    pub fn parse_for(&mut self, for_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        let name = self.loop_name()?;
        // The range, constructed by parse_rank, the `..` cells inert
        // delimiters read by position.
        let parts = self.drive_until_open(RightSide::Range)?;
        let dotdot = self.types.dotdot_;
        let types = self.types;
        // The `..` cells are uses of that identity: its binding, read through.
        // SAFETY: every item `drive_until_open` returned is a dyad from the store.
        let is_dotdot = |d: &DyadPtr| unsafe { types.through(*d) } == dotdot;
        let (start, end, step) = match parts.as_slice() {
            [(s, _), (d, _), (e, _)] if is_dotdot(d) => (*s, *e, None),
            [(s, _), (d, _), (e, _), (d2, _), (st, _)] if is_dotdot(d) && is_dotdot(d2) => {
                (*s, *e, Some(*st))
            }
            _ => return Err(ParseError::ExpectedRange),
        };

        // Concrete types must match, literals commit, all-literals default to i32.
        let types = self.types;
        // SAFETY: `step` is the reduced dyad just parsed.
        let step_was_literal =
            step.is_some_and(|s| unsafe { (*types.through(s)).ty } == types.rational);
        let mut parts = vec![start, end];
        if let Some(s) = step {
            parts.push(s);
        }
        // SAFETY: `parts` are reduced dyads just parsed.
        let logos =
            unsafe { crate::identities::resolve_loop_parts(self.rt.store, types, &mut parts)? };
        let (start, end) = (parts[0], parts[1]);
        let step = parts.get(2).copied().unwrap_or(std::ptr::null_mut());
        if step_was_literal {
            use crate::identities::numtype;
            // SAFETY: `step` is the committed literal just built; `logos` a numtype node.
            let (bits, nt) = unsafe {
                // A marked place here would mean the literal check above let a
                // variable through, and reading its tagged offset as an address is the crash class the reading rule ends.
                use crate::identities::read::{read_kind, Read};
                if !matches!(read_kind(types, step), Read::Scalar(_))
                    || crate::dyad::is_place((*step).value)
                {
                    return Err(ParseError::BadStep);
                }
                (numtype::read_scalar((*step).ty, (*step).value), numtype::of_type_node(logos))
            };
            if numtype::apply_compare(numtype::CmpOp::Gt, nt, bits, 0) == 0 {
                return Err(ParseError::BadStep);
            }
        }

        // SAFETY: `logos` is a numtype node from resolve_loop_parts.
        let width = unsafe { crate::identities::numtype::of_type_node(logos) }.bytes();
        let var = self.alloc_local(logos, width);
        let parent = self.scopes.current().unwrap_or(std::ptr::null_mut());
        let scope = crate::identities::scope::mint(self.rt.store, types.scope, parent);
        // A repeated body: a name declared outside may not be moved or dropped
        // inside; the loop variable, declared in the scope pushed next, is inside.
        self.scopes.push_barrier();
        self.scopes.push(scope);
        if let Some((nstart, nlen)) = name {
            let source = self.source;
            self.declare_name(&source[nstart..nstart + nlen], var, nstart)?;
        }
        // Parse-time rebinding is off inside a repeated body.
        self.runtime_depth += 1;
        self.expect_open()?;
        let body = self.parse_sequence()?;
        self.expect_close()?;
        self.runtime_depth -= 1;
        self.scopes.pop();
        self.scopes.pop_barrier();
        // SAFETY: `body` is the reduced dyad just parsed.
        if self.frames.is_empty() && unsafe { contains_return(types, body) } {
            return Err(ParseError::EarlyReturn);
        }

        let value =
            self.rt.store.alloc_operands(&[var, start, end, step, body, self.types.ops.for_]);
        Ok(self.rt.store.alloc_raw(for_id, value))
    }

    /// A field of a node reached by path, itself reached (DESIGN ›The dyad's
    /// read surface‹: a node's fields get you more nodes): a scope's `dyads`
    /// and `dyads[k]`, an array's `size`, an operator node's operand by its
    /// role. `None` leaves the read to the rules for any `@dyad` value.
    ///
    /// # Safety
    /// `node` must be a node from the store.
    unsafe fn reached_member(
        &mut self,
        node: DyadPtr,
        name: &str,
        index: Option<usize>,
        bracket: bool,
    ) -> Result<Option<(DyadPtr, usize)>, ParseError> {
        use crate::identities::{array, numtype::NumType, scope};
        let types = self.types;
        let ty = (*node).ty;
        if ty == types.scope && name == "dyads" {
            let arr = scope::dyads(self.rt.store, types.array_, node);
            if arr.is_null() {
                return Err(ParseError::BadReflectRead);
            }
            return match index {
                Some(k) => {
                    let item = *array::items(arr).get(k).ok_or(ParseError::BadReflectRead)?;
                    Ok(Some((self.path_operand(item), 1)))
                }
                // An index known only at run: every read here folds at parse.
                None if bracket => Err(ParseError::BadReflectRead),
                None => Ok(Some((self.reach(arr), 0))),
            };
        }
        if ty == types.array_ && name == "size" {
            let size = array::items(node).len() as i64;
            return Ok(Some((self.scalar_value(NumType::U64, size), 0)));
        }
        Ok(self.node_field(node, name)?.map(|n| (n, 0)))
    }

    /// Field `name` of a node, as [`Parser::path_operand`] yields it: an
    /// operator node's operand by its role, a run type's field in declaration
    /// order. `None` when the node's type defines no such field.
    ///
    /// # Safety
    /// `node` must be a node from the store.
    unsafe fn node_field(
        &mut self,
        node: DyadPtr,
        name: &str,
    ) -> Result<Option<DyadPtr>, ParseError> {
        use crate::identities::{array, meta};
        let ty = (*node).ty;
        let value = (*node).value;
        if ty.is_null() || value.is_null() || crate::dyad::is_place(value) {
            return Ok(None);
        }
        let slot = match meta::kind_of(ty) {
            Some(meta::TUPLE_TAG | meta::LIST_TAG) => (0..meta::arity_of(ty))
                .find(|&i| crate::reflect::text_of(meta::role_of(ty, i)) == name.as_bytes()),
            // `[field…, null, spec]`: the fields are the nodes the constructor filled.
            Some(meta::RECORD_TAG) if !meta::run_body_of(ty).is_null() => {
                let mut fields = ScopeStack::new();
                fields.push(meta::record_scope_of(ty));
                fields.resolve(self.trie, name).ok().and_then(|r| {
                    array::items(meta::record_fields_of(ty)).iter().position(|&f| f == r.identity)
                })
            }
            _ => None,
        };
        let Some(i) = slot else {
            return Ok(None);
        };
        let field = *(value as *const DyadPtr).add(i);
        if field.is_null() {
            return Err(ParseError::BadReflectRead);
        }
        Ok(Some(self.path_operand(field)))
    }

    /// A node reached by path, as a value: itself when reading it runs
    /// nothing (a name, a literal, a place), else its address, so reading a
    /// path never runs code.
    ///
    /// # Safety
    /// `node` must be a node from the store.
    unsafe fn path_operand(&mut self, node: DyadPtr) -> DyadPtr {
        use crate::identities::read::{read_kind, Read};
        let types = self.types;
        let runs = (*node).ty != types.binding_
            && ((*node).ty == types.scope || matches!(read_kind(types, node), Read::Executable(_)));
        if runs {
            self.reach(node)
        } else {
            node
        }
    }

    /// The cell after `for` decides: a spelling followed by `in` is the name;
    /// a fresh spelling not followed by `in` wanted it, since nothing declared
    /// lexes there; anything else begins the range, and the cursor goes back.
    fn loop_name(&mut self) -> Result<Option<(usize, usize)>, ParseError> {
        let at = self.pos;
        let Some((nstart, nlen, fresh)) = self.lex_spelling_fresh() else {
            return Ok(None);
        };
        if self.consume_token(self.types.in_) {
            return Ok(Some((nstart, nlen)));
        }
        if fresh {
            return Err(ParseError::ExpectedIn);
        }
        self.pos = at;
        Ok(None)
    }

    /// `lhs.name` to a place: a numeric node over the instance's storage at
    /// the field's byte offset (DESIGN ›Resolution is one rule‹). The field
    /// name resolves in the record type's own scope alone.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    pub(crate) unsafe fn field_access(
        &mut self,
        lhs: DyadPtr,
        nstart: usize,
        nlen: usize,
        index: Option<usize>,
        key: Option<DyadPtr>,
        call: Option<Vec<DyadPtr>>,
    ) -> Result<(DyadPtr, usize), ParseError> {
        let unit_call = matches!(call, Some(ref a) if a.is_empty());
        // The member name is read as its raw spelling: a field is dot-only,
        // so what the driver resolved it to is beside the point. `index` and
        // `unit_call` are the reads that take one more cell; the count consumed is returned.
        {
            // `source` is `&'a str` (Copy), independent of the `&mut self` the
            // member reads then need.
            let source = self.source;
            let name = &source[nstart..nstart + nlen];
            // `this.f` inside a parse body: a field of the node being built;
            // before the dyad view below, since `this` is a `dyad ?` place.
            if let Some(def) = self.definitions.last() {
                if !def.this_param.is_null() && lhs == def.this_param {
                    return self.this_field(name, nstart).map(|n| (n, 0));
                }
            }
            // `this.f` inside a held run body being constructed: the field's
            // place in the function being built.
            if self.run_body.as_ref().is_some_and(|rb| lhs == rb.this_param) {
                return self.run_body_field(name, nstart).map(|n| (n, 0));
            }
            if let Some(&logos) = self.fields_views.get(&lhs) {
                return self.fields_member(lhs, logos, name).map(|n| (n, 0));
            }
            // A scope cell's lines, read when the constructor runs.
            {
                use crate::identities::tape;
                let (types, store) = (self.types, &mut *self.rt.store);
                if (*lhs).ty == types.tape.slot && name == "dyads" {
                    return Ok(match key {
                        Some(i) => (tape::build_cell_dyad_at(store, types, lhs, i), 1),
                        None => (tape::build_cell_dyads(store, types, lhs), 0),
                    });
                }
                if (*lhs).ty == types.tape.cell_dyads && name == "size" {
                    return Ok((tape::build_cell_dyads_size(store, types, lhs), 0));
                }
            }
            if let Some(node) = self.known_node(lhs, self.member_root) {
                if let Some(read) = self.reached_member(node, name, index, key.is_some())? {
                    return Ok(read);
                }
            }
            // `(2 ^ 3).lhs`: the node a comptime value was folded from keeps its fields.
            let lhs = self.unfolded.get(&lhs).copied().unwrap_or(lhs);
            if (*lhs).ty == self.types.dyad_ {
                return self.view_member(lhs, name).map(|n| (n, 0));
            }
            // `here.scope`, `caller.scope` (DESIGN ›Meta-navigation‹): `here`
            // knows its scope at the appearance, so the read folds; `caller`
            // reads the pass, so its `.scope` is a node run inside a constructor.
            let h = self.types.here;
            if (*lhs).ty == h.here || (*lhs).ty == h.caller {
                if name != "scope" {
                    return Err(ParseError::BadReflectRead);
                }
                let node = if (*lhs).ty == h.here {
                    self.reach(crate::identities::here::scope_of_here(lhs))
                } else {
                    crate::identities::here::build_caller_scope(self.rt.store, self.types)
                };
                return Ok((node, 0));
            }
            // A scope's link up is `.back`; `.scope` is the scope of a spot, a call or a name.
            if name == "back" && crate::identities::here::yields_scope_address(self.types, lhs) {
                let node = crate::identities::here::build_back(self.rt.store, self.types, lhs);
                return Ok((node, 0));
            }
            if crate::identities::is_type_value(self.types, lhs) {
                let n = self.logos_member(lhs, name, index)?;
                return Ok((n, usize::from(name == "roles")));
            }
            // A place holding a type: its fields are the identity's, which
            // nobody knows until the program runs (DESIGN ›A type is a comptime value‹).
            if matches!(
                crate::identities::read::read_kind(self.types, lhs),
                crate::identities::read::Read::Container(t) if t == self.types.type_ || t == self.types.dyad_
            ) || crate::identities::yields_type(self.types, lhs)
            {
                return Err(ParseError::TypeKnownOnlyAtRun);
            }
            if name == "type" {
                return Err(ParseError::TypeIsColonRead);
            }
            // An operator node's slots are the fields its own type defines:
            // `.operands[i]` fetches one, no view involved; a null slot is the checked error until `?`.
            if name == "operands"
                && !(*lhs).ty.is_null()
                && matches!(
                    crate::identities::meta::kind_of((*lhs).ty),
                    Some(crate::identities::meta::TUPLE_TAG | crate::identities::meta::LIST_TAG)
                )
            {
                let i = index.ok_or(ParseError::ExpectedIndexBracket)?;
                if i >= crate::identities::meta::arity_of((*lhs).ty) || (*lhs).value.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                let ops = (*lhs).value as *const DyadPtr;
                let operand = *ops.add(i);
                if operand.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                return Ok((operand, 1));
            }
            if let Some(field) = self.node_field(lhs, name)? {
                return Ok((field, 0));
            }
            // `.compile` is the fn type's shared member (DESIGN ›Execution is
            // function application‹); the name-compare stands in for resolution
            // through the type's scope. The leaf is minted now, entry zero; the run patches it in.
            if &self.source[nstart..nstart + nlen] == "compile" && (*lhs).ty == self.types.fn_type {
                if !unit_call {
                    return Err(ParseError::ExpectedOpen);
                }
                let code = crate::identities::callable::mint(
                    self.rt.store,
                    self.types.callable_,
                    0,
                    self.types.conv_container,
                );
                let value = self.rt.store.alloc_operands(&[lhs, code, self.types.ops.compile_]);
                return Ok((self.rt.store.alloc_raw(self.types.compile_, value), 1));
            }
        }
        // A tape's natives, members of `parsing_tape`'s scope that are not
        // laid-out fields, are built as a call with the receiver's address first.
        if let Some(recv) = crate::identities::tape::receiver_addr(self.rt.store, self.types, lhs) {
            let name = &self.source[nstart..nstart + nlen];
            if let Some((op, leaf)) = crate::identities::tape::member(&self.types.tape, name) {
                let types = self.types;
                if op == types.tape.is_constructed || op == types.tape.spelling {
                    let k = key.ok_or(ParseError::ExpectedIndexBracket)?;
                    let node = crate::identities::tape::build_member(
                        self.rt.store,
                        types,
                        recv,
                        op,
                        leaf,
                        &[k],
                    )?;
                    return Ok((node, 1));
                }
                let args = call.ok_or(ParseError::ExpectedOpen)?;
                if op == types.tape.insert || op == types.tape.remove || op == types.tape.recenter {
                    self.forget_narrowed();
                }
                let node = crate::identities::tape::build_member(
                    self.rt.store,
                    types,
                    recv,
                    op,
                    leaf,
                    &args,
                )?;
                return Ok((node, 1));
            }
        }
        // Through a record pointer, `p@.x` folds the field offset into the deref.
        if (*lhs).ty == self.types.deref_ {
            let (ptr_expr, pointee, base_off) = crate::identities::pointer::deref_parts(lhs);
            if pointee.is_null() || !crate::identities::meta::is_record_type(pointee) {
                return Err(ParseError::UnsupportedOperands);
            }
            let (field, offset, _) = match self.resolve_field(pointee, nstart, nlen) {
                Ok(found) => found,
                Err(e) => {
                    let name = &self.source[nstart..nstart + nlen];
                    return self.shared_member_read(pointee, name).map(|n| (n, 0)).ok_or(e);
                }
            };
            let types = self.types;
            return Ok((
                crate::identities::pointer::build_deref(
                    self.rt.store,
                    types,
                    ptr_expr,
                    (*field).ty,
                    base_off as usize + offset,
                ),
                0,
            ));
        }
        // The access is a place, its offset folded into the instance's own
        // place now; `wrapping_add` keeps a frame-tagged value a valid tagged offset.
        let record_logos = (*lhs).ty;
        if record_logos.is_null()
            || !crate::identities::meta::is_record_type(record_logos)
            || (*lhs).value.is_null()
        {
            return Err(ParseError::UnsupportedOperands);
        }
        let (field, offset, binding) = match self.resolve_field(record_logos, nstart, nlen) {
            Ok(found) => found,
            Err(e) => {
                let name = &self.source[nstart..nstart + nlen];
                return self.shared_member_read(record_logos, name).map(|n| (n, 0)).ok_or(e);
            }
        };
        let addr = (*lhs).value.wrapping_add(offset);
        let node = self.rt.store.alloc_raw((*field).ty, addr);
        // Every binding along the path grants a write into the place: the
        // name the path starts at, then each field.
        let mut path = self.paths.get(&lhs).cloned().unwrap_or_default();
        if path.is_empty() && !self.member_root.is_null() {
            path.push(self.member_root);
        }
        path.push(binding);
        self.paths.insert(node, path);
        Ok((node, 0))
    }

    /// `.`'s constructor: the member read of `tape[-1]` named by the cell to
    /// the right, plus the `[i]` or `()` cell some reads take (see
    /// [`Parser::field_access`]); every consumed cell is spliced out.
    pub(crate) fn construct_field_access(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // The left is read as it stands: an identity's fields are read off the
        // token before its own constructor wakes (DESIGN ›Text is the quote‹),
        // so a callable to the left is the identity itself, not a call in waiting.
        let (lhs, root) = match tape.at(-1).copied() {
            Some(cell) => (self.operand_dyad(cell)?, cell.binding(self.types)),
            None => return Err(ParseError::MissingOperand),
        };
        // The member is read by its spelling at the offset it was lexed at, so a
        // keyword there (`.type`) reads as a name.
        let this_read =
            self.definitions.last().is_some_and(|d| !d.this_param.is_null() && d.this_param == lhs);
        self.member_asleep = this_read;
        let m = self.cell_at(tape, 1);
        self.member_asleep = false;
        let Some(m) = m? else {
            return Err(ParseError::ExpectedField);
        };
        let mstart = m.start;
        let save = self.pos;
        self.pos = mstart;
        let member = self.lex_spelling();
        self.pos = save;
        let Some((nstart, nlen)) = member else {
            return Err(ParseError::ExpectedField);
        };
        // The optional `[i]` or `(…)` after the member, lexed on demand; a
        // bracket is the member's argument list.
        self.cell_at(tape, 2)?;
        let index = self.index_at(tape, 2);
        let bracket = tape.at(2).filter(|c| c.is_bracket()).map(|c| c.dyad);
        // SAFETY: a bracket cell is a node from the store.
        let call = bracket.map(|d| unsafe { self.args_of(d) });
        let key = self.index_node_at(tape, 2);
        self.member_root = root;
        // SAFETY: `lhs` is a reduced dyad off the tape.
        let access = unsafe { self.field_access(lhs, nstart, nlen, index, key, call) };
        self.member_root = std::ptr::null_mut();
        let (node, consumed) = access?;
        for _ in 0..(1 + consumed) {
            tape.remove(1);
        }
        tape.remove(-1);
        // A type identity a body's `this.f` folded to stands as its own
        // unconstructed cell, so it reads its right side as anywhere else.
        // SAFETY: `node` is a node from the store.
        let folded_type = (self.run_body.is_some() || this_read)
            && unsafe {
                (*node).ty == self.types.type_ && crate::identities::meta::kind_of(node).is_some()
            };
        if folded_type {
            tape.set_dyad(0, node);
        } else {
            tape.place(node);
        }
        Ok(Constructed::Placed)
    }

    /// The comptime index a `[…]` cell carries when its interior is a
    /// non-negative literal: what the reflection reads fold at parse.
    fn index_at(&self, tape: &ParsingTape, offset: isize) -> Option<usize> {
        let key = self.index_node_at(tape, offset)?;
        // SAFETY: the interior is a node from the store.
        unsafe {
            if (*key).ty != self.types.rational {
                return None;
            }
            let i = crate::identities::rational::mold(key)?;
            if i < 0 {
                None
            } else {
                Some(i as usize)
            }
        }
    }

    /// The one value a `[…]` cell's `dyads` hold, prose aside: any expression, for the
    /// reads whose natives run it.
    fn index_node_at(&self, tape: &ParsingTape, offset: isize) -> Option<DyadPtr> {
        let c = tape.at(offset)?;
        if !c.constructed {
            return None;
        }
        let d = c.dyad;
        // SAFETY: a dyad cell is a node from the store; a `square_brackets` node's value
        // holds its `dyads` first, as a scope's does.
        unsafe {
            if (*d).ty != self.types.square_brackets {
                return None;
            }
            let defer_ = self.types.defer_;
            let mut values = crate::identities::scope::exprs_of(d)?.iter().copied().filter(|&e| {
                !crate::identities::numtype::is_comment_type((*e).ty) && (*e).ty != defer_
            });
            let key = values.next()?;
            values.next().is_none().then_some(key)
        }
    }

    /// `[` is `(` in square brackets: the interior parsed as any bracket's,
    /// closed by its `]`. It lands a `square_brackets` cell (DESIGN ›The
    /// scope's constructor is the driver‹), or right after a tape value the element read
    /// `t[k]`, folded here since a tape value has no constructor of its own yet.
    pub(crate) fn construct_index(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let (scope, key) = self.parse_block()?;
        self.expect_close_sq()?;
        // An index right after a tape value is the element read, a slot node
        // the identity to its left owns; after a `.` it stays a passive cell.
        if let Some(left) = tape.at(-1).copied() {
            // A fresh spelling to the left is a member name after `.`, never a tape.
            if !left.is_fresh() && self.is_operand_cell(&left) {
                let lhs = self.operand_dyad(left)?;
                let types = self.types;
                // SAFETY: `lhs` is a reduced dyad from the store.
                let recv =
                    unsafe { crate::identities::tape::receiver_addr(self.rt.store, types, lhs) };
                if let Some(recv) = recv {
                    let mut node =
                        crate::identities::tape::build_slot(self.rt.store, types, recv, key);
                    // SAFETY: `node` is the slot node just built.
                    if unsafe { crate::identities::tape::cell_key(types, node) }
                        .is_some_and(|k| self.is_narrowed(k))
                    {
                        // SAFETY: as above.
                        node = unsafe {
                            crate::identities::tape::build_cell_value(self.rt.store, types, node)
                        };
                    }
                    tape.remove(-1);
                    tape.place(node);
                    return Ok(Constructed::Placed);
                }
                // SAFETY: `lhs` and `key` are reduced dyads from the store.
                let get = unsafe {
                    crate::identities::hashmap::build_get(self.rt.store, types, lhs, key)
                }?;
                if let Some(node) = get {
                    tape.remove(-1);
                    tape.place(node);
                    return Ok(Constructed::Placed);
                }
            }
        }
        // SAFETY: `scope` is the block `parse_block` minted, closed.
        let dyads =
            unsafe { crate::identities::scope::dyads(self.rt.store, self.types.array_, scope) };
        let value = self.rt.store.alloc_operands(&[dyads, std::ptr::null_mut()]);
        let node = self.rt.store.alloc_raw(self.types.square_brackets, value);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Against `record_logos`'s own scope alone: an enclosing binding of the
    /// same spelling can never shadow or double a field.
    ///
    /// # Safety
    /// `record_logos` must be a record type node from the store.
    unsafe fn resolve_field(
        &mut self,
        record_logos: DyadPtr,
        nstart: usize,
        nlen: usize,
    ) -> Result<(DyadPtr, usize, DyadPtr), ParseError> {
        let source = self.source;
        let name = &source[nstart..nstart + nlen];
        let mut field_scope = ScopeStack::new();
        field_scope.push(crate::identities::meta::record_scope_of(record_logos));
        let resolved = field_scope.resolve(self.trie, name).map_err(ParseError::Resolve)?;
        let field = resolved.identity;
        let (fields, _) = crate::identities::instance::layout(record_logos)?;
        let (_, _, offset) = fields
            .iter()
            .copied()
            .find(|&(f, _, _)| f == field)
            .ok_or(ParseError::ExpectedField)?;
        Ok((field, offset, resolved.binding))
    }

    /// A function whose declared return type is `type`: it yields a type,
    /// resolved at comptime.
    ///
    /// # Safety
    /// `callee` must be a resolved dyad from the store.
    unsafe fn returns_type(&self, callee: DyadPtr) -> bool {
        if callee.is_null() || (*callee).ty != self.types.fn_type {
            return false;
        }
        let fields = (*callee).value as *const DyadPtr;
        !fields.is_null() && *fields.add(FN_OUTPUT) == self.types.type_
    }

    /// The call runs on the pass and the result bits are the produced type
    /// node's address (DESIGN ›Build and run are one self-directing pass‹); a
    /// run failure or a non-type result is `NonComptimeTypeCall`.
    ///
    /// # Safety
    /// `call` must be a reduced call node from the store.
    unsafe fn eval_type_call(&mut self, call: DyadPtr) -> Result<DyadPtr, ParseError> {
        // What stands before the call runs first, so the call reads committed state.
        self.drain()?;
        let bits = self.run_on_pass(call).map_err(|e| match e {
            crate::run::RunError::MintFailed(_) => ParseError::Run(e),
            _ => ParseError::NonComptimeTypeCall,
        })?;
        let node = bits as usize as DyadPtr;
        // The bits are read as a node address, so they must be one: bits that
        // were never a node are the checked error, never a dereference.
        if !self.rt.store.contains(node) {
            return Err(ParseError::NonComptimeTypeCall);
        }
        if crate::identities::is_type_value(self.types, node) {
            Ok(node)
        } else {
            Err(ParseError::NonComptimeTypeCall)
        }
    }

    /// The lhs's static type must be a pointer type: a pointer variable or
    /// `&x`, a pointer field place, or another deref whose pointee is a pointer.
    ///
    /// # Safety
    /// `lhs` must be a reduced dyad from the store.
    pub(crate) unsafe fn build_deref(&mut self, lhs: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The pointer expression is stored as it stands (a use of a name is
        // its binding); its type is read through the reading rule.
        let read = self.types.through(lhs);
        let ptr_ty = if (*read).ty == self.types.deref_ {
            crate::identities::pointer::deref_parts(read).1
        } else {
            (*read).ty
        };
        let pointee = if ptr_ty == self.types.plus || ptr_ty == self.types.minus {
            // A pointer step, `(p + k)@`: its pointee is the stepped pointer's.
            match crate::identities::numtype_of(self.types, read) {
                crate::identities::Operand::Pointer(pointee) => pointee,
                _ => return Err(ParseError::UnsupportedOperands),
            }
        } else {
            // The pointee rides on the reading rule's answer.
            let Some((crate::identities::read::Read::Pointer(pointee), _)) =
                crate::identities::read::place_layout(self.types, ptr_ty)
            else {
                return Err(ParseError::UnsupportedOperands);
            };
            pointee
        };
        let types = self.types;
        Ok(crate::identities::pointer::build_deref(self.rt.store, types, lhs, pointee, 0))
    }

    /// `&`'s constructor: an `addr` node that resolves the place's address at
    /// run/lower time, so a frame-relative local yields a per-activation
    /// address. A comptime binding has no storage.
    pub(crate) fn construct_address_of(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let Some(&cell) = tape.at(1) else {
            return Err(ParseError::BadAddressOf);
        };
        // Keywords, operators, literals: not places.
        if !cell.constructed
            && !cell.is_fresh()
            && self.ctor_of(self.cell_identity(&cell)).is_some()
        {
            return Err(ParseError::BadAddressOf);
        }
        let node = self.operand_dyad(cell)?;
        // SAFETY: `node` is a resolved dyad from the store.
        let addr = unsafe {
            // A place the reading rule can name, marked as storage: a literal's
            // blob is not one, nor a node of a code-carrying type (an operand run, no fields).
            use crate::identities::read::{read_kind, Read};
            let placed = matches!(
                read_kind(self.types, node),
                Read::Scalar(_) | Read::Pointer(_) | Read::Aggregate
            ) && crate::dyad::is_place((*node).value);
            if !placed {
                return Err(ParseError::BadAddressOf);
            }
            self.check_capture(node)?;
            crate::identities::pointer::build_addr(self.rt.store, self.types, node)
        };
        tape.remove(1);
        tape.place(addr);
        Ok(Constructed::Placed)
    }

    /// The place operand of `own`/`drop`/`free`, the place node itself (not an
    /// `addr`). With `ends_name` a bare name comes back as the `Ended` the
    /// caller hands to `mark_dead`; a name from outside a loop or `fn` body is refused.
    pub(crate) fn place_operand_cell(
        &mut self,
        tape: &mut ParsingTape,
        ends_name: bool,
    ) -> Result<(DyadPtr, Option<Ended>), ParseError> {
        let Some(&cell) = tape.at(1) else {
            return Err(ParseError::MissingOperand);
        };
        let (node, ended) = if cell.constructed {
            (cell.dyad, None)
        } else {
            let r = match self.scopes.resolve(self.trie, cell.spelling()) {
                Ok(r) => r,
                Err(e) => {
                    self.pos = cell.start;
                    return Err(ParseError::Resolve(e));
                }
            };
            if self.ctor_of(r.identity).is_some() {
                return Err(ParseError::MissingOperand);
            }
            let ended = if ends_name {
                if self.scopes.crosses_barrier(r.scope) {
                    self.pos = cell.start;
                    return Err(ParseError::OwnOfOuterName);
                }
                Some(Ended { binding: r.binding })
            } else {
                None
            };
            self.note_outer_read(r.binding);
            (r.identity, ended)
        };
        // SAFETY: `node` is a resolved dyad from the store.
        unsafe {
            self.check_capture(node)?;
        }
        tape.remove(1);
        Ok((node, ended))
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

    /// The six fields, the spelling as a string node (`a:name`).
    /// A gate word marks the declaration that just reduced to its right.
    pub(crate) fn gate_declared(&mut self, gate: DyadPtr) -> Result<(), ParseError> {
        let binding = self.last_declared;
        if binding.is_null() {
            return Err(ParseError::GateNeedsDeclaration);
        }
        self.add_gate(binding, gate)
    }

    /// A write into a place reached by a path is granted by every binding
    /// along it, `immut` vetoing first; the constructor's fill of its fresh
    /// node is granted unless `immut` vetoes it; a place reached no such way passes here.
    pub(crate) fn check_path_write(&self, target: DyadPtr) -> Result<(), ParseError> {
        if let Some(&binding) = self.fills.get(&target) {
            // SAFETY: a fill's binding is a binding dyad from the store.
            if unsafe { Binding::has_gate(binding, self.types.immut_) } {
                // SAFETY: as above.
                return Err(ParseError::Immutable(Box::new(unsafe { Binding::spelling(binding) })));
            }
            return Ok(());
        }
        for &binding in self.paths.get(&target).map_or(&[][..], Vec::as_slice) {
            // SAFETY: the path holds binding dyads from the store.
            unsafe {
                if Binding::has_gate(binding, self.types.immut_) {
                    return Err(ParseError::Immutable(Box::new(Binding::spelling(binding))));
                }
                if !Binding::has_gate(binding, self.types.mut_) {
                    return Err(ParseError::NotMutable(Box::new(Binding::spelling(binding))));
                }
            }
        }
        Ok(())
    }

    /// Add `gate` to a name's binding, once.
    pub(crate) fn add_gate(&mut self, binding: DyadPtr, gate: DyadPtr) -> Result<(), ParseError> {
        // SAFETY: `binding` is a binding dyad from the store.
        unsafe {
            if Binding::has_gate(binding, gate) {
                return Err(ParseError::DoubleGate);
            }
            Binding::add_gate(self.rt.store, self.types.array_, binding, gate);
        }
        Ok(())
    }

    fn mint_binding(&mut self, identity: DyadPtr, scope: DyadPtr, spelling: &[u8]) -> DyadPtr {
        let name =
            crate::identities::string::build_text(self.rt.store, self.types.string_, spelling);
        Binding::alloc(self.rt.store, self.types.binding_, Binding::new(identity, scope, name))
    }

    /// One binding per declared name, a dyad of type `binding` (DESIGN ›`mut` is
    /// a gate on the binding‹); an error is reported at `at`, the name's own
    /// offset, so the caret lands on the name.
    pub(crate) fn declare_name(
        &mut self,
        name: &str,
        identity: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let scope = self.scopes.current().expect("declare needs an open scope");
        let binding = self.mint_binding(identity, scope, name.as_bytes());
        // SAFETY: `binding` was minted by `Binding::alloc` just above.
        if let Err(e) = unsafe { self.scopes.declare(self.trie, name, binding) } {
            self.pos = at;
            return Err(ParseError::Resolve(e));
        }
        Ok(binding)
    }

    /// The pattern twin of `declare_name`.
    pub(crate) fn declare_pattern(
        &mut self,
        key: &str,
        identity: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let scope = self.scopes.current().expect("declare needs an open scope");
        let binding = self.mint_binding(identity, scope, key.as_bytes());
        // SAFETY: `binding` was minted by `Binding::alloc` just above.
        if let Err(e) = unsafe { self.scopes.declare_pattern(self.trie, key, binding) } {
            self.pos = at;
            return Err(ParseError::Resolve(e));
        }
        Ok(binding)
    }

    /// Checked against its siblings alone; a failure is reported at `at`.
    fn declare_field_name(
        &mut self,
        name: &str,
        identity: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let scope = self.scopes.current().expect("declare needs an open scope");
        let binding = self.mint_binding(identity, scope, name.as_bytes());
        // SAFETY: `binding` was minted by `Binding::alloc` just above.
        if let Err(e) = unsafe { self.scopes.declare_field(self.trie, name, binding) } {
            self.pos = at;
            return Err(ParseError::Resolve(e));
        }
        Ok(binding)
    }

    /// `node` has emptied `ended`'s place: its name is dead from here on
    /// (DESIGN ›Memory and concurrency‹).
    pub(crate) fn mark_dead(&mut self, ended: Ended, node: DyadPtr) {
        // SAFETY: `ended.binding` is the binding the resolver returned for the operand.
        unsafe { self.scopes.mark_dead(ended.binding, node) };
    }

    fn consume_else(&mut self) -> bool {
        self.consume_token(self.types.else_)
    }

    fn expect_arrow(&mut self) -> Result<(), ParseError> {
        if self.consume_token(self.types.arrow_) {
            Ok(())
        } else {
            Err(ParseError::ExpectedArrow)
        }
    }

    /// Every further `@` cell to the right, then the base type cell, any type
    /// identity (DESIGN ›Pointer types are prefix `@T`‹), built into the
    /// pointer type. What a place of `@T` reads as never depends on `T`.
    pub(crate) fn construct_pointer_type(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let mut depth = 1usize;
        while matches!(self.cell_at(tape, 1)?, Some(c) if !c.constructed && self.cell_identity(&c) == self.types.at_)
        {
            tape.remove(1);
            depth += 1;
        }
        let Some(cell) = self.cell_at(tape, 1)? else {
            return Err(ParseError::UnsupportedOperands);
        };
        let base = self.operand_dyad(cell)?;
        // SAFETY: `base` is a resolved dyad from the store.
        if !unsafe { crate::identities::is_type_value(self.types, base) } {
            return Err(ParseError::UnsupportedOperands);
        }
        tape.remove(1);
        let mut logos = base;
        for _ in 0..depth {
            // SAFETY: `logos` is the base type node, or the pointer type minted in the previous round.
            logos = unsafe {
                crate::identities::pointer::make_pointer_type(
                    self.rt.store,
                    self.types.type_,
                    logos,
                )
            };
        }
        tape.place(logos);
        Ok(Constructed::Placed)
    }

    /// The callee's own constructor's work (DESIGN ›`X (…)` is one spelling,
    /// and X's constructor decides what the bracket is‹): a numeric callee is
    /// a conversion, a record type constructs an instance, a type-returning callee resolves now.
    ///
    /// # Safety
    /// `callee` and every `arg` must be dyads from the store.
    pub(crate) unsafe fn build_call(
        &mut self,
        callee: DyadPtr,
        args: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        let args = args.to_vec();
        // Fail-closed until ownership-gated parameters let the callee declare
        // that it takes the value.
        for &arg in &args {
            // SAFETY: `args` are reduced dyads just parsed.
            if unsafe { crate::identities::drop_model::is_owning_value(self.types, arg) } {
                return Err(ParseError::UnboundOwningValue);
            }
        }
        // SAFETY: `callee` is a resolved dyad from the store; a record type carries the record `run_body_of` reads.
        let (is_numtype, is_record) = unsafe {
            (
                crate::identities::is_numtype_node(self.types, callee),
                crate::identities::meta::is_record_type(callee),
            )
        };
        if is_numtype {
            // SAFETY: `callee` is a numtype node; `args` are reduced dyads.
            unsafe { crate::identities::build_cast(self.rt.store, self.types, callee, &args) }
        } else if is_record {
            // SAFETY: `callee` is a record type node.
            if unsafe { !crate::identities::meta::run_body_of(callee).is_null() } {
                return Err(ParseError::RunTypeApplied);
            }
            // A record type applied to its field values constructs an
            // instance, like `i32(a)`.
            let types = self.types;
            // SAFETY: `callee` is a record type node; `args` are reduced dyads from the store.
            unsafe {
                // A per-call local sized from the record layout, so a recursive
                // call fills its own copy.
                let (_, size) = crate::identities::instance::layout(callee)?;
                let instance = self.alloc_local(callee, size.max(1));
                crate::identities::instance::build_ctor(
                    self.rt.store,
                    types,
                    types.construct_,
                    callee,
                    instance,
                    &args,
                )
            }
        } else {
            // Each uncommitted literal argument commits to its parameter's
            // declared type; an unbound callee has no signature yet and commits nothing.
            let types = self.types;
            let mut args = args;
            // SAFETY: `callee` and `args` are reduced dyads from the store.
            unsafe {
                crate::identities::commit_call_args(self.rt.store, types, callee, &mut args)?;
            }
            let call = build_call(self.rt.store, callee, &args);
            // A type-returning call resolves now, at comptime, so the result
            // flows as an ordinary type value; the outer-name check runs first,
            // since a body that reads a dead name must not run.
            // SAFETY: `callee` is a reduced dyad.
            if unsafe { self.returns_type(callee) } {
                self.check_call_reads(callee)?;
                // Inside a body an argument known only at run leaves the call
                // to run with it, its result a type value at run.
                let comptime = args.iter().all(|&a| {
                    // SAFETY: `args` are reduced dyads from the store.
                    unsafe { crate::identities::is_comptime_arg(types, a) }
                });
                if self.runtime_depth > 0 && !comptime {
                    return Ok(call);
                }
                // SAFETY: `call` was just built over reduced dyads.
                unsafe { self.eval_type_call(call) }
            } else {
                Ok(call)
            }
        }
    }

    /// Mint a scope node carrying its enclosing scope (DESIGN ›Meta-navigation‹)
    /// and push it; the link is set before anything inside is parsed, so a
    /// constructor running inside walks up over settled structure.
    fn open_scope(&mut self) -> DyadPtr {
        let parent = self.scopes.current().unwrap_or(std::ptr::null_mut());
        let scope = crate::identities::scope::mint(self.rt.store, self.types.scope, parent);
        self.scopes.push(scope);
        scope
    }

    /// One expression is returned as itself; several become a sequence node
    /// `[expr0 … exprN, null]` yielding the trailing one, itself the scope
    /// its declarations live in (DESIGN ›A scope's value is what it evaluates to‹).
    pub fn parse_sequence(&mut self) -> Result<DyadPtr, ParseError> {
        self.parse_block().map(|(_, value)| value)
    }

    /// [`Parser::parse_sequence`], with the scope the block opened, whose `dyads` hold
    /// every line whatever the value collapsed to.
    fn parse_block(&mut self) -> Result<(DyadPtr, DyadPtr), ParseError> {
        // `open` has one entry per open scope, so its length is the nesting
        // depth; past the limit the parse is the checked error, not a Rust stack overflow.
        if self.open.len() >= MAX_BRACKET_DEPTH {
            return Err(ParseError::TooDeep);
        }
        let scope = self.open_scope();
        let narrowed = self.narrow_next.take().into_iter().collect();
        self.open.push(OpenScope { narrowed, ..OpenScope::default() });
        let array_ = self.types.array_;
        while let Some(item) = self.parse_next() {
            let item = item?;
            // SAFETY: the pending bindings were minted by this parser's declares;
            // `scope` is the innermost open scope, minted by `open_scope` above.
            unsafe { self.scopes.close_item(self.rt.store, array_, item) };
            // A binding's `defer free <place>` is drained right after its
            // statement, so the defer sits at its source position, the right
            // LIFO rank among other statements' defers.
            let depth = self.open.len() - 1;
            if !self.open[depth].defers.is_empty() {
                let drained = std::mem::take(&mut self.open[depth].defers);
                for &d in &drained {
                    // SAFETY: `d` is a `defer free <place>` node the binding site
                    // just built; `scope` a scope `open_scope` minted.
                    unsafe {
                        let place = crate::identities::drop_model::teardown_place_of(d);
                        self.open[depth].owned.push(place);
                        crate::identities::scope::push_item(self.rt.store, array_, scope, d);
                    }
                }
            }
        }
        self.scopes.pop();
        // SAFETY: `scope` was minted by `open_scope` with a value, so it has an
        // array; the store outlives the slice and nothing pushes to it below.
        let exprs = unsafe {
            crate::identities::scope::dyads(self.rt.store, array_, scope);
            crate::identities::scope::exprs_of(scope).expect("a block scope has dyads")
        };
        // What the block parsed and did not run runs when the block runs;
        // what it did run stands in the body as its result.
        let owned_here = self.open.pop().expect("pushed above").owned;
        // Prose and a `defer` (it runs at exit, never as the tail) are
        // invisible to value flow.
        let defer_ = self.types.defer_;
        // SAFETY: `exprs` are reduced dyads just parsed/built.
        let is_value = |e: DyadPtr| unsafe {
            !crate::identities::numtype::is_comment_type((*e).ty) && (*e).ty != defer_
        };
        let values = exprs.iter().filter(|&&e| is_value(e)).count();
        match (values, exprs.len()) {
            // An empty `( )`, or a bracket holding only prose: the scope node
            // with nothing to run, yielding unit.
            (0, _) => {
                // SAFETY: `scope` was minted by `open_scope` above and is unaliased.
                unsafe {
                    crate::identities::scope::fill(scope, self.types.ops.scope_);
                }
                Ok((scope, scope))
            }
            (_, 1) => Ok((scope, exprs[0])),
            _ => {
                // Outside a function a `return` before the tail has nothing to leave.
                let types = self.types;
                let tail = exprs.iter().rposition(|&e| is_value(e)).expect("values >= 1");
                for (i, &e) in exprs.iter().enumerate() {
                    // SAFETY: `e` is a reduced dyad just parsed.
                    if i != tail && self.frames.is_empty() && unsafe { contains_return(types, e) } {
                        return Err(ParseError::EarlyReturn);
                    }
                }
                // Ownership must not escape as this scope's value: the
                // teardown frees the place on the way out. Only places this
                // scope frees are checked; an enclosing scope's owning place is an ordinary borrow.
                // SAFETY: `exprs[tail]` is a reduced dyad just parsed.
                let tail_value = unsafe {
                    let t = exprs[tail];
                    // `return x` yields `x`, so the escape rides its operand.
                    if (*t).ty == types.return_ && !(*t).value.is_null() {
                        *((*t).value as *const DyadPtr)
                    } else {
                        t
                    }
                };
                // SAFETY: `tail_value` is a reduced dyad from the store.
                if owned_here.contains(&unsafe { types.through(tail_value) }) {
                    return Err(ParseError::OwningEscape);
                }
                // SAFETY: `scope` was minted by `open_scope` above and is unaliased.
                unsafe {
                    crate::identities::scope::fill(scope, self.types.ops.scope_);
                }
                Ok((scope, scope))
            }
        }
    }

    /// `?`'s entry on a binding refuses every read of the value until it is
    /// written: the target of `x = …` passes, and so does `x:…`, which reads
    /// the binding (DESIGN ›Declarations are immutable by default‹).
    fn check_unwritten(&mut self, cell: &Cell) -> Result<(), ParseError> {
        let binding = cell.binding(self.types);
        // SAFETY: a non-null cell binding is a binding dyad from the store.
        if binding.is_null() || !unsafe { Binding::has_gate(binding, self.types.unknown) } {
            return Ok(());
        }
        if matches!(self.peek_token(), Some((t, _)) if t == self.types.assign || t == self.types.colon_)
        {
            return Ok(());
        }
        self.pos = cell.start;
        // SAFETY: as above.
        Err(ParseError::Unwritten(Box::new(unsafe { Binding::spelling(binding) })))
    }

    pub(crate) fn note_write(&mut self, node: DyadPtr) {
        // SAFETY: `node` is the `=` node just built.
        if unsafe { (*node).ty } == self.types.tape.write {
            self.forget_narrowed();
        }
        self.last_write = (node, self.scopes.current().unwrap_or(std::ptr::null_mut()));
    }

    fn is_narrowed(&self, key: (DyadPtr, i32)) -> bool {
        self.open.iter().any(|s| s.narrowed.contains(&key))
    }

    /// After a tape edit a checked index may name another cell.
    fn forget_narrowed(&mut self) {
        for s in &mut self.open {
            s.narrowed.clear();
        }
        self.narrow_next = None;
    }

    /// A statement `x = …` built in the block that declared `x := T ?` lifts
    /// `?`'s read veto from that line on; a write nested in a group, an `if`,
    /// a loop or a `fn` body is no statement of that block, so it does not.
    ///
    /// # Safety
    /// `item` must be a reduced dyad from the store.
    pub unsafe fn fill_if_sibling_write(&mut self, item: DyadPtr) {
        let (node, scope) = self.last_write;
        if item.is_null() || item != node {
            return;
        }
        let target = *((*item).value as *const DyadPtr);
        if target.is_null()
            || (*target).ty != self.types.binding_
            || !Binding::has_gate(target, self.types.unknown)
            || Binding::read(target).scope != scope
        {
            return;
        }
        Binding::remove_gate(self.rt.store, self.types.array_, target, self.types.unknown);
    }

    /// The one sequencing step, shared by `parse_sequence`, the drivers and a
    /// type body: a comment node or one expression, an optional `,` after it
    /// consumed. `None` at the sequence's end. Where parse order is run order the item is left pending.
    pub fn parse_next(&mut self) -> Option<Result<DyadPtr, ParseError>> {
        loop {
            // What the last segment yielded, its expression and the prose
            // lifted out of it, goes out first.
            if let Some(item) = self.queued.pop_front() {
                // Where parse order is run order the item is pending: it runs
                // when the pass needs a value or when its scope runs.
                if self.runtime_depth == 0 {
                    self.open.last_mut().expect("the root scope is open").unrun.push(item);
                }
                return Some(Ok(item));
            }
            self.skip_whitespace();
            let bare_branch = self.else_ends == Some(self.open.len());
            if self.pos >= self.source.len()
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
                Err(e) => return Some(Err(e)),
            };
            // The `,` is consumed once the segment before it is constructed; a
            // `,` where nothing stands is purely for the reader.
            if matches!(boundary, Boundary::Comma) && !bare_branch {
                self.consume_separator();
            }
            if items.len() > 1 {
                self.pos = items[1].1;
                return Some(Err(ParseError::Trailing));
            }
            if let Some(&(item, _)) = items.first() {
                // SAFETY: `item` is a reduced dyad just constructed.
                unsafe { self.fill_if_sibling_write(item) };
            }
            let mut ordered = std::mem::take(&mut self.lifted);
            if let Some((item, start)) = items.pop() {
                ordered.push((start, item));
            }
            ordered.sort_by_key(|&(start, _)| start);
            self.queued.extend(ordered.into_iter().map(|(_, n)| n));
        }
    }

    /// `#` followed by a `«…»` string or raw text to the end of the line
    /// (DESIGN ›Text literals are plain values; `#` is the one comment constructor‹).
    pub(crate) fn construct_comment(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let node = self.comment_after_hash()?;
        tape.place(node);
        Ok(Constructed::Placed)
    }

    fn comment_after_hash(&mut self) -> Result<DyadPtr, ParseError> {
        let bytes = self.source.as_bytes();
        // Spaces (not the newline) may separate `#` from its text.
        while self.pos < bytes.len() && matches!(bytes[self.pos], b' ' | b'\t') {
            self.pos += 1;
        }
        let source = self.source;
        let text_node = if source[self.pos..].starts_with('«') {
            // `# «…»`: the string form ends at the `»`, not the line.
            let r =
                self.scopes.resolve(self.trie, &source[self.pos..]).map_err(ParseError::Resolve)?;
            let start = self.pos;
            self.pos += r.matched;
            self.construct_leaf(r.identity, start, r.matched)?.ok_or(ParseError::BadLiteral)?
        } else {
            let start = self.pos;
            while self.pos < bytes.len() && bytes[self.pos] != b'\n' {
                self.pos += 1;
            }
            let text = source[start..self.pos].trim_end();
            crate::identities::string::build_text(
                self.rt.store,
                self.types.string_,
                text.as_bytes(),
            )
        };
        Ok(self.rt.store.alloc_raw(self.types.comment_, text_node.cast()))
    }

    /// `name := value`: the name is declared before the value parses, so the
    /// value can refer to it, and the fixpoint then makes the placeholder BE
    /// the value. Legal only opening its expression; anywhere else it declines.
    pub(crate) fn construct_decl(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // The name is the cell to the left: a spelling, or the node `regex «…»`
        // placed, whose text is a pattern; anything else declines.
        let Some(tok) = tape.at(-1).copied() else {
            return Ok(Constructed::Decline);
        };
        let pattern: Option<Vec<u8>> = if tok.constructed {
            // SAFETY: a constructed cell holds a node from the store.
            if unsafe { (*tok.dyad).ty } == self.types.regex_ {
                // SAFETY: a `regex` node's value is a text blob.
                Some(unsafe { crate::identities::string::text(tok.dyad) }.to_vec())
            } else {
                return Ok(Constructed::Decline);
            }
        } else {
            None
        };
        // Copied out: the cell's text is independent of the `&mut self` the
        // declaration and value parse need, and of the fresh dyad written below.
        let name: String = match &pattern {
            Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            None => tok.spelling().to_string(),
        };
        let name: &str = &name;
        // The placeholder: the fresh dyad the cell already holds (DESIGN ›The
        // scope's constructor is the driver‹), or a fresh one for a known
        // spelling. `fn`-typed so a recursive self-call sees a function-typed callee.
        let placeholder = if tok.is_fresh() {
            // SAFETY: a fresh cell's dyad is a dyad from the store.
            unsafe { (*tok.dyad).ty = self.types.fn_type };
            tok.dyad
        } else {
            self.rt.store.alloc_raw(self.types.fn_type, std::ptr::null_mut())
        };
        let declared = if pattern.is_some() {
            self.declare_pattern(name, placeholder, tok.start)
        } else {
            self.declare_name(name, placeholder, tok.start)
        };
        let binding = match declared {
            Ok(binding) => binding,
            Err(e) => {
                // The stuck point is the name itself (it is what shadows).
                self.pos = tok.start;
                return Err(e);
            }
        };
        // A `fn` literal opening the value publishes its signature onto the
        // placeholder; the binding is the one a `lex_rank = …` line in the value writes.
        self.pending_fn = placeholder;
        self.filling.push(binding);
        let value = self.parse_expression();
        self.filling.pop();
        self.last_declared = binding;
        self.pending_fn = std::ptr::null_mut();
        let value = value?;
        // A bare name as the value is its binding (a use); the fixpoint
        // inspects the dyad behind it and keeps `value` as what the initializer stores.
        // SAFETY: `value` is a dyad from the store.
        let read = unsafe { self.types.through(value) };
        // Fixpoint: make the placeholder *be* the value, so references to
        // `name` captured while parsing the value resolve to it. A box on the
        // right is decided first; a rational value gets a place of `rational_number`.
        // SAFETY: `read` is a dyad from the store.
        let rational = unsafe { crate::identities::rational::is_rational_value(self.types, read) };
        // SAFETY: `read` is a reduced dyad from the store.
        let box_ty = match unsafe { crate::identities::read::read_kind(self.types, read) } {
            _ if rational => Some(self.types.rational),
            crate::identities::read::Read::Container(t)
                if t == self.types.type_ || t == self.types.dyad_ =>
            {
                Some(t)
            }
            // SAFETY: as above.
            _ if unsafe { (*read).ty } == self.types.tape.cell_value => Some(self.types.type_),
            // SAFETY: as above.
            _ => unsafe { crate::identities::hashmap::box_of(self.types, read) },
        };
        // SAFETY: `placeholder` was minted for the name and nothing has read a value from it; `binding`, `value` and `read` are dyads from the store.
        let declared = unsafe {
            if self.holes.remove(&value) {
                // `x := i32 ?`: the place `?` built is what the name binds to;
                // nothing initializes it, and `?`'s entry refuses a read until
                // a sibling write fills it. A hashmap's zeroed place is already
                // its empty map, so nothing is unknown to refuse.
                self.scopes.rebind(binding, value);
                if !crate::identities::hashmap::is_hashmap(self.types, (*value).ty) {
                    Binding::add_gate(
                        self.rt.store,
                        self.types.array_,
                        binding,
                        self.types.unknown,
                    );
                }
                value
            } else if (*read).ty == self.types.construct_ {
                let ops = (*read).value as *mut DyadPtr;
                let instance = *ops;
                (*placeholder).ty = (*instance).ty;
                (*placeholder).value = (*instance).value;
                *ops = placeholder;
                value
            } else if crate::identities::read::read_kind(self.types, read)
                == crate::identities::read::Read::Identity
            {
                // A type value: the name becomes another spelling of the type.
                self.scopes.rebind(binding, read);
                read
            } else if let Some(t) = box_ty {
                // A box on the right: reads are copy by default, so `x` gets
                // its own box and a copy of what `a` holds.
                let place = self.alloc_local(t, 8);
                let init = crate::identities::build_init(self.rt.store, self.types, place, read)?;
                self.scopes.rebind(binding, place);
                init
            } else if crate::identities::drop_model::is_owning_value(self.types, value) {
                // An owning value lands in a place here, the one site that
                // knows the name it binds, so the teardown attaches here (DESIGN
                // ›Explicit heap‹): an owning `@pointee` place, the value in it, `defer free <place>` in this scope.
                let pointee = crate::identities::drop_model::owning_pointee_of(self.types, value)
                    .expect("is_owning_value implies a pointee");
                let owning_ty = crate::identities::pointer::make_owning_pointer_type(
                    self.rt.store,
                    self.types.type_,
                    pointee,
                    self.types.ops.teardown_,
                );
                // A pointer is 8 bytes (U64-wide), whatever it points at.
                let place = self.alloc_local(owning_ty, 8);
                let init = crate::identities::build_init(self.rt.store, self.types, place, value)?;
                self.scopes.rebind(binding, place);
                // The owning check is kept on to guard a future caller
                // inserting a free over a borrow.
                let free_node = crate::identities::drop_model::build_teardown(
                    self.rt.store,
                    self.types,
                    self.types.free_,
                    place,
                    true,
                )?;
                let defer_node = crate::identities::drop_model::build_defer(
                    self.rt.store,
                    self.types,
                    free_node,
                );
                self.open.last_mut().expect("a scope's defer list is open").defers.push(defer_node);
                init
            } else if (*read).ty != self.types.rational
                && matches!(
                    crate::identities::numtype_of(self.types, value),
                    crate::identities::Operand::Concrete(_)
                        | crate::identities::Operand::Pointer(_)
                )
            {
                // A runtime numeric or pointer value is snapshotted: fresh
                // per-call storage, the name bound to it, the value kept as a
                // re-runnable initializer, so a read is a plain load and a loop-body local re-initializes on entry.
                let (ty_node, width) =
                    crate::identities::scalar_binding_type(self.rt.store, self.types, value);
                let place = self.alloc_local(ty_node, width);
                let init = crate::identities::build_init(self.rt.store, self.types, place, value)?;
                self.scopes.rebind(binding, place);
                init
            } else {
                (*placeholder).ty = (*read).ty;
                (*placeholder).value = (*read).value;
                placeholder
            }
        };
        let node = crate::identities::declare::build(
            self.rt.store,
            self.types.declare_,
            self.types.ops.declare_,
            binding,
            value,
            declared,
        );
        tape.remove(-1); // the name token, consumed
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Consume the path token (raw text up to whitespace or `,`, or a `«…»`
    /// string), load the file, and place `{type: import, value: [path, tail,
    /// op]}`. The load happens here, once per run; the node's run only re-yields the tail.
    pub(crate) fn construct_import(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // The load is a comptime effect: inside a deferred-or-repeated body
        // parse order and run order do not coincide.
        if self.runtime_depth != 0 {
            return Err(ParseError::ImportInRuntimeBody);
        }
        self.skip_whitespace();
        let source = self.source;
        let start = self.pos;
        let path_text: String = if source[self.pos..].starts_with('«') {
            let r =
                self.scopes.resolve(self.trie, &source[self.pos..]).map_err(ParseError::Resolve)?;
            let s = self.pos;
            self.pos += r.matched;
            let node =
                self.construct_leaf(r.identity, s, r.matched)?.ok_or(ParseError::ExpectedPath)?;
            // SAFETY: the leaf just built is a string node.
            String::from_utf8_lossy(unsafe { crate::identities::string::text(node) }).into_owned()
        } else {
            let bytes = source.as_bytes();
            while self.pos < bytes.len()
                && !bytes[self.pos].is_ascii_whitespace()
                && bytes[self.pos] != b','
            {
                self.pos += 1;
            }
            source[start..self.pos].to_string()
        };
        if path_text.is_empty() {
            self.pos = start;
            return Err(ParseError::ExpectedPath);
        }
        let tail = self.import_file(&path_text)?;
        let types = self.types;
        let path_node = crate::identities::string::build_text(
            self.rt.store,
            types.string_,
            path_text.as_bytes(),
        );
        let value = self.rt.store.alloc_operands(&[path_node, tail, types.ops.import_]);
        let node = self.rt.store.alloc_raw(types.import_, value);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Read the `«…»` to the right at discovery and place a `regex` node
    /// holding the pattern's bytes (DESIGN ›The scope's constructor is the
    /// driver‹). Compiled here, so a bad pattern errors at its quote, since the trie compiles lazily.
    pub(crate) fn construct_regex(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        self.skip_whitespace();
        let source = self.source;
        let start = self.pos;
        if !source[start..].starts_with('«') {
            return Err(ParseError::ExpectedPattern);
        }
        let r = self.scopes.resolve(self.trie, &source[start..]).map_err(ParseError::Resolve)?;
        self.pos += r.matched;
        let quote = self
            .construct_leaf(r.identity, start, r.matched)?
            .ok_or(ParseError::ExpectedPattern)?;
        // SAFETY: the leaf just built is a string node.
        let pattern = unsafe { crate::identities::string::text(quote) }.to_vec();
        let text = String::from_utf8_lossy(&pattern);
        let bad = if pattern.is_empty() {
            Some("a pattern must not be empty".to_string())
        } else {
            // The engine's report ends in its one-line reason; the lines
            // before repeat the pattern with a caret.
            regex::bytes::Regex::new(&format!("^(?:{text})")).err().map(|e| {
                e.to_string()
                    .lines()
                    .last()
                    .unwrap_or("")
                    .trim()
                    .trim_start_matches("error: ")
                    .to_string()
            })
        };
        if let Some(reason) = bad {
            self.pos = start;
            return Err(ParseError::BadPattern(reason));
        }
        let node =
            crate::identities::string::build_text(self.rt.store, self.types.regex_, &pattern);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Read the `«…»` to the right at discovery and place the node that lexes
    /// it when it runs (DESIGN ›Text is the quote‹): the cells are made at
    /// each run, against the scopes open then.
    pub(crate) fn construct_lex(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let quote = self.quote_after("lex")?;
        let node = crate::identities::lex::build(self.rt.store, self.types, quote);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    pub(crate) fn construct_print(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let parts = self.interpolated_quote("print")?;
        let node = crate::identities::print::build(self.rt.store, self.types, &parts);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    pub(crate) fn construct_error(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let parts = self.interpolated_quote("error")?;
        let node = crate::identities::error::build(self.rt.store, self.types, &parts);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// The `«…»` to the right of `print` or `error`, read into its parts.
    fn interpolated_quote(&mut self, word: &'static str) -> Result<Vec<DyadPtr>, ParseError> {
        let quote = self.quote_after(word)?;
        let end = self.pos;
        // SAFETY: `quote` is the string node the `«…»` constructor just built.
        let len = unsafe { crate::identities::string::text(quote) }.len();
        // The literal has no escapes, so its text is the source between the guillemets.
        let inner = end - 2 - len;
        let parts = self.quote_parts(inner, inner + len)?;
        self.pos = end;
        Ok(parts)
    }

    /// A `print` or `error` quote's text runs and `{…}` expressions, each
    /// expression parsed here, in the scope the word appears in; `\{` and `\}`
    /// are the braces as text.
    fn quote_parts(&mut self, from: usize, to: usize) -> Result<Vec<DyadPtr>, ParseError> {
        let source = self.source;
        let bytes = source.as_bytes();
        let mut parts = Vec::new();
        let mut text = Vec::new();
        let mut i = from;
        while i < to {
            match bytes[i] {
                b'\\' if i + 1 < to && matches!(bytes[i + 1], b'{' | b'}') => {
                    text.push(bytes[i + 1]);
                    i += 2;
                }
                b'}' => {
                    self.pos = i;
                    return Err(ParseError::StrayInterpolationClose);
                }
                b'{' => {
                    let Some(close) = source[i + 1..to].find('}').map(|k| i + 1 + k) else {
                        self.pos = i;
                        return Err(ParseError::UnclosedInterpolation);
                    };
                    if !text.is_empty() {
                        let t = crate::identities::string::build_text(
                            self.rt.store,
                            self.types.string_,
                            &text,
                        );
                        parts.push(t);
                        text.clear();
                    }
                    parts.push(self.parse_within(i + 1, close)?);
                    i = close + 1;
                }
                b => {
                    text.push(b);
                    i += 1;
                }
            }
        }
        if !text.is_empty() || parts.is_empty() {
            parts.push(crate::identities::string::build_text(
                self.rt.store,
                self.types.string_,
                &text,
            ));
        }
        Ok(parts)
    }

    /// One expression over `source[from..to]` alone: the source is cut at `to`
    /// so the segment ends there, and offsets stay those of the whole line.
    fn parse_within(&mut self, from: usize, to: usize) -> Result<DyadPtr, ParseError> {
        let whole = self.source;
        self.source = &whole[..to];
        self.pos = from;
        let parsed = self.parse_expression().and_then(|expr| {
            self.skip_whitespace();
            if self.pos < to {
                Err(ParseError::Trailing)
            } else {
                Ok(expr)
            }
        });
        self.source = whole;
        parsed
    }

    /// The `«…»` string node to the right of a raw-text word, consumed at discovery.
    fn quote_after(&mut self, word: &'static str) -> Result<DyadPtr, ParseError> {
        self.skip_whitespace();
        let source = self.source;
        let start = self.pos;
        if !source[start..].starts_with('«') {
            return Err(ParseError::ExpectedQuote(word));
        }
        let r = self.scopes.resolve(self.trie, &source[start..]).map_err(ParseError::Resolve)?;
        self.pos += r.matched;
        self.construct_leaf(r.identity, start, r.matched)?.ok_or(ParseError::ExpectedQuote(word))
    }

    /// The node placed at discovery carries the scope open at its appearance
    /// (DESIGN ›Meta-navigation‹): inside a constructor's body the definition
    /// site; the use site is `caller.scope`.
    pub(crate) fn construct_here(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let scope = self.scopes.current().unwrap_or(std::ptr::null_mut());
        let node = crate::identities::here::build_here(self.rt.store, self.types, scope);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// The node whose `.scope` reads the pass's position when a constructor
    /// runs it (DESIGN ›Meta-navigation‹).
    pub(crate) fn construct_caller(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let node = crate::identities::here::build_caller(self.rt.store, self.types);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// On a first load the file runs in its own section, a fresh stack of the
    /// root plus a section scope, so it sees ambient names and its own imports
    /// only (DESIGN ›Importing is dropping the text there‹); its `pub` names then land here.
    fn import_file(&mut self, path_text: &str) -> Result<DyadPtr, ParseError> {
        let joined = self.dir.join(path_text);
        let canon = joined
            .canonicalize()
            .map_err(|e| ParseError::ImportRead(format!("{}: {e}", joined.display())))?;
        match self.imports.entries.get(&canon) {
            Some(ImportState::Loading) => {
                return Err(ParseError::ImportCycle(path_text.to_string()))
            }
            Some(ImportState::Loaded { pubs, tail }) => {
                let (pubs, tail) = (pubs.clone(), *tail);
                self.publish(&pubs)?;
                return Ok(tail);
            }
            None => {}
        }
        let text = std::fs::read_to_string(&canon)
            .map_err(|e| ParseError::ImportRead(format!("{}: {e}", joined.display())))?;
        // Sources are process-lived: every span and every later report
        // indexes into its file's text.
        let text: &'static str = Box::leak(text.into_boxed_str());
        self.imports.entries.insert(canon.clone(), ImportState::Loading);

        // A fresh stack of the root (ambient names) plus a fresh scope node
        // the file's declarations land in.
        let root = *self.scopes.open.first().expect("an import site has an open root scope");
        let mut nested = ScopeStack::new();
        nested.push(root);
        let section = nested.push_section(self.rt.store, self.types.scope);
        self.imports.sections.insert(section);

        let saved_source = std::mem::replace(&mut self.source, text);
        let saved_pos = std::mem::replace(&mut self.pos, 0);
        let saved_scopes = std::mem::replace(&mut self.scopes, nested);
        let saved_dir = std::mem::replace(
            &mut self.dir,
            canon.parent().map(Into::into).unwrap_or_else(|| PathBuf::from(".")),
        );
        let saved_frames = std::mem::take(&mut self.frames);
        let saved_definitions = std::mem::take(&mut self.definitions);
        let saved_pending_fn = std::mem::replace(&mut self.pending_fn, std::ptr::null_mut());
        let saved_runtime_depth = std::mem::replace(&mut self.runtime_depth, 0);

        let inner = self.run_imported();
        let inner_pos = self.pos;

        self.source = saved_source;
        self.pos = saved_pos;
        self.scopes = saved_scopes;
        self.dir = saved_dir;
        self.frames = saved_frames;
        self.definitions = saved_definitions;
        self.pending_fn = saved_pending_fn;
        self.runtime_depth = saved_runtime_depth;

        match inner {
            Err(message) => {
                // Remove the Loading entry so a later attempt (a REPL retry)
                // reports the real failure, not a phantom cycle.
                self.imports.entries.remove(&canon);
                Err(ParseError::ImportFailed {
                    path: path_text.to_string(),
                    rendered: crate::report::render(path_text, text, inner_pos, &message),
                })
            }
            Ok((pubs, tail)) => {
                self.imports
                    .entries
                    .insert(canon, ImportState::Loaded { pubs: pubs.clone(), tail });
                self.publish(&pubs)?;
                Ok(tail)
            }
        }
    }

    /// The nested pass over an imported file: each statement pending, the
    /// file run at its end, collecting the `pub` (name, identity) pairs and
    /// the tail node. On failure the message is returned with `offset` at the stuck point.
    fn run_imported(&mut self) -> Result<(Vec<(String, DyadPtr)>, DyadPtr), String> {
        let mut pubs = Vec::new();
        let mut tail = std::ptr::null_mut();
        while let Some(item) = self.parse_next() {
            let node = item.map_err(|e| crate::report::parse_message(&e))?;
            // SAFETY: `node` is the line `parse_next` just returned.
            unsafe { self.close_item(node) };
            // SAFETY: `node` was just parsed into the store, which outlives the pass.
            unsafe {
                if (*node).ty != self.types.comment_ {
                    tail = node;
                }
                if (*node).ty == self.types.declare_ {
                    let name = Binding::spelling(crate::identities::declare::binding_of(node));
                    let resolved = self
                        .scopes
                        .resolve(self.trie, &name)
                        .map_err(|_| format!("name `{name}` did not stay resolvable"))?;
                    if Binding::has_gate(resolved.binding, self.types.pub_) {
                        pubs.push((name, resolved.identity));
                    }
                }
            }
        }
        // A stray `)` ends the loop without being consumed, as in the drivers.
        if !self.source[self.pos..].trim_start().is_empty() {
            return Err("unexpected `)` — no scope is open here".to_string());
        }
        self.drain().map_err(|e| crate::report::parse_message(&e))?;
        Ok((pubs, tail))
    }

    /// Pub-only exposure, the ordinary visibility rule: a collision with a
    /// live name is the ordinary shadowing error. Idempotent where the name
    /// already resolves to the same identity.
    fn publish(&mut self, pubs: &[(String, DyadPtr)]) -> Result<(), ParseError> {
        // A collision is the import's: reported where the import stands.
        let at = self.pos;
        for (name, identity) in pubs {
            if let Ok(r) = self.scopes.resolve(self.trie, name) {
                if r.identity == *identity {
                    continue;
                }
            }
            self.declare_name(name, *identity, at)?;
        }
        Ok(())
    }

    /// `:`'s constructor (DESIGN ›The dyad's read surface‹): a field of the
    /// binding to the left, the cell taken as it stands, never through the
    /// reading rule; the member is the raw spelling at the cell to the right.
    pub(crate) fn construct_binding_read(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let lhs = match tape.at(-1).copied() {
            Some(cell) => self.as_operand(cell)?,
            None => return Err(ParseError::MissingOperand),
        };
        let Some(m) = self.cell_at(tape, 1)? else {
            return Err(ParseError::ExpectedField);
        };
        let save = self.pos;
        self.pos = m.start;
        let member = self.lex_spelling();
        self.pos = save;
        let Some((nstart, nlen)) = member else {
            return Err(ParseError::ExpectedField);
        };
        // SAFETY: `lhs` is a dyad off the tape.
        let node = unsafe { self.binding_read(lhs, nstart, nlen)? };
        tape.remove(1);
        tape.remove(-1);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// On a binding, `type` is the type of the dyad the name stands for,
    /// `start` the declaring line once it is complete, and any other field the
    /// instance-field read in `binding`'s own scope; on a constructed node
    /// `type` is its own and the path answers the rest (DESIGN ›Meta-navigation‹),
    /// `start`/`end` null.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn binding_read(
        &mut self,
        lhs: DyadPtr,
        nstart: usize,
        nlen: usize,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let source = self.source;
        let name = &source[nstart..nstart + nlen];
        // The binding's `dyad` field is storage, not a spelling.
        if name == "dyad" || name == "value" {
            return Err(ParseError::CellNotReachable);
        }
        // A name answers from its own binding; a node reached by path from itself.
        let lhs = if (*lhs).ty == types.binding_ {
            lhs
        } else {
            self.known_node(lhs, std::ptr::null_mut()).unwrap_or(lhs)
        };
        // Read when the constructor runs.
        if crate::identities::tape::is_cell_read(types, lhs) {
            if name == "type" {
                return Ok(crate::identities::tape::build_cell_type(self.rt.store, types, lhs));
            }
            // `t[k]:name`: the spelling of the binding the cell holds.
            if name == "name" {
                return Ok(crate::identities::tape::build_slot_name(self.rt.store, types, lhs));
            }
            return Err(ParseError::ExpectedField);
        }
        if (*lhs).ty == types.binding_ {
            if name == "type" {
                // Through a settled box: the type of what the name holds.
                let cell = self.settled_type(Binding::read(lhs).dyad);
                if cell.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                return Ok((*cell).ty);
            }
            // `b:start`: the declaring line, once it is complete, is a node
            // like any other (DESIGN ›The dyad's read surface‹).
            let start = Binding::read(lhs).start;
            if name == "start" && !start.is_null() {
                return Ok(self.path_operand(start));
            }
            let (field, offset, _) = self.resolve_field(types.binding_, nstart, nlen)?;
            let addr = (*lhs).value.wrapping_add(offset);
            // `a:name`: a `string`-typed place over the slot, marked as one so
            // the reading rule sees the container it is (no place of type `string` exists in a layout).
            if name == "name" {
                let place = crate::dyad::global_place(addr);
                return Ok(self.rt.store.alloc_raw(types.string_, place));
            }
            // `a:lex_rank`: an `f64` place a user may write, so it carries the
            // place mark `=` asks for.
            if name == "lex_rank" {
                let place = crate::dyad::global_place(addr);
                return Ok(self.rt.store.alloc_raw((*field).ty, place));
            }
            return Ok(self.rt.store.alloc_raw((*field).ty, addr));
        }
        // `this.f:type` is the field's declared type, not the type of the read that reaches it;
        // an untyped field's type is the written value's, unknown until the constructor runs.
        if crate::identities::this::is_field_read(types, lhs) && name == "type" {
            let declared =
                self.fills.get(&lhs).map_or(std::ptr::null_mut(), |&b| (*Binding::read(b).dyad).ty);
            if declared.is_null() {
                return Err(ParseError::BadReflectRead);
            }
            return Ok(declared);
        }
        let value = match name {
            "type" => return Ok((*lhs).ty),
            "scope" => self.scopes.current().unwrap_or(std::ptr::null_mut()),
            "start" | "end" | "gate" => std::ptr::null_mut(),
            _ => {
                // Not a field of a binding: the same error a binding read gives.
                self.resolve_field(types.binding_, nstart, nlen)?;
                return Err(ParseError::ExpectedField);
            }
        };
        Ok(self.address_value(types.dyad_, value))
    }

    /// A pointer-typed literal with its own eight bytes of storage, read at
    /// run time like any pointer variable.
    fn address_value(&mut self, pointee: DyadPtr, addr: DyadPtr) -> DyadPtr {
        crate::identities::pointer::address_value(self.rt.store, self.types, pointee, addr)
    }

    /// `node` reached by path: as a value its address, so using it runs nothing;
    /// the reads after it fold through [`Parser::known_node`].
    fn reach(&mut self, node: DyadPtr) -> DyadPtr {
        self.address_value(self.types.dyad_, node)
    }

    /// The node an `@dyad` value stands for when the parse knows it: an
    /// address no `=` writes, or a name `root` no `=` can move, followed through
    /// its declaring line to the right side as written (DESIGN ›The dyad's read
    /// surface‹: a binding gets you a dyad or a node).
    ///
    /// # Safety
    /// `value` must be a dyad from the store; `root` null or a binding dyad.
    unsafe fn known_node(&self, value: DyadPtr, root: DyadPtr) -> Option<DyadPtr> {
        use crate::identities::{declare, read};
        let types = self.types;
        if !root.is_null() {
            if Binding::has_gate(root, types.mut_) {
                return None;
            }
            let start = Binding::read(root).start;
            if start.is_null()
                || (*start).ty != types.declare_
                || declare::binding_of(start) != root
            {
                return None;
            }
            let rhs = declare::rhs_of(start);
            let next = if (*rhs).ty == types.binding_ { rhs } else { std::ptr::null_mut() };
            return self.known_node(types.through(rhs), next);
        }
        let stored = (*value).value;
        if stored.is_null()
            || crate::dyad::is_place(stored)
            || !matches!(read::read_kind(types, value), read::Read::Pointer(p) if p == types.dyad_)
        {
            return None;
        }
        let node = *(stored as *const DyadPtr);
        (!node.is_null() && self.rt.store.contains(node)).then_some(node)
    }

    /// Exactly the cell's two fields, `.type` and `.value` (DESIGN ›The dyad's
    /// read surface‹); nothing else reads through the view, and nothing here writes.
    ///
    /// # Safety
    /// `view` must be a node of type `dyad`.
    unsafe fn view_member(&mut self, view: DyadPtr, name: &str) -> Result<DyadPtr, ParseError> {
        let viewed = (*view).value as DyadPtr;
        if viewed.is_null() {
            return Err(ParseError::BadReflectRead);
        }
        match name {
            "type" => Ok((*viewed).ty),
            // The raw address as a u64; the `@void` spelling waits for pointer-value plumbing.
            "value" => Ok(self.scalar_value(
                crate::identities::numtype::NumType::U64,
                (*viewed).value as usize as i64,
            )),
            _ => Err(ParseError::BadReflectRead),
        }
    }

    /// The shared metadata stored once per type: `.arity`, `.roles[i]`,
    /// `.parse_rank`, `.associativity`, `.parse`, `.fields`,
    /// `.size_bytes`, `.scope`; a null slot is the honest undefined and errors until `?` exists.
    ///
    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn logos_member(
        &mut self,
        logos: DyadPtr,
        name: &str,
        index: Option<usize>,
    ) -> Result<DyadPtr, ParseError> {
        use crate::identities::meta;
        use crate::identities::numtype::NumType;
        if meta::kind_of(logos).is_none() {
            return Err(ParseError::BadReflectRead);
        }
        let operand_kind = matches!(meta::kind_of(logos), Some(meta::TUPLE_TAG | meta::LIST_TAG));
        match name {
            "arity" if operand_kind => {
                Ok(self.scalar_value(NumType::I64, meta::arity_of(logos) as i64))
            }
            "roles" if operand_kind => {
                let i = index.ok_or(ParseError::ExpectedIndexBracket)?;
                if i >= meta::arity_of(logos) {
                    return Err(ParseError::BadReflectRead);
                }
                Ok(meta::role_of(logos, i))
            }
            "parse_rank" => {
                Ok(self.scalar_value(NumType::F64, meta::parse_rank_of(logos).to_bits() as i64))
            }
            // `lex_rank` is the name's binding's, not the type's, so it falls to
            // the unknown-member error below. Associativity's values are the identities `left` and `right`.
            "associativity" => Ok(match meta::assoc_of(logos) {
                Assoc::Left => self.types.left_,
                Assoc::Right => self.types.right_,
            }),
            // The Logos function a body filled the slot with, or the view of a native leaf.
            "parse" => {
                let c = meta::constructor_of(logos);
                if c.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                if (*c).ty == self.types.fn_type {
                    return Ok(c);
                }
                Ok(self.rt.store.alloc_raw(self.types.dyad_, c as *mut u8))
            }
            "run" => Err(ParseError::NoOwnRun),
            "fields" if meta::is_record_type(logos) => {
                let view = self
                    .rt
                    .store
                    .alloc_raw(self.types.dyad_, meta::record_fields_of(logos) as *mut u8);
                self.fields_views.insert(view, logos);
                Ok(view)
            }
            "size_bytes" if meta::is_record_type(logos) => {
                Ok(self.scalar_value(NumType::U64, meta::record_size_of(logos) as i64))
            }
            "size_bytes" => match crate::identities::read::place_layout(self.types, logos) {
                Some((
                    crate::identities::read::Read::Scalar(_)
                    | crate::identities::read::Read::Pointer(_),
                    width,
                )) => Ok(self.scalar_value(NumType::U64, width as i64)),
                _ => Err(ParseError::BadReflectRead),
            },
            "scope" if meta::is_record_type(logos) => Ok(self
                .rt
                .store
                .alloc_raw(self.types.dyad_, meta::record_scope_of(logos) as *mut u8)),
            "type" => Err(ParseError::TypeIsColonRead),
            // A type's `.` answers only for the slots `type` declares; what its
            // instances hold is reached through `.fields` (DESIGN ›Resolution is one rule‹).
            _ if self.shared_member_of(logos, name).is_some() => {
                Err(ParseError::MemberThroughFields(name.to_string()))
            }
            _ if self.per_node_field_of(logos, name) => {
                Err(ParseError::PerNodeThroughType(name.to_string()))
            }
            _ => Err(ParseError::BadReflectRead),
        }
    }

    /// `t.fields.x`: a `shared` member of `t`, the instances' `run`, or else
    /// the view's own `.type` and `.value`.
    ///
    /// # Safety
    /// `view` must be the `dyad` view `.fields` built over `logos`, a record type.
    unsafe fn fields_member(
        &mut self,
        view: DyadPtr,
        logos: DyadPtr,
        name: &str,
    ) -> Result<DyadPtr, ParseError> {
        // The held body is no function until a node's field types construct
        // it, so it is read through a view, as `.parse` reads a native leaf.
        if name == "run" {
            let held = crate::identities::meta::run_body_of(logos);
            if held.is_null() {
                return Err(ParseError::BadReflectRead);
            }
            return Ok(self.rt.store.alloc_raw(self.types.dyad_, held as *mut u8));
        }
        if let Some(identity) = self.shared_member_read(logos, name) {
            return Ok(identity);
        }
        if self.per_node_field_of(logos, name) {
            return Err(ParseError::PerNodeThroughType(name.to_string()));
        }
        self.view_member(view, name)
    }

    /// A `shared` member of `logos`, one place stored with the type, read
    /// through the type or a node alike: the member's own gate decides a
    /// write, since the place is no node's.
    ///
    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn shared_member_read(&mut self, logos: DyadPtr, name: &str) -> Option<DyadPtr> {
        let (identity, binding) = self.shared_member_of(logos, name)?;
        self.paths.insert(identity, vec![binding]);
        Some(identity)
    }

    /// The identity and binding of `logos`'s `shared` member `name`, declared
    /// in the definition body's scope.
    ///
    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn shared_member_of(&self, logos: DyadPtr, name: &str) -> Option<(DyadPtr, DyadPtr)> {
        use crate::identities::meta;
        if !meta::is_record_type(logos) {
            return None;
        }
        let body = meta::record_body_of(logos);
        if body.is_null() {
            return None;
        }
        let mut members = ScopeStack::new();
        members.push(body);
        let r = members.resolve(self.trie, name).ok()?;
        Some((r.identity, r.binding))
    }

    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn per_node_field_of(&self, logos: DyadPtr, name: &str) -> bool {
        use crate::identities::meta;
        if !meta::is_record_type(logos) {
            return false;
        }
        let mut fields = ScopeStack::new();
        fields.push(meta::record_scope_of(logos));
        fields.resolve(self.trie, name).is_ok()
    }

    /// Fresh storage holding `bits` at `nt`'s width: the reflection counts
    /// and measures are ordinary typed values, comparable with literals.
    fn scalar_value(&mut self, nt: crate::identities::numtype::NumType, bits: i64) -> DyadPtr {
        let ty = self.types.numtypes[nt as usize];
        let width = nt.bytes();
        let bytes = bits.to_ne_bytes();
        let storage = self.rt.store.alloc_bytes(&bytes[..width]);
        self.rt.store.alloc_raw(ty, storage)
    }

    /// The next token of the source as a tape cell with its offset, or `None`
    /// at the end; a spelling the trie does not know becomes a fresh-name
    /// cell, declared by a following `:=` or reported at the boundary.
    fn lex_cell(&mut self) -> Result<Option<(Cell, usize)>, ParseError> {
        let in_run_order = match &self.feed {
            Some(feed) => feed.in_run_order,
            None => true,
        };
        let mut cell = if self.feed.is_some() {
            let Some(cell) = self.feed_take() else { return Ok(None) };
            cell
        } else {
            self.skip_whitespace();
            let source = self.source;
            let Some((cell, next)) =
                lex_token(&self.scopes, self.trie, self.rt.store, source, self.pos)
                    .map_err(ParseError::Resolve)?
            else {
                return Ok(None);
            };
            self.pos = next;
            cell
        };
        // A box is read by the pass wherever it stands as an operand, and the
        // read is honest only if everything parsed before it has run; so where
        // parse order is run order, its cell is the point the pass runs to.
        if in_run_order && self.runtime_depth == 0 && !cell.is_fresh() {
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

    /// A slot of the call a held `type (…)` is being built in, which is live.
    fn is_live_call_slot(&self, id: DyadPtr) -> bool {
        // SAFETY: `id` is null or a resolved dyad from the store.
        !id.is_null()
            && matches!(crate::dyad::frame_ref(unsafe { (*id).value }),
                Some((depth, _)) if depth > 0 && depth == self.held_depth && self.frames.len() == depth)
    }

    /// A place holding a type denotes the type it holds, since lexing a box's
    /// cell first ran everything before it (DESIGN ›A type is a comptime
    /// value‹); in a deferred body, or a frame slot, the place denotes itself,
    /// except a slot of the call a held `type (…)` is being built in, which is live.
    fn settled_type(&self, id: DyadPtr) -> DyadPtr {
        if self.runtime_depth > 0 {
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
            let addr = match crate::dyad::global_ref((*id).value) {
                Some(addr) => addr,
                None if self.is_live_call_slot(id) => match self.rt.place_addr(id) {
                    Some(addr) => addr,
                    None => return id,
                },
                None => return id,
            };
            let held = std::ptr::read_unaligned(addr as *const DyadPtr);
            if held.is_null() || !self.rt.store.contains(held) {
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
    fn cell_identity(&self, cell: &Cell) -> DyadPtr {
        self.settled_type(cell.identity(self.types))
    }

    /// The held cell whose spelling starts at the cursor, if any; the running
    /// index is corrected either way, since a boundary rewinds the cursor.
    fn feed_index(&mut self) -> Option<usize> {
        self.skip_whitespace();
        let pos = self.pos;
        let feed = self.feed.as_mut()?;
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
    fn resolve_fresh(&self, spelling: &str) -> Result<Resolved, ResolveError> {
        let r = self.scopes.resolve(self.trie, spelling)?;
        if let Some(feed) = &self.feed {
            if !self.scopes.position(r.scope).is_some_and(|p| p >= feed.base_depth) {
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
            }
        }
    }

    fn feed_take(&mut self) -> Option<Cell> {
        let i = self.feed_index()?;
        let mut cell = self.feed.as_ref()?.cells[i];
        self.feed.as_mut()?.next = i + 1;
        self.pos = cell.end();
        self.feed_resolve(&mut cell);
        Some(cell)
    }

    fn feed_peek(&mut self) -> Option<(DyadPtr, usize)> {
        let i = self.feed_index()?;
        let mut cell = self.feed.as_ref()?.cells[i];
        self.feed_resolve(&mut cell);
        if cell.is_fresh() {
            return None;
        }
        Some((cell.identity(self.types), cell.len))
    }

    /// Lex a held run body's text once, at the definition: every token an
    /// unconstructed cell, brackets included; a `#` comment's text is passed
    /// over as its constructor will read it. `text` must live for the run.
    fn lex_body_fragment(&mut self, text: &'static str) -> Result<ParsingTape, ParseError> {
        let mut tape = ParsingTape::new();
        let mut pos = 0;
        while let Some((cell, next)) = lex_token(&self.scopes, self.trie, self.rt.store, text, pos)
            .map_err(ParseError::Resolve)?
        {
            pos = next;
            let is_hash = cell.spelling() == "#";
            tape.push(cell);
            if is_hash {
                let bytes = text.as_bytes();
                while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
                    pos += 1;
                }
                if !text[pos..].starts_with('«') {
                    while pos < bytes.len() && bytes[pos] != b'\n' {
                        pos += 1;
                    }
                }
            }
        }
        tape.set_cursor(0);
        Ok(tape)
    }

    /// `None` for an inert cell (a value, a delimiter). The identity's own
    /// slot first; failing that, its type's shared instance constructor,
    /// application (DESIGN ›The constructor is a field‹).
    fn ctor_of(&self, id: DyadPtr) -> Option<ConstructFn> {
        // SAFETY: `id` is a resolved dyad from the store.
        unsafe {
            if !id.is_null() && (*id).ty == self.types.fn_type {
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
    unsafe fn identity_head(&self, id: DyadPtr) -> Option<DyadPtr> {
        if id.is_null()
            || (*id).ty != self.types.type_
            || crate::identities::meta::kind_of(id).is_none()
        {
            None
        } else {
            Some(id)
        }
    }

    /// A node its type's own `parse` built, standing on the tape, whose type's
    /// fields block fills the instances' `parse`; a place of the type is not
    /// one, since the node it will hold is known only when the program runs.
    fn is_awake_instance(&self, id: DyadPtr) -> bool {
        // SAFETY: `id` is null or a resolved dyad from the store; the trio is read only on a record type.
        unsafe {
            !id.is_null()
                && !(*id).value.is_null()
                && !crate::dyad::is_place((*id).value)
                && crate::identities::meta::is_record_type((*id).ty)
                && !crate::identities::meta::instances_parse_of((*id).ty).is_null()
        }
    }

    /// The place of `id` on the one axis (its record's parse_rank), or
    /// `prec::APPLY` for a cell that carries no record.
    fn precedence_of_cell(&self, id: DyadPtr) -> f64 {
        if self.is_awake_instance(id) {
            // SAFETY: `is_awake_instance` read the trio of this record type.
            return unsafe { crate::identities::meta::instances_parse_rank_of((*id).ty) };
        }
        // SAFETY: `id` is null or a resolved dyad from the store.
        unsafe {
            match self.identity_head(id) {
                Some(id) => crate::identities::meta::parse_rank_of(id),
                None => crate::identities::meta::prec::APPLY,
            }
        }
    }

    /// Its record's, or left for an instance running its type's shared
    /// constructor, which carries no record of its own.
    fn assoc_of_cell(&self, id: DyadPtr) -> Assoc {
        if self.is_awake_instance(id) {
            // SAFETY: `is_awake_instance` read the trio of this record type.
            return unsafe { crate::identities::meta::instances_assoc_of((*id).ty) };
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
        let was = std::mem::replace(&mut self.discovering, discovery);
        let outcome = construct(self, id, tape);
        self.discovering = was;
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
            // SAFETY: a constructed cell's dyad is a node from the store.
            if logos && !cell.dyad.is_null() && (instance || unsafe { (*cell.dyad).ty } == id) {
                let spelling = cell.spelling().to_string();
                // SAFETY: the node is the one `run_logos_ctor` minted for `id`, `[field…, null, spec]`.
                let folded =
                    unsafe { self.resolve_specialization(cell.dyad, cell.start, &spelling)? };
                // A comptime node folded: its literal stands in the cell.
                if let Some(lit) = folded {
                    tape.place(lit);
                    return Ok(());
                }
            }
            // A node that runs a function's body is a use of every outer name
            // that body reads, checked where the node comes to exist, whichever
            // constructor built it; the error points at the construct's own token.
            if !cell.dyad.is_null() {
                // SAFETY: a constructed cell holds a node from the store.
                let callee = unsafe { (*cell.dyad).ty };
                // SAFETY: `callee` is the resolved identity of the cell.
                if let Err(e) = unsafe { self.check_call_reads(callee) } {
                    self.pos = cell.start;
                    return Err(e);
                }
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
            self.pos = cell.start;
            return Err(ParseError::CellLeftUnconstructed(Box::new(cell.spelling().to_string())));
        }
        let declined = matches!(outcome, Constructed::Decline) || (!logos && tape.edits() == edits);
        if !declined {
            self.pos = cell.start;
            return Err(ParseError::CellLeftUnconstructed(Box::new(cell.spelling().to_string())));
        }
        // The identity stands as its own value: the use of its name, its binding.
        let value = self.stand_as_value(tape, id);
        tape.place(value);
        Ok(())
    }

    /// A `fn` node in the constructor slot: judged by the flag alone.
    fn is_logos_ctor(&self, id: DyadPtr) -> bool {
        // SAFETY: `id` is a resolved dyad from the store; the slot is read only where a record head exists.
        self.is_awake_instance(id)
            || unsafe {
                !id.is_null()
                    && (*id).ty == self.types.type_
                    && crate::identities::meta::kind_of(id).is_some()
                    && crate::identities::meta::is_record_type(id)
                    && {
                        let leaf = crate::identities::meta::constructor_of(id);
                        !leaf.is_null() && (*leaf).ty == self.types.fn_type
                    }
            }
    }

    /// An identity a cell may be handed on to: a function value, or a type
    /// node with a record head.
    fn is_identity_node(&self, d: DyadPtr) -> bool {
        // SAFETY: `d` is null or a resolved dyad from the store.
        unsafe {
            !d.is_null()
                && ((*d).ty == self.types.fn_type
                    || ((*d).ty == self.types.type_
                        && crate::identities::meta::kind_of(d).is_some()))
        }
    }

    /// Every cell up to the next `,`, `)`, or the end of input, none consumed,
    /// constructing at discovery each cell whose identity sits at or above `(`
    /// on the axis (DESIGN ›The scope's constructor is the driver‹).
    fn lex_segment(&mut self, tape: &mut ParsingTape) -> Result<Boundary, ParseError> {
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
        let was = std::mem::replace(&mut self.lex_mode, stop_at_open);
        let result = loop {
            match self.lex_next(tape, false) {
                Ok(None) => {}
                Ok(Some(boundary)) => break Ok(boundary),
                Err(e) => break Err(e),
            }
        };
        self.lex_mode = was;
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
        let Some((cell, start)) = self.lex_cell()? else {
            return Ok(Some(Boundary::Eof));
        };
        let id = self.cell_identity(&cell);
        if !cell.constructed {
            self.check_unwritten(&cell)?;
            if id == self.types.sep_ {
                self.pos = start;
                return Ok(Some(Boundary::Comma));
            }
            if id == self.types.close_ || id == self.types.close_sq_ {
                self.pos = start;
                return Ok(Some(Boundary::Close));
            }
            if id == self.types.else_ && self.else_ends == Some(self.open.len()) {
                self.pos = start;
                return Ok(Some(Boundary::Else));
            }
            if let Some(mode) = self.lex_mode {
                if mode != RightSide::Condition && id == self.types.open_ && !tape.is_empty() {
                    // In a range a `(` after an unconstructed identity with
                    // a constructor is that identity's (DESIGN ›`X (…)` is one
                    // spelling, and X's constructor decides‹); a return type takes no bracket, so there the first `(` is the body.
                    let owner_pending = mode == RightSide::Range
                        && matches!(tape.last(), Some(l) if !self.is_operand_cell(l));
                    if !owner_pending {
                        self.pos = start;
                        return Ok(Some(Boundary::Open));
                    }
                }
            }
        }
        if self.lex_mode == Some(RightSide::Condition) && self.condition_ends(tape, Some(&cell)) {
            self.pos = start;
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
                let asleep = (self.lex_mode == Some(RightSide::ReturnType) && reader)
                    || self.member_asleep
                    || self.tight_read_takes(id);
                if prec >= crate::identities::meta::prec::OPEN
                    && !asleep
                    && !(lazy && !crate::identities::meta::prec::built_as_lexed(prec))
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
                let asleep = (self.lex_mode == Some(RightSide::ReturnType) && reader)
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
        if offset <= 0 {
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
    fn construct_segment(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Vec<(DyadPtr, usize)>, ParseError> {
        let comment_ = self.types.comment_;
        let prose: Vec<(usize, usize, DyadPtr)> = tape
            .iter()
            // SAFETY: a constructed cell holds a node from the store.
            .filter(|(_, c)| c.constructed && unsafe { (*c.dyad).ty } == comment_)
            .map(|(n, c)| (n, c.start, c.dyad))
            .collect();
        for (n, start, d) in prose {
            self.lifted.push((start, d));
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
            let Some((n, _, construct, id)) = best else { break };
            tape.center_on(n);
            self.run_ctor(construct, id, tape, false)?;
        }
        let cells: Vec<Cell> = tape.iter().map(|(_, c)| *c).collect();
        let mut items = Vec::with_capacity(cells.len());
        for cell in cells {
            let start = cell.start;
            items.push((self.as_operand(cell)?, start));
        }
        Ok(items)
    }

    /// The cells up to the next `(`, the right side an identity reads before
    /// its bracket: a condition, a range, a return type (DESIGN ›The scope's
    /// constructor is the driver‹). A `(` standing first is part of the read; a later one is the bracket.
    fn drive_until_open(&mut self, mode: RightSide) -> Result<Vec<(DyadPtr, usize)>, ParseError> {
        let mut tape = ParsingTape::new();
        self.lex_segment_until(&mut tape, Some(mode))?;
        self.construct_segment(&mut tape)
    }

    /// One segment, lexed to the next `,`, `)`, or end of input (left
    /// unconsumed) and constructed to exactly one cell: what a discovery-time
    /// constructor drives, and the REPL's line.
    pub fn parse_expression(&mut self) -> Result<DyadPtr, ParseError> {
        let mut tape = ParsingTape::new();
        self.lex_segment(&mut tape)?;
        let items = self.construct_segment(&mut tape)?;
        self.one_of(items)
    }

    /// None is an empty expression, more than one the leftover cell, reported
    /// at the second.
    fn one_of(&mut self, items: Vec<(DyadPtr, usize)>) -> Result<DyadPtr, ParseError> {
        match items.len() {
            0 => Err(ParseError::Empty),
            1 => Ok(items[0].0),
            _ => {
                self.pos = items[1].1;
                Err(self.leftover_error(items[0].0))
            }
        }
    }

    /// Usually `Trailing`; where the first leftover is a box holding a node,
    /// the real reason is that the box could not say what it holds (a deferred
    /// body), so `a 5` never juxtaposed.
    fn leftover_error(&self, first: DyadPtr) -> ParseError {
        // SAFETY: `first` is a constructed node from the store.
        let node_box = unsafe {
            let d = self.types.through(first);
            !d.is_null()
                && (((*d).ty == self.types.type_ || (*d).ty == self.types.dyad_)
                    && crate::dyad::is_place((*d).value)
                    || !crate::identities::is_type_value(self.types, d)
                        && crate::identities::yields_type(self.types, d))
        };
        if node_box {
            ParseError::TypeKnownOnlyAtRun
        } else {
            ParseError::Trailing
        }
    }
}

/// Where a segment stopped, none consumed by the lexing step.
enum Boundary {
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
enum RightSide {
    /// `for`'s range: a bracket after a pending identity is that identity's.
    Range,
    /// The first bracket is the body, and an identity that reads its own
    /// bracket is not woken.
    ReturnType,
    /// `if`'s and `while`'s: ends at the first complete expression; see `condition_ends`.
    Condition,
}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)] // a test reads the nodes it built a line above
mod tests {
    use super::*;

    /// A binding dyad for `identity`, leaked like the dyads below.
    fn rec(identity: DyadPtr) -> DyadPtr {
        rec_in(identity, std::ptr::null_mut())
    }

    fn rec_in(identity: DyadPtr, scope: DyadPtr) -> DyadPtr {
        let fields = Box::into_raw(Box::new(Binding::new(identity, scope, std::ptr::null_mut())));
        Box::into_raw(Box::new(crate::dyad::Dyad {
            ty: std::ptr::null_mut(),
            value: fields as *mut u8,
        }))
    }

    /// The fields behind a binding dyad.
    fn f(binding: DyadPtr) -> Binding {
        // SAFETY: only `rec`-built dyads reach the trie in these tests.
        unsafe { Binding::read(binding) }
    }

    /// A distinct sentinel address per tag, never dereferenced.
    fn dyad(tag: usize) -> DyadPtr {
        std::ptr::without_provenance_mut(tag)
    }

    fn dyad_cells(tags: &[usize]) -> Vec<Cell> {
        tags.iter().map(|&t| unsafe { Cell::built(dyad(t)) }).collect()
    }

    #[test]
    fn offset_indexing_is_cursor_relative() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12, 13]));
        t.set_cursor(2);
        assert_eq!(t.at(0).unwrap().dyad, dyad(12));
        assert_eq!(t.at(-1).unwrap().dyad, dyad(11));
        assert_eq!(t.at(1).unwrap().dyad, dyad(13));
        assert_eq!(t.at(-2).unwrap().dyad, dyad(10));
        assert!(t.at(2).is_none());
        assert!(t.at(-3).is_none());
        assert_eq!(t.cursor(), 2);
    }

    #[test]
    fn insert_left_keeps_cursor_on_same_cell() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1);
        t.insert(0, unsafe { Cell::built(dyad(99)) });
        assert_eq!(t.at(0).unwrap().dyad, dyad(11));
        assert_eq!(t.at(-1).unwrap().dyad, dyad(99));
        assert_eq!(t.len(), 4);
        assert_eq!(t.cursor(), 2);
    }

    #[test]
    fn insert_right_leaves_cursor() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1);
        t.insert(1, unsafe { Cell::built(dyad(99)) });
        assert_eq!(t.at(0).unwrap().dyad, dyad(11));
        assert_eq!(t.at(1).unwrap().dyad, dyad(99));
        assert_eq!(t.at(2).unwrap().dyad, dyad(12));
    }

    #[test]
    fn remove_left_keeps_cursor_on_same_cell() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(2);
        let gone = t.remove(-1);
        assert_eq!(gone.unwrap().dyad, dyad(11));
        assert_eq!(t.at(0).unwrap().dyad, dyad(12));
        assert_eq!(t.at(-1).unwrap().dyad, dyad(10));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn removing_the_center_moves_it_to_the_next_cell_or_past_the_end() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1);
        t.remove(0);
        assert_eq!(t.at(0).unwrap().dyad, dyad(12));
        t.remove(0);
        assert!(t.at(0).is_none(), "past the end");
        assert_eq!(t.at(-1).unwrap().dyad, dyad(10), "the tail is still behind the center");
        assert_eq!(t.cursor(), t.len());
        t.insert(1, unsafe { Cell::built(dyad(7)) });
        assert_eq!(t.last().unwrap().dyad, dyad(7));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn unconstructed_and_constructed_cells_coexist() {
        // Told apart by the tape's own flag, never by the dyad.
        let mut t = ParsingTape::new();
        t.insert(0, unsafe { Cell::lexed(dyad(3), "abc", 0, 3) });
        t.insert(1, unsafe { Cell::built(dyad(7)) });
        assert_eq!(t.is_constructed(0), Some(false));
        assert_eq!(t.is_constructed(1), Some(true));
        assert_eq!(t.own_text(), Some("abc"));
        assert_eq!(t.spelling(1), Some(""), "a built cell answers the empty text");
        assert_eq!(t.at(1).unwrap().dyad, dyad(7));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn a_cell_is_rewritable_in_place_until_placed() {
        let mut t = ParsingTape::from_cells(vec![
            unsafe { Cell::lexed(dyad(1), "a b c", 0, 1) },
            unsafe { Cell::lexed(dyad(2), "a b c", 2, 1) },
            unsafe { Cell::lexed(dyad(3), "a b c", 4, 1) },
        ]);
        t.set_cursor(1);
        t.at_mut(1).unwrap().len = 2;
        assert_eq!(t.at(1).unwrap().len, 2);
        t.reduce_here(dyad(9));
        assert_eq!(t.len(), 1);
        let c = *t.at(0).unwrap();
        assert!(c.constructed);
        assert_eq!((c.dyad, c.start, c.len), (dyad(9), 0, 6));
        t.recenter(0);
        assert_eq!(t.cursor(), 0);
    }

    /// Parse one expression and run it on a runtime with the lexer attached.
    fn go(
        src: &str,
        store: &mut crate::store::Store,
        trie: &mut crate::regex_trie::RegexTrie,
        types: &Core,
        scopes: ScopeStack,
    ) -> (i64, ScopeStack) {
        let mut p = Parser::new(src, store, trie, types, scopes);
        let node = p.parse_expression().unwrap_or_else(|e| panic!("{src}: {e:?}"));
        let scopes = p.into_scopes();
        let mut rt = crate::run::Runtime::new(types, store);
        // SAFETY: `node` was just parsed into the store.
        let v = rt.hosting(&scopes, trie, None, |rt| unsafe { rt.run(node) });
        (v.unwrap_or_else(|e| panic!("{src}: {e:?}")), scopes)
    }

    #[test]
    fn a_scope_carries_its_enclosing_scope() {
        let mut store = crate::store::Store::new();
        let mut trie = crate::regex_trie::RegexTrie::new();
        let core = crate::identities::Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let mut p = Parser::new("(1, 2, (3, 4))", &mut store, &mut trie, types, scopes);
        let outer = p.parse_expression().unwrap();
        // SAFETY: the nodes were just parsed into the store.
        unsafe {
            use crate::identities::scope::{exprs_of, parent_of};
            assert_eq!((*outer).ty, types.scope);
            let inner = exprs_of(outer).expect("a sequence")[2];
            assert_eq!((*inner).ty, types.scope);
            assert_eq!(parent_of(inner), outer);
            assert_eq!(parent_of(outer), core.root_scope);
            assert!(parent_of(core.root_scope).is_null(), "the arche encloses nothing");
        }
    }

    #[test]
    fn here_is_where_the_line_is_written() {
        let mut store = crate::store::Store::new();
        let mut trie = crate::regex_trie::RegexTrie::new();
        let core = crate::identities::Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let (root, scopes) = go("here.scope", &mut store, &mut trie, types, scopes);
        assert_eq!(root as usize, core.root_scope as usize);
        let (above, scopes) = go("here.scope.back", &mut store, &mut trie, types, scopes);
        assert_eq!(above, 0, "the arche has no enclosing scope");
        // Past the arche is the checked error, never a dereference.
        let mut p = Parser::new("here.scope.back.back", &mut store, &mut trie, types, scopes);
        let node = p.parse_expression().unwrap();
        let scopes = p.into_scopes();
        let mut rt = crate::run::Runtime::new(types, &mut store);
        // SAFETY: `node` was just parsed into the store.
        let err = rt.hosting(&scopes, &trie, None, |rt| unsafe { rt.run(node) }).unwrap_err();
        assert_eq!(err, crate::run::RunError::NullPointer);
        // `caller.scope` outside a constructor is the checked error.
        let mut p = Parser::new("caller.scope", &mut store, &mut trie, types, scopes);
        let node = p.parse_expression().unwrap();
        let scopes = p.into_scopes();
        let mut rt = crate::run::Runtime::new(types, &mut store);
        // SAFETY: `node` was just parsed into the store.
        let err = rt.hosting(&scopes, &trie, None, |rt| unsafe { rt.run(node) }).unwrap_err();
        assert_eq!(err, crate::run::RunError::NoCaller);
    }

    #[test]
    fn lex_hands_back_the_tape_unconstructed() {
        use crate::identities::Core;
        use crate::regex_trie::RegexTrie;
        use crate::store::Store;

        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let plus = lex_fragment(&scopes, &trie, &mut store, "+").unwrap();
        assert_eq!(plus.len(), 1);
        let cell = *plus.at(0).unwrap();
        assert!(!cell.constructed);
        assert_eq!(cell.identity(types), types.plus, "the cell points at `+`'s binding");
        assert!(!cell.binding(types).is_null());
        assert_eq!(plus.spelling(0), Some("+"));

        let group = lex_fragment(&scopes, &trie, &mut store, "(a, b)").unwrap();
        let cells = group.cells();
        assert_eq!(cells.len(), 5, "a group is its tokens, the bracket not woken");
        assert!(cells.iter().all(|c| !c.constructed));
        assert_eq!(cells[0].identity(types), types.open_);
        assert_eq!(cells[2].identity(types), types.sep_);
        assert_eq!(cells[4].identity(types), types.close_);
        assert!(cells[1].is_fresh() && cells[3].is_fresh(), "names nothing declared are fresh");
        assert_eq!(cells[1].spelling(), "a");
        assert_eq!(cells[3].spelling(), "b");
        assert_eq!(group.cursor(), 0, "the fragment is centered on its first cell");

        let five = lex_fragment(&scopes, &trie, &mut store, " 5 ").unwrap();
        assert_eq!(five.len(), 1);
        assert_eq!(five.at(0).unwrap().identity(types), types.rational);
        assert_eq!(five.spelling(0), Some("5"), "`lex «5»` carries «5»");

        let none = lex_fragment(&scopes, &trie, &mut store, "  ").unwrap();
        assert!(none.is_empty());
        assert!(
            lex_fragment(&scopes, &trie, &mut store, "«").is_err(),
            "nothing spells a lone quote mark"
        );
    }

    #[test]
    fn a_handle_outlives_the_center_and_counts_edits() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1);
        let own = t.mark();
        let edits = t.edits();
        t.recenter(1);
        assert_eq!(t.cell_at_mark(own).map(|c| c.dyad), Some(dyad(11)));
        assert_eq!(t.edits(), edits, "moving the center is not an edit");
        t.restore(own);
        assert_eq!(t.at(0).unwrap().dyad, dyad(11));
        t.set_dyad(0, dyad(7));
        assert!(t.cell_at_mark(own).unwrap().constructed, "a write keeps the flag");
        assert_eq!(t.cell_at_mark(own).unwrap().dyad, dyad(7));
        assert_eq!(t.edits(), edits + 1, "a write is an edit");
        t.set_constructed(0, false);
        assert!(!t.cell_at_mark(own).unwrap().constructed, "the flag is its own write");
        assert_eq!(t.edits(), edits + 2, "a flag write is an edit");
        t.insert(1, unsafe { Cell::built(dyad(8)) });
        assert_eq!(t.edits(), edits + 3, "a splice is an edit");
        t.remove(0);
        assert!(t.cell_at_mark(own).is_none(), "a removed cell is gone by its handle");
        assert_eq!(t.edits(), edits + 4);
    }

    #[test]
    fn a_fragment_splices_in_order_with_its_spellings() {
        let frag = vec![unsafe { Cell::lexed(dyad(1), "x y", 0, 1) }, unsafe {
            Cell::lexed(dyad(2), "x y", 2, 1)
        }];
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11]));
        t.splice(1, frag.clone());
        let ids: Vec<_> = t.cells().iter().map(|c| c.dyad).collect();
        assert_eq!(ids, vec![dyad(10), dyad(1), dyad(2), dyad(11)]);
        assert_eq!(t.spelling(1), Some("x"));
        assert_eq!(t.spelling(2), Some("y"));
        assert_eq!(t.cursor(), 0, "the center keeps pointing at the same cell");

        let mut t = ParsingTape::from_cells(dyad_cells(&[10]));
        t.splice(0, frag.clone());
        let ids: Vec<_> = t.cells().iter().map(|c| c.dyad).collect();
        assert_eq!(ids, vec![dyad(1), dyad(2), dyad(10)]);

        let mut empty = ParsingTape::new();
        empty.splice(3, frag.clone());
        assert_eq!(empty.len(), 2);
        assert_eq!(empty.cursor(), 0, "on an empty tape the first cell becomes the center");

        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11]));
        let own = t.cells();
        t.splice(2, own);
        let ids: Vec<_> = t.cells().iter().map(|c| c.dyad).collect();
        assert_eq!(ids, vec![dyad(10), dyad(11), dyad(10), dyad(11)]);
    }

    #[test]
    fn a_logos_constructor_runs_when_its_identity_appears() {
        // A postfix `squared` whose constructor is written in Logos, one above `*`'s parse_rank.
        use crate::identities::Core;
        use crate::regex_trie::RegexTrie;
        use crate::store::Store;

        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let (_, s) =
            go("sq := fn (a := i32 ?) -> i32 ( a * a )", &mut store, &mut trie, types, scopes);
        let (_, s) = go(
            "squared := type ( \
                fields = ( a := i32 ?, output := type ?, shared run = ( sq(this.a) ) ), \
                parse_rank = *.parse_rank + 1, \
                parse = ( this.a = tape[-1], this.output = i32, tape[0] = this, \
                          tape.is_constructed[0] = true, tape.remove(-1) ) )",
            &mut store,
            &mut trie,
            types,
            s,
        );

        let (_, s) = go("x := i32 5", &mut store, &mut trie, types, s);
        let (v, s) = go("x squared", &mut store, &mut trie, types, s);
        assert_eq!(v, 25);
        let (v, s) = go("x squared + 1", &mut store, &mut trie, types, s);
        assert_eq!(v, 26, "squared binds tighter than +");
        let (v, s) = go("2 * x squared", &mut store, &mut trie, types, s);
        assert_eq!(v, 50, "and tighter than *");
        let (_, s) =
            go("f := fn (y := i32 ?) -> i32 ( y squared + 1 )", &mut store, &mut trie, types, s);
        let (v, _s) = go("f(3)", &mut store, &mut trie, types, s);
        assert_eq!(v, 10, "the node a constructor built calls like any other");
    }

    #[test]
    fn a_constructor_writes_a_cell_from_logos() {
        // The write and the flag are two lines: `t[0] = g` repoints, `t.is_constructed[0] = true` marks.
        use crate::binding::Binding;
        use crate::identities::Core;
        use crate::regex_trie::RegexTrie;
        use crate::store::Store;

        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let tape = {
            let mut tape = ParsingTape::new();
            let mut p = Parser::new("a + b", &mut store, &mut trie, types, scopes);
            p.lex_segment(&mut tape).unwrap();
            scopes = p.into_scopes();
            tape.set_cursor(1);
            Box::into_raw(Box::new(tape))
        };
        // SAFETY: `tape` is a live box the natives write through.
        unsafe {
            let plus = (*tape).at(0).unwrap().dyad;
            let storage = store.alloc_bytes(&(tape as usize as u64).to_ne_bytes());
            // Marked as storage, as the parser marks every place it allocates.
            let t = store.alloc_raw(core.tape.parsing_tape, crate::dyad::global_place(storage));
            let rec = Binding::alloc(
                &mut store,
                core.binding_,
                Binding::new(t, core.root_scope, std::ptr::null_mut()),
            );
            scopes.declare(&mut trie, "t", rec).unwrap();

            let (v, s) = go("t[0]:type", &mut store, &mut trie, types, scopes);
            assert_eq!(v as DyadPtr, core.type_, "an identity's type is the root");

            let (_, s) = go(
                "g := fn (p := i32 ?, q := i32 ?) -> i32 ( p + q )",
                &mut store,
                &mut trie,
                types,
                s,
            );
            let (_, s) = go("t[0] = g", &mut store, &mut trie, types, s);
            let c = *(*tape).at(0).unwrap();
            assert!(!c.constructed, "a write never sets the flag");
            assert_ne!(c.dyad, plus, "the pointer is replaced");
            assert_eq!(
                types.through(c.dyad),
                types.through(s.resolve(&trie, "g").unwrap().binding),
                "the cell now names g"
            );
            let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
            assert_eq!(v, 0);
            let (_, s) = go("t.is_constructed[0] = true", &mut store, &mut trie, types, s);
            assert!((*tape).at(0).unwrap().constructed, "the flag is the constructor's line");
            let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
            assert_eq!(v, 1);

            let (_, s) = go("t.remove(1)", &mut store, &mut trie, types, s);
            let (_, _s) = go("t.remove(-1)", &mut store, &mut trie, types, s);
            assert_eq!((*tape).len(), 1);

            drop(Box::from_raw(tape));
        }
    }

    #[test]
    fn the_tape_affordances_are_reachable_from_logos() {
        use crate::binding::Binding;
        use crate::identities::Core;
        use crate::regex_trie::RegexTrie;
        use crate::store::Store;

        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let tape = {
            let mut tape = ParsingTape::new();
            let mut p = Parser::new("a + b", &mut store, &mut trie, types, scopes);
            p.lex_segment(&mut tape).unwrap();
            scopes = p.into_scopes();
            tape.set_cursor(0);
            Box::into_raw(Box::new(tape))
        };
        // SAFETY: `tape` is a live box; the natives and the checks below read through the same handle.
        unsafe {
            assert_eq!((*tape).len(), 3);
            let plus = (*tape).at(1).unwrap().dyad;
            assert!(!(*tape).at(1).unwrap().constructed, "a lexed cell is unconstructed");

            let storage = store.alloc_bytes(&(tape as usize as u64).to_ne_bytes());
            // Marked as storage, as the parser marks every place it allocates.
            let t = store.alloc_raw(core.tape.parsing_tape, crate::dyad::global_place(storage));
            let rec = Binding::alloc(
                &mut store,
                core.binding_,
                Binding::new(t, core.root_scope, std::ptr::null_mut()),
            );
            scopes.declare(&mut trie, "t", rec).unwrap();

            let (v, s) = go("t.is_constructed[1]", &mut store, &mut trie, types, scopes);
            assert_eq!(v, 0);
            let (v, s) = go("t.spelling[1]", &mut store, &mut trie, types, s);
            assert_eq!(crate::identities::string::text(v as DyadPtr), b"+");
            let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
            assert_eq!(crate::identities::string::text(v as DyadPtr), b"a");
            let (v, s) = go("t[1]:name", &mut store, &mut trie, types, s);
            assert_eq!(crate::identities::string::text(v as DyadPtr), b"+");
            let (v, s) = go("t.remove(1)", &mut store, &mut trie, types, s);
            assert_eq!(v as DyadPtr, plus, "remove yields the cell it took");
            assert_eq!((*tape).len(), 2);

            let (a_rec, b_rec) = ((*tape).at(0).unwrap().dyad, (*tape).at(1).unwrap().dyad);
            let (_, s) = go("i := i64 1", &mut store, &mut trie, types, s);
            let (v, s) = go("t[i]", &mut store, &mut trie, types, s);
            assert_eq!(v as DyadPtr, b_rec, "an index may be a name");
            let (_, s) = go("t.recenter(1)", &mut store, &mut trie, types, s);
            let (v, s) = go("t[-1]", &mut store, &mut trie, types, s);
            assert_eq!(v as DyadPtr, a_rec, "a negative index reads the left context");
            let (v, s) = go("t[i - 2]", &mut store, &mut trie, types, s);
            assert_eq!(v as DyadPtr, a_rec, "an index may be an expression");
            let (_, s) = go("t.recenter(-1)", &mut store, &mut trie, types, s);

            let (_, s) = go("t[0] = dyad (i32, 7)", &mut store, &mut trie, types, s);
            let c0 = *(*tape).at(0).unwrap();
            assert!(!c0.constructed, "a write replaces the pointer and nothing more");
            assert_eq!((*c0.dyad).ty, core.i32_);
            let (v, s) = go("t[0]", &mut store, &mut trie, types, s);
            assert_eq!(v as DyadPtr, c0.dyad, "the element read yields the cell");
            let (_, s) = go("t.is_constructed[0] = true", &mut store, &mut trie, types, s);
            let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
            assert_eq!(v, 1, "the flag is the constructor's own write");
            let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
            assert_eq!(
                crate::identities::string::text(v as DyadPtr),
                b"a",
                "the text stays after the cell is constructed"
            );

            let (_, s) = go("t.insert(0, lex «9»)", &mut store, &mut trie, types, s);
            assert_eq!((*tape).len(), 3);
            assert_eq!((*tape).at(0).unwrap().dyad, c0.dyad, "the center stays on its cell");
            let (_, s) = go("t.recenter(-1)", &mut store, &mut trie, types, s);
            let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
            assert_eq!(v, 0, "a spliced cell is unconstructed");
            let (v, s) = go("t[0]", &mut store, &mut trie, types, s);
            assert_eq!((*(v as DyadPtr)).ty, core.binding_, "the spliced cell is a binding");
            assert_eq!(types.through(v as DyadPtr), core.rational, "the number pattern's");
            let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
            assert_eq!(
                crate::identities::string::text(v as DyadPtr),
                b"9",
                "a spliced cell keeps the text it was lexed from"
            );
            let (_, s) = go("t[0] = dyad (i32, 9)", &mut store, &mut trie, types, s);
            let (v, s) = go("t[0]", &mut store, &mut trie, types, s);
            assert_eq!((*(v as DyadPtr)).ty, core.i32_, "the written cell, now the center");
            assert_ne!(v as DyadPtr, c0.dyad);
            let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
            assert_eq!(crate::identities::string::text(v as DyadPtr), b"9");
            let mut p = Parser::new("t.insert(0, dyad (i32, 9))", &mut store, &mut trie, types, s);
            assert_eq!(p.parse_expression().unwrap_err(), ParseError::InsertTakesTape);
            let s = p.into_scopes();

            // A use of a name handed in is its binding; the flag is untouched.
            let (_, s) = go("t[0] = t", &mut store, &mut trie, types, s);
            assert!(!(*tape).at(0).unwrap().constructed);
            assert_eq!((*tape).at(0).unwrap().dyad, rec);

            // Through a function taking the tape as a pointer parameter.
            let (_, s) = go(
                "g := fn (p := @parsing_tape ?) -> void ( p@.remove(0) )",
                &mut store,
                &mut trie,
                types,
                s,
            );
            let (_, _s) = go("g(&t)", &mut store, &mut trie, types, s);
            assert_eq!((*tape).len(), 2);

            drop(Box::from_raw(tape));
        }
    }

    #[test]
    fn resolves_a_name_declared_in_an_open_scope() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        let id = dyad(1);
        unsafe { scopes.declare(&mut trie, "a", rec(id)) }.unwrap();
        assert_eq!(scopes.resolve(&trie, "a").unwrap().identity, id);
    }

    #[test]
    fn same_name_in_sibling_scopes_resolves_the_open_one() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let (outer, inner) = (dyad(100), dyad(101));

        scopes.push(outer);
        unsafe { scopes.declare(&mut trie, "x", rec(dyad(1))) }.unwrap();
        scopes.pop();

        scopes.push(inner);
        unsafe { scopes.declare(&mut trie, "x", rec(dyad(2))) }.unwrap();
        assert_eq!(scopes.resolve(&trie, "x").unwrap().identity, dyad(2));

        scopes.pop();
        scopes.push(outer);
        assert_eq!(scopes.resolve(&trie, "x").unwrap().identity, dyad(1));
    }

    #[test]
    fn out_of_scope_is_distinct_from_unknown() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        unsafe { scopes.declare(&mut trie, "y", rec(dyad(1))) }.unwrap();
        scopes.pop();

        assert_eq!(scopes.resolve(&trie, "y"), Err(ResolveError::OutOfScope("y".into())));
        assert_eq!(scopes.resolve(&trie, "nope"), Err(ResolveError::Unknown("nope".into())));
    }

    #[test]
    fn shadowing_is_rejected() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let (outer, inner) = (dyad(100), dyad(101));

        scopes.push(outer);
        unsafe { scopes.declare(&mut trie, "a", rec(dyad(1))) }.unwrap();
        assert_eq!(
            unsafe { scopes.declare(&mut trie, "a", rec(dyad(2))) },
            Err(ResolveError::Shadowed("a".into()))
        );
        scopes.push(inner);
        assert_eq!(
            unsafe { scopes.declare(&mut trie, "a", rec(dyad(3))) },
            Err(ResolveError::Shadowed("a".into()))
        );
    }

    #[test]
    fn rollback_undoes_journalled_declarations() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        unsafe { scopes.declare(&mut trie, "keep", rec(dyad(1))) }.unwrap();
        scopes.commit();
        unsafe { scopes.declare(&mut trie, "gone", rec(dyad(2))) }.unwrap();

        scopes.rollback(&mut trie);
        assert_eq!(scopes.resolve(&trie, "keep").unwrap().identity, dyad(1));
        assert_eq!(scopes.resolve(&trie, "gone"), Err(ResolveError::Unknown("gone".into())));
        unsafe { scopes.declare(&mut trie, "gone", rec(dyad(3))) }.unwrap();
        assert_eq!(scopes.resolve(&trie, "gone").unwrap().identity, dyad(3));
    }

    #[test]
    fn rebind_points_a_spelling_at_the_original_identity() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        let alias = rec(dyad(1));
        unsafe { scopes.declare(&mut trie, "alias", alias) }.unwrap();
        unsafe { scopes.rebind(alias, dyad(2)) };
        assert_eq!(scopes.resolve(&trie, "alias").unwrap().identity, dyad(2));
        // The declare's journal entry still covers the rebound binding.
        scopes.rollback(&mut trie);
        assert_eq!(scopes.resolve(&trie, "alias"), Err(ResolveError::Unknown("alias".into())));
    }

    #[test]
    fn truncate_restores_a_known_depth() {
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        scopes.push(dyad(101));
        scopes.push(dyad(102));
        scopes.truncate(1);
        assert_eq!(scopes.depth(), 1);
        assert_eq!(scopes.current(), Some(dyad(100)));
        assert!(!scopes.is_open(dyad(101)));
    }

    #[test]
    fn a_dead_name_resolves_as_dead_and_may_be_redeclared() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let scope = dyad(100);
        scopes.push(scope);
        let a1 = rec(dyad(1));
        unsafe { scopes.declare(&mut trie, "a", a1) }.unwrap();
        unsafe { scopes.mark_dead(a1, dyad(50)) };

        assert_eq!(scopes.resolve(&trie, "a"), Err(ResolveError::Dead("a".into())));
        unsafe { scopes.declare(&mut trie, "a", rec(dyad(2))) }.unwrap();
        assert_eq!(scopes.resolve(&trie, "a").unwrap().identity, dyad(2));
        // The dead entry is still indexed: its range is what reflection reads.
        let m = trie.get("a").unwrap();
        assert_eq!(m.bindings.len(), 2);
        assert!(m.bindings.iter().any(|&c| f(c).dyad == dyad(1) && f(c).end == dyad(50)));
    }

    #[test]
    fn rollback_restores_a_dead_mark() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let scope = dyad(100);
        scopes.push(scope);
        let a1 = rec(dyad(1));
        unsafe { scopes.declare(&mut trie, "a", a1) }.unwrap();
        scopes.commit();

        unsafe { scopes.mark_dead(a1, dyad(50)) };
        unsafe { scopes.declare(&mut trie, "a", rec(dyad(2))) }.unwrap();
        scopes.rollback(&mut trie);

        assert_eq!(scopes.resolve(&trie, "a").unwrap().identity, dyad(1));
        assert_eq!(trie.get("a").unwrap().bindings.len(), 1);
    }

    #[test]
    fn two_spellings_of_equal_rank_matching_the_same_length_are_a_tie() {
        // Two patterns entered as the core enters its own (raw keys).
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let sc = dyad(9);
        scopes.push(sc);
        trie.insert("a[0-9]", rec_in(dyad(1), sc));
        trie.insert("[a-z]1", rec_in(dyad(2), sc));
        assert_eq!(scopes.resolve(&trie, "a1"), Err(ResolveError::Tied));
        assert_eq!(scopes.resolve(&trie, "a2").unwrap().identity, dyad(1));
        assert_eq!(scopes.resolve(&trie, "b1").unwrap().identity, dyad(2));
        // A longer match at equal rank wins, as `:=` wins over `:`.
        trie.insert("a1x", rec_in(dyad(3), sc));
        assert_eq!(scopes.resolve(&trie, "a1x").unwrap().identity, dyad(3));
    }

    #[test]
    fn settle_patches_start_and_end_to_the_body_item() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let scope = dyad(100);
        scopes.push(scope);
        let a1 = rec(dyad(1));
        unsafe { scopes.declare(&mut trie, "a", a1) }.unwrap();
        let a = |trie: &RegexTrie| f(trie.get("a").unwrap().bindings[0]);
        assert!(a(&trie).start.is_null());

        unsafe { scopes.settle_item(scope, dyad(10)) };
        assert_eq!(a(&trie).start, dyad(10));
        assert!(a(&trie).end.is_null());

        unsafe { scopes.mark_dead(a1, dyad(50)) };
        assert_eq!(a(&trie).end, dyad(50), "provisional: the own/drop node");
        unsafe { scopes.settle_item(scope, dyad(11)) };
        assert_eq!(a(&trie).end, dyad(11), "settled: the body item");
        assert_eq!(a(&trie).start, dyad(10), "start untouched by the end's settle");

        // A rebind keeps the range, and the pending endpoint holds the binding.
        let b_rec = rec(dyad(3));
        unsafe { scopes.declare(&mut trie, "b", b_rec) }.unwrap();
        unsafe { scopes.rebind(b_rec, dyad(4)) };
        unsafe { scopes.settle_item(scope, dyad(12)) };
        let b = f(trie.get("b").unwrap().bindings[0]);
        assert_eq!((b.dyad, b.start), (dyad(4), dyad(12)));
    }

    #[test]
    fn a_barrier_between_a_name_and_the_current_scope_is_detected() {
        let mut scopes = ScopeStack::new();
        let (outer, body, inner) = (dyad(100), dyad(101), dyad(102));
        scopes.push(outer);
        scopes.push_barrier();
        scopes.push(body);
        scopes.push(inner);
        assert!(scopes.crosses_barrier(outer));
        assert!(!scopes.crosses_barrier(body));
        assert!(!scopes.crosses_barrier(inner));
        scopes.pop();
        scopes.pop();
        scopes.pop_barrier();
        assert!(!scopes.crosses_barrier(outer));
    }

    #[test]
    fn truncating_past_a_body_drops_its_barrier() {
        // Or every later top-level `own`/`drop` is refused for the rest of the session.
        let mut scopes = ScopeStack::new();
        let (outer, body) = (dyad(100), dyad(101));
        scopes.push(outer);
        scopes.push_barrier();
        scopes.push(body);
        assert!(scopes.crosses_barrier(outer));
        scopes.truncate(1);
        assert!(!scopes.crosses_barrier(outer), "the closed body's barrier is gone");
    }

    #[test]
    fn two_live_bindings_resolve_to_the_innermost_scope() {
        let mut trie = RegexTrie::new();
        let (a, b) = (dyad(100), dyad(101));
        trie.insert("z", rec_in(dyad(1), a));
        trie.insert("z", rec_in(dyad(2), b));

        let mut scopes = ScopeStack::new();
        scopes.push(a);
        scopes.push(b);
        assert_eq!(scopes.resolve(&trie, "z").map(|r| r.identity), Ok(dyad(2)));
        scopes.pop();
        assert_eq!(scopes.resolve(&trie, "z").map(|r| r.identity), Ok(dyad(1)));
    }
}
