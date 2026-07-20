// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

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
//! trie ([`crate::regex_trie`]) is only the name index. Still to come: pending
//! tokens lexed lazily onto the tape itself (v1 lexes on demand but resolves
//! each name eagerly at scan), and the general deferred-reduction driver that
//! runs each identity's `constructor` (v1's driver owns the scheduling; see
//! [`Construct`]).

use std::collections::HashSet;

use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::regex_trie::{RegexTrie, RegexTrieError};
use crate::store::Store;

/// A pending, not-yet-reduced token: the source span it was lexed from and the
/// identity it denotes. In the target model a token's identity is not fixed until
/// it reduces into its dyad, so a higher-precedence operator to its right can still
/// rewrite it (the token-rewriting mechanism); a reduced [`Cell::Dyad`] is frozen
/// against that. The v1 driver builds no such operators, so it resolves each name
/// eagerly at scan and pushes the token with `identity` already set: the
/// null-until-reduction path ([`Token::new`]) is real but, until macros arrive, is
/// exercised only by the tape tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// Byte offset of the token in the source.
    pub start: usize,
    /// Byte length of the matched span.
    pub len: usize,
    /// The identity this token denotes, or null until resolved. The v1 driver sets
    /// it eagerly at scan; deferred resolution arrives with token-rewriting operators.
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

    /// Append `cell` at the end and move the cursor to it. Used by the driver as
    /// it shifts operands and operators onto the frontier.
    pub fn push(&mut self, cell: Cell) {
        self.cells.push(cell);
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
        self.cells.splice(i - 1..=i + 1, [Cell::Dyad(dyad)]);
        self.cursor = self.cursor.min(self.cells.len().saturating_sub(1));
        true
    }

    /// The completed dyads flanking the cursor — an infix construct's two
    /// operands, the model's `tape[-1]` and `tape[+1]` reads at reduction.
    pub fn binary_operands(&self) -> Result<(DyadPtr, DyadPtr), ParseError> {
        let lhs = self.at(-1).and_then(Cell::as_dyad).ok_or(ParseError::MissingOperand)?;
        let rhs = self.at(1).and_then(Cell::as_dyad).ok_or(ParseError::MissingOperand)?;
        Ok((lhs, rhs))
    }

    /// The completed dyad immediately left of the cursor, if any — a postfix
    /// construct's left context (`.`'s value, `@`'s pointer), null when the
    /// construct opens fresh (`@` as the pointer-type prefix).
    pub fn left_dyad(&self) -> Option<DyadPtr> {
        self.at(-1).and_then(Cell::as_dyad)
    }

    /// The construct's own token — the cursor cell — its source span still
    /// attached. How an atom constructor reaches its matched text.
    pub fn own_token(&self) -> Option<Token> {
        self.at(0).and_then(Cell::as_token).copied()
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
    /// Every `(spelling, scope)` declared since the last [`ScopeStack::commit`].
    /// The REPL's undo log: a failed line rolls its declarations back out of the
    /// name index ([`ScopeStack::rollback`]), so a typo never burns a name for
    /// the rest of the session.
    journal: Vec<(String, DyadPtr)>,
}

impl ScopeStack {
    /// An empty scope stack.
    pub fn new() -> Self {
        ScopeStack { open: Vec::new(), set: HashSet::new(), journal: Vec::new() }
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

    /// Accept the journalled declarations: they are permanent, the undo log
    /// can be dropped.
    pub fn commit(&mut self) {
        self.journal.clear();
    }

    /// Undo every declaration journalled since the last [`ScopeStack::commit`],
    /// removing each from the name index (newest first). Removal is by
    /// spelling *and* declaring scope, so outer declarations of the same
    /// spelling are untouched.
    pub fn rollback(&mut self, trie: &mut RegexTrie) {
        while let Some((name, scope)) = self.journal.pop() {
            // The entry was inserted by this journal's own declare; a failed
            // removal means it was already pruned, which is fine.
            let _ = trie.remove(&name, scope);
        }
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
        let mut live = m.contexts.iter().filter(|c| self.is_open(c.scope));
        match (live.next(), live.next()) {
            (None, _) => Err(ResolveError::OutOfScope),
            (Some(c), None) => {
                Ok(Resolved { matched: m.matched, identity: c.identity, scope: c.scope })
            }
            (Some(_), Some(_)) => Err(ResolveError::Ambiguous),
        }
    }

    /// Declare `name` denoting `identity` in the current scope, enforcing
    /// no-shadowing: [`ResolveError::Shadowed`] if `name` already resolves to a
    /// live candidate in the open scopes. A known-but-out-of-scope or unknown
    /// name is free to (re)declare here. Requires a current scope. The
    /// declaration is journalled for [`ScopeStack::rollback`].
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
            // Known but closed, or unknown: both fine to declare here.
            Err(ResolveError::OutOfScope | ResolveError::Unknown) => {}
            // Ambiguous or an index error: surface it rather than declaring atop.
            Err(e) => return Err(e),
        }
        trie.insert(name, IdContext::new(identity, scope));
        self.journal.push((name.to_string(), scope));
        Ok(())
    }

    /// Re-point the just-declared `name` in the current scope at `identity`.
    /// Used by the declaration fixpoint when the value turns out to *be* an
    /// existing identity (a type): the name becomes another spelling of that
    /// node, so pointer-identity checks (`is_numtype_node`, type equality) see
    /// the original. The journal entry from the declare still covers it.
    pub fn rebind(&mut self, trie: &mut RegexTrie, name: &str, identity: DyadPtr) {
        let scope = self.current().expect("rebind needs an open scope");
        let _ = trie.remove(name, scope);
        trie.insert(name, IdContext::new(identity, scope));
    }

    /// Re-point `name`, declared in `scope` (an open ancestor, from
    /// [`Resolved::scope`]), at `identity`. Unlike [`ScopeStack::rebind`] the
    /// target is the *declaring* scope, not the current one: a type variable's
    /// fill inside a comptime-taken branch completes the outer declaration,
    /// rather than binding a block-local spelling that dies with the branch.
    pub fn rebind_at(trie: &mut RegexTrie, name: &str, identity: DyadPtr, scope: DyadPtr) {
        let _ = trie.remove(name, scope);
        trie.insert(name, IdContext::new(identity, scope));
    }
}

/// Operator associativity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
}

/// The core type handles the parser needs to type the nodes it opens and to
/// resolve abstract operators. Bundled so that adding a handle does not churn
/// [`Parser::new`]'s signature; an `Infix` `build` callback receives it so an
/// operator like `+` can pick its concrete machine op from the operand types.
#[derive(Debug, Clone, Copy)]
pub struct CoreTypes {
    /// `scope`: the type of each scope the parser opens.
    pub scope: DyadPtr,
    /// `array` (of `dyad@`): a sequence's expression list rides behind one.
    pub array_: DyadPtr,
    /// `struct`: the type of a parameter-list / field-list node.
    pub struct_: DyadPtr,
    /// `fn`: the type of a function; a call whose callee is `fn`-typed yields a
    /// value (which the arithmetic operators' `is_numeric` check treats as numeric).
    pub fn_type: DyadPtr,
    /// `i32`: an alias for `numtypes[I32]`, the seed's default numeric type.
    pub i32_: DyadPtr,
    /// The numeric primitive type nodes, indexed by `NumType` (null if unregistered).
    /// A resolved operator stores the relevant one in its value slot.
    pub numtypes: [DyadPtr; 10],
    /// `bool`: the type a comparison produces and an `if` condition must be.
    pub bool_: DyadPtr,
    /// `rational_number`: a numeric literal, molds to a concrete numeric type.
    pub rational: DyadPtr,
    /// `return`: the optional early yield; used to commit a `return`-wrapped rational
    /// tail to the function's declared return type.
    pub return_: DyadPtr,
    /// `if`: the value-producing conditional; used to commit a rational in either branch
    /// (a tail position) to the function's declared return type.
    pub if_: DyadPtr,
    /// `while`: the loop statement; unit-valued, so value positions reject it.
    pub while_: DyadPtr,
    /// `for`: the counted-loop statement; unit-valued like `while`.
    pub for_: DyadPtr,
    /// The `Type : Type` root; pointer type nodes are typed by it.
    pub type_: DyadPtr,
    /// `deref`: the dereference node postfix `@` builds.
    pub deref_: DyadPtr,
    /// `storeptr`: the store-through node `=` builds over a deref lhs.
    pub storeptr_: DyadPtr,
    /// `addr`: the address-of node prefix `&` builds (resolves its place's
    /// address at run/lower time, per-activation for a frame local).
    pub addr_: DyadPtr,
    /// `construct`: the struct-construction statement a struct-typed call builds.
    pub construct_: DyadPtr,
    /// `string`: the text-literal type (`«…»`); inert in the seed, above all the
    /// comment substance.
    pub string_: DyadPtr,
    /// `comment`: the prose-node type a statement-level `#` builds; reflectable
    /// graph structure, invisible to value flow.
    pub comment_: DyadPtr,
    /// `convert`: the shared scalar numeric conversion; a conversion node's result type
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
    /// `declare`: the type of the declaration node `name := value` builds; a
    /// statement yielding unit.
    pub declare_: DyadPtr,
    /// `compile`: the fn type's shared member (`f.compile()`); a statement
    /// yielding unit, so value positions reject it.
    pub compile_: DyadPtr,
    /// `callable`: the type of every exec leaf and of a compiled fn's code
    /// (`[entry: @exec, convention]`).
    pub callable_: DyadPtr,
    /// `container-i64`: the convention compiled artifacts are minted under.
    pub conv_container: DyadPtr,
    /// `(` — the opening paren/call token; the expect-helpers compare against it.
    pub open_: DyadPtr,
    /// `)` — the closing paren token.
    pub close_: DyadPtr,
    /// `:` — the typed-declaration / field-list token.
    pub colon_: DyadPtr,
    /// `,` — the one explicit separator.
    pub sep_: DyadPtr,
    /// `->` — the return-type arrow.
    pub arrow_: DyadPtr,
    /// `else` — the branch token `if`'s constructor consumes.
    pub else_: DyadPtr,
    /// `in` — the loop-range token `for`'s constructor consumes.
    pub in_: DyadPtr,
    /// `..` — the range token `for`'s constructor consumes.
    pub dotdot_: DyadPtr,
    /// `.` — the field-access token (its constructor consumes `tape[-1]`).
    pub dot_: DyadPtr,
    /// `@` — the pointer token (postfix deref / pointer-type prefix).
    pub at_: DyadPtr,
    /// `:=` — the declaration token.
    pub declare_tok: DyadPtr,
    /// The concrete-op leaves (`add_i32`, `lt_f64`, `store_u8`, …): the
    /// parse-time resolver's `(family, operand type) → leaf` table. A builder
    /// resolves an application to one leaf and stores it in the node's op slot;
    /// run jumps through the node, never a table (issue #44).
    pub ops: crate::identities::ops::OpLeaves,
}

