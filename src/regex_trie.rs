// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The hybrid regex-trie, the lexing name index. Literal prefixes ride a
//! byte-path; residual regex chunks are branches, matched separately so
//! lookup is true longest-match, a literal beating a regex at equal length.
//! Values are record dyads the store owns; resolution policy is the parser's.

use std::cell::RefCell;

use regex::bytes::Regex;

use crate::dyad::DyadPtr;
use crate::record::Record;

use crate::regex_splitting::{is_pure_literal, regex_splitting, Segment};

/// `child_indices` sentinel: no child for this byte.
const NONE: u32 = u32::MAX;
/// The byte reserved as the end-of-word marker; its child slot holds `None`.
const EOW: usize = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegexTrieError {
    /// No entry matches the spelling.
    NodeNotFound,
    /// A residual regex segment failed to compile.
    BadPattern(String),
}

/// The value at an end-of-word node; every alternation path of one `insert`
/// holds the same record pointers.
#[derive(Debug)]
pub struct Leaf {
    pub regex_key: String,
    pub records: Vec<DyadPtr>,
}

/// `records` is the full candidate list, one per declaring scope; the parser
/// picks the one live in the open scopes.
#[derive(Debug)]
pub struct MatchResult<'a> {
    /// Bytes consumed from the start of the input.
    pub matched: usize,
    pub regex_key: &'a str,
    pub records: &'a [DyadPtr],
}

struct RegexEntry {
    node: Box<RegexTrie>,
    pattern: String,
    /// `^(?:pattern)`, compiled on first use so a bad pattern surfaces at
    /// lookup rather than at insert.
    matcher: RefCell<Option<Regex>>,
}

impl RegexEntry {
    fn new(pattern: &str) -> Self {
        RegexEntry {
            node: Box::new(RegexTrie::new()),
            pattern: pattern.to_string(),
            matcher: RefCell::new(None),
        }
    }

    /// Length of this branch's non-empty match at the start of `hay`.
    fn match_len(&self, hay: &[u8]) -> Result<Option<usize>, RegexTrieError> {
        if self.matcher.borrow().is_none() {
            let anchored = format!("^(?:{})", self.pattern);
            let re = Regex::new(&anchored)
                .map_err(|err| RegexTrieError::BadPattern(format!("{anchored}: {err}")))?;
            *self.matcher.borrow_mut() = Some(re);
        }
        let borrow = self.matcher.borrow();
        let re = borrow.as_ref().unwrap();
        Ok(re.find(hay).and_then(|m| (m.start() == 0 && m.end() > 0).then_some(m.end())))
    }
}

/// A node: `child_indices` maps a byte to a slot in `children`; byte 0 is the
/// EOW sentinel.
pub struct RegexTrie {
    child_indices: [u32; 256],
    children: Vec<Option<Box<RegexTrie>>>,
    leaf_value: Option<Leaf>,
    regexes: Vec<RegexEntry>,
}

impl Default for RegexTrie {
    fn default() -> Self {
        Self::new()
    }
}

impl RegexTrie {
    pub fn new() -> Self {
        RegexTrie {
            child_indices: [NONE; 256],
            children: Vec::new(),
            leaf_value: None,
            regexes: Vec::new(),
        }
    }

    fn lit_child(&self, c: u8) -> Option<&RegexTrie> {
        let idx = self.child_indices[c as usize];
        if idx == NONE {
            return None;
        }
        self.children[idx as usize].as_deref()
    }

    /// End-of-word: the byte-0 slot exists and is null.
    fn check_eow(&self) -> bool {
        let idx = self.child_indices[EOW];
        idx != NONE && self.children[idx as usize].is_none()
    }

    /// Any literal child other than EOW, or any regex branch.
    fn has_children(&self) -> bool {
        for key in 1..256usize {
            let idx = self.child_indices[key];
            if idx != NONE && self.children[idx as usize].is_some() {
                return true;
            }
        }
        !self.regexes.is_empty()
    }

