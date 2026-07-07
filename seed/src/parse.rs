//! The parsing tape: the working frontier the constructors drive.
//!
//! The tape holds both the dyads reduced so far but not yet final and the tokens
//! still to consume (see DESIGN ›Elaboration is deferred-reduction operator
//! precedence‹). Indexing is relative to the `cursor`, the cell of the identity
//! currently being constructed: offset 0 is the cursor, negative offsets reach
//! left into reduced context, positive offsets reach right into pending tokens.
//! `insert`/`remove` splice the frontier and keep the cursor pointing at the
//! same cell, which is the whole macro / custom-syntax mechanism.
//!
//! This module holds the parser's own state: the tape substrate (above), the
//! scope stack, and name resolution over it. The parser owns resolution; the
//! trie ([`crate::lex::trie`]) is only the name index. Still to come (V1PLAN
//! Phase 3): wiring the tape to lazily lex pending tokens from source, and the
//! deferred-reduction driver that runs each identity's `constructor`.

use std::collections::HashSet;

use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::lex::{RegexTrie, RegexTrieError};

/// A pending, not-yet-reduced token: the source span it was lexed from and, once
/// resolved, the identity it denotes. A token's identity locks at reduction, so
/// until then it may be rewritten (a higher-precedence operator to its right can
/// change it); a reduced [`Cell::Dyad`] is frozen against that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// Byte offset of the token in the source.
    pub start: usize,
    /// Byte length of the matched span.
    pub len: usize,
    /// The identity this token resolves to, or null until resolved.
    pub identity: DyadPtr,
}

impl Token {
    /// A token over `start..start + len`, not yet resolved.
    pub fn new(start: usize, len: usize) -> Self {
        Token { start, len, identity: std::ptr::null_mut() }
    }
}

/// One cell of the tape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// A pending token, still rewritable until reduced.
    Token(Token),
    /// A dyad already reduced from earlier cells, frozen against rewriting.
    Dyad(DyadPtr),
}

impl Cell {
    /// The reduced dyad, if this cell is one.
    pub fn as_dyad(&self) -> Option<DyadPtr> {
        match self {
            Cell::Dyad(d) => Some(*d),
            Cell::Token(_) => None,
        }
    }

    /// The pending token, if this cell is one.
    pub fn as_token(&self) -> Option<&Token> {
        match self {
            Cell::Token(t) => Some(t),
            Cell::Dyad(_) => None,
        }
    }
}

/// The working frontier of a scope: reduced dyads interleaved with pending
/// tokens, indexed relative to the `cursor`.
#[derive(Debug, Default)]
pub struct ParsingTape {
    cells: Vec<Cell>,
    cursor: usize,
}

impl ParsingTape {
    /// An empty tape.
    pub fn new() -> Self {
        ParsingTape { cells: Vec::new(), cursor: 0 }
    }

    /// A tape over `cells`, cursor at index 0.
    pub fn from_cells(cells: Vec<Cell>) -> Self {
        ParsingTape { cells, cursor: 0 }
    }

    /// Number of cells currently on the tape.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// True if the tape has no cells.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The cursor's absolute index (the cell of the identity being constructed).
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Move the cursor to absolute index `i` (clamped to `[0, len]`; `len` is the
    /// one-past-end "at end" position).
    pub fn set_cursor(&mut self, i: usize) {
        self.cursor = i.min(self.cells.len());
    }

    /// Map a cursor-relative offset to an absolute index, if in range.
    fn abs(&self, offset: isize) -> Option<usize> {
        let i = self.cursor as isize + offset;
        if i >= 0 && (i as usize) < self.cells.len() {
            Some(i as usize)
        } else {
            None
        }
    }

    /// The cell at cursor-relative `offset` (0 is the cursor), or `None` if out
    /// of range.
    pub fn at(&self, offset: isize) -> Option<&Cell> {
        self.abs(offset).map(|i| &self.cells[i])
    }

    /// Mutable access to the cell at cursor-relative `offset`.
    pub fn at_mut(&mut self, offset: isize) -> Option<&mut Cell> {
        self.abs(offset).map(move |i| &mut self.cells[i])
    }

    /// Insert `cell` at cursor-relative `offset`, shifting later cells right. The
    /// cursor keeps pointing at the same cell, so `insert(0, ..)` splices *just
    /// left* of the cursor and `insert(1, ..)` splices just right of it.
    /// `offset` is clamped so an out-of-range splice lands at the near end.
    pub fn insert(&mut self, offset: isize, cell: Cell) {
        let old_len = self.cells.len();
        let i = (self.cursor as isize + offset).clamp(0, old_len as isize) as usize;
        self.cells.insert(i, cell);
        // The cell previously at `cursor` shifts right only if it exists and sits
        // at or after the insertion point; follow it so `at(0)` is unchanged.
        if i <= self.cursor && self.cursor < old_len {
            self.cursor += 1;
        }
    }

    /// Remove and return the cell at cursor-relative `offset`, shifting later
    /// cells left. Removing a cell before the cursor moves its absolute index
    /// back by one so it keeps pointing at the same cell.
    pub fn remove(&mut self, offset: isize) -> Option<Cell> {
        let i = self.abs(offset)?;
        let cell = self.cells.remove(i);
        if i < self.cursor {
            self.cursor -= 1;
        }
        Some(cell)
    }
}

/// A resolved name: how many source bytes it matched and the single identity
/// live in the open scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// Bytes consumed from the start of the input.
    pub matched: usize,
    /// The identity live in the open scopes.
    pub identity: DyadPtr,
}

