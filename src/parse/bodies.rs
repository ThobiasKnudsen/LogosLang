// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Bodies: function bodies over a frame, run bodies, blocks and sequences, and the
//! control flow that reads a body; the fn record's slots and the analysis over a body.

use super::*;
use crate::dyad;

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
    pub(super) returns: Vec<DyadPtr>,
}

#[derive(Default)]
pub(super) struct OpenScope {
    /// The `defer free <place>` nodes the scope's owning bindings inserted,
    /// drained into the body after each statement.
    pub(super) defers: Vec<DyadPtr>,
    /// The places those teardowns free, once drained: no tail or `return`
    /// leaving the scope may hand one out.
    owned: Vec<DyadPtr>,
    /// Items parsed at depth 0 and not yet run: `drain` runs them when the
    /// pass needs a value, the scope's own run otherwise.
    pub(super) unrun: Vec<DyadPtr>,
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
/// first-read order, or null; read at every call (DESIGN ›`own` and `drop`
/// are static‹).
pub const FN_OUTER: usize = 5;

/// A `u64` leaf of [`RECEIVER_READS`] and [`RECEIVER_WRITES`] bits: whether a
/// `share` function's body reads or writes a field of its value; null for neither.
pub const FN_RECEIVER: usize = 6;

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

/// Empty for a function that reads none, and for a declaration's placeholder
/// whose value is still being parsed.
///
/// # Safety
/// `fn_node` must be a function node: its value null, or the operands
/// `parse_fn` builds (the early signature included).
pub unsafe fn fn_outer<'a>(fn_node: DyadPtr) -> &'a [DyadPtr] {
    let fields = dyad::value(fn_node) as *const DyadPtr;
    if fields.is_null() {
        return &[];
    }
    let outer = *fields.add(FN_OUTER);
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
    if f.is_null() || dyad::ty(f) != types.fn_type || dyad::value(f).is_null() {
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
    if !f.is_null() && dyad::ty(f) == types.fn_type && !dyad::value(f).is_null() {
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
    if dyad::ty(node) != types.bool_ || dyad::value(node).is_null() {
        return None;
    }
    Some(std::ptr::read_unaligned(dyad::value(node) as *const i32) != 0)
}

/// A cell or line of the tape, by the tape's place, a literal index and the line.
pub(crate) type CellKey = (DyadPtr, i32, crate::identities::tape::Line);

/// `tape[k]:type == T` or `!=` with `T` a number type or `type`, either side first, or
/// `tape[k]:type ⊆ T` with `T` a number type, each under any number of `not`: the cell
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
        if dyad::value(node).is_null() {
            return false;
        }
        let arr = *(dyad::value(node) as *const DyadPtr);
        return crate::identities::array::items(arr).iter().any(|&e| contains_return(types, e));
    }
    false
}

/// `{type: callee, value: [args…, null]}`: null-terminated so `run` can count
/// the arguments; a nullary call carries a null value.
pub(super) fn build_call(store: &mut Store, callee: DyadPtr, args: &[DyadPtr]) -> DyadPtr {
    let value = if args.is_empty() {
        std::ptr::null_mut()
    } else {
        let mut ops = args.to_vec();
        ops.push(std::ptr::null_mut());
        store.alloc_operands(&ops)
    };
    store.alloc_raw(callee, value)
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
        let mut cells = (*crate::identities::run_body::cells_of(held)).cells();
        // A fresh cell's dyad is the placeholder a `:=` in the body fills, so
        // each construction gets its own and the held cells stay as lexed.
        for cell in &mut cells {
            if cell.is_fresh() {
                cell.dyad = self.rt.store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
            }
        }
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
        let spec = self.rt.store.alloc_raw(types.fn_type, std::ptr::null_mut());
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
            let ty = if dyad::ty(field) == types.type_ {
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
            .filter(|&i| dyad::ty(fields[i]) == types.type_)
            .map_or(types.void_, |i| key[i]);
        let (input, places) = self.hidden_param_record(&params, 0)?;
        self.cx.run_fields = Some((ty, places.clone()));
        let param_scope = crate::identities::meta::record_scope_of(input);
        self.cx.scopes.push(param_scope);
        let declared =
            type_fields.iter().try_for_each(|&(n, t)| self.declare_name(n, t, 0).map(|_| ()));
        self.cx.scopes.pop();
        declared?;
        self.fn_over_body(types.fn_type, input, &places, output, spec)
    }

    /// `(start, len)` of the text inside the `( … )` at the cursor, consumed
    /// with its brackets and constructed by nothing; the closer is found by
    /// lexing token by token, so a bracket inside a quote or a `#` comment is text.
    pub(super) fn body_text_extent(&mut self) -> Result<(usize, usize), ParseError> {
        self.expect_open()?;
        let source = self.cx.source;
        let bytes = source.as_bytes();
        let start = self.cx.pos;
        let mut depth = 0usize;
        loop {
            self.skip_whitespace();
            if self.cx.pos >= bytes.len() {
                return Err(ParseError::UnclosedBracket);
            }
            if bytes[self.cx.pos] == b'#' {
                self.cx.pos += 1;
                while self.cx.pos < bytes.len() && matches!(bytes[self.cx.pos], b' ' | b'\t') {
                    self.cx.pos += 1;
                }
                // The line form ends at the newline; the string form, `# «…»`,
                // is the quote token the lexer reads next.
                if !source[self.cx.pos..].starts_with('«') {
                    while self.cx.pos < bytes.len() && bytes[self.cx.pos] != b'\n' {
                        self.cx.pos += 1;
                    }
                    continue;
                }
            }
            let r = self
                .cx
                .scopes
                .lex(self.trie, &source[self.cx.pos..])
                .map_err(ParseError::Resolve)?;
            let id = if r.fresh { std::ptr::null_mut() } else { r.identity };
            if id == self.types.open_ || id == self.types.open_sq_ {
                depth += 1;
            } else if id == self.types.close_ || id == self.types.close_sq_ {
                if depth == 0 {
                    let len = self.cx.pos - start;
                    self.cx.pos += r.matched;
                    return Ok((start, len));
                }
                depth -= 1;
            }
            self.cx.pos += r.matched;
        }
    }

    /// `fn ( params ) -> ret ( body )` (DESIGN ›A function's surface‹): the
    /// node is `[input, output, body, bcode, frame, outer]`. `declared` is the
    /// declaration's placeholder the signature publishes onto before the body parses.
    ///
    /// # Safety
    /// `declared` must be null or a placeholder dyad from the store that
    /// nothing has read a value from yet; the early signature is written through it.
    pub unsafe fn parse_fn(
        &mut self,
        fn_type: DyadPtr,
        declared: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        // A `share` function reached through a value takes that value
        // (DESIGN ›The constructor is a field‹): the `fn` takes it first.
        let member = !declared.is_null()
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
            // SAFETY: `input` was just built by `parse_record`; `declared` is the caller's placeholder.
            return unsafe { self.fn_over_body(fn_type, input, &params, output, declared) };
        }
        // The value is the first parameter, the one no name declares.
        let this = params[0];
        let def = self.cx.definitions.last_mut().expect("checked above");
        def.this_param = this;
        def.read_receiver = false;
        def.wrote_receiver = false;
        // SAFETY: as above.
        let f = unsafe { self.fn_over_body(fn_type, input, &params, output, declared) };
        let def = self.cx.definitions.last_mut().expect("still open");
        def.this_param = std::ptr::null_mut();
        let receiver = (u64::from(def.read_receiver) * RECEIVER_READS)
            | (u64::from(def.wrote_receiver) * RECEIVER_WRITES);
        if let (Ok(&f), true) = (f.as_ref(), receiver != 0) {
            let bytes = self.rt.store.alloc_bytes(&receiver.to_ne_bytes());
            let u64_ty = self.types.numtypes[crate::identities::NumType::U64 as usize];
            let leaf = self.rt.store.alloc_raw(u64_ty, bytes);
            // SAFETY: `f` is the node `fn_over_body` just built over its seven-slot record.
            unsafe { *(dyad::value(f) as *mut DyadPtr).add(FN_RECEIVER) = leaf };
        }
        f
    }

