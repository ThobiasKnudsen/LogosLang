// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The reading rule: how a node's value is read, decided once in `read_kind`, which
//! every reader asks. What decides it: the binding hop, storage by its declared type,
//! then `fn` (the one identity no record can carry), then the type's record kind byte.
//! DESIGN ›The dyad's read surface‹, ›A scope lays out its declarations…‹.

use super::callable;
use super::meta;
use super::numtype::{self, NumType, ADDR_TAG, COMMENT_TAG, STRING_TAG, VOID_TAG};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::Core;

/// `Copy` and register-sized: it is asked on every interpreted value read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// Load at the type's width.
    Scalar(NumType),
    /// Eight bytes holding an address; the pointee is here for `@` and `=` to consult.
    Pointer(DyadPtr),
    /// Eight bytes holding a node address; carries the declared type (`type`, `dyad`, or
    /// null for a bare parameter's slot) so a writer knows what the box may take.
    Container(DyadPtr),
    /// A type standing as a value: its own address is the value.
    Identity,
    /// A node a Logos `parse` built: its own address is the value.
    Node,
    /// A dyad view: the stored address is the value.
    Address,
    /// A comptime rational, molded on read.
    Literal,
    /// A run-time rational: sixteen bytes `[num, den]` in a place, read by address and
    /// copied like a record.
    Rational,
    /// A record instance place: read by field or by address, never whole.
    Aggregate,
    /// No whole-value read: text, unit, a regex, an array, a callable, a convention, a
    /// parse-only identity.
    Opaque,
    /// Prose, or a `fn` literal standing as a statement: yields 0.
    Unit,
    Executable(Dispatch),
    /// A slot marker, a fresh spelling's dyad.
    Undefined,
}

/// Carried out of `read_kind` so neither tier re-derives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// The callee: the node's type, or that type's `code`.
    Call(DyadPtr),
    /// The callable leaf in the node's op slot, already checked to be one.
    Leaf(DyadPtr),
    /// An operand record with no leaf in its op slot: a named refusal.
    None,
}

/// # Safety
/// `node` must be null or a valid dyad from the store; every identity standing in a
/// type slot carries its record.
pub unsafe fn read_kind(types: &Core, node: DyadPtr) -> Read {
    let node = types.through(node);
    if node.is_null() {
        return Read::Undefined;
    }
    // Storage is known by having come through a binding, never by a mark on the value word.
    if let Some(t) = types.storage_type(node) {
        return place_kind(types, t);
    }
    let op = dyad::ty(node);
    let value = dyad::value(node);
    // A hole or a fresh spelling holds nothing.
    if op.is_null() {
        return Read::Undefined;
    }
    // Before any record read: a function node's value is an operand array, not a record.
    if dyad::ty(op) == types.fn_type {
        return Read::Executable(Dispatch::Call(op));
    }
    // `fn`'s own record is an operand record like `+`'s, so only the identity tells them apart.
    if op == types.fn_type {
        return Read::Unit;
    }
    let Some(kind) = meta::kind_of(op) else {
        // The roots are back-filled in `Core::build`, and holes never stand in a type
        // slot, so no reachable node is classified by a type with no record.
        debug_assert!(!dyad::value(op).is_null(), "a type with no record stands in a type slot");
        return Read::Undefined;
    };
    match kind {
        k if k < VOID_TAG => Read::Scalar(numtype::of_type_node(op)),
        ADDR_TAG => Read::Pointer(numtype::pointee_of(op)),
        meta::TYPEREC_TAG => Read::Identity,
        meta::DYAD_TAG => Read::Address,
        meta::RECORD_TAG => {
            if meta::is_node_valued(op, types.fn_type) {
                return Read::Node;
            }
            if !meta::run_body_of(op).is_null() {
                // The node runs as the function built for its field-type set,
                // and not at all until one exists.
                let spec = super::run_body::spec_of(node);
                if spec.is_null() {
                    Read::Executable(Dispatch::None)
                } else {
                    Read::Executable(Dispatch::Call(spec))
                }
            } else {
                Read::Aggregate
            }
        }
        meta::TUPLE_TAG | meta::LIST_TAG => {
            let Some(idx) = meta::op_slot_of(op) else {
                return Read::Executable(Dispatch::None);
            };
            let slots = value as *const DyadPtr;
            if slots.is_null() {
                return Read::Executable(Dispatch::None);
            }
            let leaf = *slots.add(idx);
            if !leaf.is_null() && callable::is_callable(leaf) {
                Read::Executable(Dispatch::Leaf(leaf))
            } else {
                Read::Executable(Dispatch::None)
            }
        }
        meta::FRACTION_TAG => Read::Literal,
        COMMENT_TAG | VOID_TAG => Read::Unit,
        // The rest have no whole-value read.
        _ => {
            debug_assert!(matches!(
                kind,
                STRING_TAG
                    | meta::ARRAY_TAG
                    | meta::CALLABLE_TAG
                    | meta::CONVENTION_TAG
                    | meta::TOKEN_TAG
            ));
            Read::Opaque
        }
    }
}

