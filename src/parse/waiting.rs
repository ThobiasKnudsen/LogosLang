// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A line of a body whose value's type is known only when it runs, since it reads a `dyad`
//! name, or a `-> dyad` call as an infix's operand: found before it is lexed, held whole, and
//! parsed each time it runs over the nodes such values hold then (`identities::waiting`, ›A
//! `dyad ?` place is transparent: a placeholder for a new node of any type‹ [[R293]]). Held
//! whole, not parsed up to the value: stand-in for #280.

use super::*;
use crate::identities::read::{read_kind, Read};

/// What the scan of a line found: the first read that makes it wait, and the first word such a
/// line cannot hold.
#[derive(Default)]
struct Found {
    read: bool,
    refused: Option<(usize, String)>,
}

impl<'a> Parser<'a> {
    /// Where the line at the cursor ends when it waits for the run: a line of a body that reads
    /// a `dyad` name for its value, outside a `fn` or `type (…)` written in it. The cursor stays.
    /// A line that waits and holds `:=`, `return`, `free` or `move` is refused here:
    /// stand-in for #281.
    pub(super) fn waiting_line_end(&mut self) -> Result<Option<usize>, ParseError> {
        let in_run_order = self.cx.feed.as_ref().is_none_or(|f| f.in_run_order);
        let open = self.cx.open.last();
        if self.cx.runtime_depth == 0
            || !in_run_order
            || open.is_none_or(|o| o.list || o.arguments)
            || !self.cx.definitions.is_empty()
        {
            return Ok(None);
        }
        let start = self.cx.pos;
        let mut found = Found::default();
        let scanned = self.scan_level(0, &mut found);
        let end = self.cx.pos;
        self.cx.pos = start;
        // A line that does not scan is left to the parse to report where it fails.
        if scanned.is_err() || !found.read {
            return Ok(None);
        }
        if let Some((at, word)) = found.refused {
            self.cx.pos = at;
            return Err(ParseError::WaitingLineHolds(Box::new(word)));
        }
        Ok(Some(end))
    }

