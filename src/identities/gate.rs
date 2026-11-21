// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `pub`, `mut` and `static`, the gate words, and where a gate lives: in the type of the
//! value it gates. Each set of gate words is one gate mint, a type, and the gates over `X`
//! are one gate node, `[gate mint | X]`, where `X` is never a gate node itself. `type_mint`
//! keeps the gate mints and the gate nodes over a type, the gated types. A name's gate node
//! stands where its binding points: over the type of its bytes for a name laid out in a frame,
//! over the node it denotes for any other, and in the declared type a field's `?` node holds.
//! No gate node is no gate: the name is read from its own scope and below, and written
//! nowhere after its declaration.
//! ›Gates are type nodes: `x := mut i32 5`; the record holds no gates‹ [[R381]]

use super::{hole, meta, type_mint, Cx};
use crate::binding::Binding;
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError};
use crate::store::Store;
use crate::Core;

/// Returns `pub`, `mut`, `static`, and the former every gate mint is kept under, the type of
/// the gate mints.
pub(super) fn register(cx: &mut Cx) -> (DyadPtr, DyadPtr, DyadPtr, DyadPtr) {
    let pub_ = word(cx, "pub");
    cx.metas.insert(pub_, construct);
    let mut_ = word(cx, "mut");
    cx.metas.insert(mut_, construct);
    let static_ = word(cx, "static");
    cx.metas.insert(static_, outside_block);
    let former = meta::record(cx.store, meta::TYPEREC_TAG, meta::prec::INERT);
    let gate_ = cx.store.alloc_head(cx.type_, former);
    (pub_, mut_, static_, gate_)
}

/// No node is ever typed by a gate word; its identity stands in a gate mint's record.
pub(super) fn word(cx: &mut Cx, name: &str) -> DyadPtr {
    let record = meta::token_record(cx, meta::prec::PREFIX, crate::parse::Assoc::Right, &[]);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare(name, id);
    id
}

/// Whether `p` is a gate node, a node typed by a gate mint.
///
/// # Safety
/// `p` must be null or a dyad from the store.
pub(crate) unsafe fn is_node(p: DyadPtr) -> bool {
    if p.is_null() {
        return false;
    }
    let mint = dyad::ty(p);
    !mint.is_null() && meta::kind_of(mint) == Some(meta::GATE_TAG)
}

/// What the gates of `p` stand over: `p` itself where it is no gate node.
///
/// # Safety
/// As [`is_node`].
pub(crate) unsafe fn under(p: DyadPtr) -> DyadPtr {
    if is_node(p) {
        dyad::head(p) as DyadPtr
    } else {
        p
    }
}

/// Whether `gate` is among the gates of `p`; none where `p` is no gate node.
///
/// # Safety
/// As [`is_node`].
pub(crate) unsafe fn has(p: DyadPtr, gate: DyadPtr) -> bool {
    is_node(p) && meta::gates_of(dyad::ty(p)).any(|g| g == gate)
}

/// The gates of `p`, in the order their mint keeps.
///
/// # Safety
/// As [`is_node`].
pub(crate) unsafe fn set_of(p: DyadPtr) -> Vec<DyadPtr> {
    if is_node(p) {
        meta::gates_of(dyad::ty(p)).collect()
    } else {
        Vec::new()
    }
}

/// The one order a set of gate words is kept in, each word once, so a set has one mint however
/// it was written: `pub mut` and `mut pub` are one.
fn ordered(gates: &[DyadPtr]) -> Vec<DyadPtr> {
    let mut set = gates.to_vec();
    set.sort_unstable();
    set.dedup();
    set
}

fn new_mint(store: &mut Store, gate_: DyadPtr, set: &[DyadPtr]) -> DyadPtr {
    let record = meta::gate_record(store, set);
    store.alloc_head(gate_, record)
}

/// The gate mint of the set `gates`, made once per set.
///
/// # Safety
/// Each of `gates` must be a gate word's identity from the store.
///
/// # Panics
/// Where `type_mint`'s own functions fail: a defect of the seed.
pub(crate) unsafe fn mint_of(store: &mut Store, types: &Core, gates: &[DyadPtr]) -> DyadPtr {
    let set = ordered(gates);
    let gate_ = types.gate_;
    let made = |store: &mut Store| Some(new_mint(store, gate_, &set));
    type_mint::mint_of(store, types, gate_, &set, made).expect("a gate mint is made")
}

/// The gate node of `mint` over `x`, made once per pair.
///
/// # Safety
/// `mint` must be a gate mint; `x` null or a dyad from the store that is no gate node.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn over(store: &mut Store, types: &Core, mint: DyadPtr, x: DyadPtr) -> DyadPtr {
    debug_assert!(!is_node(x), "gates never nest");
    let made = |store: &mut Store| Some(store.alloc_head(mint, x as *mut u8));
    type_mint::mint_of(store, types, mint, &[x], made).expect("a gate node is made")
}

/// The gate node of `mint` over the node `x` a name denotes, a function, a type or a number:
/// it gates `x` for that one name, so it is the name's own and `type_mint` keeps none.
///
/// # Safety
/// As [`over`].
pub(super) unsafe fn over_node(store: &mut Store, mint: DyadPtr, x: DyadPtr) -> DyadPtr {
    debug_assert!(!is_node(x), "gates never nest");
    store.alloc_head(mint, x as *mut u8)
}

