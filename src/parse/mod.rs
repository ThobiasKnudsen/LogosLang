// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The parser: one pass over the source, the scope's constructor lexing a segment and
//! constructing its cells highest parse_rank first (DESIGN ›The scope's constructor is the
//! driver‹). This file holds the parser and the context a sub-parse pushes; each job is the
//! file beside it: tape, scopes, error, lex, driver, operands, declare, fields, types, bodies, quotes.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::binding::Binding;
use crate::dyad::DyadPtr;
use crate::regex_trie::{RegexTrie, RegexTrieError};
use crate::store::Store;
use crate::Core;

mod bodies;
mod declare;
mod driver;
mod error;
mod fields;
mod lex;
mod operands;
mod quotes;
mod scopes;
mod tape;
mod types;
mod waiting;

pub use bodies::*;
pub use driver::*;
pub use error::*;
pub use lex::lex_fragment;
pub(crate) use lex::*;
pub use quotes::*;
pub use scopes::*;
pub use tape::*;
pub use types::*;
pub(crate) use waiting::HeldLine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
}

/// The one-pass elaborator: deferred reduction by parse_rank over the explicit
/// tape, not Pratt; operators wait on the tape until parse_rank says to
/// reduce them.
pub struct Parser<'a> {
    /// The sub-parse being driven now; `outer` holds the ones it interrupted, innermost last.
    cx: Context<'a>,
    outer: Vec<Context<'a>>,
    trie: &'a mut RegexTrie,
    types: &'a Core,
    imports: Imports,
    /// The pass's one runtime (DESIGN ›Build and run are one self-directing
    /// pass‹): one runtime keeps the frame arena and the allocation ledger
    /// whole across the pass.
    rt: crate::run::Runtime<'a>,
    /// The lowering table, when the driver attached one: handed to a nested
    /// import's runtime so `f.compile()` at an imported top level works as at
    /// the driver's own.
    lower: Option<&'a crate::compile::LowerTable>,
}