    fn lit_child_or_create(&mut self, c: u8) -> &mut RegexTrie {
        debug_assert!(c != 0, "byte 0 is reserved for the EOW sentinel");
        let slot = self.child_indices[c as usize];
        if slot == NONE {
            let new_idx = self.children.len() as u32;
            self.children.push(Some(Box::new(RegexTrie::new())));
            self.child_indices[c as usize] = new_idx;
            self.children[new_idx as usize].as_mut().unwrap()
        } else {
            self.children[slot as usize].as_mut().unwrap()
        }
    }

    fn regex_child_or_create(&mut self, pattern: &str) -> &mut RegexTrie {
        if let Some(i) = self.regexes.iter().position(|e| e.pattern == pattern) {
            return &mut self.regexes[i].node;
        }
        self.regexes.push(RegexEntry::new(pattern));
        let last = self.regexes.len() - 1;
        &mut self.regexes[last].node
    }

    fn ensure_eow(&mut self) {
        if self.child_indices[EOW] == NONE {
            let new_idx = self.children.len() as u32;
            self.children.push(None);
            self.child_indices[EOW] = new_idx;
        }
    }

    fn walk_create<'a>(node: &'a mut RegexTrie, path: &[Segment]) -> &'a mut RegexTrie {
        let mut current = node;
        for seg in path {
            if seg.is_lit {
                for &c in seg.str.as_bytes() {
                    current = current.lit_child_or_create(c);
                }
            } else {
                current = current.regex_child_or_create(&seg.str);
            }
        }
        current
    }

    fn locate(&self, path: &[Segment]) -> Option<&RegexTrie> {
        let mut current = self;
        for seg in path {
            if seg.is_lit {
                for &c in seg.str.as_bytes() {
                    if c == 0 {
                        return None;
                    }
                    let idx = current.child_indices[c as usize];
                    if idx == NONE {
                        return None;
                    }
                    current = current.children[idx as usize].as_ref()?;
                }
            } else {
                let i = current.regexes.iter().position(|e| e.pattern == seg.str)?;
                current = &current.regexes[i].node;
            }
        }
        Some(current)
    }

    /// The records under exactly `key` as inserted: a declaration-time question,
    /// unlike [`get`](Self::get), which asks what a text lexes as.
    pub fn records_for_key(&self, key: &str) -> Option<&[DyadPtr]> {
        let path = regex_splitting(key).into_iter().next()?;
        let node = self.locate(&path).filter(|n| n.check_eow())?;
        let leaf = node.leaf_value.as_ref().filter(|v| v.regex_key == key)?;
        Some(&leaf.records)
    }

    /// Appends: a spelling carries one record per scope it is declared in. The
    /// no-shadowing rule is the parser's; the trie only stores.
    pub fn insert(&mut self, key: &str, record: DyadPtr) {
        debug_assert!(!key.is_empty());

        if is_pure_literal(key) {
            return self.insert_literal_fast(key, record);
        }

        for path in &regex_splitting(key) {
            let leaf = Self::walk_create(self, path);
            leaf.ensure_eow();
            push_record(&mut leaf.leaf_value, key, record);
        }
    }

    fn insert_literal_fast(&mut self, s: &str, record: DyadPtr) {
        let mut current = self;
        for &c in s.as_bytes() {
            current = current.lit_child_or_create(c);
        }
        current.ensure_eow();
        push_record(&mut current.leaf_value, s, record);
    }

    /// Longest match at the start of `string`; a literal beats a regex of
    /// equal length, so a short keyword never blocks a longer identifier.
    pub fn get(&self, string: &str) -> Result<MatchResult<'_>, RegexTrieError> {
        debug_assert!(!string.is_empty());
        match self.longest(string.as_bytes(), 0)? {
            Some((matched, leaf)) => {
                Ok(MatchResult { matched, regex_key: &leaf.regex_key, records: &leaf.records })
            }
            None => Err(RegexTrieError::NodeNotFound),
        }
    }

    /// The literal child first, then each regex branch in insertion order; a
    /// candidate replaces the best only when strictly longer, so the literal
    /// path and earlier-declared regexes win ties.
    fn longest<'s>(
        &'s self,
        hay: &[u8],
        pos: usize,
    ) -> Result<Option<(usize, &'s Leaf)>, RegexTrieError> {
        let mut best: Option<(usize, &'s Leaf)> = None;
        if self.check_eow() {
            if let Some(leaf) = self.leaf_value.as_ref() {
                best = Some((pos, leaf));
            }
        }
        if pos < hay.len() {
            let c = hay[pos];
            if c != 0 {
                if let Some(child) = self.lit_child(c) {
                    if let Some(cand) = child.longest(hay, pos + 1)? {
                        if best.is_none_or(|(n, _)| cand.0 > n) {
                            best = Some(cand);
                        }
                    }
                }
            }
            for entry in &self.regexes {
                if let Some(len) = entry.match_len(&hay[pos..])? {
                    if let Some(cand) = entry.node.longest(hay, pos + len)? {
                        if best.is_none_or(|(n, _)| cand.0 > n) {
                            best = Some(cand);
                        }
                    }
                }
            }
        }
        Ok(best)
    }

    /// Every match at the start of `string`, for ambiguity inspection.
    pub fn get_all_matches(&self, string: &str) -> Result<Vec<MatchResult<'_>>, RegexTrieError> {
        let bytes = string.as_bytes();
        let mut out: Vec<MatchResult<'_>> = Vec::new();
        let mut stack: Vec<(&RegexTrie, usize)> = vec![(self, 0)];

        while let Some((current, pos)) = stack.pop() {
            if current.check_eow() {
                if let Some(v) = &current.leaf_value {
                    out.push(MatchResult {
                        matched: pos,
                        regex_key: &v.regex_key,
                        records: &v.records,
                    });
                }
            }
            if pos >= bytes.len() {
                continue;
            }

            let c = bytes[pos];
            if c != 0 {
                let idx = current.child_indices[c as usize];
                if idx != NONE {
                    if let Some(child) = &current.children[idx as usize] {
                        stack.push((child, pos + 1));
                    }
                }
            }

            for entry in &current.regexes {
                if let Some(len) = entry.match_len(&bytes[pos..])? {
                    stack.push((&entry.node, pos + len));
                }
            }
        }
        Ok(out)
    }

    /// Remove the live record declared in `scope` for `regex_key` and return
    /// the identity it denoted. A dead record in the same scope stays, its
    /// range being what reflection reads; a leaf is pruned only when its last record goes.
    pub fn remove(&mut self, regex_key: &str, scope: DyadPtr) -> Result<DyadPtr, RegexTrieError> {
        debug_assert!(!regex_key.is_empty());
        let paths = regex_splitting(regex_key);

        // Verify every path before mutating anything.
        let mut held: Option<DyadPtr> = None;
        for path in &paths {
            let node = self.locate(path).filter(|n| n.check_eow());
            let ident = match node.and_then(|n| n.leaf_value.as_ref()) {
                Some(v) if v.regex_key == regex_key => {
                    match v.records.iter().find(|&&r| is_live_in(r, scope)) {
                        // SAFETY: every stored pointer is a record dyad.
                        Some(&r) => unsafe { Record::read(r).dyad },
                        None => return Err(RegexTrieError::NodeNotFound),
                    }
                }
                _ => return Err(RegexTrieError::NodeNotFound),
            };
            match held {
                None => held = Some(ident),
                Some(h) if h == ident => {}
                Some(_) => return Err(RegexTrieError::NodeNotFound),
            }
        }

        for path in &paths {
            let steps = flatten(path);
            Self::prune_remove(self, &steps, 0, scope);
        }

        Ok(held.expect("at least one path verified"))
    }

    /// Returns true when `node` became empty and the caller should drop the link to it.
    fn prune_remove(node: &mut RegexTrie, steps: &[Step], i: usize, scope: DyadPtr) -> bool {
        if i == steps.len() {
            if let Some(leaf) = &mut node.leaf_value {
                leaf.records.retain(|&r| !is_live_in(r, scope));
                if leaf.records.is_empty() {
                    node.leaf_value = None;
                    let eow = node.child_indices[EOW];
                    if eow != NONE {
                        node.child_indices[EOW] = NONE;
                        node.children[eow as usize] = None;
                    }
                }
            }
            return !node.has_children() && node.leaf_value.is_none() && !node.check_eow();
        }

        match &steps[i] {
            Step::Lit(c) => {
                let idx = node.child_indices[*c as usize];
                if idx == NONE {
                    return false;
                }
                let child_empty = match &mut node.children[idx as usize] {
                    Some(child) => Self::prune_remove(child, steps, i + 1, scope),
                    None => return false,
                };
                if child_empty {
                    node.child_indices[*c as usize] = NONE;
                    node.children[idx as usize] = None;
                }
            }
            Step::Regex(p) => match node.regexes.iter().position(|e| &e.pattern == p) {
                Some(ri) => {
                    let child_empty =
                        Self::prune_remove(&mut node.regexes[ri].node, steps, i + 1, scope);
                    if child_empty {
                        node.regexes.remove(ri);
                    }
                }
                None => return false,
            },
        }

        !node.has_children() && node.leaf_value.is_none() && !node.check_eow()
    }

    /// The trie as text, literal chains compressed.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.dump_rec(0, &mut out);
        out
    }

    fn literal_chain(&self) -> (String, &RegexTrie) {
        let mut label = String::new();
        let mut curr = self;
        loop {
            let mut only: Option<u8> = None;
            let mut count = 0;
            for key in 1..256usize {
                let idx = curr.child_indices[key];
                if idx != NONE && curr.children[idx as usize].is_some() {
                    count += 1;
                    only = Some(key as u8);
                    if count > 1 {
                        break;
                    }
                }
            }
            if count != 1 || !curr.regexes.is_empty() {
                break;
            }
            let c = only.unwrap();
            label.push(c as char);
            let idx = curr.child_indices[c as usize];
            match &curr.children[idx as usize] {
                Some(child) => curr = child,
                None => break,
            }
        }
        (label, curr)
    }

    fn indent(n: usize, out: &mut String) {
        for _ in 0..n * 4 {
            out.push(' ');
        }
    }

    fn dump_rec(&self, indent: usize, out: &mut String) {
        let (label, target) = self.literal_chain();
        if !label.is_empty() {
            Self::indent(indent, out);
            out.push_str(&format!("{label} (lit)"));
            if target.check_eow() && !target.has_children() {
                out.push_str(" (EOW)\n");
                return;
            }
            out.push('\n');
            target.dump_rec(indent + 1, out);
            return;
        }

        if self.check_eow() {
            Self::indent(indent, out);
            out.push_str("(EOW)\n");
        }
        self.dump_branches(indent, out);
    }

    fn dump_branches(&self, indent: usize, out: &mut String) {
        let mut rx: Vec<(&str, &RegexTrie)> =
            self.regexes.iter().map(|e| (e.pattern.as_str(), &*e.node)).collect();
        rx.sort_by(|a, b| a.0.cmp(b.0));
        for (rstr, rchild) in rx {
            Self::indent(indent + 1, out);
            out.push_str(&format!("{rstr} (regex)"));
            if rchild.check_eow() && !rchild.has_children() {
                out.push_str(" (EOW)\n");
            } else {
                out.push('\n');
                rchild.dump_rec(indent + 2, out);
            }
        }

        let mut keys: Vec<u8> = Vec::new();
        for key in 1..256usize {
            let idx = self.child_indices[key];
            if idx != NONE && self.children[idx as usize].is_some() {
                keys.push(key as u8);
            }
        }
        keys.sort_unstable();
        for c in keys {
            let idx = self.child_indices[c as usize];
            if let Some(child) = &self.children[idx as usize] {
                let (lbl, target) = child.literal_chain();
                Self::indent(indent + 1, out);
                out.push_str(&format!("{}{} (lit)", c as char, lbl));
                if target.check_eow() && !target.has_children() {
                    out.push_str(" (EOW)\n");
                } else {
                    out.push('\n');
                    target.dump_rec(indent + 2, out);
                }
            }
        }
    }
}

