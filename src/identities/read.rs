// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The reading rule: how a node's value is read, decided once.
//!
//! DESIGN ›Declarations are immutable by default‹: "the type defines how the
//! value is read"; ›The dyad's read surface‹: "two fields, and the type answers
//! the rest". Before this file the seed had about a hundred places that each
//! decided by hand what a node's `.value` pointed at — scalar storage at some
//! width, an 8-byte container holding a node address, the node's own address, a
//! record's fields, a rational's fraction — and the interpreter's and the
//! compiler's data paths were two copies of that decision in different orders.
//! Every crash of 12 September 2026 (#74 #75 #76 #85 #118) was one of those
//! guesses reading the wrong thing. [`read_kind`] is the one place the question
//! is answered (#82, rule R1); every reader asks it.
//!
//! What decides the answer, in order:
//!
//! 1. A use of a name is its record; the rule hops to the dyad it names first.
//! 2. A node whose type is *a function* is a call. This must come before any
//!    record is read: a function node's value is an operand array, so reading
//!    its first byte as a kind tag would be exactly the crash class above.
//! 3. The `fn` literal standing as a statement yields unit. Its own record is an
//!    operand record like `+`'s, so the kind byte cannot tell them apart; only
//!    the identity can, and its op slot is `frame`, a byte count.
//! 4. The **type's** record kind byte — `kind_of((*node).ty)`, never
//!    `kind_of(node)`: a value node's own bytes are its value, not a record.
//! 5. The **node's** place mark, [`crate::dyad::is_place`], which says whether
//!    `.value` is storage or the datum itself. It matters only where the datum
//!    is itself an address (a type, a dyad, a bare parameter's container): a
//!    numeric literal committed at parse holds untagged storage and reads as a
//!    scalar all the same.
//! 6. For a record type, whether it carries a `code`: then a node of it is a
//!    call of that code (›Execution is function application‹), else an instance.
//!
//! So: the type's record, the node's mark, and the one identity no record can
//! carry (`fn`). Nothing here consults a list of identity names.

use super::callable;
use super::meta;
use super::numtype::{self, NumType, ADDR_TAG, COMMENT_TAG, STRING_TAG, VOID_TAG};
use crate::dyad::{is_place, DyadPtr};
use crate::parse::CoreTypes;

/// How a node's value is read. `Copy` and register-sized: it is asked on every
/// interpreted value read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// A numeric or bool value: load at the type's width. The place mark does
    /// not matter here — a declared place, a committed literal, and a
    /// reflection scalar all hold storage the type says how to read.
    Scalar(NumType),
    /// A pointer value: eight bytes holding an address, its pointee the type
    /// node carried. Read like a `U64`, but it is an address, not a number —
    /// which is why a literal may fill a scalar and never a pointer, and why
    /// the pointee is here for `@` and `=` to consult.
    Pointer(DyadPtr),
    /// A place holding a node address: eight bytes, read as the address they
    /// hold. Carries the declared type — `type` for a `type ?` box, `dyad` for
    /// a `dyad ?` box, null for a bare parameter's slot — so a writer can say
    /// what the box may take without reading the node's type slot again.
    Container(DyadPtr),
    /// A logos standing as a value: its own address is the value (›A type is a
    /// comptime value‹: "a logos node standing as a value carries its identity
    /// as its value").
    Identity,
    /// A dyad view (`x:dyad`): the stored address is the value.
    Address,
    /// A comptime rational: molded to its integer value on read.
    Literal,
    /// A record instance place: read by field or by address, never whole. No
    /// size rides here — nobody reads one today and computing it allocates;
    /// the calling convention (#115) adds it when a copy needs it.
    Aggregate,
    /// A blob with no whole-value read: text, unit, a regex, an array, a
    /// callable, a convention, a parse-only identity. Refused as it always was.
    Opaque,
    /// Prose, or a `fn` literal standing as a statement: yields 0.
    Unit,
    /// Not data: run or lower by dispatch.
    Executable(Dispatch),
    /// Declared, nothing to read yet: a bare hole, a slot marker, a fresh
    /// spelling's dyad.
    Undefined,
}

/// Where an executable node's behaviour lives. Carried out of [`read_kind`] so
/// neither tier re-derives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// A call: the callee is the node's type, or that type's `code`.
    Call(DyadPtr),
    /// An operator or statement application: the callable leaf its constructor
    /// stored in the node's op slot, already checked to be one. The compiler
    /// keys its lowering table on the node's type, which it has in hand.
    Leaf(DyadPtr),
    /// An operand record with no leaf in its op slot — the tape's path markers
    /// `cell_value` and `cell_operands`, or a node built without one. Today
    /// this fell off the end of the interpreter's ladder; now it is a named
    /// refusal.
    None,
}

