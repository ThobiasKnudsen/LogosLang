// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The reading rule: how a node's value is read, decided once in `read_kind`, which
//! every reader asks. What decides it: the binding hop, storage by its declared type,
//! then `fn` (the one identity no record can carry), then the type's record kind byte.
//! DESIGN ›The dyad's read surface‹, ›A scope lays out its declarations…‹. Beside it,
//! `place_of`: whether a node is a place at all.

use super::callable;
use super::meta;
use super::numtype::{self, NumType, ADDR_TAG, COMMENT_TAG, STRING_TAG, VOID_TAG};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::store::Store;
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
    // A hole or a field node holds nothing.
    if op == types.unknown {
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
        debug_assert!(!dyad::head(op).is_null(), "a type with no record stands in a type slot");
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
            let leaf = *(value as *const DyadPtr).add(idx);
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

/// What a sequence, an arm or a `move` hands on from `node`: its output, a plain number read
/// as an `i32`: stand-in for #214.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn handed_on(types: &Core, node: DyadPtr) -> DyadPtr {
    if dyad::ty(types.through(node)) == types.rational {
        return types.i32_;
    }
    output_type(types, node)
}

/// The type `node` gives back when it runs, read from the node: a place's declared type, a
/// call's `-> T`, the output a Logos-written type's set was built for, the `output_type` slot
/// a built-in's parse wrote, `void` for a node that runs and declares no `output_type`; a node
/// of no other kind is a value and gives back its own type. Null where nothing knows it before
/// the program runs. DESIGN ›A node's output type is per node, and its parse writes it‹, ›`=`
/// sits beside `:=`, and returns nothing‹.
///
/// # Safety
/// `node` must be null or a reduced dyad from the store.
pub unsafe fn output_type(types: &Core, node: DyadPtr) -> DyadPtr {
    use crate::parse::FN_OUTPUT;
    let node = types.through(node);
    if node.is_null() {
        return node;
    }
    if let Some(t) = types.storage_type(node) {
        return t;
    }
    let op = dyad::ty(node);
    if dyad::ty(op) == types.fn_type {
        return *(dyad::value(op) as *const DyadPtr).add(FN_OUTPUT);
    }
    // A function standing as a value is a `fn`; its own `output_type` field is what its calls
    // give back.
    if op == types.fn_type {
        return op;
    }
    if meta::is_record_type(op) && !meta::run_body_of(op).is_null() {
        let spec = super::run_body::spec_of(node);
        return if spec.is_null() {
            spec
        } else {
            *(dyad::value(spec) as *const DyadPtr).add(FN_OUTPUT)
        };
    }
    match meta::output_slot_of(op) {
        Some(i) => *(dyad::value(node) as *const DyadPtr).add(i),
        None if meta::op_slot_of(op).is_some() => types.void_,
        None => types.logos_of(node),
    }
}

/// The type `node` gives where its value is used, or the error that says why it gives none.
/// DESIGN ›`=` sits beside `:=`, and returns nothing‹, ›`if` reads its own right side‹.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn value_type(
    types: &Core,
    node: DyadPtr,
) -> Result<DyadPtr, crate::parse::ParseError> {
    let d = types.through(node);
    if !d.is_null() && types.storage_type(d).is_none() {
        if dyad::ty(d) == types.if_ {
            return super::if_mod::value_type(types, d);
        }
        // A block's value is its last line's, refused for that line's reason.
        if dyad::ty(d) == types.scope {
            if let Some(last) = crate::parse::last_sequence_expr(d) {
                value_type(types, last)?;
            }
        }
    }
    match output_type(types, node) {
        t if t == types.void_ => Err(crate::parse::ParseError::StatementAsValue),
        t => Ok(t),
    }
}