enum Step {
    Lit(u8),
    Regex(String),
}

fn is_live_in(record: DyadPtr, scope: DyadPtr) -> bool {
    // SAFETY: every pointer the trie stores is a record dyad from the store.
    let fields = unsafe { Record::read(record) };
    fields.scope == scope && !fields.is_dead()
}

fn push_record(leaf: &mut Option<Leaf>, key: &str, record: DyadPtr) {
    match leaf {
        Some(l) => l.records.push(record),
        None => *leaf = Some(Leaf { regex_key: key.to_string(), records: vec![record] }),
    }
}

fn flatten(path: &[Segment]) -> Vec<Step> {
    let mut v = Vec::new();
    for seg in path {
        if seg.is_lit {
            for &c in seg.str.as_bytes() {
                v.push(Step::Lit(c));
            }
        } else {
            v.push(Step::Regex(seg.str.clone()));
        }
    }
    v
}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)] // a test reads the nodes it built a line above
mod tests {
    use super::*;
    use crate::dyad::Dyad;

    /// A leaked dyad whose address is its id; serves as identity and as scope.
    fn dummy(tag: usize) -> DyadPtr {
        Box::into_raw(Box::new(Dyad { ty: std::ptr::null_mut(), value: tag as *mut u8 }))
    }

    /// A leaked record dyad in `scope` for `identity`.
    fn rec(identity: DyadPtr, scope: DyadPtr) -> DyadPtr {
        let fields = Box::into_raw(Box::new(Record::new(identity, scope, std::ptr::null_mut())));
        Box::into_raw(Box::new(Dyad { ty: std::ptr::null_mut(), value: fields as *mut u8 }))
    }

