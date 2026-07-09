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
//! trie ([`crate::regex_trie`]) is only the name index. Still to come (V1PLAN
//! Phase 3): wiring the tape to lazily lex pending tokens from source, and the
//! deferred-reduction driver that runs each identity's `constructor`.

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

/// Operator associativity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
}

/// The core type handles the parser needs to type the nodes it opens: a `scope`
/// for each field list, a `struct` for each parameter/field list. Bundled so that
/// adding a handle does not churn [`Parser::new`]'s signature.
#[derive(Debug, Clone, Copy)]
pub struct CoreTypes {
    /// `scope`: the type of each scope the parser opens.
    pub scope: DyadPtr,
    /// `struct`: the type of a parameter-list / field-list node.
    pub struct_: DyadPtr,
}

/// The fields of a function node's value struct, in order, as built by
/// [`Parser::parse_fn`]: the input `struct`, the return type, the reflectable body,
/// and the compiled `bcode`. The native *leaf* functions (`=`, `+`, `return`) keep
/// their machine code in the run version's table instead (they have a null value
/// slot); a user function carries its own compiled `bcode` here, null until
/// compiled, and `run` jumps to it when present (DESIGN ›Execution is function
/// application‹).
pub const FN_INPUT: usize = 0;
/// See [`FN_INPUT`].
pub const FN_OUTPUT: usize = 1;
/// See [`FN_INPUT`].
pub const FN_BODY: usize = 2;
/// See [`FN_INPUT`]. The compiled machine code (`exec@`), null until compiled.
pub const FN_BCODE: usize = 3;

/// A core identity's native parse-time behaviour: how the driver schedules the
/// token and how its dyad is built. Core identities are hand-built natives (see
/// `crate::identities`); a self-hosted Logos would carry this as graph metadata
/// on the type instead. In this seed the driver owns tape scheduling and the
/// `build` functions only construct the node; general tape-driving constructors
/// (needed for macros and token-rewriting operators) come later.
#[derive(Clone, Copy)]
pub enum Construct {
    /// A literal/atom: build a leaf node from the matched source span.
    Atom(fn(&mut Store, DyadPtr, &str) -> Result<DyadPtr, ParseError>),
    /// A prefix keyword that takes the rest of the expression as one operand
    /// (e.g. `return <expr>`): build a node from the identity and that parsed
    /// operand. v1 consumes to the end of the current expression; delimited forms
    /// come with brackets.
    Prefix(fn(&mut Store, DyadPtr, DyadPtr) -> Result<DyadPtr, ParseError>),
    /// An infix binary operator with a precedence and associativity: build a node
    /// from its operator identity and two already-reduced operands.
    Infix {
        precedence: f64,
        assoc: Assoc,
        build: fn(&mut Store, DyadPtr, DyadPtr, DyadPtr) -> Result<DyadPtr, ParseError>,
    },
    /// An opening bracket `(`: parse the body up to the matching close; the
    /// scope's value is what that body evaluates to (DESIGN ›A scope's value is
    /// what it evaluates to‹). An explicit `return` is optional.
    Open,
    /// A closing bracket `)`: ends the current scope's body.
    Close,
    /// The `struct` keyword: parse the following `( field-list )` into a struct
    /// node. The field list is a bespoke sub-parse ([`Parser::parse_struct`]),
    /// because a field name is a *fresh* spelling the eager-resolve driver cannot
    /// resolve; it is reused for a function's parameter list (DESIGN ›A function's
    /// surface‹: the parameter list *is* a struct).
    Struct,
    /// A field separator `,`: ends the current field's type expression the way `)`
    /// ends a scope. The field-list parser consumes it; the generic driver treats
    /// it as a structural break.
    Separator,
    /// A declaration colon `:` in `name : type`: separates a field name from its
    /// type. v1 appears only inside a field list; the general declaration operator
    /// is later.
    Colon,
    /// The `fn` keyword: parse a function literal `fn ( params ) -> ret ( body )`
    /// (DESIGN ›A function's surface‹) via [`Parser::parse_fn`]. The parameter list
    /// is a `struct` (step 2's field list); the body is a `( )` scope with the
    /// parameters open.
    Fn,
    /// The return arrow `->` in a fn signature: separates the parameter list from
    /// the return type. Consumed by [`Parser::parse_fn`].
    Arrow,
}

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
}

