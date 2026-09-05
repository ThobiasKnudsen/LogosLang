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
//! `(` on the one precedence axis ([`crate::identities::meta::prec`]) — the
//! brackets, the literals, `#`, `import`, `:=`, and the identities that read
//! their own bracket or right side — and, at the segment boundary (`,`, the
//! closer, the end of input), constructs the rest highest precedence first,
//! associativity breaking ties, each constructor taking what its syntax needs
//! from the fully lexed segment, left or right, with no lookahead. A leftover
//! cell is the checked error; prose is lifted out beside the segment's
//! expression. Each identity's `constructor` ([`ConstructFn`]) edits the tape
//! in place and reports applied or declined; the driver decides only *when*.
//!
//! This module also holds the scope stack and name resolution over it. The
//! parser owns resolution; the trie ([`crate::regex_trie`]) is only the name
//! index. Still to come: the tape's four affordances as Logos-reachable
//! identities and a cell as a bare dyad pointer with `is_constructed` beside
//! it (#60), constructors written in Logos and a precedence spelled relative
//! to another's (#61), `lex «…»` (#62).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::regex_trie::{RegexTrie, RegexTrieError};
use crate::store::Store;

/// A pending, not-yet-reduced token: the source span it was lexed from and the
/// identity it denotes. A token's identity is not fixed until it is consumed
/// into a dyad — a reduced [`Cell::Dyad`] is frozen against rewriting, a token
/// is not. The driver resolves names eagerly at scan where it can (the
/// identity set on push) and pushes a *fresh* name as the null-until-consumed
/// form ([`Token::new`]): a following `:=`/`:` declares it at reduction, and
/// any other consumer converts it through `as_operand`, which re-resolves the
/// span. Full deferred resolution for already-resolvable names — what
/// token-rewriting operators need — rides the same null path later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// Byte offset of the token in the source.
    pub start: usize,
    /// Byte length of the matched span.
    pub len: usize,
    /// The identity this token denotes, or null until consumed (a fresh name
    /// awaiting its declaration or its resolution error).
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
    /// A bracket, constructed at discovery: the finished scope cell `(`'s
    /// constructor leaves (DESIGN ›The scope's constructor is the driver‹).
    /// Told apart from a plain dyad because `X (…)` is X's decision — a
    /// callable reads the bracket to its right as its arguments, a numeric
    /// logos as its conversion — while `f 3` is not a call.
    Scope(DyadPtr),
}

impl Cell {
    /// The reduced dyad, if this cell is one (a bracket's scope included).
    pub fn as_dyad(&self) -> Option<DyadPtr> {
        match self {
            Cell::Dyad(d) | Cell::Scope(d) => Some(*d),
            Cell::Token(_) => None,
        }
    }

    /// The pending token, if this cell is one.
    pub fn as_token(&self) -> Option<&Token> {
        match self {
            Cell::Token(t) => Some(t),
            Cell::Dyad(_) | Cell::Scope(_) => None,
        }
    }

    /// The bracket's scope, if this cell is one.
    pub fn as_scope(&self) -> Option<DyadPtr> {
        match self {
            Cell::Scope(d) => Some(*d),
            _ => None,
        }
    }
}

/// The working frontier of a scope: reduced dyads interleaved with pending
/// tokens, indexed relative to the `cursor`.
#[derive(Debug, Default)]
pub struct ParsingTape {
    cells: Vec<Cell>,
    /// The source offset each cell was lexed at — the derived source map's
    /// seed-side stand-in, kept beside the cells so an error over a cell
    /// (a leftover one, an undeclared name) points at where it stood.
    starts: Vec<usize>,
    cursor: usize,
}

impl ParsingTape {
    /// An empty tape.
    pub fn new() -> Self {
        ParsingTape { cells: Vec::new(), starts: Vec::new(), cursor: 0 }
    }

    /// A tape over `cells`, cursor at index 0 (positions unknown).
    pub fn from_cells(cells: Vec<Cell>) -> Self {
        let starts = vec![0; cells.len()];
        ParsingTape { cells, starts, cursor: 0 }
    }

    /// The source offset the cell at absolute index `i` was lexed at.
    pub fn start_of(&self, i: usize) -> usize {
        self.starts.get(i).copied().unwrap_or(0)
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
        let start = self.start_of(self.cursor);
        self.cells.insert(i, cell);
        self.starts.insert(i, start);
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
        self.starts.remove(i);
        if i < self.cursor {
            self.cursor -= 1;
        }
        Some(cell)
    }

    /// Append `cell`, lexed at source offset `start`, and move the cursor to
    /// it. Used by the driver as it lexes a segment onto the frontier.
    pub fn push(&mut self, cell: Cell, start: usize) {
        self.cells.push(cell);
        self.starts.push(start);
        self.cursor = self.cells.len() - 1;
    }

    /// The cell at absolute index `i`, or `None` if out of range.
    pub fn cell(&self, i: usize) -> Option<&Cell> {
        self.cells.get(i)
    }

    /// The last cell on the tape, if any.
    pub fn last(&self) -> Option<&Cell> {
        self.cells.last()
    }

    /// Remove and return the last cell, if any. Used by application: the callee
    /// preceding a `(` is popped and replaced by the call node.
    pub fn pop(&mut self) -> Option<Cell> {
        let cell = self.cells.pop();
        self.starts.pop();
        self.cursor = self.cursor.min(self.cells.len().saturating_sub(1));
        cell
    }

    /// Reduce a binary operator: replace the three cells at `i - 1`, `i`, `i + 1`
    /// with a single reduced `dyad`. Returns false if `i` is not flanked by two
    /// cells. The cursor is clamped to the shortened tape.
    pub fn reduce_binary(&mut self, i: usize, dyad: DyadPtr) -> bool {
        if i == 0 || i + 1 >= self.cells.len() {
            return false;
        }
        let start = self.starts[i - 1];
        self.cells.splice(i - 1..=i + 1, [Cell::Dyad(dyad)]);
        self.starts.splice(i - 1..=i + 1, [start]);
        self.cursor = self.cursor.min(self.cells.len().saturating_sub(1));
        true
    }

    /// The construct's own token — the cursor cell — its source span still
    /// attached. How an atom constructor reaches its matched text.
    pub fn own_token(&self) -> Option<Token> {
        self.at(0).and_then(Cell::as_token).copied()
    }

    /// Replace the cursor cell — the construct's own token — with the dyad it
    /// built: the in-place edit nearly every constructor ends with. (A
    /// constructor may equally leave a *token* here via [`ParsingTape::at_mut`],
    /// or splice cells anywhere with `insert`/`remove`; this is only the
    /// common case.)
    pub fn place(&mut self, dyad: DyadPtr) {
        *self.at_mut(0).expect("the construct's token cell is at the cursor") = Cell::Dyad(dyad);
    }

    /// Replace the cursor cell — `(`'s own token — with the finished bracket
    /// cell: the scope its constructor built, marked as a bracket so the
    /// identity to its left can read it as its argument.
    pub fn place_scope(&mut self, dyad: DyadPtr) {
        *self.at_mut(0).expect("the construct's token cell is at the cursor") = Cell::Scope(dyad);
    }

    /// Reduce the triple around the cursor — `tape[-1]`, the construct's own
    /// token, `tape[+1]` — to the single `dyad`: an infix constructor's
    /// in-place splice at reduction.
    pub fn reduce_here(&mut self, dyad: DyadPtr) {
        let i = self.cursor;
        assert!(self.reduce_binary(i, dyad), "an infix reduces between two operands");
    }
}

/// A resolved name: how many source bytes it matched, the single identity
/// live in the open scopes, and the scope it was declared in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// Bytes consumed from the start of the input.
    pub matched: usize,
    /// The identity live in the open scopes.
    pub identity: DyadPtr,
    /// The scope the winning declaration was made in (an open ancestor) — what
    /// a rebind that completes that declaration must target.
    pub scope: DyadPtr,
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
    /// The spelling is declared in an open scope, but an `own` or `drop` above
    /// made it dead: a read, a write, or a pass is refused, and only `:=` may
    /// follow (DESIGN ›Name resolution is scope-filtered‹, ruled 3 September
    /// 2026).
    Dead,
    /// The name index itself rejected the lookup (e.g. a bad regex pattern).
    Index(RegexTrieError),
}

/// One act on the name index since the last [`ScopeStack::commit`], undone
/// newest-first by [`ScopeStack::rollback`].
#[derive(Debug)]
enum Journal {
    /// `name` was declared in `scope`: rollback removes its live entry.
    Declared { name: String, scope: DyadPtr },
    /// `name`'s entry for `identity` in `scope` was made dead by an `own` or
    /// `drop`: rollback restores the `end` it had before.
    Ended { name: String, scope: DyadPtr, identity: DyadPtr, prev_end: DyadPtr },
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
    name: String,
    scope: DyadPtr,
    identity: DyadPtr,
    endpoint: Endpoint,
}

/// A bare name an `own` or `drop` is about to make dead: what
/// [`Parser::parse_place_operand`] hands back so the keyword's constructor can
/// call [`Parser::mark_dead`] with the node it built.
#[derive(Debug)]
pub(crate) struct Ended {
    pub(crate) name: String,
    pub(crate) scope: DyadPtr,
    pub(crate) identity: DyadPtr,
}

/// The parse-time scope stack: the chain of open scopes with an O(1) membership
/// set. This is the parser's own spine (the graph's ancestor chain during
/// elaboration); a scope is identified by its dyad address. Resolution filters a
/// spelling's candidates in the name index down to the one whose declaring scope
/// is open and whose range covers the frontier (DESIGN ›Name resolution is
/// scope-filtered‹: live = declared, scope open, not yet made dead by `own` or
/// `drop`), and declaration enforces no-shadowing against it.
#[derive(Debug, Default)]
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
        self.pending.retain(|p| p.scope != s);
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
        let Some(idx) = self.open.iter().position(|&s| s == scope) else {
            return false;
        };
        self.barriers.iter().any(|&b| b > idx)
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
    /// parses (the REPL) restores its known depth with this.
    pub fn truncate(&mut self, depth: usize) {
        while self.open.len() > depth {
            self.pop();
        }
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
                Journal::Ended { name, scope, identity, prev_end } => {
                    trie.update(&name, |c| {
                        if c.scope == scope && c.identity == identity {
                            c.end = prev_end;
                            true
                        } else {
                            false
                        }
                    });
                }
            }
        }
        self.pending.clear();
    }

    /// Resolve `name` against `trie` to the single identity live in the open
    /// scopes: [`ResolveError::Unknown`] if the spelling is not indexed,
    /// [`ResolveError::OutOfScope`] if it is but no declaration is open,
    /// [`ResolveError::Dead`] if the open declarations were all made dead by an
    /// `own` or `drop`, and [`ResolveError::Ambiguous`] if more than one is
    /// live (a corrupt index, which no-shadowing otherwise makes impossible).
    pub fn resolve(&self, trie: &RegexTrie, name: &str) -> Result<Resolved, ResolveError> {
        let m = match trie.get(name) {
            Ok(m) => m,
            Err(RegexTrieError::NodeNotFound) => return Err(ResolveError::Unknown),
            Err(e) => return Err(ResolveError::Index(e)),
        };
        // Word characters bind maximally: a match that ends mid-identifier is not
        // a token — `incr` must never lex as `in` + `cr`, nor `i32abc` as `i32` +
        // `abc`. (Symbol tokens like `:=` are unaffected: they do not end in a
        // word character.)
        let bytes = name.as_bytes();
        let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        if m.matched > 0
            && m.matched < bytes.len()
            && word(bytes[m.matched - 1])
            && word(bytes[m.matched])
        {
            return Err(ResolveError::Unknown);
        }
        // During elaboration the point of use is the frontier, so "range covers
        // the point" is exactly "not yet made dead" (DESIGN ›Name resolution is
        // scope-filtered‹, ruled 3 September 2026).
        let mut live = m.contexts.iter().filter(|c| self.is_open(c.scope) && !c.is_dead());
        match (live.next(), live.next()) {
            (None, _) => {
                if m.contexts.iter().any(|c| self.is_open(c.scope)) {
                    Err(ResolveError::Dead)
                } else {
                    Err(ResolveError::OutOfScope)
                }
            }
            (Some(c), None) => {
                Ok(Resolved { matched: m.matched, identity: c.identity, scope: c.scope })
            }
            (Some(_), Some(_)) => Err(ResolveError::Ambiguous),
        }
    }

    /// Declare `name` denoting `identity` in the current scope, enforcing
    /// no-shadowing: [`ResolveError::Shadowed`] if `name` already resolves to a
    /// live candidate in the open scopes. A known-but-out-of-scope, unknown, or
    /// dead name is free to (re)declare here — the dead case being the one
    /// thing that may follow an `own`/`drop`, a fresh entry beside the dead one.
    /// Requires a current scope. The declaration is journalled for
    /// [`ScopeStack::rollback`], and its `start` awaits the finished body item.
    pub fn declare(
        &mut self,
        trie: &mut RegexTrie,
        name: &str,
        identity: DyadPtr,
    ) -> Result<(), ResolveError> {
        let scope = self.current().expect("declare needs an open scope");
        match self.resolve(trie, name) {
            // Already live in an open scope: shadowing is disallowed.
            Ok(_) => return Err(ResolveError::Shadowed),
            // Known but closed, unknown, or dead: all fine to declare here.
            Err(ResolveError::OutOfScope | ResolveError::Unknown | ResolveError::Dead) => {}
            // Ambiguous or an index error: surface it rather than declaring atop.
            Err(e) => return Err(e),
        }
        trie.insert(name, IdContext::new(identity, scope));
        self.journal.push(Journal::Declared { name: name.to_string(), scope });
        self.pending.push(Pending {
            name: name.to_string(),
            scope,
            identity,
            endpoint: Endpoint::Start,
        });
        Ok(())
    }

    /// Make `name`'s entry for `identity` in `scope` dead from here on: an
    /// `own` or `drop` (`node`) emptied its place, so every later use is
    /// refused until a `:=` redeclares the spelling (DESIGN ›Memory and
    /// concurrency‹, *`own` and `drop` are static*). `node` is the provisional
    /// `end`; [`ScopeStack::settle_item`] replaces it with the body item once
    /// the line is complete. Journalled for [`ScopeStack::rollback`].
    pub fn mark_dead(
        &mut self,
        trie: &mut RegexTrie,
        name: &str,
        scope: DyadPtr,
        identity: DyadPtr,
        node: DyadPtr,
    ) {
        let mut prev_end = std::ptr::null_mut();
        trie.update(name, |c| {
            if c.scope == scope && c.identity == identity {
                prev_end = c.end;
                c.end = node;
                true
            } else {
                false
            }
        });
        self.journal.push(Journal::Ended { name: name.to_string(), scope, identity, prev_end });
        self.pending.push(Pending {
            name: name.to_string(),
            scope,
            identity,
            endpoint: Endpoint::End,
        });
    }

    /// `scope` just appended `item` to its body: every range endpoint pending
    /// for that scope now points at the item, the declaring or ending line
    /// as a whole (an `own` inside an `if` body ends the outer name at the
    /// `if`, DESIGN ›Name resolution is scope-filtered‹).
    pub fn settle_item(&mut self, trie: &mut RegexTrie, scope: DyadPtr, item: DyadPtr) {
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].scope != scope {
                i += 1;
                continue;
            }
            let p = self.pending.swap_remove(i);
            trie.update(&p.name, |c| {
                if c.scope == p.scope && c.identity == p.identity {
                    match p.endpoint {
                        Endpoint::Start => c.start = item,
                        Endpoint::End => c.end = item,
                    }
                    true
                } else {
                    false
                }
            });
        }
    }

    /// Re-point the just-declared `name` in the current scope at `identity`.
    /// Used by the declaration fixpoint when the value turns out to *be* an
    /// existing identity (a logos): the name becomes another spelling of that
    /// node, so pointer-identity checks (`is_numtype_node`, logos equality) see
    /// the original. The journal entry from the declare still covers it, and
    /// the entry's range is kept.
    pub fn rebind(&mut self, trie: &mut RegexTrie, name: &str, identity: DyadPtr) {
        let scope = self.current().expect("rebind needs an open scope");
        self.rebind_at(trie, name, identity, scope);
    }

    /// Re-point `name`, declared in `scope` (an open ancestor, from
    /// [`Resolved::scope`]), at `identity`. Unlike [`ScopeStack::rebind`] the
    /// target is the *declaring* scope, not the current one: a logos variable's
    /// fill inside a comptime-taken branch completes the outer declaration,
    /// rather than binding a block-local spelling that dies with the branch.
    /// The live entry is changed in place, so its range survives; a pending
    /// endpoint follows the identity.
    pub fn rebind_at(&mut self, trie: &mut RegexTrie, name: &str, identity: DyadPtr, scope: DyadPtr) {
        let mut old = std::ptr::null_mut();
        trie.update(name, |c| {
            if c.scope == scope && !c.is_dead() {
                old = c.identity;
                c.identity = identity;
                true
            } else {
                false
            }
        });
        for p in &mut self.pending {
            if p.scope == scope && p.identity == old && p.name == name {
                p.identity = identity;
            }
        }
    }
}

