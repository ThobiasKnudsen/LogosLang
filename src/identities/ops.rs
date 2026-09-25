// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The concrete machine operations, `add_i32`, `lt_f64`, `store_u8`, …: one
//! spelling-less `callable` leaf per (operation, machine type), minted from one
//! table-driven loop. A surface operator resolves each application to a leaf and
//! stores it in the node's op slot, so run reads the slot and jumps.

use crate::dyad::DyadPtr;
use crate::run::{RunError, RunFn, Runtime};

use super::callable::{self, Callables};
use super::numtype::{apply_arith, apply_compare, write_scalar_nt, ArithOp, CmpOp, NumType};
use super::{operands, Cx};

/// The parse-time resolver's table; the interpreter never consults it, each shim's type being
/// baked in.
#[derive(Clone, Copy, Debug)]
pub struct OpLeaves {
    /// `[ArithOp][NumType]`; null only for the unmintable float remainders.
    pub(crate) arith: [[DyadPtr; 10]; 5],
    /// `[CmpOp][NumType]`.
    pub(crate) cmp: [[DyadPtr; 10]; 6],
    /// The leaves over rational values, interpreted only.
    pub(crate) rational_arith: [DyadPtr; 5],
    pub(crate) rational_cmp: [DyadPtr; 6],
    /// `[NumType]`: the `=` store leaf at that width; a pointer target stores as `U64`.
    pub(crate) store: [DyadPtr; 10],
    pub(crate) and_: DyadPtr,
    pub(crate) or_: DyadPtr,
    pub(crate) convert_: DyadPtr,
    /// The statement natives, one leaf each: control flow branches on graph structure.
    pub(crate) if_: DyadPtr,
    pub(crate) while_: DyadPtr,
    pub(crate) for_: DyadPtr,
    pub(crate) return_: DyadPtr,
    pub(crate) not_: DyadPtr,
    pub(crate) subset_: DyadPtr,
    pub(crate) construct_: DyadPtr,
    pub(crate) deref_: DyadPtr,
    pub(crate) storeptr_: DyadPtr,
    pub(crate) addr_: DyadPtr,
    pub(crate) scope_: DyadPtr,
    pub(crate) ran_: DyadPtr,
    pub(crate) declare_: DyadPtr,
    pub(crate) compile_: DyadPtr,
    pub(crate) alloc_: DyadPtr,
    /// `free`'s op leaf and every owning pointer's stored destructor.
    pub(crate) teardown_: DyadPtr,
    pub(crate) own_: DyadPtr,
    pub(crate) drop_: DyadPtr,
    /// A `drop` over a place holding a node: the node's instances' `drop` run on it.
    pub(crate) instance_drop_: DyadPtr,
    /// A `free` over an owning field: the block the field holds, freed.
    pub(crate) field_free_: DyadPtr,
    /// A no-op; the scope machinery runs the inner.
    pub(crate) defer_: DyadPtr,
    pub(crate) import_: DyadPtr,
}

impl OpLeaves {
    pub(crate) fn arith_leaf(&self, op: ArithOp, nt: NumType) -> DyadPtr {
        self.arith[op as usize][nt as usize]
    }

    pub(crate) fn cmp_leaf(&self, op: CmpOp, nt: NumType) -> DyadPtr {
        self.cmp[op as usize][nt as usize]
    }

    pub(crate) fn rational_arith_leaf(&self, op: ArithOp) -> DyadPtr {
        self.rational_arith[op as usize]
    }

    pub(crate) fn rational_cmp_leaf(&self, op: CmpOp) -> DyadPtr {
        self.rational_cmp[op as usize]
    }

    /// What makes an operator node a rational value, and what the compiler refuses.
    pub(crate) fn is_rational_leaf(&self, leaf: DyadPtr) -> bool {
        !leaf.is_null()
            && (self.rational_arith.contains(&leaf) || self.rational_cmp.contains(&leaf))
    }

    pub(crate) fn store_leaf(&self, nt: NumType) -> DyadPtr {
        self.store[nt as usize]
    }

