// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A `binding`: one per declared name, the trie entry itself. The dyad the name
//! denotes, the scope it was declared in, its range of life, its spelling and its lex
//! rank. Not the `static`-member record a type carries (`meta`).
//! `#[repr(C)]`, so the `binding` type's field offsets are the struct's.

use crate::dyad::DyadPtr;
use crate::store::Store;

use super::{Core, Cx};
use crate::dyad;

/// One per name, never per identity: `x := type i32` binds a second name to i32's own
/// dyad, so `pub x := type i32` gates what `x` points at through a gate node of its own,
/// never i32 itself.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Binding {
    /// What the name stands for: the type of its bytes for a name laid out in a frame, a
    /// field's `?` node, else the node it denotes. In the record itself this word may point
    /// at a gate node over that ([`super::gate`]); [`Binding::read`] gives what stands under
    /// it, and [`Binding::gated`] the word as it stands.
    pub dyad: DyadPtr,
    /// The enclosing scope, not the declaration node: a dyad has no parent pointer, and
    /// keying by scope makes liveness an O(1) test against the open scopes.
    pub scope: DyadPtr,
    /// The body item of `scope` that declares the name; null until the parser fills it, and at
    /// top level.
    pub start: DyadPtr,
    /// The body item holding the `move` or `free` that made the name dead (the node itself
    /// while that item is still parsing); null while the name is alive.
    pub end: DyadPtr,
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
    /// The field paths under the name that a `move` or `free` reached, an `array` of bindings,
    /// each with the field as its `dyad`, its own `end` and the paths under it; null until a
    /// partial move.
    pub paths: DyadPtr,
    /// The record this one copies, an imported `pub` name's in the file; null on a name's
    /// own record. A copy holds its own `scope` and `start` alone: every other field of it is
    /// read and written on this record.
    pub origin: DyadPtr,
    /// On the spelling-less place an `x:f` read lays over `x`'s record, `x`'s binding; null on
    /// every other binding.
    pub over: DyadPtr,
    /// Declared outside any function in a body the pass does not run in parse order, a loop
    /// or a branch: at parse the name holds no value yet.
    pub unmade: bool,
    /// The spelling is a pattern, keyed in the trie as the recognizer it is; any other
    /// spelling is keyed escaped, so it stands for itself.
    pub pattern: bool,
    /// Declared without a value and not yet written: a read is refused until a write fills it
    /// (›Reading a place before its first write is refused at parse‹ [[R104]]).
    pub unread: bool,
    /// Its line gives its type when it runs, and a write converts into that type.
    pub typed_at_run: bool,
    /// The binding site made the name the owner of the node it holds (›Holding is decided at
    /// the binding site, parameters included‹ [[R61]]).
    pub holds: bool,
    /// A word its body got from the language, which no text declared: a type body's slot word
    /// (›Slot words are known only inside a type body‹ [[R220]]), a `parse` body's `tape`
    /// (›`tape` is a word the `parse` slot declares into its body‹ [[R191]]). A body inside
    /// another has its own beside the outer one's, and a lookup finds the innermost.
    pub body_word: bool,
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
            name,
            lex_rank: 0.0,
            frame: std::ptr::null_mut(),
            offset: 0,
            unwritten: std::ptr::null_mut(),
            paths: std::ptr::null_mut(),
            origin: std::ptr::null_mut(),
            over: std::ptr::null_mut(),
            unmade: false,
            pattern: false,
            unread: false,
            typed_at_run: false,
            holds: false,
            body_word: false,
        }
    }

    pub fn is_dead(&self) -> bool {
        !self.end.is_null()
    }

    /// The trie key `spelling` enters under: the one place a spelling's key is decided.
    pub fn key(&self, spelling: &str) -> String {
        if self.pattern {
            spelling.to_string()
        } else {
            regex::escape(spelling)
        }
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

    /// Whether the name is a field, laid out inside each value of the type that is its frame.
    ///
    /// # Safety
    /// As `is_storage`.
    pub unsafe fn is_field(&self) -> bool {
        !self.frame.is_null() && !self.is_storage()
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
        let f = Self::shared(dyad);
        (*f).dyad = ty;
        (*f).frame = frame;
        (*f).offset = offset;
    }

    /// A field's offset inside each value of `ty`; the binding's `dyad` stays the field node.
    ///
    /// # Safety
    /// As `Binding::read`; `ty` a type node.
    pub unsafe fn set_field_offset(dyad: DyadPtr, ty: DyadPtr, offset: usize) {
        let f = Self::shared(dyad);
        (*f).frame = ty;
        (*f).offset = offset;
    }

    /// Store `rec` and return its dyad, the value every trie entry is and every use of the name
    /// points at.
    pub fn alloc(store: &mut Store, binding_ty: DyadPtr, rec: Binding) -> DyadPtr {
        store.alloc_binding(binding_ty, rec)
    }

    /// A copy of the record `dyad` keeps, for one more scope to enter: the same name, one record
    /// wherever it is entered (›Importing is dropping the text there, wrapped in its own scope‹
    /// [[R306]]).
    ///
    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn copy_of(store: &mut Store, binding_ty: DyadPtr, dyad: DyadPtr) -> DyadPtr {
        let origin = Self::origin_of(dyad);
        let scope = (*Self::fields(origin)).scope;
        let rec =
            Binding { origin, ..Binding::new(std::ptr::null_mut(), scope, std::ptr::null_mut()) };
        Self::alloc(store, binding_ty, rec)
    }

    /// The name's own record: `dyad` itself, or the one it is a copy of.
    ///
    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn origin_of(dyad: DyadPtr) -> DyadPtr {
        let origin = (*Self::fields(dyad)).origin;
        if origin.is_null() {
            dyad
        } else {
            origin
        }
    }

    /// The fields a copy holds itself, by offset, each a pointer; every other field is its
    /// origin's.
    const OWN: [usize; 3] = [
        std::mem::offset_of!(Binding, scope),
        std::mem::offset_of!(Binding, start),
        std::mem::offset_of!(Binding, origin),
    ];

    /// A copy, never a reference: a reference minted from the raw pointer would alias
    /// whatever reference a caller up the chain still holds. Each field from the record that
    /// holds it ([`Binding::holder`]).
    ///
    /// # Safety
    /// `dyad` must be a binding dyad from the store (its type the `binding` identity, its
    /// value from `Binding::alloc`).
    pub unsafe fn read(dyad: DyadPtr) -> Binding {
        let own = Self::fields(dyad);
        let mut rec = std::ptr::read(Self::shared(dyad));
        let into = std::ptr::addr_of_mut!(rec) as *mut u8;
        for offset in Self::OWN {
            *(into.add(offset) as *mut DyadPtr) =
                *((own as *const u8).add(offset) as *const DyadPtr);
        }
        rec.dyad = super::gate::under(rec.dyad);
        rec
    }

    /// The `dyad` word as the record holds it: a gate node where the name's value is gated.
    ///
    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn gated(dyad: DyadPtr) -> DyadPtr {
        (*Self::shared(dyad)).dyad
    }

    /// The record whose bytes hold the field at `offset` of the name `dyad`: the copy's own for
    /// the fields it holds itself, its origin's for every other.
    ///
    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn holder(dyad: DyadPtr, offset: usize) -> DyadPtr {
        if Self::OWN.contains(&offset) {
            dyad
        } else {
            Self::origin_of(dyad)
        }
    }

    /// The raw place; the setters write one field through it and no reference is created.
    ///
    /// # Safety
    /// As `Binding::read`.
    unsafe fn fields(dyad: DyadPtr) -> *mut Binding {
        dyad::value(dyad) as *mut Binding
    }

    /// The raw place of every field a copy does not hold itself: the origin's.
    ///
    /// # Safety
    /// As `Binding::read`.
    unsafe fn shared(dyad: DyadPtr) -> *mut Binding {
        Self::fields(Self::origin_of(dyad))
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_scope(dyad: DyadPtr, scope: DyadPtr) {
        (*Self::fields(dyad)).scope = scope;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_dyad(dyad: DyadPtr, identity: DyadPtr) {
        (*Self::shared(dyad)).dyad = identity;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_start(dyad: DyadPtr, item: DyadPtr) {
        (*Self::fields(dyad)).start = item;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_end(dyad: DyadPtr, item: DyadPtr) {
        (*Self::shared(dyad)).end = item;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn replace_end(dyad: DyadPtr, item: DyadPtr) -> DyadPtr {
        let f = Self::shared(dyad);
        let prev = (*f).end;
        (*f).end = item;
        prev
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_lex_rank(dyad: DyadPtr, rank: f64) {
        (*Self::shared(dyad)).lex_rank = rank;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_pattern(dyad: DyadPtr, pattern: bool) {
        (*Self::shared(dyad)).pattern = pattern;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_body_word(dyad: DyadPtr) {
        (*Self::shared(dyad)).body_word = true;
    }

    /// # Safety
    /// As `Binding::read`; `fields` null or an `array` node from the store.
    pub unsafe fn set_unwritten(dyad: DyadPtr, fields: DyadPtr) {
        (*Self::shared(dyad)).unwritten = fields;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_unread(dyad: DyadPtr, unread: bool) {
        (*Self::shared(dyad)).unread = unread;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_typed_at_run(dyad: DyadPtr) {
        (*Self::shared(dyad)).typed_at_run = true;
    }

    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn set_holds(dyad: DyadPtr) {
        (*Self::shared(dyad)).holds = true;
    }

    /// The binding of the path under the name that ends at `field`, null while no `move` or
    /// `free` reached it.
    ///
    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn path(dyad: DyadPtr, field: DyadPtr) -> DyadPtr {
        let paths = (*Self::shared(dyad)).paths;
        if paths.is_null() {
            return std::ptr::null_mut();
        }
        super::array::items(paths)
            .iter()
            .copied()
            .find(|&p| (*Self::shared(p)).dyad == field)
            .unwrap_or(std::ptr::null_mut())
    }

    /// # Safety
    /// As `Binding::read`; `array_ty` the `array` identity, `path` a binding dyad from the store.
    pub unsafe fn add_path(store: &mut Store, array_ty: DyadPtr, dyad: DyadPtr, path: DyadPtr) {
        let paths = (*Self::shared(dyad)).paths;
        let mut items =
            if paths.is_null() { Vec::new() } else { super::array::items(paths).to_vec() };
        items.push(path);
        (*Self::shared(dyad)).paths = super::array::build(store, array_ty, &items);
    }

    /// Whether a path under the name has ended, at any depth, so the name is dead as a whole.
    ///
    /// # Safety
    /// As `Binding::read`.
    pub unsafe fn has_ended_path(dyad: DyadPtr) -> bool {
        let paths = (*Self::shared(dyad)).paths;
        !paths.is_null()
            && super::array::items(paths)
                .iter()
                .any(|&p| !(*Self::shared(p)).end.is_null() || Self::has_ended_path(p))
    }

    /// # Safety
    /// As `Binding::read`; the binding's `name` must be null or a string node.
    pub unsafe fn spelling(dyad: DyadPtr) -> String {
        let name = (*Self::shared(dyad)).name;
        if name.is_null() {
            String::new()
        } else {
            String::from_utf8_lossy(super::string::text(name)).into_owned()
        }
    }

    /// Whether a name declares the binding: a receiver and a run body's field of type `type` are
    /// places no name declares.
    ///
    /// # Safety
    /// As [`Binding::spelling`].
    pub unsafe fn is_named(dyad: DyadPtr) -> bool {
        let name = (*Self::shared(dyad)).name;
        !name.is_null() && !super::string::text(name).is_empty()
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
/// wait for `dyad`, `@`, and `array` to exist. Each is read from outside, so each is `pub`,
/// its type a gate node of the gate mint `pub_mint`.
pub(super) fn register_type(
    cx: &mut Cx,
    scope_ty: DyadPtr,
    array_ty: DyadPtr,
    dyad_ty: DyadPtr,
    f64_ty: DyadPtr,
    u64_ty: DyadPtr,
    pub_mint: DyadPtr,
) {
    let binding_ = cx.binding_;
    let scope = super::scope::mint(cx.store, scope_ty, std::ptr::null_mut());
    let mut fields = Vec::with_capacity(8);
    let mut bindings = Vec::with_capacity(8);
    // SAFETY: `dyad_ty` is the type node `Core::build` minted.
    let at_dyad = unsafe { super::pointer::early_pointer_type(cx.store, cx.type_, dyad_ty) };
    // `name` is a place of type `string`: eight bytes holding the string node's address.
    let typed = [
        ("dyad", at_dyad),
        ("scope", at_dyad),
        ("start", at_dyad),
        ("end", at_dyad),
        ("name", cx.string_),
        ("lex_rank", f64_ty),
        ("frame", at_dyad),
        ("offset", u64_ty),
    ];
    for (i, (name, ty)) in typed.into_iter().enumerate() {
        // SAFETY: `pub_mint` is a gate mint and `ty` a type node.
        let gated = unsafe { super::gate::early_over(cx.store, pub_mint, ty) };
        let field = super::hole::build(cx.store, cx.unknown, gated, std::ptr::null_mut());
        let binding = cx.declare_in(scope, name, field);
        // SAFETY: `binding` was just minted by `declare_in`; `binding_` is the type node.
        unsafe { Binding::set_field_offset(binding, binding_, i * 8) };
        fields.push(field);
        bindings.push(binding);
    }
    // The Logos-visible fields come first and are each one word; the rest is the seed's.
    debug_assert_eq!(fields.len() * 8, std::mem::offset_of!(Binding, unwritten));
    let fields_arr = super::array::build(cx.store, array_ty, &fields);
    let bindings = super::array::build(cx.store, array_ty, &bindings);
    let layout = super::meta::record_layout(
        cx.store,
        scope,
        fields_arr,
        bindings,
        (fields.len() * 8) as u64,
        std::ptr::null_mut(),
        super::meta::prec::APPLY,
        crate::parse::Assoc::Left,
    );
    // SAFETY: `binding_` is the type node minted at the head of the build.
    unsafe { dyad::set_head(binding_, layout) };
    cx.declare("binding", binding_);
}