/// How storage declared `t` reads: at the type's own layout, or as the eight-byte
/// container every other type's place holds (a node's address, a bare parameter's operand).
///
/// # Safety
/// `t` must be null or a valid dyad from the store.
pub unsafe fn place_kind(types: &Core, t: DyadPtr) -> Read {
    match place_layout(types, t) {
        Some((read, _)) => read,
        None => Read::Container(t),
    }
}

/// What a place of `t` reads as and how many bytes it takes, or `None` when no place of
/// `t` can exist. The allocation side of `read_kind`: a read of a place must agree with
/// its allocation, or two frame slots overlap and no read-side check can catch it.
///
/// # Safety
/// `t` must be null or a valid dyad from the store.
pub unsafe fn place_layout(types: &Core, t: DyadPtr) -> Option<(Read, usize)> {
    let t = super::type_identity_of(types, t)?;
    let kind = meta::kind_of(t)?;
    match kind {
        meta::FRACTION_TAG => Some((Read::Rational, 16)),
        k if k < VOID_TAG => {
            let nt = numtype::of_type_node(t);
            Some((Read::Scalar(nt), nt.bytes()))
        }
        ADDR_TAG => Some((Read::Pointer(numtype::pointee_of(t)), 8)),
        meta::TYPEREC_TAG | meta::DYAD_TAG => Some((Read::Container(t), 8)),
        meta::RECORD_TAG => {
            if meta::is_node_valued(t, types.fn_type) {
                return Some((Read::Container(t), 8));
            }
            // A node of a type with a run is a call, so the type has no place.
            if !meta::run_body_of(t).is_null() {
                return None;
            }
            Some((Read::Aggregate, (meta::record_size_of(t) as usize).max(1)))
        }
        _ => None,
    }
}

