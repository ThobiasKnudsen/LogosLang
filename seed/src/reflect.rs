// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The generic structure walker: read any node's shape from the graph alone.
//!
//! DESIGN's reflectability goal (issue #42) is that a tool can walk a node's
//! structure with no per-identity Rust knowledge — the layout that says how to
//! read each value rides the graph, as each identity's shared-member record
//! ([`crate::identities::meta`]). [`describe`] is that walker, and its dispatch
//! is exactly `run`'s (DESIGN ›Execution is function application‹): consult the
//! node's type; a value of a struct type is an instance, a value of a function
//! is an application (its operands per the function's record, or a call), and
//! everything else is data read through its type's record — grounding out at
//! the `Type : Type` fixed point. The only handles it takes are the same three
//! fixed points the interpreter holds (`type`, `fn`, `struct`); everything else
//! comes from records.
//!
//! What stays opaque is machine code, not structure: a callable's entry is the
//! reflection boundary (invoked, never read into), and the parse `Construct`s
//! and Cranelift lowering remain table-keyed Rust, so [`Shape`] tells you how a
//! node is *built*, never what it computes.

use crate::dyad::DyadPtr;
use crate::identities::instance;
use crate::identities::meta;
use crate::identities::numtype::{
    self, NumType, ADDR_TAG, COMMENT_TAG, STRING_TAG, VOID_TAG,
};
use crate::parse::CoreTypes;

/// One operand slot of a [`Shape::Tuple`] or a [`Shape::List`] head: the role
/// naming it (a string node from the identity's record) and the operand node
/// standing in it (null when an optional operand is absent, like an else-less
/// `if`'s third slot).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    /// The role-name string node.
    pub role: DyadPtr,
    /// The operand in this slot, or null when absent.
    pub node: DyadPtr,
}

/// A node's structure, read from the graph alone.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// A scalar value, read at its type's width (a `bool` is physically an i32;
    /// a pointer value is its own variant below).
    Scalar(NumType),
    /// The unit type's value: nothing to read.
    Unit,
    /// Text `[len: u64][bytes]` (a `«…»` string).
    Text,
    /// A comment: prose whose substance is the punned string node.
    Prose {
        /// The string node holding the comment's text.
        text: DyadPtr,
    },
    /// An uncommitted comptime rational `[num: i64, den: i64]`.
    Fraction,
    /// A pointer value: an 8-byte address, its pointee type carried by the
    /// pointer type's record.
    Pointer {
        /// The pointee type node (`@T` → `T`).
        pointee: DyadPtr,
    },
    /// An application/value of fixed named operand slots: an operator or
    /// statement node (`[lhs, rhs, type]`, `[condition, then, else]`, …) or an
    /// fn value (`[input, output, body, bcode]`).
    Tuple {
        /// The operands, one per role in the type's record.
        slots: Vec<Slot>,
    },
    /// Fixed named head slots then a null-terminated variadic tail: a sequence
    /// (empty head), a struct definition (`[scope, field…, null]`), a
    /// construction (`[instance, arg…, null]`).
    List {
        /// The fixed, named prefix.
        head: Vec<Slot>,
        /// The variadic tail.
        tail: Vec<DyadPtr>,
    },
    /// An array of `dyad@`: the elements live behind the node's `[len, data]`
    /// value (a sequence's expression list rides in one of these).
    Array {
        /// The elements, in order.
        items: Vec<DyadPtr>,
    },
    /// A callable leaf: the complete jump information — an opaque `@exec` entry
    /// (machine code is the reflection boundary; one invokes it, never reads
    /// into it) under a declared convention.
    Callable {
        /// The convention identity the jump follows.
        convention: DyadPtr,
    },
    /// A calling-convention identity, named by its string node.
    Convention {
        /// The convention's name string node.
        name: DyadPtr,
    },
    /// A call: the callee is the node's type, the arguments its value.
    Call {
        /// The user function being applied.
        callee: DyadPtr,
        /// The arguments, in order.
        args: Vec<DyadPtr>,
    },
    /// An instance of a struct type: fields at derived offsets.
    Instance {
        /// Each field's declaration node, machine type, and byte offset.
        fields: Vec<(DyadPtr, NumType, usize)>,
        /// The instance storage's total size in bytes.
        size: usize,
    },
    /// The node is itself a type/identity carrying a shared-member record: its
    /// layout kind and parse members are the record's.
    TypeNode {
        /// The node's own record kind (a tag from `numtype`/`meta`).
        kind: u8,
        /// The node's parse precedence (0.0 when it is not an operator).
        precedence: f64,
    },
    /// Declared but not (yet) defined: a null type, a null value where operands
    /// would be, or a layout that cannot be derived.
    Undefined,
}

