// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `compile`: lower a dyad tree to native code with Cranelift, `run`'s
//! sibling: one IR node per graph node, finalized to a callable `bcode`.
//! The interpreter is the oracle; compiled and interpreted results must agree.

use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    types, AbiParam, Endianness, InstBuilder, MemFlagsData, StackSlot, StackSlotData,
    StackSlotKind, Value,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};

use crate::dyad::{frame_ref, DyadPtr};
use crate::identities::numtype::{is_void_type, of_type_node, ArithOp, CmpOp, NumType};
use crate::identities::read::{read_kind, Dispatch, Read};
use crate::identities::{numtype_of, operands, Operand};
use crate::parse::{fn_frame_size, FN_BCODE, FN_BODY, FN_INPUT, FN_OUTPUT};
use crate::Core;

/// Emit the IR for a node and return the SSA value it computes, recursing on
/// operands via [`Lowerer::lower`].
pub type LowerFn = fn(&mut Lowerer, DyadPtr) -> Result<Value, CompileError>;

/// What the analysis pass records about a function's frame places. An offset
/// promotes to a register variable only when every use is a scalar read or
/// write of one consistent type; a materialized address pins it to memory.
#[derive(Default)]
pub(crate) struct PlaceStats {
    /// Offset → the one Cranelift type seen.
    uses: HashMap<usize, types::Type>,
    /// Offsets seen at more than one type: never promoted.
    conflicted: Vec<usize>,
    /// Byte ranges `(offset, len)` whose address escaped; anything overlapping stays in memory.
    dirty: Vec<(usize, usize)>,
    /// A frame address of unknown extent escaped: promote nothing.
    kill: bool,
}

impl PlaceStats {
    fn record_use(&mut self, off: usize, ct: types::Type) {
        match self.uses.get(&off) {
            Some(&seen) if seen != ct => self.conflicted.push(off),
            _ => {
                self.uses.insert(off, ct);
            }
        }
    }

    /// Used consistently, address never taken, no dirty overlap.
    fn promotable(&self) -> Vec<(usize, types::Type)> {
        if self.kill {
            return Vec::new();
        }
        let mut out: Vec<(usize, types::Type)> = self
            .uses
            .iter()
            .filter(|(off, ct)| {
                let (off, width) = (**off, ct.bytes() as usize);
                !self.conflicted.contains(&off)
                    && !self
                        .dirty
                        .iter()
                        .any(|&(doff, dlen)| off < doff + dlen && doff < off + width)
            })
            .map(|(&off, &ct)| (off, ct))
            .collect();
        // Deterministic variable numbering across builds.
        out.sort_by_key(|&(off, _)| off);
        out
    }
}

/// Lowering rules keyed by operation identity.
pub type LowerTable = HashMap<DyadPtr, LowerFn>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// No lowering rule for this node's operation.
    NotLowerable(DyadPtr),
    /// A null storage address; refused rather than baking a load from address 0.
    Uninitialized,
    /// A frame place compiled outside any call.
    NoActivation,
    /// A pointer whose pointee is neither scalar nor pointer.
    NotDerefable,
    /// A construction of a type with no field layout.
    NoLayout(DyadPtr),
    /// A sequence with no expression array, or no expression to yield.
    EmptyScope,
    /// A builder's invariant found broken: a seed bug, named.
    Internal(&'static str),
    /// A literal with no exact `i32` value.
    UncomputableLiteral,
    /// A call's argument count differs from the callee's parameter count.
    ArityMismatch,
    /// Cranelift rejected the setup, function, or finalization.
    Cranelift(String),
}

/// The lowering context: a Cranelift function under construction, the rule
/// table, and the host pointer type. The builder is not exposed; rules use the
/// typed helpers below.
pub struct Lowerer<'a, 'f> {
    builder: &'a mut FunctionBuilder<'f>,
    lower: &'a LowerTable,
    ptr_ty: types::Type,
    /// Plain flags: storage is only byte-aligned, so no alignment is assumed.
    flags: MemFlagsData,
    /// So a call can reference the function being defined, or a compiled callee's code.
    module: &'a mut dyn Module,
    /// So a self-call becomes a direct `call` the JIT patches to this function.
    func_id: FuncId,
    types: &'a Core,
    /// The function being compiled (null for a bare expression), so a call to
    /// it is recognized as recursion.
    self_fn: DyadPtr,
    /// This call's activation record: the parameters spilled on entry, then the
    /// frame-relative locals, at their parse-assigned offsets. `None` for an
    /// empty frame. The machine stack gives each activation its own copy.
    frame_slot: Option<StackSlot>,
    /// The analysis pass's stats; `None` on the real pass.
    collect: Option<&'a mut PlaceStats>,
    /// Offset → register variable and type; a promoted place never touches its
    /// frame memory. Empty on the analysis pass.
    promoted: &'a HashMap<usize, (Variable, types::Type)>,
}

impl Lowerer<'_, '_> {
    /// Dispatch to the node's lowering rule, asking the reading rule the same
    /// question `run` asks, in the same order.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store.
    pub unsafe fn lower(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        let node = self.through(node);
        let op = (*node).ty;
        match read_kind(self.types, node) {
            Read::Executable(Dispatch::Call(f)) => self.lower_call_to(f, node),
            // Rational operations and places are interpreted only.
            Read::Executable(Dispatch::Leaf(leaf)) if self.types.ops.is_rational_leaf(leaf) => {
                Err(CompileError::NotLowerable(node))
            }
            Read::Executable(Dispatch::Leaf(_)) => match self.lower.get(&op).copied() {
                Some(f) => f(self, node),
                None => Err(CompileError::NotLowerable(op)),
            },
            Read::Executable(Dispatch::None) => Err(CompileError::NotLowerable(op)),
            Read::Unit => Ok(self.const_i32(0)),
            // An identity's address is baked: identities and the code baking
            // them are both per-run.
            Read::Identity => Ok(self.builder.ins().iconst(types::I64, node as i64)),
            Read::Address => Ok(self.builder.ins().iconst(types::I64, (*node).value as i64)),
            Read::Container(t) if t == self.types.rational => Err(CompileError::NotLowerable(node)),
            Read::Container(_) => self.read_place(node, types::I64),
            Read::Literal => match crate::identities::rational::mold(node) {
                Some(v) => Ok(self.const_i32(v)),
                None => Err(CompileError::UncomputableLiteral),
            },
            // A null value slot is a comptime binding with no storage.
            Read::Scalar(nt) => {
                if (*node).value.is_null() {
                    return Err(CompileError::Uninitialized);
                }
                self.read_place(node, nt.cranelift_type())
            }
            Read::Pointer(_) => {
                if (*node).value.is_null() {
                    return Err(CompileError::Uninitialized);
                }
                self.read_place(node, NumType::U64.cranelift_type())
            }
            Read::Aggregate | Read::Opaque | Read::Undefined => Err(CompileError::NotLowerable(op)),
        }
    }

