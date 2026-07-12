//! `i32`: the type of a 32-bit integer value. A data type (its type is `type`);
//! its values carry four bytes. It has no run behaviour (data is read through its
//! layout) but does carry a compile lowering: a variable reads from its storage.

use super::numtype::{lower_var, NumType};
use super::Cx;
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;

/// Register `i32`: its spelling (so it resolves as a field/type name), its `NumType`
/// tag (stored in its value slot so run/compile can recover the type from the graph),
/// and its lowering rule (the shared numeric variable load; the interpreter reads it
/// through the type's width, see [`super::numtype::read_scalar`]).
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let tag = cx.store.alloc_bytes(&NumType::I32.tag_bytes());
    let id = cx.store.alloc_raw(cx.type_, tag);
    cx.trie.insert("i32", IdContext::new(id, cx.root_scope));
    cx.lower.insert(id, lower_var);
    id
}
