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

mod tape;
pub use tape::*;
mod scopes;
pub use scopes::*;
mod error;
pub use error::*;
mod lex;
pub(crate) use lex::*;
mod driver;
pub use driver::*;
mod declare;
mod fields;
mod operands;
mod types;
pub use types::*;
mod bodies;
pub use bodies::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
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
    /// The placeholder of the declaration awaiting its value, or null:
    /// `parse_fn` publishes the signature onto it before the body parses, so
    /// a recursive self-call resolves its types.
    pending_fn: DyadPtr,
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
    /// The type and field parameters of the run body being constructed.
    run_fields: Option<(DyadPtr, Vec<DyadPtr>)>,
    /// Open function frames, innermost last: empty at top level, where
    /// declarations get global storage; inside a function each local claims
    /// the next byte offset in the top frame.
    frames: Vec<OpenFn>,
    /// How many deferred-or-repeated bodies enclose the position, where parse
    /// order and run order do not coincide; comptime effects that rebind names
    /// at parse are rejected while non-zero. Comptime-taken `if` branches do not count.
    runtime_depth: u32,
    /// The type definitions open around the position, innermost last; slot
    /// fills and fields write into the top.
    definitions: Vec<OpenType>,
    /// The open scopes, innermost last, the base entry the top level; its
    /// length is the bracket depth.
    open: Vec<OpenScope>,
    /// The folder relative import paths resolve against: the importing file's
    /// own folder during a nested import, else the working directory.
    dir: PathBuf,
    /// Comment cells lifted out of a segment at its boundary, with the offset
    /// each was lexed at, handed out as body items in source order beside the
    /// segment's expression.
    lifted: Vec<(usize, DyadPtr)>,
    /// Body items a constructed segment yielded, in source order, not yet
    /// handed out.
    queued: std::collections::VecDeque<Cell>,
    /// The last `=` built: what the sibling-write rule reads at the next item.
    last_write: LastWrite,
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
    /// While an `if` reads a bare branch: the depth of the branch's own scope,
    /// where `else` and `,` end it; a nested bracket reads them as usual.
    else_ends: Option<usize>,
    /// The held run body whose cells the driver is reading now, if any.
    feed: Option<Feed>,
    /// While a held `type (…)` is built at its run, the function frames open
    /// where it was written: a place declared at that depth is the type's, global.
    held_depth: usize,
    /// A cell the branch about to open is checked against a type, with that type.
    narrow_next: Option<(CellKey, DyadPtr)>,
    /// While a `share` line of a type body is read, the frame depth a
    /// `fn` written as the member's value opens at: that `fn` takes the value first.
    member_fn_depth: Option<usize>,
    /// While the initializer of a `share` name parses: the frame depth it
    /// stands at. It runs once, at the definition, so its places are global
    /// and no call's slot is in reach (DESIGN ›Two muts, and the storage partition‹).
    share_init: Option<usize>,
    /// While a type body's field declaration reads its type: a hole there names the
    /// field's type and needs no place, so a type without one, `square_brackets ?`, may stand.
    field_hole: bool,
}

impl<'a> Context<'a> {
    fn top(source: &'a str, scopes: ScopeStack) -> Self {
        Context {
            source,
            pos: 0,
            scopes,
            pending_fn: std::ptr::null_mut(),
            filling: Vec::new(),
            filling_at: Vec::new(),
            last_declared: std::ptr::null_mut(),
            member_root: std::ptr::null_mut(),
            member_path: Vec::new(),
            run_fields: None,
            frames: Vec::new(),
            runtime_depth: 0,
            definitions: Vec::new(),
            open: vec![OpenScope::default()],
            dir: PathBuf::from("."),
            lifted: Vec::new(),
            queued: std::collections::VecDeque::new(),
            last_write: LastWrite::NONE,
            discovering: false,
            member_asleep: false,
            reader: std::ptr::null_mut(),
            lex_mode: None,
            else_ends: None,
            feed: None,
            held_depth: 0,
            narrow_next: None,
            member_fn_depth: None,
            share_init: None,
            field_hole: false,
        }
    }

