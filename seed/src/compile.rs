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

use cranelift_codegen::ir::{types, AbiParam, InstBuilder, MemFlagsData, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};

use crate::dyad::DyadPtr;
use crate::parse::{FN_BCODE, FN_BODY, FN_INPUT};

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
    /// A function with parameters was handed to [`compile_fn`]. v1 compiles only
    /// nullary functions; parameters need the calling convention (frame slots),
    /// which is later work.
    NotNullary,
    /// Cranelift rejected the setup, function, or finalization.
    Cranelift(String),
}

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
}

impl Lowerer<'_, '_> {
    /// Lower `node`: dispatch to its operation's lowering rule.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store; lowering dereferences it and
    /// its operands to read baked constants and structure.
    pub unsafe fn lower(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        let op = (*node).ty;
        match self.lower.get(&op).copied() {
            Some(f) => f(self, node),
            None => Err(CompileError::NotLowerable(op)),
        }
    }

    /// An `i32` immediate.
    pub fn const_i32(&mut self, v: i32) -> Value {
        self.builder.ins().iconst(types::I32, i64::from(v))
    }

    /// Load an `i32` from a baked host address.
    pub fn load_i32(&mut self, addr: *const u8) -> Value {
        let p = self.builder.ins().iconst(self.ptr_ty, addr as usize as i64);
        self.builder.ins().load(types::I32, self.flags, p, 0)
    }

    /// Store an `i32` to a baked host address.
    pub fn store_i32(&mut self, addr: *mut u8, v: Value) {
        let p = self.builder.ins().iconst(self.ptr_ty, addr as usize as i64);
        self.builder.ins().store(self.flags, v, p, 0);
    }

    /// Integer addition.
    pub fn add(&mut self, a: Value, b: Value) -> Value {
        self.builder.ins().iadd(a, b)
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
    /// Call the compiled `fn() -> i32`.
    ///
    /// # Safety
    /// The compiled function must have signature `fn() -> i32` (it does, when
    /// produced by [`compile_nullary_i32`]) and any host addresses it baked in
    /// must still be valid.
    pub unsafe fn call_i32(&self) -> i32 {
        let f: extern "C" fn() -> i32 = std::mem::transmute(self.ptr);
        f()
    }
}

/// Compile a nullary `fn () -> i32` and install its machine code on the node.
/// Lowers the function's `body` (see [`crate::parse::FN_BODY`]) to a callable, then
/// writes the `exec@` into the node's `bcode` slot ([`crate::parse::FN_BCODE`]) so
/// [`crate::run`] jumps to it instead of walking the body. Parameters and non-`i32`
/// returns need the calling convention and wider lowering, which are later.
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
pub unsafe fn compile_fn(lower: &LowerTable, fn_node: DyadPtr) -> Result<Compiled, CompileError> {
    let fields = (*fn_node).value as *const DyadPtr;
    if fields.is_null() {
        return Err(CompileError::NotLowerable(fn_node));
    }
    // v1 compiles only nullary functions: a parameter would lower to a load from a
    // frame slot the (unbuilt) calling convention never fills, producing a load
    // from a null address. The input struct's value is `[scope, field0 …, null]`
    // (see `Parser::parse_struct`), so it has a parameter iff index 1 is not the
    // null terminator.
    let params = (*(*fields.add(FN_INPUT))).value as *const DyadPtr;
    if !params.is_null() && !(*params.add(1)).is_null() {
        return Err(CompileError::NotNullary);
    }
    let body = *fields.add(FN_BODY);
    let compiled = compile_nullary_i32(lower, body)?;
    // Install the exec@ (a machine-code address) into the node's bcode slot, punned
    // into the pointer-sized cell. `run` reads it back and jumps.
    let bcode_slot = ((*fn_node).value as *mut DyadPtr).add(FN_BCODE);
    *bcode_slot = compiled.ptr as DyadPtr;
    Ok(compiled)
}

/// Compile `root` as a nullary function returning `i32`.
///
/// # Safety
/// `root` must be a valid dyad tree from the store, and any variable storage its
/// leaves reference must outlive every call to the returned [`Compiled`] (the
/// addresses are baked into the code).
pub unsafe fn compile_nullary_i32(
    lower: &LowerTable,
    root: DyadPtr,
) -> Result<Compiled, CompileError> {
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
    ctx.func.signature.returns.push(AbiParam::new(types::I32));

    let mut fctx = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let ret = {
            let mut lw =
                Lowerer { builder: &mut builder, lower, ptr_ty, flags: MemFlagsData::new() };
            lw.lower(root)?
        };
        builder.ins().return_(&[ret]);
        builder.finalize();
    }

    let id = module
        .declare_function("main", Linkage::Export, &ctx.func.signature)
        .map_err(cl)?;
    module.define_function(id, &mut ctx).map_err(cl)?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().map_err(cl)?;
    let ptr = module.get_finalized_function(id);

    Ok(Compiled { module, ptr })
}

/// Map any `Display` Cranelift error into [`CompileError::Cranelift`].
fn cl<E: std::fmt::Display>(e: E) -> CompileError {
    CompileError::Cranelift(e.to_string())
}
