// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `print «…»`: the output word. The node `[parts, op]` writes its quote
//! and a newline to stdout each time it runs, and yields unit: `parts` holds
//! the text runs and the `{…}` expressions, each run and shown as echo shows it.
//! DESIGN ›The command line is Logos source‹

use std::io::Write;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct PrintIds {
    pub print: DyadPtr,
    pub leaf: DyadPtr,
}

/// At the rank `lex` and `import` have: a word that consumes raw text to its
/// right at discovery.
pub(crate) fn register(cx: &mut Cx, cs: &Callables) -> PrintIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        crate::parse::Assoc::Left,
        &["parts", "op"],
    );
    let print = cx.store.alloc_raw(cx.type_, record);
    cx.declare("print", print);
    cx.metas.insert(print, |p, _id, tape| p.construct_print(tape));
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    PrintIds { print, leaf }
}

pub(crate) fn build(store: &mut Store, types: &Core, parts: &[DyadPtr]) -> DyadPtr {
    let parts = super::array::build(store, types.array_, parts);
    let value = store.alloc_operands(&[parts, types.print.leaf]);
    store.alloc_raw(types.print.print, value)
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    let mut line = render(rt, node)?;
    line.push(b'\n');
    std::io::stdout()
        .lock()
        .write_all(&line)
        .map_err(|e| RunError::Output(Box::new(e.to_string())))?;
    Ok(0)
}

/// The text of a `print` or `error` node: its text runs as written, each
/// `{…}` run and shown as the echo shows it.
pub(crate) fn render(rt: &mut Runtime, node: DyadPtr) -> Result<Vec<u8>, RunError> {
    let mut line = Vec::new();
    // SAFETY: `node` is a `print` or `error` node from the store; its parts
    // operand is the array its `build` made, of string nodes and parsed expressions.
    unsafe {
        let parts = *((*node).value as *const DyadPtr);
        for &part in super::array::items(parts) {
            // A `{…}` that yields a string, `{a:name}`, is a read, not a text run.
            if (*part).ty == rt.types().string_
                && super::read::read_kind(rt.types(), part) == super::read::Read::Opaque
            {
                line.extend_from_slice(super::string::text(part));
            } else {
                let bits = rt.run(part)?;
                line.extend_from_slice(super::display_value(rt.types(), part, bits).as_bytes());
            }
        }
    }
    Ok(line)
}
