// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The concrete machine operations: `add_i32`, `lt_f64`, `store_u8`, … — one
//! spelling-less identity per (operation, machine type), each a [`callable`]
//! value immutably carrying its `@exec` (DESIGN ›Concrete machine operations
//! are identities‹; issue #44).
//!
//! A surface operator (`+`, `<`, `=`) is a parse-time constructor owning no
//! code: it *resolves* each application to one of these leaves and stores the
//! leaf in the node's op slot, so nothing mutates and nothing is looked up in
//! a side table at run time — to evaluate a node, read its op slot and jump.
//! The ~120 machine ops the original design worried would be ~120 files are
//! ~120 graph *nodes*, registered here from one table-driven loop; their
//! bodies stay the shared type-switched helpers in [`super::numtype`], each
//! shim a monomorphic wrapper with its (operation, type) pair baked in as
//! const generics.
//!
//! Float remainder mints no leaf: `%` over floats is rejected at parse
//! (Cranelift has no float remainder), so a node referencing it cannot exist.

use crate::dyad::DyadPtr;
use crate::run::{RunError, RunFn, Runtime};

use super::callable::{self, Callables};
use super::numtype::{apply_arith, apply_compare, write_scalar_nt, ArithOp, CmpOp, NumType};
use super::{operands, Cx};

/// The concrete-op leaves, indexed by operation and [`NumType`] — the parse-time
/// resolver's table (`(family, operand type) → leaf`). Rides [`crate::parse::CoreTypes`]
/// so the `Construct` builders can resolve; the interpreter never consults it
/// (each shim's type is baked in), and it retires into versioned scopes with the
/// rest of the Rust-side parse tables at self-hosting.
#[derive(Clone, Copy, Debug)]
pub struct OpLeaves {
    /// `[ArithOp][NumType]` → leaf; null only for the unmintable float remainders.
    pub(crate) arith: [[DyadPtr; 10]; 5],
    /// `[CmpOp][NumType]` → leaf.
    pub(crate) cmp: [[DyadPtr; 10]; 6],
    /// `[NumType]` → the `=` store leaf writing at that width (a pointer target
    /// stores as its 8-byte address, `U64`, per `numtype::of_type_node`).
    pub(crate) store: [DyadPtr; 10],
    /// `and`'s short-circuiting native — a single leaf (bool has one width),
    /// minted by [`super::and::register`].
    pub(crate) and_: DyadPtr,
    /// `or`'s short-circuiting native, minted by [`super::or::register`].
    pub(crate) or_: DyadPtr,
    /// `convert`'s native — a single leaf; its from/to pair rides the node as
    /// graph data. Minted by [`super::convert::register`].
    pub(crate) convert_: DyadPtr,
    /// The statement natives, one leaf each, minted by their identities'
    /// registrations: control flow branches on graph structure, so no
    /// per-machine-type variants exist.
    pub(crate) if_: DyadPtr,
    /// `while`'s native.
    pub(crate) while_: DyadPtr,
    /// `for`'s native.
    pub(crate) for_: DyadPtr,
    /// `return`'s native.
    pub(crate) return_: DyadPtr,
    /// `not`'s native.
    pub(crate) not_: DyadPtr,
    /// `construct`'s native (struct construction).
    pub(crate) construct_: DyadPtr,
    /// `deref`'s native (postfix `@`).
    pub(crate) deref_: DyadPtr,
    /// `storeptr`'s native (`=` through a deref).
    pub(crate) storeptr_: DyadPtr,
    /// `addr`'s native (prefix `&`): resolves a place's address at run time.
    pub(crate) addr_: DyadPtr,
    /// `scope`'s sequence native (run the array in order, yield the tail).
    pub(crate) scope_: DyadPtr,
    /// `declare`'s native (run the initializer for effect, yield unit).
    pub(crate) declare_: DyadPtr,
    /// `compile`'s native (`f.compile()`, the fn type's shared member).
    pub(crate) compile_: DyadPtr,
}

impl OpLeaves {
    /// The arithmetic leaf for `op` over `nt`.
    pub(crate) fn arith_leaf(&self, op: ArithOp, nt: NumType) -> DyadPtr {
        self.arith[op as usize][nt as usize]
    }

    /// The comparison leaf for `op` over `nt`.
    pub(crate) fn cmp_leaf(&self, op: CmpOp, nt: NumType) -> DyadPtr {
        self.cmp[op as usize][nt as usize]
    }