    pub fn const_i32(&mut self, v: i32) -> Value {
        self.builder.ins().iconst(types::I32, i64::from(v))
    }

    /// The reading rule over an operand: a binding operand yields the dyad it names.
    ///
    /// # Safety
    /// `p` must be null or a valid dyad from the store.
    pub(crate) unsafe fn through(&self, p: DyadPtr) -> DyadPtr {
        crate::binding::through(self.types.binding_, p)
    }

    /// A place's address as an SSA pointer: a baked `iconst` for a global,
    /// `stack_addr(frame_slot, offset)` for a frame place. Taking the address
    /// pins the place to memory, so the analysis pass marks it dirty here.
    ///
    /// # Safety
    /// `node` must be a valid place node; a frame-relative one only appears in a
    /// function whose [`compile_body`] created a `frame_slot`.
    pub(crate) unsafe fn place_addr(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        let node = self.through(node);
        if let Some(stats) = self.collect.as_deref_mut() {
            if let Some((_, off)) = frame_ref((*node).value) {
                let logos = (*node).ty;
                if crate::identities::numtype::is_scalar_type(logos) {
                    stats.dirty.push((off, of_type_node(logos).bytes()));
                } else {
                    // Unknown extent: promote nothing in this function.
                    stats.kill = true;
                }
            }
        }
        debug_assert!(
            frame_ref((*node).value).is_none_or(|(_, off)| !self.promoted.contains_key(&off)),
            "a promoted place's address must never be taken (the analysis pass keeps them apart)"
        );
        self.place_addr_raw(node)
    }

    /// [`Self::place_addr`] without the analysis bookkeeping; the one place the
    /// compiler decodes the frame tag.
    unsafe fn place_addr_raw(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        let node = self.through(node);
        match frame_ref((*node).value) {
            Some((_, off)) => {
                // A frame place with no frame under it: the checked error the
                // interpreter gives for the same shape, not a panic.
                let Some(slot) = self.frame_slot else {
                    return Err(CompileError::NoActivation);
                };
                Ok(self.builder.ins().stack_addr(self.ptr_ty, slot, off as i32))
            }
            // The global tag comes off before the address is baked.
            None => {
                let v = (*node).value;
                let addr = crate::dyad::global_ref(v).unwrap_or(v);
                Ok(self.builder.ins().iconst(self.ptr_ty, addr as usize as i64))
            }
        }
    }

    /// A promoted frame place reads its register variable; everything else
    /// loads from its address. The analysis pass records the use.
    ///
    /// # Safety
    /// `node` must be a valid place node holding a `ct`-typed scalar.
    pub(crate) unsafe fn read_place(
        &mut self,
        node: DyadPtr,
        ct: types::Type,
    ) -> Result<Value, CompileError> {
        let node = self.through(node);
        if let Some((_, off)) = frame_ref((*node).value) {
            if let Some(&(var, vct)) = self.promoted.get(&off) {
                debug_assert_eq!(vct, ct, "a promoted place is used at one type");
                return Ok(self.builder.use_var(var));
            }
            if let Some(stats) = self.collect.as_deref_mut() {
                stats.record_use(off, ct);
            }
        }
        let addr = self.place_addr_raw(node)?;
        Ok(self.load_at(ct, addr, 0))
    }

    /// The dual of [`Self::read_place`].
    ///
    /// # Safety
    /// As [`Self::read_place`].
    pub(crate) unsafe fn write_place(
        &mut self,
        node: DyadPtr,
        ct: types::Type,
        v: Value,
    ) -> Result<(), CompileError> {
        let node = self.through(node);
        if let Some((_, off)) = frame_ref((*node).value) {
            if let Some(&(var, vct)) = self.promoted.get(&off) {
                debug_assert_eq!(vct, ct, "a promoted place is used at one type");
                self.builder.def_var(var, v);
                return Ok(());
            }
            if let Some(stats) = self.collect.as_deref_mut() {
                stats.record_use(off, ct);
            }
        }
        let addr = self.place_addr_raw(node)?;
        self.store_at(ct, addr, 0, v);
        Ok(())
    }

    /// The core handles, for an identity's lowering to reach the op-leaf table.
    pub(crate) fn types(&self) -> &Core {
        self.types
    }

    pub(crate) fn load_at(&mut self, ct: types::Type, addr: Value, offset: i64) -> Value {
        self.builder.ins().load(ct, self.flags, addr, offset as i32)
    }

    /// Run `ok` unless `addr` is null; on null, park the null-pointer fault and
    /// yield a zero of `ct`. Compiled code cannot simply stop (a block needs its
    /// terminator, and there is no trap handler), so the faulted call runs on
    /// over zeros the runtime discards when it reads the park back.
    pub(crate) fn guard_non_null<F>(
        &mut self,
        addr: Value,
        ct: types::Type,
        ok: F,
    ) -> Result<Value, CompileError>
    where
        F: FnOnce(&mut Self) -> Result<Value, CompileError>,
    {
        let zero = self.builder.ins().iconst(self.ptr_ty, 0);
        let is_null = self.icmp(IntCC::Equal, addr, zero);
        self.branch(
            is_null,
            |s| {
                let mut sig = s.module.make_signature();
                sig.returns.push(AbiParam::new(types::I64));
                let sigref = s.builder.import_signature(sig);
                let entry = crate::run::park_null_pointer as *const () as usize;
                let a = s.builder.ins().iconst(s.ptr_ty, entry as i64);
                s.builder.ins().call_indirect(sigref, a, &[]);
                Ok(s.zero_of(ct))
            },
            ok,
        )
    }