/// Read `node`'s structure from the graph. `types` supplies only the three
/// fixed-point handles the interpreter also holds (`type_`, `fn_type`,
/// `struct_`); every layout decision comes from the records.
///
/// # Safety
/// `node` must be a valid dyad from the store, in the shapes the parser and
/// [`crate::identities::Core::build`] produce (every identity in type position
/// carries its record).
pub unsafe fn describe(types: &CoreTypes, node: DyadPtr) -> Shape {
    let ty = (*node).ty;
    if ty.is_null() {
        return Shape::Undefined;
    }
    // A value of a struct type is an instance: its layout derives from the
    // definition's field list.
    if (*ty).ty == types.struct_ {
        return match instance::layout(ty) {
            Ok((fields, size)) => Shape::Instance { fields, size },
            Err(_) => Shape::Undefined,
        };
    }
    // A value of a function is an application — a call (the operators are
    // plain types since #44; their applications read below, per their records).
    if (*ty).ty == types.fn_type {
        return Shape::Call { callee: ty, args: scan_null_terminated((*node).value) };
    }
    // Data: read the node through its type's record, grounding at Type : Type.
    let Some(kind) = meta::kind_of(ty) else {
        return Shape::Undefined; // an unbound placeholder standing as a type
    };
    match kind {
        k if k < VOID_TAG => Shape::Scalar(numtype::of_type_node(ty)),
        VOID_TAG => Shape::Unit,
        STRING_TAG => Shape::Text,
        COMMENT_TAG => Shape::Prose { text: (*node).value.cast() },
        ADDR_TAG => Shape::Pointer { pointee: numtype::pointee_of(ty) },
        meta::ARRAY_TAG => Shape::Array {
            items: crate::identities::array::items(node).to_vec(),
        },
        meta::CALLABLE_TAG => Shape::Callable {
            convention: crate::identities::callable::convention_of(node),
        },
        meta::CONVENTION_TAG => Shape::Convention { name: (*node).value.cast() },
        meta::FRACTION_TAG => Shape::Fraction,
        meta::TYPEREC_TAG => Shape::TypeNode {
            kind: meta::kind_of(node).unwrap_or(meta::TOKEN_TAG),
            precedence: meta::precedence_of(node),
        },
        meta::TUPLE_TAG | meta::LIST_TAG => operands_of(ty, node),
        _ => Shape::Undefined, // a TOKEN-kinded type has no values
    }
}

