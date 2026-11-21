// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The machine instructions, one identity per arithmetic operation and machine type, each named
//! by its number type as a member, `i32.add`, and applied as a call, `i32.add(lhs, rhs)`. Each is
//! a mint of the seed's own operation of its family, so a node of it runs and lowers as that
//! family's nodes do. A mint whose run is one instruction over its fields keeps it, and its nodes
//! jump to it. ›Which machine operation an operator is (`+` over `i32` → `add_i32`) is named by
//! its mint when it is made, through the operand type's own member‹ [[R185]]

use super::numtype::{ArithOp, NumType};
use super::ops::OpLeaves;
use super::{array, meta, Cx};
use crate::dyad::{self, DyadPtr};
use crate::parse::{Constructed, ParseError, Parser, ParsingTape};
use crate::Core;

/// The member each arithmetic operation's instruction is named by, in `ArithOp`'s order.
const MEMBERS: [&str; 5] = ["add", "sub", "mul", "div", "rem"];

/// `rational_number`'s column, after the ten number types'.
const RATIONAL: usize = 10;

/// Each arithmetic instruction, `[ArithOp][NumType]` with `rational_number` last; null where no
/// machine one exists.
#[derive(Debug, Clone, Copy)]
pub struct Instructions {
    arith: [[DyadPtr; 11]; 5],
}

/// `families` holds the seed's operation of each `ArithOp`, in its order.
pub(super) fn register(
    cx: &mut Cx,
    type_: DyadPtr,
    array_: DyadPtr,
    numtypes: &[DyadPtr; 10],
    rational: DyadPtr,
    ops: &OpLeaves,
    families: [DyadPtr; 5],
) -> Instructions {
    let mut arith = [[std::ptr::null_mut(); 11]; 5];
    for (o, &family) in families.iter().enumerate() {
        let op = ArithOp::from_tag(o as u8);
        for (n, &t) in numtypes.iter().enumerate() {
            let leaf = ops.arith_leaf(op, NumType::from_tag(n as u8));
            if !leaf.is_null() {
                arith[o][n] = build(cx, type_, array_, family, t, leaf);
            }
        }
        arith[o][RATIONAL] =
            build(cx, type_, array_, family, rational, ops.rational_arith_leaf(op));
    }
    Instructions { arith }
}

fn build(
    cx: &mut Cx,
    type_: DyadPtr,
    array_: DyadPtr,
    family: DyadPtr,
    t: DyadPtr,
    leaf: DyadPtr,
) -> DyadPtr {
    let key = array::build(cx.store, array_, &[t, t]);
    // SAFETY: `family` carries its operand record of two roles; `key` was just built.
    let record = unsafe { meta::instruction_record(cx.store, family, t, leaf, key) };
    let id = cx.store.alloc_head(type_, record);
    cx.metas.insert(id, construct);
    id
}

/// The instruction the number type `t` names as its member `name`, `i32.add`; `None` for any
/// other type or member. The number types are written in the seed: stand-in for #492.
///
/// # Safety
/// `t` must be a type node from the store.
pub(crate) unsafe fn member(types: &Core, t: DyadPtr, name: &str) -> Option<DyadPtr> {
    let o = MEMBERS.iter().position(|&m| m == name)?;
    let n = if t == types.rational {
        RATIONAL
    } else if super::is_numtype_node(types, t) {
        super::numtype::of_type_node(t) as usize
    } else {
        return None;
    };
    let id = types.instructions.arith[o][n];
    (!id.is_null()).then_some(id)
}

/// The instruction `spec`, the function a mint's run was built into, names: its body is one
/// call of an instruction whose arguments are the function's parameters, the mint's fields, in
/// their order. `None` for any other body.
///
/// # Safety
/// `spec` must be a `fn` node a run body was built into.
pub(crate) unsafe fn named_by(types: &Core, spec: DyadPtr) -> Option<DyadPtr> {
    use crate::parse::{FN_BODY, FN_INPUT};
    let slots = dyad::value(spec) as *const DyadPtr;
    let body = types.through(*slots.add(FN_BODY));
    if body.is_null() {
        return None;
    }
    let line = if dyad::ty(body) == types.scope {
        match super::scope::exprs_of(body) {
            Some(&[line]) => types.through(line),
            _ => return None,
        }
    } else {
        body
    };
    if line.is_null() {
        return None;
    }
    let instruction = dyad::ty(line);
    if !types.instructions.arith.iter().flatten().any(|&i| i == instruction) {
        return None;
    }
    let parameters = array::items(meta::record_field_bindings_of(*slots.add(FN_INPUT)));
    let arguments = std::slice::from_raw_parts(dyad::value(line) as *const DyadPtr, 2);
    let same = parameters.len() == arguments.len()
        && parameters.iter().zip(arguments).all(|(&p, &a)| {
            let a = types.through(a);
            a == p
                || (dyad::ty(a) == types.binding_ && {
                    let (a, p) =
                        (super::binding::Binding::read(a), super::binding::Binding::read(p));
                    a.frame == p.frame && a.offset == p.offset
                })
        });
    same.then_some(instruction)
}

/// `i32.add(a, b)`: each argument landed in the instruction's field type, as a call's are in its
/// parameters'. Without a bracket to its right the instruction stands as itself.
fn construct(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    let Some(bracket) = p.read_arguments(tape, 1)?.filter(|c| c.is_bracket()).map(|c| c.dyad)
    else {
        return Ok(Constructed::Decline);
    };
    // SAFETY: `bracket` is the bracket's node from the store.
    let args = unsafe { p.args_of(bracket) };
    let types = p.types();
    // SAFETY: `id` is an instruction `register` made, a mint.
    let key = unsafe { super::mint::key_of(id) }.expect("an instruction is a mint");
    if args.len() != key.len() {
        return Err(ParseError::CtorArity);
    }
    let mut words = Vec::with_capacity(args.len());
    for (&arg, &t) in args.iter().zip(key) {
        // SAFETY: each argument is a reduced dyad and `t` a type node, both from the store.
        words.push(unsafe { super::land(p.store(), types, arg, t) }?);
    }
    let node = p.store().alloc_words(id, &words);
    // SAFETY: `node` was just built over reduced dyads.
    let node = unsafe { super::rational::slotted(p, node) };
    tape.remove(1);
    tape.place(node);
    Ok(Constructed::Placed)
}