    fn f(record: DyadPtr) -> Record {
        // SAFETY: only `rec`-built dyads are inserted in these tests.
        unsafe { Record::read(record) }
    }

    #[test]
    fn literal_longest_match() {
        let root = dummy(100);
        let mut t = RegexTrie::new();
        let (colon, colon_eq, eq, plus) = (dummy(1), dummy(2), dummy(3), dummy(4));
        t.insert(":", rec(colon, root));
        t.insert(":=", rec(colon_eq, root));
        t.insert("=", rec(eq, root));
        t.insert("+", rec(plus, root));

        let m = t.get(":=").unwrap();
        assert_eq!(m.matched, 2);
        assert_eq!(m.regex_key, ":=");
        assert_eq!(f(m.records[0]).dyad, colon_eq);

        let m = t.get(":x").unwrap();
        assert_eq!(m.matched, 1);
        assert_eq!(f(m.records[0]).dyad, colon);

        assert_eq!(f(t.get("=").unwrap().records[0]).dyad, eq);
    }

    #[test]
    fn regex_branch_matches() {
        let root = dummy(100);
        let mut t = RegexTrie::new();
        let num = dummy(1);
        t.insert("[0-9]+", rec(num, root));
        let m = t.get("123abc").unwrap();
        assert_eq!(m.matched, 3);
        assert_eq!(f(m.records[0]).dyad, num);
    }

