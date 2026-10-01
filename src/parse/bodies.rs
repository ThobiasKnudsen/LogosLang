// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Bodies: function bodies over a frame, run bodies, blocks and sequences, and the
//! control flow that reads a body; the fn record's slots and the analysis over a body.

use super::*;
use crate::dyad;
use crate::identities::drop_model::ExitItem;

/// One enclosing function body being parsed: parameters claim the frame's
/// first offsets, the body's locals continue after them (DESIGN ›Resolution
/// is one rule‹).
pub(super) struct OpenFn {
    /// The `fn` node whose call frame this is: what a local's binding names as its frame.
    pub(super) frame: DyadPtr,
    /// Bytes claimed so far by parameters and frame-relative locals.
    pub(super) size: usize,
    /// The scope depth the function's barrier begins at: a name declared
    /// below it is from outside the function.
    pub(super) below: usize,
    /// The bindings of the outer names the body has read so far, first-read
    /// order, each once: the function's `FN_OUTER` list.
    pub(super) outer: Vec<DyadPtr>,
    /// `open`'s length outside the body: the scopes a `return` leaves are the ones past it.
    pub(super) open_below: usize,
    /// Every `return` in the body, committed to the result type as the tail is.
    pub(super) returns: Vec<OpenReturn>,
}

/// A `return` in a function body being parsed. Its line may still end names after it, in a
/// part the `return` leaves before: the `return` frees those itself (DESIGN ›A value's
/// teardown runs where its life ends; the ending identity reads the type's `free` slot‹).
pub(super) struct OpenReturn {
    node: DyadPtr,
    /// The names the scopes it leaves hold at it, in declaration order; an `if` whose arm
    /// frees one on the `return`'s way out takes it off.
    live: Vec<DyadPtr>,
    /// The names its line ended after it.
    ended: Vec<DyadPtr>,
    /// The innermost scope it is still inside, whose line holding it is closed or not.
    level: usize,
    line_closed: bool,
    /// Where in `ended_log` its line at `level` goes on after it.
    cursor: usize,
}

#[derive(Default)]
pub(super) struct OpenScope {
    /// What the scope's end runs, in line order: the names it holds, which no tail or
    /// `return` leaving the scope may hand out, and its `defer` lines.
    pub(super) exit: Vec<ExitItem>,
    /// The lines closed so far, so the index of the one being parsed.
    pub(super) lines: usize,
    /// Held names a `move` or `free` in the line being parsed ended; the line's close
    /// settles them.
    pub(super) ended: Vec<DyadPtr>,
    /// At the top level: owners a `move` or `free` ended that no open scope holds, which the
    /// top scope's exit does, from an earlier REPL line.
    pub(super) ended_earlier: Vec<DyadPtr>,
    /// Items parsed at depth 0 and not yet run, each with its line and the names it ends that
    /// an enclosing scope or an earlier REPL line holds: `drain` runs them when the pass needs
    /// a value, the scope's own run otherwise.
    pub(super) unrun: Vec<(DyadPtr, usize, Vec<DyadPtr>)>,
    /// The lines the pass has run, up to which a fault's end ends what they hold.
    ran: usize,
    /// Names this scope holds, or at the top level an earlier REPL line, whose ending line in
    /// a nested scope the pass has run: a fault's end frees them no more.
    reached: Vec<DyadPtr>,
    /// The block node whose `dyads` hold the items, when the scope runs as one
    /// later: a drain leaves its cursor on the frame. None at the top level and in a type body.
    scope: Option<DyadPtr>,
    /// A `[…]`: its items go whole to the call that takes them, never pending
    /// (DESIGN ›A bracket goes to the call whole‹).
    pub(super) list: bool,
    /// The tape cells and lines a check in this scope has narrowed, each with the type
    /// its read yields.
    pub(super) narrowed: Vec<(CellKey, DyadPtr)>,
}

/// A function node's value record, in order: input `record`, return type,
/// body, compiled `callable` (null until compiled), frame size, outer names
/// (DESIGN ›Execution is function application‹).
pub const FN_INPUT: usize = 0;

pub const FN_OUTPUT: usize = 1;

pub const FN_BODY: usize = 2;

/// A `callable` node (`[entry: @exec, convention]`), null until compiled.
pub const FN_BCODE: usize = 3;

/// A `u64` leaf: the frame's byte size, parameters first, then the locals at
/// their offsets; null for none. Both tiers read it on entry.
pub const FN_FRAME: usize = 4;

/// An `array` of the bindings of the outer names the body reads, in
/// first-read order, or null; read at every call (DESIGN ›`move` and `free`
/// are static‹).
pub const FN_OUTER: usize = 5;

/// A `u64` leaf of [`RECEIVER_READS`] and [`RECEIVER_WRITES`] bits: whether a
/// `share` function's body reads or writes a field of its value; null for neither.
pub const FN_RECEIVER: usize = 6;

pub const FN_SLOTS: usize = FN_RECEIVER + 1;

pub const RECEIVER_READS: u64 = 1;

pub const RECEIVER_WRITES: u64 = 2;

/// `0` when the slot is null (no parameters and no locals).
///
/// # Safety
/// `fn_node` must be a function node whose value is the seven-slot record
/// `parse_fn` builds.
pub unsafe fn fn_frame_size(fn_node: DyadPtr) -> usize {
    let frame = *(dyad::value(fn_node) as *const DyadPtr).add(FN_FRAME);
    if frame.is_null() {
        0
    } else {
        std::ptr::read_unaligned(dyad::value(frame) as *const u64) as usize
    }
}

/// Empty for a function that reads none, and for one whose body is still being parsed.
///
/// # Safety
/// `fn_node` must be a function node: the operands `parse_fn` builds, the early
/// signature included.
pub unsafe fn fn_outer<'a>(fn_node: DyadPtr) -> &'a [DyadPtr] {
    let outer = *(dyad::value(fn_node) as *const DyadPtr).add(FN_OUTER);
    if outer.is_null() {
        &[]
    } else {
        crate::identities::array::items(outer)
    }
}

/// The [`FN_RECEIVER`] bits; `0` for anything but a built function node.
///
/// # Safety
/// `f` must be a resolved dyad from the store.
pub(crate) unsafe fn fn_receiver(types: &Core, f: DyadPtr) -> u64 {
    if f.is_null() || dyad::ty(f) != types.fn_type {
        return 0;
    }
    let leaf = *(dyad::value(f) as *const DyadPtr).add(FN_RECEIVER);
    if leaf.is_null() {
        0
    } else {
        std::ptr::read_unaligned(dyad::value(leaf) as *const u64)
    }
}

/// Each open bracket recurses through `parse_sequence` and `(`'s constructor,
/// so a wall of `(` costs Rust stack like a runaway recursion: sized well
/// under `crate::WORK_STACK_BYTES` and far past anything a person writes.
pub const MAX_BRACKET_DEPTH: usize = 2_000;

/// A `bool` value, a comparison, or a logical operator.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_bool_result(types: &Core, node: DyadPtr) -> bool {
    let node = types.through(node);
    let logos = types.type_of(node);
    // A sequence's value is its trailing expression's.
    if logos == types.scope {
        return match last_sequence_expr(node) {
            Some(last) => is_bool_result(types, last),
            None => false,
        };
    }
    // `and`/`or` over two booleans is one; over two non-booleans it is a group.
    if logos == types.and_ || logos == types.or_ {
        let (lhs, rhs) = crate::identities::operands(node);
        return is_bool_result(types, lhs) && is_bool_result(types, rhs);
    }
    // A call, or a node of a type with a run, is what its function declares it returns.
    let f = if crate::identities::meta::is_record_type(logos)
        && !crate::identities::meta::run_body_of(logos).is_null()
    {
        crate::identities::run_body::spec_of(node)
    } else {
        logos
    };
    if !f.is_null() && dyad::ty(f) == types.fn_type {
        return *(dyad::value(f) as *const DyadPtr).add(FN_OUTPUT) == types.bool_;
    }
    logos == types.bool_
        || logos == types.lt
        || logos == types.gt
        || logos == types.eq
        || logos == types.le
        || logos == types.ge
        || logos == types.ne
        || logos == types.not_
        || logos == types.subset
        || logos == types.tape.is_constructed
}

