// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `compile`: lower a dyad tree to native code with Cranelift.
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
use cranelift_codegen::ir::{types, AbiParam, Endianness, InstBuilder, MemFlagsData, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};

use crate::dyad::DyadPtr;
use crate::identities::numtype::{
    is_void_type, numtype_of_type, of_type_node, ArithOp, CmpOp, NumType,
};
use crate::identities::{numtype_of, operands, Operand};
use crate::parse::{CoreTypes, FN_BCODE, FN_BODY, FN_INPUT, FN_OUTPUT};

/// A lowering rule: emit the IR for a node and return the SSA value it computes,
/// recursing on operands via [`Lowerer::lower`].
pub type LowerFn = fn(&mut Lowerer, DyadPtr) -> Result<Value, CompileError>;

/// Lowering rules keyed by operation identity (a primitive's compiled form).
pub type LowerTable = HashMap<DyadPtr, LowerFn>;

/// Why compilation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// No lowering rule is registered for this node's operation.
    NotLowerable(DyadPtr),
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
    UncompiledCallee(DyadPtr),
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
/// table `lower` dispatches through, and the host pointer type for baked
/// addresses. The `builder` is not exposed; lowering rules use the small typed
/// helpers below, so `crate::identities` needs only Cranelift's `Value`.
pub struct Lowerer<'a, 'f> {
    builder: &'a mut FunctionBuilder<'f>,
    lower: &'a LowerTable,
    ptr_ty: types::Type,
    /// Memory flags for loads/stores: plain (no alignment assumption, may trap),
    /// since variable storage is only byte-aligned. The builder interns these.
    flags: MemFlagsData,
    /// Parameter nodes mapped to the function's block params (its arguments). A
    /// parameter reference lowers to its argument value, the compiled analogue of
    /// the interpreter reading its frame; every other node dispatches through
    /// `lower`.
    params: &'a HashMap<DyadPtr, Value>,
    /// The module the function is compiled into, so a call can reference the function
    /// being defined (self-recursion) or an already-compiled callee's machine code.
    module: &'a mut dyn Module,
    /// The id of the function under construction, so a self-call becomes a direct
    /// `call` the JIT patches to this function's own address.
    func_id: FuncId,
    /// The core type handles: `types.fn_type` tells a call from data (a node whose
    /// operation is `fn`-typed with no lowering rule is a call), and the rest let a
    /// call's arguments resolve their numeric types at the ABI boundary.
    types: CoreTypes,
    /// The function node being compiled (null for a bare expression), so a call to it
    /// is recognized as self-recursion rather than a call to other machine code.
    self_fn: DyadPtr,
}

