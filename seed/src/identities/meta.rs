// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The shared-member record every core identity carries in its value slot — the
//! seed's realization of the sealed `type` model (DESIGN ›A type's metadata is
//! shared by its values‹, issue #30) and of layout-as-graph-data (issue #42).
//!
//! Anything that stands in a node's `ty` position stores, once, the members its
//! values share: its parse `precedence` and `associativity`, its `constructor`
//! (a native callable leaf; see below) and `destructor` (null: the honest
//! undefined until drop semantics exist), and the *layout* that says how a
//! value of it is read — a scalar width, a text or fraction blob, a pointer's
//! pointee, or the arity and role names of an application's operands. A
//! generic walker ([`crate::reflect`]) reads any node's structure from these
//! records alone, and the parser dispatches from them too: the driver
//! classifies a token from constructor presence, the precedence field (NaN
//! never-extends / finite infix / +inf tight extender), and the kind byte —
//! the constructors drive the tape (DESIGN ›Source becomes runnable›), and
//! the schedule byte that stood in until they did is gone. The constructor is
//! a `seed-parse` callable (one [`crate::parse::ConstructFn`] signature)
//! whose body stays `native` until self-hosting ports it to Logos source (the
//! #42 boundary). Only `run`'s lowering table remains Rust-side, awaiting
//! per-backend keying.
//!
//! Record layout (unaligned, native-endian, byte offsets):
//!
//! ```text
//! [0]        u8   kind — the type-tag namespace (see below)
//! [1]        u8   associativity (0 left-to-right, 1 right-to-left)
//! [2..10]    f64  precedence — NaN: never extends left; finite: infix;
//!                 +inf: tight extender (call `(`, postfix `.`/`@`)
//! [10..18]   u64  constructor — a callable leaf (`seed-parse` convention,
//!                 one `ConstructFn` signature), or 0: undefined
//! [18..26]   u64  destructor  (0: undefined until drop semantics exist)
//! [26..]     payload, per kind:
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
use crate::parse::{Assoc};
use crate::store::Store;

use super::numtype::ADDR_TAG;
use super::{string, Cx};

/// Kind: values are applications of `arity` fixed `dyad@` operand slots, each
/// named by a role string (a null slot is an absent optional, like an else-less
/// `if`'s third operand). Also the shape of an fn value's
/// `[input, output, body, bcode]`.
pub(crate) const TUPLE_TAG: u8 = 14;
/// Kind: values are `arity` fixed named `dyad@` slots followed by a
/// null-terminated variadic tail — a sequence (`arity` 0), a construction
/// (`[instance, arg…, null]`). (A struct *definition*'s value is a
/// [`STRUCT_TAG`] record, not a list.)
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
/// Kind: a struct *type* node's record (issue #47) — the stored layout the
/// constructor derives at definition (DESIGN ›a type whose constructor derives
/// the layout automatically — reading the field declarations in its scope and
/// filling `fields` and `size_bytes`‹), locked before first instantiation. The
/// payload is `[scope: @dyad][fields: @dyad (an array node over the field
/// declarations)][size_bytes: u64]`, 24 bytes. Giving struct types a real
/// record also makes their first value byte an honest kind tag — before this,
/// it was a node address's low byte, and any tag read on it was garbage.
pub(crate) const STRUCT_TAG: u8 = 23;

/// Byte offset of the associativity in a record.
const ASSOC_OFF: usize = 1;
/// Byte offset of the precedence.
const PREC_OFF: usize = 2;
/// Byte offset of the reserved constructor slot.
const CTOR_OFF: usize = 10;
/// Byte offset of the reserved destructor slot.
const DTOR_OFF: usize = 18;
/// Byte offset of the kind-specific payload (a pointer type's pointee, or an
/// operand record's arity + roles).
pub(crate) const PAYLOAD_OFF: usize = 26;

/// Build a plain record: `kind` and `precedence`, no payload. The scalar
/// types, the text substance, the foundations, and the parse-only tokens.
/// `precedence` is the extender signal the driver classifies by: NaN for a
/// token that never extends an expression to its left, `+inf` for a tight
/// extender (`(` as call, postfix `.`/`@`), finite only on the infix
/// operators (which carry operand records instead).
pub(crate) fn record(store: &mut Store, kind: u8, precedence: f64) -> *mut u8 {
    let blob = header(kind, Assoc::Left, precedence);
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
    roles: &[&str],
) -> *mut u8 {
    debug_assert!(matches!(kind, TUPLE_TAG | LIST_TAG), "operand records carry operand kinds");
    debug_assert!(!cx.string_.is_null(), "role names need the string type registered");
    let mut blob = header(kind, assoc, precedence).to_vec();
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
    let mut blob = header(ADDR_TAG, Assoc::Left, f64::NAN).to_vec();
    blob.extend_from_slice(&(pointee as usize).to_ne_bytes());
    store.alloc_bytes(&blob)
}

