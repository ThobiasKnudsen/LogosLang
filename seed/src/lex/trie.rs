//! The hybrid regex-trie — a port of `regex_trie.zig`.
//!
//! Each node holds a 256-entry `child_indices` table mapping a byte to a slot in
//! `children`; byte 0 is reserved as the end-of-word (EOW) sentinel (its slot
//! holds `None`). Patterns that are not pure literals are split (see
//! [`super::regex_splitting`]) so their literal prefixes ride the fast byte-path
//! and only the residual regex chunks become `regexes` branches. Lookup is
//! greedy longest-match, trying the literal child first and falling back to the
//! node's combined regex.
//!
//! The stored value is a [`DyadPtr`] — a pointer to the node that the matched
//! text denotes (the Zig original stored a type-erased `?*anyopaque` here). The
//! trie does **not** own those dyads (they live in the graph/store), so it only
//! holds and returns the pointer; nothing is freed on removal.
//!
//! Differences from the Zig original, all behaviour-preserving:
//! - PCRE2 is replaced by the Rust `regex` crate (`regex::bytes`, byte-oriented
//!   like the PCRE2 8-bit API). Lookaround/backreferences therefore cannot
//!   compile; [`RegexTrieError::BadPattern`] surfaces that instead of panicking.
//! - Because the value is a `Copy` pointer the trie does not own, the Zig's
//!   shared-value/`freed`-flag bookkeeping collapses to copying the pointer to
//!   each alternation path; `remove` compares pointers directly.
//! - The lazily-built matcher lives behind `RefCell`/`Cell`, so `get` is `&self`.
//! - Alternatives are identified by named capture groups `(?P<gN>..)` rather than
//!   ovector positions, which is immune to capture groups inside a pattern.
//! - `remove`'s upward prune is recursion rather than an explicit parent stack.

use std::cell::{Cell, RefCell};

use regex::bytes::Regex;

use crate::dyad::DyadPtr;

use super::regex_splitting::{is_pure_literal, regex_splitting, Segment};

/// `child_indices` sentinel: no child for this byte.
const NONE: u32 = u32::MAX;
/// Byte index reserved as the end-of-word marker.
const EOW: usize = 0;

/// Errors surfaced by trie operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegexTrieError {
    /// `get`/`remove` found no matching entry.
    NodeNotFound,
    /// `insert` hit an entry that already carries a value.
    DuplicateLeafValue,
    /// A residual regex segment failed to compile with the `regex` crate.
    BadPattern(String),
}

/// The value stored at an end-of-word node: the pattern key and the dyad it
/// denotes. Alternation paths of one `insert` each carry a copy with the same
/// `data` pointer.
#[derive(Debug)]
pub struct Leaf {
    pub regex_key: String,
    pub data: DyadPtr,
}

/// The result of a successful lookup. `regex_key` borrows the stored entry;
/// `data` is the (copied) dyad pointer.
#[derive(Debug)]
pub struct MatchResult<'a> {
    /// Number of bytes consumed from the start of the input.
    pub matched: usize,
    /// The pattern key that matched.
    pub regex_key: &'a str,
    /// The dyad the matched text denotes.
    pub data: DyadPtr,
}

struct RegexEntry {
    node: Box<RegexTrie>,
    pattern: String,
}

/// A node of the hybrid regex-trie.
pub struct RegexTrie {
    child_indices: [u32; 256],
    children: Vec<Option<Box<RegexTrie>>>,
    leaf_value: Option<Leaf>,
    regexes: Vec<RegexEntry>,
    matcher: RefCell<Option<Regex>>,
    matcher_dirty: Cell<bool>,
}

impl Default for RegexTrie {
    fn default() -> Self {
        Self::new()
    }
}

impl RegexTrie {
    /// A fresh, empty trie node.
    pub fn new() -> Self {
        RegexTrie {
            child_indices: [NONE; 256],
            children: Vec::new(),
            leaf_value: None,
            regexes: Vec::new(),
            matcher: RefCell::new(None),
            matcher_dirty: Cell::new(false),
        }
    }

    // --- structural helpers -------------------------------------------------

    /// True if this node is an end-of-word (its byte-0 slot exists and is null).
    fn check_eow(&self) -> bool {
        let idx = self.child_indices[EOW];
        idx != NONE && self.children[idx as usize].is_none()
    }

