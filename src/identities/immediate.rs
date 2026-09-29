// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `immediate x`: runs the expression to its right while it parses and stands as the
//! value. No node of it is ever built: the constructor places the value itself.
//! DESIGN ›`immediate x` runs the expression to its right as soon as it is parsed, and stands as its value‹

use super::{meta, Cx};

/// A reader, as `defer` is: it drives its right side to the boundary.
pub(crate) fn register(cx: &mut Cx) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::READER,
        crate::parse::Assoc::Right,
        &["expr", "op"],
    );
    let immediate = cx.store.alloc_head(cx.type_, record);
    cx.declare("immediate", immediate);
    cx.metas.insert(immediate, |p, _id, tape| p.construct_immediate(tape));
}