    /// One level of the line: at the top the line itself, up to its boundary, left unconsumed;
    /// below it the lines of a bracket, up to its closer, consumed. Whether a line of this level
    /// reads a `dyad` name; a `:=` on a line that does is refused, and at the top a `:=` at all.
    fn scan_level(&mut self, depth: usize, found: &mut Found) -> Result<bool, ParseError> {
        self.check_depth(depth)?;
        let types = self.types;
        let top = depth == 0;
        let bare_branch = top && self.cx.else_ends == Some(self.cx.open.len());
        let (mut any, mut line_reads) = (false, false);
        let mut declares: Option<usize> = None;
        let (mut prev, mut before_prev): (Option<DyadPtr>, Option<DyadPtr>) = (None, None);
        // After a `fn` or `type`, the rest of the level is a body read where it is built.
        let mut skipping = false;
        // A `-> dyad` function just scanned, with whether an infix stands before it; and whether
        // the bracket just closed was its call's.
        let (mut callee, mut after_call): (Option<bool>, bool) = (None, false);
        loop {
            self.skip_whitespace();
            let at = self.cx.pos;
            let ends_here = at >= self.cx.source.len();
            let r = if ends_here { None } else { Some(self.token_at(at, true)?) };
            let id = r.as_ref().map(|r| r.identity);
            let boundary = ends_here
                || id.is_some_and(|id| {
                    id == types.sep_ || self.is_closer(id) || (bare_branch && id == types.else_)
                });
            if boundary {
                if line_reads || top {
                    if let Some(at) = declares.take() {
                        found.refused.get_or_insert((at, ":=".into()));
                    }
                }
                if top {
                    return Ok(any);
                }
                let (Some(r), Some(id)) = (r, id) else { return Err(ParseError::UnclosedBracket) };
                self.cx.pos = at + r.matched;
                if self.is_closer(id) {
                    return Ok(any);
                }
                (line_reads, declares, prev, before_prev, skipping) =
                    (false, None, None, None, false);
                (callee, after_call) = (None, false);
                continue;
            }
            let (Some(r), Some(id)) = (r, id) else { unreachable!("a boundary at the end") };
            self.cx.pos = at + r.matched;
            if self.is_opener(id) {
                let inner = if skipping {
                    self.skip_bracket(id, depth + 1).map(|_| false)?
                } else {
                    self.scan_level(depth + 1, found)?
                };
                let call_operand = callee.take();
                after_call = call_operand == Some(false);
                let reads = inner || call_operand == Some(true);
                line_reads |= reads;
                any |= reads;
                found.read |= reads;
                (before_prev, prev) = (prev, Some(types.open_));
                continue;
            }
            if skipping {
                continue;
            }
            if id == types.fn_type || id == types.type_ {
                skipping = true;
                continue;
            }
            // A call that gives a `dyad`, read by an infix on either side.
            if std::mem::take(&mut after_call) && self.is_value_infix(id) {
                (line_reads, any, found.read) = (true, true, true);
            }
            // SAFETY: a resolved token's identity is a dyad from the store.
            callee = (prev != Some(types.dot_) && unsafe { self.gives_dyad(id) })
                .then(|| prev.is_some_and(|p| self.is_value_infix(p)));
            if id == types.return_ || id == types.free_ || id == types.move_ {
                let word = self.cx.source[at..at + r.matched].to_string();
                found.refused.get_or_insert((at, word));
            }
            if id == types.declare_tok {
                declares.get_or_insert(at);
            }
            // SAFETY: a resolved token's identity is a dyad from the store.
            let dyad_read = prev != Some(types.dot_)
                && unsafe { read_kind(types, id) } == Read::Container(types.dyad_);
            // SAFETY: as above.
            let target_holds_dyad = before_prev
                .is_some_and(|t| unsafe { read_kind(types, t) == Read::Container(types.dyad_) });
            if dyad_read && !self.reads_the_box(prev, target_holds_dyad, top, bare_branch) {
                (line_reads, any, found.read) = (true, true, true);
            }
            (before_prev, prev) = (prev, Some(id));
        }
    }

    /// Whether the `dyad` name just scanned, after `prev`, stands for its place rather than the
    /// node it holds: what `=` writes, `&` points at, `free` and `move` end, and a value taken
    /// whole by a `:=`, a `return`, an `=` into another `dyad` place or the line itself.
    fn reads_the_box(
        &mut self,
        prev: Option<DyadPtr>,
        target_holds_dyad: bool,
        top: bool,
        bare_branch: bool,
    ) -> bool {
        let types = self.types;
        let next = self.peek_token().map(|(id, _)| id);
        if next == Some(types.assign) {
            return true;
        }
        if prev.is_some_and(|p| {
            p == types.free_
                || p == types.move_
                || self.precedence_of_cell(p) == crate::identities::meta::prec::ADDRESS
        }) {
            return true;
        }
        let at_end = self.cx.pos >= self.cx.source.len()
            || next.is_some_and(|n| {
                n == types.sep_ || self.is_closer(n) || (bare_branch && n == types.else_)
            });
        at_end
            && match prev {
                None => top,
                Some(p) => {
                    p == types.declare_tok
                        || p == types.return_
                        || (p == types.assign && target_holds_dyad)
                }
            }
    }

    /// Whether `id` is a function whose result is a `dyad`, so the type of what its call gives
    /// is known only at run.
    ///
    /// # Safety
    /// `id` must be null or a dyad from the store.
    unsafe fn gives_dyad(&self, id: DyadPtr) -> bool {
        !id.is_null()
            && crate::dyad::ty(id) == self.types.fn_type
            && *(crate::dyad::value(id) as *const DyadPtr).add(FN_OUTPUT) == self.types.dyad_
    }

