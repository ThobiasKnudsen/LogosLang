// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Splitting a regex pattern into literal runs and residual regex segments for
//! the trie: alternation yields several paths, each an ordered list of
//! [`Segment`]s.

use regex_syntax::hir::{Class, Hir, HirKind, Literal};

/// Cap on path explosion from cartesian alternation.
const MAX_PATHS: usize = 1000;

/// A literal run or a residual regex chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub str: String,
    pub is_lit: bool,
}

impl Segment {
    fn lit(s: &str) -> Segment {
        Segment { str: s.to_string(), is_lit: true }
    }
    fn rx(s: &str) -> Segment {
        Segment { str: s.to_string(), is_lit: false }
    }
}

fn bytes_to_string(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Literal arms consume whole UTF-8 sequences and copy their raw bytes: `lead
/// as char` would re-encode a non-ASCII byte as different bytes, corrupting `«`.
fn utf8_len(lead: u8) -> usize {
    match lead {
        b if b < 0x80 => 1,
        b if b >= 0xF0 => 4,
        b if b >= 0xE0 => 3,
        _ => 2,
    }
}

/// No regex metacharacters: such patterns go straight to the trie's byte-path.
pub fn is_pure_literal(s: &str) -> bool {
    for &c in s.as_bytes() {
        match c {
            b'.' | b'*' | b'+' | b'?' | b'|' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'^'
            | b'$' | b'\\' => return false,
            _ => {}
        }
    }
    true
}

/// `(min, max)` of a quantifier, `usize::MAX` standing for infinity.
fn get_quant_n(q: &str) -> (usize, usize) {
    const INF: usize = usize::MAX;
    match q {
        "*" => return (0, INF),
        "+" => return (1, INF),
        "?" => return (0, 1),
        _ => {}
    }
    let b = q.as_bytes();
    if b.len() > 1 && b[0] == b'{' && b[b.len() - 1] == b'}' {
        let inner = &q[1..q.len() - 1];
        if let Some(comma) = inner.find(',') {
            let min_s = &inner[..comma];
            let max_s = &inner[comma + 1..];
            let min_n = if !min_s.is_empty() { min_s.parse().unwrap_or(0) } else { 0 };
            let max_n = if !max_s.is_empty() { max_s.parse().unwrap_or(INF) } else { INF };
            return (min_n, max_n);
        } else {
            let n = inner.parse().unwrap_or(0);
            return (n, n);
        }
    }
    (0, 0)
}

/// The trie matches a path segment by segment and never backs up between
/// them, so a literal the regex chunk before it could eat (`[a-z]+` eats the
/// `ing` of `running`) stays inside that chunk, escaped.
fn keep_eatable_literals(path: &mut [Segment]) {
    let mut chunk = String::new();
    for seg in path.iter_mut() {
        if seg.is_lit {
            if chunk.is_empty() || !may_eat(&chunk, &seg.str) {
                chunk.clear();
                continue;
            }
            seg.str = regex::escape(&seg.str);
            seg.is_lit = false;
        }
        chunk.push_str(&seg.str);
    }
}

/// Whether `chunk` run on its own can end elsewhere than where `literal`
/// needs it to: some leaf of it matches the literal's first char, or its own
/// preferences can stop it early (a lazy repetition, an alternation, or a
/// second repetition of varying length: `a*(ab)?` on `aab` stops at 2). A
/// chunk the syntax parser rejects is kept whole; the trie reports it at
/// lookup.
fn may_eat(chunk: &str, literal: &str) -> bool {
    let Some(first) = literal.chars().next() else {
        return false;
    };
    let mut buf = [0u8; 4];
    let first_bytes = first.encode_utf8(&mut buf).as_bytes();
    let Ok(hir) = regex_syntax::ParserBuilder::new().utf8(false).build().parse(chunk) else {
        return true;
    };
    // `varying` counts the repetitions of varying length, a repetition around
    // one counting as well.
    fn walk(hir: &Hir, first: char, first_bytes: &[u8], varying: &mut usize) -> bool {
        match hir.kind() {
            HirKind::Empty | HirKind::Look(_) => false,
            HirKind::Literal(Literal(bytes)) => {
                bytes.windows(first_bytes.len()).any(|w| w == first_bytes)
            }
            HirKind::Class(Class::Unicode(class)) => {
                class.ranges().iter().any(|r| r.start() <= first && first <= r.end())
            }
            HirKind::Class(Class::Bytes(class)) => first_bytes
                .iter()
                .any(|&b| class.ranges().iter().any(|r| r.start() <= b && b <= r.end())),
            HirKind::Repetition(rep) => {
                let inner = *varying;
                let eats = walk(&rep.sub, first, first_bytes, varying);
                if rep.max != Some(rep.min) || *varying > inner {
                    *varying += 1;
                }
                !rep.greedy || eats
            }
            HirKind::Capture(cap) => walk(&cap.sub, first, first_bytes, varying),
            HirKind::Concat(subs) => subs.iter().any(|s| walk(s, first, first_bytes, varying)),
            HirKind::Alternation(_) => true,
        }
    }
    let mut varying = 0;
    walk(&hir, first, first_bytes, &mut varying) || varying > 1
}

/// A run of single literal chars becomes one literal segment.
fn merge_adjacent(path: &mut Vec<Segment>) {
    if path.is_empty() {
        return;
    }
    let mut out: Vec<Segment> = Vec::with_capacity(path.len());
    for seg in path.drain(..) {
        if let Some(last) = out.last_mut() {
            if last.is_lit == seg.is_lit {
                last.str.push_str(&seg.str);
                continue;
            }
        }
        out.push(seg);
    }
    *path = out;
}

fn parse_atom(s: &[u8], pos: &mut usize) -> Vec<Vec<Segment>> {
    let mut result: Vec<Vec<Segment>> = Vec::new();
    if *pos >= s.len() {
        return result;
    }

    let c = s[*pos];
    if c == b'\\' {
        *pos += 1;
        if *pos >= s.len() {
            result.push(vec![Segment::lit("\\")]);
            return result;
        }
        let esc = s[*pos];
        *pos += 1;
        match esc {
            b'd' | b'D' | b'w' | b'W' | b's' | b'S' | b'b' | b'B' | b'A' | b'Z' | b'z' | b'R' => {
                let mut t = String::from("\\");
                t.push(esc as char);
                result.push(vec![Segment::rx(&t)]);
            }
            b'p' | b'P' => {
                if *pos < 2 {
                    result.push(vec![Segment::lit(&(esc as char).to_string())]);
                    return result;
                }
                let start = *pos - 2;
                while *pos < s.len() && s[*pos] != b'}' {
                    *pos += 1;
                }
                if *pos < s.len() {
                    *pos += 1;
                }
                result.push(vec![Segment::rx(&bytes_to_string(&s[start..*pos]))]);
            }
            b'Q' => {
                let q_start = *pos;
                let mut closed = false;
                while *pos + 1 < s.len() {
                    if s[*pos] == b'\\' && s[*pos + 1] == b'E' {
                        result.push(vec![Segment::lit(&bytes_to_string(&s[q_start..*pos]))]);
                        *pos += 2;
                        closed = true;
                        break;
                    }
                    *pos += 1;
                }
                if !closed {
                    result.push(vec![Segment::lit(&bytes_to_string(&s[q_start..]))]);
                    *pos = s.len();
                }
            }
            _ => {
                // An escaped literal char, byte-preserving; see `utf8_len`.
                let start = *pos - 1;
                let end = (start + utf8_len(esc)).min(s.len());
                *pos = end;
                result.push(vec![Segment::lit(&bytes_to_string(&s[start..end]))]);
            }
        }
        result
    } else if c == b'.' {
        *pos += 1;
        result.push(vec![Segment::rx(".")]);
        result
    } else if c == b'[' {
        let start = *pos;
        *pos += 1;
        let mut empty_class = true;
        while *pos < s.len() && s[*pos] != b']' {
            empty_class = false;
            if s[*pos] == b'\\' && *pos + 1 < s.len() {
                *pos += 1;
            }
            if *pos < s.len() {
                *pos += 1;
            }
        }
        if *pos < s.len() && s[*pos] == b']' {
            *pos += 1;
        }
        let class = if empty_class { "[]".to_string() } else { bytes_to_string(&s[start..*pos]) };
        result.push(vec![Segment::rx(&class)]);
        result
    } else if c == b'(' {
        *pos += 1;
        let mut is_look = false;
        let look_pos = *pos;
        if *pos + 1 < s.len() && s[*pos] == b'?' {
            *pos += 1;
            if *pos >= s.len() {
                if *pos > 0 {
                    *pos -= 1;
                }
            } else {
                let next = s[*pos];
                if next == b'=' || next == b'!' || next == b':' {
                    is_look = true;
                } else if *pos > 0 {
                    *pos -= 1;
                }
            }
        }
        if is_look {
            if *pos < 2 {
                *pos = look_pos;
            } else {
                let group_start = *pos - 2;
                let mut level: i32 = 1;
                while *pos < s.len() {
                    if s[*pos] == b'(' {
                        level += 1;
                    } else if s[*pos] == b')' {
                        level -= 1;
                        if level == 0 {
                            break;
                        }
                    }
                    *pos += 1;
                }
                let mut length = *pos - group_start;
                if *pos < s.len() && s[*pos] == b')' {
                    length += 1;
                    *pos += 1;
                } else {
                    *pos = s.len();
                }
                result.push(vec![Segment::rx(&bytes_to_string(
                    &s[group_start..group_start + length],
                ))]);
                return result;
            }
        }
        // A capturing group: parse its contents, drop the parens.
        let mut paths = parse_re(s, pos);
        if *pos < s.len() && s[*pos] == b')' {
            *pos += 1;
        } else {
            let tail = bytes_to_string(&s[*pos..]);
            for path in paths.iter_mut() {
                path.push(Segment::rx(&tail));
            }
        }
        paths
    } else if c == b'^' || c == b'$' {
        *pos += 1;
        result.push(vec![Segment::rx(&(c as char).to_string())]);
        result
    } else {
        // A plain literal char: the whole UTF-8 sequence, byte-preserving.
        let end = (*pos + utf8_len(c)).min(s.len());
        let lit = bytes_to_string(&s[*pos..end]);
        *pos = end;
        result.push(vec![Segment::lit(&lit)]);
        result
    }
}

fn parse_term(s: &[u8], pos: &mut usize) -> Vec<Vec<Segment>> {
    let paths = parse_atom(s, pos);
    if paths.is_empty() {
        return paths;
    }

    let quant_begin = *pos;
    let mut has_quant = false;
    if *pos < s.len() {
        let qc = s[*pos];
        if qc == b'*' || qc == b'+' || qc == b'?' {
            *pos += 1;
            has_quant = true;
        } else if qc == b'{' {
            let brace_start = *pos;
            *pos += 1;
            while *pos < s.len() && s[*pos].is_ascii_digit() {
                *pos += 1;
            }
            if *pos < s.len() && s[*pos] == b',' {
                *pos += 1;
            }
            while *pos < s.len() && s[*pos].is_ascii_digit() {
                *pos += 1;
            }
            if *pos < s.len() && s[*pos] == b'}' {
                *pos += 1;
                has_quant = true;
            } else {
                *pos = brace_start;
            }
        }
    }
    if !has_quant {
        return paths;
    }

    let quant_str = bytes_to_string(&s[quant_begin..*pos]);
    // A trailing `?` (lazy) or `+` (possessive) becomes part of the carried quant.
    if *pos < s.len() && (s[*pos] == b'?' || s[*pos] == b'+') {
        *pos += 1;
    }
    let full_quant = bytes_to_string(&s[quant_begin..*pos]);

    let (min, max) = get_quant_n(&quant_str);

    if min == 0 && max == 0 {
        // A degenerate quantifier (`{0}`): carry the remainder as a regex tail.
        let tail = bytes_to_string(&s[quant_begin..]);
        let mut paths = paths;
        for path in paths.iter_mut() {
            path.push(Segment::rx(&tail));
        }
        *pos = s.len();
        return paths;
    }

    // Small fixed repetition expands by cartesian product, bounded by MAX_PATHS
    // each round so `(a|b|c|d|e){9}` cannot explode before the cap applies.
    if max != usize::MAX && max == min && min > 0 && min < 10 {
        let mut repeated: Vec<Vec<Segment>> = vec![Vec::new()];
        for _ in 0..min {
            let mut new_rep: Vec<Vec<Segment>> = Vec::new();
            'outer: for pre in &repeated {
                for p in &paths {
                    if new_rep.len() >= MAX_PATHS {
                        break 'outer;
                    }
                    let mut np = pre.clone();
                    np.extend(p.iter().cloned());
                    merge_adjacent(&mut np);
                    new_rep.push(np);
                }
            }
            repeated = new_rep;
        }
        return repeated;
    }

    // Variable repetition: attach the quantifier to the last segment of each path.
    let mut new_paths: Vec<Vec<Segment>> = Vec::new();
    for p in &paths {
        let mut np = p.clone();
        if let Some(last) = np.last_mut() {
            // A multi-char literal is a merged group (`(ab)`): quantified bare it
            // would misparse `(ab)+` as `ab+`, so it is wrapped.
            if last.is_lit && last.str.len() > 1 {
                last.str = format!("(?:{}){}", last.str, full_quant);
            } else {
                last.str.push_str(&full_quant);
            }
            last.is_lit = false;
        } else {
            np.push(Segment::rx(&full_quant));
        }
        merge_adjacent(&mut np);
        new_paths.push(np);
    }
    if min == 0 {
        // The empty path last, so longer alternatives win first.
        new_paths.push(Vec::new());
    }
    new_paths
}

