// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The `static`-member record every identity carries in its value slot: parse rank
//! and associativity, the constructor and destructor leaves, the run-body slot, the
//! interned pointer type, and the layout its values are read by.
//!
//! Record layout (unaligned, native-endian, byte offsets):
//!
//! ```text
//! [0]        u8   kind: the type-tag namespace (see below)
//! [1]        u8   associativity (0 left-to-right, 1 right-to-left)
//! [2..10]    f64  parse_rank
//! [10..18]   u64  constructor: a `seed-parse` callable leaf, or 0
//! [18..26]   u64  destructor: the owning pointer's teardown, else 0
//! [26..34]   u64  run body: the lexed body a `run = (…)` line held, or 0
//! [34..42]   u64  pointer type: the interned `@T` of this type, or 0
//! [42..50]   u64  extent: a `seed-extent` callable leaf for a token that reads its own
//!                 extent, or 0
//! [50..58]   u64  mints: this type's mints made so far, an array, or 0
//! [58..66]   u64  mint of: the type this one is a mint of, or 0
//! [66]       u8   1 where the type's definition says a value of it is its node's address
//! [67]       u8   where its nodes carry the `own` mark: `EVERY_NODE` here for every node, else
//!                 the slot of each node that holds it, the `own` word or null; `NO_SLOT` nowhere
//! [68]       u8   the slots whose node a node of this type hands on as its value, one bit each
//! [69]       u8   the slots whose lines it hands on the last of, one bit each
//! [70..]     payload, per kind:
//!              ADDR              pointee type node (`dyad@`)
//!              TUPLE/LIST/TOKEN   u8 arity, u8 op slot, u8 output slot (`NO_SLOT` for
//!                                 none), then arity × `dyad@` role-name strings; a token's
//!                                 roles name the sides it stands between, never a slot
//!              RECORD            see `RECORD_TAG`
//!            a mint's is its origin's, no op or output slot, then `[output_type, op, key]`,
//!            the words its nodes share and the array of field types it was made for
//! ```
//!
//! The kind byte continues `numtype`'s tag space (`NumType` 0–9, `VOID_TAG` 10,
//! `STRING_TAG` 11, `COMMENT_TAG` 12, `ADDR_TAG` 13).

use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ExtentFn};
use crate::store::Store;

use super::numtype::ADDR_TAG;
use super::{string, Cx};
use crate::dyad;

/// Values are `arity` fixed `dyad@` operand slots, each named by a role string; a
/// null slot is an absent optional. Also an fn value's `[input, output, body, bcode]`.
pub(crate) const TUPLE_TAG: u8 = 14;
/// Values are `arity` fixed named `dyad@` slots, then a null-terminated variadic tail.
pub(crate) const LIST_TAG: u8 = 15;
/// Values are `[num: i64, den: i64]` comptime fractions.
pub(crate) const FRACTION_TAG: u8 = 16;
/// Values are themselves types, each carrying a record like this one; the root's kind.
pub(crate) const TYPEREC_TAG: u8 = 18;
/// A parse-only token; no values exist. Its roles name the sides it stands between: `lhs` first
/// where it continues what stands to its left, none where it starts what follows it.
pub(crate) const TOKEN_TAG: u8 = 19;
/// Values are `[entry: @exec, convention]`, 16 bytes; every exec leaf is one.
pub(crate) const CALLABLE_TAG: u8 = 20;
/// Values are calling-convention identities; the value slot holds the name string node.
pub(crate) const CONVENTION_TAG: u8 = 21;
/// Values are `[len: u64][data: @dyad]`, 16 bytes; a list never lives inline in a node.
pub(crate) const ARRAY_TAG: u8 = 22;
/// A record type node's record: payload `[scope: @dyad][fields: @dyad][size_bytes: u64]
/// [body: @dyad][free: @dyad][bracket builder: @dyad][field bindings: @dyad]`, 56 bytes, locked
/// at definition; `body` is null where the type has none, `free` where its body fills none.
pub(crate) const RECORD_TAG: u8 = 23;
/// Values are dyad views: the value IS the viewed node's address.
pub(crate) const DYAD_TAG: u8 = 24;

