// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `error`: the word of error values. `error.X := «description»` declares the category `X`, a
//! name of the scope it stands in, and `error.X «message»` raises it, `{…}` read as `print`
//! reads it. Inside a function that gives `T!` the raise fills the result's `err` and leaves the
//! function; anywhere else it is the node `[parts, op, output_type, category]`, whose run ends the
//! run as a fault. A `try` that passes an error on makes it one hop longer here, and one at the
//! top level hands it to the command line.
//! ›Error values are in v0.1.0; an error names a category, which is a scoped name‹ [[R143]]

use cranelift_codegen::ir::Value;

use super::binding::Binding;
use super::callable::{self, Callables};
use super::{carried, hole, meta, print, union, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::regex_trie::RegexTrie;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// The binding record of the category a raise names.
const CATEGORY: usize = 3;
/// The `try` node a passing node stands in, its hop's location.
const LOCATION: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct ErrorIds {
    pub error: DyadPtr,
    pub leaf: DyadPtr,
    /// The error value's type, `array hop`, which `error` names where a type is wanted; the
    /// native one until identities/error.logos is read.
    pub value: DyadPtr,
    /// The error value's native type, which `value` is until identities/error.logos is read, and
    /// which a `T!` made before then holds as `err`.
    pub native: DyadPtr,
    /// The type of one element of the error value.
    pub hop: DyadPtr,
    /// error.logos's function that makes the error value of one raise.
    pub one_hop: DyadPtr,
    /// The binding record of `error.out_of_memory`, the category of an `alloc` that gets no
    /// memory; null until identities/error.logos is read.
    pub out_of_memory: DyadPtr,
    /// The binding record of `error.no_whole_number`, the category of a float converted into an
    /// integer type that has no whole number; null until identities/error.logos is read.
    pub no_whole_number: DyadPtr,
    /// A raise read as the error value it makes, `[parts, op, output_type, category]`: what a
    /// raise inside a function that gives `T!` fills `err` with.
    pub raised: DyadPtr,
    pub raised_leaf: DyadPtr,
    /// error.logos's function that makes the error value a `try` passes on.
    pub passed: DyadPtr,
    /// The error value a `try` passes on, `[source, op, output_type, location]`, `source` the one
    /// it was handed and `location` the `try`.
    pub passing: DyadPtr,
    pub passing_leaf: DyadPtr,
    /// The error value a `try` at the top level hands to the command line, `[source, op,
    /// output_type]`, `source` the one it was handed: its run stops the run with that error.
    pub handed: DyadPtr,
    pub handed_leaf: DyadPtr,
    /// An `alloc` read as the `@T!` it gives, `[given, refused, op, output_type]`: `given` fills
    /// the union's `value` with the block, and `refused` its `err` where the alloc gets no memory.
    pub refusable: DyadPtr,
    pub refusable_leaf: DyadPtr,
    /// The error value of an `alloc` that got no memory, `[op, output_type]`: one hop of
    /// `error.out_of_memory`.
    pub refused: DyadPtr,
    pub refused_leaf: DyadPtr,
}

const ARRAY: carried::File = carried::File {
    path: "identities/array.logos",
    source: include_str!("../../identities/array.logos"),
};
const FILE: carried::File = carried::File {
    path: "identities/error.logos",
    source: include_str!("../../identities/error.logos"),
};
const OUT_OF_MEMORY: &str = "error.out_of_memory";
const NO_WHOLE_NUMBER: &str = "error.no_whole_number";

/// Reads identities/error.logos into `section`, after the array it is built from: the error
/// value's type, its element's, the function that makes the value of one raise, and the
/// categories the seed raises, of an `alloc` that gets no memory and of a float with no whole
/// number, the names of either file the root is given.
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
    let function = |name| {
        let f = carried::name_in(trie, section, &FILE, name).identity;
        // SAFETY: `f` is the dyad a name of the section holds.
        let made_by = unsafe { dyad::ty(f) };
        assert!(made_by == core.fn_type, "{}: `{name}` is no function", FILE.path);
        f
    };
    let ids = ErrorIds {
        value: ty("hops"),
        hop: ty("hop"),
        one_hop: function("one_hop"),
        out_of_memory: carried::name_in(trie, section, &FILE, OUT_OF_MEMORY).binding,
        no_whole_number: carried::name_in(trie, section, &FILE, NO_WHOLE_NUMBER).binding,
        passed: function("passed"),
        ..core.error
    };
    let published = [
        (OUT_OF_MEMORY.to_string(), ids.out_of_memory),
        (NO_WHOLE_NUMBER.to_string(), ids.no_whole_number),
    ];
    carried::publish(store, trie, core, &FILE, &published);
    ids
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
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["source", "op", "output_type", "location"],
    );
    let passing = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `passing` carries the record just built.
    unsafe { meta::mark_own(passing) };
    cx.lower.insert(passing, lower_passing);
    let passing_leaf = callable::mint_native(cx.store, cs.callable, run_passing, cs.seed_native);
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["source", "op", "output_type"],
    );
    let handed = cx.store.alloc_head(cx.type_, record);
    let handed_leaf = callable::mint_native(cx.store, cs.callable, run_handed, cs.seed_native);
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["given", "refused", "op", "output_type"],
    );
    let refusable = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `refusable` carries the record just built.
    unsafe { meta::mark_own(refusable) };
    let refusable_leaf =
        callable::mint_native(cx.store, cs.callable, run_refusable, cs.seed_native);
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["op", "output_type"],
    );
    let refused = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `refused` carries the record just built.
    unsafe { meta::mark_own(refused) };
    let refused_leaf = callable::mint_native(cx.store, cs.callable, run_refused, cs.seed_native);
    let null = std::ptr::null_mut();
    ErrorIds {
        error,
        leaf,
        value: null,
        native: null,
        hop: null,
        one_hop: null,
        out_of_memory: null,
        no_whole_number: null,
        raised,
        raised_leaf,
        passed: null,
        passing,
        passing_leaf,
        handed,
        handed_leaf,
        refusable,
        refusable_leaf,
        refused,
        refused_leaf,
    }
}

