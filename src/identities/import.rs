// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `import`: the one identity that loads a file (#58; DESIGN ›The command
//! line is Logos source‹ and ›Importing is dropping the text there, wrapped in
//! its own scope‹). Its constructor consumes the path token — raw text to
//! whitespace or `,`, or a quoted `«…»` string — and the load happens in the
//! pass: the file's text parses and runs top to bottom right there, inside
//! its own scope. Upward only `pub` escapes; downward the file's trie sees
//! ambient names and its own imports only; a file loads once per run, and the
//! import graph must be a DAG (all ruled August 2026). The machinery lives on
//! [`crate::parse::Parser::construct_import`]; this file registers the
//! identity and its run native.
//!
//! The import node `{type: import, value: [path, tail, op]}` is the
//! reflectable trace of the load. Running it does NOT run the file again —
//! that happened in the pass, once — it re-yields the file's tail value by
//! running the tail node, the same stable read a bare name's re-run performs
//! (a declaration tail re-runs harmlessly; a value tail is a read).

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::id_context::IdContext;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::dyad::DyadPtr;

/// The index of the tail slot in an import node's value; the path string node
/// sits at 0.
const IMPORT_TAIL: usize = 1;

/// Register `import`: the spelling, the operand record, the constructor hook,
/// and the run native. Returns `(import identity, run leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        Assoc::Left,
        &["path", "tail", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("import", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, |p, _id, tape| p.construct_import(tape));
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

/// The tail slot of an import node: the imported file's last non-comment
/// expression, or null for a declaration-only file. The drivers display an
/// import's value through this node's logos.
///
/// # Safety
/// `node` must be an import node as `construct_import` lays it out.
pub unsafe fn tail_of(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr).add(IMPORT_TAIL)
}

/// Run: re-yield the imported file's tail value (see the module doc); a
/// declaration-only file's import yields unit.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid import node; a non-null tail is a valid dyad.
    unsafe {
        let tail = tail_of(node);
        if tail.is_null() {
            Ok(0)
        } else {
            rt.run(tail)
        }
    }
}