const ASSOC_OFF: usize = 1;
/// The parse_rank.
const PREC_OFF: usize = 2;
const CTOR_OFF: usize = 10;
const DTOR_OFF: usize = 18;
/// The lexed `run` body with its constructed functions.
const RUN_BODY_OFF: usize = 26;
/// The interned `@T` of this type, so two spellings of `@i32` are one node.
const POINTER_TYPE_OFF: usize = 34;
const EXTENT_OFF: usize = 42;
const MINTS_OFF: usize = 50;
const MINT_OF_OFF: usize = 58;
const NODE_VALUE_OFF: usize = 66;
const OWN_OFF: usize = 67;
const HANDS_ON_OFF: usize = 68;
const HANDS_ON_LINES_OFF: usize = 69;
pub(crate) const PAYLOAD_OFF: usize = 70;

/// The one parse_rank axis. Higher binds tighter. A constructor at or above `OPEN`
/// runs at discovery, one below at the segment boundary, highest first, associativity
/// breaking ties. DESIGN fixes the order and the threshold, never the numbers.
/// DESIGN ›The scope's constructor is the driver‹.
pub(crate) mod prec {
    /// Above `(`, so it acts at discovery as a segment boundary.
    pub const COMMA: f64 = 100.0;
    /// A number, `«…»`, `true`/`false`, and `#`: constructed the moment it is lexed.
    pub const LITERAL: f64 = 96.0;
    /// `import`: consumes its raw path token at discovery.
    pub const IMPORT: f64 = 95.0;
    /// `:=` and `=`: each reads its left and drives its right side to the boundary.
    pub const DECLARE: f64 = 93.0;
    /// The identities that read their own bracket or right side: `fn`, `for`, `while`, `defer`,
    /// `type`, `if`, `scope`.
    pub const READER: f64 = 92.0;
    /// `(`: the discovery threshold.
    pub const OPEN: f64 = 90.0;
    /// Application (`f(x)`, `pt(1, 2)`): reads its bracket lazily in source order, so the
    /// tight reads sit above it and the bracket readers above those.
    pub const APPLY: f64 = 91.0;
    /// `?`: above a type that takes a value, so in `i32 ?` the unknown stands before `i32` takes it.
    pub const HOLE: f64 = 87.0;
    /// A type taking the value to its right, `i64 m`, and `lossy` between it and the value: below
    /// `(` and `?`, above `&` and the prefix words. ›A parse is ranked by what it takes from its
    /// right‹ [[R116]].
    pub const CONVERT: f64 = 86.0;
    /// `.`, `:`, `@`: the tight reads of the cell to their left, above the readers and
    /// below `:=`/`=`; the right cell is lexed lazily inside the constructor.
    pub const TIGHT: f64 = 92.5;
    /// `&`.
    pub const ADDRESS: f64 = 85.0;
    /// The prefix words over a place: `own`, `move`, `free`, `alloc`, `pub`.
    pub const PREFIX: f64 = 82.0;
    pub const MULTIPLICATIVE: f64 = 70.0;
    pub const ADDITIVE: f64 = 60.0;
    /// `..`, the range infix `for` reads.
    pub const RANGE: f64 = 55.0;
    pub const COMPARE: f64 = 50.0;
    pub const EQUALITY: f64 = 40.0;
    /// Above `and` so `a and not b` reads as in Python; below the comparisons so `not a == b`
    /// negates the comparison.
    pub const NOT: f64 = 35.0;
    pub const AND: f64 = 30.0;
    pub const OR: f64 = 20.0;
    pub const RETURN: f64 = 10.0;
    /// No constructor: a delimiter, a data type, a node record; never constructed by the driver.
    pub const INERT: f64 = 0.0;

    /// A cell whose construction is its lexing: `(`, `[`, a literal, a raw-text word. Only
    /// these are built when a constructor's read lexes them.
    pub fn built_as_lexed(rank: f64) -> bool {
        rank == OPEN || rank >= IMPORT
    }
}

/// A record with no payload.
pub(crate) fn record(store: &mut Store, kind: u8, parse_rank: f64) -> *mut u8 {
    record_assoc(store, kind, parse_rank, Assoc::Left)
}

/// A record with no payload, for a type whose values are their nodes' addresses.
pub(crate) fn record_of_nodes(store: &mut Store, kind: u8, parse_rank: f64) -> *mut u8 {
    let mut blob = header(kind, Assoc::Left, parse_rank);
    blob[NODE_VALUE_OFF] = 1;
    store.alloc_bytes(&blob)
}