/// The error value's native type: the address of a block of 16 bytes, where error.logos's
/// `hops` keeps `ptr`, then `size`, so a place of either holds the same word. `array`, which
/// `hops` is built from, has `parse` bodies that give `void!`, whose `err` is the error value, so
/// the native type is made before any file is read and the `T!`s made before error.logos is
/// read hold it (›A dependency cycle among core identities is broken with native versions
/// first‹ [[R16]]). No program reaches a value of it: only the driver reads one, which it ends
/// as error.logos's `hops`.
pub(super) fn native(store: &mut Store, core: &Core) -> ErrorIds {
    let scope = super::scope::mint(store, core.scope, std::ptr::null_mut());
    let none = super::array::build(store, core.array_, &[]);
    let extent = meta::Extent { size_bytes: 16, align: 8 };
    let (rank, assoc, parse) = (meta::prec::APPLY, crate::parse::Assoc::Left, core.derived_parse);
    let record = meta::record_layout(store, scope, none, none, extent, scope, rank, assoc, parse);
    let block = store.alloc_head(core.type_, record);
    // SAFETY: `block` is the type node just made.
    let value = unsafe { super::pointer::make_pointer_type(store, core, block) };
    ErrorIds { value, native: value, ..core.error }
}

/// Whether `t` is the error value's type: error.logos's `hops`, or the native type that stands
/// for it in the `T!`s made before error.logos is read.
pub(crate) fn is_value(types: &Core, t: DyadPtr) -> bool {
    t == types.error.value || t == types.error.native
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
    hop(rt, *(dyad::value(node) as *const DyadPtr).add(CATEGORY), node, message)
}