    fn zero_of(&mut self, ct: types::Type) -> Value {
        match ct {
            types::F32 => self.builder.ins().f32const(0.0),
            types::F64 => self.builder.ins().f64const(0.0),
            _ => self.builder.ins().iconst(ct, 0),
        }
    }

    pub(crate) fn store_at(&mut self, ct: types::Type, addr: Value, offset: i64, v: Value) {
        debug_assert_eq!(
            self.builder.func.dfg.value_type(v),
            ct,
            "store-through's value must lower to the pointee's type"
        );
        self.builder.ins().store(self.flags, v, addr, offset as i32);
    }

    /// Kept for `not`, which lowers `not x` as `x == 0`.
    pub fn icmp_eq(&mut self, a: Value, b: Value) -> Value {
        self.icmp(IntCC::Equal, a, b)
    }

    /// Zero-extended to the `I32` bool (`icmp` yields a one-bit `I8`).
    fn icmp(&mut self, cc: IntCC, a: Value, b: Value) -> Value {
        let c = self.builder.ins().icmp(cc, a, b);
        self.builder.ins().uextend(types::I32, c)
    }

    fn fcmp(&mut self, cc: FloatCC, a: Value, b: Value) -> Value {
        let c = self.builder.ins().fcmp(cc, a, b);
        self.builder.ins().uextend(types::I32, c)
    }

    /// The operand type is the committed left operand's; the result follows
    /// the operand `Value`s.
    ///
    /// # Safety
    /// `node` must be a resolved binary numeric operator node `[lhs, rhs, op]`.
    pub(crate) unsafe fn lower_arith(
        &mut self,
        node: DyadPtr,
        op: ArithOp,
    ) -> Result<Value, CompileError> {
        let (lhs, rhs) = operands(node);
        let nt = match numtype_of(self.types, lhs) {
            Operand::Concrete(nt) => nt,
            // A pointer step: the offset was scaled to `i64` bytes at parse.
            Operand::Pointer(_) => NumType::I64,
            _ => {
                return Err(CompileError::Internal(
                    "a resolved arithmetic node has a numeric operand",
                ))
            }
        };
        let l = self.lower(lhs)?;
        let r = self.lower(rhs)?;
        if nt.is_float() {
            return Ok(match op {
                ArithOp::Add => self.builder.ins().fadd(l, r),
                ArithOp::Sub => self.builder.ins().fsub(l, r),
                ArithOp::Mul => self.builder.ins().fmul(l, r),
                ArithOp::Div => self.builder.ins().fdiv(l, r),
                ArithOp::Rem => unreachable!("float % is rejected at parse"),
            });
        }
        Ok(match op {
            ArithOp::Add => self.builder.ins().iadd(l, r),
            ArithOp::Sub => self.builder.ins().isub(l, r),
            ArithOp::Mul => self.builder.ins().imul(l, r),
            ArithOp::Div => self.lower_int_div(nt, l, r)?,
            ArithOp::Rem => self.lower_int_rem(nt, l, r)?,
        })
    }

    fn const_int(&mut self, nt: NumType, imm: i64) -> Value {
        self.builder.ins().iconst(nt.cranelift_type(), imm)
    }

    /// Total, saturating semantics as the interpreter's: `x / 0` is MAX and the
    /// signed MIN/-1 overflow saturates to MAX; a raw `sdiv` would trap on both.
    fn lower_int_div(&mut self, nt: NumType, l: Value, r: Value) -> Result<Value, CompileError> {
        let zero = self.const_int(nt, 0);
        let is_zero = self.icmp(IntCC::Equal, r, zero);
        self.branch(
            is_zero,
            |s| Ok(s.const_int(nt, nt.max_imm())),
            |s| {
                if nt.is_signed_int() {
                    let m1 = s.const_int(nt, -1);
                    let r_is_m1 = s.icmp(IntCC::Equal, r, m1);
                    let min = s.const_int(nt, nt.min_imm());
                    let l_is_min = s.icmp(IntCC::Equal, l, min);
                    let overflow = s.builder.ins().band(r_is_m1, l_is_min);
                    s.branch(
                        overflow,
                        |s2| Ok(s2.const_int(nt, nt.max_imm())),
                        |s2| Ok(s2.builder.ins().sdiv(l, r)),
                    )
                } else {
                    Ok(s.builder.ins().udiv(l, r))
                }
            },
        )
    }

    /// Total semantics as the interpreter's: `x % 0` is MAX, and a signed
    /// `x % -1` is 0 (which also covers the MIN/-1 trap).
    fn lower_int_rem(&mut self, nt: NumType, l: Value, r: Value) -> Result<Value, CompileError> {
        let zero = self.const_int(nt, 0);
        let is_zero = self.icmp(IntCC::Equal, r, zero);
        self.branch(
            is_zero,
            |s| Ok(s.const_int(nt, nt.max_imm())),
            |s| {
                if nt.is_signed_int() {
                    let m1 = s.const_int(nt, -1);
                    let r_is_m1 = s.icmp(IntCC::Equal, r, m1);
                    s.branch(
                        r_is_m1,
                        |s2| Ok(s2.const_int(nt, 0)),
                        |s2| Ok(s2.builder.ins().srem(l, r)),
                    )
                } else {
                    Ok(s.builder.ins().urem(l, r))
                }
            },
        )
    }