/// The gate mint of `p`'s gates and `gate`.
///
/// # Safety
/// `p` must be null or a dyad from the store; `gate` a gate word's identity.
///
/// # Panics
/// As [`mint_of`].
unsafe fn mint_with(store: &mut Store, types: &Core, p: DyadPtr, gate: DyadPtr) -> DyadPtr {
    let mut set = set_of(p);
    set.push(gate);
    mint_of(store, types, &set)
}

/// The type `p` with `gate` added to its gates: the gate node of the larger set over the type
/// `p`'s gates stand over.
///
/// # Safety
/// `p` must be null or a type node from the store; `gate` a gate word's identity.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn with(store: &mut Store, types: &Core, p: DyadPtr, gate: DyadPtr) -> DyadPtr {
    let mint = mint_with(store, types, p, gate);
    over(store, types, mint, under(p))
}

/// [`mint_of`] while the seed registers its identities, before a core stands.
///
/// # Safety
/// As [`mint_of`]; `gate_` must be the former [`register`] gave.
pub(super) unsafe fn early_mint(store: &mut Store, gate_: DyadPtr, gates: &[DyadPtr]) -> DyadPtr {
    let set = ordered(gates);
    let made = |store: &mut Store| Some(new_mint(store, gate_, &set));
    type_mint::early(store, gate_, &set, made).expect("a gate mint is made")
}

/// [`over`] while the seed registers its identities.
///
/// # Safety
/// As [`over`].
pub(super) unsafe fn early_over(store: &mut Store, mint: DyadPtr, x: DyadPtr) -> DyadPtr {
    let made = |store: &mut Store| Some(store.alloc_head(mint, x as *mut u8));
    type_mint::early(store, mint, &[x], made).expect("a gate node is made")
}

/// The word of the name `binding` its gate node stands in: the declared type a field's `?`
/// node holds, else what the binding points at.
///
/// # Safety
/// `binding` must be a binding dyad from the store.
pub(crate) unsafe fn word_of_name(types: &Core, binding: DyadPtr) -> DyadPtr {
    let word = Binding::gated(binding);
    if hole::is_hole(types.unknown, word) {
        hole::gated_type_in(word)
    } else {
        word
    }
}

/// Whether the name `binding` has `gate`, read from the type of its value.
///
/// # Safety
/// As [`word_of_name`]; `gate` a gate word's identity.
pub(crate) unsafe fn name_has(types: &Core, binding: DyadPtr, gate: DyadPtr) -> bool {
    has(word_of_name(types, binding), gate)
}

/// Gives the name `binding` the gate `gate` beside those it has.
///
/// # Safety
/// As [`name_has`].
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn add_to_name(store: &mut Store, types: &Core, binding: DyadPtr, gate: DyadPtr) {
    let word = Binding::gated(binding);
    if hole::is_hole(types.unknown, word) {
        hole::set_type(word, with(store, types, hole::gated_type_in(word), gate));
    } else if Binding::read(binding).is_storage() {
        Binding::set_dyad(binding, with(store, types, word, gate));
    } else {
        let mint = mint_with(store, types, word, gate);
        Binding::set_dyad(binding, over_node(store, mint, under(word)));
    }
}

/// Whether `binding`'s gates grant a read from the scope `from`: with no `pub` it is read from
/// its own scope and below, along the parent links, and with `pub` from any scope (›A value's
/// gates are one ordered set in its type, combined as grants and vetoes‹ [[R81]]). A `.` from
/// outside a file, a type or a `{ … }` reaches only its `pub` names so (›Fields are read with a
/// dot outside their scope‹ [[R234]]).
///
/// # Safety
/// `binding` must be a binding dyad from the store; `from` null or a scope node.
pub(crate) unsafe fn grants_read(types: &Core, binding: DyadPtr, from: DyadPtr) -> bool {
    if name_has(types, binding, types.pub_) {
        return true;
    }
    let own = Binding::read(binding).scope;
    let mut at = from;
    while !at.is_null() {
        if at == own {
            return true;
        }
        at = super::scope::parent_of(at);
    }
    false
}

/// A type body's reader takes the word at a line's start; anywhere else on
/// a body line it stands for nothing.
fn outside_block(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<Constructed, ParseError> {
    if p.in_type_body_lines() {
        return Err(ParseError::StaticMisplaced);
    }
    construct(p, id, tape)
}

/// Anything but a declaration to the right is a parse error, not a silent
/// no-op: a gate that marked nothing would be a lie in the source.
fn construct(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<Constructed, ParseError> {
    let inner = p.take_right(tape)?;
    let types = p.types();
    // `mut alloc …` gives a pointer that may be written through; it carries no mark, since
    // no write through `@` is checked yet: stand-in for #33.
    // SAFETY: `inner` is a reduced dyad just parsed.
    if id == types.mut_ && unsafe { super::mint::origin(dyad::ty(inner)) } == types.alloc_ {
        tape.place(inner);
        return Ok(Constructed::Placed);
    }
    // SAFETY: `inner` is a reduced dyad just parsed.
    if unsafe { dyad::ty(inner) } != types.declare_ {
        return Err(ParseError::GateNeedsDeclaration);
    }
    p.gate_declared(id)?;
    tape.place(inner);
    Ok(Constructed::Placed)
}
