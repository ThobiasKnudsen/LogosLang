// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The scope stack and name resolution over it: the chain of enclosing scopes read off
//! the graph, with the O(1) open set (DESIGN ›Meta-navigation walks the graph‹); the trie is
//! only the name index.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// The spelling matched only a fresh-spelling pattern; only
    /// `ScopeStack::lex` ever answers `true`.
    pub fresh: bool,
    /// Bytes consumed from the start of the input.
    pub matched: usize,
    /// What a use of the name points at (DESIGN ›The dyad's read surface‹).
    pub binding: DyadPtr,
    /// The binding's dyad.
    pub identity: DyadPtr,
    /// The scope the winning declaration was made in: what a rebind that
    /// completes it must target.
    pub scope: DyadPtr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// Not in the name index at all; carries the leading word at the position.
    Unknown(String),
    /// Known, but no declaration of it is in an open scope.
    OutOfScope(String),
    /// Declaring it would shadow a declaration still live in an open scope.
    Shadowed(String),
    /// Declared in an open scope but made dead by an `own` or `drop`; only
    /// `:=` may follow (DESIGN ›Name resolution is scope-filtered‹).
    Dead(String),
    /// Two spellings of equal `lex_rank` match the same length: an
    /// inconsistency in the definitions, never a pick by declaration order.
    Tied,
    /// The name index itself rejected the lookup.
    Index(RegexTrieError),
}

