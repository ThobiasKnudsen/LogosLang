// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `lex «…»`: the lexer as an identity. The node `[text, op]` lexes the text
//! against the scopes open when it runs and yields the fragment's handle, a
//! `parsing_tape` value a constructor splices with `tape.insert(k, lex «…»)`.
//! DESIGN ›Text is the quote‹

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct LexIds {
    pub lex: DyadPtr,
    pub leaf: DyadPtr,
}

/// At the rank `regex` and `import` have: a word that consumes raw text to
/// its right at discovery.
pub(crate) fn register(cx: &mut Cx, cs: &Callables) -> LexIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        crate::parse::Assoc::Left,
        &["text", "op"],
    );
    let lex = cx.store.alloc_raw(cx.type_, record);
    cx.declare("lex", lex);
    cx.metas.insert(lex, |p, _id, tape| p.construct_lex(tape));
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    LexIds { lex, leaf }
}

pub(crate) fn build(store: &mut Store, types: &Core, text: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[text, types.lex.leaf]);
    store.alloc_raw(types.lex.lex, value)
}

/// A `lex «…»` node is the one value a tape fragment has in the seed.
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
pub(crate) unsafe fn is_fragment(types: &Core, node: DyadPtr) -> bool {
    let d = types.through(node);
    !d.is_null() && (*d).ty == types.lex.lex
}

/// The text operand is the quote's string node, or a node that runs to one
/// (`tape.spelling[k]`).
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `lex` node from the store; its text operand is a reduced dyad.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let string_ty = rt.types().string_;
        let mut text = rt.through(*ops);
        if text.is_null() {
            return Err(RunError::NotText);
        }
        if (*text).ty != string_ty {
            text = rt.run(*ops)? as DyadPtr;
        }
        if text.is_null() || (*text).ty != string_ty || (*text).value.is_null() {
            return Err(RunError::NotText);
        }
        let bytes = super::string::text(text);
        let text = std::str::from_utf8(bytes).map_err(|_| RunError::NotText)?;
        Ok(rt.lex(text)? as i64)
    }
}
