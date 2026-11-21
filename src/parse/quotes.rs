// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The words that read the `«…»` to their right (`regex`, `lex`, `print`, `abort`, `error`, `file`),
//! `here`, `caller`, and the two that read a file as Logos, `logos` and `import`.

use super::*;
use crate::dyad;
use crate::identities::drop_model::ExitItem;
use crate::identities::string::Piece;

/// The once-per-run registry of files read as Logos: a file loads once per run, every reader
/// sharing the one loaded section, and a path met again while its own load is in progress is a
/// cycle.
#[derive(Debug, Default)]
pub struct Imports {
    entries: HashMap<PathBuf, ImportState>,
    /// The section scope each loaded file's declarations landed in: a
    /// section's names live for the run, so a `pub` function's body may read
    /// a private sibling from anywhere (›Importing is dropping the text there, wrapped in its own
    /// scope‹ [[R306]]).
    pub(super) sections: HashSet<DyadPtr>,
}

#[derive(Debug)]
enum ImportState {
    /// Reading it as Logos again now is a cycle.
    Loading,
    /// `section` is the file's scope; `names` each name its lines declared, its spelling and its
    /// binding there, in declaration order.
    Loaded { section: DyadPtr, names: Vec<(String, DyadPtr)>, text: &'static str },
}

impl Imports {
    /// The text of every file loaded so far.
    pub fn sources(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.values().filter_map(|state| match state {
            ImportState::Loaded { text, .. } => Some(&text[..]),
            ImportState::Loading => None,
        })
    }

    /// The names the file whose scope `section` is declared; `None` for any other scope.
    pub(super) fn names_of(&self, section: DyadPtr) -> Option<&[(String, DyadPtr)]> {
        self.entries.values().find_map(|state| match state {
            ImportState::Loaded { section: s, names, .. } if *s == section => Some(&names[..]),
            _ => None,
        })
    }
}

/// A file a `file «…»` reached: its node, the path as written and where it is on this computer.
struct Reached {
    node: DyadPtr,
    path: String,
    canon: PathBuf,
}

impl<'a> Parser<'a> {
    pub(crate) fn construct_file(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if !self.finds_right_side(tape) {
            return Ok(Constructed::Decline);
        }
        let reached = self.reach_file()?;
        let file = self.reach(reached.node);
        tape.place(file);
        Ok(Constructed::Placed)
    }

    /// The `«…»` after `file`, an address written as plain text, and the file it names, read now
    /// (›`file «path»` and `url «address»` reach a file; `import` sets its `pub` names where it
    /// stands‹ [[R375]]): relative to the reading file's folder, and inside its project's folder
    /// wherever the reader is a file.
    fn reach_file(&mut self) -> Result<Reached, ParseError> {
        let (start, _) = self.quote_span_after("file")?;
        let quote = crate::identities::string::read(self.cx.source, start)
            .map_err(|e| self.lex_error(0, e))?;
        let fail = |p: &mut Self, e: ParseError| {
            p.cx.pos = start;
            Err(e)
        };
        if quote.pieces.iter().any(|piece| matches!(piece, Piece::Scope(_))) {
            return fail(self, ParseError::AddressNotPlain);
        }
        let path = String::from_utf8_lossy(&quote.text(self.cx.source)).into_owned();
        let joined = self.cx.dir.join(&path);
        let unread = |e: std::io::Error| ParseError::FileRead(format!("{}: {e}", joined.display()));
        let canon = match joined.canonicalize() {
            Ok(canon) => canon,
            Err(e) => return fail(self, unread(e)),
        };
        if let Some(root) = self.cx.root.as_ref().filter(|root| !canon.starts_with(root)) {
            let root = root.display().to_string();
            return fail(self, ParseError::FileOutsideProject { path, root });
        }
        let bytes = match std::fs::read(&canon) {
            Ok(bytes) => bytes,
            Err(e) => return fail(self, unread(e)),
        };
        let node = crate::identities::file::build(self.rt.store, self.types, &path, &bytes);
        Ok(Reached { node, path, canon })
    }

