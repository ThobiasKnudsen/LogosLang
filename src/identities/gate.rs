// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `pub`, `mut`, `static` and `lossy`, the gate words, `offset`, the gate a type body's field
//! takes its byte from, and where a gate lives: in the type of the value it gates. Each set of
//! gates is one gate mint, a type, whatever byte an `offset` holds, and the gates over `X` are
//! one gate node, `[gate mint | X]`, or `[gate mint | X, the offset instance]` where `offset` is
//! among them, and `X` is never a gate node itself. `type_mint` keeps the gate mints and the gate
//! nodes over a type, the gated types, one per byte where `offset` is among the gates, each made
//! with its own `offset` instance. A name's gate node stands where its binding points: over the
//! type of its bytes for a name laid out in a frame, over the node it denotes for any other, and
//! in the declared type a field's `?` node holds. No gate node is no gate: the name is read from
//! its own scope and below, and written nowhere after its declaration.
//! ›Gates are type nodes: `x := mut i32 5`; the record holds no gates‹ [[R381]]

use super::type_mint::KeyPart;
use super::{hole, meta, type_mint, Cx};
use crate::binding::Binding;
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError};
use crate::store::Store;
use crate::Core;

/// The gate words: `pub`, `mut`, `static`, `lossy`; `offset`; and the former every gate mint is
/// kept under, the type of the gate mints.
pub(super) struct GateIds {
    pub(super) words: [DyadPtr; 4],
    pub(super) offset: DyadPtr,
    pub(super) gate_: DyadPtr,
}

/// The gate words as written, in the order [`GateIds::words`] and [`words`] keep them. They stand
/// in the one rank with the number types (›Every identity has a `parse_rank`, and `(` works in two
/// steps‹ [[R259]]): in `i32 lossy f64 2.5`, `f64` is built first, then `lossy`, then `i32`.
const WORDS: [&str; 4] = ["pub", "mut", "static", "lossy"];

pub(super) fn register(cx: &mut Cx) -> GateIds {
    let words = WORDS.map(|spelling| {
        let id = word(cx, spelling);
        cx.metas.insert(id, construct);
        id
    });
    // Only a type body knows `offset`, which declares it beside `self` (›Inside a type body, a
    // field's byte is its `offset N` relation: `a := offset 1 mut i32`‹ [[R385]]).
    let record = meta::token_record(cx, meta::prec::ONE_RANK, crate::parse::Assoc::Right, &[]);
    let offset = cx.store.alloc_head(cx.type_, record);
    cx.metas.insert(offset, |p, _id, tape| p.construct_offset(tape));
    let former = meta::record(cx.store, meta::TYPEREC_TAG, meta::prec::INERT);
    let gate_ = cx.store.alloc_head(cx.type_, former);
    GateIds { words, offset, gate_ }
}

/// The gates of a gate node apart from the type they stand over: their mint, and the byte its
/// `offset` instance holds where `offset` is among them, carried as one so that whatever moves a
/// node's gates onto another node moves its byte too (›Inside a type body, a field's byte is its
/// `offset N` relation: `a := offset 1 mut i32`‹ [[R385]]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Gates {
    mint: DyadPtr,
    byte: Option<u64>,
}

impl Gates {
    pub(crate) const NONE: Self = Self { mint: std::ptr::null_mut(), byte: None };

    pub(crate) fn is_none(self) -> bool {
        self.mint.is_null()
    }
}

/// The gates of the gate words `words`.
///
/// # Safety
/// Each of `words` must be a gate word's identity from the store, and none `offset`, which
/// [`offset`] gives with its byte.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn of_words(store: &mut Store, types: &Core, words: &[DyadPtr]) -> Gates {
    debug_assert!(!words.contains(&types.offset), "`offset` is given with its byte");
    Gates { mint: mint_of(store, types, words), byte: None }
}

