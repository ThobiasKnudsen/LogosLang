// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `:=`: the declaration operator. `name := value` binds a *fresh* name to a value
//! in the current scope, declaring the name *before* the value is elaborated so the
//! value can refer to it — a function can name itself and recurse.
//!
//! It is purely a parse-time construct: the driver
//! ([`crate::parse::Parser::parse_expression`]) performs the binding when it sees a
//! fresh name followed by `:=`. Here we only register the token so the parser
//! recognizes it; the trie longest-matches `:=` over the field-list `:`.

use super::Cx;
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::Construct;

/// Register `:=` as a declaration token. Returns its identity; the driver dispatches
/// on its `Construct::Declare`, not the id, so the handle is otherwise unused. `:=`
/// carries no run or compile behaviour — it produces the bound value node at parse
/// time and is gone by the time the graph runs.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert(":=", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Declare);
    id
}