/// Deliberately no scope unwrapping: a sequence-valued condition may carry
/// effectful non-tail expressions a fold would silently drop.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn bool_literal_value(types: &Core, node: DyadPtr) -> Option<bool> {
    let node = types.through(node);
    if dyad::ty(node) != types.bool_ {
        return None;
    }
    Some(std::ptr::read_unaligned(dyad::value(node) as *const i32) != 0)
}

/// A cell or line of the tape, by the tape's place, a literal index and the line.
pub(crate) type CellKey = (DyadPtr, i32, crate::identities::tape::Line);

/// `tape[k].type == T` or `!=` with `T` a number type or `type`, either side first, or
/// `tape[k].type ⊆ T` with `T` a number type, each under any number of `not`: the cell
/// checked, the type it narrows to, and whether the branch that holds is the `then`.
///
/// # Safety
/// `cond` must be a reduced dyad from the store.
unsafe fn type_check_of(types: &Core, cond: DyadPtr) -> Option<(CellKey, DyadPtr, bool)> {
    let cond = types.through(cond);
    if dyad::ty(cond) == types.not_ {
        let (key, ty, holds) = type_check_of(types, *(dyad::value(cond) as *const DyadPtr))?;
        return Some((key, ty, !holds));
    }
    let subset = dyad::ty(cond) == types.subset;
    let eq = subset || dyad::ty(cond) == types.eq;
    if !eq && dyad::ty(cond) != types.ne {
        return None;
    }
    let ops = dyad::value(cond) as *const DyadPtr;
    let (l, r) = (types.through(*ops), types.through(*ops.add(1)));
    let is_read = |n: DyadPtr| dyad::ty(n) == types.tape.cell_type;
    let (read, ty) = if is_read(l) {
        (l, r)
    } else if is_read(r) && !subset {
        (r, l)
    } else {
        return None;
    };
    let number = crate::identities::is_numtype_node(types, ty)
        || crate::identities::meta::is_node_valued(ty, types.fn_type);
    if !number && (subset || ty != types.type_) {
        return None;
    }
    crate::identities::tape::cell_key(types, read).map(|key| (key, ty, eq))
}

/// Trailing comment nodes are prose, not the tail; `None` for a scope with
/// no expression array.
///
/// # Safety
/// `node` must be a valid dyad from the store; a non-null value must be the
/// `[exprs, op, parent]` triple `parse_sequence` fills.
pub(crate) unsafe fn last_sequence_expr(node: DyadPtr) -> Option<DyadPtr> {
    crate::identities::scope::exprs_of(node)?
        .iter()
        .rev()
        .find(|&&e| !crate::identities::numtype::is_comment_type(dyad::ty(e)))
        .copied()
}

/// The positions `commit_tail` enumerates: a `return` itself, an `if`'s
/// branches, a sequence's expressions.
///
/// # Safety
/// `node` must be a valid dyad from the store, with the value shapes its
/// type implies.
pub(crate) unsafe fn contains_return(types: &Core, node: DyadPtr) -> bool {
    let logos = dyad::ty(node);
    if logos == types.return_ {
        return true;
    }
    if logos == types.if_ {
        let p = dyad::value(node) as *const DyadPtr;
        let (then, els) = (*p.add(1), *p.add(2));
        return contains_return(types, then) || (!els.is_null() && contains_return(types, els));
    }
    if logos == types.scope {
        let arr = *(dyad::value(node) as *const DyadPtr);
        return crate::identities::array::items(arr).iter().any(|&e| contains_return(types, e));
    }
    false
}

/// `{type: callee, value: [args…, null]}`: null-terminated so `run` can count
/// the arguments; a nullary call is a leaf, its one zero word the terminator.
pub(crate) fn build_call(store: &mut Store, callee: DyadPtr, args: &[DyadPtr]) -> DyadPtr {
    if args.is_empty() {
        return store.alloc_leaf(callee);
    }
    let mut ops = args.to_vec();
    ops.push(std::ptr::null_mut());
    store.alloc_words(callee, &ops)
}

/// The words of a null-terminated run, the terminator left out: a call's arguments as
/// `build_call` laid them, a list operand's tail.
///
/// # Safety
/// `p` must point at a null-terminated run of node words.
pub unsafe fn null_terminated<'a>(p: *const DyadPtr) -> &'a [DyadPtr] {
    let n = (0..).take_while(|&i| !(*p.add(i)).is_null()).count();
    std::slice::from_raw_parts(p, n)
}

impl<'a> Parser<'a> {
    /// The function a held run body is for one field-type set: the cells lexed
    /// at the definition are constructed now, over the type's own scopes and one
    /// unnamed parameter per field. Entered on the type first, so a recursive use resolves to it.
    ///
    /// # Safety
    /// `ty` must carry a record whose run body is `held`; `key` one type per
    /// instance field.
    pub(super) unsafe fn construct_run_body(
        &mut self,
        ty: DyadPtr,
        held: DyadPtr,
        key: &[DyadPtr],
        at: usize,
        spelling: &str,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let text = crate::identities::run_body::text_of(held);
        let cells = (*crate::identities::run_body::cells_of(held)).held_cells();
        let mut chain = Vec::new();
        let mut scope =
            crate::identities::scope::parent_of(crate::identities::meta::record_scope_of(ty));
        while !scope.is_null() {
            chain.push(scope);
            scope = crate::identities::scope::parent_of(scope);
        }
        let mut nested = ScopeStack::new();
        for &scope in chain.iter().rev() {
            nested.push(scope);
        }
        let base_depth = nested.depth();
        let spec = self.rt.store.alloc_words(types.fn_type, &[std::ptr::null_mut(); FN_SLOTS]);
        crate::identities::run_body::insert(self.rt.store, types, held, key, spec);

        let cx = Context {
            feed: Some(Feed { cells, next: 0, base_depth, in_run_order: false }),
            ..Context::nested(&self.cx, text, nested)
        };
        let g = self.enter(cx);
        let inner = g.0.construct_run_body_in(ty, key, spec);
        let inner_pos = g.0.cx.pos;
        drop(g);

        match inner {
            Ok(f) => Ok(f),
            Err(e) => {
                crate::identities::run_body::remove(self.rt.store, types, held, key);
                self.cx.pos = at;
                Err(ParseError::RunBodyFailed {
                    name: spelling.to_string(),
                    rendered: crate::report::render(
                        &format!("run body of `{spelling}`"),
                        text,
                        inner_pos,
                        &crate::report::parse_message(&e),
                    ),
                })
            }
        }
    }

    /// Over the swapped-in state: the hidden parameter record, the return
    /// type (the type the set holds in the field named `output_type`, or `void`),
    /// and the body read as a function's, filling `spec`.
    ///
    /// # Safety
    /// As `construct_run_body`; `spec` the `fn` node minted for the set.
    unsafe fn construct_run_body_in(
        &mut self,
        ty: DyadPtr,
        key: &[DyadPtr],
        spec: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let field_scope = crate::identities::meta::record_scope_of(ty);
        let fields =
            crate::identities::array::items(crate::identities::meta::record_fields_of(ty)).to_vec();
        // The parameters are the fields alone, in order, each under its field's name: a
        // node's operands are its fields, and its call passes exactly those.
        let names: HashMap<DyadPtr, String> = self
            .trie
            .bindings_in(field_scope)
            .into_iter()
            .map(|b| (Binding::read(b).dyad, Binding::spelling(b)))
            .collect();
        let mut params: Vec<(Option<&str>, DyadPtr)> = Vec::with_capacity(fields.len());
        let mut type_fields = Vec::new();
        for (i, &field) in fields.iter().enumerate() {
            let name = names.get(&field).map(String::as_str);
            let ty = if crate::identities::hole::type_in(field) == types.type_ {
                // A field of type `type` names the type this set holds in it, so
                // `output_type 1` reads `i32 1`; its parameter place goes unnamed.
                type_fields.extend(name.map(|n| (n, key[i])));
                params.push((None, types.type_));
                continue;
            } else if key[i] == types.scope || key[i] == types.square_brackets {
                // A bare parameter: the bracket the node's field holds, read by its lines.
                std::ptr::null_mut()
            } else {
                key[i]
            };
            params.push((name, ty));
        }
        let mut scope = ScopeStack::new();
        scope.push(field_scope);
        let output = scope
            .resolve(self.trie, "output_type")
            .ok()
            .and_then(|r| fields.iter().position(|&f| f == r.identity))
            .filter(|&i| crate::identities::hole::type_in(fields[i]) == types.type_)
            .map_or(types.void_, |i| key[i]);
        let (input, places) = self.hidden_param_record(&params, 0)?;
        self.cx.run_fields = Some((ty, places.clone()));
        let param_scope = crate::identities::meta::record_scope_of(input);
        self.cx.scopes.push(param_scope);
        let declared =
            type_fields.iter().try_for_each(|&(n, t)| self.declare_name(n, t, 0).map(|_| ()));
        self.cx.scopes.pop();
        declared?;
        self.fn_over_body(types.fn_type, input, &places, output, spec, std::ptr::null_mut())
    }