/// Right for a prefix word, so that in a chain the rightmost runs first and each finds
/// its operand constructed.
pub(crate) fn record_assoc(store: &mut Store, kind: u8, parse_rank: f64, assoc: Assoc) -> *mut u8 {
    debug_assert_ne!(kind, TOKEN_TAG, "a token's record names its sides: `token_record`");
    let blob = header(kind, assoc, parse_rank);
    store.alloc_bytes(&blob)
}

pub(crate) fn operand_record(
    cx: &mut Cx,
    kind: u8,
    parse_rank: f64,
    assoc: Assoc,
    roles: &[&str],
) -> *mut u8 {
    debug_assert!(matches!(kind, TUPLE_TAG | LIST_TAG), "operand records carry operand kinds");
    roles_record(cx, kind, parse_rank, assoc, roles)
}

/// A parse-only token's record, `sides` the sides it stands between, `lhs` first where it
/// continues what stands to its left (`..`, `.`, `:=`), none where it starts what follows it
/// (`«`, `(`, a gate word).
pub(crate) fn token_record(cx: &mut Cx, parse_rank: f64, assoc: Assoc, sides: &[&str]) -> *mut u8 {
    roles_record(cx, TOKEN_TAG, parse_rank, assoc, sides)
}

fn roles_record(cx: &mut Cx, kind: u8, parse_rank: f64, assoc: Assoc, roles: &[&str]) -> *mut u8 {
    debug_assert!(!cx.string_.is_null(), "role names need the string logos registered");
    let mut blob = header(kind, assoc, parse_rank).to_vec();
    let slot_of = |name: &str| roles.iter().position(|&r| r == name).map_or(NO_SLOT, |i| i as u8);
    blob[OWN_OFF] = slot_of("own");
    blob.extend_from_slice(&[roles.len() as u8, slot_of("op"), slot_of("output_type")]);
    for role in roles {
        let name = string::build_text(cx.store, cx.string_, role.as_bytes());
        blob.extend_from_slice(&(name as usize).to_ne_bytes());
    }
    cx.store.alloc_bytes(&blob)
}

/// A built-in's mint's record: its operand record's roles with no op slot and no output slot,
/// then the words every node of the mint shares, the field types it was made for last.
///
/// # Safety
/// `origin` must carry a `TUPLE_TAG` operand record; `key` must be an array node.
pub(crate) unsafe fn mint_record(
    store: &mut Store,
    origin: DyadPtr,
    output_type: DyadPtr,
    op: DyadPtr,
    key: DyadPtr,
) -> *mut u8 {
    let kind = kind_of(origin).expect("a mint's origin carries a record");
    debug_assert_eq!(kind, TUPLE_TAG, "a built-in's mint's nodes are fixed operand runs");
    let mut blob = header(kind, Assoc::Left, prec::INERT).to_vec();
    blob[MINT_OF_OFF..MINT_OF_OFF + 8].copy_from_slice(&(origin as usize).to_ne_bytes());
    // Its nodes carry the `own` mark and hand on what the origin's do.
    let shared = OWN_OFF..PAYLOAD_OFF;
    blob[shared.clone()]
        .copy_from_slice(std::slice::from_raw_parts(dyad::head(origin).add(OWN_OFF), shared.len()));
    let arity = arity_of(origin);
    blob.extend_from_slice(&[arity as u8, NO_SLOT, NO_SLOT]);
    for i in 0..arity {
        blob.extend_from_slice(&(role_of(origin, i) as usize).to_ne_bytes());
    }
    for word in [output_type, op, key] {
        blob.extend_from_slice(&(word as usize).to_ne_bytes());
    }
    store.alloc_bytes(&blob)
}

/// This type's mints made so far, an array; null while it has none.
///
/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn mints_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(MINTS_OFF) as *const DyadPtr)
}

/// # Safety
/// `id` must carry a record and `mints` must be an array node of its mints.
pub(crate) unsafe fn install_mints(id: DyadPtr, mints: DyadPtr) {
    std::ptr::write_unaligned(dyad::head(id).add(MINTS_OFF) as *mut DyadPtr, mints);
}

/// The type `id` is a mint of; null for a type that is no mint.
///
/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn mint_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(MINT_OF_OFF) as *const DyadPtr)
}

const MINT_OUTPUT: usize = 0;
const MINT_OP: usize = 1;
const MINT_KEY: usize = 2;

