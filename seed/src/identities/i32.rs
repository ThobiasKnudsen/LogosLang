//! `i32`: the type of a 32-bit integer value. A data type (its type is `type`);
//! its values carry four bytes. It has no run behaviour (data is read through its
//! layout) but does carry a compile lowering: a variable reads from its storage.

use cranelift_codegen::ir::Value;

use super::Cx;
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;

/// Register `i32`: its spelling (so it resolves as a field/type name) and its
/// lowering rule.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.type_, std::ptr::null_mut());
    cx.trie.insert("i32", IdContext::new(id, cx.root_scope));
    cx.lower.insert(id, lower);
    id
}

/// Lower an `i32` variable to a load from its baked storage address. Guards a null
/// address, mirroring the interpreter's `BadValue` for a declared-but-uninitialised
/// variable — otherwise the compiler bakes a load from address 0 and SIGSEGVs at
/// call time where the interpreter cleanly errors.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    let addr = unsafe { (*node).value };
    if addr.is_null() {
        return Err(CompileError::BadValue);
    }
    Ok(lw.load_i32(addr))
}