/// Build a call node `{ty: callee, value: [args…, null]}`, the application
/// `callee(args)`. Like a binary operator's `{ty: op, value: [lhs, rhs]}`, a call's
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
    metas: &'a std::collections::HashMap<DyadPtr, Construct>,
    /// The core type handles the parser types opened nodes with (see [`CoreTypes`]).
    types: CoreTypes,
}

impl<'a> Parser<'a> {
    /// A parser over `source`, resolving against `scopes` and `metas`, allocating
    /// into `store`, and lexing via `trie`. `types` are the core handles the parser
    /// types the scopes and structs it opens with.
    pub fn new(
        source: &'a str,
        store: &'a mut Store,
        trie: &'a mut RegexTrie,
        metas: &'a std::collections::HashMap<DyadPtr, Construct>,
        types: CoreTypes,
        scopes: ScopeStack,
    ) -> Self {
        Parser { source, pos: 0, scopes, store, trie, metas, types }
    }

    /// Advance past ASCII whitespace.
    fn skip_ws(&mut self) {
        let bytes = self.source.as_bytes();
        while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// Consume the closing `)` that matches an opening `(`, or fail if the body
    /// ended at something else (or the end of input).
    fn expect_close(&mut self) -> Result<(), ParseError> {
        self.skip_ws();
        let source = self.source;
        if self.pos >= source.len() {
            return Err(ParseError::UnclosedBracket);
        }
        let start = self.pos;
        let r = self
            .scopes
            .resolve(self.trie, &source[start..])
            .map_err(ParseError::Resolve)?;
        match self.metas.get(&r.identity).copied() {
            Some(Construct::Close) => {
                self.pos = start + r.matched;
                Ok(())
            }
            _ => Err(ParseError::UnclosedBracket),
        }
    }

    /// Peek the next token's `Construct` without consuming it. `None` at end of
    /// input or when the token is not a known structural identity: a fresh name
    /// resolves to nothing here and is read raw by the field-list parser instead.
    fn peek_kind(&mut self) -> Option<(DyadPtr, usize, Construct)> {
        self.skip_ws();
        let source = self.source;
        if self.pos >= source.len() {
            return None;
        }
        let r = self.scopes.resolve(self.trie, &source[self.pos..]).ok()?;
        let c = self.metas.get(&r.identity).copied()?;
        Some((r.identity, r.matched, c))
    }

    /// Consume the `(` that opens a field list, or fail.
    fn expect_open(&mut self) -> Result<(), ParseError> {
        match self.peek_kind() {
            Some((_, matched, Construct::Open)) => {
                self.pos += matched;
                Ok(())
            }
            _ => Err(ParseError::ExpectedOpen),
        }
    }

    /// Consume a `:` if the next token is one, reporting whether it was.
    fn consume_colon(&mut self) -> bool {
        match self.peek_kind() {
            Some((_, matched, Construct::Colon)) => {
                self.pos += matched;
                true
            }
            _ => false,
        }
    }

    /// Consume a `,` if the next token is one, reporting whether it was.
    fn consume_separator(&mut self) -> bool {
        match self.peek_kind() {
            Some((_, matched, Construct::Separator)) => {
                self.pos += matched;
                true
            }
            _ => false,
        }
    }

    /// Whether the next token is a closing `)` (peek, no consume).
    fn at_close(&mut self) -> bool {
        matches!(self.peek_kind(), Some((_, _, Construct::Close)))
    }

    /// Read a raw identifier `[A-Za-z_][A-Za-z0-9_]*` at the cursor, advancing past
    /// it, returning its `(start, len)`; `None` if the next non-space byte does not
    /// begin an identifier. Declaration position reads fresh names raw, since they
    /// are not yet in the name index to resolve (the sketch's `declare(name:string)`).
    fn lex_identifier(&mut self) -> Option<(usize, usize)> {
        self.skip_ws();
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
    /// in the struct's own scope. The node is
    /// `{ty: struct_type, value -> [scope, field0 … fieldN, null]}` (scope at index
    /// 0, null-terminated). Fresh field names are read raw here, which is why the
    /// field list needs its own sub-parse rather than the generic driver.
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
            self.scopes.declare(self.trie, name, field).map_err(ParseError::Resolve)?;
            fields.push(field);
            if !self.consume_separator() {
                break;
            }
        }

        self.scopes.pop();
        self.expect_close()?;

        let mut ops = Vec::with_capacity(fields.len() + 2);
        ops.push(scope);
        ops.extend_from_slice(&fields);
        ops.push(std::ptr::null_mut());
        let value = self.store.alloc_operands(&ops);
        Ok(self.store.alloc_raw(struct_type, value))
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
    pub fn parse_fn(&mut self, fn_type: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The parameter list is a struct; parse_struct opens and closes its scope.
        let input = self.parse_struct(self.types.struct_)?;
        self.expect_arrow()?;
        let output = self.parse_return_type()?;

        // Reopen the parameter scope (the input struct's `value[0]`) so the body
        // resolves parameters, then parse the `( body )`.
        // SAFETY: `input` is the struct just built; its `value[0]` is its scope.
        let scope = unsafe { *((*input).value as *const DyadPtr) };
        self.scopes.push(scope);
        self.expect_open()?;
        let body = self.parse_expression()?;
        self.expect_close()?;
        self.scopes.pop();

        // `bcode` starts null; `compile_fn` installs the exec@ into this slot.
        let value = self.store.alloc_operands(&[input, output, body, std::ptr::null_mut()]);
        Ok(self.store.alloc_raw(fn_type, value))
    }

    /// Consume the `->` that separates a fn's parameter list from its return type.
    fn expect_arrow(&mut self) -> Result<(), ParseError> {
        match self.peek_kind() {
            Some((_, matched, Construct::Arrow)) => {
                self.pos += matched;
                Ok(())
            }
            _ => Err(ParseError::ExpectedArrow),
        }
    }

    /// Parse a fn's return type: a single resolved type identity (`i32`, …). v1
    /// return types are one identity; compound type expressions arrive later.
    fn parse_return_type(&mut self) -> Result<DyadPtr, ParseError> {
        self.skip_ws();
        let source = self.source;
        if self.pos >= source.len() {
            return Err(ParseError::ExpectedReturnType);
        }
        let r = self.scopes.resolve(self.trie, &source[self.pos..]).map_err(ParseError::Resolve)?;
        self.pos += r.matched;
        Ok(r.identity)
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

    /// Parse one expression to a single dyad, consuming source from the current
    /// position. Each call drives its own tape, so a prefix constructor can parse
    /// its operand by calling this again (the parser is a service the constructors
    /// re-enter, per the sealed "constructors drive" model).
    pub fn parse_expression(&mut self) -> Result<DyadPtr, ParseError> {
        let mut tape = ParsingTape::new();
        loop {
            self.skip_ws();
            if self.pos >= self.source.len() {
                break;
            }
            let source = self.source;
            let start = self.pos;
            let r = self
                .scopes
                .resolve(self.trie, &source[start..])
                .map_err(ParseError::Resolve)?;
            let id = r.identity;
            let c = self.metas.get(&id).copied();

            // A structural delimiter (`)`, `,`, `:`, `->`) ends this
            // (sub-)expression; leave it unconsumed for the enclosing constructor
            // (the opener that started the scope, the field-list or fn parser).
            if matches!(
                c,
                Some(Construct::Close | Construct::Separator | Construct::Colon | Construct::Arrow)
            ) {
                break;
            }
            self.pos = start + r.matched;

            match c {
                // A plain operand: a reference to the resolved identity.
                None => tape.push(Cell::Dyad(id)),
                // A literal: build its leaf now from the matched span.
                Some(Construct::Atom(build)) => {
                    let span = &source[start..start + r.matched];
                    let dyad = build(self.store, id, span)?;
                    tape.push(Cell::Dyad(dyad));
                }
                // A prefix keyword: parse the rest of the expression as its
                // operand, then build. (v1 grabs to the end of the expression.)
                Some(Construct::Prefix(build)) => {
                    let operand = self.parse_expression()?;
                    let dyad = build(self.store, id, operand)?;
                    tape.push(Cell::Dyad(dyad));
                }
                // An opening bracket. If a reduced operand precedes it, this is a
                // call: `callee( args )` (juxtaposition, binding tightest). Else it
                // is a grouping scope whose value is its body.
                Some(Construct::Open) => {
                    if matches!(tape.last(), Some(Cell::Dyad(_))) {
                        let callee = tape.pop().and_then(|c| c.as_dyad()).unwrap();
                        let args = self.parse_arg_list()?;
                        self.expect_close()?;
                        let call = build_call(self.store, callee, &args);
                        tape.push(Cell::Dyad(call));
                    } else {
                        let body = self.parse_expression()?;
                        self.expect_close()?;
                        tape.push(Cell::Dyad(body));
                    }
                }
                // The `struct` keyword: parse its `( field-list )` into a struct
                // node (a bespoke sub-parse; fresh field names can't be resolved).
                Some(Construct::Struct) => {
                    let s = self.parse_struct(id)?;
                    tape.push(Cell::Dyad(s));
                }
                // The `fn` keyword: parse a `fn ( params ) -> ret ( body )` literal.
                Some(Construct::Fn) => {
                    let f = self.parse_fn(id)?;
                    tape.push(Cell::Dyad(f));
                }
                // An operator: reduce anything binding tighter to its left, then
                // shift it onto the tape as a pending token.
                Some(Construct::Infix { precedence, assoc, .. }) => {
                    self.reduce_pending(&mut tape, precedence, assoc)?;
                    tape.push(Cell::Token(Token { start, len: r.matched, identity: id }));
                }
                // Handled by the structural-break peek above.
                Some(
                    Construct::Close | Construct::Separator | Construct::Colon | Construct::Arrow,
                ) => {
                    unreachable!("structural delimiter ends the loop")
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
            let lhs = match tape.cell(op_idx - 1).and_then(Cell::as_dyad) {
                Some(d) => d,
                None => break,
            };
            let rhs = match tape.cell(op_idx + 1).and_then(Cell::as_dyad) {
                Some(d) => d,
                None => break,
            };
            let (prev_prec, build) = match self.metas.get(&op_id).copied() {
                Some(Construct::Infix { precedence, build, .. }) => (precedence, build),
                _ => break,
            };
            if !(prev_prec > prec || (prev_prec == prec && assoc == Assoc::Left)) {
                break;
            }
            let dyad = build(self.store, op_id, lhs, rhs)?;
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
            let lhs =
                tape.cell(op_idx - 1).and_then(Cell::as_dyad).ok_or(ParseError::MissingOperand)?;
            let rhs =
                tape.cell(op_idx + 1).and_then(Cell::as_dyad).ok_or(ParseError::MissingOperand)?;
            let build = match self.metas.get(&op_id).copied() {
                Some(Construct::Infix { build, .. }) => build,
                _ => return Err(ParseError::Trailing),
            };
            let dyad = build(self.store, op_id, lhs, rhs)?;
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