    #[test]
    fn literal_beats_regex_at_same_node() {
        let root = dummy(100);
        let mut t = RegexTrie::new();
        let (kw, ident) = (dummy(1), dummy(2));
        t.insert("if", rec(kw, root));
        t.insert("[a-z]+", rec(ident, root));

        assert_eq!(f(t.get("if").unwrap().records[0]).dyad, kw);
        assert_eq!(f(t.get("foo").unwrap().records[0]).dyad, ident);
    }

    #[test]
    fn unknown_input_is_not_found() {
        let root = dummy(100);
        let mut t = RegexTrie::new();
        t.insert("foo", rec(dummy(1), root));
        assert!(matches!(t.get("bar"), Err(RegexTrieError::NodeNotFound)));
    }

    #[test]
    fn same_spelling_two_scopes_are_both_stored() {
        let (outer, inner) = (dummy(100), dummy(101));
        let mut t = RegexTrie::new();
        let (id_outer, id_inner) = (dummy(1), dummy(2));
        t.insert("x", rec(id_outer, outer));
        t.insert("x", rec(id_inner, inner));

        let m = t.get("x").unwrap();
        assert_eq!(m.records.len(), 2);
        let ids: Vec<_> = m.records.iter().map(|&c| f(c).dyad).collect();
        assert!(ids.contains(&id_outer) && ids.contains(&id_inner));
    }

