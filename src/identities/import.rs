// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `import`: the one identity that loads a file, in the pass, inside its own
//! scope. The node `[path, tail, op]` is the trace of the load; running it
//! yields the file's tail value without running the file again.
//! DESIGN ›Importing is dropping the text there, wrapped in its own scope‹

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};

/// The tail slot; the path string node sits at 0.
const IMPORT_TAIL: usize = 1;

/// Returns `(import identity, run leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        Assoc::Left,
        &["path", "tail", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("import", id);
    cx.metas.insert(id, |p, _id, tape| p.construct_import(tape));
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

/// The imported file's last non-comment expression, or null for a
/// declaration-only file.
///
/// # Safety
/// `node` must be an import node as `construct_import` lays it out.
pub unsafe fn tail_of(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr).add(IMPORT_TAIL)
}

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
