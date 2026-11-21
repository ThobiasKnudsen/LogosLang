// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `language`, the type whose values are languages, and `logos`, the Logos language:
//! `logos : language : type`. Inside Logos code `logos (…)` is the plain group, and
//! `logos file «…»` reads a file as Logos.
//! ›The ground is one identity, spelled `type`; `logos` is a language‹ [[R134]]

use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError, Parser, ParsingTape};

/// Returns `logos`.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    // A language is an identity carrying a record, as a type is.
    let record = meta::record(cx.store, meta::TYPEREC_TAG, meta::prec::INERT);
    let language = cx.store.alloc_head(cx.type_, record);
    cx.declare("language", language);
    let record = meta::record(cx.store, meta::TYPEREC_TAG, meta::prec::READER);
    let logos = cx.store.alloc_head(language, record);
    cx.declare("logos", logos);
    cx.metas.insert(logos, construct);
    logos
}

fn construct(
    p: &mut Parser,
    _id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    if let Some(scope) = p.construct_logos_file(tape)? {
        tape.place(scope);
        return Ok(Constructed::Placed);
    }
    // The door leads into the scope it stands in, so the word goes and its bracket is read as
    // Logos's own `(`, where `logos` stood.
    if p.reads_own_bracket(tape) {
        tape.remove(0);
        return Ok(Constructed::Placed);
    }
    Ok(Constructed::Decline)
}