/// Why a name could not be resolved or declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// The spelling is not in the name index at all (an unknown token).
    Unknown,
    /// The spelling is known, but none of its declarations is in an open scope:
    /// a genuine out-of-scope use, distinct from an unknown one.
    OutOfScope,
    /// More than one live candidate. Impossible under no-shadowing, so it signals
    /// a corrupt index.
    Ambiguous,
    /// A declaration would shadow a name already live in an open scope.
    Shadowed,
    /// The name index itself rejected the lookup (e.g. a bad regex pattern).
    Index(RegexTrieError),
}

/// The parse-time scope stack: the chain of open scopes with an O(1) membership
/// set. This is the parser's own spine (the graph's ancestor chain during
/// elaboration); a scope is identified by its dyad address. Resolution filters a
/// spelling's candidates in the name index down to the one whose declaring scope
/// is open, and declaration enforces no-shadowing against it.
#[derive(Debug, Default)]
pub struct ScopeStack {
    open: Vec<DyadPtr>,
    set: HashSet<DyadPtr>,
}

impl ScopeStack {
    /// An empty scope stack.
    pub fn new() -> Self {
        ScopeStack { open: Vec::new(), set: HashSet::new() }
    }

    /// Enter `scope`.
    pub fn push(&mut self, scope: DyadPtr) {
        self.open.push(scope);
        self.set.insert(scope);
    }

    /// Leave the innermost scope, returning it.
    pub fn pop(&mut self) -> Option<DyadPtr> {
        let s = self.open.pop()?;
        self.set.remove(&s);
        Some(s)
    }

    /// The innermost open scope.
    pub fn current(&self) -> Option<DyadPtr> {
        self.open.last().copied()
    }

    /// Whether `scope` is currently open. O(1).
    pub fn is_open(&self, scope: DyadPtr) -> bool {
        self.set.contains(&scope)
    }

    /// Resolve `name` against `trie` to the single identity live in the open
    /// scopes: [`ResolveError::Unknown`] if the spelling is not indexed,
    /// [`ResolveError::OutOfScope`] if it is but no declaration is open, and
    /// [`ResolveError::Ambiguous`] if more than one is (a corrupt index, which
    /// no-shadowing otherwise makes impossible).
    pub fn resolve(&self, trie: &RegexTrie, name: &str) -> Result<Resolved, ResolveError> {
        let m = match trie.get(name) {
            Ok(m) => m,
            Err(RegexTrieError::NodeNotFound) => return Err(ResolveError::Unknown),
            Err(e) => return Err(ResolveError::Index(e)),
        };
        let mut live = m.contexts.iter().filter(|c| self.is_open(c.scope));
        match (live.next(), live.next()) {
            (None, _) => Err(ResolveError::OutOfScope),
            (Some(c), None) => Ok(Resolved { matched: m.matched, identity: c.identity }),
            (Some(_), Some(_)) => Err(ResolveError::Ambiguous),
        }
    }

