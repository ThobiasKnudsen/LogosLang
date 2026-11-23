// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `pub`, `mut`, `static` and `lossy`, the gate words, and where a gate lives: in the type of the
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

/// The gate words: `pub`, `mut`, `static`, `lossy`, and the former every gate mint is kept under,
/// the type of the gate mints.
pub(super) struct GateIds {
    pub(super) words: [DyadPtr; 4],
    pub(super) gate_: DyadPtr,
}

/// The gate words as written, with their ranks, in the order [`GateIds::words`] and [`words`]
/// keep them. `lossy` stands with the number types, whose rank it shares (›Every identity has a
/// `parse_rank`, and `(` works in two steps‹ [[R259]]): in `i32 lossy f64 2.5`, `f64` is built
/// first, then `lossy`, then `i32`.
const WORDS: [(&str, f64); 4] = [
    ("pub", meta::prec::PREFIX),
    ("mut", meta::prec::PREFIX),
    ("static", meta::prec::PREFIX),
    ("lossy", meta::prec::CONVERT),
];

pub(super) fn register(cx: &mut Cx) -> GateIds {
    let words = WORDS.map(|(spelling, rank)| {
        let id = word(cx, spelling, rank);
        cx.metas.insert(id, construct);
        id
    });
    let former = meta::record(cx.store, meta::TYPEREC_TAG, meta::prec::INERT);
    let gate_ = cx.store.alloc_head(cx.type_, former);
    GateIds { words, gate_ }
}

