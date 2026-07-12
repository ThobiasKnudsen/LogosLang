//! `if ( cond ) ( then ) else ( else )`: the value-producing conditional. `if` is a
//! function (its own type is `fn`), like the operators; its node is
//! `{ty: if, value: [cond, then, else]}`. The condition must be a `bool` (checked at
//! parse time); run evaluates only the taken branch, compile emits a two-way branch
//! merging to a single value (DESIGN ›A scope's value is what it evaluates to‹).
//!
//! The surface parse lives in [`crate::parse::Parser::parse_if`] (it drives three
//! bracketed sub-parses and the `else` keyword); here we register the identity, its
//! run native and lowering, and the `else` token the parser consumes.

use cranelift_codegen::ir::Value;

use super::Cx;
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::Construct;
use crate::run::{RunError, Runtime};

/// The index of the condition in an `if` node's value struct.
const IF_COND: usize = 0;
/// The index of the then-branch.
const IF_THEN: usize = 1;
/// The index of the else-branch.
const IF_ELSE: usize = 2;

/// Register `if` (the conditional keyword, its run native, and its lowering) and the
/// `else` token the parser consumes between the branches. Returns the `if` identity.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let if_ = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("if", IdContext::new(if_, cx.root_scope));
    cx.metas.insert(if_, Construct::If);
    cx.bcode.insert(if_, run);
    cx.lower.insert(if_, lower);

    let else_ = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("else", IdContext::new(else_, cx.root_scope));
    cx.metas.insert(else_, Construct::Else);

    if_
}

/// The `(cond, then, else)` operands of an `if` node.
///
/// # Safety
/// `node` must be an `if` node built by [`crate::parse::Parser::parse_if`], with a
/// `[cond, then, else]` value.
unsafe fn branches(node: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
    (*p.add(IF_COND), *p.add(IF_THEN), *p.add(IF_ELSE))
}

/// Run: evaluate the condition, then run only the taken branch (a non-zero condition
/// is true, matching the compiled `brif`).
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `if` node with `[cond, then, else]` operands.
    unsafe {
        let (cond, then, els) = branches(node);
        if rt.run(cond)? != 0 {
            rt.run(then)
        } else {
            rt.run(els)
        }
    }
}

/// Lower: a two-way branch on the condition, each arm lowering its branch and
/// jumping to a merge block whose parameter is the `if`'s value.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `if` node with `[cond, then, else]` operands.
    unsafe {
        let (cond, then, els) = branches(node);
        lw.lower_if(cond, then, els)
    }
}
