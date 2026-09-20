// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `ran`: an item that has run in the pass, kept in the graph with its result.
//!
//! DESIGN ›Build and run are one self-directing pass‹ (13 September 2026):
//! the pass runs only as far as it must, in order, and never twice. When the
//! parse needs a value before a scope has finished parsing — a box read where
//! an identity is needed, a call for a type, a Logos-written constructor, a
//! type's own body, an import — everything parsed before that point runs
//! first, and each executable item that ran becomes `{type: ran, value:
//! [expr, value, op]}`: `expr` the node as it was, `value` an eight-byte cell
//! holding what its run yielded, `op` the leaf that reads the cell. The
//! rewrite is in place ([`rewrite`]), so every holder of the item's address —
//! a block's expression array, an argument slot, an `if`'s condition — sees
//! the ran form without being told, and the scope's own run *reads* the item
//! instead of running it again.
//!
//! The reading rule classifies a ran node `Executable(Leaf)` like any operand
//! record, and its run is the cell read. Readers that ask what an expression
//! *is* — its result type, whether it is a statement, whether it yields an
//! owning pointer — look through it with [`expr_of`], the way `through` hops
//! a name's record; the reading rule and `run` do not, which is the point.
//! Like `scope`, `ran` is built by the parser and never spelled in source.

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::NumType;
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// The slot holding the item as it was.
const EXPR: usize = 0;
/// The slot holding the cell with the run's result.
const VALUE: usize = 1;

/// Register `ran`: the operand record, the run leaf, and the lowering.
/// Returns `(identity, leaf)`; the identity has no spelling.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["expr", "value", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    cx.lower.insert(id, lower);
    (id, leaf)
}

/// Build a ran node over `expr` whose run yielded `bits`.
pub fn build(store: &mut Store, types: &Core, expr: DyadPtr, bits: i64) -> DyadPtr {
    let storage = store.alloc_bytes(&bits.to_ne_bytes());
    let cell = store.alloc_raw(types.numtypes[NumType::I64 as usize], storage);
    let value = store.alloc_operands(&[expr, cell, types.ops.ran_]);
    store.alloc_raw(types.ran_, value)
}

/// Turn `node`, an item that just ran and yielded `bits`, into a ran node in
/// place: the item's type and value move to a fresh node, which becomes the
/// ran node's `expr`. Every pointer to `node` now points at the ran form.
///
/// # Safety
/// `node` must be a valid dyad from the store that nothing is reading while
/// this runs.
pub unsafe fn rewrite(store: &mut Store, types: &Core, node: DyadPtr, bits: i64) {
    let copy = store.alloc_raw((*node).ty, (*node).value);
    let ran = build(store, types, copy, bits);
    (*node).ty = (*ran).ty;
    (*node).value = (*ran).value;
}

/// The item a ran node holds, or `node` itself for anything else: the hop for
/// readers that ask what an expression is, not what its value reads as.
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
pub unsafe fn expr_of(types: &Core, node: DyadPtr) -> DyadPtr {
    if node.is_null() || (*node).ty != types.ran_ {
        return node;
    }
    *((*node).value as *const DyadPtr).add(EXPR)
}

/// What a ran node's run yielded.
///
/// # Safety
/// `node` must be a ran node as [`build`] lays it out.
pub unsafe fn value_of(node: DyadPtr) -> i64 {
    let cell = *((*node).value as *const DyadPtr).add(VALUE);
    std::ptr::read_unaligned((*cell).value as *const i64)
}

/// Run: read the cell. The item ran once, in the pass; this is the read that
/// stands in for it wherever the item's scope runs.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a ran node; its value slot is the cell, a scalar node.
    unsafe {
        let cell = *((*node).value as *const DyadPtr).add(VALUE);
        rt.run(cell)
    }
}

/// Lower: the cell read, the same on both tiers — a scope that ran an item
/// in the pass and is compiled whole afterwards reads the result it kept.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run`].
    unsafe {
        let cell = *((*node).value as *const DyadPtr).add(VALUE);
        lw.lower(cell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identities::read::{read_kind, Dispatch, Read};
    use crate::identities::{display_value, numtype_of, Core, Operand};
    use crate::parse::{is_bool_result, Parser, ScopeStack};
    use crate::regex_trie::RegexTrie;

    #[test]
    fn a_ran_item_reads_as_its_result_and_answers_as_its_expression() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = &core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let (decl, cmp) = {
            let mut p = Parser::new("x := i32 1", &mut store, &mut trie, types, scopes);
            let decl = p.parse_expression().unwrap();
            let scopes = p.into_scopes();
            let mut p = Parser::new("x == 1", &mut store, &mut trie, types, scopes);
            (decl, p.parse_expression().unwrap())
        };
        let mut rt = Runtime::new(types, &mut store);
        // SAFETY: both nodes were just parsed into `store`, which outlives `rt`.
        unsafe {
            rt.run(decl).unwrap();
            let bits = rt.run(cmp).unwrap();
            assert_eq!(bits, 1);
            let ran = build(rt.store, types, cmp, bits);
            // The reading rule: an operand record dispatching to the leaf.
            assert_eq!(read_kind(types, ran), Read::Executable(Dispatch::Leaf(types.ops.ran_)));
            // Its run is the cell read; its value is at hand without a run.
            assert_eq!(rt.run(ran).unwrap(), 1);
            assert_eq!(value_of(ran), 1);
            // Readers that ask what it *is* see the comparison.
            assert_eq!(expr_of(types, ran), cmp);
            assert_eq!(expr_of(types, cmp), cmp);
            assert!(is_bool_result(types, ran));
            assert!(matches!(numtype_of(types, ran), Operand::Concrete(NumType::I32)));
            assert_eq!(display_value(types, ran, 1), "true");
            // Rewriting in place: the old address now reads as the ran form,
            // and the item lives on behind it.
            rewrite(rt.store, types, cmp, 1);
            assert_eq!((*cmp).ty, types.ran_);
            assert_eq!((*expr_of(types, cmp)).ty, types.eq);
            assert_eq!(rt.run(cmp).unwrap(), 1);
            assert_eq!(display_value(types, cmp, 1), "true");
        }
    }
}
