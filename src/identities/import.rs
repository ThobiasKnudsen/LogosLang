// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `import`: the one identity that loads a file, in the pass, inside its own
//! scope. The node `[path, op]` is the trace of the load; running it does
//! nothing, the file having run once at the load.
//! DESIGN ›Importing is dropping the text there, wrapped in its own scope‹

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};

/// Returns `(import identity, run leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        Assoc::Left,
        &["path", "op", "output_type"],
    );
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare("import", id);
    cx.metas.insert(id, |p, _id, tape| p.construct_import(tape));
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

fn run(_rt: &mut Runtime, _node: DyadPtr) -> Result<i64, RunError> {
    Ok(0)
}