    /// A sub-parse over `source`: fresh state, keeping only what the interrupted parse
    /// leaves in force across it.
    fn nested(outer: &Self, source: &'a str, scopes: ScopeStack) -> Self {
        Context {
            dir: outer.dir.clone(),
            held_depth: outer.held_depth,
            last_declared: outer.last_declared,
            last_write: outer.last_write,
            reader: outer.reader,
            ..Context::top(source, scopes)
        }
    }
}

/// The last `=` node built, the scope it was built in, and the name and field it fills when
/// its target was a `v.f` the unwritten-fields rule noted (nulls otherwise).
#[derive(Clone, Copy)]
struct LastWrite {
    node: DyadPtr,
    scope: DyadPtr,
    fill: (DyadPtr, DyadPtr),
}

impl LastWrite {
    const NONE: LastWrite = LastWrite {
        node: std::ptr::null_mut(),
        scope: std::ptr::null_mut(),
        fill: (std::ptr::null_mut(), std::ptr::null_mut()),
    };
}

/// Puts back the context a [`Parser::enter`] set aside, by return or by unwinding.
struct Entered<'p, 'a>(&'p mut Parser<'a>);

impl Drop for Entered<'_, '_> {
    fn drop(&mut self) {
        let outer = self.0.outer.pop().expect("pushed by enter");
        self.0.cx = outer;
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

    /// The REPL parses each line with a fresh `Parser` over one persistent
    /// scope stack.
    pub fn into_scopes(self) -> ScopeStack {
        self.cx.scopes
    }
}

impl<'a> Parser<'a> {
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
        let bytes = self.cx.source.as_bytes();
        // Spaces (not the newline) may separate `#` from its text.
        while self.cx.pos < bytes.len() && matches!(bytes[self.cx.pos], b' ' | b'\t') {
            self.cx.pos += 1;
        }
        let source = self.cx.source;
        let text_node = if source[self.cx.pos..].starts_with('«') {
            // `# «…»`: the string form ends at the `»`, not the line.
            let r = self
                .cx
                .scopes
                .resolve(self.trie, &source[self.cx.pos..])
                .map_err(ParseError::Resolve)?;
            let start = self.cx.pos;
            self.cx.pos += r.matched;
            self.construct_leaf(r.identity, start, r.matched)?.ok_or(ParseError::BadLiteral)?
        } else {
            let start = self.cx.pos;
            while self.cx.pos < bytes.len() && bytes[self.cx.pos] != b'\n' {
                self.cx.pos += 1;
            }
            let text = source[start..self.cx.pos].trim_end();
            crate::identities::string::build_text(
                self.rt.store,
                self.types.string_,
                text.as_bytes(),
            )
        };
        Ok(self.rt.store.alloc_raw(self.types.comment_, text_node.cast()))
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
        if self.cx.runtime_depth != 0 {
            return Err(ParseError::ImportInRuntimeBody);
        }
        self.skip_whitespace();
        let source = self.cx.source;
        let start = self.cx.pos;
        let path_text: String = if source[self.cx.pos..].starts_with('«') {
            let r = self
                .cx
                .scopes
                .resolve(self.trie, &source[self.cx.pos..])
                .map_err(ParseError::Resolve)?;
            let s = self.cx.pos;
            self.cx.pos += r.matched;
            let node =
                self.construct_leaf(r.identity, s, r.matched)?.ok_or(ParseError::ExpectedPath)?;
            // SAFETY: the leaf just built is a string node.
            String::from_utf8_lossy(unsafe { crate::identities::string::text(node) }).into_owned()
        } else {
            let bytes = source.as_bytes();
            while self.cx.pos < bytes.len()
                && !bytes[self.cx.pos].is_ascii_whitespace()
                && bytes[self.cx.pos] != b','
            {
                self.cx.pos += 1;
            }
            source[start..self.cx.pos].to_string()
        };
        if path_text.is_empty() {
            self.cx.pos = start;
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
        let source = self.cx.source;
        let start = self.cx.pos;
        if !source[start..].starts_with('«') {
            return Err(ParseError::ExpectedPattern);
        }
        let r = self.cx.scopes.resolve(self.trie, &source[start..]).map_err(ParseError::Resolve)?;
        self.cx.pos += r.matched;
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
            self.cx.pos = start;
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
        let end = self.cx.pos;
        // SAFETY: `quote` is the string node the `«…»` constructor just built.
        let len = unsafe { crate::identities::string::text(quote) }.len();
        // The literal has no escapes, so its text is the source between the guillemets.
        let inner = end - 2 - len;
        let parts = self.quote_parts(inner, inner + len)?;
        self.cx.pos = end;
        Ok(parts)
    }

