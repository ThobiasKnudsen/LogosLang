// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The shared-member record every identity carries in its value slot: parse rank
//! and associativity, the constructor and destructor leaves, the code and run-body
//! slots, the interned pointer type, and the layout its values are read by.
//!
//! Record layout (unaligned, native-endian, byte offsets):
//!
//! ```text
//! [0]        u8   kind: the type-tag namespace (see below)
//! [1]        u8   associativity (0 left-to-right, 1 right-to-left)
//! [2..10]    f64  parse_rank
//! [10..18]   u64  constructor: a `seed-parse` callable leaf, or 0
//! [18..26]   u64  destructor: the owning pointer's teardown, else 0
//! [26..34]   u64  code: the `fn` node a node of the type runs as, or 0
//! [34..42]   u64  run body: the lexed body a `run = (…)` line held, or 0
//! [42..50]   u64  pointer type: the interned `@T` of this type, or 0
//! [50..]     payload, per kind:
//!              ADDR              pointee type node (`dyad@`)
//!              TUPLE/LIST         u8 arity, then arity × `dyad@` role-name strings
//! ```
//!
//! The kind byte continues `numtype`'s tag space (`NumType` 0–9, `VOID_TAG` 10,
//! `STRING_TAG` 11, `COMMENT_TAG` 12, `ADDR_TAG` 13).

use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::store::Store;

use super::numtype::ADDR_TAG;
use super::{string, Cx};

/// Values are `arity` fixed `dyad@` operand slots, each named by a role string; a
/// null slot is an absent optional. Also an fn value's `[input, output_type, body, bcode]`.
pub(crate) const TUPLE_TAG: u8 = 14;
/// Values are `arity` fixed named `dyad@` slots, then a null-terminated variadic tail.
pub(crate) const LIST_TAG: u8 = 15;
/// Values are `[num: i64, den: i64]` comptime fractions.
pub(crate) const FRACTION_TAG: u8 = 16;
/// Values are themselves types, each carrying a record like this one; the root's kind.
pub(crate) const TYPEREC_TAG: u8 = 18;
/// A parse-only token; no values exist.
pub(crate) const TOKEN_TAG: u8 = 19;
/// Values are `[entry: @exec, convention]`, 16 bytes; every exec leaf is one.
pub(crate) const CALLABLE_TAG: u8 = 20;
/// Values are calling-convention identities; the value slot holds the name string node.
pub(crate) const CONVENTION_TAG: u8 = 21;
/// Values are `[len: u64][data: @dyad]`, 16 bytes; a list never lives inline in a node.
pub(crate) const ARRAY_TAG: u8 = 22;
/// A record type node's record: payload `[scope: @dyad][fields: @dyad][size_bytes: u64]
/// [body: @dyad][drop: @dyad]`, 40 bytes, locked at definition; `body` is null where the type
/// has none, `drop` where its body fills none.
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
pub(crate) const PAYLOAD_OFF: usize = 42;

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
    /// Application and juxtaposition (`f(x)`, `i32 5`): reads its bracket lazily in source
    /// order, so the tight reads sit above it and the bracket readers above those.
    pub const APPLY: f64 = 91.0;
    /// `?`: just below application, so `i32 ?` finds its type standing to the left.
    pub const HOLE: f64 = 87.0;
    /// `.`, `:`, `@`: the tight reads of the cell to their left, above the readers and
    /// below `:=`/`=`; the right cell is lexed lazily inside the constructor.
    pub const TIGHT: f64 = 92.5;
    /// `&`.
    pub const ADDRESS: f64 = 85.0;
    /// The prefix words over a place: `own`, `drop`, `free`, `alloc`, `pub`.
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

