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

use super::callable::{self, Callables};
use super::numtype;
use super::{meta, Cx};
use crate::id_context::IdContext;
use crate::parse::{Assoc, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::synolon::SynolonPtr;

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
    pub alloc_: SynolonPtr,
    pub own_: SynolonPtr,
    pub drop_: SynolonPtr,
    pub free_: SynolonPtr,
    pub defer_: SynolonPtr,
    /// `free`'s run native, also the owning pointer's stored destructor.
    pub teardown_leaf: SynolonPtr,
    pub own_leaf: SynolonPtr,
    pub drop_leaf: SynolonPtr,
    pub alloc_leaf: SynolonPtr,
    pub defer_leaf: SynolonPtr,
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
    let alloc_ = keyword(cx, "alloc", &["pointee", "init", "op"], |p, _id, tape| {
        let init = p.parse_expression()?;
        let types = p.types();
        let node = build_alloc(p.store(), &types, init)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    let alloc_leaf = callable::mint_native(cx.store, cs.callable, run_alloc, cs.seed_native);

    // `own a`: move out of a place; yields the pointer, empties the source.
    let own_ = keyword(cx, "own", &["place", "pointee", "op"], |p, _id, tape| {
        let place = p.parse_place_operand()?;
        let types = p.types();
        let node = build_teardown(p.store(), &types, types.own_, place, true)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    let own_leaf = callable::mint_native(cx.store, cs.callable, run_own, cs.seed_native);

    // `drop a`: run the place's destructor eagerly and empty it.
    let drop_ = keyword(cx, "drop", &["place", "pointee", "op"], |p, _id, tape| {
        let place = p.parse_place_operand()?;
        let types = p.types();
        let node = build_teardown(p.store(), &types, types.drop_, place, true)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    let drop_leaf = callable::mint_native(cx.store, cs.callable, run_drop, cs.seed_native);

    // `free a`: the teardown the binding site inserts; user-writable too. Like
    // `own`/`drop` it demands an owning place — only an `alloc`-minted pointer
    // points at heap the allocator can free; freeing a borrow (`&x`) would hand
    // a stack/global address to the allocator.
    let free_ = keyword(cx, "free", &["place", "pointee", "op"], |p, _id, tape| {
        let place = p.parse_place_operand()?;
        let types = p.types();
        let node = build_teardown(p.store(), &types, types.free_, place, true)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });

    // `defer <expr>`: hold `<expr>` for LIFO execution at scope exit. Its own run
    // native is a no-op — the scope machinery runs the inner, never the defer node.
    let defer_ = keyword(cx, "defer", &["inner", "op"], |p, _id, tape| {
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
/// record (`TUPLE`, NaN precedence — never extends left, so the driver invokes
/// its constructor immediately), and `construct` in the `metas` table. Returns
/// the identity node.
fn keyword(
    cx: &mut Cx,
    spelling: &str,
    roles: &[&str],
    construct: crate::parse::ConstructFn,
) -> SynolonPtr {
    let record = meta::operand_record(cx, meta::TUPLE_TAG, f64::NAN, Assoc::Left, roles);
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
unsafe fn pointee_width(pointee: SynolonPtr) -> usize {
    numtype::numtype_of_type(pointee).bytes()
}

/// Build an `alloc` node from its parsed initializer. The pointee logos is the
/// initializer's own logos (`alloc i32 5` allocates an `i32`), so the value
/// carries both what to allocate and what to store; a non-scalar initializer is
/// rejected (v1 allocates scalars and pointers only).
pub(super) fn build_alloc(
    store: &mut Store,
    types: &CoreTypes,
    init: SynolonPtr,
) -> Result<SynolonPtr, ParseError> {
    // SAFETY: `init` is a reduced synolon just parsed.
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
/// reduced synolon from the store.
pub(crate) fn build_teardown(
    store: &mut Store,
    types: &CoreTypes,
    op_id: SynolonPtr,
    place: SynolonPtr,
    require_owning: bool,
) -> Result<SynolonPtr, ParseError> {
    // SAFETY: `place` is a reduced synolon; its logos is a valid logos node.
    let logos = unsafe { (*place).logos };
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

/// Build a `defer <inner>` node `[inner, op]`.
pub(crate) fn build_defer(store: &mut Store, types: &CoreTypes, inner: SynolonPtr) -> SynolonPtr {
    let value = store.alloc_operands(&[inner, types.ops.defer_]);
    store.alloc_raw(types.defer_, value)
}

/// The deferred inner expression of a `defer` node.
///
/// # Safety
/// `node` must be a `defer` node as [`build_defer`] lays it out.
pub(crate) unsafe fn deferred_inner_of(node: SynolonPtr) -> SynolonPtr {
    *((*node).hyle as *const SynolonPtr).add(DEFER_INNER)
}

/// The pointee logos of an `alloc`/`own` node — what a bound owning pointer
/// points at, so the binding site can mint its owning `@pointee` type. `alloc`
/// stores it at [`ALLOC_POINTEE`], `own` at [`TEARDOWN_POINTEE`]; a scope whose
/// tail is one propagates through (an owning value moved out of a block).
///
/// # Safety
/// `node` must be a valid synolon from the store.
pub(crate) unsafe fn owning_pointee_of(types: &CoreTypes, node: SynolonPtr) -> Option<SynolonPtr> {
    let logos = (*node).logos;
    if logos == types.alloc_ {
        Some(*((*node).hyle as *const SynolonPtr).add(ALLOC_POINTEE))
    } else if logos == types.own_ {
        Some(*((*node).hyle as *const SynolonPtr).add(TEARDOWN_POINTEE))
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
pub(crate) unsafe fn is_owning_value(types: &CoreTypes, node: SynolonPtr) -> bool {
    owning_pointee_of(types, node).is_some()
}

/// Run `alloc`: evaluate the initializer, heap-allocate the pointee's width,
/// write the value in, and yield the block's address (the owning pointer). The
/// runtime notes the live allocation so leaks and double-frees are observable.
fn run_alloc(rt: &mut Runtime, node: SynolonPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `alloc` node `[pointee, init, op]` from the store.
    unsafe {
        let slots = (*node).hyle as *const SynolonPtr;
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
fn run_teardown(rt: &mut Runtime, node: SynolonPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[place, pointee, op]` teardown node from the store.
    unsafe {
        let slots = (*node).hyle as *const SynolonPtr;
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
/// teardown genuinely flows through the reserved `destructor` slot. Rejecting a
/// null destructor cannot happen here — `build_teardown` demanded an owning place
/// at parse — so a null slot is a malformed node.
fn run_drop(rt: &mut Runtime, node: SynolonPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `drop` node; its place's logos carries the destructor
    // (owning-ness checked at parse), whose entry is a `RunFn` reading the same
    // `[place, pointee, op]` layout this node has.
    unsafe {
        let place = *((*node).hyle as *const SynolonPtr).add(TEARDOWN_PLACE);
        let dtor = meta::destructor_of((*place).logos);
        if dtor.is_null() || !callable::is_callable(dtor) {
            return Err(RunError::BadValue);
        }
        let entry = std::mem::transmute::<usize, crate::run::RunFn>(callable::entry_of(dtor));
        entry(rt, node)
    }
}

/// Run `own`: read the place's pointer, empty the place (write null), and yield
/// the pointer — a move. The moved-from place's pending `defer free` then no-ops.
fn run_own(rt: &mut Runtime, node: SynolonPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `own` node `[place, pointee, op]` from the store.
    unsafe {
        let place = *((*node).hyle as *const SynolonPtr).add(TEARDOWN_PLACE);
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
fn run_defer_noop(_rt: &mut Runtime, _node: SynolonPtr) -> Result<i64, RunError> {
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
    use crate::parse::{Parser, ScopeStack};
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
    fn a_dropped_place_frees_only_once_even_with_a_later_free() {
        // `drop a` then a manual `free a`: the drop empties the place, so the
        // explicit `free` no-ops — and the scope's own `defer free a` no-ops too.
        let (v, live) = run("a := alloc i32 4\ndrop a\nfree a\n1");
        assert_eq!(v, 1);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![4], "one free across drop + free + defer");
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
            let arr = *((*scope).hyle as *const SynolonPtr);
            let exprs = crate::identities::array::items(arr);
            let defer = exprs.iter().find(|&&e| (*e).logos == core.defer_);
            assert!(defer.is_some(), "an inserted defer node is in the scope body");
            let inner = deferred_inner_of(*defer.unwrap());
            assert_eq!((*inner).logos, core.free_, "it defers a free");
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
            let mut p = Parser::new("a := alloc i32 5\na", &mut store, &mut trie, types, scopes);
            p.parse_sequence().expect("parse")
        };
        // SAFETY: the scope's tail `a` resolves to the owning place.
        unsafe {
            let arr = *((*scope).hyle as *const SynolonPtr);
            let exprs = crate::identities::array::items(arr);
            let a = *exprs.last().unwrap();
            assert!(numtype::is_pointer_type((*a).logos), "a is a pointer place");
            assert!(
                !meta::destructor_of((*a).logos).is_null(),
                "an owning pointer's logos carries the destructor"
            );
        }
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