    /// A signed or unsigned `icmp`, or an `fcmp`, at the width the node's leaf
    /// carries, zero-extended to the `I32` bool.
    ///
    /// # Safety
    /// `node` must be a resolved binary numeric operator node `[lhs, rhs, op]`.
    pub(crate) unsafe fn lower_compare(
        &mut self,
        node: DyadPtr,
        op: CmpOp,
    ) -> Result<Value, CompileError> {
        let (lhs, rhs) = operands(node);
        // The width is in the node: the builder chose a leaf for the resolved
        // operand types, so they are not classified a second time here.
        let leaf = *((*node).value as *const DyadPtr).add(2);
        let Some(logos) = self.types.ops.cmp_nt_of(leaf) else {
            return Err(CompileError::Internal(
                "a resolved comparison node holds a comparison leaf",
            ));
        };
        let l = self.lower(lhs)?;
        let r = self.lower(rhs)?;
        if logos.is_float() {
            let cc = match op {
                CmpOp::Lt => FloatCC::LessThan,
                CmpOp::Gt => FloatCC::GreaterThan,
                CmpOp::Le => FloatCC::LessThanOrEqual,
                CmpOp::Ge => FloatCC::GreaterThanOrEqual,
                CmpOp::Eq => FloatCC::Equal,
                CmpOp::Ne => FloatCC::NotEqual,
            };
            Ok(self.fcmp(cc, l, r))
        } else {
            let s = logos.is_signed_int();
            let cc = match (op, s) {
                (CmpOp::Lt, true) => IntCC::SignedLessThan,
                (CmpOp::Lt, false) => IntCC::UnsignedLessThan,
                (CmpOp::Gt, true) => IntCC::SignedGreaterThan,
                (CmpOp::Gt, false) => IntCC::UnsignedGreaterThan,
                (CmpOp::Le, true) => IntCC::SignedLessThanOrEqual,
                (CmpOp::Le, false) => IntCC::UnsignedLessThanOrEqual,
                (CmpOp::Ge, true) => IntCC::SignedGreaterThanOrEqual,
                (CmpOp::Ge, false) => IntCC::UnsignedGreaterThanOrEqual,
                (CmpOp::Eq, _) => IntCC::Equal,
                (CmpOp::Ne, _) => IntCC::NotEqual,
            };
            Ok(self.icmp(cc, l, r))
        }
    }

    /// Cast `v` from `from` to `to` as the interpreter's `apply_cast` does
    /// (Rust `as`); float→int saturates rather than trapping.
    pub(crate) fn emit_cast(&mut self, from: NumType, to: NumType, v: Value) -> Value {
        let tct = to.cranelift_type();
        match (from.is_float(), to.is_float()) {
            // Same width is a no-op: signedness is the consumer's concern.
            (false, false) => {
                if to.bytes() == from.bytes() {
                    v
                } else if to.bytes() > from.bytes() {
                    if from.is_signed_int() {
                        self.builder.ins().sextend(tct, v)
                    } else {
                        self.builder.ins().uextend(tct, v)
                    }
                } else {
                    self.builder.ins().ireduce(tct, v)
                }
            }
            (false, true) => {
                if from.is_signed_int() {
                    self.builder.ins().fcvt_from_sint(tct, v)
                } else {
                    self.builder.ins().fcvt_from_uint(tct, v)
                }
            }
            // Saturating, NaN → 0, matching Rust `as`.
            (true, false) => {
                if to.is_signed_int() {
                    self.builder.ins().fcvt_to_sint_sat(tct, v)
                } else {
                    self.builder.ins().fcvt_to_uint_sat(tct, v)
                }
            }
            (true, true) => {
                if to.bytes() > from.bytes() {
                    self.builder.ins().fpromote(tct, v)
                } else if to.bytes() < from.bytes() {
                    self.builder.ins().fdemote(tct, v)
                } else {
                    v
                }
            }
        }
    }

    /// A two-way branch on `cond`, each arm yielding a value of one agreed type,
    /// merged into one value; leaves the builder in the sealed merge block, so
    /// nesting composes. The shared spine of `if`, `and` and `or`.
    fn branch<T, E>(&mut self, cond: Value, then_arm: T, else_arm: E) -> Result<Value, CompileError>
    where
        T: FnOnce(&mut Self) -> Result<Value, CompileError>,
        E: FnOnce(&mut Self) -> Result<Value, CompileError>,
    {
        let then_b = self.builder.create_block();
        let else_b = self.builder.create_block();
        let merge_b = self.builder.create_block();

        self.builder.ins().brif(cond, then_b, &[], else_b, &[]);
        self.builder.seal_block(then_b);
        self.builder.seal_block(else_b);

        self.builder.switch_to_block(then_b);
        let then_v = then_arm(self)?;
        // The merge takes the arms' type, so `if` yields whatever width its branches do.
        let result =
            self.builder.append_block_param(merge_b, self.builder.func.dfg.value_type(then_v));
        self.builder.ins().jump(merge_b, &[then_v.into()]);

        self.builder.switch_to_block(else_b);
        let else_v = else_arm(self)?;
        self.builder.ins().jump(merge_b, &[else_v.into()]);

        self.builder.seal_block(merge_b);
        self.builder.switch_to_block(merge_b);
        Ok(result)
    }

    /// # Safety
    /// `cond`/`then`/`els` must be valid dyads from the store.
    pub unsafe fn lower_if(
        &mut self,
        cond: DyadPtr,
        then: DyadPtr,
        els: DyadPtr,
    ) -> Result<Value, CompileError> {
        let c = self.lower(cond)?;
        // SAFETY: `then`/`els` are the dyads the caller's contract covers.
        self.branch(c, |s| unsafe { s.lower(then) }, |s| unsafe { s.lower(els) })
    }