    /// True if this node has any literal child (other than EOW) or any regex branch.
    fn has_children(&self) -> bool {
        for key in 1..256usize {
            let idx = self.child_indices[key];
            if idx != NONE && self.children[idx as usize].is_some() {
                return true;
            }
        }
        !self.regexes.is_empty()
    }

    /// Get or create the literal child for byte `c` and return a mutable ref to it.
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

    /// Get or create the regex branch for `pattern` and return a mutable ref to it.
    fn regex_child_or_create(&mut self, pattern: &str) -> &mut RegexTrie {
        if let Some(i) = self.regexes.iter().position(|e| e.pattern == pattern) {
            return &mut self.regexes[i].node;
        }
        self.matcher_dirty.set(true);
        self.regexes
            .push(RegexEntry { node: Box::new(RegexTrie::new()), pattern: pattern.to_string() });
        let last = self.regexes.len() - 1;
        &mut self.regexes[last].node
    }

    /// Ensure the EOW sentinel slot exists on this node.
    fn ensure_eow(&mut self) {
        if self.child_indices[EOW] == NONE {
            let new_idx = self.children.len() as u32;
            self.children.push(None);
            self.child_indices[EOW] = new_idx;
        }
    }

    /// Walk (creating nodes as needed) from `node` through one split path,
    /// returning the leaf node at its end.
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

