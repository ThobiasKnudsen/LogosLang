// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The comment node: `{type: comment, value -> string node}`, prose as
//! reflectable structure, invisible to value flow. Built by `#` from a `«…»`
//! string or raw text to the end of the line, one token with its text.
//! DESIGN ›Text literals are plain values; `#` is the one comment constructor‹

use super::numtype::COMMENT_TAG;
use super::{meta, string, Cx};
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, Extent, ParseError, Parser, ParsingTape};

/// The `comment` logos has no spelling and no run entry: the interpreter's data
/// path yields unit for it off its tag. `#` is the constructor. Returns the
/// comment type and `#`.
pub(crate) fn register(cx: &mut Cx) -> (DyadPtr, DyadPtr) {
    let record = meta::record(cx.store, COMMENT_TAG, meta::prec::INERT);
    let id = cx.store.alloc_head(cx.type_, record);

    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::LITERAL);
    let hash = cx.store.alloc_head(cx.type_, record);
    cx.declare("#", hash);
    cx.metas.insert(hash, construct);
    cx.extents.insert(hash, |text| {
        let at = text_start(text);
        if text[at..].starts_with('«') {
            string::read(text, at).map(string::Quote::extent)
        } else {
            let len = text[at..].find('\n').map_or(text.len(), |n| at + n);
            Ok(Extent { len, scopes: Vec::new() })
        }
    });
    (id, hash)
}

/// Past `#` and the spaces (not the newline) that may separate it from its text.
fn text_start(spelling: &str) -> usize {
    let rest = spelling['#'.len_utf8()..].trim_start_matches([' ', '\t']);
    spelling.len() - rest.len()
}

/// The comment over the cell's own spelling, never the text around it: a `#` spliced from
/// a `lex` fragment reads what it was lexed from (DESIGN ›`tape.spelling[k]` is the text a
/// cell was lexed from‹).
fn construct(
    p: &mut Parser,
    _id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    let spelling = tape.own_text().ok_or(ParseError::BadLiteral)?;
    let at = text_start(spelling);
    let text = if spelling[at..].starts_with('«') {
        string::read(spelling, at).map_err(ParseError::Resolve)?.text(spelling)
    } else {
        spelling[at..].trim_end().as_bytes().to_vec()
    };
    let types = p.types();
    let text_node = string::build_text(p.store(), types.string_, &text);
    let node = p.store().alloc_head(types.comment_, text_node.cast());
    tape.place(node);
    Ok(Constructed::Placed)
}
