// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A `record`: one per declared name, the trie entry itself — the dyad the name
//! denotes, the scope it was declared in, its range of life inside that scope,
//! and its gate set (DESIGN ›`mut` is a gate on the record‹ and ›Name
//! resolution is scope-filtered‹, ruled 7 September 2026; `id_context` renamed
//! `record` the same day). Not the *shared-member record* a type value
//! holds (`meta`): that is a type's metadata, this is a name's.
//!
//! A single spelling can denote different identities in different scopes, so the
//! name index stores a *list* of `record`s per spelling (see [`crate::regex_trie`]).
//! Resolution keeps the candidate whose `scope` is currently open *and* whose
//! range covers the point of use, and, because shadowing is disallowed (a name
//! may not be redeclared while another declaration of it is live), exactly one
//! candidate survives, or none for a genuine out-of-scope use. Two survivors is
//! impossible under that rule and signals a corrupt index (see DESIGN ›Name
//! resolution is scope-filtered‹).
//!
//! One record per *name*, never one per identity: `x := i32` binds a second
//! name to i32's own dyad, and a record shared through the dyad would have let
//! `x := pub i32` gate i32 itself. Per-name records keep `x:gate` x's and
//! `i32:gate` i32's, with both `:dyad` at one cell.
//!
//! The `scope` stores the *enclosing scope* rather than the declaration node
//! because a dyad has no parent pointer: keying by scope makes membership an
//! O(1) test against the set of open scopes during elaboration.
//!
//! The range (DESIGN ›Name resolution is scope-filtered‹, ruled 3 September
//! 2026): `start` is the item of the declaring scope's body that declares the
//! name, and `end` the item holding the `own` or `drop` that makes it dead —
//! null while the name is alive, i.e. to the scope's end. While that item is
//! still being parsed `end` holds the `own`/`drop` node itself, so a later use
//! in the same line already fails; the parser settles both to the body item
//! once the item is complete. A dead entry stays indexed: the range is what
//! reflection reads, and a fresh declaration of the same spelling in the same
//! scope sits beside it. During elaboration the point of use is always the
//! frontier, so liveness reduces to `end` being null; the position comparison
//! is for resolution from a later context, which nothing performs yet.
//!
//! The gate set: v0.1.0 has no gates (DESIGN ›Feasibility and effort‹, ruled
//! 4 September 2026), so `gate` is null on every record until gates land
//! (issue #33). The slot exists so the one lookup that answers reachable and
//! live is the one that will answer permitted.
//!
//! A record is a value (DESIGN ›The dyad's read surface‹, ruled 8 September
//! 2026): a dyad of type `record` whose value points at these seven fields,
//! allocated in the store so its address is stable for the run. The trie holds
//! that dyad's address, a use of the name stores it in its operand slot, `:`
//! reads its fields, and reading it as a value yields what the dyad it names
//! yields (the reading rule, applied by the interpreter and the lowering).
//! Six pointers and an `f64` laid out in declaration order, `#[repr(C)]`, so the `record`
//! type's field offsets are the struct's.

use crate::dyad::DyadPtr;
use crate::store::Store;

use super::Cx;