/// The machine type a cell of `t` is loaded and stored at through a pointer: a scalar's own,
/// and eight bytes of address for a pointer or a value of a type built by a Logos `parse`
/// (DESIGN ›A value of a type built by a Logos `parse` travels as a pointer‹). `None` when a
/// cell of `t` has no whole-value load.
///
/// # Safety
/// `t` must be null or a valid dyad from the store.
pub unsafe fn cell_numtype(types: &Core, t: DyadPtr) -> Option<NumType> {
    match place_layout(types, t)? {
        (Read::Scalar(nt), _) => Some(nt),
        (Read::Pointer(_), _) => Some(NumType::U64),
        (Read::Container(c), _) if meta::is_node_valued(c, types.fn_type) => Some(NumType::U64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identities::{array, declare, Core};
    use crate::parse::{Parser, ScopeStack};
    use crate::regex_trie::RegexTrie;
    use crate::store::Store;

    /// Parse `src` in a fresh core and return the sequence's items, with the index they resolve in.
    fn parse_seq(src: &str) -> (Store, RegexTrie, Core, Vec<DyadPtr>) {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let seq = {
            let mut scopes = ScopeStack::new();
            scopes.push(core.root_scope);
            let mut p = Parser::new(src, &mut store, &mut trie, &core, scopes);
            p.parse_sequence().unwrap()
        };
        // SAFETY: a sequence node's first slot is its expression array.
        let exprs = unsafe { array::items(*(dyad::value(seq) as *const DyadPtr)).to_vec() };
        (store, trie, core, exprs)
    }

    #[test]
    fn every_identity_is_an_identity() {
        // Every root and everything classified by it, `left`/`right` included.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = &core;
        // SAFETY: all handles are identities Core::build just allocated.
        unsafe {
            for id in [
                core.type_,
                core.fn_type,
                core.scope,
                core.binding_,
                core.i32_,
                core.bool_,
                core.void_,
                core.string_,
                core.rational,
                core.dyad_,
                core.plus,
                types.left_,
                types.right_,
            ] {
                assert_eq!(read_kind(types, id), Read::Identity, "{id:p}");
            }
            for &n in &types.numtypes {
                assert_eq!(read_kind(types, n), Read::Identity);
            }
        }
    }

    #[test]
    fn places_read_by_their_type_and_values_by_their_kind() {
        let (_store, _trie, core, exprs) = parse_seq(
            "x := i32 5,\n\
             p := &x,\n\
             a := type ?,\n\
             d := dyad ?,\n\
             w := type ( y := i64 ? ),\n\
             q := w(7),\n\
             f := fn (n := i32 ?, b) -> i32 ( n ),\n\
             true,\n\
             5,\n\
             «hi»,\n\
             # prose\n\
             w.scope,\n\
             i32 1 + i32 2,\n\
             f(1, 2),\n\
             ?",
        );
        let types = &core;
        // SAFETY: every node was just parsed into `_store`, which is alive.
        unsafe {
            // The place is behind the binding's initializer store or construction.
            let declared = |i: usize| {
                let d = declare::declared_of(exprs[i]);
                if dyad::ty(d) == core.assign {
                    types.through(crate::identities::operands(d).0)
                } else if dyad::ty(d) == core.construct_ {
                    *(dyad::value(d) as *const DyadPtr)
                } else {
                    d
                }
            };
            assert_eq!(read_kind(types, declared(0)), Read::Scalar(NumType::I32));
            assert!(types.is_storage(declared(0)));
            assert_eq!(read_kind(types, declared(1)), Read::Pointer(core.i32_));
            assert_eq!(read_kind(types, declared(2)), Read::Container(core.type_));
            assert_eq!(read_kind(types, declared(3)), Read::Container(core.dyad_));
            assert_eq!(read_kind(types, declared(4)), Read::Identity);
            assert_eq!(read_kind(types, declared(5)), Read::Aggregate);
            let f = declared(6);
            assert_eq!(read_kind(types, f), Read::Unit);
            let input = *(dyad::value(f) as *const DyadPtr).add(crate::parse::FN_INPUT);
            let mut params = ScopeStack::new();
            params.push(meta::record_scope_of(input));
            let param = |name: &str| params.resolve(&_trie, name).unwrap().binding;
            assert_eq!(read_kind(types, param("n")), Read::Scalar(NumType::I32));
            assert_eq!(read_kind(types, param("b")), Read::Container(std::ptr::null_mut()));
            assert_eq!(types.frame_of(param("b")), Some((crate::binding::Frame::Call(f), 4)));
            assert_eq!(read_kind(types, exprs[7]), Read::Scalar(NumType::I32));
            assert!(!types.is_storage(exprs[7]), "a literal is no storage");
            assert_eq!(read_kind(types, exprs[8]), Read::Literal);
            assert_eq!(read_kind(types, exprs[9]), Read::Opaque);
            assert_eq!(read_kind(types, exprs[10]), Read::Unit);
            assert_eq!(read_kind(types, exprs[11]), Read::Address);
            match read_kind(types, exprs[12]) {
                Read::Executable(Dispatch::Leaf(leaf)) => assert!(callable::is_callable(leaf)),
                other => panic!("a `+` application reads as its leaf, got {other:?}"),
            }
            assert_eq!(read_kind(types, exprs[13]), Read::Executable(Dispatch::Call(f)));
            assert_eq!(read_kind(types, exprs[14]), Read::Identity);
        }
    }

    #[test]
    fn a_node_of_a_run_type_is_a_call_of_its_function_and_a_leafless_record_is_named() {
        let (mut store, _trie, core, exprs) = parse_seq(
            "pw := type (\n\
                 a := ?, b := i32 ?, output_type := type ?, share run = ( a ),\n\
                 share parse_rank = *.parse_rank + 1,\n\
                 share associativity = right,\n\
                 share parse = (\n\
                     tape[0]:type = pw, tape[0].a = tape[-1],\n\
                     tape[0].b = tape[1],\n\
                     tape[0].output_type = tape[-1]:type,\n\
                     ,\n\
                     tape.is_constructed[0] = true,\n\
                     tape.remove(1),\n\
                     tape.remove(-1)\n\
                 )\n\
             ),\n\
             x := i32 2,\n\
             x pw 3",
        );
        let types = &core;
        // SAFETY: nodes just parsed; the hand-built node below is well-formed.
        unsafe {
            let pw = declare::declared_of(exprs[0]);
            assert_eq!(read_kind(types, pw), Read::Identity);
            let spec = crate::identities::run_body::spec_of(exprs[2]);
            assert!(!spec.is_null());
            assert_eq!(read_kind(types, exprs[2]), Read::Executable(Dispatch::Call(spec)));
            let leafless =
                store.alloc_words(core.plus, &[exprs[2], exprs[2], std::ptr::null_mut()]);
            assert_eq!(read_kind(types, leafless), Read::Executable(Dispatch::None));
        }
    }

    #[test]
    fn a_binding_laid_out_in_a_frame_is_the_storage_and_reads_by_its_type() {
        use crate::binding::{Binding, Frame};
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let name = crate::identities::string::build_text(&mut store, core.string_, b"n");
        let mut bind = |dyad: DyadPtr| {
            Binding::alloc(&mut store, core.binding_, Binding::new(dyad, core.root_scope, name))
        };
        let (top, alias, local, bare, boxed, field) = (
            bind(std::ptr::null_mut()),
            bind(core.i32_),
            bind(std::ptr::null_mut()),
            bind(std::ptr::null_mut()),
            bind(std::ptr::null_mut()),
            bind(std::ptr::null_mut()),
        );
        let f = store.alloc_leaf(core.fn_type);
        let field_node = store.alloc_leaf(core.i32_);
        let off = store.arena_alloc(4);
        // SAFETY: every handle was just minted into `store`, which outlives the reads.
        unsafe {
            Binding::lay_out(top, core.i32_, core.root_scope, off);
            Binding::lay_out(local, core.i32_, f, 8);
            Binding::lay_out(bare, std::ptr::null_mut(), f, 16);
            Binding::lay_out(boxed, core.type_, core.root_scope, 8);
            Binding::set_dyad(field, field_node);
            Binding::set_field_offset(field, core.i32_, 4);
            // Storage stands as itself, its type the declared one, its bytes in its frame.
            assert_eq!(core.through(top), top);
            assert_eq!(core.type_of(top), core.i32_);
            assert!(core.is_storage(top));
            assert_eq!(core.frame_of(top), Some((Frame::Root, off)));
            assert_eq!(read_kind(&core, top), Read::Scalar(NumType::I32));
            assert_eq!(core.frame_of(local), Some((Frame::Call(f), 8)));
            assert_eq!(read_kind(&core, local), Read::Scalar(NumType::I32));
            assert_eq!(read_kind(&core, bare), Read::Container(std::ptr::null_mut()));
            assert_eq!(read_kind(&core, boxed), Read::Container(core.type_));
            // A name for an identity hops to it; a field's binding is no storage of its own.
            assert_eq!(core.through(alias), core.i32_);
            assert!(!core.is_storage(alias));
            assert_eq!(read_kind(&core, alias), Read::Identity);
            assert_eq!(core.through(field), field_node);
            assert_eq!(core.frame_of(field), None);
            assert_eq!(core.type_of(field), core.i32_);
            // The binding type lays its own fields out, `frame` and `offset` among them.
            let fields = array::items(meta::record_fields_of(core.binding_));
            assert_eq!(fields.len(), 9);
            let mut scope = ScopeStack::new();
            scope.push(meta::record_scope_of(core.binding_));
            for (name, at) in [("dyad", 0), ("lex_rank", 48), ("frame", 56), ("offset", 64)] {
                let r = scope.resolve(&trie, name).unwrap();
                assert_eq!(Binding::read(r.binding).offset, at, "{name}");
                assert_eq!(Binding::read(r.binding).frame, core.binding_, "{name}");
            }
        }
    }

    #[test]
    fn a_value_word_is_never_read_for_a_mark() {
        // A node whose value word carries bits 63, 62 and 47 is read by its type alone: the
        // word is never followed.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let bits: *mut u8 = std::ptr::without_provenance_mut((1 << 63) | (1 << 62) | (1 << 47));
        let i64_ = core.numtypes[NumType::I64 as usize];
        let mut marked = |ty: DyadPtr| store.alloc_head(ty, bits);
        let (n, t, d, r) =
            (marked(i64_), marked(core.type_), marked(core.dyad_), marked(core.rational));
        // SAFETY: the nodes were just minted; `read_kind` reads only their type slots.
        unsafe {
            assert_eq!(read_kind(&core, n), Read::Scalar(NumType::I64));
            assert_eq!(read_kind(&core, t), Read::Identity);
            assert_eq!(read_kind(&core, d), Read::Address);
            assert_eq!(read_kind(&core, r), Read::Literal);
            assert!(!core.is_storage(n) && !core.is_storage(t));
        }
    }

    #[test]
    fn a_place_of_a_type_is_sized_by_the_same_rule() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = &core;
        // SAFETY: all handles are identities Core::build just allocated.
        unsafe {
            assert_eq!(place_layout(types, core.i32_), Some((Read::Scalar(NumType::I32), 4)));
            assert_eq!(place_layout(types, core.bool_), Some((Read::Scalar(NumType::I32), 4)));
            assert_eq!(
                place_layout(types, types.numtypes[NumType::F64 as usize]),
                Some((Read::Scalar(NumType::F64), 8))
            );
            assert_eq!(place_layout(types, core.type_), Some((Read::Container(core.type_), 8)));
            assert_eq!(place_layout(types, core.dyad_), Some((Read::Container(core.dyad_), 8)));
            assert_eq!(place_layout(types, core.rational), Some((Read::Rational, 16)));
            for t in [core.string_, core.void_, core.comment_, core.plus, core.fn_type] {
                assert_eq!(place_layout(types, t), None, "{t:p}");
            }
            assert_eq!(place_layout(types, std::ptr::null_mut()), None);
        }
        let (_store, _trie, core, exprs) = parse_seq(
            "w := type ( x := i64 ?, y := i64 ? ),\n\
             c := type ( a := i32 ?, share run = ( a ) )",
        );
        let types = &core;
        // SAFETY: the declared identities were just parsed.
        unsafe {
            let w = declare::declared_of(exprs[0]);
            assert_eq!(place_layout(types, w), Some((Read::Aggregate, 16)));
            let c = declare::declared_of(exprs[1]);
            assert_eq!(place_layout(types, c), None);
        }
    }
}