/// error.logos's `one_hop` of `category`'s record, `message` and the node `at` that makes it.
///
/// # Safety
/// `category` must be a category's binding record, and error.logos read.
unsafe fn hop(
    rt: &mut Runtime,
    category: DyadPtr,
    at: DyadPtr,
    message: &[u8],
) -> Result<i64, RunError> {
    let types: *const Core = rt.types();
    // SAFETY: the runtime borrows the core, so the store's growth leaves it where it is.
    let types = &*types;
    let text = super::string::build_text(rt.store(), types.string_, message);
    rt.apply_values(types.error.one_hop, &[category as i64, text as i64, at as i64])
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

/// The run error the error value `value` stops the run with where nobody receives it, at the top
/// level and in a parse that gives one back: its first hop's category and message, the raise's,
/// as a raise nobody handles shows (›Target: errors are values (`T!`), handled by `if`, passed on
/// by `try`…‹ [[R140]]). `value` is only read: whoever holds it ends it.
///
/// # Safety
/// As [`first_hop`].
pub(crate) unsafe fn handed_on(rt: &mut Runtime, value: DyadPtr) -> Result<RunError, RunError> {
    let hop = first_hop(rt, value)?.expect("an error value holds its raise's hop");
    Ok(RunError::Raised(Box::new(hop)))
}

/// A `try` at the top level handing `source`, the error value it was handed, to the command line.
pub(crate) fn build_handed(store: &mut Store, types: &Core, source: DyadPtr) -> DyadPtr {
    store.alloc_words(types.error.handed, &[source, types.error.handed_leaf, types.void_])
}

fn run_handed(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a handed node, whose source gives a value of the error value's type.
    unsafe {
        let value = rt.run(*(dyad::value(node) as *const DyadPtr))?;
        Err(handed_on(rt, value as DyadPtr)?)
    }
}

/// The `alloc` node `alloc` read as the `@T!` it gives, `fallible`, whose member holes are `value`
/// and `err`: the block fills `value`, and where the alloc gets no memory, an error of
/// `error.out_of_memory` fills `err` (›There is no bare `error «…»`; `abort «…»` stops the
/// program; `alloc` returns `@T!`‹ [[R145]]).
///
/// # Safety
/// `alloc` must be an `alloc` node, `fallible` the union `@T!` of the pointer it gives, and
/// `value` and `err` its member holes.
pub(crate) unsafe fn build_refusable(
    store: &mut Store,
    types: &Core,
    alloc: DyadPtr,
    [fallible, value, err]: [DyadPtr; 3],
) -> DyadPtr {
    let given = union::build_fill(store, types, alloc, fallible, value);
    let made =
        store.alloc_words(types.error.refused, &[types.error.refused_leaf, hole::type_in(err)]);
    let refused = union::build_fill(store, types, made, fallible, err);
    let words = [given, refused, types.error.refusable_leaf, fallible];
    store.alloc_words(types.error.refusable, &words)
}

fn run_refusable(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a refusable node, whose `given` and `refused` are fill nodes of its union.
    unsafe {
        let parts = dyad::value(node) as *const DyadPtr;
        match rt.run(*parts) {
            Err(RunError::OutOfMemory) => {
                let refused = *parts.add(1);
                rt.refusing_alloc(|rt| rt.run(refused)).unwrap_or(Err(RunError::OutOfMemory))
            }
            given => given,
        }
    }
}

/// error.logos's `one_hop` of `error.out_of_memory`, its own node the hop's location. The category
/// is read where it runs, since the `alloc`s of the files read before error.logos are built before
/// it is declared; an `alloc` that gets no memory then stops the run.
fn run_refused(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    let category = rt.types().error.out_of_memory;
    if category.is_null() {
        return Err(RunError::OutOfMemory);
    }
    // SAFETY: `category` is set where error.logos is read.
    unsafe { hop(rt, category, node, b"this alloc got no memory") }
}

/// error.logos's `one_hop` of `error.no_whole_number`, `at` the hop's location: the error value
/// of a float converted into an integer type that has no whole number. Before error.logos is
/// read the conversion stops the run.
pub(crate) fn no_whole_number(rt: &mut Runtime, at: DyadPtr) -> Result<i64, RunError> {
    let category = rt.types().error.no_whole_number;
    if category.is_null() {
        return Err(RunError::NoWholeNumber);
    }
    // SAFETY: `category` is set where error.logos is read.
    unsafe { hop(rt, category, at, b"this float is not a number (NaN) or is infinite") }
}

/// The category and the message of the first hop of the error value `value`, the raise's. `None`
/// for an error value with no hop.
///
/// # Safety
/// `value` must be a value of error.logos's `hops`, which is its node (›A value of a type built
/// by a Logos `parse` travels as a pointer‹ [[R225]]).
unsafe fn first_hop(
    rt: &mut Runtime,
    value: DyadPtr,
) -> Result<Option<(String, String)>, RunError> {
    let types: *const Core = rt.types();
    // SAFETY: the runtime borrows the core, so the store's growth leaves it where it is.
    let types = &*types;
    let (hops, hop) = (types.error.value, types.error.hop);
    let field = |ty: DyadPtr, name: &str| {
        let bindings = super::array::items(meta::record_field_bindings_of(ty));
        let k = bindings.iter().position(|&b| Binding::spelling(b) == name);
        let k = k.expect("error.logos declares the field");
        let field = super::array::items(meta::record_fields_of(ty))[k];
        (k, super::hole::type_in(field), Binding::read(bindings[k]).offset)
    };
    let ((ptr, ptr_type, _), (size, size_type, _)) = (field(hops, "ptr"), field(hops, "size"));
    let first = super::this::slot_value(rt, value, ptr, ptr_type)? as *const u8;
    if first.is_null() || super::this::slot_value(rt, value, size, size_type)? == 0 {
        return Ok(None);
    }
    // The hops lie one after another in `ptr`'s block, each in its own bytes.
    debug_assert!(super::this::in_bytes(types, hop));
    let word = |name| std::ptr::read_unaligned(first.add(field(hop, name).2) as *const usize);
    let category = word("category") as DyadPtr;
    let message = super::string::text(word("message") as DyadPtr);
    Ok(Some((Binding::spelling(category), String::from_utf8_lossy(message).into_owned())))
}

/// The error value a `try` at `location` passes on where it runs: `source`'s, the error value it
/// was handed, with one more hop, its own (›A raise is `error.X «message»`; the error value is an
/// array with one element per hop‹ [[R144]]).
pub(crate) fn build_passing(
    store: &mut Store,
    types: &Core,
    source: DyadPtr,
    location: DyadPtr,
) -> DyadPtr {
    let words = [source, types.error.passing_leaf, types.error.value, location];
    store.alloc_words(types.error.passing, &words)
}

/// error.logos's `passed` of `below`, the error value a passing node `node` was handed, with the
/// node's location as the new hop's: the one step both tiers take.
///
/// # Safety
/// `node` must be a node [`build_passing`] built, `below` a value of the error value's type.
unsafe fn passed_on(rt: &mut Runtime, node: DyadPtr, below: i64) -> Result<i64, RunError> {
    let passed = rt.types().error.passed;
    let location = *(dyad::value(node) as *const DyadPtr).add(LOCATION);
    rt.apply_values(passed, &[below, location as i64])
}

fn run_passing(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a passing node, whose source gives the error value it was handed.
    unsafe {
        let below = rt.run(*(dyad::value(node) as *const DyadPtr))?;
        passed_on(rt, node, below)
    }
}

fn lower_passing(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as [`run_passing`].
    unsafe {
        let below = lw.lower(*(dyad::value(node) as *const DyadPtr))?;
        let node = lw.node_addr(node);
        lw.call_seed(compiled_passing as *const () as usize, &[node, below])
    }
}

/// # Safety
/// Called only by compiled code, with its context, a passing node `lower_passing` baked and the
/// error value its source gave.
unsafe extern "C" fn compiled_passing(
    ctx: *mut crate::run::Context,
    node: DyadPtr,
    below: i64,
) -> i64 {
    crate::run::interpret_step(ctx, |rt| {
        // SAFETY: the caller's contract is `passed_on`'s.
        unsafe { passed_on(rt, node, below) }
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

/// Whether a node of type `ty` ends the run where it runs: a raise, an `abort`, or an error a
/// `try` hands to the command line.
pub(crate) fn ends_run(types: &Core, ty: DyadPtr) -> bool {
    ty == types.error.error || ty == types.abort.abort || ty == types.error.handed
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identities::{by_copy, numtype, read};

    #[test]
    fn the_native_error_value_holds_what_hops_holds() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let (native, hops) = (core.error.native, core.error.value);
        assert_ne!(native, hops, "error.logos's `hops` replaces the native type");
        // SAFETY: both are type nodes `Core::build` made, `native` a pointer type.
        unsafe {
            assert_eq!(read::carry_of(&core, native), read::carry_of(&core, hops));
            assert_eq!(by_copy::place_width(&core, native), by_copy::place_width(&core, hops));
            let block = numtype::pointee_of(native);
            assert_eq!(meta::record_size_of(block), meta::record_size_of(hops));
            assert_eq!(meta::record_align_of(block), meta::record_align_of(hops));
            let laid: Vec<(String, usize)> =
                super::super::array::items(meta::record_field_bindings_of(hops))
                    .iter()
                    .map(|&b| (Binding::spelling(b), Binding::read(b).offset))
                    .filter(|(name, _)| name == "ptr" || name == "size")
                    .collect();
            assert_eq!(laid, [("ptr".to_string(), 0), ("size".to_string(), 8)]);
        }
    }

    #[test]
    fn either_error_value_type_is_the_error_value() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        assert!(is_value(&core, core.error.value));
        assert!(is_value(&core, core.error.native));
        assert!(!is_value(&core, core.error.hop));
    }
}