/// One candidate for a spelling: the dyad it denotes, the scope it was
/// declared in, its range of life within that scope's body, and its gate set.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Record {
    /// The dyad this name denotes (`a:dyad`).
    pub dyad: DyadPtr,
    /// The enclosing scope the declaration lives in. Whether this scope is open
    /// decides whether the candidate is live.
    pub scope: DyadPtr,
    /// The body item of `scope` that declares the name; null until the parser
    /// settles it (and at top level, which has no body array).
    pub start: DyadPtr,
    /// The body item of `scope` holding the `own` or `drop` that made the name
    /// dead — the node itself while that item is still parsing — or null while
    /// the name is alive.
    pub end: DyadPtr,
    /// The name's gate set; null while the name has none, which in v0.1.0 is
    /// always.
    pub gate: DyadPtr,
    /// The spelling the trie holds for this name, as a string node (`a:name`;
    /// DESIGN ›The dyad's read surface‹, ruled 14 September 2026: "the record
    /// holds every fact tied to the name … The trie already keys on it, so
    /// the record only learns to give it back"). The pattern text for a
    /// pattern identity. Null only on a record a test builds by hand.
    pub name: DyadPtr,
    /// The spelling's `lex_rank` (`a:lex_rank`; DESIGN ›The constructor is a
    /// field‹, ruled 14 September 2026: "it ranks a *spelling* against the
    /// other spellings that could match at a text position, and a second
    /// name for the same identity … is a different spelling with its own
    /// rank, so the slot is name data and lives on the record"). `0` by
    /// default; the two fresh-spelling patterns carry `-1`. Read by the
    /// lexer at every match ([`crate::parse::ScopeStack::select`]).
    pub lex_rank: f64,
}

impl Record {
    /// A new, live, ungated `record` pairing `dyad` with its declaring `scope`
    /// under the spelling `name`, at the default lex rank.
    pub fn new(dyad: DyadPtr, scope: DyadPtr, name: DyadPtr) -> Self {
        Record {
            dyad,
            scope,
            start: std::ptr::null_mut(),
            end: std::ptr::null_mut(),
            gate: std::ptr::null_mut(),
            name,
            lex_rank: 0.0,
        }
    }

    /// Whether an `own` or `drop` has ended this name's life.
    pub fn is_dead(&self) -> bool {
        !self.end.is_null()
    }

    /// Store `rec` and return its dyad: `{type: record, value -> the fields}`,
    /// the value every trie entry is and every use of the name points at.
    pub fn alloc(store: &mut Store, record_ty: DyadPtr, rec: Record) -> DyadPtr {
        let fields = store.alloc_record(rec);
        store.alloc_raw(record_ty, fields as *mut u8)
    }

    /// A copy of the fields behind a record dyad. A copy, never a reference:
    /// the record is read on every operand evaluation in both tiers
    /// ([`through`]), and a reference minted from the raw pointer there would
    /// alias whatever reference a caller up the chain still holds. The seven
    /// words copy for the cost of one load each, and the borrow checker has
    /// nothing to be lied to about.
    ///
    /// # Safety
    /// `dyad` must be a record dyad from the store (its type the `record`
    /// identity, its value from [`Record::alloc`]).
    pub unsafe fn read(dyad: DyadPtr) -> Record {
        std::ptr::read(Self::fields(dyad))
    }

    /// The fields behind a record dyad, as the raw place they are. The
    /// setters below write one field through it; no reference is created.
    ///
    /// # Safety
    /// As [`Record::read`].
    unsafe fn fields(dyad: DyadPtr) -> *mut Record {
        (*dyad).value as *mut Record
    }

    /// Write the `scope` field: the declaration's writer ([`crate::parse::ScopeStack::declare`]).
    ///
    /// # Safety
    /// As [`Record::read`].
    pub unsafe fn set_scope(dyad: DyadPtr, scope: DyadPtr) {
        (*Self::fields(dyad)).scope = scope;
    }

    /// Write the `dyad` field: a rebind's writer ([`crate::parse::ScopeStack::rebind`]).
    ///
    /// # Safety
    /// As [`Record::read`].
    pub unsafe fn set_dyad(dyad: DyadPtr, identity: DyadPtr) {
        (*Self::fields(dyad)).dyad = identity;
    }

    /// Write the `start` field: the parser settling the declaring item.
    ///
    /// # Safety
    /// As [`Record::read`].
    pub unsafe fn set_start(dyad: DyadPtr, item: DyadPtr) {
        (*Self::fields(dyad)).start = item;
    }

    /// Write the `end` field: `own`/`drop` ending the name, and the parser
    /// settling or rolling that back.
    ///
    /// # Safety
    /// As [`Record::read`].
    pub unsafe fn set_end(dyad: DyadPtr, item: DyadPtr) {
        (*Self::fields(dyad)).end = item;
    }