/// Right for a prefix word, so that in a chain the rightmost runs first and each finds
/// its operand constructed.
pub(crate) fn record_assoc(store: &mut Store, kind: u8, parse_rank: f64, assoc: Assoc) -> *mut u8 {
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
    debug_assert!(!cx.string_.is_null(), "role names need the string logos registered");
    let mut blob = header(kind, assoc, parse_rank).to_vec();
    blob.push(roles.len() as u8);
    for role in roles {
        let name = string::build_text(cx.store, cx.string_, role.as_bytes());
        blob.extend_from_slice(&(name as usize).to_ne_bytes());
    }
    cx.store.alloc_bytes(&blob)
}

pub(crate) fn pointer_record(store: &mut Store, pointee: DyadPtr) -> *mut u8 {
    let mut blob = header(ADDR_TAG, Assoc::Left, prec::INERT).to_vec();
    blob.extend_from_slice(&(pointee as usize).to_ne_bytes());
    store.alloc_bytes(&blob)
}

pub(crate) fn record_layout(
    store: &mut Store,
    scope: DyadPtr,
    fields: DyadPtr,
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
    store.alloc_bytes(&blob)
}

/// Where a record type's field names are declared.
///
/// # Safety
/// `id` must carry a `RECORD_TAG` record.
pub(crate) unsafe fn record_scope_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(PAYLOAD_OFF) as *const DyadPtr)
}

/// # Safety
/// As `record_scope_of`.
pub(crate) unsafe fn record_fields_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(PAYLOAD_OFF + 8) as *const DyadPtr)
}

/// # Safety
/// As `record_scope_of`.
pub(crate) unsafe fn record_size_of(id: DyadPtr) -> u64 {
    std::ptr::read_unaligned((*id).value.add(PAYLOAD_OFF + 16) as *const u64)
}

/// The definition body's scope, or null where the type has no body.
///
/// # Safety
/// As `record_scope_of`.
pub(crate) unsafe fn record_body_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(PAYLOAD_OFF + 24) as *const DyadPtr)
}

const INSTANCES_DROP_OFF: usize = PAYLOAD_OFF + 32;

/// The `fn` a `drop = (…)` line filled, over the one parameter `this`; null where the type
/// body fills none.
///
/// # Safety
/// As `record_scope_of`.
pub(crate) unsafe fn instances_drop_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(INSTANCES_DROP_OFF) as *const DyadPtr)
}

/// # Safety
/// `id` must carry a `RECORD_TAG` record; `drop` must be null or a `fn` node from the store.
pub(crate) unsafe fn install_instances_drop(id: DyadPtr, drop: DyadPtr) {
    std::ptr::write_unaligned((*id).value.add(INSTANCES_DROP_OFF) as *mut DyadPtr, drop);
}

fn header(kind: u8, assoc: Assoc, parse_rank: f64) -> [u8; PAYLOAD_OFF] {
    let mut h = [0u8; PAYLOAD_OFF];
    h[0] = kind;
    h[ASSOC_OFF] = match assoc {
        Assoc::Left => 0,
        Assoc::Right => 1,
    };
    h[PREC_OFF..CTOR_OFF].copy_from_slice(&parse_rank.to_ne_bytes());
    // The slots from CTOR_OFF on stay zero until their writers fill them.
    h
}

/// Null is the undefined constructor of a delimiter token or a data type, inert on the tape.
///
/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn constructor_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(CTOR_OFF) as *const DyadPtr)
}

/// Null on every identity but an owning pointer type; never faked with a no-op.
///
/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn destructor_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(DTOR_OFF) as *const DyadPtr)
}

/// # Safety
/// `id` must carry a record and `leaf` must be a callable leaf whose entry is a `ConstructFn`.
pub(crate) unsafe fn install_constructor(id: DyadPtr, leaf: DyadPtr) {
    std::ptr::write_unaligned((*id).value.add(CTOR_OFF) as *mut DyadPtr, leaf);
}