    /// The `file «…»` written after `logos` or `import`, read as Logos: the file reached, and the
    /// section it loaded into. Its address, the one thing it reads, is plain text, so it always
    /// runs in the pass, once per run, inside a body too; what stands before it runs first where
    /// the pass runs its line, and in a body only the lines before the body's.
    fn read_logos_after(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<(Reached, DyadPtr), ParseError> {
        let at = tape.at(0).map_or(self.cx.pos, |c| c.start);
        let file = self.types.file;
        let consumed = self.consume_token(file);
        debug_assert!(consumed, "`file` is the next word");
        let reached = self.reach_file()?;
        self.run_first(tape, at)?;
        let section = self.load_logos(&reached)?;
        Ok((reached, section))
    }

    /// `import file «…»`: the file read as Logos, its `pub` names set where the `import` stands,
    /// and `{type: import, value: [file, op]}` placed, whose run does nothing.
    pub(crate) fn construct_import(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if !self.finds_right_side(tape) {
            return Ok(Constructed::Decline);
        }
        self.skip_whitespace();
        let start = self.cx.pos;
        if self.next_identity() != Some(self.types.file) {
            self.cx.pos = start;
            return Err(ParseError::ImportTakesFile);
        }
        let (file, section) = self.read_logos_after(tape)?;
        let names = self.imports.names_of(section).expect("a loaded section").to_vec();
        // SAFETY: each name is a binding dyad the file's pass built.
        let pubs: Vec<_> =
            names.into_iter().filter(|&(_, b)| unsafe { self.read_gate(b) }.is_ok()).collect();
        self.publish(&pubs)?;
        let types = self.types;
        let node = self.rt.store.alloc_words(types.import_, &[file.node, types.ops.import_]);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// `logos file «…»`: the file read as Logos and its scope given, reached as a node, so its
    /// `pub` names are read with `.`. `None` where `file` does not follow, or `logos` was not
    /// woken where it is written.
    pub(crate) fn construct_logos_file(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Option<DyadPtr>, ParseError> {
        let start = self.cx.pos;
        if !self.cx.discovering || self.next_identity() != Some(self.types.file) {
            self.cx.pos = start;
            return Ok(None);
        }
        let (_, section) = self.read_logos_after(tape)?;
        Ok(Some(self.reach(section)))
    }

    /// `.name` over a file's scope: its name `name` where the gate read lets it through, used as
    /// a name is; `None` where `node` is no file's scope. So only its `pub` names are reached
    /// (›`logos` and `import` load a Logos file; whoever reads it reaches only its `pub` names‹
    /// [[R303]]).
    pub(super) fn file_member(
        &self,
        node: DyadPtr,
        name: &str,
    ) -> Option<Result<DyadPtr, ParseError>> {
        let names = self.imports.names_of(node)?;
        let Some(&(_, binding)) = names.iter().find(|(spelling, _)| spelling == name) else {
            return Some(Err(ParseError::NoPubName(Box::new(name.to_string()))));
        };
        // SAFETY: `binding` is a binding dyad the file's pass built.
        Some(unsafe { self.read_gate(binding) }.map(|()| binding))
    }

    /// Read the `«…»` to the right at discovery and place a `regex` node
    /// holding the pattern's bytes (DESIGN ›The scope's constructor is the
    /// driver‹). Compiled here, so a bad pattern errors at its quote, since the trie compiles lazily.
    pub(crate) fn construct_regex(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if !self.finds_right_side(tape) {
            return Ok(Constructed::Decline);
        }
        self.skip_whitespace();
        let source = self.cx.source;
        let start = self.cx.pos;
        if !source[start..].starts_with('«') {
            return Err(ParseError::ExpectedPattern);
        }
        let r = self.token_at(start, false)?;
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
        if !self.finds_right_side(tape) {
            return Ok(Constructed::Decline);
        }
        let quote = self.quote_after("lex")?;
        let node = crate::identities::lex::build(self.rt.store, self.types, quote);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    pub(crate) fn construct_print(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if !self.finds_right_side(tape) {
            return Ok(Constructed::Decline);
        }
        let parts = self.interpolated_quote("print")?;
        let node = crate::identities::print::build(self.rt.store, self.types, &parts);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    pub(crate) fn construct_abort(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if !self.finds_right_side(tape) {
            return Ok(Constructed::Decline);
        }
        let parts = self.interpolated_quote("abort")?;
        let node = crate::identities::abort::build(self.rt.store, self.types, &parts);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// `error`'s own parse (›Error values are in v0.1.0; an error names a category, which is a
    /// scoped name‹ [[R143]]): `.X := «description»` to its right declares the category `X`, and
    /// `.X «message»` raises it. Any other `.` is a read of `error` itself, left to `.`; anything
    /// else to its right names no category (›There is no bare `error «…»`; `abort «…»` stops the
    /// program; `alloc` returns `@T!`‹ [[R145]]).
    pub(crate) fn construct_error(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if !self.cx.discovering || tape.at(1).is_some() {
            return Ok(Constructed::Decline);
        }
        let after = self.cx.pos;
        let word = tape.at(0).map_or(after, |cell| cell.start);
        if !self.consume_token(self.types.dot_) {
            if !self.finds_right_side(tape) {
                return Ok(Constructed::Decline);
            }
            self.cx.pos = word;
            return Err(ParseError::ErrorNeedsCategory);
        }
        if self.consume_token(self.types.declare_tok) {
            self.cx.pos = word;
            return Err(ParseError::ErrorNeedsCategory);
        }
        if let Some((at, len)) = self.lex_spelling() {
            let name = format!("error.{}", &self.cx.source[at..at + len]);
            if self.consume_token(self.types.declare_tok) {
                return self.declare_category(tape, &name, word);
            }
            self.skip_whitespace();
            if self.cx.source[self.cx.pos..].starts_with('«') {
                return self.raise(tape, &name, word);
            }
        }
        self.cx.pos = after;
        Ok(Constructed::Decline)
    }

    /// `error.X := «description»`: the category is a name of the open scope, entered as every
    /// name is, under its whole path. Its record holds the description, and a `lex_rank` under
    /// every other, so no lex reads the path as one word and `error`'s parse alone reaches it.
    /// The line takes no gate word: its record is not offered to one.
    fn declare_category(
        &mut self,
        tape: &mut ParsingTape,
        name: &str,
        at: usize,
    ) -> Result<Constructed, ParseError> {
        let description = self.quote_after("error.X :=")?;
        let scope = self.cx.scopes.current().expect("a line stands in an open scope");
        let binding = self.mint_binding(description, scope, name.as_bytes());
        // SAFETY: `binding` was minted by `Binding::alloc` just above.
        unsafe { Binding::set_lex_rank(binding, f64::NEG_INFINITY) };
        self.enter_binding(name, binding, at)?;
        self.cx.last_declared = std::ptr::null_mut();
        let (store, types) = (&mut *self.rt.store, self.types);
        let node =
            crate::identities::declare::build(store, types, binding, description, description);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// `error.X «message»` over the category `name` a scope in reach declares.
    fn raise(
        &mut self,
        tape: &mut ParsingTape,
        name: &str,
        at: usize,
    ) -> Result<Constructed, ParseError> {
        let Some(category) = self.cx.scopes.named(self.trie, name) else {
            self.cx.pos = at;
            return Err(ParseError::UnknownCategory(Box::new(name.to_string())));
        };
        let parts = self.interpolated_quote("error")?;
        let node = crate::identities::error::build(self.rt.store, self.types, &parts, category);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// The `«…»` to the right of `print`, `abort` or a raise, read into its parts: text runs, and
    /// each `{…}` read here as a scope inside the one the word appears in. The parts are parts of
    /// the line the word stands in, as an argument list's lines are of the call's line: a point
    /// in a later part where the pass needs a value runs the earlier ones first, and each then
    /// stands for the value it gave (›The pass runs only as far as it must, in order, and never
    /// twice‹ [[R288]]).
    fn interpolated_quote(&mut self, word: &'static str) -> Result<Vec<DyadPtr>, ParseError> {
        let (start, len) = self.quote_span_after(word)?;
        let quote = crate::identities::string::read(self.cx.source, start)
            .map_err(|e| self.lex_error(0, e))?;
        self.cx.open.push(OpenScope::quote());
        let read = self.read_parts(quote.pieces);
        let mut closed = self.cx.open.pop().expect("pushed above");
        if let Err(e) = read {
            self.end_after_fault(&closed);
            return Err(e);
        }
        self.hand_on_notes(&mut closed);
        self.hand_on_held(&mut closed);
        let mut parts = closed.quote.take().expect("a quote's scope holds its parts");
        if parts.is_empty() {
            parts.push(crate::identities::string::build_text(
                self.rt.store,
                self.types.string_,
                b"",
            ));
        }
        self.cx.pos = start + len;
        Ok(parts)
    }

    /// The pieces of a quote, read into the open quote's scope, each a line of it: a text run as
    /// itself, and a `{…}` as the scope it opens, which the pass runs first where a later part
    /// needs it to, as it does a line of its own.
    fn read_parts(&mut self, pieces: Vec<Piece>) -> Result<(), ParseError> {
        for piece in pieces {
            let part = match piece {
                Piece::Text(text) => {
                    crate::identities::string::build_text(self.rt.store, self.types.string_, &text)
                }
                Piece::Scope(inner) => {
                    let Some(part) = self.parse_within(inner.start, inner.end)? else {
                        continue;
                    };
                    // A line that waits gives the quote a `dyad`, since its type is known only when
                    // it runs; the line the quote stands in frees that node at its end.
                    // SAFETY: `part` is the reduced dyad just parsed.
                    if unsafe {
                        crate::identities::waiting::lands_in(self.types, part, self.types.dyad_)
                    } {
                        self.note_made(part);
                    }
                    // SAFETY: as above.
                    if let Err(e) = unsafe { crate::identities::read::value_type(self.types, part) }
                    {
                        return Err(self.fail_at(inner.start, e));
                    }
                    if self.cx.runtime_depth == 0 && !self.cx.each_pass {
                        let open = self.cx.open.last_mut().expect("the quote's scope is open");
                        open.unrun.push(UnrunLine::new(part, open.lines));
                    }
                    part
                }
                Piece::StrayClose(at) => {
                    self.cx.pos = at;
                    return Err(ParseError::StrayInterpolationClose);
                }
            };
            let open = self.cx.open.last_mut().expect("the quote's scope is open");
            open.quote.as_mut().expect("the quote's scope holds its parts").push(part);
            // SAFETY: `part` is the part just read.
            unsafe { self.settle_line(part) };
        }
        Ok(())
    }

    /// A quote's `{…}`, a scope over its interior `from..to` (DESIGN ›`{ }` is text
    /// interpolation‹): its value, or none when it holds only prose and so adds no text. In a body
    /// lexed once its cells are the ones lexed with the quote; where the quote is read from the
    /// source they are lexed now. The source is cut at `to` so the scope ends there, and offsets
    /// stay those of the whole line.
    fn parse_within(&mut self, from: usize, to: usize) -> Result<Option<DyadPtr>, ParseError> {
        let whole = self.cx.source;
        self.cx.source = &whole[..to];
        self.cx.pos = from;
        // The line's scan read the quote as one token, so its interior is scanned on its own.
        let cuts = std::mem::take(&mut self.cx.cuts);
        let cuts_end = std::mem::replace(&mut self.cx.cuts_end, from);
        let parsed = self.parse_block(false, LinesEnd::TextEnd).map(|(scope, value)| {
            // SAFETY: `scope` is the scope node just built; its lines are reduced dyads.
            let lines = unsafe { crate::identities::scope::exprs_of(scope) }.unwrap_or_default();
            // SAFETY: as above.
            let prose =
                |&e: &DyadPtr| unsafe { crate::identities::numtype::is_comment_type(dyad::ty(e)) };
            (!lines.iter().all(prose)).then_some(value.dyad)
        });
        (self.cx.cuts, self.cx.cuts_end) = (cuts, cuts_end);
        self.cx.source = whole;
        parsed
    }

    /// The `«…»` string node to the right of a raw-text word, consumed at discovery.
    fn quote_after(&mut self, word: &'static str) -> Result<DyadPtr, ParseError> {
        let (start, len) = self.quote_span_after(word)?;
        self.construct_leaf(self.types.quote_, start, len)?.ok_or(ParseError::ExpectedQuote(word))
    }

    /// Where the `«…»` to the right of a raw-text word stands, consumed at discovery.
    fn quote_span_after(&mut self, word: &'static str) -> Result<(usize, usize), ParseError> {
        self.skip_whitespace();
        let source = self.cx.source;
        let start = self.cx.pos;
        if !source[start..].starts_with('«') {
            return Err(ParseError::ExpectedQuote(word));
        }
        let r = self.token_at(start, false)?;
        self.cx.pos += r.matched;
        Ok((start, r.matched))
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

    /// The file read as Logos, its section given. On a first load the file runs in its own
    /// section, a fresh stack of the root plus a section scope, so it sees ambient names and what
    /// it reads itself only (›Importing is dropping the text there, wrapped in its own scope‹
    /// [[R306]]); a file the command line reads roots its project there, and every file read from
    /// it keeps that root.
    fn load_logos(&mut self, file: &Reached) -> Result<DyadPtr, ParseError> {
        let Reached { node, path, canon } = file;
        match self.imports.entries.get(canon) {
            Some(ImportState::Loading) => return Err(ParseError::ImportCycle(path.clone())),
            Some(ImportState::Loaded { section, .. }) => return Ok(*section),
            None => {}
        }
        // SAFETY: `node` is the `file` node `reach_file` built.
        let text = std::str::from_utf8(unsafe { crate::identities::file::bytes(*node) })
            .map_err(|_| ParseError::FileRead(format!("{path}: it is not UTF-8 text")))?;
        // Sources are process-lived: every span and every later report
        // indexes into its file's text.
        let text: &'static str = Box::leak(text.to_owned().into_boxed_str());
        self.imports.entries.insert(canon.clone(), ImportState::Loading);

        // A fresh stack of the root (ambient names) plus a fresh scope node
        // the file's declarations land in.
        let root = *self.cx.scopes.open.first().expect("an import site has an open root scope");
        let mut nested = ScopeStack::new();
        nested.push(root);
        let section = nested.push_section(self.rt.store, self.types.scope);
        self.imports.sections.insert(section);

        let dir: PathBuf = canon.parent().map(Into::into).unwrap_or_else(|| PathBuf::from("."));
        let cx = Context {
            root: Some(self.cx.root.clone().unwrap_or_else(|| dir.clone())),
            dir,
            ..Context::nested(&self.cx, text, nested)
        };
        let g = self.enter(cx);
        // A file read as Logos is text dropped in place: it shares the reader's bracket stack, so
        // its `drain` runs the items in source order.
        // Its top level is a scope of its own, which lives as long as the program: what it
        // still holds at its end, and its `defer`s, join the program's exit.
        g.0.cx.open = std::mem::take(&mut g.0.outer.last_mut().expect("pushed by enter").open);
        g.0.cx.open.push(OpenScope::top_level());
        let inner = g.0.run_imported(section);
        let inner_pos = g.0.cx.pos;
        let top = g.0.cx.open.pop().expect("pushed above");
        if inner.is_err() {
            g.0.end_after_fault(&top);
        } else {
            let program = g.0.cx.open.first_mut().expect("the program's scope is open");
            let from = program.lines + 1;
            // A line that gave the file a value ended it: what its later lines hold never was.
            let ran = top.reached_lines();
            for item in top.exit.into_iter().filter(|h| h.end.is_none() && h.held_at(ran)) {
                program.imported.insert(item.what);
                program.exit.push(ExitItem { from, ..item });
            }
            program.held.extend(top.held);
        }
        g.0.outer.last_mut().expect("pushed by enter").open = std::mem::take(&mut g.0.cx.open);
        drop(g);

        match inner {
            Err(message) => {
                // Remove the Loading entry so a later attempt (a REPL retry)
                // reports the real failure, not a phantom cycle.
                self.imports.entries.remove(canon);
                Err(ParseError::ImportFailed {
                    path: path.clone(),
                    rendered: crate::report::render(path, text, inner_pos, &message),
                })
            }
            Ok(names) => {
                let loaded = ImportState::Loaded { section, names, text };
                self.imports.entries.insert(canon.clone(), loaded);
                Ok(section)
            }
        }
    }

    /// The nested pass over an imported file, whose lines land in `section`: the file read
    /// and run, then the (name, binding) pairs its lines declared, in declaration order. On
    /// failure the message is returned with `offset` at the stuck point.
    fn run_imported(&mut self, section: DyadPtr) -> Result<Vec<(String, DyadPtr)>, String> {
        let message = |e: ParseError| crate::report::parse_message(&e);
        self.read_lines(LinesEnd::TextEnd).map_err(message)?;
        self.drain().map_err(message)?;
        let types = self.types;
        // SAFETY: `section` is the file's scope node, its lines nodes from the store; a declare
        // line holds the binding it declared.
        let names = unsafe {
            crate::identities::scope::exprs_of(section)
                .unwrap_or(&[])
                .iter()
                .filter(|&&line| dyad::ty(line) == types.declare_)
                .map(|&line| crate::identities::declare::binding_of(line))
                .map(|binding| (Binding::spelling(binding), binding))
                .collect()
        };
        Ok(names)
    }

    /// Pub-only exposure, the ordinary visibility rule: a collision with a
    /// live name is the ordinary shadowing error. Idempotent where an open scope already
    /// holds the name, or a live name for the same identity. The importer's name is a copy of
    /// the file's binding record in the importer's scope that holds only its scope and line, so
    /// an end or a `lex_rank` written anywhere is the name's everywhere
    /// (›Importing is dropping the text there, wrapped in its own scope‹ [[R306]]).
    fn publish(&mut self, pubs: &[(String, DyadPtr)]) -> Result<(), ParseError> {
        // A collision is the import's: reported where the import stands.
        let at = self.cx.pos;
        for (name, exported) in pubs {
            // SAFETY: `exported` is a binding dyad the file's pass built, and every binding the
            // trie or the scopes hand back is one from the store.
            let same = unsafe {
                let origin = Binding::origin_of(*exported);
                let key = Binding::read(*exported).key(name);
                let held = self.trie.bindings_for_key(&key).is_some_and(|bindings| {
                    bindings.iter().any(|&r| {
                        self.cx.scopes.is_open(Binding::read(r).scope)
                            && Binding::origin_of(r) == origin
                    })
                });
                let names = |r: DyadPtr| Binding::read(r).names(r);
                let identity = names(*exported);
                held || self
                    .cx
                    .scopes
                    .live(self.trie, name, *exported)
                    .ok()
                    .flatten()
                    .is_some_and(|r| names(r) == identity)
            };
            if same {
                continue;
            }
            // SAFETY: as above.
            let binding =
                unsafe { Binding::copy_of(self.rt.store, self.types.binding_, *exported) };
            self.enter_binding(name, binding, at)?;
        }
        Ok(())
    }
}
