// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `pub`: the first gate. Gates are access-setting identities (DESIGN ›Read
//! and write are one mechanism across the system‹ — "`pub` and `mut` are
//! themselves gates"), and their spelling belongs to their constructor like
//! every identity on the tape: `pub` is a prefix word over a declaration,
//! `pub x := 5`. Unmarked stays fail-closed — private — so `pub` is the only
//! marking; there is no `private` word to write.
//!
//! The constructor parses the declaration to its right and fills the declare
//! node's gate slot ([`super::declare::set_gate`]): the deviation lives in the
//! declaration's own structure (DESIGN ›Metadata has three homes‹), where the
//! visibility read `import` performs (#58) finds it. Within one section the
//! marking changes nothing — visibility restricts *peer* access across the
//! section boundary, and the seed is a single section until `import` lands —
//! so this slice is the surface and the structure; the boundary that consults
//! them is #58's.

use super::{meta, Cx};
use crate::synolon::SynolonPtr;
use crate::id_context::IdContext;
use crate::parse::{Constructed, ParseError};

/// Register `pub`: a fresh-start word (a plain token record, NaN precedence —
/// never extends left, so the driver invokes the constructor immediately).
/// No node is ever typed by `pub`; its identity exists to be named in a
/// declare node's gate slot.
pub(super) fn register(cx: &mut Cx) -> SynolonPtr {
    let record = meta::record(cx.store, meta::TOKEN_TAG, f64::NAN);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("pub", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, construct);
    id
}

/// `pub`'s constructor: parse the expression to the right, demand it reduced
/// to a declaration, and fill its gate slot. Anything else — a value, a bare
/// name, a second `pub` — is a parse error, not a silent no-op: a gate that
/// marked nothing would be a lie in the source.
fn construct(
    p: &mut crate::parse::Parser,
    _id: SynolonPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<Constructed, ParseError> {
    let inner = p.parse_expression()?;
    let types = p.types();
    // SAFETY: `inner` is a reduced synolon just parsed.
    unsafe {
        if (*inner).logos != types.declare_ {
            return Err(ParseError::GateNeedsDeclaration);
        }
        if !super::declare::gate_of(inner).is_null() {
            return Err(ParseError::DoubleGate);
        }
        super::declare::set_gate(inner, types.pub_);
    }
    tape.place(inner);
    Ok(Constructed::Placed)
}