/// Operator associativity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
}

/// The core logos handles the parser needs to logos the nodes it opens and to
/// resolve abstract operators. Bundled so that adding a handle does not churn
/// [`Parser::new`]'s signature; an `Infix` `build` callback receives it so an
/// operator like `+` can pick its concrete machine op from the operand logos.
#[derive(Debug, Clone, Copy)]
pub struct CoreTypes {
    /// `scope`: the logos of each scope the parser opens.
    pub scope: DyadPtr,
    /// `array` (of `dyad@`): a sequence's expression list rides behind one.
    pub array_: DyadPtr,
    /// `fn`: the logos of a function; a call whose callee is `fn`-typed yields a
    /// value (which the arithmetic operators' `is_numeric` check treats as numeric).
    pub fn_type: DyadPtr,
    /// `i32`: an alias for `numtypes[I32]`, the seed's default numeric logos.
    pub i32_: DyadPtr,
    /// The numeric primitive logos nodes, indexed by `NumType` (null if unregistered).
    /// A resolved operator stores the relevant one in its value slot.
    pub numtypes: [DyadPtr; 10],
    /// `bool`: the logos a comparison produces and an `if` condition must be.
    pub bool_: DyadPtr,
    /// `rational_number`: a numeric literal, molds to a concrete numeric logos.
    pub rational: DyadPtr,
    /// `return`: the optional early yield; used to commit a `return`-wrapped rational
    /// tail to the function's declared return logos.
    pub return_: DyadPtr,
    /// `if`: the value-producing conditional; used to commit a rational in either branch
    /// (a tail position) to the function's declared return logos.
    pub if_: DyadPtr,
    /// `while`: the loop statement; unit-valued, so value positions reject it.
    pub while_: DyadPtr,
    /// `for`: the counted-loop statement; unit-valued like `while`.
    pub for_: DyadPtr,
    /// The `logos := logos ?` root; pointer logos nodes are typed by it.
    pub type_: DyadPtr,
    /// `deref`: the dereference node postfix `@` builds.
    pub deref_: DyadPtr,
    /// `storeptr`: the store-through node `=` builds over a deref lhs.
    pub storeptr_: DyadPtr,
    /// `addr`: the address-of node prefix `&` builds (resolves its place's
    /// address at run/lower time, per-activation for a frame local).
    pub addr_: DyadPtr,
    /// `alloc`: the heap-allocation keyword; its node yields an owning pointer (#49).
    pub alloc_: DyadPtr,
    /// `own`: the ownership-move keyword; yields the pointer, empties the source.
    pub own_: DyadPtr,
    /// `drop`: the eager-teardown keyword; runs the place's destructor, empties it.
    pub drop_: DyadPtr,
    /// `free`: the allocator teardown `alloc` inserts as `defer free <place>`.
    pub free_: DyadPtr,
    /// `defer`: the scope-exit LIFO teardown holder (`defer <expr>`).
    pub defer_: DyadPtr,
    /// `pub`: the first gate identity (#33); a declare node's gate slot holds
    /// it when the declaration was written `pub name := …`.
    pub pub_: DyadPtr,
    /// `import`: the one identity that loads a file (#58); its node is the
    /// reflectable trace of the load, and running it re-yields the file's tail.
    pub import_: DyadPtr,
    /// `dyad`: the spelled view identity (#52) — `(dyad a)` wraps a
    /// value as its cell, and `.` reads the cell.
    pub dyad_: DyadPtr,
    /// `index`: the passive node a `[i]` cell carries (a comptime literal in
    /// the seed).
    pub index_: DyadPtr,
    /// `construct`: the record-construction statement a record-typed call builds.
    pub construct_: DyadPtr,
    /// `string`: the text-literal logos (`«…»`); inert in the seed, above all the
    /// comment substance.
    pub string_: DyadPtr,
    /// `comment`: the prose-node logos a statement-level `#` builds; reflectable
    /// graph structure, invisible to value flow.
    pub comment_: DyadPtr,
    /// `convert`: the shared scalar numeric conversion; a conversion node's result logos
    /// is its target (recognized as a numeric-producing operand).
    pub convert: DyadPtr,
    /// `+` (addition); recognized as a numeric-producing operand.
    pub plus: DyadPtr,
    /// `-` (subtraction); recognized as a numeric-producing operand.
    pub minus: DyadPtr,
    /// `*` (multiplication); recognized as a numeric-producing operand.
    pub times: DyadPtr,
    /// `/` (division); recognized as a numeric-producing operand.
    pub div_: DyadPtr,
    /// `%` (remainder); recognized as a numeric-producing operand.
    pub rem_: DyadPtr,
    /// `<` (less-than); its result is `bool` (an `if` condition).
    pub lt: DyadPtr,
    /// `>` (greater-than); its result is `bool`.
    pub gt: DyadPtr,
    /// `==` (equality); its result is `bool`.
    pub eq: DyadPtr,
    /// `<=` (less-than-or-equal); its result is `bool`.
    pub le: DyadPtr,
    /// `>=` (greater-than-or-equal); its result is `bool`.
    pub ge: DyadPtr,
    /// `!=` (inequality); its result is `bool`.
    pub ne: DyadPtr,
    /// `and` (short-circuiting logical conjunction); its result is `bool`.
    pub and_: DyadPtr,
    /// `or` (short-circuiting logical disjunction); its result is `bool`.
    pub or_: DyadPtr,
    /// `not` (logical negation); its result is `bool`.
    pub not_: DyadPtr,
    /// `=` (assignment); its applications yield the stored value.
    pub assign: DyadPtr,
    /// `declare`: the logos of the declaration node `name := value` builds; a
    /// statement yielding unit.
    pub declare_: DyadPtr,
    /// `compile`: the fn logos's shared member (`f.compile()`); a statement
    /// yielding unit, so value positions reject it.
    pub compile_: DyadPtr,
    /// `callable`: the logos of every exec leaf and of a compiled fn's code
    /// (`[entry: @exec, convention]`).
    pub callable_: DyadPtr,
    /// `container-i64`: the convention compiled artifacts are minted under.
    pub conv_container: DyadPtr,
    /// `(` — the opening paren/call token; the expect-helpers compare against it.
    pub open_: DyadPtr,
    /// `)` — the closing paren token.
    pub close_: DyadPtr,
    /// `:` — the typed-declaration / field-list token.
    /// `,` — the one explicit separator.
    pub sep_: DyadPtr,
    /// `->` — the return-logos arrow.
    pub arrow_: DyadPtr,
    /// `else` — the branch token `if`'s constructor consumes.
    pub else_: DyadPtr,
    /// `in` — the loop-range token `for`'s constructor consumes.
    pub in_: DyadPtr,
    /// `..` — the range token `for`'s constructor consumes.
    pub dotdot_: DyadPtr,
    /// `.` — the field-access token (its constructor consumes `tape[-1]`).
    pub dot_: DyadPtr,
    /// `@` — the pointer token (postfix deref / pointer-logos prefix).
    pub at_: DyadPtr,
    /// `:=` — the declaration token.
    pub declare_tok: DyadPtr,
    /// The concrete-op leaves (`add_i32`, `lt_f64`, `store_u8`, …): the
    /// parse-time resolver's `(family, operand logos) → leaf` table. A builder
    /// resolves an application to one leaf and stores it in the node's op slot;
    /// run jumps through the node, never a table (issue #44).
    pub ops: crate::identities::ops::OpLeaves,
}

/// The fields of a function node's value record, in order, as built by
/// [`Parser::parse_fn`]: the input `record`, the return logos, the reflectable body,
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

/// The activation-record byte size a function node declares in its [`FN_FRAME`]
/// slot: the `u64` the slot's leaf holds, or `0` when the slot is null (no
/// parameters and no locals). Read on every call to size the per-call storage.
///
/// # Safety
/// `fn_node` must be a function node whose value is `[input, output, body,
/// bcode, frame]` as [`Parser::parse_fn`] builds it.
pub unsafe fn fn_frame_size(fn_node: DyadPtr) -> usize {
    let frame = *((*fn_node).value as *const DyadPtr).add(FN_FRAME);
    if frame.is_null() {
        0
    } else {
        std::ptr::read_unaligned((*frame).value as *const u64) as usize
    }
}


