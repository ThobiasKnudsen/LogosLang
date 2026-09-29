// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `string` and its `«…»` literal: a plain value, inert in the seed (nothing
//! consumes one yet). Storage: `[len: u64][bytes]`, the native-endian length
//! then the UTF-8 text; the type node self-describes via [`STRING_TAG`].
//! DESIGN ›Strings nest by depth; only five escapes; `«…»` is the only spelling‹

use std::ops::Range;

use super::numtype::STRING_TAG;
use super::{meta, Cx};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, Extent, ParseError, Parser, ParsingTape, ResolveError};
use crate::store::Store;

/// Fills the record into the type node the build minted first (every record's
/// `name` is a string node, so the type exists before the first declaration).
pub(crate) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, STRING_TAG, meta::prec::LITERAL);
    let id = cx.string_;
    // SAFETY: `id` is the string type node the build allocated, its value null until now.
    unsafe { dyad::set_head(id, record) };
    cx.declare("«", id);
    cx.metas.insert(id, construct);
    cx.extents.insert(id, |text| read(text, 0).map(Quote::extent));
    id
}

fn construct(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    let spelling = tape.own_text().ok_or(ParseError::BadLiteral)?;
    let quote = read(spelling, 0).map_err(ParseError::Resolve)?;
    let node = build_text(p.store(), id, &quote.text(spelling));
    tape.place(node);
    Ok(Constructed::Placed)
}

/// What a quote holds, in order.
pub(crate) enum Piece {
    /// Text, its escapes applied.
    Text(Vec<u8>),
    /// A `{…}` the quote opened itself: the byte range of its interior.
    Scope(Range<usize>),
    /// A `}` that closes nothing: text in a plain string, the error in `print` and `error`.
    StrayClose(usize),
}

pub(crate) struct Quote {
    /// One past the closing `»`.
    pub(crate) end: usize,
    pub(crate) pieces: Vec<Piece>,
}

impl Quote {
    /// What the lexer takes: the quote's end as its length, and its `{…}` scopes.
    pub(crate) fn extent(self) -> Extent {
        let scopes = self
            .pieces
            .into_iter()
            .filter_map(|p| match p {
                Piece::Scope(r) => Some(r),
                Piece::Text(_) | Piece::StrayClose(_) => None,
            })
            .collect();
        Extent { len: self.end, scopes }
    }

    /// The text a string holds: the escapes applied, each `{…}` as written.
    pub(crate) fn text(&self, source: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for piece in &self.pieces {
            match piece {
                Piece::Text(t) => out.extend_from_slice(t),
                Piece::Scope(r) => {
                    out.extend_from_slice(&source.as_bytes()[r.start - 1..r.end + 1])
                }
                Piece::StrayClose(_) => out.push(b'}'),
            }
        }
        out
    }
}

/// The five characters a backslash makes text.
const ESCAPED: [char; 5] = ['«', '»', '{', '}', '\\'];