/// Where the mint's `k`th shared word lies, after its payload.
///
/// # Safety
/// `id` must be a mint (`mint_of` non-null), its record from `mint_record`.
unsafe fn mint_word_at(id: DyadPtr, k: usize) -> *mut DyadPtr {
    let words = ROLES_OFF + arity_of(id) * std::mem::size_of::<DyadPtr>();
    dyad::head(id).add(words + k * std::mem::size_of::<DyadPtr>()) as *mut DyadPtr
}

/// The type every node of the mint gives back.
///
/// # Safety
/// As `mint_word_at`.
pub(crate) unsafe fn mint_output_of(id: DyadPtr) -> DyadPtr {
    mint_word_at(id, MINT_OUTPUT).read_unaligned()
}

/// What every node of the mint runs, its machine operation.
///
/// # Safety
/// As `mint_word_at`.
pub(crate) unsafe fn mint_op_of(id: DyadPtr) -> DyadPtr {
    mint_word_at(id, MINT_OP).read_unaligned()
}

/// The array of field types the mint was made for.
///
/// # Safety
/// As `mint_word_at`.
pub(crate) unsafe fn mint_key_of(id: DyadPtr) -> DyadPtr {
    mint_word_at(id, MINT_KEY).read_unaligned()
}

pub(crate) fn pointer_record(store: &mut Store, pointee: DyadPtr) -> *mut u8 {
    let mut blob = header(ADDR_TAG, Assoc::Left, prec::INERT).to_vec();
    blob.extend_from_slice(&(pointee as usize).to_ne_bytes());
    store.alloc_bytes(&blob)
}

/// `bindings` is the array of the fields' bindings, in the order of `fields`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_layout(
    store: &mut Store,
    scope: DyadPtr,
    fields: DyadPtr,
    bindings: DyadPtr,
    size_bytes: u64,
    body: DyadPtr,
    parse_rank: f64,
    assoc: Assoc,
) -> *mut u8 {
    let mut blob = header(RECORD_TAG, assoc, parse_rank).to_vec();
    blob.extend_from_slice(&(scope as usize).to_ne_bytes());
    blob.extend_from_slice(&(fields as usize).to_ne_bytes());
    blob.extend_from_slice(&size_bytes.to_ne_bytes());
    blob.extend_from_slice(&(body as usize).to_ne_bytes());
    blob.extend_from_slice(&0usize.to_ne_bytes());
    blob.extend_from_slice(&0usize.to_ne_bytes());
    blob.extend_from_slice(&(bindings as usize).to_ne_bytes());
    store.alloc_bytes(&blob)
}

/// Where a record type's field names are declared.
///
/// # Safety
/// `id` must carry a `RECORD_TAG` record.
pub(crate) unsafe fn record_scope_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(PAYLOAD_OFF) as *const DyadPtr)
}

/// # Safety
/// As `record_scope_of`.
pub(crate) unsafe fn record_fields_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(PAYLOAD_OFF + 8) as *const DyadPtr)
}

/// # Safety
/// As `record_scope_of`.
pub(crate) unsafe fn record_size_of(id: DyadPtr) -> u64 {
    std::ptr::read_unaligned(dyad::head(id).add(PAYLOAD_OFF + 16) as *const u64)
}

/// The definition body's scope, or null where the type has no body.
///
/// # Safety
/// As `record_scope_of`.
pub(crate) unsafe fn record_body_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(PAYLOAD_OFF + 24) as *const DyadPtr)
}

const INSTANCES_FREE_OFF: usize = PAYLOAD_OFF + 32;

/// The `fn` a `static free = (…)` line filled, over the one unnamed value parameter; null where the type
/// body fills none.
///
/// # Safety
/// As `record_scope_of`.
pub(crate) unsafe fn instances_free_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(INSTANCES_FREE_OFF) as *const DyadPtr)
}

/// # Safety
/// `id` must carry a `RECORD_TAG` record; `free` must be null or a `fn` node from the store.
pub(crate) unsafe fn install_instances_free(id: DyadPtr, free: DyadPtr) {
    std::ptr::write_unaligned(dyad::head(id).add(INSTANCES_FREE_OFF) as *mut DyadPtr, free);
}

const BRACKET_BUILDER_OFF: usize = PAYLOAD_OFF + 40;