    #[test]
    fn insert_then_use_in_same_pass() {
        // Declarations extend the lexer mid-parse.
        let root = dummy(100);
        let mut t = RegexTrie::new();
        t.insert("=", rec(dummy(1), root));
        assert!(t.get("widget").is_err());
        let widget = dummy(42);
        t.insert("widget", rec(widget, root));
        let m = t.get("widget = 1").unwrap();
        assert_eq!(m.matched, 6);
        assert_eq!(f(m.records[0]).dyad, widget);
    }

    #[test]
    fn alternation_shares_one_context() {
        let root = dummy(100);
        let mut t = RegexTrie::new();
        let d = dummy(7);
        t.insert("ab|cd", rec(d, root));
        assert_eq!(t.get("ab").unwrap().matched, 2);
        assert_eq!(f(t.get("ab").unwrap().records[0]).dyad, d);
        assert_eq!(f(t.get("cd").unwrap().records[0]).dyad, d);

        assert_eq!(t.remove("ab|cd", root).unwrap(), d);
        assert!(t.get("ab").is_err());
        assert!(t.get("cd").is_err());
    }

    #[test]
    fn remove_one_scope_keeps_the_other() {
        let (outer, inner) = (dummy(100), dummy(101));
        let mut t = RegexTrie::new();
        let (id_outer, id_inner) = (dummy(1), dummy(2));
        t.insert("x", rec(id_outer, outer));
        t.insert("x", rec(id_inner, inner));

        assert_eq!(t.remove("x", inner).unwrap(), id_inner);
        let m = t.get("x").unwrap();
        assert_eq!(m.records.len(), 1);
        assert_eq!(f(m.records[0]).dyad, id_outer);
    }

    #[test]
    fn alternation_paths_share_one_record_dyad() {
        let root = dummy(100);
        let mut t = RegexTrie::new();
        let (d, ender) = (dummy(7), dummy(8));
        t.insert("ab|cd", rec(d, root));
        let via_ab = t.get("ab").unwrap().records[0];
        let via_cd = t.get("cd").unwrap().records[0];
        assert_eq!(via_ab, via_cd, "one record dyad, two paths");
        unsafe { Record::set_end(via_ab, ender) };
        assert_eq!(f(t.get("cd").unwrap().records[0]).end, ender);
    }

    #[test]
    fn remove_takes_only_the_live_context() {
        // A name ended and redeclared in one scope has a dead and a live record there.
        let root = dummy(100);
        let mut t = RegexTrie::new();
        let (old, new, ender) = (dummy(1), dummy(2), dummy(9));
        let old_rec = rec(old, root);
        t.insert("x", old_rec);
        unsafe { Record::set_end(old_rec, ender) };
        t.insert("x", rec(new, root));
        assert_eq!(t.get("x").unwrap().records.len(), 2);

        assert_eq!(t.remove("x", root).unwrap(), new);
        let m = t.get("x").unwrap();
        assert_eq!(m.records.len(), 1);
        assert_eq!(f(m.records[0]).dyad, old);
        assert!(f(m.records[0]).is_dead());
        assert_eq!(t.remove("x", root), Err(RegexTrieError::NodeNotFound));
    }

