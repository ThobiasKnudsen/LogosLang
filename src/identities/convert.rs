// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Numeric conversion, `i32(a)`, `f64(x)`: one shared `convert` identity for
//! every scalar cast, the node `[operand, from, to, op]` with the source and
//! target as numtype nodes, or `rational_number` as the source for a rational value, read
//! at run. The only cross-type path; there is no implicit coercion.

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::{apply_cast, of_type_node, NumType};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// No spelling: a conversion is built from the `logos(value)` surface.
/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["operand", "from", "to", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(id, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

pub(crate) fn build_convert(
    store: &mut Store,
    types: &Core,
    operand: DyadPtr,
    from: DyadPtr,
    to: DyadPtr,
) -> DyadPtr {
    let value = store.alloc_operands(&[operand, from, to, types.ops.convert_]);
    store.alloc_raw(types.convert, value)
}

/// # Safety
/// `node` must be a conversion node `[operand, from, to, op]`.
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr, NumType) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p, *p.add(1), of_type_node(*p.add(2)))
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid conversion node; a rational value is the address of its
    // sixteen bytes.
    unsafe {
        let (operand, from, to) = parts(node);
        let v = rt.run(operand)?;
        if from == rt.types().rational {
            let p = v as usize as *const u8;
            if p.is_null() {
                return Err(RunError::Uninitialized);
            }
            let (num, den) = super::rational::read_at(p);
            if den == 0 {
                return Err(RunError::Uninitialized);
            }
            return Ok(super::rational::cast_pair(num, den, to));
        }
        Ok(apply_cast(of_type_node(from), to, v))
    }
}

/// A rational value is interpreted only, as every operator over one is.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid conversion node.
    unsafe {
        let (operand, from, to) = parts(node);
        if from == lw.types().rational {
            return Err(CompileError::NotLowerable(node));
        }
        let v = lw.lower(operand)?;
        Ok(lw.emit_cast(of_type_node(from), to, v))
    }
}