/// Everything a sub-parse (a held `type (…)`, a run body, an `import`) sets aside and puts
/// back: one value, pushed by [`Parser::enter`] and popped by its guard on every path.
struct Context<'a> {
    source: &'a str,
    pos: usize,
    scopes: ScopeStack,
    /// The binding of the declaration awaiting its value, or null: `parse_fn` points it at
    /// the `fn` node before the body parses, so a recursive self-call resolves its types.
    pending_binding: DyadPtr,
    /// The bindings of the declarations whose right side is being driven,
    /// innermost last: what a `lex_rank = …` line writes. A stand-in for a
    /// definition writing a binding field from inside `:=`.
    filling: Vec<DyadPtr>,
    /// Where each of those right sides starts in the source.
    filling_at: Vec<usize>,
    /// The binding of the last declaration that reduced: what a gate word to
    /// its left marks.
    last_declared: DyadPtr,
    /// The binding of the name left of the `.` being constructed, or null.
    member_root: DyadPtr,
    /// The bindings the member read under construction reaches, root first: what `.` writes
    /// onto the cell it places, for the write it may be the target of.
    member_path: Vec<DyadPtr>,
    /// The type and field parameters of the run body being constructed, and the `fn` node whose
    /// frame they lie in.
    run_fields: Option<(DyadPtr, Vec<DyadPtr>, DyadPtr)>,
    /// Open function frames, innermost last: empty at top level, where
    /// declarations get global storage; inside a function each local claims
    /// the next byte offset in the top frame.
    frames: Vec<OpenFn>,
    /// How many deferred-or-repeated bodies enclose the position, where parse order and run order
    /// do not coincide, so a part runs in the pass only by [`Parser::runs_in_pass`]. Comptime-taken
    /// `if` branches do not count.
    runtime_depth: u32,
    /// A `while` condition is being parsed, which runs again on every pass.
    each_pass: bool,
    /// That condition read a `dyad` name through to the node it holds now.
    each_pass_read: bool,
    /// The type definitions open around the position, innermost last; slot
    /// fills and fields write into the top.
    definitions: Vec<OpenType>,
    /// The open scopes, innermost last, the base entry the top level; its
    /// length is the bracket depth.
    open: Vec<OpenScope>,
    /// The folder a relative `file «…»` path resolves against: the reading file's own folder
    /// while a file is read as Logos, else the working directory.
    dir: PathBuf,
    /// The project's folder, which a `file «…»` stays inside: the folder of the file the command
    /// line read, which every file read from it keeps; `None` on the command line, which reaches
    /// any path.
    root: Option<PathBuf>,
    /// Body items a constructed segment yielded, in the order they are lines,
    /// not yet handed out.
    queued: std::collections::VecDeque<Cell>,
    /// The last `=` built, or held line that writes when it runs: what the sibling-write rule
    /// reads at the next item.
    last_write: LastWrite,
    /// While a held line is parsed up to a cut where an `=` stands: the source ends before that
    /// `=`, so what was read last is the place it writes.
    cut_before_assign: bool,
    /// Whether the constructor now running was woken at discovery, its token
    /// just lexed and the source after it unread, rather than at the boundary:
    /// an identity that reads its own bracket reads source only at discovery.
    discovering: bool,
    /// While `.` lexes the member right of a parse's `tape[0]`: the member is
    /// read by its spelling, so a raw-text word it happens to spell is not woken.
    member_asleep: bool,
    /// The identity whose constructor is running, whose lazy reads lex its right side.
    reader: DyadPtr,
    /// The stop mode of the segment being lexed, so a lazy read inside a
    /// constructor stops at the same boundaries the loop would.
    lex_mode: Option<RightSide>,
    /// Where a bracket of the line being parsed opens that holds a read of what a call that gives
    /// a `dyad` gives, still ahead: what stands before it runs first (`Parser::note_cuts`).
    cuts: Vec<usize>,
    /// Where the source the last such scan covered ends.
    cuts_end: usize,
    /// While an `if` reads a bare branch: the depth of the branch's own scope,
    /// where `else` and `,` end it; a nested bracket reads them as usual.
    else_ends: Option<usize>,
    /// The held run body whose cells the driver is reading now, if any.
    feed: Option<Feed>,
    /// The part of the held line being parsed that was parsed where it is written, which its
    /// first segment starts from.
    kept: Option<waiting::Kept>,
    /// While a held `type (…)` is built at its run, the function frames open
    /// where it was written: a place declared at that depth is the type's, global.
    held_depth: usize,
    /// A cell the branch about to open is checked against a type, with that type.
    narrow_next: Option<(CellKey, DyadPtr)>,
    /// While a `static` line of a type body is read, the frame depth a
    /// `fn` written as the member's value opens at: that `fn` takes the value first.
    member_fn_depth: Option<usize>,
    /// While a `static` name's value or an `immediate` operand parses: the frame depth
    /// it stands at. It runs once, at the definition, so its places are global
    /// and no call's slot is in reach (DESIGN ›Two muts, and the storage partition‹).
    once_at: Option<usize>,
    /// While a type body's field declaration reads its type: a hole there names the
    /// field's type and needs no place, so a type without one, `square_brackets ?`, may stand.
    field_hole: bool,
    /// While a function's signature reads the `T` of `-> T`, built included: `own` there marks
    /// the result.
    result_type: bool,
    /// Every held name a `move` or `free` ended, with the index of the open scope that held
    /// it: an `if` reads which of its arms ended which.
    ended_log: Vec<(DyadPtr, usize)>,
    /// While a branch the pass drops is parsed only to find where it ends: it never runs, so
    /// it ends no name.
    dropping: usize,
    /// What the construct around the block about to open does with its value.
    next_block: bodies::BlockValue,
    /// The construct around the block about to open starts its line, so the block's value is
    /// the line's, unless a reader to its right takes it.
    next_passes_out: bool,
    /// The tape the innermost scope's line being read is built on, compared by address only: a
    /// reader's right side is built on a tape of its own.
    line_tape: *const tape::ParsingTape,
    /// The block that closed last, with the values its last line made that it handed on as its
    /// own value.
    handed_tail: Option<(DyadPtr, Vec<DyadPtr>)>,
    /// Where the runtime's values of the line that gave the program its value begin, kept to
    /// their line's end until its echo has read them.
    tail_values: Option<usize>,
    /// The line that gave the program its value, with the value.
    program_value: Option<(DyadPtr, i64)>,
    /// Each `( … )` cell closed with a line refused as a line of a scope, where that line starts
    /// and why: the line the cell stands in decides once it closes.
    held_back: Vec<(DyadPtr, usize, ParseError)>,
}

