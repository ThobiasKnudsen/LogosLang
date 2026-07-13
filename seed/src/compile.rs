//! `compile` (V1PLAN Phase 5): lower a dyad tree to native code with Cranelift.
//!
//! `compile` is `run`'s sibling: where `run` walks the graph and computes,
//! `compile` walks it and *emits* machine code, one IR node per graph node, then
//! finalizes to a callable function (the `bcode`). The result is meant to be
//! observably identical to `run` (the interpreter is the compiler's oracle; see
//! V1PLAN Milestone 2).
//!
//! Each core primitive carries a lowering rule (its [`LowerFn`]) exactly as it
//! carries a run native, kept in `crate::core`'s `lower` table. This seed lowers
//! a single nullary expression returning `i32`, with operand addresses and
//! literals baked as immediates (DESIGN ›operand access is baked into the machine
//! code‹). Whole-`fn` bodies, control flow, and SSA locals are later work.

use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{types, AbiParam, Endianness, InstBuilder, MemFlagsData, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};

use crate::dyad::DyadPtr;
use crate::identities::numtype::{
    numtype_of_type, of_type_node, stored_type, ArithOp, CmpOp, NumType,
};
use crate::identities::operands;
use crate::parse::{FN_BCODE, FN_BODY, FN_INPUT, FN_OUTPUT};

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
/// helpers below, so `crate::core` needs only Cranelift's `Value`.
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
    /// `fn`: a node whose operation is `fn`-typed but has no lowering rule is a call.
    fn_type: DyadPtr,
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
        // Leaf natives are in the lower table (handled above); a user function is not.
        if !op.is_null() && (*op).ty == self.fn_type {
            return self.lower_call(node);
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

    /// Lower a binary arithmetic operator (`+`/`-`/`*`): read the type stored in the
    /// node's value slot and emit the matching machine op over the lowered operands
    /// (`iadd`/`fadd`, …). The result type follows the operand `Value`s.
    ///
    /// # Safety
    /// `node` must be a resolved binary numeric operator node `[lhs, rhs, type]`.
    pub(crate) unsafe fn lower_arith(
        &mut self,
        node: DyadPtr,
        op: ArithOp,
    ) -> Result<Value, CompileError> {
        let float = of_type_node(stored_type(node)).is_float();
        let (lhs, rhs) = operands(node);
        let l = self.lower(lhs)?;
        let r = self.lower(rhs)?;
        Ok(match (op, float) {
            (ArithOp::Add, false) => self.builder.ins().iadd(l, r),
            (ArithOp::Sub, false) => self.builder.ins().isub(l, r),
            (ArithOp::Mul, false) => self.builder.ins().imul(l, r),
            (ArithOp::Add, true) => self.builder.ins().fadd(l, r),
            (ArithOp::Sub, true) => self.builder.ins().fsub(l, r),
            (ArithOp::Mul, true) => self.builder.ins().fmul(l, r),
        })
    }

    /// Lower a binary comparison (`<`/`>`/`==`/…): read the stored operand type and emit
    /// `icmp` (signed or unsigned per the type) or `fcmp`, zero-extended to the `I32`
    /// bool.
    ///
    /// # Safety
    /// `node` must be a resolved binary numeric operator node `[lhs, rhs, type]`.
    pub(crate) unsafe fn lower_compare(
        &mut self,
        node: DyadPtr,
        op: CmpOp,
    ) -> Result<Value, CompileError> {
        let ty = of_type_node(stored_type(node));
        let (lhs, rhs) = operands(node);
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

    /// Emit a two-way branch on `cond` (a non-zero i32 is true): run `then_arm` in the
    /// taken block and `else_arm` in the other, each yielding an `i32`, merged into a
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
        let result = self.builder.append_block_param(merge_b, types::I32);

        // Branch to the two arms; both their predecessors (this block) are now known.
        self.builder.ins().brif(cond, then_b, &[], else_b, &[]);
        self.builder.seal_block(then_b);
        self.builder.seal_block(else_b);

        self.builder.switch_to_block(then_b);
        let then_v = then_arm(self)?;
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

    /// Lower a call's arguments — its value struct `[arg0 …, null]`, or null for a
    /// nullary call — to their SSA values, in order.
    ///
    /// # Safety
    /// `node` must be a call node from the store.
    unsafe fn lower_args(&mut self, node: DyadPtr) -> Result<Vec<Value>, CompileError> {
        let mut args = Vec::new();
        let p = (*node).value as *const DyadPtr;
        if !p.is_null() {
            let mut i = 0;
            while !(*p.add(i)).is_null() {
                let v = self.lower(*p.add(i))?;
                args.push(v);
                i += 1;
            }
        }
        Ok(args)
    }

    /// Lower a call `callee(args)`. A self-call (the function being compiled) becomes
    /// a direct Cranelift `call` to this function — a relocation the JIT patches to
    /// this function's own address, which is what makes compiled recursion work. A
    /// call to another already-compiled function becomes a `call_indirect` through
    /// its baked machine address. A call to a not-yet-compiled function has no
    /// address, so it cannot be lowered ([`CompileError::UncompiledCallee`]) and the
    /// enclosing function stays interpreted.
    ///
    /// # Safety
    /// `node` must be a call node from the store whose `ty` is a user function.
    unsafe fn lower_call(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        let callee = (*node).ty;
        let args = self.lower_args(node)?;
        // The calling convention is uniform `(i64…) -> i64` (see `compile_body`): args
        // and the result are the `i64` bit-container, converted at the boundary. (v1
        // values are i32; sign-extend/reduce accordingly.)
        let args64 = self.widen_args(&args);

        if callee == self.self_fn {
            // Self-recursion: reference the function under construction by its id, so
            // the JIT resolves the call to this very function's address.
            let fref = self.module.declare_func_in_func(self.func_id, &mut *self.builder.func);
            let inst = self.builder.ins().call(fref, &args64);
            let r = self.builder.inst_results(inst)[0];
            return Ok(self.builder.ins().ireduce(types::I32, r));
        }

        // Otherwise the callee must already be compiled: call its machine code
        // through the address baked in its `bcode` slot.
        let fields = (*callee).value as *const DyadPtr;
        if fields.is_null() {
            return Err(CompileError::UncompiledCallee(callee));
        }
        let bcode = *fields.add(FN_BCODE);
        if bcode.is_null() {
            return Err(CompileError::UncompiledCallee(callee));
        }
        let mut sig = self.module.make_signature();
        for _ in &args64 {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));
        let sigref = self.builder.import_signature(sig);
        let addr = self.builder.ins().iconst(self.ptr_ty, bcode as usize as i64);
        let inst = self.builder.ins().call_indirect(sigref, addr, &args64);
        let r = self.builder.inst_results(inst)[0];
        Ok(self.builder.ins().ireduce(types::I32, r))
    }

    /// Widen each `i32` argument to the `i64` bit-container the calling convention
    /// passes (sign-extend, matching the interpreter's `i64::from(i32)`).
    fn widen_args(&mut self, args: &[Value]) -> Vec<Value> {
        let mut out = Vec::with_capacity(args.len());
        for &a in args {
            out.push(self.builder.ins().sextend(types::I64, a));
        }
        out
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

/// Compile a `fn (params) -> i32` and install its machine code on the node. Reads
/// the parameter nodes from the input struct and the `body` (see
/// [`crate::parse::FN_BODY`]), compiles the body against an `i32`-per-parameter
/// signature (parameter references lower to the function's arguments), then writes
/// the `exec@` into the node's `bcode` slot ([`crate::parse::FN_BCODE`]) so
/// [`crate::run`] calls it with the arguments instead of walking the body. Non-`i32`
/// parameters/returns are later work.
///
/// The returned [`Compiled`] *owns* the executable memory; the installed `bcode` is
/// only valid while it is alive, so the caller must keep it alive for as long as the
/// function may be run compiled (a use-after-free otherwise). This is the same
/// lifetime contract as [`Compiled`] itself; graph-managed ownership arrives with
/// deoptimization.
///
/// # Safety
/// `fn_node` must be a valid function node (`{ty: fn, value -> [input, output,
/// body, bcode]}`) from the store, and any storage its body references must outlive
/// every call to the returned [`Compiled`].
pub unsafe fn compile_fn(
    lower: &LowerTable,
    fn_type: DyadPtr,
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
    let ret_ty = numtype_of_type(*fields.add(FN_OUTPUT));
    // The fn node is its own self-reference: a call to it inside `body` is recursion.
    let compiled = compile_body(lower, fn_type, fn_node, body, &params, ret_ty)?;
    // Install the exec@ (a machine-code address) into the node's bcode slot, punned
    // into the pointer-sized cell. `run` reads it back and calls it.
    let bcode_slot = ((*fn_node).value as *mut DyadPtr).add(FN_BCODE);
    *bcode_slot = compiled.ptr as DyadPtr;
    Ok(compiled)
}

/// Compile `root` as a nullary function returning `i32` (a bare expression with no
/// parameters).
///
/// # Safety
/// See [`compile_body`].
pub unsafe fn compile_nullary_i32(
    lower: &LowerTable,
    fn_type: DyadPtr,
    root: DyadPtr,
) -> Result<Compiled, CompileError> {
    // A bare expression is not a function, so there is no self to recurse into; v1
    // bare expressions are i32 (or bool, physically i32).
    compile_body(lower, fn_type, std::ptr::null_mut(), root, &[], NumType::I32)
}

/// Compile `root` as a function returning `i32` with one `i32` argument per entry in
/// `params`, mapping each parameter node to its argument. `root` references those
/// parameter nodes where it uses them (they resolve to the block params), and its
/// other leaves bake addresses/immediates as usual.
///
/// # Safety
/// `root` must be a valid dyad tree from the store, and any variable storage its
/// leaves reference must outlive every call to the returned [`Compiled`] (the
/// addresses are baked into the code).
pub(crate) unsafe fn compile_body(
    lower: &LowerTable,
    fn_type: DyadPtr,
    self_fn: DyadPtr,
    root: DyadPtr,
    params: &[DyadPtr],
    ret_ty: NumType,
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

        let ret = {
            let mut lw = Lowerer {
                builder: &mut builder,
                lower,
                ptr_ty,
                flags: MemFlagsData::new(),
                params: &param_map,
                module: &mut module,
                func_id,
                fn_type,
                self_fn,
            };
            lw.lower(root)?
        };
        // Widen the result back to the `i64` bit-container for the uniform return.
        let ret64 = widen_to_i64(&mut builder, ret, ret_ty);
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