    /// The header re-evaluates the condition, the body jumps back, and the
    /// header seals only after that back-edge exists. Yields unit.
    ///
    /// # Safety
    /// `cond`/`body` must be valid dyads from the store.
    pub unsafe fn lower_while(
        &mut self,
        cond: DyadPtr,
        body: DyadPtr,
    ) -> Result<Value, CompileError> {
        let header = self.builder.create_block();
        let body_b = self.builder.create_block();
        let exit = self.builder.create_block();

        self.builder.ins().jump(header, &[]);
        self.builder.switch_to_block(header);
        let c = self.lower(cond)?;
        self.builder.ins().brif(c, body_b, &[], exit, &[]);
        self.builder.switch_to_block(body_b);
        self.builder.seal_block(body_b);
        self.lower(body)?;
        self.builder.ins().jump(header, &[]);
        self.builder.seal_block(header);
        self.builder.switch_to_block(exit);
        self.builder.seal_block(exit);
        Ok(self.const_i32(0))
    }

    /// The start goes into the loop variable's place, the end and step (default 1)
    /// are hoisted as SSA, a `step > 0` guard runs zero iterations otherwise, as
    /// interpreted; then read, compare `< end`, body, increment. Yields unit.
    ///
    /// # Safety
    /// The parts must be valid dyads from the store; `step` may be null for the default.
    pub unsafe fn lower_for(
        &mut self,
        var: DyadPtr,
        start: DyadPtr,
        end: DyadPtr,
        step: DyadPtr,
        body: DyadPtr,
    ) -> Result<Value, CompileError> {
        let nt = of_type_node((*var).ty);
        let ct = nt.cranelift_type();

        let s = self.lower(start)?;
        self.write_place(var, ct, s)?;
        let e = self.lower(end)?;
        let d = if !step.is_null() {
            self.lower(step)?
        } else if ct == types::F32 {
            self.builder.ins().f32const(1.0)
        } else if ct == types::F64 {
            self.builder.ins().f64const(1.0)
        } else {
            self.builder.ins().iconst(ct, 1)
        };

        let header = self.builder.create_block();
        let body_b = self.builder.create_block();
        let exit = self.builder.create_block();

        let pos = if nt.is_float() {
            let zero = if ct == types::F32 {
                self.builder.ins().f32const(0.0)
            } else {
                self.builder.ins().f64const(0.0)
            };
            self.fcmp(FloatCC::GreaterThan, d, zero)
        } else {
            let zero = self.builder.ins().iconst(ct, 0);
            let cc = if nt.is_signed_int() {
                IntCC::SignedGreaterThan
            } else {
                IntCC::UnsignedGreaterThan
            };
            self.icmp(cc, d, zero)
        };
        self.builder.ins().brif(pos, header, &[], exit, &[]);

        self.builder.switch_to_block(header);
        let v = self.read_place(var, ct)?;
        let cond = if nt.is_float() {
            self.fcmp(FloatCC::LessThan, v, e)
        } else {
            let cc =
                if nt.is_signed_int() { IntCC::SignedLessThan } else { IntCC::UnsignedLessThan };
            self.icmp(cc, v, e)
        };
        self.builder.ins().brif(cond, body_b, &[], exit, &[]);

        self.builder.switch_to_block(body_b);
        self.builder.seal_block(body_b);
        self.lower(body)?;
        let v2 = self.read_place(var, ct)?;
        let inc = if nt.is_float() {
            self.builder.ins().fadd(v2, d)
        } else {
            // A step past the counter's width ends the loop, as the
            // interpreter's checked add does.
            let (sum, overflow) = if nt.is_signed_int() {
                self.builder.ins().sadd_overflow(v2, d)
            } else {
                self.builder.ins().uadd_overflow(v2, d)
            };
            let step_b = self.builder.create_block();
            self.builder.ins().brif(overflow, exit, &[], step_b, &[]);
            self.builder.switch_to_block(step_b);
            self.builder.seal_block(step_b);
            sum
        };
        self.write_place(var, ct, inc)?;
        self.builder.ins().jump(header, &[]);
        self.builder.seal_block(header);
        self.builder.switch_to_block(exit);
        self.builder.seal_block(exit);
        Ok(self.const_i32(0))
    }

    /// A statement: both arms yield unit, so the merge always agrees.
    ///
    /// # Safety
    /// `cond`/`then` must be valid dyads from the store.
    pub unsafe fn lower_if_stmt(
        &mut self,
        cond: DyadPtr,
        then: DyadPtr,
    ) -> Result<Value, CompileError> {
        let c = self.lower(cond)?;
        self.branch(
            c,
            |s| {
                // SAFETY: `then` is the dyad the caller's contract covers.
                unsafe { s.lower(then) }?;
                Ok(s.const_i32(0))
            },
            |s| Ok(s.const_i32(0)),
        )
    }

    /// Short-circuit: `b` is not evaluated when `a` is false.
    ///
    /// # Safety
    /// `a`/`b` must be valid dyads from the store.
    pub unsafe fn lower_and(&mut self, a: DyadPtr, b: DyadPtr) -> Result<Value, CompileError> {
        let va = self.lower(a)?;
        // SAFETY: `b` is the dyad the caller's contract covers.
        self.branch(va, |s| unsafe { s.lower(b) }, |s| Ok(s.const_i32(0)))
    }

    /// Short-circuit: `b` is not evaluated when `a` is true.
    ///
    /// # Safety
    /// `a`/`b` must be valid dyads from the store.
    pub unsafe fn lower_or(&mut self, a: DyadPtr, b: DyadPtr) -> Result<Value, CompileError> {
        let va = self.lower(a)?;
        // SAFETY: `b` is the dyad the caller's contract covers.
        self.branch(va, |s| Ok(s.const_i32(1)), |s| unsafe { s.lower(b) })
    }

