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
    /// The dyad a use of the name yields: the binding's, or the binding itself where it is
    /// the storage.
    pub identity: DyadPtr,
    /// The scope the winning declaration was made in: what a rebind that
    /// completes it must target.
    pub scope: DyadPtr,
}

/// What stands left of a token: what tells apart the two identities one spelling names in one
/// scope, the one that reads a value to its left and the one that does not (›`-` negates where a
/// space stands before it and none after, or no value stands to its left; any other `-`
/// subtracts‹ [[R157]]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    /// No value: the start, an opener, a `,`, an operator.
    Nothing,
    /// A value, the token right after it.
    Value,
    /// A value, then whitespace before the token.
    SpacedValue,
    /// A `.` that reads a member: the token is the member's name, which holds no `.` itself.
    Member,
}

impl Left {
    /// Whether the identity that reads no value to its left stands here, `after` being the byte
    /// right after the spelling: none to its left, or a space before it and none after.
    fn takes_none(self, after: Option<&u8>) -> bool {
        match self {
            Left::Nothing => true,
            Left::Value | Left::Member => false,
            Left::SpacedValue => !after.is_some_and(u8::is_ascii_whitespace),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// Not in the name index at all; carries the leading word at the position.
    Unknown(String),
    /// Known, but no declaration of it is in an open scope.
    OutOfScope(String),
    /// Declaring it would shadow a declaration still live in an open scope.
    Shadowed(String),
    /// Declared in an open scope but made dead by a `move` or `free`; only
    /// `:=` may follow (DESIGN ›Name resolution is scope-filtered‹).
    Dead(String),
    /// Declared, but its `:=` is still driving its value: the node does not exist yet.
    Unbuilt(String),
    /// Two spellings of equal `lex_rank` match the same length: an
    /// inconsistency in the definitions, never a pick by declaration order.
    Tied,
    /// A token reading its own extent found a `«` or `{` the text ends inside, or a `»`
    /// inside a `{…}` that closes no `«`: `bracket` is that character, `at` bytes into the
    /// text the lookup was given.
    Unmatched { bracket: char, at: usize },
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

/// A bare name or a field path `name.f.g` a `move` or `free` is about to end, handed back so
/// the keyword's constructor can mark it dead with the node it built.
#[derive(Debug)]
pub(crate) struct Ended {
    pub(crate) binding: DyadPtr,
    /// The bindings of the fields the path goes through, the name's first; empty when the
    /// name itself ends.
    pub(crate) path: Vec<DyadPtr>,
}

impl Ended {
    pub(crate) fn name(binding: DyadPtr) -> Self {
        Ended { binding, path: Vec::new() }
    }

    /// The name itself, not a path under it.
    pub(crate) fn is_name(&self) -> bool {
        self.path.is_empty()
    }
}

/// The operand of a `move` or `free`.
#[derive(Debug)]
pub(crate) enum Taken {
    /// The node the place is reached through, and the name or the path under a name it ends.
    Place(DyadPtr, Option<Ended>),
    /// A value no name holds, and where it starts in the source.
    Value(DyadPtr, usize),
    /// A hole, `T ?` or `?`, which is neither, and where it starts.
    Hole(usize),
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
    /// Stack depths where a body that runs again or later begins; `move`/`free`
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

    /// A barrier began after `scope` was pushed, so `move`/`free` of a name
    /// from it is refused here.
    pub fn crosses_barrier(&self, scope: DyadPtr) -> bool {
        let Some(idx) = self.position(scope) else {
            return false;
        };
        self.barriers.iter().any(|&b| b > idx)
    }

    /// Whether a body that runs again or later encloses the position.
    pub fn in_barrier(&self) -> bool {
        !self.barriers.is_empty()
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
    /// `move`/`free` for the rest of the session.
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
    /// declarations of the same spelling stay. The ends of `freed` stand: the line's run
    /// freed their values, which no rollback brings back.
    pub fn rollback(&mut self, trie: &mut RegexTrie, freed: &[DyadPtr]) {
        while let Some(act) = self.journal.pop() {
            match act {
                Journal::Declared { name, scope } => {
                    // A failed removal means the entry was already pruned.
                    let _ = trie.remove(&name, scope);
                }
                Journal::Ended { binding, .. } if freed.contains(&binding) => {}
                Journal::Ended { binding, prev_end } => {
                    // SAFETY: a journalled binding is a binding dyad from the store.
                    unsafe { Binding::set_end(binding, prev_end) };
                }
            }
        }
        self.pending.clear();
    }

    pub fn resolve(&self, trie: &RegexTrie, name: &str) -> Result<Resolved, ResolveError> {
        self.select(trie, name, false, Left::Value)
    }

    /// As `resolve`, but the fresh-spelling patterns compete too: a spelling
    /// nothing declared answers with `fresh` set and the run's length. `left` is what stands
    /// left of `text`.
    pub fn lex(&self, trie: &RegexTrie, text: &str, left: Left) -> Result<Resolved, ResolveError> {
        self.select(trie, text, true, left)
    }

    /// The one selection rule (DESIGN ›The scope's constructor is the driver‹):
    /// a match ending between two word characters is no candidate; of the
    /// rest, highest `lex_rank` first, longest match at equal rank.
    fn select(
        &self,
        trie: &RegexTrie,
        text: &str,
        include_fresh: bool,
        left: Left,
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
            let spans_path = left == Left::Member && bytes[..m.matched].contains(&b'.');
            if m.matched == 0 || cuts_word || spans_path || (fresh && !include_fresh) {
                continue;
            }
            // At the frontier "range covers the point" is exactly "not dead".
            // Two live bindings of one spelling are one declared against its siblings alone
            // (`declare_field`): the innermost open scope's wins (›Slot words are known only
            // inside a type body‹ [[R220]]). Two in one scope are the spelling's two
            // identities, and what stands left of it picks one.
            let live = || {
                m.bindings
                    .iter()
                    .copied()
                    .filter(|&r| self.is_open(fields(r).scope) && !fields(r).is_dead())
            };
            let innermost = live().map(|r| self.position(fields(r).scope)).max();
            let twins: Vec<DyadPtr> =
                live().filter(|&r| Some(self.position(fields(r).scope)) == innermost).collect();
            let binding = match twins[..] {
                [] => None,
                [one] => Some(one),
                _ => {
                    let takes_none = left.takes_none(bytes.get(m.matched));
                    let fits = |r: DyadPtr| {
                        // SAFETY: `r` is a binding dyad from the store, and the identity it names
                        // is null or a dyad from the store, which `takes_lhs` is given only when
                        // not null.
                        unsafe {
                            let id = fields(r).names(r);
                            !id.is_null() && crate::identities::meta::takes_lhs(id) != takes_none
                        }
                    };
                    twins.into_iter().max_by_key(|&r| fits(r))
                }
            };
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
                // SAFETY: `binding` is a binding dyad from the store.
                let identity = unsafe { f.names(binding) };
                let r = Resolved { fresh, matched, binding, identity, scope: f.scope };
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
    /// name is free to redeclare, a fresh entry beside the dead one. `name` is
    /// the binding's spelling, entered under the key its record says.
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
        // SAFETY: the caller's contract.
        unsafe { self.declare_in(trie, name, binding, scope) }
    }

    /// `declare` into `scope`, an open scope that need not be the innermost: a type body's fields
    /// are declared from its lines.
    ///
    /// # Safety
    /// As [`ScopeStack::declare`].
    pub unsafe fn declare_in(
        &mut self,
        trie: &mut RegexTrie,
        name: &str,
        binding: DyadPtr,
        scope: DyadPtr,
    ) -> Result<(), ResolveError> {
        if self.live(trie, name, binding)?.is_some() {
            return Err(ResolveError::Shadowed(name.to_string()));
        }
        // SAFETY: `binding` is a binding dyad from the store, built for this name.
        let key = unsafe { Binding::read(binding) }.key(name);
        // SAFETY: as above.
        unsafe { Binding::set_scope(binding, scope) };
        trie.insert(&key, binding);
        self.journal.push(Journal::Declared { name: key, scope });
        self.pending.push(Pending { binding, endpoint: Endpoint::Start });
        Ok(())
    }

    /// The live binding a declaration of `name`, keyed as `binding`'s record says, would
    /// shadow: one under that very key, or for a spelling that stands for itself, one its
    /// whole text resolves to. A pattern meets only its own key: two patterns that can
    /// match the same length at equal rank are caught when text hits both (`Tied`), since
    /// deciding it here needs automaton intersection.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store.
    pub unsafe fn live(
        &self,
        trie: &RegexTrie,
        name: &str,
        binding: DyadPtr,
    ) -> Result<Option<DyadPtr>, ResolveError> {
        // SAFETY: `binding` and every pointer the trie stores are binding dyads from the store.
        let fields = |r: DyadPtr| unsafe { Binding::read(r) };
        let rec = fields(binding);
        let keyed = trie.bindings_for_key(&rec.key(name)).and_then(|bindings| {
            bindings
                .iter()
                .copied()
                .find(|&r| self.is_open(fields(r).scope) && !fields(r).is_dead())
        });
        if keyed.is_some() || rec.pattern {
            return Ok(keyed);
        }
        match self.resolve(trie, name) {
            Ok(r) => Ok((r.matched == name.len()).then_some(r.binding)),
            Err(ResolveError::OutOfScope(_) | ResolveError::Unknown(_) | ResolveError::Dead(_)) => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
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

    /// Where the ends written from here on begin, for [`ScopeStack::undo_ends`].
    pub fn ends_mark(&self) -> usize {
        self.journal.len()
    }

    /// The names of scopes still open that a `move` or `free` ended since `mark` are live again,
    /// as rollback leaves them, and stay in the journal and the pending endpoints: the next arm
    /// of an `if` starts from the names as they stood when the `if` began (›A binding is live
    /// while its scope is open and it is not dead; it carries a range‹ [[R337]]). Returns each
    /// such name once, with the node that ended it.
    ///
    /// # Safety
    /// Every journalled binding must be a binding dyad from the store.
    pub unsafe fn undo_ends(&mut self, mark: usize) -> Vec<(DyadPtr, DyadPtr)> {
        let mut undone: Vec<(DyadPtr, DyadPtr)> = Vec::new();
        for act in self.journal[mark..].iter().rev() {
            let &Journal::Ended { binding, prev_end } = act else {
                continue;
            };
            // SAFETY: the caller's contract.
            if !self.is_open(unsafe { Binding::read(binding).scope }) {
                continue;
            }
            // SAFETY: as above.
            let node = unsafe { Binding::replace_end(binding, prev_end) };
            if !undone.iter().any(|&(b, _)| b == binding) {
                undone.push((binding, node));
            }
        }
        undone
    }

    /// `binding`, which a `move` or `free` ended since `mark`, is live as if nothing had ended it:
    /// its end goes back to what it was at `mark`, and its journal entries since then and its
    /// pending end go, so neither a line's close nor an enclosing `if` ends it again.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store.
    pub unsafe fn spare(&mut self, mark: usize, binding: DyadPtr) {
        let mut at = self.journal.len();
        while at > mark {
            at -= 1;
            if let Journal::Ended { binding: ended, prev_end } = self.journal[at] {
                if ended == binding {
                    // SAFETY: the caller's contract.
                    unsafe { Binding::replace_end(binding, prev_end) };
                    self.journal.remove(at);
                }
            }
        }
        self.pending.retain(|p| p.binding != binding || p.endpoint != Endpoint::End);
    }

    /// Every endpoint pending for `scope` now points at `item`, the line as a
    /// whole: a `move` inside an `if` body ends the outer name at the `if`
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

    /// Checked against its siblings alone: a live binding of the spelling in an enclosing scope
    /// stands beside it, both live until this scope closes. Only a name that spells one already
    /// live on purpose is declared so: a type body's slot words (›Slot words are known only inside
    /// a type body‹ [[R220]]), a kept record's mirror of the names it holds, a run's `output_type`
    /// over the member.
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
        // SAFETY: `binding` is a binding dyad from the store, built for this name.
        let key = unsafe { Binding::read(binding) }.key(name);
        // SAFETY: as above.
        unsafe { Binding::set_scope(binding, scope) };
        trie.insert(&key, binding);
        self.journal.push(Journal::Declared { name: key, scope });
        self.pending.push(Pending { binding, endpoint: Endpoint::Start });
        Ok(())
    }

    /// A word a body gets from the language and no text spelled, a `parse` body's `tape` (›`tape`
    /// is a word the `parse` slot declares into its body‹ [[R191]]): the same word of a body this
    /// one is written in stands beside it, both live until this scope closes, and any other live
    /// name of the spelling refuses it.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; this writes its `scope` and its mark.
    pub unsafe fn declare_body_word(
        &mut self,
        trie: &mut RegexTrie,
        name: &str,
        binding: DyadPtr,
    ) -> Result<(), ResolveError> {
        // SAFETY: the caller's contract; `live` hands back a binding dyad from the store.
        unsafe {
            Binding::set_body_word(binding);
            match self.live(trie, name, binding)? {
                Some(outer) if Binding::read(outer).body_word => {
                    self.declare_field(trie, name, binding)
                }
                _ => self.declare(trie, name, binding),
            }
        }
    }

    /// A live binding of `name` declared in `scope` itself.
    fn declared_in(
        &self,
        trie: &RegexTrie,
        name: &str,
        scope: DyadPtr,
    ) -> Result<bool, ResolveError> {
        let matches = trie.get_all_matches(name).map_err(ResolveError::Index)?;
        // SAFETY: every pointer the trie stores is a binding dyad.
        let fields = |r: DyadPtr| unsafe { Binding::read(r) };
        Ok(matches
            .iter()
            .filter(|m| m.matched == name.len())
            .flat_map(|m| m.bindings)
            .any(|&r| fields(r).scope == scope && !fields(r).is_dead()))
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