impl Lowerer<'_, '_> {
    /// Lower `node`: a parameter reference to its block param, else dispatch to its
    /// operation's lowering rule.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store; lowering dereferences it and
    /// its operands to read baked constants and structure.
    pub unsafe fn lower(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        if let Some(&v) = self.params.get(&node) {
            return Ok(v);
        }
        let op = (*node).ty;
        if let Some(f) = self.lower.get(&op).copied() {
            return f(self, node);
        }
        // A node whose operation is a user function is a call: `op` is the callee.
        // Leaf natives are in the lower table (handled above); a user function is
        // not. A core identity's operand record is not an fn record — every such
        // identity has a lowering rule above, so this guards the invariant.
        if !op.is_null() && (*op).ty == self.types.fn_type {
            if crate::identities::meta::is_operand_record(op) {
                return Err(CompileError::NotLowerable(op));
            }
            return self.lower_call(node);
        }
        // A pointer-typed leaf (an `&x` literal or a pointer variable): pointer
        // type nodes are created per use, so they are not in the identity-keyed
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

    /// Load a value of Cranelift type `ct` from a baked host address.
    pub(crate) fn load(&mut self, ct: types::Type, addr: *const u8) -> Value {
        let p = self.builder.ins().iconst(self.ptr_ty, addr as usize as i64);
        self.builder.ins().load(ct, self.flags, p, 0)
    }

    /// Store `v` into the baked host address `addr` at Cranelift type `ct`'s width —
    /// the storage dual of [`load`], and the compiler's analogue of the interpreter's
    /// `write_scalar`. `v` must already have type `ct`: a resolved assignment lowers its
    /// right side to the target variable's type, so the store width and the value width
    /// agree. The `debug_assert` guards that invariant (a mismatch would silently store
    /// the wrong number of bytes, since Cranelift's `store` writes `v`'s own width).
    pub(crate) fn store(&mut self, ct: types::Type, addr: *mut u8, v: Value) {
        debug_assert_eq!(
            self.builder.func.dfg.value_type(v),
            ct,
            "assignment's right side must lower to the target variable's type"
        );
        let p = self.builder.ins().iconst(self.ptr_ty, addr as usize as i64);
        self.builder.ins().store(self.flags, v, p, 0);
    }

    /// Load a `ct`-typed value through a *runtime* address (an SSA i64 pointer)
    /// at a byte offset — the dereference's load, where [`Self::load`] takes a
    /// baked parse-time address.
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

    /// Lower a binary arithmetic operator (`+`/`-`/`*`): the operand type is the
    /// (committed) left operand's — the op slot holds the concrete op, not a
    /// type — and the matching machine op is emitted over the lowered operands
    /// (`iadd`/`fadd`, …). The result type follows the operand `Value`s.
    ///
    /// # Safety
    /// `node` must be a resolved binary numeric operator node `[lhs, rhs, op]`.
    pub(crate) unsafe fn lower_arith(
        &mut self,
        node: DyadPtr,
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

    /// An integer constant of `nt`'s Cranelift type.
    fn const_int(&mut self, nt: NumType, imm: i64) -> Value {
        self.builder.ins().iconst(nt.cranelift_type(), imm)
    }

    /// Lower an integer division with the total, saturating semantics (see
    /// [`ArithOp`]): a zero divisor yields the type's MAX, the signed MIN/-1
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
    /// `x % 0` is the type's MAX, and a signed `x % -1` is the well-defined 0
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

    /// Lower a binary comparison (`<`/`>`/`==`/…): the operand type is the
    /// (committed) left operand's, and the matching `icmp` (signed or unsigned
    /// per the type) or `fcmp` is emitted, zero-extended to the `I32` bool.
    ///
    /// # Safety
    /// `node` must be a resolved binary numeric operator node `[lhs, rhs, op]`.
    pub(crate) unsafe fn lower_compare(
        &mut self,
        node: DyadPtr,
        op: CmpOp,
    ) -> Result<Value, CompileError> {
        let (lhs, rhs) = operands(node);
        let ty = match numtype_of(&self.types, lhs) {
            Operand::Concrete(nt) => nt,
            // Resolution committed both operands; anything else cannot exist here.
            _ => return Err(CompileError::BadValue),
        };
        let l = self.lower(lhs)?;
        let r = self.lower(rhs)?;
        if ty.is_float() {
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
            let s = ty.is_signed_int();
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
    /// type (the merge takes the arms' width, not a fixed `i32`), merged into a
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
        // The merged value takes the branches' type (they must agree) from the then arm,
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
    /// `cond`/`then`/`els` must be valid dyads from the store.
    pub unsafe fn lower_if(
        &mut self,
        cond: DyadPtr,
        then: DyadPtr,
        els: DyadPtr,
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
        // Both of the header's predecessors (the entry jump and the back-edge)
        // now exist.
        self.builder.seal_block(header);
        self.builder.switch_to_block(exit);
        self.builder.seal_block(exit);
        Ok(self.const_i32(0))
    }

    /// Lower a `for` loop: store the start into the variable's baked address,
    /// hoist the end and step (default 1) as SSA in the pre-header, guard
    /// `step > 0` (a non-positive step runs zero iterations, as interpreted),
    /// then loop — load the variable, compare `< end` (signed/unsigned/float per
    /// the loop type), run the body for effect, increment through the address.
    /// Yields unit. Block discipline as [`Lowerer::lower_while`].
    ///
    /// # Safety
    /// The parts must be valid dyads from the store, as `Parser::parse_for`
    /// builds them (`step` may be null for the default).
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
        let addr = (*var).value;

        let s = self.lower(start)?;
        self.store(ct, addr, s);
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
        let v = self.load(ct, addr as *const u8);
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
        let v2 = self.load(ct, addr as *const u8);
        let inc = if nt.is_float() {
            self.builder.ins().fadd(v2, d)
        } else {
            self.builder.ins().iadd(v2, d)
        };
        self.store(ct, addr, inc);
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
    /// `a`/`b` must be valid dyads from the store.
    pub unsafe fn lower_and(&mut self, a: DyadPtr, b: DyadPtr) -> Result<Value, CompileError> {
        let va = self.lower(a)?;
        self.branch(va, |s| unsafe { s.lower(b) }, |s| Ok(s.const_i32(0)))
    }

    /// Lower `a or b` short-circuit: when `a` is true the result is `true` and `b` is
    /// not evaluated; otherwise the result is `b`.
    ///
    /// # Safety
    /// `a`/`b` must be valid dyads from the store.
    pub unsafe fn lower_or(&mut self, a: DyadPtr, b: DyadPtr) -> Result<Value, CompileError> {
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
    /// argument widens into the `i64` bit-container per its *own* resolved type —
    /// the compiled analogue of `eval_args` reading each argument at its width —
    /// and the result narrows per the callee's declared return type, a void callee
    /// yielding unit. The argument count is checked against the callee's parameters
    /// ([`CompileError::ArityMismatch`], mirroring the interpreter).
    ///
    /// # Safety
    /// `node` must be a call node from the store whose `ty` is a user function.
    unsafe fn lower_call(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        let callee = (*node).ty;
        let fields = (*callee).value as *const DyadPtr;
        if fields.is_null() {
            return Err(CompileError::UncompiledCallee(callee));
        }
        // The callee's parameter count (input value is `[scope, p0 …, null]`) and
        // numeric return type (`None` for void).
        let params = (*(*fields.add(FN_INPUT))).value as *const DyadPtr;
        let mut param_count = 0;
        if !params.is_null() {
            while !(*params.add(param_count + 1)).is_null() {
                param_count += 1;
            }
        }
        let out = *fields.add(FN_OUTPUT);
        let ret = if is_void_type(out) { None } else { Some(numtype_of_type(out)) };

        // Lower each argument and widen it into its i64 bit-container.
        let args = (*node).value as *const DyadPtr; // [arg0 …, null] or null
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
    /// ABI). The caller reinterprets the bits per the function's return type.
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
/// parameter nodes from the input struct and the `body` (see
/// [`crate::parse::FN_BODY`]), compiles the body with each parameter reference
/// lowering to its matching argument (narrowed from the `i64` bit-container to the
/// parameter's declared type) and the return following the declared output
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
/// `fn_node` must be a valid function node (`{ty: fn, value -> [input, output,
/// body, bcode]}`) from the store, and any storage its body references must outlive
/// every call to the returned [`Compiled`].
pub unsafe fn compile_fn(
    store: &mut crate::store::Store,
    lower: &LowerTable,
    types: CoreTypes,
    fn_node: DyadPtr,
) -> Result<Compiled, CompileError> {
    let fields = (*fn_node).value as *const DyadPtr;
    if fields.is_null() {
        return Err(CompileError::NotLowerable(fn_node));
    }
    // The parameter nodes: the input struct's value is `[scope, p0 …, null]`, so
    // they run from index 1 to the null terminator (see `Parser::parse_struct`).
    let mut params = Vec::new();
    let pstart = (*(*fields.add(FN_INPUT))).value as *const DyadPtr;
    if !pstart.is_null() {
        let mut i = 1;
        while !(*pstart.add(i)).is_null() {
            params.push(*pstart.add(i));
            i += 1;
        }
    }
    let body = *fields.add(FN_BODY);
    // A `-> void` function yields unit (compiled to `return 0`); every other output is
    // a numeric type the body's value widens to.
    let out = *fields.add(FN_OUTPUT);
    let ret = if is_void_type(out) { None } else { Some(numtype_of_type(out)) };
    // The fn node is its own self-reference: a call to it inside `body` is recursion.
    let compiled = compile_body(lower, types, fn_node, body, &params, ret)?;
    // Mint the callable — the finalized entry plus its convention — and install
    // the node into the bcode slot. `run` reads the entry back and calls it.
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

/// Compile `root` as a nullary function returning `i32` (a bare expression with no
/// parameters).
///
/// # Safety
/// See [`compile_body`].
pub unsafe fn compile_nullary_i32(
    lower: &LowerTable,
    types: CoreTypes,
    root: DyadPtr,
) -> Result<Compiled, CompileError> {
    // A bare expression is not a function, so there is no self to recurse into; v1
    // bare expressions are i32 (or bool, physically i32).
    compile_body(lower, types, std::ptr::null_mut(), root, &[], Some(NumType::I32))
}

/// Compile `root` as a function of `params`, mapping each parameter node to its
/// argument (an `i64` bit-container narrowed to the parameter's declared type) and
/// returning `ret` (`None` for `-> void`, which yields unit). `root` references those
/// parameter nodes where it uses them (they resolve to the block params), and its
/// other leaves bake addresses/immediates as usual.
///
/// # Safety
/// `root` must be a valid dyad tree from the store, and any variable storage its
/// leaves reference must outlive every call to the returned [`Compiled`] (the
/// addresses are baked into the code).
pub(crate) unsafe fn compile_body(
    lower: &LowerTable,
    types: CoreTypes,
    self_fn: DyadPtr,
    root: DyadPtr,
    params: &[DyadPtr],
    ret: Option<NumType>,
) -> Result<Compiled, CompileError> {
    // Fail fast on arities the compiled calling convention cannot call, so the
    // function stays interpreted (its bcode is never installed) instead of
    // compiling into a body that errors only at the call site.
    if params.len() > MAX_COMPILED_PARAMS {
        return Err(CompileError::UnsupportedArity(params.len()));
    }
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
    // real type at the boundary. This keeps `run::call_compiled` a fixed
    // `fn(i64…) -> i64` regardless of the parameter/return types.
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

        // Each parameter node maps to its block param (the matching function arg). The
        // block param is the `i64` bit-container; narrow it to the parameter's real
        // type before the body uses it.
        let block_params = builder.block_params(entry).to_vec();
        let mut param_map = HashMap::new();
        for (&p, &v) in params.iter().zip(block_params.iter()) {
            let nt = numtype_of_type((*p).ty);
            let vn = narrow_from_i64(&mut builder, v, nt);
            param_map.insert(p, vn);
        }

        let value = {
            let mut lw = Lowerer {
                builder: &mut builder,
                lower,
                ptr_ty,
                flags: MemFlagsData::new(),
                params: &param_map,
                module: &mut module,
                func_id,
                types,
                self_fn,
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

    module.define_function(func_id, &mut ctx).map_err(cl)?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().map_err(cl)?;
    let ptr = module.get_finalized_function(func_id);

    Ok(Compiled { module, ptr })
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