/// # Safety
/// `id` must carry a record and `leaf` must be a callable leaf whose entry runs the
/// identity's teardown.
pub(crate) unsafe fn install_destructor(id: DyadPtr, leaf: DyadPtr) {
    std::ptr::write_unaligned((*id).value.add(DTOR_OFF) as *mut DyadPtr, leaf);
}

/// The lexed body a `run = (…)` line held, constructed per field-type set;
/// null for a type with no run.
///
/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn run_body_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(RUN_BODY_OFF) as *const DyadPtr)
}

/// # Safety
/// `id` must carry a record and `body` must be a run-body node from the store.
pub(crate) unsafe fn install_run_body(id: DyadPtr, body: DyadPtr) {
    std::ptr::write_unaligned((*id).value.add(RUN_BODY_OFF) as *mut DyadPtr, body);
}

/// The interned `@T` of `id`, or null while none has been minted.
///
/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn pointer_type_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(POINTER_TYPE_OFF) as *const DyadPtr)
}

/// # Safety
/// `id` must carry a record and `p` must be the plain pointer type node over `id`.
pub(crate) unsafe fn install_pointer_type(id: DyadPtr, p: DyadPtr) {
    std::ptr::write_unaligned((*id).value.add(POINTER_TYPE_OFF) as *mut DyadPtr, p);
}

/// `None` where there is no record to read: a null value (an unbound placeholder) or
/// a tagged place. Every accessor below contracts on `Some`, so this is where a node
/// that is not an identity is turned away.
///
/// # Safety
/// `id` must be a valid dyad from the store.
pub(crate) unsafe fn kind_of(id: DyadPtr) -> Option<u8> {
    let v = (*id).value;
    if v.is_null() || crate::dyad::is_place(v) {
        None
    } else {
        Some(*(v as *const u8))
    }
}

/// Safe on any node: `kind_of` turns away both a null value and a type-valued place.
///
/// # Safety
/// `id` must be null or a valid dyad from the store.
pub(crate) unsafe fn is_record_type(id: DyadPtr) -> bool {
    !id.is_null()
        && !(*id).ty.is_null()
        && (*id).ty == (*(*id).ty).ty
        && kind_of(id) == Some(RECORD_TAG)
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
    !ctor.is_null() && (*ctor).ty == fn_type
}

/// The last fixed slot of the type's operand record, where a resolved node stores its
/// callable leaf; `None` for kinds without fixed slots.
///
/// # Safety
/// `id` must be a valid dyad from the store whose non-null value is a record.
pub(crate) unsafe fn op_slot_of(id: DyadPtr) -> Option<usize> {
    match kind_of(id) {
        Some(TUPLE_TAG | LIST_TAG) => {
            let arity = arity_of(id);
            arity.checked_sub(1)
        }
        _ => None,
    }
}

/// # Safety
/// `id` must carry a record (`kind_of` is `Some`).
pub(crate) unsafe fn parse_rank_of(id: DyadPtr) -> f64 {
    let v = (*id).value;
    f64::from_ne_bytes(std::ptr::read_unaligned(v.add(PREC_OFF) as *const [u8; 8]))
}

/// # Safety
/// As `parse_rank_of`.
pub(crate) unsafe fn assoc_of(id: DyadPtr) -> Assoc {
    if *(*id).value.add(ASSOC_OFF) == 0 {
        Assoc::Left
    } else {
        Assoc::Right
    }
}

/// # Safety
/// `id` must carry an operand record (a `TUPLE_TAG` or `LIST_TAG` kind).
pub(crate) unsafe fn arity_of(id: DyadPtr) -> usize {
    *(*id).value.add(PAYLOAD_OFF) as usize
}

/// # Safety
/// As `arity_of`, with `i < arity_of(id)`.
pub(crate) unsafe fn role_of(id: DyadPtr, i: usize) -> DyadPtr {
    let p = (*id).value.add(PAYLOAD_OFF + 1 + i * std::mem::size_of::<DyadPtr>());
    std::ptr::read_unaligned(p as *const DyadPtr)
}