    /// The store leaf writing at `nt`'s width.
    pub(crate) fn store_leaf(&self, nt: NumType) -> DyadPtr {
        self.store[nt as usize]
    }
}

/// Run a binary arithmetic node with the (operation, type) pair baked in:
/// evaluate both operands and apply the shared helper. The concrete op never
/// reads a type from the node — its type *is* this instantiation.
fn arith_run<const OP: u8, const NT: u8>(
    rt: &mut Runtime,
    node: DyadPtr,
) -> Result<i64, RunError> {
    // SAFETY: `node` is a resolved binary operator application whose first two
    // slots are its operands, as the family builders construct.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = rt.run(lhs)?;
        let r = rt.run(rhs)?;
        Ok(apply_arith(ArithOp::from_tag(OP), NumType::from_tag(NT), l, r))
    }
}

/// Run a binary comparison node with the (operation, type) pair baked in; the
/// result is the i32 0/1 bool.
fn cmp_run<const OP: u8, const NT: u8>(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as [`arith_run`].
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = rt.run(lhs)?;
        let r = rt.run(rhs)?;
        Ok(apply_compare(CmpOp::from_tag(OP), NumType::from_tag(NT), l, r))
    }
}

/// Run an assignment node with the target width baked in: evaluate the right
/// operand, write it into the left operand's storage, yield the value.
fn store_run<const NT: u8>(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an assignment application `[lhs, rhs, op]`; `lhs` is a
    // typed variable whose storage the builder checked assignable.
    unsafe {
        let (lhs, rhs) = operands(node);
        let bits = rt.run(rhs)?;
        let slot = rt.place_addr(lhs).ok_or(RunError::BadValue)?;
        if slot.is_null() {
            return Err(RunError::BadValue);
        }
        write_scalar_nt(NumType::from_tag(NT), slot, bits);
        Ok(bits)
    }
}

/// One row of monomorphic shims: a family instantiated across all ten machine
/// types (a function item coerces to the `RunFn` pointer in a const array).
macro_rules! shim_row {
    ($f:ident, $o:literal) => {
        [
            $f::<$o, 0>,
            $f::<$o, 1>,
            $f::<$o, 2>,
            $f::<$o, 3>,
            $f::<$o, 4>,
            $f::<$o, 5>,
            $f::<$o, 6>,
            $f::<$o, 7>,
            $f::<$o, 8>,
            $f::<$o, 9>,
        ]
    };
}

/// The arithmetic shims, `[ArithOp][NumType]`.
const ARITH_SHIMS: [[RunFn; 10]; 5] = [
    shim_row!(arith_run, 0),
    shim_row!(arith_run, 1),
    shim_row!(arith_run, 2),
    shim_row!(arith_run, 3),
    shim_row!(arith_run, 4),
];

/// The comparison shims, `[CmpOp][NumType]`.
const CMP_SHIMS: [[RunFn; 10]; 6] = [
    shim_row!(cmp_run, 0),
    shim_row!(cmp_run, 1),
    shim_row!(cmp_run, 2),
    shim_row!(cmp_run, 3),
    shim_row!(cmp_run, 4),
    shim_row!(cmp_run, 5),
];

/// The store shims, `[NumType]`.
const STORE_SHIMS: [RunFn; 10] = [
    store_run::<0>,
    store_run::<1>,
    store_run::<2>,
    store_run::<3>,
    store_run::<4>,
    store_run::<5>,
    store_run::<6>,
    store_run::<7>,
    store_run::<8>,
    store_run::<9>,
];