    /// The width is in the node: re-deriving it from the target's type disagrees for a
    /// `type ?` or `dyad ?` box. `None` for a pointer that is not a store leaf.
    pub(crate) fn store_nt_of(&self, leaf: DyadPtr) -> Option<NumType> {
        if leaf.is_null() {
            return None;
        }
        self.store.iter().position(|&l| l == leaf).map(|i| NumType::from_tag(i as u8))
    }

    /// The reverse of `cmp_leaf`; `None` for a pointer that is not a comparison leaf.
    pub(crate) fn cmp_nt_of(&self, leaf: DyadPtr) -> Option<NumType> {
        if leaf.is_null() {
            return None;
        }
        self.cmp
            .iter()
            .find_map(|row| row.iter().position(|&l| l == leaf))
            .map(|i| NumType::from_tag(i as u8))
    }
}

/// The concrete op never reads a type from the node; its type is this instantiation.
fn arith_run<const OP: u8, const NT: u8>(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a resolved binary application whose first two slots are its operands.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = rt.run(lhs)?;
        let r = rt.run(rhs)?;
        Ok(apply_arith(ArithOp::from_tag(OP), NumType::from_tag(NT), l, r))
    }
}

fn cmp_run<const OP: u8, const NT: u8>(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `arith_run`.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = rt.run(lhs)?;
        let r = rt.run(rhs)?;
        Ok(apply_compare(CmpOp::from_tag(OP), NumType::from_tag(NT), l, r))
    }
}

fn store_run<const NT: u8>(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an assignment application `[lhs, rhs, op]`; `lhs` is a
    // typed variable whose storage the builder checked assignable.
    unsafe {
        let (lhs, rhs) = operands(node);
        let bits = rt.run(rhs)?;
        let slot = rt.place_addr(lhs).ok_or(RunError::NoActivation)?;
        if slot.is_null() {
            return Err(RunError::Uninitialized);
        }
        write_scalar_nt(NumType::from_tag(NT), slot, bits);
        Ok(bits)
    }
}

/// A function item coerces to the `RunFn` pointer in a const array.
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
    let mut rational_arith = [std::ptr::null_mut(); 5];
    for (o, &run) in super::rational::ARITH_RUNS.iter().enumerate() {
        rational_arith[o] = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    }
    let mut rational_cmp = [std::ptr::null_mut(); 6];
    for (o, &run) in super::rational::CMP_RUNS.iter().enumerate() {
        rational_cmp[o] = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    }
    // The single-native leaves are minted by their own registrations.
    OpLeaves {
        arith,
        cmp,
        rational_arith,
        rational_cmp,
        store,
        and_: std::ptr::null_mut(),
        or_: std::ptr::null_mut(),
        convert_: std::ptr::null_mut(),
        if_: std::ptr::null_mut(),
        while_: std::ptr::null_mut(),
        for_: std::ptr::null_mut(),
        return_: std::ptr::null_mut(),
        not_: std::ptr::null_mut(),
        subset_: std::ptr::null_mut(),
        construct_: std::ptr::null_mut(),
        deref_: std::ptr::null_mut(),
        storeptr_: std::ptr::null_mut(),
        addr_: std::ptr::null_mut(),
        scope_: std::ptr::null_mut(),
        ran_: std::ptr::null_mut(),
        declare_: std::ptr::null_mut(),
        compile_: std::ptr::null_mut(),
        alloc_: std::ptr::null_mut(),
        instance_drop_: std::ptr::null_mut(),
        field_free_: std::ptr::null_mut(),
        teardown_: std::ptr::null_mut(),
        own_: std::ptr::null_mut(),
        drop_: std::ptr::null_mut(),
        defer_: std::ptr::null_mut(),
        import_: std::ptr::null_mut(),
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

        let mut rt = Runtime::new(&core, &mut store);
        // SAFETY: the node and its operands were just built; the leaf is a
        // minted seed-native callable.
        assert_eq!(unsafe { rt.run(node) }.unwrap(), 42);
    }

    #[test]
    fn a_leaf_entry_jumps_and_computes() {
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

        let mut rt = Runtime::new(&core, &mut store);
        // SAFETY: the leaf was minted from a seed-native RunFn shim; the node's
        // operands are valid committed scalars.
        let got = unsafe {
            let entry: RunFn = std::mem::transmute(callable::entry_of(leaf));
            entry(&mut rt, node).unwrap()
        };
        assert_eq!(got, 42);
    }
}