    /// `(start, len)` of the text inside the `( … )` at the cursor, consumed
    /// with its brackets and constructed by nothing: an untaken branch dropped unlexed, a
    /// body held to be lexed later.
    pub(super) fn body_text_extent(&mut self) -> Result<(usize, usize), ParseError> {
        self.expect_open()?;
        let start = self.cx.pos;
        let close = self.skip_bracket(self.types.open_, 0)?;
        Ok((start, close - start))
    }

    /// `fn ( params ) -> ret ( body )` (DESIGN ›A function's surface‹): the
    /// node is `[input, output, body, bcode, frame, outer]`. `binding` is the
    /// declaration's, pointed at the node before the body parses, or null.
    ///
    /// # Safety
    /// `binding` must be null or a binding dyad from the store.
    pub unsafe fn parse_fn(
        &mut self,
        fn_type: DyadPtr,
        binding: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        // A `share` function reached through a value takes that value
        // (DESIGN ›The constructor is a field‹): the `fn` takes it first.
        let member = !binding.is_null()
            && self.cx.member_fn_depth == Some(self.cx.frames.len())
            && self.cx.definitions.last().is_some_and(|d| d.this_param.is_null());
        let (input, params) = self.parse_record_taking(member.then_some(self.types.dyad_))?;
        self.expect_arrow()?;
        let output = {
            let items = self.drive_until_open(RightSide::ReturnType);
            let out = self.one_of(items?).map(|c| c.dyad).map_err(|e| match e {
                ParseError::Empty => ParseError::ExpectedReturnType,
                e => e,
            })?;
            // A named return type is a use of that name: its binding, read
            // through to the type it names.
            // SAFETY: `out` is a reduced dyad from the store.
            unsafe { self.types.through(out) }
        };
        if !member {
            // SAFETY: `input` was just built by `parse_record`; `binding` is the caller's.
            return unsafe {
                self.fn_over_body(fn_type, input, &params, output, std::ptr::null_mut(), binding)
            };
        }
        // The value is the first parameter, the one no name declares.
        let this = params[0];
        let def = self.cx.definitions.last_mut().expect("checked above");
        def.this_param = this;
        def.read_receiver = false;
        def.wrote_receiver = false;
        // SAFETY: as above.
        let f = unsafe {
            self.fn_over_body(fn_type, input, &params, output, std::ptr::null_mut(), binding)
        };
        let def = self.cx.definitions.last_mut().expect("still open");
        def.this_param = std::ptr::null_mut();
        let receiver = (u64::from(def.read_receiver) * RECEIVER_READS)
            | (u64::from(def.wrote_receiver) * RECEIVER_WRITES);
        if let (Ok(&f), true) = (f.as_ref(), receiver != 0) {
            let u64_ty = self.types.numtypes[crate::identities::NumType::U64 as usize];
            let leaf = self.rt.store.alloc_blob(u64_ty, &receiver.to_ne_bytes());
            // SAFETY: `f` is the node `fn_over_body` just built over its seven-slot record.
            unsafe { *(dyad::value(f) as *mut DyadPtr).add(FN_RECEIVER) = leaf };
        }
        f
    }

    /// The half of `parse_fn` after the signature, shared with a slot body read bare: the
    /// frame opened on the `fn` node, the parameters laid out in it on their bindings, the
    /// body parsed deferred with the parameter scope reopened. The node is `spec`, a run
    /// body's node minted for its set, or fresh; its signature is on it, and `binding`
    /// points at it, before the body parses, so a recursive self-call resolves its types.
    ///
    /// # Safety
    /// `input` must be a record node and `params` its fields' bindings in order; `spec`
    /// null or a `fn` node minted with `FN_SLOTS` null words that nothing has filled;
    /// `binding` null or a binding dyad from the store.
    pub(super) unsafe fn fn_over_body(
        &mut self,
        fn_type: DyadPtr,
        input: DyadPtr,
        params: &[DyadPtr],
        output: DyadPtr,
        spec: DyadPtr,
        binding: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let node = if spec.is_null() {
            let mut early = [std::ptr::null_mut(); FN_SLOTS];
            early[FN_INPUT] = input;
            early[FN_OUTPUT] = output;
            self.rt.store.alloc_words(fn_type, &early)
        } else {
            // SAFETY: `spec` holds `FN_SLOTS` words (the caller's contract).
            unsafe {
                let slots = dyad::value(spec) as *mut DyadPtr;
                *slots.add(FN_INPUT) = input;
                *slots.add(FN_OUTPUT) = output;
            }
            spec
        };
        if !binding.is_null() {
            // SAFETY: `binding` is a binding dyad from the store (the caller's contract).
            unsafe { Binding::set_dyad(binding, node) };
        }
        // A call frame is an instance of its function, so a parameter resolves
        // to a frame slot as a local does (DESIGN ›Resolution is one rule‹). The
        // barrier begins at the current depth, so a lesser depth is outside the function.
        self.cx.frames.push(OpenFn {
            frame: node,
            size: 0,
            below: self.cx.scopes.depth(),
            outer: Vec::new(),
            open_below: self.cx.open.len(),
            returns: Vec::new(),
        });
        // SAFETY: `input` is the record just built, `params` its bindings.
        unsafe {
            let fields = crate::identities::meta::record_fields_of(input);
            for (&param, &binding) in crate::identities::array::items(fields).iter().zip(params) {
                // Sized by the rule the argument block lays them out by (`by_copy::slots`).
                let logos = crate::identities::hole::type_in(param);
                let width = crate::identities::by_copy::param_width(self.types, logos);
                let frame = self.cx.frames.last_mut().expect("pushed above");
                Binding::lay_out(binding, logos, node, frame.size);
                frame.size += width;
            }
        }

        // The body is deferred (it runs at calls), so parse-time rebinding is
        // off inside.
        // SAFETY: `input` is the record just built; its record stores its scope.
        let scope = unsafe { crate::identities::meta::record_scope_of(input) };
        // Names declared outside the body may not be moved or freed inside;
        // the parameters, in the scope pushed next, may (DESIGN ›A function's surface‹).
        self.cx.scopes.push_barrier();
        self.cx.scopes.push(scope);
        self.cx.runtime_depth += 1;
        let body = self.bracketed_body();
        self.cx.runtime_depth -= 1;
        self.cx.scopes.pop();
        self.cx.scopes.pop_barrier();
        let OpenFn { size: frame_size, outer, returns, .. } =
            self.cx.frames.pop().expect("parse_fn pushed a frame");
        let body = body?;

        // A comptime-rational tail commits to the declared return type here,
        // so `fn () -> i64 (…)` returns i64 rather than the i32 default.
        // SAFETY: `body`/`output` are valid dyads just built.
        let body =
            unsafe { crate::identities::commit_fn_body(self.rt.store, self.types, body, output)? };
        for r in returns {
            let ends: Vec<DyadPtr> =
                r.live.iter().copied().filter(|name| r.ended.contains(name)).collect();
            // SAFETY: `r.node` is a `return` node built in this body, not yet run; `ends` are
            // places the scopes it leaves hold.
            unsafe {
                crate::identities::commit_fn_body(self.rt.store, self.types, r.node, output)?;
                crate::identities::return_mod::set_ends(self.rt.store, self.types, r.node, &ends);
            }
        }

        let frame = if frame_size == 0 {
            std::ptr::null_mut()
        } else {
            let u64_ty = self.types.numtypes[crate::identities::NumType::U64 as usize];
            self.rt.store.alloc_blob(u64_ty, &(frame_size as u64).to_ne_bytes())
        };
        let outer = if outer.is_empty() {
            std::ptr::null_mut()
        } else {
            crate::identities::array::build(self.rt.store, self.types.array_, &outer)
        };
        // SAFETY: `node`'s value is the seven-slot record written above, nothing else's.
        unsafe {
            let slots = dyad::value(node) as *mut DyadPtr;
            *slots.add(FN_BODY) = body;
            *slots.add(FN_FRAME) = frame;
            *slots.add(FN_OUTER) = outer;
        }
        Ok(node)
    }

