// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The drop model: `alloc`, `own`, `drop`, `free`, and `defer`, one mechanism in one
//! file. `alloc T v` yields an owning `@T` (a pointer type with a non-null destructor);
//! binding it inserts `defer free <place>`, and `own`/`drop` empty the place to null so
//! a pending teardown no-ops. None of the five lower. DESIGN ›Explicit heap, and no implicit
//! destruction‹.

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype;
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// `alloc` is `[pointee, init, op]`.
const ALLOC_POINTEE: usize = 0;
const ALLOC_INIT: usize = 1;
/// `free`/`drop`/`own` are `[place, pointee, op]`.
const TEARDOWN_PLACE: usize = 0;
const TEARDOWN_POINTEE: usize = 1;
/// `defer` is `[inner, op]`.
const DEFER_INNER: usize = 0;

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

pub(super) fn register(cx: &mut Cx, cs: &Callables) -> DropModel {
    // Minted once: `free`'s op leaf and the owning pointer's destructor slot both point
    // at it, so `drop` reaches the same code as an inserted `free`.
    let teardown_leaf = callable::mint_native(cx.store, cs.callable, run_teardown, cs.seed_native);

    let alloc_ =
        keyword(cx, "alloc", meta::prec::PREFIX, &["pointee", "init", "op"], |p, _id, tape| {
            let init = p.take_right(tape)?;
            let types = p.types();
            let node = build_alloc(p.store(), types, init)?;
            tape.place(node);
            Ok(crate::parse::Constructed::Placed)
        });
    let alloc_leaf = callable::mint_native(cx.store, cs.callable, run_alloc, cs.seed_native);

    let own_ =
        keyword(cx, "own", meta::prec::PREFIX, &["place", "pointee", "op"], |p, _id, tape| {
            let (place, ended) = p.place_operand_cell(tape, true)?;
            let types = p.types();
            let node = build_teardown(p.store(), types, types.own_, place, true)?;
            tape.place(node);
            if let Some(ended) = ended {
                p.mark_dead(ended, node);
            }
            Ok(crate::parse::Constructed::Placed)
        });
    let own_leaf = callable::mint_native(cx.store, cs.callable, run_own, cs.seed_native);

    // Any identity may be dropped: an owning place gets the teardown node, anything else
    // an inert `drop` node whose work is the parse-time dead mark. `drop = …` never
    // reaches here: `=` constructs first and takes the lone `drop` as the slot's name.
    let drop_ =
        keyword(cx, "drop", meta::prec::PREFIX, &["place", "pointee", "op"], |p, _id, tape| {
            let (place, ended) = p.place_operand_cell(tape, true)?;
            let types = p.types();
            let node = if is_owning_place(place) {
                build_teardown(p.store(), types, types.drop_, place, true)?
            } else {
                build_inert_drop(p.store(), types, place)
            };
            tape.place(node);
            if let Some(ended) = ended {
                p.mark_dead(ended, node);
            }
            Ok(crate::parse::Constructed::Placed)
        });
    cx.lower.insert(drop_, lower_drop);
    let drop_leaf = callable::mint_native(cx.store, cs.callable, run_drop, cs.seed_native);

    // `free` demands an owning place too: freeing a borrow would hand a stack or global
    // address to the allocator.
    let free_ =
        keyword(cx, "free", meta::prec::PREFIX, &["place", "pointee", "op"], |p, _id, tape| {
            // The raw teardown verb leaves the name alive: only `own`/`drop` end it.
            let (place, _) = p.place_operand_cell(tape, false)?;
            let types = p.types();
            let node = build_teardown(p.store(), types, types.free_, place, true)?;
            tape.place(node);
            Ok(crate::parse::Constructed::Placed)
        });

    // Its run native is a no-op: the scope machinery runs the inner, never the defer node.
    let defer_ = keyword(cx, "defer", meta::prec::READER, &["inner", "op"], |p, _id, tape| {
        let inner = p.parse_expression()?;
        let types = p.types();
        let node = build_defer(p.store(), types, inner);
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

fn keyword(
    cx: &mut Cx,
    spelling: &str,
    parse_rank: f64,
    roles: &[&str],
    construct: crate::parse::ConstructFn,
) -> DyadPtr {
    let record = meta::operand_record(cx, meta::TUPLE_TAG, parse_rank, Assoc::Right, roles);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare(spelling, id);
    cx.metas.insert(id, construct);
    id
}

/// A scalar's `Layout` is `(width, width)`: every scalar width is a power of two, so it
/// is a valid alignment.
///
/// # Safety
/// `width` must be a non-zero scalar width; the block is freed exactly once by
/// `heap_free` with the same `width`.
unsafe fn heap_alloc(width: usize) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(width, width)
        .expect("a scalar width is a valid power-of-two layout");
    std::alloc::alloc(layout)
}

/// # Safety
/// `ptr` must be a live block from `heap_alloc` with the same `width`.
unsafe fn heap_free(ptr: *mut u8, width: usize) {
    let layout = std::alloc::Layout::from_size_align(width, width)
        .expect("a scalar width is a valid power-of-two layout");
    std::alloc::dealloc(ptr, layout);
}

/// # Safety
/// `pointee` must be a scalar or pointer type node.
unsafe fn pointee_width(pointee: DyadPtr) -> usize {
    numtype::of_type_node(pointee).bytes()
}

/// The pointee type is the initializer's own type (`alloc i32 5` allocates an `i32`);
/// a non-scalar initializer is rejected.
pub(super) fn build_alloc(
    store: &mut Store,
    types: &Core,
    init: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `init` is a reduced dyad just parsed.
    let operand = unsafe { crate::identities::numtype_of(types, init) };
    let pointee = match operand {
        crate::identities::Operand::Concrete(_) | crate::identities::Operand::Pointer(_) => {
            // SAFETY: as above; the operand is scalar or pointer, what `scalar_binding_type` takes.
            unsafe { crate::identities::scalar_binding_type(store, types, init).0 }
        }
        _ => return Err(ParseError::UnsupportedOperands),
    };
    let value = store.alloc_operands(&[pointee, init, types.ops.alloc_]);
    Ok(store.alloc_raw(types.alloc_, value))
}

/// When `require_owning`, the place must carry a non-null destructor: a borrow or a
/// plain value cannot be moved or dropped. `place` must be a reduced dyad from the store.
pub(crate) fn build_teardown(
    store: &mut Store,
    types: &Core,
    op_id: DyadPtr,
    place: DyadPtr,
    require_owning: bool,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `place` is a reduced dyad; its type is a valid type node.
    let logos = unsafe { (*place).ty };
    // SAFETY: `logos` is a type node from the store (above).
    if unsafe { !numtype::is_pointer_type(logos) } {
        return Err(ParseError::BadAssignTarget);
    }
    // SAFETY: a pointer type carries a record, which the destructor slot is in.
    if require_owning && unsafe { meta::destructor_of(logos).is_null() } {
        return Err(ParseError::BadAssignTarget);
    }
    // SAFETY: as above: a pointer type's record holds its pointee.
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

/// A pointer type whose `destructor` slot is set, as opposed to a borrow or a plain
/// value. `place` must be a reduced dyad from the store.
pub(crate) fn is_owning_place(place: DyadPtr) -> bool {
    // SAFETY: `place` is a reduced dyad; its type is a valid type node.
    unsafe {
        let logos = (*place).ty;
        numtype::is_pointer_type(logos) && !meta::destructor_of(logos).is_null()
    }
}

/// `[place, null, op]`: the null pointee marks nothing to run or free. Its work was done
/// at parse, where the name became dead; it stands in the body for reflection.
fn build_inert_drop(store: &mut Store, types: &Core, place: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[place, std::ptr::null_mut(), types.ops.drop_]);
    store.alloc_raw(types.drop_, value)
}

/// The inert form is unit; the owning form has no lowering, so the function declines to
/// compile and stays interpreted.
fn lower_drop(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a `drop` node `[place, pointee, op]` from the store.
    let pointee = unsafe { *((*node).value as *const DyadPtr).add(TEARDOWN_POINTEE) };
    if pointee.is_null() {
        Ok(lw.const_i32(0))
    } else {
        Err(CompileError::NotLowerable(node))
    }
}

pub(crate) fn build_defer(store: &mut Store, types: &Core, inner: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[inner, types.ops.defer_]);
    store.alloc_raw(types.defer_, value)
}

/// # Safety
/// `node` must be a `defer` node from `build_defer`.
pub(crate) unsafe fn deferred_inner_of(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr).add(DEFER_INNER)
}