/// The fields of a function node's value struct, in order, as built by
/// [`Parser::parse_fn`]: the input `struct`, the return type, the reflectable body,
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
/// per-call storage its `:=` locals, loop variables, and struct instances
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


/// What a constructor produced. `Node` is the built dyad; the *driver* splices
/// it over the cells the construct consumed (its own token, a consumed left,
/// an infix triple), so tape surgery stays in one place. `Decline` is "not
/// mine": the constructor consumed nothing, the driver drops its token,
/// rewinds to its start, and lets the expression finalize — the left-hand twin
/// of DESIGN ›a constructor that accepts not consuming its right-hand side
/// then yields its own dyad as-is‹ (which is an ordinary `Node` of itself).
pub enum Constructed {
    /// The constructed dyad.
    Node(DyadPtr),
    /// The construct does not apply here; nothing was consumed.
    Decline,
}

/// How the driver treats a resolved identity — computed from its record alone:
/// constructor presence, the precedence field, and the record kind. This is
/// the whole of the driver's dispatch; there is no schedule table (DESIGN
/// ›The driver needs only one token of lookahead: it lexes the next token,
/// reads its `precedence`, and shifts or reduces the frontier accordingly‹).
#[derive(Clone, Copy)]
enum Class {
    /// A plain operand — a user binding, a data-type value: pushed.
    Operand,
    /// A finite-precedence extender (an infix operator): shifted and reduced
    /// by precedence; its constructor is invoked at reduction.
    Extender(f64, Assoc),
    /// An infinite-precedence (tight) extender — `(` as a call, postfix
    /// `.`/`@`: its constructor is invoked immediately over the left context.
    Tight(ConstructFn),
    /// A fresh-start constructor (keyword opener, literal, `&`): invoked; it
    /// consumes forward tokens itself.
    Construct(ConstructFn),
    /// A bare delimiter token (`)`, `,`, `:`, `->`, `else`, `in`, `..`, `:=`):
    /// never consumed by the driver — the expression finalizes and the
    /// enclosing constructor eats it.
    Delimiter,
}

/// Every identity's parse-time constructor — the one `seed-parse` entry
/// signature its constructor-slot leaf carries. The constructor receives the
/// parser (a service it re-enters for `parse_expression`, the expect-helpers,
/// declaration), the identity being constructed, and the tape with the cursor
/// on the construct's own token: it reads its span from the cursor cell, its
/// left context from `tape.at(-1)` (the model's `tape[-1]`), its right operand
/// from `tape.at(1)` (an infix, invoked at reduction), and any further tokens
/// by consuming source forward. It returns what it built; the driver owns the
/// splice.
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
    /// A construct that requires a `(` (a `struct`/parameter list) was not
    /// followed by one.
    ExpectedOpen,
    /// A field list expected a field name where it found neither a name nor `)`.
    ExpectedField,
    /// A fn signature's parameter list was not followed by `->`.
    ExpectedArrow,
    /// A fn signature's `->` was not followed by a return type.
    ExpectedReturnType,
    /// An abstract operator (e.g. `+`) could not resolve a concrete machine op for
    /// its operand types (DESIGN ›a `+` over mismatched or sizeless types simply
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
    /// A binary operator's operands were two *different* concrete numeric types (e.g.
    /// `i32` and `f64`). Cross-type arithmetic needs an explicit cast; there is no
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
    /// A struct construction's argument count did not match its field count.
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
    /// A numeric conversion `type(value)` was malformed: not exactly one operand, or a
    /// non-numeric operand (there is nothing to convert).
    BadCast,
    /// A typed declaration's `name :` — or a type variable's fill `name = …` —
    /// was followed by something that is not a type value: the type slot holds
    /// a type, so the expression must evaluate to one (a spelled type, or a
    /// `-> type` call resolved at parse time).
    BadDeclaredType,
    /// A type variable was assigned inside a deferred or repeated body (a fn
    /// body, loop body, or runtime `if` branch). The fill rebinds the name at
    /// parse time, which is only sound where parsing and running coincide.
    NonComptimeTypeAssign,
    /// A typed declaration of a non-numeric type (`a : type`, a struct, a
    /// pointer, `bool`, `void`) — the declared-type storage for those is not in
    /// the seed yet, and this names the gap instead of mis-storing the value.
    NonNumericDeclaredType,
    /// A `-> type` call could not be resolved at parse time — either running it
    /// failed (its arguments were not comptime-known) or it did not yield a type.
    /// A type-returning function is evaluated during parsing (roadmap #30), so its
    /// arguments must be known then.
    NonComptimeTypeCall,
    /// A nested function referenced (or took the address of) a local or a
    /// parameter of an enclosing function — a closure capture, which v1 does not
    /// support. Each function's locals and parameters live in its own per-call
    /// activation; reaching an outer one would read the wrong frame at run time.
    CapturedLocal,
}

/// Build a call node `{ty: callee, value: [args…, null]}`, the application
/// `callee(args)`. Like a binary operator's `{ty: op, value: [lhs, rhs]}`, a call's
/// value is the operand array of its arguments (null-terminated so `run` can count
/// them); a nullary call carries a null value. The callee's type decides how the
/// call runs, exactly as an operator's does.
/// Whether `node`'s result is a `bool`: a `bool` literal/value, a comparison
/// (`<`/`>`/`==`/…), or a logical operator (`and`/`or`/`not`). An `if` condition and
/// a logical operator's operands must be one; arithmetic and other values are not.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_bool_result(types: &CoreTypes, node: DyadPtr) -> bool {
    let ty = (*node).ty;
    // A sequence's value is its trailing expression's.
    if ty == types.scope {
        return match last_sequence_expr(node) {
            Some(last) => is_bool_result(types, last),
            None => false,
        };
    }
    ty == types.bool_
        || ty == types.lt
        || ty == types.gt
        || ty == types.eq
        || ty == types.le
        || ty == types.ge
        || ty == types.ne
        || ty == types.and_
        || ty == types.or_
        || ty == types.not_
}