/// Whether a line's last value is shown: a line that gives nothing shows nothing, and the echo
/// is a use, so a value refused anywhere, as an `if` whose arms give two types, is refused here.
/// DESIGN ›A value is shown as the text its type's `print` slot gives back‹, ›`if` reads its
/// own right side‹.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub unsafe fn is_shown(types: &Core, node: DyadPtr) -> Result<bool, crate::parse::ParseError> {
    use crate::parse::ParseError;
    match value_type(types, node) {
        Ok(_) => Ok(true),
        Err(
            ParseError::StatementAsValue | ParseError::MissingElse | ParseError::ArmGivesNothing,
        ) => Ok(false),
        Err(e) => Err(e),
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

/// What makes a node a place (DESIGN ›The checker reads a fact base the graph provides; the
/// rules over it are user-definable‹, Places, and ›`a[k]` is an application, exactly as
/// `a(k)`‹).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// A name, a field path, a dereference, or a cell of the tape, of its `is_constructed`
    /// flags or of a map.
    Itself,
    /// A call that ends in a dereference: its place is that dereference.
    Call(CallTail),
}

/// A call whose callee is a `fn` whose body ends in a dereference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallTail {
    call: DyadPtr,
    callee: DyadPtr,
    body: DyadPtr,
    tail: DyadPtr,
    /// The body line the dereference stands on; `None` when the body is the dereference.
    line: Option<usize>,
}

/// Whether `node` is a place, and which; `None` for a value nothing holds. A binding's own
/// field, `x:f`, is read at elaboration and is no place.
///
/// # Safety
/// `node` must be null or a reduced dyad from the store.
pub unsafe fn place_of(types: &Core, node: DyadPtr) -> Option<Place> {
    if node.is_null() {
        return None;
    }
    let ty = dyad::ty(node);
    let itself = if ty == types.field_ {
        types.binding_field_of(node).is_none()
    } else {
        ty == types.binding_
            || super::this::is_field_read(types, node)
            || is_owning_slot(node)
            || ty == types.deref_
            || ty == types.hashmap.get
            || ty == types.tape.is_constructed
            || super::tape::is_cell_read(types, node)
    };
    if itself {
        return Some(Place::Itself);
    }
    call_tail(types, node).map(Place::Call)
}

/// The node an owning field's slot holds, which a read of a node known at parse folds to:
/// only a field's slot is typed an owning pointer, never a value, and a hole's type is `?`.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
unsafe fn is_owning_slot(node: DyadPtr) -> bool {
    let ty = dyad::ty(node);
    meta::kind_of(ty) == Some(ADDR_TAG) && !meta::destructor_of(ty).is_null()
}

/// A body holding a `return` could leave with a value where the place owes an address, and
/// one whose end runs something would free what the address points at before the place is
/// used.
///
/// # Safety
/// As `place_of`.
unsafe fn call_tail(types: &Core, node: DyadPtr) -> Option<CallTail> {
    let Read::Executable(Dispatch::Call(callee)) = read_kind(types, node) else {
        return None;
    };
    if dyad::ty(callee) != types.fn_type {
        return None;
    }
    let body = *(dyad::value(callee) as *const DyadPtr).add(crate::parse::FN_BODY);
    if body.is_null() || crate::parse::contains_return(types, body) {
        return None;
    }
    let body = types.through(body);
    let (tail, line) = if dyad::ty(body) == types.scope {
        let lines = super::scope::exprs_of(body)?;
        if super::drop_model::any_held_somewhere(super::scope::exit_items(body)) {
            return None;
        }
        let i = lines.iter().rposition(|&e| !numtype::is_comment_type(dyad::ty(e)))?;
        (types.through(lines[i]), Some(i))
    } else {
        (body, None)
    };
    (dyad::ty(tail) == types.deref_).then_some(CallTail {
        call: types.through(node),
        callee,
        body,
        tail,
        line,
    })
}

/// The node every word meets a place as: `node` itself, or for a call that ends in a
/// dereference, that dereference. `None` when `node` is no place.
///
/// # Safety
/// As `place_of`.
pub unsafe fn place_node(store: &mut Store, types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    match place_of(types, node)? {
        Place::Itself => Some(node),
        Place::Call(call) => Some(call_place(store, types, call)),
    }
}