    /// The half of `parse_fn` after the signature, shared with a slot body read bare: the
    /// frame opened on the `fn` node, the parameters laid out in it on their bindings, the
    /// body parsed deferred with the parameter scope reopened. The node is `declared`, the
    /// declaration's placeholder or a run body's `spec`, or a fresh cell; its signature is
    /// on it before the body parses, so a recursive self-call resolves its types.
    ///
    /// # Safety
    /// `input` must be a record node and `params` its fields' bindings in order; `declared`
    /// null or a `fn`-typed dyad from the store that nothing has read a value from yet.
    pub(super) unsafe fn fn_over_body(
        &mut self,
        fn_type: DyadPtr,
        input: DyadPtr,
        params: &[DyadPtr],
        output: DyadPtr,
        declared: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let node = if declared.is_null() {
            self.rt.store.alloc_raw(fn_type, std::ptr::null_mut())
        } else {
            declared
        };
        let early = self.rt.store.alloc_operands(&[
            input,
            output,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ]);
        // SAFETY: `node` is the placeholder or a fresh cell; nothing has read a value from it.
        unsafe { dyad::set_value(node, early) };
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
                let logos = dyad::ty(param);
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
        // Names declared outside the body may not be moved or dropped inside;
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
            // SAFETY: `r` is a `return` node built in this body.
            unsafe { crate::identities::commit_fn_body(self.rt.store, self.types, r, output)? };
        }