fn parse_concat(s: &[u8], pos: &mut usize) -> Vec<Vec<Segment>> {
    let mut sub_groups: Vec<Vec<Vec<Segment>>> = Vec::new();
    while *pos < s.len() && s[*pos] != b'|' && s[*pos] != b')' {
        let sub = parse_term(s, pos);
        if !sub.is_empty() {
            sub_groups.push(sub);
        } else {
            break;
        }
    }

    let mut current: Vec<Vec<Segment>> = vec![Vec::new()];
    for group in &sub_groups {
        let mut new_current: Vec<Vec<Segment>> = Vec::new();
        'outer: for prefix in &current {
            for suffix in group {
                if new_current.len() >= MAX_PATHS {
                    break 'outer;
                }
                let mut np = prefix.clone();
                np.extend(suffix.iter().cloned());
                merge_adjacent(&mut np);
                new_current.push(np);
            }
        }
        current = new_current;
        if current.is_empty() {
            return current;
        }
    }
    current
}

fn parse_alt(s: &[u8], pos: &mut usize) -> Vec<Vec<Segment>> {
    let mut paths = parse_concat(s, pos);
    while *pos < s.len() && s[*pos] == b'|' {
        *pos += 1;
        let sub = parse_concat(s, pos);
        for sp in sub {
            paths.push(sp);
        }
    }
    if *pos < s.len() && s[*pos] != b')' {
        let tail = bytes_to_string(&s[*pos..]);
        for path in paths.iter_mut() {
            path.push(Segment::rx(&tail));
        }
    }
    paths
}

