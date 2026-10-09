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
        let path_text: String = if source[self.cx.pos..].starts_with('«') {
            let r = self.token_at(start, false)?;
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
    /// each `{…}` expression parsed here, in the scope the word appears in.
    fn interpolated_quote(&mut self, word: &'static str) -> Result<Vec<DyadPtr>, ParseError> {
        let (start, len) = self.quote_span_after(word)?;
        let quote = crate::identities::string::read(self.cx.source, start)
            .map_err(|e| self.lex_error(0, e))?;
        let mut parts = Vec::new();
        for piece in quote.pieces {
            match piece {
                Piece::Text(text) => parts.push(crate::identities::string::build_text(
                    self.rt.store,
                    self.types.string_,
                    &text,
                )),
                Piece::Scope(inner) => {
                    let part = self.parse_within(inner.start, inner.end)?;
                    // SAFETY: `part` is the reduced dyad just parsed.
                    if let Err(e) = unsafe { crate::identities::read::value_type(self.types, part) }
                    {
                        return Err(self.fail_at(inner.start, e));
                    }
                    parts.push(part);
                }
                Piece::StrayClose(at) => {
                    self.cx.pos = at;
                    return Err(ParseError::StrayInterpolationClose);
                }
            }
        }
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

    /// A quote's `{…}` scope, one expression over its interior `from..to`: in a body lexed
    /// once its cells are the ones lexed with the quote; where the quote is read from the
    /// source they are lexed now. The source is cut at `to` so the segment ends there, and
    /// offsets stay those of the whole line.
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
        let (start, len) = self.quote_span_after(word)?;
        self.construct_leaf(self.types.string_, start, len)?.ok_or(ParseError::ExpectedQuote(word))
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
        g.0.cx.open.push(OpenScope::default());
        let inner = g.0.run_imported();
        let inner_pos = g.0.cx.pos;
        let file = g.0.cx.open.pop().expect("pushed above");
        if inner.is_err() {
            g.0.end_after_fault(&file);
        } else {
            let program = g.0.cx.open.first_mut().expect("the program's scope is open");
            let from = program.lines + 1;
            program.exit.extend(
                file.exit.into_iter().filter(|h| h.end.is_none()).map(|h| ExitItem { from, ..h }),
            );
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

    /// The nested pass over an imported file: each statement pending, the
    /// file run at its end, collecting the `pub` (name, binding) pairs. On
    /// failure the message is returned with `offset` at the stuck point.
    fn run_imported(&mut self) -> Result<Vec<(String, DyadPtr)>, String> {
        let mut pubs = Vec::new();
        while let Some(item) = self.parse_next() {
            let node = item.map_err(|e| crate::report::parse_message(&e))?;
            // SAFETY: `node` is the line `parse_next` just returned.
            unsafe { self.close_item(node) };
            // SAFETY: `node` was just parsed into the store, which outlives the pass.
            unsafe {
                if dyad::ty(node) == self.types.declare_ {
                    let name = Binding::spelling(crate::identities::declare::binding_of(node));
                    let resolved = self
                        .cx
                        .scopes
                        .resolve(self.trie, &name)
                        .map_err(|_| format!("name `{name}` did not stay resolvable"))?;
                    if Binding::has_gate(resolved.binding, self.types.pub_) {
                        pubs.push((name, resolved.binding));
                    }
                }
            }
        }
        // A stray `)` ends the loop without being consumed, as in the drivers.
        if !self.cx.source[self.cx.pos..].trim_start().is_empty() {
            return Err("unexpected `)` — no scope is open here".to_string());
        }
        self.drain().map_err(|e| crate::report::parse_message(&e))?;
        Ok(pubs)
    }

    /// Pub-only exposure, the ordinary visibility rule: a collision with a
    /// live name is the ordinary shadowing error. Idempotent where the name
    /// already resolves to the same identity. The importer's name is the file's
    /// binding again: the same storage and the same gates (DESIGN ›Importing is
    /// dropping the text there, wrapped in its own scope‹).
    fn publish(&mut self, pubs: &[(String, DyadPtr)]) -> Result<(), ParseError> {
        // A collision is the import's: reported where the import stands.
        let at = self.cx.pos;
        for (name, exported) in pubs {
            // SAFETY: `exported` is a binding dyad the file's pass built.
            let (b, identity) = unsafe {
                let b = Binding::read(*exported);
                (b, b.names(*exported))
            };
            if let Ok(r) = self.cx.scopes.resolve(self.trie, name) {
                // SAFETY: both are dyads from the store; a storage name is the same name when
                // it is laid out over the same bytes.
                let same = unsafe {
                    r.identity == identity
                        || (self.types.frame_of(r.identity).is_some()
                            && self.types.frame_of(r.identity) == self.types.frame_of(identity))
                };
                if same {
                    continue;
                }
            }
            let binding = self.declare_name(name, identity, at)?;
            // SAFETY: `binding` was just declared; `exported` as above.
            unsafe {
                Binding::copy_gates(binding, *exported);
                if b.is_storage() {
                    Binding::lay_out(binding, b.dyad, b.frame, b.offset);
                }
            }
        }
        Ok(())
    }
}