/// The call's dereference, over a call of a copy of the callee whose body yields the address
/// where the original reads through it: the body runs with the call's arguments, bounds
/// check included, and the place is where its dereference points.
///
/// # Safety
/// `call` must come from `place_of`.
pub unsafe fn call_place(store: &mut Store, types: &Core, call: CallTail) -> DyadPtr {
    use crate::parse::{FN_BCODE, FN_BODY, FN_OUTPUT, FN_RECEIVER};
    let (address, pointee, offset) = super::pointer::deref_parts(call.tail);
    let body = match call.line {
        None => address,
        Some(i) => {
            let mut lines = super::array::items(super::scope::exprs_array(call.body)).to_vec();
            lines[i] = address;
            super::scope::with_exprs(store, types, call.body, &lines)
        }
    };
    let fields = dyad::value(call.callee) as *const DyadPtr;
    let mut record: Vec<DyadPtr> = (0..=FN_RECEIVER).map(|k| *fields.add(k)).collect();
    record[FN_OUTPUT] = super::pointer::make_pointer_type(store, types.type_, pointee);
    record[FN_BODY] = body;
    record[FN_BCODE] = std::ptr::null_mut();
    let finder = store.alloc_words(dyad::ty(call.callee), &record);
    let args = crate::parse::null_terminated(dyad::value(call.call) as *const DyadPtr);
    let found = crate::parse::build_call(store, finder, args);
    super::pointer::build_deref(store, types, found, pointee, offset as usize)
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
                     tape[0].type = pw, tape[0].a = tape[-1],\n\
                     tape[0].b = tape[1],\n\
                     tape[0].output_type = tape[-1].type,\n\
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
            let leafless = store
                .alloc_words(core.plus, &[exprs[2], exprs[2], std::ptr::null_mut(), core.i32_]);
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
        let f = store.alloc_words(core.fn_type, &[std::ptr::null_mut(); crate::parse::FN_SLOTS]);
        let field_node = crate::identities::hole::build(
            &mut store,
            core.unknown,
            core.i32_,
            std::ptr::null_mut(),
        );
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
    fn a_place_is_a_name_a_field_path_a_dereference_a_cell_or_a_call_that_ends_in_one() {
        let (mut store, _trie, core, exprs) = parse_seq(
            "point := type ( x := i32 ?, y := i32 ? ),\n\
             mut v := point (1, 2),\n\
             x := i32 1,\n\
             a := &x,\n\
             get := fn (p := @i32 ?) -> i32 ( p@ ),\n\
             seven := fn () -> i32 ( 7 ),\n\
             early := fn (p := @i32 ?) -> i32 ( return p@ ),\n\
             kept := fn (p := @i32 ?) -> i32 ( q := alloc 1 of i32 2, p@ ),\n\
             v.x,\n\
             a@,\n\
             get(a),\n\
             seven(),\n\
             early(a),\n\
             kept(a),\n\
             5,\n\
             a + 1,\n\
             v:lex_rank,\n\
             @i32 ?",
        );
        let types = &core;
        // SAFETY: every node was just parsed into `store`, which is alive.
        unsafe {
            let a = declare::binding_of(exprs[3]);
            assert_eq!(place_of(types, a), Some(Place::Itself), "a name");
            assert_eq!(place_of(types, exprs[8]), Some(Place::Itself), "a field path");
            assert_eq!(place_of(types, exprs[9]), Some(Place::Itself), "a dereference");
            assert!(matches!(place_of(types, exprs[10]), Some(Place::Call(_))));
            let place = place_node(&mut store, types, exprs[10]).expect("a call place");
            assert_eq!(dyad::ty(place), types.deref_, "met as the dereference it ends in");
            let (found, pointee, _) = super::super::pointer::deref_parts(place);
            assert_eq!(pointee, core.i32_);
            // The copy of the callee yields the address its body reads through.
            let finder = dyad::ty(found);
            let output = *(dyad::value(finder) as *const DyadPtr).add(crate::parse::FN_OUTPUT);
            assert_eq!(numtype::pointee_of(output), core.i32_);
            for (k, what) in [
                (11, "a call ending in a literal"),
                (12, "a call whose body holds a `return`"),
                (13, "a call whose body holds a teardown of its own"),
                (14, "a literal"),
                (15, "an arithmetic value"),
                (16, "a binding's own field"),
                (17, "a hole"),
            ] {
                assert_eq!(place_of(types, exprs[k]), None, "{what}");
                assert_eq!(place_node(&mut store, types, exprs[k]), None, "{what}");
            }
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