    /// Walk an existing path read-only, returning the node it ends at (if present).
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
                    match &current.children[idx as usize] {
                        Some(child) => current = child,
                        None => return None,
                    }
                }
            } else {
                match current.regexes.iter().position(|e| e.pattern == seg.str) {
                    Some(i) => current = &current.regexes[i].node,
                    None => return None,
                }
            }
        }
        Some(current)
    }

    // --- matcher ------------------------------------------------------------

    /// Recompile this node's combined regex from its `regexes` list if dirty.
    fn ensure_matcher(&self) -> Result<(), RegexTrieError> {
        if !self.matcher_dirty.get() {
            return Ok(());
        }
        if self.regexes.is_empty() {
            *self.matcher.borrow_mut() = None;
        } else {
            // `^(?:(?P<g0>p0)|(?P<g1>p1)|...)` — anchored at the start of the
            // remaining input; the matching named group identifies which branch.
            let mut pat = String::from("^(?:");
            for (i, e) in self.regexes.iter().enumerate() {
                if i > 0 {
                    pat.push('|');
                }
                pat.push_str("(?P<g");
                pat.push_str(&i.to_string());
                pat.push('>');
                pat.push_str(&e.pattern);
                pat.push(')');
            }
            pat.push(')');
            let re = Regex::new(&pat)
                .map_err(|err| RegexTrieError::BadPattern(format!("{pat}: {err}")))?;
            *self.matcher.borrow_mut() = Some(re);
        }
        self.matcher_dirty.set(false);
        Ok(())
    }

    /// Match the combined regex against `hay`; return `(branch_index, match_len)`
    /// for the first alternative that matched a non-empty prefix.
    fn match_alt(&self, hay: &[u8]) -> Result<Option<(usize, usize)>, RegexTrieError> {
        self.ensure_matcher()?;
        let borrow = self.matcher.borrow();
        let re = match borrow.as_ref() {
            Some(r) => r,
            None => return Ok(None),
        };
        if let Some(caps) = re.captures(hay) {
            let whole = caps.get(0).unwrap();
            if whole.start() == 0 && whole.end() > 0 {
                for i in 0..self.regexes.len() {
                    let name = format!("g{i}");
                    if caps.name(&name).is_some() {
                        return Ok(Some((i, whole.end())));
                    }
                }
            }
        }
        Ok(None)
    }

    // --- insert -------------------------------------------------------------

    /// Insert `key` (a literal or regex pattern) denoting dyad `data`.
    pub fn insert(&mut self, key: &str, data: DyadPtr) -> Result<(), RegexTrieError> {
        debug_assert!(!key.is_empty());

        if is_pure_literal(key) {
            return self.insert_literal_fast(key, data);
        }

        let paths = regex_splitting(key);

        // Phase 1: build structure and reject duplicates before assigning anything.
        for path in &paths {
            let leaf = Self::walk_create(self, path);
            leaf.ensure_eow();
            if leaf.leaf_value.is_some() {
                return Err(RegexTrieError::DuplicateLeafValue);
            }
        }

        // Phase 2: every path of this insert points at the same dyad.
        for path in &paths {
            let leaf = Self::walk_create(self, path);
            leaf.leaf_value = Some(Leaf { regex_key: key.to_string(), data });
        }
        Ok(())
    }

    /// Fast path for pure-literal keys: walk byte-by-byte, no splitting.
    fn insert_literal_fast(&mut self, s: &str, data: DyadPtr) -> Result<(), RegexTrieError> {
        let mut current = self;
        for &c in s.as_bytes() {
            current = current.lit_child_or_create(c);
        }
        current.ensure_eow();
        if current.leaf_value.is_some() {
            return Err(RegexTrieError::DuplicateLeafValue);
        }
        current.leaf_value = Some(Leaf { regex_key: s.to_string(), data });
        Ok(())
    }

    // --- get ----------------------------------------------------------------

    /// Greedy longest-match lookup at the start of `string`. Literal children are
    /// tried before the node's regex branches; the longest reachable EOW wins.
    pub fn get(&self, string: &str) -> Result<MatchResult<'_>, RegexTrieError> {
        debug_assert!(!string.is_empty());
        let bytes = string.as_bytes();

        let mut current: &RegexTrie = self;
        let mut pos = 0usize;
        let mut max_matched = 0usize;
        let mut max_value: Option<&Leaf> = None;

        while pos < bytes.len() {
            let c = bytes[pos];
            let mut advanced = false;
            let mut advance_len = 0usize;

            // Literal child first (O(1) via the index array).
            if c != 0 {
                let idx = current.child_indices[c as usize];
                if idx != NONE {
                    if let Some(child) = &current.children[idx as usize] {
                        current = child;
                        advance_len = 1;
                        advanced = true;
                    }
                }
            }

            if !advanced {
                if !current.regexes.is_empty() {
                    if let Some((w, len)) = current.match_alt(&bytes[pos..])? {
                        current = &current.regexes[w].node;
                        advance_len = len;
                        advanced = true;
                    }
                }
                if !advanced {
                    break;
                }
            }

            pos += advance_len;

            if current.check_eow() {
                max_matched = pos;
                max_value = current.leaf_value.as_ref();
            }
        }

        // No literal-path match: try a regex straight from the root.
        if max_matched == 0 && !self.regexes.is_empty() {
            if let Some((w, len)) = self.match_alt(bytes)? {
                let target = &self.regexes[w].node;
                if target.check_eow() {
                    if let Some(val) = &target.leaf_value {
                        return Ok(MatchResult {
                            matched: len,
                            regex_key: &val.regex_key,
                            data: val.data,
                        });
                    }
                }
            }
        }

        if max_matched > 0 {
            let val = max_value.expect("EOW reached implies a stored value");
            return Ok(MatchResult { matched: max_matched, regex_key: &val.regex_key, data: val.data });
        }
        Err(RegexTrieError::NodeNotFound)
    }

    /// Every match at the start of `string`, exploring all paths. Slower than
    /// [`get`](Self::get); useful for ambiguity inspection.
    pub fn get_all_matches(&self, string: &str) -> Result<Vec<MatchResult<'_>>, RegexTrieError> {
        let bytes = string.as_bytes();
        let mut out: Vec<MatchResult<'_>> = Vec::new();
        let mut stack: Vec<(&RegexTrie, usize)> = vec![(self, 0)];

        while let Some((current, pos)) = stack.pop() {
            if current.check_eow() {
                if let Some(v) = &current.leaf_value {
                    out.push(MatchResult { matched: pos, regex_key: &v.regex_key, data: v.data });
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

            if !current.regexes.is_empty() {
                current.ensure_matcher()?;
                let borrow = current.matcher.borrow();
                if let Some(re) = borrow.as_ref() {
                    if let Some(caps) = re.captures(&bytes[pos..]) {
                        let whole = caps.get(0).unwrap();
                        if whole.start() == 0 && whole.end() > 0 {
                            for i in 0..current.regexes.len() {
                                let name = format!("g{i}");
                                if caps.name(&name).is_some() {
                                    stack.push((&current.regexes[i].node, pos + whole.end()));
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    // --- remove -------------------------------------------------------------

    /// Remove the entry for `regex_key` and return the dyad it denoted. Errors
    /// with [`RegexTrieError::NodeNotFound`] if any of the key's paths is absent
    /// or they do not all point at the same dyad.
    pub fn remove(&mut self, regex_key: &str) -> Result<DyadPtr, RegexTrieError> {
        debug_assert!(!regex_key.is_empty());
        let paths = regex_splitting(regex_key);

        // Verify every path resolves to the same dyad before mutating anything.
        let mut held: Option<DyadPtr> = None;
        for path in &paths {
            let node = self.locate(path).filter(|n| n.check_eow());
            let lv = match node.and_then(|n| n.leaf_value.as_ref()) {
                Some(v) if v.regex_key == regex_key => v,
                _ => return Err(RegexTrieError::NodeNotFound),
            };
            match held {
                None => held = Some(lv.data),
                Some(h) if h == lv.data => {}
                Some(_) => return Err(RegexTrieError::NodeNotFound),
            }
        }

        // Clear the value at each path's leaf and prune now-empty nodes.
        for path in &paths {
            let steps = flatten(path);
            Self::prune_remove(self, &steps, 0);
        }

        Ok(held.expect("at least one path verified"))
    }

    /// Recursively descend `steps` from `node`, clearing the leaf at the end and
    /// pruning empty children on the way back up. Returns true if `node` itself
    /// became empty and the caller should drop the link to it.
    fn prune_remove(node: &mut RegexTrie, steps: &[Step], i: usize) -> bool {
        if i == steps.len() {
            node.leaf_value = None;
            let eow = node.child_indices[EOW];
            if eow != NONE {
                node.child_indices[EOW] = NONE;
                node.children[eow as usize] = None;
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
                    Some(child) => Self::prune_remove(child, steps, i + 1),
                    None => return false,
                };
                if child_empty {
                    node.child_indices[*c as usize] = NONE;
                    node.children[idx as usize] = None;
                }
            }
            Step::Regex(p) => match node.regexes.iter().position(|e| &e.pattern == p) {
                Some(ri) => {
                    let child_empty = Self::prune_remove(&mut node.regexes[ri].node, steps, i + 1);
                    if child_empty {
                        node.matcher_dirty.set(true);
                        node.regexes.remove(ri);
                    }
                }
                None => return false,
            },
        }

        !node.has_children() && node.leaf_value.is_none() && !node.check_eow()
    }

    // --- debug print --------------------------------------------------------

    /// Render the trie structure as text (literal chains compressed), in the
    /// style of the Zig `print`.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.dump_rec(0, &mut out);
        out
    }

    /// Follow a single-literal-child chain, returning the accumulated label and
    /// the node the chain ends at.
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

/// A single navigation step, used by `remove`'s recursive prune.
enum Step {
    Lit(u8),
    Regex(String),
}

/// Flatten a split path into per-byte literal steps and regex steps.
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
mod tests {
    use super::*;
    use crate::dyad::Dyad;

    /// Leak a distinct dyad and return a pointer to it (its address is its id).
    /// Leaking is fine in tests — the process exits.
    fn dummy(tag: usize) -> DyadPtr {
        Box::into_raw(Box::new(Dyad { ty: std::ptr::null_mut(), value: tag as *mut u8 }))
    }

    #[test]
    fn literal_longest_match() {
        // The `a = a + 1` operator set: `:` vs `:=` must disambiguate by length.
        let mut t = RegexTrie::new();
        let (colon, colon_eq, eq, plus) = (dummy(1), dummy(2), dummy(3), dummy(4));
        t.insert(":", colon).unwrap();
        t.insert(":=", colon_eq).unwrap();
        t.insert("=", eq).unwrap();
        t.insert("+", plus).unwrap();

        let m = t.get(":=").unwrap();
        assert_eq!(m.matched, 2);
        assert_eq!(m.regex_key, ":=");
        assert_eq!(m.data, colon_eq);

        let m = t.get(":x").unwrap();
        assert_eq!(m.matched, 1);
        assert_eq!(m.data, colon);

        assert_eq!(t.get("=").unwrap().data, eq);
    }

    #[test]
    fn regex_branch_matches() {
        let mut t = RegexTrie::new();
        let num = dummy(1);
        t.insert("[0-9]+", num).unwrap();
        let m = t.get("123abc").unwrap();
        assert_eq!(m.matched, 3);
        assert_eq!(m.data, num);
    }

    #[test]
    fn literal_beats_regex_at_same_node() {
        // get() commits to the literal child; the regex is only the fallback.
        let mut t = RegexTrie::new();
        let (kw, ident) = (dummy(1), dummy(2));
        t.insert("if", kw).unwrap();
        t.insert("[a-z]+", ident).unwrap();

        assert_eq!(t.get("if").unwrap().data, kw);
        assert_eq!(t.get("foo").unwrap().data, ident);
    }

    #[test]
    fn unknown_input_is_not_found() {
        let mut t = RegexTrie::new();
        t.insert("foo", dummy(1)).unwrap();
        assert!(matches!(t.get("bar"), Err(RegexTrieError::NodeNotFound)));
    }

    #[test]
    fn duplicate_insert_rejected() {
        let mut t = RegexTrie::new();
        t.insert("dup", dummy(1)).unwrap();
        assert_eq!(t.insert("dup", dummy(2)), Err(RegexTrieError::DuplicateLeafValue));
    }

    #[test]
    fn insert_then_use_in_same_pass() {
        // V1PLAN Phase 2: declare a token, then immediately lex a use of it.
        let mut t = RegexTrie::new();
        t.insert("=", dummy(1)).unwrap();
        assert!(t.get("widget").is_err());
        let widget = dummy(42);
        t.insert("widget", widget).unwrap();
        let m = t.get("widget = 1").unwrap();
        assert_eq!(m.matched, 6);
        assert_eq!(m.data, widget);
    }

    #[test]
    fn alternation_shares_one_value() {
        let mut t = RegexTrie::new();
        let d = dummy(7);
        t.insert("ab|cd", d).unwrap();
        assert_eq!(t.get("ab").unwrap().matched, 2);
        assert_eq!(t.get("ab").unwrap().data, d);
        assert_eq!(t.get("cd").unwrap().data, d);

        // Removing the key returns the shared dyad and drops both paths.
        assert_eq!(t.remove("ab|cd").unwrap(), d);
        assert!(t.get("ab").is_err());
        assert!(t.get("cd").is_err());
    }

    #[test]
    fn remove_literal_then_gone() {
        let mut t = RegexTrie::new();
        let (foo, foobar) = (dummy(1), dummy(2));
        t.insert("foo", foo).unwrap();
        t.insert("foobar", foobar).unwrap();
        assert_eq!(t.remove("foo").unwrap(), foo);
        assert!(t.get("foo").is_err());
        // The longer key sharing the prefix survives.
        assert_eq!(t.get("foobar").unwrap().data, foobar);
    }

    #[test]
    fn remove_missing_is_error() {
        let mut t = RegexTrie::new();
        t.insert("foo", dummy(1)).unwrap();
        assert_eq!(t.remove("bar"), Err(RegexTrieError::NodeNotFound));
    }

    #[test]
    fn get_all_matches_finds_every_path() {
        let mut t = RegexTrie::new();
        let (lit_a, ident) = (dummy(1), dummy(2));
        t.insert("a", lit_a).unwrap();
        t.insert("[a-z]+", ident).unwrap();

        let mut ms = t.get_all_matches("abc").unwrap();
        ms.sort_by_key(|m| m.matched);
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].matched, 1);
        assert_eq!(ms[0].data, lit_a);
        assert_eq!(ms[1].matched, 3);
        assert_eq!(ms[1].data, ident);
    }

    #[test]
    fn bad_pattern_surfaces_error() {
        // Lookaround is unsupported by the `regex` crate; report it cleanly.
        let mut t = RegexTrie::new();
        t.insert("(?=foo)", dummy(1)).unwrap();
        match t.get("foobar") {
            Err(RegexTrieError::BadPattern(_)) => {}
            other => panic!("expected BadPattern, got {other:?}"),
        }
    }
}