    /// A `return` just built: inside a function it leaves every scope open
    /// since the body began, so it may not hand out a place one of them holds,
    /// nor an owning value; the body's close commits it to the result type.
    ///
    /// # Safety
    /// `node` must be a `return` node `[value, op]` from the store.
    pub(crate) unsafe fn note_return(&mut self, node: DyadPtr) -> Result<(), ParseError> {
        let Some(frame) = self.cx.frames.last() else {
            return Ok(());
        };
        let types = self.types;
        let value = *(dyad::value(node) as *const DyadPtr);
        let place = types.through(value);
        let left = |p: DyadPtr| self.holder_of(p).is_some_and(|holder| holder >= frame.open_below);
        if left(place) {
            return Err(ParseError::OwningEscape);
        }
        if crate::identities::drop_model::lent_place(types, place).is_some_and(left) {
            return Err(ParseError::AddressOfHeld);
        }
        if crate::identities::drop_model::is_owning_value(types, value) {
            return Err(ParseError::OwnershipAcrossReturn);
        }
        let live = self.cx.open[frame.open_below..]
            .iter()
            .flat_map(|s| &s.exit)
            .filter(|item| item.end.is_none())
            .map(|item| item.what)
            .collect();
        let open = OpenReturn {
            node,
            live,
            ended: Vec::new(),
            level: self.cx.open.len() - 1,
            line_closed: false,
            cursor: self.cx.ended_log.len(),
        };
        self.cx.frames.last_mut().expect("checked above").returns.push(open);
        Ok(())
    }

    /// The returns of the function being parsed from index `from`; none outside one.
    fn returns_from(&mut self, from: usize) -> &mut [OpenReturn] {
        self.cx.frames.last_mut().map(|f| &mut f.returns[from..]).unwrap_or_default()
    }

    fn returns_len(&self) -> usize {
        self.cx.frames.last().map_or(0, |f| f.returns.len())
    }

    /// `if cond then` with an optional `else else`: the node is
    /// `[cond, then, else]`, else null when absent, so an else-less `if` is a
    /// statement. Each branch is a bracket or the next expression, and a bare
    /// `else` binds to the nearest `if`; `if` opens no scope.
    pub fn parse_if(&mut self, if_type: DyadPtr) -> Result<DyadPtr, ParseError> {
        let returns_start = self.returns_len();
        let items = self.drive_until_open(RightSide::Condition)?;
        let cond = self.one_of(items)?.dyad;
        let types = self.types;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(types, cond) } {
            return Err(ParseError::NonBoolCondition);
        }

        // A comptime condition resolves the conditional now, in the one pass:
        // an untaken bracket's tokens are dropped unlexed, so nothing inside
        // is resolved, committed or declared, and branches for other comptime types coexist.
        // SAFETY: `cond` is the reduced dyad just parsed.
        if let Some(truth) = unsafe { bool_literal_value(types, cond) } {
            return self.parse_comptime_if(if_type, cond, truth);
        }

        // DESIGN ›A type is a comptime value, resolved in the pass‹, a checked tape cell.
        // SAFETY: `cond` is the reduced dyad just parsed.
        let check = unsafe { type_check_of(types, cond) };

        // A runtime branch may or may not run, so parse-time rebinding is off
        // inside it.
        self.cx.runtime_depth += 1;
        self.cx.narrow_next = check.filter(|&(_, _, holds)| holds).map(|(key, ty, _)| (key, ty));
        let (if_depth, log_start) = (self.cx.open.len(), self.cx.ended_log.len());
        let returns_then = self.returns_len();
        let then = self.parse_branch();
        let (log_then, returns_else) = (self.cx.ended_log.len(), self.returns_len());
        self.cx.narrow_next = None;
        // `else if …` is sugar: the `if` right after `else` becomes the
        // else-branch directly, so a chain nests right-associatively.
        let branches = then.and_then(|then| {
            let els = if self.consume_else() {
                if self.consume_token(self.types.if_) {
                    self.parse_if(if_type)
                } else {
                    self.cx.narrow_next =
                        check.filter(|&(_, _, holds)| !holds).map(|(key, ty, _)| (key, ty));
                    self.parse_branch()
                }
            } else {
                Ok(std::ptr::null_mut())
            };
            els.map(|els| (then, els))
        });
        self.cx.narrow_next = None;
        self.cx.runtime_depth -= 1;
        let (then, els) = branches?;

        // A `!=` check whose branch raises leaves the cell checked for the rest of the scope.
        if let Some((key, ty, false)) = check {
            // SAFETY: `then` is the reduced dyad just parsed.
            if els.is_null() && unsafe { dyad::ty(types.through(then)) } == types.error.error {
                let scope = self.cx.open.last_mut().expect("the `if` stands in an open scope");
                scope.narrowed.push((key, ty));
            }
        }