/// Whether a constructor applied. A constructor never hands a result to a
/// scheduling driver: it edits the tape *in place* — usually replacing its own
/// token with the dyad it built ([`ParsingTape::place`]), splicing out the
/// neighbours it consumed, and sometimes leaving another *token*, or inserting
/// tokens elsewhere on the frontier (the macro mechanism: DESIGN ›a
/// constructor may splice tokens in or drop upcoming ones before they lex‹).
/// `Placed` reports only that it did; the tape is the result. `Decline` is
/// "not mine": the constructor consumed nothing and touched nothing, and the
/// driver drops its token, rewinds to its start, and lets the expression
/// finalize (or shifts the token, for an extender) — an explicit signal,
/// because a tape left holding a token is a legitimate outcome, not a
/// refusal.
pub enum Constructed {
    /// The construct applied; its edits are on the tape.
    Placed,
    /// The construct does not apply here; nothing was consumed.
    Decline,
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
/// precedence decision — never what they leave.
pub type ConstructFn =
    fn(&mut Parser, DyadPtr, &mut ParsingTape) -> Result<Constructed, ParseError>;

/// Why elaboration failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Name resolution failed.
    Resolve(ResolveError),
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
    /// A fn signature's `->` was not followed by a return logos.
    ExpectedReturnType,
    /// An abstract operator (e.g. `+`) could not resolve a concrete machine op for
    /// its operand logos (DESIGN ›a `+` over mismatched or sizeless logos simply
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
    /// A binary operator's operands were two *different* concrete numeric logos (e.g.
    /// `i32` and `f64`). Cross-logos arithmetic needs an explicit cast; there is no
    /// implicit coercion.
    TypeMismatch,
    /// A number literal had no exact value in the logos it was committed to (a decimal
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
    /// A gate word (`pub`) was not followed by a declaration: a gate fills a
    /// declare node's gate slot, so anything else leaves it nothing to mark.
    GateNeedsDeclaration,
    /// A declaration was gated twice (`pub pub x := …`).
    DoubleGate,
    /// An `import` was not followed by a path token.
    ExpectedPath,
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
    /// scalar, an index past the arity, `.fields` of a non-record logos), an
    /// unknown member on a view or logos, or a read whose answer is the honest
    /// undefined (a null constructor slot). Answering `?` instead waits for
    /// the `?` identity (#38).
    BadReflectRead,
    /// A collection member (`.operands`, `.roles`) without its `[index]` —
    /// element access is `[…]` (ruled August 2026), and the bare collection
    /// as a first-class value waits for the array logos (#47).
    ExpectedIndexBracket,
    /// `.logos` on something that is not a dyad view: `.` reads only the
    /// fields a logos defines, which are about the value — a value's logos is
    /// never one of its own fields (ruled August 2026). The view puts the
    /// logos into the value: `(dyad x).ty`.
    TypeNeedsView,
    /// A record construction's argument count did not match its field count.
    CtorArity,
    /// A `for` was not followed by a loop-variable name.
    ExpectedLoopVar,
    /// A `for`'s loop variable was not followed by `in`.
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
    /// A typed declaration's `name :` — or a logos variable's fill `name = …` —
    /// was followed by something that is not a logos value: the logos slot holds
    /// a logos, so the expression must evaluate to one (a spelled logos, or a
    /// `-> logos` call resolved at parse time).
    BadDeclaredType,
    /// A logos variable was assigned inside a deferred or repeated body (a fn
    /// body, loop body, or runtime `if` branch). The fill rebinds the name at
    /// parse time, which is only sound where parsing and running coincide.
    NonComptimeTypeAssign,
    /// A typed declaration of a non-numeric logos (`a := logos ?`, a record, a
    /// pointer, `bool`, `void`) — the declared-logos storage for those is not in
    /// the seed yet, and this names the gap instead of mis-storing the value.
    NonNumericDeclaredType,
    /// A `-> logos` call could not be resolved at parse time — either running it
    /// failed (its arguments were not comptime-known) or it did not yield a logos.
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
    /// behind the return logos, and a plain `@T` carries no destructor, so the
    /// caller could not know it owes a `free`. Fail-closed until a return logos
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

/// Build a call node `{type: callee, value: [args…, null]}`, the application
/// `callee(args)`. Like a binary operator's `{type: op, value: [lhs, rhs]}`, a call's
/// value is the operand array of its arguments (null-terminated so `run` can count
/// them); a nullary call carries a null value. The callee's logos decides how the
/// call runs, exactly as an operator's does.
/// Whether `node`'s result is a `bool`: a `bool` literal/value, a comparison
/// (`<`/`>`/`==`/…), or a logical operator (`and`/`or`/`not`). An `if` condition and
/// a logical operator's operands must be one; arithmetic and other values are not.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_bool_result(types: &CoreTypes, node: DyadPtr) -> bool {
    let logos = (*node).ty;
    // A sequence's value is its trailing expression's.
    if logos == types.scope {
        return match last_sequence_expr(node) {
            Some(last) => is_bool_result(types, last),
            None => false,
        };
    }
    logos == types.bool_
        || logos == types.lt
        || logos == types.gt
        || logos == types.eq
        || logos == types.le
        || logos == types.ge
        || logos == types.ne
        || logos == types.and_
        || logos == types.or_
        || logos == types.not_
}

/// The parse-time truth of a bool literal — `{type: bool, value -> i32 0/1}`, the
/// shape the `true`/`false` keywords and every comptime fold produce — or `None`
/// for anything else. Deliberately no scope unwrapping: a sequence-valued
/// condition may carry effectful non-tail expressions that a fold would silently
/// drop, so only a bare literal (pure by construction) counts as comptime.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn bool_literal_value(types: &CoreTypes, node: DyadPtr) -> Option<bool> {
    if (*node).ty != types.bool_ || (*node).value.is_null() {
        return None;
    }
    Some(std::ptr::read_unaligned((*node).value as *const i32) != 0)
}