    /// The line at the cursor, ending at `end`, held as it is written: lexed against the scopes
    /// open here, its names checked as a use of them is, and parsed when it runs.
    pub(super) fn hold_line(&mut self, end: usize) -> Result<Cell, ParseError> {
        let types = self.types;
        let start = self.cx.pos;
        let end = start + self.cx.source[start..end].trim_end().len();
        let text = crate::identities::string::build_text(
            self.rt.store,
            types.string_,
            &self.cx.source.as_bytes()[start..end],
        );
        // SAFETY: `text` is the string node just built; its bytes live for the store.
        let held = unsafe { crate::identities::string::text(text) };
        let held = std::str::from_utf8(held).expect("copied from the source text");
        let fragment = lex_fragment(&self.cx.scopes, self.trie, self.rt.store, types, held)
            .map_err(|e| self.lex_error(start, e))?;
        for cell in fragment.held_cells() {
            let binding = cell.binding(types);
            if binding.is_null() {
                continue;
            }
            self.cx.pos = start + cell.start;
            // SAFETY: a resolved cell's identity is a dyad from the store.
            unsafe { self.check_capture(cell.identity(types))? };
            self.check_made(binding)?;
            self.note_outer_read(binding);
        }
        self.cx.pos = end;
        let cells = Box::into_raw(Box::new(fragment));
        let scope = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        let frames: Vec<DyadPtr> = self.cx.frames.iter().map(|f| f.frame).collect();
        let node = crate::identities::waiting::build_line(
            self.rt.store,
            types,
            text,
            cells,
            scope,
            &frames,
        );
        // SAFETY: `node` was just built in the store.
        Ok(unsafe { Cell::built(node) })
    }

    /// Parse `node`, which waits for the run, inside the run that reached it: a held line over
    /// the scopes open where it was written, its `dyad` names standing for the nodes they hold
    /// now. What it gives lands in its `into`.
    ///
    /// # Safety
    /// `node` must be a node `identities::waiting` built; the call it was written in must be the
    /// innermost activation.
    pub(super) unsafe fn parse_waiting(
        &mut self,
        node: DyadPtr,
    ) -> Result<DyadPtr, crate::run::RunError> {
        let types = self.types;
        let (text, cells, scope, open_fns, into) = crate::identities::waiting::line_parts(node);
        let bytes = crate::identities::string::text(text);
        let text: &'a str = std::str::from_utf8(bytes).expect("copied from the source text");
        let cells = (*cells).held_cells();
        let mut chain = Vec::new();
        let mut at = scope;
        while !at.is_null() {
            chain.push(at);
            at = crate::identities::scope::parent_of(at);
        }
        let mut nested = ScopeStack::new();
        for &scope in chain.iter().rev() {
            nested.push(scope);
        }
        let base_depth = nested.depth();
        // One frame per function open where the line was written, so a read of their names is
        // no capture and a name declared in the line is laid out in the program frame.
        let depth = open_fns.len();
        let frames = open_fns
            .iter()
            .map(|&f| OpenFn {
                frame: f,
                size: 0,
                below: 0,
                outer: Vec::new(),
                open_below: 0,
                returns: Vec::new(),
            })
            .collect();
        let cx = Context {
            frames,
            held_depth: depth,
            feed: Some(Feed { cells, next: 0, base_depth, in_run_order: true }),
            next_block: if into.is_null() { BlockValue::Thrown } else { BlockValue::Returned },
            ..Context::nested(&self.cx, text, nested)
        };
        let g = self.enter(cx);
        let built = g.0.parse_sequence();
        let built_pos = g.0.cx.pos;
        drop(g);
        let built = built.and_then(|line| {
            if into.is_null() {
                return Ok(line);
            }
            crate::identities::commit_fn_body(self.rt.store, types, line, into)
        });
        built.map_err(|e| {
            crate::run::RunError::WaitFailed(Box::new(crate::report::render(
                "held line",
                text,
                built_pos,
                &crate::report::parse_message(&e),
            )))
        })
    }
}
