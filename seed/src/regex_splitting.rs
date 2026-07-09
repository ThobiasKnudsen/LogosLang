//! Splitting a regex pattern into literal-prefix paths and residual regex
//! segments — a port of `regex_splitting.zig`.
//!
//! A pattern becomes a list of *paths* (alternation produces several); each path
//! is an ordered list of [`Segment`]s, every segment either a pure literal run
//! (pushed into the trie byte-by-byte) or a residual regex chunk (kept as a
//! regex branch). Rust ownership replaces the Zig allocator/deinit bookkeeping,
//! so the logic is the same while the memory plumbing is gone.

/// Cap on path explosion from cartesian alternation, matching the Zig original.
const MAX_PATHS: usize = 1000;

/// One piece of a split pattern: either a literal run or a residual regex chunk.
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

/// A literal has no regex metacharacters; such patterns bypass splitting and
/// go straight into the trie's literal byte-path.
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

/// Decode a base quantifier into `(min, max)`, `usize::MAX` standing for infinity.
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

/// Collapse adjacent segments of the same kind into one (so a run of single
/// literal chars becomes one literal segment).
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
                result.push(vec![Segment::lit(&(esc as char).to_string())]);
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
                result.push(vec![Segment::rx(&bytes_to_string(&s[group_start..group_start + length]))]);
                return result;
            }
        }
        // Standard (capturing) group: parse its contents, drop the parens.
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
        *pos += 1;
        result.push(vec![Segment::lit(&(c as char).to_string())]);
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
        // Degenerate quantifier (e.g. `{0}`): carry the remainder as a regex tail.
        let tail = bytes_to_string(&s[quant_begin..]);
        let mut paths = paths;
        for path in paths.iter_mut() {
            path.push(Segment::rx(&tail));
        }
        *pos = s.len();
        return paths;
    }

    // Small fixed repetition: expand by cartesian product. Bounded by MAX_PATHS
    // each round (as parse_concat's alternation is) so a pathological pattern like
    // `(a|b|c|d|e){9}` cannot blow up to millions of paths before any cap applies;
    // the expansion is truncated rather than allowed to explode.
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
            // A multi-char *literal* segment is a capturing group's merged contents
            // (e.g. `(ab)`); attaching the quantifier bare would misparse `(ab)+`
            // as `ab+` (`a` then `b+`), so wrap it in a non-capturing group. A
            // single char or an already-regex atom (`[0-9]`, `\d`, `(?:…)`) quantifies
            // correctly as-is.
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
        // Optional: append the empty path last so longer alternatives win first.
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

/// Split `pattern` into the paths the trie inserts. Pure literals short-circuit
/// to a single one-segment path.
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
        // `ab[0-9]+` -> one path: literal "ab" then regex "[0-9]+".
        assert_eq!(regex_splitting("ab[0-9]+"), vec![vec![lit("ab"), rx("[0-9]+")]]);
    }

    #[test]
    fn alternation_splits_into_paths() {
        assert_eq!(regex_splitting("ab|cd"), vec![vec![lit("ab")], vec![lit("cd")]]);
    }

    #[test]
    fn optional_appends_empty_path_last() {
        // `ab?` -> the `?` makes `b` a regex segment (`b?`), giving the full path
        // then the shorter "a" path (empty tail appended last so longer wins).
        assert_eq!(
            regex_splitting("ab?"),
            vec![vec![lit("a"), rx("b?")], vec![lit("a")]]
        );
    }

    #[test]
    fn fixed_repetition_expands() {
        assert_eq!(regex_splitting("a{3}"), vec![vec![lit("aaa")]]);
    }

    #[test]
    fn capturing_group_repetition_keeps_grouping() {
        // `(ab)+` must not degrade to `ab+` (`a` then `b+`); the group's contents
        // are wrapped so the quantifier applies to the whole unit.
        assert_eq!(regex_splitting("(ab)+"), vec![vec![rx("(?:ab)+")]]);
    }

    #[test]
    fn single_char_and_class_repetition_are_left_bare() {
        // A single char or a char class is already one unit — no wrapping.
        assert_eq!(regex_splitting("ab?"), vec![vec![lit("a"), rx("b?")], vec![lit("a")]]);
        assert_eq!(regex_splitting("[0-9]+"), vec![vec![rx("[0-9]+")]]);
    }
}
