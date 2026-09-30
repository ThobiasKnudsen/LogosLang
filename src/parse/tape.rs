// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The parsing tape: the cells a segment is lexed into and constructed over, each a
//! dyad plus the facts of its appearance (DESIGN ›The scope's constructor is the driver‹).

use super::*;
use crate::dyad;

/// One cell of the tape: a dyad pointer (a binding, the built node, or null for a fresh
/// spelling) plus the tape's own two facts, the flag and the lexed spelling
/// (DESIGN ›The scope's constructor is the driver‹).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The binding or the node; null for a fresh spelling.
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
    /// The bindings a field place was reached through, root first, an `array` node; null
    /// for a cell reached no such way. What a write into the place must be granted by.
    pub path: DyadPtr,
    /// The run type's node a comptime value was folded from, whose fields `.` still reads.
    pub origin: DyadPtr,
    /// The name and the field a `v.f` built as a write's target fills, or nulls.
    pub target: (DyadPtr, DyadPtr),
    /// The valueless place `?` built, no declaration having taken it yet.
    pub hole: bool,
    /// A hole `own` marked over a type whose body fills `free`: the name or field declared
    /// with it owns the node written into it.
    pub owning: bool,
}

impl Cell {
    const NO_FACTS: (DyadPtr, DyadPtr, (DyadPtr, DyadPtr), bool, bool) = (
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        (std::ptr::null_mut(), std::ptr::null_mut()),
        false,
        false,
    );

    fn facts_from(&mut self, from: &Cell) {
        self.path = from.path;
        self.origin = from.origin;
        self.target = from.target;
        self.hole = from.hole;
        self.owning = from.owning;
    }

    /// The cell placed over `dyad`, its facts cleared.
    fn over(&mut self, dyad: DyadPtr) {
        let (path, origin, target, hole, owning) = Self::NO_FACTS;
        self.dyad = dyad;
        self.constructed = true;
        self.bracket = false;
        self.path = path;
        self.origin = origin;
        self.target = target;
        self.hole = hole;
        self.owning = owning;
    }
}

impl Cell {
    /// # Safety
    /// `text` must outlive every read of the cell's spelling; `dyad` must be
    /// null (a fresh spelling) or a dyad from the store.
    pub unsafe fn lexed(dyad: DyadPtr, text: &str, start: usize, len: usize) -> Self {
        let (path, origin, target, hole, owning) = Self::NO_FACTS;
        Cell {
            dyad,
            constructed: false,
            bracket: false,
            text: Some(std::ptr::NonNull::from(text)),
            start,
            len,
            path,
            origin,
            target,
            hole,
            owning,
        }
    }

    /// # Safety
    /// `dyad` must be null or a dyad from the store.
    pub unsafe fn built(dyad: DyadPtr) -> Self {
        let (path, origin, target, hole, owning) = Self::NO_FACTS;
        Cell {
            dyad,
            constructed: true,
            bracket: false,
            text: None,
            start: 0,
            len: 0,
            path,
            origin,
            target,
            hole,
            owning,
        }
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

    /// The binding read through to its dyad, a node itself, or null for a fresh spelling.
    pub fn identity(&self, types: &Core) -> DyadPtr {
        // SAFETY: a cell's dyad is null or a dyad from the store.
        unsafe { types.through(self.dyad) }
    }

    /// Null when the cell holds no binding (a fresh spelling, a minted
    /// identity, a constructed node).
    pub fn binding(&self, types: &Core) -> DyadPtr {
        // SAFETY: a cell's dyad is null or a dyad from the store.
        let bound = !self.dyad.is_null() && unsafe { dyad::ty(self.dyad) } == types.binding_;
        if !self.constructed && bound {
            self.dyad
        } else {
            std::ptr::null_mut()
        }
    }

    /// A lexed spelling nothing has declared: no node, its text on the tape.
    pub fn is_fresh(&self) -> bool {
        !self.constructed && self.dyad.is_null() && self.text.is_some()
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
    pub(super) head: Option<usize>,
    tail: Option<usize>,
    /// `None` is the one-past-end position.
    pub(super) center: Option<usize>,
    len: usize,
    /// How many times the frontier was edited: what tells a decline from a
    /// constructor that edited the tape and left its own cell unconstructed.
    edits: u64,
    /// Lexed to its boundary: a read past the last cell finds nothing there
    /// (DESIGN ›The scope's constructor is the driver‹), so a return type's
    /// constructor does not reach the body bracket.
    pub(super) sealed: bool,
    /// The cells of the `{…}` scopes its quotes opened, lexed with them: no frontier holds
    /// them, a held body's construction reads them by position.
    pub(crate) inner: Vec<Cell>,
}

impl ParsingTape {
    pub fn new() -> Self {
        Self::default()
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

    /// A held body's cells in source order, its scopes' among them: what its feed serves.
    pub fn held_cells(&self) -> Vec<Cell> {
        let mut out = self.cells();
        out.extend_from_slice(&self.inner);
        out.sort_by_key(|c| c.start);
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
        self.at_mut(0).expect("the construct's cell is at the center").over(dyad);
        self.edits += 1;
    }

    pub fn place_bracket(&mut self, dyad: DyadPtr) {
        self.place(dyad);
        self.at_mut(0).expect("placed above").bracket = true;
    }

    /// A bracket standing as its one expression stands with that expression's facts.
    pub fn place_bracket_cell(&mut self, cell: Cell) {
        self.place_bracket(cell.dyad);
        self.at_mut(0).expect("placed above").facts_from(&cell);
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