    /// A self-call is a direct `call` the JIT patches to this function; a
    /// compiled callee is a `call_indirect` through its baked entry; an
    /// uncompiled callee is a jump into the interpreter. Each argument widens
    /// into its container per its own type; the result narrows per the
    /// callee's return type. Compile order decides the call's shape, never
    /// whether it compiles.
    ///
    /// # Safety
    /// `callee` must be a `fn` node from the store and `node` a node whose
    /// value is its null-terminated argument run.
    unsafe fn lower_call_to(
        &mut self,
        callee: DyadPtr,
        node: DyadPtr,
    ) -> Result<Value, CompileError> {
        let fields = (*callee).value as *const DyadPtr;
        if fields.is_null() {
            // No signature to size the call by: an unbound placeholder.
            return Err(CompileError::NotLowerable(callee));
        }
        let input = *fields.add(FN_INPUT);
        let param_count =
            crate::identities::array::items(crate::identities::meta::record_fields_of(input)).len();
        let ret = return_kind(self.types, *fields.add(FN_OUTPUT))?;

        let args = (*node).value as *const DyadPtr; // [arg0 …, null] or null
        let mut args64 = Vec::new();
        if !args.is_null() {
            let mut i = 0;
            while !(*args.add(i)).is_null() {
                let arg = *args.add(i);
                let v = self.lower(arg)?;
                // Already at the container's width: a type value or an address.
                if self.builder.func.dfg.value_type(v) == types::I64 {
                    args64.push(v);
                    i += 1;
                    continue;
                }
                let nt = match numtype_of(self.types, arg) {
                    Operand::Concrete(nt) => nt,
                    // A pointer rides the container as its 8-byte address.
                    Operand::Pointer(_) => NumType::U64,
                    // An uncommitted literal is the bare-literal i32 default; a
                    // non-numeric value (a void call's unit) rides as the i32 unit.
                    Operand::Literal | Operand::NonNumeric => NumType::I32,
                };
                args64.push(widen_to_i64(self.builder, v, nt));
                i += 1;
            }
        }
        if args64.len() != param_count {
            return Err(CompileError::ArityMismatch);
        }

        // The containers in a block on this frame; a nullary call passes no block.
        let argv = if args64.is_empty() {
            self.builder.ins().iconst(self.ptr_ty, 0)
        } else {
            let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                (args64.len() * 8) as u32,
                3,
            ));
            for (i, &v) in args64.iter().enumerate() {
                self.builder.ins().stack_store(v, slot, (i * 8) as i32);
            }
            self.builder.ins().stack_addr(self.ptr_ty, slot, 0)
        };
        let argc = self.builder.ins().iconst(types::I64, args64.len() as i64);
        let inst = if callee == self.self_fn {
            let fref = self.module.declare_func_in_func(self.func_id, &mut *self.builder.func);
            self.builder.ins().call(fref, &[argv, argc])
        } else {
            let bcode = *fields.add(FN_BCODE);
            if bcode.is_null() {
                let fn_node = self.builder.ins().iconst(self.ptr_ty, callee as i64);
                let mut sig = self.module.make_signature();
                for _ in 0..3 {
                    sig.params.push(AbiParam::new(types::I64));
                }
                sig.returns.push(AbiParam::new(types::I64));
                let sigref = self.builder.import_signature(sig);
                let entry = crate::run::interpret_call as *const () as usize;
                let addr = self.builder.ins().iconst(self.ptr_ty, entry as i64);
                self.builder.ins().call_indirect(sigref, addr, &[fn_node, argc, argv])
            } else {
                let entry = crate::identities::callable::entry_of(bcode);
                let mut sig = self.module.make_signature();
                sig.params.push(AbiParam::new(self.ptr_ty));
                sig.params.push(AbiParam::new(types::I64));
                sig.returns.push(AbiParam::new(types::I64));
                let sigref = self.builder.import_signature(sig);
                let addr = self.builder.ins().iconst(self.ptr_ty, entry as i64);
                self.builder.ins().call_indirect(sigref, addr, &[argv, argc])
            }
        };
        let r = self.builder.inst_results(inst)[0];
        Ok(match ret {
            Some(nt) => narrow_from_i64(self.builder, r, nt),
            None => self.const_i32(0),
        })
    }
}

/// A JIT-compiled function and the module owning its executable memory.
pub struct Compiled {
    // Kept alive so the executable memory `ptr` points into stays mapped.
    #[allow(dead_code)]
    module: JITModule,
    ptr: *const u8,
}

impl Compiled {
    /// Call the compiled function with no arguments and return the raw `i64` container.
    ///
    /// # Safety
    /// The compiled function must be nullary and any host addresses it baked
    /// in must still be valid.
    pub unsafe fn call(&self) -> i64 {
        let f: crate::run::MachineFn = std::mem::transmute(self.ptr);
        f(std::ptr::null(), 0)
    }
}

/// Compile a function literal and install its machine code, as a `callable`
/// under the `container-i64` convention, into the node's `bcode` slot. The
/// returned [`Compiled`] owns the executable memory: the installed entry is
/// valid only while it is alive.
///
/// # Safety
/// `fn_node` must be a valid function node from the store, and any storage its
/// body references must outlive every call to the returned [`Compiled`].
pub unsafe fn compile_fn(
    store: &mut crate::store::Store,
    lower: &LowerTable,
    types: &Core,
    fn_node: DyadPtr,
) -> Result<Compiled, CompileError> {
    let compiled = compile_fn_body(lower, types, fn_node)?;
    let code = crate::identities::callable::mint(
        store,
        types.callable_,
        compiled.ptr as usize,
        types.conv_container,
    );
    let bcode_slot = ((*fn_node).value as *mut DyadPtr).add(FN_BCODE);
    *bcode_slot = code;
    Ok(compiled)
}

/// The shared work of [`compile_fn`] and [`compile_into`]: the body's machine
/// code, nothing minted or installed.
///
/// # Safety
/// As [`compile_fn`].
unsafe fn compile_fn_body(
    lower: &LowerTable,
    types: &Core,
    fn_node: DyadPtr,
) -> Result<Compiled, CompileError> {
    let fields = (*fn_node).value as *const DyadPtr;
    if fields.is_null() {
        return Err(CompileError::NotLowerable(fn_node));
    }
    let input = *fields.add(FN_INPUT);
    let params: Vec<DyadPtr> =
        crate::identities::array::items(crate::identities::meta::record_fields_of(input)).to_vec();
    let body = *fields.add(FN_BODY);
    let ret = return_kind(types, *fields.add(FN_OUTPUT))?;
    compile_body(lower, types, fn_node, body, &params, ret)
}

