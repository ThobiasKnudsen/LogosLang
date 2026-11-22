// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `error`: the word of error values. `error.X := «description»` declares the category `X`, a
//! name of the scope it stands in, and `error.X «message»` raises it, `{…}` read as `print`
//! reads it. Inside a function that gives `T!` the raise fills the result's `err` and leaves the
//! function; anywhere else it is the node `[parts, op, output_type, category]`, whose run ends the
//! run as a fault.
//! ›Error values are in v0.1.0; an error names a category, which is a scoped name‹ [[R143]]

use cranelift_codegen::ir::Value;

use super::binding::Binding;
use super::callable::{self, Callables};
use super::{carried, meta, print, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::regex_trie::RegexTrie;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// The binding record of the category a raise names.
const CATEGORY: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct ErrorIds {
    pub error: DyadPtr,
    pub leaf: DyadPtr,
    /// The error value's type, `array hop`, which `error` names where a type is wanted; null
    /// until identities/error.logos is read.
    pub value: DyadPtr,
    /// The type of one element of the error value.
    pub hop: DyadPtr,
    /// error.logos's function that makes the error value of one raise.
    pub one_hop: DyadPtr,
    /// A raise read as the error value it makes, `[parts, op, output_type, category]`: what a
    /// raise inside a function that gives `T!` fills `err` with.
    pub raised: DyadPtr,
    pub raised_leaf: DyadPtr,
}

const ARRAY: carried::File = carried::File {
    path: "identities/array.logos",
    source: include_str!("../../identities/array.logos"),
};
const FILE: carried::File = carried::File {
    path: "identities/error.logos",
    source: include_str!("../../identities/error.logos"),
};

/// Reads identities/error.logos into `section`, after the array it is built from: the error
/// value's type, its element's, and the function that makes the value of one raise. Nothing of
/// either file reaches the root.
///
/// # Panics
/// Where a file the seed was built with does not parse or run: a defect of the seed.
pub(super) fn load(
    store: &mut Store,
    trie: &mut RegexTrie,
    core: &Core,
    section: DyadPtr,
) -> ErrorIds {
    carried::read(store, trie, core, section, &ARRAY);
    carried::read(store, trie, core, section, &FILE);
    let ty = |name| {
        let named = carried::name_in(trie, section, &FILE, name);
        // SAFETY: a binding of the section is a binding dyad from the store.
        unsafe { super::type_identity_of(core, Binding::read(named.binding).dyad) }
            .unwrap_or_else(|| panic!("{}: `{name}` is no type", FILE.path))
    };
    let one_hop = carried::name_in(trie, section, &FILE, "one_hop").identity;
    // SAFETY: `one_hop` is the dyad a name of the section holds.
    let made_by = unsafe { dyad::ty(one_hop) };
    assert!(made_by == core.fn_type, "{}: `one_hop` is no function", FILE.path);
    ErrorIds { value: ty("hops"), hop: ty("hop"), one_hop, ..core.error }
}

/// At the rank `lex` and `print` have: a word that reads its own right side at discovery.
pub(crate) fn register(cx: &mut Cx, cs: &Callables) -> ErrorIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        crate::parse::Assoc::Left,
        &["parts", "op", "output_type", "category"],
    );
    let error = cx.store.alloc_head(cx.type_, record);
    cx.declare("error", error);
    cx.metas.insert(error, |p, _id, tape| p.construct_error(tape));
    cx.lower.insert(error, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["parts", "op", "output_type", "category"],
    );
    let raised = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `raised` carries the record just built.
    unsafe { meta::mark_own(raised) };
    cx.lower.insert(raised, lower_raised);
    let raised_leaf = callable::mint_native(cx.store, cs.callable, run_raised, cs.seed_native);
    let null = std::ptr::null_mut();
    ErrorIds { error, leaf, value: null, hop: null, one_hop: null, raised, raised_leaf }
}

/// A raise of `category`, the binding record its declaration made.
pub(crate) fn build(
    store: &mut Store,
    types: &Core,
    parts: &[DyadPtr],
    category: DyadPtr,
) -> DyadPtr {
    let parts = super::array::build(store, types.array_, parts);
    store.alloc_words(types.error.error, &[parts, types.error.leaf, types.void_, category])
}

