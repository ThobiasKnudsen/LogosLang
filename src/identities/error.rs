// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `error «…»`: the one error primitive. The node `[parts, op]` never returns:
//! running it aborts the run with its quote's text as the message, `{…}` read
//! as `print` reads it. DESIGN ›Error handling‹

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
        &["parts", "op"],
    );
    let error = cx.store.alloc_raw(cx.type_, record);
    cx.declare("error", error);
    cx.metas.insert(error, |p, _id, tape| p.construct_error(tape));
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    ErrorIds { error, leaf }
}

pub(crate) fn build(store: &mut Store, types: &Core, parts: &[DyadPtr]) -> DyadPtr {
    let parts = super::array::build(store, types.array_, parts);
    let value = store.alloc_operands(&[parts, types.error.leaf]);
    store.alloc_raw(types.error.error, value)
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    let message = super::print::render(rt, node)?;
    Err(RunError::Raised(Box::new(String::from_utf8_lossy(&message).into_owned())))
}