        // A held name one arm moved or freed ends on every path: the other arm frees it at
        // its end, an else-less `if` in the arm it gains, and a condition that leaves before
        // either arm runs frees it too (DESIGN ›`move` and `free` are static: the parse marks
        // the name dead‹, rule 2).
        let log_end = self.cx.ended_log.len();
        let then_ends = self.ended_outside(log_then..log_end, if_depth);
        let else_ends = self.ended_outside(log_start..log_then, if_depth);
        let condition_ends = self.ended_outside(log_start..log_end, if_depth);
        // A `return` in the condition or the then arm leaves through the `if`, which frees
        // these on its way; an else arm's `return` stands after what the then arm ended.
        let returns = self.returns_from(returns_start);
        let (in_condition, in_then) = returns.split_at_mut(returns_then - returns_start);
        for r in in_condition {
            r.live.retain(|name| !condition_ends.contains(name));
        }
        for r in &mut in_then[..returns_else - returns_then] {
            r.live.retain(|name| !then_ends.contains(name));
        }
        let node = crate::identities::if_mod::build(self.rt.store, types, cond, then, els);
        // SAFETY: `node` was just built; the ends are places the enclosing scopes hold.
        unsafe {
            crate::identities::if_mod::set_ends(
                self.rt.store,
                types,
                node,
                [&then_ends, &else_ends, &condition_ends],
            )
        };
        Ok(node)
    }

    /// The held names the ended log's `range` ended that a scope outside depth `inside`
    /// holds, in the order they were declared.
    fn ended_outside(&self, range: std::ops::Range<usize>, inside: usize) -> Vec<DyadPtr> {
        let mut names: Vec<(usize, usize, DyadPtr)> = self.cx.ended_log[range]
            .iter()
            .filter(|&&(_, holder)| holder < inside)
            .map(|&(name, holder)| {
                // SAFETY: held places and `name` are dyads from the store.
                let at =
                    self.cx.open[holder].exit.iter().position(|h| unsafe { self.holds(h, name) });
                (holder, at.unwrap_or(usize::MAX), name)
            })
            .collect();
        names.sort_unstable();
        names.into_iter().map(|(_, _, name)| name).collect()
    }

    fn bracketed_body(&mut self) -> Result<DyadPtr, ParseError> {
        self.expect_open()?;
        let body = self.parse_sequence()?;
        self.expect_close()?;
        Ok(body)
    }

    /// A branch of `if`, or a `while` body: a bracket, or the next expression,
    /// which ends before an `else` (DESIGN ›Expressions are self-delimiting‹).
    fn parse_branch(&mut self) -> Result<DyadPtr, ParseError> {
        if self.at_open() {
            return self.bracketed_body();
        }
        // A bare branch is a scope as a bracket is, holding one expression.
        self.skip_whitespace();
        let start = self.cx.pos;
        let was = self.cx.else_ends.replace(self.cx.open.len() + 1);
        let body = self.parse_sequence();
        self.cx.else_ends = was;
        if self.cx.pos == start {
            return Err(ParseError::Empty);
        }
        body
    }

    /// An untaken branch: a bracket is dropped unlexed; a bare one is parsed
    /// and dropped, since only constructing it finds where it ends.
    fn skip_branch(&mut self) -> Result<(), ParseError> {
        if self.at_open() {
            return self.body_text_extent().map(|_| ());
        }
        self.cx.runtime_depth += 1;
        self.cx.dropping += 1;
        let dead = self.parse_branch();
        self.cx.dropping -= 1;
        self.cx.runtime_depth -= 1;
        dead.map(|_| ())
    }

    /// True with an else: the then-branch is the result and the else-tail is
    /// dropped. False: the then-branch is dropped. An else-less `if` stays a
    /// statement node either way, since it yields unit.
    fn parse_comptime_if(
        &mut self,
        if_type: DyadPtr,
        cond: DyadPtr,
        truth: bool,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        if truth {
            let then = self.parse_branch()?;
            if self.consume_else() {
                self.skip_else_tail(if_type)?;
                return Ok(then);
            }
            let none = std::ptr::null_mut();
            return Ok(crate::identities::if_mod::build(self.rt.store, types, cond, then, none));
        }
        self.skip_branch()?;
        if self.consume_else() {
            if self.consume_token(self.types.if_) {
                return self.parse_if(if_type);
            }
            return self.parse_branch();
        }
        let none = std::ptr::null_mut();
        Ok(crate::identities::if_mod::build(self.rt.store, types, cond, cond, none))
    }

    /// Drop the dead tail after a taken `else`: `if ( cond ) then` links
    /// while further `else`s follow, or the final branch. A bare condition's
    /// end is found only by constructing it, so that link is parsed and dropped.
    fn skip_else_tail(&mut self, if_type: DyadPtr) -> Result<(), ParseError> {
        loop {
            if !self.consume_token(self.types.if_) {
                return self.skip_branch();
            }
            if !self.at_open() {
                self.cx.runtime_depth += 1;
                self.cx.dropping += 1;
                let dead = self.parse_if(if_type);
                self.cx.dropping -= 1;
                self.cx.runtime_depth -= 1;
                return dead.map(|_| ());
            }
            self.body_text_extent()?; // ( cond )
            self.skip_branch()?;
            if !self.consume_else() {
                return Ok(());
            }
        }
    }

    /// The node is `[operand, op, bool]`.
    ///
    /// # Safety
    /// `operand` must be a reduced dyad from the store.
    pub(crate) unsafe fn build_not(
        &mut self,
        not_id: DyadPtr,
        operand: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        if !is_bool_result(types, operand) {
            return Err(ParseError::NonBoolOperands);
        }
        // A bool-literal operand folds now (pure, nothing lost): what keeps a
        // comptime chain comptime.
        if let Some(v) = bool_literal_value(types, operand) {
            return Ok(crate::identities::bool_mod::literal_node(
                self.rt.store,
                self.types.bool_,
                !v,
            ));
        }
        Ok(self.rt.store.alloc_words(not_id, &[operand, types.ops.not_, types.bool_]))
    }

    /// `while cond body`: the node is `[cond, body]`, a statement yielding
    /// unit; the condition and body are read as `if`'s are; the body's value is
    /// thrown away (DESIGN ›a loop body's is thrown away‹); a `return` in it
    /// leaves the enclosing function.
    pub fn parse_while(&mut self, while_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The condition runs every pass too, so a name declared outside may not be moved or
        // freed in it either.
        self.cx.scopes.push_barrier();
        let body = self.parse_while_parts();
        self.cx.scopes.pop_barrier();
        let (cond, body) = body?;
        let types = self.types;
        // SAFETY: `body` is the reduced dyad just parsed.
        if self.cx.frames.is_empty() && unsafe { contains_return(types, body) } {
            return Err(ParseError::EarlyReturn);
        }
        Ok(self.rt.store.alloc_words(while_id, &[cond, body, types.ops.while_, types.void_]))
    }

    fn parse_while_parts(&mut self) -> Result<(DyadPtr, DyadPtr), ParseError> {
        let items = self.drive_until_open(RightSide::Condition)?;
        let cond = self.one_of(items)?.dyad;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(self.types, cond) } {
            return Err(ParseError::NonBoolCondition);
        }
        // A repeated body: parse-time rebinding is off inside.
        self.cx.runtime_depth += 1;
        let body = self.parse_branch();
        self.cx.runtime_depth -= 1;
        Ok((cond, body?))
    }

    /// `for i in a..b ( body )`, with an optional `..step` and optionally no
    /// index (DESIGN ›The scope's constructor is the driver‹): end-exclusive;
    /// the counter is a fresh block-local either way, so both tiers run one node shape.
    pub fn parse_for(&mut self, for_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        let name = self.loop_name()?;
        // The range, constructed by parse_rank, the `..` cells inert
        // delimiters read by position.
        let parts: Vec<(DyadPtr, usize)> = self
            .drive_until_open(RightSide::Range)?
            .into_iter()
            .map(|(c, s)| (c.dyad, s))
            .collect();
        let dotdot = self.types.dotdot_;
        let types = self.types;
        // The `..` cells are uses of that identity: its binding, read through.
        // SAFETY: every item `drive_until_open` returned is a dyad from the store.
        let is_dotdot = |d: &DyadPtr| unsafe { types.through(*d) } == dotdot;
        let (start, end, step) = match parts.as_slice() {
            [(s, _), (d, _), (e, _)] if is_dotdot(d) => (*s, *e, None),
            [(s, _), (d, _), (e, _), (d2, _), (st, _)] if is_dotdot(d) && is_dotdot(d2) => {
                (*s, *e, Some(*st))
            }
            _ => return Err(ParseError::ExpectedRange),
        };

        // Concrete types must match, literals commit, all-literals default to i32.
        let types = self.types;
        // SAFETY: `step` is the reduced dyad just parsed.
        let step_was_literal =
            step.is_some_and(|s| unsafe { dyad::ty(types.through(s)) } == types.rational);
        let mut parts = vec![start, end];
        if let Some(s) = step {
            parts.push(s);
        }
        // SAFETY: `parts` are reduced dyads just parsed.
        let logos =
            unsafe { crate::identities::resolve_loop_parts(self.rt.store, types, &mut parts)? };
        let (start, end) = (parts[0], parts[1]);
        let step = parts.get(2).copied().unwrap_or(std::ptr::null_mut());
        if step_was_literal {
            use crate::identities::numtype;
            // SAFETY: `step` is the committed literal just built; `logos` a numtype node.
            let (bits, nt) = unsafe {
                // Storage here would mean the literal check above let a variable through.
                use crate::identities::read::{read_kind, Read};
                if !matches!(read_kind(types, step), Read::Scalar(_)) || types.is_storage(step) {
                    return Err(ParseError::BadStep);
                }
                (
                    numtype::read_scalar(dyad::ty(step), dyad::value(step)),
                    numtype::of_type_node(logos),
                )
            };
            if numtype::apply_compare(numtype::CmpOp::Gt, nt, bits, 0) == 0 {
                return Err(ParseError::BadStep);
            }
        }

        // SAFETY: `logos` is a numtype node from resolve_loop_parts.
        let width = unsafe { crate::identities::numtype::of_type_node(logos) }.bytes();
        let parent = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        let scope = crate::identities::scope::mint(self.rt.store, types.scope, parent);
        // A repeated body: a name declared outside may not be moved or freed
        // inside; the loop variable, declared in the scope pushed next, is inside.
        self.cx.scopes.push_barrier();
        self.cx.scopes.push(scope);
        // Parse-time rebinding is off inside a repeated body, the counter's name included.
        self.cx.runtime_depth += 1;
        // A named counter is a name of the enclosing frame; a nameless one is the run's own.
        let mut var = std::ptr::null_mut();
        let body = match name {
            Some((nstart, nlen)) => {
                let source = self.cx.source;
                self.declare_name(&source[nstart..nstart + nlen], logos, nstart).and_then(
                    |binding| {
                        // SAFETY: `binding` was just declared; `logos` is a type node.
                        var = unsafe { self.place_for(binding, logos, width) };
                        self.bracketed_body()
                    },
                )
            }
            None => self.bracketed_body(),
        };
        self.cx.runtime_depth -= 1;
        self.cx.scopes.pop();
        self.cx.scopes.pop_barrier();
        let body = body?;
        // SAFETY: `body` is the reduced dyad just parsed.
        if self.cx.frames.is_empty() && unsafe { contains_return(types, body) } {
            return Err(ParseError::EarlyReturn);
        }

        let ops = [var, start, end, step, body, types.ops.for_, types.void_];
        Ok(self.rt.store.alloc_words(for_id, &ops))
    }

    /// The cell after `for` decides: a spelling followed by `in` is the name;
    /// a fresh spelling not followed by `in` wanted it, since nothing declared
    /// lexes there; anything else begins the range, and the cursor goes back.
    fn loop_name(&mut self) -> Result<Option<(usize, usize)>, ParseError> {
        let at = self.cx.pos;
        let Some((nstart, nlen, fresh)) = self.lex_spelling_fresh() else {
            return Ok(None);
        };
        if self.consume_token(self.types.in_) {
            return Ok(Some((nstart, nlen)));
        }
        if fresh {
            return Err(ParseError::ExpectedIn);
        }
        self.cx.pos = at;
        Ok(None)
    }

    /// The callee's own constructor's work (DESIGN ›`X (…)` is one spelling,
    /// and X's constructor decides what the bracket is‹): a numeric callee is
    /// a conversion, a record type constructs an instance, a type-returning callee resolves now.
    ///
    /// # Safety
    /// `callee` and every `arg` must be dyads from the store.
    pub(crate) unsafe fn build_call(
        &mut self,
        callee: DyadPtr,
        args: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        let mut args = args.to_vec();
        for i in 0..args.len() {
            // SAFETY: `args` are reduced dyads from the store.
            if let Some((connective, a, b)) = crate::identities::group::members(self.types, args[i])
            {
                args[i] = a;
                let a = self.build_call(callee, &args)?;
                args[i] = b;
                let b = self.build_call(callee, &args)?;
                return crate::identities::group::join(self.rt.store, self.types, connective, a, b);
            }
        }
        // Fail-closed until ownership-gated parameters let the callee declare
        // that it takes the value.
        for &arg in &args {
            // SAFETY: `args` are reduced dyads just parsed.
            if unsafe { crate::identities::drop_model::is_owning_value(self.types, arg) } {
                return Err(ParseError::UnboundOwningValue);
            }
        }
        // SAFETY: `callee` is a resolved dyad from the store; a record type carries the record `run_body_of` reads.
        let (is_numtype, is_record) = unsafe {
            (
                crate::identities::is_numtype_node(self.types, callee),
                crate::identities::meta::is_record_type(callee),
            )
        };
        if is_numtype {
            // SAFETY: `callee` is a numtype node; `args` are reduced dyads.
            unsafe { crate::identities::build_cast(self.rt.store, self.types, callee, &args) }
        } else if is_record {
            // SAFETY: `callee` is a record type node.
            if unsafe { !crate::identities::meta::run_body_of(callee).is_null() } {
                return Err(ParseError::RunTypeApplied);
            }
            // A record type applied to its field values constructs an
            // instance, like `i32(a)`.
            let types = self.types;
            // SAFETY: `callee` is a record type node; `args` are reduced dyads from the store.
            unsafe {
                crate::identities::instance::build_ctor(
                    self.rt.store,
                    types,
                    types.construct_,
                    callee,
                    &args,
                )
            }
        } else {
            // Each uncommitted literal argument commits to its parameter's
            // declared type; a callee that is no `fn` has no signature and commits nothing.
            let types = self.types;
            let mut args = args;
            // SAFETY: `callee` and `args` are reduced dyads from the store.
            unsafe {
                crate::identities::commit_call_args(self.rt.store, types, callee, &mut args)?;
            }
            let call = build_call(self.rt.store, callee, &args);
            // SAFETY: `callee` is a reduced dyad; a `fn` node holds `FN_SLOTS` words.
            let record_out = unsafe {
                if dyad::ty(callee) == types.fn_type {
                    let out = *(dyad::value(callee) as *const DyadPtr).add(FN_OUTPUT);
                    crate::identities::by_copy::record_width(types, out).map(|w| (out, w))
                } else {
                    None
                }
            };
            if let Some((out, _)) = record_out {
                // The record result is copied out where the call runs: into the name that
                // takes it, else scratch.
                return Ok(crate::identities::by_copy::build_result(
                    self.rt.store,
                    types,
                    std::ptr::null_mut(),
                    call,
                    out,
                ));
            }
            // A type-returning call resolves now, at comptime, so the result
            // flows as an ordinary type value; the outer-name check runs first,
            // since a body that reads a dead name must not run.
            // SAFETY: `callee` is a reduced dyad.
            if unsafe { self.returns_type(callee) } {
                self.check_call_reads(callee)?;
                // Inside a body an argument known only at run leaves the call
                // to run with it, its result a type value at run.
                let comptime = args.iter().all(|&a| {
                    // SAFETY: `args` are reduced dyads from the store.
                    unsafe { crate::identities::is_comptime_arg(types, a) }
                });
                if self.cx.runtime_depth > 0 && !comptime {
                    return Ok(call);
                }
                // SAFETY: `call` was just built over reduced dyads.
                unsafe { self.eval_type_call(call) }
            } else {
                Ok(call)
            }
        }
    }

    /// Mint a scope node carrying its enclosing scope (DESIGN ›Meta-navigation‹)
    /// and push it; the link is set before anything inside is parsed, so a
    /// constructor running inside walks up over settled structure.
    pub(super) fn open_scope(&mut self) -> DyadPtr {
        let parent = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        let scope = crate::identities::scope::mint(self.rt.store, self.types.scope, parent);
        self.cx.scopes.push(scope);
        scope
    }

    /// One expression is returned as itself; several become a sequence node
    /// `[expr0 … exprN, null]` yielding the trailing one, itself the scope
    /// its declarations live in (DESIGN ›A scope's value is what it evaluates to‹).
    pub fn parse_sequence(&mut self) -> Result<DyadPtr, ParseError> {
        self.parse_sequence_cell().map(|c| c.dyad)
    }

    /// [`Self::parse_sequence`] as the cell, its facts with it.
    pub(crate) fn parse_sequence_cell(&mut self) -> Result<Cell, ParseError> {
        self.parse_block(false).map(|(_, value)| value)
    }

    /// `open` has one entry per open scope, and `unopened` counts the skipped brackets it
    /// does not hold, so together they are the nesting depth; past the limit the parse is
    /// the checked error, not a Rust stack overflow.
    pub(super) fn check_depth(&self, unopened: usize) -> Result<(), ParseError> {
        if self.cx.open.len() + unopened >= MAX_BRACKET_DEPTH {
            return Err(ParseError::TooDeep);
        }
        Ok(())
    }

    /// [`Parser::parse_sequence`], with the scope the block opened, whose `dyads` hold
    /// every line whatever the value collapsed to.
    pub(super) fn parse_block(&mut self, list: bool) -> Result<(DyadPtr, Cell), ParseError> {
        self.check_depth(0)?;
        let scope = self.open_scope();
        let narrowed = self.cx.narrow_next.take().into_iter().collect();
        self.cx.open.push(OpenScope { narrowed, scope: Some(scope), list, ..OpenScope::default() });
        let array_ = self.types.array_;
        // SAFETY: null is a legal cell dyad.
        let mut last = unsafe { Cell::built(std::ptr::null_mut()) };
        while let Some(item) = self.next_cell() {
            last = match item {
                Ok(cell) => cell,
                Err(e) => {
                    self.cx.scopes.pop();
                    let open = self.cx.open.pop().expect("pushed above");
                    self.end_after_fault(&open);
                    return Err(e);
                }
            };
            // SAFETY: `last` is the line `next_cell` just returned.
            unsafe { self.close_line(last.dyad) };
        }
        self.cx.scopes.pop();
        // SAFETY: `scope` was minted by `open_scope` with a value, so it has an
        // array; the store outlives the slice and nothing pushes to it below.
        let exprs = unsafe {
            crate::identities::scope::dyads(self.rt.store, array_, scope);
            crate::identities::scope::exprs_of(scope).expect("a block scope has dyads")
        };
        // What the block parsed and did not run runs when the block runs;
        // what it did run stands in the body as its result.
        let mut closed = self.cx.open.pop().expect("pushed above");
        self.close_returns(self.cx.open.len());
        // Prose and a `defer` (it runs at exit, never as the tail) are
        // invisible to value flow.
        let defer_ = self.types.defer_;
        // SAFETY: `exprs` are reduced dyads just parsed/built.
        let is_value = |e: DyadPtr| unsafe {
            !crate::identities::numtype::is_comment_type(dyad::ty(e)) && dyad::ty(e) != defer_
        };
        let values = exprs.iter().filter(|&&e| is_value(e)).count();
        if values > 0 {
            // Outside a function a `return` before the tail has nothing to leave.
            let types = self.types;
            let tail = exprs.iter().rposition(|&e| is_value(e)).expect("values >= 1");
            for (i, &e) in exprs.iter().enumerate() {
                if i != tail
                    && self.cx.frames.is_empty()
                    // SAFETY: `e` is a reduced dyad just parsed.
                    && unsafe { contains_return(types, e) }
                {
                    self.end_after_fault(&closed);
                    return Err(ParseError::EarlyReturn);
                }
            }
            // A last value that is a place this scope holds moves out to whoever takes the
            // value, so the scope's end holds it no longer; a `return` of one would hand out
            // freed memory. An enclosing scope's place is an ordinary borrow.
            // SAFETY: `exprs[tail]` is a reduced dyad just parsed.
            let (tail_value, returned) = unsafe {
                let t = exprs[tail];
                // `return x` yields `x`, so the escape rides its operand.
                if dyad::ty(t) == types.return_ {
                    (*(dyad::value(t) as *const DyadPtr), true)
                } else {
                    (t, false)
                }
            };
            // SAFETY: `tail_value` is a reduced dyad from the store.
            let place = unsafe { types.through(tail_value) };
            // SAFETY: held places and `place` are dyads from the store.
            let held_at = |p: DyadPtr| closed.exit.iter().position(|h| unsafe { self.holds(h, p) });
            // SAFETY: as above.
            let lent = unsafe { crate::identities::drop_model::lent_place(types, place) };
            if lent.and_then(held_at).is_some() {
                self.end_after_fault(&closed);
                return Err(ParseError::AddressOfHeld);
            }
            if let Some(at) = held_at(place) {
                if returned {
                    self.end_after_fault(&closed);
                    return Err(ParseError::OwningEscape);
                }
                closed.exit[at].end = Some(tail);
                // A plain record moves out as its bytes' address, which the taker copies.
                // SAFETY: `place` is a place this scope's binding site laid out, and
                // `tail` indexes the scope's own lines, which nothing else reads yet.
                unsafe {
                    let moved = if crate::identities::this::is_plain(types, types.type_of(place)) {
                        crate::identities::by_copy::build_out(self.rt.store, types, place)
                    } else {
                        crate::identities::drop_model::build_move(self.rt.store, types, place)
                    };
                    let (_, lines) = crate::identities::array::parts(
                        crate::identities::scope::exprs_array(scope),
                    );
                    *(lines as *mut DyadPtr).add(tail) = moved;
                }
            }
        }
        // One line, and nothing for the end to run: the line stands as itself.
        let ends_run = closed.exit.iter().any(ExitItem::held_somewhere);
        if values > 0 && exprs.len() == 1 && !ends_run {
            // SAFETY: `exprs` are reduced dyads from the store.
            let expr = unsafe { Cell::built(exprs[0]) };
            return Ok((scope, if last.dyad == exprs[0] { last } else { expr }));
        }
        let exit: Vec<DyadPtr> =
            closed.exit.iter().map(|h| h.build(self.rt.store, self.types)).collect();
        // SAFETY: `scope` was minted by `open_scope` above and is unaliased.
        let cell = unsafe {
            crate::identities::scope::fill(self.types, scope);
            crate::identities::scope::extend_exit(self.rt.store, array_, scope, &exit);
            Cell::built(scope)
        };
        Ok((scope, cell))
    }

    /// A line of the innermost scope is complete: the names it declared or ended settle on
    /// it (see [`ScopeStack::close_item`]), and a `defer` line joins the exit. A held name the
    /// line ended is held up to it; one an enclosing scope holds, this scope holds from its
    /// start up to it, so a `return` or fault before the line frees it (DESIGN ›`move` and
    /// `free` are static: the parse marks the name dead‹), and the pass that runs the line
    /// tells the holder.
    ///
    /// # Safety
    /// `item` must be a line this parser just returned.
    unsafe fn close_line(&mut self, item: DyadPtr) {
        // SAFETY: the pending bindings were minted by this parser's declares.
        unsafe { self.cx.scopes.close_item(self.rt.store, self.types.array_, item) };
        let depth = self.cx.open.len() - 1;
        let line = self.cx.open[depth].lines;
        if let Some(frame) = self.cx.frames.last_mut() {
            for r in frame.returns.iter_mut().filter(|r| r.level == depth && !r.line_closed) {
                r.ended.extend(self.cx.ended_log[r.cursor..].iter().map(|&(name, _)| name));
                r.line_closed = true;
            }
        }
        for name in std::mem::take(&mut self.cx.open[depth].ended) {
            let open = &self.cx.open[depth];
            // SAFETY: held places and `name` are dyads from the store.
            if let Some(at) = open.exit.iter().position(|h| unsafe { self.holds(h, name) }) {
                self.cx.open[depth].exit[at].end = Some(line);
                continue;
            }
            let open = &mut self.cx.open[depth];
            if line > 0 {
                open.exit.insert(0, ExitItem { what: name, from: 0, end: Some(line) });
            }
            if let Some((_, _, ends)) = open.unrun.last_mut().filter(|(_, at, _)| *at == line) {
                ends.push(name);
            }
        }
        // SAFETY: `item` is a reduced dyad from the store.
        if unsafe { dyad::ty(item) } == self.types.defer_ {
            self.cx.open[depth].exit.push(ExitItem { what: item, from: line + 1, end: None });
        }
        self.cx.open[depth].lines += 1;
    }

    /// The block at `depth` closed: a `return` inside it stands in the enclosing scope's line
    /// from here, which may end names after the block.
    fn close_returns(&mut self, depth: usize) {
        let log_len = self.cx.ended_log.len();
        for r in self.returns_from(0).iter_mut().filter(|r| r.level == depth) {
            r.level = depth - 1;
            r.line_closed = false;
            r.cursor = log_len;
        }
    }

    /// A driver's top-level line is complete: see [`Parser::close_line`].
    ///
    /// # Safety
    /// `item` must be a line this parser just returned.
    pub unsafe fn close_item(&mut self, item: DyadPtr) {
        // SAFETY: the caller's contract.
        unsafe { self.close_line(item) }
    }

    /// Run everything parsed and not yet run, outermost scope first, and hand
    /// back the last item run with its value: the tail, on the stack (DESIGN ›The
    /// pass runs only as far as it must, in order, and never twice‹). A block's
    /// cursor moves to its last line, so its own run starts after it. The lists
    /// are taken first, so a nested drain finds nothing. A scope's lines count as run as
    /// they go, prose, `defer` and `[…]` lines included.
    pub fn drain(&mut self) -> Result<Option<(DyadPtr, i64)>, ParseError> {
        let lists: Vec<_> =
            self.cx.open.iter_mut().map(|s| (s.scope, std::mem::take(&mut s.unrun))).collect();
        let mut items = Vec::new();
        for (depth, (scope, list)) in lists.into_iter().enumerate() {
            if list.is_empty() {
                continue;
            }
            if let Some(scope) = scope {
                // SAFETY: `scope` is the block `open_scope` minted; the pending items are the
                // last of its `dyads`, the item being parsed not yet among them.
                let lines =
                    unsafe { crate::identities::scope::exprs_of(scope) }.map_or(0, <[_]>::len);
                self.rt.set_cursor(scope, lines);
            }
            for (node, line, ends) in list {
                // A `[…]` line is a list that goes whole to the call that takes it, which
                // evaluates its lines when it runs (DESIGN ›A bracket goes to the call whole‹).
                // SAFETY: every pending item is a dyad this parser built into its store.
                let runs = unsafe {
                    let ty = dyad::ty(node);
                    !crate::identities::numtype::is_comment_type(ty)
                        && ty != self.types.defer_
                        && ty != self.types.square_brackets
                };
                items.push((node, runs, depth, line, ends));
            }
        }
        let mut last = None;
        let mut left = items.iter().filter(|&&(_, runs, ..)| runs).count();
        for (node, runs, depth, line, ends) in items {
            if runs {
                // The holder frees the names the line ends no more, even when the line
                // faults, as a scope's run counts its ending line (`ExitItem::held_at`).
                for name in ends {
                    let at = self.holder_of(name).unwrap_or(0);
                    self.cx.open[at].reached.push(name);
                }
                // A line's scratch goes with the line; the last item's is the value handed on.
                let mark = self.rt.stack_mark();
                // SAFETY: every pending item is a dyad this parser built into its store, which outlives the pass.
                let bits = unsafe { self.run_on_pass(node) }.map_err(ParseError::Run)?;
                left -= 1;
                if left > 0 {
                    self.rt.stack_release(mark);
                }
                last = Some((node, bits));
            }
            self.cx.open[depth].ran = line + 1;
        }
        Ok(last)
    }

    /// A fault or a failed parse unwinds the parse of an open scope: what the lines the pass
    /// ran hold and defer ends, and the fault stays the error shown (DESIGN ›A checked error is a fault:
    /// the task that hit it is cancelled‹). The pass knows which lines ran, so the scope that
    /// holds a name alone decides: a name held from the scope's start is an enclosing
    /// scope's, and one a nested line the pass ran ended is not held.
    pub(super) fn end_after_fault(&mut self, open: &OpenScope) {
        let types = self.types;
        // SAFETY: held places and reached names are dyads from the store.
        let reached =
            |item: &ExitItem| open.reached.iter().any(|&n| unsafe { self.holds(item, n) });
        let ends: Vec<ExitItem> = open
            .exit
            .iter()
            .rev()
            // SAFETY: as above.
            .filter(|item| unsafe {
                item.from > 0 && item.held_at_fault(types, open.ran) && !reached(item)
            })
            .copied()
            .collect();
        for item in ends {
            // SAFETY: the pass laid the scope's places out in the frame `self.rt` runs.
            if unsafe { item.run(&mut self.rt) }.is_err() {
                return;
            }
        }
    }

    /// `immediate x` (DESIGN ›`immediate x` runs the expression to its right as soon as
    /// it is parsed, and stands as its value‹): the operand is read as a `share` value is,
    /// once at the definition, runs after everything before it, and its value is the cell.
    pub(crate) fn construct_immediate(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let saved_once = self.cx.once_at.replace(self.cx.frames.len());
        let saved_depth = std::mem::replace(&mut self.cx.runtime_depth, 0);
        let operand = self.parse_expression();
        self.cx.once_at = saved_once;
        self.cx.runtime_depth = saved_depth;
        // SAFETY: the operand was just parsed into this parser's store.
        let value = unsafe { self.immediate_value(operand?) }?;
        tape.place(value);
        Ok(Constructed::Placed)
    }

    /// What an `immediate` operand stands as: itself when nothing runs, else what the
    /// graph can hold of its result: a number, a bool, a type or a node.
    ///
    /// # Safety
    /// `node` must be a reduced dyad this parser built into its store.
    unsafe fn immediate_value(&mut self, node: DyadPtr) -> Result<DyadPtr, ParseError> {
        use crate::identities::numtype::NumType;
        use crate::identities::read::{read_kind, Dispatch, Read};
        use crate::identities::{numtype_of, Operand};
        let types = self.types;
        let kind = read_kind(types, node);
        let place = types.is_storage(node);
        if !matches!(kind, Read::Executable(_)) && !place {
            return Ok(node);
        }
        if let Read::Executable(Dispatch::Call(f)) = kind {
            if self.returns_type(f) {
                return self.eval_type_call(node);
            }
        }
        let bool_ = is_bool_result(types, node);
        let nt = match numtype_of(types, node) {
            Operand::Concrete(nt) => nt,
            Operand::Literal => NumType::I32,
            _ if bool_ => NumType::I32,
            _ => return Err(ParseError::ImmediateNotHeld),
        };
        self.drain()?;
        let bits = self.run_on_pass(node).map_err(ParseError::Run)?;
        if bool_ {
            let bool_ty = types.bool_;
            return Ok(crate::identities::bool_mod::literal_node(
                self.rt.store,
                bool_ty,
                bits != 0,
            ));
        }
        Ok(self.scalar_value(nt, bits))
    }

    /// The end of the program: the root scope's own run (DESIGN ›The scope's
    /// constructor is the driver‹), yielding the tail it ran.
    pub fn finish(&mut self) -> Result<Option<(DyadPtr, i64)>, ParseError> {
        self.drain()
    }

    /// The program's end: what the program holds and defers, handed to the top scope,
    /// runs after every top-level line. The file driver calls this at program end; a
    /// nested scope ran its own at its end.
    pub fn exit(&mut self) -> Result<(), ParseError> {
        let reached = self.cx.open[0].lines;
        let top = self.hand_exit();
        // SAFETY: `top` is the top scope, its places in the program frame `self.rt` runs.
        unsafe { crate::identities::scope::run_exit(&mut self.rt, top, reached) }
            .map_err(ParseError::Run)
    }

    /// The program's end after a fault or a failed parse, as a nested scope's
    /// ([`Self::end_after_fault`]).
    pub fn exit_after_fault(&mut self) {
        let top = std::mem::take(&mut self.cx.open[0]);
        self.end_after_fault(&top);
    }

    /// What the program holds and defers so far joins the top scope's exit, where the
    /// REPL keeps a session's across its lines, and a name this parse ended that an earlier
    /// line's item holds is held no longer; returns the top scope.
    pub fn hand_exit(&mut self) -> DyadPtr {
        use crate::identities::scope;
        self.end_earlier_holds();
        let top = self.cx.scopes.current().expect("the top scope is open");
        let items: Vec<DyadPtr> = std::mem::take(&mut self.cx.open[0].exit)
            .iter()
            .map(|h| h.build(self.rt.store, self.types))
            .collect();
        // SAFETY: `top` is a scope node the driver minted; `items` exit items just built.
        unsafe { scope::extend_exit(self.rt.store, self.types.array_, top, &items) };
        top
    }

    /// The names an earlier REPL line holds that this parse ended: the top scope's end frees
    /// them no more. Returns them.
    pub fn end_earlier_holds(&mut self) -> Vec<DyadPtr> {
        use crate::identities::{drop_model, scope};
        let top = self.cx.scopes.current().expect("the top scope is open");
        let ended = std::mem::take(&mut self.cx.open[0].ended_earlier);
        // SAFETY: `top` is a scope node the driver minted, its exit null or exit items.
        unsafe {
            for &node in scope::exit_items(top) {
                let item = drop_model::exit_item_of(node);
                if ended.iter().any(|&name| self.holds(&item, name)) {
                    drop_model::end_exit_item(self.rt.store, self.types, node);
                }
            }
        }
        ended
    }

    /// [`Self::end_earlier_holds`] for a line that ran and failed: only the names whose `free` or
    /// `move` it reached (DESIGN ›`free` and `move` end a name; no drop flag‹).
    pub fn end_failed_earlier_holds(&mut self) -> Vec<DyadPtr> {
        let rt = &self.rt;
        // SAFETY: the ended names are places an earlier line laid out in the program frame.
        self.cx.open[0]
            .ended_earlier
            .retain(|&name| unsafe { crate::identities::drop_model::ended_at_run(rt, name) });
        self.end_earlier_holds()
    }

    /// [`Self::end_earlier_holds`] for a line that never ran as a whole: only the names whose
    /// ending line the pass ran.
    pub fn end_reached_earlier_holds(&mut self) -> Vec<DyadPtr> {
        let top = &mut self.cx.open[0];
        let reached = std::mem::take(&mut top.reached);
        top.ended_earlier.retain(|name| reached.contains(name));
        self.end_earlier_holds()
    }
}