/// The place an inserted `defer free <place>` frees; the escape check compares a scope's
/// tail against these.
///
/// # Safety
/// `defer_node` must be a `defer` node over a teardown, as the binding site builds.
pub(crate) unsafe fn teardown_place_of(defer_node: DyadPtr) -> DyadPtr {
    let inner = deferred_inner_of(defer_node);
    *((*inner).value as *const DyadPtr).add(TEARDOWN_PLACE)
}

/// What a bound owning pointer points at, so the binding site can mint its owning
/// `@pointee` type; a scope whose tail is one propagates through.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn owning_pointee_of(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    let node = types.through(node);
    let logos = (*node).ty;
    if logos == types.alloc_ {
        Some(*((*node).value as *const DyadPtr).add(ALLOC_POINTEE))
    } else if logos == types.own_ {
        Some(*((*node).value as *const DyadPtr).add(TEARDOWN_POINTEE))
    } else if logos == types.scope {
        // A block that yields an owning value moves ownership to the binder.
        crate::parse::last_sequence_expr(node).and_then(|tail| owning_pointee_of(types, tail))
    } else if logos == types.ran_ {
        // An item that ran in the pass owns what its expression owns.
        owning_pointee_of(types, super::ran::expr_of(types, node))
    } else {
        None
    }
}