    /// Write the `end` field and hand back what it held.
    ///
    /// # Safety
    /// As [`Record::read`].
    pub unsafe fn replace_end(dyad: DyadPtr, item: DyadPtr) -> DyadPtr {
        std::mem::replace(&mut (*Self::fields(dyad)).end, item)
    }

    /// Write the `lex_rank` field: a type body's `lex_rank = …` line and the
    /// fresh-spelling patterns.
    ///
    /// # Safety
    /// As [`Record::read`].
    pub unsafe fn set_lex_rank(dyad: DyadPtr, rank: f64) {
        (*Self::fields(dyad)).lex_rank = rank;
    }
}

/// The reading rule (DESIGN ›The dyad's read surface‹, 8 September 2026): a
/// record "read as a value yields what the dyad it names yields". A use of a
/// name stores the record, so every reader of an operand — type inference at
/// parse, the interpreter, the lowering — hops through it once; anything that
/// is not a record passes unchanged. `:` is the one read that does not hop.
///
/// # Safety
/// `p` must be null or a valid dyad from the store; `record_ty` the `record`
/// identity.
pub unsafe fn through(record_ty: DyadPtr, p: DyadPtr) -> DyadPtr {
    if !p.is_null() && (*p).ty == record_ty {
        (*Record::fields(p)).dyad
    } else {
        p
    }
}

/// Give the `record` type its layout and spelling, at the end of the build:
/// a field scope holding the seven names — six `@dyad` places (`name` too,
/// #120: no place of type `string` exists, so the `:` read is what hands the
/// name back as a string container) and `lex_rank`, an `f64` place (#122) —
/// the `fields` array, and the `RECORD_TAG` layout with `size_bytes` = the
/// struct's fifty-six. Every record dyad minted earlier already carries this
/// type; only its definition waited for `dyad`, `@`, and `array` to exist.
/// The seed's `.` and `:` field reads then serve a record like any user
/// record (`resolve_field`, `instance::layout`), which is what makes
/// `a:scope` an ordinary field read.
pub(super) fn register_type(
    cx: &mut Cx,
    scope_ty: DyadPtr,
    array_ty: DyadPtr,
    dyad_ty: DyadPtr,
    f64_ty: DyadPtr,
) {
    let record_ = cx.record_;
    let scope = cx.store.alloc_raw(scope_ty, std::ptr::null_mut());
    let mut fields = Vec::with_capacity(7);
    for name in ["dyad", "scope", "start", "end", "gate"] {
        let at_dyad = super::pointer::make_pointer_type(cx.store, cx.type_, dyad_ty);
        let field = cx.store.alloc_raw(at_dyad, std::ptr::null_mut());
        cx.declare_in(scope, name, field);
        fields.push(field);
    }
    // `name` is laid out as an `@dyad` place like the five (no place of type
    // `string` exists, `read::place_layout`); the `:` read hands it back as
    // the string container it holds (`Parser::record_read`).
    let at_dyad = super::pointer::make_pointer_type(cx.store, cx.type_, dyad_ty);
    let name_field = cx.store.alloc_raw(at_dyad, std::ptr::null_mut());
    cx.declare_in(scope, "name", name_field);
    fields.push(name_field);
    let rank_field = cx.store.alloc_raw(f64_ty, std::ptr::null_mut());
    cx.declare_in(scope, "lex_rank", rank_field);
    fields.push(rank_field);
    debug_assert_eq!(fields.len() * 8, std::mem::size_of::<Record>());
    let fields_arr = super::array::build(cx.store, array_ty, &fields);
    let layout = super::meta::record_layout(
        cx.store,
        scope,
        fields_arr,
        (fields.len() * 8) as u64,
        std::ptr::null_mut(),
        super::meta::prec::APPLY,
        crate::parse::Assoc::Left,
    );
    // SAFETY: `record_` is the type node minted at the head of the build.
    unsafe { (*record_).value = layout };
    cx.declare("record", record_);
}