/// The run type whose node builds a value of this type from a bracket; null where none
/// does. stand-in for #152.
///
/// # Safety
/// As `record_scope_of`.
pub(crate) unsafe fn bracket_builder_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(BRACKET_BUILDER_OFF) as *const DyadPtr)
}

/// # Safety
/// `id` must carry a `RECORD_TAG` record; `builder` a type node from the store.
pub(crate) unsafe fn install_bracket_builder(id: DyadPtr, builder: DyadPtr) {
    std::ptr::write_unaligned(dyad::head(id).add(BRACKET_BUILDER_OFF) as *mut DyadPtr, builder);
}

const FIELD_BINDINGS_OFF: usize = PAYLOAD_OFF + 48;

/// The array of the fields' bindings, in the order of `record_fields_of`, each carrying its
/// field's offset in a value (DESIGN ›A scope lays out its declarations; a use reaches the
/// offset through its binding‹).
///
/// # Safety
/// As `record_scope_of`.
pub(crate) unsafe fn record_field_bindings_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(FIELD_BINDINGS_OFF) as *const DyadPtr)
}

fn header(kind: u8, assoc: Assoc, parse_rank: f64) -> [u8; PAYLOAD_OFF] {
    let mut h = [0u8; PAYLOAD_OFF];
    h[0] = kind;
    h[ASSOC_OFF] = match assoc {
        Assoc::Left => 0,
        Assoc::Right => 1,
    };
    h[PREC_OFF..CTOR_OFF].copy_from_slice(&parse_rank.to_ne_bytes());
    h[OWN_OFF] = NO_SLOT;
    // The other slots from CTOR_OFF on stay zero until their writers fill them.
    h
}

/// Null is the undefined constructor of a delimiter token or a data type, inert on the tape.
///
/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn constructor_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(CTOR_OFF) as *const DyadPtr)
}

/// Null on every identity but an owning pointer type; never faked with a no-op.
///
/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn destructor_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(DTOR_OFF) as *const DyadPtr)
}

/// # Safety
/// `id` must carry a record and `leaf` must be a callable leaf whose entry is a `ConstructFn`.
pub(crate) unsafe fn install_constructor(id: DyadPtr, leaf: DyadPtr) {
    std::ptr::write_unaligned(dyad::head(id).add(CTOR_OFF) as *mut DyadPtr, leaf);
}

/// # Safety
/// `id` must carry a record and `leaf` must be a callable leaf whose entry runs the
/// identity's teardown.
pub(crate) unsafe fn install_destructor(id: DyadPtr, leaf: DyadPtr) {
    std::ptr::write_unaligned(dyad::head(id).add(DTOR_OFF) as *mut DyadPtr, leaf);
}

/// The lexed body a `run = (…)` line held, constructed per field-type set;
/// null for a type with no run.
///
/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn run_body_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(RUN_BODY_OFF) as *const DyadPtr)
}

/// # Safety
/// `id` must carry a record and `body` must be a run-body node from the store.
pub(crate) unsafe fn install_run_body(id: DyadPtr, body: DyadPtr) {
    std::ptr::write_unaligned(dyad::head(id).add(RUN_BODY_OFF) as *mut DyadPtr, body);
}

/// The interned `@T` of `id`, or null while none has been minted.
///
/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn pointer_type_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(POINTER_TYPE_OFF) as *const DyadPtr)
}

/// # Safety
/// `id` must carry a record and `p` must be the plain pointer type node over `id`.
pub(crate) unsafe fn install_pointer_type(id: DyadPtr, p: DyadPtr) {
    std::ptr::write_unaligned(dyad::head(id).add(POINTER_TYPE_OFF) as *mut DyadPtr, p);
}

/// Null on every identity whose extent is its spelling.
///
/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn extent_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned(dyad::head(id).add(EXTENT_OFF) as *const DyadPtr)
}

/// # Safety
/// `id` must carry a record and `leaf` must be a callable leaf whose entry is an `ExtentFn`.
pub(crate) unsafe fn install_extent(id: DyadPtr, leaf: DyadPtr) {
    std::ptr::write_unaligned(dyad::head(id).add(EXTENT_OFF) as *mut DyadPtr, leaf);
}