/// How the declared output travels in the container: not at all for `void`,
/// as an address for a type value, at its width for a scalar; anything else
/// (a record) is not compilable yet.
///
/// # Safety
/// `out` must be a type node from the store.
unsafe fn return_kind(types: &Core, out: DyadPtr) -> Result<Option<NumType>, CompileError> {
    if is_void_type(out) {
        Ok(None)
    } else if out == types.type_ {
        Ok(Some(NumType::I64))
    } else if crate::identities::numtype::is_scalar_type(out) {
        Ok(Some(of_type_node(out)))
    } else {
        Err(CompileError::NotLowerable(out))
    }
}

/// `f.compile()`'s run half: compile the body and install the entry into
/// `code_leaf`, the callable the parser pre-minted, then that leaf into the
/// `bcode` slot. The artifact is deliberately leaked: the entry must stay
/// valid for every later call, and nothing in the graph owns artifacts yet.
///
/// # Safety
/// As [`compile_fn`]; `code_leaf` must be a callable value from the store.
pub(crate) unsafe fn compile_into(
    lower: &LowerTable,
    types: &Core,
    fn_node: DyadPtr,
    code_leaf: DyadPtr,
) -> Result<(), CompileError> {
    let compiled = compile_fn_body(lower, types, fn_node)?;
    crate::identities::callable::install_entry(code_leaf, compiled.ptr as usize);
    let bcode_slot = ((*fn_node).value as *mut DyadPtr).add(FN_BCODE);
    *bcode_slot = code_leaf;
    std::mem::forget(compiled);
    Ok(())
}

/// # Safety
/// See [`compile_body`].
pub unsafe fn compile_nullary_i32(
    lower: &LowerTable,
    types: &Core,
    root: DyadPtr,
) -> Result<Compiled, CompileError> {
    // No self to recurse into; v1 bare expressions are i32 (or bool, physically i32).
    compile_body(lower, types, std::ptr::null_mut(), root, &[], Some(NumType::I32))
}

/// Compile `root` as a function of `params`, each argument spilled from its
/// container into the parameter's frame slot on entry, returning `ret` (`None`
/// for `-> void`, which yields unit).
///
/// # Safety
/// `root` must be a valid dyad tree from the store, and any variable storage its
/// leaves reference must outlive every call to the returned [`Compiled`] (the
/// addresses are baked into the code).
pub(crate) unsafe fn compile_body(
    lower: &LowerTable,
    types: &Core,
    self_fn: DyadPtr,
    root: DyadPtr,
    params: &[DyadPtr],
    ret: Option<NumType>,
) -> Result<Compiled, CompileError> {
    // Two passes: the first lowers into a discarded function, recording how
    // every frame place is used; the offsets used only as consistent scalars,
    // address never taken, promote to register variables on the real pass.
    let mut stats = PlaceStats::default();
    build_pass(lower, types, self_fn, root, params, ret, Some(&mut stats), &[], false)?;
    let promote = stats.promotable();
    let compiled = build_pass(lower, types, self_fn, root, params, ret, None, &promote, true)?;
    Ok(compiled.expect("the finishing pass returns the artifact"))
}