/// A raise of `category` read as the error value it makes where it runs: an array of one hop,
/// holding the category's record, the message, the raise's own node and `?` (›A raise is
/// `error.X «message»`; the error value is an array with one element per hop‹ [[R144]]).
pub(crate) fn build_raised(
    store: &mut Store,
    types: &Core,
    parts: &[DyadPtr],
    category: DyadPtr,
) -> DyadPtr {
    let parts = super::array::build(store, types.array_, parts);
    let words = [parts, types.error.raised_leaf, types.error.value, category];
    store.alloc_words(types.error.raised, &words)
}

/// The error value the raised node `node` makes with `message`: error.logos's `one_hop` of the
/// category's record, the message and the raise's own node.
///
/// # Safety
/// `node` must be a node [`build_raised`] built.
unsafe fn made(rt: &mut Runtime, node: DyadPtr, message: &[u8]) -> Result<i64, RunError> {
    let types: *const Core = rt.types();
    // SAFETY: the runtime borrows the core, so the store's growth leaves it where it is.
    let types = &*types;
    let text = super::string::build_text(rt.store(), types.string_, message);
    let category = *(dyad::value(node) as *const DyadPtr).add(CATEGORY);
    rt.apply_values(types.error.one_hop, &[category as i64, text as i64, node as i64])
}

fn run_raised(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    let message = print::render(rt, node)?;
    // SAFETY: `node` is a raised node `build_raised` made.
    unsafe { made(rt, node, &message) }
}

fn lower_raised(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a raised node `build_raised` made; its parts are reduced dyads.
    unsafe {
        let values = print::lower_parts(lw, node)?;
        let node = lw.node_addr(node);
        lw.call_seed(compiled_raised as *const () as usize, &[node, values])
    }
}

/// # Safety
/// Called only by compiled code, with its context, a raised node from the store and a block
/// holding one container per part, in order.
unsafe extern "C" fn compiled_raised(
    ctx: *mut crate::run::Context,
    node: DyadPtr,
    values: *const i64,
) -> i64 {
    crate::run::interpret_step(ctx, |rt| {
        // SAFETY: the caller's contract is `render_compiled`'s and `made`'s.
        unsafe {
            let message = print::render_compiled(rt, node, values)?;
            made(rt, node, &message)
        }
    })
}

/// Whether `binding` is a category's record: a name `error` goes by, spelled with its path, so a
/// lex of it wakes `error`'s parse.
///
/// # Safety
/// `binding` must be null or a binding dyad from the store.
pub(crate) unsafe fn is_category(types: &Core, binding: DyadPtr) -> bool {
    !binding.is_null()
        && Binding::read(binding).dyad == types.error.error
        && Binding::spelling(binding).starts_with("error.")
}

/// Whether a node of type `ty` ends the run where it runs: a raise or an `abort`.
pub(crate) fn ends_run(types: &Core, ty: DyadPtr) -> bool {
    ty == types.error.error || ty == types.abort.abort
}

/// # Safety
/// `node` must be a raise `build` made.
unsafe fn fault(node: DyadPtr, message: Vec<u8>) -> RunError {
    let category = *(dyad::value(node) as *const DyadPtr).add(CATEGORY);
    let message = String::from_utf8_lossy(&message).into_owned();
    RunError::Raised(Box::new((Binding::spelling(category), message)))
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    let message = print::render(rt, node)?;
    // SAFETY: `node` is a raise `build` made.
    Err(unsafe { fault(node, message) })
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a raise `build` made; its parts are reduced dyads.
    unsafe { print::lower_shown(lw, node, compiled_raise) }
}

/// # Safety
/// Called only by compiled code, with its context, a raise from the store and a block holding
/// one container per part, in order.
unsafe extern "C" fn compiled_raise(
    ctx: *mut crate::run::Context,
    node: DyadPtr,
    values: *const i64,
) -> i64 {
    crate::run::interpret_step(ctx, |rt| {
        // SAFETY: the caller's contract is `render_compiled`'s and `fault`'s.
        Err(unsafe { fault(node, print::render_compiled(rt, node, values)?) })
    })
}
