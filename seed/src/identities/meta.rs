// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The shared-member record every core identity carries in its value slot — the
//! seed's realization of the sealed `type` model (DESIGN ›A type's metadata is
//! shared by its values‹, issue #30) and of layout-as-graph-data (issue #42).
//!
//! Anything that stands in a node's `ty` position stores, once, the members its
//! values share: its parse `precedence` and `associativity`, slots for its
//! `constructor` and `destructor`, and the *layout* that says how a value of it
//! is read — a scalar width, a text or fraction blob, a pointer's pointee, or
//! the arity and role names of an application's operands. A generic walker
//! ([`crate::reflect`]) reads any node's structure from these records alone; the
//! native `run`/`lower`/`Construct` *logic* stays in the per-phase Rust tables
//! (the #42 boundary: structure becomes data now, behaviour is ported one
//! identity at a time at self-hosting), which is why the constructor and
//! destructor slots are reserved zeroes for the seed.
//!
//! Record layout (unaligned, native-endian, byte offsets):
//!
//! ```text
//! [0]        u8   kind — the type-tag namespace (see below)
//! [1]        u8   associativity (0 left-to-right, 1 right-to-left)
//! [2..10]    f64  precedence
//! [10..18]   u64  constructor (reserved 0; the native Construct lives in `metas`)
//! [18..26]   u64  destructor  (reserved 0)
//! [26]       u8   schedule — how the driver schedules the token (see
//!                 [`crate::parse::Schedule`]; 0 = plain operand)
//! [27..]     payload, per kind:
//!              ADDR              pointee type node (`dyad@`)
//!              TUPLE/LIST         u8 arity, then arity × `dyad@` role-name strings
//! ```
//!
//! The kind byte continues [`numtype`](super::numtype)'s tag space (`NumType`
//! 0–9, `VOID_TAG` 10, `STRING_TAG` 11, `COMMENT_TAG` 12, `ADDR_TAG` 13), so
//! every existing first-byte tag read keeps working unchanged. Since #44 the
//! only fn-typed nodes are real functions (operators are plain types), so no
//! discriminant is needed to tell them apart — the record *is* the identity's
//! whole value, and the code its applications run lives on the callable
//! leaves their op slots reference.

use crate::dyad::DyadPtr;
use crate::parse::{Assoc, Schedule};
use crate::store::Store;

use super::numtype::ADDR_TAG;
use super::{string, Cx};

/// Kind: values are applications of `arity` fixed `dyad@` operand slots, each
/// named by a role string (a null slot is an absent optional, like an else-less
/// `if`'s third operand). Also the shape of an fn value's
/// `[input, output, body, bcode]`.
pub(crate) const TUPLE_TAG: u8 = 14;
/// Kind: values are `arity` fixed named `dyad@` slots followed by a
/// null-terminated variadic tail — a sequence (`arity` 0), a struct definition
/// (`[scope, field…, null]`), a construction (`[instance, arg…, null]`).
pub(crate) const LIST_TAG: u8 = 15;
/// Kind: values are `[num: i64, den: i64]` comptime fractions (`rational`).
pub(crate) const FRACTION_TAG: u8 = 16;
/// Kind: values are themselves types — each carries a record like this one. The
/// `Type : Type` root's kind, where the recursion grounds.
pub(crate) const TYPEREC_TAG: u8 = 18;
/// Kind: a parse-only token (`,`, `(`, `->`, `else`, …); no values exist.
pub(crate) const TOKEN_TAG: u8 = 19;
/// Kind: values are the complete jump information — `[entry: @exec, convention]`,
/// 16 bytes. The `callable` type's kind (DESIGN ›The callable ground is `@exec`‹,
/// issue #44): every exec leaf (`add_i32`, `if_native`, a compiled fn's code) is a
/// value of it, and jumping consumes exactly this record.
pub(crate) const CALLABLE_TAG: u8 = 20;
/// Kind: values are calling-convention identities (declared metadata a backend
/// renders per target; decisive at the FFI boundary). A convention value's slot
/// holds its name string node.
pub(crate) const CONVENTION_TAG: u8 = 21;
/// Kind: values are arrays of `dyad@` — `[len: u64][data: @dyad]`, 16 bytes,
/// the list itself behind one indirection (settled: a growable thing never
/// lives inline in a node's value). The seed's first array form; element-typed
/// arrays and surface syntax arrive with the `array` type proper.
pub(crate) const ARRAY_TAG: u8 = 22;

/// Byte offset of the associativity in a record.
const ASSOC_OFF: usize = 1;
/// Byte offset of the precedence.
const PREC_OFF: usize = 2;
/// Byte offset of the reserved constructor slot.
const CTOR_OFF: usize = 10;
/// Byte offset of the reserved destructor slot.
const DTOR_OFF: usize = 18;
/// Byte offset of the schedule byte.
const SCHED_OFF: usize = 26;
/// Byte offset of the kind-specific payload (a pointer type's pointee, or an
/// operand record's arity + roles).
pub(crate) const PAYLOAD_OFF: usize = 27;

