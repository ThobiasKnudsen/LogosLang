// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The generic structure walker: read any node's shape from the graph alone,
//! with no per-identity Rust. [`describe`] dispatches as `run` does, through
//! the node's type and its record. Machine code is the reflection boundary.

use crate::binding::Binding;
use crate::dyad::DyadPtr;
use crate::identities::instance;
use crate::identities::meta;
use crate::identities::numtype::{self, NumType, ADDR_TAG, COMMENT_TAG, STRING_TAG, VOID_TAG};
use crate::identities::read::{read_kind, Read};
use crate::Core;

/// One operand slot: its role-name string node and the operand standing in it
/// (null when an optional operand is absent).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    pub role: DyadPtr,
    pub node: DyadPtr,
}

/// A node's structure, read from the graph alone.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// A scalar read at its type's width (a `bool` is physically an i32).
    Scalar(NumType),
    /// The unit value: nothing to read.
    Unit,
    /// `[len: u64][bytes]`.
    Text,
    /// A comment; its substance is the punned string node.
    Prose {
        /// The string node holding the comment's text.
        text: DyadPtr,
    },
    /// An uncommitted comptime rational `[num: i64, den: i64]`.
    Fraction,
    /// An 8-byte address; the pointee type rides the pointer type's record.
    Pointer {
        /// `@T` → `T`.
        pointee: DyadPtr,
    },
    /// Fixed named operand slots: an operator or statement node, or an fn value.
    Tuple {
        /// One per role in the type's record.
        slots: Vec<Slot>,
    },
    /// Fixed named head slots then a null-terminated tail: a sequence (empty
    /// head) or a construction (`[instance, arg…, null]`).
    List {
        /// One per role in the type's record.
        head: Vec<Slot>,
        tail: Vec<DyadPtr>,
    },
    /// A record type definition: the layout its definition derived and locked.
    RecordLogos {
        /// The scope its field names are declared in.
        scope: DyadPtr,
        fields: Vec<DyadPtr>,
        size_bytes: u64,
    },
    /// An array of `dyad@` behind the node's `[len, data]` value.
    Array {
        /// The elements, in order.
        items: Vec<DyadPtr>,
    },
    /// A callable leaf: an opaque `@exec` entry under a declared convention,
    /// invoked and never read into.
    Callable {
        /// The convention identity the jump follows.
        convention: DyadPtr,
    },
    /// A calling-convention identity, named by its string node.
    Convention {
        /// The name's string node.
        name: DyadPtr,
    },
    /// The callee is the node's type, the arguments its value.
    Call {
        /// The user function being applied.
        callee: DyadPtr,
        args: Vec<DyadPtr>,
    },
    /// A value of a record type: fields at derived offsets.
    Instance {
        /// Each field's declaration node, machine type, and byte offset.
        fields: Vec<(DyadPtr, NumType, usize)>,
        size: usize,
    },
    /// The node is itself a type carrying a shared-member record.
    LogosNode {
        /// A tag from `numtype`/`meta`.
        kind: u8,
        parse_rank: f64,
        /// A callable leaf, or null for a pure delimiter or a data type with no parse role.
        constructor: DyadPtr,
        /// Null on every identity but the owning pointer.
        destructor: DyadPtr,
    },
    /// A name's binding: the dyad it names, its scope, its range, its gate set.
    /// A use of a name stores one, so a walker follows `dyad` to the value.
    /// DESIGN ›The dyad's read surface‹.
    Binding { dyad: DyadPtr, scope: DyadPtr, start: DyadPtr, end: DyadPtr, gate: DyadPtr },
    /// A place holding a node address, a `type ?` or `dyad ?` box: eight bytes
    /// known when the program runs.
    Container,
    /// A null type, a null value where operands would be, or a layout that
    /// cannot be derived.
    Undefined,
}

