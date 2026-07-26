// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `compile`: lower a synolon tree to native code with Cranelift.
//!
//! `compile` is `run`'s sibling: where `run` walks the graph and computes,
//! `compile` walks it and *emits* machine code, one IR node per graph node, then
//! finalizes to a callable function (the `bcode`). The result is meant to be
//! observably identical to `run` (the interpreter is the compiler's oracle).
//!
//! Each core primitive carries a lowering rule (its [`LowerFn`]) exactly as it
//! carries a run native, kept in [`crate::identities::Core`]'s `lower` table. The
//! seed compiles whole `fn` bodies: parameters map to block params, `if` and the
//! short-circuiting `and`/`or` lower to branch-and-merge blocks, and a call
//! lowers to a direct self-call (compiled recursion) or a `call_indirect` to an
//! already-compiled callee, with operand addresses and literals baked as
//! immediates (DESIGN ›operand access is baked into the machine code‹). The
//! calling convention is uniform — every parameter and result is the
//! interpreter's `i64` bit-container, reinterpreted at the boundary — capped at
//! [`MAX_COMPILED_PARAMS`] parameters.

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

use crate::synolon::{frame_ref, SynolonPtr};
use crate::identities::numtype::{
    is_void_type, numtype_of_type, of_type_node, ArithOp, CmpOp, NumType,
};
use crate::identities::{numtype_of, operands, Operand};
use crate::parse::{fn_frame_size, CoreTypes, FN_BCODE, FN_BODY, FN_INPUT, FN_OUTPUT};

/// A lowering rule: emit the IR for a node and return the SSA value it computes,
/// recursing on operands via [`Lowerer::lower`].
pub type LowerFn = fn(&mut Lowerer, SynolonPtr) -> Result<Value, CompileError>;

/// What the register-promotion analysis pass records about a function's frame
/// places (DESIGN ›Operands travel on the stack‹: compiled code has "locals
/// assigned to registers or stack slots"). A frame offset promotes to a
/// register variable only when every observation of it is a plain scalar read
/// or write of one consistent logos; materializing its *address* (`&x`, an
/// instance base, a bare parameter's container) pins it to memory, since a
/// register has no address.
#[derive(Default)]
pub(crate) struct PlaceStats {
    /// Offsets read/written as scalars: offset → the one Cranelift logos seen.
    uses: HashMap<usize, types::Type>,
    /// Offsets seen at more than one logos — never promoted.
    conflicted: Vec<usize>,
    /// Byte ranges whose address escaped into the code: `(offset, len)`.
    /// Anything overlapping one stays in memory.
    dirty: Vec<(usize, usize)>,
    /// A frame address of unknown extent escaped (a non-scalar place):
    /// promote nothing.
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

    /// The promotable offsets and their logos: used consistently, address
    /// never taken, no dirty overlap.
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

/// Lowering rules keyed by operation identity (a primitive's compiled form).
pub type LowerTable = HashMap<SynolonPtr, LowerFn>;

/// Why compilation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// No lowering rule is registered for this node's operation.
    NotLowerable(SynolonPtr),
    /// A node's storage address is null: a declared-but-uninitialised variable or
    /// assignment target. The interpreter returns `RunError::BadValue` for the same
    /// node; the compiler refuses rather than baking a load/store to address 0.
    BadValue,
    /// A numeric literal has no exact `i32` value to compute — a non-integer
    /// rational (e.g. `3.14`) or an integer outside `i32` range. Mirrors
    /// `RunError::UncomputableLiteral`.
    UncomputableLiteral,
    /// The function has more parameters than the seed's compiled calling convention
    /// supports (at most three `i32` args; see [`crate::run`]). Rejected at compile
    /// time so a 4+ parameter function stays interpreted rather than compiling to a
    /// body that errors only when called.
    UnsupportedArity(usize),
    /// A call targets a function that is neither the one being compiled nor already
    /// compiled, so there is no machine address to call. The enclosing function stays
    /// interpreted rather than baking a call to nothing.
    UncompiledCallee(SynolonPtr),
    /// A call's argument count did not match the callee's parameter count — the
    /// compile-time mirror of `RunError::ArityMismatch`, refused instead of baking a
    /// call with the wrong signature.
    ArityMismatch,
    /// Cranelift rejected the setup, function, or finalization.
    Cranelift(String),
}

/// The most parameters a compiled function may take, bounded by `run`'s
/// `call_compiled` arity dispatch (0..=3 `i32` args). Kept here so compilation
/// fails fast instead of installing bcode a later call cannot invoke.
pub const MAX_COMPILED_PARAMS: usize = 3;

