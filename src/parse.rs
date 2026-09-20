// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The parsing tape and the driver: the scope's constructor lexing and
//! constructing its segments (DESIGN ›The scope's constructor is the driver‹,
//! the eager-segment model ruled 30 August 2026; converged here in #59).
//!
//! The tape is a segment's working frontier — the cells lexed since the last
//! boundary, each a pending token, a constructed dyad, or a bracket — indexed
//! relative to the `cursor`, the cell of the identity being constructed:
//! offset 0 is the cursor, negative offsets reach left, positive right.
//! `insert`/`remove` splice the frontier and keep the cursor on the same cell,
//! which is the whole macro / custom-syntax mechanism.
//!
//! The driver ([`Parser::lex_segment`], [`Parser::construct_segment`]) lexes
//! one token at a time, constructing at discovery every identity at or above
//! `(` on the one parse_rank axis ([`crate::identities::meta::prec`]) — the
//! brackets, the literals, `#`, `import`, `:=`, and the identities that read
//! their own bracket or right side — and, at the segment boundary (`,`, the
//! closer, the end of input), constructs the rest highest parse_rank first,
//! associativity breaking ties, each constructor taking what its syntax needs
//! from the fully lexed segment, left or right, with no lookahead. A leftover
//! cell is the checked error; prose is lifted out beside the segment's
//! expression. Each identity's `constructor` ([`ConstructFn`]) edits the tape
//! in place and reports applied or declined; the driver decides only *when*.
//!
//! This module also holds the scope stack and name resolution over it. The
//! parser owns resolution; the trie ([`crate::regex_trie`]) is only the name
//! index. Open here: the slot words as root identities where the type body
//! alone should know them (DESIGN ›The constructor is a field‹, 19 September
//! 2026), the parser's per-identity state a Logos-written constructor cannot
//! reach (#97), the dispatch on identity handles where the record should
//! decide (#99), and the run body constructed per field-type set (#133).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::dyad::DyadPtr;
use crate::record::Record;
use crate::regex_trie::{RegexTrie, RegexTrieError};
use crate::store::Store;
use crate::Core;

/// One cell of the tape (DESIGN ›The scope's constructor is the driver‹, 2
/// and 7 September 2026): a pointer to a dyad — unconstructed, the **record**
/// the trie resolved for the spelling, or a fresh dyad with both slots null
/// for a spelling the trie does not know (`:=` fills it); constructed, the
/// node the constructor built — plus the tape's own two facts about the
/// cell, whether it is constructed and the text it was lexed from
/// (`tape.spelling[k]`, ruled 14 September 2026; the span doubles as the
/// derived source map's seed-side stand-in). The text rides on the cell so a
/// cell `lex «…»` lexed and `insert` spliced keeps its spelling on the tape
/// it lands on (#62). Never a token wrapper: the identity a cell denotes is
/// read through its record ([`Cell::identity`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The record, the fresh dyad, or the node.
    pub dyad: DyadPtr,
    /// The tape's own flag: `is_constructed` for this cell.
    pub constructed: bool,
    /// The tape's own mark that this constructed cell is the group a `(`
    /// landed, so the identity to its left may claim it (`X (…)` is X's
    /// decision). A seed stand-in: DESIGN lands every bracket as a scope cell,
    /// whose type would say so, but the seed still unwraps a one-expression
    /// group to its expression (`parse_sequence`), which loses that type; the
    /// mark goes when the unwrap goes.
    pub bracket: bool,
    /// The text the cell was lexed from — the parser's source, or the string
    /// a `lex «…»` ran over — which `start..start + len` indexes; `None` for
    /// a cell nothing lexed (a constructor-built or parser-minted one), which
    /// answers the empty spelling. The text outlives every read of the cell:
    /// a source lives for its parse, a string's bytes for the store.
    text: Option<std::ptr::NonNull<str>>,
    /// Byte offset of the cell's spelling in its text (0 for a cell that
    /// was never lexed).
    pub start: usize,
    /// Byte length of that spelling.
    pub len: usize,
}

impl Cell {
    /// An unconstructed cell pointing at `dyad`, lexed from `text` at
    /// `start..start + len`.
    ///
    /// # Safety
    /// `text` must outlive every read of the cell's spelling: it is the
    /// parser's source, an import's text (leaked for the run), or a string
    /// node's store-lived bytes. `dyad` must be null or a dyad from the store.
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

    /// A constructed cell holding `dyad`, with no spelling of its own.
    ///
    /// # Safety
    /// `dyad` must be null or a dyad from the store.
    pub unsafe fn built(dyad: DyadPtr) -> Self {
        Cell { dyad, constructed: true, bracket: false, text: None, start: 0, len: 0 }
    }

    /// The text this cell was lexed from — `tape.spelling[k]`, the tape's
    /// second per-cell fact beside the flag (DESIGN ›The scope's constructor
    /// is the driver‹, ruled 14 September 2026): the empty text for a cell
    /// nothing lexed, so a constructor can test for a lexed cell with one
    /// read and no fault. The text stays after the cell is constructed, since
    /// a write keeps the span.
    pub fn spelling(&self) -> &str {
        match self.text {
            // SAFETY: `lexed` took a `&str` that outlives the cell (its
            // contract), so the pointer is live for as long as `self` is.
            Some(p) => unsafe { p.as_ref() }.get(self.start..self.end()).unwrap_or(""),
            None => "",
        }
    }

    /// One past the cell's last byte.
    pub fn end(&self) -> usize {
        self.start + self.len
    }

    /// The identity this cell denotes — what the driver dispatches on: the
    /// record read through to its dyad, a fresh dyad itself, a node itself.
    pub fn identity(&self, types: &Core) -> DyadPtr {
        // SAFETY: a cell's dyad is null or a dyad from the store.
        unsafe { types.through(self.dyad) }
    }

    /// The record this cell points at, or null when it holds none (a fresh
    /// spelling, a parser-minted identity, a constructed node).
    pub fn record(&self, types: &Core) -> DyadPtr {
        // SAFETY: as above.
        if !self.constructed && !self.dyad.is_null() && unsafe { (*self.dyad).ty } == types.record_
        {
            self.dyad
        } else {
            std::ptr::null_mut()
        }
    }

    /// A spelling the trie did not know: an unconstructed cell whose fresh
    /// dyad has both slots null.
    pub fn is_fresh(&self) -> bool {
        // SAFETY: as above.
        !self.constructed && !self.dyad.is_null() && unsafe { (*self.dyad).ty }.is_null()
    }

    /// A bracket: the finished group `(`'s constructor leaves, which the
    /// identity to its left may claim (`X (…)` is X's decision). A *named*
    /// scope value is a record, so `f s` is no call.
    pub fn is_bracket(&self) -> bool {
        self.constructed && self.bracket
    }
}

/// A node of the tape's doubly linked list: a cell and its two links.
#[derive(Debug, Clone, Copy)]
struct Node {
    cell: Cell,
    prev: Option<usize>,
    next: Option<usize>,
    /// Whether the node is on the list: a removed node stays in the arena,
    /// unlinked, so a handle to it ([`ParsingTape::mark`]) can still say
    /// that its cell is gone.
    linked: bool,
}

/// The working frontier of a scope: a doubly linked list of cells with a
/// movable center (DESIGN ›The scope's constructor is the driver‹, ruled 8
/// September 2026 to be the list, not a vector): `tape[0]` is the construct's
/// own cell, negative offsets its left context, positive its right; `insert`
/// and `remove` at the center are constant-time splices, `tape[k]` walks k
/// links. Nodes live in an arena so handles stay valid across splices; a
/// removed node is simply unlinked. Absolute indices (`cell(i)`, `cursor()`,
/// `set_cursor(i)`) walk from the head and serve the boundary-time driver.
#[derive(Debug, Default)]
pub struct ParsingTape {
    nodes: Vec<Node>,
    head: Option<usize>,
    tail: Option<usize>,
    /// The center: `None` is the one-past-end position (after the last cell
    /// was removed, or on an empty tape).
    center: Option<usize>,
    len: usize,
    /// How many times the frontier was edited — a cell linked, unlinked,
    /// placed, or written. What tells a decline ("the frontier untouched",
    /// DESIGN ›The scope's constructor is the driver‹) from a constructor
    /// that edited the tape and left its own cell unconstructed (#81).
    edits: u64,
}

impl ParsingTape {
    /// An empty tape.
    pub fn new() -> Self {
        ParsingTape { nodes: Vec::new(), head: None, tail: None, center: None, len: 0, edits: 0 }
    }

    /// The edit count: unchanged across a call means the frontier was not
    /// touched.
    pub fn edits(&self) -> u64 {
        self.edits
    }

    /// The cell a handle from [`ParsingTape::mark`] names, or `None` when
    /// that cell has been removed since — how the driver judges a
    /// constructor's outcome on the construct's own cell, whatever the
    /// center is after the call (#81).
    pub fn cell_at_mark(&self, mark: Option<usize>) -> Option<&Cell> {
        let n = mark?;
        self.nodes[n].linked.then(|| &self.nodes[n].cell)
    }

    /// Replace the pointer of the cell at `offset` — a native's `tape[k] =
    /// …` — and nothing more: the flag stays, the spelling stays ("the text
    /// stays readable after the cell is constructed"; DESIGN, 19 September
    /// 2026: "the write replaces the pointer and nothing more"). `false` off
    /// the tape.
    pub fn set_dyad(&mut self, offset: isize, dyad: DyadPtr) -> bool {
        let Some(c) = self.at_mut(offset) else {
            return false;
        };
        c.dyad = dyad;
        c.bracket = false;
        self.edits += 1;
        true
    }

    /// Set the flag of the cell at `offset` — a native's `tape.is_constructed[k]
    /// = flag`, the constructor's own word that its cell is done (19
    /// September 2026). `false` off the tape.
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

    /// Number of cells currently on the tape.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True if the tape has no cells.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The node `offset` links from the center; `None` off either end. Past
    /// the end (no center), negative offsets walk back from the tail.
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

    /// The node at absolute index `i`, walking from the head.
    fn node_abs(&self, i: usize) -> Option<usize> {
        let mut n = self.head?;
        for _ in 0..i {
            n = self.nodes[n].next?;
        }
        Some(n)
    }

    /// The cell at cursor-relative `offset` (0 is the center), or `None` if
    /// out of range.
    pub fn at(&self, offset: isize) -> Option<&Cell> {
        self.node_at(offset).map(|n| &self.nodes[n].cell)
    }

    /// Mutable access to the cell at cursor-relative `offset`.
    pub fn at_mut(&mut self, offset: isize) -> Option<&mut Cell> {
        self.node_at(offset).map(move |n| &mut self.nodes[n].cell)
    }

    /// The cell at absolute index `i`, or `None` if out of range. Walks from
    /// the head: a loop over the tape uses [`ParsingTape::iter`] instead.
    pub fn cell(&self, i: usize) -> Option<&Cell> {
        self.node_abs(i).map(|n| &self.nodes[n].cell)
    }

    /// Every cell on the tape in order, each with the handle of its node,
    /// which [`ParsingTape::center_on`] takes: one walk of the list, where
    /// `cell(i)` for each `i` walked it once per cell (#92: the boundary
    /// driver was cubic in the segment's length because of that).
    pub(crate) fn iter(&self) -> impl Iterator<Item = (usize, &Cell)> + '_ {
        std::iter::successors(self.head, move |&n| self.nodes[n].next)
            .map(move |n| (n, &self.nodes[n].cell))
    }

    /// Move the center to the node `handle` names, a handle
    /// [`ParsingTape::iter`] handed out for a cell still on the tape.
    pub(crate) fn center_on(&mut self, handle: usize) {
        debug_assert!(self.nodes[handle].linked, "a handle names a cell on the tape");
        self.center = Some(handle);
    }

    /// The last cell on the tape, if any.
    pub fn last(&self) -> Option<&Cell> {
        self.tail.map(|n| &self.nodes[n].cell)
    }

    /// The source offset the cell at absolute index `i` was lexed at.
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

    /// Move the center to absolute index `i` (`len` and beyond: past the end).
    pub fn set_cursor(&mut self, i: usize) {
        self.center = self.node_abs(i);
    }

    /// Move the center `offset` links: the re-centered view a constructor
    /// hands to another (`tape.recenter(k)`).
    pub fn recenter(&mut self, offset: isize) {
        self.center = self.node_at(offset);
    }

    /// Whether the cell at `offset` is constructed, or `None` off the tape.
    pub fn is_constructed(&self, offset: isize) -> Option<bool> {
        self.at(offset).map(|c| c.constructed)
    }

    /// The text the cell at `offset` was lexed from — `tape.spelling[k]`
    /// ([`Cell::spelling`]) — or `None` off the tape.
    pub fn spelling(&self, offset: isize) -> Option<&str> {
        self.at(offset).map(Cell::spelling)
    }

    /// Every cell in tape order, copied out: the snapshot `splice` takes of
    /// a fragment, so a tape may be spliced into itself.
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

    /// Put the center back at `mark`.
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
                // At the end.
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

    /// The node an insertion at cursor-relative `offset` lands before: the
    /// cell there, the head past the left end, the end (`None`) past the
    /// right end or on a tape whose center is past the end.
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

    /// Splice `cell` in at cursor-relative `offset`: before the cell there,
    /// so `insert(0, ..)` lands just left of the center and `insert(1, ..)`
    /// just right of it; past either end it lands at that end. The center
    /// keeps pointing at the same cell.
    pub fn insert(&mut self, offset: isize, cell: Cell) {
        self.splice(offset, vec![cell]);
    }

    /// Splice `cells` in, in order, at cursor-relative `offset` — the place
    /// [`ParsingTape::insert`] puts one cell — with their flags and
    /// spellings: `tape.insert(k, lex «…»)`, "`insert` splices a tape into a
    /// tape" (DESIGN ›Text is the quote‹, 14 September 2026). On an empty
    /// tape the first cell becomes the center.
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

    /// Unlink and return the cell at cursor-relative `offset`. Removing the
    /// center moves it to the next cell (past the end if there is none);
    /// removing any other cell leaves it where it is.
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

    /// The construct's own spelling — the center cell's text, while the cell
    /// is unconstructed. How an atom constructor reaches its matched text.
    pub fn own_text(&self) -> Option<&str> {
        self.at(0).filter(|c| !c.constructed).map(Cell::spelling)
    }

    /// Replace the center cell's dyad with the node the constructor built and
    /// mark it constructed: the in-place edit nearly every constructor ends
    /// with (`tape[0] = dyad (…)` in the sketches). The span stays.
    pub fn place(&mut self, dyad: DyadPtr) {
        let cell = self.at_mut(0).expect("the construct's cell is at the center");
        cell.dyad = dyad;
        cell.constructed = true;
        cell.bracket = false;
        self.edits += 1;
    }

    /// Replace `(`'s own cell with the group it built, marked as a bracket.
    pub fn place_bracket(&mut self, dyad: DyadPtr) {
        self.place(dyad);
        self.at_mut(0).expect("placed above").bracket = true;
    }

    /// Reduce the triple around the center — `tape[-1]`, the construct's own
    /// cell, `tape[+1]` — to the single constructed `dyad`, spanning all
    /// three: an infix constructor's in-place splice.
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

/// A resolved name: how many source bytes it matched, the single record live
/// in the open scopes, the identity it names, and the scope it was declared in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// Whether the spelling is a fresh run — the index knows it only through
    /// one of the two fresh-spelling patterns (#110) — rather than a declared
    /// name; only [`ScopeStack::lex`] ever answers `true`.
    pub fresh: bool,
    /// Bytes consumed from the start of the input.
    pub matched: usize,
    /// The name's record (a dyad of type `record`) — what a use of the name
    /// points at (DESIGN ›The dyad's read surface‹, 8 September 2026).
    pub record: DyadPtr,
    /// The identity live in the open scopes: the record's dyad.
    pub identity: DyadPtr,
    /// The scope the winning declaration was made in (an open ancestor) — what
    /// a rebind that completes that declaration must target.
    pub scope: DyadPtr,
}

/// Why a name could not be resolved or declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// The spelling is not in the name index at all (an unknown token): the
    /// leading word at the position, or empty for no text at all.
    Unknown(String),
    /// The spelling is known, but none of its declarations is in an open scope:
    /// a genuine out-of-scope use, distinct from an unknown one.
    OutOfScope(String),
    /// More than one live candidate. Impossible under no-shadowing, so it signals
    /// a corrupt index.
    Ambiguous(String),
    /// Declaring the spelling would shadow a declaration of it that is still
    /// live in an open scope.
    Shadowed(String),
    /// The spelling is declared in an open scope, but an `own` or `drop` above
    /// made it dead: a read, a write, or a pass is refused, and only `:=` may
    /// follow (DESIGN ›Name resolution is scope-filtered‹, ruled 3 September
    /// 2026).
    Dead(String),
    /// Two declared spellings of equal `lex_rank` match the same length here:
    /// an inconsistency in the definitions, never a pick by declaration order
    /// (DESIGN ›The scope's constructor is the driver‹, ruled 10 September
    /// 2026). One of them must rank above the other.
    Tied,
    /// The name index itself rejected the lookup (e.g. a bad regex pattern).
    Index(RegexTrieError),
}

/// The spelling an unknown-name error names: the leading run of word
/// characters at the position, or the first character when the text starts
/// with something else. Nothing matched, so this is the reader's best guess at
/// what the writer meant as one name.
fn unknown_spelling(text: &str) -> String {
    let word = text.split(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("");
    if word.is_empty() {
        text.chars().next().map(String::from).unwrap_or_default()
    } else {
        word.to_string()
    }
}

/// One act on the name index since the last [`ScopeStack::commit`], undone
/// newest-first by [`ScopeStack::rollback`].
#[derive(Debug)]
enum Journal {
    /// `name` was declared in `scope`: rollback removes its live entry.
    Declared { name: String, scope: DyadPtr },
    /// `record` was made dead by an `own` or `drop`: rollback restores the
    /// `end` it had before.
    Ended { record: DyadPtr, prev_end: DyadPtr },
}

/// Which endpoint of an entry's range a [`Pending`] settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    Start,
    End,
}

/// A range endpoint waiting for the body item that carries it: the declaring
/// or ending line is still being parsed, so the entry holds a provisional
/// value (null for `start`, the `own`/`drop` node for `end`) until the
/// declaring scope appends the finished item ([`ScopeStack::settle_item`]).
#[derive(Debug)]
struct Pending {
    /// The record whose range awaits the item; its own `scope` says which
    /// scope's next item settles it.
    record: DyadPtr,
    endpoint: Endpoint,
}

/// A bare name an `own` or `drop` is about to make dead: its record, what
/// [`Parser::place_operand_cell`] hands back so the keyword's constructor can
/// call [`Parser::mark_dead`] with the node it built.
#[derive(Debug)]
pub(crate) struct Ended {
    pub(crate) record: DyadPtr,
}

/// The parse-time scope stack: the chain of open scopes with an O(1) membership
/// set — a cache over the parent link every scope node carries since #123
/// (DESIGN ›Meta-navigation‹: "the open-scope membership set is a cache over
/// the link, which the seed's `ScopeStack.open` is to become"); a scope is
/// identified by its dyad address. Resolution filters a
/// spelling's candidates in the name index down to the one whose declaring scope
/// is open and whose range covers the frontier (DESIGN ›Name resolution is
/// scope-filtered‹: live = declared, scope open, not yet made dead by `own` or
/// `drop`), and declaration enforces no-shadowing against it.
#[derive(Debug)]
pub struct ScopeStack {
    open: Vec<DyadPtr>,
    set: HashSet<DyadPtr>,
    /// Every declaration and every dead mark since the last
    /// [`ScopeStack::commit`]. The REPL's undo log: a failed line rolls them
    /// back ([`ScopeStack::rollback`]), so a typo never burns a name for the
    /// rest of the session and never leaves a moved name dead.
    journal: Vec<Journal>,
    /// Range endpoints awaiting their body item ([`ScopeStack::settle_item`]).
    pending: Vec<Pending>,
    /// Stack depths at which a body that runs again or later begins — a loop
    /// body or a `fn` body. An `own`/`drop` of a name declared below such a
    /// depth is the checked error of DESIGN ›Memory and concurrency‹ (*Bodies
    /// that run again or later*): the loop would read a dead name on its next
    /// pass, and a function may own only what its parameters hand it.
    barriers: Vec<usize>,
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

impl ScopeStack {
    /// An empty scope stack.
    pub fn new() -> Self {
        ScopeStack {
            open: Vec::new(),
            set: HashSet::new(),
            journal: Vec::new(),
            pending: Vec::new(),
            barriers: Vec::new(),
        }
    }

    /// Enter `scope`.
    pub fn push(&mut self, scope: DyadPtr) {
        self.open.push(scope);
        self.set.insert(scope);
    }

    /// Leave the innermost scope, returning it. Endpoints still pending for it
    /// can no longer settle (a record or parameter scope has no item loop) and
    /// are dropped.
    pub fn pop(&mut self) -> Option<DyadPtr> {
        let s = self.open.pop()?;
        self.set.remove(&s);
        // SAFETY: every pending record is a record dyad from the store.
        self.pending.retain(|p| unsafe { Record::read(p.record).scope } != s);
        Some(s)
    }

    /// Mark that a body which runs again or later — a loop body, a `fn` body —
    /// begins with the next scope pushed. Paired with [`ScopeStack::pop_barrier`].
    pub fn push_barrier(&mut self) {
        self.barriers.push(self.open.len());
    }

    /// Leave the innermost such body.
    pub fn pop_barrier(&mut self) {
        self.barriers.pop();
    }

    /// Whether a name declared in `scope` (an open ancestor) lies outside a
    /// body that runs again or later — i.e. a barrier began after `scope` was
    /// pushed — so that `own`/`drop` of it here is refused.
    pub fn crosses_barrier(&self, scope: DyadPtr) -> bool {
        let Some(idx) = self.position(scope) else {
            return false;
        };
        self.barriers.iter().any(|&b| b > idx)
    }

    /// The depth at which `scope` is open — its index from the root — or
    /// `None` when it is not on the stack.
    pub fn position(&self, scope: DyadPtr) -> Option<usize> {
        self.open.iter().position(|&s| s == scope)
    }

    /// The innermost open scope.
    pub fn current(&self) -> Option<DyadPtr> {
        self.open.last().copied()
    }

    /// Whether `scope` is currently open. O(1).
    pub fn is_open(&self, scope: DyadPtr) -> bool {
        self.set.contains(&scope)
    }

    /// Number of open scopes.
    pub fn depth(&self) -> usize {
        self.open.len()
    }

    /// Pop scopes until `depth` remain. An error propagating out of a nested
    /// parse skips the balancing pops; a caller that keeps the stack across
    /// parses (the REPL) restores its known depth with this. The barriers of
    /// the bodies closed this way go with them: a barrier left behind would
    /// stand between every later top-level scope and the names it declares,
    /// refusing `own`/`drop` for the rest of the session (#91).
    pub fn truncate(&mut self, depth: usize) {
        while self.open.len() > depth {
            self.pop();
        }
        // A barrier's value is the index of the body scope it was pushed for,
        // so it lives exactly while that scope is open.
        self.barriers.retain(|&b| b < depth);
    }

    /// Accept the journalled acts: they are permanent, the undo log can be
    /// dropped. Endpoints still pending belong to a top level that has no body
    /// array to settle against (the REPL session, the command line) and are
    /// dropped with it.
    pub fn commit(&mut self) {
        self.journal.clear();
        self.pending.clear();
    }

    /// Undo every act journalled since the last [`ScopeStack::commit`], newest
    /// first: a declaration is removed from the name index (by spelling *and*
    /// declaring scope, so outer declarations of the same spelling are
    /// untouched), and a dead mark is lifted, the entry's `end` restored.
    pub fn rollback(&mut self, trie: &mut RegexTrie) {
        while let Some(act) = self.journal.pop() {
            match act {
                Journal::Declared { name, scope } => {
                    // The entry was inserted by this journal's own declare; a
                    // failed removal means it was already pruned, which is fine.
                    let _ = trie.remove(&name, scope);
                }
                Journal::Ended { record, prev_end } => {
                    // SAFETY: a journalled record is a record dyad from the store.
                    unsafe { Record::set_end(record, prev_end) };
                }
            }
        }
        self.pending.clear();
    }

    /// Resolve `name` against `trie` to the single identity live in the open
    /// scopes: [`ResolveError::Unknown`] if nothing declared lexes there (a
    /// fresh spelling included), [`ResolveError::OutOfScope`] if a spelling is
    /// known but no declaration is open, [`ResolveError::Dead`] if the open
    /// declarations were all made dead by an `own` or `drop`,
    /// [`ResolveError::Ambiguous`] if more than one is live (a corrupt index,
    /// which no-shadowing otherwise makes impossible), and
    /// [`ResolveError::Tied`] for equal rank and length.
    pub fn resolve(&self, trie: &RegexTrie, name: &str) -> Result<Resolved, ResolveError> {
        self.select(trie, name, false)
    }

    /// Lex the spelling at the start of `text`: as [`ScopeStack::resolve`],
    /// but the two fresh-spelling patterns compete too, so a spelling nothing
    /// declared resolves with [`Resolved::fresh`] set and the run's length, and
    /// the lexer mints the fresh dyad.
    pub fn lex(&self, trie: &RegexTrie, text: &str) -> Result<Resolved, ResolveError> {
        self.select(trie, text, true)
    }

