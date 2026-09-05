// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The ruled drop model (issue #49, DESIGN ›Explicit heap, and no implicit
//! destruction‹): `alloc`, `own`, `drop`, `free`, and `defer`, the five
//! identities that give the seed explicit heap ownership with constructor-inserted
//! teardown. They are one mechanism, so they share this one file rather than the
//! usual one-file-per-identity split.
//!
//! **The model.** `alloc T v` heap-allocates room for a `T`, writes `v` into it,
//! and yields an *owning* pointer — an ordinary `@T` whose logos node carries a
//! non-null `destructor` (the first identity in the seed that does; a `&x` borrow
//! mints the same `@T` with a null destructor, so owning-ness rides the node
//! `alloc` built, not `@T` in general). Binding that owning pointer to a name
//! inserts `defer free <place>` into the place's scope (the parser does this at
//! the binding site — `alloc`'s result lands in a place only there — see
//! [`crate::parse::Parser::construct_decl`]). `defer` runs its teardown LIFO at
//! scope exit, by the scope's own machinery ([`crate::identities::scope`]), as
//! ordinary reflectable body structure, never hidden drop glue.
//!
//! **Ownership may not slip out of the machinery that frees it.** Three rules
//! fail closed rather than leak or hand back freed memory, each guarding a way
//! out: an owning value must be **bound to a name** (an unbound one, a bare
//! `alloc` handed to a call, has nothing to attach its teardown to); a scope's
//! **value** may not be a place that scope owns (the inserted teardown frees it
//! on the way out, so the value handed back would already be freed — `own` is
//! how ownership leaves a scope); and ownership may not cross a **function
//! return** (a block hands ownership to its binder in full view of the parse,
//! but a call hides its body behind a return logos that cannot yet say it
//! transfers ownership, so the caller would not know it owes a `free`). The
//! last lifts once a logos carries its ownership mode — `take`/`drop` as gates
//! on a reference, the same primitive as `pub`/`mut` (issue #53).
//!
//! **Teardown follows the owner.** `own a` is a move: it reads `a`'s pointer,
//! empties `a`, and yields the pointer; bound to a new name it inserts a *fresh*
//! `defer free` at that binding, so the teardown migrates with ownership. `drop a`
//! runs `a`'s destructor eagerly and empties `a`. Both are `take`s, and the
//! empty is the v1 stand-in for the phase-bit drop flag: the place is written to
//! **null**, so any pending `defer free`/`drop` over it is the sanctioned no-op —
//! no double free, and an `own`-escaped source frees nothing (DESIGN's null
//! *undefined*). *(v1 approximations, recorded in DESIGN ›Explicit heap‹, July
//! 2026: the null-pointer drop flag; attachment only at a named binding, so a
//! bare owning temporary passed as an argument stays rejected; a single system
//! allocator; and `free`/`drop` coinciding for the one owning type. Shared
//! ownership — a `share` co-owning bind with a refcount destructor — is the
//! recorded next layer on this same machinery.)*
//!
//! **Node shapes** (operand records, so the op slot carries the run native):
//! `alloc` → `[pointee, init, op]`; `free`/`drop`/`own` → `[place, pointee, op]`;
//! `defer` → `[inner, op]`. `free`'s run native *is* the owning pointer's stored
//! destructor, so `drop` — which reads the destructor off the place's logos and
//! invokes it — routes straight to the same teardown, exercising the slot.
//!
//! **Compilation.** None of the five lower: a function whose body reaches one
//! declines to compile and stays interpreted (the sanctioned deopt, Q4 ruling),
//! so heap paths run on the body-walk in both tiers.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype;
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::id_context::IdContext;
use crate::parse::{Assoc, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::dyad::DyadPtr;

/// Operand index of the pointee logos in an `alloc` node (`[pointee, init, op]`).
const ALLOC_POINTEE: usize = 0;
/// Operand index of the initializer value in an `alloc` node.
const ALLOC_INIT: usize = 1;
/// Operand index of the place in a `free`/`drop`/`own` node (`[place, pointee, op]`).
const TEARDOWN_PLACE: usize = 0;
/// Operand index of the pointee logos in a `free`/`drop`/`own` node.
const TEARDOWN_POINTEE: usize = 1;
/// Operand index of the deferred inner expression in a `defer` node (`[inner, op]`).
const DEFER_INNER: usize = 0;

/// The identities and natives the drop model registers, returned to `Core::build`.
pub(super) struct DropModel {
    pub alloc_: DyadPtr,
    pub own_: DyadPtr,
    pub drop_: DyadPtr,
    pub free_: DyadPtr,
    pub defer_: DyadPtr,
    /// `free`'s run native, also the owning pointer's stored destructor.
    pub teardown_leaf: DyadPtr,
    pub own_leaf: DyadPtr,
    pub drop_leaf: DyadPtr,
    pub alloc_leaf: DyadPtr,
    pub defer_leaf: DyadPtr,
}

/// Register all five identities: their spellings, operand records, and run
/// natives. Called from `Core::build` after the callable machinery and the
/// numeric logos exist (an `alloc` node's init is a numeric value; the natives
/// are callable leaves).
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> DropModel {
    // The shared teardown native, minted once: `free`'s op leaf AND the owning
    // pointer's destructor slot both point at it, which is what makes `drop`'s
    // "run the place's destructor" reach the same code as an inserted `free`.
    let teardown_leaf = callable::mint_native(cx.store, cs.callable, run_teardown, cs.seed_native);

    // `alloc T v`: a fresh-start keyword constructor (NaN precedence → the driver
    // invokes it immediately). Its constructor parses the following typed value.
    let alloc_ = keyword(cx, "alloc", meta::prec::PREFIX, &["pointee", "init", "op"], |p, _id, tape| {
        let init = p.parse_expression()?;
        let types = p.types();
        let node = build_alloc(p.store(), &types, init)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    let alloc_leaf = callable::mint_native(cx.store, cs.callable, run_alloc, cs.seed_native);

    // `own a`: move out of a place; yields the pointer, empties the source.
    // `own a`: move the pointer out, emptying the source; `a` is dead from here
    // (DESIGN ›Memory and concurrency‹, *`own` and `drop` are static*).
    let own_ = keyword(cx, "own", meta::prec::PREFIX, &["place", "pointee", "op"], |p, _id, tape| {
        let (place, ended) = p.parse_place_operand(true)?;
        let types = p.types();
        let node = build_teardown(p.store(), &types, types.own_, place, true)?;
        tape.place(node);
        if let Some(ended) = ended {
            p.mark_dead(ended, node);
        }
        Ok(crate::parse::Constructed::Placed)
    });
    let own_leaf = callable::mint_native(cx.store, cs.callable, run_own, cs.seed_native);

    // `drop a`: run the place's destructor eagerly and empty it; `a` is dead
    // from here. Any identity may be dropped (DESIGN ›Memory and concurrency‹:
    // "drop x applies to any identity, running its destructor where one is set
    // and emptying the place either way, so one verb releases a name whatever
    // its type"): an owning place gets the teardown node, anything else an
    // inert `drop` node — the emptying is the parse-time dead mark, and the
    // run-time write is not needed, since no later use can observe the place.
    let drop_ = keyword(cx, "drop", meta::prec::PREFIX, &["place", "pointee", "op"], |p, _id, tape| {
        let (place, ended) = p.parse_place_operand(true)?;
        let types = p.types();
        let node = if is_owning_place(place) {
            build_teardown(p.store(), &types, types.drop_, place, true)?
        } else {
            build_inert_drop(p.store(), &types, place)
        };
        tape.place(node);
        if let Some(ended) = ended {
            p.mark_dead(ended, node);
        }
        Ok(crate::parse::Constructed::Placed)
    });
    cx.lower.insert(drop_, lower_drop);
    let drop_leaf = callable::mint_native(cx.store, cs.callable, run_drop, cs.seed_native);

    // `free a`: the teardown the binding site inserts; user-writable too. Like
    // `own`/`drop` it demands an owning place — only an `alloc`-minted pointer
    // points at heap the allocator can free; freeing a borrow (`&x`) would hand
    // a stack/global address to the allocator.
    let free_ = keyword(cx, "free", meta::prec::PREFIX, &["place", "pointee", "op"], |p, _id, tape| {
        // The raw teardown verb leaves the name alive: only `own`/`drop` end it.
        let (place, _) = p.parse_place_operand(false)?;
        let types = p.types();
        let node = build_teardown(p.store(), &types, types.free_, place, true)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });

    // `defer <expr>`: hold `<expr>` for LIFO execution at scope exit. Its own run
    // native is a no-op — the scope machinery runs the inner, never the defer node.
    let defer_ = keyword(cx, "defer", meta::prec::READER, &["inner", "op"], |p, _id, tape| {
        let inner = p.parse_expression()?;
        let types = p.types();
        let node = build_defer(p.store(), &types, inner);
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    let defer_leaf = callable::mint_native(cx.store, cs.callable, run_defer_noop, cs.seed_native);

    DropModel {
        alloc_,
        own_,
        drop_,
        free_,
        defer_,
        teardown_leaf,
        own_leaf,
        drop_leaf,
        alloc_leaf,
        defer_leaf,
    }
}

/// Register a fresh-start keyword identity: `spelling` in the trie, an operand
/// record (`TUPLE`, at `precedence` on the one axis), and `construct` in the
/// `metas` table. Returns
/// the identity node.
fn keyword(
    cx: &mut Cx,
    spelling: &str,
    precedence: f64,
    roles: &[&str],
    construct: crate::parse::ConstructFn,
) -> DyadPtr {
    let record = meta::operand_record(cx, meta::TUPLE_TAG, precedence, Assoc::Left, roles);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert(spelling, IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, construct);
    id
}

/// Heap-allocate `width` bytes for a scalar/pointer value. v1's single allocator
/// is the system one; a scalar's `Layout` is `(width, width)` — every scalar
/// width (1/2/4/8) is a power of two, so it is a valid alignment.
///
/// # Safety
/// `width` must be a non-zero scalar width; the returned block is freed exactly
/// once by [`heap_free`] with the same `width` (`free`'s null-place no-op is what
/// enforces the once).
unsafe fn heap_alloc(width: usize) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(width, width)
        .expect("a scalar width is a valid power-of-two layout");
    std::alloc::alloc(layout)
}

/// Free a block [`heap_alloc`] returned for `width` bytes.
///
/// # Safety
/// `ptr` must be a live block from [`heap_alloc`] with the same `width`.
unsafe fn heap_free(ptr: *mut u8, width: usize) {
    let layout = std::alloc::Layout::from_size_align(width, width)
        .expect("a scalar width is a valid power-of-two layout");
    std::alloc::dealloc(ptr, layout);
}

/// The byte width of a pointee logos (`i32` → 4, a pointer `@T` → 8).
///
/// # Safety
/// `pointee` must be a scalar or pointer logos node.
unsafe fn pointee_width(pointee: DyadPtr) -> usize {
    numtype::numtype_of_type(pointee).bytes()
}

/// Build an `alloc` node from its parsed initializer. The pointee logos is the
/// initializer's own logos (`alloc i32 5` allocates an `i32`), so the value
/// carries both what to allocate and what to store; a non-scalar initializer is
/// rejected (v1 allocates scalars and pointers only).
pub(super) fn build_alloc(
    store: &mut Store,
    types: &CoreTypes,
    init: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `init` is a reduced dyad just parsed.
    let pointee = match unsafe { crate::identities::numtype_of(types, init) } {
        crate::identities::Operand::Concrete(_) | crate::identities::Operand::Pointer(_) => {
            unsafe { crate::identities::scalar_binding_type(store, types, init).0 }
        }
        _ => return Err(ParseError::UnsupportedOperands),
    };
    let value = store.alloc_operands(&[pointee, init, types.ops.alloc_]);
    Ok(store.alloc_raw(types.alloc_, value))
}

/// Build a `free`/`drop`/`own` node `[place, pointee, op]` over `place`. When
/// `require_owning`, the place must carry a non-null destructor (an owning
/// pointer) — a borrow (`&x`) or a plain value cannot be moved or dropped.
///
/// # Safety-free at the call boundary; reads `place`'s logos, which must be a
/// reduced dyad from the store.
pub(crate) fn build_teardown(
    store: &mut Store,
    types: &CoreTypes,
    op_id: DyadPtr,
    place: DyadPtr,
    require_owning: bool,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `place` is a reduced dyad; its logos is a valid logos node.
    let logos = unsafe { (*place).ty };
    if unsafe { !numtype::is_pointer_type(logos) } {
        return Err(ParseError::BadAssignTarget);
    }
    if require_owning && unsafe { meta::destructor_of(logos).is_null() } {
        // A borrow or a non-owning pointer: nothing to move or drop.
        return Err(ParseError::BadAssignTarget);
    }
    let pointee = unsafe { numtype::pointee_of(logos) };
    let leaf = if op_id == types.own_ {
        types.ops.own_
    } else if op_id == types.drop_ {
        types.ops.drop_
    } else {
        types.ops.teardown_
    };
    let value = store.alloc_operands(&[place, pointee, leaf]);
    Ok(store.alloc_raw(op_id, value))
}

/// Whether `place`'s logos is an owning pointer: a pointer type whose
/// `destructor` slot is set (the node `alloc` built, or an `own`-bound place),
/// as opposed to a borrow or a plain value.
///
/// # Safety-free at the call boundary; reads `place`'s logos, which must be a
/// reduced dyad from the store.
fn is_owning_place(place: DyadPtr) -> bool {
    // SAFETY: `place` is a reduced dyad; its logos is a valid logos node.
    unsafe {
        let logos = (*place).ty;
        numtype::is_pointer_type(logos) && !meta::destructor_of(logos).is_null()
    }
}

/// Build the `drop` node for a non-owning place: `[place, null, op]`, the
/// null pointee marking that there is no destructor to run and nothing to
/// free. It runs and lowers to unit; its work is done at parse, where the
/// name became dead. It still stands in the body as the emptying node
/// reflection reads (DESIGN ›Name resolution is scope-filtered‹).
fn build_inert_drop(store: &mut Store, types: &CoreTypes, place: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[place, std::ptr::null_mut(), types.ops.drop_]);
    store.alloc_raw(types.drop_, value)
}

/// Lower a `drop` node: the inert form (null pointee) is unit; the owning form
/// has no lowering, like `alloc`/`free`, so the function declines to compile
/// and stays interpreted (the sanctioned deopt).
fn lower_drop(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a `drop` node `[place, pointee, op]` from the store.
    let pointee = unsafe { *((*node).value as *const DyadPtr).add(TEARDOWN_POINTEE) };
    if pointee.is_null() {
        Ok(lw.const_i32(0))
    } else {
        Err(CompileError::NotLowerable(node))
    }
}

/// Build a `defer <inner>` node `[inner, op]`.
pub(crate) fn build_defer(store: &mut Store, types: &CoreTypes, inner: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[inner, types.ops.defer_]);
    store.alloc_raw(types.defer_, value)
}

/// The deferred inner expression of a `defer` node.
///
/// # Safety
/// `node` must be a `defer` node as [`build_defer`] lays it out.
pub(crate) unsafe fn deferred_inner_of(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr).add(DEFER_INNER)
}

/// The place an inserted teardown frees: the `defer free <place>` node's inner
/// free node, read back to its place slot. The binding site pushes these onto
/// the scope's pending list, so a scope can ask which places *it* will free —
/// which is what the escape check in [`crate::parse::Parser::parse_sequence`]
/// compares its tail against.
///
/// # Safety
/// `defer_node` must be a `defer` node over a teardown, as the binding site
/// builds ([`build_defer`] over [`build_teardown`]).
pub(crate) unsafe fn teardown_place_of(defer_node: DyadPtr) -> DyadPtr {
    let inner = deferred_inner_of(defer_node);
    *((*inner).value as *const DyadPtr).add(TEARDOWN_PLACE)
}

/// The pointee logos of an `alloc`/`own` node — what a bound owning pointer
/// points at, so the binding site can mint its owning `@pointee` type. `alloc`
/// stores it at [`ALLOC_POINTEE`], `own` at [`TEARDOWN_POINTEE`]; a scope whose
/// tail is one propagates through (an owning value moved out of a block).
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn owning_pointee_of(types: &CoreTypes, node: DyadPtr) -> Option<DyadPtr> {
    let logos = (*node).ty;
    if logos == types.alloc_ {
        Some(*((*node).value as *const DyadPtr).add(ALLOC_POINTEE))
    } else if logos == types.own_ {
        Some(*((*node).value as *const DyadPtr).add(TEARDOWN_POINTEE))
    } else if logos == types.scope {
        // A block that yields an owning value moves ownership to the binder.
        crate::parse::last_sequence_expr(node).and_then(|tail| owning_pointee_of(types, tail))
    } else {
        None
    }
}

/// Whether `node` produces an owning pointer — the binding-site test that decides
/// whether `a := <node>` mints an owning place and inserts `defer free a`.
///
/// # Safety
/// As [`owning_pointee_of`].
pub(crate) unsafe fn is_owning_value(types: &CoreTypes, node: DyadPtr) -> bool {
    owning_pointee_of(types, node).is_some()
}

/// Run `alloc`: evaluate the initializer, heap-allocate the pointee's width,
/// write the value in, and yield the block's address (the owning pointer). The
/// runtime notes the live allocation so leaks and double-frees are observable.
fn run_alloc(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `alloc` node `[pointee, init, op]` from the store.
    unsafe {
        let slots = (*node).value as *const DyadPtr;
        let pointee = *slots.add(ALLOC_POINTEE);
        let init = *slots.add(ALLOC_INIT);
        let bits = rt.run(init)?;
        let width = pointee_width(pointee);
        let mem = heap_alloc(width);
        if mem.is_null() {
            return Err(RunError::BadValue);
        }
        numtype::write_scalar(pointee, mem, bits);
        rt.note_alloc();
        Ok(mem as i64)
    }
}

/// Run the teardown (`free`, and the owning pointer's destructor): read the
/// place's pointer; if it is null (an emptied place) do nothing — the sanctioned
/// no-op — otherwise free the block, note the free, and null the place so a
/// second teardown over it also no-ops.
fn run_teardown(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[place, pointee, op]` teardown node from the store.
    unsafe {
        let slots = (*node).value as *const DyadPtr;
        let place = *slots.add(TEARDOWN_PLACE);
        let pointee = *slots.add(TEARDOWN_POINTEE);
        let slot = rt.place_addr(place).ok_or(RunError::BadValue)?;
        if slot.is_null() {
            return Err(RunError::BadValue);
        }
        let ptr = std::ptr::read_unaligned(slot as *const i64) as u64 as *mut u8;
        if ptr.is_null() {
            return Ok(0); // emptied place: the sanctioned no-op
        }
        // Tests observe teardown *order* (LIFO) by the value each freed block held.
        #[cfg(test)]
        FREE_LOG.with(|log| log.borrow_mut().push(numtype::read_scalar(pointee, ptr)));
        heap_free(ptr, pointee_width(pointee));
        rt.note_free();
        std::ptr::write_unaligned(slot as *mut i64, 0); // the drop flag
        Ok(0)
    }
}

/// Run `drop`: read the destructor off the place's logos and invoke it, so the
/// teardown genuinely flows through the reserved `destructor` slot. The inert
/// form (a null pointee: a non-owning place, dropped only to end its name) is
/// unit. Otherwise a null destructor cannot happen here — `build_teardown`
/// demanded an owning place at parse — so a null slot is a malformed node.
fn run_drop(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `drop` node; in the owning form its place's logos
    // carries the destructor (owning-ness checked at parse), whose entry is a
    // `RunFn` reading the same `[place, pointee, op]` layout this node has.
    unsafe {
        let slots = (*node).value as *const DyadPtr;
        if (*slots.add(TEARDOWN_POINTEE)).is_null() {
            return Ok(0);
        }
        let place = *slots.add(TEARDOWN_PLACE);
        let dtor = meta::destructor_of((*place).ty);
        if dtor.is_null() || !callable::is_callable(dtor) {
            return Err(RunError::BadValue);
        }
        let entry = std::mem::transmute::<usize, crate::run::RunFn>(callable::entry_of(dtor));
        entry(rt, node)
    }
}

/// Run `own`: read the place's pointer, empty the place (write null), and yield
/// the pointer — a move. The moved-from place's pending `defer free` then no-ops.
fn run_own(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `own` node `[place, pointee, op]` from the store.
    unsafe {
        let place = *((*node).value as *const DyadPtr).add(TEARDOWN_PLACE);
        let slot = rt.place_addr(place).ok_or(RunError::BadValue)?;
        if slot.is_null() {
            return Err(RunError::BadValue);
        }
        let ptr = std::ptr::read_unaligned(slot as *const i64);
        std::ptr::write_unaligned(slot as *mut i64, 0); // empty the source
        Ok(ptr)
    }
}

/// Run a `defer` node in place: a no-op. A defer holds its inner for scope-exit
/// execution; the scope machinery ([`crate::identities::scope`]) and the
/// top-level drain run the inner, never this node, so reaching it directly means
/// a defer stood outside any scope — harmless unit.
fn run_defer_noop(_rt: &mut Runtime, _node: DyadPtr) -> Result<i64, RunError> {
    Ok(0)
}

// Test-only log of the value each freed block held, in teardown order — so a
// test can assert LIFO ordering (the `20`-block frees before the `10`-block).
#[cfg(test)]
thread_local! {
    static FREE_LOG: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identities::Core;
    use crate::parse::{Parser, ResolveError, ScopeStack};
    use crate::regex_trie::RegexTrie;

    /// Parse `src` as one top-level scope and run it with the drop model wired
    /// (the runtime knows `defer`, so `scope::run` runs teardowns at exit).
    /// Returns the tail value and the count of still-live heap blocks — zero if
    /// the program frees everything it allocates.
    fn run(src: &str) -> (i64, usize) {
        FREE_LOG.with(|l| l.borrow_mut().clear());
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = core.types();
        let root = {
            let mut p = Parser::new(src, &mut store, &mut trie, types, scopes);
            p.parse_sequence().expect("parse")
        };
        let mut rt = Runtime::new(core.fn_type, core.rational)
            .with_compiler(&core.lower, types)
            .with_defer_type(core.defer_);
        // SAFETY: `root` is the scope just parsed into `store`, which outlives `rt`.
        let bits = unsafe { rt.run(root) }.expect("run");
        (bits, rt.live_allocs())
    }

    fn free_log() -> Vec<i64> {
        FREE_LOG.with(|l| l.borrow().clone())
    }

    /// Parse `src` as one top-level scope, returning the parse error it raises.
    /// For the fail-closed paths, where the point is that the source never runs.
    fn parse_err(src: &str) -> ParseError {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = core.types();
        let mut p = Parser::new(src, &mut store, &mut trie, types, scopes);
        p.parse_sequence().expect_err("expected a parse error")
    }

    #[test]
    fn alloc_reads_back_and_frees_at_scope_exit() {
        // `a := alloc i32 5` allocates, `a@` reads the 5 back; the inserted
        // `defer free a` frees it at scope exit — nothing left live.
        let (v, live) = run("a := alloc i32 5\na@");
        assert_eq!(v, 5);
        assert_eq!(live, 0, "scope exit frees the allocation");
    }

    #[test]
    fn alloc_inside_a_function_frees_when_the_call_returns() {
        let (v, live) = run("main := fn () -> i32 ( p := alloc i32 42  p@ )\nmain()");
        assert_eq!(v, 42);
        assert_eq!(live, 0, "the call's frame scope frees its alloc");
    }

    #[test]
    fn early_drop_does_not_double_free() {
        // `drop a` runs the destructor and empties `a`; the scope's pending
        // `defer free a` then no-ops (the emptied place), so the block frees once.
        let (v, live) = run("a := alloc i32 3\ndrop a\n99");
        assert_eq!(v, 99);
        assert_eq!(live, 0, "drop frees once; the deferred free no-ops");
        assert_eq!(free_log(), vec![3], "exactly one free happened");
    }

    #[test]
    fn own_moves_ownership_and_the_source_scope_frees_nothing() {
        // `b := own a` empties `a` and takes the pointer; `a`'s deferred free
        // no-ops and `b`'s frees. One free, and the block reads 7 through `b`.
        let (v, live) = run("a := alloc i32 7\nb := own a\nb@");
        assert_eq!(v, 7);
        assert_eq!(live, 0, "the moved pointer is freed once, through b");
        assert_eq!(free_log(), vec![7], "own does not double-free the source");
    }

    #[test]
    fn own_out_of_an_inner_block_frees_at_the_outer_owner() {
        // The classic escape: an inner block allocs and `own`s the pointer out;
        // the inner scope frees nothing (its place emptied), the outer binder owns
        // and frees at the outer scope's exit.
        let (v, live) = run("b := ( a := alloc i32 8  own a )\nb@");
        assert_eq!(v, 8);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![8], "freed once, at the outer owner");
    }

    #[test]
    fn teardown_runs_lifo() {
        // Two allocations; teardown reverses construction order, so the second
        // block (holding 20) frees before the first (holding 10).
        let (_v, live) = run("a := alloc i32 10\nb := alloc i32 20\n0");
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![20, 10], "LIFO: last allocated frees first");
    }

    #[test]
    fn a_dropped_name_takes_no_later_free() {
        // `drop a` makes `a` dead at parse (DESIGN ›Memory and concurrency‹,
        // *`own` and `drop` are static*): a later `free a` is a use of a dead
        // name, refused before anything runs — the run-time flag that once made
        // it a no-op is still there for the scope's own `defer free a`.
        assert_eq!(
            parse_err("a := alloc i32 4\ndrop a\nfree a\n1"),
            ParseError::Resolve(ResolveError::Dead)
        );
    }

    #[test]
    fn a_dead_name_may_be_redeclared_after_own() {
        // The two-line shape the ruling keeps: move out, then `:=` the same
        // spelling into a fresh place. The old place's deferred free no-ops on
        // its null, `b` frees the moved block, the new `a` frees its own — LIFO,
        // so the new `a` (9) goes before `b` (7).
        let (v, live) = run("a := alloc i32 7\nb := own a\na := alloc i32 9\na@ + b@");
        assert_eq!(v, 16);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![9, 7], "fresh place, old teardown a no-op");
    }

    #[test]
    fn a_read_after_own_is_refused() {
        assert_eq!(
            parse_err("a := alloc i32 7\nb := own a\na@"),
            ParseError::Resolve(ResolveError::Dead)
        );
    }

    #[test]
    fn a_write_after_own_is_refused() {
        // Refilling a moved-from place with `=` is declined, to stay declined:
        // a dead name takes no writes either, only `:=`.
        assert_eq!(
            parse_err("a := alloc i32 7\nb := own a\na = b"),
            ParseError::Resolve(ResolveError::Dead)
        );
    }

    #[test]
    fn a_pass_after_drop_is_refused_in_the_same_line() {
        // While the line is still parsing the entry's `end` is the `drop` node
        // itself, so the second argument already sees a dead name.
        assert_eq!(
            parse_err("h := fn (x : i32, p : @i32) -> i32 ( x )\na := alloc i32 1\nh(drop a, a)"),
            ParseError::Resolve(ResolveError::Dead)
        );
    }

    #[test]
    fn a_move_inside_a_nested_block_ends_the_outer_name_after_the_block() {
        // *Nested block*: maybe-moved is moved for the name. After the `if`
        // the outer `a` is dead whichever branch ran; the run-time null decides
        // whether its teardown fires. A redeclaration after the block is fine.
        assert_eq!(
            parse_err("a := alloc i32 7\nc := i32 1\nif (c == 1) ( b := own a  b@ )\na@"),
            ParseError::Resolve(ResolveError::Dead)
        );
        let (v, live) =
            run("a := alloc i32 7\nc := i32 1\nif (c == 1) ( b := own a  b@ )\na := alloc i32 9\na@");
        assert_eq!(v, 9);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![7, 9], "b frees at the if's exit, the new a at the end");
    }

    #[test]
    fn a_move_of_an_outer_name_inside_a_loop_body_is_refused() {
        // *Bodies that run again or later*: the next pass would read a dead name.
        assert_eq!(
            parse_err("a := alloc i32 7\nc := i32 1\nwhile (c == 1) ( b := own a  c = 0 )"),
            ParseError::OwnOfOuterName
        );
        assert_eq!(
            parse_err("a := alloc i32 7\nfor i in 0..2 ( b := own a )"),
            ParseError::OwnOfOuterName
        );
        assert_eq!(
            parse_err("a := alloc i32 7\nfor i in 0..2 ( drop a )"),
            ParseError::OwnOfOuterName
        );
    }

    #[test]
    fn drop_frees_the_name_of_a_plain_value() {
        // One verb releases a name whatever its type: `drop n` on an i32 ends
        // `n` at parse and runs to unit, and the spelling is free for `:=`.
        let (v, live) = run("n := i32 5\ndrop n\nn := i32 6\nn");
        assert_eq!(v, 6);
        assert_eq!(live, 0);
        assert_eq!(
            parse_err("n := i32 5\ndrop n\nn + 1"),
            ParseError::Resolve(ResolveError::Dead)
        );
    }

    #[test]
    fn a_dropped_parameter_is_reusable_in_its_body_and_still_compiles() {
        // A parameter is same-level with the body (DESIGN ›A function's
        // surface‹): `drop n` ends it from that line, the redeclaration gets a
        // fresh frame slot, and the inert drop lowers to unit, so the function
        // compiles rather than declining like a heap path would.
        let (v, live) = run("f := fn (n : i32) -> i32 ( drop n  n := i32 4  n )\nf.compile()\nf(1)");
        assert_eq!(v, 4);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_move_of_an_outer_name_inside_a_fn_body_is_refused() {
        // A function may own only what its parameters hand it.
        assert_eq!(
            parse_err("a := alloc i32 7\nf := fn () -> i32 ( b := own a  b@ )"),
            ParseError::OwnOfOuterName
        );
        // A name declared inside the body is inside the barrier: still the
        // ownership-across-return error, not this one.
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( p := alloc i32 7  own p )\nmk()"),
            ParseError::OwnershipAcrossReturn
        );
    }

    #[test]
    fn the_inserted_defer_is_reflectable_graph_structure() {
        // The teardown `alloc`'s binding inserts is an ordinary `defer` node in
        // the scope's body — reachable by structural walk, so `describe` sees it.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = core.types();
        let scope = {
            let mut p = Parser::new("a := alloc i32 5\n0", &mut store, &mut trie, types, scopes);
            p.parse_sequence().expect("parse")
        };
        // SAFETY: `scope` is a sequence node; its body is an array of exprs.
        unsafe {
            let arr = *((*scope).value as *const DyadPtr);
            let exprs = crate::identities::array::items(arr);
            let defer = exprs.iter().find(|&&e| (*e).ty == core.defer_);
            assert!(defer.is_some(), "an inserted defer node is in the scope body");
            let inner = deferred_inner_of(*defer.unwrap());
            assert_eq!((*inner).ty, core.free_, "it defers a free");
            // It describes without panicking — reflectable like any node.
            let _ = crate::reflect::describe(&types, *defer.unwrap());
        }
    }

    #[test]
    fn owning_pointer_carries_a_destructor_but_a_borrow_does_not() {
        // The one identity whose destructor slot is non-null: the owning pointer
        // `alloc` mints. A `&x` borrow of the same shape stays null-destructored,
        // so `drop`/`own` reject it (owning-ness rides the node, not `@T`).
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = core.types();
        let scope = {
            // The tail is a deref, not `a` itself: handing the owning place out
            // as the scope's value is the escape the parser now rejects, so the
            // place is reached through the inserted teardown instead.
            let mut p = Parser::new("a := alloc i32 5\na@", &mut store, &mut trie, types, scopes);
            p.parse_sequence().expect("parse")
        };
        // SAFETY: the scope body holds the inserted `defer free a`, whose place
        // slot is the owning place `a`.
        unsafe {
            let arr = *((*scope).value as *const DyadPtr);
            let exprs = crate::identities::array::items(arr);
            let defer = *exprs
                .iter()
                .find(|&&e| (*e).ty == core.defer_)
                .expect("the binding inserted a defer");
            let a = teardown_place_of(defer);
            assert!(numtype::is_pointer_type((*a).ty), "a is a pointer place");
            assert!(
                !meta::destructor_of((*a).ty).is_null(),
                "an owning pointer's logos carries the destructor"
            );
        }
    }

    #[test]
    fn an_unbound_owning_temporary_is_rejected_not_leaked() {
        // DESIGN attaches the teardown at the binding site, so an owning value
        // handed straight to a call has no name to hang its `free` on. It must
        // fail closed rather than leak; ownership-gated parameters (#53) are what
        // will let a callee declare that it takes the value.
        assert_eq!(
            parse_err("f := fn (p : @i32) -> i32 ( p@ )\nf(alloc i32 5)"),
            ParseError::UnboundOwningValue
        );
    }

    #[test]
    fn ownership_may_not_escape_a_scope_as_its_value() {
        // Without this check the scope's inserted `defer free` runs on the way
        // out and the caller receives an already-freed pointer — a use-after-free,
        // not merely a leak. DESIGN ruled `own` as how ownership leaves a scope.
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( p := alloc i32 7  p )\nmk()"),
            ParseError::OwningEscape
        );
        // `return p` yields the same escape through the return's operand.
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( p := alloc i32 7  return p )\nmk()"),
            ParseError::OwningEscape
        );
    }

    #[test]
    fn ownership_may_not_cross_a_function_return_yet() {
        // A block hands ownership to its binder because the parse sees its tail,
        // but a call hides the body behind the return logos, which cannot yet say
        // "I transfer ownership" — so the caller would not know it owes a `free`
        // and would leak. Fail closed until ownership-gated logos land (#53).
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( p := alloc i32 7  own p )\nmk()"),
            ParseError::OwnershipAcrossReturn
        );
        // The same for handing a bare fresh allocation out of a function.
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( alloc i32 7 )\nmk()"),
            ParseError::OwnershipAcrossReturn
        );
    }

    #[test]
    fn own_hands_ownership_to_an_enclosing_binder() {
        // What `own` *can* do today: escape a block to the binder that encloses
        // it, which the parse sees. The inner place empties (its teardown
        // no-ops), the outer binder owns, and the block is freed exactly once.
        let (v, live) = run("b := ( a := alloc i32 8  own a )\nb@");
        assert_eq!(v, 8);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![8], "freed once, by the outer owner");
    }

    #[test]
    fn a_borrow_may_still_be_handed_out_as_a_scope_value() {
        // The escape check must not catch borrows: only places the scope itself
        // frees are owned. A pointer to an ordinary local carries no destructor,
        // so passing it out stays legal.
        let (v, live) = run("x := i32 9\nr := ( &x )\nr@");
        assert_eq!(v, 9);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_loop_body_frees_every_iteration() {
        // The body is a scope, so its teardown runs at each iteration's exit
        // rather than accumulating: three allocations, three frees, none live.
        let (_v, live) = run("i := i32 0\nwhile (i < 3) ( p := alloc i32 5  i = i + 1 )\ni");
        assert_eq!(live, 0, "no allocation outlives its iteration");
        assert_eq!(free_log().len(), 3, "one free per iteration");
    }

    #[test]
    fn the_compiled_tier_reads_heap_memory_identically() {
        // Interpreter/JIT parity over a drop path: the heap block is allocated
        // and freed by the interpreted tier, while the function reading through
        // the pointer is compiled. Both tiers must see the same 42.
        let (v, live) = run(
            "f := fn (p : @i32) -> i32 ( p@ + 1 )\na := alloc i32 41\nb := f(a)\nf.compile()\nc := f(a)\nb + c",
        );
        assert_eq!(v, 84, "interpreted and compiled reads agree");
        assert_eq!(live, 0);
    }

    #[test]
    fn a_function_with_a_heap_path_declines_to_compile() {
        // The Q4 deopt boundary (interpreter/JIT parity): `alloc`/`free`/`drop`
        // have no lowering, so compiling a body that reaches one fails cleanly —
        // the function stays interpreted, never miscompiled.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = core.types();
        let src = "main := fn () -> i32 ( p := alloc i32 5  p@ )\nmain.compile()";
        let root = {
            let mut p = Parser::new(src, &mut store, &mut trie, types, scopes);
            p.parse_sequence().expect("parse")
        };
        let mut rt = Runtime::new(core.fn_type, core.rational)
            .with_compiler(&core.lower, types)
            .with_defer_type(core.defer_);
        // SAFETY: `root` is the script just parsed into `store`.
        let result = unsafe { rt.run(root) };
        assert!(
            matches!(result, Err(RunError::CompileFailed(_))),
            "compiling a heap function declines (deopt), got {result:?}"
        );
    }
}
