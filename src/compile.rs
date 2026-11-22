// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `compile`: lower a dyad tree to native code with Cranelift, `run`'s
//! sibling: one IR node per graph node, finalized to a callable `bcode`.
//! The interpreter is the oracle; compiled and interpreted results must agree.

use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    types, AbiParam, Block, Endianness, InstBuilder, MemFlagsData, StackSlot, StackSlotData,
    StackSlotKind, Value,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};

use crate::dyad;
use crate::dyad::DyadPtr;
use crate::identities::numtype::{is_void_type, of_type_node, ArithOp, CmpOp, NumType};
use crate::identities::read::{read_kind, Dispatch, Read};
use crate::identities::{by_copy, mint, operand_of, operands, Operand};
use crate::parse::{fn_frame_size, FN_BCODE, FN_BODY, FN_OUTPUT};
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
    /// A line parsed only when it runs, since a type its parse needs is known only then.
    ParsedAtRun,
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
    /// A call's argument count differs from the callee's parameter count.
    ArityMismatch,
    /// Cranelift rejected the setup, function, or finalization.
    Cranelift(String),
    /// A body nested deeper than the compiling thread's stack holds.
    TooDeep,
}

/// One thing being lowered that ends values when a `return` or a fault leaves through it, read
/// from the list its run frees by as the interpreter unwinds it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Unwind<'a> {
    /// A scope at its line `line`: those of `items`, its exit items, held there, and once that
    /// line `failed`, those it ended and may not have reached.
    Scope { items: &'a [DyadPtr], line: usize, failed: bool },
    /// An `if` arm's or condition's ends.
    Ends(&'a [DyadPtr]),
    /// An argument a call holds, passed as `this`; `first` marks the call's first, from which
    /// its arguments end as one.
    Held { arg: DyadPtr, this: Value, first: bool },
    /// A line, holding the first `count` of the values its lowering `id` made that nothing
    /// took, which its end frees last first (›A value's teardown runs where its life ends; the
    /// ending identity reads the type's `free` slot‹ [[R57]]).
    Line { id: usize, count: usize },
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
    /// The declared result, `None` for `-> void`: what a `return` widens.
    ret: Option<NumType>,
    /// A record result's slot address, from the argument block, and its width.
    ret_dest: Option<(Value, usize)>,
    /// The run's context, this call's first argument, handed on to every call.
    ctx: Value,
    /// The run's fault count as this code last read it, at entry or at the fault it is leaving.
    faults_seen: Value,
    /// What a `return` or a fault lowered now leaves through, outermost first.
    unwind: Vec<Unwind<'a>>,
    /// The fault exit made for each state of what is open, every scope failed at its line.
    exits: HashMap<Vec<Unwind<'a>>, Block>,
    /// The `held` slot of each scope being lowered, innermost last: the nodes whose values a
    /// line holds to its end.
    plans: Vec<&'a [DyadPtr]>,
    /// The value of each planned node, 0 from its scope's start until the node gives one, so a
    /// line's end reads 0 where a branch that makes it did not run.
    planned: HashMap<DyadPtr, Variable>,
    /// The nodes each line lowered so far made that nothing took, by its `Unwind::Line` id.
    lines: Vec<Vec<DyadPtr>>,
    /// Where a value given now goes, innermost last: each scope, `if` or loop lowered as a value.
    targets: Vec<Target>,
}

/// A scope, `if` or loop lowered as a value, which the values its lines, arms or body give pass
/// out to: `done` takes the value, `at` is how much of the unwind stack lies outside it.
#[derive(Clone, Copy)]
struct Target {
    at: usize,
    done: Block,
    ty: Option<types::Type>,
}

impl<'a> Lowerer<'a, '_> {
    /// Dispatch to the node's lowering rule, asking the reading rule the same
    /// question `run` asks, in the same order.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store.
    pub unsafe fn lower(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        if crate::stack::exhausted(crate::stack::limit()) {
            return Err(CompileError::TooDeep);
        }
        let node = self.through(node);
        let value = self.lower_node(node)?;
        if self.planned(node) {
            self.hold_to_line_end(node, value)?;
        }
        Ok(value)
    }

    /// Whether a line holds the value `node` gives to its end, which ends it: any open scope's,
    /// since a `return` from a scope inside lowers an outer scope's `defer` lines.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store.
    pub(crate) unsafe fn planned(&self, node: DyadPtr) -> bool {
        let node = self.through(node);
        self.plans.iter().any(|plan| plan.contains(&node))
    }

    /// The value a line being lowered holds for `node`, lowered in it before: read again there
    /// without lowering twice. `None` where no line holds one.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store.
    pub(crate) unsafe fn kept_value(&mut self, node: DyadPtr) -> Option<Value> {
        let node = self.through(node);
        let var = *self.planned.get(&node)?;
        Some(self.builder.use_var(var))
    }

    /// The value of planned `node`, given as `value`, joins the innermost line's, which its end
    /// frees.
    fn hold_to_line_end(&mut self, node: DyadPtr, value: Value) -> Result<(), CompileError> {
        let Some(&var) = self.planned.get(&node) else {
            return Err(CompileError::Internal("a planned value has no variable"));
        };
        if self.builder.func.dfg.value_type(value) != types::I64 {
            return Err(CompileError::NotLowerable(node));
        }
        let line = self.unwind.iter_mut().rev().find_map(|u| match u {
            Unwind::Line { id, count } => Some((*id, count)),
            _ => None,
        });
        let Some((id, count)) = line else {
            return Err(CompileError::Internal("a planned value is made outside every line"));
        };
        self.builder.def_var(var, value);
        self.lines[id].push(node);
        *count = self.lines[id].len();
        Ok(())
    }

    /// Lower `node` as a line of its own: what it makes that nothing takes ends when it ends, or
    /// when a `return` or fault leaves it.
    ///
    /// # Safety
    /// As [`Self::lower`].
    pub(crate) unsafe fn lower_line(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        let at = self.unwind.len();
        self.unwind.push(Unwind::Line { id: self.lines.len(), count: 0 });
        self.lines.push(Vec::new());
        let value = self.lower(node)?;
        self.end_to(at)?;
        Ok(value)
    }

    /// [`Self::lower`]'s dispatch, for `node` read through.
    ///
    /// # Safety
    /// As [`Self::lower`].
    unsafe fn lower_node(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        let op = dyad::ty(node);
        match read_kind(self.types, node) {
            Read::Executable(Dispatch::Call(f)) => self.lower_call_into(f, node, None),
            // Rational operations and places are interpreted only.
            Read::Executable(Dispatch::Leaf(leaf)) if self.types.ops.is_rational_leaf(leaf) => {
                Err(CompileError::NotLowerable(node))
            }
            // A mint is lowered by the rule of the type it is a mint of.
            Read::Executable(Dispatch::Leaf(_)) => {
                match self.lower.get(&mint::lowered_as(op)).copied() {
                    Some(f) => f(self, node),
                    None => Err(CompileError::NotLowerable(op)),
                }
            }
            Read::Executable(Dispatch::None) => Err(CompileError::NotLowerable(op)),
            Read::Unit => Ok(self.const_i32(0)),
            Read::Identity | Read::Node => Ok(self.node_addr(node)),
            // A view's value is the viewed node.
            Read::Address => Ok(self.node_addr(dyad::head(node).cast())),
            Read::Container(_) | Read::Behind(_) => self.read_place(node, types::I64),
            // A rational travels as the address of its sixteen bytes; only its steps refuse.
            Read::Rational => self.place_addr(node),
            Read::Scalar(nt) => self.read_place(node, nt.cranelift_type()),
            Read::Pointer(_) => self.read_place(node, NumType::U64.cranelift_type()),
            Read::Aggregate | Read::Opaque | Read::Undefined => Err(CompileError::NotLowerable(op)),
        }
    }

    pub fn const_i32(&mut self, v: i32) -> Value {
        self.builder.ins().iconst(types::I32, i64::from(v))
    }

    pub(crate) fn const_i64(&mut self, v: i64) -> Value {
        self.builder.ins().iconst(types::I64, v)
    }

    /// A node as machine code holds it, in the `i64` container: the one seam every
    /// node bake goes through. Baked, since nodes and the code baking them are both per-run.
    pub(crate) fn node_addr(&mut self, node: DyadPtr) -> Value {
        self.builder.ins().iconst(types::I64, node as i64)
    }

    /// The reading rule over an operand: a binding operand yields the dyad it names.
    ///
    /// # Safety
    /// `p` must be null or a valid dyad from the store.
    pub(crate) unsafe fn through(&self, p: DyadPtr) -> DyadPtr {
        self.types.through(p)
    }

    /// A place's address as an SSA pointer: the context's root base plus the offset for a
    /// program-frame place, `stack_addr(frame_slot, offset)` for a call-frame place. Taking
    /// the address pins the place to memory, so the analysis pass marks it dirty here.
    ///
    /// # Safety
    /// `node` must be a valid place node; a frame-relative one only appears in a
    /// function whose [`compile_body`] created a `frame_slot`.
    pub(crate) unsafe fn place_addr(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        let node = self.through(node);
        let (offset, logos) = (self.frame_offset(node), self.types.type_of(node));
        if let Some(stats) = self.collect.as_deref_mut() {
            if let Some(off) = offset {
                if crate::identities::numtype::is_scalar_type(logos) {
                    stats.dirty.push((off, of_type_node(logos).bytes()));
                } else {
                    // Unknown extent: promote nothing in this function.
                    stats.kill = true;
                }
            }
        }
        debug_assert!(
            self.frame_offset(node).is_none_or(|off| !self.promoted.contains_key(&off)),
            "a promoted place's address must never be taken (the analysis pass keeps them apart)"
        );
        self.place_addr_raw(node)
    }

    /// The offset of a place of the compiled function's own frame in this call's activation
    /// record, what promotion keys on; `None` for a program-frame place, another function's
    /// place and a literal's bytes.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store.
    unsafe fn frame_offset(&self, node: DyadPtr) -> Option<usize> {
        match self.types.frame_of(node) {
            Some((crate::binding::Frame::Call(f), off)) if self.is_own_frame(f) => Some(off),
            Some(_) | None => None,
        }
    }

    /// Whether a place of `f`'s frame lies in this call's: the two instance one record.
    ///
    /// # Safety
    /// `f` must be a `fn` node from the store.
    unsafe fn is_own_frame(&self, f: DyadPtr) -> bool {
        !self.self_fn.is_null()
            && crate::run::frame_record(f) == crate::run::frame_record(self.self_fn)
    }

    /// [`Self::place_addr`] without the analysis bookkeeping; the one place the
    /// compiler decodes a place's frame.
    unsafe fn place_addr_raw(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        let node = self.through(node);
        match self.types.frame_of(node) {
            Some((crate::binding::Frame::Root, off)) => {
                let at = std::mem::offset_of!(crate::run::Context, root) as i64;
                let base = self.load_at(self.ptr_ty, self.ctx, at);
                Ok(self.builder.ins().iadd_imm(base, off as i64))
            }
            // A place of another function's frame, or of none under this code: the checked
            // error the interpreter gives for the same shape, not a panic.
            Some((crate::binding::Frame::Call(f), off)) => match self.frame_slot {
                Some(slot) if self.is_own_frame(f) => {
                    Ok(self.builder.ins().stack_addr(self.ptr_ty, slot, off as i32))
                }
                _ => Err(CompileError::NoActivation),
            },
            // A literal's own bytes: an immediate, since literals and the code are both per-run.
            None => Ok(self.builder.ins().iconst(self.ptr_ty, dyad::value(node) as usize as i64)),
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
        if let Some(off) = self.frame_offset(node) {
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
        if let Some(off) = self.frame_offset(node) {
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

    /// Lower the scope `node`'s `lines` as a value, as its run does: the value is the one the
    /// first of its lines that gives one gives, `None` for a scope of prose alone.
    ///
    /// # Safety
    /// `node` must be a scope node from the store and `lines` its reduced lines.
    pub(crate) unsafe fn lower_scope(
        &mut self,
        node: DyadPtr,
        lines: &[DyadPtr],
    ) -> Result<Option<Value>, CompileError> {
        self.open_target();
        let lowered = self.lower_scope_given(node, lines)?;
        let value = self.close_target();
        Ok(lowered.then_some(value))
    }

    /// Lower the scope `node`'s `lines`, then its end, as its run does: a line stands at its
    /// index, which a `return` in it ends the scope at, and a value a line gives passes out to
    /// the innermost target, ending the scope at that line (›A scope's value is what it
    /// evaluates to, and `return` is an optional early exit from the enclosing function‹
    /// [[R207]]). Whether a line was lowered, one of prose alone lowering nothing.
    ///
    /// # Safety
    /// As [`Self::lower_scope`].
    pub(crate) unsafe fn lower_scope_given(
        &mut self,
        node: DyadPtr,
        lines: &[DyadPtr],
    ) -> Result<bool, CompileError> {
        use crate::identities::flow;
        let at = self.unwind.len();
        let items = crate::identities::scope::exit_items(node);
        let open = |line| Unwind::Scope { items, line, failed: false };
        self.unwind.push(open(0));
        let plan = crate::identities::scope::held_of(node);
        let zero = self.builder.ins().iconst(types::I64, 0);
        for &made in plan {
            let var = self.builder.declare_var(types::I64);
            self.builder.def_var(var, zero);
            self.planned.insert(made, var);
        }
        self.plans.push(plan);
        let mut lowered = false;
        let mut ended = false;
        for (line, &expr) in lines.iter().enumerate() {
            if flow::is_prose(self.types, expr) {
                continue;
            }
            self.unwind[at] = open(line);
            lowered = true;
            let flow = flow::of(self.types, expr);
            if flow.gives {
                self.lower_line_given(expr)?;
            } else {
                self.lower_line(expr)?;
            }
            if flow.ends_scope() {
                ended = true;
                break;
            }
        }
        // A scope that ended at a line has run its end on each way out of it.
        let ended = if ended {
            self.unwind.truncate(at);
            Ok(())
        } else {
            self.unwind[at] = open(lines.len());
            self.end_to(at)
        };
        self.plans.pop();
        ended?;
        Ok(lowered)
    }

    /// A target for the values lowered from here on to pass out to, until
    /// [`Self::close_target`].
    pub(crate) fn open_target(&mut self) {
        let done = self.builder.create_block();
        // The outermost target is the function body's, whose value is the function's result,
        // so a body every way out of which is a `return` still closes on a value of that type.
        let ty = match (self.ret_dest, self.ret) {
            _ if !self.targets.is_empty() => None,
            (Some(_), _) => Some(types::I64),
            (None, Some(nt)) => Some(nt.cranelift_type()),
            (None, None) => None,
        };
        if let Some(ty) = ty {
            self.builder.append_block_param(done, ty);
        }
        self.targets.push(Target { at: self.unwind.len(), done, ty });
    }

    /// The innermost target closes: the way that falls through it gives a zero, and the value
    /// is what each way gave, unit where none gave one.
    pub(crate) fn close_target(&mut self) -> Value {
        let target = self.targets.pop().expect("a target closes after it opens");
        match target.ty {
            Some(ty) => {
                let zero = self.zero_of(ty);
                self.builder.ins().jump(target.done, &[zero.into()]);
            }
            None => {
                self.builder.ins().jump(target.done, &[]);
            }
        }
        self.builder.seal_block(target.done);
        self.builder.switch_to_block(target.done);
        match target.ty {
            Some(_) => self.builder.block_params(target.done)[0],
            None => self.const_i32(0),
        }
    }

    /// `value` passes out to the innermost target, ending what is open inside it as a `return`
    /// ends what it leaves; what follows is unreachable.
    pub(crate) fn give(&mut self, value: Value) -> Result<(), CompileError> {
        let Some(target) = self.targets.last_mut() else {
            return Err(CompileError::Internal("a value is given outside every block"));
        };
        let ty = self.builder.func.dfg.value_type(value);
        match target.ty {
            None => {
                self.builder.append_block_param(target.done, ty);
                target.ty = Some(ty);
            }
            Some(had) if had != ty => {
                return Err(CompileError::Internal("the values one block gives lower differently"))
            }
            Some(_) => {}
        }
        let (at, done) = (target.at, target.done);
        let open = self.unwind.clone();
        // Each scope the value passes out of ends after the line that gave it, which completed.
        for u in &mut self.unwind[at..] {
            if let Unwind::Scope { line, .. } = u {
                *line += 1;
            }
        }
        // SAFETY: what is open was opened by the lowering of scopes, arms, calls and lines.
        let left = unsafe { self.end_to(at) };
        self.unwind = open;
        left?;
        self.builder.ins().jump(done, &[value.into()]);
        let after = self.builder.create_block();
        self.builder.switch_to_block(after);
        self.builder.seal_block(after);
        Ok(())
    }

    /// Lower `node` where the value it gives passes out, as [`crate::identities::flow::run_line`]
    /// runs it.
    ///
    /// # Safety
    /// `node` must be null or a valid dyad from the store.
    pub(crate) unsafe fn lower_given(&mut self, node: DyadPtr) -> Result<(), CompileError> {
        if crate::stack::exhausted(crate::stack::limit()) {
            return Err(CompileError::TooDeep);
        }
        crate::identities::flow::lower_line(self, node)
    }

    /// [`Self::lower_given`] as a line of its own, as [`Self::lower_line`].
    ///
    /// # Safety
    /// As [`Self::lower_given`].
    unsafe fn lower_line_given(&mut self, node: DyadPtr) -> Result<(), CompileError> {
        let at = self.unwind.len();
        self.unwind.push(Unwind::Line { id: self.lines.len(), count: 0 });
        self.lines.push(Vec::new());
        self.lower_given(node)?;
        self.end_to(at)
    }

    /// Ends what is open above `at`, innermost first: its own end, or a `return` or fault
    /// leaving it, as `scope::run_exit`, `scope::run_exit_after_fault` and the interpreter's
    /// unwinding do. Each is closed before its teardowns are lowered, so a fault among them
    /// leaves what lies outside it, skipping the rest of that end; a call's held arguments close
    /// together, so a fault in one's `free` skips the rest, as `drop_model::end_held_args` does,
    /// and a line's values together, as `drop_model::end_line` does.
    ///
    /// # Safety
    /// Every open scope's items must be exit item nodes and every end a held place.
    unsafe fn end_to(&mut self, at: usize) -> Result<(), CompileError> {
        use crate::identities::drop_model;
        while self.unwind.len() > at {
            let k = self.unwind.len() - 1;
            match self.unwind[k] {
                Unwind::Scope { items, line, failed } => {
                    self.unwind.truncate(k);
                    for &item in items.iter().rev() {
                        let item = drop_model::exit_item_of(item);
                        let runs = if failed {
                            item.held_at_fault(self.types, line)
                        } else {
                            item.held_at(line)
                        };
                        if runs {
                            item.lower(self)?;
                        }
                    }
                }
                Unwind::Ends(ends) => {
                    self.unwind.truncate(k);
                    for &place in ends.iter().rev() {
                        drop_model::lower_end_held(self, place)?;
                    }
                }
                Unwind::Held { .. } => {
                    let first = self.innermost(at);
                    for held in self.unwind.split_off(first).into_iter().rev() {
                        if let Unwind::Held { arg, this, .. } = held {
                            drop_model::lower_end_value(self, arg, this)?;
                        }
                    }
                }
                Unwind::Line { id, count } => {
                    self.unwind.truncate(k);
                    let made: Vec<_> = self.lines[id][..count].to_vec();
                    for node in made.into_iter().rev() {
                        let var = self.planned[&node];
                        let this = self.builder.use_var(var);
                        drop_model::lower_end_value(self, node, this)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Where the innermost thing open above `floor` starts: a call's held arguments are one,
    /// from the first it holds.
    fn innermost(&self, floor: usize) -> usize {
        let k = self.unwind.len() - 1;
        if !matches!(self.unwind[k], Unwind::Held { .. }) {
            return k;
        }
        (floor..=k)
            .rev()
            .find(|&j| matches!(self.unwind[j], Unwind::Held { first: true, .. }))
            .unwrap_or(floor)
    }

    /// After a step that can park a fault: when the run's fault count passed the one this code
    /// last saw, it leaves as a `return` leaves, every scope it stops having failed at its line
    /// and an end it stops in skipped (DESIGN ›A checked error is a fault: the task that hit it
    /// is cancelled‹ [[R141]]).
    fn leave_at_fault(&mut self) -> Result<(), CompileError> {
        let faults_at = std::mem::offset_of!(crate::run::Context, faults) as i64;
        let now = self.load_at(types::I64, self.ctx, faults_at);
        let faulted = self.builder.ins().icmp(IntCC::NotEqual, now, self.faults_seen);
        let open = self
            .unwind
            .iter()
            .map(|&u| match u {
                Unwind::Scope { items, line, .. } => Unwind::Scope { items, line, failed: true },
                held_or_ends => held_or_ends,
            })
            .collect();
        let on = self.builder.create_block();
        self.fault_exit(open, &|b, exit| {
            b.ins().brif(faulted, exit, &[], on, &[]);
        })?;
        self.builder.seal_block(on);
        self.builder.switch_to_block(on);
        Ok(())
    }

    /// Ends the block with `branch` into the fault exit of `open`, made and filled the first
    /// time: one exit per state of what is open, shared by every step that faults there and by
    /// the exits inside it, so each thing open is ended once on the way out, not once per step.
    /// The exit ends the innermost thing open through [`Self::end_to`], a fault there going
    /// straight to the exit of what lies outside it, then goes on to that exit; the exit with
    /// nothing open returns.
    fn fault_exit(
        &mut self,
        open: Vec<Unwind<'a>>,
        branch: &dyn Fn(&mut FunctionBuilder, Block),
    ) -> Result<(), CompileError> {
        if let Some(&exit) = self.exits.get(&open) {
            branch(self.builder, exit);
            return Ok(());
        }
        let exit = self.builder.create_block();
        self.exits.insert(open.clone(), exit);
        branch(self.builder, exit);
        self.builder.switch_to_block(exit);
        if open.is_empty() {
            let zero = self.builder.ins().iconst(types::I64, 0);
            self.builder.ins().return_(&[zero]);
            return Ok(());
        }
        let faults_at = std::mem::offset_of!(crate::run::Context, faults) as i64;
        let now = self.load_at(types::I64, self.ctx, faults_at);
        let outer = (std::mem::replace(&mut self.unwind, open), self.faults_seen);
        self.faults_seen = now;
        // SAFETY: what is open was opened by the lowering of exit items and held places.
        let left = unsafe { self.end_to(self.innermost(0)) }.and_then(|()| {
            let outside = self.unwind.clone();
            self.fault_exit(outside, &|b, exit| {
                b.ins().jump(exit, &[]);
            })
        });
        (self.unwind, self.faults_seen) = outer;
        left
    }

    /// The core handles, for an identity's lowering to reach the op-leaf table.
    pub(crate) fn types(&self) -> &Core {
        self.types
    }

    pub(crate) fn load_at(&mut self, ct: types::Type, addr: Value, offset: i64) -> Value {
        self.builder.ins().load(ct, self.flags, addr, offset as i32)
    }

    /// Run `ok` unless `addr` is null; on null, park the null-pointer fault and leave. The
    /// arm's zero of `ct` stands where the merge needs a value and is never read.
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
                s.call_seed(crate::run::park_null_pointer as *const () as usize, &[])?;
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

    /// Copy `width` bytes from `src` to `dst`.
    pub(crate) fn copy_bytes(&mut self, dst: Value, src: Value, width: usize) {
        emit_copy(self.builder, self.flags, dst, src, width);
    }

    /// The address `offset` bytes past `addr`.
    pub(crate) fn offset_addr(&mut self, addr: Value, offset: i64) -> Value {
        if offset == 0 {
            addr
        } else {
            self.builder.ins().iadd_imm(addr, offset)
        }
    }

    /// `v` in its `i64` container, as an argument travels.
    pub(crate) fn widen(&mut self, v: Value, nt: NumType) -> Value {
        widen_to_i64(self.builder, v, nt)
    }

    /// `v`, the lowered value of `node`, in its `i64` container, as an argument travels and as
    /// the interpreter's run gives it.
    ///
    /// # Safety
    /// `node` must be a valid dyad from the store.
    pub(crate) unsafe fn container(&mut self, node: DyadPtr, v: Value) -> Value {
        // Already at the container's width: a type value or an address.
        if self.builder.func.dfg.value_type(v) == types::I64 {
            return v;
        }
        let nt = match operand_of(self.types, node) {
            Operand::Concrete(nt) => nt,
            // A pointer rides the container as its 8-byte address.
            Operand::Pointer(_) => NumType::U64,
            // A non-numeric value (a `bool`, a void call's unit) rides as the i32 unit; a plain
            // number lowers to its address, already an `i64`.
            Operand::Literal | Operand::NonNumeric => NumType::I32,
        };
        widen_to_i64(self.builder, v, nt)
    }

    /// The `nt` value an `i64` container holds, as a call's result is read.
    pub(crate) fn narrow(&mut self, v: Value, nt: NumType) -> Value {
        narrow_from_i64(self.builder, v, nt)
    }

    /// `width` bytes of this function's frame for a result nobody named: the compiled
    /// form of the interpreter's scratch, picked at compile time.
    pub(crate) fn scratch_slot(&mut self, width: usize) -> Value {
        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (width as u32).next_multiple_of(8),
            3,
        ));
        self.builder.ins().stack_addr(self.ptr_ty, slot, 0)
    }

    /// The containers in a block on this frame, its address; null for none.
    pub(crate) fn spill(&mut self, containers: &[Value]) -> Value {
        if containers.is_empty() {
            return self.builder.ins().iconst(self.ptr_ty, 0);
        }
        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (containers.len() * 8) as u32,
            3,
        ));
        for (i, &v) in containers.iter().enumerate() {
            self.builder.ins().stack_store(v, slot, (i * 8) as i32);
        }
        self.builder.ins().stack_addr(self.ptr_ty, slot, 0)
    }

    /// A call to the seed's `extern "C"` function at `entry`, the context first, then
    /// every argument and the result an `i64`: a step whose work is the store's, which
    /// machine code cannot allocate in. A fault it parks is left at once.
    pub(crate) fn call_seed(
        &mut self,
        entry: usize,
        args: &[Value],
    ) -> Result<Value, CompileError> {
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(self.ptr_ty));
        for _ in args {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));
        let sigref = self.builder.import_signature(sig);
        let addr = self.builder.ins().iconst(self.ptr_ty, entry as i64);
        let all: Vec<Value> = std::iter::once(self.ctx).chain(args.iter().copied()).collect();
        let inst = self.builder.ins().call_indirect(sigref, addr, &all);
        let result = self.builder.inst_results(inst)[0];
        self.leave_at_fault()?;
        Ok(result)
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
    /// `node` must be a node of an arithmetic operator's mint, `[lhs, rhs]`.
    pub(crate) unsafe fn lower_arith(
        &mut self,
        node: DyadPtr,
        op: ArithOp,
    ) -> Result<Value, CompileError> {
        let (lhs, rhs) = operands(node);
        let nt = match operand_of(self.types, lhs) {
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

    /// `ineg` or `fneg` at the operand's width.
    ///
    /// # Safety
    /// `node` must be a node of negation's mint over a number type, `[operand]`.
    pub(crate) unsafe fn lower_negate(&mut self, node: DyadPtr) -> Result<Value, CompileError> {
        let operand = *(dyad::value(node) as *const DyadPtr);
        let Operand::Concrete(nt) = operand_of(self.types, operand) else {
            return Err(CompileError::Internal("a resolved negation has a numeric operand"));
        };
        let v = self.lower(operand)?;
        Ok(if nt.is_float() { self.builder.ins().fneg(v) } else { self.builder.ins().ineg(v) })
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

    /// A signed or unsigned `icmp`, or an `fcmp`, at the width the leaf of the node's
    /// mint carries, zero-extended to the `I32` bool.
    ///
    /// # Safety
    /// `node` must be a node of a comparison's mint, `[lhs, rhs]`.
    pub(crate) unsafe fn lower_compare(
        &mut self,
        node: DyadPtr,
        op: CmpOp,
    ) -> Result<Value, CompileError> {
        let (lhs, rhs) = operands(node);
        // The width is in the mint: the parse chose its leaf for the resolved operand
        // types, so they are not classified a second time here.
        let leaf = mint::operation_of(dyad::ty(node)).unwrap_or(std::ptr::null_mut());
        if let Some(equal) = self.types.ops.type_cmp_of(leaf) {
            let l = self.lower(lhs)?;
            let r = self.lower(rhs)?;
            let entry = crate::identities::ops::compiled_types_read_the_same as *const () as usize;
            let same = self.call_seed(entry, &[l, r])?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            let cc = if equal { IntCC::NotEqual } else { IntCC::Equal };
            return Ok(self.icmp(cc, same, zero));
        }
        // A map's compare against `?` is interpreted only.
        let Some(logos) = self.types.ops.cmp_nt_of(leaf) else {
            return Err(CompileError::NotLowerable(node));
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

    /// Cast `v` from `from` to `to` as the interpreter's `apply_cast` does.
    pub(crate) fn emit_cast(
        &mut self,
        from: NumType,
        to: NumType,
        v: Value,
    ) -> Result<Value, CompileError> {
        let tct = to.cranelift_type();
        Ok(match (from.is_float(), to.is_float()) {
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
            // The wrap and the NaN fault are the interpreter's own function.
            (true, false) => {
                let bits = widen_to_i64(self.builder, v, from);
                let from_tag = self.builder.ins().iconst(types::I64, from as i64);
                let to_tag = self.builder.ins().iconst(types::I64, to as i64);
                let entry = crate::run::convert_float_to_int as *const () as usize;
                let whole = self.call_seed(entry, &[bits, from_tag, to_tag])?;
                narrow_from_i64(self.builder, whole, to)
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
        })
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

    /// Each arm is lowered, then the places it frees at its end; the value an arm gives passes
    /// out to the innermost target, as the `if`'s run hands it on. A null arm runs nothing.
    ///
    /// # Safety
    /// `cond` and the arms must be null or valid dyads from the store, the ends held places.
    pub unsafe fn lower_if_given(
        &mut self,
        cond: DyadPtr,
        condition_ends: &'a [DyadPtr],
        [(then, then_ends), (els, else_ends)]: [(DyadPtr, &'a [DyadPtr]); 2],
    ) -> Result<(), CompileError> {
        let c = self.lower_condition(cond, condition_ends)?;
        let then_b = self.builder.create_block();
        let else_b = self.builder.create_block();
        let merge_b = self.builder.create_block();
        self.builder.ins().brif(c, then_b, &[], else_b, &[]);
        self.builder.seal_block(then_b);
        self.builder.seal_block(else_b);
        for (block, arm, ends) in [(then_b, then, then_ends), (else_b, els, else_ends)] {
            self.builder.switch_to_block(block);
            self.lower_arm(arm, ends)?;
            self.builder.ins().jump(merge_b, &[]);
        }
        self.builder.seal_block(merge_b);
        self.builder.switch_to_block(merge_b);
        Ok(())
    }

    /// An `if`'s condition, which frees `ends` only when a `return` leaves it: the arms free
    /// them otherwise.
    ///
    /// # Safety
    /// `cond` must be a valid dyad from the store, `ends` held places.
    unsafe fn lower_condition(
        &mut self,
        cond: DyadPtr,
        ends: &'a [DyadPtr],
    ) -> Result<Value, CompileError> {
        let at = self.unwind.len();
        self.unwind.push(Unwind::Ends(ends));
        let c = self.lower(cond)?;
        self.unwind.truncate(at);
        Ok(c)
    }

    /// An arm and the places it frees at its end, or as a `return` leaves it or a value it gives
    /// passes out.
    ///
    /// # Safety
    /// `arm` must be null or a valid dyad from the store, `ends` held places.
    unsafe fn lower_arm(&mut self, arm: DyadPtr, ends: &'a [DyadPtr]) -> Result<(), CompileError> {
        let at = self.unwind.len();
        self.unwind.push(Unwind::Ends(ends));
        self.lower_given(arm)?;
        self.end_to(at)
    }

    /// `lower` lowered as a value: what it gives passes out to a target of its own, whose value
    /// is what was given, a zero where it fell through, unit where nothing could give.
    pub(crate) fn lower_as_value(
        &mut self,
        lower: impl FnOnce(&mut Self) -> Result<(), CompileError>,
    ) -> Result<Value, CompileError> {
        self.open_target();
        lower(self)?;
        Ok(self.close_target())
    }

    /// The header re-evaluates the condition, the body jumps back, and the header seals only
    /// after that back-edge exists; a value the body gives ends the loop and passes out to the
    /// innermost target.
    ///
    /// # Safety
    /// `cond`/`body` must be valid dyads from the store.
    pub unsafe fn lower_while(&mut self, cond: DyadPtr, body: DyadPtr) -> Result<(), CompileError> {
        let header = self.builder.create_block();
        let body_b = self.builder.create_block();
        let exit = self.builder.create_block();

        self.builder.ins().jump(header, &[]);
        self.builder.switch_to_block(header);
        let c = self.lower(cond)?;
        self.builder.ins().brif(c, body_b, &[], exit, &[]);
        self.builder.switch_to_block(body_b);
        self.builder.seal_block(body_b);
        self.lower_given(body)?;
        self.builder.ins().jump(header, &[]);
        self.builder.seal_block(header);
        self.builder.switch_to_block(exit);
        self.builder.seal_block(exit);
        Ok(())
    }

    /// The start goes into the counter, the end and step (default 1) are hoisted as SSA, a
    /// `step > 0` guard runs zero iterations otherwise, as interpreted; then read, compare
    /// `< end`, body, increment. A named counter is its binding's place; a nameless one
    /// (`var` null) is an SSA variable of this function. A value the body gives ends the loop
    /// and passes out to the innermost target.
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
    ) -> Result<(), CompileError> {
        let Operand::Concrete(nt) = operand_of(self.types, start) else {
            return Err(CompileError::Internal("a loop's range is committed to one number type"));
        };
        let ct = nt.cranelift_type();
        let nameless = var.is_null().then(|| self.builder.declare_var(ct));

        let s = self.lower(start)?;
        self.write_counter(var, nameless, ct, s)?;
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
        let v = self.read_counter(var, nameless, ct)?;
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
        self.lower_given(body)?;
        let v2 = self.read_counter(var, nameless, ct)?;
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
        self.write_counter(var, nameless, ct, inc)?;
        self.builder.ins().jump(header, &[]);
        self.builder.seal_block(header);
        self.builder.switch_to_block(exit);
        self.builder.seal_block(exit);
        Ok(())
    }

    /// # Safety
    /// `var` must be null or a valid place node.
    unsafe fn read_counter(
        &mut self,
        var: DyadPtr,
        nameless: Option<Variable>,
        ct: types::Type,
    ) -> Result<Value, CompileError> {
        match nameless {
            Some(v) => Ok(self.builder.use_var(v)),
            None => self.read_place(var, ct),
        }
    }

    /// # Safety
    /// As [`Self::read_counter`].
    unsafe fn write_counter(
        &mut self,
        var: DyadPtr,
        nameless: Option<Variable>,
        ct: types::Type,
        value: Value,
    ) -> Result<(), CompileError> {
        match nameless {
            Some(v) => {
                self.builder.def_var(v, value);
                Ok(())
            }
            None => self.write_place(var, ct, value),
        }
    }

    /// `return X` leaves the function from wherever it stands, freeing `ends` last first, then
    /// what each scope, `if` and call it leaves ends as the `return` unwinds through it, and
    /// counting the call out as the epilogue does. What follows it is unreachable, so the
    /// value handed back to the enclosing lowering is a zero of `X`'s type.
    ///
    /// # Safety
    /// `value` must be a valid dyad from the store, `ends` held places.
    pub unsafe fn lower_return(
        &mut self,
        value: DyadPtr,
        ends: &[DyadPtr],
    ) -> Result<Value, CompileError> {
        let v = self.lower(value)?;
        for &place in ends.iter().rev() {
            crate::identities::drop_model::lower_end_held(self, place)?;
        }
        // The lowering after the jump still stands inside everything the `return` left.
        let open = self.unwind.clone();
        let left = self.end_to(0);
        self.unwind = open;
        left?;
        let ret64 = match (self.ret_dest, self.ret) {
            (Some((dest, width)), _) => {
                self.copy_bytes(dest, v, width);
                dest
            }
            (None, Some(nt)) => widen_to_i64(self.builder, v, nt),
            (None, None) => self.builder.ins().iconst(types::I64, 0),
        };
        self.builder.ins().return_(&[ret64]);
        let after = self.builder.create_block();
        self.builder.switch_to_block(after);
        self.builder.seal_block(after);
        Ok(match self.builder.func.dfg.value_type(v) {
            types::F32 => self.builder.ins().f32const(0.0),
            types::F64 => self.builder.ins().f64const(0.0),
            ty => self.builder.ins().iconst(ty, 0),
        })
    }

    /// Both sides run; a lowered `bool` is an `I32` `0` or `1`, so the bitwise `and` is the
    /// logical one.
    ///
    /// # Safety
    /// `a`/`b` must be valid dyads from the store.
    pub unsafe fn lower_and(&mut self, a: DyadPtr, b: DyadPtr) -> Result<Value, CompileError> {
        let va = self.lower(a)?;
        let vb = self.lower(b)?;
        Ok(self.builder.ins().band(va, vb))
    }

    /// Both sides run, as [`Self::lower_and`].
    ///
    /// # Safety
    /// `a`/`b` must be valid dyads from the store.
    pub unsafe fn lower_or(&mut self, a: DyadPtr, b: DyadPtr) -> Result<Value, CompileError> {
        let va = self.lower(a)?;
        let vb = self.lower(b)?;
        Ok(self.builder.ins().bor(va, vb))
    }

    /// A self-call is a direct `call` the JIT patches to this function; a callee
    /// with a `bcode` leaf is a `call_indirect` through the entry read from that
    /// leaf at the jump, or the interpreter when the entry is zero; a callee with
    /// no leaf is a jump into the interpreter. The argument block is
    /// laid out as the interpreter's: a record copied at its width, anything else
    /// widened into its container; `dest` is the slot a record result is copied
    /// into. The result narrows per the callee's return type. Compile order
    /// decides the call's shape, never whether it compiles.
    ///
    /// # Safety
    /// `callee` must be a `fn` node from the store and `node` a node whose
    /// value is its null-terminated argument run; `dest` an address of the result's width.
    pub(crate) unsafe fn lower_call_into(
        &mut self,
        callee: DyadPtr,
        node: DyadPtr,
        dest: Option<Value>,
    ) -> Result<Value, CompileError> {
        let fields = dyad::value(callee) as *const DyadPtr;
        let ret = return_kind(self.types, *fields.add(FN_OUTPUT))?;
        let core = self.types;
        let args = crate::parse::null_terminated(dyad::value(node) as *const DyadPtr);
        if args.len() != by_copy::slots(core, callee).count() {
            return Err(CompileError::ArityMismatch);
        }
        let words = by_copy::words(core, callee);
        // Each argument the call holds is ended at its end, or by a `return` in a later one.
        let held_from = self.unwind.len();
        let argv = if words == 0 {
            self.builder.ins().iconst(self.ptr_ty, 0)
        } else {
            let block = self.builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                (words * 8) as u32,
                3,
            ));
            if by_copy::result_width(core, callee).is_some() {
                let dest = dest.ok_or(CompileError::NotLowerable(node))?;
                self.builder.ins().stack_store(dest, block, 0);
            }
            for (i, slot) in by_copy::slots(core, callee).enumerate() {
                let arg = args[i];
                let at = (slot.word * 8) as i32;
                let holds = dyad::ty(arg) == core.hold_;
                if let Some(width) = slot.width {
                    let src = by_copy::lower_record_addr(self, arg)?;
                    let dst = self.builder.ins().stack_addr(self.ptr_ty, block, at);
                    self.copy_bytes(dst, src, width);
                    if holds {
                        let first = self.unwind.len() == held_from;
                        self.unwind.push(Unwind::Held { arg, this: dst, first });
                    }
                    continue;
                }
                let v = self.lower(arg)?;
                let v = self.container(arg, v);
                self.builder.ins().stack_store(v, block, at);
                if holds {
                    let first = self.unwind.len() == held_from;
                    self.unwind.push(Unwind::Held { arg, this: v, first });
                }
            }
            self.builder.ins().stack_addr(self.ptr_ty, block, 0)
        };
        let argc = self.builder.ins().iconst(types::I64, words as i64);
        let ctx = self.ctx;
        // Each call of machine code leaves at a fault it parked, as `call_seed` does.
        let r = if callee == self.self_fn {
            let fref = self.module.declare_func_in_func(self.func_id, &mut *self.builder.func);
            let inst = self.builder.ins().call(fref, &[ctx, argv, argc]);
            let r = self.builder.inst_results(inst)[0];
            self.leave_at_fault()?;
            r
        } else {
            let fn_node = self.node_addr(callee);
            let interpret = |s: &mut Self| -> Result<Value, CompileError> {
                let entry = crate::run::interpret_call as *const () as usize;
                s.call_seed(entry, &[fn_node, argc, argv])
            };
            let bcode = *fields.add(FN_BCODE);
            if bcode.is_null() {
                interpret(self)?
            } else {
                // Read at the jump, never baked: a recompile of the callee reaches
                // this caller, and a nulled entry sends it to the body-walk.
                let leaf = self.node_addr(bcode);
                let blob = self.builder.ins().iadd_imm(leaf, crate::dyad::PAYLOAD_OFFSET);
                let entry_at = crate::identities::callable::ENTRY_OFF as i64;
                let entry = self.load_at(self.ptr_ty, blob, entry_at);
                let zero = self.builder.ins().iconst(self.ptr_ty, 0);
                let has_code = self.icmp(IntCC::NotEqual, entry, zero);
                self.branch(
                    has_code,
                    |s| {
                        let mut sig = s.module.make_signature();
                        sig.params.push(AbiParam::new(s.ptr_ty));
                        sig.params.push(AbiParam::new(s.ptr_ty));
                        sig.params.push(AbiParam::new(types::I64));
                        sig.returns.push(AbiParam::new(types::I64));
                        let sigref = s.builder.import_signature(sig);
                        let inst = s.builder.ins().call_indirect(sigref, entry, &[ctx, argv, argc]);
                        let r = s.builder.inst_results(inst)[0];
                        s.leave_at_fault()?;
                        Ok(r)
                    },
                    interpret,
                )?
            }
        };
        self.end_to(held_from)?;
        Ok(match ret {
            Some(nt) => narrow_from_i64(self.builder, r, nt),
            None => self.const_i32(0),
        })
    }
}

/// A JIT-compiled function and the module owning its executable memory,
/// given back when the artifact drops.
pub struct Artifact {
    module: Option<JITModule>,
    ptr: *const u8,
}

impl Artifact {
    /// Call the compiled function with no arguments under `rt`; the raw `i64` container.
    ///
    /// # Safety
    /// The compiled function must be nullary and any host addresses it baked
    /// in must still be valid.
    pub unsafe fn call(&self, rt: &mut crate::run::Runtime) -> Result<i64, crate::run::RunError> {
        rt.call_compiled(self.ptr, &[])
    }
}

impl Drop for Artifact {
    fn drop(&mut self) {
        if let Some(module) = self.module.take() {
            // SAFETY: the store drops an artifact only while no jump into machine
            // code is live, a test drops a nullary one after its calls, and nothing
            // jumps to `ptr` afterwards.
            unsafe { module.free_memory() };
        }
    }
}

/// Compile a function literal, install its machine code as a `callable` under
/// the `container-i64` convention in the node's `bcode` slot, and hand the
/// store the artifact: the entry stays valid until the store retires it.
///
/// # Safety
/// `fn_node` must be a valid function node from the store, and any storage its
/// body references must outlive every call of the installed code.
pub unsafe fn compile_fn(
    store: &mut crate::store::Store,
    lower: &LowerTable,
    types: &Core,
    fn_node: DyadPtr,
) -> Result<(), CompileError> {
    let artifact = compile_fn_body(lower, types, fn_node)?;
    let bcode_slot = (dyad::value(fn_node) as *mut DyadPtr).add(FN_BCODE);
    if (*bcode_slot).is_null() {
        *bcode_slot =
            crate::identities::callable::mint(store, types.callable_, 0, types.conv_container);
    }
    install(fn_node, artifact.ptr as usize);
    store.install_artifact(fn_node, artifact);
    Ok(())
}

/// The entry goes into the leaf the `bcode` slot holds: every caller compiled so
/// far reads that leaf at the jump, so a recompile reaches them there.
///
/// # Safety
/// `fn_node` must be a fn node whose `bcode` slot holds a callable leaf.
unsafe fn install(fn_node: DyadPtr, entry: usize) {
    let leaf = *(dyad::value(fn_node) as *const DyadPtr).add(FN_BCODE);
    crate::identities::callable::install_entry(leaf, entry);
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
) -> Result<Artifact, CompileError> {
    let fields = dyad::value(fn_node) as *const DyadPtr;
    let body = *fields.add(FN_BODY);
    let ret = return_kind(types, *fields.add(FN_OUTPUT))?;
    compile_body(lower, types, fn_node, body, ret)
}

/// How the declared output travels in the container: not at all for `void`,
/// as an address for a type value, a value that is its node's address, a record behind its
/// pointer, or a record's result slot, at its width for a scalar.
///
/// # Safety
/// `out` must be a type node from the store.
unsafe fn return_kind(types: &Core, out: DyadPtr) -> Result<Option<NumType>, CompileError> {
    use crate::identities::read::{carry_of, place_kind, Carry, Read};
    if is_void_type(out) {
        Ok(None)
    } else if out == types.type_
        || out == types.dyad_
        || crate::identities::meta::value_is_node(out, types.fn_type)
        || matches!(place_kind(types, out), Read::Behind(_))
        || carry_of(types, out).and_then(Carry::bytes).is_some()
    {
        Ok(Some(NumType::I64))
    } else if crate::identities::numtype::is_scalar_type(out) {
        Ok(Some(of_type_node(out)))
    } else {
        Err(CompileError::NotLowerable(out))
    }
}

/// `f.compile()`'s run half: compile the body and install the entry into the
/// function's `bcode` leaf, which is `code_leaf`, the callable the parser
/// pre-minted, when the function had none; the store takes the artifact and
/// retires the one before it.
///
/// # Safety
/// As [`compile_fn`]; `code_leaf` must be a callable value from the store.
pub(crate) unsafe fn compile_into(
    store: &mut crate::store::Store,
    lower: &LowerTable,
    types: &Core,
    fn_node: DyadPtr,
    code_leaf: DyadPtr,
) -> Result<(), CompileError> {
    let artifact = compile_fn_body(lower, types, fn_node)?;
    let bcode_slot = (dyad::value(fn_node) as *mut DyadPtr).add(FN_BCODE);
    if (*bcode_slot).is_null() {
        *bcode_slot = code_leaf;
    }
    install(fn_node, artifact.ptr as usize);
    store.install_artifact(fn_node, artifact);
    Ok(())
}

/// # Safety
/// See [`compile_body`].
pub unsafe fn compile_nullary_i32(
    lower: &LowerTable,
    types: &Core,
    root: DyadPtr,
) -> Result<Artifact, CompileError> {
    // No self to recurse into; v1 bare expressions are i32 (or bool, physically i32).
    compile_body(lower, types, std::ptr::null_mut(), root, Some(NumType::I32))
}

/// Compile `root` as the body of `self_fn`, each argument copied from the argument
/// block into the parameter's frame slot on entry, returning `ret` (`None` for
/// `-> void`, which yields unit).
///
/// # Safety
/// `root` must be a valid dyad tree from the store, and any variable storage its
/// leaves reference must outlive every call to the returned [`Artifact`] (the
/// addresses are baked into the code).
pub(crate) unsafe fn compile_body(
    lower: &LowerTable,
    types: &Core,
    self_fn: DyadPtr,
    root: DyadPtr,
    ret: Option<NumType>,
) -> Result<Artifact, CompileError> {
    // Two passes: the first lowers into a discarded function, recording how
    // every frame place is used; the offsets used only as consistent scalars,
    // address never taken, promote to register variables on the real pass.
    let mut stats = PlaceStats::default();
    build_pass(lower, types, self_fn, root, ret, Some(&mut stats), &[], false)?;
    let promote = stats.promotable();
    let compiled = build_pass(lower, types, self_fn, root, ret, None, &promote, true)?;
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
    ret: Option<NumType>,
    mut collect: Option<&mut PlaceStats>,
    promote: &[(usize, types::Type)],
    finish: bool,
) -> Result<Option<Artifact>, CompileError> {
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
    // The one compiled signature, `(context, argv, argc) -> i64` (`crate::run::MachineFn`).
    ctx.func.signature.params.push(AbiParam::new(ptr_ty));
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

        // The stack guard's compiled half: the prologue has claimed this call's frame, and
        // under the run's limit the call parks the fault and leaves before its body runs.
        let run_ctx = builder.block_params(entry)[0];
        let limit_at = std::mem::offset_of!(crate::run::Context, stack_limit) as i32;
        let limit = builder.ins().load(ptr_ty, MemFlagsData::new(), run_ctx, limit_at);
        let sp = builder.ins().get_stack_pointer(ptr_ty);
        let over = builder.ins().icmp(IntCC::UnsignedLessThan, sp, limit);
        let fault = builder.create_block();
        let body_b = builder.create_block();
        builder.ins().brif(over, fault, &[], body_b, &[]);
        builder.switch_to_block(fault);
        builder.seal_block(fault);
        let mut park_sig = module.make_signature();
        park_sig.params.push(AbiParam::new(ptr_ty));
        park_sig.returns.push(AbiParam::new(types::I64));
        let park_sigref = builder.import_signature(park_sig);
        let park =
            builder.ins().iconst(ptr_ty, crate::run::park_too_deep as *const () as usize as i64);
        builder.ins().call_indirect(park_sigref, park, &[run_ctx]);
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().return_(&[zero]);
        builder.switch_to_block(body_b);
        builder.seal_block(body_b);
        let faults_at = std::mem::offset_of!(crate::run::Context, faults) as i32;
        let faults_seen = builder.ins().load(types::I64, MemFlagsData::new(), run_ctx, faults_at);

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
        let slots: Vec<by_copy::Slot> =
            if self_fn.is_null() { Vec::new() } else { by_copy::slots(types, self_fn).collect() };
        let mut promoted: HashMap<usize, (Variable, types::Type)> = HashMap::new();
        for &(off, ct) in promote {
            let var = builder.declare_var(ct);
            promoted.insert(off, (var, ct));
            if slots.iter().any(|s| s.offset == off) {
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
        // type; a bare or type-valued parameter keeps the full container, and a
        // record is copied at its width into a slot that stays in memory.
        let argv = builder.block_params(entry)[1];
        let ret_dest = if self_fn.is_null() {
            None
        } else {
            by_copy::result_width(types, self_fn)
                .map(|width| (builder.ins().load(types::I64, MemFlagsData::new(), argv, 0), width))
        };
        for s in &slots {
            let (p, off) = (s.param, s.offset);
            let at = (s.word * 8) as i32;
            if let Some(width) = s.width {
                if let Some(stats) = collect.as_deref_mut() {
                    stats.dirty.push((off, width));
                }
                let slot = frame_slot.expect("parameters occupy the frame, so a frame slot exists");
                let dst = builder.ins().stack_addr(ptr_ty, slot, off as i32);
                let src = builder.ins().iadd_imm(argv, i64::from(at));
                emit_copy(&mut builder, MemFlagsData::new(), dst, src, width);
                continue;
            }
            let v = builder.ins().load(types::I64, MemFlagsData::new(), argv, at);
            let logos = crate::identities::hole::type_in(p);
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
                ret,
                ret_dest,
                ctx: run_ctx,
                faults_seen,
                unwind: Vec::new(),
                exits: HashMap::new(),
                plans: Vec::new(),
                planned: HashMap::new(),
                lines: Vec::new(),
                targets: Vec::new(),
            };
            lw.lower(root)?
        };
        // The body's value widens back to the container, or is a record's address,
        // copied into the caller's slot; a `-> void` body ran for effect and returns unit.
        let ret64 = match (ret_dest, ret) {
            (Some((dest, width)), _) => {
                emit_copy(&mut builder, MemFlagsData::new(), dest, value, width);
                dest
            }
            (None, Some(nt)) => widen_to_i64(&mut builder, value, nt),
            (None, None) => builder.ins().iconst(types::I64, 0),
        };
        builder.ins().return_(&[ret64]);
        // A fault exit gains a jump from every step lowered after it, so it is sealed last.
        builder.seal_all_blocks();
        builder.finalize();
    }

    if !finish {
        // SAFETY: nothing was defined in this module, so no code points into it.
        unsafe { module.free_memory() };
        return Ok(None);
    }
    module.define_function(func_id, &mut ctx).map_err(cl)?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().map_err(cl)?;
    let ptr = module.get_finalized_function(func_id);

    Ok(Some(Artifact { module: Some(module), ptr }))
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

/// `width` bytes from `src` to `dst`, widest loads first, no alignment assumed.
fn emit_copy(b: &mut FunctionBuilder, flags: MemFlagsData, dst: Value, src: Value, width: usize) {
    let mut at = 0;
    for (ty, size) in [(types::I64, 8), (types::I32, 4), (types::I16, 2), (types::I8, 1)] {
        while width - at >= size {
            let v = b.ins().load(ty, flags, src, at as i32);
            b.ins().store(flags, v, dst, at as i32);
            at += size;
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