fn parse_re(s: &[u8], pos: &mut usize) -> Vec<Vec<Segment>> {
    parse_alt(s, pos)
}

/// The paths the trie inserts for `pattern`; a pure literal is one one-segment path.
pub fn regex_splitting(pattern: &str) -> Vec<Vec<Segment>> {
    if is_pure_literal(pattern) {
        return vec![vec![Segment::lit(pattern)]];
    }

    let s = pattern.as_bytes();
    let mut pos = 0usize;
    let mut paths = parse_re(s, &mut pos);

    if pos > s.len() {
        pos = s.len();
    }
    if pos < s.len() {
        let tail = bytes_to_string(&s[pos..]);
        if paths.is_empty() {
            paths.push(vec![Segment::rx(&tail)]);
        } else {
            for path in paths.iter_mut() {
                path.push(Segment::rx(&tail));
            }
        }
    }
    for path in paths.iter_mut() {
        keep_eatable_literals(path);
        merge_adjacent(path);
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(s: &str) -> Segment {
        Segment { str: s.to_string(), is_lit: true }
    }
    fn rx(s: &str) -> Segment {
        Segment { str: s.to_string(), is_lit: false }
    }

    #[test]
    fn pure_literal_detection() {
        assert!(is_pure_literal(":="));
        assert!(is_pure_literal("hello"));
        assert!(!is_pure_literal("[0-9]"));
        assert!(!is_pure_literal("a|b"));
        assert!(!is_pure_literal("a.b"));
    }

    #[test]
    fn pure_literal_single_path() {
        assert_eq!(regex_splitting(":="), vec![vec![lit(":=")]]);
    }

    #[test]
    fn literal_prefix_then_regex() {
        assert_eq!(regex_splitting("ab[0-9]+"), vec![vec![lit("ab"), rx("[0-9]+")]]);
    }

    #[test]
    fn alternation_splits_into_paths() {
        assert_eq!(regex_splitting("ab|cd"), vec![vec![lit("ab")], vec![lit("cd")]]);
    }

    #[test]
    fn optional_appends_empty_path_last() {
        // The empty tail is appended last, so the longer path wins.
        assert_eq!(regex_splitting("ab?"), vec![vec![lit("a"), rx("b?")], vec![lit("a")]]);
    }

    #[test]
    fn fixed_repetition_expands() {
        assert_eq!(regex_splitting("a{3}"), vec![vec![lit("aaa")]]);
    }

    #[test]
    fn capturing_group_repetition_keeps_grouping() {
        assert_eq!(regex_splitting("(ab)+"), vec![vec![rx("(?:ab)+")]]);
    }

    #[test]
    fn a_literal_the_chunk_can_eat_stays_in_the_chunk() {
        assert_eq!(regex_splitting("[a-z]+ing"), vec![vec![rx("[a-z]+ing")]]);
        assert_eq!(regex_splitting("[0-9]+0"), vec![vec![rx("[0-9]+0")]]);
        // Every atom of the chunk counts, not only the one before the literal.
        assert_eq!(
            regex_splitting("[a-z]+ing[0-9]*a"),
            vec![vec![rx("[a-z]+ing[0-9]*a")], vec![rx("[a-z]+inga")]]
        );
        // The literal is escaped as it joins the chunk.
        assert_eq!(regex_splitting("[a-z.]+\\.x"), vec![vec![rx("[a-z.]+\\.x")]]);
    }

    #[test]
    fn a_literal_the_chunk_cannot_eat_splits_off() {
        assert_eq!(regex_splitting("a[0-9]+b"), vec![vec![lit("a"), rx("[0-9]+"), lit("b")]]);
        assert_eq!(regex_splitting("x+y"), vec![vec![rx("x+"), lit("y")]]);
        assert_eq!(
            regex_splitting("[0-9]+\\.[0-9]+"),
            vec![vec![rx("[0-9]+"), lit("."), rx("[0-9]+")]]
        );
    }

    #[test]
    fn a_lazy_or_alternating_chunk_keeps_its_literal() {
        assert_eq!(regex_splitting("[0-9]+?x"), vec![vec![rx("[0-9]+?x")]]);
        assert_eq!(regex_splitting("(?:a|ab)c"), vec![vec![rx("(?:a|ab)c")]]);
    }

    #[test]
    fn a_chunk_with_two_varying_repetitions_keeps_its_literal() {
        assert_eq!(
            regex_splitting("a*(ab)?c"),
            vec![
                vec![rx("a*(?:ab)?c")],
                vec![rx("a*"), lit("c")],
                vec![rx("(?:ab)?"), lit("c")],
                vec![lit("c")]
            ]
        );
        assert_eq!(
            regex_splitting("[0-9]+(?:[0-9]k)?x"),
            vec![vec![rx("[0-9]+(?:[0-9]k)?x")], vec![rx("[0-9]+"), lit("x")]]
        );
        // A fixed count around a varying repetition counts as a second one.
        assert_eq!(regex_splitting("(?:a*b){12}x"), vec![vec![rx("(?:a*b){12}x")]]);
        // One varying repetition alone still lets the literal split off.
        assert_eq!(regex_splitting("[0-9]{2,5}x"), vec![vec![rx("[0-9]{2,5}"), lit("x")]]);
    }

    #[test]
    fn single_char_and_class_repetition_are_left_bare() {
        assert_eq!(regex_splitting("ab?"), vec![vec![lit("a"), rx("b?")], vec![lit("a")]]);
        assert_eq!(regex_splitting("[0-9]+"), vec![vec![rx("[0-9]+")]]);
    }
}