/// The gate `offset` alone, giving the byte `byte`.
///
/// # Panics
/// As [`mint_of`].
pub(crate) fn offset(store: &mut Store, types: &Core, byte: u64) -> Gates {
    // SAFETY: `offset` is a gate's identity from the store.
    let mint = unsafe { mint_of(store, types, &[types.offset]) };
    Gates { mint, byte: Some(byte) }
}

/// Every gate word with its spelling.
pub(crate) fn words(types: &Core) -> [(DyadPtr, &'static str); 4] {
    let ids = [types.pub_, types.mut_, types.static_, types.lossy];
    std::array::from_fn(|i| (ids[i], WORDS[i]))
}

/// No node is ever typed by a gate word; its identity stands in a gate mint's record.
pub(super) fn word(cx: &mut Cx, name: &str) -> DyadPtr {
    let record = meta::token_record(cx, meta::prec::ONE_RANK, crate::parse::Assoc::Right, &[]);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare(name, id);
    id
}

/// The gate words of the gated type `t` as written before it, `offset 1 pub mut`: `offset` with its
/// byte first, then the others in the order its mint keeps; `None` where `t` is no gate node.
///
/// # Safety
/// As [`is_node`].
pub(crate) unsafe fn written(types: &Core, t: DyadPtr) -> Option<String> {
    if !is_node(t) {
        return None;
    }
    let all = words(types);
    let mut written: Vec<String> =
        byte_of(types, t).map(|n| format!("offset {n}")).into_iter().collect();
    written.extend(set_of(t).into_iter().filter(|&g| g != types.offset).map(|g| {
        let word = all.iter().find(|&&(id, _)| id == g);
        word.expect("a gate mint holds only gate words").1.to_string()
    }));
    Some(written.join(" "))
}

/// The byte the `offset` gate of `p` gives, read from the `offset` instance its gate node points
/// to; `None` where `p` is no gate node or has no `offset` (›Inside a type body, a field's byte is
/// its `offset N` relation: `a := offset 1 mut i32`‹ [[R385]]).
///
/// # Safety
/// As [`is_node`].
pub(crate) unsafe fn byte_of(types: &Core, p: DyadPtr) -> Option<u64> {
    if !has(p, types.offset) {
        return None;
    }
    let at = (dyad::value(p) as *const DyadPtr).add(1).read();
    (!at.is_null()).then(|| (dyad::value(at) as *const u64).read_unaligned())
}

/// Whether the value `node` gives is of a type gated `lossy`, which a number type takes with
/// loss (›A type's `parse` decides what converts into it; unwritten, a value converts only into
/// a type that holds it exactly‹ [[R155]]): a name, a parameter or a field declared so, a value
/// the gate word gave, or a call whose result is.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn is_lossy(types: &Core, node: DyadPtr) -> bool {
    let node = crate::binding::through(types, node);
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

/// Whether `p` is a value a gate word gave, `[G | v]`: a node whose type word `G` is a gate node,
/// over the type the value `v` gives (›A gate word reads the nearest value to its right and gives
/// it with its type gated‹ [[R102]]).
///
/// # Safety
/// `p` must be null or a dyad from the store.
pub(crate) unsafe fn is_value(types: &Core, p: DyadPtr) -> bool {
    if p.is_null() {
        return false;
    }
    let gated = dyad::ty(p);
    let mint = if gated.is_null() { gated } else { dyad::ty(gated) };
    !mint.is_null() && dyad::ty(mint) == types.gate_
}

/// The value a gate word gave `p` over, and its gates; `None` for any other node.
///
/// # Safety
/// As [`is_value`].
pub(crate) unsafe fn value_parts(types: &Core, p: DyadPtr) -> Option<(DyadPtr, Gates)> {
    is_value(types, p).then(|| (dyad::head(p) as DyadPtr, parts(types, dyad::ty(p)).1))
}

/// `value` with `gates` among those of its type (›A gate word reads the nearest value to its
/// right and gives it with its type gated‹ [[R102]]): a type standing as itself gives the gated
/// type, `mut i32`; a value the gate words gave has its one gate node grow; any other value, a
/// type `held` as a value of `type` among them, gives `[G | value]`, `G` the gate node of the
/// gates over the type the value gives. A name read there is a copy, so its own gates are not
/// among them (›Gates are type nodes: `x := mut i32 5`; the record holds no gates‹ [[R381]]).
/// A group's members are gated each (›`and`/`or` on non-booleans build a group; every operator
/// on a group applies to each member‹ [[R37]]). No gate gives `value` as it is. `field` says the
/// value is a type body field's own, the one value an `offset` may gate.
///
/// # Errors
/// `DoubleGate` where one of `gates` is among the gates already, `TypeKnownOnlyAtRun` where the
/// type the value gives is known only when the program runs, `OffsetOutsideField` where an
/// `offset` would gate a value that is no field's own.
///
/// # Safety
/// `value` must be a reduced dyad from the store.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn give(
    store: &mut Store,
    types: &Core,
    gates: Gates,
    value: DyadPtr,
    held: bool,
    field: bool,
) -> Result<DyadPtr, ParseError> {
    if gates.is_none() {
        return Ok(value);
    }
    if let Some((connective, a, b)) = super::group::members(types, value) {
        let a = give(store, types, gates, a, false, field)?;
        let b = give(store, types, gates, b, false, field)?;
        return super::group::join(store, types, connective, a, b);
    }
    let (under, own, is_type) = match value_parts(types, value) {
        Some((v, own)) => (v, own, false),
        // `?` is a value, the unknown, though the seed spells it as a type node (›`?` is one
        // value, the unknown; `key := T` declares without a value, and `key := T ?` is refused‹
        // [[R103]]).
        None => {
            match super::type_identity_of(types, value).filter(|&t| !held && t != types.unknown) {
                Some(t) => {
                    let (under, own) = parts(types, t);
                    (under, own, true)
                }
                None => (value, Gates::NONE, false),
            }
        }
    };
    let had = words_of(own);
    if words_of(gates).iter().any(|g| had.contains(g)) {
        return Err(ParseError::DoubleGate);
    }
    let gates = union(store, types, own, gates);
    if is_type {
        return Ok(over(store, types, gates, under));
    }
    // stand-in for Question 384
    if gates.byte.is_some() && (!field || words_of(gates).contains(&types.static_)) {
        return Err(ParseError::OffsetOutsideField);
    }
    let t = super::read::output_type(types, under);
    if t.is_null() {
        return Err(ParseError::TypeKnownOnlyAtRun);
    }
    let gated = over(store, types, gates, t);
    Ok(store.alloc_head(gated, under as *mut u8))
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

/// What the gates of `p` stand over, and the gates: `p` itself and no gate where `p` is no gate
/// node.
///
/// # Safety
/// As [`is_node`].
pub(crate) unsafe fn parts(types: &Core, p: DyadPtr) -> (DyadPtr, Gates) {
    if is_node(p) {
        (dyad::head(p) as DyadPtr, Gates { mint: dyad::ty(p), byte: byte_of(types, p) })
    } else {
        (p, Gates::NONE)
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

/// The gate node of `gates` over `x`, made once per pair; where `offset` is among them, once per
/// pair and byte, pointing to an `offset` instance holding the byte (›Inside a type body, a
/// field's byte is its `offset N` relation: `a := offset 1 mut i32`‹ [[R385]]).
///
/// # Safety
/// `gates` must be some gate; `x` null or a dyad from the store that is no gate node.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn over(store: &mut Store, types: &Core, gates: Gates, x: DyadPtr) -> DyadPtr {
    debug_assert!(!is_node(x), "gates never nest");
    let Some(n) = gates.byte else {
        let make = |store: &mut Store| Some(made(store, types, gates, x));
        return type_mint::mint_of(store, types, gates.mint, &[x], make)
            .expect("a gate node is made");
    };
    if !types.type_mint.is_read() {
        return made(store, types, gates, x);
    }
    let key = [KeyPart::Node(x), KeyPart::Word(n)];
    let (place, kept) =
        type_mint::find(store, types, gates.mint, &key).expect("`type_mint` walks a key");
    if let Some(node) = kept {
        return node;
    }
    let node = made(store, types, gates, x);
    type_mint::keep(store, types, place, node).expect("`type_mint` keeps a mint");
    node
}

/// A new gate node of `gates` over `x`: `[mint | x]`, or `[mint | x, the offset instance]`.
///
/// # Safety
/// As [`over`].
unsafe fn made(store: &mut Store, types: &Core, gates: Gates, x: DyadPtr) -> DyadPtr {
    match gates.byte {
        None => store.alloc_head(gates.mint, x as *mut u8),
        Some(n) => {
            let at = store.alloc_blob(types.offset, &n.to_ne_bytes());
            store.alloc_words(gates.mint, &[x, at])
        }
    }
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

/// The type `t` with `gates` among its own, `t` itself where there is no gate: a gate word over a
/// type with nothing after it gives the gated type, `mut i32`, as `i32` alone gives `i32` (›A
/// gate word reads the nearest value to its right and gives it with its type gated‹ [[R102]]).
///
/// # Safety
/// `t` must be a type node from the store.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn gated(store: &mut Store, types: &Core, gates: Gates, t: DyadPtr) -> DyadPtr {
    if gates.is_none() {
        return t;
    }
    let (t, own) = parts(types, t);
    let gates = union(store, types, own, gates);
    over(store, types, gates, t)
}

/// The gates `a` and `b` together, no gate where neither has one; an `offset` among them keeps
/// its byte.
///
/// # Safety
/// `a` and `b` must come from this store.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn union(store: &mut Store, types: &Core, a: Gates, b: Gates) -> Gates {
    if a.is_none() || b.is_none() {
        return if a.is_none() { b } else { a };
    }
    let mut set = words_of(a);
    set.extend(words_of(b));
    Gates { mint: mint_of(store, types, &set), byte: a.byte.or(b.byte) }
}

/// `gates` without `gate`, no gate where none is left; an `offset` left among them keeps its
/// byte.
///
/// # Safety
/// As [`union`]; `gate` a gate's identity.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn without(
    store: &mut Store,
    types: &Core,
    gates: Gates,
    gate: DyadPtr,
) -> Gates {
    let set: Vec<DyadPtr> = words_of(gates).into_iter().filter(|&g| g != gate).collect();
    if set.is_empty() {
        return Gates::NONE;
    }
    let byte = gates.byte.filter(|_| gate != types.offset);
    Gates { mint: mint_of(store, types, &set), byte }
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

/// Gives the name `binding` `gates` beside those it has.
///
/// # Safety
/// As [`name_has`].
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn add_to_name(store: &mut Store, types: &Core, binding: DyadPtr, gates: Gates) {
    if gates.is_none() {
        return;
    }
    let word = Binding::gated(binding);
    if hole::is_hole(types.unknown, word) {
        hole::set_type(word, gated(store, types, gates, hole::gated_type_in(word)));
    } else if Binding::read(binding).is_storage() {
        Binding::set_dyad(binding, gated(store, types, gates, word));
    } else {
        let (under, own) = parts(types, word);
        let gates = union(store, types, own, gates);
        Binding::set_dyad(binding, made(store, types, gates, under));
    }
}

/// Whether `binding`'s gates grant a read from the scope `from`: with no `pub` it is read from
/// its own scope and below, along the parent links, and with `pub` from any scope (›A value's
/// gates are one ordered set in its type, combined as grants and vetoes‹ [[R81]]). A `.` from
/// outside a file, a type or a `{ … }` reaches only its `pub` names so (›Fields are read with a
/// dot, outside their type and inside it through `self`‹ [[R234]]).
///
/// # Safety
/// `binding` must be a binding dyad from the store; `from` null or a scope node.
pub(crate) unsafe fn grants_read(types: &Core, binding: DyadPtr, from: DyadPtr) -> bool {
    name_has(types, binding, types.pub_) || within(from, Binding::read(binding).scope)
}

/// Whether the scope `from` is `own` or below it, along the parent links: where a name without
/// `pub` is read.
///
/// # Safety
/// `from` must be null or a scope node.
pub(crate) unsafe fn within(from: DyadPtr, own: DyadPtr) -> bool {
    let mut at = from;
    while !at.is_null() {
        if at == own {
            return true;
        }
        at = super::scope::parent_of(at);
    }
    false
}

/// `gates` and the gate word `gate`, `None` where `gate` stands among them already.
///
/// # Safety
/// As [`union`]; `gate` a gate word's identity.
///
/// # Panics
/// As [`mint_of`].
pub(crate) unsafe fn joined(
    store: &mut Store,
    types: &Core,
    gates: Gates,
    gate: DyadPtr,
) -> Option<Gates> {
    if words_of(gates).contains(&gate) {
        return None;
    }
    let word = of_words(store, types, &[gate]);
    Some(union(store, types, gates, word))
}

/// The gates of `gates` by their identities, `offset` among them where it is one.
///
/// # Safety
/// `gates` must come from this store.
pub(crate) unsafe fn words_of(gates: Gates) -> Vec<DyadPtr> {
    if gates.is_none() {
        Vec::new()
    } else {
        meta::gates_of(gates.mint).collect()
    }
}

/// A gate word reads the nearest value to its right, as a type does (`Parser::type_operand`), and
/// gives it with `id` among the gates of its type, as [`give`] gives it (›A gate word reads the
/// nearest value to its right and gives it with its type gated‹ [[R102]]). A declaration is
/// no value to gate, its gate standing right of its own `:=` (›Left of `:=` stands one name (or
/// regex), and nothing else‹ [[R101]]), and neither is any other line that gives nothing; a
/// place is one, whatever its type.
fn construct(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<Constructed, ParseError> {
    let word_at = tape.at(0).map(|word| word.start);
    let types = p.types();
    // SAFETY: `id` is a gate word.
    let gates = unsafe { of_words(p.store(), types, &[id]) };
    mark(p, tape, gates, word_at)
}

/// The value right of the word at the center, given with `gates` among those of its type, as
/// [`give`] gives it, in that word's place; an error is placed at `word_at`, where the word
/// stands.
pub(crate) fn mark(
    p: &mut crate::parse::Parser,
    tape: &mut crate::parse::ParsingTape,
    gates: Gates,
    word_at: Option<usize>,
) -> Result<Constructed, ParseError> {
    // The value as built, so a gate word nearer the value built there joins this one's gates:
    // `lossy mut f64 2.5` is `mut lossy f64`.
    let Some(right) = p.type_operand(tape, 1)? else {
        let e = if tape.at(1).is_some() {
            ParseError::GateBeforeLaterWord
        } else {
            ParseError::MissingOperand
        };
        return Err(match word_at {
            Some(at) => p.fail_at(at, e),
            None => e,
        });
    };
    let word_at = word_at.unwrap_or(right.start);
    let inner = p.as_operand(right)?;
    tape.remove(1);
    let types = p.types();
    // SAFETY: `inner` is a reduced dyad just parsed.
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
    let field = p.in_field_line();
    // SAFETY: `inner` is a reduced dyad.
    let gated = unsafe { give(p.store(), types, gates, inner, right.type_value, field) };
    let gated = gated.map_err(|e| match e {
        ParseError::DoubleGate => p.fail_at(word_at, e),
        e => p.fail_at(right.start, e),
    })?;
    let mut value = right;
    value.dyad = gated;
    value.constructed = true;
    tape.place_gated(value);
    Ok(Constructed::Placed)
}