/// Read `node`'s structure from the graph; `types` supplies only the fixed-point
/// handles the interpreter also holds, every layout decision comes from records.
///
/// # Safety
/// `node` must be a valid dyad from the store, in the shapes the parser and
/// `Core::build` produce.
pub unsafe fn describe(types: &Core, node: DyadPtr) -> Shape {
    let logos = (*node).ty;
    if logos.is_null() {
        return Shape::Undefined;
    }
    if meta::is_record_type(node) {
        return Shape::RecordLogos {
            scope: meta::record_scope_of(node),
            fields: crate::identities::array::items(meta::record_fields_of(node)).to_vec(),
            size_bytes: meta::record_size_of(node),
        };
    }
    // A name's binding reads as its five pointers, before the instance arm
    // below would lay it out as a five-field record value.
    if logos == types.binding_ {
        let f = Binding::read(node);
        return Shape::Binding {
            dyad: f.dyad,
            scope: f.scope,
            start: f.start,
            end: f.end,
            gate: f.gate,
        };
    }
    // Asked of the same rule execution uses, so reflection and the tiers never disagree.
    if matches!(read_kind(types, node), Read::Container(_)) {
        return Shape::Container;
    }
    if meta::is_record_type(logos) {
        return match instance::layout(logos) {
            Ok((fields, size)) => Shape::Instance { fields, size },
            Err(_) => Shape::Undefined,
        };
    }
    if (*logos).ty == types.fn_type {
        return Shape::Call { callee: logos, args: scan_null_terminated((*node).value) };
    }
    let Some(kind) = meta::kind_of(logos) else {
        return Shape::Undefined; // an unbound placeholder standing as a logos
    };
    match kind {
        k if k < VOID_TAG => Shape::Scalar(numtype::of_type_node(logos)),
        VOID_TAG => Shape::Unit,
        STRING_TAG => Shape::Text,
        COMMENT_TAG => Shape::Prose { text: (*node).value.cast() },
        ADDR_TAG => Shape::Pointer { pointee: numtype::pointee_of(logos) },
        meta::ARRAY_TAG => Shape::Array { items: crate::identities::array::items(node).to_vec() },
        meta::CALLABLE_TAG => {
            Shape::Callable { convention: crate::identities::callable::convention_of(node) }
        }
        meta::CONVENTION_TAG => Shape::Convention { name: (*node).value.cast() },
        meta::FRACTION_TAG => Shape::Fraction,
        meta::TYPEREC_TAG => Shape::LogosNode {
            kind: meta::kind_of(node).unwrap_or(meta::TOKEN_TAG),
            parse_rank: meta::parse_rank_of(node),
            constructor: meta::constructor_of(node),
            destructor: meta::destructor_of(node),
        },
        meta::TUPLE_TAG | meta::LIST_TAG => operands_of(logos, node),
        _ => Shape::Undefined, // a TOKEN-kinded logos has no values
    }
}

/// A tuple's fixed slots, or a list's fixed head plus null-terminated tail,
/// per `logos`'s operand record.
///
/// # Safety
/// `logos` carries an operand record; `node.value` has the shape it declares.
unsafe fn operands_of(logos: DyadPtr, node: DyadPtr) -> Shape {
    let value = (*node).value as *const DyadPtr;
    if value.is_null() {
        return Shape::Undefined; // declared, no operands yet
    }
    let kind = meta::kind_of(logos).expect("operand records have a kind");
    let arity = meta::arity_of(logos);
    let slots =
        (0..arity).map(|i| Slot { role: meta::role_of(logos, i), node: *value.add(i) }).collect();
    match kind {
        meta::TUPLE_TAG => Shape::Tuple { slots },
        meta::LIST_TAG => Shape::List {
            head: slots,
            tail: scan_null_terminated((*node).value.add(arity * std::mem::size_of::<DyadPtr>())),
        },
        _ => unreachable!("operand records are tuple or list"),
    }
}

/// The nodes of a null-terminated `dyad@` array (empty for a null array).
///
/// # Safety
/// A non-null `value` must point at a null-terminated `dyad@` array.
unsafe fn scan_null_terminated(value: *mut u8) -> Vec<DyadPtr> {
    let p = value as *const DyadPtr;
    let mut out = Vec::new();
    if p.is_null() {
        return out;
    }
    let mut i = 0;
    while !(*p.add(i)).is_null() {
        out.push(*p.add(i));
        i += 1;
    }
    out
}