    /// The one selection rule (DESIGN ›The scope's constructor is the
    /// driver‹, ruled 10 September 2026): among every candidate the index
    /// matches at the start of `text`, a match that would end between two
    /// word characters is no candidate (`in` never cuts `incr`, `i32` never
    /// cuts `i32abc`; the word pattern, greedy over word characters, never
    /// ends inside one); of the rest, those with a live record compete, the
    /// highest `lex_rank` first and the longest match at equal rank; equal
    /// rank and equal length is [`ResolveError::Tied`]. When no candidate has
    /// a live record, the error says why the declared ones have none.
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
        // SAFETY: every pointer the trie stores is a record dyad from the store.
        let fields = |r: DyadPtr| unsafe { Record::read(r) };
        // One candidate per record, at its longest match: a pattern with an
        // optional tail reports every end it can reach, and an alternation
        // holds the same record on each of its paths. A match with no live
        // record is no candidate; why it has none is kept for the error.
        let mut cands: Vec<(DyadPtr, usize, bool)> = Vec::new();
        let mut why_none: Option<ResolveError> = None;
        for m in &matches {
            let cuts_word =
                m.matched < bytes.len() && word(bytes[m.matched - 1]) && word(bytes[m.matched]);
            let fresh = crate::identities::fresh::is_fresh_key(m.regex_key);
            if m.matched == 0 || cuts_word || (fresh && !include_fresh) {
                continue;
            }
            // During elaboration the point of use is the frontier, so "range
            // covers the point" is exactly "not yet made dead" (DESIGN ›Name
            // resolution is scope-filtered‹, ruled 3 September 2026).
            let mut live = m
                .records
                .iter()
                .copied()
                .filter(|&r| self.is_open(fields(r).scope) && !fields(r).is_dead());
            let record = match (live.next(), live.next()) {
                (None, _) => {
                    let spelling = text[..m.matched].to_string();
                    if m.records.iter().any(|&r| self.is_open(fields(r).scope)) {
                        why_none = Some(ResolveError::Dead(spelling));
                    } else if why_none.is_none() {
                        why_none = Some(ResolveError::OutOfScope(spelling));
                    }
                    continue;
                }
                (Some(r), None) => r,
                (Some(_), Some(_)) => {
                    return Err(ResolveError::Ambiguous(text[..m.matched].to_string()))
                }
            };
            match cands.iter_mut().find(|(r, _, _)| *r == record) {
                Some(e) => e.1 = e.1.max(m.matched),
                None => cands.push((record, m.matched, fresh)),
            }
        }
        let mut best: Option<(f64, usize, Resolved)> = None;
        let mut tied = false;
        for (record, matched, fresh) in cands {
            let f = fields(record);
            // The spelling's own rank, off its record (#122): a second name
            // for one identity ranks on its own.
            let rank = f.lex_rank;
            let better = match &best {
                None => true,
                Some((r, n, _)) => rank > *r || (rank == *r && matched > *n),
            };
            if better {
                let r = Resolved { fresh, matched, record, identity: f.dyad, scope: f.scope };
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

    /// Declare `name` denoting `identity` in the current scope, enforcing
    /// no-shadowing: [`ResolveError::Shadowed`] if `name` already resolves to a
    /// live candidate in the open scopes. A known-but-out-of-scope, unknown, or
    /// dead name is free to (re)declare here — the dead case being the one
    /// thing that may follow an `own`/`drop`, a fresh entry beside the dead one.
    /// Requires a current scope. The declaration is journalled for
    /// [`ScopeStack::rollback`], and its `start` awaits the finished body item.
    ///
    /// # Safety
    /// `record` must be a record dyad from the store ([`Record::alloc`]); this
    /// writes its `scope` field through the pointer.
    pub unsafe fn declare(
        &mut self,
        trie: &mut RegexTrie,
        name: &str,
        record: DyadPtr,
    ) -> Result<(), ResolveError> {
        let scope = self.current().expect("declare needs an open scope");
        match self.resolve(trie, name) {
            // Already live in an open scope: shadowing is disallowed.
            Ok(_) => return Err(ResolveError::Shadowed(name.to_string())),
            // Known but closed, unknown, or dead: all fine to declare here.
            Err(ResolveError::OutOfScope(_) | ResolveError::Unknown(_) | ResolveError::Dead(_)) => {
            }
            // Ambiguous or an index error: surface it rather than declaring atop.
            Err(e) => return Err(e),
        }
        // The spelling is text, whatever characters it has (`^`, `<=>`): it
        // enters the index as a literal key, never as a pattern (#110).
        let key = regex::escape(name);
        // SAFETY: `record` is a record dyad from the store, built for this name.
        unsafe { Record::set_scope(record, scope) };
        trie.insert(&key, record);
        self.journal.push(Journal::Declared { name: key, scope });
        self.pending.push(Pending { record, endpoint: Endpoint::Start });
        Ok(())
    }

    /// Declare the pattern `key` denoting `record` in the current scope (#114,
    /// `regex «…» := …`): the key enters the index as the recognizer it is,
    /// never escaped. No haystack check applies to a pattern; the one
    /// consistency check the seed can make at the definition is that this very
    /// key is not already live in an open scope ([`ResolveError::Shadowed`]).
    /// Two different patterns that can match the same length at equal rank are
    /// the inconsistency DESIGN ›The scope's constructor is the driver‹ rules an
    /// error at the second declaration; deciding it needs automaton
    /// intersection the seed does not have, so the lexer's
    /// [`ResolveError::Tied`] stands in when text hits both. Journalled like a
    /// name, under the key as inserted.
    ///
    /// # Safety
    /// `record` must be a record dyad from the store ([`Record::alloc`]); this
    /// writes its `scope` field through the pointer.
    pub unsafe fn declare_pattern(
        &mut self,
        trie: &mut RegexTrie,
        key: &str,
        record: DyadPtr,
    ) -> Result<(), ResolveError> {
        let scope = self.current().expect("declare needs an open scope");
        if let Some(records) = trie.records_for_key(key) {
            // SAFETY: every pointer the trie stores is a record dyad from the store.
            let fields = |r: DyadPtr| unsafe { Record::read(r) };
            if records.iter().any(|&r| self.is_open(fields(r).scope) && !fields(r).is_dead()) {
                return Err(ResolveError::Shadowed(key.to_string()));
            }
        }
        // SAFETY: `record` is a record dyad from the store, built for this pattern.
        unsafe { Record::set_scope(record, scope) };
        trie.insert(key, record);
        self.journal.push(Journal::Declared { name: key.to_string(), scope });
        self.pending.push(Pending { record, endpoint: Endpoint::Start });
        Ok(())
    }

    /// Make `name`'s entry for `identity` in `scope` dead from here on: an
    /// `own` or `drop` (`node`) emptied its place, so every later use is
    /// refused until a `:=` redeclares the spelling (DESIGN ›Memory and
    /// concurrency‹, *`own` and `drop` are static*). `node` is the provisional
    /// `end`; [`ScopeStack::settle_item`] replaces it with the body item once
    /// the line is complete. Journalled for [`ScopeStack::rollback`].
    ///
    /// # Safety
    /// `record` must be a record dyad from the store (one the resolver
    /// returned); this writes its `end` field through the pointer.
    pub unsafe fn mark_dead(&mut self, record: DyadPtr, node: DyadPtr) {
        // SAFETY: `record` is a record dyad the trie returned.
        let prev_end = unsafe { Record::replace_end(record, node) };
        self.journal.push(Journal::Ended { record, prev_end });
        self.pending.push(Pending { record, endpoint: Endpoint::End });
    }

    /// `scope` just appended `item` to its body: every range endpoint pending
    /// for that scope now points at the item, the declaring or ending line
    /// as a whole (an `own` inside an `if` body ends the outer name at the
    /// `if`, DESIGN ›Name resolution is scope-filtered‹).
    ///
    /// # Safety
    /// Every pending record must be a record dyad from the store (which
    /// [`ScopeStack::declare`] and [`ScopeStack::mark_dead`] guarantee); `item`
    /// is stored in its range field, never read.
    pub unsafe fn settle_item(&mut self, scope: DyadPtr, item: DyadPtr) {
        let mut i = 0;
        while i < self.pending.len() {
            let record = self.pending[i].record;
            // SAFETY: every pending record is a record dyad from the store.
            if unsafe { Record::read(record).scope } != scope {
                i += 1;
                continue;
            }
            let p = self.pending.swap_remove(i);
            // SAFETY: as above.
            unsafe {
                match p.endpoint {
                    Endpoint::Start => Record::set_start(record, item),
                    Endpoint::End => Record::set_end(record, item),
                }
            }
        }
    }

    /// Declare `name` denoting the record in the current scope, checked
    /// against its *siblings* alone (DESIGN ›The constructor is a field‹:
    /// "a field's declaration is checked only against its siblings", so
    /// `x := 1, p := type (instance = (x := i32 ?))` is legal): a live record
    /// of the spelling in this very scope is [`ResolveError::Shadowed`]; one
    /// in an enclosing scope stands beside it, both live only until this
    /// scope closes — and a field's type names no sibling, so nothing inside
    /// the list resolves the spelling.
    ///
    /// # Safety
    /// `record` must be a record dyad from the store ([`Record::alloc`]); this
    /// writes its `scope` field through the pointer.
    pub unsafe fn declare_field(
        &mut self,
        trie: &mut RegexTrie,
        name: &str,
        record: DyadPtr,
    ) -> Result<(), ResolveError> {
        let scope = self.current().expect("declare needs an open scope");
        if self.declared_in(trie, name, scope)? {
            return Err(ResolveError::Shadowed(name.to_string()));
        }
        // The spelling is text, whatever characters it has (`^`, `<=>`): it
        // enters the index as a literal key, never as a pattern (#110).
        let key = regex::escape(name);
        // SAFETY: `record` is a record dyad from the store, built for this name.
        unsafe { Record::set_scope(record, scope) };
        trie.insert(&key, record);
        self.journal.push(Journal::Declared { name: key, scope });
        self.pending.push(Pending { record, endpoint: Endpoint::Start });
        Ok(())
    }

    /// Whether a live record of the spelling `name` was declared in `scope`
    /// itself: the sibling check of [`ScopeStack::declare_field`], run too
    /// across the two halves of an `instance = (…)` block — its `shared`
    /// members live in the type's own scope, its fields in the list's — so
    /// that the block is one scope for no-shadowing (DESIGN ›The constructor
    /// is a field‹, 19 September 2026: "the instance block being the scope
    /// where `y` is live for `z`'s line and no-shadowing applies as before").
    pub fn declared_in(
        &self,
        trie: &RegexTrie,
        name: &str,
        scope: DyadPtr,
    ) -> Result<bool, ResolveError> {
        match trie.get(name) {
            Ok(m) if m.matched == name.len() => {
                // SAFETY: every pointer the trie stores is a record dyad.
                let fields = |r: DyadPtr| unsafe { Record::read(r) };
                Ok(m.records.iter().any(|&r| fields(r).scope == scope && !fields(r).is_dead()))
            }
            Ok(_) | Err(RegexTrieError::NodeNotFound) => Ok(false),
            Err(e) => Err(ResolveError::Index(e)),
        }
    }

    /// Re-point `record` at `identity`. Used by the declaration fixpoint when
    /// the value turns out to *be* an existing identity (a type): the name
    /// becomes another spelling of that node — its own record, one per name,
    /// pointing at the shared dyad — so pointer-identity checks
    /// (`is_numtype_node`, logos equality) see the original. The record's
    /// range and journal entry are untouched; a pending endpoint follows,
    /// since it holds the record and not the identity.
    ///
    /// # Safety
    /// `record` must be a record dyad from the store; this writes its `dyad`
    /// field through the pointer.
    pub unsafe fn rebind(&mut self, record: DyadPtr, identity: DyadPtr) {
        // SAFETY: `record` is a record dyad from the store.
        unsafe { Record::set_dyad(record, identity) };
    }
}

/// Operator associativity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
}

/// The fields of a function node's value record, in order, as built by
/// [`Parser::parse_fn`]: the input `record`, the return type, the reflectable body,
/// and the compiled code. The concrete ops (`add_i32`, `if_native`, …) are
/// `callable` leaves the nodes reference from their op slots instead; a user
/// function carries its own compiled callable here, null until compiled, and
/// `run` jumps to it when present (DESIGN ›Execution is function application‹).
pub const FN_INPUT: usize = 0;
/// See [`FN_INPUT`].
pub const FN_OUTPUT: usize = 1;
/// See [`FN_INPUT`].
pub const FN_BODY: usize = 2;
/// See [`FN_INPUT`]. The compiled code — a `callable` node (`[entry: @exec,
/// convention]`), null until compiled.
pub const FN_BCODE: usize = 3;
/// See [`FN_INPUT`]. The activation-record byte size: a `u64` leaf holding the
/// total size of the function's frame — its parameters first, then the
/// per-call storage its `:=` locals, loop variables, and record instances
/// occupy at their offsets — or null for a function with no parameters and no
/// locals. Read by both tiers on entry — [`crate::run::Runtime`] to claim the
/// interpreter's frame from its activation stack, the compiler to size the
/// Cranelift stack slot. A trailing slot, so every reader of
/// `FN_INPUT..=FN_BCODE` is unaffected.
pub const FN_FRAME: usize = 4;
/// See [`FN_INPUT`]. The outer names the body reads: an `array` of their
/// records — every record the body's parse resolved from outside the
/// function, in first-read order — or null when it reads none. Filled by the
/// parse the body already makes and read at every call (#125; DESIGN ›`own`
/// and `drop` are static‹: "A call is a use of every outer name the callee's
/// body reads … The list is the function's own, filled by the parse it
/// already makes"). A trailing slot, like `FN_FRAME`.
pub const FN_OUTER: usize = 5;

/// The activation-record byte size a function node declares in its [`FN_FRAME`]
/// slot: the `u64` the slot's leaf holds, or `0` when the slot is null (no
/// parameters and no locals). Read on every call to size the per-call storage.
///
/// # Safety
/// `fn_node` must be a function node whose value is `[input, output, body,
/// bcode, frame, outer]` as [`Parser::parse_fn`] builds it.
pub unsafe fn fn_frame_size(fn_node: DyadPtr) -> usize {
    let frame = *((*fn_node).value as *const DyadPtr).add(FN_FRAME);
    if frame.is_null() {
        0
    } else {
        std::ptr::read_unaligned((*frame).value as *const u64) as usize
    }
}

/// The records of the outer names a function node's body reads, from its
/// [`FN_OUTER`] slot: empty for a function that reads none, and for a
/// declaration's placeholder whose value is still being parsed (a recursive
/// self-call inside the body has nothing to check yet).
///
/// # Safety
/// `fn_node` must be a function node: its value null, or the operands
/// [`Parser::parse_fn`] builds (the early signature included).
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

/// The slot words, in the order the identities on [`Core::slots`] and
/// [`SlotKind`] follow (DESIGN ›The constructor is a field‹: "`parse_rank`,
/// `lex_rank`, `associativity`, `parse`, `drop`, `run`, and `instance` are
/// places `type` declares"). The seed spells them as root identities, known
/// everywhere; DESIGN (ruled 19 September 2026, later) has them known only
/// inside a type body, the type's own on a bare line and the instances' in
/// the block, so a variable named `run` outside a body is legal there and a
/// shadowing error here — a listed divergence, to be undone. `drop` is not
/// here: it is the statement keyword, one word, which `=` takes as the slot's
/// name when it stands alone to its left ([`SlotKind::Drop`]).
/// `lex_rank` is the seed's stand-in for writing a name's rank in its body
/// (#122). `instance` is the field list of every instance (#128; spelled
/// `value` from 17 to 19 September 2026), read before `=` reads its right
/// side ([`Parser::instance_block_fill`]). `run` (spelled `code` until #133
/// slice 4) is filled inside the instance block, `shared run = (…)`, and kept
/// in the record head beside the constructor.
pub const SLOT_NAMES: [&str; 6] =
    ["parse_rank", "lex_rank", "associativity", "parse", "run", "instance"];

/// How deep scopes may nest before the parse is refused (#80). Each open
/// bracket recurses `parse_sequence` -> `parse_next` -> `lex_segment` ->
/// `run_ctor` -> `(`'s constructor -> `parse_sequence`, so a wall of `(` costs Rust
/// stack the same way a runaway recursion does, and answered it the same way:
/// by aborting. Sized well under what [`crate::WORK_STACK_BYTES`] holds, and
/// far past anything a person writes — source nested two thousand brackets
/// deep is generated, and generated source can be told a number.
pub const MAX_BRACKET_DEPTH: usize = 2_000;

/// What a line of a `type (…)` body is (DESIGN ›The constructor is a field‹:
/// a body line fills a slot, `instance = (…)` among them, or is prose; a bare
/// member is [`ParseError::MemberOutsideInstanceBlock`] since 19 September
/// 2026). Anything else is [`ParseError::TypeBodyLine`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyLine {
    /// A comment.
    Prose,
    /// A declaration — a member of the type's own, or a slot fill, which is
    /// the declare node [`Parser::slot_fill`] yields.
    Declare,
    /// None of those.
    Other,
}

/// One of the six slots, by [`SLOT_NAMES`] position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    ParseRank = 0,
    LexRank = 1,
    Associativity = 2,
    Parse = 3,
    Run = 4,
    Instance = 5,
    /// The `drop` keyword standing left of `=`: a slot with no spelling in
    /// [`SLOT_NAMES`], since the word is the statement's.
    Drop = 6,
}

impl SlotKind {
    /// The slot's spelling: its entry in [`SLOT_NAMES`], or the keyword's
    /// own for [`SlotKind::Drop`], which has none there.
    fn name(self) -> &'static str {
        match self {
            SlotKind::Drop => "drop",
            kind => SLOT_NAMES[kind as usize],
        }
    }

    /// The slot at `i` in [`SLOT_NAMES`].
    fn of(i: usize) -> Self {
        match i {
            0 => SlotKind::ParseRank,
            1 => SlotKind::LexRank,
            2 => SlotKind::Associativity,
            3 => SlotKind::Parse,
            4 => SlotKind::Run,
            _ => SlotKind::Instance,
        }
    }
}

/// A `type (…)` definition being parsed (#61): its body scope, and what its
/// lines have filled so far — the head the type node takes at the close, the
/// constructor to install, and the `instance = (…)` block's layout.
struct OpenType {
    scope: DyadPtr,
    parse_rank: f64,
    /// Set by a `lex_rank = …` line; written onto the record of the
    /// declaration this body is the value of at the close (#122).
    lex_rank: Option<f64>,
    assoc: Assoc,
    ctor: DyadPtr,
    /// Set by a `shared run = fn …` line in the instance block: the fn an
    /// instance of the type runs as (#63; #133 slice 4) — the 4 September
    /// shape, kept until the bare body runs (#133 slice 9).
    run: DyadPtr,
    /// Set by a `shared run = (…)` line in the instance block: the body
    /// lexed once into its cells, the [`crate::identities::run_body`] node
    /// [`Parser::slot_body_fill`] built, installed on the type at the close;
    /// a node of the type constructs it per field-type set (#133 slice 8).
    run_body: DyadPtr,
    instance: Option<(DyadPtr, DyadPtr, u64)>,
    /// The hidden `this` parameter of the `parse` body being read, while it
    /// is read ([`Parser::slot_body_fill`]); null otherwise. `.` on it reads a
    /// field of the node being built ([`Parser::this_field`]).
    this_param: DyadPtr,
    /// True while a `shared` line of the instance block is being parsed
    /// ([`Parser::shared_member`]): a slot fill there is the instances' slot,
    /// on a bare line the type's own (DESIGN ›The constructor is a field‹,
    /// 19 September 2026).
    in_block: bool,
}

/// Whether a constructor applied. A constructor never hands a result to a
/// scheduling driver: it edits the tape *in place* — usually replacing its own
/// token with the dyad it built ([`ParsingTape::place`]), splicing out the
/// neighbours it consumed, and sometimes leaving another *token*, or inserting
/// tokens elsewhere on the frontier (the macro mechanism: DESIGN ›a
/// constructor may splice tokens in or drop upcoming ones before they lex‹).
/// `Placed` reports only that it did; the tape is the result. `Decline` is
/// "not mine": the constructor consumed nothing and touched nothing, and the
/// driver leaves the identity standing as its own value ([`Parser::run_ctor`];
/// DESIGN, 19 September 2026: a constructor with nothing to consume "should
/// rather just be itself in constructed state") — an explicit signal, because
/// a tape left holding a token is a legitimate outcome, not a refusal.
pub enum Constructed {
    /// The construct applied; its edits are on the tape.
    Placed,
    /// The construct does not apply here; nothing was consumed.
    Decline,
}

/// A Logos-written constructor as a [`ConstructFn`]: the function in the
/// identity's constructor slot, run over the tape (see
/// [`Parser::run_logos_ctor`]).
fn logos_constructor(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    // SAFETY: `id` is the identity whose slot `construct_of` just read.
    let f = unsafe { crate::identities::meta::constructor_of(id) };
    // SAFETY: `f` is the fn the type's constructor slot holds, `id` the
    // resolved type, and the tape's cells are the driver's.
    unsafe { p.run_logos_ctor(f, id, tape) }
}

/// The application constructor as a [`ConstructFn`] (see
/// [`Parser::construct_application`]).
fn application(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    p.construct_application(id, tape)
}

/// Every identity's parse-time constructor — the one `seed-parse` entry
/// signature its constructor-slot leaf carries. The constructor receives the
/// parser (a service it re-enters for `parse_expression`, the expect-helpers,
/// declaration), the identity being constructed, and the tape with the cursor
/// on the construct's own token: it reads its span from the cursor cell, its
/// left context from `tape.at(-1)` (the model's `tape[-1]`), its right operand
/// from `tape.at(1)` (an infix, invoked at reduction), and any further tokens
/// by consuming source forward — and it edits the tape *in place*: what it
/// consumed it splices out, what it built it leaves at the cursor (a dyad, or
/// another token). The driver decides only *when* constructors run — the
/// parse_rank decision — never what they leave.
pub type ConstructFn =
    fn(&mut Parser, DyadPtr, &mut ParsingTape) -> Result<Constructed, ParseError>;

/// An open scope's bookkeeping while its body parses (see [`Parser::open`]).
#[derive(Default)]
struct OpenScope {
    /// The `defer free <place>` nodes the scope's owning bindings inserted
    /// (issue #49), drained into the body after each statement.
    defers: Vec<DyadPtr>,
    /// The items parsed at depth 0 and not yet run; run by
    /// [`Parser::drain`] when the pass needs a value, and otherwise by the
    /// scope's own run (a block's, or the top level's [`Parser::finish`]).
    unrun: Vec<DyadPtr>,
}

/// Why elaboration failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Name resolution failed.
    Resolve(ResolveError),
    /// A Logos-written constructor, invoked for an appearance of its identity
    /// (#61), failed while running; carries the rendered run error.
    ConstructorFailed(Box<String>),
    /// An item the pass ran failed (DESIGN ›Build and run are one
    /// self-directing pass‹): the run error, reported by the drivers as one.
    Run(crate::run::RunError),
    /// A line of a `type (…)` body that neither fills a slot, declares a
    /// member, lays out its instances (`instance = (…)`), nor is prose (#61).
    TypeBodyLine,
    /// A second `instance = (…)` block in one type body.
    DoubleInstance,
    /// A bare `:=` line in a type body: members are declared inside
    /// `instance = (…)` (DESIGN ›The constructor is a field‹, 19 September 2026).
    MemberOutsideInstanceBlock,
    /// `shared` outside an `instance = (…)` block.
    SharedOutsideInstanceBlock,
    /// `shared` not followed by a `name := value` declaration.
    SharedNeedsDeclaration,
    /// A slot word left of `=` where no type is being defined.
    SlotOutsideDefinition,
    /// A bare `run = …` line: a type's own run, which nothing in the seed
    /// runs; an instance's run is `shared run = (…)` in the block.
    OwnRunNotInSeed,
    /// The instances' parse trio (or a nested `instance`) in the block.
    InstanceSlotNotInSeed,
    /// A `drop = …` fill, on a bare line or in the block.
    DropSlotNotInSeed,
    /// A slot word inside the instance block without `shared`.
    InstanceSlotNeedsShared,
    /// A binding in a type body inserted a teardown, which no scope exit runs.
    DeferInTypeBody,
    /// A type body's own declaration failed while running at the definition
    /// (#87); carries the rendered run error.
    TypeBodyFailed(Box<String>),
    /// Scopes nested deeper than [`MAX_BRACKET_DEPTH`] (#80): the checked
    /// error a wall of brackets gets, instead of the Rust stack overflowing.
    TooDeep,
    /// The pass needed a type identity and found a place that will only hold
    /// one when the program runs (#75): a hole's layout, a field of the
    /// identity, an `==` that must fold. DESIGN ›A type is a comptime value‹
    /// (12 September 2026) keeps exactly this work in the pass, while passing,
    /// storing and comparing a type value stay ordinary.
    TypeKnownOnlyAtRun,
    /// `=` into an owning place whose right side does not own (#79): a borrow,
    /// or a bare name that is itself an owner. Either leaves two teardowns
    /// over one block, or one over memory the store owns.
    NonOwningIntoOwning,
    /// `dyad (T, v)` for a `T` whose values are bytes at a width the seed
    /// cannot fill from a value node (#85): a pointer, `void`, a record type.
    BadDyadType,
    /// `parse_rank = …` whose value is not a number known at the definition.
    NonComptimeRank,
    /// `associativity = …` with something other than `left` or `right`.
    BadAssociativity,
    /// `parse = …` with something other than a function taking the tape by
    /// value (the seed's shape until #126).
    BadConstructorSignature,
    /// `lex_rank = …` in a type body that is not the value of a declaration:
    /// the rank is the name's (#122), and here there is no name.
    LexRankNeedsName,
    /// `shared run = …` with a value that is neither a bare body nor, in the
    /// shape kept until #133 slice 9, a function (#63).
    BadRunSlot,
    /// A type whose `run` is a held body applied to arguments, `^(2, 3)`:
    /// only the type's own `parse` fills a node's fields and output, so the
    /// call form has no node to resolve a run for.
    RunBodyHeld,
    /// A held run body could not be constructed for a field-type set: the
    /// type's spelling at the use, and the failure rendered against the
    /// body's own text.
    RunBodyFailed { name: String, rendered: String },
    /// `this.f` in a parse body of a type with no `instance = (…)` block
    /// above it: there is no field to reach.
    ThisNeedsInstanceBlock,
    /// `this.f` naming no field of the instance block; carries `f`.
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
    /// An opening `(` had no matching `)`.
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
    /// An abstract operator (e.g. `+`) could not resolve a concrete machine op for
    /// its operand types (DESIGN ›a `+` over mismatched or sizeless logos simply
    /// does not lower until that is resolved‹).
    UnsupportedOperands,
    /// An `if` condition was not a `bool` (a comparison result or `bool` value).
    NonBoolCondition,
    /// An `if` without an `else` was used where a value is required (a numeric
    /// operand or a numeric function's tail): with no false branch it yields unit,
    /// not a value.
    MissingElse,
    /// A logical operator (`and`/`or`/`not`) was applied to a non-`bool` operand.
    NonBoolOperands,
    /// A binary operator's operands were two *different* concrete numeric type (e.g.
    /// `i32` and `f64`). Cross-logos arithmetic needs an explicit cast; there is no
    /// implicit coercion.
    TypeMismatch,
    /// A number literal had no exact value in the type it was committed to (a decimal
    /// molded to an integer, or an out-of-range integer).
    UncomputableLiteral,
    /// A `return` in a non-tail position of a scope's sequence: v1 `return` is the
    /// tail yield, and an early return would silently not exit (no unwinding yet),
    /// so it is rejected rather than mis-run. Early exit arrives with control flow.
    EarlyReturn,
    /// A unit-valued statement (a `while` loop) stood where a value is required (a
    /// numeric function's tail).
    StatementAsValue,
    /// An assignment target that is not a typed numeric variable. A comptime
    /// (`:=`-bound rational) binding has no machine storage to write — writing its
    /// value slot would corrupt the fraction — and nothing else has storage yet.
    BadAssignTarget,
    /// The assignment target is a bare literal: `x := 5` binds the name to the
    /// number itself (DESIGN ›Numbers‹: a literal stays `rational`, "committing
    /// to a concrete type only when it finally lands in a classified slot"), so
    /// there is no storage for `x = 6` to write. Carries the literal's spelling
    /// so the message can point at the typed declaration that makes a place.
    AssignToLiteral(Box<String>),
    /// A gate word (`pub`) was not followed by a declaration: a gate fills a
    /// declare node's gate slot, so anything else leaves it nothing to mark.
    GateNeedsDeclaration,
    /// A declaration was gated twice (`pub pub x := …`).
    DoubleGate,
    /// An `import` was not followed by a path token.
    ExpectedPath,
    /// A `regex` was not followed by a `«…»` quote (#114).
    ExpectedPattern,
    /// A `lex` was not followed by a `«…»` quote (#62).
    ExpectedQuote,
    /// `tape.insert(k, …)` was handed something that is not a tape: `insert`
    /// splices a tape into a tape (DESIGN ›Text is the quote‹, 14 September
    /// 2026), a `lex «…»` fragment or a `parsing_tape` value.
    InsertTakesTape,
    /// A constructor edited the tape and returned with its own cell still
    /// unconstructed and still its own identity (#81): neither a decline
    /// (the frontier untouched) nor a construction — "an unfinished
    /// construct", the checked error rather than a second run. Carries the
    /// cell's spelling.
    CellLeftUnconstructed(Box<String>),
    /// The pattern a `regex «…»` quotes does not compile (the regex engine's
    /// reason), or is empty. Reported at the quote, at the definition, since
    /// the index compiles a branch only on first lookup.
    BadPattern(String),
    /// An `import` inside a deferred-or-repeated body (a fn body, a loop, a
    /// runtime branch): the load happens once, at parse, so `import` belongs
    /// where parse order and run order coincide.
    ImportInRuntimeBody,
    /// The imported file could not be read: the joined path and the OS error.
    ImportRead(String),
    /// An import cycle: the named file is already loading. The import graph
    /// must be a DAG (ruled August 2026).
    ImportCycle(String),
    /// The imported file failed to parse or run; `rendered` is the inner
    /// report, positioned in the imported file's own source.
    ImportFailed {
        /// The path as written at the import site.
        path: String,
        /// The fully rendered inner report (file:line:col, caret and all).
        rendered: String,
    },
    /// A reflection read that does not fit the node's logos (`.operand` on a
    /// scalar, an index past the arity, `.fields` of a non-record type), an
    /// unknown member on a view or logos, or a read whose answer is the honest
    /// undefined (a null constructor slot). Answering `?` instead waits for
    /// the `?` identity (#38).
    BadReflectRead,
    /// A collection member (`.operands`, `.roles`) without its `[index]` —
    /// element access is `[…]` (ruled August 2026), and the bare collection
    /// as a first-class value waits for the array logos (#47).
    ExpectedIndexBracket,
    /// `.logos` on something that is not a dyad view: `.` reads only the
    /// fields a type defines, which are about the value — a value's type is
    /// never one of its own fields (ruled August 2026). The view puts the
    /// type into the value: `x:dyad.type`.
    TypeNeedsView,
    /// A record construction's argument count did not match its field count.
    CtorArity,
    /// A `for` was followed by a fresh spelling and then not by `in`: a fresh
    /// spelling can only be the loop variable, and the variable wants `in`.
    ExpectedIn,
    /// A `for`'s range was malformed: a missing `..`, or a range part that is not
    /// a primary (a literal, a resolved name with `.field`s, or a `( … )` scope —
    /// a bare full expression would consume the body's `(` as a call).
    ExpectedRange,
    /// A `for`'s literal step was not positive: with the end-exclusive `var < end`
    /// condition, a non-positive step could never terminate as stated.
    BadStep,
    /// An `&` of something without storage to point at: a comptime binding or a
    /// non-place expression.
    BadAddressOf,
    /// A numeric conversion `logos(value)` was malformed: not exactly one operand, or a
    /// non-numeric operand (there is nothing to convert).
    BadCast,
    /// A typed declaration's `name :` — or a type variable's fill `name = …` —
    /// was followed by something that is not a type value: the type slot holds
    /// a type, so the expression must evaluate to one (a spelled logos, or a
    /// `-> logos` call resolved at parse time).
    BadDeclaredType,
    /// A typed declaration of a non-numeric type (`a := logos ?`, a record, a
    /// pointer, `bool`, `void`) — the declared-logos storage for those is not in
    /// the seed yet, and this names the gap instead of mis-storing the value.
    NonNumericDeclaredType,
    /// A `-> logos` call could not be resolved at parse time — either running it
    /// failed (its arguments were not comptime-known) or it did not yield a type.
    /// A logos-returning function is evaluated during parsing (roadmap #30), so its
    /// arguments must be known then.
    NonComptimeTypeCall,
    /// A nested function referenced (or took the address of) a local or a
    /// parameter of an enclosing function — a closure capture, which v1 does not
    /// support. Each function's locals and parameters live in its own per-call
    /// activation; reaching an outer one would read the wrong frame at run time.
    CapturedLocal,
    /// An owning value (`alloc …`, `own x`) stood somewhere no name binds it —
    /// as a call argument, say. The teardown attaches at the *binding site*
    /// (DESIGN ›Explicit heap, and no implicit destruction‹, issue #49), so an
    /// unbound owning value has no place to hang its `defer free` on and would
    /// leak. Fail-closed until the temporary-attachment rule is ruled, which is
    /// ownership-gated parameters (issue #53): bind it to a name first.
    UnboundOwningValue,
    /// A scope's value is a bare owning place, so ownership would escape the
    /// scope that frees it — the inserted `defer free` runs at exit and the
    /// value handed out is already freed. DESIGN ruled `own` as how ownership
    /// leaves a scope (it "transfers ownership and removes the source
    /// identity"), so this asks for the explicit `own x`.
    OwningEscape,
    /// A function body hands ownership out through its return. A block may do
    /// this (its binder sees the tail at parse), but a call hides the body
    /// behind the return type, and a plain `@T` carries no destructor, so the
    /// caller could not know it owes a `free`. Fail-closed until a return type
    /// can declare that it transfers ownership — the ownership-gate work,
    /// issue #53.
    OwnershipAcrossReturn,
    /// An `own` or `drop` inside a loop body or a `fn` body names a place
    /// declared outside it. The loop would read a dead name on its next pass,
    /// and a function may own only what its parameters hand it (DESIGN ›Memory
    /// and concurrency‹, *Bodies that run again or later*, ruled 3 September
    /// 2026).
    OwnOfOuterName,
}

/// Whether `node`'s result is a `bool`: a `bool` literal/value, a comparison
/// (`<`/`>`/`==`/…), or a logical operator (`and`/`or`/`not`). An `if` condition and
/// a logical operator's operands must be one; arithmetic and other values are not.
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
    logos == types.bool_
        || logos == types.lt
        || logos == types.gt
        || logos == types.eq
        || logos == types.le
        || logos == types.ge
        || logos == types.ne
        || logos == types.not_
        || logos == types.tape.is_constructed
}

/// The parse-time truth of a bool literal — `{type: bool, value -> i32 0/1}`, the
/// shape the `true`/`false` keywords and every comptime fold produce — or `None`
/// for anything else. Deliberately no scope unwrapping: a sequence-valued
/// condition may carry effectful non-tail expressions that a fold would silently
/// drop, so only a bare literal (pure by construction) counts as comptime.
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

/// The trailing *value* expression of a sequence node
/// `{type: scope, value: [exprs, op, parent]}` — trailing comment nodes are
/// prose, not the tail — or `None` for a scope with no expression array (a
/// record/parameter-list scope).
///
/// # Safety
/// `node` must be a valid dyad from the store; a non-null value must be the
/// `[exprs, op, parent]` triple as [`Parser::parse_sequence`] fills it.
pub(crate) unsafe fn last_sequence_expr(node: DyadPtr) -> Option<DyadPtr> {
    crate::identities::scope::exprs_of(node)?
        .iter()
        .rev()
        .find(|&&e| !crate::identities::numtype::is_comment_type((*e).ty))
        .copied()
}

/// Whether `node` is or contains a `return` in the positions v1 recognizes as
/// value-producing (the same enumeration as `commit_tail`: a `return` itself, an
/// `if`'s branches, a sequence's expressions). Used to reject a `return` in a
/// non-tail sequence position, where it would run without exiting.
///
/// # Safety
/// `node` must be a valid dyad from the store, with the value shapes its type
/// implies (as the parser builds them).
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

/// Build a call node `{type: callee, value: [args…, null]}`, the application
/// `callee(args)`. Like a binary operator's `{type: op, value: [lhs, rhs]}`, a call's
/// value is the operand array of its arguments (null-terminated so `run` can count
/// them); a nullary call carries a null value. The callee's type decides how the
/// call runs, exactly as an operator's does.
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