/// The binding-site test: whether `a := <node>` mints an owning place and inserts `defer free a`.
///
/// # Safety
/// As `owning_pointee_of`.
pub(crate) unsafe fn is_owning_value(types: &Core, node: DyadPtr) -> bool {
    owning_pointee_of(types, node).is_some()
}

/// The runtime notes the live allocation so leaks and double frees are observable.
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
            return Err(RunError::OutOfMemory);
        }
        numtype::write_scalar(pointee, mem, bits);
        rt.note_alloc();
        Ok(mem as i64)
    }
}

/// A null pointer (an emptied place) is the sanctioned no-op; after freeing, the place is
/// nulled so a second teardown over it also no-ops.
fn run_teardown(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[place, pointee, op]` teardown node from the store.
    unsafe {
        let slots = (*node).value as *const DyadPtr;
        let place = *slots.add(TEARDOWN_PLACE);
        let pointee = *slots.add(TEARDOWN_POINTEE);
        let slot = rt.place_addr(place).ok_or(RunError::NoActivation)?;
        if slot.is_null() {
            return Err(RunError::Uninitialized);
        }
        let ptr = std::ptr::read_unaligned(slot as *const i64) as u64 as *mut u8;
        if ptr.is_null() {
            return Ok(0); // emptied place: the sanctioned no-op
        }
        // Tests observe teardown order (LIFO) by the value each freed block held.
        #[cfg(test)]
        FREE_LOG.with(|log| log.borrow_mut().push(numtype::read_scalar(pointee, ptr)));
        heap_free(ptr, pointee_width(pointee));
        rt.note_free();
        std::ptr::write_unaligned(slot as *mut i64, 0);
        Ok(0)
    }
}

/// The teardown flows through the reserved `destructor` slot. A null destructor here is a
/// malformed node: `build_teardown` demanded an owning place at parse.
fn run_drop(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `drop` node; in the owning form its place's type carries a
    // destructor whose entry is a `RunFn` over this node's layout.
    unsafe {
        let slots = (*node).value as *const DyadPtr;
        if (*slots.add(TEARDOWN_POINTEE)).is_null() {
            return Ok(0);
        }
        let place = *slots.add(TEARDOWN_PLACE);
        let dtor = meta::destructor_of((*place).ty);
        if dtor.is_null() || !callable::is_callable(dtor) {
            return Err(RunError::NoDestructor(place));
        }
        let entry = std::mem::transmute::<usize, crate::run::RunFn>(callable::entry_of(dtor));
        entry(rt, node)
    }
}

/// A move: the moved-from place's pending `defer free` then no-ops.
fn run_own(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `own` node `[place, pointee, op]` from the store.
    unsafe {
        let place = *((*node).value as *const DyadPtr).add(TEARDOWN_PLACE);
        let slot = rt.place_addr(place).ok_or(RunError::NoActivation)?;
        if slot.is_null() {
            return Err(RunError::Uninitialized);
        }
        let ptr = std::ptr::read_unaligned(slot as *const i64);
        std::ptr::write_unaligned(slot as *mut i64, 0);
        Ok(ptr)
    }
}

/// The scope machinery and the top-level drain run the inner, never this node; reaching
/// it directly means a defer stood outside any scope.
fn run_defer_noop(_rt: &mut Runtime, _node: DyadPtr) -> Result<i64, RunError> {
    Ok(0)
}

// Test-only log of the value each freed block held, in teardown order.
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

    /// Parse and run `src`; returns the tail value and the count of still-live heap blocks.
    fn run(src: &str) -> (i64, usize) {
        FREE_LOG.with(|l| l.borrow_mut().clear());
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = &core;
        let root = {
            let mut p =
                Parser::new(src, &mut store, &mut trie, types, scopes).with_lower(&core.lower);
            p.parse_sequence().expect("parse")
        };
        let mut rt = Runtime::new(&core, &mut store).with_compiler(&core.lower);
        // SAFETY: `root` is the scope just parsed into `store`, which outlives `rt`.
        let bits = unsafe { rt.run(root) }.expect("run");
        (bits, rt.live_allocs())
    }

    #[test]
    fn an_owning_place_takes_only_an_owning_value() {
        assert_eq!(
            parse_err("x := i32 1, a := alloc i32 5, a = &x"),
            ParseError::NonOwningIntoOwning
        );
        assert_eq!(
            parse_err("a := alloc i32 5, b := alloc i32 6, a = b"),
            ParseError::NonOwningIntoOwning
        );
        assert_eq!(run("x := i32 1, p := &x, p = &x, p@").0, 1);
        assert_eq!(run("a := alloc i32 5, a@ = 9, a@"), (9, 0));

        // The displaced block is not freed yet; the leak is pinned as a number.
        assert_eq!(run("a := alloc i32 5, a = alloc i32 6, a@"), (6, 1));
        assert_eq!(run("a := alloc i32 5, b := alloc i32 6, a = own b, a@"), (6, 1));
    }

    fn free_log() -> Vec<i64> {
        FREE_LOG.with(|l| l.borrow().clone())
    }

    /// The parse error `src` raises; for the fail-closed paths.
    fn parse_err(src: &str) -> ParseError {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = &core;
        let mut p = Parser::new(src, &mut store, &mut trie, types, scopes);
        p.parse_sequence().expect_err("expected a parse error")
    }

    #[test]
    fn alloc_reads_back_and_frees_at_scope_exit() {
        let (v, live) = run("a := alloc i32 5,\na@");
        assert_eq!(v, 5);
        assert_eq!(live, 0, "scope exit frees the allocation");
    }

    #[test]
    fn alloc_inside_a_function_frees_when_the_call_returns() {
        let (v, live) = run("main := fn () -> i32 ( p := alloc i32 42, p@ ),\nmain()");
        assert_eq!(v, 42);
        assert_eq!(live, 0, "the call's frame scope frees its alloc");
    }

    #[test]
    fn early_drop_does_not_double_free() {
        let (v, live) = run("a := alloc i32 3,\ndrop a,\n99");
        assert_eq!(v, 99);
        assert_eq!(live, 0, "drop frees once; the deferred free no-ops");
        assert_eq!(free_log(), vec![3], "exactly one free happened");
    }

    #[test]
    fn own_moves_ownership_and_the_source_scope_frees_nothing() {
        let (v, live) = run("a := alloc i32 7,\nb := own a,\nb@");
        assert_eq!(v, 7);
        assert_eq!(live, 0, "the moved pointer is freed once, through b");
        assert_eq!(free_log(), vec![7], "own does not double-free the source");
    }

    #[test]
    fn own_out_of_an_inner_block_frees_at_the_outer_owner() {
        let (v, live) = run("b := ( a := alloc i32 8, own a ),\nb@");
        assert_eq!(v, 8);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![8], "freed once, at the outer owner");
    }

    #[test]
    fn teardown_runs_lifo() {
        let (_v, live) = run("a := alloc i32 10,\nb := alloc i32 20,\n0");
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![20, 10], "LIFO := last ? allocated frees first");
    }

    #[test]
    fn a_dropped_name_takes_no_later_free() {
        // A later `free a` is a use of a dead name, refused before anything runs.
        assert_eq!(
            parse_err("a := alloc i32 4,\ndrop a,\nfree a,\n1"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_dead_name_may_be_redeclared_after_own() {
        // LIFO: the new `a` (9) frees before `b` (7).
        let (v, live) = run("a := alloc i32 7,\nb := own a,\na := alloc i32 9,\na@ + b@");
        assert_eq!(v, 16);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![9, 7], "fresh place, old teardown a no-op");
    }

    #[test]
    fn a_read_after_own_is_refused() {
        assert_eq!(
            parse_err("a := alloc i32 7,\nb := own a,\na@"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_write_after_own_is_refused() {
        assert_eq!(
            parse_err("a := alloc i32 7,\nb := own a,\na = b"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_pass_after_drop_is_refused_in_the_same_line() {
        // While the line is still parsing the entry's `end` is the `drop` node itself.
        assert_eq!(
            parse_err(
                "h := fn (x := i32 ?, p := @i32 ?) -> i32 ( x ),\na := alloc i32 1,\nh(drop a, a)"
            ),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
    }

    #[test]
    fn a_move_inside_a_nested_block_ends_the_outer_name_after_the_block() {
        // Maybe-moved is moved for the name; the run-time null decides whether its teardown fires.
        assert_eq!(
            parse_err("a := alloc i32 7,\nc := i32 1,\nif (c == 1) ( b := own a, b@ ),\na@"),
            ParseError::Resolve(ResolveError::Dead("a".into()))
        );
        let (v, live) =
            run("a := alloc i32 7,\nc := i32 1,\nif (c == 1) ( b := own a, b@ ),\na := alloc i32 9,\na@");
        assert_eq!(v, 9);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![7, 9], "b frees at the if's exit, the new a at the end");
    }

    #[test]
    fn a_move_of_an_outer_name_inside_a_loop_body_is_refused() {
        // The next pass would read a dead name.
        assert_eq!(
            parse_err("a := alloc i32 7,\nc := i32 1,\nwhile (c == 1) ( b := own a, c = 0 )"),
            ParseError::OwnOfOuterName
        );
        assert_eq!(
            parse_err("a := alloc i32 7,\nfor i in 0..2 ( b := own a )"),
            ParseError::OwnOfOuterName
        );
        assert_eq!(
            parse_err("a := alloc i32 7,\nfor i in 0..2 ( drop a )"),
            ParseError::OwnOfOuterName
        );
    }

    #[test]
    fn drop_frees_the_name_of_a_plain_value() {
        let (v, live) = run("n := i32 5,\ndrop n,\nn := i32 6,\nn");
        assert_eq!(v, 6);
        assert_eq!(live, 0);
        assert_eq!(
            parse_err("n := i32 5,\ndrop n,\nn + 1"),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
    }

    #[test]
    fn a_dropped_parameter_is_reusable_in_its_body_and_still_compiles() {
        // A parameter is same-level with the body; the inert drop lowers to unit, so the
        // function compiles.
        let (v, live) =
            run("f := fn (n := i32 ?) -> i32 ( drop n, n := i32 4, n ),\nf.compile(),\nf(1)");
        assert_eq!(v, 4);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_move_of_an_outer_name_inside_a_fn_body_is_refused() {
        // A function may own only what its parameters hand it.
        assert_eq!(
            parse_err("a := alloc i32 7,\nf := fn () -> i32 ( b := own a, b@ )"),
            ParseError::OwnOfOuterName
        );
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( p := alloc i32 7, own p ),\nmk()"),
            ParseError::OwnershipAcrossReturn
        );
    }

    #[test]
    fn the_inserted_defer_is_reflectable_graph_structure() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = &core;
        let scope = {
            let mut p = Parser::new("a := alloc i32 5,\n0", &mut store, &mut trie, types, scopes);
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
            let _ = crate::reflect::describe(types, *defer.unwrap());
        }
    }

    #[test]
    fn owning_pointer_carries_a_destructor_but_a_borrow_does_not() {
        // Owning-ness rides the node `alloc` mints, not `@T`.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = &core;
        let scope = {
            let mut p = Parser::new("a := alloc i32 5,\na@", &mut store, &mut trie, types, scopes);
            p.parse_sequence().expect("parse")
        };
        // SAFETY: the scope body holds the inserted `defer free a`, whose place slot is `a`.
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
        // An owning value handed straight to a call has no name to hang its `free` on.
        assert_eq!(
            parse_err("f := fn (p := @i32 ?) -> i32 ( p@ ),\nf(alloc i32 5)"),
            ParseError::UnboundOwningValue
        );
    }

    #[test]
    fn ownership_may_not_escape_a_scope_as_its_value() {
        // The scope's `defer free` would run on the way out and hand back a freed pointer.
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( p := alloc i32 7, p ),\nmk()"),
            ParseError::OwningEscape
        );
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( p := alloc i32 7, return p ),\nmk()"),
            ParseError::OwningEscape
        );
    }

    #[test]
    fn ownership_may_not_cross_a_function_return_yet() {
        // A return type cannot yet say it transfers ownership, so the caller would not know it
        // owes a `free`.
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( p := alloc i32 7, own p ),\nmk()"),
            ParseError::OwnershipAcrossReturn
        );
        assert_eq!(
            parse_err("mk := fn () -> @i32 ( alloc i32 7 ),\nmk()"),
            ParseError::OwnershipAcrossReturn
        );
    }

    #[test]
    fn a_call_is_a_use_of_the_outer_names_the_body_reads() {
        // Calling a function is a use of each outer name its body reads, at the call.
        assert_eq!(
            parse_err(
                "n := i32 0,\nclimb := fn () -> i32 ( n = n + 1, n ),\nclimb(),\ndrop n,\nclimb()"
            ),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        assert_eq!(
            parse_err("n := i32 0,\nclimb := fn () -> i32 ( n ),\ndrop n,\nn := i32 10,\nclimb()"),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        let (v, live) =
            run("n := i32 0,\nclimb := fn () -> i32 ( n = n + 1, n ),\nclimb(),\ndrop n,\n\
             n := i32 10,\nclimb2 := fn () -> i32 ( n = n + 1, n ),\nclimb2()");
        assert_eq!(v, 11);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_call_reading_a_dropped_owning_pointer_is_refused() {
        // A dropped name's place may hold anything, here a freed block.
        assert_eq!(
            parse_err("p := alloc i32 5,\nf := fn () -> i32 ( p@ ),\ndrop p,\nf()"),
            ParseError::Resolve(ResolveError::Dead("p".into()))
        );
        let (v, live) = run(
            "p := alloc i32 5,\nf := fn (q := @i32 ?) -> i32 ( q@ ),\nr := alloc i32 4,\ndrop p,\nf(r)",
        );
        assert_eq!(v, 4);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_call_is_a_use_of_what_its_callees_read() {
        // A callee's outer names join the caller's list.
        assert_eq!(
            parse_err(
                "n := i32 0,\nclimb := fn () -> i32 ( n ),\ng := fn () -> i32 ( climb() ),\n\
                 drop n,\ng()"
            ),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        assert_eq!(
            parse_err(
                "n := i32 1,\nouter := fn () -> i32 ( inner := fn () -> i32 ( n ), inner() ),\n\
                 drop n,\nouter()"
            ),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
    }

    #[test]
    fn a_node_of_a_run_type_is_a_use_of_every_name_its_body_reads() {
        const POW: &str = "n := i32 2,\n\
            ^ := type (\n\
                instance = ( a := i32 ?, b := i32 ?, output := type ?, shared run = ( this.a * this.b * n ) ),\n\
                parse_rank = *.parse_rank + 1,\n\
                associativity = right,\n\
                parse = (\n\
                    this.a = tape[-1],\n\
                    this.b = tape[1],\n\
                    this.output = i32,\n\
                    tape[0] = this,\n\
                    tape.is_constructed[0] = true,\n\
                    tape.remove(1),\n\
                    tape.remove(-1)\n\
                )\n\
            ),\n";
        let (v, live) = run(&format!("{POW}2 ^ 3"));
        assert_eq!(v, 12);
        assert_eq!(live, 0);
        // Whether the body is constructed after the drop or was built before it.
        assert_eq!(
            parse_err(&format!("{POW}drop n,\n2 ^ 3")),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
        assert_eq!(
            parse_err(&format!("{POW}2 ^ 3,\ndrop n,\n2 ^ 3")),
            ParseError::Resolve(ResolveError::Dead("n".into()))
        );
    }

    #[test]
    fn own_hands_ownership_to_an_enclosing_binder() {
        let (v, live) = run("b := ( a := alloc i32 8, own a ),\nb@");
        assert_eq!(v, 8);
        assert_eq!(live, 0);
        assert_eq!(free_log(), vec![8], "freed once, by the outer owner");
    }

    #[test]
    fn a_borrow_may_still_be_handed_out_as_a_scope_value() {
        // Only places the scope itself frees are owned; a borrow carries no destructor.
        let (v, live) = run("x := i32 9,\nr := ( &x ),\nr@");
        assert_eq!(v, 9);
        assert_eq!(live, 0);
    }

    #[test]
    fn a_loop_body_frees_every_iteration() {
        let (_v, live) = run("i := i32 0,\nwhile (i < 3) ( p := alloc i32 5, i = i + 1 ),\ni");
        assert_eq!(live, 0, "no allocation outlives its iteration");
        assert_eq!(free_log().len(), 3, "one free per iteration");
    }

    #[test]
    fn the_compiled_tier_reads_heap_memory_identically() {
        let (v, live) = run(
            "f := fn (p := @i32 ?) -> i32 ( p@ + 1 ),\na := alloc i32 41,\nb := f(a),\nf.compile(),\nc := f(a),\nb + c",
        );
        assert_eq!(v, 84, "interpreted and compiled reads agree");
        assert_eq!(live, 0);
    }

    #[test]
    fn a_function_with_a_heap_path_declines_to_compile() {
        // Heap paths have no lowering, so the function stays interpreted, never miscompiled.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let types = &core;
        let src = "main := fn () -> i32 ( p := alloc i32 5, p@ ),\nmain.compile()";
        let root = {
            let mut p =
                Parser::new(src, &mut store, &mut trie, types, scopes).with_lower(&core.lower);
            p.parse_sequence().expect("parse")
        };
        let mut rt = Runtime::new(&core, &mut store).with_compiler(&core.lower);
        // SAFETY: `root` is the script just parsed into `store`.
        let result = unsafe { rt.run(root) };
        assert!(
            matches!(result, Err(RunError::CompileFailed(_))),
            "compiling a heap function declines (deopt), got {result:?}"
        );
    }
}
