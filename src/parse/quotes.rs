// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The words that read the `«…»` to their right (`regex`, `lex`, `print`, `error`), `here`,
//! `caller`, and `import`, which reads a file as text dropped in place.

use super::*;
use crate::dyad;
use crate::identities::drop_model::ExitItem;
use crate::identities::string::Piece;

/// The once-per-run import registry: a file loads once per run, every
/// importer sharing the one loaded section, and a path met again while its
/// own load is in progress is a cycle.
#[derive(Debug, Default)]
pub struct Imports {
    entries: HashMap<PathBuf, ImportState>,
    /// The section scope each loaded file's declarations landed in: a
    /// section's names live for the run, so a `pub` function's body may read
    /// a private sibling from anywhere (DESIGN ›Importing is dropping the text there‹).
    pub(super) sections: HashSet<DyadPtr>,
}

#[derive(Debug)]
enum ImportState {
    /// Importing it again now is a cycle.
    Loading,
    /// The `pub` names in declaration order, each with the identity it
    /// resolves to inside the file.
    /// `pubs`: each `pub` name's spelling and its binding in the file's scope.
    Loaded { pubs: Vec<(String, DyadPtr)>, text: &'static str },
}

impl Imports {
    /// The text of every file loaded so far.
    pub fn sources(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.values().filter_map(|state| match state {
            ImportState::Loaded { text, .. } => Some(&text[..]),
            ImportState::Loading => None,
        })
    }
}

impl<'a> Parser<'a> {
    /// Consume the path token (raw text up to whitespace or `,`, or a `«…»`
    /// string), load the file, and place `{type: import, value: [path, op]}`.
    /// The load happens here, once per run; the node's run does nothing.
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
        let path_text: String = match self.raw_path_end(start) {
            Some(end) => {
                self.cx.pos = end;
                source[start..end].to_string()
            }
            None => {
                let r = self.token_at(start, false)?;
                self.cx.pos += r.matched;
                let node = self
                    .construct_leaf(r.identity, start, r.matched)?
                    .ok_or(ParseError::ExpectedPath)?;
                // SAFETY: the leaf just built is a string node.
                let text = unsafe { crate::identities::string::text(node) };
                String::from_utf8_lossy(text).into_owned()
            }
        };
        if path_text.is_empty() {
            self.cx.pos = start;
            return Err(ParseError::ExpectedPath);
        }
        // The load runs the file now, so what stands before the `import` runs first.
        let at = tape.at(0).map_or(start, |c| c.start);
        self.run_first(tape, at)?;
        self.import_file(&path_text)?;
        let types = self.types;
        let path_node = crate::identities::string::build_text(
            self.rt.store,
            types.string_,
            path_text.as_bytes(),
        );
        let ops = [path_node, types.ops.import_];
        let node = self.rt.store.alloc_words(types.import_, &ops);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Where the path an `import` reads from `start` ends, where it is written raw: at whitespace,
    /// a `,` or the text's end. `None` for a `«…»` string, which is one token.
    pub(super) fn raw_path_end(&self, start: usize) -> Option<usize> {
        let rest = &self.cx.source[start..];
        if rest.starts_with('«') {
            return None;
        }
        let len = rest.find(|c: char| c.is_ascii_whitespace() || c == ',').unwrap_or(rest.len());
        Some(start + len)
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

    /// The `«…»` to the right of `print` or `error`, read into its parts: text runs, and
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

    /// On a first load the file runs in its own section, a fresh stack of the
    /// root plus a section scope, so it sees ambient names and its own imports
    /// only (DESIGN ›Importing is dropping the text there‹); its `pub` names then land here.
    fn import_file(&mut self, path_text: &str) -> Result<(), ParseError> {
        let joined = self.cx.dir.join(path_text);
        let canon = joined
            .canonicalize()
            .map_err(|e| ParseError::ImportRead(format!("{}: {e}", joined.display())))?;
        match self.imports.entries.get(&canon) {
            Some(ImportState::Loading) => {
                return Err(ParseError::ImportCycle(path_text.to_string()))
            }
            Some(ImportState::Loaded { pubs, .. }) => {
                let pubs = pubs.clone();
                self.publish(&pubs)?;
                return Ok(());
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
        // Its top level is a scope of its own, which lives as long as the program: what it
        // still holds at its end, and its `defer`s, join the program's exit.
        g.0.cx.open = std::mem::take(&mut g.0.outer.last_mut().expect("pushed by enter").open);
        g.0.cx.open.push(OpenScope::top_level());
        let inner = g.0.run_imported(section);
        let inner_pos = g.0.cx.pos;
        let file = g.0.cx.open.pop().expect("pushed above");
        if inner.is_err() {
            g.0.end_after_fault(&file);
        } else {
            let program = g.0.cx.open.first_mut().expect("the program's scope is open");
            let from = program.lines + 1;
            // A line that gave the file a value ended it: what its later lines hold never was.
            let ran = file.reached_lines();
            for item in file.exit.into_iter().filter(|h| h.end.is_none() && h.held_at(ran)) {
                program.imported.insert(item.what);
                program.exit.push(ExitItem { from, ..item });
            }
            program.held.extend(file.held);
        }
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
            Ok(pubs) => {
                self.imports
                    .entries
                    .insert(canon, ImportState::Loaded { pubs: pubs.clone(), text });
                self.publish(&pubs)?;
                Ok(())
            }
        }
    }

    /// The nested pass over an imported file, whose lines land in `section`: the file read
    /// and run, then its `pub` (name, binding) pairs in declaration order. On failure the
    /// message is returned with `offset` at the stuck point.
    fn run_imported(&mut self, section: DyadPtr) -> Result<Vec<(String, DyadPtr)>, String> {
        let message = |e: ParseError| crate::report::parse_message(&e);
        self.read_lines(LinesEnd::TextEnd).map_err(message)?;
        self.drain().map_err(message)?;
        let types = self.types;
        // SAFETY: `section` is the file's scope node, its lines nodes from the store; a declare
        // line holds the binding it declared.
        let pubs = unsafe {
            crate::identities::scope::exprs_of(section)
                .unwrap_or(&[])
                .iter()
                .filter(|&&line| dyad::ty(line) == types.declare_)
                .map(|&line| crate::identities::declare::binding_of(line))
                .filter(|&binding| Binding::has_gate(binding, types.pub_))
                .map(|binding| (Binding::spelling(binding), binding))
                .collect()
        };
        Ok(pubs)
    }

    /// Pub-only exposure, the ordinary visibility rule: a collision with a
    /// live name is the ordinary shadowing error. Idempotent where an open scope already
    /// holds the name, or a live name for the same identity. The importer's name is a copy of
    /// the file's binding record in the importer's scope, kept in step with it, so an end
    /// anywhere ends the name everywhere
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
            let binding = unsafe {
                Binding::copy_of(self.rt.store, self.types.binding_, self.types.array_, *exported)
            };
            self.enter_binding(name, binding, at)?;
        }
        Ok(())
    }
}
