// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Splitting a regex pattern into literal runs and residual regex chunks for
//! the trie: alternation yields several paths, each an ordered list of
//! [`Segment`]s. The pattern is read once, by `regex-syntax`, and the split
//! walks the tree it gives.

use regex_syntax::hir::{Capture, Class, Hir, HirKind, Literal, Repetition};

/// A part whose paths would pass this many stays one chunk.
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

/// The pattern as the trie's engine reads it, over any bytes.
fn parse(pattern: &str) -> Option<Hir> {
    regex_syntax::ParserBuilder::new().utf8(false).build().parse(pattern).ok()
}

/// No regex metacharacters: such patterns go straight to the trie's byte-path.
pub fn is_pure_literal(s: &str) -> bool {
    !s.chars().any(regex_syntax::is_meta_character)
}

/// The trie matches a path segment by segment and never backs up between
/// them, so a literal the regex chunk before it could eat stays inside that
/// chunk, escaped.
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

/// Whether `chunk` run alone can end elsewhere than where `literal` needs it
/// to: a leaf matches the literal's first char, or a lazy repetition, an
/// alternation or a second repetition of varying length can stop it early.
/// A chunk the parser rejects is kept whole; the trie reports it at lookup.
fn may_eat(chunk: &str, literal: &str) -> bool {
    let Some(first) = literal.chars().next() else {
        return false;
    };
    let mut buf = [0u8; 4];
    let first_bytes = first.encode_utf8(&mut buf).as_bytes();
    let Some(hir) = parse(chunk) else {
        return true;
    };
    // `varying` counts the repetitions of varying length, a repetition around
    // one counting as well.
    fn walk(hir: &Hir, first: char, first_bytes: &[u8], varying: &mut usize) -> bool {
        match hir.kind() {
            HirKind::Empty | HirKind::Look(_) => false,
            // A byte literal can end inside a char; a text literal cannot.
            HirKind::Literal(Literal(bytes)) => match std::str::from_utf8(bytes) {
                Ok(text) => text.contains(first),
                Err(_) => first_bytes.iter().any(|b| bytes.contains(b)),
            },
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

/// `hir` as one regex chunk, in the text `regex-syntax` prints for it.
fn chunk(hir: &Hir) -> Segment {
    Segment::rx(&bare(hir).to_string())
}

/// `hir` without capture groups, which change nothing a chunk matches, while a
/// named one a count repeats would be declared twice in the chunk. A repetition
/// of a repetition keeps a group, since the printer writes `(?:a+)?` as `a+?`.
fn bare(hir: &Hir) -> Hir {
    match hir.kind() {
        HirKind::Capture(cap) => bare(&cap.sub),
        HirKind::Concat(subs) => Hir::concat(subs.iter().map(bare).collect()),
        HirKind::Alternation(subs) => Hir::alternation(subs.iter().map(bare).collect()),
        HirKind::Repetition(rep) => {
            let mut sub = bare(&rep.sub);
            if matches!(sub.kind(), HirKind::Repetition(_)) {
                sub = Hir::capture(Capture { index: 1, name: None, sub: Box::new(sub) });
            }
            Hir::repetition(Repetition {
                min: rep.min,
                max: rep.max,
                greedy: rep.greedy,
                sub: Box::new(sub),
            })
        }
        _ => hir.clone(),
    }
}

/// Each prefix followed by each suffix, or `None` past [`MAX_PATHS`].
fn product(prefixes: &[Vec<Segment>], suffixes: &[Vec<Segment>]) -> Option<Vec<Vec<Segment>>> {
    if prefixes.len() * suffixes.len() > MAX_PATHS {
        return None;
    }
    let mut out = Vec::with_capacity(prefixes.len() * suffixes.len());
    for prefix in prefixes {
        for suffix in suffixes {
            let mut path = prefix.clone();
            path.extend(suffix.iter().cloned());
            merge_adjacent(&mut path);
            out.push(path);
        }
    }
    Some(out)
}

/// Every way through `hir`, at most [`MAX_PATHS`] of them.
fn paths_of(hir: &Hir) -> Vec<Vec<Segment>> {
    let paths = match hir.kind() {
        HirKind::Empty => Some(vec![Vec::new()]),
        HirKind::Literal(Literal(bytes)) => match std::str::from_utf8(bytes) {
            // Byte 0 is the trie's end-of-word mark.
            Ok(text) if !text.contains('\0') => Some(vec![vec![Segment::lit(text)]]),
            _ => None,
        },
        HirKind::Class(_) | HirKind::Look(_) => None,
        HirKind::Capture(cap) => Some(paths_of(&cap.sub)),
        HirKind::Concat(subs) => {
            subs.iter().try_fold(vec![Vec::new()], |paths, sub| product(&paths, &paths_of(sub)))
        }
        HirKind::Alternation(subs) => {
            let paths: Vec<_> = subs.iter().flat_map(paths_of).collect();
            (paths.len() <= MAX_PATHS).then_some(paths)
        }
        // A small exact count is the atom written that many times; any other
        // quantifier covers its whole atom, so the atom stays one chunk.
        HirKind::Repetition(rep) if rep.max == Some(rep.min) && rep.min < 10 => {
            let once = paths_of(&rep.sub);
            (0..rep.min).try_fold(vec![Vec::new()], |paths, _| product(&paths, &once))
        }
        HirKind::Repetition(_) => None,
    };
    paths.unwrap_or_else(|| vec![vec![chunk(hir)]])
}

/// The paths the trie inserts for `pattern`; a pure literal is one one-segment
/// path. A pattern the parser rejects, or one that matches only nothing, is one
/// chunk: the trie reports the first at lookup, and the second never lexes.
pub fn regex_splitting(pattern: &str) -> Vec<Vec<Segment>> {
    if is_pure_literal(pattern) {
        return vec![vec![Segment::lit(pattern)]];
    }
    let mut paths = parse(pattern).map(|hir| paths_of(&hir)).unwrap_or_default();
    // A match of nothing is no match, so an empty path would end at a leaf no lex reaches.
    paths.retain(|path| !path.is_empty());
    if paths.is_empty() {
        return vec![vec![Segment::rx(pattern)]];
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
        assert_eq!(regex_splitting("(?:a|ab)c"), vec![vec![lit("ac")], vec![lit("abc")]]);
    }

    #[test]
    fn an_optional_atom_is_one_chunk_that_may_match_nothing() {
        assert_eq!(regex_splitting("ab?"), vec![vec![lit("a"), rx("b?")]]);
        assert_eq!(regex_splitting("a*"), vec![vec![rx("a*")]]);
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
    fn a_quantifier_covers_its_whole_group() {
        assert_eq!(regex_splitting("(a[0-9])+"), vec![vec![rx("(?:a[0-9])+")]]);
        assert_eq!(regex_splitting("([a-z]+ing)?"), vec![vec![rx("(?:[a-z]+(?:ing))?")]]);
        assert_eq!(regex_splitting("(a|b)+c"), vec![vec![rx("[ab]+"), lit("c")]]);
    }

    #[test]
    fn a_quantified_escaped_char_keeps_its_escape() {
        assert_eq!(regex_splitting("(a\\.b)+"), vec![vec![rx("(?:a\\.b)+")]]);
        assert_eq!(regex_splitting("\\.+"), vec![vec![rx("\\.+")]]);
    }

    #[test]
    fn a_zero_count_drops_its_atom() {
        assert_eq!(regex_splitting("xb{0}c"), vec![vec![lit("xc")]]);
    }

    #[test]
    fn an_escape_is_read_as_the_regex_crate_reads_it() {
        assert_eq!(regex_splitting("k\\x41"), vec![vec![lit("kA")]]);
        assert_eq!(regex_splitting("k\\u{41}"), vec![vec![lit("kA")]]);
        assert_eq!(regex_splitting("k\\t"), vec![vec![lit("k\t")]]);
        assert_eq!(regex_splitting("k\\n"), vec![vec![lit("k\n")]]);
    }

    #[test]
    fn an_inline_flag_applies_to_what_it_covers() {
        assert_eq!(regex_splitting("(?i)ab"), vec![vec![rx("[Aa][Bb]")]]);
        assert_eq!(regex_splitting("k(?-u:[a-z])"), vec![vec![lit("k"), rx("(?-u:[a-z])")]]);
        assert_eq!(regex_splitting("(?x) a b [0-9]"), vec![vec![lit("ab"), rx("[0-9]")]]);
    }

    #[test]
    fn a_byte_the_byte_path_cannot_hold_is_a_chunk() {
        assert_eq!(regex_splitting("a\\x00"), vec![vec![rx("(?:a\u{0})")]]);
        assert_eq!(regex_splitting("(?-u:\\xFF)"), vec![vec![rx("(?-u:\\xFF)")]]);
    }

    #[test]
    fn a_repetition_of_a_repetition_keeps_its_group() {
        assert_eq!(regex_splitting("(?:a+)?"), vec![vec![rx("(a+)?")]]);
        assert_eq!(regex_splitting("(?:a{2})?"), vec![vec![rx("(a{2})?")]]);
    }

    #[test]
    fn a_count_names_its_group_once() {
        assert_eq!(regex_splitting("(?:(?P<n>a)+){2}"), vec![vec![rx("a+a+")]]);
    }

    #[test]
    fn a_part_past_the_path_cap_stays_one_chunk() {
        assert_eq!(
            regex_splitting("(ab|cd|ef|gh|ij){5}"),
            vec![vec![rx("(?:(?:ab)|(?:cd)|(?:ef)|(?:gh)|(?:ij)){5}")]]
        );
        assert_eq!(regex_splitting("(ab|cd|ef){4}").len(), 81);
    }

    #[test]
    fn a_pattern_without_a_path_of_its_own_is_one_chunk() {
        assert_eq!(regex_splitting("(?:)"), vec![vec![rx("(?:)")]]);
        assert_eq!(regex_splitting("a{0}"), vec![vec![rx("a{0}")]]);
        assert_eq!(regex_splitting("(?=foo)"), vec![vec![rx("(?=foo)")]]);
        assert_eq!(regex_splitting("a|"), vec![vec![lit("a")]]);
    }

    #[test]
    fn a_literal_the_chunk_can_eat_stays_in_the_chunk() {
        assert_eq!(regex_splitting("[a-z]+ing"), vec![vec![rx("[a-z]+ing")]]);
        assert_eq!(regex_splitting("[0-9]+0"), vec![vec![rx("[0-9]+0")]]);
        // Every atom of the chunk counts, not only the one before the literal.
        assert_eq!(regex_splitting("[a-z]+ing[0-9]*a"), vec![vec![rx("[a-z]+ing[0-9]*a")]]);
        // The literal is escaped as it joins the chunk.
        assert_eq!(regex_splitting("[a-z.]+\\.x"), vec![vec![rx("[\\.a-z]+\\.x")]]);
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
        assert_eq!(regex_splitting("(?:a|ab)+c"), vec![vec![rx("(?:a|(?:ab))+c")]]);
    }

    #[test]
    fn a_chunk_with_two_varying_repetitions_keeps_its_literal() {
        assert_eq!(regex_splitting("a*(ab)?c"), vec![vec![rx("a*(?:ab)?c")]]);
        assert_eq!(regex_splitting("[0-9]+(?:[0-9]k)?x"), vec![vec![rx("[0-9]+(?:[0-9]k)?x")]]);
        // A fixed count around a varying repetition counts as a second one.
        assert_eq!(regex_splitting("(?:a*b){12}x"), vec![vec![rx("(?:a*b){12}x")]]);
        // One varying repetition alone still lets the literal split off.
        assert_eq!(regex_splitting("[0-9]{2,5}x"), vec![vec![rx("[0-9]{2,5}"), lit("x")]]);
    }

    #[test]
    fn may_eat_judges_byte_classes_multibyte_chars_and_looks() {
        assert!(may_eat("(?-u:[a-z])+", "ing"));
        assert!(!may_eat("[a-z]+", "ø"));
        assert!(may_eat("[a-zø]+", "ø"));
        assert!(may_eat("(?-u:\\xC3)*", "ø"));
        assert!(!may_eat("é+", "ø"));
        assert!(may_eat("aø+", "ø"));
        assert!(!may_eat("[a-z]+\\b", "."));
        assert!(may_eat("[a-z]+\\b", "x"));
    }

    #[test]
    fn single_char_and_class_repetition_are_left_bare() {
        assert_eq!(regex_splitting("ab?"), vec![vec![lit("a"), rx("b?")]]);
        assert_eq!(regex_splitting("[0-9]+"), vec![vec![rx("[0-9]+")]]);
    }
}