/// The text of a string node.
///
/// # Safety
/// `node` must be a string node whose store outlives the returned slice.
pub unsafe fn text_of<'a>(node: DyadPtr) -> &'a [u8] {
    crate::identities::string::text(node)
}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)] // a test reads the nodes it built a line above
mod tests {
    use super::*;
    use crate::identities::Core;
    use crate::parse::{Assoc, Parser, ScopeStack};
    use crate::regex_trie::RegexTrie;
    use crate::store::Store;

    /// One store/trie/core, parsing each source in turn.
    fn parse_all(sources: &[&str]) -> (Store, Core, Vec<DyadPtr>) {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut roots = Vec::new();
        for src in sources {
            let mut scopes = ScopeStack::new();
            scopes.push(core.root_scope);
            let mut p = Parser::new(src, &mut store, &mut trie, &core, scopes);
            roots.push(p.parse_expression().unwrap());
        }
        (store, core, roots)
    }

    #[test]
    fn a_use_of_a_name_is_its_binding_and_reads_through() {
        let (mut store, core, roots) = parse_all(&["x := i32 41", "x + 1"]);
        let types = &core;
        // SAFETY: all nodes were just parsed into the store.
        unsafe {
            let Shape::Tuple { slots } = describe(types, roots[1]) else {
                panic!("an application should be a tuple");
            };
            let Shape::Binding { dyad, scope, start, end, gate } = describe(types, slots[0].node)
            else {
                panic!("a named operand should be its binding");
            };
            assert_eq!(describe(types, dyad), Shape::Scalar(NumType::I32));
            assert_eq!(scope, core.root_scope);
            // Top level has no body array, so `start` stays null; `end` null is alive.
            assert!(start.is_null() && end.is_null() && gate.is_null());
            assert_eq!(types.through(slots[0].node), dyad);
            let mut rt = crate::run::Runtime::new(types, &mut store);
            rt.run(roots[0]).unwrap();
            assert_eq!(rt.run(roots[1]).unwrap(), 42);
        }
    }

    #[test]
    fn a_name_and_its_alias_have_two_bindings_over_one_dyad() {
        let (_store, core, roots) = parse_all(&["y := i32", "y", "i32"]);
        let types = &core;
        // SAFETY: all nodes were just parsed into the store.
        unsafe {
            let Shape::Binding { dyad: via_y, .. } = describe(types, roots[1]) else {
                panic!("a bare name is its binding");
            };
            let Shape::Binding { dyad: via_i32, .. } = describe(types, roots[2]) else {
                panic!("a bare name is its binding");
            };
            assert_eq!(via_y, via_i32, "both names point at the one dyad");
            assert_eq!(via_y, core.i32_);
            assert_ne!(roots[1], roots[2], "two names, two bindings");
        }
    }