/// The lowering context: a Cranelift function under construction plus the rule
/// table `lower` dispatches through, and the host pointer logos for baked
/// addresses. The `builder` is not exposed; lowering rules use the small typed
/// helpers below, so `crate::identities` needs only Cranelift's `Value`.
pub struct Lowerer<'a, 'f> {
    builder: &'a mut FunctionBuilder<'f>,
    lower: &'a LowerTable,
    ptr_ty: types::Type,
    /// Memory flags for loads/stores: plain (no alignment assumption, may trap),
    /// since variable storage is only byte-aligned. The builder interns these.
    flags: MemFlagsData,
    /// The module the function is compiled into, so a call can reference the function
    /// being defined (self-recursion) or an already-compiled callee's machine code.
    module: &'a mut dyn Module,
    /// The id of the function under construction, so a self-call becomes a direct
    /// `call` the JIT patches to this function's own address.
    func_id: FuncId,
    /// The core logos handles: `logos.fn_type` tells a call from data (a node whose
    /// operation is `fn`-typed with no lowering rule is a call), and the rest let a
    /// call's arguments resolve their numeric logos at the ABI boundary.
    types: CoreTypes,
    /// The function node being compiled (null for a bare expression), so a call to it
    /// is recognized as self-recursion rather than a call to other machine code.
    self_fn: SynolonPtr,
    /// The Cranelift stack slot backing this call's activation record — the
    /// compiled analogue of the interpreter's frame, holding the function's
    /// parameters (spilled from the block params on entry) and frame-relative
    /// locals at their parse-assigned offsets. `None` for a function with no
    /// parameters and no locals (an empty frame). A frame place lowers to
    /// `stack_addr(slot, offset)`; the machine call stack gives each activation
    /// its own copy, which is what makes compiled recursion correct.
    frame_slot: Option<StackSlot>,
    /// Analysis mode: the stats the first lowering pass records frame-place
    /// usage into. `None` on the real (second) pass.
    collect: Option<&'a mut PlaceStats>,
    /// The promoted frame places, offset → register variable and its logos —
    /// DESIGN's "locals assigned to registers or stack slots": a promoted
    /// place reads and writes a Cranelift variable (a register after regalloc)
    /// instead of its frame-slot memory. Empty on the analysis pass.
    promoted: &'a HashMap<usize, (Variable, types::Type)>,
}

impl Lowerer<'_, '_> {
    /// Lower `node`: dispatch to its operation's lowering rule; a parameter
    /// reads its frame slot through the same place machinery as a local.
    ///
    /// # Safety
    /// `node` must be a valid synolon from the store; lowering dereferences it and
    /// its operands to read baked constants and structure.
    pub unsafe fn lower(&mut self, node: SynolonPtr) -> Result<Value, CompileError> {
        let op = (*node).logos;
        // A bare parameter (`fn (a)`) has no declared logos; its frame slot holds
        // the full i64 bit-container the call passed.
        if op.is_null() {
            if frame_ref((*node).hyle).is_some() {
                let addr = self.place_addr(node);
                return Ok(self.load_at(types::I64, addr, 0));
            }
            return Err(CompileError::NotLowerable(op));
        }
        if let Some(f) = self.lower.get(&op).copied() {
            return f(self, node);
        }
        // A declaration statement — a fn literal standing as an expression —
        // lowers to unit, exactly as it runs to unit. (A record logos standing
        // as an expression is a logos value: the self-classified-root branch
        // below bakes its address, mirroring run.)
        if op == self.types.fn_type {
            return Ok(self.const_i32(0));
        }
        // A logos node standing as a value carries its identity AS its value:
        // its bits are its own address, baked as an i64 immediate — run's rule
        // mirrored (the interpreter is the compiler's oracle), which is what
        // lets a `-> logos` function compile: logos identities are interned and
        // per-run, and so is the machine code baking them. A frame place
        // *typed* by a logos (`t : logos`, a logos-valued parameter) is not a
        // logos standing as a value: its slot holds the bound logos's address,
        // read as the container.
        if op == (*op).logos {
            if frame_ref((*node).hyle).is_some() {
                return Ok(self.read_place(node, types::I64));
            }
            return Ok(self.builder.ins().iconst(types::I64, node as i64));
        }
        // A node whose operation is a user function is a call: `op` is the
        // callee. The operator identities are plain logos with lowering rules
        // above; only real functions are fn-typed.
        if !op.is_null() && (*op).logos == self.types.fn_type {
            return self.lower_call(node);
        }
        // A pointer-typed leaf (an `&x` literal or a pointer variable): pointer
        // logos nodes are created per use, so they are not in the identity-keyed
        // table; load the 8-byte address blob like any numeric variable.
        if !op.is_null() && crate::identities::numtype::is_pointer_type(op) {
            return crate::identities::numtype::lower_var(self, node);
        }
        Err(CompileError::NotLowerable(op))
    }

    /// An `i32` immediate.
    pub fn const_i32(&mut self, v: i32) -> Value {
        self.builder.ins().iconst(types::I32, i64::from(v))
    }

    /// The address a place node denotes, as an SSA pointer value: a baked
    /// `iconst` for a global/top-level place (an absolute host address), or
    /// `stack_addr(frame_slot, offset)` for a frame-relative place of this
    /// call. Materializing a frame place's address pins it to memory — a
    /// register has no address — so the analysis pass records it as dirty
    /// here; scalar reads and writes that need no address go through
    /// [`Self::read_place`]/[`Self::write_place`] instead, which is what lets
    /// them promote.
    ///
    /// # Safety
    /// `node` must be a valid place node; a frame-relative one only appears in a
    /// function whose [`compile_body`] created a `frame_slot`.
    pub(crate) unsafe fn place_addr(&mut self, node: SynolonPtr) -> Value {
        if let Some(stats) = self.collect.as_deref_mut() {
            if let Some((_, off)) = frame_ref((*node).hyle) {
                let logos = (*node).logos;
                if crate::identities::numtype::is_scalar_place_type(logos) {
                    stats.dirty.push((off, numtype_of_type(logos).bytes()));
                } else {
                    // A non-scalar place (an instance base, a bare parameter's
                    // container): the escaping extent is unknown here, so
                    // promote nothing in this function.
                    stats.kill = true;
                }
            }
        }
        debug_assert!(
            frame_ref((*node).hyle)
                .is_none_or(|(_, off)| !self.promoted.contains_key(&off)),
            "a promoted place's address must never be taken (the analysis pass keeps them apart)"
        );
        self.place_addr_raw(node)
    }

