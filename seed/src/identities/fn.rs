//! `fn`: the type whose values are functions. `run` recognizes a function by its
//! type being this. v1 `fn` carries no fields yet: its instances' behaviour lives
//! in the run/compile tables, and a body/input/output layout arrives with
//! richer functions.
//!
//! Surface syntax (v1, minimal): `fn <body>` parses to a *nullary* function (a
//! thunk) whose value is the parsed body; the prefix grabs the rest of the
//! expression as the body. There is no parameter list or return type yet, so
//! `fn a = a + 1` does NOT bind `a` as an argument: it is a thunk whose body is
//! the assignment `a = a + 1` over an existing `a`. A real parameter list and a
//! delimited body arrive with bracket parsing (scopes).

use super::Cx;
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Construct, ParseError};
use crate::store::Store;

/// Create the `fn` type (its own type is `type`) and return it. Called before the
/// build context exists, since `=`/`+` reference `fn` as their type.
pub(super) fn register(store: &mut Store, type_: DyadPtr) -> DyadPtr {
    store.alloc_raw(type_, std::ptr::null_mut())
}

/// Register `fn`'s surface syntax: the spelling and its prefix constructor. Done
/// after the build context exists so it can add to the parser's table.
pub(super) fn register_syntax(cx: &mut Cx) {
    cx.trie.insert("fn", IdContext::new(cx.fn_type, cx.root_scope));
    cx.metas.insert(cx.fn_type, Construct::Prefix(build));
}

/// Build a function `{ty: fn, value: body}` from its parsed body. A function with
/// no `bcode` is interpreted by walking this body (see `crate::run`).
fn build(store: &mut Store, fn_type: DyadPtr, body: DyadPtr) -> Result<DyadPtr, ParseError> {
    Ok(store.alloc_raw(fn_type, body.cast()))
}
