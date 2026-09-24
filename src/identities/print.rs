// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `print «…»`: the output word. The node `[text, op]` writes its quote's
//! text and a newline to stdout each time it runs, and yields unit.
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
        &["text", "op"],
    );
    let print = cx.store.alloc_raw(cx.type_, record);
    cx.declare("print", print);
    cx.metas.insert(print, |p, _id, tape| p.construct_print(tape));
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    PrintIds { print, leaf }
}

pub(crate) fn build(store: &mut Store, types: &Core, text: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[text, types.print.leaf]);
    store.alloc_raw(types.print.print, value)
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `print` node from the store; its text operand is the quote's string node.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let text = rt.through(*ops);
        if text.is_null() || (*text).ty != rt.types().string_ || (*text).value.is_null() {
            return Err(RunError::NotText);
        }
        let mut line = super::string::text(text).to_vec();
        line.push(b'\n');
        std::io::stdout()
            .lock()
            .write_all(&line)
            .map_err(|e| RunError::Output(Box::new(e.to_string())))?;
    }
    Ok(0)
}