    /// [`Self::place_addr`] without the analysis bookkeeping — the address
    /// materialization itself. The one place the compiler decodes the frame
    /// tag (see [`crate::synolon::FRAME_TAG`]); the depth is a parse-time capture
    /// guard, only the offset into this call's stack slot matters here.
    unsafe fn place_addr_raw(&mut self, node: SynolonPtr) -> Value {
        match frame_ref((*node).hyle) {
            Some((_, off)) => {
                let slot = self.frame_slot.expect("a frame-relative place needs a frame slot");
                self.builder.ins().stack_addr(self.ptr_ty, slot, off as i32)
            }
            None => self.builder.ins().iconst(self.ptr_ty, (*node).hyle as usize as i64),
        }
    }

    /// Read a place as a `ct`-typed scalar: a promoted frame place reads its
    /// register variable; everything else loads from its address. The analysis
    /// pass records the use, which is what qualifies the place for promotion.
    ///
    /// # Safety
    /// `node` must be a valid place node holding a `ct`-typed scalar.
    pub(crate) unsafe fn read_place(&mut self, node: SynolonPtr, ct: types::Type) -> Value {
        if let Some((_, off)) = frame_ref((*node).hyle) {
            if let Some(&(var, vct)) = self.promoted.get(&off) {
                debug_assert_eq!(vct, ct, "a promoted place is used at one type");
                return self.builder.use_var(var);
            }
            if let Some(stats) = self.collect.as_deref_mut() {
                stats.record_use(off, ct);
            }
        }
        let addr = self.place_addr_raw(node);
        self.load_at(ct, addr, 0)
    }

    /// Write `v` (of logos `ct`) to a place — the dual of [`Self::read_place`]:
    /// a promoted frame place defines its register variable; everything else
    /// stores through its address.
    ///
    /// # Safety
    /// As [`Self::read_place`].
    pub(crate) unsafe fn write_place(&mut self, node: SynolonPtr, ct: types::Type, v: Value) {
        if let Some((_, off)) = frame_ref((*node).hyle) {
            if let Some(&(var, vct)) = self.promoted.get(&off) {
                debug_assert_eq!(vct, ct, "a promoted place is used at one type");
                self.builder.def_var(var, v);
                return;
            }
            if let Some(stats) = self.collect.as_deref_mut() {
                stats.record_use(off, ct);
            }
        }
        let addr = self.place_addr_raw(node);
        self.store_at(ct, addr, 0, v);
    }

    /// Load a `ct`-typed value through a *runtime* address (an SSA i64 pointer)
    /// at a byte offset. The address is a runtime value — from [`Self::place_addr`]
    /// (a baked `iconst` or a frame `stack_addr`) or a dereferenced pointer.
    pub(crate) fn load_at(&mut self, ct: types::Type, addr: Value, offset: i64) -> Value {
        self.builder.ins().load(ct, self.flags, addr, offset as i32)
    }

    /// Store `v` through a runtime address at a byte offset — the dual of
    /// [`Self::load_at`].
    pub(crate) fn store_at(&mut self, ct: types::Type, addr: Value, offset: i64, v: Value) {
        debug_assert_eq!(
            self.builder.func.dfg.value_type(v),
            ct,
            "store-through's value must lower to the pointee's type"
        );
        self.builder.ins().store(self.flags, v, addr, offset as i32);
    }

    /// Equality of two same-typed integer values, as an `i32` 0/1. Kept for `not`
    /// (which lowers `not x` as `x == 0`); the numeric comparison operators go through
    /// [`Lowerer::lower_compare`].
    pub fn icmp_eq(&mut self, a: Value, b: Value) -> Value {
        self.icmp(IntCC::Equal, a, b)
    }

    /// An integer comparison `a cc b`, zero-extended to the `I32` bool (`icmp` yields a
    /// one-bit `I8`).
    fn icmp(&mut self, cc: IntCC, a: Value, b: Value) -> Value {
        let c = self.builder.ins().icmp(cc, a, b);
        self.builder.ins().uextend(types::I32, c)
    }

    /// A float comparison `a cc b`, zero-extended to the `I32` bool.
    fn fcmp(&mut self, cc: FloatCC, a: Value, b: Value) -> Value {
        let c = self.builder.ins().fcmp(cc, a, b);
        self.builder.ins().uextend(types::I32, c)
    }

