//! `rational_number`: the numeric-literal carrier. A data type; v1 molds a
//! literal to a concrete `i32` at parse (the general coerce-to-the-sibling's-type
//! path is later). Parse builds the leaf from the source span, compile bakes it
//! as an immediate, and run reads it through its layout.

use cranelift_codegen::ir::Value;

use super::Cx;
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Construct, ParseError};
use crate::store::Store;

/// Register `rational_number`: its spelling, literal constructor, and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.type_, std::ptr::null_mut());
    cx.trie.insert("-?[0-9]+", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Atom(build));
    cx.lower.insert(id, lower);
    id
}

/// Build a rational literal `{ty: rational, value: <i32 bytes>}` from its span.
fn build(store: &mut Store, rational: DyadPtr, span: &str) -> Result<DyadPtr, ParseError> {
    let n: i32 = span.parse().map_err(|_| ParseError::BadLiteral)?;
    let value = store.alloc_bytes(&n.to_ne_bytes());
    Ok(store.alloc_raw(rational, value))
}

/// Lower a molded rational literal to an `i32` immediate.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    let v = unsafe { std::ptr::read_unaligned((*node).value as *const i32) };
    Ok(lw.const_i32(v))
}