    #[test]
    fn every_identity_declares_its_parse_members() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        use crate::identities::meta::prec;
        let cases: &[(&str, f64, bool)] = &[
            ("+", prec::ADDITIVE, true),
            ("-", prec::ADDITIVE, true),
            ("*", prec::MULTIPLICATIVE, true),
            ("/", prec::MULTIPLICATIVE, true),
            ("%", prec::MULTIPLICATIVE, true),
            ("<", prec::COMPARE, true),
            (">", prec::COMPARE, true),
            ("<=", prec::COMPARE, true),
            (">=", prec::COMPARE, true),
            ("==", prec::EQUALITY, true),
            ("!=", prec::EQUALITY, true),
            ("and", prec::AND, true),
            ("or", prec::OR, true),
            ("=", prec::DECLARE, true),
            ("42", prec::LITERAL, true),
            ("«t»", prec::LITERAL, true),
            ("return", prec::RETURN, true),
            ("logos", prec::READER, true),
            ("fn", prec::READER, true),
            ("if", prec::READER, true),
            ("not", prec::NOT, true),
            ("⊆", prec::EQUALITY, true),
            ("while", prec::READER, true),
            ("for", prec::READER, true),
            ("&", prec::ADDRESS, true),
            (".", prec::TIGHT, true),
            ("@", prec::TIGHT, true),
            ("(", prec::OPEN, true),
            (":=", prec::DECLARE, true),
            (")", prec::INERT, false),
            (",", prec::COMMA, false),
            ("->", prec::INERT, false),
            ("else", prec::INERT, false),
            ("in", prec::INERT, false),
            ("of", prec::INERT, false),
            ("..", prec::RANGE, false),
            ("i32", prec::APPLY, true),
            ("f64", prec::APPLY, true),
            ("bool", prec::INERT, false),
            ("void", prec::INERT, false),
        ];
        for &(spelling, parse_rank, has_ctor) in cases {
            let id = scopes.resolve(&trie, spelling).unwrap().identity;
            // SAFETY: every resolved identity carries its registration-built record.
            unsafe {
                assert_eq!(meta::parse_rank_of(id), parse_rank, "parse_rank of {spelling}");
                let ctor = meta::constructor_of(id);
                assert_eq!(!ctor.is_null(), has_ctor, "constructor of {spelling}");
                if has_ctor {
                    assert!(
                        crate::identities::callable::is_callable(ctor),
                        "constructor of {spelling} is a callable leaf"
                    );
                    assert_eq!(
                        crate::identities::callable::convention_of(ctor),
                        core.conv_seed_parse,
                        "constructor convention of {spelling}"
                    );
                }
                assert!(meta::destructor_of(id).is_null(), "destructor of {spelling}");
            }
        }
    }

    #[test]
    fn field_names_resolve_through_the_shared_index_alone() {
        // A field is reachable only through the shared index filtered by the record's scope.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let node = {
            let mut p = Parser::new(
                "logos (fields = (alpha := i32 ?, beta := i32 ?))",
                &mut store,
                &mut trie,
                &core,
                scopes,
            );
            p.parse_expression().unwrap()
        };
        // SAFETY: the root is the record type just parsed, from the store.
        unsafe {
            let scope = meta::record_scope_of(node);
            let falpha = crate::identities::array::items(meta::record_fields_of(node))[0];
            assert_eq!(describe(&core, falpha), Shape::Scalar(NumType::I32));
            let mut inner = ScopeStack::new();
            inner.push(scope);
            assert_eq!(inner.resolve(&trie, "alpha").unwrap().identity, falpha);
            let mut outer = ScopeStack::new();
            outer.push(core.root_scope);
            assert!(matches!(
                outer.resolve(&trie, "alpha"),
                Err(crate::parse::ResolveError::OutOfScope(_))
            ));
        }
    }

    #[test]
    fn an_fn_value_reflects_all_six_slots() {
        // A stale fn record would hide a trailing slot from reflection.
        let (_store, core, roots) = parse_all(&["fn (n := i32 ?) -> i32 ( x := n, x )"]);
        // SAFETY: the root is the fn value just parsed, from the store.
        let Shape::Tuple { slots } = (unsafe { describe(&core, roots[0]) }) else {
            panic!("an fn value reads as its fixed slots");
        };
        let roles: Vec<&[u8]> = slots.iter().map(|s| unsafe { text_of(s.role) }).collect();
        assert_eq!(roles, [b"input" as &[u8], b"output", b"body", b"bcode", b"frame", b"outer"]);
        assert!(!slots[4].node.is_null(), "a fn with locals carries its frame size");
        assert!(!slots[5].node.is_null(), "a fn whose body reads an outer name lists it");
    }

    #[test]
    fn the_outer_slot_lists_exactly_the_names_read_from_outside() {
        let (_store, _core, roots) =
            parse_all(&["n := i32 0", "fn (a := i32 ?) -> i32 ( b := a, n + b + n )"]);
        // SAFETY: the second root is the fn value just parsed; its list holds binding dyads.
        let mut names: Vec<String> = unsafe { crate::parse::fn_outer(roots[1]) }
            .iter()
            .map(|&r| unsafe {
                let name = crate::identities::binding::Binding::read(r).name;
                String::from_utf8_lossy(text_of(name)).into_owned()
            })
            .collect();
        names.sort();
        assert_eq!(names, ["+", ":=", "n"]);
    }

    #[test]
    fn a_node_box_describes_as_a_container() {
        let (_store, core, roots) = parse_all(&["a := type ?", "d := dyad ?", "x := i32 5"]);
        let types = &core;
        // SAFETY: the declare nodes were just parsed; their declared slots are the places.
        unsafe {
            let place = |i: usize| {
                let d = crate::identities::declare::declared_of(roots[i]);
                if (*d).ty == core.assign {
                    types.through(crate::identities::operands(d).0)
                } else {
                    d
                }
            };
            assert_eq!(describe(types, place(0)), Shape::Container);
            assert_eq!(describe(types, place(1)), Shape::Container);
            assert_eq!(describe(types, place(2)), Shape::Scalar(NumType::I32));
        }
    }

    #[test]
    fn every_core_identity_carries_its_record() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // SAFETY: all handles are identities Core::build just allocated.
        unsafe {
            for (id, kind, arity) in [
                (core.plus, meta::TUPLE_TAG, 3),
                (core.minus, meta::TUPLE_TAG, 3),
                (core.times, meta::TUPLE_TAG, 3),
                (core.div_, meta::TUPLE_TAG, 3),
                (core.rem_, meta::TUPLE_TAG, 3),
                (core.lt, meta::TUPLE_TAG, 3),
                (core.gt, meta::TUPLE_TAG, 3),
                (core.le, meta::TUPLE_TAG, 3),
                (core.ge, meta::TUPLE_TAG, 3),
                (core.eq, meta::TUPLE_TAG, 3),
                (core.ne, meta::TUPLE_TAG, 3),
                (core.and_, meta::TUPLE_TAG, 3),
                (core.or_, meta::TUPLE_TAG, 3),
                (core.assign, meta::TUPLE_TAG, 3),
                (core.if_, meta::TUPLE_TAG, 4),
                (core.while_, meta::TUPLE_TAG, 3),
                (core.for_, meta::TUPLE_TAG, 6),
                (core.convert, meta::TUPLE_TAG, 4),
                (core.deref_, meta::TUPLE_TAG, 4),
                (core.storeptr_, meta::TUPLE_TAG, 5),
                (core.return_, meta::TUPLE_TAG, 2),
                (core.not_, meta::TUPLE_TAG, 2),
                (core.construct_, meta::LIST_TAG, 2),
            ] {
                assert_eq!(meta::kind_of(id), Some(kind));
                assert_eq!(meta::arity_of(id), arity);
            }
            assert_eq!(meta::kind_of(core.i32_), Some(NumType::I32 as u8));
            assert_eq!(meta::kind_of(core.bool_), Some(NumType::I32 as u8));
            assert_eq!(meta::kind_of(core.void_), Some(VOID_TAG));
            assert_eq!(meta::kind_of(core.string_), Some(STRING_TAG));
            assert_eq!(meta::kind_of(core.comment_), Some(COMMENT_TAG));
            assert_eq!(meta::kind_of(core.rational), Some(meta::FRACTION_TAG));
            assert_eq!(meta::kind_of(core.type_), Some(meta::TYPEREC_TAG));
            assert_eq!(meta::kind_of(core.scope), Some(meta::TUPLE_TAG));
            assert_eq!(meta::kind_of(core.ran_), Some(meta::TUPLE_TAG));
            assert_eq!(meta::kind_of(core.array_), Some(meta::ARRAY_TAG));
            assert_eq!(meta::kind_of(core.fn_type), Some(meta::TUPLE_TAG));
            assert_eq!(meta::arity_of(core.fn_type), crate::parse::FN_OUTER + 1);
        }
    }

    #[test]
    fn parse_members_and_roles_are_graph_data() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // SAFETY: the handles are identities with records; roles are string nodes.
        unsafe {
            use crate::identities::meta::prec;
            assert_eq!(meta::parse_rank_of(core.plus), prec::ADDITIVE);
            assert_eq!(meta::assoc_of(core.plus), Assoc::Left);
            assert_eq!(meta::parse_rank_of(core.times), prec::MULTIPLICATIVE);
            assert_eq!(meta::parse_rank_of(core.assign), prec::DECLARE);
            assert_eq!(meta::assoc_of(core.assign), Assoc::Right);
            assert_eq!(meta::parse_rank_of(core.lt), prec::COMPARE);
            assert_eq!(meta::parse_rank_of(core.eq), prec::EQUALITY);

            let roles: Vec<&[u8]> = (0..meta::arity_of(core.for_))
                .map(|i| text_of(meta::role_of(core.for_, i)))
                .collect();
            assert_eq!(roles, [&b"variable"[..], b"start", b"end", b"step", b"body", b"op"]);
            assert_eq!(text_of(meta::role_of(core.return_, 0)), b"value");
            assert_eq!(text_of(meta::role_of(core.fn_type, 1)), b"output");
        }
    }

    #[test]
    fn describe_reads_a_program_from_the_graph_alone() {
        let (_store, core, roots) = parse_all(&[
            "mut x := i32 41",
            "point := logos (fields = (a := i32 ?, b := i64 ?))",
            "pt := point(3, 4)",
            "x = x + 1",
            // A runtime condition: a comptime-known one would fold the `if` away at parse.
            "if (x < 2) ( 3 ) else ( 4 )",
            "for i in 0..10 ( x = x + 1 )",
            "( 5, # prose\n 6 )",
            "inc := fn (p := @i32 ?) -> void ( p@ = p@ + 1 )",
        ]);
        let types = &core;

        // SAFETY: all nodes were just parsed into the store.
        unsafe {
            let Shape::Tuple { slots } = describe(types, roots[0]) else {
                panic!("a declaration should be a tuple");
            };
            assert_eq!(text_of(slots[0].role), b"lhs");
            assert!(matches!(describe(types, slots[0].node), Shape::Binding { .. }));
            assert_eq!(Binding::spelling(slots[0].node), "x");
            assert_eq!(text_of(slots[1].role), b"rhs");
            assert_eq!(text_of(slots[2].role), b"declared");
            let Shape::Tuple { slots: init } = describe(types, slots[2].node) else {
                panic!("a scalar binding's initializer should be a tuple");
            };
            assert_eq!(text_of(init[0].role), b"lhs");
            assert_eq!(describe(types, init[0].node), Shape::Scalar(NumType::I32));

            let Shape::Tuple { slots } = describe(types, roots[1]) else {
                panic!("a declaration should be a tuple");
            };
            let Shape::RecordLogos { scope, fields, size_bytes } = describe(types, slots[1].node)
            else {
                panic!("record definition should read its stored layout");
            };
            assert!(!scope.is_null());
            assert_eq!(fields.len(), 2);
            assert_eq!(size_bytes, 12); // i32 + i64, packed

            let Shape::Tuple { slots } = describe(types, roots[2]) else {
                panic!("a declaration should be a tuple");
            };
            let Shape::List { head, tail } = describe(types, slots[1].node) else {
                panic!("construction should be a list");
            };
            assert_eq!(text_of(head[0].role), b"instance");
            assert_eq!(tail.len(), 2);
            let Shape::Instance { fields, size } = describe(types, head[0].node) else {
                panic!("the constructed value should be an instance");
            };
            assert_eq!(size, 12);
            assert_eq!((fields[0].1, fields[0].2), (NumType::I32, 0));
            assert_eq!((fields[1].1, fields[1].2), (NumType::I64, 4));

            let Shape::Tuple { slots } = describe(types, roots[3]) else {
                panic!("assignment should be a tuple");
            };
            assert_eq!(text_of(slots[0].role), b"lhs");
            assert_eq!(text_of(slots[1].role), b"rhs");
            let Shape::Tuple { slots: sum } = describe(types, slots[1].node) else {
                panic!("the sum should be a tuple");
            };
            assert_eq!(text_of(sum[2].role), b"op");
            let Shape::Callable { convention } = describe(types, sum[2].node) else {
                panic!("the op slot should hold a callable leaf");
            };
            assert_eq!(convention, core.conv_seed_native);

            let Shape::Tuple { slots } = describe(types, roots[4]) else {
                panic!("if should be a tuple");
            };
            assert_eq!(text_of(slots[0].role), b"condition");
            assert!(!slots[2].node.is_null());

            let Shape::Tuple { slots } = describe(types, roots[5]) else {
                panic!("for should be a tuple");
            };
            assert_eq!(slots.len(), 6);
            assert_eq!(text_of(slots[3].role), b"step");
            assert!(slots[3].node.is_null());
            assert_eq!(describe(types, slots[0].node), Shape::Scalar(NumType::I32));

            let Shape::Tuple { slots } = describe(types, roots[6]) else {
                panic!("a sequence should be a tuple");
            };
            assert_eq!(text_of(slots[0].role), b"exprs");
            let Shape::Array { items } = describe(types, slots[0].node) else {
                panic!("the exprs slot should be an array");
            };
            let Shape::Prose { text } = describe(types, items[1]) else {
                panic!("the comment should be prose");
            };
            assert_eq!(text_of(text), b"prose");

            let Shape::Tuple { slots: decl } = describe(types, roots[7]) else {
                panic!("a declaration should be a tuple");
            };
            let Shape::Tuple { slots } = describe(types, decl[1].node) else {
                panic!("an fn value should be a tuple");
            };
            assert_eq!(text_of(slots[0].role), b"input");
            let Shape::RecordLogos { fields: params, .. } = describe(types, slots[0].node) else {
                panic!("the input record reads its stored layout");
            };
            let Shape::Pointer { pointee } = describe(types, params[0]) else {
                panic!("the parameter should be a pointer");
            };
            assert_eq!(pointee, core.i32_);

            let Shape::LogosNode { kind, parse_rank, constructor, destructor } =
                describe(types, core.plus)
            else {
                panic!("an identity self-describes");
            };
            assert_eq!((kind, parse_rank), (meta::TUPLE_TAG, meta::prec::ADDITIVE));
            assert!(!constructor.is_null() && destructor.is_null());
            let Shape::LogosNode { kind, parse_rank, constructor, destructor } =
                describe(types, core.i32_)
            else {
                panic!("an identity self-describes");
            };
            assert_eq!((kind, parse_rank), (NumType::I32 as u8, meta::prec::APPLY));
            assert!(!constructor.is_null() && destructor.is_null());
            let Shape::LogosNode { kind, parse_rank, constructor, destructor } =
                describe(types, core.type_)
            else {
                panic!("an identity self-describes");
            };
            assert_eq!((kind, parse_rank), (meta::TYPEREC_TAG, meta::prec::READER));
            assert!(!constructor.is_null() && destructor.is_null());
        }
    }

    #[test]
    fn the_whole_store_describes() {
        let (store, core, _roots) = parse_all(&[
            "point := logos (fields = (a := i32 ?, b := i64 ?))",
            "pt := point(3, 4)",
            "q := &pt",
            "f := fn (v := i64 ?) -> i64 ( if (v < 2) ( 1 ) else ( v * 2 ) )",
            "mut y := i64 1",
            "y = f(21)",
            "for i in 0..10 ( y = y + 1 )",
            "( «text», # prose\n 3.5 )",
        ]);
        let types = &core;

        let mut counts = std::collections::HashMap::new();
        for node in store.iter() {
            // SAFETY: `iter` yields every allocated dyad; describe only reads.
            let shape = unsafe { describe(types, node) };
            let name = match shape {
                Shape::Scalar(_) => "scalar",
                Shape::Unit => "unit",
                Shape::Text => "text",
                Shape::Prose { .. } => "prose",
                Shape::Fraction => "fraction",
                Shape::Pointer { .. } => "pointer",
                Shape::Tuple { .. } => "tuple",
                Shape::List { .. } => "list",
                Shape::Array { .. } => "array",
                Shape::Callable { .. } => "callable",
                Shape::Convention { .. } => "convention",
                Shape::Call { .. } => "call",
                Shape::Instance { .. } => "instance",
                Shape::RecordLogos { .. } => "record-logos",
                Shape::Binding { .. } => "binding",
                Shape::LogosNode { .. } => "logos",
                Shape::Container => "container",
                Shape::Undefined => "undefined",
            };
            *counts.entry(name).or_insert(0usize) += 1;
        }
        for expected in [
            "scalar", "text", "prose", "fraction", "pointer", "tuple", "list", "call", "instance",
            "logos",
        ] {
            assert!(counts.get(expected).copied().unwrap_or(0) > 0, "no {expected} described");
        }
        assert!(counts["logos"] >= 40, "core identities should describe as logos");
    }
}