    /// Lower a binary arithmetic operator (`+`/`-`/`*`): the operand logos is the
    /// (committed) left operand's — the op slot holds the concrete op, not a
    /// logos — and the matching machine op is emitted over the lowered operands
    /// (`iadd`/`fadd`, …). The result logos follows the operand `Value`s.
    ///
    /// # Safety
    /// `node` must be a resolved binary numeric operator node `[lhs, rhs, op]`.
    pub(crate) unsafe fn lower_arith(
        &mut self,
        node: SynolonPtr,
        op: ArithOp,
    ) -> Result<Value, CompileError> {
        let (lhs, rhs) = operands(node);
        let nt = match numtype_of(&self.types, lhs) {
            Operand::Concrete(nt) => nt,
            // Resolution committed both operands; anything else cannot exist here.
            _ => return Err(CompileError::BadValue),
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

    /// An integer constant of `nt`'s Cranelift logos.
    fn const_int(&mut self, nt: NumType, imm: i64) -> Value {
        self.builder.ins().iconst(nt.cranelift_type(), imm)
    }

    /// Lower an integer division with the total, saturating semantics (see
    /// [`ArithOp`]): a zero divisor yields the logos's MAX, the signed MIN/-1
    /// overflow saturates to MAX, quotients truncate toward zero. Matches the
    /// interpreter's `apply_arith` — a raw `sdiv`/`udiv` would trap on the two
    /// impossible cases instead.
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

    /// Lower an integer remainder with the total semantics (see [`ArithOp`]):
    /// `x % 0` is the logos's MAX, and a signed `x % -1` is the well-defined 0
    /// (which also covers the MIN/-1 trap). Matches the interpreter's
    /// `apply_arith`.
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

    /// Lower a binary comparison (`<`/`>`/`==`/…): the operand logos is the
    /// (committed) left operand's, and the matching `icmp` (signed or unsigned
    /// per the logos) or `fcmp` is emitted, zero-extended to the `I32` bool.
    ///
    /// # Safety
    /// `node` must be a resolved binary numeric operator node `[lhs, rhs, op]`.
    pub(crate) unsafe fn lower_compare(
        &mut self,
        node: SynolonPtr,
        op: CmpOp,
    ) -> Result<Value, CompileError> {
        let (lhs, rhs) = operands(node);
        let logos = match numtype_of(&self.types, lhs) {
            Operand::Concrete(nt) => nt,
            // Resolution committed both operands; anything else cannot exist here.
            _ => return Err(CompileError::BadValue),
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

    /// Emit the machine code casting `v` (a native `from` value) to `to`, matching
    /// [`crate::identities::numtype::apply_cast`] (Rust `as`): integer widen/narrow,
    /// int↔float conversion (float→int **saturating**, so it agrees with Rust rather
    /// than trapping), and float↔float promote/demote. The result is the native `to`
    /// value; the ABI boundary re-encodes it into the `i64` container.
    pub(crate) fn emit_cast(&mut self, from: NumType, to: NumType, v: Value) -> Value {
        let tct = to.cranelift_type();
        match (from.is_float(), to.is_float()) {
            // int -> int: same width is a no-op (the bits are unchanged; signedness is
            // the consumer's concern), else extend or reduce to the target width.
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
            // int -> float.
            (false, true) => {
                if from.is_signed_int() {
                    self.builder.ins().fcvt_from_sint(tct, v)
                } else {
                    self.builder.ins().fcvt_from_uint(tct, v)
                }
            }
            // float -> int, saturating (NaN -> 0) to match Rust `as` on the interpreter.
            (true, false) => {
                if to.is_signed_int() {
                    self.builder.ins().fcvt_to_sint_sat(tct, v)
                } else {
                    self.builder.ins().fcvt_to_uint_sat(tct, v)
                }
            }
            // float -> float: promote to a wider format, demote to a narrower one.
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

    /// Emit a two-way branch on `cond` (a non-zero i32 is true): run `then_arm` in the
    /// taken block and `else_arm` in the other, each yielding a value of one agreed
    /// logos (the merge takes the arms' width, not a fixed `i32`), merged into a
    /// single value. Leaves the builder positioned in the (sealed) merge block, so the
    /// caller's next instruction — an enclosing lowering, or `compile_body`'s trailing
    /// `return_` — lands there. Nesting composes: an arm that itself branches leaves
    /// the builder in its own merge, from which this arm's `jump` fires. The shared
    /// spine of `if` and the short-circuiting `and`/`or`.
    fn branch<T, E>(
        &mut self,
        cond: Value,
        then_arm: T,
        else_arm: E,
    ) -> Result<Value, CompileError>
    where
        T: FnOnce(&mut Self) -> Result<Value, CompileError>,
        E: FnOnce(&mut Self) -> Result<Value, CompileError>,
    {
        let then_b = self.builder.create_block();
        let else_b = self.builder.create_block();
        let merge_b = self.builder.create_block();

        // Branch to the two arms; both their predecessors (this block) are now known.
        self.builder.ins().brif(cond, then_b, &[], else_b, &[]);
        self.builder.seal_block(then_b);
        self.builder.seal_block(else_b);

        self.builder.switch_to_block(then_b);
        let then_v = then_arm(self)?;
        // The merged value takes the branches' logos (they must agree) from the then arm,
        // so `if` yields whatever width its branches do rather than a fixed i32.
        let result = self.builder.append_block_param(merge_b, self.builder.func.dfg.value_type(then_v));
        self.builder.ins().jump(merge_b, &[then_v.into()]);

        self.builder.switch_to_block(else_b);
        let else_v = else_arm(self)?;
        self.builder.ins().jump(merge_b, &[else_v.into()]);

        // Both arms have jumped; the merge block's predecessors are complete.
        self.builder.seal_block(merge_b);
        self.builder.switch_to_block(merge_b);
        Ok(result)
    }

    /// Lower an `if`: branch on the condition, each arm lowering its branch to the
    /// `if`'s value. See [`Lowerer::branch`].
    ///
    /// # Safety
    /// `cond`/`then`/`els` must be valid synolons from the store.
    pub unsafe fn lower_if(
        &mut self,
        cond: SynolonPtr,
        then: SynolonPtr,
        els: SynolonPtr,
    ) -> Result<Value, CompileError> {
        let c = self.lower(cond)?;
        self.branch(c, |s| unsafe { s.lower(then) }, |s| unsafe { s.lower(els) })
    }

    /// Lower a `while` loop: jump into a header block that re-evaluates the
    /// condition (variables live in memory, so each iteration reloads them), a
    /// body block that runs for effect and jumps back, and an exit block. The
    /// header seals only after the body's back-edge exists. Yields unit (0).
    ///
    /// # Safety
    /// `cond`/`body` must be valid synolons from the store.
    pub unsafe fn lower_while(
        &mut self,
        cond: SynolonPtr,
        body: SynolonPtr,
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
        // Both of the header's predecessors (the entry jump and the back-edge)
        // now exist.
        self.builder.seal_block(header);
        self.builder.switch_to_block(exit);
        self.builder.seal_block(exit);
        Ok(self.const_i32(0))
    }

    /// Lower a `for` loop: write the start into the loop variable's place
    /// (its register variable when promoted, its storage otherwise), hoist the
    /// end and step (default 1) as SSA in the pre-header, guard `step > 0` (a
    /// non-positive step runs zero iterations, as interpreted), then loop —
    /// read the variable, compare `< end` (signed/unsigned/float per the loop
    /// logos), run the body for effect, increment. Yields unit. Block
    /// discipline as [`Lowerer::lower_while`].
    ///
    /// # Safety
    /// The parts must be valid synolons from the store, as `Parser::parse_for`
    /// builds them (`step` may be null for the default).
    pub unsafe fn lower_for(
        &mut self,
        var: SynolonPtr,
        start: SynolonPtr,
        end: SynolonPtr,
        step: SynolonPtr,
        body: SynolonPtr,
    ) -> Result<Value, CompileError> {
        let nt = of_type_node((*var).logos);
        let ct = nt.cranelift_type();

        let s = self.lower(start)?;
        self.write_place(var, ct, s);
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

        // Guard: a non-positive step runs zero iterations.
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
        let v = self.read_place(var, ct);
        let cond = if nt.is_float() {
            self.fcmp(FloatCC::LessThan, v, e)
        } else {
            let cc = if nt.is_signed_int() {
                IntCC::SignedLessThan
            } else {
                IntCC::UnsignedLessThan
            };
            self.icmp(cc, v, e)
        };
        self.builder.ins().brif(cond, body_b, &[], exit, &[]);

        self.builder.switch_to_block(body_b);
        self.builder.seal_block(body_b);
        self.lower(body)?;
        let v2 = self.read_place(var, ct);
        let inc = if nt.is_float() {
            self.builder.ins().fadd(v2, d)
        } else {
            self.builder.ins().iadd(v2, d)
        };
        self.write_place(var, ct, inc);
        self.builder.ins().jump(header, &[]);
        // Both of the header's predecessors (the entry brif and the back-edge)
        // now exist.
        self.builder.seal_block(header);
        self.builder.switch_to_block(exit);
        self.builder.seal_block(exit);
        Ok(self.const_i32(0))
    }

    /// Lower an else-less `if`: a statement, not a value — the then-branch runs for
    /// its effect when the condition holds, and both arms yield unit (0), so the
    /// merge always agrees. See [`Lowerer::branch`].
    ///
    /// # Safety
    /// `cond`/`then` must be valid synolons from the store.
    pub unsafe fn lower_if_stmt(
        &mut self,
        cond: SynolonPtr,
        then: SynolonPtr,
    ) -> Result<Value, CompileError> {
        let c = self.lower(cond)?;
        self.branch(
            c,
            |s| {
                unsafe { s.lower(then) }?;
                Ok(s.const_i32(0))
            },
            |s| Ok(s.const_i32(0)),
        )
    }

    /// Lower `a and b` short-circuit: when `a` is false the result is `false` and `b`
    /// is not evaluated; otherwise the result is `b`.
    ///
    /// # Safety
    /// `a`/`b` must be valid synolons from the store.
    pub unsafe fn lower_and(&mut self, a: SynolonPtr, b: SynolonPtr) -> Result<Value, CompileError> {
        let va = self.lower(a)?;
        self.branch(va, |s| unsafe { s.lower(b) }, |s| Ok(s.const_i32(0)))
    }

    /// Lower `a or b` short-circuit: when `a` is true the result is `true` and `b` is
    /// not evaluated; otherwise the result is `b`.
    ///
    /// # Safety
    /// `a`/`b` must be valid synolons from the store.
    pub unsafe fn lower_or(&mut self, a: SynolonPtr, b: SynolonPtr) -> Result<Value, CompileError> {
        let va = self.lower(a)?;
        self.branch(va, |s| Ok(s.const_i32(1)), |s| unsafe { s.lower(b) })
    }

    /// Lower a call `callee(args)`. A self-call (the function being compiled) becomes
    /// a direct Cranelift `call` to this function — a relocation the JIT patches to
    /// this function's own address, which is what makes compiled recursion work. A
    /// call to another already-compiled function becomes a `call_indirect` through
    /// its baked machine address. A call to a not-yet-compiled function has no
    /// address, so it cannot be lowered ([`CompileError::UncompiledCallee`]) and the
    /// enclosing function stays interpreted.
    ///
    /// The boundary follows the uniform convention (see `compile_body`): each
    /// argument widens into the `i64` bit-container per its *own* resolved logos —
    /// the compiled analogue of `eval_args` reading each argument at its width —
    /// and the result narrows per the callee's declared return logos, a void callee
    /// yielding unit. The argument count is checked against the callee's parameters
    /// ([`CompileError::ArityMismatch`], mirroring the interpreter).
    ///
    /// # Safety
    /// `node` must be a call node from the store whose `logos` is a user function.
    unsafe fn lower_call(&mut self, node: SynolonPtr) -> Result<Value, CompileError> {
        let callee = (*node).logos;
        let fields = (*callee).hyle as *const SynolonPtr;
        if fields.is_null() {
            return Err(CompileError::UncompiledCallee(callee));
        }
        // The callee's parameter count (from the input record's stored fields
        // array) and return logos (`None` for void; a `-> logos` callee cannot
        // appear here — its calls comptime-resolve at parse — but the guard
        // keeps the tag read honest).
        let input = *fields.add(FN_INPUT);
        let param_count =
            crate::identities::array::items(crate::identities::meta::record_fields_of(input)).len();
        let out = *fields.add(FN_OUTPUT);
        let ret = if is_void_type(out) {
            None
        } else if out == self.types.type_ {
            Some(NumType::I64)
        } else if crate::identities::numtype::is_scalar_place_type(out) {
            Some(numtype_of_type(out))
        } else {
            return Err(CompileError::NotLowerable(out));
        };

        // Lower each argument and widen it into its i64 bit-container.
        let args = (*node).hyle as *const SynolonPtr; // [arg0 …, null] or null
        let mut args64 = Vec::new();
        if !args.is_null() {
            let mut i = 0;
            while !(*args.add(i)).is_null() {
                let arg = *args.add(i);
                let v = self.lower(arg)?;
                let nt = match numtype_of(&self.types, arg) {
                    Operand::Concrete(nt) => nt,
                    // A pointer rides the container as its 8-byte address.
                    Operand::Pointer(_) => NumType::U64,
                    // An uncommitted literal lowers as the bare-literal i32 default;
                    // a non-numeric value (a void call's unit) rides as the i32 unit.
                    Operand::Literal | Operand::NonNumeric => NumType::I32,
                };
                args64.push(widen_to_i64(self.builder, v, nt));
                i += 1;
            }
        }
        if args64.len() != param_count {
            return Err(CompileError::ArityMismatch);
        }

        let inst = if callee == self.self_fn {
            // Self-recursion: reference the function under construction by its id, so
            // the JIT resolves the call to this very function's address.
            let fref = self.module.declare_func_in_func(self.func_id, &mut *self.builder.func);
            self.builder.ins().call(fref, &args64)
        } else {
            // Otherwise the callee must already be compiled: call its machine code
            // through the entry of the callable node in its `bcode` slot.
            let bcode = *fields.add(FN_BCODE);
            if bcode.is_null() {
                return Err(CompileError::UncompiledCallee(callee));
            }
            let entry = crate::identities::callable::entry_of(bcode);
            let mut sig = self.module.make_signature();
            for _ in 0..param_count {
                sig.params.push(AbiParam::new(types::I64));
            }
            sig.returns.push(AbiParam::new(types::I64));
            let sigref = self.builder.import_signature(sig);
            let addr = self.builder.ins().iconst(self.ptr_ty, entry as i64);
            self.builder.ins().call_indirect(sigref, addr, &args64)
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
    /// Call the compiled `fn() -> i64` and return the raw `i64` bit-container it
    /// yields (the interpreter's value representation; see [`compile_body`]'s uniform
    /// ABI). The caller reinterprets the bits per the function's return logos.
    ///
    /// # Safety
    /// The compiled function must be nullary (it is, when produced by
    /// [`compile_nullary_i32`]) and any host addresses it baked in must still be valid.
    pub unsafe fn call(&self) -> i64 {
        let f: extern "C" fn() -> i64 = std::mem::transmute(self.ptr);
        f()
    }
}

/// Compile a function literal and install its machine code on the node. Reads the
/// parameter nodes from the input record and the `body` (see
/// [`crate::parse::FN_BODY`]), compiles the body with each parameter reference
/// lowering to its matching argument (narrowed from the `i64` bit-container to the
/// parameter's declared logos) and the return following the declared output
/// (`-> void` yields unit), then mints a `callable` node — the finalized entry
/// under the `container-i64` convention, the backend's licensed mint (DESIGN ›The
/// callable ground is `@exec`‹) — into the node's `bcode` slot
/// ([`crate::parse::FN_BCODE`]) so [`crate::run`] calls it with the arguments
/// instead of walking the body. One representation of jumpable code in the whole
/// graph: a compiled fn's code is the same kind of value `add_i32` carries.
///
/// The returned [`Compiled`] *owns* the executable memory; the installed callable
/// is only valid while it is alive, so the caller must keep it alive for as long
/// as the function may be run compiled (a use-after-free otherwise). This is the
/// same lifetime contract as [`Compiled`] itself; graph-managed ownership arrives
/// with deoptimization.
///
/// # Safety
/// `fn_node` must be a valid function node (`{logos: fn, value -> [input, output,
/// body, bcode]}`) from the store, and any storage its body references must outlive
/// every call to the returned [`Compiled`].
pub unsafe fn compile_fn(
    store: &mut crate::store::Store,
    lower: &LowerTable,
    types: CoreTypes,
    fn_node: SynolonPtr,
) -> Result<Compiled, CompileError> {
    let compiled = compile_fn_body(lower, types, fn_node)?;
    // Mint the callable — the finalized entry plus its convention — and install
    // the node into the bcode slot. `run` reads the entry back and calls it.
    let code = crate::identities::callable::mint(
        store,
        types.callable_,
        compiled.ptr as usize,
        types.conv_container,
    );
    let bcode_slot = ((*fn_node).hyle as *mut SynolonPtr).add(FN_BCODE);
    *bcode_slot = code;
    Ok(compiled)
}

/// Compile a function node's body to machine code, without minting or
/// installing anything — the shared work of [`compile_fn`] and
/// [`compile_into`].
///
/// # Safety
/// As [`compile_fn`].
unsafe fn compile_fn_body(
    lower: &LowerTable,
    types: CoreTypes,
    fn_node: SynolonPtr,
) -> Result<Compiled, CompileError> {
    let fields = (*fn_node).hyle as *const SynolonPtr;
    if fields.is_null() {
        return Err(CompileError::NotLowerable(fn_node));
    }
    // The parameter nodes, from the input record's stored fields array (see
    // `Parser::parse_record`).
    let input = *fields.add(FN_INPUT);
    let params: Vec<SynolonPtr> =
        crate::identities::array::items(crate::identities::meta::record_fields_of(input)).to_vec();
    let body = *fields.add(FN_BODY);
    // A `-> void` function yields unit (compiled to `return 0`); a `-> logos`
    // function yields a logos identity's address, already the i64 container
    // (logos values are node addresses, so a logos-returning function is
    // integers in, an integer out — and comptime evaluation runs it like any
    // other, jumping to installed bcode per ›Build and run are one
    // self-directing pass‹); every other compilable output is a scalar logos
    // the body's value widens to. A remaining non-scalar output (a record)
    // refuses cleanly.
    let out = *fields.add(FN_OUTPUT);
    let ret = if is_void_type(out) {
        None
    } else if out == types.type_ {
        Some(NumType::I64)
    } else if crate::identities::numtype::is_scalar_place_type(out) {
        Some(numtype_of_type(out))
    } else {
        return Err(CompileError::NotLowerable(out));
    };
    // The fn node is its own self-reference: a call to it inside `body` is recursion.
    compile_body(lower, types, fn_node, body, &params, ret)
}

/// `f.compile()`'s run half: compile `fn_node`'s body and install the finalized
/// entry into `code_leaf`, the callable the parser pre-minted (with a zero
/// entry) when it built the compile node — minting needs the store, which the
/// parser holds; patching an entry does not. The leaf then goes into the fn's
/// `bcode` slot, so the next call jumps to the machine code instead of walking
/// the body (DESIGN ›Build and run are one self-directing pass‹: "a construct
/// can `compile` and then `run` a function during the same pass").
///
/// The compiled artifact owns its executable memory and is deliberately
/// leaked: the installed entry must stay valid for every later call, and
/// graph-managed artifact ownership arrives with deoptimization (see
/// [`compile_fn`]'s lifetime note). One leak per `f.compile()`, alive to
/// process exit — the same lifetime the machine code itself needs.
///
/// # Safety
/// As [`compile_fn`]; `code_leaf` must be a callable value from the store.
pub(crate) unsafe fn compile_into(
    lower: &LowerTable,
    types: CoreTypes,
    fn_node: SynolonPtr,
    code_leaf: SynolonPtr,
) -> Result<(), CompileError> {
    let compiled = compile_fn_body(lower, types, fn_node)?;
    crate::identities::callable::install_entry(code_leaf, compiled.ptr as usize);
    let bcode_slot = ((*fn_node).hyle as *mut SynolonPtr).add(FN_BCODE);
    *bcode_slot = code_leaf;
    std::mem::forget(compiled);
    Ok(())
}

/// Compile `root` as a nullary function returning `i32` (a bare expression with no
/// parameters).
///
/// # Safety
/// See [`compile_body`].
pub unsafe fn compile_nullary_i32(
    lower: &LowerTable,
    types: CoreTypes,
    root: SynolonPtr,
) -> Result<Compiled, CompileError> {
    // A bare expression is not a function, so there is no self to recurse into; v1
    // bare expressions are i32 (or bool, physically i32).
    compile_body(lower, types, std::ptr::null_mut(), root, &[], Some(NumType::I32))
}

/// Compile `root` as a function of `params`, spilling each argument (an `i64`
/// bit-container, narrowed to the parameter's declared logos where it has one)
/// into the parameter's frame slot on entry, and returning `ret` (`None` for
/// `-> void`, which yields unit). `root` references those parameter nodes where
/// it uses them — they read their frame slots through the same place machinery
/// as locals, which is what makes `&param` and parameter reassignment agree
/// with the interpreter — and its other leaves bake addresses/immediates as
/// usual.
///
/// # Safety
/// `root` must be a valid synolon tree from the store, and any variable storage its
/// leaves reference must outlive every call to the returned [`Compiled`] (the
/// addresses are baked into the code).
pub(crate) unsafe fn compile_body(
    lower: &LowerTable,
    types: CoreTypes,
    self_fn: SynolonPtr,
    root: SynolonPtr,
    params: &[SynolonPtr],
    ret: Option<NumType>,
) -> Result<Compiled, CompileError> {
    // Fail fast on arities the compiled calling convention cannot call, so the
    // function stays interpreted (its bcode is never installed) instead of
    // compiling into a body that errors only at the call site.
    if params.len() > MAX_COMPILED_PARAMS {
        return Err(CompileError::UnsupportedArity(params.len()));
    }
    // Two passes (DESIGN ›Operands travel on the stack‹: compiled code has
    // "locals assigned to registers or stack slots"). The first lowers into a
    // discarded function while recording how every frame place is used; the
    // offsets used only as consistent scalars, with their address never
    // materialized, then promote to register variables on the real pass —
    // FunctionBuilder's SSA construction carries them across blocks, and the
    // register allocator keeps them out of memory. Address-taken places
    // (`&x`), instance bases, and bare parameters stay in the frame slot,
    // where the interpreter's layout and pointers into the frame need them.
    let mut stats = PlaceStats::default();
    build_pass(lower, types, self_fn, root, params, ret, Some(&mut stats), &[], false)?;
    let promote = stats.promotable();
    let compiled = build_pass(lower, types, self_fn, root, params, ret, None, &promote, true)?;
    Ok(compiled.expect("the finishing pass returns the artifact"))
}

/// One lowering pass of [`compile_body`]: analysis (`collect` set, `finish`
/// false — the built function is discarded) or the real build (`promote`
/// filled, `finish` true — the function is defined and finalized). `promote`
/// lists the frame offsets to place in register variables, with their logos;
/// the variables themselves are minted from the pass's own builder.
///
/// # Safety
/// See [`compile_body`].
#[allow(clippy::too_many_arguments)]
unsafe fn build_pass(
    lower: &LowerTable,
    types: CoreTypes,
    self_fn: SynolonPtr,
    root: SynolonPtr,
    params: &[SynolonPtr],
    ret: Option<NumType>,
    mut collect: Option<&mut PlaceStats>,
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
    // The calling convention is uniform `(i64…) -> i64`: every parameter and the
    // result is passed as the interpreter's `i64` bit-container, reinterpreted to its
    // real logos at the boundary. This keeps `run::call_compiled` a fixed
    // `fn(i64…) -> i64` regardless of the parameter/return logos.
    for _ in params {
        ctx.func.signature.params.push(AbiParam::new(types::I64));
    }
    ctx.func.signature.returns.push(AbiParam::new(types::I64));

    // Declare the function before lowering its body, so a self-call can reference its
    // id; the JIT patches that call to this function's own address once it is defined.
    let func_id = module
        .declare_function("main", Linkage::Export, &ctx.func.signature)
        .map_err(cl)?;

    let mut fctx = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        // The activation record: one explicit stack slot sized to the function's
        // frame — its parameters first, its frame-relative locals after — 8-byte
        // aligned. A frameless function (`self_fn` null for a bare expression,
        // or no parameters and no locals) gets none, and its places all bake
        // absolute addresses as before.
        let frame_size = if self_fn.is_null() { 0 } else { fn_frame_size(self_fn) };
        let frame_slot = (frame_size > 0).then(|| {
            // Rounded up to whole i64 words so the zeroing below covers it.
            let size = (frame_size as u32).next_multiple_of(8);
            let slot = builder
                .create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, size, 3));
            // Zero the record on entry, exactly as the interpreter zeroes its
            // frame: a typed declaration (`a : i32`) has no initializer, so its
            // first read must see the same zeroed "undefined" on both tiers.
            let zero = builder.ins().iconst(types::I64, 0);
            for off in (0..size).step_by(8) {
                builder.ins().stack_store(zero, slot, off as i32);
            }
            slot
        });

        // Every promoted place becomes a builder-minted variable, defined
        // before the body lowers: parameters from their narrowed arguments
        // below, locals to the all-zero value of their logos — the register
        // form of the zeroed frame, so a typed declaration's first read sees
        // the same zeroed "undefined" on both tiers.
        let param_offs: Vec<Option<usize>> =
            params.iter().map(|&p| frame_ref((*p).hyle).map(|(_, off)| off)).collect();
        let mut promoted: HashMap<usize, (Variable, types::Type)> = HashMap::new();
        for &(off, ct) in promote {
            let var = builder.declare_var(ct);
            promoted.insert(off, (var, ct));
            if param_offs.iter().flatten().any(|&poff| poff == off) {
                continue; // defined from its argument below
            }
            let zero = match ct {
                types::F32 => builder.ins().f32const(0.0),
                types::F64 => builder.ins().f64const(0.0),
                _ => builder.ins().iconst(ct, 0),
            };
            builder.def_var(var, zero);
        }

        // Bind each argument — the compiled side of the one calling
        // convention: the caller passes the i64 bit-containers per the uniform
        // signature, and entry narrows each to the parameter's declared scalar
        // logos (a bare or logos-valued parameter keeps the full container). A
        // promoted parameter defines its register variable; the rest spill
        // into the slot the parser assigned, where `&param` and the
        // interpreter's layout expect them.
        let block_params = builder.block_params(entry).to_vec();
        for (&p, &v) in params.iter().zip(block_params.iter()) {
            // A parameter always has a parse-assigned slot; a function node
            // without one is malformed and cannot be compiled.
            let Some((_, off)) = frame_ref((*p).hyle) else {
                return Err(CompileError::NotLowerable(p));
            };
            let logos = (*p).logos;
            let scalar = crate::identities::numtype::is_scalar_place_type(logos);
            if let Some(&(var, _)) = promoted.get(&off) {
                // A promoted container parameter (a logos-valued `t : logos`,
                // promoted through its i64 reads) keeps the full container.
                let vn = if scalar { narrow_from_i64(&mut builder, v, numtype_of_type(logos)) } else { v };
                builder.def_var(var, vn);
                continue;
            }
            let slot = frame_slot.expect("parameters occupy the frame, so a frame slot exists");
            if scalar {
                let vn = narrow_from_i64(&mut builder, v, numtype_of_type(logos));
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
                collect: collect.as_deref_mut(),
                promoted: &promoted,
            };
            lw.lower(root)?
        };
        // Widen the body's value back to the `i64` bit-container for the uniform return;
        // a `-> void` function discards it (the body ran for effect) and returns unit 0.
        let ret64 = match ret {
            Some(nt) => widen_to_i64(&mut builder, value, nt),
            None => builder.ins().iconst(types::I64, 0),
        };
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

/// Narrow the `i64` bit-container `v` to `nt`'s native Cranelift value at the ABI
/// boundary. Integers reduce to their width; floats reinterpret the container's bits
/// (`f64` is the whole 64 bits, `f32` the low 32), the inverse of [`widen_to_i64`].
fn narrow_from_i64(b: &mut FunctionBuilder, v: Value, nt: NumType) -> Value {
    match nt {
        // `f64`: the container *is* the raw f64 bits (see `read_scalar`), reinterpret.
        NumType::F64 => b.ins().bitcast(types::F64, bitcast_flags(), v),
        // `f32`: the f32 bits are the container's low 32; take them, then reinterpret.
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

/// Widen `nt`'s native value `v` back to the `i64` bit-container: sign-extend signed
/// integers, zero-extend unsigned, reinterpret float bits (matching `read_scalar`,
/// which zero-extends an `f32`'s 32 bits and takes an `f64`'s 64 bits raw).
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

/// Memory flags for a scalar `bitcast`: an explicit endianness is required, but for a
/// same-size scalar reinterpret the byte order does not affect the result (it only
/// matters when lane count/size differ), so a fixed `Little` is correct on any host.
fn bitcast_flags() -> MemFlagsData {
    MemFlagsData::new().with_endianness(Endianness::Little)
}

/// Map any `Display` Cranelift error into [`CompileError::Cranelift`].
fn cl<E: std::fmt::Display>(e: E) -> CompileError {
    CompileError::Cranelift(e.to_string())
}
