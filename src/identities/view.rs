// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `dyad`, the cell type, spelled (DESIGN ›Substrate vocabulary‹: "The cell
//! is the `dyad`, its two slots the `.type` and the `.value`"). Inert on the
//! tape: it has no constructor yet — `dyad (type, value)` construction from
//! Logos is #60's — and stands as its own value, so `@dyad` can name a
//! pointer to a cell and a record's fields are typed by it.
//!
//! The dyad *view* — a value of this type whose value is a cell's address,
//! on which `.type` and `.value` read the cell's two fields
//! ([`crate::parse::Parser::view_member`]) — is spelled `a:dyad`, the record
//! read (DESIGN ›The dyad's read surface‹, 7–8 September 2026: "`:dyad` is
//! the view, and `(dyad a)` as a second spelling for the same read is
//! superseded"). The type members (`.arity`, `.roles[i]`, `.precedence`, …)
//! read the shared record through the type `.type` yields. Reads fold at
//! parse; writing stays with constructors and the tape ops (#60).

use super::{meta, Cx};
use crate::dyad::DyadPtr;

/// Register `dyad`, the cell type: a spelled identity with no constructor.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::DYAD_TAG, meta::prec::INERT);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("dyad", id);
    id
}