        let frame = if frame_size == 0 {
            std::ptr::null_mut()
        } else {
            let bytes = self.rt.store.alloc_bytes(&(frame_size as u64).to_ne_bytes());
            let u64_ty = self.types.numtypes[crate::identities::NumType::U64 as usize];
            self.rt.store.alloc_raw(u64_ty, bytes)
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
    /// since the body began, so it may not hand out a place their teardowns
    /// free, nor an owning value; the body's close commits it to the result type.
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
        if self.cx.open[frame.open_below..].iter().any(|s| s.owned.contains(&place)) {
            return Err(ParseError::OwningEscape);
        }
        if crate::identities::drop_model::is_owning_value(types, value) {
            return Err(ParseError::OwnershipAcrossReturn);
        }
        self.cx.frames.last_mut().expect("checked above").returns.push(node);
        Ok(())
    }

    /// `if cond then` with an optional `else else`: the node is
    /// `[cond, then, else]`, else null when absent, so an else-less `if` is a
    /// statement. Each branch is a bracket or the next expression, and a bare
    /// `else` binds to the nearest `if`; `if` opens no scope.
    pub fn parse_if(&mut self, if_type: DyadPtr) -> Result<DyadPtr, ParseError> {
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
        let then = self.parse_branch();
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

        let value = self.rt.store.alloc_operands(&[cond, then, els, self.types.ops.if_]);
        Ok(self.rt.store.alloc_raw(if_type, value))
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
            return self.skip_group();
        }
        self.cx.runtime_depth += 1;
        let dead = self.parse_branch();
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
        if truth {
            let then = self.parse_branch()?;
            if self.consume_else() {
                self.skip_else_tail(if_type)?;
                return Ok(then);
            }
            let value = self.rt.store.alloc_operands(&[
                cond,
                then,
                std::ptr::null_mut(),
                self.types.ops.if_,
            ]);
            return Ok(self.rt.store.alloc_raw(if_type, value));
        }
        self.skip_branch()?;
        if self.consume_else() {
            if self.consume_token(self.types.if_) {
                return self.parse_if(if_type);
            }
            return self.parse_branch();
        }
        let value =
            self.rt.store.alloc_operands(&[cond, cond, std::ptr::null_mut(), self.types.ops.if_]);
        Ok(self.rt.store.alloc_raw(if_type, value))
    }

    /// Drop a balanced `( … )` group without parsing it (DESIGN ›a constructor
    /// may splice tokens in or drop upcoming ones before they lex‹): `«…»` text
    /// and `#` prose are skipped opaquely, their parentheses being text.
    fn skip_group(&mut self) -> Result<(), ParseError> {
        /// `pos` must point at `«`; `None` if unterminated.
        fn skip_text(bytes: &[u8], mut pos: usize) -> Option<usize> {
            pos += 2; // the «
            while pos + 1 < bytes.len() {
                if bytes[pos] == 0xC2 && bytes[pos + 1] == 0xBB {
                    return Some(pos + 2);
                }
                pos += 1;
            }
            None
        }
        self.expect_open()?;
        let bytes = self.cx.source.as_bytes();
        let mut depth = 1usize;
        while self.cx.pos < bytes.len() {
            match bytes[self.cx.pos] {
                b'(' => {
                    depth += 1;
                    self.cx.pos += 1;
                }
                b')' => {
                    depth -= 1;
                    self.cx.pos += 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                0xC2 if bytes.get(self.cx.pos + 1) == Some(&0xAB) => {
                    self.cx.pos =
                        skip_text(bytes, self.cx.pos).ok_or(ParseError::UnclosedBracket)?;
                }
                b'#' => {
                    // `#` takes a following «…» string or the rest of the line,
                    // as the comment constructor reads it.
                    self.cx.pos += 1;
                    while self.cx.pos < bytes.len() && matches!(bytes[self.cx.pos], b' ' | b'\t') {
                        self.cx.pos += 1;
                    }
                    if bytes.get(self.cx.pos) == Some(&0xC2)
                        && bytes.get(self.cx.pos + 1) == Some(&0xAB)
                    {
                        self.cx.pos =
                            skip_text(bytes, self.cx.pos).ok_or(ParseError::UnclosedBracket)?;
                    } else {
                        while self.cx.pos < bytes.len() && bytes[self.cx.pos] != b'\n' {
                            self.cx.pos += 1;
                        }
                    }
                }
                _ => self.cx.pos += 1,
            }
        }
        Err(ParseError::UnclosedBracket)
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
                let dead = self.parse_if(if_type);
                self.cx.runtime_depth -= 1;
                return dead.map(|_| ());
            }
            self.skip_group()?; // ( cond )
            self.skip_branch()?;
            if !self.consume_else() {
                return Ok(());
            }
        }
    }

    /// The node is `{type: not, value: operand}`.
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
        let value = self.rt.store.alloc_operands(&[operand, self.types.ops.not_]);
        Ok(self.rt.store.alloc_raw(not_id, value))
    }

    /// `while cond body`: the node is `[cond, body]`, a statement yielding
    /// unit; the condition and body are read as `if`'s are; the body's value is
    /// thrown away (DESIGN ›a loop body's is thrown away‹); a `return` in it
    /// leaves the enclosing function.
    pub fn parse_while(&mut self, while_id: DyadPtr) -> Result<DyadPtr, ParseError> {
        let items = self.drive_until_open(RightSide::Condition)?;
        let cond = self.one_of(items)?.dyad;
        let types = self.types;
        // SAFETY: `cond` is the reduced dyad just parsed.
        if !unsafe { is_bool_result(types, cond) } {
            return Err(ParseError::NonBoolCondition);
        }
        // A repeated body: parse-time rebinding is off inside, and a name
        // declared outside may not be moved or dropped inside.
        self.cx.runtime_depth += 1;
        self.cx.scopes.push_barrier();
        let body = self.parse_branch();
        self.cx.scopes.pop_barrier();
        self.cx.runtime_depth -= 1;
        let body = body?;
        // SAFETY: `body` is the reduced dyad just parsed.
        if self.cx.frames.is_empty() && unsafe { contains_return(types, body) } {
            return Err(ParseError::EarlyReturn);
        }
        let value = self.rt.store.alloc_operands(&[cond, body, self.types.ops.while_]);
        Ok(self.rt.store.alloc_raw(while_id, value))
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
        // A repeated body: a name declared outside may not be moved or dropped
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

        let value =
            self.rt.store.alloc_operands(&[var, start, end, step, body, self.types.ops.for_]);
        Ok(self.rt.store.alloc_raw(for_id, value))
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
            // declared type; an unbound callee has no signature yet and commits nothing.
            let types = self.types;
            let mut args = args;
            // SAFETY: `callee` and `args` are reduced dyads from the store.
            unsafe {
                crate::identities::commit_call_args(self.rt.store, types, callee, &mut args)?;
            }
            let call = build_call(self.rt.store, callee, &args);
            // SAFETY: `callee` is a reduced dyad; a `fn` callee's value is its field record or null.
            let record_out = unsafe {
                if dyad::ty(callee) == types.fn_type && !dyad::value(callee).is_null() {
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

    /// [`Parser::parse_sequence`], with the scope the block opened, whose `dyads` hold
    /// every line whatever the value collapsed to.
    pub(super) fn parse_block(&mut self, list: bool) -> Result<(DyadPtr, Cell), ParseError> {
        // `open` has one entry per open scope, so its length is the nesting
        // depth; past the limit the parse is the checked error, not a Rust stack overflow.
        if self.cx.open.len() >= MAX_BRACKET_DEPTH {
            return Err(ParseError::TooDeep);
        }
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
                    self.cx.open.pop();
                    return Err(e);
                }
            };
            let item = last.dyad;
            // SAFETY: the pending bindings were minted by this parser's declares;
            // `scope` is the innermost open scope, minted by `open_scope` above.
            unsafe { self.cx.scopes.close_item(self.rt.store, array_, item) };
            // A binding's `defer free <place>` is drained right after its
            // statement, so the defer sits at its source position, the right
            // LIFO rank among other statements' defers.
            let depth = self.cx.open.len() - 1;
            if !self.cx.open[depth].defers.is_empty() {
                let drained = std::mem::take(&mut self.cx.open[depth].defers);
                for &d in &drained {
                    // SAFETY: `d` is a `defer free <place>` node the binding site
                    // just built; `scope` a scope `open_scope` minted.
                    unsafe {
                        let place = crate::identities::drop_model::teardown_place_of(d);
                        self.cx.open[depth].owned.push(place);
                        crate::identities::scope::push_item(self.rt.store, array_, scope, d);
                    }
                }
            }
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
        let owned_here = self.cx.open.pop().expect("pushed above").owned;
        // Prose and a `defer` (it runs at exit, never as the tail) are
        // invisible to value flow.
        let defer_ = self.types.defer_;
        // SAFETY: `exprs` are reduced dyads just parsed/built.
        let is_value = |e: DyadPtr| unsafe {
            !crate::identities::numtype::is_comment_type(dyad::ty(e)) && dyad::ty(e) != defer_
        };
        let values = exprs.iter().filter(|&&e| is_value(e)).count();
        match (values, exprs.len()) {
            // An empty `( )`, or a bracket holding only prose: the scope node
            // with nothing to run, yielding unit.
            (0, _) => {
                // SAFETY: `scope` was minted by `open_scope` above and is unaliased.
                let cell = unsafe {
                    crate::identities::scope::fill(scope, self.types.ops.scope_);
                    Cell::built(scope)
                };
                Ok((scope, cell))
            }
            (_, 1) => {
                // SAFETY: `exprs` are reduced dyads from the store.
                let expr = unsafe { Cell::built(exprs[0]) };
                Ok((scope, if last.dyad == exprs[0] { last } else { expr }))
            }
            _ => {
                // Outside a function a `return` before the tail has nothing to leave.
                let types = self.types;
                let tail = exprs.iter().rposition(|&e| is_value(e)).expect("values >= 1");
                for (i, &e) in exprs.iter().enumerate() {
                    if i != tail
                        && self.cx.frames.is_empty()
                        // SAFETY: `e` is a reduced dyad just parsed.
                        && unsafe { contains_return(types, e) }
                    {
                        return Err(ParseError::EarlyReturn);
                    }
                }
                // A last value that is a place this scope owns moves out to
                // whoever takes the value, so the teardown on the way out finds
                // it empty; a `return` of one would hand out freed memory. Only
                // places this scope frees count; an enclosing scope's owning place is an ordinary borrow.
                // SAFETY: `exprs[tail]` is a reduced dyad just parsed.
                let (tail_value, returned) = unsafe {
                    let t = exprs[tail];
                    // `return x` yields `x`, so the escape rides its operand.
                    if dyad::ty(t) == types.return_ && !dyad::value(t).is_null() {
                        (*(dyad::value(t) as *const DyadPtr), true)
                    } else {
                        (t, false)
                    }
                };
                // SAFETY: `tail_value` is a reduced dyad from the store.
                let place = unsafe { types.through(tail_value) };
                if owned_here.contains(&place) {
                    if returned {
                        return Err(ParseError::OwningEscape);
                    }
                    // SAFETY: `place` is a place this scope's binding site minted, and
                    // `tail` indexes the scope's own lines, which nothing else reads yet.
                    unsafe {
                        let moved = if crate::identities::drop_model::is_owning_place(types, place)
                        {
                            crate::identities::drop_model::build_teardown(
                                self.rt.store,
                                types,
                                types.own_,
                                place,
                                true,
                            )?
                        } else {
                            crate::identities::drop_model::build_instance_own(
                                self.rt.store,
                                types,
                                place,
                                types.type_of(place),
                            )
                        };
                        let (_, lines) = crate::identities::array::parts(
                            crate::identities::scope::exprs_array(scope),
                        );
                        *(lines as *mut DyadPtr).add(tail) = moved;
                    }
                }
                // SAFETY: `scope` was minted by `open_scope` above and is unaliased.
                let cell = unsafe {
                    crate::identities::scope::fill(scope, self.types.ops.scope_);
                    Cell::built(scope)
                };
                Ok((scope, cell))
            }
        }
    }

    /// A driver's top-level line is complete: see [`ScopeStack::close_item`].
    ///
    /// # Safety
    /// `item` must be a line this parser just returned.
    pub unsafe fn close_item(&mut self, item: DyadPtr) {
        // SAFETY: the pending bindings were minted by this parser's declares.
        unsafe { self.cx.scopes.close_item(self.rt.store, self.types.array_, item) }
    }

    /// The top level's `defer free` nodes, which no `parse_sequence` drained
    /// (the top level is no block); insertion order, the caller reverses.
    pub fn take_pending_defers(&mut self) -> Vec<DyadPtr> {
        match self.cx.open.first_mut() {
            Some(base) => std::mem::take(&mut base.defers),
            None => Vec::new(),
        }
    }

    /// Run everything parsed and not yet run, outermost scope first, and hand
    /// back the last item run with its value: the tail, on the stack (DESIGN ›The
    /// pass runs only as far as it must, in order, and never twice‹). A block's
    /// cursor moves to its last line, so its own run starts after it. The lists
    /// are taken first, so a nested drain finds nothing.
    pub fn drain(&mut self) -> Result<Option<(DyadPtr, i64)>, ParseError> {
        let lists: Vec<(Option<DyadPtr>, Vec<DyadPtr>)> =
            self.cx.open.iter_mut().map(|s| (s.scope, std::mem::take(&mut s.unrun))).collect();
        let mut items = Vec::new();
        for (scope, list) in lists {
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
            for node in list {
                // A `[…]` line is a list that goes whole to the call that takes it, which
                // evaluates its lines when it runs (DESIGN ›A bracket goes to the call whole‹).
                // SAFETY: every pending item is a dyad this parser built into its store.
                let skip = unsafe {
                    let ty = dyad::ty(node);
                    crate::identities::numtype::is_comment_type(ty)
                        || ty == self.types.defer_
                        || ty == self.types.square_brackets
                };
                if skip {
                    continue;
                }
                items.push(node);
            }
        }
        let mut last = None;
        let n = items.len();
        for (i, node) in items.into_iter().enumerate() {
            // A line's scratch goes with the line; the last item's is the value handed on.
            let mark = self.rt.stack_mark();
            // SAFETY: every pending item is a dyad this parser built into its store, which outlives the pass.
            let bits = unsafe { self.run_on_pass(node) }.map_err(ParseError::Run)?;
            if i + 1 < n {
                self.rt.stack_release(mark);
            }
            last = Some((node, bits));
        }
        Ok(last)
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

    /// The root scope's exit: the top level's teardowns, LIFO. The file
    /// driver calls this at program end; a nested scope ran its own at exit.
    pub fn exit(&mut self) -> Result<(), ParseError> {
        for defer_node in self.take_pending_defers().into_iter().rev() {
            // SAFETY: `defer_node` is a `defer` node in the store, which outlives the pass.
            unsafe { crate::identities::run_deferred(&mut self.rt, defer_node) }
                .map_err(ParseError::Run)?;
        }
        Ok(())
    }
}