/// Build a struct type node's record (issue #47): the [`STRUCT_TAG`] head and
/// the stored layout — the field-list scope, the `fields` array node over the
/// field declarations, and the derived `size_bytes` — filled at definition,
/// where the type's layout locks (DESIGN ›A type's layout-relevant slots must
/// be defined and frozen before its first instantiation‹).
pub(crate) fn struct_record(
    store: &mut Store,
    scope: DyadPtr,
    fields: DyadPtr,
    size_bytes: u64,
) -> *mut u8 {
    let mut blob = header(STRUCT_TAG, Assoc::Left, f64::NAN).to_vec();
    blob.extend_from_slice(&(scope as usize).to_ne_bytes());
    blob.extend_from_slice(&(fields as usize).to_ne_bytes());
    blob.extend_from_slice(&size_bytes.to_ne_bytes());
    store.alloc_bytes(&blob)
}

/// The stored scope of a struct type node — where its field names are declared.
///
/// # Safety
/// `id` must carry a [`STRUCT_TAG`] record ([`struct_record`]).
pub(crate) unsafe fn struct_scope_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(PAYLOAD_OFF) as *const DyadPtr)
}

/// The stored `fields` array node of a struct type node — the field
/// declarations, in order, behind one indirection (the system's first
/// element-typed array in spirit: an array of `dyad`).
///
/// # Safety
/// As [`struct_scope_of`].
pub(crate) unsafe fn struct_fields_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(PAYLOAD_OFF + 8) as *const DyadPtr)
}

/// The stored `size_bytes` of a struct type node — the packed byte size its
/// instances occupy, derived at definition.
///
/// # Safety
/// As [`struct_scope_of`].
pub(crate) unsafe fn struct_size_of(id: DyadPtr) -> u64 {
    std::ptr::read_unaligned((*id).value.add(PAYLOAD_OFF + 16) as *const u64)
}

/// The fixed head of every record: kind, associativity, precedence, and the
/// two reserved slots. Precedence doubles as the extender signal: NaN (never
/// extends left) / finite (infix, shift-reduce) / +inf (tight extender,
/// constructor invoked immediately over its left).
fn header(kind: u8, assoc: Assoc, precedence: f64) -> [u8; PAYLOAD_OFF] {
    let mut h = [0u8; PAYLOAD_OFF];
    h[0] = kind;
    h[ASSOC_OFF] = match assoc {
        Assoc::Left => 0,
        Assoc::Right => 1,
    };
    h[PREC_OFF..CTOR_OFF].copy_from_slice(&precedence.to_ne_bytes());
    // CTOR_OFF..DTOR_OFF and DTOR_OFF..PAYLOAD_OFF stay zero: reserved.
    let _ = DTOR_OFF;
    h
}

/// The constructor stored in `id`'s record: a callable leaf under the
/// `seed-parse` convention (its entry a Rust shim whose signature the schedule
/// byte selects), or null — the *undefined* constructor of a pure delimiter
/// token or a data type, whose parse role is scheduling alone.
///
/// # Safety
/// As [`precedence_of`].
pub(crate) unsafe fn constructor_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(CTOR_OFF) as *const DyadPtr)
}

/// The destructor stored in `id`'s record — null for every seed identity: the
/// honest *undefined*, filled the day drop semantics exist, never faked with a
/// no-op.
///
/// # Safety
/// As [`precedence_of`].
pub(crate) unsafe fn destructor_of(id: DyadPtr) -> DyadPtr {
    std::ptr::read_unaligned((*id).value.add(DTOR_OFF) as *const DyadPtr)
}

/// Install `leaf` (a callable value) as `id`'s constructor — the registration
/// back-fill's writer, run once per identity while the record is still under
/// construction; nothing has read the slot before the fill.
///
/// # Safety
/// `id` must carry a record and `leaf` must be a callable leaf whose entry
/// matches `id`'s schedule byte.
pub(crate) unsafe fn install_constructor(id: DyadPtr, leaf: DyadPtr) {
    std::ptr::write_unaligned((*id).value.add(CTOR_OFF) as *mut DyadPtr, leaf);
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