/// Build a plain record: `kind` and `schedule`, no precedence, no payload. The
/// scalar types, the text substance, the foundations, and the parse-only tokens
/// (whose schedule is their whole parse role).
pub(crate) fn record(store: &mut Store, kind: u8, schedule: Schedule) -> *mut u8 {
    let blob = header(kind, Assoc::Left, 0.0, schedule);
    store.alloc_bytes(&blob)
}

/// Build an operand record for an operator/statement identity: its layout
/// `kind` ([`TUPLE_TAG`] or [`LIST_TAG`]), its parse
/// `precedence`/`assoc`/`schedule`, and one role-name string node per operand
/// slot.
pub(crate) fn operand_record(
    cx: &mut Cx,
    kind: u8,
    precedence: f64,
    assoc: Assoc,
    schedule: Schedule,
    roles: &[&str],
) -> *mut u8 {
    debug_assert!(matches!(kind, TUPLE_TAG | LIST_TAG), "operand records carry operand kinds");
    debug_assert!(!cx.string_.is_null(), "role names need the string type registered");
    let mut blob = header(kind, assoc, precedence, schedule).to_vec();
    blob.push(roles.len() as u8);
    for role in roles {
        let name = string::build_text(cx.store, cx.string_, role.as_bytes());
        blob.extend_from_slice(&(name as usize).to_ne_bytes());
    }
    cx.store.alloc_bytes(&blob)
}

/// Build a pointer type's record: kind [`ADDR_TAG`], the pointee node as the
/// payload. Pointer types are created fresh per use and carry no parse members.
pub(crate) fn pointer_record(store: &mut Store, pointee: DyadPtr) -> *mut u8 {
    let mut blob = header(ADDR_TAG, Assoc::Left, 0.0, Schedule::Operand).to_vec();
    blob.extend_from_slice(&(pointee as usize).to_ne_bytes());
    store.alloc_bytes(&blob)
}

/// The fixed head of every record: kind, associativity, precedence, the two
/// reserved slots, and the schedule byte.
fn header(kind: u8, assoc: Assoc, precedence: f64, schedule: Schedule) -> [u8; PAYLOAD_OFF] {
    let mut h = [0u8; PAYLOAD_OFF];
    h[0] = kind;
    h[ASSOC_OFF] = match assoc {
        Assoc::Left => 0,
        Assoc::Right => 1,
    };
    h[PREC_OFF..CTOR_OFF].copy_from_slice(&precedence.to_ne_bytes());
    // CTOR_OFF..DTOR_OFF and DTOR_OFF..SCHED_OFF stay zero: reserved.
    let _ = DTOR_OFF;
    h[SCHED_OFF] = schedule as u8;
    h
}

/// The parse schedule stored in `id`'s record.
///
/// # Safety
/// As [`precedence_of`].
pub(crate) unsafe fn schedule_of(id: DyadPtr) -> Schedule {
    Schedule::from_tag(*(*id).value.add(SCHED_OFF))
}

/// The record kind of `id`, or `None` for a null value slot (a still-unbound
/// declaration placeholder).
///
/// # Safety
/// `id` must be a valid dyad from the store whose non-null value is a record.
pub(crate) unsafe fn kind_of(id: DyadPtr) -> Option<u8> {
    let v = (*id).value;
    if v.is_null() {
        None
    } else {
        Some(*(v as *const u8))
    }
}

/// The index of a runnable node's *op slot* — the last fixed slot of its
/// type's operand record, where a resolved node stores its callable leaf
/// (issue #44: dispatch flows through the node, not the identity). `None` for
/// kinds without fixed slots (any data type).
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

/// The parse precedence stored in `id`'s record.
///
/// # Safety
/// `id` must carry a record ([`kind_of`] is `Some`).
pub(crate) unsafe fn precedence_of(id: DyadPtr) -> f64 {
    let v = (*id).value;
    f64::from_ne_bytes(std::ptr::read_unaligned(v.add(PREC_OFF) as *const [u8; 8]))
}

/// The associativity stored in `id`'s record.
///
/// # Safety
/// As [`precedence_of`].
pub(crate) unsafe fn assoc_of(id: DyadPtr) -> Assoc {
    if *(*id).value.add(ASSOC_OFF) == 0 {
        Assoc::Left
    } else {
        Assoc::Right
    }
}

/// The operand arity stored in `id`'s operand record.
///
/// # Safety
/// `id` must carry an operand record (a [`TUPLE_TAG`] or [`LIST_TAG`] kind).
pub(crate) unsafe fn arity_of(id: DyadPtr) -> usize {
    *(*id).value.add(PAYLOAD_OFF) as usize
}

/// The `i`-th operand's role-name string node in `id`'s operand record.
///
/// # Safety
/// As [`arity_of`], with `i < arity_of(id)`.
pub(crate) unsafe fn role_of(id: DyadPtr, i: usize) -> DyadPtr {
    let p = (*id).value.add(PAYLOAD_OFF + 1 + i * std::mem::size_of::<DyadPtr>());
    std::ptr::read_unaligned(p as *const DyadPtr)
}