    /// Declare `name` denoting `identity` in the current scope, enforcing
    /// no-shadowing: [`ResolveError::Shadowed`] if `name` already resolves to a
    /// live candidate in the open scopes. A known-but-out-of-scope or unknown
    /// name is free to (re)declare here. Requires a current scope.
    pub fn declare(
        &self,
        trie: &mut RegexTrie,
        name: &str,
        identity: DyadPtr,
    ) -> Result<(), ResolveError> {
        let scope = self.current().expect("declare needs an open scope");
        match self.resolve(trie, name) {
            // Already live in an open scope: shadowing is disallowed.
            Ok(_) => return Err(ResolveError::Shadowed),
            // Known but closed, or unknown: both fine to declare here.
            Err(ResolveError::OutOfScope | ResolveError::Unknown) => {}
            // Ambiguous or an index error: surface it rather than declaring atop.
            Err(e) => return Err(e),
        }
        trie.insert(name, IdContext::new(identity, scope));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A distinct sentinel address per tag (never dereferenced).
    fn dyad(tag: usize) -> DyadPtr {
        std::ptr::without_provenance_mut(tag)
    }

    fn dyad_cells(tags: &[usize]) -> Vec<Cell> {
        tags.iter().map(|&t| Cell::Dyad(dyad(t))).collect()
    }

    #[test]
    fn offset_indexing_is_cursor_relative() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12, 13]));
        t.set_cursor(2); // points at dyad(12)
        assert_eq!(t.at(0).unwrap().as_dyad(), Some(dyad(12)));
        assert_eq!(t.at(-1).unwrap().as_dyad(), Some(dyad(11)));
        assert_eq!(t.at(1).unwrap().as_dyad(), Some(dyad(13)));
        assert_eq!(t.at(-2).unwrap().as_dyad(), Some(dyad(10)));
        assert!(t.at(2).is_none()); // past the end
        assert!(t.at(-3).is_none()); // before the start
    }

    #[test]
    fn insert_left_keeps_cursor_on_same_cell() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1); // dyad(11)
        t.insert(0, Cell::Dyad(dyad(99))); // splice just left of the cursor
        assert_eq!(t.at(0).unwrap().as_dyad(), Some(dyad(11)));
        assert_eq!(t.at(-1).unwrap().as_dyad(), Some(dyad(99)));
        assert_eq!(t.len(), 4);
    }

    #[test]
    fn insert_right_leaves_cursor() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(1); // dyad(11)
        t.insert(1, Cell::Dyad(dyad(99)));
        assert_eq!(t.at(0).unwrap().as_dyad(), Some(dyad(11)));
        assert_eq!(t.at(1).unwrap().as_dyad(), Some(dyad(99)));
        assert_eq!(t.at(2).unwrap().as_dyad(), Some(dyad(12)));
    }

    #[test]
    fn remove_left_keeps_cursor_on_same_cell() {
        let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
        t.set_cursor(2); // dyad(12)
        let gone = t.remove(-1); // remove dyad(11)
        assert_eq!(gone.unwrap().as_dyad(), Some(dyad(11)));
        assert_eq!(t.at(0).unwrap().as_dyad(), Some(dyad(12)));
        assert_eq!(t.at(-1).unwrap().as_dyad(), Some(dyad(10)));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn token_and_dyad_cells_coexist() {
        // The tape's defining property: pending tokens and reduced dyads on one
        // frontier.
        let mut t = ParsingTape::new();
        t.insert(0, Cell::Token(Token::new(0, 3)));
        t.insert(1, Cell::Dyad(dyad(7)));
        assert!(t.at(0).unwrap().as_token().is_some());
        assert_eq!(t.at(1).unwrap().as_dyad(), Some(dyad(7)));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn rewrite_a_pending_token_in_place() {
        // Tokens are mutable until reduced: a constructor can change one on the
        // tape (the mechanism behind token-rewriting operators like `X`).
        let mut t = ParsingTape::from_cells(vec![Cell::Token(Token::new(4, 1))]);
        if let Some(Cell::Token(tok)) = t.at_mut(0) {
            tok.len = 2;
        }
        assert_eq!(t.at(0).unwrap().as_token().unwrap().len, 2);
    }

    // --- scope stack + name resolution --------------------------------------

    #[test]
    fn resolves_a_name_declared_in_an_open_scope() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        let id = dyad(1);
        scopes.declare(&mut trie, "a", id).unwrap();
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
        scopes.declare(&mut trie, "x", dyad(1)).unwrap();
        scopes.pop(); // close outer

        scopes.push(inner);
        scopes.declare(&mut trie, "x", dyad(2)).unwrap();
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
        scopes.declare(&mut trie, "y", dyad(1)).unwrap();
        scopes.pop(); // close the scope

        assert_eq!(scopes.resolve(&trie, "y"), Err(ResolveError::OutOfScope));
        assert_eq!(scopes.resolve(&trie, "nope"), Err(ResolveError::Unknown));
    }

    #[test]
    fn shadowing_is_rejected() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let (outer, inner) = (dyad(100), dyad(101));

        scopes.push(outer);
        scopes.declare(&mut trie, "a", dyad(1)).unwrap();
        // Same scope: redeclaration rejected.
        assert_eq!(scopes.declare(&mut trie, "a", dyad(2)), Err(ResolveError::Shadowed));
        // Nested scope while the outer declaration is live: still rejected.
        scopes.push(inner);
        assert_eq!(scopes.declare(&mut trie, "a", dyad(3)), Err(ResolveError::Shadowed));
    }

    #[test]
    fn two_live_candidates_is_the_corruption_canary() {
        // No-shadowing prevents this via declare, so inject straight into the
        // index to prove resolve reports corruption.
        let mut trie = RegexTrie::new();
        let (a, b) = (dyad(100), dyad(101));
        trie.insert("z", IdContext::new(dyad(1), a));
        trie.insert("z", IdContext::new(dyad(2), b));

        let mut scopes = ScopeStack::new();
        scopes.push(a);
        scopes.push(b); // both open at once
        assert_eq!(scopes.resolve(&trie, "z"), Err(ResolveError::Ambiguous));
    }
}