/// The reading rule for `node`. See the module doc for what decides it.
///
/// # Safety
/// `node` must be null or a valid dyad from the store, as the parser and
/// [`super::Core::build`] produce; every identity standing in a type slot
/// carries its record.
pub unsafe fn read_kind(types: &CoreTypes, node: DyadPtr) -> Read {
    // 1. A use of a name reads through to what it names.
    let node = types.through(node);
    if node.is_null() {
        return Read::Undefined;
    }
    let op = (*node).ty;
    let value = (*node).value;
    let place = is_place(value);
    // A node with no type: a bare parameter's slot holds the container its
    // call bound; a hole, a slot marker, or a fresh spelling holds nothing.
    if op.is_null() {
        return if place { Read::Container(op) } else { Read::Undefined };
    }
    // 2. A value of a function is a call — before any record read.
    if (*op).ty == types.fn_type {
        return Read::Executable(Dispatch::Call(op));
    }
    // 3. The `fn` literal as a statement.
    if op == types.fn_type {
        return Read::Unit;
    }
    // 4. The type's record kind.
    let Some(kind) = meta::kind_of(op) else {
        // No reachable node is classified by a type that carries no record:
        // the four roots are back-filled in `Core::build`, and the markers and
        // holes that do have null values never stand in a type slot. Asserted
        // rather than assumed, so the debug suite is the proof that
        // `is_scalar_type`'s null-value default can go (#82, step 10).
        debug_assert!(!(*op).value.is_null(), "a type with no record stands in a type slot");
        return if place { Read::Container(op) } else { Read::Undefined };
    };
    // 5 and 6.
    match kind {
        k if k < VOID_TAG => Read::Scalar(numtype::of_type_node(op)),
        ADDR_TAG => Read::Pointer(numtype::pointee_of(op)),
        meta::TYPEREC_TAG => {
            if place {
                Read::Container(op)
            } else {
                Read::Identity
            }
        }
        meta::DYAD_TAG => {
            if place {
                Read::Container(op)
            } else {
                Read::Address
            }
        }
        meta::RECORD_TAG => {
            let code = meta::code_of(op);
            if !place && !code.is_null() {
                Read::Executable(Dispatch::Call(code))
            } else {
                Read::Aggregate
            }
        }
        meta::TUPLE_TAG | meta::LIST_TAG => {
            if place {
                return Read::Container(op);
            }
            // The op slot: the last fixed slot of the type's operand record.
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
        meta::FRACTION_TAG => {
            if place {
                Read::Container(op)
            } else {
                Read::Literal
            }
        }
        COMMENT_TAG => {
            if place {
                Read::Container(op)
            } else {
                Read::Unit
            }
        }
        // VOID_TAG, STRING_TAG, ARRAY_TAG, CALLABLE_TAG, CONVENTION_TAG,
        // TOKEN_TAG: no whole-value read; a place of one holds a container.
        _ => {
            debug_assert!(matches!(
                kind,
                VOID_TAG
                    | STRING_TAG
                    | meta::ARRAY_TAG
                    | meta::CALLABLE_TAG
                    | meta::CONVENTION_TAG
                    | meta::TOKEN_TAG
            ));
            if place {
                Read::Container(op)
            } else {
                Read::Opaque
            }
        }
    }
}

/// The reading rule asked of a *type*: what a place of `t` reads as, and how
/// many bytes it takes — or `None` when no place of `t` can exist. This is the
/// allocation side of [`read_kind`]: `construct_hole` and `parse_fn` size a
/// place from it, and a read of that place must agree with them or two frame
/// slots overlap, which no read-side check can catch. One table for both.
///
/// A numeric or pointer type is a scalar at its width; `type` and `dyad` are
/// boxes holding a node address; a record type is an aggregate of its layout's
/// size, unless it carries a `code` — then a node of it is a call, never a
/// place (#63). Everything else (text, void, prose, a parse-only identity)
/// has no place. `bool` is a 4-byte scalar by kind; that a `bool ?` place is
/// still refused is `construct_hole`'s stated exception (#47), not this rule's.
///
/// # Safety
/// `t` must be null or a valid dyad from the store.
pub unsafe fn place_layout(types: &CoreTypes, t: DyadPtr) -> Option<(Read, usize)> {
    let t = super::type_identity_of(types, t)?;
    let kind = meta::kind_of(t)?;
    match kind {
        k if k < VOID_TAG => {
            let nt = numtype::of_type_node(t);
            Some((Read::Scalar(nt), nt.bytes()))
        }
        ADDR_TAG => Some((Read::Pointer(numtype::pointee_of(t)), 8)),
        meta::TYPEREC_TAG | meta::DYAD_TAG => Some((Read::Container(t), 8)),
        meta::RECORD_TAG => {
            if !meta::code_of(t).is_null() {
                return None;
            }
            Some((Read::Aggregate, (meta::record_size_of(t) as usize).max(1)))
        }
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

    /// Parse `src` as one top-level sequence in a fresh core and return the
    /// sequence's expressions in order, with the core and store kept alive.
    /// An item the pass had to run stands in the sequence as its ran form
    /// ([`crate::identities::ran`]); these tests ask what each item *is*, so
    /// they look through it.
    fn parse_seq(src: &str) -> (Store, Core, Vec<DyadPtr>) {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let seq = {
            let mut scopes = ScopeStack::new();
            scopes.push(core.root_scope);
            let mut p = Parser::new(src, &mut store, &mut trie, core.types(), scopes);
            p.parse_sequence().unwrap()
        };
        let types = core.types();
        // SAFETY: a sequence node's first slot is its expression array.
        let exprs = unsafe {
            array::items(*((*seq).value as *const DyadPtr))
                .iter()
                .map(|&e| crate::identities::ran::expr_of(&types, e))
                .collect()
        };
        (store, core, exprs)
    }

    #[test]
    fn every_identity_is_an_identity() {
        // The classifier ground and everything classified by it — including the
        // four roots that are minted null and back-filled in `Core::build`, the
        // numeric logos, and the two associativity values `left`/`right`, which
        // is the one place `Identity` is wider than "is the type root".
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = core.types();
        // SAFETY: all handles are identities Core::build just allocated.
        unsafe {
            for id in [
                core.type_,
                core.fn_type,
                core.scope_,
                core.ran_,
                core.record_,
                core.i32_,
                core.bool_,
                core.void,
                core.string_,
                core.rational,
                core.dyad_,
                core.plus,
                types.left_,
                types.right_,
            ] {
                assert_eq!(read_kind(&types, id), Read::Identity, "{id:p}");
            }
            for &n in &types.numtypes {
                assert_eq!(read_kind(&types, n), Read::Identity);
            }
            // The six slot markers have null slots on both sides.
            for &m in &types.slots {
                assert_eq!(read_kind(&types, m), Read::Undefined);
            }
        }
    }

    #[test]
    fn places_read_by_their_type_and_values_by_their_kind() {
        let (_store, core, exprs) = parse_seq(
            "x := i32 5,\n\
             p := &x,\n\
             a := type ?,\n\
             d := dyad ?,\n\
             w := type ( instance (y := i64 ?) ),\n\
             q := w(7),\n\
             f := fn (n := i32 ?, b) -> i32 ( n ),\n\
             true,\n\
             5,\n\
             «hi»,\n\
             # prose\n\
             x:dyad,\n\
             i32 1 + i32 2,\n\
             f(1, 2),\n\
             ?",
        );
        let types = core.types();
        // SAFETY: every node was just parsed into `_store`, which is alive.
        unsafe {
            // A declaration's "declared" slot is the binding, or the initializer
            // that fills it: a scalar or pointer binding snapshots through a
            // `place = value` store, a record binding through a construction
            // whose head is the instance. The place is behind either.
            let declared = |i: usize| {
                let d = declare::declared_of(exprs[i]);
                if (*d).ty == core.assign {
                    types.through(crate::identities::operands(d).0)
                } else if (*d).ty == core.construct_ {
                    *((*d).value as *const DyadPtr)
                } else {
                    d
                }
            };
            // A declared numeric place is read at its width; the place itself is
            // marked, its `&` is a pointer place read as an address.
            assert_eq!(read_kind(&types, declared(0)), Read::Scalar(NumType::I32));
            assert!(is_place((*declared(0)).value));
            assert_eq!(read_kind(&types, declared(1)), Read::Pointer(core.i32_));
            // The two node boxes hold a container.
            assert_eq!(read_kind(&types, declared(2)), Read::Container(core.type_));
            assert_eq!(read_kind(&types, declared(3)), Read::Container(core.dyad_));
            // A record type is an identity; its instance is an aggregate place.
            assert_eq!(read_kind(&types, declared(4)), Read::Identity);
            assert_eq!(read_kind(&types, declared(5)), Read::Aggregate);
            // A fn literal stands as a statement; its parameters are a scalar
            // frame place and, for the bare `b`, a container.
            let f = declared(6);
            assert_eq!(read_kind(&types, f), Read::Unit);
            let input = *((*f).value as *const DyadPtr).add(crate::parse::FN_INPUT);
            let params = array::items(meta::record_fields_of(input));
            assert_eq!(read_kind(&types, params[0]), Read::Scalar(NumType::I32));
            assert_eq!(read_kind(&types, params[1]), Read::Container(std::ptr::null_mut()));
            // Values by kind: a bool literal is scalar storage (untagged, still
            // storage), a rational is a literal, text is opaque, prose is unit,
            // a view is an address, an application is its leaf, a call is a
            // call, a bare hole is undefined.
            assert_eq!(read_kind(&types, exprs[7]), Read::Scalar(NumType::I32));
            assert!(!is_place((*exprs[7]).value), "a literal's storage carries no mark");
            assert_eq!(read_kind(&types, exprs[8]), Read::Literal);
            assert_eq!(read_kind(&types, exprs[9]), Read::Opaque);
            assert_eq!(read_kind(&types, exprs[10]), Read::Unit);
            assert_eq!(read_kind(&types, exprs[11]), Read::Address);
            match read_kind(&types, exprs[12]) {
                Read::Executable(Dispatch::Leaf(leaf)) => assert!(callable::is_callable(leaf)),
                other => panic!("a `+` application reads as its leaf, got {other:?}"),
            }
            assert_eq!(read_kind(&types, exprs[13]), Read::Executable(Dispatch::Call(f)));
            assert_eq!(read_kind(&types, exprs[14]), Read::Undefined);
        }
    }

    #[test]
    fn a_code_carrying_type_is_a_call_and_a_leafless_record_is_named() {
        let (mut store, core, exprs) = parse_seq(
            "pw := type (\n\
                 parse_rank = *.parse_rank + 1,\n\
                 associativity = right,\n\
                 constructor = fn (tape := parsing_tape ?) -> void (\n\
                     tape[0]:dyad.type = pw,\n\
                     tape[0]:dyad.value.operands.append(tape[-1] and tape[1]),\n\
                     tape.remove(1),\n\
                     tape.remove(-1)\n\
                 ),\n\
                 code = fn (a := i32 ?, b := i32 ?) -> i32 ( a )\n\
             ),\n\
             2 pw 3",
        );
        let types = core.types();
        // SAFETY: nodes just parsed; the hand-built node below is well-formed.
        unsafe {
            let pw = declare::declared_of(exprs[0]);
            assert_eq!(read_kind(&types, pw), Read::Identity);
            let code = meta::code_of(pw);
            assert!(!code.is_null());
            assert_eq!(read_kind(&types, exprs[1]), Read::Executable(Dispatch::Call(code)));
            // An operand record whose op slot holds no leaf is a named refusal,
            // not a fall-through: the shape of the tape's path markers.
            let value = store.alloc_operands(&[exprs[1], exprs[1], std::ptr::null_mut()]);
            let leafless = store.alloc_raw(core.plus, value);
            assert_eq!(read_kind(&types, leafless), Read::Executable(Dispatch::None));
        }
    }

    #[test]
    fn a_place_of_a_type_is_sized_by_the_same_rule() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let types = core.types();
        // SAFETY: all handles are identities Core::build just allocated.
        unsafe {
            assert_eq!(place_layout(&types, core.i32_), Some((Read::Scalar(NumType::I32), 4)));
            assert_eq!(place_layout(&types, core.bool_), Some((Read::Scalar(NumType::I32), 4)));
            assert_eq!(
                place_layout(&types, types.numtypes[NumType::F64 as usize]),
                Some((Read::Scalar(NumType::F64), 8))
            );
            assert_eq!(place_layout(&types, core.type_), Some((Read::Container(core.type_), 8)));
            assert_eq!(place_layout(&types, core.dyad_), Some((Read::Container(core.dyad_), 8)));
            // No place: text, void, prose, a parse-only identity, a non-identity.
            for t in [core.string_, core.void, core.comment_, core.plus, core.fn_type] {
                assert_eq!(place_layout(&types, t), None, "{t:p}");
            }
            assert_eq!(place_layout(&types, std::ptr::null_mut()), None);
        }
        // A record type is an aggregate of its size; a code-carrying one has no
        // place, since a node of it is a call.
        let (_store, core, exprs) = parse_seq(
            "w := type ( instance (x := i64 ?, y := i64 ?) ),\n\
             c := type ( code = fn (a := i32 ?) -> i32 ( a ) )",
        );
        let types = core.types();
        // SAFETY: the declared identities were just parsed.
        unsafe {
            let w = declare::declared_of(exprs[0]);
            assert_eq!(place_layout(&types, w), Some((Read::Aggregate, 16)));
            let c = declare::declared_of(exprs[1]);
            assert_eq!(place_layout(&types, c), None);
        }
    }
}