    /// A `print` or `error` quote's text runs and `{…}` expressions, each
    /// expression parsed here, in the scope the word appears in; `\{` and `\}`
    /// are the braces as text.
    fn quote_parts(&mut self, from: usize, to: usize) -> Result<Vec<DyadPtr>, ParseError> {
        let source = self.cx.source;
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
                    self.cx.pos = i;
                    return Err(ParseError::StrayInterpolationClose);
                }
                b'{' => {
                    let Some(close) = source[i + 1..to].find('}').map(|k| i + 1 + k) else {
                        self.cx.pos = i;
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
        let whole = self.cx.source;
        self.cx.source = &whole[..to];
        self.cx.pos = from;
        let parsed = self.parse_expression().and_then(|expr| {
            self.skip_whitespace();
            if self.cx.pos < to {
                Err(ParseError::Trailing)
            } else {
                Ok(expr)
            }
        });
        self.cx.source = whole;
        parsed
    }

    /// The `«…»` string node to the right of a raw-text word, consumed at discovery.
    fn quote_after(&mut self, word: &'static str) -> Result<DyadPtr, ParseError> {
        self.skip_whitespace();
        let source = self.cx.source;
        let start = self.cx.pos;
        if !source[start..].starts_with('«') {
            return Err(ParseError::ExpectedQuote(word));
        }
        let r = self.cx.scopes.resolve(self.trie, &source[start..]).map_err(ParseError::Resolve)?;
        self.cx.pos += r.matched;
        self.construct_leaf(r.identity, start, r.matched)?.ok_or(ParseError::ExpectedQuote(word))
    }

    /// The node placed at discovery carries the scope open at its appearance
    /// (DESIGN ›Meta-navigation‹): inside a constructor's body the definition
    /// site; the use site is `caller.scope`.
    pub(crate) fn construct_here(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let scope = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
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
        let joined = self.cx.dir.join(path_text);
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
        let root = *self.cx.scopes.open.first().expect("an import site has an open root scope");
        let mut nested = ScopeStack::new();
        nested.push(root);
        let section = nested.push_section(self.rt.store, self.types.scope);
        self.imports.sections.insert(section);

        let cx = Context {
            dir: canon.parent().map(Into::into).unwrap_or_else(|| PathBuf::from(".")),
            ..Context::nested(&self.cx, text, nested)
        };
        let g = self.enter(cx);
        // An import is text dropped in place (DESIGN ›Importing is dropping the text there‹):
        // it shares the importer's bracket stack, so its `drain` runs the items in source order.
        g.0.cx.open = std::mem::take(&mut g.0.outer.last_mut().expect("pushed by enter").open);
        let inner = g.0.run_imported();
        let inner_pos = g.0.cx.pos;
        g.0.outer.last_mut().expect("pushed by enter").open = std::mem::take(&mut g.0.cx.open);
        drop(g);

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
                        .cx
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
        if !self.cx.source[self.cx.pos..].trim_start().is_empty() {
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
        let at = self.cx.pos;
        for (name, identity) in pubs {
            if let Ok(r) = self.cx.scopes.resolve(self.trie, name) {
                if r.identity == *identity {
                    continue;
                }
            }
            self.declare_name(name, *identity, at)?;
        }
        Ok(())
    }
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
                a := i32 ?, output_type := type ?, share run = ( sq(a) ), \
                share parse_rank = *.parse_rank + 1, \
                share parse = ( tape[0]:type = squared, tape[0].a = tape[-1], tape[0].output_type = i32, \
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