/// The trailing *value* expression of a sequence node
/// `{type: scope, value: [exprs, op]}` — trailing comment nodes are prose, not
/// the tail — or `None` for a scope with no expression array (a
/// record/parameter-list scope).
///
/// # Safety
/// `node` must be a valid dyad from the store; a non-null value must be the
/// `[exprs, op]` pair as built by [`Parser::parse_sequence`].
pub(crate) unsafe fn last_sequence_expr(node: DyadPtr) -> Option<DyadPtr> {
    if (*node).value.is_null() {
        return None;
    }
    let arr = *((*node).value as *const DyadPtr);
    crate::identities::array::items(arr)
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
/// `node` must be a valid dyad from the store, with the value shapes its logos
/// implies (as the parser builds them).
unsafe fn contains_return(types: &CoreTypes, node: DyadPtr) -> bool {
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
        return crate::identities::array::items(arr)
            .iter()
            .any(|&e| contains_return(types, e));
    }
    false
}

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
/// identities, so two importers of the same file see the same logos, never two
/// copies — and the import graph must be a DAG, so a path met again while its
/// own load is still in progress is a cycle (ruled August 2026). The REPL
/// threads one registry across its per-line parsers (a session is a run); the
/// command-line driver's single parser holds one for the whole run.
#[derive(Debug, Default)]
pub struct Imports {
    entries: HashMap<PathBuf, ImportState>,
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
/// stack, and reduces the tape by operator precedence, running each identity's
/// native `Construct`. The scheduling is a deferred-reduction operator
/// precedence over the explicit tape (not Pratt): operators wait on the tape as
/// pending tokens until precedence says to reduce them.
pub struct Parser<'a> {
    source: &'a str,
    pos: usize,
    scopes: ScopeStack,
    store: &'a mut Store,
    trie: &'a mut RegexTrie,
    /// The core logos handles the parser logos opened nodes with (see [`CoreTypes`]).
    types: CoreTypes,
    /// The placeholder of the declaration currently awaiting its value, or null.
    /// When the value opens with a `fn` literal, [`Parser::parse_fn`] publishes the
    /// signature onto it before the body parses, so a recursive self-call resolves
    /// its parameter and return logos instead of the unbound-placeholder defaults.
    pending_fn: DyadPtr,
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
    /// order do NOT coincide. Comptime effects that rebind names at parse (a logos
    /// variable's fill) are rejected while this is non-zero: inside such a body
    /// the rebinding would happen once, at the wrong time, and on both runtime
    /// branches. Comptime-taken `if` branches do not count (they run iff parsed).
    runtime_depth: u32,
    /// The constructor-inserted teardown registry (issue #49), one list per open
    /// scope. A binding of an owning value (`a := alloc …`) pushes `defer free a`
    /// onto the top list; [`Parser::parse_sequence`] drains it into the scope's
    /// body so the defer runs at scope exit as ordinary structure. The base entry
    /// (index 0) collects top-level bindings, drained by the file driver.
    pending_defers: Vec<Vec<DyadPtr>>,
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
    /// The lowering table, when the driver attached one: the nested import
    /// pass hands it to its runtime so `f.compile()` at an imported top level
    /// works exactly as at the driver's own top level (one pass, one behavior).
    lower: Option<&'a crate::compile::LowerTable>,
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
        types: CoreTypes,
        scopes: ScopeStack,
    ) -> Self {
        Parser {
            source,
            pos: 0,
            scopes,
            store,
            trie,
            types,
            pending_fn: std::ptr::null_mut(),
            lifted: Vec::new(),
            queued: std::collections::VecDeque::new(),
            discovering: false,
            holes: HashSet::new(),
            frames: Vec::new(),
            runtime_depth: 0,
            pending_defers: vec![Vec::new()],
            dir: PathBuf::from("."),
            imports: Imports::default(),
            lower: None,
        }
    }

    /// Attach the lowering table (`Core::lower`), so an imported file's
    /// top-level `f.compile()` runs under the nested pass exactly as under the
    /// driver. The caller keeps the `Core` alive for the parser's life, as it
    /// already does for the store.
    pub fn with_lower(mut self, lower: &'a crate::compile::LowerTable) -> Self {
        self.lower = Some(lower);
        self
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

    /// The source being parsed — how a constructor reads its own token's span
    /// (the returned `&'a str` outlives the `&self` borrow, so a span slice and
    /// a later `&mut self` service call compose).
    pub(crate) fn source(&self) -> &'a str {
        self.source
    }

    /// The store the constructors allocate into.
    pub(crate) fn store(&mut self) -> &mut Store {
        self.store
    }

    /// The core logos handles (copied out, so a `&mut self` call can follow).
    pub(crate) fn types(&self) -> CoreTypes {
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
            self.store.alloc_bytes(&vec![0u8; width])
        } else {
            let depth = self.frames.len();
            let frame = self.frames.last_mut().unwrap();
            let offset = frame.size;
            frame.size += width;
            crate::dyad::frame_place(depth, offset)
        };
        self.store.alloc_raw(ty_node, place)
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
        if let Some((depth, _)) = crate::dyad::frame_ref((*node).value) {
            if depth != self.frames.len() {
                return Err(ParseError::CapturedLocal);
            }
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
    /// drained (the file driver runs top-level items itself). The file driver
    /// runs their inners LIFO at program exit — the top level's own scope-exit.
    /// Returns them in insertion order; the caller reverses for LIFO.
    pub fn take_pending_defers(&mut self) -> Vec<DyadPtr> {
        match self.pending_defers.first_mut() {
            Some(base) => std::mem::take(base),
            None => Vec::new(),
        }
    }

    /// Recover the scope stack, consuming the parser. The REPL parses each line
    /// with a fresh `Parser` over one persistent store/trie/scope-stack, so
    /// declarations made on earlier lines stay resolvable.
    pub fn into_scopes(self) -> ScopeStack {
        self.scopes
    }

    /// Advance past trivia: ASCII whitespace and `#` line comments (a `#` runs to
    /// the end of its line). Statement-level `#`s never reach this — the sequence
    /// parser builds them into reflectable comment nodes first
    /// ([`Parser::parse_comment`]) — so discarding here covers only
    /// *mid-expression* `#`s, the seed's remaining approximation of the settled
    /// design (DESIGN ›Text literals are plain values; `#` is the one comment
    /// constructor‹); the full constructor form arrives at self-hosting.
    fn skip_trivia(&mut self) {
        // Whitespace only: `#` is an identity the driver lexes into a comment
        // cell (DESIGN ›`#` is the one comment constructor‹), never trivia.
        self.skip_whitespace();
    }

    /// Consume the closing `)` that matches an opening `(`, or fail if the body
    /// ended at something else (or the end of input).
    pub(crate) fn expect_close(&mut self) -> Result<(), ParseError> {
        self.skip_trivia();
        let source = self.source;
        if self.pos >= source.len() {
            return Err(ParseError::UnclosedBracket);
        }
        let start = self.pos;
        let r = self
            .scopes
            .resolve(self.trie, &source[start..])
            .map_err(ParseError::Resolve)?;
        if r.identity == self.types.close_ {
            self.pos = start + r.matched;
            Ok(())
        } else {
            Err(ParseError::UnclosedBracket)
        }
    }

    /// Application — the constructor an instance of `fn`, a record logos, or
    /// (through its own constructor) a numeric logos runs for the bracket to
    /// its right: DESIGN ›`X (…)` is one spelling, and X's constructor decides
    /// what the bracket is‹ — a call, an instance construction, a conversion —
    /// never `(`'s decision, which builds a group and nothing else (#59 step
    /// 2). Without a `(` directly ahead the identity stands as its own value
    /// (`f(i32, 3)` passes the logos; `g := f` names the function).
    pub(crate) fn construct_application(
        &mut self,
        id: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if let Some(Cell::Scope(scope)) = tape.at(1).copied() {
            // SAFETY: `scope` is the bracket's node from the store.
            let args = unsafe { self.args_of(scope) };
            let node = self.build_call(id, &args)?;
            tape.remove(1);
            tape.place(node);
        } else {
            tape.place(id);
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
        if (*scope).ty != self.types.scope || (*scope).value.is_null() {
            return vec![scope];
        }
        let arr = *((*scope).value as *const DyadPtr);
        let defer_ = self.types.defer_;
        crate::identities::array::items(arr)
            .iter()
            .copied()
            .filter(|&e| {
                !crate::identities::numtype::is_comment_type((*e).ty) && (*e).ty != defer_
            })
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
        tape.push(Cell::Token(Token { start, len, identity: id }), start);
        match construct(self, id, &mut tape)? {
            Constructed::Placed => Ok(tape.cell(0).and_then(Cell::as_dyad)),
            Constructed::Decline => Ok(None),
        }
    }

    /// The constructor of `id`, decoded from its constructor-slot leaf — the
    /// parse-time analogue of `run`'s op-slot jump: dispatch flows through the
    /// graph, no table anywhere. `None` for an undefined constructor (a
    /// delimiter token, a data logos).
    fn construct_of(&self, id: DyadPtr) -> Option<ConstructFn> {
        // SAFETY: `id` is a resolved identity; every constructor leaf is
        // minted from a `ConstructFn` at registration (`Core::build`) — one
        // convention, one signature, so the transmute is exact.
        unsafe {
            let leaf = crate::identities::meta::constructor_of(id);
            if leaf.is_null() {
                return None;
            }
            let entry = crate::identities::callable::entry_of(leaf);
            Some(std::mem::transmute::<usize, ConstructFn>(entry))
        }
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
    pub(crate) fn construct_hole(&mut self, tape: &mut ParsingTape) -> Result<Constructed, ParseError> {
        let types = self.types;
        let base = match tape.at(-1).copied() {
            Some(cell @ (Cell::Dyad(_) | Cell::Scope(_)))
            | Some(cell @ Cell::Token(Token { identity: _, .. })) => {
                // A fresh name to the left is not a type; leave it for the
                // boundary's own report.
                if matches!(cell, Cell::Token(t) if t.identity.is_null()) {
                    None
                } else {
                    let d = self.as_operand(cell)?;
                    // SAFETY: `d` is a resolved dyad from the store.
                    if unsafe { crate::identities::is_type_value(&types, d) } {
                        Some(d)
                    } else {
                        None
                    }
                }
            }
            None => None,
        };
        let node = match base {
            None => self.store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut()),
            Some(mut t) => {
                let mut depth = 0usize;
                while matches!(tape.at(-2 - depth as isize), Some(Cell::Token(a)) if a.identity == types.at_) {
                    depth += 1;
                }
                for _ in 0..depth {
                    t = crate::identities::pointer::make_pointer_type(self.store, types.type_, t);
                }
                // SAFETY: `t` is a type node from the store.
                let place = unsafe {
                    if t == types.type_ {
                        // A type variable: the null-valued placeholder — the
                        // undefined type, the null value being the marker no
                        // real type node has — filled once by `name = <type>`.
                        self.store.alloc_raw(types.type_, std::ptr::null_mut())
                    } else if crate::identities::is_numtype_node(&types, t) {
                        let nt = crate::identities::numtype::of_type_node(t);
                        self.alloc_local(t, nt.bytes())
                    } else if crate::identities::numtype::is_pointer_type(t) {
                        self.alloc_local(t, 8)
                    } else {
                        // A record or bool place by declared type waits for the
                        // instance machinery to read it (#47).
                        return Err(ParseError::NonNumericDeclaredType);
                    }
                };
                for _ in 0..(1 + depth) {
                    tape.remove(-1);
                }
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
            Cell::Dyad(_) | Cell::Scope(_) => true,
            // A fresh name resolves at consumption; a resolved token stands as
            // an operand only when nothing would construct it and it is not a
            // bare delimiter (`..`, `->`, `else`, `in`), which no operator
            // takes as an operand.
            Cell::Token(t) => {
                t.identity.is_null()
                    || (self.ctor_of(t.identity).is_none() && !self.is_delimiter(t.identity))
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

    /// Convert an operand cell to its dyad at consumption — the one seam every
    /// reader goes through. A reduced dyad passes; a resolved token yields its
    /// identity (rejecting a capture, as the old scan-time push did); a
    /// fresh-name token re-resolves its span for the precise error, reported
    /// at the token's own start — the same message and position the eager
    /// driver produced at scan.
    pub(crate) fn as_operand(&mut self, cell: Cell) -> Result<DyadPtr, ParseError> {
        match cell {
            Cell::Dyad(d) | Cell::Scope(d) => Ok(d),
            Cell::Token(t) => {
                let id = if t.identity.is_null() {
                    let source = self.source;
                    match self.scopes.resolve(self.trie, &source[t.start..]) {
                        Ok(r) => r.identity,
                        Err(e) => {
                            self.pos = t.start;
                            return Err(ParseError::Resolve(e));
                        }
                    }
                } else {
                    t.identity
                };
                // SAFETY: `id` is a resolved dyad from the store.
                unsafe { self.check_capture(id)? };
                Ok(id)
            }
        }
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
        self.skip_trivia();
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

    /// Whether the next token is a closing `)` (peek, no consume).
    fn at_close(&mut self) -> bool {
        matches!(self.peek_token(), Some((id, _)) if id == self.types.close_)
    }

    /// Read a raw identifier `[A-Za-z_][A-Za-z0-9_]*` at the cursor, advancing past
    /// it, returning its `(start, len)`; `None` if the next non-space byte does not
    /// begin an identifier. Declaration position reads fresh names raw, since they
    /// are not yet in the name index to resolve (the sketch's `declare(name:string)`).
    fn lex_identifier(&mut self) -> Option<(usize, usize)> {
        self.skip_trivia();
        let bytes = self.source.as_bytes();
        let start = self.pos;
        match bytes.get(start) {
            Some(&b) if b.is_ascii_alphabetic() || b == b'_' => {}
            _ => return None,
        }
        let mut end = start + 1;
        while let Some(&b) = bytes.get(end) {
            if b.is_ascii_alphanumeric() || b == b'_' {
                end += 1;
            } else {
                break;
            }
        }
        self.pos = end;
        Some((start, end - start))
    }

    /// Parse a `( field-list )` into a record node. `record_logos` is the identity
    /// that introduced it (`record`, or later `fn`'s parameter list). Fields are
    /// `name := T ?` or a bare `name`, separated by `,`; each becomes a `:`
    /// declaration dyad `{type: field-logos, value: undefined}` whose name is declared
    /// in the record's own scope. The node's value is a [`RECORD_TAG`] record
    /// storing the layout the definition derives — the scope, the `fields`
    /// array node, and the packed `size_bytes` — filled here, where the logos's
    /// layout locks (issue #47; DESIGN ›a logos whose constructor derives the
    /// layout automatically — reading the field declarations in its scope and
    /// filling `fields` and `size_bytes`‹). Fresh field names are read raw
    /// here, which is why the field list needs its own sub-parse rather than
    /// the generic driver.
    ///
    /// [`RECORD_TAG`]: crate::identities::meta::RECORD_TAG
    pub fn parse_record(&mut self, record_logos: DyadPtr) -> Result<DyadPtr, ParseError> {
        self.expect_open()?;
        // The record's own scope: a `scope`-typed node keyed by address for
        // open-scope membership. Field names are declared into it.
        let scope = self.store.alloc_raw(self.types.scope, std::ptr::null_mut());
        self.scopes.push(scope);

        let mut fields = Vec::new();
        loop {
            if self.at_close() {
                break;
            }
            let (start, len) = self.lex_identifier().ok_or(ParseError::ExpectedField)?;
            // `self.source` is `&'a str` (Copy), so this slice is independent of the
            // `&mut self` the reentrant logos-parse and the declaration then need.
            let source = self.source;
            let name = &source[start..start + len];
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
            let field = self.store.alloc_raw(logos, std::ptr::null_mut());
            // The field's NAME is not stored on the record: declaring it here
            // puts an id_context in the shared name index, and resolution is
            // open-scope filtering over that one index (DESIGN ›Name resolution
            // is scope-filtered‹; a per-record names store is recorded as
            // rejected).
            self.scopes.declare(self.trie, name, field).map_err(ParseError::Resolve)?;
            fields.push(field);
            if !self.consume_separator() {
                break;
            }
        }

        self.scopes.pop();
        self.expect_close()?;

        // The stored layout: fields pack in declaration order, a scalar at its
        // logos's width and anything else (a bare or logos-valued name, only
        // meaningful for parameter lists) as the 8-byte container — the same
        // width rule parameters claim frame offsets by.
        let size_bytes: u64 = fields
            .iter()
            .map(|&f| {
                // SAFETY: `f` is the field dyad just built.
                let logos = unsafe { (*f).ty };
                if unsafe {
                    crate::identities::numtype::is_scalar_place_type(logos)
                } {
                    unsafe { crate::identities::numtype::numtype_of_type(logos) }.bytes() as u64
                } else {
                    8
                }
            })
            .sum();
        let fields_arr = crate::identities::array::build(self.store, self.types.array_, &fields);
        let record =
            crate::identities::meta::record_layout(self.store, scope, fields_arr, size_bytes);
        Ok(self.store.alloc_raw(record_logos, record.cast()))
    }

    /// Parse a function literal `fn ( params ) -> ret ( body )` (DESIGN ›A
    /// function's surface‹), given `fn_type` (the resolved `fn` identity). The
    /// parameter list is a `record` (the step-2 field list); the return logos after
    /// `->` is a single logos identity; the body is a `( )` scope parsed with the
    /// parameter scope reopened, so parameters resolve inside it. The node is
    /// `{type: fn, value -> [input, output, body, bcode]}` — the params record, the
    /// return logos, the reflectable body, and the compiled `bcode` (null until
    /// [`crate::compile::compile_fn`] installs it).
    ///
    /// A function's value is what its body evaluates to; an explicit `return` is
    /// *optional* and, for v1's single-expression body, `return X` and `X` yield the
    /// same value in tail position (early-return semantics, `return` unwinding out
    /// of control flow, arrive with `if`/`while`).
    ///
    /// `declared` (null when the literal does not open a declaration's value) is
    /// the declaration's placeholder: the signature publishes onto it — body and
    /// bcode still null — before the body parses, so a recursive self-call inside
    /// the body reads real parameter and return logos.
    pub fn parse_fn(&mut self, fn_type: DyadPtr, declared: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The parameter list is a record; parse_record opens and closes its scope.
        let input = self.parse_record(self.types.type_)?;
        self.expect_arrow()?;
        // The return logos: the cells up to the body bracket, constructed to
        // one (`i32`, `@i32`, later `array i32`).
        let output = {
            let items = self.drive_until_open(RightSide::ReturnType)?;
            self.one_of(items).map_err(|e| match e {
                ParseError::Empty => ParseError::ExpectedReturnType,
                e => e,
            })?
        };

        // Open this function's frame and give the parameters its first per-call
        // byte offsets — a call frame is an instance of its function, so a
        // parameter resolves to a frame slot exactly as a local does (DESIGN
        // ›Resolution is one rule‹), and the caller writes the argument values
        // into those slots (›Operands travel on the stack‹). A scalar-typed
        // parameter stores at its logos's width, like a local of that logos;
        // anything else — a bare `name`, a logos-valued parameter — rides the
        // full 8-byte i64 bit-container the call convention already passes.
        // The body's local declarations then claim the offsets after these; a
        // nested `fn` literal pushes its own frame, so its state never lands
        // in this one.
        self.frames.push(OpenFn { size: 0 });
        let depth = self.frames.len();
        // SAFETY: `input` is the record just built; its record stores the
        // fields array, and each parameter's value slot is still the null
        // parse_record left there.
        unsafe {
            let fields = crate::identities::meta::record_fields_of(input);
            for &param in crate::identities::array::items(fields) {
                let logos = (*param).ty;
                let width = if crate::identities::numtype::is_scalar_place_type(logos) {
                    crate::identities::numtype::numtype_of_type(logos).bytes()
                } else {
                    8
                };
                let frame = self.frames.last_mut().expect("parse_fn just pushed a frame");
                let offset = frame.size;
                frame.size += width;
                (*param).value = crate::dyad::frame_place(depth, offset);
            }
        }

        if !declared.is_null() {
            let early = self.store.alloc_operands(&[
                input,
                output,
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
        // but a *call* hides the body behind the return logos, and a plain `@T`
        // carries no destructor — so the caller could not know it owes a `free`
        // and would leak. Fail closed until a return logos can declare that it
        // hands ownership over, which is the ownership-gate work (issue #53).
        // SAFETY: `body` is the reduced dyad just parsed.
        if unsafe { crate::identities::drop_model::is_owning_value(&self.types, body) } {
            return Err(ParseError::OwnershipAcrossReturn);
        }
        let frame_size = self.frames.pop().expect("parse_fn pushed a frame").size;

        // A comptime-rational tail expression commits to the declared return logos here
        // (the typed slot), so `fn () -> i64 ( 2000000000 + 2000000000 )` returns i64
        // rather than molding to the i32 default.
        // SAFETY: `body`/`output` are valid dyads just built.
        let body = unsafe { crate::identities::commit_fn_body(self.store, &self.types, body, output)? };

        // `bcode` starts null; `compile_fn` installs the exec@ into that slot.
        // FN_FRAME holds the activation-record byte size — parameters first,
        // locals after, a `u64` leaf both tiers read on entry — or null when
        // the function declares no parameters and no locals.
        let frame = if frame_size == 0 {
            std::ptr::null_mut()
        } else {
            let bytes = self.store.alloc_bytes(&(frame_size as u64).to_ne_bytes());
            let u64_ty = self.types.numtypes[crate::identities::NumType::U64 as usize];
            self.store.alloc_raw(u64_ty, bytes)
        };
        let value = self.store.alloc_operands(&[input, output, body, std::ptr::null_mut(), frame]);
        Ok(self.store.alloc_raw(fn_type, value))
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
        if !unsafe { is_bool_result(&types, cond) } {
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
        if let Some(truth) = unsafe { bool_literal_value(&types, cond) } {
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

        let value = self.store.alloc_operands(&[cond, then, els, self.types.ops.if_]);
        Ok(self.store.alloc_raw(if_type, value))
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
            let value = self
                .store
                .alloc_operands(&[cond, then, std::ptr::null_mut(), self.types.ops.if_]);
            return Ok(self.store.alloc_raw(if_type, value));
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
            self.store.alloc_operands(&[cond, cond, std::ptr::null_mut(), self.types.ops.if_]);
        Ok(self.store.alloc_raw(if_type, value))
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
                    self.pos =
                        skip_text(bytes, self.pos).ok_or(ParseError::UnclosedBracket)?;
                }
                b'#' => {
                    // `#` takes a following «…» string or the rest of the line,
                    // exactly as the comment constructor reads it.
                    self.pos += 1;
                    while self.pos < bytes.len() && matches!(bytes[self.pos], b' ' | b'\t') {
                        self.pos += 1;
                    }
                    if bytes.get(self.pos) == Some(&0xC2)
                        && bytes.get(self.pos + 1) == Some(&0xAB)
                    {
                        self.pos =
                            skip_text(bytes, self.pos).ok_or(ParseError::UnclosedBracket)?;
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
        if !is_bool_result(&types, operand) {
            return Err(ParseError::NonBoolOperands);
        }
        // A bool-literal operand folds now (pure, nothing lost), like the
        // `==`/`and`/`or` folds — what keeps a comptime chain comptime.
        if let Some(v) = bool_literal_value(&types, operand) {
            return Ok(crate::identities::bool_mod::literal_node(
                self.store,
                self.types.bool_,
                !v,
            ));
        }
        let value = self.store.alloc_operands(&[operand, self.types.ops.not_]);
        Ok(self.store.alloc_raw(not_id, value))
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
        if !unsafe { is_bool_result(&types, cond) } {
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
        if unsafe { contains_return(&types, body) } {
            return Err(ParseError::EarlyReturn);
        }
        let value = self.store.alloc_operands(&[cond, body, self.types.ops.while_]);
        Ok(self.store.alloc_raw(while_id, value))
    }

    /// Parse a counted loop `for i in a..b ( body )` / `for i in a..b..d ( body )`
    /// (given the resolved `for` identity). The range is end-exclusive and its
    /// parts are *primaries* ([`Parser::parse_range_operand`]) — a full expression
    /// parse would consume the body's `(` as a call on the endpoint. The loop
    /// variable is a fresh block-local of the range's resolved numeric logos; a
    /// literal step must be positive ([`ParseError::BadStep`]); the loop is a
    /// statement yielding unit, and a `return` in the body is rejected
    /// ([`ParseError::EarlyReturn`], no unwinding to exit with).
    pub fn parse_for(&mut self, for_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        let (nstart, nlen) = self.lex_identifier().ok_or(ParseError::ExpectedLoopVar)?;
        let source = self.source;
        let name = &source[nstart..nstart + nlen];
        if !self.consume_token(self.types.in_) {
            return Err(ParseError::ExpectedIn);
        }
        // The range: the cells up to the body bracket — `start .. end` or
        // `start .. end .. step` — constructed by precedence, the `..` cells
        // inert delimiters read by position.
        let parts = self.drive_until_open(RightSide::Condition)?;
        let dotdot = self.types.dotdot_;
        let (start, end, step) = match parts.as_slice() {
            [(s, _), (d, _), (e, _)] if *d == dotdot => (*s, *e, None),
            [(s, _), (d, _), (e, _), (d2, _), (st, _)] if *d == dotdot && *d2 == dotdot => {
                (*s, *e, Some(*st))
            }
            _ => return Err(ParseError::ExpectedRange),
        };

        // Resolve the loop logos across the range parts (concrete logos must
        // match, literals commit, all-literals default to i32).
        let types = self.types;
        // SAFETY: `step` is the reduced dyad just parsed.
        let step_was_literal =
            step.is_some_and(|s| unsafe { (*s).ty } == types.rational);
        let mut parts = vec![start, end];
        if let Some(s) = step {
            parts.push(s);
        }
        // SAFETY: `parts` are reduced dyads just parsed.
        let logos = unsafe { crate::identities::resolve_loop_parts(self.store, &types, &mut parts)? };
        let (start, end) = (parts[0], parts[1]);
        let step = parts.get(2).copied().unwrap_or(std::ptr::null_mut());
        if step_was_literal {
            use crate::identities::numtype;
            // SAFETY: `step` is the committed literal just built; `logos` a numtype node.
            let (bits, nt) = unsafe {
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
        let scope = self.store.alloc_raw(types.scope, std::ptr::null_mut());
        // A repeated body: a name declared outside it may not be moved or
        // dropped inside; the loop variable, declared in the scope pushed next,
        // is inside.
        self.scopes.push_barrier();
        self.scopes.push(scope);
        self.scopes.declare(self.trie, name, var).map_err(ParseError::Resolve)?;
        // Parse-time rebinding is off inside a repeated body.
        self.runtime_depth += 1;
        self.expect_open()?;
        let body = self.parse_sequence()?;
        self.expect_close()?;
        self.runtime_depth -= 1;
        self.scopes.pop();
        self.scopes.pop_barrier();
        // SAFETY: `body` is the reduced dyad just parsed.
        if unsafe { contains_return(&types, body) } {
            return Err(ParseError::EarlyReturn);
        }

        let value = self.store.alloc_operands(&[var, start, end, step, body, self.types.ops.for_]);
        Ok(self.store.alloc_raw(for_id, value))
    }

    /// Resolve a field access `lhs.name` to a *place*: an ordinary numeric node
    /// over the instance's storage at the field's byte offset (DESIGN ›Resolution
    /// is one rule‹ — the declaration found decides, and a field declaration is
    /// the offset inside the value area). The field name resolves in the record
    /// logos's own scope, alone (never against the enclosing scopes). The `.` has
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
        unit_call: bool,
    ) -> Result<(DyadPtr, usize), ParseError> {
        // `.` does exactly one job (ruled August 2026): reading fields the
        // logos defines, which are always about the value. A value's logos is
        // never one of its own fields — the retired universal `.logos`
        // metaproperty did a second job here — so reading a logos takes the
        // dyad view, `(dyad x).ty`, where the logos IS in the value.
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
            if (*lhs).ty == self.types.dyad_ {
                return self.view_member(lhs, name).map(|n| (n, 0));
            }
            if crate::identities::is_type_value(&self.types, lhs) {
                let n = self.logos_member(lhs, name, index)?;
                return Ok((n, usize::from(name == "roles")));
            }
            if name == "type" {
                return Err(ParseError::TypeNeedsView);
            }
            // An operator node's slots are the fields its own logos defines
            // (#52, corrected August 2026): `.operands` is the collection
            // that logos defines, and `[i]` fetches an element from it —
            // `(x + x).operands[0]` — no view involved, exactly as `p.x`
            // reads a record field. A null slot (an absent optional) is the
            // ruled checked error until `?`.
            if name == "operands"
                && !(*lhs).ty.is_null()
                && matches!(
                    crate::identities::meta::kind_of((*lhs).ty),
                    Some(
                        crate::identities::meta::TUPLE_TAG
                            | crate::identities::meta::LIST_TAG
                    )
                )
            {
                let i = index.ok_or(ParseError::ExpectedIndexBracket)?;
                if i >= crate::identities::meta::arity_of((*lhs).ty)
                    || (*lhs).value.is_null()
                {
                    return Err(ParseError::BadReflectRead);
                }
                let ops = (*lhs).value as *const DyadPtr;
                let operand = *ops.add(i);
                if operand.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                return Ok((operand, 1));
            }
            // `.compile` on an fn-typed value is the fn logos's shared member
            // (DESIGN ›Execution is function application‹: "The `fn` logos
            // carries two shared functions: `compile` … and `run`"; `run` is
            // calling). `f.compile()` builds a compile statement whose run
            // lowers `f`'s body and installs its `bcode`, so the next call
            // jumps to machine code. The name-compare here is the seed's
            // stand-in for shared-member resolution through the logos's scope
            // (one mechanism at self-hosting); reserved only on fn-typed
            // values, so a record field named `compile` still resolves. The
            // `()` is mandatory — compile is a function, applied like any
            // other, taking no arguments (DESIGN ›Operands travel on the
            // stack‹). The callable leaf is minted NOW, entry zero, because
            // minting needs the store the parser holds; the run patches the
            // finalized entry in.
            if &self.source[nstart..nstart + nlen] == "compile"
                && (*lhs).ty == self.types.fn_type
            {
                if !unit_call {
                    return Err(ParseError::ExpectedOpen);
                }
                let code = crate::identities::callable::mint(
                    self.store,
                    self.types.callable_,
                    0,
                    self.types.conv_container,
                );
                let value =
                    self.store.alloc_operands(&[lhs, code, self.types.ops.compile_]);
                return Ok((self.store.alloc_raw(self.types.compile_, value), 1));
            }
        }
        // Through a record pointer, `p@.x` folds the field offset into the deref
        // (the address is runtime; the offset and the field's logos are not).
        if (*lhs).ty == self.types.deref_ {
            let (ptr_expr, pointee, base_off) =
                crate::identities::pointer::deref_parts(lhs);
            if pointee.is_null() || !crate::identities::meta::is_record_type(pointee) {
                return Err(ParseError::UnsupportedOperands);
            }
            let (field, offset) = self.resolve_field(pointee, nstart, nlen)?;
            let types = self.types;
            return Ok((
                crate::identities::pointer::build_deref(
                    self.store,
                    &types,
                    ptr_expr,
                    (*field).ty,
                    base_off as usize + offset,
                ),
                0,
            ));
        }
        // The direct case: an instance of a record logos, with storage — the
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
        Ok((self.store.alloc_raw((*field).ty, addr), 0))
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
        // quote‹: `i32.precedence`), so a callable or a logos name to the
        // left is the identity itself, not a call in waiting.
        let lhs = match tape.at(-1).copied() {
            Some(cell) => self.as_operand(cell)?,
            None => return Err(ParseError::MissingOperand),
        };
        // The member is the cell to the right, read by its spelling at the
        // offset it was lexed at — a keyword there (`.type`) was constructed
        // at discovery and stands as a dyad, its spelling still in the source.
        let Some(&_) = tape.at(1) else {
            return Err(ParseError::ExpectedField);
        };
        let mstart = tape.start_of(tape.cursor() + 1);
        let save = self.pos;
        self.pos = mstart;
        let member = self.lex_identifier();
        self.pos = save;
        let Some((nstart, nlen)) = member else {
            return Err(ParseError::ExpectedField);
        };
        let m = Token { start: nstart, len: nlen, identity: std::ptr::null_mut() };
        let index = self.index_at(tape, 2);
        // SAFETY: a scope cell is a node from the store.
        let unit_call =
            matches!(tape.at(2), Some(Cell::Scope(s)) if unsafe { self.is_empty_scope(*s) });
        // SAFETY: `lhs` is a reduced dyad off the tape.
        let (node, consumed) =
            unsafe { self.field_access(lhs, m.start, m.len, index, unit_call)? };
        for _ in 0..(1 + consumed) {
            tape.remove(1);
        }
        tape.remove(-1);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// The comptime index a `[…]` cell carries, if the cell at `offset` is one.
    fn index_at(&self, tape: &ParsingTape, offset: isize) -> Option<usize> {
        let d = tape.at(offset)?.as_dyad()?;
        // SAFETY: a dyad cell is a node from the store; an index node's value
        // is its literal operand first.
        unsafe {
            if (*d).ty != self.types.index_ {
                return None;
            }
            let lit = *((*d).value as *const DyadPtr);
            let i = crate::identities::rational::mold(lit)?;
            if i < 0 {
                None
            } else {
                Some(i as usize)
            }
        }
    }

    /// Whether `node` is a scope with nothing in it — the `()` cell.
    ///
    /// # Safety
    /// `node` must be a dyad from the store.
    unsafe fn is_empty_scope(&self, node: DyadPtr) -> bool {
        if (*node).ty != self.types.scope || (*node).value.is_null() {
            return false;
        }
        let arr = *((*node).value as *const DyadPtr);
        crate::identities::array::items(arr).is_empty()
    }

    /// `[`'s constructor: an index cell — the interior read once, generically
    /// (DESIGN ›The constructor is a field‹: "`[…]` constructs itself … into a
    /// passive node carrying the index"), a comptime literal in the seed —
    /// closed by its `]`, which it consumes itself.
    pub(crate) fn construct_index(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        self.skip_trivia();
        let source = self.source;
        let r = self
            .scopes
            .resolve(self.trie, &source[self.pos..])
            .map_err(ParseError::Resolve)?;
        if r.identity != self.types.rational {
            return Err(ParseError::BadReflectRead);
        }
        let start = self.pos;
        self.pos += r.matched;
        let lit =
            self.construct_leaf(r.identity, start, r.matched)?.ok_or(ParseError::BadLiteral)?;
        self.skip_trivia();
        if self.source.as_bytes().get(self.pos) != Some(&b']') {
            return Err(ParseError::ExpectedIndexBracket);
        }
        self.pos += 1;
        let value = self.store.alloc_operands(&[lit, std::ptr::null_mut()]);
        let node = self.store.alloc_raw(self.types.index_, value);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Resolve the field name spelled at `nstart..nstart + nlen` against `record_logos`'s own scope
    /// alone (its value[0] — an enclosing binding of the same spelling can never
    /// shadow or double a field), returning the field node and its byte offset.
    ///
    /// # Safety
    /// `record_logos` must be a record logos node from the store.
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
        let field =
            field_scope.resolve(self.trie, name).map_err(ParseError::Resolve)?.identity;
        let (fields, _) = crate::identities::instance::layout(record_logos)?;
        let (_, _, offset) = fields
            .iter()
            .copied()
            .find(|&(f, _, _)| f == field)
            .ok_or(ParseError::ExpectedField)?;
        Ok((field, offset))
    }

    /// Whether `callee` is a function whose declared return logos is the `logos` root —
    /// it yields a logos, resolved at comptime (roadmap #30).
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

    /// Comptime-evaluate a logos-returning call to the concrete logos it produces,
    /// substituting that logos node for the call. The call runs under a fresh
    /// interpreter — which works off raw handles and never touches the store — so
    /// interpretation doubles as parse-time evaluation (DESIGN ›Build and run are one
    /// self-directing pass‹); the result bits are the produced logos node's address.
    /// A run failure (e.g. a runtime-only argument) or a non-logos result is reported
    /// as [`ParseError::NonComptimeTypeCall`].
    ///
    /// # Safety
    /// `call` must be a reduced call node from the store.
    unsafe fn eval_type_call(&mut self, call: DyadPtr) -> Result<DyadPtr, ParseError> {
        let mut rt = crate::run::Runtime::new(self.types.fn_type, self.types.rational);
        let bits = rt.run(call).map_err(|_| ParseError::NonComptimeTypeCall)?;
        let node = bits as usize as DyadPtr;
        if crate::identities::is_type_value(&self.types, node) {
            Ok(node)
        } else {
            Err(ParseError::NonComptimeTypeCall)
        }
    }

    /// Build a postfix dereference `lhs@`: the lhs's static logos must be a
    /// pointer logos — a pointer variable or `&x` literal (its `logos`), a pointer
    /// field place, or another deref whose pointee is a pointer (`p@@`).
    ///
    /// # Safety
    /// `lhs` must be a reduced dyad from the store.
    pub(crate) unsafe fn build_deref(&mut self, lhs: DyadPtr) -> Result<DyadPtr, ParseError> {
        let ptr_ty = if (*lhs).ty == self.types.deref_ {
            crate::identities::pointer::deref_parts(lhs).1
        } else {
            (*lhs).ty
        };
        if ptr_ty.is_null() || !crate::identities::numtype::is_pointer_type(ptr_ty) {
            return Err(ParseError::UnsupportedOperands);
        }
        let pointee = crate::identities::numtype::pointee_of(ptr_ty);
        let types = self.types;
        Ok(crate::identities::pointer::build_deref(self.store, &types, lhs, pointee, 0))
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
        if let Cell::Token(t) = cell {
            // Keywords, operators, literals: not places.
            if !t.identity.is_null() && self.ctor_of(t.identity).is_some() {
                return Err(ParseError::BadAddressOf);
            }
        }
        let node = self.as_operand(cell)?;
        // SAFETY: `node` is a resolved dyad from the store.
        let addr = unsafe {
            let logos = (*node).ty;
            let is_place = crate::identities::is_numtype_node(&self.types, logos)
                || crate::identities::numtype::is_pointer_type(logos)
                || crate::identities::meta::is_record_type(logos);
            if !is_place || (*node).value.is_null() {
                // Comptime bindings have no storage.
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
            crate::identities::pointer::build_addr(self.store, &self.types, node)
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
        let (node, ended) = match cell {
            Cell::Token(t) => {
                let source = self.source;
                let r = match self.scopes.resolve(self.trie, &source[t.start..]) {
                    Ok(r) => r,
                    Err(e) => {
                        self.pos = t.start;
                        return Err(ParseError::Resolve(e));
                    }
                };
                if self.ctor_of(r.identity).is_some() {
                    return Err(ParseError::MissingOperand);
                }
                let ended = if ends_name {
                    if self.scopes.crosses_barrier(r.scope) {
                        self.pos = t.start;
                        return Err(ParseError::OwnOfOuterName);
                    }
                    let name = source[t.start..t.start + r.matched].to_string();
                    Some(Ended { name, scope: r.scope, identity: r.identity })
                } else {
                    None
                };
                (r.identity, ended)
            }
            Cell::Dyad(d) | Cell::Scope(d) => (d, None),
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

    /// The `own`/`drop` node `node` has emptied `ended`'s place: its name is dead
    /// from here on (DESIGN ›Memory and concurrency‹, *`own` and `drop` are
    /// static*). See [`ScopeStack::mark_dead`].
    pub(crate) fn mark_dead(&mut self, ended: Ended, node: DyadPtr) {
        self.scopes.mark_dead(self.trie, &ended.name, ended.scope, ended.identity, node);
    }

    /// Consume an `else` if the next token is one, reporting whether it was.
    fn consume_else(&mut self) -> bool {
        self.consume_token(self.types.else_)
    }

    /// Consume the `->` that separates a fn's parameter list from its return logos.
    fn expect_arrow(&mut self) -> Result<(), ParseError> {
        if self.consume_token(self.types.arrow_) {
            Ok(())
        } else {
            Err(ParseError::ExpectedArrow)
        }
    }

    /// The prefix `@` over cells: every further `@` cell to the right, then
    /// the base logos cell — a resolved logos name (`@i32`, `@@point`) — built
    /// into the pointer logos; the consumed cells are spliced out.
    pub(crate) fn construct_pointer_type(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let mut depth = 1usize;
        while matches!(tape.at(1), Some(Cell::Token(t)) if t.identity == self.types.at_) {
            tape.remove(1);
            depth += 1;
        }
        let Some(&cell) = tape.at(1) else {
            return Err(ParseError::UnsupportedOperands);
        };
        let base = self.as_operand(cell)?;
        // SAFETY: `base` is a resolved dyad from the store.
        let is_type = crate::identities::is_numtype_node(&self.types, base)
            || unsafe { crate::identities::meta::is_record_type(base) };
        if !is_type {
            return Err(ParseError::UnsupportedOperands);
        }
        tape.remove(1);
        let mut logos = base;
        for _ in 0..depth {
            logos =
                crate::identities::pointer::make_pointer_type(self.store, self.types.type_, logos);
        }
        tape.place(logos);
        Ok(Constructed::Placed)
    }

    /// Build a call `callee ( args )` over the arguments its bracket cell
    /// held — the callee's own constructor's work (DESIGN ›`X (…)` is one
    /// spelling, and X's constructor decides what the bracket is‹). A numeric
    /// logos callee is a conversion (`i32(a)`), a record logos constructs an
    /// instance, a logos-returning callee resolves NOW at comptime; any other
    /// callee is an ordinary call.
    pub(crate) fn build_call(
        &mut self,
        callee: DyadPtr,
        args: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        let args = args.to_vec();
        // An owning value handed straight to a call has no name to hang its
        // `defer free` on, so it would leak (issue #49; DESIGN's open
        // temporary-attachment point). Fail-closed until ownership-gated
        // parameters land (issue #53) and the callee can declare that it takes
        // the value. SAFETY: `args` are reduced dyads just parsed.
        for &arg in &args {
            if unsafe { crate::identities::drop_model::is_owning_value(&self.types, arg) } {
                return Err(ParseError::UnboundOwningValue);
            }
        }
        if crate::identities::is_numtype_node(&self.types, callee) {
            // SAFETY: `callee` is a numtype node; `args` are reduced dyads.
            unsafe { crate::identities::build_cast(self.store, &self.types, callee, &args) }
        } else if unsafe { crate::identities::meta::is_record_type(callee) } {
            // A record logos applied to its field values constructs an
            // instance — the constructor doctrine, like `i32(a)`.
            let types = self.types;
            // SAFETY: `callee` is a record logos node; `args` are reduced dyads
            // from the store.
            unsafe {
                // The instance is a per-call local (a frame slot inside a
                // function), sized from the record layout, so a recursive call
                // fills its own copy.
                let (_, size) = crate::identities::instance::layout(callee)?;
                let instance = self.alloc_local(callee, size.max(1));
                crate::identities::instance::build_ctor(
                    self.store,
                    &types,
                    types.construct_,
                    callee,
                    instance,
                    &args,
                )
            }
        } else {
            // Each uncommitted literal argument commits to its parameter's
            // declared logos (the typed slot); an unbound callee has no
            // signature yet and commits nothing.
            let types = self.types;
            let mut args = args;
            // SAFETY: `callee` and `args` are reduced dyads from the store.
            unsafe {
                crate::identities::commit_call_args(self.store, &types, callee, &mut args)?;
            }
            let call = build_call(self.store, callee, &args);
            // A call whose callee returns a logos is resolved NOW, at comptime:
            // run it and substitute the concrete logos it produces (roadmap
            // #30), so the result flows as an ordinary logos value through
            // `==`, `:=`, `.logos`, and display. SAFETY: `callee`/`call` are
            // reduced dyads.
            if unsafe { self.returns_type(callee) } {
                unsafe { self.eval_type_call(call) }
            } else {
                Ok(call)
            }
        }
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
        let scope = self.store.alloc_raw(self.types.scope, std::ptr::null_mut());
        self.scopes.push(scope);
        self.pending_defers.push(Vec::new());
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
            self.scopes.settle_item(self.trie, scope, item);
            // A binding of an owning value inserts `defer free <place>` into this
            // scope's pending list (issue #49); drain it right after the statement
            // so the defer sits at its source position — the right LIFO rank
            // among other statements' defers, and `scope::run` runs them all at
            // scope exit. Nested blocks drained their own before returning.
            let depth = self.pending_defers.len() - 1;
            if !self.pending_defers[depth].is_empty() {
                let drained: Vec<DyadPtr> = self.pending_defers[depth].drain(..).collect();
                for &d in &drained {
                    // SAFETY: `d` is a `defer free <place>` node the binding site
                    // just built.
                    owned_here.push(unsafe {
                        crate::identities::drop_model::teardown_place_of(d)
                    });
                }
                exprs.extend(drained);
            }
        }
        self.scopes.pop();
        self.pending_defers.pop();
        // Prose is invisible to value flow, and so is a `defer` (it runs at exit,
        // never as the tail): the expression count and the tail below are taken
        // over the non-comment, non-defer expressions.
        // SAFETY: `exprs` are reduced dyads just parsed/built.
        let defer_ = self.types.defer_;
        let is_value = |e: DyadPtr| unsafe {
            !crate::identities::numtype::is_comment_type((*e).ty) && (*e).ty != defer_
        };
        let values = exprs.iter().filter(|&&e| is_value(e)).count();
        match (values, exprs.len()) {
            // An empty `( )`, or a bracket holding only prose: the scope node
            // with nothing to run, yielding unit — `f()`'s argument cell, a
            // `fn () -> …` with no parameters read by its own parser.
            (0, _) => {
                let arr =
                    crate::identities::array::build(self.store, self.types.array_, &exprs);
                let value = self.store.alloc_operands(&[arr, self.types.ops.scope_]);
                // SAFETY: `scope` was just allocated and is unaliased.
                unsafe {
                    (*scope).value = value;
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
                    if i != tail && unsafe { contains_return(&types, e) } {
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
                if owned_here.contains(&tail_value) {
                    return Err(ParseError::OwningEscape);
                }
                // A scope IS an array: the expression list lives behind one
                // indirection (its own array node), never inline in the scope's
                // value, which is the `[exprs, op]` pair.
                let arr =
                    crate::identities::array::build(self.store, self.types.array_, &exprs);
                let value = self.store.alloc_operands(&[arr, self.types.ops.scope_]);
                // SAFETY: `scope` was just allocated and is unaliased.
                unsafe {
                    (*scope).value = value;
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
    /// [`Parser::parse_sequence`] (which collects a whole block) and the file
    /// driver (which runs each top-level item as it is parsed — build and run
    /// are one pass, so parse-time evaluation sees every earlier item's effect).
    pub fn parse_next(&mut self) -> Option<Result<DyadPtr, ParseError>> {
        loop {
            // What the last segment yielded — its expression and the prose
            // lifted out of it, in source order — goes out first.
            if let Some(item) = self.queued.pop_front() {
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
            let mut ordered: Vec<(usize, DyadPtr)> = self.lifted.drain(..).collect();
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
            let r = self
                .scopes
                .resolve(self.trie, &source[self.pos..])
                .map_err(ParseError::Resolve)?;
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
            crate::identities::string::build_text(self.store, self.types.string_, text.as_bytes())
        };
        Ok(self.store.alloc_raw(self.types.comment_, text_node.cast()))
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
        // (redeclaring a live one is the no-shadowing error below); anything
        // but a token there (a value, a bracket) declines.
        let Some(tok) = tape.at(-1).and_then(Cell::as_token).copied() else {
            return Ok(Constructed::Decline);
        };
        // `source` is `&'a str` (Copy), independent of the `&mut self` the
        // declaration and value parse then need (as in `parse_record`).
        let source = self.source;
        let name = &source[tok.start..tok.start + tok.len];
        // The placeholder is `fn`-typed so a recursive self-call sees a
        // function-typed callee while the value is still parsing; the
        // fixpoint below overwrites it with the value's real logos.
        let placeholder = self.store.alloc_raw(self.types.fn_type, std::ptr::null_mut());
        if let Err(e) = self.scopes.declare(self.trie, name, placeholder) {
            // The stuck point is the name itself (it is what shadows).
            self.pos = tok.start;
            return Err(ParseError::Resolve(e));
        }
        // If the value opens with a `fn` literal, parse_fn publishes its
        // signature onto the placeholder before the body parses.
        self.pending_fn = placeholder;
        let value = self.parse_expression()?;
        self.pending_fn = std::ptr::null_mut();
        // Fixpoint: make the placeholder *be* the value, so references to
        // `name` captured while parsing the value resolve to it. A
        // construction binds the name to the *instance* (the storage)
        // and keeps the construct statement as the initializer: the name
        // is the place, the statement fills it each run. A *logos* value
        // (`x := i32`, `p := record(…)`) rebinds the name to the logos
        // node itself instead — the name becomes another spelling of
        // that logos, so the pointer-identity checks (`is_numtype_node`,
        // cross-logos mismatch, record-logos equality) see the original.
        // SAFETY: `placeholder`/`value` are valid dyads just built.
        let declared = unsafe {
            if self.holes.remove(&value) {
                // `x := i32 ?`: the place `?` built, with the declared type
                // and no value, is what the name binds to — reads are loads,
                // `=` reassigns, nothing initializes it.
                self.scopes.rebind(self.trie, name, value);
                value
            } else if (*value).ty == self.types.construct_ {
                let ops = (*value).value as *mut DyadPtr;
                let instance = *ops;
                (*placeholder).ty = (*instance).ty;
                (*placeholder).value = (*instance).value;
                *ops = placeholder;
                value
            } else if (*value).ty == self.types.type_ {
                self.scopes.rebind(self.trie, name, value);
                value
            } else if crate::identities::drop_model::is_owning_value(&self.types, value) {
                // An owning value (`alloc …`, `own a`, or a block yielding one)
                // lands in a place here — the one site that knows the name it
                // binds — so this is where the constructor-inserted teardown
                // attaches (issue #49, DESIGN ›Explicit heap‹: attachment at the
                // binding site). Mint an *owning* `@pointee` place (its logos
                // carries the destructor, so `drop`/`own` on it are legal),
                // snapshot the value into it like any pointer, then insert
                // `defer free <place>` into this scope. Ownership landing in a
                // fresh place is what re-arms teardown after an `own` move: the
                // moved-from place no-ops, the new place owes the free.
                let pointee = crate::identities::drop_model::owning_pointee_of(&self.types, value)
                    .expect("is_owning_value implies a pointee");
                let owning_ty = crate::identities::pointer::make_owning_pointer_type(
                    self.store,
                    self.types.type_,
                    pointee,
                    self.types.ops.teardown_,
                );
                // A pointer is 8 bytes (U64-wide), whatever it points at.
                let place = self.alloc_local(owning_ty, 8);
                let init =
                    crate::identities::build_scalar_init(self.store, &self.types, place, value)?;
                self.scopes.rebind(self.trie, name, place);
                // `place` was just minted with the owning pointer logos (its
                // destructor set), so the owning check passes; keeping it on
                // guards against a future caller inserting a free over a borrow.
                let free_node = crate::identities::drop_model::build_teardown(
                    self.store,
                    &self.types,
                    self.types.free_,
                    place,
                    true,
                )?;
                let defer_node =
                    crate::identities::drop_model::build_defer(self.store, &self.types, free_node);
                self.pending_defers
                    .last_mut()
                    .expect("a scope's defer list is open")
                    .push(defer_node);
                init
            } else if (*value).ty != self.types.rational
                && matches!(
                    crate::identities::numtype_of(&self.types, value),
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
                    crate::identities::scalar_binding_type(self.store, &self.types, value);
                let place = self.alloc_local(ty_node, width);
                let init =
                    crate::identities::build_scalar_init(self.store, &self.types, place, value)?;
                self.scopes.rebind(self.trie, name, place);
                init
            } else {
                (*placeholder).ty = (*value).ty;
                (*placeholder).value = (*value).value;
                placeholder
            }
        };
        // The declaration is graph structure, not parse vapor: the
        // expression is a declare node carrying the spelling (the
        // nominal identity's human half), the binding, and its native.
        let name_node =
            crate::identities::string::build_text(self.store, self.types.string_, name.as_bytes());
        let node = crate::identities::declare::build(
            self.store,
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
        // like a logos variable's fill.
        if self.runtime_depth != 0 {
            return Err(ParseError::ImportInRuntimeBody);
        }
        self.skip_whitespace();
        let source = self.source;
        let start = self.pos;
        let path_text: String = if source[self.pos..].starts_with('«') {
            let r = self
                .scopes
                .resolve(self.trie, &source[self.pos..])
                .map_err(ParseError::Resolve)?;
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
            self.store,
            types.string_,
            path_text.as_bytes(),
        );
        let value = self.store.alloc_operands(&[path_node, tail, types.ops.import_]);
        let node = self.store.alloc_raw(types.import_, value);
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
        let section = self.store.alloc_raw(self.types.scope, std::ptr::null_mut());
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
        let saved_pending_fn = std::mem::replace(&mut self.pending_fn, std::ptr::null_mut());
        let saved_runtime_depth = std::mem::replace(&mut self.runtime_depth, 0);

        let inner = self.run_imported();
        let inner_pos = self.pos;

        self.source = saved_source;
        self.pos = saved_pos;
        self.scopes = saved_scopes;
        self.dir = saved_dir;
        self.frames = saved_frames;
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
                self.imports.entries.insert(canon, ImportState::Loaded { pubs: pubs.clone(), tail });
                self.publish(&pubs)?;
                Ok(tail)
            }
        }
    }

    /// The nested top-to-bottom pass over an imported file: parse each
    /// statement and run it — the one pass, under a fresh interpreter working
    /// off raw handles as [`Parser::eval_type_call`] does — collecting the
    /// `pub` declarations' (name, identity) pairs and the file's tail node.
    /// (`f.compile()` at an imported top level is not wired yet: the parser
    /// carries no lowering table; compiled members keep working inside the
    /// importing program.) On failure the message is returned with
    /// [`Parser::offset`] left at the stuck point in the imported source.
    fn run_imported(&mut self) -> Result<(Vec<(String, DyadPtr)>, DyadPtr), String> {
        let mut pubs = Vec::new();
        let mut tail = std::ptr::null_mut();
        let mut rt = crate::run::Runtime::new(self.types.fn_type, self.types.rational)
            .with_defer_type(self.types.defer_);
        if let Some(lower) = self.lower {
            rt = rt.with_compiler(lower, self.types);
        }
        while let Some(item) = self.parse_next() {
            let node = item.map_err(|e| crate::report::parse_message(&e))?;
            // SAFETY: `node` was just parsed into the store, which outlives
            // the pass; the runtime works off raw handles.
            unsafe {
                rt.run(node).map_err(|e| crate::report::run_message(&e))?;
                if (*node).ty != self.types.comment_ {
                    tail = node;
                }
                if (*node).ty == self.types.declare_
                    && crate::identities::declare::gate_of(node) == self.types.pub_
                {
                    let name_node = *((*node).value as *const DyadPtr);
                    let name =
                        String::from_utf8_lossy(crate::identities::string::text(name_node))
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
        Ok((pubs, tail))
    }

    /// Declare an imported file's `pub` names into the current (importing)
    /// scope — pub-only exposure, the ordinary visibility rule, so a collision
    /// with a live name is the ordinary shadowing error. Idempotent where the
    /// name already resolves to the same identity (the same file imported
    /// twice into one scope).
    fn publish(&mut self, pubs: &[(String, DyadPtr)]) -> Result<(), ParseError> {
        for (name, identity) in pubs {
            if let Ok(r) = self.scopes.resolve(self.trie, name) {
                if r.identity == *identity {
                    continue;
                }
            }
            self.scopes.declare(self.trie, name, *identity).map_err(ParseError::Resolve)?;
        }
        Ok(())
    }

    /// `dyad`'s constructor body (#52): view the expression to the right.
    /// The view value is `{type: dyad, value: <the viewed node's address>}`
    /// — the one place a logos sits in a value, which is what makes `.logos` on
    /// it an ordinary field read (ruled August 2026).
    pub(crate) fn construct_view(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let inner = self.take_right(tape)?;
        let types = self.types;
        let node = self.store.alloc_raw(types.dyad_, inner as *mut u8);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// A member read on a dyad view (#52, ›The dyad's read surface‹):
    /// exactly the cell's two fields, `.logos` and `.value` — the dyad logos
    /// defines nothing else, so nothing else reads through the view. The
    /// value-decoding reads (`.operand(i)`) are ordinary `.` on the value
    /// itself, through its own logos (corrected August 2026). Read-only by
    /// construction: nothing here writes.
    ///
    /// # Safety
    /// `view` must be a view node as [`Parser::construct_view`] builds it.
    unsafe fn view_member(
        &mut self,
        view: DyadPtr,
        name: &str,
    ) -> Result<DyadPtr, ParseError> {
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

    /// A member read on a node standing as a logos (#52): the shared metadata
    /// this crate stores once per logos — `.arity`, `.roles[i]`,
    /// `.precedence`, `.associativity`, `.constructor`, `.destructor`, and the
    /// record layout `.fields`, `.size_bytes`, `.scope`. Typically reached as
    /// `(dyad a).ty.arity`. A null constructor/destructor slot is the
    /// honest undefined and errors until `?` exists.
    ///
    /// # Safety
    /// `logos` must be a logos identity node from the store.
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
        let operand_kind =
            matches!(meta::kind_of(logos), Some(meta::TUPLE_TAG | meta::LIST_TAG));
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
            "precedence" => Ok(self.scalar_value(
                NumType::F64,
                meta::precedence_of(logos).to_bits() as i64,
            )),
            "associativity" => Ok(self.scalar_value(
                NumType::I64,
                match meta::assoc_of(logos) {
                    Assoc::Left => 0,
                    Assoc::Right => 1,
                },
            )),
            "constructor" => {
                let c = meta::constructor_of(logos);
                if c.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                Ok(self.store.alloc_raw(self.types.dyad_, c as *mut u8))
            }
            "destructor" => {
                let d = meta::destructor_of(logos);
                if d.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                Ok(self.store.alloc_raw(self.types.dyad_, d as *mut u8))
            }
            "fields" if meta::is_record_type(logos) => {
                Ok(self
                    .store
                    .alloc_raw(self.types.dyad_, meta::record_fields_of(logos) as *mut u8))
            }
            "size_bytes" if meta::is_record_type(logos) => {
                Ok(self.scalar_value(NumType::I64, meta::record_size_of(logos) as i64))
            }
            "scope" if meta::is_record_type(logos) => {
                Ok(self
                    .store
                    .alloc_raw(self.types.dyad_, meta::record_scope_of(logos) as *mut u8))
            }
            "type" => Err(ParseError::TypeNeedsView),
            _ => Err(ParseError::BadReflectRead),
        }
    }

    /// Build a typed scalar value node: fresh storage holding `bits` at `nt`'s
    /// width. The reflection counts (`.arity`, `.size_bytes`) and measures
    /// (`.precedence`) are ordinary typed values, comparable with literals.
    fn scalar_value(
        &mut self,
        nt: crate::identities::numtype::NumType,
        bits: i64,
    ) -> DyadPtr {
        let ty = self.types.numtypes[nt as usize];
        let width = nt.bytes();
        let bytes = bits.to_ne_bytes();
        let storage = self.store.alloc_bytes(&bytes[..width]);
        self.store.alloc_raw(ty, storage)
    }

    /// A logos variable's fill, tried by `=`'s constructor at reduction:
    /// `name = <logos>` where the name token's binding is an unfilled logos
    /// placeholder (`logos == logos`, null value — the marker no real logos node
    /// has). The fill rebinds the name to the logos node at parse, completing
    /// the `name := logos ?` declaration — logos are comptime, so the assignment
    /// is elaboration, not a runtime store; from here the name is a full
    /// spelling of the logos (`==` folds, `a 5` juxtaposes, printing reads
    /// it). Only at a comptime execution position: inside a deferred or
    /// repeated body the rebind would fire once at parse, the wrong time and
    /// on both runtime branches ([`ParseError::NonComptimeTypeAssign`]). A
    /// second fill finds a real logos node, never the placeholder, and returns
    /// `None` into ordinary (rejected) assignment: define-once.
    pub(crate) fn try_type_fill(
        &mut self,
        tape: &ParsingTape,
    ) -> Result<Option<DyadPtr>, ParseError> {
        let Some(tok) = tape.at(-1).and_then(Cell::as_token).copied() else {
            return Ok(None);
        };
        let binding = tok.identity;
        if binding.is_null() {
            return Ok(None);
        }
        // SAFETY: `binding` is a resolved dyad from the store.
        if unsafe { !((*binding).ty == self.types.type_ && (*binding).value.is_null()) } {
            return Ok(None);
        }
        if self.runtime_depth > 0 {
            self.pos = tok.start;
            return Err(ParseError::NonComptimeTypeAssign);
        }
        let rhs = tape.at(1).copied().ok_or(ParseError::MissingOperand)?;
        let t = self.as_operand(rhs)?;
        // SAFETY: `t` is a reduced dyad off the tape.
        if !unsafe { crate::identities::is_type_value(&self.types, t) } {
            self.pos = tok.start;
            return Err(ParseError::BadDeclaredType);
        }
        let source = self.source;
        let name = &source[tok.start..tok.start + tok.len];
        let decl_scope =
            self.scopes.resolve(self.trie, name).map_err(ParseError::Resolve)?.scope;
        self.scopes.rebind_at(self.trie, name, t, decl_scope);
        // The fill IS the definition completing the declaration: a declare
        // node, a silent statement.
        let name_node =
            crate::identities::string::build_text(self.store, self.types.string_, name.as_bytes());
        let node = crate::identities::declare::build(
            self.store,
            self.types.declare_,
            self.types.ops.declare_,
            name_node,
            t,
        );
        Ok(Some(node))
    }

    /// One lex step: the next token as a tape cell with its source offset, or
    /// `None` at the end of input. Only whitespace is skipped — `#` is an
    /// identity, constructed at discovery like any literal. A spelling the
    /// trie does not know becomes a fresh-name cell (null identity, its span
    /// kept), declared by a following `:=` or reported at the boundary.
    fn lex_cell(&mut self) -> Result<Option<(Cell, usize)>, ParseError> {
        self.skip_whitespace();
        if self.pos >= self.source.len() {
            return Ok(None);
        }
        let source = self.source;
        let start = self.pos;
        match self.scopes.resolve(self.trie, &source[start..]) {
            Ok(r) => {
                self.pos = start + r.matched;
                let cell = Cell::Token(Token { start, len: r.matched, identity: r.identity });
                Ok(Some((cell, start)))
            }
            Err(e) => match self.lex_identifier() {
                Some((nstart, nlen)) => Ok(Some((Cell::Token(Token::new(nstart, nlen)), nstart))),
                None => Err(ParseError::Resolve(e)),
            },
        }
    }

    /// The constructor an appearance of `id` runs, or `None` for an inert
    /// cell (a value, a delimiter). The identity's own slot first; failing
    /// that, its type's shared instance constructor — application for an
    /// instance of `fn` and for a record logos (DESIGN ›The constructor is a
    /// field‹: "whether the name resolves to X's own slot or to the type's
    /// shared one is ordinary field semantics").
    fn ctor_of(&self, id: DyadPtr) -> Option<ConstructFn> {
        // SAFETY: `id` is a resolved dyad from the store.
        unsafe {
            if id.is_null() {
                return None;
            }
            if (*id).ty == self.types.fn_type {
                return Some(application);
            }
            if (*id).ty != self.types.type_ || crate::identities::meta::kind_of(id).is_none() {
                return None;
            }
            if crate::identities::meta::is_record_type(id) {
                return Some(application);
            }
            self.construct_of(id)
        }
    }

    /// The place of `id` on the one axis (its record's precedence), or
    /// [`prec::INERT`] for a cell that carries no record.
    ///
    /// [`prec::INERT`]: crate::identities::meta::prec::INERT
    fn precedence_of_cell(&self, id: DyadPtr) -> f64 {
        // SAFETY: as [`Parser::ctor_of`].
        unsafe {
            if id.is_null()
                || (*id).ty != self.types.type_
                || crate::identities::meta::kind_of(id).is_none()
                || crate::identities::meta::is_record_type(id)
            {
                crate::identities::meta::prec::APPLY
            } else {
                crate::identities::meta::precedence_of(id)
            }
        }
    }

    /// The associativity of `id`'s constructor: its record's, or left for an
    /// instance running its type's shared constructor (application), which
    /// carries no record of its own.
    fn assoc_of_cell(&self, id: DyadPtr) -> Assoc {
        // SAFETY: as [`Parser::ctor_of`].
        unsafe {
            if id.is_null()
                || (*id).ty != self.types.type_
                || crate::identities::meta::kind_of(id).is_none()
                || crate::identities::meta::is_record_type(id)
            {
                Assoc::Left
            } else {
                crate::identities::meta::assoc_of(id)
            }
        }
    }

    /// Run `construct` for the cell at the tape's cursor and settle the
    /// outcome on the cell: a Decline, or a Placed that left the token
    /// standing, makes the cell stand as its own value (DESIGN ›The
    /// constructor is a field‹: "a constructor that runs and finds nothing to
    /// consume declines, the frontier untouched" — `i32` before a `,`). There
    /// is no holding and no re-invocation.
    fn run_ctor(
        &mut self,
        construct: ConstructFn,
        id: DyadPtr,
        tape: &mut ParsingTape,
        discovery: bool,
    ) -> Result<(), ParseError> {
        let start = tape.start_of(tape.cursor());
        let was = std::mem::replace(&mut self.discovering, discovery);
        let outcome = construct(self, id, tape);
        self.discovering = was;
        let outcome = outcome?;
        // The same token, at the same offset: a splice that moved the cursor
        // onto a later cell of the same identity (`5 + 20 + 12`) is not it.
        let standing = matches!(tape.at(0), Some(Cell::Token(t)) if t.identity == id)
            && tape.start_of(tape.cursor()) == start;
        if matches!(outcome, Constructed::Decline) || standing {
            if let Some(cell) = tape.at_mut(0) {
                *cell = Cell::Dyad(id);
            }
        }
        Ok(())
    }

    /// Lex one segment onto `tape`: every cell up to the next `,`, `)`, or
    /// the end of input — none of which is consumed — constructing at
    /// discovery each cell whose identity sits at or above `(` on the axis
    /// (DESIGN ›The scope's constructor is the driver‹: "a token whose
    /// identity's precedence is at or above `(`'s own is constructed at
    /// discovery, before the next token is lexed … every other token is
    /// placed on the tape unconstructed").
    fn lex_segment(&mut self, tape: &mut ParsingTape) -> Result<Boundary, ParseError> {
        self.lex_segment_until(tape, None)
    }

    /// [`Parser::lex_segment`], optionally stopping before a `(` that follows
    /// at least one cell — the right side an identity reads up to its body
    /// bracket ([`Parser::drive_until_open`]), in one of two modes.
    fn lex_segment_until(
        &mut self,
        tape: &mut ParsingTape,
        stop_at_open: Option<RightSide>,
    ) -> Result<Boundary, ParseError> {
        loop {
            let Some((cell, start)) = self.lex_cell()? else {
                return Ok(Boundary::Eof);
            };
            if let Cell::Token(t) = cell {
                if t.identity == self.types.sep_ {
                    self.pos = start;
                    return Ok(Boundary::Comma);
                }
                if t.identity == self.types.close_ {
                    self.pos = start;
                    return Ok(Boundary::Close);
                }
                if let Some(mode) = stop_at_open {
                    if t.identity == self.types.open_ && !tape.is_empty() {
                        // In a condition the bracket is the caller's only when
                        // nothing before it would read it: a `(` after an
                        // unconstructed identity with a constructor is that
                        // identity's — `f(x)`'s arguments, `not (c)`'s operand,
                        // `==`'s right operand — never the body (DESIGN ›`X (…)`
                        // is one spelling, and X's constructor decides‹). A
                        // return logos takes no bracket, so there the first
                        // `(` is the body: `fn () -> i32 ( body )` "is taken by
                        // `fn` before `i32`'s juxtaposition could read a
                        // conversion".
                        let owner_pending = mode == RightSide::Condition
                            && matches!(tape.last(), Some(l) if !self.is_operand_cell(l));
                        if !owner_pending {
                            self.pos = start;
                            return Ok(Boundary::Open);
                        }
                    }
                }
            }
            tape.push(cell, start);
            if let Cell::Token(t) = cell {
                if let Some(construct) = self.ctor_of(t.identity) {
                    let prec = self.precedence_of_cell(t.identity);
                    // A right-side read stops before its caller's bracket, so
                    // an identity that reads its own bracket (`type`, `fn`) is
                    // not woken there: `-> type ( body )` names the classifier
                    // and leaves the body to `fn`.
                    let reader = prec == crate::identities::meta::prec::READER
                        || prec == crate::identities::meta::prec::DECLARE;
                    let asleep = stop_at_open == Some(RightSide::ReturnType) && reader;
                    if prec >= crate::identities::meta::prec::OPEN && !asleep {
                        self.run_ctor(construct, t.identity, tape, true)?;
                    }
                }
            }
        }
    }

    /// Construct a lexed segment at its boundary: comment cells are lifted out
    /// (prose is void-valued and invisible to value flow), then the
    /// unconstructed cells run highest precedence first, associativity
    /// breaking ties — left keeps the leftmost first, right the rightmost —
    /// each constructor taking what its syntax needs from the fully lexed
    /// segment, left or right, with no lookahead; what remains is read as
    /// operands, an undeclared spelling being the checked error at its own
    /// position. Returns the constructed cells in order with their offsets.
    fn construct_segment(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Vec<(DyadPtr, usize)>, ParseError> {
        let mut i = 0;
        while i < tape.len() {
            // SAFETY: a dyad cell is a node from the store.
            let is_comment = matches!(tape.cell(i), Some(Cell::Dyad(d)) if unsafe { (**d).ty } == self.types.comment_);
            if is_comment {
                let d = tape.cell(i).and_then(Cell::as_dyad).expect("matched above");
                self.lifted.push((tape.start_of(i), d));
                tape.set_cursor(i);
                tape.remove(0);
            } else {
                i += 1;
            }
        }
        loop {
            let mut best: Option<(usize, f64, ConstructFn, DyadPtr)> = None;
            for i in 0..tape.len() {
                let Some(Cell::Token(t)) = tape.cell(i) else { continue };
                let Some(construct) = self.ctor_of(t.identity) else { continue };
                let prec = self.precedence_of_cell(t.identity);
                let right = self.assoc_of_cell(t.identity) == Assoc::Right;
                let better = match best {
                    None => true,
                    Some((_, bp, _, _)) => prec > bp || (prec == bp && right),
                };
                if better {
                    best = Some((i, prec, construct, t.identity));
                }
            }
            let Some((i, _, construct, id)) = best else { break };
            tape.set_cursor(i);
            self.run_ctor(construct, id, tape, false)?;
        }
        let mut items = Vec::with_capacity(tape.len());
        for i in 0..tape.len() {
            let cell = *tape.cell(i).expect("in range");
            let start = tape.start_of(i);
            items.push((self.as_operand(cell)?, start));
        }
        Ok(items)
    }

    /// Lex and construct the cells up to the next `(` — the right side an
    /// identity reads before its bracket: an `if`'s or `while`'s condition,
    /// a `for`'s range, a `fn`'s return logos (DESIGN ›The scope's
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
                Err(ParseError::Trailing)
            }
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
    /// A `fn`'s return logos: the first bracket is the body, and an identity
    /// that reads its own bracket is not woken.
    ReturnType,
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
    fn rollback_undoes_journalled_declarations() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        scopes.declare(&mut trie, "keep", dyad(1)).unwrap();
        scopes.commit(); // committed declarations survive a rollback
        scopes.declare(&mut trie, "gone", dyad(2)).unwrap();

        scopes.rollback(&mut trie);
        assert_eq!(scopes.resolve(&trie, "keep").unwrap().identity, dyad(1));
        assert_eq!(scopes.resolve(&trie, "gone"), Err(ResolveError::Unknown));
        // The rolled-back name is free again — no permanent "shadowed".
        scopes.declare(&mut trie, "gone", dyad(3)).unwrap();
        assert_eq!(scopes.resolve(&trie, "gone").unwrap().identity, dyad(3));
    }

    #[test]
    fn rebind_points_a_spelling_at_the_original_identity() {
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        scopes.push(dyad(100));
        scopes.declare(&mut trie, "alias", dyad(1)).unwrap();
        scopes.rebind(&mut trie, "alias", dyad(2));
        assert_eq!(scopes.resolve(&trie, "alias").unwrap().identity, dyad(2));
        // The declare's journal entry still covers the rebound binding.
        scopes.rollback(&mut trie);
        assert_eq!(scopes.resolve(&trie, "alias"), Err(ResolveError::Unknown));
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
        scopes.declare(&mut trie, "a", dyad(1)).unwrap();
        scopes.mark_dead(&mut trie, "a", scope, dyad(1), dyad(50));

        assert_eq!(scopes.resolve(&trie, "a"), Err(ResolveError::Dead));
        scopes.declare(&mut trie, "a", dyad(2)).unwrap();
        assert_eq!(scopes.resolve(&trie, "a").unwrap().identity, dyad(2));
        // The dead entry is still indexed: its range is what reflection reads.
        let m = trie.get("a").unwrap();
        assert_eq!(m.contexts.len(), 2);
        assert!(m.contexts.iter().any(|c| c.identity == dyad(1) && c.end == dyad(50)));
    }

    #[test]
    fn rollback_restores_a_dead_mark() {
        // A REPL line that moves a name and then fails must leave the name
        // alive, exactly as a failed declaration leaves the spelling free.
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let scope = dyad(100);
        scopes.push(scope);
        scopes.declare(&mut trie, "a", dyad(1)).unwrap();
        scopes.commit();

        scopes.mark_dead(&mut trie, "a", scope, dyad(1), dyad(50));
        scopes.declare(&mut trie, "a", dyad(2)).unwrap();
        scopes.rollback(&mut trie);

        assert_eq!(scopes.resolve(&trie, "a").unwrap().identity, dyad(1));
        assert_eq!(trie.get("a").unwrap().contexts.len(), 1);
    }

    #[test]
    fn settle_patches_start_and_end_to_the_body_item() {
        // The range runs between body items: the declaring line, and the line
        // holding the `own`/`drop` (the node itself only while that line parses).
        let mut trie = RegexTrie::new();
        let mut scopes = ScopeStack::new();
        let scope = dyad(100);
        scopes.push(scope);
        scopes.declare(&mut trie, "a", dyad(1)).unwrap();
        let ctx = |trie: &RegexTrie| trie.get("a").unwrap().contexts[0];
        assert!(ctx(&trie).start.is_null());

        scopes.settle_item(&mut trie, scope, dyad(10));
        assert_eq!(ctx(&trie).start, dyad(10));
        assert!(ctx(&trie).end.is_null());

        scopes.mark_dead(&mut trie, "a", scope, dyad(1), dyad(50));
        assert_eq!(ctx(&trie).end, dyad(50), "provisional: the own/drop node");
        scopes.settle_item(&mut trie, scope, dyad(11));
        assert_eq!(ctx(&trie).end, dyad(11), "settled: the body item");
        assert_eq!(ctx(&trie).start, dyad(10), "start untouched by the end's settle");

        // A rebind keeps the range and moves the pending endpoint with it.
        scopes.declare(&mut trie, "b", dyad(3)).unwrap();
        scopes.rebind(&mut trie, "b", dyad(4));
        scopes.settle_item(&mut trie, scope, dyad(12));
        let b = trie.get("b").unwrap().contexts[0];
        assert_eq!((b.identity, b.start), (dyad(4), dyad(12)));
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