/// The reader of a token that reads its own extent (DESIGN ›Unknown spellings are two
/// pattern identities‹), or `None`. Safe on any node, as `kind_of` is.
///
/// # Safety
/// `id` must be null or a valid dyad from the store.
pub(crate) unsafe fn extent_reader(id: DyadPtr) -> Option<ExtentFn> {
    if id.is_null() || kind_of(id).is_none() {
        return None;
    }
    let leaf = extent_of(id);
    if leaf.is_null() {
        return None;
    }
    // SAFETY: `install_extent` is the one writer, and takes only a leaf minted from an `ExtentFn`.
    Some(std::mem::transmute::<usize, ExtentFn>(super::callable::entry_of(leaf)))
}

/// `None` where there is no record to read: no head yet (a type still being defined) or a
/// node that is no type. A type is typed by the self-typed root, or by a type whose values are
/// types, as `logos` is by `language`. Every accessor below contracts on `Some`, so this is
/// where a node that is not an identity is turned away.
///
/// # Safety
/// `id` must be a valid dyad from the store.
pub(crate) unsafe fn kind_of(id: DyadPtr) -> Option<u8> {
    let v = dyad::head(id);
    let t = dyad::ty(id);
    if v.is_null() || (dyad::ty(t) != t && kind_of(t) != Some(TYPEREC_TAG)) {
        None
    } else {
        Some(*(v as *const u8))
    }
}

/// Safe on any node: `kind_of` turns away a node with no record and one that is no type.
///
/// # Safety
/// `id` must be null or a valid dyad from the store.
pub(crate) unsafe fn is_record_type(id: DyadPtr) -> bool {
    !id.is_null() && kind_of(id) == Some(RECORD_TAG)
}

/// A record type whose values a Logos-written `parse` builds and no `run` executes: a value
/// of it is the node's address, eight bytes in a place. DESIGN ›A value of a type built by a
/// Logos `parse` travels as a pointer‹.
///
/// # Safety
/// `id` must be null or a valid dyad from the store; `fn_type` the `fn` identity.
pub(crate) unsafe fn is_node_valued(id: DyadPtr, fn_type: DyadPtr) -> bool {
    if !is_record_type(id) || !run_body_of(id).is_null() {
        return false;
    }
    let ctor = constructor_of(id);
    !ctor.is_null() && dyad::ty(ctor) == fn_type
}

/// Whether a value of `id` is its node's address, eight bytes in a place: the type's definition
/// says so, or its values are nodes a Logos `parse` builds (`is_node_valued`). Safe on any node,
/// as `kind_of` is.
///
/// # Safety
/// `id` must be null or a valid dyad from the store; `fn_type` the `fn` identity.
pub(crate) unsafe fn value_is_node(id: DyadPtr, fn_type: DyadPtr) -> bool {
    if id.is_null() || kind_of(id).is_none() {
        return false;
    }
    *dyad::head(id).add(NODE_VALUE_OFF) != 0 || is_node_valued(id, fn_type)
}

/// Where a node of a type carries the `own` mark a type's `run` gets at its close: what the node
/// gives is made for whoever takes it (›Three fail-closed ownership rules, and `-> own @T`‹
/// [[R62]]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnMark {
    Nowhere,
    /// On the type's record, for every node.
    Every,
    /// In this slot of each node: the `own` word, or null.
    Slot(usize),
}

/// # Safety
/// `id` must carry a record.
pub(crate) unsafe fn mark_own(id: DyadPtr) {
    *dyad::head(id).add(OWN_OFF) = EVERY_NODE;
}

/// Safe on any node, as `kind_of` is.
///
/// # Safety
/// `id` must be null or a valid dyad from the store.
pub(crate) unsafe fn own_mark_of(id: DyadPtr) -> OwnMark {
    if id.is_null() || kind_of(id).is_none() {
        return OwnMark::Nowhere;
    }
    match *dyad::head(id).add(OWN_OFF) {
        NO_SLOT => OwnMark::Nowhere,
        EVERY_NODE => OwnMark::Every,
        slot => OwnMark::Slot(slot as usize),
    }
}