/// The once-per-run import registry (#58): canonical path → load state. A file
/// loads once per run — every importer shares the one loaded section and its
/// identities, so two importers of the same file see the same type, never two
/// copies — and the import graph must be a DAG, so a path met again while its
/// own load is still in progress is a cycle (ruled August 2026). The REPL
/// threads one registry across its per-line parsers (a session is a run); the
/// command-line driver's single parser holds one for the whole run.
#[derive(Debug, Default)]
pub struct Imports {
    entries: HashMap<PathBuf, ImportState>,
    /// The section scope each loaded file's declarations landed in. A
    /// section's names live for the run once declared — "all importers …
    /// share the one loaded scope and its identities" (DESIGN ›Importing is
    /// dropping the text there‹) — so a call of a `pub` function whose body
    /// reads a private sibling is a use of a live name from anywhere, though
    /// the section is on no caller's stack ([`Parser::check_call_reads`]).
    sections: HashSet<DyadPtr>,
}

#[derive(Debug)]
enum ImportState {
    /// The file's own pass is in progress: importing it again now is a cycle.
    Loading,
    /// Loaded: the `pub` names the file exposes, in declaration order, each
    /// paired with the identity its spelling resolves to inside the file, and
    /// the file's tail node (null for a declaration-only file).
    Loaded { pubs: Vec<(String, DyadPtr)>, tail: DyadPtr },
}

/// The one-pass elaborator: lexes on demand, resolves names against the scope
/// stack, and reduces the tape by operator parse_rank, running each identity's
/// native `Construct`. The scheduling is a deferred-reduction operator
/// parse_rank over the explicit tape (not Pratt): operators wait on the tape as
/// pending tokens until parse_rank says to reduce them.
pub struct Parser<'a> {
    source: &'a str,
    pos: usize,
    scopes: ScopeStack,
    trie: &'a mut RegexTrie,
    /// The core type handles the parser logos opened nodes with (see [`Core`]).
    types: &'a Core,
    /// The placeholder of the declaration currently awaiting its value, or null.
    /// When the value opens with a `fn` literal, [`Parser::parse_fn`] publishes the
    /// signature onto it before the body parses, so a recursive self-call resolves
    /// its parameter and return type instead of the unbound-placeholder defaults.
    pending_fn: DyadPtr,
    /// The records of the declarations whose right side is being driven,
    /// innermost last: what a `lex_rank = …` line in a `type (…)` body
    /// writes (#122) — the seed's stand-in for the reach DESIGN records open
    /// (how a definition writes a record field from inside `:=`), kept
    /// because a pattern identity has no other spelling to be written on.
    filling: Vec<DyadPtr>,
    /// A stack of open function frames, one per enclosing function body being
    /// parsed. Empty at top level, where declarations get absolute global storage
    /// that persists across REPL lines; non-empty inside a function, where each
    /// local declaration claims the next byte offset in the current frame (via
    /// [`Parser::alloc_local`]) and bumps the top accumulator. [`Parser::parse_fn`]
    /// pushes it around the body and writes the final size into the fn's
    /// [`FN_FRAME`] slot.
    frames: Vec<OpenFn>,
    /// How many deferred-or-repeated bodies enclose the current position — fn
    /// bodies, loop bodies, and runtime `if` branches — where parse order and run
    /// order do NOT coincide. Comptime effects that rebind names at parse (a type
    /// variable's fill) are rejected while this is non-zero: inside such a body
    /// the rebinding would happen once, at the wrong time, and on both runtime
    /// branches. Comptime-taken `if` branches do not count (they run iff parsed).
    runtime_depth: u32,
    /// The type definitions open around the current position, innermost
    /// last (#61): a `type (…)` body pushes one while its lines parse, and
    /// the slot fills and the `instance = (…)` block write into it.
    definitions: Vec<OpenType>,
    /// The open scopes, innermost last; the base entry is the top level. Each
    /// carries the constructor-inserted teardowns its bindings pushed (issue
    /// #49; [`Parser::parse_sequence`] drains them into the body so a defer
    /// runs at scope exit as ordinary structure, and [`Parser::exit`] runs
    /// the top level's) and the items it has parsed and not yet run
    /// ([`Parser::drain`]). Its length is the bracket depth.
    open: Vec<OpenScope>,
    /// The folder relative import paths resolve against — the importing file's
    /// own folder during a nested import, the working directory when the
    /// importer is the command line or REPL (ruled August 2026).
    dir: PathBuf,
    /// The once-per-run import registry (see [`Imports`]).
    imports: Imports,
    /// Comment cells lifted out of a segment at its boundary, with the offset
    /// each was lexed at: prose is reflectable structure interleaved with the
    /// code, invisible to value flow, and [`Parser::parse_next`] hands them
    /// out as body items in source order beside the segment's expression.
    lifted: Vec<(usize, DyadPtr)>,
    /// Body items a constructed segment yielded, in source order, not yet
    /// handed out by [`Parser::parse_next`].
    queued: std::collections::VecDeque<DyadPtr>,
    /// The valueless places `?` built (`i32 ?`, `@T ?`, `point ?`): a `:=`
    /// binds its name straight to such a place, no snapshot and no initializer
    /// — the declaration by declared type.
    holes: HashSet<DyadPtr>,
    /// Whether the constructor now running was woken at discovery — its
    /// token just lexed, the source after it unread — rather than at the
    /// segment boundary. An identity that reads its own bracket (`type`)
    /// reads source only at discovery; at a boundary its bracket, had it one,
    /// would already stand on the tape as a cell.
    discovering: bool,
    /// The stop mode of the segment being lexed ([`Parser::lex_segment_until`]),
    /// kept here so a lazy read inside a constructor ([`Parser::cell_at`])
    /// stops at the same boundaries the loop would.
    lex_mode: Option<RightSide>,
    /// The pass's one runtime (DESIGN ›Build and run are one self-directing
    /// pass‹): everything the parser runs — a Logos-written constructor, a
    /// type body's declarations, a rank, a call for a type, an imported
    /// file, and the items the drivers hand back through [`Parser::value_of`]
    /// — runs here, off raw handles into the store, with the defer type and
    /// the store attached and the compiler when [`Parser::with_lower`]
    /// attached one. One runtime keeps the frame arena and the allocation
    /// ledger whole across the pass.
    rt: crate::run::Runtime<'a>,
    /// The lowering table, when the driver attached one: the nested import
    /// pass hands it to its runtime so `f.compile()` at an imported top level
    /// works exactly as at the driver's own top level (one pass, one behavior).
    lower: Option<&'a crate::compile::LowerTable>,
    /// The held run body whose cells the driver is reading now, if any.
    feed: Option<Feed>,
    /// The construction of a held run body in progress, if any: what
    /// `this.f` means there.
    run_body: Option<RunBodyCx>,
}

/// The cells of a held run body being constructed (#133 slice 8): the
/// driver's next cell comes from here instead of the lexer, found by the
/// cursor's position in the body's text, so a boundary's rewind of the
/// cursor re-exposes a cell exactly as it does a token, and the readers that
/// take raw text at the cursor (a declared name, a literal's digits, a
/// comment's line) read the body's text where the cell stands. A cell whose
/// spelling nothing had declared at the definition resolves now, against
/// the construction's own scopes alone — the parameters and the body's
/// locals, at `base_depth` and above — so a body sees what its definition
/// saw plus what it declares itself, never the use site's names.
struct Feed {
    cells: Vec<Cell>,
    next: usize,
    base_depth: usize,
}

/// What `this.f` means inside a held run body under construction: the
/// field's parameter place, or for a field of type `type` the type this
/// field-type set holds in it ([`Parser::run_body_field`]).
struct RunBodyCx {
    this_param: DyadPtr,
    field_scope: DyadPtr,
    fields: Vec<DyadPtr>,
    places: Vec<DyadPtr>,
    key: Vec<DyadPtr>,
}

/// One token off `text` at `pos` — the lex step the driver and `lex «…»`
/// share (#62): whitespace skipped, the spelling the open scopes and the
/// index resolve at the start of the rest ([`ScopeStack::lex`]), as a cell
/// over `text` with the position after it. A spelling the index knows only
/// through a fresh-spelling pattern (#110) mints its fresh dyad with both
/// slots null into `store` — the pattern's construction, done at the lex.
/// `None` at the end of the text. `text` must outlive every read of the
/// cell ([`Cell::lexed`]).
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
        r.record
    };
    // SAFETY: `text` is the caller's source (the parser's, or an import's
    // held for the run); `dyad` is the record or fresh dyad the index gave.
    let cell = unsafe { Cell::lexed(dyad, text, start, r.matched) };
    Ok(Some((cell, start + r.matched)))
}

/// The lexer over a string, as `lex «…»` runs it (#62; DESIGN ›Text is the
/// quote‹: "the cells the lexer would have put on the frontier, each a
/// pointer to the record the trie resolved at the lex site … unconstructed,
/// no constructor woken"): every token of `text` as an unconstructed cell
/// with its spelling, on a fresh tape centered on its first cell — the
/// fragment `insert` splices. `text` must outlive the fragment's reads: a
/// string node's bytes live for the store.
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

/// One enclosing function body being parsed: the byte-size accumulator its
/// parameter and local declarations claim offsets from. Parameters claim the
/// frame's first offsets (a call frame is an instance of its function — DESIGN
/// ›Resolution is one rule‹), the body's locals continue after them, and both
/// are frame-relative places the one capture guard covers by depth.
struct OpenFn {
    /// Bytes claimed so far by this function's parameters and frame-relative
    /// locals.
    size: usize,
    /// The scope depth the function's barrier begins at: a name declared in a
    /// scope below it is a name from outside the function.
    below: usize,
    /// The records of the outer names the body has read so far, in
    /// first-read order, each once — the function's [`FN_OUTER`] list
    /// (#125), filled by [`Parser::note_outer_read`] as the body's parse
    /// resolves them.
    outer: Vec<DyadPtr>,
}