/// The reader's best guess at what the writer meant as one name: the leading
/// word run, or the first character.
fn unknown_spelling(text: &str) -> String {
    let word = text.split(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("");
    if word.is_empty() {
        text.chars().next().map(String::from).unwrap_or_default()
    } else {
        word.to_string()
    }
}

/// One act on the name index since the last commit, undone newest-first by
/// rollback.
#[derive(Debug)]
enum Journal {
    /// Rollback removes the live entry.
    Declared { name: String, scope: DyadPtr },
    /// Rollback restores the `end` the binding had before.
    Ended { binding: DyadPtr, prev_end: DyadPtr },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    Start,
    End,
}

/// A range endpoint waiting for the body item that carries it: the entry holds
/// a provisional value until the declaring scope appends the finished item.
#[derive(Debug)]
struct Pending {
    /// Its own `scope` says which scope's next item settles it.
    binding: DyadPtr,
    endpoint: Endpoint,
}

/// A bare name an `own` or `drop` is about to make dead, handed back so the
/// keyword's constructor can mark it dead with the node it built.
#[derive(Debug)]
pub(crate) struct Ended {
    pub(crate) binding: DyadPtr,
}

/// The open scopes with an O(1) membership set: a cache over the parent link
/// every scope node carries (DESIGN ›Meta-navigation‹). Resolution keeps the
/// one live candidate (DESIGN ›Name resolution is scope-filtered‹).
#[derive(Debug)]
pub struct ScopeStack {
    pub(super) open: Vec<DyadPtr>,
    set: HashSet<DyadPtr>,
    /// The REPL's undo log: a failed line rolls its declarations and dead
    /// marks back, so a typo never burns a name for the session.
    journal: Vec<Journal>,
    pending: Vec<Pending>,
    /// Stack depths where a body that runs again or later begins; `own`/`drop`
    /// of a name declared below one is refused (DESIGN ›Memory and concurrency‹).
    barriers: Vec<usize>,
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

impl ScopeStack {
    pub fn new() -> Self {
        ScopeStack {
            open: Vec::new(),
            set: HashSet::new(),
            journal: Vec::new(),
            pending: Vec::new(),
            barriers: Vec::new(),
        }
    }

    pub fn push(&mut self, scope: DyadPtr) {
        self.open.push(scope);
        self.set.insert(scope);
    }

    /// Open a section under the innermost open scope: its parent link is that
    /// scope, so a walk up from a body constructed inside it reaches the root.
    pub fn push_section(&mut self, store: &mut Store, scope_ty: DyadPtr) -> DyadPtr {
        let parent = *self.open.last().expect("a section opens under an open scope");
        let section = crate::identities::scope::mint(store, scope_ty, parent);
        self.push(section);
        section
    }

    /// Endpoints still pending for the scope can no longer settle and are
    /// dropped.
    pub fn pop(&mut self) -> Option<DyadPtr> {
        let s = self.open.pop()?;
        self.set.remove(&s);
        // SAFETY: every pending binding is a binding dyad from the store.
        self.pending.retain(|p| unsafe { Binding::read(p.binding).scope } != s);
        Some(s)
    }

    /// A body that runs again or later begins with the next scope pushed.
    pub fn push_barrier(&mut self) {
        self.barriers.push(self.open.len());
    }

    pub fn pop_barrier(&mut self) {
        self.barriers.pop();
    }

    /// A barrier began after `scope` was pushed, so `own`/`drop` of a name
    /// from it is refused here.
    pub fn crosses_barrier(&self, scope: DyadPtr) -> bool {
        let Some(idx) = self.position(scope) else {
            return false;
        };
        self.barriers.iter().any(|&b| b > idx)
    }

    pub fn position(&self, scope: DyadPtr) -> Option<usize> {
        self.open.iter().position(|&s| s == scope)
    }

    pub fn current(&self) -> Option<DyadPtr> {
        self.open.last().copied()
    }

    pub fn is_open(&self, scope: DyadPtr) -> bool {
        self.set.contains(&scope)
    }

    pub fn depth(&self) -> usize {
        self.open.len()
    }

    /// The REPL's recovery after an error skipped the balancing pops. The
    /// barriers of the bodies closed this way go too, or they would refuse
    /// `own`/`drop` for the rest of the session.
    pub fn truncate(&mut self, depth: usize) {
        while self.open.len() > depth {
            self.pop();
        }
        // A barrier lives exactly while the body scope it was pushed for is open.
        self.barriers.retain(|&b| b < depth);
    }

    /// A kept line: the undo log goes, and an endpoint still pending is
    /// dropped (the REPL settles the line's first, at `close_item`).
    pub fn commit(&mut self) {
        self.journal.clear();
        self.pending.clear();
    }

    /// A declaration is removed by spelling *and* declaring scope, so outer
    /// declarations of the same spelling stay.
    pub fn rollback(&mut self, trie: &mut RegexTrie) {
        while let Some(act) = self.journal.pop() {
            match act {
                Journal::Declared { name, scope } => {
                    // A failed removal means the entry was already pruned.
                    let _ = trie.remove(&name, scope);
                }
                Journal::Ended { binding, prev_end } => {
                    // SAFETY: a journalled binding is a binding dyad from the store.
                    unsafe { Binding::set_end(binding, prev_end) };
                }
            }
        }
        self.pending.clear();
    }

    pub fn resolve(&self, trie: &RegexTrie, name: &str) -> Result<Resolved, ResolveError> {
        self.select(trie, name, false)
    }

    /// As `resolve`, but the fresh-spelling patterns compete too: a spelling
    /// nothing declared answers with `fresh` set and the run's length.
    pub fn lex(&self, trie: &RegexTrie, text: &str) -> Result<Resolved, ResolveError> {
        self.select(trie, text, true)
    }

    /// The one selection rule (DESIGN ›The scope's constructor is the driver‹):
    /// a match ending between two word characters is no candidate; of the
    /// rest, highest `lex_rank` first, longest match at equal rank.
    fn select(
        &self,
        trie: &RegexTrie,
        text: &str,
        include_fresh: bool,
    ) -> Result<Resolved, ResolveError> {
        if text.is_empty() {
            return Err(ResolveError::Unknown(String::new()));
        }
        let matches = trie.get_all_matches(text).map_err(ResolveError::Index)?;
        let bytes = text.as_bytes();
        let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        // SAFETY: every pointer the trie stores is a binding dyad from the store.
        let fields = |r: DyadPtr| unsafe { Binding::read(r) };
        // One candidate per binding, at its longest match: an optional tail
        // reports every end, an alternation holds the binding on each path.
        let mut cands: Vec<(DyadPtr, usize, bool)> = Vec::new();
        let mut why_none: Option<ResolveError> = None;
        for m in &matches {
            let cuts_word =
                m.matched < bytes.len() && word(bytes[m.matched - 1]) && word(bytes[m.matched]);
            let fresh = crate::identities::fresh::is_fresh_key(m.regex_key);
            if m.matched == 0 || cuts_word || (fresh && !include_fresh) {
                continue;
            }
            // At the frontier "range covers the point" is exactly "not dead".
            // Two live bindings of one spelling are a field's or a slot word's,
            // declared against their siblings alone: the innermost open
            // scope's wins (DESIGN ›The constructor is a field‹).
            let binding = m
                .bindings
                .iter()
                .copied()
                .filter(|&r| self.is_open(fields(r).scope) && !fields(r).is_dead())
                .max_by_key(|&r| self.position(fields(r).scope));
            let Some(binding) = binding else {
                let spelling = text[..m.matched].to_string();
                if m.bindings.iter().any(|&r| self.is_open(fields(r).scope)) {
                    why_none = Some(ResolveError::Dead(spelling));
                } else if why_none.is_none() {
                    why_none = Some(ResolveError::OutOfScope(spelling));
                }
                continue;
            };
            match cands.iter_mut().find(|(r, _, _)| *r == binding) {
                Some(e) => e.1 = e.1.max(m.matched),
                None => cands.push((binding, m.matched, fresh)),
            }
        }
        let mut best: Option<(f64, usize, Resolved)> = None;
        let mut tied = false;
        for (binding, matched, fresh) in cands {
            let f = fields(binding);
            // Off its binding: a second name for one identity ranks on its own.
            let rank = f.lex_rank;
            let better = match &best {
                None => true,
                Some((r, n, _)) => rank > *r || (rank == *r && matched > *n),
            };
            if better {
                let r = Resolved { fresh, matched, binding, identity: f.dyad, scope: f.scope };
                best = Some((rank, matched, r));
                tied = false;
            } else if matches!(&best, Some((r, n, _)) if rank == *r && matched == *n) {
                tied = true;
            }
        }
        match best {
            Some(_) if tied => Err(ResolveError::Tied),
            Some((_, _, r)) => Ok(r),
            None => Err(why_none.unwrap_or_else(|| ResolveError::Unknown(unknown_spelling(text)))),
        }
    }

    /// No-shadowing: `Shadowed` if `name` is live in an open scope; a dead
    /// name is free to redeclare, a fresh entry beside the dead one.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; this writes its `scope`.
    pub unsafe fn declare(
        &mut self,
        trie: &mut RegexTrie,
        name: &str,
        binding: DyadPtr,
    ) -> Result<(), ResolveError> {
        let scope = self.current().expect("declare needs an open scope");
        match self.resolve(trie, name) {
            Ok(_) => return Err(ResolveError::Shadowed(name.to_string())),
            Err(ResolveError::OutOfScope(_) | ResolveError::Unknown(_) | ResolveError::Dead(_)) => {
            }
            Err(e) => return Err(e),
        }
        // The spelling enters the index as a literal key, never as a pattern.
        let key = regex::escape(name);
        // SAFETY: `binding` is a binding dyad from the store, built for this name.
        unsafe { Binding::set_scope(binding, scope) };
        trie.insert(&key, binding);
        self.journal.push(Journal::Declared { name: key, scope });
        self.pending.push(Pending { binding, endpoint: Endpoint::Start });
        Ok(())
    }

    /// The key enters the index as the recognizer it is, never escaped; the
    /// one check is that this very key is not live in an open scope. Two
    /// patterns that can match the same length at equal rank are caught only
    /// when text hits both (`Tied`): deciding it here needs automaton
    /// intersection.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store ([`Binding::alloc`]); this
    /// writes its `scope` field through the pointer.
    pub unsafe fn declare_pattern(
        &mut self,
        trie: &mut RegexTrie,
        key: &str,
        binding: DyadPtr,
    ) -> Result<(), ResolveError> {
        let scope = self.current().expect("declare needs an open scope");
        if let Some(bindings) = trie.bindings_for_key(key) {
            // SAFETY: every pointer the trie stores is a binding dyad from the store.
            let fields = |r: DyadPtr| unsafe { Binding::read(r) };
            if bindings.iter().any(|&r| self.is_open(fields(r).scope) && !fields(r).is_dead()) {
                return Err(ResolveError::Shadowed(key.to_string()));
            }
        }
        // SAFETY: `binding` is a binding dyad from the store, built for this pattern.
        unsafe { Binding::set_scope(binding, scope) };
        trie.insert(key, binding);
        self.journal.push(Journal::Declared { name: key.to_string(), scope });
        self.pending.push(Pending { binding, endpoint: Endpoint::Start });
        Ok(())
    }

    /// `node` is the provisional `end` until `settle_item` replaces it with
    /// the body item (DESIGN ›Memory and concurrency‹).
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; this writes its `end`.
    pub unsafe fn mark_dead(&mut self, binding: DyadPtr, node: DyadPtr) {
        // SAFETY: `binding` is a binding dyad the trie returned.
        let prev_end = unsafe { Binding::replace_end(binding, node) };
        self.journal.push(Journal::Ended { binding, prev_end });
        self.pending.push(Pending { binding, endpoint: Endpoint::End });
    }

    /// Every endpoint pending for `scope` now points at `item`, the line as a
    /// whole: an `own` inside an `if` body ends the outer name at the `if`
    /// (DESIGN ›Name resolution is scope-filtered‹).
    ///
    /// # Safety
    /// Every pending binding must be a binding dyad from the store; `item` is
    /// stored, never read.
    pub unsafe fn settle_item(&mut self, scope: DyadPtr, item: DyadPtr) {
        let mut i = 0;
        while i < self.pending.len() {
            let binding = self.pending[i].binding;
            // SAFETY: every pending binding is a binding dyad from the store.
            if unsafe { Binding::read(binding).scope } != scope {
                i += 1;
                continue;
            }
            let p = self.pending.swap_remove(i);
            // SAFETY: every pending binding is a binding dyad from the store.
            unsafe {
                match p.endpoint {
                    Endpoint::Start => Binding::set_start(binding, item),
                    Endpoint::End => Binding::set_end(binding, item),
                }
            }
        }
    }

    /// `item` is a complete line of the innermost scope: the names it declared
    /// or ended settle on it, and it joins the scope's `dyads`.
    ///
    /// # Safety
    /// As [`ScopeStack::settle_item`]; `array_ty` the `array` identity.
    pub unsafe fn close_item(&mut self, store: &mut Store, array_ty: DyadPtr, item: DyadPtr) {
        let scope = self.current().expect("a line closes inside an open scope");
        // SAFETY: the caller's contract.
        unsafe {
            self.settle_item(scope, item);
            crate::identities::scope::push_item(store, array_ty, scope, item);
        }
    }

    /// Checked against its siblings alone (DESIGN ›The constructor is a
    /// field‹): a live binding of the spelling in an enclosing scope stands
    /// beside it, both live until this scope closes.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; this writes its `scope`.
    pub unsafe fn declare_field(
        &mut self,
        trie: &mut RegexTrie,
        name: &str,
        binding: DyadPtr,
    ) -> Result<(), ResolveError> {
        let scope = self.current().expect("declare needs an open scope");
        if self.declared_in(trie, name, scope)? {
            return Err(ResolveError::Shadowed(name.to_string()));
        }
        // The spelling enters the index as a literal key, never as a pattern.
        let key = regex::escape(name);
        // SAFETY: `binding` is a binding dyad from the store, built for this name.
        unsafe { Binding::set_scope(binding, scope) };
        trie.insert(&key, binding);
        self.journal.push(Journal::Declared { name: key, scope });
        self.pending.push(Pending { binding, endpoint: Endpoint::Start });
        Ok(())
    }

    /// The sibling check, run too across a type body's two scopes: its
    /// `share` members live in the body's, its fields in the list's.
    pub fn declared_in(
        &self,
        trie: &RegexTrie,
        name: &str,
        scope: DyadPtr,
    ) -> Result<bool, ResolveError> {
        match trie.get(name) {
            Ok(m) if m.matched == name.len() => {
                // SAFETY: every pointer the trie stores is a binding dyad.
                let fields = |r: DyadPtr| unsafe { Binding::read(r) };
                Ok(m.bindings.iter().any(|&r| fields(r).scope == scope && !fields(r).is_dead()))
            }
            Ok(_) | Err(RegexTrieError::NodeNotFound) => Ok(false),
            Err(e) => Err(ResolveError::Index(e)),
        }
    }

    /// The name becomes another spelling of an existing identity (a type), so
    /// pointer-identity checks see the original; the binding's range, journal
    /// entry and pending endpoint are untouched.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; this writes its `dyad`.
    pub unsafe fn rebind(&mut self, binding: DyadPtr, identity: DyadPtr) {
        // SAFETY: `binding` is a binding dyad from the store.
        unsafe { Binding::set_dyad(binding, identity) };
    }
}