/// The quote whose `«` is at `from`: the scope that counts `«`/`»` and `{`/`}` by depth
/// and reads a backslash before one of them, or before a backslash, as that character.
/// A `{…}` is code: a backslash in it is itself, and a `»` in it that closes no `«` is the
/// error once the `{` closes; if the text ends first, the `{` is. Offsets, in the pieces
/// and in an error, are into `source`.
pub(crate) fn read(source: &str, from: usize) -> Result<Quote, ResolveError> {
    debug_assert!(source[from..].starts_with('«'));
    // Each opener, where it stands, and for a `{` the first `»` in it that closed nothing.
    let mut open: Vec<(char, usize, Option<usize>)> = vec![('«', from, None)];
    let mut pieces = Vec::new();
    let mut run: Vec<u8> = Vec::new();
    // The interior's start while a `{` the quote opened itself is open; its bytes are no text.
    let mut scope: Option<usize> = None;
    let mut i = from + '«'.len_utf8();
    while let Some(c) = source[i..].chars().next() {
        let at = i;
        i += c.len_utf8();
        let in_text = scope.is_none();
        let keep = |c: char, run: &mut Vec<u8>| {
            if in_text {
                run.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            }
        };
        let top = open.last().map(|&(o, _, _)| o);
        match c {
            '\\' if top == Some('«') => {
                match source[i..].chars().next().filter(|e| ESCAPED.contains(e)) {
                    Some(e) => {
                        i += e.len_utf8();
                        keep(e, &mut run);
                    }
                    None => keep(c, &mut run),
                }
            }
            '«' => {
                open.push(('«', at, None));
                keep(c, &mut run);
            }
            '»' if top == Some('«') => {
                open.pop();
                if open.is_empty() {
                    if !run.is_empty() {
                        pieces.push(Piece::Text(run));
                    }
                    return Ok(Quote { end: i, pieces });
                }
                keep(c, &mut run);
            }
            '»' => {
                let stray = &mut open.last_mut().expect("a `{` is open").2;
                stray.get_or_insert(at);
                keep(c, &mut run);
            }
            '{' => {
                if open.len() == 1 {
                    if !run.is_empty() {
                        pieces.push(Piece::Text(std::mem::take(&mut run)));
                    }
                    scope = Some(i);
                } else {
                    keep(c, &mut run);
                }
                open.push(('{', at, None));
            }
            '}' if top == Some('{') => {
                if let Some((_, _, Some(stray))) = open.pop() {
                    return Err(ResolveError::Unmatched { bracket: '»', at: stray });
                }
                if open.len() == 1 {
                    let start = scope.take().expect("the quote's own `{` opened a scope");
                    pieces.push(Piece::Scope(start..at));
                } else {
                    keep(c, &mut run);
                }
            }
            '}' if open.len() == 1 => {
                if !run.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut run)));
                }
                pieces.push(Piece::StrayClose(at));
            }
            c => keep(c, &mut run),
        }
    }
    let &(bracket, at, _) = open.last().expect("the quote's own `«` is open until its `»`");
    Err(ResolveError::Unmatched { bracket, at })
}

pub(crate) fn build_text(store: &mut Store, string_ty: DyadPtr, text: &[u8]) -> DyadPtr {
    let mut blob = Vec::with_capacity(8 + text.len());
    blob.extend_from_slice(&(text.len() as u64).to_ne_bytes());
    blob.extend_from_slice(text);
    store.alloc_blob(string_ty, &blob)
}

/// # Safety
/// `node` must be a string node built by [`build_text`] (its value the
/// `[len, bytes]` blob), and the slice must not outlive the store.
pub(crate) unsafe fn text<'a>(node: DyadPtr) -> &'a [u8] {
    let p = dyad::value(node);
    let len = std::ptr::read_unaligned(p as *const u64) as usize;
    std::slice::from_raw_parts(p.add(8), len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(q: &Quote) -> Vec<String> {
        q.pieces
            .iter()
            .map(|p| match p {
                Piece::Text(t) => format!("text {}", String::from_utf8_lossy(t)),
                Piece::Scope(r) => format!("scope {r:?}"),
                Piece::StrayClose(at) => format!("stray {at}"),
            })
            .collect()
    }

    #[test]
    fn a_quote_counts_both_pairs_and_reads_five_escapes() {
        let src = "«a {x} \\{b\\} «c» \\q }» tail";
        let q = read(src, 0).unwrap();
        assert_eq!(&src[q.end..], " tail");
        assert_eq!(pieces(&q), ["text a ", "scope 5..6", "text  {b} «c» \\q ", "stray 23"]);
        assert_eq!(q.text(src), "a {x} {b} «c» \\q }".as_bytes());

        let src = "«{«a\\»}» }»";
        let q = read(src, 0).unwrap();
        assert_eq!(pieces(&q), ["scope 3..13"]);
        assert_eq!(&src[3..13], "«a\\»}» ");

        assert_eq!(read("x «a»", 2).unwrap().end, 7);
    }

    #[test]
    fn an_unclosed_quote_names_its_innermost_opener() {
        let unclosed = |src: &str| read(src, 0).err();
        let unmatched = |bracket, at| Some(ResolveError::Unmatched { bracket, at });
        assert_eq!(unclosed("«a «b»"), unmatched('«', 0));
        assert_eq!(unclosed("«a {b»"), unmatched('{', 4));
        assert_eq!(unclosed("«a\\»"), unmatched('«', 0));
        assert_eq!(unclosed("«{«a}»"), unmatched('{', 2));
        // A `»` in code closes no `«`: the error once its `{` closes.
        assert_eq!(unclosed("«x {»} y»"), unmatched('»', 5));
        assert_eq!(unclosed("«a «b {c »} d» e»"), unmatched('»', 11));
        assert_eq!(unclosed("«{\\»}»"), unmatched('»', 4));
    }
}