/// Mint every concrete-op leaf from the shim tables — the one registration
/// loop. Each leaf is a `callable` value under the `seed-native` convention,
/// its entry the monomorphic shim's address.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> OpLeaves {
    let mut arith = [[std::ptr::null_mut(); 10]; 5];
    for (o, row) in ARITH_SHIMS.iter().enumerate() {
        for (n, &shim) in row.iter().enumerate() {
            // Float remainder is rejected at parse; the leaf would be a lie.
            if o == ArithOp::Rem as usize && NumType::from_tag(n as u8).is_float() {
                continue;
            }
            arith[o][n] = callable::mint(cx.store, cs.callable, shim as usize, cs.seed_native);
        }
    }
    let mut cmp = [[std::ptr::null_mut(); 10]; 6];
    for (o, row) in CMP_SHIMS.iter().enumerate() {
        for (n, &shim) in row.iter().enumerate() {
            cmp[o][n] = callable::mint(cx.store, cs.callable, shim as usize, cs.seed_native);
        }
    }
    let mut store = [std::ptr::null_mut(); 10];
    for (n, &shim) in STORE_SHIMS.iter().enumerate() {
        store[n] = callable::mint(cx.store, cs.callable, shim as usize, cs.seed_native);
    }
    // The single-native leaves are minted where their shims live (`and`, `or`,
    // `convert`, the statement natives); their registrations fill these in.
    OpLeaves {
        arith,
        cmp,
        store,
        and_: std::ptr::null_mut(),
        or_: std::ptr::null_mut(),
        convert_: std::ptr::null_mut(),
        if_: std::ptr::null_mut(),
        while_: std::ptr::null_mut(),
        for_: std::ptr::null_mut(),
        return_: std::ptr::null_mut(),
        not_: std::ptr::null_mut(),
        construct_: std::ptr::null_mut(),
        deref_: std::ptr::null_mut(),
        storeptr_: std::ptr::null_mut(),
        addr_: std::ptr::null_mut(),
        scope_: std::ptr::null_mut(),
        declare_: std::ptr::null_mut(),
        compile_: std::ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identities::Core;
    use crate::regex_trie::RegexTrie;
    use crate::store::Store;

    #[test]
    fn every_concrete_op_is_a_seed_native_callable() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut seen = std::collections::HashSet::new();
        let all = core
            .ops
            .arith
            .iter()
            .flatten()
            .chain(core.ops.cmp.iter().flatten())
            .chain(core.ops.store.iter());
        for (i, &leaf) in all.enumerate() {
            // The two float remainders are the only unminted slots.
            let is_float_rem = i == ArithOp::Rem as usize * 10 + NumType::F32 as usize
                || i == ArithOp::Rem as usize * 10 + NumType::F64 as usize;
            if is_float_rem {
                assert!(leaf.is_null(), "float remainder must not exist");
                continue;
            }
            // SAFETY: every minted leaf is a valid dyad from the store.
            unsafe {
                assert!(callable::is_callable(leaf));
                assert_eq!(callable::convention_of(leaf), core.conv_seed_native);
                assert_ne!(callable::entry_of(leaf), 0);
            }
            assert!(seen.insert(leaf), "leaves are distinct identities");
        }
        assert_eq!(seen.len(), 5 * 10 - 2 + 6 * 10 + 10);
    }

    #[test]
    fn the_op_slot_dispatches_through_the_graph_alone() {
        // No table exists anywhere: a resolved node reaches its result through
        // its op slot and nothing else.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let l = store.alloc_bytes(&20i32.to_ne_bytes());
        let lhs = store.alloc_raw(core.i32_, l);
        let r = store.alloc_bytes(&22i32.to_ne_bytes());
        let rhs = store.alloc_raw(core.i32_, r);
        let leaf = core.ops.arith_leaf(ArithOp::Add, NumType::I32);
        let value = store.alloc_operands(&[lhs, rhs, leaf]);
        let node = store.alloc_raw(core.plus, value);

        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_);
        // SAFETY: the node and its operands were just built; the leaf is a
        // minted seed-native callable.
        assert_eq!(unsafe { rt.run(node) }.unwrap(), 42);
    }

    #[test]
    fn a_leaf_entry_jumps_and_computes() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // Two committed i32 values and a node whose first two slots are they —
        // exactly what a resolved `+` application will look like.
        let l = store.alloc_bytes(&20i32.to_ne_bytes());
        let lhs = store.alloc_raw(core.i32_, l);
        let r = store.alloc_bytes(&22i32.to_ne_bytes());
        let rhs = store.alloc_raw(core.i32_, r);
        let leaf = core.ops.arith_leaf(ArithOp::Add, NumType::I32);
        let value = store.alloc_operands(&[lhs, rhs, leaf]);
        let node = store.alloc_raw(core.plus, value);

        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_);
        // SAFETY: the leaf was minted from a seed-native RunFn shim; the node's
        // operands are valid committed scalars.
        let got = unsafe {
            let entry: RunFn = std::mem::transmute(callable::entry_of(leaf));
            entry(&mut rt, node).unwrap()
        };
        assert_eq!(got, 42);
    }
}