/// Read `node`'s operands per `ty`'s operand record: a tuple's fixed slots, or
/// a list's fixed head plus null-terminated tail.
///
/// # Safety
/// `ty` carries an operand record; `node.value` has the shape it declares.
unsafe fn operands_of(ty: DyadPtr, node: DyadPtr) -> Shape {
    let value = (*node).value as *const DyadPtr;
    if value.is_null() {
        return Shape::Undefined; // declared, no operands yet
    }
    let kind = meta::kind_of(ty).expect("operand records have a kind");
    let arity = meta::arity_of(ty);
    let slots = (0..arity)
        .map(|i| Slot { role: meta::role_of(ty, i), node: *value.add(i) })
        .collect();
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

/// The text of a string node — the public face of the reflection accessor, for
/// reading a [`Slot`]'s role name or a [`Shape::Prose`]'s substance.
///
/// # Safety
/// `node` must be a string node (`{ty: string, value -> [len, bytes]}`) whose
/// store outlives the returned slice.
pub unsafe fn text_of<'a>(node: DyadPtr) -> &'a [u8] {
    crate::identities::string::text(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identities::Core;
    use crate::parse::{Assoc, Parser, ScopeStack};
    use crate::regex_trie::RegexTrie;
    use crate::store::Store;

    /// One store/trie/core/scope setup, parsing each source expression in turn.
    fn parse_all(sources: &[&str]) -> (Store, Core, Vec<DyadPtr>) {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut roots = Vec::new();
        for src in sources {
            let mut scopes = ScopeStack::new();
            scopes.push(core.root_scope);
            let mut p = Parser::new(src, &mut store, &mut trie, &core.metas, core.types(), scopes);
            roots.push(p.parse_expression().unwrap());
        }
        (store, core, roots)
    }

    /// Every core identity carries its shared-member record, with the layout
    /// kind its values need — the #42 acceptance shape.
    #[test]
    fn every_core_identity_carries_its_record() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // SAFETY: all handles are identities Core::build just allocated.
        unsafe {
            // Operator/statement identities: operand records.
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
            // Data and foundation types.
            assert_eq!(meta::kind_of(core.i32_), Some(NumType::I32 as u8));
            assert_eq!(meta::kind_of(core.bool_), Some(NumType::I32 as u8));
            assert_eq!(meta::kind_of(core.void), Some(VOID_TAG));
            assert_eq!(meta::kind_of(core.string_), Some(STRING_TAG));
            assert_eq!(meta::kind_of(core.comment_), Some(COMMENT_TAG));
            assert_eq!(meta::kind_of(core.rational), Some(meta::FRACTION_TAG));
            assert_eq!(meta::kind_of(core.type_), Some(meta::TYPEREC_TAG));
            assert_eq!(meta::kind_of(core.scope_), Some(meta::TUPLE_TAG));
            assert_eq!(meta::kind_of(core.array_), Some(meta::ARRAY_TAG));
            assert_eq!(meta::kind_of(core.struct_), Some(meta::LIST_TAG));
            assert_eq!(meta::kind_of(core.fn_type), Some(meta::TUPLE_TAG));
            assert_eq!(meta::arity_of(core.fn_type), 4);
        }
    }

    /// Precedence, associativity, and role names are graph data, read back from
    /// the records the parser now dispatches on — the #30 shared members.
    #[test]
    fn parse_members_and_roles_are_graph_data() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // SAFETY: the handles are identities with records; roles are string nodes.
        unsafe {
            assert_eq!(meta::precedence_of(core.plus), 2.0);
            assert_eq!(meta::assoc_of(core.plus), Assoc::Left);
            assert_eq!(meta::precedence_of(core.times), 3.0);
            assert_eq!(meta::precedence_of(core.assign), 1.0);
            assert_eq!(meta::assoc_of(core.assign), Assoc::Right);
            assert_eq!(meta::precedence_of(core.lt), 1.5);
            assert_eq!(meta::precedence_of(core.eq), 1.4);

            let roles: Vec<&[u8]> =
                (0..meta::arity_of(core.for_)).map(|i| text_of(meta::role_of(core.for_, i))).collect();
            assert_eq!(roles, [&b"variable"[..], b"start", b"end", b"step", b"body", b"op"]);
            assert_eq!(text_of(meta::role_of(core.return_, 0)), b"value");
            assert_eq!(text_of(meta::role_of(core.fn_type, 1)), b"output");
        }
    }

    /// The walker reads a real program's structure — operators, control flow,
    /// structs, pointers, calls, comments — from the graph alone: no `Construct`,
    /// no `metas`, no per-identity Rust.
    #[test]
    fn describe_reads_a_program_from_the_graph_alone() {
        let (_store, core, roots) = parse_all(&[
            "x := i32 41",
            "point := struct (a : i32, b : i64)",
            "pt := point(3, 4)",
            "x = x + 1",
            "if (1 < 2) ( 3 ) else ( 4 )",
            "for i in 0..10 ( x = x + 1 )",
            "( 5, # prose\n 6 )",
            "inc := fn (p : @i32) -> void ( p@ = p@ + 1 )",
        ]);
        let types = core.types();

        // SAFETY: all nodes were just parsed into the store.
        unsafe {
            // `x := i32 41` is a declare node: the spelling and the snapshot
            // initializer (`place = 41`) that fills `x`'s storage — the binding
            // is graph structure, like the construction below. The initializer's
            // target (its `lhs`) is the i32 variable the name resolves to.
            let Shape::Tuple { slots } = describe(&types, roots[0]) else {
                panic!("a declaration should be a tuple");
            };
            assert_eq!(text_of(slots[0].role), b"name");
            assert_eq!(text_of(slots[0].node), b"x");
            let Shape::Tuple { slots: init } = describe(&types, slots[1].node) else {
                panic!("a scalar binding's initializer should be a tuple");
            };
            assert_eq!(text_of(init[0].role), b"lhs");
            assert_eq!(describe(&types, init[0].node), Shape::Scalar(NumType::I32));

            // The struct definition, behind its declaration: [scope, a, b] —
            // one named head, two fields.
            let Shape::Tuple { slots } = describe(&types, roots[1]) else {
                panic!("a declaration should be a tuple");
            };
            let Shape::List { head, tail } = describe(&types, slots[1].node) else {
                panic!("struct definition should be a list");
            };
            assert_eq!(head.len(), 1);
            assert_eq!(text_of(head[0].role), b"scope");
            assert_eq!(tail.len(), 2);

            // The construction, behind its declaration: [instance, op | args…];
            // the instance lays out a:0, b:4.
            let Shape::Tuple { slots } = describe(&types, roots[2]) else {
                panic!("a declaration should be a tuple");
            };
            let Shape::List { head, tail } = describe(&types, slots[1].node) else {
                panic!("construction should be a list");
            };
            assert_eq!(text_of(head[0].role), b"instance");
            assert_eq!(tail.len(), 2);
            let Shape::Instance { fields, size } = describe(&types, head[0].node) else {
                panic!("the constructed value should be an instance");
            };
            assert_eq!(size, 12);
            assert_eq!((fields[0].1, fields[0].2), (NumType::I32, 0));
            assert_eq!((fields[1].1, fields[1].2), (NumType::I64, 4));

            // `x = x + 1`: a [lhs, rhs] tuple whose rhs is a [lhs, rhs, type] tuple.
            let Shape::Tuple { slots } = describe(&types, roots[3]) else {
                panic!("assignment should be a tuple");
            };
            assert_eq!(text_of(slots[0].role), b"lhs");
            assert_eq!(text_of(slots[1].role), b"rhs");
            let Shape::Tuple { slots: sum } = describe(&types, slots[1].node) else {
                panic!("the sum should be a tuple");
            };
            assert_eq!(text_of(sum[2].role), b"op");
            let Shape::Callable { convention } = describe(&types, sum[2].node) else {
                panic!("the op slot should hold a callable leaf");
            };
            assert_eq!(convention, core.conv_seed_native);

            // The if: [condition, then, else], all present here.
            let Shape::Tuple { slots } = describe(&types, roots[4]) else {
                panic!("if should be a tuple");
            };
            assert_eq!(text_of(slots[0].role), b"condition");
            assert!(!slots[2].node.is_null());

            // The for: [variable, start, end, step, body, op], the step absent.
            let Shape::Tuple { slots } = describe(&types, roots[5]) else {
                panic!("for should be a tuple");
            };
            assert_eq!(slots.len(), 6);
            assert_eq!(text_of(slots[3].role), b"step");
            assert!(slots[3].node.is_null());
            assert_eq!(describe(&types, slots[0].node), Shape::Scalar(NumType::I32));

            // The sequence: `[exprs, op]` — its expression list rides behind
            // the array node in the first slot; the middle element is prose.
            let Shape::Tuple { slots } = describe(&types, roots[6]) else {
                panic!("a sequence should be a tuple");
            };
            assert_eq!(text_of(slots[0].role), b"exprs");
            let Shape::Array { items } = describe(&types, slots[0].node) else {
                panic!("the exprs slot should be an array");
            };
            let Shape::Prose { text } = describe(&types, items[1]) else {
                panic!("the comment should be prose");
            };
            assert_eq!(text_of(text), b"prose");

            // The fn value, behind its declaration: [input, output, body, bcode];
            // `p : @i32` describes as a pointer to i32.
            let Shape::Tuple { slots: decl } = describe(&types, roots[7]) else {
                panic!("a declaration should be a tuple");
            };
            let Shape::Tuple { slots } = describe(&types, decl[1].node) else {
                panic!("an fn value should be a tuple");
            };
            assert_eq!(text_of(slots[0].role), b"input");
            let Shape::List { tail: params, .. } = describe(&types, slots[0].node) else {
                panic!("the input struct should be a list");
            };
            let Shape::Pointer { pointee } = describe(&types, params[0]) else {
                panic!("the parameter should be a pointer");
            };
            assert_eq!(pointee, core.i32_);

            // Identities self-describe as types: their kind and precedence.
            assert_eq!(
                describe(&types, core.plus),
                Shape::TypeNode { kind: meta::TUPLE_TAG, precedence: 2.0 }
            );
            assert_eq!(
                describe(&types, core.i32_),
                Shape::TypeNode { kind: NumType::I32 as u8, precedence: 0.0 }
            );
            assert_eq!(
                describe(&types, core.type_),
                Shape::TypeNode { kind: meta::TYPEREC_TAG, precedence: 0.0 }
            );
        }
    }

    /// The whole store — every identity, application, literal, scope, record
    /// role, and storage node a real program creates — describes without panics,
    /// and the shapes cover the expected variety.
    #[test]
    fn the_whole_store_describes() {
        let (store, core, _roots) = parse_all(&[
            "point := struct (a : i32, b : i64)",
            "pt := point(3, 4)",
            "q := &pt",
            "f := fn (v : i64) -> i64 ( if (v < 2) ( 1 ) else ( v * 2 ) )",
            "y := i64 1",
            "y = f(21)",
            "for i in 0..10 ( y = y + 1 )",
            "( «text», # prose\n 3.5 )",
        ]);
        let types = core.types();

        let mut counts = std::collections::HashMap::new();
        for node in store.iter() {
            // SAFETY: `iter` yields every allocated dyad; describe only reads.
            let shape = unsafe { describe(&types, node) };
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
                Shape::TypeNode { .. } => "type",
                Shape::Undefined => "undefined",
            };
            *counts.entry(name).or_insert(0usize) += 1;
        }
        for expected in
            ["scalar", "text", "prose", "fraction", "pointer", "tuple", "list", "call", "instance", "type"]
        {
            assert!(counts.get(expected).copied().unwrap_or(0) > 0, "no {expected} described");
        }
        // Every registered identity self-describes; the walker sees at least the
        // ones the Core exposes by handle.
        assert!(counts["type"] >= 40, "core identities should describe as types");
    }
}