/// The bytes a field of `logos` packs at in a record's layout and claims in a
/// frame: a scalar at its own width, anything else — a bare or type-valued
/// name, a record, the parsing tape — the 8-byte container the call
/// convention passes ([`Parser::parse_field_list`], [`Parser::fn_over_body`]).
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
    /// A parser over `source`, resolving against `scopes`, allocating into
    /// `store`, and lexing via `trie`. `logos` are the core handles the parser
    /// logos the scopes and records it opens with. Dispatch needs nothing else:
    /// schedules and constructors are read from the identities' own records.
    pub fn new(
        source: &'a str,
        store: &'a mut Store,
        trie: &'a mut RegexTrie,
        types: &'a Core,
        scopes: ScopeStack,
    ) -> Self {
        // The parser owns its runtime and reaches the store through it, so
        // the one `&mut Store` has one owner the borrow checker can see.
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
            lifted: Vec::new(),
            queued: std::collections::VecDeque::new(),
            discovering: false,
            lex_mode: None,
            holes: HashSet::new(),
            frames: Vec::new(),
            runtime_depth: 0,
            definitions: Vec::new(),
            open: vec![OpenScope::default()],
            dir: PathBuf::from("."),
            imports: Imports::default(),
            lower: None,
            feed: None,
            run_body: None,
        }
    }

    /// Attach the lowering table (`Core::lower`), so an imported file's
    /// top-level `f.compile()` runs under the nested pass exactly as under the
    /// driver. The caller keeps the `Core` alive for the parser's life, as it
    /// already does for the store.
    pub fn with_lower(mut self, lower: &'a crate::compile::LowerTable) -> Self {
        self.lower = Some(lower);
        self.rt.set_compiler(lower);
        self
    }

    /// What `node` yields: an item that ran in the pass has its value at hand
    /// ([`crate::identities::ran`]); anything else runs now, on the pass's
    /// runtime. The drivers' one read of an item's value.
    ///
    /// # Safety
    /// `node` must be a valid dyad this parser built into its store.
    pub unsafe fn value_of(&mut self, node: DyadPtr) -> Result<i64, ParseError> {
        if (*node).ty == self.types.ran_ {
            return Ok(crate::identities::ran::value_of(node));
        }
        self.run_on_pass(node).map_err(ParseError::Run)
    }

    /// Run `node` on the pass's runtime with the lexer attached (#62): a
    /// `lex «…»` inside what runs resolves against the scopes open now — for
    /// a constructor, the appearance's — and the index, the seed's reading
    /// of "the record the trie resolved at the lex site" (DESIGN ›Text is
    /// the quote‹). The parser reads neither the scopes nor the index while
    /// the runtime runs, which keeps the raw handles sound, and takes them
    /// back before its next step.
    ///
    /// # Safety
    /// `node` must be a valid dyad this parser built into its store.
    unsafe fn run_on_pass(&mut self, node: DyadPtr) -> Result<i64, crate::run::RunError> {
        // SAFETY: as the caller's contract.
        self.rt.lexing(&self.scopes, self.trie, |rt| unsafe { rt.run(node) })
    }

    /// The root scope's exit (issue #49): run the teardowns the top level's
    /// owning bindings inserted, LIFO, on the pass's runtime. The file driver
    /// calls this at program end; a nested scope ran its own at its exit.
    pub fn exit(&mut self) -> Result<(), ParseError> {
        for defer_node in self.take_pending_defers().into_iter().rev() {
            // SAFETY: `defer_node` is a `defer` node built into the store,
            // which outlives the pass.
            unsafe { crate::identities::run_deferred(&mut self.rt, defer_node) }
                .map_err(ParseError::Run)?;
        }
        Ok(())
    }

    /// Thread an existing import registry through this parser. The REPL uses
    /// this: a session is one run, so its per-line parsers must share one
    /// registry for once-per-run to hold across lines.
    pub fn with_imports(mut self, imports: Imports) -> Self {
        self.imports = imports;
        self
    }

    /// Take the import registry back out (the REPL's per-line thread; pairs
    /// with [`Parser::with_imports`]).
    pub fn take_imports(&mut self) -> Imports {
        std::mem::take(&mut self.imports)
    }

    /// The store the constructors allocate into.
    pub(crate) fn store(&mut self) -> &mut Store {
        self.rt.store
    }

    /// The core type handles (copied out, so a `&mut self` call can follow).
    pub(crate) fn types(&self) -> &'a Core {
        self.types
    }

    /// Allocate storage for a function-local place of `width` bytes, typed
    /// `ty_node`. Inside a function (the frame stack is non-empty) the place is
    /// *frame-relative*: it claims the next offset in the current frame — after
    /// the parameters, which claimed the frame's first offsets at the signature
    /// — and its storage is per-call: the interpreter's frame on its activation
    /// stack, the JIT's stack slot. At top level it is an absolute global blob,
    /// exactly as before. The node is `{type: ty_node, value: <place>}`, its value
    /// an [`crate::dyad::FRAME_TAG`] offset or a real address respectively.
    fn alloc_local(&mut self, ty_node: DyadPtr, width: usize) -> DyadPtr {
        let place = if self.frames.is_empty() {
            // Marked as storage, not as a definition's record: every place
            // carries a tag so the two are told apart everywhere, not only
            // where a frame exists (see [`crate::dyad::GLOBAL_TAG`]).
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

    /// Reject a *capture*: a reference to a frame-relative place — a local or a
    /// parameter — that belongs to an enclosing function's frame (its depth is
    /// not the current one). v1 has no closures, so a nested function cannot
    /// read an outer function's per-call state — doing so would resolve against
    /// the wrong activation record at run time. A place of the current frame,
    /// and every absolute (global) place, pass.
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

    /// The body being parsed has read the name `record` (null: no name). For
    /// every open function whose barrier the record's declaring scope lies
    /// below, the read is of a name from outside that function, and the
    /// record joins its [`FN_OUTER`] list — once (#125; DESIGN ›`own` and
    /// `drop` are static‹: "a function's body resolves the names outside it
    /// when it is parsed, so the function knows which outer names it reads").
    /// A nested function's read of a name outside both functions joins both
    /// lists: a call of the outer runs the inner. A record whose scope is on
    /// no stack — a section's name, reached through a call — lies below every
    /// function.
    fn note_outer_read(&mut self, record: DyadPtr) {
        if record.is_null() || self.frames.is_empty() {
            return;
        }
        // SAFETY: a non-null record here came from the resolver or from a
        // function's own list: a record dyad from the store.
        let scope = unsafe { Record::read(record).scope };
        let depth = self.scopes.position(scope).unwrap_or(0);
        for frame in &mut self.frames {
            if depth < frame.below && !frame.outer.contains(&record) {
                frame.outer.push(record);
            }
        }
    }

    /// A call is a use of every outer name the callee's body reads (#125;
    /// DESIGN ›`own` and `drop` are static‹, ruled 15 September 2026:
    /// "calling it at a point is a use of each of them at that point … the
    /// check is a range lookup per outer name at each call, at elaboration").
    /// `callee` is what the node runs: a function node, or a type carrying a
    /// `code` (whose code is the function); anything else runs no body and
    /// passes. Each listed record is checked as a bare use of the name here
    /// would be — its scope open (or a section's, whose names live for the
    /// run), and not made dead by an `own` or `drop` — the existing
    /// dead-name and out-of-scope errors; a redeclared spelling is another
    /// record, so the body's name stays the dead one. And as a use, each
    /// name joins the lists of the functions being parsed, so a call of a
    /// caller is a use of what its callee reads.
    ///
    /// # Safety
    /// `callee` must be null or a dyad from the store.
    pub(crate) unsafe fn check_call_reads(&mut self, callee: DyadPtr) -> Result<(), ParseError> {
        if callee.is_null() {
            return Ok(());
        }
        // SAFETY: `callee` is a dyad from the store — a constructed node's
        // type, or a resolved callee.
        let function = unsafe {
            if (*callee).ty == self.types.fn_type {
                callee
            } else if crate::identities::meta::is_record_type(callee) {
                crate::identities::meta::code_of(callee)
            } else {
                return Ok(());
            }
        };
        if function.is_null() {
            return Ok(());
        }
        // SAFETY: `function` is a function node; its list holds record dyads.
        let outer = unsafe { fn_outer(function) };
        for &record in outer {
            // SAFETY: as above.
            let fields = unsafe { Record::read(record) };
            let name = || {
                if fields.name.is_null() {
                    String::new()
                } else {
                    // SAFETY: a record's `name` is a string node.
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
            self.note_outer_read(record);
        }
        Ok(())
    }

    /// Advance past whitespace only (never a `#`): the sequence parser peeks at a
    /// statement-level `#` itself, to build the reflectable comment node.
    fn skip_whitespace(&mut self) {
        let bytes = self.source.as_bytes();
        while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// The current byte offset into the source. After a parse error this is the
    /// *stuck point* — the position [`crate::report`] renders as
    /// `file:line:col` — which for the common errors (an unknown name, a bad
    /// literal) sits exactly at the offending token; an error raised after its
    /// operands were consumed points just past its construct. After a
    /// successful parse it is where consumption stopped, so a caller can check
    /// for trailing input (a stray `)` breaks the sequence loop unconsumed).
    pub fn offset(&self) -> usize {
        self.pos
    }

    /// Take the top-level scope's pending teardowns (issue #49): the `defer free`
    /// nodes that top-level owning bindings inserted, which no `parse_sequence`
    /// drained (the top level is no block). [`Parser::exit`] runs their inners
    /// LIFO at program exit — the top level's own scope-exit; the REPL collects
    /// them per line and runs them at session exit. Returns them in insertion
    /// order; the caller reverses for LIFO.
    pub fn take_pending_defers(&mut self) -> Vec<DyadPtr> {
        match self.open.first_mut() {
            Some(base) => std::mem::take(&mut base.defers),
            None => Vec::new(),
        }
    }

    /// Run everything parsed and not yet run, outermost scope first, in
    /// order: the pass running because it must (DESIGN ›Build and run are
    /// one self-directing pass‹, 13 September 2026) — a box it is about to
    /// read, a call for a type, a Logos-written constructor, a type's own
    /// body, an import, or the end of the program. Each executable item
    /// becomes its ran form in place ([`crate::identities::ran`]), so the
    /// scope it stands in reads it later instead of running it again; a
    /// read stays a read, prose stays prose, and a `defer` is held for its
    /// scope's exit as ever. The lists are taken before anything runs, so a
    /// drain inside a drain finds nothing. The item being parsed is in no
    /// list: it runs after, so order is left to right.
    pub fn drain(&mut self) -> Result<(), ParseError> {
        let lists: Vec<Vec<DyadPtr>> =
            self.open.iter_mut().map(|s| std::mem::take(&mut s.unrun)).collect();
        for node in lists.into_iter().flatten() {
            // SAFETY: every pending item is a dyad this parser built into its
            // store, which outlives the pass; the runtime works off raw handles.
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

    /// The end of the program: run what the top level parsed and has not
    /// run — the root scope's own run (DESIGN ›The scope's constructor is
    /// the driver‹: "the root scope run by `import` over a file and by the
    /// command line over its one line").
    pub fn finish(&mut self) -> Result<(), ParseError> {
        self.drain()
    }

    /// Recover the scope stack, consuming the parser. The REPL parses each line
    /// with a fresh `Parser` over one persistent store/trie/scope-stack, so
    /// declarations made on earlier lines stay resolvable.
    pub fn into_scopes(self) -> ScopeStack {
        self.scopes
    }

    /// Consume the closing `)` that matches an opening `(`, or fail if the body
    /// ended at something else (or the end of input).
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

    /// Consume the `]` that matches an opening `[`, or fail (a `)` there is
    /// the mismatched closer, the same error).
    pub(crate) fn expect_close_sq(&mut self) -> Result<(), ParseError> {
        if self.consume_token(self.types.close_sq_) {
            Ok(())
        } else {
            Err(ParseError::UnclosedBracket)
        }
    }

    /// Application — the constructor an instance of `fn`, a record type, or
    /// (through its own constructor) a numeric type runs for the bracket to
    /// its right: DESIGN ›`X (…)` is one spelling, and X's constructor decides
    /// what the bracket is‹ — a call, an instance construction, a conversion —
    /// never `(`'s decision, which builds a group and nothing else (#59 step
    /// 2). Without a `(` directly ahead the identity stands as its own value
    /// (`f(i32, 3)` passes the type; `g := f` names the function).
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

    /// The arguments a bracket cell hands a callable: the scope's expressions
    /// in order (prose and teardown structure aside), a single expression
    /// standing alone as itself, an empty `()` as none — DESIGN ›A function's
    /// surface‹: "the caller's positional arguments are the parameter list's
    /// holes, in order".
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

    /// Invoke `id`'s constructor over a fresh single-token tape — the
    /// service-site form of the driver's dispatch, for a construct invoked from
    /// inside another constructor (a comment's `«…»` text, a range endpoint
    /// literal). `Ok(None)` when `id` has no constructor or it declined.
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

    /// The constructor of `id`, decoded from its constructor-slot leaf — the
    /// parse-time analogue of `run`'s op-slot jump: dispatch flows through the
    /// graph, no table anywhere. `None` for an undefined constructor (a
    /// delimiter token, a data type).
    fn construct_of(&self, id: DyadPtr) -> Option<ConstructFn> {
        // SAFETY: `id` is a resolved identity; a constructor leaf is minted
        // from a `ConstructFn` at registration (`Core::build`) under the
        // `seed-parse` convention, and that convention is checked before
        // the transmute, so the signature is exact; a Logos function in the
        // slot is a dyad, told apart by its type.
        unsafe {
            let leaf = crate::identities::meta::constructor_of(id);
            if leaf.is_null() {
                return None;
            }
            // A constructor written in Logos (#61): the slot holds the
            // function itself, run for every appearance of the identity.
            if (*leaf).ty == self.types.fn_type {
                return Some(logos_constructor);
            }
            // A leaf under any other convention would be jumped to with the
            // wrong signature: the one bug this check turns into a message.
            assert_eq!(
                crate::identities::callable::convention_of(leaf),
                self.types.conv_seed_parse,
                "a constructor leaf must carry the seed-parse convention"
            );
            let entry = crate::identities::callable::entry_of(leaf);
            Some(std::mem::transmute::<usize, ConstructFn>(entry))
        }
    }

    /// Run a Logos-written constructor for an appearance of its identity
    /// (DESIGN ›The constructor is a field‹: "an appearance of X runs X's
    /// `constructor` field"; #61). The function takes the tape by value — a
    /// `parsing_tape` instance is the tape's handle, eight bytes — so the
    /// argument is the driver's own tape, re-centered on the cell by the
    /// driver already; the natives it reaches edit that tape in place and
    /// allocate into the parser's store, both through raw handles while the
    /// driver makes no use of either. A run error is the checked
    /// [`ParseError::ConstructorFailed`]. What the constructor left is read
    /// off the cell by [`Parser::run_ctor`], applied or declined.
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
        // A constructor runs now, by nature; what stands before it runs first.
        self.drain()?;
        // Its run is a call: a use of every outer name its body reads (#125),
        // refused at the identity's own cell.
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
        // `this`: a fresh node of the type being defined, minted per run
        // (DESIGN, 18 September 2026: "`this` in `parse` is a fresh node of
        // the type being defined … its `value` block stands instantiated
        // with every hole its own, never tied to a cell"): the operand run
        // a node of `owner` runs as, one null slot per instance field, the
        // constructor filling them by name and placing the node itself. A
        // body read bare takes it as its second parameter; the wrapped
        // `fn (tape := parsing_tape ?)` form (until #133 slice 9) takes the
        // tape alone.
        // SAFETY: `f` is the fn node the slot fill checked; `owner` is the
        // record type whose slot holds it.
        let args = unsafe {
            let input = *((*f).value as *const DyadPtr).add(FN_INPUT);
            let params =
                crate::identities::array::items(crate::identities::meta::record_fields_of(input));
            if params.len() == 2 {
                let fields = crate::identities::array::items(
                    crate::identities::meta::record_fields_of(owner),
                );
                // One slot per field, the run's terminator, and the slot the
                // function built for the node's field-type set goes into
                // ([`crate::identities::run_body::spec_of`], #133 slice 8).
                let slots = vec![std::ptr::null_mut(); fields.len() + 2];
                let run = self.rt.store.alloc_operands(&slots);
                let this = self.rt.store.alloc_raw(owner, run);
                let this_arg = self
                    .scalar_value(crate::identities::numtype::NumType::U64, this as usize as i64);
                vec![handle, this_arg]
            } else {
                vec![handle]
            }
        };
        let call = build_call(self.rt.store, f, &args);
        // Inside the call, `caller.scope` reads the pass's position (#123).
        self.rt.enter_constructor();
        // SAFETY: `call` was just built into the store; the tape and the
        // store are reached only through the natives until `run` returns.
        let out = unsafe { self.run_on_pass(call) };
        self.rt.leave_constructor();
        out.map_err(|e| ParseError::ConstructorFailed(Box::new(crate::report::run_message(&e))))?;
        Ok(Constructed::Placed)
    }

    /// Whether the running constructor was woken at discovery (see the
    /// `discovering` field): the one moment a bracket reader may read source.
    pub(crate) fn discovering(&self) -> bool {
        self.discovering
    }

    /// `?`'s constructor (DESIGN ›Declarations are immutable by default‹):
    /// a fresh dyad with both slots undefined at every appearance — or, with
    /// a type standing to its left, that type's valueless place, the type
    /// stamped and the value left undefined: `i32 ?` a zeroed place at the
    /// width, `@i32 ?` a pointer place, `type ?` a type variable a later
    /// `name = <type>` fills (a record place by declared type is #47). The type to the left is
    /// read as it stands — `@`s and then a type name or a constructed type —
    /// and those cells are consumed.
    /// `dyad`'s constructor (DESIGN ›Feasibility‹: "`dyad (type, value)`
    /// construction from Logos", #60): with a bracket to its right, `dyad (T,
    /// v)` builds a store-owned cell of type `T`. A numeric `T` takes a
    /// literal `v` committed to its width, exactly as `T v` does; any other
    /// `T` takes `v` as the node the cell's value points at — the reading the
    /// sketches' `tape[0] = dyad (scope, body)` needs, what a constructor
    /// hands in being the constructor's business (#61). Without a bracket,
    /// `dyad` stands as its value, the cell type.
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
        // SAFETY: the bracket cell holds a node from the store; its arguments
        // are reduced dyads.
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
            // place of `ty` (#82): a scalar is storage at its width, filled
            // from a literal of it; nothing else has a whole value a cell can
            // hold from a node.
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
                // `bool` is physically an i32 0/1 in storage, so the cell gets
                // its own copy of the literal's byte. Only `true` and `false`
                // are literals; a runtime comparison has no bits at parse.
                if (*read).ty != types.bool_ || (*read).value.is_null() {
                    return Err(ParseError::UnsupportedOperands);
                }
                let bits = std::ptr::read_unaligned((*read).value as *const i32);
                let storage = self.rt.store.alloc_bytes(&bits.to_ne_bytes());
                self.rt.store.alloc_raw(types.bool_, storage)
            } else {
                // Every other type reads its `.value` as bytes at its own
                // width — a pointer as an address, `void` as nothing, a record
                // as its fields — and the old branch handed each of them the
                // *node* as those bytes: `dyad (bool, 0)` was true because a
                // node address is nonzero, `dyad (@i32, 5)` read the rational
                // node's own bytes as an i32, and a write through it corrupted
                // the store (#85). Nothing builds such a cell and nothing has
                // ruled what one would mean, so it is refused rather than
                // guessed: fail-closed, the same inertness DESIGN ›The
                // constructor is a field‹ gives a type with no constructor.
                return Err(ParseError::BadDyadType);
            }
        };
        tape.remove(1);
        tape.place(cell);
        Ok(Constructed::Placed)
    }

    pub(crate) fn construct_hole(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let types = self.types;
        let base = match tape.at(-1).copied() {
            Some(cell) => {
                // A fresh name to the left is not a type; leave it for the
                // boundary's own report.
                if cell.is_fresh() {
                    None
                } else {
                    // Through `settled_type`: a box the pass has already
                    // filled declares with the type it holds.
                    let d = self.operand_dyad(cell)?;
                    let d = self.settled_type(d);
                    // SAFETY: `d` is a resolved dyad from the store.
                    unsafe {
                        // A place holding a type cannot say what a hole's
                        // layout is: that is the one thing DESIGN ›A type is a
                        // comptime value‹ keeps in the pass — "a place whose
                        // *layout* waits on a runtime type is the thing that
                        // stays refused". Named, rather than left to fall
                        // through as two stray cells.
                        match crate::identities::read::read_kind(types, d) {
                            crate::identities::read::Read::Identity => Some(d),
                            crate::identities::read::Read::Container(t)
                                if t == types.type_ || t == types.dyad_ =>
                            {
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
            None => self.rt.store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut()),
            Some(t) => {
                // The place's width comes from the reading rule asked of the
                // type (`place_layout`, #82): the same table a read of the
                // place consults, so allocation and read cannot disagree. A
                // code-carrying type has no place — a node of it is a call
                // (#63), as `f ?` has none either — and text, void and prose
                // have none. One exception stays by identity: a `bool` place is
                // a 4-byte scalar by kind, but its literals cannot yet be
                // stored into one (#47), so declaring one is refused here
                // rather than allowed halfway.
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

    /// Take the pending declaration placeholder (see [`Parser::pending_fn`]):
    /// `fn`'s constructor claims it so a recursive self-call inside the body
    /// resolves the published signature.
    pub(crate) fn take_pending_fn(&mut self) -> DyadPtr {
        std::mem::replace(&mut self.pending_fn, std::ptr::null_mut())
    }

    /// Put a taken placeholder back — `fn`'s constructor suppresses the
    /// handoff around a literal that does not open its (sub-)expression, so a
    /// grouped literal deeper in the same declaration can still claim it.
    pub(crate) fn restore_pending_fn(&mut self, pending: DyadPtr) {
        self.pending_fn = pending;
    }

    /// True when `cell` stands as a completed operand at the frontier: a
    /// reduced dyad, or a token that does not extend — a resolved operand or a
    /// fresh name in waiting (null identity). A pending extender token is not
    /// an operand.
    pub(crate) fn is_operand_cell(&self, cell: &Cell) -> bool {
        match cell {
            c if c.constructed => true,
            // A fresh name resolves at consumption; a resolved token stands as
            // an operand only when nothing would construct it and it is not a
            // bare delimiter (`..`, `->`, `else`, `in`), which no operator
            // takes as an operand.
            c => {
                let id = self.cell_identity(c);
                c.is_fresh() || (self.ctor_of(id).is_none() && !self.is_delimiter(id))
            }
        }
    }

    /// Whether `id` is a bare delimiter token — an identity whose record is a
    /// parse-only token with no constructor (`)`, `,`, `->`, `else`, `in`,
    /// `..`): read by the constructs that spell them, never an operand.
    fn is_delimiter(&self, id: DyadPtr) -> bool {
        // SAFETY: as [`Parser::ctor_of`].
        unsafe {
            !id.is_null()
                && (*id).ty == self.types.type_
                && crate::identities::meta::kind_of(id) == Some(crate::identities::meta::TOKEN_TAG)
        }
    }

    /// Convert an operand cell to what a node stores for it — the one seam
    /// every constructor goes through. A reduced dyad passes; a resolved token
    /// yields the name's **record** (DESIGN ›The dyad's read surface‹: "a use
    /// of a name in code … stores the record, never the dyad"), rejecting a
    /// capture as the old scan-time push did; a fresh-name token re-resolves
    /// its span for the precise error, reported at the token's own start — the
    /// same message and position the eager driver produced at scan. A token
    /// the parser minted for an identity (no record) stands as the identity.
    /// Readers that need the value behind the operand take
    /// [`Parser::operand_dyad`] instead.
    pub(crate) fn as_operand(&mut self, cell: Cell) -> Result<DyadPtr, ParseError> {
        match cell {
            c if c.constructed => Ok(c.dyad),
            c => {
                let (id, record) = if c.is_fresh() {
                    match self.resolve_fresh(c.spelling()) {
                        Ok(r) => (r.identity, r.record),
                        Err(e) => {
                            self.pos = c.start;
                            return Err(ParseError::Resolve(e));
                        }
                    }
                } else {
                    (self.cell_identity(&c), c.record(self.types))
                };
                // SAFETY: `id` is a resolved dyad from the store.
                unsafe { self.check_capture(id)? };
                self.note_outer_read(record);
                Ok(if record.is_null() { id } else { record })
            }
        }
    }

    /// Whether the next token is a tight read of the cell to its left — `:`
    /// or `.` — that takes a right-side reader's own cell before the reader
    /// wakes (DESIGN ›Text is the quote‹: "the tight `.` runs over the token
    /// before its constructor wakes"; `:` beside it, and the reads above the
    /// readers on the axis, 8 September 2026). The seed's form of the rule:
    /// the model has the read, constructed at discovery when the reader lexes
    /// it at `tape[1]`, take the reader's cell; the seed's right-side drives
    /// lex onto a fresh tape, so the reader is put to sleep before it drives
    /// and the read finds it at the boundary. The readers are the identities
    /// at `prec::READER`; the raw-text consumers and `:=` sit above the reads
    /// and keep their right side.
    fn tight_read_takes(&mut self, id: DyadPtr) -> bool {
        if self.precedence_of_cell(id) != crate::identities::meta::prec::READER {
            return false;
        }
        matches!(self.peek_token(), Some((n, _)) if n == self.types.colon_ || n == self.types.dot_)
    }

    /// Construct the cells to the left of the cursor — the segment since the
    /// last boundary — to exactly one operand, and remove them from the tape:
    /// the place `=` reads before it drives its right side (DESIGN ›The
    /// scope's constructor is the driver‹, `=` beside `:=`, 8 September 2026).
    /// `None` when nothing stands to the left.
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

    /// What an identity's constructor places when it declines its right and
    /// "stands as its own value" (DESIGN ›The scope's constructor is the
    /// driver‹): the use of the name, its record, when the cell was lexed from
    /// a spelling — the bare identity only for a token the parser minted.
    pub(crate) fn stand_as_value(&self, tape: &ParsingTape, id: DyadPtr) -> DyadPtr {
        match tape.at(0).map(|c| c.record(self.types)) {
            Some(record) if !record.is_null() => record,
            _ => id,
        }
    }

    /// The dyad behind an operand cell: [`Parser::as_operand`] read through
    /// the reading rule — for the constructors that inspect or bind the value
    /// itself (a type to the left of `?` or `@`, the place `&` takes, `.`'s
    /// left side) rather than store a use of it.
    pub(crate) fn operand_dyad(&mut self, cell: Cell) -> Result<DyadPtr, ParseError> {
        let p = self.as_operand(cell)?;
        // SAFETY: `p` is a dyad from the store.
        let d = unsafe { self.types.through(p) };
        // A box the pass has already filled reads as what it holds, which is
        // what an operand position wants. An assignment's *target* does not
        // come through here, so `a = f64` still writes the box.
        Ok(self.settled_type(d))
    }

    /// The completed operand immediately left of the tape's cursor, converted
    /// — a tight extender's left context (`.`'s value, `@`'s pointer, `(`'s
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

    /// The two completed operands flanking the tape's cursor, converted — an
    /// infix construct's operands at reduction (the model's `tape[-1]` and
    /// `tape[+1]` reads). `Ok(None)` when either side is structurally missing
    /// (the construct was invoked fresh): the caller declines and the driver
    /// shifts the token instead.
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

    /// Peek the next token's identity and length without consuming it — the
    /// graph, not a schedule table, is what the callers compare against
    /// (`id == self.logos.else_`). `None` at end of input or when nothing
    /// resolves.
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

    /// Consume the next token if it is the identity `id`, reporting whether it
    /// was.
    fn consume_token(&mut self, id: DyadPtr) -> bool {
        match self.peek_token() {
            Some((t, matched)) if t == id => {
                self.pos += matched;
                true
            }
            _ => false,
        }
    }

    /// Consume the `(` that opens a field list, or fail.
    pub(crate) fn expect_open(&mut self) -> Result<(), ParseError> {
        if self.consume_token(self.types.open_) {
            Ok(())
        } else {
            Err(ParseError::ExpectedOpen)
        }
    }

    /// Whether the next token is an opening `(` (peek, no consume) — the
    /// lookahead `logos`'s merged constructor branches on: a `(` opens the
    /// record path, anything else yields the classifier itself.
    pub(crate) fn at_open(&mut self) -> bool {
        matches!(self.peek_token(), Some((id, _)) if id == self.types.open_)
    }

    /// Consume a `,` if the next token is one, reporting whether it was.
    fn consume_separator(&mut self) -> bool {
        self.consume_token(self.types.sep_)
    }

    /// Whether the next token is a closer, `)` or `]` (peek, no consume): a
    /// closer ends the body being parsed, and the opener's own expect-helper
    /// then checks it is the matching one.
    fn at_close(&mut self) -> bool {
        matches!(self.peek_token(), Some((id, _)) if id == self.types.close_ || id == self.types.close_sq_)
    }

    /// Read one spelling at the cursor — what the index lexes there, a
    /// declared name or a fresh run, never a bracket, the separator, a quote,
    /// or the comment mark — advancing past it and returning its `(start,
    /// len)`; `None` if nothing lexes. Declaration and member positions read
    /// spellings this way: a fresh one is not yet in the index to resolve, and
    /// a member resolves in its owner's scope, not here.
    fn lex_spelling(&mut self) -> Option<(usize, usize)> {
        self.lex_spelling_fresh().map(|(start, len, _)| (start, len))
    }

    /// [`Parser::lex_spelling`] keeping the index's answer to whether the
    /// spelling is fresh (nothing declared lexes there) — what a position that
    /// may hold either a declaration or a use, `for`'s first cell, decides by.
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

    /// Parse a `( field-list )` into a record node. `record_logos` is the identity
    /// that introduced it (`record`, or later `fn`'s parameter list). Fields are
    /// `name := T ?` or a bare `name`, separated by `,`; each becomes a `:`
    /// declaration dyad `{type: field-logos, value: undefined}` whose name is declared
    /// in the record's own scope. The node's value is a [`RECORD_TAG`] record
    /// storing the layout the definition derives — the scope, the `fields`
    /// array node, and the packed `size_bytes` — filled here, where the type's
    /// layout locks (issue #47; DESIGN ›a type whose constructor derives the
    /// layout automatically — reading the field declarations in its scope and
    /// filling `fields` and `size_bytes`‹). Fresh field names are read raw
    /// here, which is why the field list needs its own sub-parse rather than
    /// the generic driver.
    ///
    /// [`RECORD_TAG`]: crate::identities::meta::RECORD_TAG
    pub fn parse_record(&mut self) -> Result<DyadPtr, ParseError> {
        let record_logos = self.types.type_;
        let (scope, fields_arr, size_bytes) = self.parse_field_list(false)?;
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

    /// The field list `( name := T ?, name, … )`: its scope, its `fields`
    /// array, and the packed size. A `fn`'s parameter list checks each name
    /// against every open scope (the body reopens the list's scope, so a
    /// parameter may not shadow a name the body could still mean); an
    /// `instance = (…)` block's fields are checked against their siblings alone
    /// (`relaxed`; DESIGN ›The constructor is a field‹).
    fn parse_field_list(&mut self, relaxed: bool) -> Result<(DyadPtr, DyadPtr, u64), ParseError> {
        self.expect_open()?;
        // The record's own scope: a `scope`-typed node keyed by address for
        // open-scope membership. Field names are declared into it.
        let scope = self.open_scope();

        let mut fields = Vec::new();
        loop {
            if self.at_close() {
                break;
            }
            let (start, len) = self.lex_spelling().ok_or(ParseError::ExpectedField)?;
            // `self.source` is `&'a str` (Copy), so this slice is independent of the
            // `&mut self` the reentrant logos-parse and the declaration then need.
            let source = self.source;
            let name = &source[start..start + len];
            // `shared name := value` (DESIGN ›Two muts, and the storage
            // partition‹, 19 September 2026): one place stored with the type,
            // not a field of the layout — legal in an `instance = (…)` block alone.
            if name == "shared" {
                if !relaxed {
                    self.pos = start;
                    return Err(ParseError::SharedOutsideInstanceBlock);
                }
                self.shared_member(start)?;
                continue;
            }
            // A slot fill inside the block is written `shared run = (…)`: an
            // unmarked one would be a per-instance default, not in the seed.
            if relaxed && (name == "drop" || SLOT_NAMES.contains(&name)) {
                self.pos = start;
                return Err(ParseError::InstanceSlotNeedsShared);
            }
            // `name := T ?` declares the field's type through the hole `?`
            // built (DESIGN ›A function's surface‹: fields written `name :=
            // T ?`); a bare name leaves the field's type slot undefined.
            let logos = if self.consume_token(self.types.declare_tok) {
                let value = self.parse_expression()?;
                if self.holes.remove(&value) {
                    // SAFETY: `value` is the place `?` just built.
                    unsafe { (*value).ty }
                } else {
                    // SAFETY: `value` is a reduced dyad just parsed.
                    if unsafe { (*value).ty }.is_null() {
                        std::ptr::null_mut() // `name := ?`, the bare hole
                    } else {
                        self.pos = start;
                        return Err(ParseError::BadDeclaredType);
                    }
                }
            } else {
                std::ptr::null_mut()
            };
            let field = self.rt.store.alloc_raw(logos, std::ptr::null_mut());
            // The field's NAME is not stored on the record: declaring it here
            // puts a record in the shared name index, and resolution is
            // open-scope filtering over that one index (DESIGN ›Name resolution
            // is scope-filtered‹; a per-record names store is recorded as
            // rejected).
            if relaxed {
                // One block, one no-shadowing rule (DESIGN ›The constructor
                // is a field‹, 19 September 2026: the instance block is "the
                // scope where `y` is live … and no-shadowing applies as
                // before"): a field is checked against the `shared` members
                // too, which live in the type's own scope
                // ([`Parser::shared_member`]).
                let body = self
                    .definitions
                    .last()
                    .expect("a relaxed field list is an instance block")
                    .scope;
                if self.scopes.declared_in(self.trie, name, body).map_err(ParseError::Resolve)? {
                    self.pos = start;
                    return Err(ParseError::Resolve(ResolveError::Shadowed(name.to_string())));
                }
                self.declare_field_name(name, field, start)?;
            } else {
                self.declare_name(name, field, start)?;
            }
            fields.push(field);
            if !self.consume_separator() {
                break;
            }
        }

        self.scopes.pop();
        self.expect_close()?;

        // The stored layout: fields pack in declaration order, a scalar at its
        // type's width and anything else (a bare or type-valued name, only
        // meaningful for parameter lists) as the 8-byte container — the same
        // width rule parameters claim frame offsets by.
        // SAFETY: each field is the dyad just built, its type null or a type
        // node.
        let size_bytes: u64 = fields.iter().map(|&f| unsafe { field_width((*f).ty) }).sum();
        let fields_arr = crate::identities::array::build(self.rt.store, self.types.array_, &fields);
        Ok((scope, fields_arr, size_bytes))
    }

    /// `shared name := value` inside an `instance = (…)` block (DESIGN ›Two
    /// muts, and the storage partition‹, ruled 19 September 2026: "`shared x
    /// := …` is one place stored with the type, reached through every node
    /// and through the type itself; an unmarked member is a place per node").
    /// The declaration is made in the type's own body scope, not the field
    /// list's, so it is stored once — the storage the type's bare members had
    /// until 19 September 2026 (#87) — pending like every body line and run at
    /// the body's close; read `g.y` through the type. Reading it through an
    /// instance, `a.y`, waits on #116 (deliberate-deferred, #133). The block
    /// stays one scope for no-shadowing: the member is checked against the
    /// fields declared before it, as a field is against the members
    /// ([`Parser::parse_field_list`]). `shared` itself is a word of the block
    /// and never a field's name (ruled 19 September 2026, Thobias: "reserved
    /// inside instance inside type"), so `shared := …` is the missing
    /// declaration.
    fn shared_member(&mut self, at: usize) -> Result<(), ParseError> {
        let body = match self.definitions.last() {
            Some(def) => def.scope,
            None => {
                self.pos = at;
                return Err(ParseError::SharedOutsideInstanceBlock);
            }
        };
        if self.consume_token(self.types.declare_tok) {
            self.pos = at;
            return Err(ParseError::SharedNeedsDeclaration);
        }
        let field_scope = self.scopes.pop().expect("the field list's scope is open");
        self.definitions.last_mut().expect("checked above").in_block = true;
        let items = self.shared_line();
        self.definitions.last_mut().expect("checked above").in_block = false;
        self.scopes.push(field_scope);
        // The line's one declaration — a member or a slot fill, both declare
        // nodes — with its prose lifted out beside it, as every segment's is;
        // anything else is not what `shared` marks.
        let mut declared = None;
        for item in items? {
            // SAFETY: `item` is a reduced dyad just parsed.
            let ty = unsafe { (*item).ty };
            if ty == self.types.comment_ {
                // SAFETY: the pending records were minted by this parser's declares.
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
            let name = std::str::from_utf8(self.declared_name(item))
                .expect("a spelling is source text")
                .to_string();
            if self
                .scopes
                .declared_in(self.trie, &name, field_scope)
                .map_err(ParseError::Resolve)?
            {
                self.pos = at;
                return Err(ParseError::Resolve(ResolveError::Shadowed(name)));
            }
        }
        // SAFETY: the pending records were minted by this parser's declares.
        unsafe { self.scopes.settle_item(body, item) };
        Ok(())
    }

    /// The items of the segment after `shared`: the first [`Parser::parse_next`]
    /// lexes and constructs it and queues what it yielded, its expression and
    /// the prose lifted out of it; the rest are taken while they stand queued,
    /// before anything further is lexed, so none is left for the enclosing
    /// body's loop to take as a line of its own.
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

    /// The spelling a declare node declares: its name string, as
    /// `declare::build` lays the node out, the string node at 0.
    fn declared_name(&self, item: DyadPtr) -> &[u8] {
        // SAFETY: `item` is a declare node from the store.
        unsafe { crate::reflect::text_of(*((*item).value as *const DyadPtr)) }
    }

    /// Whether a declare node is one of the slot fills — its name a slot's
    /// ([`SLOT_NAMES`]), the node [`Parser::slot_fill`] yields — rather than a
    /// member declaration. The spelling decides, which is sound while the slot
    /// words are reserved: no `:=` can declare one.
    fn is_slot_fill(&self, item: DyadPtr) -> bool {
        let text = self.declared_name(item);
        SLOT_NAMES.iter().any(|s| s.as_bytes() == text)
    }

    /// `type (…)`'s body (DESIGN ›The constructor is a field‹, #61; 19
    /// September 2026): an ordinary scope whose bare lines fill the type's
    /// own slots (its constructor, its parse_rank) — a bare `:=` line is the
    /// checked error — while its `instance = (…)` block holds what lives on
    /// instances, a `shared` member stored once among them. The slot words
    /// ([`SLOT_NAMES`]) are root identities in the seed, so `parse_rank := 5`
    /// is the no-shadowing error and `parse_rank = …` the fill
    /// ([`Parser::slot_fill`], the instance block's
    /// [`Parser::instance_block_fill`]). Every other line must be prose: a
    /// type body is
    /// definition-time code and nothing runs it later, so a line that would
    /// only run is refused. The type node then carries the instance layout,
    /// this scope as its body (members read `g.y`), the parse_rank and
    /// associativity filled (the call defaults otherwise), and the Logos
    /// constructor installed in its slot.
    pub fn parse_type_body(&mut self, id: DyadPtr) -> Result<DyadPtr, ParseError> {
        self.expect_open()?;
        let scope = self.open_scope();
        self.definitions.push(OpenType {
            scope,
            parse_rank: crate::identities::meta::prec::APPLY,
            lex_rank: None,
            assoc: Assoc::Left,
            ctor: std::ptr::null_mut(),
            run: std::ptr::null_mut(),
            run_body: std::ptr::null_mut(),
            instance: None,
            this_param: std::ptr::null_mut(),
            in_block: false,
        });
        // A `fn` literal on a slot's right side must not claim the enclosing
        // declaration's placeholder (`x := type (parse = fn …)`).
        let suppressed = self.take_pending_fn();
        // The body's declarations are pending until its close, whatever
        // encloses it: a type is comptime and its members are stored at the
        // definition (#87). Everything before the body runs first, so a
        // member's initializer reads committed state, and what fails among
        // the body's own lines is reported as the body's.
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
            // The rank is the name's, not the type's (#122): it goes on the
            // record of the declaration this body is the value of. A body
            // standing nowhere a name is being declared has no record to
            // write, the checked error.
            let Some(&record) = self.filling.last() else {
                return Err(ParseError::LexRankNeedsName);
            };
            // SAFETY: `record` is a record dyad `:=` minted before driving
            // its value, and it is live for the whole drive.
            unsafe { Record::set_lex_rank(record, rank) };
        }
        if !def.ctor.is_null() {
            // SAFETY: `node` was just built; nothing has read its slot.
            unsafe { crate::identities::meta::install_constructor(node, def.ctor) };
        }
        if !def.run.is_null() {
            // SAFETY: as above; `def.run` is the fn node the body's fill checked.
            unsafe { crate::identities::meta::install_code(node, def.run) };
        }
        if !def.run_body.is_null() {
            // SAFETY: as above; `def.run_body` is the `lex` node the fill built.
            unsafe { crate::identities::meta::install_run_body(node, def.run_body) };
        }
        Ok(node)
    }

    /// The lines of a type body, each settled as the body item of the
    /// names it declared and checked to be one of the kinds a body holds.
    fn type_body_lines(&mut self, scope: DyadPtr) -> Result<(), ParseError> {
        // The body's declarations run at the definition, the one pass over
        // a type body as over a file ([`Self::run_imported`]): a `shared`
        // member of the instance block ([`Self::shared_member`]) is stored
        // once, read `g.y`. Without the run, a typed member's initializer
        // never stored and `g.y` read the zeroed place (#87), while the
        // untyped `y := 3` worked because it folds at parse. The lines are
        // pending as they parse and run at the body's close
        // ([`Self::parse_type_body`]); this loop only checks what each line
        // is.
        while let Some(item) = self.parse_next() {
            let item = item?;
            // SAFETY: the pending records were minted by this parser's declares.
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
                // A bare `instance` line stands as the marker and declares nothing,
                // the same error as any other line that declares nothing (#87).
                BodyLine::Other => return Err(ParseError::TypeBodyLine),
                // A bare `:=` line is not allowed in a type body (DESIGN ›The
                // constructor is a field‹, 19 September 2026: "a body's bare
                // lines only fill slots with `=`, and every member, shared or
                // per node, is declared in the `instance = (…)` block").
                BodyLine::Declare if !self.is_slot_fill(item) => {
                    return Err(ParseError::MemberOutsideInstanceBlock);
                }
                BodyLine::Prose | BodyLine::Declare => {}
            }
        }
        Ok(())
    }

    /// `instance = (…)`'s fill (#128, spelled `value` until 19 September 2026; DESIGN ›The constructor is a field‹: the
    /// `value` block "saying what the value slot of every node of it holds",
    /// "a slot filled with `=` like the rest"): the block of per-instance
    /// fields (`name := T ?`, a place per instance), parsed as a field list
    /// checked against its siblings alone. `=` calls this before reading its
    /// right side, since the bracket is a field list, not an expression. The
    /// line's item is a declare node over the marker itself, whose null type
    /// the declaration's run skips.
    pub(crate) fn instance_block_fill(&mut self) -> Result<DyadPtr, ParseError> {
        let def = self.definitions.last().expect("slot_of found an open definition");
        // `shared instance = (…)` inside the block is the instances' own
        // `instance` slot, not in the seed (#133): refused like their parse
        // trio, never read and thrown away — the enclosing block is stored
        // only when its list closes, so the double-block check below would
        // not see it.
        if def.in_block {
            return Err(ParseError::InstanceSlotNotInSeed);
        }
        if def.instance.is_some() {
            return Err(ParseError::DoubleInstance);
        }
        let instance = self.parse_field_list(true)?;
        self.definitions.last_mut().expect("checked above").instance = Some(instance);
        let types = self.types;
        let name = SlotKind::Instance.name();
        let name_node =
            crate::identities::string::build_text(self.rt.store, types.string_, name.as_bytes());
        Ok(crate::identities::declare::build(
            self.rt.store,
            types.declare_,
            types.ops.declare_,
            name_node,
            types.slots[SlotKind::Instance as usize],
        ))
    }

    /// Which slot `target` names, if any: a use of one of the slot words
    /// ([`Core::slots`]), or of `drop`. Whether that fill reaches a type
    /// being defined is [`Parser::filling_definition`]'s question.
    ///
    /// # Safety
    /// `target` must be a dyad from the store.
    pub(crate) unsafe fn slot_of(&self, target: DyadPtr) -> Option<SlotKind> {
        // The word arrives as the record of its use, or as the identity
        // itself when its own constructor stood aside (`drop`).
        // SAFETY: `target` is a reduced dyad from the store.
        unsafe {
            let id =
                if (*target).ty == self.types.record_ { Record::read(target).dyad } else { target };
            if id == self.types.drop_ {
                return Some(SlotKind::Drop);
            }
            self.types.slots.iter().position(|&m| m == id).map(SlotKind::of)
        }
    }

    /// Whether `=` here fills a slot of the type being defined: the current
    /// scope is the innermost open definition's own (DESIGN ›The constructor
    /// is a field‹, 19 September 2026: "`=` with a slot word on its left fills
    /// the innermost enclosing definition's slot on a bare line and its
    /// instances' slot inside the block, and outside any definition it is the
    /// checked error"). A `parse_rank = 3` inside a constructor's body is
    /// that function's own business, refused.
    pub(crate) fn filling_definition(&self) -> bool {
        self.definitions.last().is_some_and(|def| self.scopes.current() == Some(def.scope))
    }

    /// Fill a slot of the type being defined (DESIGN ›The constructor is a
    /// field‹: "A slot `type` declared is filled with `=`"). The parse_rank
    /// is the value's number, run now — `*.parse_rank + 1` is comptime field
    /// arithmetic — so it must be known at the definition (the seed's form of
    /// "resolved by the operator's first use"); the associativity is `left`
    /// or `right`; the constructor (`parse`) a function taking the tape by
    /// value; the `value` block is filled before the right side is read
    /// ([`Parser::instance_block_fill`]), so it never arrives here. The fill is
    /// a silent statement, the declare node the type variable's fill yields.
    /// The number a rank slot's right side stands for: a literal molds to
    /// f64 without running; a concrete expression runs now, on the pass's
    /// runtime, after everything parsed before it (a rank may read a
    /// variable). Anything else is [`ParseError::NonComptimeRank`].
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
                // SAFETY: as above.
                unsafe { self.run_on_pass(value) }.map_err(|_| ParseError::NonComptimeRank)?
            }
        };
        Ok(match nt {
            None | Some(NumType::F64) => f64::from_bits(bits as u64),
            Some(NumType::F32) => f64::from(f32::from_bits(bits as u32)),
            Some(_) => bits as f64,
        })
    }

    ///
    /// # Safety
    /// `value` must be a dyad from the store.
    pub(crate) unsafe fn slot_fill(
        &mut self,
        kind: SlotKind,
        value: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        // Which slot a fill reaches (DESIGN ›The constructor is a field‹, 19
        // September 2026): inside the instance block, `shared run = (…)` is
        // the instances' run; the instances' parse trio and drop there, and a
        // type's own run or drop on a bare line, are not in the seed yet
        // (#133), each refused with its own message.
        let in_block = self.definitions.last().expect("slot_of found an open definition").in_block;
        match kind {
            SlotKind::Drop => return Err(ParseError::DropSlotNotInSeed),
            SlotKind::Run if !in_block => return Err(ParseError::OwnRunNotInSeed),
            SlotKind::Run => {}
            _ if in_block => return Err(ParseError::InstanceSlotNotInSeed),
            _ => {}
        }
        // SAFETY: `value` is a reduced dyad from the store.
        let read = unsafe { types.through(value) };
        // A rank is a number the pass needs now; computed before the open
        // definition is borrowed, since computing it may run what stands
        // before it.
        let rank = match kind {
            SlotKind::ParseRank | SlotKind::LexRank => Some(self.rank_value(value, read)?),
            _ => None,
        };
        let def = self.definitions.last_mut().expect("slot_of found an open definition");
        match kind {
            SlotKind::ParseRank => {
                if let Some(rank) = rank {
                    def.parse_rank = rank;
                }
            }
            SlotKind::LexRank => {
                if let Some(rank) = rank {
                    def.lex_rank = Some(rank);
                }
            }
            SlotKind::Associativity => {
                def.assoc = if read == types.left_ {
                    Assoc::Left
                } else if read == types.right_ {
                    Assoc::Right
                } else {
                    return Err(ParseError::BadAssociativity);
                };
            }
            SlotKind::Parse => {
                // SAFETY: `read` is a reduced dyad; a fn node's value is its
                // five slots, its input a record whose fields are the params.
                let takes_tape = unsafe {
                    (*read).ty == types.fn_type && {
                        let input = *((*read).value as *const DyadPtr).add(FN_INPUT);
                        let params = crate::identities::array::items(
                            crate::identities::meta::record_fields_of(input),
                        );
                        // The wrapped form takes the tape alone; a body read
                        // bare takes `tape` and `this` ([`Parser::slot_body_fill`]).
                        (params.len() == 1 || (params.len() == 2 && (*params[1]).ty == types.dyad_))
                            && (*params[0]).ty == types.tape.parsing_tape
                    }
                };
                if !takes_tape {
                    return Err(ParseError::BadConstructorSignature);
                }
                def.ctor = read;
            }
            SlotKind::Instance | SlotKind::Drop => {
                unreachable!(
                    "`drop` returned above; `instance` is routed to instance_block_fill by `=`"
                )
            }
            // `shared run = fn …`: the function an instance of the type runs
            // and compiles as (#63; DESIGN ›Execution is function
            // application‹) — the 4 September shape, kept beside the bare
            // body ([`Parser::slot_body_fill`]) until that body runs (#133
            // slice 9).
            SlotKind::Run => {
                // SAFETY: `read` is a reduced dyad from the store.
                if unsafe { (*read).ty } != types.fn_type {
                    return Err(ParseError::BadRunSlot);
                }
                def.run = read;
            }
        }
        let name = kind.name();
        let name_node =
            crate::identities::string::build_text(self.rt.store, types.string_, name.as_bytes());
        Ok(crate::identities::declare::build(
            self.rt.store,
            types.declare_,
            types.ops.declare_,
            name_node,
            value,
        ))
    }

    /// `parse = (…)` and `shared run = (…)`: a slot body read bare (#133
    /// slice 5; DESIGN ›Execution is function application‹, 17 September
    /// 2026: "`=` drives its right side … and constructs it by the left
    /// place's type's `parse`, so `parse = ( … )` with a place of type
    /// `parse` on the left reads the bracket as a deferred parse body, `run =
    /// ( … )` as a deferred run body … the `fn (tape := parsing_tape ?) ->
    /// void` wrapper a parse slot carried is gone, `tape` being a word inside
    /// `parse` as `this` is … so a slot body has no parameter list at all").
    /// `=` calls this in place of reading an expression when a body slot
    /// stands on its left and a `(` on its right
    /// ([`crate::identities::assign`]).
    ///
    /// `parse`: the body is read as `fn`'s is, over a hidden parameter record
    /// declaring `tape` as the parsing tape, so the function it yields is the
    /// one the explicit wrapper yielded and the constructor runs unchanged
    /// ([`Parser::run_logos_ctor`]). `run`: the body is held, not read — it
    /// is a body over `this`, whose fields' types no definition knows (DESIGN
    /// ›Deferral is authored‹, 20 September 2026: "the body is held as its
    /// lexed tape … and constructed once per field-type set when a node
    /// supplies the types"), and the seed holds it as the one tape-fragment
    /// value it has, the `lex` node over the body's text, since its `(` reads
    /// a bracket from text. Building a node that constructs and runs it is
    /// #133 slice 8; until then the type's `run` reads as that fragment and
    /// applying the type is refused ([`ParseError::RunBodyHeld`]).
    pub(crate) fn slot_body_fill(&mut self, kind: SlotKind) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let in_block = self.definitions.last().expect("slot_of found an open definition").in_block;
        match kind {
            // The instances' own parse is not in the seed (#133), as for the
            // wrapped form.
            SlotKind::Parse if in_block => Err(ParseError::InstanceSlotNotInSeed),
            SlotKind::Parse => {
                let at = self.pos;
                // The two names `parse` declares into its body: `tape`, the
                // tape centred on the appearance, and `this`, the fresh node
                // (a `dyad ?` place holding it; [`crate::identities::this`]).
                let (input, params) = self.hidden_param_record(
                    &[(Some("tape"), types.tape.parsing_tape), (Some("this"), types.dyad_)],
                    at,
                )?;
                let def = self.definitions.last_mut().expect("checked above");
                def.this_param = params[1];
                // SAFETY: `input` was just built, its parameters unplaced; no
                // declaration's placeholder is being filled.
                let f = unsafe {
                    self.fn_over_body(types.fn_type, input, types.void_, std::ptr::null_mut())
                };
                self.definitions.last_mut().expect("checked above").this_param =
                    std::ptr::null_mut();
                // SAFETY: `f` is the fn node just built over the body.
                unsafe { self.slot_fill(SlotKind::Parse, f?) }
            }
            // A type's own run, what runs when the type itself is used
            // plainly, is not in the seed (#133), as for the wrapped form.
            SlotKind::Run if !in_block => Err(ParseError::OwnRunNotInSeed),
            SlotKind::Run => {
                // The body with its brackets, copied into the store — a
                // source may not outlive the type (a REPL line), the store
                // does — and lexed once, now, against the scopes open here
                // (DESIGN ›Deferral is authored‹, 20 September 2026: "lexed
                // at the definition"); the cells are what the type holds.
                let (start, len) = self.body_text_extent()?;
                let source = self.source;
                let text = crate::identities::string::build_text(
                    self.rt.store,
                    types.string_,
                    &source.as_bytes()[start - 1..start + len + 1],
                );
                // SAFETY: `text` is the string node just built; its bytes
                // live for the store.
                let held = unsafe { crate::identities::string::text(text) };
                let held = std::str::from_utf8(held).expect("copied from the source text");
                let fragment = self.lex_body_fragment(held)?;
                let cells = Box::into_raw(Box::new(fragment));
                let body = crate::identities::run_body::build(self.rt.store, types, text, cells);
                self.definitions.last_mut().expect("checked above").run_body = body;
                // The line's item: a declare node over the slot word, as the
                // instance block's is ([`Parser::instance_block_fill`]).
                let name = SlotKind::Run.name();
                let name_node = crate::identities::string::build_text(
                    self.rt.store,
                    types.string_,
                    name.as_bytes(),
                );
                Ok(crate::identities::declare::build(
                    self.rt.store,
                    types.declare_,
                    types.ops.declare_,
                    name_node,
                    types.slots[SlotKind::Run as usize],
                ))
            }
            _ => unreachable!("`=` reads a bare body for `parse` and `run` only"),
        }
    }

    /// A parameter record `( name := ty ?, … )` no text spelled: the input of
    /// a slot body's function, its parameters the names the slot's type
    /// declares into the body (`tape` and `this` for `parse`). Built as
    /// [`Parser::parse_field_list`] builds a `fn`'s list — each name declared
    /// in the record's own scope and checked against every open scope, since
    /// the body reopens that scope — the fields' value slots left null for
    /// [`Parser::fn_over_body`] to place in the frame. Returns the record and
    /// its fields in order. `at` is where a shadowing error is reported: the
    /// `=` the body stands after.
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
            // way — a run body's field, through `this.f`.
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

    /// `this.f` in a parse body: the slot of the node being built that the
    /// instance field `f` names (DESIGN, 16 September 2026: "the constructor
    /// connects each operand to a field by name"; 20 September 2026: "the
    /// instance block stands before any body that reads `this.f` … a name is
    /// declared before its use as everywhere else"). `f` resolves in the
    /// block's own scope alone; its index among the fields is the slot.
    fn this_field(&mut self, name: &str, at: usize) -> Result<DyadPtr, ParseError> {
        let def = self.definitions.last().expect("this_param is set inside a definition");
        let this = def.this_param;
        let Some((scope, fields, _)) = def.instance else {
            self.pos = at;
            return Err(ParseError::ThisNeedsInstanceBlock);
        };
        let mut field_scope = ScopeStack::new();
        field_scope.push(scope);
        let field = field_scope.resolve(self.trie, name).map(|r| r.identity);
        // SAFETY: `fields` is the block's array node, its items the field dyads.
        let items = unsafe { crate::identities::array::items(fields) };
        let index = field.ok().and_then(|f| items.iter().position(|&x| x == f));
        let Some(index) = index else {
            self.pos = at;
            return Err(ParseError::ThisFieldUnknown(Box::new(name.to_string())));
        };
        let k = self.scalar_value(crate::identities::numtype::NumType::U64, index as i64);
        let types = self.types;
        Ok(crate::identities::this::build_slot(self.rt.store, types, this, k))
    }

    /// `this.f` in a run body under construction: the field's parameter
    /// place — the frame place the node's operand is evaluated into, the
    /// second of a field's two moments (DESIGN ›Execution is function
    /// application‹, item 4: "a field is filled at run, per evaluation, the
    /// node's storage being its frame") — or, for a field of type `type`,
    /// the type this field-type set holds in it, so `this.output 1` reads
    /// `i32 1` where the set's output is `i32` (#133 slice 8: "a field of
    /// type `type` whose value is known at that construction folds to the
    /// identity"). `f` resolves in the instance block's scope alone, as in a
    /// parse body.
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
        // SAFETY: the fields are the instance block's declaration dyads.
        let is_type = unsafe { (*fields[i]).ty } == self.types.type_;
        Ok(if is_type { key[i] } else { places[i] })
    }

    /// The run of a node whose type holds its `run` as a lexed body (#133
    /// slice 8; DESIGN ›Deferral is authored‹, 20 September 2026:
    /// "constructed once per field-type set when a node supplies the types …
    /// kept on the type for every later `^` over i32"): the node's field-type
    /// set is read off its slots ([`Parser::field_type_key`]), the function
    /// built for that set is found on the type or built now
    /// ([`Parser::construct_run_body`]), and the node points at it through
    /// the slot after its operand run ([`crate::identities::run_body::spec_of`]).
    /// A field whose type is not yet known leaves the slot empty: the node
    /// neither runs nor lowers until this runs again with the type at hand
    /// (ruled 20 September 2026: the choice is a function of the field
    /// types, rerun when a type arrives).
    ///
    /// # Safety
    /// `node` must be the node [`Parser::run_logos_ctor`] minted for a type
    /// whose record carries a run body, `[field…, null, spec]`.
    pub(crate) unsafe fn resolve_specialization(
        &mut self,
        node: DyadPtr,
        at: usize,
        spelling: &str,
    ) -> Result<(), ParseError> {
        let ty = (*node).ty;
        let held = crate::identities::meta::run_body_of(ty);
        if held.is_null() {
            return Ok(());
        }
        let Some(key) = self.field_type_key(ty, node)? else {
            return Ok(());
        };
        let mut spec = crate::identities::run_body::lookup(held, &key);
        if spec.is_null() {
            spec = self.construct_run_body(ty, held, &key, at, spelling)?;
        }
        crate::identities::run_body::set_spec(node, spec);
        Ok(())
    }

    /// The field-type set a node was built with, one type per instance
    /// field in order (DESIGN ›Execution is function application‹, item 4:
    /// "the field's type decides how the operand is used: `rhs := i32 ?`
    /// evaluates the operand into an i32 (an uncommitted literal molds to it
    /// as a call argument does) … a hole with no type, `lhs := ?`, takes the
    /// type of the first value written into it"): a typed field is its type,
    /// a literal written into it committed to that type in the slot; a field
    /// of type `type` is the type it holds; an untyped hole is the type of
    /// what the constructor wrote — a concrete number's type, or
    /// `rational_number` for a bare literal (ruled 20 September 2026: the
    /// literal's type, no silent i32). `None` while some field's type is
    /// not yet known.
    ///
    /// # Safety
    /// As [`Parser::resolve_specialization`]; `ty` the node's type.
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
                if matches!(numtype_of(types, slot), Operand::Literal)
                    && crate::identities::is_numtype_node(types, declared)
                {
                    let lit = types.through(slot);
                    *slots.add(i) =
                        crate::identities::commit_literal_to(self.rt.store, types, lit, declared)?;
                }
                declared
            } else {
                match numtype_of(types, slot) {
                    Operand::Concrete(nt) => types.numtypes[nt as usize],
                    Operand::Literal => types.rational,
                    Operand::Pointer(_) | Operand::NonNumeric => return Ok(None),
                }
            };
            key.push(entry);
        }
        Ok(Some(key))
    }

    /// Build the function a held run body is, for one field-type set. The
    /// body's cells, lexed once at the definition, are constructed now, the
    /// parser reading its next cell from them ([`Feed`]) with the type's own
    /// scopes open — the scope its body was written in and that scope's
    /// ancestors, never the use site's — over hidden parameters: `this`, and
    /// one unnamed place per field typed by the set, so `this.f` in the body
    /// is the field's place ([`Parser::run_body_field`]) and the function's
    /// parameters are the node's operands in order, which is what lets a
    /// node run and lower as a call of it ([`crate::run::Runtime::apply`],
    /// [`crate::compile::Lowerer::lower_call_to`]). The function is entered
    /// on the type before the body is read, so a use of the type inside its
    /// own body over the same set resolves to it, as a recursive `fn` reads
    /// its own early signature; its value is filled in when the body is
    /// built, and a failed construction leaves nothing behind. The state the
    /// pass keeps per source is swapped out and back as an import's is.
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
        // each construction gets its own, and the held cells stay as lexed.
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
        let saved_lifted = std::mem::take(&mut self.lifted);
        let saved_queued = std::mem::take(&mut self.queued);
        let saved_discovering = std::mem::replace(&mut self.discovering, false);
        let saved_run_body = self.run_body.take();
        let saved_feed = self.feed.replace(Feed { cells, next: 0, base_depth });

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

    /// The construction itself, over the swapped-in state
    /// ([`Parser::construct_run_body`]): the hidden parameter record, the
    /// function's return type — the type the set holds in the field named
    /// `output`, or `void` when the type declares none (DESIGN ›Execution is
    /// function application‹, item 5: "the output is per node and the
    /// constructor defines it") — and the body read as a function's.
    ///
    /// # Safety
    /// As [`Parser::construct_run_body`].
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
        // constructor recognizes ([`Parser::field_access`]), declared in the
        // parameter scope the body reopens.
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

    /// The text inside the `( … )` at the cursor, consumed with its brackets
    /// and constructed by nothing: `(start, len)` into the source. The end is
    /// the `)` closing the opener, found by lexing token by token with the
    /// index and counting the brackets between — a `«…»` quote is one token,
    /// so a bracket inside it is text, and a `#` comment is passed over as
    /// the comment constructor reads it, to its line's end or its quote's —
    /// so that what is held is what the driver would have read. A body that
    /// never closes is [`ParseError::UnclosedBracket`].
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
                // The line form ends at the newline; the string form, `#
                // «…»`, is the quote token the lexer reads next.
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

    /// Parse a function literal `fn ( params ) -> ret ( body )` (DESIGN ›A
    /// function's surface‹), given `fn_type` (the resolved `fn` identity). The
    /// parameter list is a `record` (a field list); the return type after `->`
    /// is a driven segment to the body bracket (`@i32` is one); the body is a
    /// `( )` scope parsed with the parameter scope reopened, so parameters
    /// resolve inside it. The node is `{type: fn, value -> [input, output,
    /// body, bcode, frame, outer]}` ([`FN_INPUT`] … [`FN_OUTER`]): the params
    /// record, the return type, the reflectable body, the compiled `bcode`
    /// (null until [`crate::compile::compile_fn`] installs it), the frame
    /// size, and the outer names the body reads.
    ///
    /// A function's value is what its body evaluates to; an explicit `return`
    /// is *optional*, `return X` and `X` yielding the same value in tail
    /// position.
    ///
    /// `declared` (null when the literal does not open a declaration's value) is
    /// the declaration's placeholder: the signature publishes onto it — body and
    /// bcode still null — before the body parses, so a recursive self-call inside
    /// the body reads real parameter and return type.
    ///
    /// # Safety
    /// `declared` must be null or a placeholder dyad from the store that nothing
    /// has read a value from yet; the early signature is written through it.
    pub unsafe fn parse_fn(
        &mut self,
        fn_type: DyadPtr,
        declared: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        // The parameter list is a record; parse_record opens and closes its scope.
        let input = self.parse_record()?;
        self.expect_arrow()?;
        // The return type: the cells up to the body bracket, constructed to
        // one (`i32`, `@i32`, later `array i32`).
        let output = {
            let items = self.drive_until_open(RightSide::ReturnType)?;
            let out = self.one_of(items).map_err(|e| match e {
                ParseError::Empty => ParseError::ExpectedReturnType,
                e => e,
            })?;
            // A named return type (`i32`, `void`) is a use of that name: its
            // record, read through to the type it names.
            // SAFETY: `out` is a reduced dyad from the store.
            unsafe { self.types.through(out) }
        };
        // SAFETY: `input` was just built by `parse_record`; `declared` is the
        // caller's placeholder under this function's own contract.
        unsafe { self.fn_over_body(fn_type, input, output, declared) }
    }

    /// The function node over the `( body )` at the cursor, given its
    /// signature: `input` a parameter record whose fields are still unplaced
    /// (as [`Parser::parse_record`] leaves them) and `output` the return type
    /// identity. The half of [`Parser::parse_fn`] after the signature, shared
    /// with a slot body read bare ([`Parser::slot_body_fill`]): the frame is
    /// opened and the parameters placed in it, the body parsed deferred with
    /// the parameter scope reopened, and the node `[input, output, body,
    /// bcode, frame, outer]` built.
    ///
    /// # Safety
    /// `input` must be a record node whose parameters' value slots are still
    /// null; `declared` as for [`Parser::parse_fn`].
    unsafe fn fn_over_body(
        &mut self,
        fn_type: DyadPtr,
        input: DyadPtr,
        output: DyadPtr,
        declared: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        // Open this function's frame and give the parameters its first per-call
        // byte offsets — a call frame is an instance of its function, so a
        // parameter resolves to a frame slot exactly as a local does (DESIGN
        // ›Resolution is one rule‹), and the caller writes the argument values
        // into those slots (›Operands travel on the stack‹). A scalar-typed
        // parameter stores at its type's width, like a local of that type;
        // anything else — a bare `name`, a type-valued parameter — rides the
        // full 8-byte i64 bit-container the call convention already passes.
        // The body's local declarations then claim the offsets after these; a
        // nested `fn` literal pushes its own frame, so its state never lands
        // in this one. The barrier pushed below begins at the current scope
        // depth — nothing between here and it opens a scope — so a record
        // declared at a lesser depth is a name from outside the function.
        self.frames.push(OpenFn { size: 0, below: self.scopes.depth(), outer: Vec::new() });
        let depth = self.frames.len();
        // SAFETY: `input` is the record just built; its record stores the
        // fields array, and each parameter's value slot is still the null
        // parse_record left there.
        unsafe {
            let fields = crate::identities::meta::record_fields_of(input);
            for &param in crate::identities::array::items(fields) {
                let logos = (*param).ty;
                // A `type`-typed parameter is a frame place holding a type:
                // DESIGN ›A type is a comptime value‹ (12 September 2026) —
                // "a type value is a node address like any other value, so it
                // may be passed to a function, held in a place, and compared
                // ... a place holding a type is therefore an ordinary place".
                // It rides the 8-byte container like every other non-scalar,
                // which is what the width below already gives it; what had to
                // change was every reader that took the frame tag for a record
                // ([`crate::identities::meta::kind_of`], #75).
                // A parameter's slot is a place of its type, sized by the same
                // rule that sizes a local (`place_layout`, #82). A bare
                // parameter has no type and holds the 8-byte container its call
                // binds. A record parameter now gets its layout's width rather
                // than the container's 8 — the call still binds only the
                // container into it until the convention copies bytes (#115).
                // `logos` is null or a type node from the store (inside the
                // enclosing SAFETY block).
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
            // SAFETY: `declared` is the just-declared placeholder; nothing has read
            // a value from it yet, and the fixpoint overwrites it when the value
            // completes.
            unsafe {
                (*declared).value = early;
            }
        }

        // Reopen the parameter scope (stored in the input record's record) so
        // the body resolves parameters, then parse the `( body )` — a deferred
        // body (it runs at calls, not at parse), so parse-time rebinding is
        // off inside.
        // SAFETY: `input` is the record just built; its record stores its scope.
        let scope = unsafe { crate::identities::meta::record_scope_of(input) };
        // The body runs later: names declared outside it may not be moved or
        // dropped inside (the parameters, declared in the scope pushed next,
        // may — a parameter is same-level with the body, DESIGN ›A function's
        // surface‹).
        self.scopes.push_barrier();
        self.scopes.push(scope);
        self.runtime_depth += 1;
        self.expect_open()?;
        let body = self.parse_sequence()?;
        self.expect_close()?;
        self.runtime_depth -= 1;
        self.scopes.pop();
        self.scopes.pop_barrier();
        // Ownership may not cross a function return yet (issue #49). A block can
        // hand ownership to its binder because the parse sees the block's tail,
        // but a *call* hides the body behind the return type, and a plain `@T`
        // carries no destructor — so the caller could not know it owes a `free`
        // and would leak. Fail closed until a return type can declare that it
        // hands ownership over, which is the ownership-gate work (issue #53).
        // SAFETY: `body` is the reduced dyad just parsed.
        if unsafe { crate::identities::drop_model::is_owning_value(self.types, body) } {
            return Err(ParseError::OwnershipAcrossReturn);
        }
        let OpenFn { size: frame_size, outer, .. } =
            self.frames.pop().expect("parse_fn pushed a frame");

        // A comptime-rational tail expression commits to the declared return type here
        // (the typed slot), so `fn () -> i64 ( 2000000000 + 2000000000 )` returns i64
        // rather than molding to the i32 default.
        // SAFETY: `body`/`output` are valid dyads just built.
        let body =
            unsafe { crate::identities::commit_fn_body(self.rt.store, self.types, body, output)? };

        // `bcode` starts null; `compile_fn` installs the exec@ into that slot.
        // FN_FRAME holds the activation-record byte size — parameters first,
        // locals after, a `u64` leaf both tiers read on entry — or null when
        // the function declares no parameters and no locals.
        let frame = if frame_size == 0 {
            std::ptr::null_mut()
        } else {
            let bytes = self.rt.store.alloc_bytes(&(frame_size as u64).to_ne_bytes());
            let u64_ty = self.types.numtypes[crate::identities::NumType::U64 as usize];
            self.rt.store.alloc_raw(u64_ty, bytes)
        };
        // FN_OUTER holds the records of the outer names the body read, which
        // every call of the function is a use of (#125) — null when it read
        // none.
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

    /// Parse a conditional `if cond ( then )` with an optional `else ( else )`
    /// (given the resolved `if` identity). The condition is whatever stands
    /// before the body bracket, a `( cond )` group included; the bodies are
    /// brackets; and the condition must be a `bool` ([`ParseError::NonBoolCondition`]). The
    /// node is `{type: if, value: [cond, then, else]}`, the else slot null when the
    /// `else` is absent: run takes the branch the condition selects, compile emits a
    /// two-way branch. An else-less `if` is a statement — it yields unit — so value
    /// positions reject it ([`ParseError::MissingElse`]); and because branches are
    /// always parenthesized, a nested `if` cannot capture an outer `else` (no
    /// dangling else). `else if ( cond ) ( then ) …` is sugar for a nested `if` in
    /// the else slot, so chains parse right-associatively without `else ( if … )`.
    /// Unlike `fn`, `if` opens no new scope — its parts resolve in the enclosing one.
    pub fn parse_if(&mut self, if_type: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The condition: the cells up to the body bracket, constructed to one
        // — a `(…)` group, a bare bool name, `not x`, `x == 1` alike (ruled 5
        // September 2026) — required to be a bool.
        let items = self.drive_until_open(RightSide::Condition)?;
        let cond = self.one_of(items)?;
        let types = self.types;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(types, cond) } {
            return Err(ParseError::NonBoolCondition);
        }

        // A comptime condition — a bool literal, the shape `true`/`false` and
        // every parse-time fold produce (`a.logos == i32`, two-literal
        // comparisons) — resolves the conditional NOW, in the one pass: the
        // taken branch parses in place and an untaken branch's tokens are
        // dropped unlexed, so nothing inside it is resolved, committed, or
        // declared. This is what lets branches for *other* comptime logos
        // coexist (`a=9.9` under `a := i32 ?` parses only in the world where it is
        // taken). SAFETY: `cond` is the reduced dyad just parsed.
        if let Some(truth) = unsafe { bool_literal_value(types, cond) } {
            return self.parse_comptime_if(if_type, cond, truth);
        }

        // Then-branch. A runtime branch may or may not run, so parse-time
        // rebinding is off inside it (and inside the else below).
        self.runtime_depth += 1;
        self.expect_open()?;
        let then = self.parse_sequence()?;
        self.expect_close()?;

        // The optional `else`, then the else-branch; absent, the slot stays null
        // and the `if` is a unit-valued statement. `else if ( cond ) ( then ) …` is
        // sugar: an `if` right after the `else` becomes the else-branch directly
        // (unparenthesized), so a chain nests right-associatively into `if` nodes
        // and needs no hand-written `else ( if … )`. The nested `if` carries its own
        // value-ness — else-less it is unit, exactly as the explicit form is — so the
        // sugar builds a structurally identical tree and introduces no new case.
        let els = if self.consume_else() {
            if self.consume_token(self.types.if_) {
                self.parse_if(if_type)?
            } else {
                self.expect_open()?;
                let els = self.parse_sequence()?;
                self.expect_close()?;
                els
            }
        } else {
            std::ptr::null_mut()
        };
        self.runtime_depth -= 1;

        let value = self.rt.store.alloc_operands(&[cond, then, els, self.types.ops.if_]);
        Ok(self.rt.store.alloc_raw(if_type, value))
    }

    /// Resolve an `if` whose condition is already a parse-time bool (roadmap #30).
    /// True with an else: the then-branch parses in place and IS the result (the
    /// if's value is the taken branch's), the dead else-tail dropped unparsed.
    /// False: the then-branch is dropped unparsed; an `else if` continues the
    /// chain through [`Parser::parse_if`] (comptime or not) and an `else ( … )`
    /// body is the result. An else-less `if` stays an ordinary statement `if`
    /// node in both cases — it yields unit whether or not its condition is
    /// comptime-known, so folding must not turn it into a value — with the
    /// then-branch parsed when true (it runs) and dropped when false (the
    /// condition doubles as a harmless never-run then-slot dummy).
    fn parse_comptime_if(
        &mut self,
        if_type: DyadPtr,
        cond: DyadPtr,
        truth: bool,
    ) -> Result<DyadPtr, ParseError> {
        if truth {
            self.expect_open()?;
            let then = self.parse_sequence()?;
            self.expect_close()?;
            if self.consume_else() {
                self.skip_else_tail()?;
                return Ok(then);
            }
            // Else-less: a statement yielding unit, comptime or not — folding
            // to the branch's value would make the same text a value or a
            // statement depending on whether the condition is comptime-known.
            // Keep the ordinary `if` node (the then-branch parsed; it runs).
            let value = self.rt.store.alloc_operands(&[
                cond,
                then,
                std::ptr::null_mut(),
                self.types.ops.if_,
            ]);
            return Ok(self.rt.store.alloc_raw(if_type, value));
        }
        self.skip_group()?;
        if self.consume_else() {
            if self.consume_token(self.types.if_) {
                return self.parse_if(if_type);
            }
            self.expect_open()?;
            let els = self.parse_sequence()?;
            self.expect_close()?;
            return Ok(els);
        }
        let value =
            self.rt.store.alloc_operands(&[cond, cond, std::ptr::null_mut(), self.types.ops.if_]);
        Ok(self.rt.store.alloc_raw(if_type, value))
    }

    /// Drop a balanced `( … )` group without parsing it — the tape's `remove`
    /// power in its minimal form (DESIGN ›a constructor may splice tokens in or
    /// drop upcoming ones before they lex‹). Comptime-`if` uses it to discard an
    /// untaken branch, so nothing inside is resolved, committed, or declared.
    /// `«…»` text (the byte pair `C2 AB` … `C2 BB`, unambiguous in UTF-8) and
    /// `#` prose (a `«…»` string, or raw text to the line's end) are skipped
    /// opaquely — their parentheses are text, not structure. An unterminated
    /// group or text is [`ParseError::UnclosedBracket`].
    fn skip_group(&mut self) -> Result<(), ParseError> {
        /// Skip a `«…»` span starting at `pos` (which must point at `«`),
        /// returning the position just past the `»`, or `None` if unterminated.
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
                    // exactly as the comment constructor reads it.
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

    /// Drop an already-`else`d dead tail without parsing it: `if ( cond )
    /// ( then )` links (looping while further `else`s follow) or the final
    /// `( body )`. Used when a comptime-true condition has taken its branch and
    /// the rest of the chain can never run.
    fn skip_else_tail(&mut self) -> Result<(), ParseError> {
        loop {
            if self.consume_token(self.types.if_) {
                self.skip_group()?; // ( cond )
                self.skip_group()?; // ( then )
                if self.consume_else() {
                    continue;
                }
                return Ok(());
            }
            return self.skip_group(); // else ( body )
        }
    }

    /// Build a logical negation `not operand` (given the resolved `not`
    /// identity) over the constructed cell to its right, which must be a
    /// `bool` ([`ParseError::NonBoolOperands`]). The node is `{type: not,
    /// value: operand}`.
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
        // A bool-literal operand folds now (pure, nothing lost), like the
        // `==`/`and`/`or` folds — what keeps a comptime chain comptime.
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

    /// Parse a loop `while ( cond ) ( body )` (given the resolved `while` identity).
    /// Both parts are parenthesized; the condition must be a `bool`
    /// ([`ParseError::NonBoolCondition`]) and is re-evaluated before each iteration;
    /// the body runs for effect, its value discarded (DESIGN ›a loop body's is
    /// thrown away‹). The node is `{type: while, value: [cond, body]}`, a statement
    /// yielding unit: value positions reject it ([`ParseError::StatementAsValue`]),
    /// and a `return` in the body is rejected ([`ParseError::EarlyReturn`]) since v1
    /// has no unwinding to exit the loop with.
    pub fn parse_while(&mut self, while_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The condition: the cells up to the body bracket (as `if`'s).
        let items = self.drive_until_open(RightSide::Condition)?;
        let cond = self.one_of(items)?;
        let types = self.types;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(types, cond) } {
            return Err(ParseError::NonBoolCondition);
        }
        // A repeated body: parse-time rebinding is off inside it, and a name
        // declared outside it may not be moved or dropped inside.
        self.runtime_depth += 1;
        self.scopes.push_barrier();
        self.expect_open()?;
        let body = self.parse_sequence()?;
        self.expect_close()?;
        self.scopes.pop_barrier();
        self.runtime_depth -= 1;
        // SAFETY: `body` is the reduced dyad just parsed.
        if unsafe { contains_return(types, body) } {
            return Err(ParseError::EarlyReturn);
        }
        let value = self.rt.store.alloc_operands(&[cond, body, self.types.ops.while_]);
        Ok(self.rt.store.alloc_raw(while_id, value))
    }

    /// Parse a counted loop `for i in a..b ( body )` / `for i in a..b..d ( body )`,
    /// or the same loops with no index, `for a..b ( body )` (given the resolved
    /// `for` identity; DESIGN ›The scope's constructor is the driver‹, ruled
    /// 17 September 2026: "`for a..b (body)` with no index is the same loop
    /// without the variable"; #129). The range is end-exclusive, the cells up to
    /// the body bracket ([`Parser::drive_until_open`]). The counter is a fresh
    /// block-local of the range's resolved numeric type either way — declared
    /// under the name when one is written, under no name otherwise — so both
    /// tiers run one node shape; a literal step must be positive
    /// ([`ParseError::BadStep`]); the loop is a statement yielding unit, and a
    /// `return` in the body is rejected ([`ParseError::EarlyReturn`], no
    /// unwinding to exit with).
    pub fn parse_for(&mut self, for_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        let name = self.loop_name()?;
        // The range: the cells up to the body bracket — `start .. end` or
        // `start .. end .. step` — constructed by parse_rank, the `..` cells
        // inert delimiters read by position.
        let parts = self.drive_until_open(RightSide::Condition)?;
        let dotdot = self.types.dotdot_;
        let types = self.types;
        // The `..` cells are uses of that identity: its record, read through.
        // SAFETY: every item `drive_until_open` returned is a dyad from the store.
        let is_dotdot = |d: &DyadPtr| unsafe { types.through(*d) } == dotdot;
        let (start, end, step) = match parts.as_slice() {
            [(s, _), (d, _), (e, _)] if is_dotdot(d) => (*s, *e, None),
            [(s, _), (d, _), (e, _), (d2, _), (st, _)] if is_dotdot(d) && is_dotdot(d2) => {
                (*s, *e, Some(*st))
            }
            _ => return Err(ParseError::ExpectedRange),
        };

        // Resolve the loop logos across the range parts (concrete type must
        // match, literals commit, all-literals default to i32).
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
                // The rule says what the step's value is before its bytes are
                // read: a scalar in untagged literal storage (#82). A marked
                // place here would mean the literal check above let a variable
                // through, and reading its tagged offset as an address is the
                // crash class the rule exists to end.
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

        // The loop variable: a fresh per-call local of the loop logos (a frame slot
        // inside a function, an absolute blob at top level).
        // SAFETY: `logos` is a numtype node from resolve_loop_parts.
        let width = unsafe { crate::identities::numtype::of_type_node(logos) }.bytes();
        let var = self.alloc_local(logos, width);
        let parent = self.scopes.current().unwrap_or(std::ptr::null_mut());
        let scope = crate::identities::scope::mint(self.rt.store, types.scope, parent);
        // A repeated body: a name declared outside it may not be moved or
        // dropped inside; the loop variable, declared in the scope pushed next,
        // is inside.
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
        if unsafe { contains_return(types, body) } {
            return Err(ParseError::EarlyReturn);
        }

        let value =
            self.rt.store.alloc_operands(&[var, start, end, step, body, self.types.ops.for_]);
        Ok(self.rt.store.alloc_raw(for_id, value))
    }

    /// The loop variable a `for` names, if it names one: the cell after `for`
    /// decides. A spelling followed by `in` is the name. A fresh spelling not
    /// followed by `in` wanted it ([`ParseError::ExpectedIn`]): nothing
    /// declared lexes there, so it can begin no range. Anything else begins
    /// the range, and the cursor goes back for the range read to lex it.
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

    /// Resolve a field access `lhs.name` to a *place*: an ordinary numeric node
    /// over the instance's storage at the field's byte offset (DESIGN ›Resolution
    /// is one rule‹ — the declaration found decides, and a field declaration is
    /// the offset inside the value area). The field name resolves in the record
    /// type's own scope, alone (never against the enclosing scopes). The `.` has
    /// already been consumed.
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
        // `.` does exactly one job (ruled August 2026): reading fields the
        // logos defines, which are always about the value. A value's type is
        // never one of its own fields — the retired universal `.logos`
        // metaproperty did a second job here — so reading a type takes the
        // dyad view, `x:dyad.type`, where the type IS in the value.
        // The member name is the cell to the right of `.`, read as its raw
        // spelling: a field is dot-only, so what the driver resolved it to
        // against the open scopes is beside the point. A `[i]` cell after it
        // (`index`) and an empty `()` (`unit_call`) are the reads that take
        // one more cell; the count consumed to the right is returned.
        {
            // `source` is `&'a str` (Copy), independent of the `&mut self` the
            // member reads then need.
            let source = self.source;
            let name = &source[nstart..nstart + nlen];
            // `this.f` inside a parse body: a field of the node being built
            // (#133 slice 6), before the dyad view below, since `this` is a
            // `dyad ?` place.
            if let Some(def) = self.definitions.last() {
                if !def.this_param.is_null() && lhs == def.this_param {
                    return self.this_field(name, nstart).map(|n| (n, 0));
                }
            }
            // `this.f` inside a held run body being constructed: the field's
            // place in the function being built (#133 slice 8).
            if self.run_body.as_ref().is_some_and(|rb| lhs == rb.this_param) {
                return self.run_body_field(name, nstart).map(|n| (n, 0));
            }
            // `t[k]:dyad.type`: the cell's type, read (#61). The write through
            // it, the retype, and `.value.operands.append(…)` were the 4
            // September shape, deleted with `this` (ruled 20 September 2026).
            if (*lhs).ty == self.types.tape.slot_dyad {
                let types = self.types;
                return match name {
                    "type" => {
                        Ok((crate::identities::tape::build_cell_type(self.rt.store, types, lhs), 0))
                    }
                    _ => Err(ParseError::BadReflectRead),
                };
            }
            if (*lhs).ty == self.types.dyad_ {
                return self.view_member(lhs, name).map(|n| (n, 0));
            }
            // `here.scope`, `caller.scope`, and a scope's own `.scope` (#123;
            // DESIGN ›Meta-navigation‹: "`here.scope` the scope that spot is
            // in, `here.scope.scope` the one above, and so up to the arche").
            // `here` knows its scope at the appearance, so the read folds to
            // the address value `x:scope` yields; `caller` reads the pass, so
            // its `.scope` is a node run inside a constructor; and `.scope`
            // over any scope address reads the parent link when it runs.
            let h = self.types.here;
            if (*lhs).ty == h.here || (*lhs).ty == h.caller {
                if name != "scope" {
                    return Err(ParseError::BadReflectRead);
                }
                let node = if (*lhs).ty == h.here {
                    let scope = crate::identities::here::scope_of_here(lhs);
                    self.address_value(self.types.dyad_, scope)
                } else {
                    crate::identities::here::build_caller_scope(self.rt.store, self.types)
                };
                return Ok((node, 0));
            }
            if name == "scope" && crate::identities::here::yields_scope_address(self.types, lhs) {
                let node = crate::identities::here::build_scope_of(self.rt.store, self.types, lhs);
                return Ok((node, 0));
            }
            if crate::identities::is_type_value(self.types, lhs) {
                let n = self.logos_member(lhs, name, index)?;
                return Ok((n, usize::from(name == "roles")));
            }
            // A place holding a type: its fields are the identity's, and which
            // identity that is nobody knows until the program runs. DESIGN ›A
            // type is a comptime value‹ (12 September 2026) says where that
            // becomes readable — "under interpretation the graph can change as
            // it runs, so a type reached at runtime can still be followed to
            // its fields, which is the reflection of *Metareflection from
            // within the language*" (#52). Until that runs, saying so.
            if matches!(
                crate::identities::read::read_kind(self.types, lhs),
                crate::identities::read::Read::Container(t) if t == self.types.type_ || t == self.types.dyad_
            ) {
                return Err(ParseError::TypeKnownOnlyAtRun);
            }
            if name == "type" {
                return Err(ParseError::TypeNeedsView);
            }
            // An operator node's slots are the fields its own logos defines
            // (#52, corrected August 2026): `.operands` is the collection
            // that type defines, and `[i]` fetches an element from it —
            // `(x + x).operands[0]` — no view involved, exactly as `p.x`
            // reads a record field. A null slot (an absent optional) is the
            // ruled checked error until `?`.
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
            // `.compile` on an fn-typed value is the fn type's shared member
            // (DESIGN ›Execution is function application‹: "The `fn` logos
            // carries two shared functions: `compile` … and `run`"; `run` is
            // calling). `f.compile()` builds a compile statement whose run
            // lowers `f`'s body and installs its `bcode`, so the next call
            // jumps to machine code. The name-compare here is the seed's
            // stand-in for shared-member resolution through the type's scope
            // (one mechanism at self-hosting); reserved only on fn-typed
            // values, so a record field named `compile` still resolves. The
            // `()` is mandatory — compile is a function, applied like any
            // other, taking no arguments (DESIGN ›Operands travel on the
            // stack‹). The callable leaf is minted NOW, entry zero, because
            // minting needs the store the parser holds; the run patches the
            // finalized entry in.
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
        // Through a record pointer, `p@.x` folds the field offset into the deref
        // (the address is runtime; the offset and the field's logos are not).
        // A tape's natives (#60): a member of `parsing_tape`'s own scope that
        // is not a laid-out field — `t.remove(k)`, `t.insert(k, cell)`,
        // `t.recenter(k)`, and the indexed `t.is_constructed[k]` and
        // `t.spelling[k]` — built as a call with the receiver's address first.
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
        if (*lhs).ty == self.types.deref_ {
            let (ptr_expr, pointee, base_off) = crate::identities::pointer::deref_parts(lhs);
            if pointee.is_null() || !crate::identities::meta::is_record_type(pointee) {
                return Err(ParseError::UnsupportedOperands);
            }
            let (field, offset) = self.resolve_field(pointee, nstart, nlen)?;
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
        // The direct case: an instance of a record type, with storage — the
        // access is a *place*, its offset folded into the instance's own place
        // now. `wrapping_add` keeps a frame-tagged instance value a valid tagged
        // offset (`FRAME_TAG | (base + field)`); for an absolute instance it is
        // ordinary pointer arithmetic. `place_addr` resolves it at run/lower time.
        let record_logos = (*lhs).ty;
        if record_logos.is_null()
            || !crate::identities::meta::is_record_type(record_logos)
            || (*lhs).value.is_null()
        {
            return Err(ParseError::UnsupportedOperands);
        }
        let (field, offset) = self.resolve_field(record_logos, nstart, nlen)?;
        let addr = (*lhs).value.wrapping_add(offset);
        Ok((self.rt.store.alloc_raw((*field).ty, addr), 0))
    }

    /// `.`'s constructor: the member read of `tape[-1]` named by the cell to
    /// the right, plus the `[i]` or `()` cell some reads take (see
    /// [`Parser::field_access`]); every consumed cell is spliced out.
    pub(crate) fn construct_field_access(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // The left is read as it stands — an identity's fields are read off
        // the token before its own constructor wakes (DESIGN ›Text is the
        // quote‹: `i32.parse_rank`), so a callable or a type name to the
        // left is the identity itself, not a call in waiting.
        let lhs = match tape.at(-1).copied() {
            Some(cell) => self.operand_dyad(cell)?,
            None => return Err(ParseError::MissingOperand),
        };
        // The member is the cell to the right, read by its spelling at the
        // offset it was lexed at — a keyword there (`.type`) was constructed
        // at discovery and stands as a dyad, its spelling still in the source.
        let Some(m) = self.cell_at(tape, 1)? else {
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
        // The optional `[i]` or `(…)` after the member: lexed on demand. A
        // bracket is the member's argument list (empty: `f.compile()`).
        self.cell_at(tape, 2)?;
        let index = self.index_at(tape, 2);
        let bracket = tape.at(2).filter(|c| c.is_bracket()).map(|c| c.dyad);
        // SAFETY: a bracket cell is a node from the store.
        let call = bracket.map(|d| unsafe { self.args_of(d) });
        let key = self.index_node_at(tape, 2);
        // SAFETY: `lhs` is a reduced dyad off the tape.
        let (node, consumed) = unsafe { self.field_access(lhs, nstart, nlen, index, key, call)? };
        for _ in 0..(1 + consumed) {
            tape.remove(1);
        }
        tape.remove(-1);
        // A type identity a run body's `this.f` folded to stands as its own
        // unconstructed cell — the driver's "another identity's turn" — so
        // it reads its right side as anywhere else: `this.output 1` is
        // `i32 1` where the set's output is `i32` (#133 slice 8).
        // SAFETY: `node` is a node from the store.
        let folded_type = self.run_body.is_some()
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

    /// The comptime index a `[…]` cell carries, if the cell at `offset` is
    /// one and its interior is a non-negative literal — what the reflection
    /// reads (`.operands[i]`, `.roles[i]`) fold at parse.
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

    /// The interior a `[…]` cell carries, if the cell at `offset` is one:
    /// any expression, for the reads whose natives run it
    /// (`t.is_constructed[k]`).
    fn index_node_at(&self, tape: &ParsingTape, offset: isize) -> Option<DyadPtr> {
        let c = tape.at(offset)?;
        if !c.constructed {
            return None;
        }
        let d = c.dyad;
        // SAFETY: a dyad cell is a node from the store; an index node's value
        // is its interior first.
        unsafe {
            if (*d).ty != self.types.index_ {
                return None;
            }
            Some(*((*d).value as *const DyadPtr))
        }
    }

    /// `[`'s constructor. `[` is `(` in square brackets (ruled 9 September
    /// 2026): the interior is parsed as any bracket's — the eager-segment
    /// loop, to one expression — and closed by its `]`, which it consumes
    /// itself. What it lands is the passive index cell (DESIGN ›The
    /// constructor is a field‹: "`[…]` constructs itself … into a passive
    /// node carrying the index"), or, right after a tape value, the element
    /// read `t[k]` — the tape's constructor consumes the bracket to its
    /// right (ruled the same day), which the seed folds here, `[` reading
    /// its left cell, since a tape value has no constructor of its own yet.
    /// The index is any expression: the natives run it.
    pub(crate) fn construct_index(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let key = self.parse_sequence()?;
        self.expect_close_sq()?;
        // `t[k]` (#60): an index right after a tape value is the element
        // read, a slot node the identity to its left owns; after a `.` the
        // index stays a passive cell for the member read to consume.
        if let Some(left) = tape.at(-1).copied() {
            // A fresh spelling to the left is a member name after `.` (or an
            // unknown name the boundary reports), never a tape.
            if !left.is_fresh() && self.is_operand_cell(&left) {
                let lhs = self.operand_dyad(left)?;
                let types = self.types;
                // SAFETY: `lhs` is a reduced dyad from the store.
                let recv =
                    unsafe { crate::identities::tape::receiver_addr(self.rt.store, types, lhs) };
                if let Some(recv) = recv {
                    let node = crate::identities::tape::build_slot(self.rt.store, types, recv, key);
                    tape.remove(-1);
                    tape.place(node);
                    return Ok(Constructed::Placed);
                }
            }
        }
        let value = self.rt.store.alloc_operands(&[key, std::ptr::null_mut()]);
        let node = self.rt.store.alloc_raw(self.types.index_, value);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Resolve the field name spelled at `nstart..nstart + nlen` against `record_logos`'s own scope
    /// alone (its value[0] — an enclosing binding of the same spelling can never
    /// shadow or double a field), returning the field node and its byte offset.
    ///
    /// # Safety
    /// `record_logos` must be a record type node from the store.
    unsafe fn resolve_field(
        &mut self,
        record_logos: DyadPtr,
        nstart: usize,
        nlen: usize,
    ) -> Result<(DyadPtr, usize), ParseError> {
        let source = self.source;
        let name = &source[nstart..nstart + nlen];
        let mut field_scope = ScopeStack::new();
        field_scope.push(crate::identities::meta::record_scope_of(record_logos));
        let field = field_scope.resolve(self.trie, name).map_err(ParseError::Resolve)?.identity;
        let (fields, _) = crate::identities::instance::layout(record_logos)?;
        let (_, _, offset) = fields
            .iter()
            .copied()
            .find(|&(f, _, _)| f == field)
            .ok_or(ParseError::ExpectedField)?;
        Ok((field, offset))
    }

    /// Whether `callee` is a function whose declared return type is the `logos` root —
    /// it yields a type, resolved at comptime (roadmap #30).
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

    /// Comptime-evaluate a type-returning call to the concrete type it produces,
    /// substituting that type node for the call. The call runs under a fresh
    /// interpreter — which works off raw handles and never touches the store — so
    /// interpretation doubles as parse-time evaluation (DESIGN ›Build and run are one
    /// self-directing pass‹); the result bits are the produced type node's address.
    /// A run failure (e.g. a runtime-only argument) or a non-logos result is reported
    /// as [`ParseError::NonComptimeTypeCall`].
    ///
    /// # Safety
    /// `call` must be a reduced call node from the store.
    unsafe fn eval_type_call(&mut self, call: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The pass needs the identity now: what stands before the call runs
        // first, so the call reads committed state.
        self.drain()?;
        let bits = self.run_on_pass(call).map_err(|_| ParseError::NonComptimeTypeCall)?;
        let node = bits as usize as DyadPtr;
        // The bits are read as a node address, so they must be one. A `-> type`
        // body whose tail is not a type is already refused at the definition
        // (`check_type_tail`, #76); this is the second wall, against any path
        // that ever hands back something else — bits that were never a node
        // are the checked error here, never a dereference.
        if !self.rt.store.contains(node) {
            return Err(ParseError::NonComptimeTypeCall);
        }
        if crate::identities::is_type_value(self.types, node) {
            Ok(node)
        } else {
            Err(ParseError::NonComptimeTypeCall)
        }
    }

    /// Build a postfix dereference `lhs@`: the lhs's static type must be a
    /// pointer type — a pointer variable or `&x` literal (its `logos`), a pointer
    /// field place, or another deref whose pointee is a pointer (`p@@`).
    ///
    /// # Safety
    /// `lhs` must be a reduced dyad from the store.
    pub(crate) unsafe fn build_deref(&mut self, lhs: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The pointer expression is stored as it stands (a use of a name is
        // its record); its type is read through the reading rule.
        let read = self.types.through(lhs);
        let ptr_ty = if (*read).ty == self.types.deref_ {
            crate::identities::pointer::deref_parts(read).1
        } else {
            (*read).ty
        };
        // The left side's type must be a pointer type, by the rule (#82); the
        // pointee rides on the answer.
        let Some((crate::identities::read::Read::Pointer(pointee), _)) =
            crate::identities::read::place_layout(self.types, ptr_ty)
        else {
            return Err(ParseError::UnsupportedOperands);
        };
        let types = self.types;
        Ok(crate::identities::pointer::build_deref(self.rt.store, types, lhs, pointee, 0))
    }

    /// `&`'s constructor: the address of the place to its right — a name, or
    /// a `.field` chain already constructed (`.` binds tighter) — a numeric,
    /// pointer, or record-typed node with a value slot. Yields an `addr` node
    /// (see [`crate::identities::pointer::build_addr`]) that resolves the
    /// place's address at run/lower time, so a frame-relative local or
    /// parameter yields a per-activation address. A comptime binding has no
    /// storage and is [`ParseError::BadAddressOf`].
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
            // A place the reading rule can name — a scalar, or a record
            // instance — that is marked as storage (#82). A comptime binding
            // has no storage; a literal's untagged blob is not a place (`&(i32
            // 5)` used to yield its address); and a node typed by a
            // code-carrying type, `&(2 ^ 3)`, has an operand run where an
            // instance has fields — its "address" was a pointer into the graph
            // that faulted on the first read. All three are refused.
            use crate::identities::read::{read_kind, Read};
            let placed = matches!(
                read_kind(self.types, node),
                Read::Scalar(_) | Read::Pointer(_) | Read::Aggregate
            ) && crate::dyad::is_place((*node).value);
            if !placed {
                return Err(ParseError::BadAddressOf);
            }
            // No taking the address of an enclosing function's local or
            // parameter (a capture).
            self.check_capture(node)?;
            // `&` is a runtime address-of node (like `@` deref), not a baked
            // literal: it resolves the place's address through `place_addr` at
            // run/lower time, so a frame-relative local or parameter yields a
            // per-activation address — a different one on each recursive call,
            // exactly like C.
            crate::identities::pointer::build_addr(self.rt.store, self.types, node)
        };
        tape.remove(1);
        tape.place(addr);
        Ok(Constructed::Placed)
    }

    /// The place operand of `own`/`drop`/`free` (issue #49): the cell to the
    /// right — a resolved name, or a `.field` chain already constructed (`.`
    /// binds tighter) — ending at a storage-backed place, yielded as the
    /// *place node itself* (not an `addr`), which the teardown builder reads
    /// to check owning-ness and reach the pointer's storage. A capture (an
    /// enclosing frame's local) is rejected. The cell is consumed.
    ///
    /// With `ends_name`, the operand is one that makes its name dead (`own`,
    /// `drop`; never `free`, the raw teardown): a bare name comes back as the
    /// [`Ended`] the caller hands to [`Parser::mark_dead`] once its node
    /// exists, and a name declared outside a loop or `fn` body the operand
    /// sits in is refused ([`ParseError::OwnOfOuterName`], DESIGN ›Memory and
    /// concurrency‹, *Bodies that run again or later*). A field path empties
    /// the field and leaves the name (its own marks are #66).
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
                Some(Ended { record: r.record })
            } else {
                None
            };
            self.note_outer_read(r.record);
            (r.identity, ended)
        };
        // SAFETY: `node` is a resolved dyad from the store.
        unsafe {
            self.check_capture(node)?;
        }
        tape.remove(1);
        Ok((node, ended))
    }

    /// The operand cell to the right of the cursor, consumed: the one read
    /// every prefix constructor makes (`return x`, `not x`, `alloc i32 5`,
    /// `dyad x`, a prefix `-`). [`ParseError::MissingOperand`] when nothing
    /// constructed stands there.
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

    /// Mint the record for `identity` declared in `scope` under `spelling`:
    /// the six fields, the spelling as a string node (`a:name`, #120).
    fn mint_record(&mut self, identity: DyadPtr, scope: DyadPtr, spelling: &[u8]) -> DyadPtr {
        let name =
            crate::identities::string::build_text(self.rt.store, self.types.string_, spelling);
        Record::alloc(self.rt.store, self.types.record_, Record::new(identity, scope, name))
    }

    /// Declare `name` in the current scope as `identity`, minting its record —
    /// one per declared name, a dyad of type `record` (DESIGN ›`mut` is a gate
    /// on the record‹) — and return it. Errors are the no-shadowing and index
    /// errors of [`ScopeStack::declare`], reported at `at`, the name's own
    /// offset in the source, so the caret lands on the name and not where the
    /// declaration happened to end.
    pub(crate) fn declare_name(
        &mut self,
        name: &str,
        identity: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let scope = self.scopes.current().expect("declare needs an open scope");
        let record = self.mint_record(identity, scope, name.as_bytes());
        // SAFETY: `record` was minted by `Record::alloc` just above.
        if let Err(e) = unsafe { self.scopes.declare(self.trie, name, record) } {
            self.pos = at;
            return Err(ParseError::Resolve(e));
        }
        Ok(record)
    }

    /// Declare the pattern `key` in the current scope as `identity`, minting
    /// its record — the pattern twin of [`Parser::declare_name`] (#114).
    pub(crate) fn declare_pattern(
        &mut self,
        key: &str,
        identity: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let scope = self.scopes.current().expect("declare needs an open scope");
        let record = self.mint_record(identity, scope, key.as_bytes());
        // SAFETY: `record` was minted by `Record::alloc` just above.
        if let Err(e) = unsafe { self.scopes.declare_pattern(self.trie, key, record) } {
            self.pos = at;
            return Err(ParseError::Resolve(e));
        }
        Ok(record)
    }

    /// Declare `name` as a field, checked against its siblings alone
    /// ([`ScopeStack::declare_field`]); a failure is reported at `at`, the
    /// name's offset.
    fn declare_field_name(
        &mut self,
        name: &str,
        identity: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let scope = self.scopes.current().expect("declare needs an open scope");
        let record = self.mint_record(identity, scope, name.as_bytes());
        // SAFETY: `record` was minted by `Record::alloc` just above.
        if let Err(e) = unsafe { self.scopes.declare_field(self.trie, name, record) } {
            self.pos = at;
            return Err(ParseError::Resolve(e));
        }
        Ok(record)
    }

    /// The `own`/`drop` node `node` has emptied `ended`'s place: its name is dead
    /// from here on (DESIGN ›Memory and concurrency‹, *`own` and `drop` are
    /// static*). See [`ScopeStack::mark_dead`].
    pub(crate) fn mark_dead(&mut self, ended: Ended, node: DyadPtr) {
        // SAFETY: `ended.record` is the record the resolver returned for the operand.
        unsafe { self.scopes.mark_dead(ended.record, node) };
    }

    /// Consume an `else` if the next token is one, reporting whether it was.
    fn consume_else(&mut self) -> bool {
        self.consume_token(self.types.else_)
    }

    /// Consume the `->` that separates a fn's parameter list from its return type.
    fn expect_arrow(&mut self) -> Result<(), ParseError> {
        if self.consume_token(self.types.arrow_) {
            Ok(())
        } else {
            Err(ParseError::ExpectedArrow)
        }
    }

    /// The prefix `@` over cells: every further `@` cell to the right, then
    /// the base type cell — any type identity (`@i32`, `@@point`, `@dyad`,
    /// `@void`; DESIGN ›Pointer types are prefix `@T`‹: "`@i32` is a pointer
    /// to an i32, composing as `@@i32` and applying to any type (`@point`,
    /// `@dyad`)") — built into the pointer type; the consumed cells are
    /// spliced out. What a place of `@T` reads as never depends on `T`
    /// (`place_layout`: eight bytes, an address), so nothing more is asked of
    /// the base; a place, a literal, or a fresh name is refused (#124).
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
        // SAFETY: `base` is a resolved dyad from the store. A pointer type as
        // the base is an inner `@` already constructed at discovery (`@@point`).
        if !unsafe { crate::identities::is_type_value(self.types, base) } {
            return Err(ParseError::UnsupportedOperands);
        }
        tape.remove(1);
        let mut logos = base;
        for _ in 0..depth {
            // SAFETY: `logos` is the base type node the tape held, or the
            // pointer type minted in the previous round.
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

    /// Build a call `callee ( args )` over the arguments its bracket cell
    /// held — the callee's own constructor's work (DESIGN ›`X (…)` is one
    /// spelling, and X's constructor decides what the bracket is‹). A numeric
    /// logos callee is a conversion (`i32(a)`), a record type constructs an
    /// instance, a type-returning callee resolves NOW at comptime; any other
    /// callee is an ordinary call.
    ///
    /// # Safety
    /// `callee` and every `arg` must be dyads from the store.
    pub(crate) unsafe fn build_call(
        &mut self,
        callee: DyadPtr,
        args: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        let args = args.to_vec();
        // An owning value handed straight to a call has no name to hang its
        // `defer free` on, so it would leak (issue #49; DESIGN's open
        // temporary-attachment point). Fail-closed until ownership-gated
        // parameters land (issue #53) and the callee can declare that it takes
        // the value.
        for &arg in &args {
            // SAFETY: `args` are reduced dyads just parsed.
            if unsafe { crate::identities::drop_model::is_owning_value(self.types, arg) } {
                return Err(ParseError::UnboundOwningValue);
            }
        }
        // SAFETY: `callee` is a resolved dyad from the store; a record type
        // carries the record `code_of` and `run_body_of` read.
        let (is_numtype, is_record) = unsafe {
            (
                crate::identities::is_numtype_node(self.types, callee),
                crate::identities::meta::is_record_type(callee),
            )
        };
        let (code, run_body) = if is_record {
            // SAFETY: as above.
            unsafe {
                (
                    crate::identities::meta::code_of(callee),
                    crate::identities::meta::run_body_of(callee),
                )
            }
        } else {
            (std::ptr::null_mut(), std::ptr::null_mut())
        };
        if is_numtype {
            // SAFETY: `callee` is a numtype node; `args` are reduced dyads.
            unsafe { crate::identities::build_cast(self.rt.store, self.types, callee, &args) }
        } else if is_record && !code.is_null() {
            // A type carrying a `code` applied to arguments is a call of that
            // function, the node typed by the type (#63; DESIGN ›Execution is
            // function application‹: "A node typed `^` thus runs and compiles
            // exactly as a node typed `f` does"), so `^(2, 3)` is the node
            // `2 ^ 3` builds. The literals commit to the code's parameters.
            let types = self.types;
            let mut args = args;
            // SAFETY: `callee` is a record type node with a code; `args` are
            // reduced dyads from the store.
            unsafe { crate::identities::commit_call_args(self.rt.store, types, code, &mut args)? };
            Ok(build_call(self.rt.store, callee, &args))
        } else if is_record {
            // A type whose `run` is a held body has nothing that builds a node
            // running it yet (#133 slice 8); an instance of its fields alone
            // would stand for a call that never runs, so it is refused.
            if !run_body.is_null() {
                return Err(ParseError::RunBodyHeld);
            }
            // A record type applied to its field values constructs an
            // instance — the constructor doctrine, like `i32(a)`.
            let types = self.types;
            // SAFETY: `callee` is a record type node; `args` are reduced dyads
            // from the store.
            unsafe {
                // The instance is a per-call local (a frame slot inside a
                // function), sized from the record layout, so a recursive call
                // fills its own copy.
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
            // declared type (the typed slot); an unbound callee has no
            // signature yet and commits nothing.
            let types = self.types;
            let mut args = args;
            // SAFETY: `callee` and `args` are reduced dyads from the store.
            unsafe {
                crate::identities::commit_call_args(self.rt.store, types, callee, &mut args)?;
            }
            let call = build_call(self.rt.store, callee, &args);
            // A call whose callee returns a type is resolved NOW, at comptime:
            // run it and substitute the concrete type it produces (roadmap
            // #30), so the result flows as an ordinary type value through
            // `==`, `:=`, `.logos`, and display. It runs before the driver
            // judges the cell, so the outer-name check runs here first: a
            // body that reads a dead name must not run (#125). SAFETY:
            // `callee`/`call` are reduced dyads.
            if unsafe { self.returns_type(callee) } {
                self.check_call_reads(callee)?;
                // SAFETY: `call` was just built over reduced dyads.
                unsafe { self.eval_type_call(call) }
            } else {
                Ok(call)
            }
        }
    }

    /// Open a scope: mint its node carrying the scope now open as its
    /// enclosing scope (#123; DESIGN ›Meta-navigation‹: "A scope node carries
    /// its enclosing scope: the parent link, null at the arche") and push it.
    /// The link is set before anything inside is parsed, so a constructor
    /// running inside walks up over settled structure; the stack is the cache.
    fn open_scope(&mut self) -> DyadPtr {
        let parent = self.scopes.current().unwrap_or(std::ptr::null_mut());
        let scope = crate::identities::scope::mint(self.rt.store, self.types.scope, parent);
        self.scopes.push(scope);
        scope
    }

    /// Parse a sequence of expressions up to the enclosing scope's end (a `)`, or
    /// the end of input), consuming an optional `,` between them (DESIGN
    /// ›Expressions are self-delimiting; `,` is the one explicit separator‹). A
    /// single expression is returned as itself; several become a sequence node
    /// `{type: scope, value: [expr0 … exprN, null]}` that runs its expressions in
    /// order and yields the trailing one (DESIGN ›A scope's value is what it
    /// evaluates to‹). Declarations inside are block-local: the sequence node is
    /// itself the scope they are declared in, pushed while the body parses. A
    /// `return` in a non-tail position is rejected ([`ParseError::EarlyReturn`]):
    /// v1 `return` is the tail yield, and running one without exiting would be
    /// silently wrong.
    pub fn parse_sequence(&mut self) -> Result<DyadPtr, ParseError> {
        // The block's scope node: the membership key while parsing and, when the
        // sequence is real, the sequence node itself.
        // `open` carries one entry per open scope, so its length is the
        // nesting depth; past the limit the parse is the checked error
        // rather than a Rust stack overflow (#80).
        if self.open.len() >= MAX_BRACKET_DEPTH {
            return Err(ParseError::TooDeep);
        }
        let scope = self.open_scope();
        self.open.push(OpenScope::default());
        let mut exprs = Vec::new();
        // The places this scope's own teardowns will free — what the escape check
        // below tests its tail against (issue #49).
        let mut owned_here: Vec<DyadPtr> = Vec::new();
        while let Some(item) = self.parse_next() {
            let item = item?;
            exprs.push(item);
            // The item is complete: the ranges of the names it declared or ended
            // in this scope now point at it (DESIGN ›Name resolution is
            // scope-filtered‹: the range runs between body items).
            // SAFETY: the pending records were minted by this parser's declares.
            unsafe { self.scopes.settle_item(scope, item) };
            // A binding of an owning value inserts `defer free <place>` into this
            // scope's pending list (issue #49); drain it right after the statement
            // so the defer sits at its source position — the right LIFO rank
            // among other statements' defers, and `scope::run` runs them all at
            // scope exit. Nested blocks drained their own before returning.
            let depth = self.open.len() - 1;
            if !self.open[depth].defers.is_empty() {
                let drained = std::mem::take(&mut self.open[depth].defers);
                for &d in &drained {
                    // SAFETY: `d` is a `defer free <place>` node the binding site
                    // just built.
                    owned_here.push(unsafe { crate::identities::drop_model::teardown_place_of(d) });
                }
                exprs.extend(drained);
            }
        }
        self.scopes.pop();
        // What the block parsed and did not run runs when the block runs; what
        // it did run (a drain the pass needed) stands in the body as its
        // result, read then.
        self.open.pop();
        // Prose is invisible to value flow, and so is a `defer` (it runs at exit,
        // never as the tail): the expression count and the tail below are taken
        // over the non-comment, non-defer expressions.
        let defer_ = self.types.defer_;
        // SAFETY: `exprs` are reduced dyads just parsed/built.
        let is_value = |e: DyadPtr| unsafe {
            !crate::identities::numtype::is_comment_type((*e).ty) && (*e).ty != defer_
        };
        let values = exprs.iter().filter(|&&e| is_value(e)).count();
        match (values, exprs.len()) {
            // An empty `( )`, or a bracket holding only prose: the scope node
            // with nothing to run, yielding unit — `f()`'s argument cell, a
            // `fn () -> …` with no parameters read by its own parser.
            (0, _) => {
                let arr = crate::identities::array::build(self.rt.store, self.types.array_, &exprs);
                // SAFETY: `scope` was minted by `open_scope` above and is unaliased.
                unsafe {
                    crate::identities::scope::fill(scope, arr, self.types.ops.scope_);
                }
                Ok(scope)
            }
            (_, 1) => Ok(exprs[0]),
            _ => {
                // Every non-tail value runs for effect only; the tail is the last
                // value expression (never a comment or a `defer`). A `return`
                // anywhere else would run without exiting (no unwinding yet), so
                // reject it.
                let types = self.types;
                let tail = exprs.iter().rposition(|&e| is_value(e)).expect("values >= 1");
                for (i, &e) in exprs.iter().enumerate() {
                    // SAFETY: `e` is a reduced dyad just parsed.
                    if i != tail && unsafe { contains_return(types, e) } {
                        return Err(ParseError::EarlyReturn);
                    }
                }
                // Ownership must not escape as this scope's *value*: the tail is
                // handed to the enclosing expression, but the teardown this scope
                // inserted frees the place on the way out, so the value handed out
                // would already be freed. DESIGN ruled `own` as the way ownership
                // leaves a scope, so require it. Only places *this* scope frees
                // are checked — handing out an enclosing scope's owning place is an
                // ordinary borrow, freed by whoever owns it.
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
                // A scope IS an array: the expression list lives behind one
                // indirection (its own array node), never inline in the scope's
                // value, which is the `[exprs, op]` pair.
                let arr = crate::identities::array::build(self.rt.store, self.types.array_, &exprs);
                // SAFETY: `scope` was minted by `open_scope` above and is unaliased.
                unsafe {
                    crate::identities::scope::fill(scope, arr, self.types.ops.scope_);
                }
                Ok(scope)
            }
        }
    }

    /// Parse the next statement-level item — a reflectable comment node or one
    /// expression — consuming an optional `,` after an expression (DESIGN
    /// ›Expressions are self-delimiting; `,` is the one explicit separator‹).
    /// `None` at the sequence's end: the end of input, or an unconsumed `)` left
    /// for the enclosing opener. This is the one sequencing step, shared by
    /// [`Parser::parse_sequence`] (which collects a whole block), the drivers
    /// (the command line, an imported file) and a type body. Where parse order
    /// is run order the item is left pending, to run when the pass needs a
    /// value or when its scope runs ([`Parser::drain`]).
    pub fn parse_next(&mut self) -> Option<Result<DyadPtr, ParseError>> {
        loop {
            // What the last segment yielded — its expression and the prose
            // lifted out of it, in source order — goes out first.
            if let Some(item) = self.queued.pop_front() {
                // Where parse order is run order, the item is pending: it runs
                // when the pass needs a value ([`Self::drain`]) or when its
                // scope runs. A deferred body's item runs with the body.
                if self.runtime_depth == 0 {
                    self.open.last_mut().expect("the root scope is open").unrun.push(item);
                }
                return Some(Ok(item));
            }
            self.skip_whitespace();
            if self.pos >= self.source.len() || self.at_close() {
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
            // The `,` is the step boundary; it is consumed here, once the
            // segment before it is constructed. A `,` written where nothing
            // stands (`x,, y`) is purely for the reader.
            if matches!(boundary, Boundary::Comma) {
                self.consume_separator();
            }
            if items.len() > 1 {
                self.pos = items[1].1;
                return Some(Err(ParseError::Trailing));
            }
            let mut ordered = std::mem::take(&mut self.lifted);
            if let Some((item, start)) = items.pop() {
                ordered.push((start, item));
            }
            ordered.sort_by_key(|&(start, _)| start);
            self.queued.extend(ordered.into_iter().map(|(_, n)| n));
        }
    }

    /// Parse a statement-level comment: `#` followed by a `«…»` string or raw
    /// text to the end of the line (the line form is sugar for the string form).
    /// Builds the reflectable comment node `{type: comment, value -> string node}`
    /// the settled design specifies (DESIGN ›Text literals are plain values; `#`
    /// is the one comment constructor‹).
    pub(crate) fn construct_comment(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let node = self.comment_after_hash()?;
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// The comment node for the text after a `#` the lexer just consumed.
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
            // Raw text to the end of the line, trimmed.
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

    /// `:=`'s constructor body: the declaration `name := value`, the name the
    /// token cell to the cursor's left. The name is declared *before* the
    /// value parses, so the value can refer to it (self-recursion); the
    /// fixpoint then makes the placeholder BE the value. A declaration is
    /// legal only opening its expression — the name its first cell, the `:=`
    /// its second — so anywhere else the construct declines and the driver
    /// finalizes (a fresh name mid-expression then errors through
    /// [`Parser::as_operand`] at its own position, exactly as before).
    pub(crate) fn construct_decl(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // The name is the cell to the left — a spelling, declared or not
        // (redeclaring a live one is the no-shadowing error below), or the
        // node `regex «…»` placed, whose text is the spelling, a pattern
        // (#114). Anything else there (a value, a bracket) declines.
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
        // The name is copied out: the cell's text is the parser's own source
        // or a fragment's, independent of the `&mut self` the declaration and
        // value parse then need, and of the fresh dyad written below.
        let name: String = match &pattern {
            Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            None => tok.spelling().to_string(),
        };
        let name: &str = &name;
        // The placeholder: for an unknown spelling, the fresh dyad the cell
        // already holds — "`:=` fills that dyad" (DESIGN ›The scope's
        // constructor is the driver‹) — or a fresh one when the spelling is
        // known (a redeclaration after an `own`/`drop`). It is `fn`-typed so a
        // recursive self-call sees a function-typed callee while the value is
        // still parsing; the fixpoint below overwrites it with the value's
        // real logos.
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
        let record = match declared {
            Ok(record) => record,
            Err(e) => {
                // The stuck point is the name itself (it is what shadows).
                self.pos = tok.start;
                return Err(e);
            }
        };
        // If the value opens with a `fn` literal, parse_fn publishes its
        // signature onto the placeholder before the body parses. The record
        // is the one a `lex_rank = …` line in the value writes (#122).
        self.pending_fn = placeholder;
        self.filling.push(record);
        let value = self.parse_expression();
        self.filling.pop();
        self.pending_fn = std::ptr::null_mut();
        let value = value?;
        // A bare name as the value is its record (a use); the fixpoint
        // inspects the dyad behind it and keeps `value` as what the
        // initializer stores.
        // SAFETY: `value` is a dyad from the store.
        let read = unsafe { self.types.through(value) };
        // Fixpoint: make the placeholder *be* the value, so references to
        // `name` captured while parsing the value resolve to it. A
        // construction binds the name to the *instance* (the storage)
        // and keeps the construct statement as the initializer: the name
        // is the place, the statement fills it each run. A *logos* value
        // (`x := i32`, `p := record(…)`) rebinds the name to the type
        // node itself instead — the name becomes another spelling of
        // that type, so the pointer-identity checks (`is_numtype_node`,
        // cross-type mismatch, record-logos equality) see the original.
        // SAFETY: `placeholder`/`value` are valid dyads just built.
        // A box on the right, by the reading rule (#82): its declared type, or
        // none. Decided before the chain below because a `let` chain would
        // need the 2024 edition.
        // SAFETY: `read` is a reduced dyad from the store.
        let box_ty = match unsafe { crate::identities::read::read_kind(self.types, read) } {
            crate::identities::read::Read::Container(t)
                if t == self.types.type_ || t == self.types.dyad_ =>
            {
                Some(t)
            }
            _ => None,
        };
        // SAFETY: `placeholder` is the node this call minted for the name and
        // nothing has read a value from it (self-references inside the value
        // captured only its address); `record` was minted above; `value` and
        // `read` are the dyads the expression parse returned. The writes in
        // this block are the fixpoint that makes the captured address mean
        // the value.
        let declared = unsafe {
            if self.holes.remove(&value) {
                // `x := i32 ?`: the place `?` built, with the declared type
                // and no value, is what the name binds to — reads are loads,
                // `=` reassigns, nothing initializes it.
                self.scopes.rebind(record, value);
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
                self.scopes.rebind(record, read);
                read
            } else if let Some(t) = box_ty {
                // A box on the right (`x := a` where `a := type ?`): reads are
                // copy by default (ruled 12 September 2026), so `x` gets its
                // own box and a copy of what `a` holds. Before the reading rule
                // the type-slot test above matched a box too and *rebound* `x`
                // to `a`'s storage, so `x = f64` silently wrote `a` (#82).
                let place = self.alloc_local(t, 8);
                let init = crate::identities::build_init(self.rt.store, self.types, place, read)?;
                self.scopes.rebind(record, place);
                init
            } else if crate::identities::drop_model::is_owning_value(self.types, value) {
                // An owning value (`alloc …`, `own a`, or a block yielding one)
                // lands in a place here — the one site that knows the name it
                // binds — so this is where the constructor-inserted teardown
                // attaches (issue #49, DESIGN ›Explicit heap‹: attachment at the
                // binding site). Mint an *owning* `@pointee` place (its type
                // carries the destructor, so `drop`/`own` on it are legal),
                // snapshot the value into it like any pointer, then insert
                // `defer free <place>` into this scope. Ownership landing in a
                // fresh place is what re-arms teardown after an `own` move: the
                // moved-from place no-ops, the new place owes the free.
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
                self.scopes.rebind(record, place);
                // `place` was just minted with the owning pointer type (its
                // destructor set), so the owning check passes; keeping it on
                // guards against a future caller inserting a free over a borrow.
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
                // A runtime numeric or pointer value is *snapshotted*:
                // fresh per-call storage (a frame slot inside a
                // function, an absolute blob at top level), the name
                // bound to that place, and the value kept as a
                // re-runnable initializer — so a read is a plain load
                // (never a re-evaluation of the initializer), `= …`
                // reassigns, and a loop-body or recursive local
                // re-initializes on each entry into its own storage. A
                // bare rational stays comptime (the guard above); a
                // fn/logos/unit value keeps its own binding below.
                let (ty_node, width) =
                    crate::identities::scalar_binding_type(self.rt.store, self.types, value);
                let place = self.alloc_local(ty_node, width);
                let init = crate::identities::build_init(self.rt.store, self.types, place, value)?;
                self.scopes.rebind(record, place);
                init
            } else {
                (*placeholder).ty = (*read).ty;
                (*placeholder).value = (*read).value;
                placeholder
            }
        };
        // The declaration is graph structure, not parse vapor: the
        // expression is a declare node carrying the spelling (the
        // nominal identity's human half), the binding, and its native.
        let name_node = crate::identities::string::build_text(
            self.rt.store,
            self.types.string_,
            name.as_bytes(),
        );
        let node = crate::identities::declare::build(
            self.rt.store,
            self.types.declare_,
            self.types.ops.declare_,
            name_node,
            declared,
        );
        tape.remove(-1); // the name token, consumed
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// `import`'s constructor body (#58): consume the path token — raw text up
    /// to whitespace or `,`, or a quoted `«…»` string (the licensed token
    /// consumption, as `#`'s) — load the file through [`Parser::import_file`],
    /// and place the reflectable import node `{type: import, value: [path,
    /// tail, op]}`. The load itself happens here, in the pass, once per run;
    /// the node's run only re-yields the file's tail value.
    pub(crate) fn construct_import(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // The load is a comptime effect: inside a fn body, loop, or runtime
        // branch, parse order and run order do not coincide, so it is rejected
        // like a type variable's fill.
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

    /// `regex`'s constructor body (#114; DESIGN ›The scope's constructor is
    /// the driver‹, ruled 10 September 2026: "`regex` is an ordinary identity
    /// that reads its own quote, as `lex` does, and yields a recognizer, which
    /// is what `:=` enters into the trie"): read the `«…»` to the right at
    /// discovery and place a `regex` node holding the pattern's bytes. The
    /// pattern is compiled here, so a bad one is the checked error at its
    /// quote rather than at the first text position the index tries it on
    /// (the trie compiles a branch lazily, on first lookup).
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
            // before it repeat the pattern with a caret.
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

    /// `lex`'s constructor body (#62; DESIGN ›Text is the quote‹: "the lexer
    /// itself, entered as an ordinary identity, applied to a string";
    /// ›The scope's constructor is the driver‹: "`regex` is an ordinary
    /// identity that reads its own quote, as `lex` does"): read the `«…»` to
    /// the right at discovery and place the node that lexes it when it runs
    /// — "there is no comptime/runtime distinction for `lex`: it turns any
    /// string into unconstructed identities whenever it runs", so the text
    /// is read here and the cells are made at each run, against the scopes
    /// open then ([`Parser::run_on_pass`]).
    pub(crate) fn construct_lex(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        self.skip_whitespace();
        let source = self.source;
        let start = self.pos;
        if !source[start..].starts_with('«') {
            return Err(ParseError::ExpectedQuote);
        }
        let r = self.scopes.resolve(self.trie, &source[start..]).map_err(ParseError::Resolve)?;
        self.pos += r.matched;
        let quote =
            self.construct_leaf(r.identity, start, r.matched)?.ok_or(ParseError::ExpectedQuote)?;
        let node = crate::identities::lex::build(self.rt.store, self.types, quote);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// `here`'s constructor (#123; DESIGN ›Meta-navigation‹: "`here` is the
    /// spot a line stands at and `here.scope` the scope that spot is in"):
    /// the node placed at discovery carries the scope open at its appearance
    /// — the body a line is written in, and inside a constructor's body that
    /// body, the definition site; the use site is `caller.scope`.
    pub(crate) fn construct_here(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let scope = self.scopes.current().unwrap_or(std::ptr::null_mut());
        let node = crate::identities::here::build_here(self.rt.store, self.types, scope);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// `caller`'s constructor (#123; DESIGN ›Meta-navigation‹: "`caller` is
    /// the same for the call … for a constructor the appearance of its
    /// identity"): the node whose `.scope` reads the pass's position when a
    /// constructor runs it ([`crate::run::Runtime::pass_scope`]).
    pub(crate) fn construct_caller(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let node = crate::identities::here::build_caller(self.rt.store, self.types);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Load `path_text` (#58): resolve against [`Parser::dir`] (file-relative;
    /// the working directory when the importer is the command line or REPL),
    /// enforce once-per-run and the DAG rule, and on a first load parse and
    /// run the file top to bottom in its own section — a fresh scope stack of
    /// the root scope plus a fresh section scope, so the file sees ambient
    /// names and its own imports only, never the import site's surroundings
    /// (ruled August 2026: importing is dropping the text there, wrapped in
    /// its own scope). Afterwards the file's `pub` names are declared into the
    /// importing scope — pub-only exposure, the ordinary visibility rule.
    /// Returns the file's tail node (null for a declaration-only file).
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
        // Sources are process-lived: every span and every later report indexes
        // into its file's text, exactly as the driver-held sources it joins.
        let text: &'static str = Box::leak(text.into_boxed_str());
        self.imports.entries.insert(canon.clone(), ImportState::Loading);

        // The file's own section: a fresh stack of the root (ambient names)
        // plus a fresh scope node the file's declarations land in.
        let root = *self.scopes.open.first().expect("an import site has an open root scope");
        let section = crate::identities::scope::mint(self.rt.store, self.types.scope, root);
        self.imports.sections.insert(section);
        let mut nested = ScopeStack::new();
        nested.push(root);
        nested.push(section);

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
                // reports the real failure again, not a phantom cycle.
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

    /// The nested top-to-bottom pass over an imported file: parse each
    /// statement, pending, and run the file at its end — a file loads once
    /// and a second import must find it ran — collecting the `pub`
    /// declarations' (name, identity) pairs and the file's tail node, which
    /// is then its ran form or a read, so running the import node never runs
    /// the file again (#88). On failure the message is returned with
    /// [`Parser::offset`] left at the stuck point in the imported source.
    fn run_imported(&mut self) -> Result<(Vec<(String, DyadPtr)>, DyadPtr), String> {
        let mut pubs = Vec::new();
        let mut tail = std::ptr::null_mut();
        while let Some(item) = self.parse_next() {
            let node = item.map_err(|e| crate::report::parse_message(&e))?;
            // SAFETY: `node` was just parsed into the store, which outlives
            // the pass.
            unsafe {
                if (*node).ty != self.types.comment_ {
                    tail = node;
                }
                if (*node).ty == self.types.declare_
                    && crate::identities::declare::gate_of(node) == self.types.pub_
                {
                    let name_node = *((*node).value as *const DyadPtr);
                    let name = String::from_utf8_lossy(crate::identities::string::text(name_node))
                        .into_owned();
                    let identity = self
                        .scopes
                        .resolve(self.trie, &name)
                        .map_err(|_| format!("pub name `{name}` did not stay resolvable"))?
                        .identity;
                    pubs.push((name, identity));
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

    /// Declare an imported file's `pub` names into the current (importing)
    /// scope — pub-only exposure, the ordinary visibility rule, so a collision
    /// with a live name is the ordinary shadowing error. Idempotent where the
    /// name already resolves to the same identity (the same file imported
    /// twice into one scope).
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

    /// `:`'s constructor (DESIGN ›The dyad's read surface‹, 7–8 September
    /// 2026): read a field of the record to the left — a use of a name is its
    /// record, so the cell is taken as it stands, never through the reading
    /// rule — or, for a constructed node, answer from its path. The member is
    /// the raw spelling at the cell to the right, exactly as `.` reads its
    /// member (a keyword there, `x:type`, was constructed at discovery and
    /// stands as a cell, its spelling still in the source).
    pub(crate) fn construct_record_read(
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
        let node = unsafe { self.record_read(lhs, nstart, nlen)? };
        tape.remove(1);
        tape.remove(-1);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// The `:` read. On a record: `dyad` yields the view of the cell it names
    /// — `{type: dyad, value: <the cell>}`, on which `.type` and `.value` read
    /// the cell ("`:dyad` is the view") — and every other field is the
    /// ordinary instance-field read on the record value, resolved against
    /// `record`'s own scope ([`Parser::resolve_field`]), so a field the record
    /// has not, `a:type`, is the same unknown-name error as `p.nonexistent`.
    /// On a constructed node, which has no record, the answers come from the
    /// path (DESIGN ›Meta-navigation‹): `dyad` is the view of the node itself;
    /// `scope` the scope open at the read — the innermost enclosing scope on
    /// every path the seed can write, since the item reads yield address
    /// values no path continues from; `start` and `end` null — the enclosing
    /// item is the segment still under construction, which a folded read
    /// cannot see; `gate` what the record holds (`pub` fills it, else null). The address values are
    /// `@dyad` literals, read at run time like any pointer.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn record_read(
        &mut self,
        lhs: DyadPtr,
        nstart: usize,
        nlen: usize,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let source = self.source;
        let name = &source[nstart..nstart + nlen];
        // `t[k]:dyad` (#61): the cell the slot holds, read when the
        // constructor runs — a record read through to the dyad it names.
        if (*lhs).ty == types.tape.slot {
            if name == "dyad" {
                return Ok(crate::identities::tape::build_slot_dyad(self.rt.store, types, lhs));
            }
            // `t[k]:name` (#120): the spelling of the record the cell holds,
            // read when the constructor runs.
            if name == "name" {
                return Ok(crate::identities::tape::build_slot_name(self.rt.store, types, lhs));
            }
            return Err(ParseError::ExpectedField);
        }
        if (*lhs).ty == types.record_ {
            if name == "dyad" {
                // Through a settled box: `a:dyad.type` asks the type of what
                // the name holds, and for `a := dyad ?` holding `i32` that is
                // `type`, exactly as `x:dyad.type` for `x := i32 5` is `i32`.
                let cell = self.settled_type(Record::read(lhs).dyad);
                return Ok(self.rt.store.alloc_raw(types.dyad_, cell as *mut u8));
            }
            let (field, offset) = self.resolve_field(types.record_, nstart, nlen)?;
            let addr = (*lhs).value.wrapping_add(offset);
            // `a:name` (#120): the slot holds the name's string node, and the
            // read is that string — a `string`-typed place over the slot,
            // marked as one so the reading rule sees the container it is
            // (no place of type `string` exists in a layout, which is why
            // the field itself is laid out `@dyad`). The display shows the
            // text; nothing else reads a string yet.
            if name == "name" {
                let place = crate::dyad::global_place(addr);
                return Ok(self.rt.store.alloc_raw(types.string_, place));
            }
            // `a:lex_rank` (#122): an `f64` place a user may write,
            // `pow:lex_rank = …`, so it carries the place mark `=` asks for.
            if name == "lex_rank" {
                let place = crate::dyad::global_place(addr);
                return Ok(self.rt.store.alloc_raw((*field).ty, place));
            }
            return Ok(self.rt.store.alloc_raw((*field).ty, addr));
        }
        let value = match name {
            "dyad" => return Ok(self.rt.store.alloc_raw(types.dyad_, lhs as *mut u8)),
            "scope" => self.scopes.current().unwrap_or(std::ptr::null_mut()),
            "start" | "end" | "gate" => std::ptr::null_mut(),
            _ => {
                // Not a field of a record: the same error a record read gives.
                self.resolve_field(types.record_, nstart, nlen)?;
                return Err(ParseError::ExpectedField);
            }
        };
        Ok(self.address_value(types.dyad_, value))
    }

    /// An `@pointee` value holding `addr`: a pointer-typed literal with its own
    /// eight bytes of storage, read at run time like any pointer variable.
    fn address_value(&mut self, pointee: DyadPtr, addr: DyadPtr) -> DyadPtr {
        crate::identities::pointer::address_value(self.rt.store, self.types, pointee, addr)
    }

    /// A member read on a dyad view (#52, ›The dyad's read surface‹):
    /// exactly the cell's two fields, `.type` and `.value` — the dyad type
    /// defines nothing else, so nothing else reads through the view. The
    /// value-decoding reads (`.operands[i]`) are ordinary `.` on the value
    /// itself, through its own type (corrected August 2026). Read-only by
    /// construction: nothing here writes.
    ///
    /// # Safety
    /// `view` must be a view node as `a:dyad` builds it
    /// ([`Parser::record_read`]).
    unsafe fn view_member(&mut self, view: DyadPtr, name: &str) -> Result<DyadPtr, ParseError> {
        let viewed = (*view).value as DyadPtr;
        if viewed.is_null() {
            return Err(ParseError::BadReflectRead);
        }
        match name {
            "type" => Ok((*viewed).ty),
            // v1: the raw address as a u64 value; the `@void` spelling waits
            // for pointer-value plumbing.
            "value" => Ok(self.scalar_value(
                crate::identities::numtype::NumType::U64,
                (*viewed).value as usize as i64,
            )),
            _ => Err(ParseError::BadReflectRead),
        }
    }

    /// A member read on a node standing as a type (#52): the shared metadata
    /// this crate stores once per type — `.arity`, `.roles[i]`,
    /// `.parse_rank`, `.associativity`, `.constructor`, `.destructor`, and the
    /// record layout `.fields`, `.size_bytes`, `.scope`. Typically reached as
    /// `a:dyad.type.arity`. A null constructor/destructor slot is the
    /// honest undefined and errors until `?` exists.
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
            // `lex_rank` is not the type's: it is the name's record's,
            // `^:lex_rank` (#122), so `.lex_rank` falls to the unknown-member
            // error below.
            // Associativity's values are the two identities `left` and
            // `right` (DESIGN ›The constructor is a field‹).
            "associativity" => Ok(match meta::assoc_of(logos) {
                Assoc::Left => self.types.left_,
                Assoc::Right => self.types.right_,
            }),
            // The constructor, `parse` (#130): the Logos function a body
            // filled the slot with (#61), or the view of a native leaf.
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
            // The run: the function an instance of the type runs as (#63),
            // the fn node itself, so `t.run.compile()` compiles it.
            "run" => {
                let c = meta::code_of(logos);
                if !c.is_null() {
                    return Ok(c);
                }
                // A body held as its lexed text (#133 slice 5): the `lex`
                // node, the seed's tape-fragment value, until a node of the
                // type constructs it.
                let held = meta::run_body_of(logos);
                if held.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                Ok(held)
            }
            "fields" if meta::is_record_type(logos) => Ok(self
                .rt
                .store
                .alloc_raw(self.types.dyad_, meta::record_fields_of(logos) as *mut u8)),
            "size_bytes" if meta::is_record_type(logos) => {
                Ok(self.scalar_value(NumType::I64, meta::record_size_of(logos) as i64))
            }
            "scope" if meta::is_record_type(logos) => Ok(self
                .rt
                .store
                .alloc_raw(self.types.dyad_, meta::record_scope_of(logos) as *mut u8)),
            "type" => Err(ParseError::TypeNeedsView),
            // A member of the type's own definition body (#61; DESIGN ›The
            // constructor is a field‹: bare lines "survive as members
            // reached `g.y`"): resolved against the body scope alone.
            _ => {
                let body = if meta::is_record_type(logos) {
                    meta::record_body_of(logos)
                } else {
                    std::ptr::null_mut()
                };
                if body.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                let mut members = ScopeStack::new();
                members.push(body);
                let r = members.resolve(self.trie, name).map_err(|_| ParseError::BadReflectRead)?;
                Ok(r.identity)
            }
        }
    }

    /// Build a typed scalar value node: fresh storage holding `bits` at `nt`'s
    /// width. The reflection counts (`.arity`, `.size_bytes`) and measures
    /// (`.parse_rank`) are ordinary typed values, comparable with literals.
    fn scalar_value(&mut self, nt: crate::identities::numtype::NumType, bits: i64) -> DyadPtr {
        let ty = self.types.numtypes[nt as usize];
        let width = nt.bytes();
        let bytes = bits.to_ne_bytes();
        let storage = self.rt.store.alloc_bytes(&bytes[..width]);
        self.rt.store.alloc_raw(ty, storage)
    }

    /// One lex step of the driver: the next token of the source as a tape
    /// cell with its offset ([`lex_token`]), or `None` at the end of input.
    /// `#` is an identity, constructed at discovery like any literal. A
    /// spelling the trie does not know becomes a fresh-name cell (null
    /// identity, its spelling kept), declared by a following `:=` or reported
    /// at the boundary. The stuck point of a failed lex is the token's start.
    fn lex_cell(&mut self) -> Result<Option<(Cell, usize)>, ParseError> {
        if self.feed.is_some() {
            return Ok(self.feed_take().map(|c| (c, c.start)));
        }
        self.skip_whitespace();
        let source = self.source;
        let Some((cell, next)) =
            lex_token(&self.scopes, self.trie, self.rt.store, source, self.pos)
                .map_err(ParseError::Resolve)?
        else {
            return Ok(None);
        };
        self.pos = next;
        // A box — a place holding a node, `type ?` or `dyad ?` — is read by
        // the pass wherever it stands as an operand ([`Self::settled_type`]),
        // and the read is honest only if everything parsed before it has
        // run. So where parse order is run order, its cell is the point the
        // pass runs to ([`Self::drain`]). A box that turns out to be an
        // assignment's target ran what stood before it a little early; that
        // is the same order, and the item being parsed has not begun.
        if self.runtime_depth == 0 && !cell.is_fresh() {
            // SAFETY: the record the trie resolved is a dyad from the store.
            let is_box = unsafe {
                matches!(
                    crate::identities::read::read_kind(self.types, cell.identity(self.types)),
                    crate::identities::read::Read::Container(t) if !t.is_null()
                )
            };
            if is_box {
                self.drain()?;
            }
        }
        Ok(Some((cell, cell.start)))
    }

    /// The identity a cell denotes *now*.
    ///
    /// A place holding a type denotes the type it holds. Where parse order is
    /// run order, lexing a box's cell first runs everything parsed before it
    /// ([`Self::lex_cell`], through [`Self::drain`]), so by the time the box
    /// is read here the assignment that filled it has run, wherever the box
    /// sits — which is how `a := type ?, a = i32, x := a 5` declares an `i32`
    /// at the top level, inside a block, and in a REPL line alike (DESIGN ›A
    /// type is a comptime value‹: "where the pass has already run the
    /// assignment that filled it, the box is read during the pass"). Inside a
    /// deferred body the assignment has not run when the body parses, so the
    /// place denotes itself and the uses that need an identity report
    /// [`ParseError::TypeKnownOnlyAtRun`]. Only a global place is read for
    /// the same reason: a frame slot has no content until its call.
    ///
    /// Everything that is not a settled type place is returned unchanged, so
    /// this is safe to ask of any cell.
    fn settled_type(&self, id: DyadPtr) -> DyadPtr {
        if self.runtime_depth > 0 {
            return id;
        }
        // SAFETY: `id` is null or a resolved dyad from the store; the read is
        // of a place this parser allocated, eight bytes wide.
        unsafe {
            if id.is_null() {
                return id;
            }
            // The two boxes whose content is a node: `type ?`, which holds an
            // identity, and `dyad ?`, which holds anything. Both store the
            // node's address, so both are read the same way — the reading
            // rule's `Container` of a declared type (#82).
            let ty = match crate::identities::read::read_kind(self.types, id) {
                crate::identities::read::Read::Container(t) if !t.is_null() => t,
                _ => return id,
            };
            let Some(addr) = crate::dyad::global_ref((*id).value) else {
                return id;
            };
            let held = std::ptr::read_unaligned(addr as *const DyadPtr);
            if held.is_null() || !self.rt.store.contains(held) {
                return id;
            }
            // A `type ?` box may hold only an identity; a `dyad ?` box holds
            // whatever was put in it.
            if ty == self.types.type_
                && crate::identities::type_identity_of(self.types, held).is_none()
            {
                return id;
            }
            held
        }
    }

    /// [`Cell::identity`] through [`Self::settled_type`]: what the cell means
    /// to the pass, which is what the driver dispatches on.
    fn cell_identity(&self, cell: &Cell) -> DyadPtr {
        self.settled_type(cell.identity(self.types))
    }

    /// The index of the held cell the cursor stands on, if it stands on one
    /// ([`Feed`]): whitespace skipped, the cell whose spelling starts at the
    /// cursor. The running index is corrected either way, since a boundary
    /// rewinds the cursor.
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

    /// Resolve a spelling that was fresh when its cell was lexed. Under a
    /// held body's construction ([`Feed`]) only the body's own names count —
    /// the parameters and the locals it declares, at the feed's base depth
    /// and above — since the body was lexed at its definition and a name
    /// declared after it, in a scope the body is written in, is not one it
    /// could see; elsewhere, the open scopes as usual.
    fn resolve_fresh(&self, spelling: &str) -> Result<Resolved, ResolveError> {
        let r = self.scopes.resolve(self.trie, spelling)?;
        if let Some(feed) = &self.feed {
            if !self.scopes.position(r.scope).is_some_and(|p| p >= feed.base_depth) {
                return Err(ResolveError::Unknown(spelling.to_string()));
            }
        }
        Ok(r)
    }

    /// Resolve a held cell whose spelling nothing had declared at the
    /// definition ([`Parser::resolve_fresh`]); an unresolved one stays fresh
    /// for the boundary to report.
    fn feed_resolve(&self, cell: &mut Cell) {
        if !cell.is_fresh() {
            return;
        }
        if let Ok(r) = self.resolve_fresh(cell.spelling()) {
            if r.matched == cell.len {
                cell.dyad = r.record;
            }
        }
    }

    /// Take the held cell at the cursor, the cursor moving past it.
    fn feed_take(&mut self) -> Option<Cell> {
        let i = self.feed_index()?;
        let mut cell = self.feed.as_ref()?.cells[i];
        self.feed.as_mut()?.next = i + 1;
        self.pos = cell.end();
        self.feed_resolve(&mut cell);
        Some(cell)
    }

    /// [`Parser::peek_token`] over the held cells.
    fn feed_peek(&mut self) -> Option<(DyadPtr, usize)> {
        let i = self.feed_index()?;
        let mut cell = self.feed.as_ref()?.cells[i];
        self.feed_resolve(&mut cell);
        if cell.is_fresh() {
            return None;
        }
        Some((cell.identity(self.types), cell.len))
    }

    /// Lex a held run body's text once, at the definition, into the fragment
    /// the type holds (#133 slice 8): every token an unconstructed cell with
    /// its spelling, brackets included; a `#` comment's text is passed over
    /// as its constructor will read it at the construction, to the line's
    /// end, or left as the quote that follows. `text` must live for the run:
    /// a string node's bytes do.
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

    /// The constructor an appearance of `id` runs, or `None` for an inert
    /// cell (a value, a delimiter). The identity's own slot first; failing
    /// that, its type's shared instance constructor — application for an
    /// instance of `fn` and for a record type (DESIGN ›The constructor is a
    /// field‹: "whether the name resolves to X's own slot or to the type's
    /// shared one is ordinary field semantics").
    ///
    /// Constructor *dispatch*, not a value read: this ladder and its siblings
    /// (`precedence_of_cell`, `assoc_of_cell`, `slot_of`, `build_call`) are
    /// #99's, left as they are by the reading rule (#82).
    fn ctor_of(&self, id: DyadPtr) -> Option<ConstructFn> {
        // SAFETY: `id` is a resolved dyad from the store.
        unsafe {
            if !id.is_null() && (*id).ty == self.types.fn_type {
                return Some(application);
            }
            let id = self.identity_head(id)?;
            // A record type runs the constructor its body filled (#61), or
            // the derived one: application, the instance construction.
            if crate::identities::meta::is_record_type(id) {
                return self.construct_of(id).or(Some(application));
            }
            self.construct_of(id)
        }
    }

    /// `id` when it is an identity carrying a record — a dyad typed by the
    /// root with a record in its value — or `None` for null, a value, an
    /// instance, a place: the one test the three dispatch readers
    /// ([`Self::ctor_of`], [`Self::precedence_of_cell`],
    /// [`Self::assoc_of_cell`]) share.
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

    /// The place of `id` on the one axis (its record's parse_rank), or
    /// [`prec::INERT`] for a cell that carries no record.
    ///
    /// [`prec::INERT`]: crate::identities::meta::prec::INERT
    fn precedence_of_cell(&self, id: DyadPtr) -> f64 {
        // SAFETY: as [`Parser::ctor_of`].
        unsafe {
            match self.identity_head(id) {
                Some(id) => crate::identities::meta::parse_rank_of(id),
                None => crate::identities::meta::prec::APPLY,
            }
        }
    }

    /// The associativity of `id`'s constructor: its record's, or left for an
    /// instance running its type's shared constructor (application), which
    /// carries no record of its own.
    fn assoc_of_cell(&self, id: DyadPtr) -> Assoc {
        // SAFETY: as [`Parser::ctor_of`].
        unsafe {
            match self.identity_head(id) {
                Some(id) => crate::identities::meta::assoc_of(id),
                None => Assoc::Left,
            }
        }
    }

    /// Run `construct` for the cell at the tape's cursor and settle the
    /// outcome on that cell (DESIGN ›The scope's constructor is the driver‹:
    /// "A constructor's outcome is read off its cell — constructed, or
    /// declined (the frontier untouched, the identity standing as its own
    /// value — `i32` before a `,`) — while an error (an operand missing, an
    /// unfinished construct) returns … There is no holding and no
    /// re-invocation"). The cell is judged by its handle, whatever the
    /// center is after the call (#81): a constructor's re-centering is its
    /// own view of the tape, so the center is put back on the cell while it
    /// stands. Progress is the driver's invariant, not the constructor's —
    /// the cell is constructed, gone, another token, or standing as its
    /// value, or the call is the checked error; nothing is run twice.
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
        // Dispatching the cell's identity is the body's read of its name — a
        // callee, an operator, a keyword alike (#125).
        if let Some(c) = tape.at(0) {
            let record = c.record(self.types);
            self.note_outer_read(record);
        }
        let was = std::mem::replace(&mut self.discovering, discovery);
        let outcome = construct(self, id, tape);
        self.discovering = was;
        let outcome = outcome?;
        // Removed itself: the splice is the outcome, and `remove` moved the
        // center to the next cell.
        let Some(cell) = tape.cell_at_mark(own).copied() else {
            return Ok(());
        };
        tape.restore(own);
        // The driver's rule (DESIGN ›Execution is function application‹,
        // ruled 19 September 2026): "flag true, done; flag false and the cell
        // holds another identity than the one whose `parse` ran, that
        // identity's turn; flag false and the same identity, the checked
        // error, a constructor that neither finished nor handed on" — and
        // "a constructor that finds nothing to consume sets its flag and
        // stands as itself". A constructor written in Rust says the last with
        // `Decline` (or by leaving the frontier untouched), and the driver
        // sets the flag for it.
        let same = self.cell_identity(&cell) == id;
        if cell.constructed {
            if same {
                // The flag set on the untouched cell: the identity stands as
                // its own value, the use of its name the cell already holds.
                return Ok(());
            }
            // A Logos-written constructor placed its node: the node's run is
            // the function built for its field-type set (#133 slice 8).
            // SAFETY: a constructed cell's dyad is a node from the store.
            if logos && !cell.dyad.is_null() && unsafe { (*cell.dyad).ty } == id {
                let spelling = cell.spelling().to_string();
                // SAFETY: the node is the one `run_logos_ctor` minted for
                // `id`, `[field…, null, spec]`.
                unsafe { self.resolve_specialization(cell.dyad, cell.start, &spelling)? };
            }
            // Constructed: a node that runs a function's body — a call, or a
            // node of a type carrying a `run` — is a use of every outer name
            // that body reads, checked here where the node comes to exist,
            // whichever constructor built it (#125). The error points at the
            // construct's own token: the callee of `climb()`, the `^` of
            // `2 ^ 3`.
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
            // Handed on to another identity ("what it built it leaves at the
            // cursor (a dyad, or another token)"), which the driver constructs
            // as its own, or reads as the use of its name. A value that is no
            // identity has no turn to take: the constructor should have set
            // the flag.
            if self.is_identity_node(self.cell_identity(&cell)) {
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
        // The identity stands as its own value: the use of its name, its
        // record, when the cell was lexed from a spelling.
        let value = self.stand_as_value(tape, id);
        tape.place(value);
        Ok(())
    }

    /// Whether `id`'s constructor is written in Logos — a `fn` node in its
    /// slot ([`Parser::construct_of`]) — and so is judged by the flag alone.
    fn is_logos_ctor(&self, id: DyadPtr) -> bool {
        // SAFETY: `id` is a resolved dyad from the store; the slot is read
        // only where a record head exists.
        unsafe {
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

    /// Whether `d` is an identity a cell may be handed on to: a function
    /// value, or a type node with a record head — what [`Parser::ctor_of`]
    /// dispatches on, and what the boundary reads as the use of a name.
    fn is_identity_node(&self, d: DyadPtr) -> bool {
        // SAFETY: `d` is null or a resolved dyad from the store.
        unsafe {
            !d.is_null()
                && ((*d).ty == self.types.fn_type
                    || ((*d).ty == self.types.type_
                        && crate::identities::meta::kind_of(d).is_some()))
        }
    }

    /// Lex one segment onto `tape`: every cell up to the next `,`, `)`, or
    /// the end of input — none of which is consumed — constructing at
    /// discovery each cell whose identity sits at or above `(` on the axis
    /// (DESIGN ›The scope's constructor is the driver‹: "a token whose
    /// identity's parse_rank is at or above `(`'s own is constructed at
    /// discovery, before the next token is lexed … every other token is
    /// placed on the tape unconstructed").
    fn lex_segment(&mut self, tape: &mut ParsingTape) -> Result<Boundary, ParseError> {
        self.lex_segment_until(tape, None)
    }

    /// [`Parser::lex_segment`], optionally stopping before a `(` that follows
    /// at least one cell — the right side an identity reads up to its body
    /// bracket ([`Parser::drive_until_open`]), in one of two modes. The loop
    /// is [`Parser::lex_next`] until a boundary; the mode is kept on the
    /// parser for the lazy reads a constructor makes meanwhile.
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
    /// driver‹): lex the next cell onto the tape's end and, if its identity's
    /// parse_rank is at or above `(`'s, construct it at discovery. A boundary
    /// token — `,`, `)`, or the bracket a right-side read stops before — is
    /// left unconsumed (`pos` rewound) and returned; `None` means a cell was
    /// lexed. This is also what a lazy `tape[k]` read runs
    /// ([`Parser::cell_at`], `lazy`), with one difference: a cell lexed on
    /// demand *inside* a constructor is constructed at discovery only if it
    /// reads nothing to its left — a bracket, a literal, a comment — since a
    /// left-reader (`.`, `:`, `@`, `:=`, `=`) would find the constructor's own
    /// unfinished cell there; those wait for the loop's next step
    /// ([`Parser::discover_pending`]), which runs before anything further is
    /// lexed, so their right side is still theirs to read.
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
            if id == self.types.sep_ {
                self.pos = start;
                return Ok(Some(Boundary::Comma));
            }
            if id == self.types.close_ || id == self.types.close_sq_ {
                self.pos = start;
                return Ok(Some(Boundary::Close));
            }
            if let Some(mode) = self.lex_mode {
                if id == self.types.open_ && !tape.is_empty() {
                    // In a condition the bracket is the caller's only when
                    // nothing before it would read it: a `(` after an
                    // unconstructed identity with a constructor is that
                    // identity's — `f(x)`'s arguments, `not (c)`'s operand,
                    // `==`'s right operand — never the body (DESIGN ›`X (…)`
                    // is one spelling, and X's constructor decides‹). A
                    // return type takes no bracket, so there the first
                    // `(` is the body: `fn () -> i32 ( body )` "is taken by
                    // `fn` before `i32`'s juxtaposition could read a
                    // conversion".
                    let owner_pending = mode == RightSide::Condition
                        && matches!(tape.last(), Some(l) if !self.is_operand_cell(l));
                    if !owner_pending {
                        self.pos = start;
                        return Ok(Some(Boundary::Open));
                    }
                }
            }
        }
        tape.push(cell);
        if !cell.constructed {
            if let Some(construct) = self.ctor_of(id) {
                let prec = self.precedence_of_cell(id);
                // A right-side read stops before its caller's bracket, so an
                // identity that reads its own bracket (`type`, `fn`) is not
                // woken there: `-> type ( body )` names the classifier and
                // leaves the body to `fn`. And a reader followed by a tight
                // read never wakes: the read takes it (DESIGN ›Text is the
                // quote‹, the seed's form of "the tight read runs first").
                let reader = prec == crate::identities::meta::prec::READER
                    || prec == crate::identities::meta::prec::DECLARE;
                let asleep = (self.lex_mode == Some(RightSide::ReturnType) && reader)
                    || self.tight_read_takes(id);
                let reads_left = prec == crate::identities::meta::prec::TIGHT
                    || prec == crate::identities::meta::prec::DECLARE;
                if prec >= crate::identities::meta::prec::OPEN && !asleep && !(lazy && reads_left) {
                    self.run_ctor(construct, id, tape, true)?;
                }
            }
        }
        if !lazy {
            self.discover_pending(tape)?;
        }
        Ok(None)
    }

    /// Construct at discovery the cells a lazy read lexed but had to leave —
    /// the left-readers — in tape order, before the loop lexes further. Each
    /// runs with the center on its own cell; the center is put back after.
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

    /// The cell `offset` links right of the center, lexed on demand: the
    /// `tape[k]` read that "lexes lazily on demand" (DESIGN ›The scope's
    /// constructor is the driver‹), what lets `:`, `.`, and `@` construct at
    /// discovery with their right cell not yet on the tape. Lexing runs the
    /// loop's own step, so a cell lexed here is constructed at discovery
    /// exactly as the loop would; the center is put back afterwards. `None`
    /// when a boundary or the end of input comes first. Negative offsets read
    /// the tape as it stands.
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

    /// Construct a lexed segment at its boundary: comment cells are lifted out
    /// (prose is void-valued and invisible to value flow), then the
    /// unconstructed cells run highest parse_rank first, associativity
    /// breaking ties — left keeps the leftmost first, right the rightmost —
    /// each constructor taking what its syntax needs from the fully lexed
    /// segment, left or right, with no lookahead; what remains is read as
    /// operands, an undeclared spelling being the checked error at its own
    /// position. Returns the constructed cells in order with their offsets.
    fn construct_segment(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Vec<(DyadPtr, usize)>, ParseError> {
        // Prose is lifted out beside the segment, in order.
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

    /// Lex and construct the cells up to the next `(` — the right side an
    /// identity reads before its bracket: an `if`'s or `while`'s condition,
    /// a `for`'s range, a `fn`'s return type (DESIGN ›The scope's
    /// constructor is the driver‹, ruled 5 September 2026: "its condition is
    /// the cells up to the body bracket, constructed to one cell"). A `(`
    /// standing first is part of the read (`if (c) (body)`); a later one is
    /// the bracket, left for the caller — so a bracket inside the read after
    /// its first cell wants the whole read parenthesized. A `,`, `)`, or the
    /// end of input stops the read too.
    fn drive_until_open(&mut self, mode: RightSide) -> Result<Vec<(DyadPtr, usize)>, ParseError> {
        let mut tape = ParsingTape::new();
        self.lex_segment_until(&mut tape, Some(mode))?;
        self.construct_segment(&mut tape)
    }

    /// Parse one expression: one segment, lexed to the next `,`, `)`, or end
    /// of input (left unconsumed) and constructed to exactly one cell.
    /// This is the value a discovery-time constructor drives — `:=`'s right
    /// side, a `defer`'s — and the REPL's line.
    pub fn parse_expression(&mut self) -> Result<DyadPtr, ParseError> {
        let mut tape = ParsingTape::new();
        self.lex_segment(&mut tape)?;
        let items = self.construct_segment(&mut tape)?;
        self.one_of(items)
    }

    /// Exactly one constructed cell, or the checked error: none is an empty
    /// expression, more than one the leftover cell DESIGN names, reported at
    /// the second cell.
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

    /// Why a segment was left with more than one cell. Usually the leftover
    /// cell DESIGN names, [`ParseError::Trailing`] — but where the first of
    /// them is a box holding a node, the real reason is that the box could not
    /// say what it holds, so `a 5` never juxtaposed. That happens inside a
    /// deferred body, where parse order is not run order, and saying "expected
    /// one expression" sends the reader looking for a missing comma.
    fn leftover_error(&self, first: DyadPtr) -> ParseError {
        // SAFETY: `first` is a constructed node from the store.
        let node_box = unsafe {
            let d = self.types.through(first);
            !d.is_null()
                && ((*d).ty == self.types.type_ || (*d).ty == self.types.dyad_)
                && crate::dyad::is_place((*d).value)
        };
        if node_box {
            ParseError::TypeKnownOnlyAtRun
        } else {
            ParseError::Trailing
        }
    }
}

/// Where a segment stopped: the separator, the closing bracket, or the end of
/// input — none consumed by the lexing step.
enum Boundary {
    Comma,
    Close,
    Eof,
    /// The `(` a right-side read stops before (its caller's bracket).
    Open,
}

/// What a right-side read is for, which decides whose a `(` inside it is
/// (see [`Parser::lex_segment_until`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum RightSide {
    /// An `if`'s or `while`'s condition, a `for`'s range: a bracket after a
    /// pending identity is that identity's.
    Condition,
    /// A `fn`'s return type: the first bracket is the body, and an identity
    /// that reads its own bracket is not woken.
    ReturnType,
}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)] // a test reads the nodes it built a line above
mod tests {
    use super::*;

    /// A distinct sentinel address per tag (never dereferenced).
    /// A record dyad for `identity`, its scope set by `declare`: leaked like
    /// the dyads below.
    fn rec(identity: DyadPtr) -> DyadPtr {
        rec_in(identity, std::ptr::null_mut())
    }

    fn rec_in(identity: DyadPtr, scope: DyadPtr) -> DyadPtr {
        let fields = Box::into_raw(Box::new(Record::new(identity, scope, std::ptr::null_mut())));
        Box::into_raw(Box::new(crate::dyad::Dyad {
            ty: std::ptr::null_mut(),
            value: fields as *mut u8,
        }))
    }

    /// The fields behind a record dyad.
    fn f(record: DyadPtr) -> Record {
        // SAFETY: only `rec`-built dyads reach the trie in these tests.
        unsafe { Record::read(record) }
    }

    fn dyad(tag: usize) -> DyadPtr {
        std::ptr::without_provenance_mut(tag)
    }

    fn dyad_cells(tags: &[usize]) -> Vec<Cell> {
        tags.iter().map(|&t| unsafe { Cell::built(dyad(t)) }).collect()
    }

    #[test]
    fn offset_indexing_is_cursor_relative() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12, 13]));
        t.set_cursor(2); // points at dyad(12)
        assert_eq!(t.at(0).unwrap().dyad, dyad(12));
        assert_eq!(t.at(-1).unwrap().dyad, dyad(11));
        assert_eq!(t.at(1).unwrap().dyad, dyad(13));
        assert_eq!(t.at(-2).unwrap().dyad, dyad(10));
        assert!(t.at(2).is_none()); // past the end
        assert!(t.at(-3).is_none()); // before the start
        assert_eq!(t.cursor(), 2);
    }

    #[test]
    fn insert_left_keeps_cursor_on_same_cell() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1); // dyad(11)
        t.insert(0, unsafe { Cell::built(dyad(99)) }); // splice just left of the cursor
        assert_eq!(t.at(0).unwrap().dyad, dyad(11));
        assert_eq!(t.at(-1).unwrap().dyad, dyad(99));
        assert_eq!(t.len(), 4);
        assert_eq!(t.cursor(), 2);
    }

    #[test]
    fn insert_right_leaves_cursor() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1); // dyad(11)
        t.insert(1, unsafe { Cell::built(dyad(99)) });
        assert_eq!(t.at(0).unwrap().dyad, dyad(11));
        assert_eq!(t.at(1).unwrap().dyad, dyad(99));
        assert_eq!(t.at(2).unwrap().dyad, dyad(12));
    }

    #[test]
    fn remove_left_keeps_cursor_on_same_cell() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(2); // dyad(12)
        let gone = t.remove(-1); // remove dyad(11)
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
        // The tape's defining property: unconstructed cells (a record, a fresh
        // dyad, their spans kept tape-side) and constructed nodes on one
        // frontier, told apart by the tape's own flag, never by the dyad.
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
        // A constructor may rewrite a pending cell's span or pointer on the
        // tape (the mechanism behind token-rewriting operators); `place`
        // marks it constructed, and `reduce_here` spans the triple.
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

    /// Parse one expression and run it, the store attached to the runtime as
    /// the parser attaches it around a constructor call.
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
        let v = rt.lexing(&scopes, trie, |rt| unsafe { rt.run(node) });
        (v.unwrap_or_else(|e| panic!("{src}: {e:?}")), scopes)
    }

    #[test]
    fn a_scope_carries_its_enclosing_scope() {
        // #123; DESIGN ›Meta-navigation walks the graph‹: "A scope node
        // carries its enclosing scope: the parent link, null at the arche,
        // and only it".
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
        // #123; DESIGN ›Meta-navigation walks the graph‹: "`here` is the spot
        // a line stands at and `here.scope` the scope that spot is in,
        // `here.scope.scope` the one above, and so up to the arche, whose
        // `.scope` is null".
        let mut store = crate::store::Store::new();
        let mut trie = crate::regex_trie::RegexTrie::new();
        let core = crate::identities::Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let (root, scopes) = go("here.scope", &mut store, &mut trie, types, scopes);
        assert_eq!(root as usize, core.root_scope as usize);
        let (above, scopes) = go("here.scope.scope", &mut store, &mut trie, types, scopes);
        assert_eq!(above, 0, "the arche has no enclosing scope");
        // Past the arche is the checked error, never a dereference.
        let mut p = Parser::new("here.scope.scope.scope", &mut store, &mut trie, types, scopes);
        let node = p.parse_expression().unwrap();
        let scopes = p.into_scopes();
        let mut rt = crate::run::Runtime::new(types, &mut store);
        // SAFETY: `node` was just parsed into the store.
        let err = rt.lexing(&scopes, &trie, |rt| unsafe { rt.run(node) }).unwrap_err();
        assert_eq!(err, crate::run::RunError::NullPointer);
        // `caller.scope` outside a constructor is the checked error: the
        // seed's stand-in for the per-call read of an ordinary function.
        let mut p = Parser::new("caller.scope", &mut store, &mut trie, types, scopes);
        let node = p.parse_expression().unwrap();
        let scopes = p.into_scopes();
        let mut rt = crate::run::Runtime::new(types, &mut store);
        // SAFETY: as above.
        let err = rt.lexing(&scopes, &trie, |rt| unsafe { rt.run(node) }).unwrap_err();
        assert_eq!(err, crate::run::RunError::NoCaller);
    }

    #[test]
    fn lex_hands_back_the_tape_unconstructed() {
        // DESIGN ›Text is the quote‹ (#62): `lex «+»` is a one-cell fragment
        // whose cell points at `+`'s record with «+» as its spelling; `lex
        // «(a, b)»` is the five cells the lexer would have put on the
        // frontier, no constructor woken; text that names nothing lexes to a
        // fresh dyad with both slots undefined, its spelling kept.
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
        assert_eq!(cell.identity(types), types.plus, "the cell points at `+`'s record");
        assert!(!cell.record(types).is_null());
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
        // #81: the driver judges a constructor's outcome by the handle of
        // the construct's own cell, not by whatever the center is after
        // the call. A handle answers the cell while it is linked and `None`
        // once removed; the edit count tells an untouched frontier from an
        // edited one, whichever way the center moved.
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
        // `tape.insert(k, lex «…»)` splices the fragment's cells, flags and
        // spellings (DESIGN ›Text is the quote‹, 14 September 2026), in
        // order, where one cell would land — and a tape spliced into itself
        // is copied first.
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
        // DESIGN ›The constructor is a field‹: "an appearance of X runs X's
        // `constructor` field" (#61). A postfix `squared`, its constructor
        // written in Logos, its parse_rank one above `*`'s: the driver runs
        // it at the boundary, and what it leaves — a `squared` node whose
        // `run` calls `sq` — runs and compiles as any call. The constructor
        // is the 18 and 19 September 2026 shape (#133 slice 6): `this` filled
        // by field name, placed, and marked done by the constructor itself.
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
                instance = ( a := ?, shared run = fn (a := i32 ?) -> i32 ( sq(a) ) ), \
                parse_rank = *.parse_rank + 1, \
                parse = ( this.a = tape[-1], tape[0] = this, tape.is_constructed[0] = true, \
                          tape.remove(-1) ) )",
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
        // The write and the flag are two lines (DESIGN ›Execution is function
        // application‹, 19 September 2026: "the write replaces the pointer and
        // nothing more; the constructor sets the flag itself,
        // `tape.is_constructed[0] = true`"). A tape over `a + b`, centered on
        // the `+`, edited from Logos: `t[0] = g` points the cell at `g` and
        // leaves it unconstructed, `t.is_constructed[0] = true` marks it.
        use crate::identities::Core;
        use crate::record::Record;
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
            let rec = Record::alloc(
                &mut store,
                core.record_,
                Record::new(t, core.root_scope, std::ptr::null_mut()),
            );
            scopes.declare(&mut trie, "t", rec).unwrap();

            // `t[k]:dyad` reads the cell through its record: the `+` identity.
            let (v, s) = go("t[0]:dyad", &mut store, &mut trie, types, scopes);
            assert_eq!(v as DyadPtr, Record::read(plus).dyad);
            let (v, s) = go("t[0]:dyad.type", &mut store, &mut trie, types, s);
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
                types.through(s.resolve(&trie, "g").unwrap().record),
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
        // DESIGN ›The scope's constructor is the driver‹: the tape's four
        // affordances — `tape[k]`, `is_constructed`, `insert`, `remove` —
        // and the re-centered view, as identities a Logos function reaches
        // (#60). A tape over `a + b` lexed by a parser, handed to Logos as a
        // `parsing_tape` instance named `t`, then edited from Logos.
        use crate::identities::Core;
        use crate::record::Record;
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
        // SAFETY: `tape` is a live box; the natives write through the handle
        // the instance holds, and the checks below read through the same one.
        unsafe {
            assert_eq!((*tape).len(), 3);
            let plus = (*tape).at(1).unwrap().dyad;
            assert!(!(*tape).at(1).unwrap().constructed, "a lexed cell is unconstructed");

            let storage = store.alloc_bytes(&(tape as usize as u64).to_ne_bytes());
            // Marked as storage, as the parser marks every place it allocates.
            let t = store.alloc_raw(core.tape.parsing_tape, crate::dyad::global_place(storage));
            let rec = Record::alloc(
                &mut store,
                core.record_,
                Record::new(t, core.root_scope, std::ptr::null_mut()),
            );
            scopes.declare(&mut trie, "t", rec).unwrap();

            let (v, s) = go("t.is_constructed[1]", &mut store, &mut trie, types, scopes);
            assert_eq!(v, 0);
            // `t.spelling[k]` (#121, ruled 14 September 2026): the text the
            // cell was lexed from, a string node — the tape's second list
            // beside the flag, read as that list's element.
            let (v, s) = go("t.spelling[1]", &mut store, &mut trie, types, s);
            assert_eq!(crate::identities::string::text(v as DyadPtr), b"+");
            let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
            assert_eq!(crate::identities::string::text(v as DyadPtr), b"a");
            // `t[k]:name` (#120): the spelling of the record a cell holds —
            // `+`'s record answers «+»; the identity's name, not the match.
            let (v, s) = go("t[1]:name", &mut store, &mut trie, types, s);
            assert_eq!(crate::identities::string::text(v as DyadPtr), b"+");
            let (v, s) = go("t.remove(1)", &mut store, &mut trie, types, s);
            assert_eq!(v as DyadPtr, plus, "remove yields the cell it took");
            assert_eq!((*tape).len(), 2);

            // `[` is `(` in square brackets (ruled 9 September 2026): the
            // index is any expression, a name or a negative literal alike,
            // run by the native.
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

            // `insert` splices a tape (ruled 14 September 2026, #62): the
            // fragment `lex «…»` hands back, its cell unconstructed with the
            // text it was lexed from; a write then repoints it in place, and
            // the text stays.
            let (_, s) = go("t.insert(0, lex «9»)", &mut store, &mut trie, types, s);
            assert_eq!((*tape).len(), 3);
            assert_eq!((*tape).at(0).unwrap().dyad, c0.dyad, "the center stays on its cell");
            let (_, s) = go("t.recenter(-1)", &mut store, &mut trie, types, s);
            let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
            assert_eq!(v, 0, "a spliced cell is unconstructed");
            let (v, s) = go("t[0]", &mut store, &mut trie, types, s);
            assert_eq!((*(v as DyadPtr)).ty, core.record_, "the spliced cell is a record");
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
            // Anything but a tape is refused at the call.
            let mut p = Parser::new("t.insert(0, dyad (i32, 9))", &mut store, &mut trie, types, s);
            assert_eq!(p.parse_expression().unwrap_err(), ParseError::InsertTakesTape);
            let s = p.into_scopes();

            // A use of a name handed in is its record; the flag is untouched.
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

    // --- scope stack + name resolution --------------------------------------

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
        // One spelling, two sibling scopes (not nested, so no shadowing). The
        // open scope decides which identity a use resolves to.
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let (outer, inner) = (dyad(100), dyad(101));

        scopes.push(outer);
        unsafe { scopes.declare(&mut trie, "x", rec(dyad(1))) }.unwrap();
        scopes.pop(); // close outer

        scopes.push(inner);
        unsafe { scopes.declare(&mut trie, "x", rec(dyad(2))) }.unwrap();
        assert_eq!(scopes.resolve(&trie, "x").unwrap().identity, dyad(2));

        scopes.pop();
        scopes.push(outer); // reopen outer instead
        assert_eq!(scopes.resolve(&trie, "x").unwrap().identity, dyad(1));
    }

    #[test]
    fn out_of_scope_is_distinct_from_unknown() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        unsafe { scopes.declare(&mut trie, "y", rec(dyad(1))) }.unwrap();
        scopes.pop(); // close the scope

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
        // Same scope: redeclaration rejected.
        assert_eq!(
            unsafe { scopes.declare(&mut trie, "a", rec(dyad(2))) },
            Err(ResolveError::Shadowed("a".into()))
        );
        // Nested scope while the outer declaration is live: still rejected.
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
        scopes.commit(); // committed declarations survive a rollback
        unsafe { scopes.declare(&mut trie, "gone", rec(dyad(2))) }.unwrap();

        scopes.rollback(&mut trie);
        assert_eq!(scopes.resolve(&trie, "keep").unwrap().identity, dyad(1));
        assert_eq!(scopes.resolve(&trie, "gone"), Err(ResolveError::Unknown("gone".into())));
        // The rolled-back name is free again — no permanent "shadowed".
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
        scopes.push(dyad(101)); // left open by an error mid-nesting
        scopes.push(dyad(102));
        scopes.truncate(1);
        assert_eq!(scopes.depth(), 1);
        assert_eq!(scopes.current(), Some(dyad(100)));
        assert!(!scopes.is_open(dyad(101)));
    }

    #[test]
    fn a_dead_name_resolves_as_dead_and_may_be_redeclared() {
        // DESIGN ›Name resolution is scope-filtered‹ (3 September 2026): an
        // `own`/`drop` makes the name dead — distinct from out-of-scope and
        // unknown — and only `:=` may follow, a fresh entry beside the dead one.
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
        assert_eq!(m.records.len(), 2);
        assert!(m.records.iter().any(|&c| f(c).dyad == dyad(1) && f(c).end == dyad(50)));
    }

    #[test]
    fn rollback_restores_a_dead_mark() {
        // A REPL line that moves a name and then fails must leave the name
        // alive, exactly as a failed declaration leaves the spelling free.
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
        assert_eq!(trie.get("a").unwrap().records.len(), 1);
    }

    #[test]
    fn two_spellings_of_equal_rank_matching_the_same_length_are_a_tie() {
        // DESIGN ›The scope's constructor is the driver‹ (ruled 10 September
        // 2026): equal rank and equal length is the inconsistency the
        // definitions must correct, never a silent pick by declaration order.
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
        // The range runs between body items: the declaring line, and the line
        // holding the `own`/`drop` (the node itself only while that line parses).
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let scope = dyad(100);
        scopes.push(scope);
        let a1 = rec(dyad(1));
        unsafe { scopes.declare(&mut trie, "a", a1) }.unwrap();
        let a = |trie: &RegexTrie| f(trie.get("a").unwrap().records[0]);
        assert!(a(&trie).start.is_null());

        unsafe { scopes.settle_item(scope, dyad(10)) };
        assert_eq!(a(&trie).start, dyad(10));
        assert!(a(&trie).end.is_null());

        unsafe { scopes.mark_dead(a1, dyad(50)) };
        assert_eq!(a(&trie).end, dyad(50), "provisional: the own/drop node");
        unsafe { scopes.settle_item(scope, dyad(11)) };
        assert_eq!(a(&trie).end, dyad(11), "settled: the body item");
        assert_eq!(a(&trie).start, dyad(10), "start untouched by the end's settle");

        // A rebind keeps the range, and the pending endpoint holds the record.
        let b_rec = rec(dyad(3));
        unsafe { scopes.declare(&mut trie, "b", b_rec) }.unwrap();
        unsafe { scopes.rebind(b_rec, dyad(4)) };
        unsafe { scopes.settle_item(scope, dyad(12)) };
        let b = f(trie.get("b").unwrap().records[0]);
        assert_eq!((b.dyad, b.start), (dyad(4), dyad(12)));
    }

    #[test]
    fn a_barrier_between_a_name_and_the_current_scope_is_detected() {
        // A loop or fn body begins a barrier: names declared outside it may not
        // be moved or dropped inside (DESIGN ›Memory and concurrency‹, *Bodies
        // that run again or later*); names declared at or inside it may.
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
        // #91: a parse error inside a fn/while/for body skips the pop of its
        // barrier; the REPL then restores the stack's depth with `truncate`,
        // and the barrier must go with the body, or every later top-level
        // `own`/`drop` is refused for the rest of the session.
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
    fn two_live_candidates_is_the_corruption_canary() {
        // No-shadowing prevents this via declare, so inject straight into the
        // index to prove resolve reports corruption.
        let mut trie = RegexTrie::new();
        let (a, b) = (dyad(100), dyad(101));
        trie.insert("z", rec_in(dyad(1), a));
        trie.insert("z", rec_in(dyad(2), b));

        let mut scopes = ScopeStack::new();
        scopes.push(a);
        scopes.push(b); // both open at once
        assert_eq!(scopes.resolve(&trie, "z"), Err(ResolveError::Ambiguous("z".into())));
    }
}