/// Every gate word with its spelling.
pub(crate) fn words(types: &Core) -> [(DyadPtr, &'static str); 4] {
    let ids = [types.pub_, types.mut_, types.static_, types.lossy];
    std::array::from_fn(|i| (ids[i], WORDS[i].0))
}

/// No node is ever typed by a gate word; its identity stands in a gate mint's record.
pub(super) fn word(cx: &mut Cx, name: &str, rank: f64) -> DyadPtr {
    let record = meta::token_record(cx, rank, crate::parse::Assoc::Right, &[]);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare(name, id);
    id
}

/// The gate words of the gated type `t` as written before it, `pub mut`, in the order its mint
/// keeps; `None` where `t` is no gate node.
///
/// # Safety
/// As [`is_node`].
pub(crate) unsafe fn written(types: &Core, t: DyadPtr) -> Option<String> {
    if !is_node(t) {
        return None;
    }
    let all = words(types);
    let written: Vec<&str> = set_of(t)
        .into_iter()
        .map(|g| {
            let word = all.iter().find(|&&(id, _)| id == g);
            word.expect("a gate mint holds only gate words").1
        })
        .collect();
    Some(written.join(" "))
}

/// Whether the value `node` gives is of a type gated `lossy`, which a number type takes with
/// loss (›A type's `parse` decides what converts into it; unwritten, a value converts only into
/// a type that holds it exactly‹ [[R155]]): a name, a parameter or a field declared so, or a
/// call whose result is.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn is_lossy(types: &Core, node: DyadPtr) -> bool {
    let node = types.through(node);
    let t = if node.is_null() {
        node
    } else if dyad::ty(node) == types.binding_ {
        word_of_name(types, node)
    } else if dyad::ty(node) == types.field_ {
        word_of_name(types, super::instance::field_parts(node).1)
    } else {
        super::read::given_type(types, node)
    };
    has(t, types.lossy)
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

/// What the gates of `p` stand over, and their mint: `p` itself and null where `p` is no gate
/// node.
///
/// # Safety
/// As [`is_node`].
pub(crate) unsafe fn parts(p: DyadPtr) -> (DyadPtr, DyadPtr) {
    if is_node(p) {
        (dyad::head(p) as DyadPtr, dyad::ty(p))
    } else {
        (p, std::ptr::null_mut())
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

/// The type `t` with the gates of `mint` among its own, `t` itself where `mint` is null: a gate
/// word over a type with nothing after it gives the gated type, `mut i32`, as `i32` alone gives
/// `i32` (›A gate word reads the nearest value to its right and gives it with its type gated‹
/// [[R102]]).
///
/// # Safety
/// `t` must be a type node from the store; `mint` null or a gate mint.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn gated(store: &mut Store, types: &Core, mint: DyadPtr, t: DyadPtr) -> DyadPtr {
    if mint.is_null() {
        return t;
    }
    let (t, own) = parts(t);
    let mint = union(store, types, own, mint);
    over(store, types, mint, t)
}

/// The gate mint of the gates of the mints `a` and `b` together, either null for no gates.
///
/// # Safety
/// `a` and `b` must be null or gate mints.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn union(store: &mut Store, types: &Core, a: DyadPtr, b: DyadPtr) -> DyadPtr {
    if a.is_null() || b.is_null() {
        return if a.is_null() { b } else { a };
    }
    let mut set = words_of(a);
    set.extend(words_of(b));
    mint_of(store, types, &set)
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

/// The gate mint of `mint`'s gates and `gate`, `None` where `gate` stands among them already.
///
/// # Safety
/// `mint` must be null or a gate mint; `gate` a gate word's identity.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn joined(
    store: &mut Store,
    types: &Core,
    mint: DyadPtr,
    gate: DyadPtr,
) -> Option<DyadPtr> {
    let mut set: Vec<DyadPtr> =
        if mint.is_null() { Vec::new() } else { meta::gates_of(mint).collect() };
    if set.contains(&gate) {
        return None;
    }
    set.push(gate);
    Some(mint_of(store, types, &set))
}

/// The gates of the gate mint `mint`, none for null.
///
/// # Safety
/// `mint` must be null or a gate mint.
pub(crate) unsafe fn words_of(mint: DyadPtr) -> Vec<DyadPtr> {
    if mint.is_null() {
        Vec::new()
    } else {
        meta::gates_of(mint).collect()
    }
}

/// A gate word reads the nearest value to its right, a word there not built yet started first,
/// and gives it with `id` among the gates of its type (›A gate word reads the nearest value to
/// its right and gives it with its type gated‹ [[R102]]). A declaration is no
/// value to gate, its gate standing right of its own `:=` (›Left of `:=` stands one name (or
/// regex), and nothing else‹ [[R101]]), and neither is any other line that gives nothing; a
/// place is one, whatever its type.
fn construct(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<Constructed, ParseError> {
    // The cell as built, so the gates a gate word of a lower rank put on it there join this one's:
    // `lossy mut f64 2.5` is `mut lossy f64`.
    let Some(right) = p.side_at(tape, 1)? else {
        return Err(ParseError::MissingOperand);
    };
    let word_at = tape.at(0).map_or(right.start, |word| word.start);
    let inner = p.as_operand(right)?;
    tape.remove(1);
    let types = p.types();
    // A value that ends in a line held for the run gives a `dyad`, as a landing does (`land`).
    // SAFETY: `inner` is a reduced dyad just parsed.
    unsafe { super::waiting::lands_in(types, inner, types.dyad_) };
    // SAFETY: as above.
    let (declares, gives_nothing) = unsafe {
        (
            dyad::ty(inner) == types.declare_,
            !right.hole && super::read::output_type(types, inner) == types.void_,
        )
    };
    if declares {
        return Err(p.fail_at(word_at, ParseError::GateNeedsValue));
    }
    if gives_nothing {
        return Err(p.fail_at(right.start, ParseError::StatementAsValue));
    }
    let mut value = right;
    value.dyad = inner;
    value.constructed = true;
    // SAFETY: the cell's gates are null or a gate mint, and `id` is a gate word.
    let Some(mint) = (unsafe { joined(p.store(), types, value.gates, id) }) else {
        return Err(p.fail_at(word_at, ParseError::DoubleGate));
    };
    tape.place_gated(value, mint);
    Ok(Constructed::Placed)
}
