// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A `binding`: one per declared name, the trie entry itself. The dyad the name
//! denotes, the scope it was declared in, its range of life, its gate set, its
//! spelling and its lex rank. Not the shared-member record a type carries (`meta`).
//! `#[repr(C)]`, so the `binding` type's field offsets are the struct's.

use crate::dyad::DyadPtr;
use crate::store::Store;

use super::{Core, Cx};
use crate::dyad;

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
    /// The closed-off identity whose instance holds the name's bytes: the root scope for
    /// the program frame, a `fn` node for its call frame, a type for a field's offset in
    /// each value; null for a name that denotes `dyad` itself.
    /// DESIGN ›A scope lays out its declarations; a use reaches the offset through its binding‹.
    pub frame: DyadPtr,
    pub offset: usize,
    /// The fields of a `T ?` name not yet written, an `array` node of the record's field
    /// dyads; null while none wait.
    pub unwritten: DyadPtr,
    /// Declared outside any function in a body the pass does not run in parse order, a loop
    /// or a branch: at parse the name holds no value yet.
    pub unmade: bool,
}

/// Where a storage binding's bytes lie, with the offset into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// The program's one frame, based at the store's arena.
    Root,
    /// A call of the `fn` node, based at the activation.
    Call(DyadPtr),
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
            frame: std::ptr::null_mut(),
            offset: 0,
            unwritten: std::ptr::null_mut(),
            unmade: false,
        }
    }

    pub fn is_dead(&self) -> bool {
        !self.end.is_null()
    }

    /// The name's bytes are in a frame of its own: a type as the frame lays a field out
    /// inside each value instead. A type identity is the one node typed by a self-typed node.
    ///
    /// # Safety
    /// `frame` must be null or a dyad from the store.
    pub unsafe fn is_storage(&self) -> bool {
        if self.frame.is_null() {
            return false;
        }
        let ty = dyad::ty(self.frame);
        dyad::ty(ty) != ty
    }

    /// What a use of the name yields: the storage itself, or the dyad the name denotes.
    ///
    /// # Safety
    /// As `is_storage`; `this` must be the binding dyad these fields were read from.
    pub unsafe fn names(&self, this: DyadPtr) -> DyadPtr {
        if self.is_storage() {
            this
        } else {
            self.dyad
        }
    }

    /// Which frame: the root scope's or a `fn` node's.
    ///
    /// # Safety
    /// `frame` must be null or a dyad from the store.
    pub unsafe fn storage(&self, types: &Core) -> Option<Frame> {
        if self.frame.is_null() {
            None
        } else if self.frame == types.root_scope {
            Some(Frame::Root)
        } else if dyad::ty(self.frame) == types.fn_type {
            Some(Frame::Call(self.frame))
        } else {
            None
        }
    }

    /// Lay the name out: its bytes are `T` at `offset` in `frame`.
    ///
    /// # Safety
    /// As `Binding::read`; `ty` null or a type node, `frame` a scope, `fn` or type node.
    pub unsafe fn lay_out(dyad: DyadPtr, ty: DyadPtr, frame: DyadPtr, offset: usize) {
        let fields = Self::fields(dyad);
        (*fields).dyad = ty;
        (*fields).frame = frame;
        (*fields).offset = offset;
    }

    /// A field's offset inside each value of `ty`; the binding's `dyad` stays the field node.
    ///
    /// # Safety
    /// As `Binding::read`; `ty` a type node.
    pub unsafe fn set_field_offset(dyad: DyadPtr, ty: DyadPtr, offset: usize) {
        let fields = Self::fields(dyad);
        (*fields).frame = ty;
        (*fields).offset = offset;
    }

    /// Store `rec` and return its dyad, the value every trie entry is and every use of the name
    /// points at.
    pub fn alloc(store: &mut Store, binding_ty: DyadPtr, rec: Binding) -> DyadPtr {
        store.alloc_binding(binding_ty, rec)
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
        dyad::value(dyad) as *mut Binding
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
    /// As `Binding::read`; `fields` null or an `array` node from the store.
    pub unsafe fn set_unwritten(dyad: DyadPtr, fields: DyadPtr) {
        (*Self::fields(dyad)).unwritten = fields;
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

    /// The gate set of another binding, the array shared: no gate is added after a
    /// declaration closes, and every edit builds a new array.
    ///
    /// # Safety
    /// As `Binding::read`, for both.
    pub unsafe fn copy_gates(dyad: DyadPtr, from: DyadPtr) {
        (*Self::fields(dyad)).gate = (*Self::fields(from)).gate;
    }

    /// # Safety
    /// As `add_gate`.
    pub unsafe fn remove_gate(store: &mut Store, array_ty: DyadPtr, dyad: DyadPtr, gate: DyadPtr) {
        let gates = (*Self::fields(dyad)).gate;
        if gates.is_null() {
            return;
        }
        let items: Vec<DyadPtr> =
            super::array::items(gates).iter().copied().filter(|&g| g != gate).collect();
        (*Self::fields(dyad)).gate = if items.is_empty() {
            std::ptr::null_mut()
        } else {
            super::array::build(store, array_ty, &items)
        };
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

/// A binding read as a value yields what the dyad it names yields, and a binding laid out
/// in a frame is the storage itself, so it stands; anything else passes unchanged. `:` is
/// the one read that does not hop.
///
/// # Safety
/// `p` must be null or a valid dyad from the store.
pub unsafe fn through(types: &Core, p: DyadPtr) -> DyadPtr {
    if p.is_null() || dyad::ty(p) != types.binding_ {
        return p;
    }
    let b = Binding::read(p);
    if b.storage(types).is_some() {
        p
    } else {
        b.dyad
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
    u64_ty: DyadPtr,
) {
    let binding_ = cx.binding_;
    let scope = super::scope::mint(cx.store, scope_ty, std::ptr::null_mut());
    let mut fields = Vec::with_capacity(9);
    // SAFETY: `dyad_ty` is the type node `Core::build` minted.
    let at_dyad = unsafe { super::pointer::make_pointer_type(cx.store, cx.type_, dyad_ty) };
    // `name` is a place of type `string`: the eight-byte container every type without a
    // layout of its own has, holding the string node, which `x:name` reads as the text.
    let typed = [
        ("dyad", at_dyad),
        ("scope", at_dyad),
        ("start", at_dyad),
        ("end", at_dyad),
        ("gate", at_dyad),
        ("name", cx.string_),
        ("lex_rank", f64_ty),
        ("frame", at_dyad),
        ("offset", u64_ty),
    ];
    for (i, (name, ty)) in typed.into_iter().enumerate() {
        let field = super::hole::build(cx.store, cx.unknown, ty, std::ptr::null_mut());
        let binding = cx.declare_in(scope, name, field);
        // SAFETY: `binding` was just minted by `declare_in`; `binding_` is the type node.
        unsafe { Binding::set_field_offset(binding, binding_, i * 8) };
        fields.push(field);
    }
    // The Logos-visible fields come first and are each one word; the rest is the seed's.
    debug_assert_eq!(fields.len() * 8, std::mem::offset_of!(Binding, unwritten));
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
    unsafe { dyad::set_head(binding_, layout) };
    cx.declare("binding", binding_);
}