/// The parse-time truth of a bool literal — `{ty: bool, value -> i32 0/1}`, the
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
/// `{ty: scope, value: [exprs, op]}` — trailing comment nodes are prose, not
/// the tail — or `None` for a scope with no expression array (a
/// struct/parameter-list scope).
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
/// `node` must be a valid dyad from the store, with the value shapes its type
/// implies (as the parser builds them).
unsafe fn contains_return(types: &CoreTypes, node: DyadPtr) -> bool {
    let ty = (*node).ty;
    if ty == types.return_ {
        return true;
    }
    if ty == types.if_ {
        let p = (*node).value as *const DyadPtr;
        let (then, els) = (*p.add(1), *p.add(2));
        return contains_return(types, then) || (!els.is_null() && contains_return(types, els));
    }
    if ty == types.scope {
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
    /// The core type handles the parser types opened nodes with (see [`CoreTypes`]).
    types: CoreTypes,
    /// The placeholder of the declaration currently awaiting its value, or null.
    /// When the value opens with a `fn` literal, [`Parser::parse_fn`] publishes the
    /// signature onto it before the body parses, so a recursive self-call resolves
    /// its parameter and return types instead of the unbound-placeholder defaults.
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
    /// order do NOT coincide. Comptime effects that rebind names at parse (a type
    /// variable's fill) are rejected while this is non-zero: inside such a body
    /// the rebinding would happen once, at the wrong time, and on both runtime
    /// branches. Comptime-taken `if` branches do not count (they run iff parsed).
    runtime_depth: u32,
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
    /// `store`, and lexing via `trie`. `types` are the core handles the parser
    /// types the scopes and structs it opens with. Dispatch needs nothing else:
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
            frames: Vec::new(),
            runtime_depth: 0,
        }
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

    /// The core type handles (copied out, so a `&mut self` call can follow).
    pub(crate) fn types(&self) -> CoreTypes {
        self.types
    }

    /// Allocate storage for a function-local place of `width` bytes, typed
    /// `ty_node`. Inside a function (the frame stack is non-empty) the place is
    /// *frame-relative*: it claims the next offset in the current frame — after
    /// the parameters, which claimed the frame's first offsets at the signature
    /// — and its storage is per-call: the interpreter's frame on its activation
    /// stack, the JIT's stack slot. At top level it is an absolute global blob,
    /// exactly as before. The node is `{ty: ty_node, value: <place>}`, its value
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
        let bytes = self.source.as_bytes();
        loop {
            while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if self.pos < bytes.len() && bytes[self.pos] == b'#' {
                while self.pos < bytes.len() && bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
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

    /// Classify a resolved identity from its record alone — constructor
    /// presence, the precedence field, and the record kind; no schedule table
    /// (DESIGN ›it lexes the next token, reads its `precedence`, and shifts or
    /// reduces the frontier accordingly‹). Only a type identity carries a
    /// record in its value slot (its `ty` is the `Type : Type` root);
    /// everything else a name can resolve to — a user binding, a fn value, a
    /// struct type, a type variable — is a plain operand.
    fn classify(&self, id: DyadPtr) -> Class {
        // SAFETY: `id` is a resolved dyad from the store; the record read is
        // gated on it being a type identity whose registration built the record.
        unsafe {
            if id.is_null()
                || (*id).ty != self.types.type_
                || crate::identities::meta::kind_of(id).is_none()
            {
                return Class::Operand;
            }
            let prec = crate::identities::meta::precedence_of(id);
            match self.construct_of(id) {
                Some(c) if prec == f64::INFINITY => Class::Tight(c),
                Some(_) if prec.is_finite() => {
                    Class::Extender(prec, crate::identities::meta::assoc_of(id))
                }
                Some(c) => Class::Construct(c),
                None if crate::identities::meta::kind_of(id)
                    == Some(crate::identities::meta::TOKEN_TAG) =>
                {
                    Class::Delimiter
                }
                None => Class::Operand,
            }
        }
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
        tape.push(Cell::Token(Token { start, len, identity: id }));
        match construct(self, id, &mut tape)? {
            Constructed::Node(d) => Ok(Some(d)),
            Constructed::Decline => Ok(None),
        }
    }

    /// The constructor of `id`, decoded from its constructor-slot leaf — the
    /// parse-time analogue of `run`'s op-slot jump: dispatch flows through the
    /// graph, no table anywhere. `None` for an undefined constructor (a
    /// delimiter token, a data type).
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

    /// Take the pending declaration placeholder (see [`Parser::pending_fn`]):
    /// `fn`'s constructor claims it so a recursive self-call inside the body
    /// resolves the published signature.
    pub(crate) fn take_pending_fn(&mut self) -> DyadPtr {
        std::mem::replace(&mut self.pending_fn, std::ptr::null_mut())
    }

    /// Peek the next token's identity and length without consuming it — the
    /// graph, not a schedule table, is what the callers compare against
    /// (`id == self.types.else_`). `None` at end of input or when nothing
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

    /// Consume a directly following rational literal, building its node;
    /// `None` (nothing consumed) when the next token is anything else. A
    /// constructor service: the numeric types' juxtaposition (`i32 3`) and
    /// `-`'s negated literal read their operand through this.
    pub(crate) fn consume_rational(&mut self) -> Result<Option<DyadPtr>, ParseError> {
        let Some((lit, matched)) = self.peek_token() else {
            return Ok(None);
        };
        if lit != self.types.rational {
            return Ok(None);
        }
        let start = self.pos;
        self.pos += matched;
        let span = &self.source[start..start + matched];
        crate::identities::rational::build(self.store, lit, span).map(Some)
    }

    /// Consume `- <rational>` as a negated literal (the literal regex is
    /// unsigned; the negative literal is the prefix `-` negating at parse), or
    /// nothing — the two-token peek restores the position when no literal
    /// follows the `-`.
    pub(crate) fn consume_negated_rational(&mut self) -> Result<Option<DyadPtr>, ParseError> {
        let save = self.pos;
        if !self.consume_token(self.types.minus) {
            return Ok(None);
        }
        match self.consume_rational()? {
            // SAFETY: the literal was just built by `consume_rational`.
            Some(lit) => Ok(Some(unsafe {
                crate::identities::rational::negate(self.store, self.types.rational, lit)
            })),
            None => {
                self.pos = save;
                Ok(None)
            }
        }
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
    fn expect_open(&mut self) -> Result<(), ParseError> {
        if self.consume_token(self.types.open_) {
            Ok(())
        } else {
            Err(ParseError::ExpectedOpen)
        }
    }

    /// Consume a `:` if the next token is one, reporting whether it was.
    fn consume_colon(&mut self) -> bool {
        self.consume_token(self.types.colon_)
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

    /// Parse a `( field-list )` into a struct node. `struct_type` is the identity
    /// that introduced it (`struct`, or later `fn`'s parameter list). Fields are
    /// `name : type` or a bare `name`, separated by `,`; each becomes a `:`
    /// declaration dyad `{ty: field-type, value: undefined}` whose name is declared
    /// in the struct's own scope. The node's value is a [`STRUCT_TAG`] record
    /// storing the layout the definition derives — the scope, the `fields`
    /// array node, and the packed `size_bytes` — filled here, where the type's
    /// layout locks (issue #47; DESIGN ›a type whose constructor derives the
    /// layout automatically — reading the field declarations in its scope and
    /// filling `fields` and `size_bytes`‹). Fresh field names are read raw
    /// here, which is why the field list needs its own sub-parse rather than
    /// the generic driver.
    ///
    /// [`STRUCT_TAG`]: crate::identities::meta::STRUCT_TAG
    pub fn parse_struct(&mut self, struct_type: DyadPtr) -> Result<DyadPtr, ParseError> {
        self.expect_open()?;
        // The struct's own scope: a `scope`-typed node keyed by address for
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
            // `&mut self` the reentrant type-parse and the declaration then need.
            let source = self.source;
            let name = &source[start..start + len];
            // Optional `: type`; a bare name leaves the field's type slot undefined.
            let ty = if self.consume_colon() {
                self.parse_expression()?
            } else {
                std::ptr::null_mut()
            };
            let field = self.store.alloc_raw(ty, std::ptr::null_mut());
            // The field's NAME is not stored on the struct: declaring it here
            // puts an id_context in the shared name index, and resolution is
            // open-scope filtering over that one index (DESIGN ›Name resolution
            // is scope-filtered‹; a per-struct names store is recorded as
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
        // type's width and anything else (a bare or type-valued name, only
        // meaningful for parameter lists) as the 8-byte container — the same
        // width rule parameters claim frame offsets by.
        let size_bytes: u64 = fields
            .iter()
            .map(|&f| {
                // SAFETY: `f` is the field dyad just built.
                let ty = unsafe { (*f).ty };
                if unsafe {
                    crate::identities::numtype::is_scalar_place_type(self.types.struct_, ty)
                } {
                    unsafe { crate::identities::numtype::numtype_of_type(ty) }.bytes() as u64
                } else {
                    8
                }
            })
            .sum();
        let fields_arr = crate::identities::array::build(self.store, self.types.array_, &fields);
        let record =
            crate::identities::meta::struct_record(self.store, scope, fields_arr, size_bytes);
        Ok(self.store.alloc_raw(struct_type, record.cast()))
    }

    /// Parse a function literal `fn ( params ) -> ret ( body )` (DESIGN ›A
    /// function's surface‹), given `fn_type` (the resolved `fn` identity). The
    /// parameter list is a `struct` (the step-2 field list); the return type after
    /// `->` is a single type identity; the body is a `( )` scope parsed with the
    /// parameter scope reopened, so parameters resolve inside it. The node is
    /// `{ty: fn, value -> [input, output, body, bcode]}` — the params struct, the
    /// return type, the reflectable body, and the compiled `bcode` (null until
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
    /// the body reads real parameter and return types.
    pub fn parse_fn(&mut self, fn_type: DyadPtr, declared: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The parameter list is a struct; parse_struct opens and closes its scope.
        let input = self.parse_struct(self.types.struct_)?;
        self.expect_arrow()?;
        let output = self.parse_return_type()?;

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
        // in this one.
        self.frames.push(OpenFn { size: 0 });
        let depth = self.frames.len();
        // SAFETY: `input` is the struct just built; its record stores the
        // fields array, and each parameter's value slot is still the null
        // parse_struct left there.
        unsafe {
            let fields = crate::identities::meta::struct_fields_of(input);
            for &param in crate::identities::array::items(fields) {
                let ty = (*param).ty;
                let width = if crate::identities::numtype::is_scalar_place_type(
                    self.types.struct_,
                    ty,
                ) {
                    crate::identities::numtype::numtype_of_type(ty).bytes()
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

        // Reopen the parameter scope (stored in the input struct's record) so
        // the body resolves parameters, then parse the `( body )` — a deferred
        // body (it runs at calls, not at parse), so parse-time rebinding is
        // off inside.
        // SAFETY: `input` is the struct just built; its record stores its scope.
        let scope = unsafe { crate::identities::meta::struct_scope_of(input) };
        self.scopes.push(scope);
        self.runtime_depth += 1;
        self.expect_open()?;
        let body = self.parse_sequence()?;
        self.expect_close()?;
        self.runtime_depth -= 1;
        self.scopes.pop();
        let frame_size = self.frames.pop().expect("parse_fn pushed a frame").size;

        // A comptime-rational tail expression commits to the declared return type here
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

    /// Parse a conditional `if ( cond ) ( then )` with an optional `else ( else )`
    /// (given the resolved `if` identity). Each part is a parenthesized expression,
    /// and the condition must be a `bool` ([`ParseError::NonBoolCondition`]). The
    /// node is `{ty: if, value: [cond, then, else]}`, the else slot null when the
    /// `else` is absent: run takes the branch the condition selects, compile emits a
    /// two-way branch. An else-less `if` is a statement — it yields unit — so value
    /// positions reject it ([`ParseError::MissingElse`]); and because branches are
    /// always parenthesized, a nested `if` cannot capture an outer `else` (no
    /// dangling else). `else if ( cond ) ( then ) …` is sugar for a nested `if` in
    /// the else slot, so chains parse right-associatively without `else ( if … )`.
    /// Unlike `fn`, `if` opens no new scope — its parts resolve in the enclosing one.
    pub fn parse_if(&mut self, if_type: DyadPtr) -> Result<DyadPtr, ParseError> {
        // Condition: a parenthesized expression, required to be a bool.
        self.expect_open()?;
        let cond = self.parse_sequence()?;
        self.expect_close()?;
        let types = self.types;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(&types, cond) } {
            return Err(ParseError::NonBoolCondition);
        }

        // A comptime condition — a bool literal, the shape `true`/`false` and
        // every parse-time fold produce (`a.type == i32`, two-literal
        // comparisons) — resolves the conditional NOW, in the one pass: the
        // taken branch parses in place and an untaken branch's tokens are
        // dropped unlexed, so nothing inside it is resolved, committed, or
        // declared. This is what lets branches for *other* comptime types
        // coexist (`a=9.9` under `a : i32` parses only in the world where it is
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

    /// Parse a logical negation `not ( operand )` (given the resolved `not`
    /// identity). The operand is parenthesized (like an `if` condition), which keeps
    /// the binding unambiguous, and must be a `bool` ([`ParseError::NonBoolOperands`]).
    /// The node is `{ty: not, value: operand}`.
    pub fn parse_not(&mut self, not_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        self.expect_open()?;
        let operand = self.parse_sequence()?;
        self.expect_close()?;
        let types = self.types;
        // SAFETY: `operand` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(&types, operand) } {
            return Err(ParseError::NonBoolOperands);
        }
        // A bool-literal operand folds now (pure, nothing lost), like the
        // `==`/`and`/`or` folds — what keeps a comptime chain comptime.
        // SAFETY: `operand` is the reduced dyad just parsed.
        if let Some(v) = unsafe { bool_literal_value(&types, operand) } {
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
    /// thrown away‹). The node is `{ty: while, value: [cond, body]}`, a statement
    /// yielding unit: value positions reject it ([`ParseError::StatementAsValue`]),
    /// and a `return` in the body is rejected ([`ParseError::EarlyReturn`]) since v1
    /// has no unwinding to exit the loop with.
    pub fn parse_while(&mut self, while_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        self.expect_open()?;
        let cond = self.parse_sequence()?;
        self.expect_close()?;
        let types = self.types;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(&types, cond) } {
            return Err(ParseError::NonBoolCondition);
        }
        // A repeated body: parse-time rebinding is off inside it.
        self.runtime_depth += 1;
        self.expect_open()?;
        let body = self.parse_sequence()?;
        self.expect_close()?;
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
    /// variable is a fresh block-local of the range's resolved numeric type; a
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
        let start = self.parse_range_operand()?;
        if !self.consume_token(self.types.dotdot_) {
            return Err(ParseError::ExpectedRange);
        }
        let end = self.parse_range_operand()?;
        let step = if self.consume_token(self.types.dotdot_) {
            Some(self.parse_range_operand()?)
        } else {
            None
        };

        // Resolve the loop type across the range parts (concrete types must
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
        let ty = unsafe { crate::identities::resolve_loop_parts(self.store, &types, &mut parts)? };
        let (start, end) = (parts[0], parts[1]);
        let step = parts.get(2).copied().unwrap_or(std::ptr::null_mut());
        if step_was_literal {
            use crate::identities::numtype;
            // SAFETY: `step` is the committed literal just built; `ty` a numtype node.
            let (bits, nt) = unsafe {
                (numtype::read_scalar((*step).ty, (*step).value), numtype::of_type_node(ty))
            };
            if numtype::apply_compare(numtype::CmpOp::Gt, nt, bits, 0) == 0 {
                return Err(ParseError::BadStep);
            }
        }

        // The loop variable: a fresh per-call local of the loop type (a frame slot
        // inside a function, an absolute blob at top level).
        // SAFETY: `ty` is a numtype node from resolve_loop_parts.
        let width = unsafe { crate::identities::numtype::of_type_node(ty) }.bytes();
        let var = self.alloc_local(ty, width);
        let scope = self.store.alloc_raw(types.scope, std::ptr::null_mut());
        self.scopes.push(scope);
        self.scopes.declare(self.trie, name, var).map_err(ParseError::Resolve)?;
        // A repeated body: parse-time rebinding is off inside it.
        self.runtime_depth += 1;
        self.expect_open()?;
        let body = self.parse_sequence()?;
        self.expect_close()?;
        self.runtime_depth -= 1;
        self.scopes.pop();
        // SAFETY: `body` is the reduced dyad just parsed.
        if unsafe { contains_return(&types, body) } {
            return Err(ParseError::EarlyReturn);
        }

        let value = self.store.alloc_operands(&[var, start, end, step, body, self.types.ops.for_]);
        Ok(self.store.alloc_raw(for_id, value))
    }

    /// Parse one bounded range part for `for` — a *primary*, never a full
    /// expression: a literal (optionally negated), a resolved name with an
    /// optional `.field` chain, or an explicit `( … )` scope. Bounded because
    /// the range is followed by the body's `( … )`, which a full expression
    /// parse would consume as a call on the endpoint.
    fn parse_range_operand(&mut self) -> Result<DyadPtr, ParseError> {
        self.skip_trivia();
        let source = self.source;
        if self.pos >= source.len() {
            return Err(ParseError::ExpectedRange);
        }
        let r = self
            .scopes
            .resolve(self.trie, &source[self.pos..])
            .map_err(ParseError::Resolve)?;
        let id = r.identity;
        if id == self.types.open_ {
            // An explicit parenthesized expression.
            self.pos += r.matched;
            let e = self.parse_sequence()?;
            self.expect_close()?;
            Ok(e)
        } else if id == self.types.rational || id == self.types.string_ {
            // A literal.
            let start = self.pos;
            self.pos += r.matched;
            self.construct_leaf(id, start, r.matched)?.ok_or(ParseError::ExpectedRange)
        } else if id == self.types.minus {
            // A negated literal (`-` then a rational), as in `-`'s constructor.
            self.consume_negated_rational()?.ok_or(ParseError::ExpectedRange)
        } else if matches!(self.classify(id), Class::Operand) {
            // A plain resolved name, with an optional `.field` chain.
            self.pos += r.matched;
            let mut node = id;
            while self.consume_token(self.types.dot_) {
                // SAFETY: `node` is a resolved dyad from the store.
                node = unsafe { self.parse_field_access(node)? };
            }
            Ok(node)
        } else {
            Err(ParseError::ExpectedRange)
        }
    }

    /// Resolve a field access `lhs.name` to a *place*: an ordinary numeric node
    /// over the instance's storage at the field's byte offset (DESIGN ›Resolution
    /// is one rule‹ — the declaration found decides, and a field declaration is
    /// the offset inside the value area). The field name resolves in the struct
    /// type's own scope, alone (never against the enclosing scopes). The `.` has
    /// already been consumed.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    pub(crate) unsafe fn parse_field_access(&mut self, lhs: DyadPtr) -> Result<DyadPtr, ParseError> {
        // `.type` is reflection, not a struct field: it yields the lhs's own type as a
        // first-class type-value (an interned type node), so it works on ANY node, not
        // only struct instances (roadmap #30; the sketch's `tape[0].type`). `type` is
        // reserved for this — a struct member literally named `type` is not honored.
        // Peek the field name and rewind if it is not `type`, so an ordinary field
        // falls through to the resolution below unchanged.
        let save = self.pos;
        if let Some((nstart, nlen)) = self.lex_identifier() {
            if &self.source[nstart..nstart + nlen] == "type" {
                // A deref's logical type is its pointee (held in the node, not in its
                // `.ty`); every other node's type is its `.ty` pointer, already an
                // interned type node ready to use as a value.
                if (*lhs).ty == self.types.deref_ {
                    let (_, pointee, _) = crate::identities::pointer::deref_parts(lhs);
                    return Ok(pointee);
                }
                return Ok((*lhs).ty);
            }
            // `.compile` on an fn-typed value is the fn type's shared member
            // (DESIGN ›Execution is function application‹: "The `fn` type
            // carries two shared functions: `compile` … and `run`"; `run` is
            // calling). `f.compile()` builds a compile statement whose run
            // lowers `f`'s body and installs its `bcode`, so the next call
            // jumps to machine code. The name-compare here is the seed's
            // stand-in for shared-member resolution through the type's scope
            // (one mechanism at self-hosting); reserved only on fn-typed
            // values, so a struct field named `compile` still resolves. The
            // `()` is mandatory — compile is a function, applied like any
            // other, taking no arguments (DESIGN ›Operands travel on the
            // stack‹). The callable leaf is minted NOW, entry zero, because
            // minting needs the store the parser holds; the run patches the
            // finalized entry in.
            if &self.source[nstart..nstart + nlen] == "compile"
                && (*lhs).ty == self.types.fn_type
            {
                self.expect_open()?;
                self.expect_close()?;
                let code = crate::identities::callable::mint(
                    self.store,
                    self.types.callable_,
                    0,
                    self.types.conv_container,
                );
                let value =
                    self.store.alloc_operands(&[lhs, code, self.types.ops.compile_]);
                return Ok(self.store.alloc_raw(self.types.compile_, value));
            }
        }
        self.pos = save;
        // Through a struct pointer, `p@.x` folds the field offset into the deref
        // (the address is runtime; the offset and the field's type are not).
        if (*lhs).ty == self.types.deref_ {
            let (ptr_expr, pointee, base_off) =
                crate::identities::pointer::deref_parts(lhs);
            if pointee.is_null() || (*pointee).ty != self.types.struct_ {
                return Err(ParseError::UnsupportedOperands);
            }
            let (field, offset) = self.resolve_field(pointee)?;
            let types = self.types;
            return Ok(crate::identities::pointer::build_deref(
                self.store,
                &types,
                ptr_expr,
                (*field).ty,
                base_off as usize + offset,
            ));
        }
        // The direct case: an instance of a struct type, with storage — the
        // access is a *place*, its offset folded into the instance's own place
        // now. `wrapping_add` keeps a frame-tagged instance value a valid tagged
        // offset (`FRAME_TAG | (base + field)`); for an absolute instance it is
        // ordinary pointer arithmetic. `place_addr` resolves it at run/lower time.
        let struct_type = (*lhs).ty;
        if struct_type.is_null()
            || (*struct_type).ty != self.types.struct_
            || (*lhs).value.is_null()
        {
            return Err(ParseError::UnsupportedOperands);
        }
        let (field, offset) = self.resolve_field(struct_type)?;
        let addr = (*lhs).value.wrapping_add(offset);
        Ok(self.store.alloc_raw((*field).ty, addr))
    }

    /// Resolve the field name at the cursor against `struct_type`'s own scope
    /// alone (its value[0] — an enclosing binding of the same spelling can never
    /// shadow or double a field), returning the field node and its byte offset.
    ///
    /// # Safety
    /// `struct_type` must be a struct type node from the store.
    unsafe fn resolve_field(
        &mut self,
        struct_type: DyadPtr,
    ) -> Result<(DyadPtr, usize), ParseError> {
        let (nstart, nlen) = self.lex_identifier().ok_or(ParseError::ExpectedField)?;
        let source = self.source;
        let name = &source[nstart..nstart + nlen];
        let mut field_scope = ScopeStack::new();
        field_scope.push(crate::identities::meta::struct_scope_of(struct_type));
        let field =
            field_scope.resolve(self.trie, name).map_err(ParseError::Resolve)?.identity;
        let (fields, _) = crate::identities::instance::layout(struct_type)?;
        let (_, _, offset) = fields
            .iter()
            .copied()
            .find(|&(f, _, _)| f == field)
            .ok_or(ParseError::ExpectedField)?;
        Ok((field, offset))
    }

    /// Whether `callee` is a function whose declared return type is the `type` root —
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
    /// A run failure (e.g. a runtime-only argument) or a non-type result is reported
    /// as [`ParseError::NonComptimeTypeCall`].
    ///
    /// # Safety
    /// `call` must be a reduced call node from the store.
    unsafe fn eval_type_call(&mut self, call: DyadPtr) -> Result<DyadPtr, ParseError> {
        let mut rt = crate::run::Runtime::new(
            self.types.fn_type,
            self.types.rational,
            self.types.struct_,
        );
        let bits = rt.run(call).map_err(|_| ParseError::NonComptimeTypeCall)?;
        let node = bits as usize as DyadPtr;
        if crate::identities::is_type_value(&self.types, node) {
            Ok(node)
        } else {
            Err(ParseError::NonComptimeTypeCall)
        }
    }

    /// Build a postfix dereference `lhs@`: the lhs's static type must be a
    /// pointer type — a pointer variable or `&x` literal (its `ty`), a pointer
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

    /// Parse a pointer type after its opening `@` (already consumed): any
    /// further `@`s deepen it (`@@i32`), then a resolved type name — a numeric
    /// type or a struct type — closes it. Fresh nodes per use; pointees carry
    /// the identity.
    pub(crate) fn parse_pointer_type(&mut self) -> Result<DyadPtr, ParseError> {
        let mut depth = 1usize;
        while self.consume_token(self.types.at_) {
            depth += 1;
        }
        self.skip_trivia();
        let source = self.source;
        if self.pos >= source.len() {
            return Err(ParseError::UnsupportedOperands);
        }
        let r = self
            .scopes
            .resolve(self.trie, &source[self.pos..])
            .map_err(ParseError::Resolve)?;
        let base = r.identity;
        // SAFETY: `base` is a resolved dyad from the store.
        let is_type = crate::identities::is_numtype_node(&self.types, base)
            || unsafe { !(*base).ty.is_null() && (*base).ty == self.types.struct_ };
        if !is_type {
            return Err(ParseError::UnsupportedOperands);
        }
        self.pos += r.matched;
        let mut ty = base;
        for _ in 0..depth {
            ty = crate::identities::pointer::make_pointer_type(self.store, self.types.type_, ty);
        }
        Ok(ty)
    }

    /// Parse an address-of after its `&` (already consumed): a resolved name
    /// with an optional `.field` chain, ending at a storage-backed place — a
    /// numeric, pointer, or struct-typed node with a value slot. Yields an
    /// `addr` node (see [`crate::identities::pointer::build_addr`]) that resolves
    /// the place's address at run/lower time, so a frame-relative local or
    /// parameter yields a per-activation address. A comptime binding has no
    /// storage and is [`ParseError::BadAddressOf`].
    pub(crate) fn parse_address_of(&mut self) -> Result<DyadPtr, ParseError> {
        self.skip_trivia();
        let source = self.source;
        if self.pos >= source.len() {
            return Err(ParseError::BadAddressOf);
        }
        let r = self
            .scopes
            .resolve(self.trie, &source[self.pos..])
            .map_err(ParseError::Resolve)?;
        if !matches!(self.classify(r.identity), Class::Operand) {
            // Keywords, operators, literals: not places.
            return Err(ParseError::BadAddressOf);
        }
        self.pos += r.matched;
        let mut node = r.identity;
        while self.consume_token(self.types.dot_) {
            // SAFETY: `node` is a resolved dyad from the store.
            node = unsafe { self.parse_field_access(node)? };
        }
        // SAFETY: `node` is a resolved dyad from the store.
        unsafe {
            let ty = (*node).ty;
            let is_place = crate::identities::is_numtype_node(&self.types, ty)
                || crate::identities::numtype::is_pointer_type(ty)
                || (!ty.is_null() && (*ty).ty == self.types.struct_);
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
            Ok(crate::identities::pointer::build_addr(self.store, &self.types, node))
        }
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

    /// Parse a fn's return type: a single resolved type identity (`i32`, …) or a
    /// pointer type (`@i32`). Compound type expressions arrive later.
    fn parse_return_type(&mut self) -> Result<DyadPtr, ParseError> {
        if self.consume_token(self.types.at_) {
            return self.parse_pointer_type();
        }
        self.skip_trivia();
        let source = self.source;
        if self.pos >= source.len() {
            return Err(ParseError::ExpectedReturnType);
        }
        let r = self.scopes.resolve(self.trie, &source[self.pos..]).map_err(ParseError::Resolve)?;
        self.pos += r.matched;
        Ok(r.identity)
    }

    /// Build a call `callee ( args )`, the `(` already consumed — the service
    /// `(`'s constructor re-enters when a completed dyad stands to its left
    /// (juxtaposition binds tightest). A numeric type callee is a conversion
    /// (`i32(a)`), a struct type constructs an instance, a type-returning
    /// callee resolves NOW at comptime; any other callee is an ordinary call.
    pub(crate) fn parse_call(&mut self, callee: DyadPtr) -> Result<DyadPtr, ParseError> {
        let args = self.parse_arg_list()?;
        self.expect_close()?;
        if crate::identities::is_numtype_node(&self.types, callee) {
            // SAFETY: `callee` is a numtype node; `args` are reduced dyads.
            unsafe { crate::identities::build_cast(self.store, &self.types, callee, &args) }
        } else if unsafe { !(*callee).ty.is_null() && (*callee).ty == self.types.struct_ } {
            // A struct type applied to its field values constructs an
            // instance — the type-constructor doctrine, like `i32(a)`.
            let types = self.types;
            // SAFETY: `callee` is a struct type node; `args` are reduced dyads
            // from the store.
            unsafe {
                // The instance is a per-call local (a frame slot inside a
                // function), sized from the struct layout, so a recursive call
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
            // declared type (the typed slot); an unbound callee has no
            // signature yet and commits nothing.
            let types = self.types;
            let mut args = args;
            // SAFETY: `callee` and `args` are reduced dyads from the store.
            unsafe {
                crate::identities::commit_call_args(self.store, &types, callee, &mut args)?;
            }
            let call = build_call(self.store, callee, &args);
            // A call whose callee returns a type is resolved NOW, at comptime:
            // run it and substitute the concrete type it produces (roadmap
            // #30), so the result flows as an ordinary type value through
            // `==`, `:=`, `.type`, and display. SAFETY: `callee`/`call` are
            // reduced dyads.
            if unsafe { self.returns_type(callee) } {
                unsafe { self.eval_type_call(call) }
            } else {
                Ok(call)
            }
        }
    }

    /// Parse a call's argument list: comma-separated value expressions up to the
    /// closing `)` (left unconsumed for the caller's [`Parser::expect_close`]). The
    /// opening `(` has already been consumed. Unlike a field list, arguments are
    /// ordinary expressions, not `name : type` declarations.
    fn parse_arg_list(&mut self) -> Result<Vec<DyadPtr>, ParseError> {
        let mut args = Vec::new();
        loop {
            if self.at_close() {
                break;
            }
            args.push(self.parse_expression()?);
            if !self.consume_separator() {
                break;
            }
        }
        Ok(args)
    }

    /// Parse a sequence of expressions up to the enclosing scope's end (a `)`, or
    /// the end of input), consuming an optional `,` between them (DESIGN
    /// ›Expressions are self-delimiting; `,` is the one explicit separator‹). A
    /// single expression is returned as itself; several become a sequence node
    /// `{ty: scope, value: [expr0 … exprN, null]}` that runs its expressions in
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
        let mut exprs = Vec::new();
        while let Some(item) = self.parse_next() {
            exprs.push(item?);
        }
        self.scopes.pop();
        // Prose is invisible to value flow: the expression count and the tail are
        // taken over the non-comment expressions.
        // SAFETY: `exprs` are reduced dyads just parsed/built.
        let values = exprs
            .iter()
            .filter(|&&e| unsafe { !crate::identities::numtype::is_comment_type((*e).ty) })
            .count();
        match (values, exprs.len()) {
            (0, _) => Err(ParseError::Empty),
            (_, 1) => Ok(exprs[0]),
            _ => {
                // Every non-tail value runs for effect only; the tail is the last
                // non-comment expression. A `return` anywhere else would run
                // without exiting (no unwinding yet), so reject it.
                let types = self.types;
                let tail = exprs
                    .iter()
                    .rposition(|&e| unsafe {
                        !crate::identities::numtype::is_comment_type((*e).ty)
                    })
                    .expect("values >= 1");
                for (i, &e) in exprs.iter().enumerate() {
                    // SAFETY: `e` is a reduced dyad just parsed.
                    if i != tail && unsafe { contains_return(&types, e) } {
                        return Err(ParseError::EarlyReturn);
                    }
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
        self.skip_whitespace();
        if self.pos >= self.source.len() {
            return None;
        }
        // A statement-level `#` builds a reflectable comment node — prose is
        // part of the body's structure (DESIGN ›`#` is the one comment
        // constructor‹). Mid-expression `#`s remain trivia ([`skip_trivia`]).
        if self.source.as_bytes()[self.pos] == b'#' {
            return Some(self.parse_comment());
        }
        if self.at_close() {
            return None;
        }
        let expr = self.parse_expression();
        if expr.is_ok() {
            // A `#` directly after the expression is the next statement-level
            // comment — the separator peek must not read through it as trivia.
            self.skip_whitespace();
            if !(self.pos < self.source.len() && self.source.as_bytes()[self.pos] == b'#') {
                // The optional `,`: a boundary the expressions already imply,
                // consumed where written (also purely for readability).
                self.consume_separator();
            }
        }
        Some(expr)
    }

    /// Parse a statement-level comment: `#` followed by a `«…»` string or raw
    /// text to the end of the line (the line form is sugar for the string form).
    /// Builds the reflectable comment node `{ty: comment, value -> string node}`
    /// the settled design specifies (DESIGN ›Text literals are plain values; `#`
    /// is the one comment constructor‹).
    fn parse_comment(&mut self) -> Result<DyadPtr, ParseError> {
        self.pos += 1; // the `#`
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

    /// Parse one expression to a single dyad, consuming source from the current
    /// position. Each call drives its own tape, so a prefix constructor can parse
    /// its operand by calling this again (the parser is a service the constructors
    /// re-enter, per the sealed "constructors drive" model). An expression is
    /// self-delimiting: a token that would start a new operand after a completed
    /// dyad ends it (left unconsumed for [`Parser::parse_sequence`]).
    pub fn parse_expression(&mut self) -> Result<DyadPtr, ParseError> {
        let mut tape = ParsingTape::new();
        loop {
            // A `#` after a completed dyad ends the expression: it is the next
            // statement-level comment, the sequence parser's to build into a node
            // ([`Parser::parse_comment`]). Only a genuinely mid-expression `#`
            // (after a pending operator) is trivia here.
            self.skip_whitespace();
            if self.pos < self.source.len()
                && self.source.as_bytes()[self.pos] == b'#'
                && matches!(tape.last(), Some(Cell::Dyad(_)))
            {
                break;
            }
            self.skip_trivia();
            if self.pos >= self.source.len() {
                break;
            }
            let source = self.source;
            let start = self.pos;

            // A fresh name in declaration position: `name := value` binds `name` to
            // the value. The name is declared *before* the value is parsed, so the
            // value can refer to it (self-recursion). A name not followed by `:=`
            // rewinds and resolves normally below.
            if let Some((nstart, nlen)) = self.lex_identifier() {
                // A fresh name followed by `:` is the typed declaration
                // `name : type`: it introduces the name and sets its type slot,
                // leaving the value undefined (DESIGN ›Declarations are immutable by
                // default‹ — `a : i32` and `a := i32 ?` will declare the same node
                // once `?` exists (no `?` token yet, issue #38); the seed
                // approximates undefined as zeroed storage until phase bits
                // land). The type may be computed: the type expression parses
                // through the ordinary machinery, so a `-> type` call (`a :
                // metatype(0)`) comptime-resolves to its concrete type first — the
                // dependent declaration is the same declaration. The fresh-name
                // gate keeps a resolvable name before `:` a field-list `:`.
                if matches!(self.peek_token(), Some((id, _)) if id == self.types.colon_)
                    && matches!(tape.last(), None | Some(Cell::Dyad(_)))
                    && self.scopes.resolve(self.trie, &source[nstart..]).is_err()
                {
                    // After a completed dyad the declaration starts the NEXT
                    // expression (expressions are self-delimiting): stop before it.
                    if matches!(tape.last(), Some(Cell::Dyad(_))) {
                        self.pos = start;
                        break;
                    }
                    let Some((_, matched)) = self.peek_token() else { unreachable!() };
                    self.pos += matched; // consume `:`
                    // The type first, the name after: the declared type must be a
                    // comptime-known type value, and the name is not yet bound
                    // while its own type parses (`a : a` fails resolution cleanly).
                    let t = self.parse_expression()?;
                    // SAFETY: `t` is the reduced dyad just parsed.
                    if !unsafe { crate::identities::is_type_value(&self.types, t) } {
                        self.pos = nstart;
                        return Err(ParseError::BadDeclaredType);
                    }
                    // The binding, by declared type. A numeric type gets a zeroed
                    // place at its width, the same shape a `:=` snapshot's place
                    // takes — reads are plain loads, `=` reassigns — but with no
                    // initializer to run; per-call (frame-relative) inside a
                    // function, absolute at top level, like every local. The
                    // `type` root declares a TYPE VARIABLE: a null-valued
                    // placeholder — the undefined type, the null value being the
                    // marker no real type node has (every registered type carries
                    // a record) — filled once by a later `name = <type>`, which
                    // rebinds the name at parse (types are comptime; roadmap #30).
                    let place = if t == self.types.type_ {
                        self.store.alloc_raw(self.types.type_, std::ptr::null_mut())
                    } else if crate::identities::is_numtype_node(&self.types, t) {
                        // SAFETY: `t` is a registered numeric type node.
                        let nt = unsafe { crate::identities::numtype::of_type_node(t) };
                        self.alloc_local(t, nt.bytes())
                    } else {
                        self.pos = nstart;
                        return Err(ParseError::NonNumericDeclaredType);
                    };
                    let name = &source[nstart..nstart + nlen];
                    if let Err(e) = self.scopes.declare(self.trie, name, place) {
                        self.pos = nstart;
                        return Err(ParseError::Resolve(e));
                    }
                    // The declaration is graph structure: a declare node carrying
                    // the spelling and the place (its run is a harmless load — a
                    // declaration is a statement, silent and unit-valued).
                    let name_node = crate::identities::string::build_text(
                        self.store,
                        self.types.string_,
                        name.as_bytes(),
                    );
                    let node = crate::identities::declare::build(
                        self.store,
                        self.types.declare_,
                        self.types.ops.declare_,
                        name_node,
                        place,
                    );
                    tape.push(Cell::Dyad(node));
                    continue;
                }
                if matches!(self.peek_token(), Some((id, _)) if id == self.types.declare_tok) {
                    // A declaration after a completed dyad starts the NEXT
                    // expression (expressions are self-delimiting): stop before it.
                    if matches!(tape.last(), Some(Cell::Dyad(_))) {
                        self.pos = start;
                        break;
                    }
                    let Some((_, matched)) = self.peek_token() else { unreachable!() };
                    self.pos += matched; // consume `:=`
                    // `source` is `&'a str` (Copy), independent of the `&mut self`
                    // the declaration and value parse then need (as in `parse_struct`).
                    let name = &source[nstart..nstart + nlen];
                    // The placeholder is `fn`-typed so a recursive self-call sees a
                    // function-typed callee while the value is still parsing; the
                    // fixpoint below overwrites it with the value's real type.
                    let placeholder =
                        self.store.alloc_raw(self.types.fn_type, std::ptr::null_mut());
                    if let Err(e) = self.scopes.declare(self.trie, name, placeholder) {
                        // The stuck point is the name itself (it is what shadows).
                        self.pos = nstart;
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
                    // is the place, the statement fills it each run. A *type* value
                    // (`x := i32`, `p := struct(…)`) rebinds the name to the type
                    // node itself instead — the name becomes another spelling of
                    // that type, so the pointer-identity checks (`is_numtype_node`,
                    // cross-type mismatch, struct-type equality) see the original.
                    // SAFETY: `placeholder`/`value` are valid dyads just built.
                    let declared = unsafe {
                        if (*value).ty == self.types.construct_ {
                            let ops = (*value).value as *mut DyadPtr;
                            let instance = *ops;
                            (*placeholder).ty = (*instance).ty;
                            (*placeholder).value = (*instance).value;
                            *ops = placeholder;
                            value
                        } else if (*value).ty == self.types.type_
                            || (*value).ty == self.types.struct_
                        {
                            self.scopes.rebind(self.trie, name, value);
                            value
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
                            // fn/type/unit value keeps its own binding below.
                            let (ty_node, width) = crate::identities::scalar_binding_type(
                                self.store,
                                &self.types,
                                value,
                            );
                            let place = self.alloc_local(ty_node, width);
                            let init = crate::identities::build_scalar_init(
                                self.store,
                                &self.types,
                                place,
                                value,
                            )?;
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
                    let name_node = crate::identities::string::build_text(
                        self.store,
                        self.types.string_,
                        name.as_bytes(),
                    );
                    let node = crate::identities::declare::build(
                        self.store,
                        self.types.declare_,
                        self.types.ops.declare_,
                        name_node,
                        declared,
                    );
                    tape.push(Cell::Dyad(node));
                    continue;
                }
                // A type variable's fill: `name = <type>` where `name` resolves
                // to an unfilled type placeholder (`ty == type`, null value — the
                // marker no real type node has). The fill rebinds the name to the
                // type node at parse, completing the `name : type` declaration —
                // types are comptime, so the assignment is elaboration, not a
                // runtime store; from here the name is a full spelling of the
                // type (`==` folds, `a 5` juxtaposes, printing reads it). Only at
                // a comptime execution position: inside a deferred or repeated
                // body the rebind would fire once at parse, the wrong time and on
                // both runtime branches ([`ParseError::NonComptimeTypeAssign`]).
                // A second fill finds a real type node, never the placeholder, and
                // falls through to ordinary (rejected) assignment: define-once.
                if let Some((eq_id, matched)) = self.peek_token() {
                    if eq_id == self.types.assign {
                        if let Ok(r) = self.scopes.resolve(self.trie, &source[nstart..]) {
                            let binding = r.identity;
                            let decl_scope = r.scope;
                            // SAFETY: `binding` is a resolved dyad from the store.
                            if unsafe {
                                (*binding).ty == self.types.type_ && (*binding).value.is_null()
                            } {
                                // After a completed dyad the fill starts the NEXT
                                // expression: stop before it.
                                if matches!(tape.last(), Some(Cell::Dyad(_))) {
                                    self.pos = start;
                                    break;
                                }
                                if self.runtime_depth > 0 {
                                    self.pos = nstart;
                                    return Err(ParseError::NonComptimeTypeAssign);
                                }
                                self.pos += matched; // consume `=`
                                let t = self.parse_expression()?;
                                // SAFETY: `t` is the reduced dyad just parsed.
                                if !unsafe { crate::identities::is_type_value(&self.types, t) } {
                                    self.pos = nstart;
                                    return Err(ParseError::BadDeclaredType);
                                }
                                let name = &source[nstart..nstart + nlen];
                                ScopeStack::rebind_at(self.trie, name, t, decl_scope);
                                // The fill IS the definition completing the
                                // declaration: a declare node, a silent statement.
                                let name_node = crate::identities::string::build_text(
                                    self.store,
                                    self.types.string_,
                                    name.as_bytes(),
                                );
                                let node = crate::identities::declare::build(
                                    self.store,
                                    self.types.declare_,
                                    self.types.ops.declare_,
                                    name_node,
                                    t,
                                );
                                tape.push(Cell::Dyad(node));
                                continue;
                            }
                        }
                    }
                }
                // Not a declaration: rewind and resolve the name normally.
                self.pos = start;
            }

            let r = match self.scopes.resolve(self.trie, &source[start..]) {
                Ok(r) => r,
                // An unresolvable token after a completed dyad starts the NEXT
                // expression (expressions are self-delimiting): stop before it,
                // exactly as a resolvable operand-starter does below. The next
                // expression may legitimately begin with it — a typed
                // declaration parsing its type expression must not trip on the
                // name that follows — and a genuinely unknown name is reported
                // at the same position when that expression parses it. With an
                // operand still pending, this expression needs the token: error.
                Err(e) => {
                    if matches!(tape.last(), Some(Cell::Dyad(_))) {
                        self.pos = start;
                        break;
                    }
                    return Err(ParseError::Resolve(e));
                }
            };
            let id = r.identity;
            let class = self.classify(id);

            // A bare delimiter (`)`, `,`, `:`, `->`, `else`, `in`, `..`, `:=`)
            // ends this (sub-)expression; leave it unconsumed for the enclosing
            // constructor (the opener that started the scope, the field-list or
            // fn parser).
            if matches!(class, Class::Delimiter) {
                break;
            }
            // A fresh start — an operand or a fresh-start constructor — while a
            // completed dyad sits at the tape's tail begins the NEXT expression
            // (DESIGN ›Expressions are self-delimiting‹): stop without
            // consuming it. An extender continues the expression (an `(` after
            // a dyad stays a call — juxtaposition binds tightest), so it is
            // never a boundary. Type-literal juxtaposition needs no exception
            // here: the numeric *type's* constructor consumes its literal
            // forward, so the literal is never scanned at this level.
            if matches!(class, Class::Operand | Class::Construct(_))
                && matches!(tape.last(), Some(Cell::Dyad(_)))
            {
                break;
            }
            self.pos = start + r.matched;

            match class {
                // A plain operand: a reference to the resolved identity. A
                // frame-local or parameter of an enclosing function (a capture)
                // is rejected — v1 has no closures.
                Class::Operand => {
                    // SAFETY: `id` is a resolved dyad from the store.
                    unsafe { self.check_capture(id)? };
                    tape.push(Cell::Dyad(id));
                }
                // A fresh-start constructor — a keyword opener (`struct`, `fn`,
                // `if`, `not`, `while`, `for`, `&`), a literal, a numeric type
                // (juxtaposition), `return`: jump it; it consumes its own
                // forward tokens. One scheduling nuance still rides here:
                // `fn`'s declaration handoff — the pending placeholder is
                // claimed by `fn`'s constructor only when the literal opens a
                // (sub-)expression (an empty tape) — which moves into `fn`'s
                // constructor with the declaration stage.
                Class::Construct(construct) => {
                    let suppressed = (id == self.types.fn_type && !tape.is_empty())
                        .then(|| self.take_pending_fn());
                    tape.push(Cell::Token(Token { start, len: r.matched, identity: id }));
                    let node = construct(self, id, &mut tape);
                    if let Some(pending) = suppressed {
                        self.pending_fn = pending;
                    }
                    let dyad = match node? {
                        Constructed::Node(d) => d,
                        Constructed::Decline => {
                            tape.pop();
                            self.pos = start;
                            break;
                        }
                    };
                    tape.pop(); // the construct's own token cell
                    tape.push(Cell::Dyad(dyad));
                }
                // A tight (infinite-precedence) extender — `(` as a call,
                // postfix `.`/`@`: its constructor is invoked immediately and
                // reads the completed dyad to its left off the tape — the
                // model's `tape[-1]` read. `.` requires one; `@` without one
                // is the pointer-type prefix; `(` without one is a grouping
                // scope. The driver splices: the construct's own token always,
                // plus the left cell iff a completed dyad stood there for the
                // constructor to consume.
                Class::Tight(construct) => {
                    tape.push(Cell::Token(Token { start, len: r.matched, identity: id }));
                    let had_left = tape.left_dyad().is_some();
                    let node = match construct(self, id, &mut tape)? {
                        Constructed::Node(d) => d,
                        Constructed::Decline => {
                            tape.pop();
                            self.pos = start;
                            break;
                        }
                    };
                    tape.pop(); // the construct's own token cell
                    if had_left {
                        tape.pop(); // the left dyad the constructor consumed
                    }
                    tape.push(Cell::Dyad(node));
                }
                // An operator: reduce anything binding tighter to its left, then
                // shift it onto the tape as a pending token; its constructor is
                // invoked at reduction, when precedence finalizes the cluster.
                Class::Extender(precedence, assoc) => {
                    // With no completed operand to its left an extender opens
                    // *fresh*, and its constructor decides — `-` consumes a
                    // following numeric literal into a negated one (`f(-1)`,
                    // `x := -5`); a Decline shifts the token as a pending
                    // operator instead (the dangling-operator path: general
                    // unary minus over non-literals is later work).
                    if !matches!(tape.last(), Some(Cell::Dyad(_))) {
                        if let Some(construct) = self.construct_of(id) {
                            tape.push(Cell::Token(Token {
                                start,
                                len: r.matched,
                                identity: id,
                            }));
                            match construct(self, id, &mut tape)? {
                                Constructed::Node(d) => {
                                    tape.pop(); // the construct's own token cell
                                    tape.push(Cell::Dyad(d));
                                    continue;
                                }
                                Constructed::Decline => {
                                    tape.pop(); // fall through to the shift
                                }
                            }
                        }
                    }
                    // Precedence and associativity came off the operator's own
                    // record at classification — the graph, not a parser
                    // table, is their source of truth.
                    self.reduce_pending(&mut tape, precedence, assoc)?;
                    tape.push(Cell::Token(Token { start, len: r.matched, identity: id }));
                }
                // Handled by the delimiter break above.
                Class::Delimiter => {
                    unreachable!("a delimiter ends the loop")
                }
            }
        }
        self.reduce_all(&mut tape)?;
        match tape.len() {
            0 => Err(ParseError::Empty),
            1 => tape.cell(0).and_then(Cell::as_dyad).ok_or(ParseError::MissingOperand),
            _ => Err(ParseError::Trailing),
        }
    }

    /// Reduce the pending operator at `tape`'s tail while it binds at least as
    /// tightly as an incoming operator of `prec`/`assoc` (strictly tighter, or
    /// equal when the incoming one is left-associative).
    fn reduce_pending(
        &mut self,
        tape: &mut ParsingTape,
        prec: f64,
        assoc: Assoc,
    ) -> Result<(), ParseError> {
        loop {
            let n = tape.len();
            if n < 3 {
                break;
            }
            let op_idx = n - 2;
            let op_id = match tape.cell(op_idx) {
                Some(Cell::Token(t)) => t.identity,
                _ => break,
            };
            // Reduce only a token flanked by two completed operands; the
            // constructor reads them back off the tape (`tape[-1]`, `tape[+1]`).
            if tape.cell(op_idx - 1).and_then(Cell::as_dyad).is_none()
                || tape.cell(op_idx + 1).and_then(Cell::as_dyad).is_none()
            {
                break;
            }
            let construct = match self.construct_of(op_id) {
                Some(c) => c,
                None => break,
            };
            // SAFETY: `op_id` is an operator identity from the store (it matched
            // `Infix` above), carrying its registration-built record.
            let prev_prec = unsafe { crate::identities::meta::precedence_of(op_id) };
            if !(prev_prec > prec || (prev_prec == prec && assoc == Assoc::Left)) {
                break;
            }
            tape.set_cursor(op_idx);
            let dyad = match construct(self, op_id, tape)? {
                Constructed::Node(d) => d,
                // An infix that declines at reduction leaves the tape as-is; the
                // seed's operators never do, but the contract stands.
                Constructed::Decline => break,
            };
            tape.reduce_binary(op_idx, dyad);
        }
        Ok(())
    }

    /// At end of input, reduce every remaining pending operator on `tape` (right
    /// to left, as the precedence invariant leaves them).
    fn reduce_all(&mut self, tape: &mut ParsingTape) -> Result<(), ParseError> {
        while tape.len() > 1 {
            let n = tape.len();
            if n < 3 {
                return Err(ParseError::Trailing);
            }
            let op_idx = n - 2;
            let op_id = match tape.cell(op_idx) {
                Some(Cell::Token(t)) => t.identity,
                _ => return Err(ParseError::Trailing),
            };
            if tape.cell(op_idx - 1).and_then(Cell::as_dyad).is_none()
                || tape.cell(op_idx + 1).and_then(Cell::as_dyad).is_none()
            {
                return Err(ParseError::MissingOperand);
            }
            let construct = match self.construct_of(op_id) {
                Some(c) => c,
                None => return Err(ParseError::Trailing),
            };
            tape.set_cursor(op_idx);
            let dyad = match construct(self, op_id, tape)? {
                Constructed::Node(d) => d,
                Constructed::Decline => return Err(ParseError::Trailing),
            };
            tape.reduce_binary(op_idx, dyad);
        }
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