    #[test]
    fn remove_literal_then_gone() {
        let root = dummy(100);
        let mut t = RegexTrie::new();
        let (foo, foobar) = (dummy(1), dummy(2));
        t.insert("foo", rec(foo, root));
        t.insert("foobar", rec(foobar, root));
        assert_eq!(t.remove("foo", root).unwrap(), foo);
        assert!(t.get("foo").is_err());
        assert_eq!(f(t.get("foobar").unwrap().records[0]).dyad, foobar);
    }

    #[test]
    fn remove_missing_is_error() {
        let root = dummy(100);
        let other = dummy(101);
        let mut t = RegexTrie::new();
        t.insert("foo", rec(dummy(1), root));
        assert_eq!(t.remove("bar", root), Err(RegexTrieError::NodeNotFound));
        assert_eq!(t.remove("foo", other), Err(RegexTrieError::NodeNotFound));
    }

    #[test]
    fn get_all_matches_finds_every_path() {
        let root = dummy(100);
        let mut t = RegexTrie::new();
        let (lit_a, ident) = (dummy(1), dummy(2));
        t.insert("a", rec(lit_a, root));
        t.insert("[a-z]+", rec(ident, root));

        let mut ms = t.get_all_matches("abc").unwrap();
        ms.sort_by_key(|m| m.matched);
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].matched, 1);
        assert_eq!(f(ms[0].records[0]).dyad, lit_a);
        assert_eq!(ms[1].matched, 3);
        assert_eq!(f(ms[1].records[0]).dyad, ident);
    }

    #[test]
    fn longest_match_regex_beats_shorter_literal() {
        // "iffy" is the identifier, not the keyword "if" plus a stray "fy".
        let root = dummy(100);
        let mut t = RegexTrie::new();
        let (kw, ident) = (dummy(1), dummy(2));
        t.insert("if", rec(kw, root));
        t.insert("[a-z]+", rec(ident, root));

        let m = t.get("if").unwrap();
        assert_eq!(m.matched, 2);
        assert_eq!(f(m.records[0]).dyad, kw);
        let m = t.get("iffy").unwrap();
        assert_eq!(m.matched, 4);
        assert_eq!(f(m.records[0]).dyad, ident);
    }

    #[test]
    fn longest_match_across_sibling_regexes() {
        let root = dummy(100);
        let mut t = RegexTrie::new();
        let (short, long) = (dummy(1), dummy(2));
        t.insert("[a-z]", rec(short, root));
        t.insert("[a-z]+", rec(long, root));
        let m = t.get("abc").unwrap();
        assert_eq!(m.matched, 3);
        assert_eq!(f(m.records[0]).dyad, long);
    }

    #[test]
    fn bad_pattern_surfaces_error() {
        // Lookaround is unsupported by the `regex` crate.
        let root = dummy(100);
        let mut t = RegexTrie::new();
        t.insert("(?=foo)", rec(dummy(1), root));
        match t.get("foobar") {
            Err(RegexTrieError::BadPattern(_)) => {}
            other => panic!("expected BadPattern, got {other:?}"),
        }
    }

    #[test]
    fn records_for_key_answers_by_the_key_as_inserted() {
        let mut t = RegexTrie::new();
        let d = |n: usize| n as DyadPtr;
        t.insert("ab", d(1));
        t.insert("a[0-9]", d(2));
        t.insert("a[0-9]", d(3));
        assert_eq!(t.records_for_key("ab"), Some(&[d(1)][..]));
        assert_eq!(t.records_for_key("a[0-9]"), Some(&[d(2), d(3)][..]));
        assert_eq!(t.records_for_key("a"), None);
        assert_eq!(t.records_for_key("a[0-9]x"), None);
        assert_eq!(t.records_for_key("[0-9]"), None);
    }
}
