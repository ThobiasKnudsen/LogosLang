// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A `binding`: one per declared name, the trie entry itself. The dyad the name
//! denotes, the scope it was declared in, its range of life, its gate set, its
//! spelling and its lex rank. Not the shared-member record a type carries (`meta`).
//! `#[repr(C)]`, so the `binding` type's field offsets are the struct's.

use crate::dyad::DyadPtr;
use crate::store::Store;

use super::Cx;

/// One per name, never per identity: `x := i32` binds a second name to i32's own
/// dyad, and a binding shared through the dyad would let `x := pub i32` gate i32 itself.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Binding {
    pub dyad: DyadPtr,
    /// The enclosing scope, not the declaration node: a dyad has no parent pointer, and
    /// keying by scope makes liveness an O(1) test against the open scopes.
    pub scope: DyadPtr,
    /// The body item of `scope` that declares the name; null until the parser fills it, and at
    /// top level.
    pub start: DyadPtr,
    /// The body item holding the `own` or `drop` that made the name dead (the node itself
    /// while that item is still parsing); null while the name is alive.
    pub end: DyadPtr,
    /// The gate words on the name in text order, an `array` node; null while none.
    pub gate: DyadPtr,
    /// The spelling as a string node; a pattern identity's pattern text. Null only on a
    /// binding a test builds by hand.
    pub name: DyadPtr,
    /// `0` by default; the two fresh-spelling patterns carry `-1`. Read by the lexer at every
    /// match.
    pub lex_rank: f64,
}

impl Binding {
    pub fn new(dyad: DyadPtr, scope: DyadPtr, name: DyadPtr) -> Self {
        Binding {
            dyad,
            scope,
            start: std::ptr::null_mut(),
            end: std::ptr::null_mut(),
            gate: std::ptr::null_mut(),
            name,
            lex_rank: 0.0,
        }
    }

    pub fn is_dead(&self) -> bool {
        !self.end.is_null()
    }

    /// Store `rec` and return its dyad, the value every trie entry is and every use of the name
    /// points at.
    pub fn alloc(store: &mut Store, binding_ty: DyadPtr, rec: Binding) -> DyadPtr {
        let fields = store.alloc_binding(rec);
        store.alloc_raw(binding_ty, fields as *mut u8)
    }

    /// A copy, never a reference: a reference minted from the raw pointer would alias
    /// whatever reference a caller up the chain still holds.
    ///
    /// # Safety
    /// `dyad` must be a binding dyad from the store (its type the `binding` identity, its
    /// value from `Binding::alloc`).
    pub unsafe fn read(dyad: DyadPtr) -> Binding {
        std::ptr::read(Self::fields(dyad))
    }

    /// The raw place; the setters write one field through it and no reference is created.
    ///
    /// # Safety
    /// As `Binding::read`.
    unsafe fn fields(dyad: DyadPtr) -> *mut Binding {
        (*dyad).value as *mut Binding
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_scope(dyad: DyadPtr, scope: DyadPtr) {
        (*Self::fields(dyad)).scope = scope;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_dyad(dyad: DyadPtr, identity: DyadPtr) {
        (*Self::fields(dyad)).dyad = identity;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_start(dyad: DyadPtr, item: DyadPtr) {
        (*Self::fields(dyad)).start = item;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_end(dyad: DyadPtr, item: DyadPtr) {
        (*Self::fields(dyad)).end = item;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn replace_end(dyad: DyadPtr, item: DyadPtr) -> DyadPtr {
        std::mem::replace(&mut (*Self::fields(dyad)).end, item)
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_lex_rank(dyad: DyadPtr, rank: f64) {
        (*Self::fields(dyad)).lex_rank = rank;
    }

    /// # Safety
    /// As `Binding::read`; `gate` an identity from the store.
    pub unsafe fn has_gate(dyad: DyadPtr, gate: DyadPtr) -> bool {
        let gates = (*Self::fields(dyad)).gate;
        !gates.is_null() && super::array::items(gates).contains(&gate)
    }

    /// Put `gate` first: the words are read right to left, so first keeps text order.
    ///
    /// # Safety
    /// As `Binding::read`; `array_ty` the `array` identity, `gate` an identity from the store.
    pub unsafe fn add_gate(store: &mut Store, array_ty: DyadPtr, dyad: DyadPtr, gate: DyadPtr) {
        let gates = (*Self::fields(dyad)).gate;
        let mut items =
            if gates.is_null() { Vec::new() } else { super::array::items(gates).to_vec() };
        items.insert(0, gate);
        (*Self::fields(dyad)).gate = super::array::build(store, array_ty, &items);
    }

    /// # Safety
    /// As `Binding::read`; the binding's `name` must be null or a string node.
    pub unsafe fn spelling(dyad: DyadPtr) -> String {
        let name = (*Self::fields(dyad)).name;
        if name.is_null() {
            String::new()
        } else {
            String::from_utf8_lossy(super::string::text(name)).into_owned()
        }
    }
}

/// A binding read as a value yields what the dyad it names yields; anything else passes
/// unchanged. `:` is the one read that does not hop.
///
/// # Safety
/// `p` must be null or a valid dyad from the store; `binding_ty` the `binding` identity.
pub unsafe fn through(binding_ty: DyadPtr, p: DyadPtr) -> DyadPtr {
    if !p.is_null() && (*p).ty == binding_ty {
        (*Binding::fields(p)).dyad
    } else {
        p
    }
}

/// Give the `binding` type its layout and spelling at the end of the build: its fields
/// wait for `dyad`, `@`, and `array` to exist.
pub(super) fn register_type(
    cx: &mut Cx,
    scope_ty: DyadPtr,
    array_ty: DyadPtr,
    dyad_ty: DyadPtr,
    f64_ty: DyadPtr,
) {
    let binding_ = cx.binding_;
    let scope = cx.store.alloc_raw(scope_ty, std::ptr::null_mut());
    let mut fields = Vec::with_capacity(7);
    // SAFETY: `dyad_ty` is the type node `Core::build` minted.
    let at_dyad = unsafe { super::pointer::make_pointer_type(cx.store, cx.type_, dyad_ty) };
    for name in ["dyad", "scope", "start", "end", "gate"] {
        let field = cx.store.alloc_raw(at_dyad, std::ptr::null_mut());
        cx.declare_in(scope, name, field);
        fields.push(field);
    }
    // `name` is an `@dyad` place too: no place of type `string` exists, and the `:` read
    // hands back the string node it holds.
    let name_field = cx.store.alloc_raw(at_dyad, std::ptr::null_mut());
    cx.declare_in(scope, "name", name_field);
    fields.push(name_field);
    let rank_field = cx.store.alloc_raw(f64_ty, std::ptr::null_mut());
    cx.declare_in(scope, "lex_rank", rank_field);
    fields.push(rank_field);
    debug_assert_eq!(fields.len() * 8, std::mem::size_of::<Binding>());
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
    // SAFETY: `binding_` is the type node minted at the head of the build.
    unsafe { (*binding_).value = layout };
    cx.declare("binding", binding_);
}