/// One pass of [`compile_body`]: analysis (`collect` set, `finish` false, the
/// function discarded) or the real build. `promote` lists the frame offsets to
/// place in register variables, with their types.
///
/// # Safety
/// See [`compile_body`].
#[allow(clippy::too_many_arguments)]
unsafe fn build_pass(
    lower: &LowerTable,
    types: &Core,
    self_fn: DyadPtr,
    root: DyadPtr,
    params: &[DyadPtr],
    ret: Option<NumType>,
    collect: Option<&mut PlaceStats>,
    promote: &[(usize, types::Type)],
    finish: bool,
) -> Result<Option<Compiled>, CompileError> {
    let mut flags = settings::builder();
    flags.set("use_colocated_libcalls", "false").map_err(cl)?;
    flags.set("is_pic", "false").map_err(cl)?;
    let isa = cranelift_native::builder()
        .map_err(|e| CompileError::Cranelift(e.to_string()))?
        .finish(settings::Flags::new(flags))
        .map_err(cl)?;
    let ptr_ty = isa.pointer_type();

    let mut module = JITModule::new(JITBuilder::with_isa(isa, default_libcall_names()));
    let mut ctx = module.make_context();
    // The one compiled signature, `(argv, argc) -> i64` (`crate::run::MachineFn`).
    ctx.func.signature.params.push(AbiParam::new(ptr_ty));
    ctx.func.signature.params.push(AbiParam::new(types::I64));
    ctx.func.signature.returns.push(AbiParam::new(types::I64));

    // Declared before the body lowers, so a self-call can reference its id.
    let func_id =
        module.declare_function("main", Linkage::Export, &ctx.func.signature).map_err(cl)?;

    let mut fctx = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        // The depth guard's compiled half: count this call in, park the fault
        // past the limit instead of claiming a frame, and count out in the epilogue.
        let depth_addr = builder.ins().iconst(ptr_ty, crate::run::call_depth_ptr() as usize as i64);
        let depth = builder.ins().load(types::I64, MemFlagsData::new(), depth_addr, 0);
        let deeper = builder.ins().iadd_imm(depth, 1);
        builder.ins().store(MemFlagsData::new(), deeper, depth_addr, 0);
        let over = builder.ins().icmp_imm(
            IntCC::UnsignedGreaterThan,
            deeper,
            crate::run::MAX_CALL_DEPTH as i64,
        );
        let fault = builder.create_block();
        let body_b = builder.create_block();
        builder.ins().brif(over, fault, &[], body_b, &[]);
        builder.switch_to_block(fault);
        builder.seal_block(fault);
        builder.ins().store(MemFlagsData::new(), depth, depth_addr, 0);
        let mut park_sig = module.make_signature();
        park_sig.returns.push(AbiParam::new(types::I64));
        let park_sigref = builder.import_signature(park_sig);
        let park =
            builder.ins().iconst(ptr_ty, crate::run::park_call_depth as *const () as usize as i64);
        builder.ins().call_indirect(park_sigref, park, &[]);
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().return_(&[zero]);
        builder.switch_to_block(body_b);
        builder.seal_block(body_b);

        // The activation record, 8-byte aligned; a frameless function gets none.
        let frame_size = if self_fn.is_null() { 0 } else { fn_frame_size(self_fn) };
        let frame_slot = (frame_size > 0).then(|| {
            // Whole i64 words, so the zeroing below covers it.
            let size = (frame_size as u32).next_multiple_of(8);
            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                size,
                3,
            ));
            // Zeroed on entry as the interpreter zeroes its frame, so an
            // uninitialized declaration reads the same zero on both tiers.
            let zero = builder.ins().iconst(types::I64, 0);
            for off in (0..size).step_by(8) {
                builder.ins().stack_store(zero, slot, off as i32);
            }
            slot
        });

        // Promoted locals start at the zero of their type, the register form of
        // the zeroed frame; promoted parameters are defined from their arguments below.
        let param_offs: Vec<Option<usize>> =
            params.iter().map(|&p| frame_ref((*p).value).map(|(_, off)| off)).collect();
        let mut promoted: HashMap<usize, (Variable, types::Type)> = HashMap::new();
        for &(off, ct) in promote {
            let var = builder.declare_var(ct);
            promoted.insert(off, (var, ct));
            if param_offs.iter().flatten().any(|&poff| poff == off) {
                continue;
            }
            let zero = match ct {
                types::F32 => builder.ins().f32const(0.0),
                types::F64 => builder.ins().f64const(0.0),
                _ => builder.ins().iconst(ct, 0),
            };
            builder.def_var(var, zero);
        }

        // Each argument loads from `argv` and narrows to its declared scalar
        // type; a bare or type-valued parameter keeps the full container.
        let argv = builder.block_params(entry)[0];
        for (i, &p) in params.iter().enumerate() {
            let v = builder.ins().load(types::I64, MemFlagsData::new(), argv, (i * 8) as i32);
            let Some((_, off)) = frame_ref((*p).value) else {
                return Err(CompileError::NotLowerable(p));
            };
            let logos = (*p).ty;
            let scalar = crate::identities::numtype::is_scalar_type(logos);
            if let Some(&(var, _)) = promoted.get(&off) {
                let vn =
                    if scalar { narrow_from_i64(&mut builder, v, of_type_node(logos)) } else { v };
                builder.def_var(var, vn);
                continue;
            }
            let slot = frame_slot.expect("parameters occupy the frame, so a frame slot exists");
            if scalar {
                let vn = narrow_from_i64(&mut builder, v, of_type_node(logos));
                builder.ins().stack_store(vn, slot, off as i32);
            } else {
                builder.ins().stack_store(v, slot, off as i32);
            }
        }

        let value = {
            let mut lw = Lowerer {
                builder: &mut builder,
                lower,
                ptr_ty,
                flags: MemFlagsData::new(),
                module: &mut module,
                func_id,
                types,
                self_fn,
                frame_slot,
                collect,
                promoted: &promoted,
            };
            lw.lower(root)?
        };
        // The body's value widens back to the container; a `-> void` body ran
        // for effect and returns unit.
        let ret64 = match ret {
            Some(nt) => widen_to_i64(&mut builder, value, nt),
            None => builder.ins().iconst(types::I64, 0),
        };
        // Count this call out (the prologue counted it in).
        let depth = builder.ins().load(types::I64, MemFlagsData::new(), depth_addr, 0);
        let shallower = builder.ins().iadd_imm(depth, -1);
        builder.ins().store(MemFlagsData::new(), shallower, depth_addr, 0);
        builder.ins().return_(&[ret64]);
        builder.finalize();
    }

    if !finish {
        return Ok(None);
    }
    module.define_function(func_id, &mut ctx).map_err(cl)?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().map_err(cl)?;
    let ptr = module.get_finalized_function(func_id);

    Ok(Some(Compiled { module, ptr }))
}

/// The inverse of [`widen_to_i64`]: integers reduce to their width; floats
/// reinterpret the container's bits (`f64` the whole 64, `f32` the low 32).
fn narrow_from_i64(b: &mut FunctionBuilder, v: Value, nt: NumType) -> Value {
    match nt {
        NumType::F64 => b.ins().bitcast(types::F64, bitcast_flags(), v),
        NumType::F32 => {
            let bits = b.ins().ireduce(types::I32, v);
            b.ins().bitcast(types::F32, bitcast_flags(), bits)
        }
        _ => {
            let ct = nt.cranelift_type();
            if ct == types::I64 {
                v
            } else {
                b.ins().ireduce(ct, v)
            }
        }
    }
}

/// Sign-extend signed integers, zero-extend unsigned, reinterpret float bits,
/// matching `read_scalar`.
fn widen_to_i64(b: &mut FunctionBuilder, v: Value, nt: NumType) -> Value {
    match nt {
        NumType::F64 => b.ins().bitcast(types::I64, bitcast_flags(), v),
        NumType::F32 => {
            let bits = b.ins().bitcast(types::I32, bitcast_flags(), v);
            b.ins().uextend(types::I64, bits)
        }
        _ if nt.cranelift_type() == types::I64 => v,
        _ if nt.is_signed_int() => b.ins().sextend(types::I64, v),
        _ => b.ins().uextend(types::I64, v),
    }
}

/// A same-size scalar reinterpret is byte-order independent, so a fixed
/// `Little` is correct on any host.
fn bitcast_flags() -> MemFlagsData {
    MemFlagsData::new().with_endianness(Endianness::Little)
}

fn cl<E: std::fmt::Display>(e: E) -> CompileError {
    CompileError::Cranelift(e.to_string())
}
