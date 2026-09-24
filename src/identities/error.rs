// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `error «…»`: the one error primitive. The node `[text, op]` never returns:
//! running it aborts the run with its quote's text as the message.
//! DESIGN ›Error handling‹

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct ErrorIds {
    pub error: DyadPtr,
    pub leaf: DyadPtr,
}

/// At the rank `lex` and `print` have: a word that consumes raw text to its
/// right at discovery.
pub(crate) fn register(cx: &mut Cx, cs: &Callables) -> ErrorIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        crate::parse::Assoc::Left,
        &["text", "op"],
    );
    let error = cx.store.alloc_raw(cx.type_, record);
    cx.declare("error", error);
    cx.metas.insert(error, |p, _id, tape| p.construct_error(tape));
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    ErrorIds { error, leaf }
}

pub(crate) fn build(store: &mut Store, types: &Core, text: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[text, types.error.leaf]);
    store.alloc_raw(types.error.error, value)
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `error` node from the store; its text operand is the quote's string node.
    unsafe {
        let text = rt.through(*((*node).value as *const DyadPtr));
        if text.is_null() || (*text).ty != rt.types().string_ || (*text).value.is_null() {
            return Err(RunError::NotText);
        }
        let message = String::from_utf8_lossy(super::string::text(text)).into_owned();
        Err(RunError::Raised(Box::new(message)))
    }
}