impl<'a> Context<'a> {
    fn top(source: &'a str, scopes: ScopeStack) -> Self {
        Context {
            source,
            pos: 0,
            scopes,
            pending_binding: std::ptr::null_mut(),
            filling: Vec::new(),
            filling_at: Vec::new(),
            last_declared: std::ptr::null_mut(),
            member_root: std::ptr::null_mut(),
            member_path: Vec::new(),
            run_fields: None,
            frames: Vec::new(),
            runtime_depth: 0,
            each_pass: false,
            each_pass_read: false,
            definitions: Vec::new(),
            open: vec![OpenScope::top_level()],
            dir: PathBuf::from("."),
            root: None,
            queued: std::collections::VecDeque::new(),
            last_write: LastWrite::NONE,
            cut_before_assign: false,
            discovering: false,
            member_asleep: false,
            reader: std::ptr::null_mut(),
            lex_mode: None,
            cuts: Vec::new(),
            cuts_end: 0,
            else_ends: None,
            feed: None,
            kept: None,
            held_depth: 0,
            narrow_next: None,
            member_fn_depth: None,
            once_at: None,
            field_hole: false,
            result_type: false,
            ended_log: Vec::new(),
            dropping: 0,
            next_block: bodies::BlockValue::Used,
            next_passes_out: false,
            line_tape: std::ptr::null(),
            handed_tail: None,
            tail_values: None,
            program_value: None,
            held_back: Vec::new(),
        }
    }

    /// A sub-parse over `source`: fresh state, keeping only what the interrupted parse
    /// leaves in force across it.
    fn nested(outer: &Self, source: &'a str, scopes: ScopeStack) -> Self {
        Context {
            dir: outer.dir.clone(),
            root: outer.root.clone(),
            held_depth: outer.held_depth,
            last_declared: outer.last_declared,
            last_write: outer.last_write,
            reader: outer.reader,
            ..Context::top(source, scopes)
        }
    }
}

/// The last `=` node built, or held line that writes, the scope it was written in, the place it
/// writes as `=` read it, and the name and the field's binding that place reached when it was a
/// `v.f` over a name (nulls otherwise).
#[derive(Clone, Copy)]
struct LastWrite {
    node: DyadPtr,
    scope: DyadPtr,
    place: DyadPtr,
    fill: (DyadPtr, DyadPtr),
}

impl LastWrite {
    const NONE: LastWrite = LastWrite {
        node: std::ptr::null_mut(),
        scope: std::ptr::null_mut(),
        place: std::ptr::null_mut(),
        fill: (std::ptr::null_mut(), std::ptr::null_mut()),
    };
}

/// Puts back the context a [`Parser::enter`] set aside, by return or by unwinding.
struct Entered<'p, 'a>(&'p mut Parser<'a>);

impl Drop for Entered<'_, '_> {
    fn drop(&mut self) {
        let outer = self.0.outer.pop().expect("pushed by enter");
        let inner = std::mem::replace(&mut self.0.cx, outer);
        // A value the sub-parse ran early stands in what it built, which may run after it.
        let stood = inner.open.into_iter().flat_map(|mut o| o.take_stood());
        let open = self.0.cx.open.last_mut().expect("the root scope is open");
        open.stood.retain(|&n| self.0.rt.stands(n));
        open.stood.extend(stood);
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
            cx: Context::top(source, scopes),
            outer: Vec::new(),
            rt,
            trie,
            types,
            imports: Imports::default(),
            lower: None,
        }
    }

    /// Drive a sub-parse under `cx`; the guard puts the interrupted context back when it drops.
    fn enter(&mut self, cx: Context<'a>) -> Entered<'_, 'a> {
        let outer = std::mem::replace(&mut self.cx, cx);
        self.outer.push(outer);
        Entered(self)
    }

    /// So an imported file's top-level `f.compile()` runs under the nested
    /// pass exactly as under the driver.
    pub fn with_lower(mut self, lower: &'a crate::compile::LowerTable) -> Self {
        self.lower = Some(lower);
        self.rt.set_compiler(lower);
        self
    }

    /// Run an item now, a REPL line or a tail the pass never ran: the drivers'
    /// one read of an item's value.
    ///
    /// # Safety
    /// `node` must be a valid dyad this parser built into its store.
    pub unsafe fn value_of(&mut self, node: DyadPtr) -> Result<i64, ParseError> {
        self.run_on_pass(node).map_err(ParseError::Run)
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

    /// Whether the read in hand is a function's result type, the `T` of `-> T`.
    pub(crate) fn reads_result_type(&self) -> bool {
        self.cx.result_type
    }

    /// The REPL parses each line with a fresh `Parser` over one persistent
    /// scope stack.
    pub fn into_scopes(self) -> ScopeStack {
        self.cx.scopes
    }

    /// The pass's runtime, to run what was parsed: a fresh one would run the
    /// scope from its start.
    pub fn into_runtime(self) -> crate::run::Runtime<'a> {
        self.rt
    }
}

impl<'a> Parser<'a> {}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)] // a test reads the nodes it built a line above
mod tests;