/// Says that a node of `id` hands on as its value what its operands `roles` give, every one
/// alike, and of the lines its operands `lines` hold, those that give it
/// ([`super::flow::giving_lines`]).
///
/// # Safety
/// `id` must carry an operand record whose roles name each of `roles` and `lines`.
pub(crate) unsafe fn install_hands_on(id: DyadPtr, roles: &[&str], lines: &[&str]) {
    let bits = |names: &[&str]| {
        names.iter().fold(0u8, |bits, name| {
            let slot = (0..arity_of(id))
                .find(|&i| crate::reflect::text_of(role_of(id, i)) == name.as_bytes())
                .expect("the record names each role it hands on");
            debug_assert!(slot < 8, "one bit per slot");
            bits | 1 << slot
        })
    };
    *dyad::head(id).add(HANDS_ON_OFF) = bits(roles);
    *dyad::head(id).add(HANDS_ON_LINES_OFF) = bits(lines);
}

/// The slots of a node of `id` whose operands it hands on as its value, one bit each, as
/// `(nodes, lines)`: those whose node it hands on, and those whose lines it hands on the giving
/// ones of.
/// `(0, 0)` where it gives its own value. Safe on any node, as `kind_of` is.
///
/// # Safety
/// `id` must be null or a valid dyad from the store.
pub(crate) unsafe fn hands_on_of(id: DyadPtr) -> (u8, u8) {
    if id.is_null() || kind_of(id).is_none() {
        return (0, 0);
    }
    (*dyad::head(id).add(HANDS_ON_OFF), *dyad::head(id).add(HANDS_ON_LINES_OFF))
}

const NO_SLOT: u8 = u8::MAX;
const EVERY_NODE: u8 = u8::MAX - 1;
const OP_SLOT_OFF: usize = PAYLOAD_OFF + 1;
const OUTPUT_SLOT_OFF: usize = PAYLOAD_OFF + 2;
const ROLES_OFF: usize = PAYLOAD_OFF + 3;

/// The slot named `op`, where a resolved node stores its callable leaf; `None` for kinds
/// without fixed slots and for a record that names none.
///
/// # Safety
/// `id` must be a valid dyad from the store whose non-null value is a record.
pub(crate) unsafe fn op_slot_of(id: DyadPtr) -> Option<usize> {
    named_slot(id, OP_SLOT_OFF)
}

/// The slot named `output_type`, where the parse writes the type the node gives back.
/// ›A mint's output type is written in its `static output_type` declaration; a type that does not
/// mint has its parse write it into each node‹ [[R194]].
///
/// # Safety
/// As `op_slot_of`.
pub(crate) unsafe fn output_slot_of(id: DyadPtr) -> Option<usize> {
    named_slot(id, OUTPUT_SLOT_OFF)
}

unsafe fn named_slot(id: DyadPtr, at: usize) -> Option<usize> {
    match kind_of(id) {
        Some(TUPLE_TAG | LIST_TAG) => {
            let slot = *dyad::head(id).add(at);
            (slot != NO_SLOT).then_some(slot as usize)
        }
        _ => None,
    }
}

/// # Safety
/// `id` must carry a record (`kind_of` is `Some`).
pub(crate) unsafe fn parse_rank_of(id: DyadPtr) -> f64 {
    let v = dyad::head(id);
    f64::from_ne_bytes(std::ptr::read_unaligned(v.add(PREC_OFF) as *const [u8; 8]))
}

/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn assoc_of(id: DyadPtr) -> Assoc {
    if *dyad::head(id).add(ASSOC_OFF) == 0 {
        Assoc::Left
    } else {
        Assoc::Right
    }
}

/// # Safety
/// `id` must carry an operand record (a `TUPLE_TAG` or `LIST_TAG` kind) or a token's.
pub(crate) unsafe fn arity_of(id: DyadPtr) -> usize {
    *dyad::head(id).add(PAYLOAD_OFF) as usize
}

/// # Safety
/// As `arity_of`, with `i < arity_of(id)`.
pub(crate) unsafe fn role_of(id: DyadPtr, i: usize) -> DyadPtr {
    let p = dyad::head(id).add(ROLES_OFF + i * std::mem::size_of::<DyadPtr>());
    std::ptr::read_unaligned(p as *const DyadPtr)
}

/// Whether `id` reads what stands to its left: an operator whose first operand is `lhs`, or a
/// token whose first side is.
///
/// # Safety
/// `id` must be a non-null dyad from the store; `kind_of` turns away a node with no record.
pub(crate) unsafe fn takes_lhs(id: DyadPtr) -> bool {
    matches!(kind_of(id), Some(TUPLE_TAG | LIST_TAG | TOKEN_TAG))
        && arity_of(id) > 0
        && crate::reflect::text_of(role_of(id, 0)) == b"lhs"
}
