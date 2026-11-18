// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Member reads: `.` over a value, a node, `tape[0]` or a type, `[`, `@`, `&`, and the
//! `:` reads of the lex level.

use super::*;
use crate::dyad;

/// The member right of `.`: the name's span in the source, and the `[i]` or `(…)` cell
/// some reads take after it.
#[derive(Clone)]
struct Member {
    start: usize,
    len: usize,
    index: Option<usize>,
    key: Option<DyadPtr>,
    call: Option<Vec<DyadPtr>>,
}

/// Whether what `p` gives is read whole, a scalar, a pointer, or bytes a record or a run-time
/// rational keeps: an address of it can be handed out.
///
/// # Safety
/// `p` must be a reduced dyad from the store.
unsafe fn has_address(types: &crate::Core, p: DyadPtr) -> bool {
    use crate::identities::read::{output_type, place_kind, Read};
    matches!(
        place_kind(types, output_type(types, p)),
        Read::Scalar(_) | Read::Pointer(_) | Read::Aggregate | Read::Rational
    )
}

impl<'a> Parser<'a> {
    /// The cell before is an unbuilt `.` or `:`, whose right side is a member's spelling.
    pub(super) fn after_tight_read(&self, tape: &ParsingTape) -> bool {
        tape.last().is_some_and(|l| self.is_unbuilt_tight_read(l))
    }

    pub(super) fn is_unbuilt_tight_read(&self, cell: &Cell) -> bool {
        !cell.constructed && {
            let id = self.cell_identity(cell);
            id == self.types.dot_ || id == self.types.colon_
        }
    }

    /// Inside a `free` or a `share` function a field of the value is named bare (DESIGN
    /// ›There is no `this`‹); fields come first, so an outer name never stands in for one.
    /// Inside a parse the same name is the checked error, the field going through `tape[0]`.
    pub(super) fn names_own_field(&self, cell: &Cell) -> bool {
        let Some(def) = self.cx.definitions.last() else {
            return false;
        };
        if def.this_param.is_null() {
            return false;
        }
        let mut fields = ScopeStack::new();
        fields.push(def.block.0);
        fields
            .resolve(self.trie, cell.spelling())
            .is_ok_and(|r| r.matched == cell.len && def.block.1.contains(&r.identity))
    }

    /// `tape[0]` inside a parse body: the cell the parse was woken on, holding its node.
    fn is_node_cell(&self, lhs: DyadPtr) -> bool {
        let Some(def) = self.cx.definitions.last().filter(|d| d.in_parse) else {
            return false;
        };
        // SAFETY: `lhs` is a reduced dyad from the store; a slot node carries `[tape, k]`.
        unsafe {
            crate::identities::tape::is_cell_read(self.types, lhs)
                && crate::identities::tape::cell_key(self.types, lhs).is_some_and(|(t, k, l)| {
                    t == def.tape_param && k == 0 && l == crate::identities::tape::Line::Cell
                })
        }
    }

    /// `tape[0].type = T`: the fresh node placed in the parse's own cell, from where its
    /// fields are written by name (DESIGN ›There is no `this`‹). The seed stamps only the
    /// type being defined.
    ///
    /// # Safety
    /// `target` must be a `tape.cell_type` node from the store; `value` a reduced dyad.
    pub(crate) unsafe fn stamp(
        &mut self,
        target: DyadPtr,
        value: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let ops = dyad::value(target) as *const DyadPtr;
        let cell = crate::identities::tape::build_slot(self.rt.store, types, *ops, *ops.add(1));
        if !self.is_node_cell(cell) || !(*ops.add(2)).is_null() {
            return Err(ParseError::StampOutsideParse);
        }
        let def = self.cx.definitions.last().expect("is_node_cell found a parse body");
        if self.types.through(value) != def.self_type {
            return Err(ParseError::StampOtherType);
        }
        let node = def.this_param;
        Ok(crate::identities::tape::build_write(self.rt.store, types, cell, node))
    }

    /// A bare call of a `share` function that works on a value takes the value the calling
    /// body is about (DESIGN ›There is no `this`‹): a `run`'s node, a `free`'s or `share`
    /// function's own, or in a parse none, so only one that reads no field of its value is
    /// called bare there.
    ///
    /// # Safety
    /// `callee` must be a reduced dyad from the store.
    pub(super) unsafe fn with_receiver(
        &mut self,
        callee: DyadPtr,
        args: Vec<DyadPtr>,
    ) -> Result<Vec<DyadPtr>, ParseError> {
        let f = self.types.through(callee);
        if !self.takes_this(f) {
            return Ok(args);
        }
        let name = || {
            let spelled = if dyad::ty(callee) == self.types.binding_ {
                Binding::spelling(callee)
            } else {
                String::new()
            };
            Box::new(spelled)
        };
        let bits = fn_receiver(self.types, f);
        let writes = bits & RECEIVER_WRITES != 0;
        let receiver = match self.cx.definitions.last_mut().filter(|d| !d.this_param.is_null()) {
            Some(def) => {
                if def.in_parse && bits & RECEIVER_READS != 0 {
                    return Err(ParseError::ShareFnNeedsValue(name()));
                }
                def.wrote_receiver |= writes && !def.in_parse;
                def.this_param
            }
            None => match &self.cx.run_fields {
                Some((ty, places)) => {
                    crate::identities::this::build_pack(self.rt.store, self.types, *ty, places)
                }
                None => return Err(ParseError::ShareFnNeedsValue(name())),
            },
        };
        let mut with = vec![receiver];
        with.extend(args);
        Ok(with)
    }

    /// The slot of the node being built that the field `f` names: `f`
    /// resolves in the fields' own scope alone, and its index among the fields
    /// declared above is the slot.
    pub(super) fn this_field(&mut self, name: &str, at: usize) -> Result<DyadPtr, ParseError> {
        let def = self.cx.definitions.last().expect("this_param is set inside a definition");
        let (this, body) = (def.this_param, def.scope);
        let (scope, items) = (def.block.0, def.block.1.clone());
        let mut field_scope = ScopeStack::new();
        field_scope.push(scope);
        let resolved = field_scope.resolve(self.trie, name).ok();
        let field = resolved.as_ref().map(|r| r.identity);
        let index = field.and_then(|f| items.iter().position(|&x| x == f));
        let Some(index) = index else {
            // A `share` member is one place read through every node, so
            // `tape[0].element_type` is the member itself, known here.
            let mut members = ScopeStack::new();
            members.push(body);
            if let Ok(r) = members.resolve(self.trie, name) {
                // A type with a `?` field keeps its `output_type` on each of its mints, one
                // place per mint, which its parse writes (›A type without a `run` whose field
                // is `?` mints one type per set of field types, as `array` mints one per
                // element type‹ [[R300]]).
                let in_parse = self.cx.definitions.last().is_some_and(|d| d.in_parse);
                // SAFETY: the block's items are field declaration dyads from the store.
                let mints = unsafe { crate::identities::mint::has_untyped_field(&items) };
                if in_parse && mints && name == crate::identities::mint::SHARED_FIELD {
                    let types = self.types;
                    return Ok(crate::identities::this::build_mint_output(
                        self.rt.store,
                        types,
                        this,
                    ));
                }
                return Ok(r.identity);
            }
            self.cx.pos = at;
            return Err(ParseError::NoSuchField(Box::new(name.to_string())));
        };
        if let Some(def) = self.cx.definitions.last_mut() {
            def.read_receiver |= !def.in_parse;
        }
        let k = self.scalar_value(crate::identities::numtype::NumType::U64, index as i64);
        let binding = resolved.as_ref().map_or(std::ptr::null_mut(), |r| r.binding);
        // Only the type's own parse fills a field through its default entry.
        let in_parse = self.cx.definitions.last().is_some_and(|d| d.in_parse);
        let fill = if in_parse { self.types.this.fill } else { std::ptr::null_mut() };
        let owner = self.cx.definitions.last().expect("read inside a definition").self_type;
        // SAFETY: the field is a declaration dyad, its type null or a type node.
        let node = unsafe {
            crate::identities::this::build_field_read(
                self.rt.store,
                self.types,
                this,
                k,
                crate::identities::hole::type_in(items[index]),
                binding,
                fill,
                owner,
            )
        };
        // A `?` field a check narrowed reads as a number of the type it was checked against, as
        // a tape cell does (›A tape read checked against a number type reads as that number‹
        // [[R229]]).
        // SAFETY: `node` is the field read just built.
        let narrowed = unsafe { crate::identities::this::field_key(self.types, node) }
            .and_then(|key| self.narrowed_to(key))
            // SAFETY: a narrowed type is a type node.
            .filter(|&ty| unsafe { crate::identities::numtype::is_scalar_type(ty) });
        Ok(match narrowed {
            Some(ty) => crate::identities::this::build_load(
                self.rt.store,
                self.types,
                this,
                k,
                ty,
                binding,
                fill,
                owner,
            ),
            _ => node,
        })
    }

    /// A field of a node reached by path, itself reached (DESIGN ›The dyad's
    /// read surface‹: a node's fields get you more nodes): a scope's `dyads`
    /// and `dyads[k]`, an array's `size`, an operator node's operand by its
    /// role. `None` leaves the read to the rules for any `@dyad` value.
    ///
    /// # Safety
    /// `node` must be a node from the store.
    unsafe fn reached_member(
        &mut self,
        node: DyadPtr,
        name: &str,
        index: Option<usize>,
        bracket: bool,
    ) -> Result<Option<(DyadPtr, usize)>, ParseError> {
        use crate::identities::{array, numtype::NumType, scope};
        let types = self.types;
        let ty = dyad::ty(node);
        if ty == types.scope && name == "dyads" {
            let arr = scope::dyads(self.rt.store, types.array_, node);
            if arr.is_null() {
                return Err(ParseError::BadReflectRead);
            }
            return match index {
                Some(k) => {
                    let item = *array::items(arr).get(k).ok_or(ParseError::BadReflectRead)?;
                    Ok(Some((self.path_operand(item), 1)))
                }
                // An index known only at run: every read here folds at parse.
                None if bracket => Err(ParseError::BadReflectRead),
                None => Ok(Some((self.reach(arr), 0))),
            };
        }
        if ty == types.array_ && name == "size" {
            let size = array::items(node).len() as i64;
            return Ok(Some((self.scalar_value(NumType::U64, size), 0)));
        }
        Ok(self.node_field(node, name)?.map(|n| (n, 0)))
    }

    /// Field `name` of a node, as [`Parser::path_operand`] yields it: an
    /// operator node's operand by its role, a run type's field in declaration
    /// order. `None` when the node's type defines no such field.
    ///
    /// # Safety
    /// `node` must be a node from the store.
    unsafe fn node_field(
        &mut self,
        node: DyadPtr,
        name: &str,
    ) -> Result<Option<DyadPtr>, ParseError> {
        use crate::identities::{array, meta};
        let ty = dyad::ty(node);
        let value = dyad::value(node);
        if self.types.is_storage(node) {
            return Ok(None);
        }
        let slot = match meta::kind_of(ty) {
            Some(meta::TUPLE_TAG | meta::LIST_TAG) => (0..meta::arity_of(ty))
                .find(|&i| crate::reflect::text_of(meta::role_of(ty, i)) == name.as_bytes()),
            // The fields are the first words of every node a Logos-written `parse` built.
            Some(meta::RECORD_TAG)
                if !meta::run_body_of(ty).is_null() || self.is_logos_ctor(ty) =>
            {
                let mut fields = ScopeStack::new();
                fields.push(meta::record_scope_of(ty));
                fields.resolve(self.trie, name).ok().and_then(|r| {
                    array::items(meta::record_fields_of(ty)).iter().position(|&f| f == r.identity)
                })
            }
            _ => None,
        };
        let Some(i) = slot else {
            return Ok(
                crate::identities::mint::shared_field(ty, name).map(|f| self.path_operand(f))
            );
        };
        let field = *(value as *const DyadPtr).add(i);
        if field.is_null() {
            return Err(ParseError::BadReflectRead);
        }
        Ok(Some(self.path_operand(field)))
    }

    /// Whether a read of `t`'s field `name` moves a whole value of its declared type
    /// ([`crate::identities::this::reads_whole`]).
    ///
    /// # Safety
    /// `t` must be a record type from the store.
    unsafe fn reads_whole(&self, t: DyadPtr, name: &str) -> bool {
        use crate::identities::{array, meta};
        let mut fields = ScopeStack::new();
        fields.push(meta::record_scope_of(t));
        fields.resolve(self.trie, name).ok().is_some_and(|r| {
            array::items(meta::record_fields_of(t)).contains(&r.identity)
                && crate::identities::this::reads_whole(
                    self.types,
                    crate::identities::hole::type_in(r.identity),
                )
        })
    }

    /// `lhs.f` where `lhs` yields a node of `t` only when the program runs, a place of the
    /// type or a call returning it: the field read through the node's address when it runs,
    /// or a `share` member, a type body's `fn` called with `lhs` as its value.
    ///
    /// # Safety
    /// `lhs` must be a reduced dyad from the store and `t` a type `is_node_valued` accepts.
    unsafe fn run_node_member(
        &mut self,
        lhs: DyadPtr,
        left: Cell,
        t: DyadPtr,
        name: &str,
        call: Option<Vec<DyadPtr>>,
    ) -> Result<Option<(DyadPtr, usize)>, ParseError> {
        use crate::identities::{array, meta, this};
        let types = self.types;
        let mut fields = ScopeStack::new();
        fields.push(meta::record_scope_of(t));
        let items = array::items(meta::record_fields_of(t));
        let found = fields
            .resolve(self.trie, name)
            .ok()
            .and_then(|r| Some((items.iter().position(|&f| f == r.identity)?, r.binding)));
        if let Some((i, binding)) = found {
            let k = self.scalar_value(crate::identities::numtype::NumType::U64, i as i64);
            let ty = crate::identities::hole::type_in(items[i]);
            let node = this::build_field_read(
                self.rt.store,
                types,
                lhs,
                k,
                ty,
                binding,
                std::ptr::null_mut(),
                t,
            );
            return Ok(Some((node, 0)));
        }
        let Some(member) = self.share_member_read(t, name) else {
            return Ok(None);
        };
        if let Some(args) = call {
            if self.takes_this(member) {
                self.check_receiver_write(&left, member)?;
                let mut with_this = vec![lhs];
                with_this.extend(args);
                // The call's result is no member: the path was the member's own.
                self.cx.member_path.clear();
                return self.build_call(member, &with_this).map(|n| Some((n, 1)));
            }
        }
        Ok(Some((member, 0)))
    }

    /// A node reached by path, as a value: itself when reading it runs
    /// nothing (a name, a literal, a place), else its address, so reading a
    /// path never runs code.
    ///
    /// # Safety
    /// `node` must be a node from the store.
    unsafe fn path_operand(&mut self, node: DyadPtr) -> DyadPtr {
        use crate::identities::read::{read_kind, Read};
        let types = self.types;
        let runs = dyad::ty(node) != types.binding_
            && (dyad::ty(node) == types.scope
                || matches!(read_kind(types, node), Read::Executable(_)));
        if runs {
            self.reach(node)
        } else {
            node
        }
    }

    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn field_access_each(
        &mut self,
        lhs: DyadPtr,
        left: Cell,
        member: Member,
    ) -> Result<(DyadPtr, usize), ParseError> {
        let Some((connective, a, b)) = crate::identities::group::members(self.types, lhs) else {
            return self.field_access(lhs, left, member);
        };
        // A member reads as `.`'s left operand does, through its name.
        let (a, b) =
            (self.settled_type(self.types.through(a)), self.settled_type(self.types.through(b)));
        let (a, consumed) = self.field_access_each(a, Cell::built(a), member.clone())?;
        let (b, _) = self.field_access_each(b, Cell::built(b), member)?;
        let joined = crate::identities::group::join(self.rt.store, self.types, connective, a, b)?;
        Ok((joined, consumed))
    }

    /// `lhs.name` to a place: a numeric node over the instance's storage at
    /// the field's byte offset (DESIGN ›Resolution is one rule‹). The field
    /// name resolves in the record type's own scope alone.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn field_access(
        &mut self,
        lhs: DyadPtr,
        left: Cell,
        member: Member,
    ) -> Result<(DyadPtr, usize), ParseError> {
        let Member { start: nstart, len: nlen, index, key, call } = member;
        let unit_call = matches!(call, Some(ref a) if a.is_empty());
        // The member name is read as its raw spelling: a field is dot-only,
        // so what the driver resolved it to is beside the point. `index` and
        // `unit_call` are the reads that take one more cell; the count consumed is returned.
        {
            // `source` is `&'a str` (Copy), independent of the `&mut self` the
            // member reads then need.
            let source = self.cx.source;
            let name = &source[nstart..nstart + nlen];
            if name == "type" {
                return self.type_read(lhs).map(|t| (t, 0));
            }
            // `tape[0].f` inside a parse body: a field of the node the parse builds or reads
            // (DESIGN ›There is no `this`‹); `.dyads` is the cell's own.
            if name != "dyads" && self.is_node_cell(lhs) {
                let node = self.cx.definitions.last().expect("a parse body is open").this_param;
                let member = self.this_field(name, nstart)?;
                // `tape[0].f(…)`, a `share` function: called on that node.
                if let Some(args) = call {
                    if self.takes_this(member) {
                        let mut with_node = vec![node];
                        with_node.extend(args);
                        return self.build_call(member, &with_node).map(|n| (n, 1));
                    }
                }
                return Ok((member, 0));
            }
            // A scope cell's lines, read when the constructor runs.
            {
                use crate::identities::tape;
                let (types, store) = (self.types, &mut *self.rt.store);
                if dyad::ty(lhs) == types.tape.slot && name == "dyads" {
                    let Some(i) = key else {
                        return Ok((tape::build_cell_dyads(store, types, lhs), 0));
                    };
                    let line = tape::build_cell_dyad_at(store, types, lhs, i);
                    return Ok((self.narrowed_read(line), 1));
                }
                if dyad::ty(lhs) == types.tape.cell_dyads && name == "size" {
                    return Ok((tape::build_cell_dyads_size(store, types, lhs), 0));
                }
                // A bare parameter holding a bracket a call was handed: its lines, read as a
                // tape cell's are, when the body runs.
                let bare = self.types.storage_type(lhs) == Some(std::ptr::null_mut());
                if bare && name == "dyads" {
                    let Some(i) = key else {
                        return Ok((tape::build_bracket_dyads(store, types, lhs), 0));
                    };
                    let line = tape::build_bracket_line(store, types, lhs, i);
                    return Ok((self.narrowed_read(line), 1));
                }
            }
            // `here.scope`, `caller.scope` (DESIGN ›Meta-navigation‹): `here`
            // knows its scope at the appearance, so the read folds; `caller`
            // reads the pass, so its `.scope` is a node run inside a constructor.
            let h = self.types.here;
            if dyad::ty(lhs) == h.here || dyad::ty(lhs) == h.caller {
                if name != "scope" {
                    return Err(ParseError::BadReflectRead);
                }
                let node = if dyad::ty(lhs) == h.here {
                    self.reach(crate::identities::here::scope_of_here(lhs))
                } else {
                    crate::identities::here::build_caller_scope(self.rt.store, self.types)
                };
                return Ok((node, 0));
            }
            // An operator expression is read through its own type alone, never through what it
            // gives.
            if self.is_operator_node(lhs) {
                if let Some(field) = self.node_field(lhs, name)? {
                    return Ok((field, 0));
                }
                let ty = dyad::ty(lhs);
                return self
                    .share_member_access(lhs, &left, ty, name, call)?
                    .ok_or(ParseError::BadReflectRead);
            }
            // A field whose declared type gives a value is read when the code runs, even through a
            // node the parse knows: the node it holds may be replaced before then, and gives that
            // value only when it runs (›A field is filled at run, per evaluation; its type decides
            // how the operand is used‹ [[R193]]).
            if let Some(t) = crate::identities::node_output(self.types, lhs) {
                if self.reads_whole(t, name) {
                    if let Some(read) = self.run_node_member(lhs, left, t, name, None)? {
                        return Ok(read);
                    }
                }
            }
            if let Some(node) = self.known_node(lhs, self.cx.member_root) {
                if let Some(read) = self.reached_member(node, name, index, key.is_some())? {
                    return Ok(read);
                }
            }
            if crate::identities::read::read_kind(self.types, lhs)
                != crate::identities::read::Read::Node
            {
                if let Some(t) = crate::identities::node_output(self.types, lhs) {
                    if let Some(read) = self.run_node_member(lhs, left, t, name, call.clone())? {
                        return Ok(read);
                    }
                }
            }
            if dyad::ty(lhs) == self.types.dyad_ {
                return self.view_member(lhs, name).map(|n| (n, 0));
            }
            // A scope's link up is `.back`; `.scope` is the scope of a spot, a call or a name.
            if name == "back" && crate::identities::here::yields_scope_address(self.types, lhs) {
                let node = crate::identities::here::build_back(self.rt.store, self.types, lhs);
                return Ok((node, 0));
            }
            if crate::identities::is_type_value(self.types, lhs) {
                let n = self.logos_member(lhs, name, index)?;
                return Ok((n, usize::from(name == "roles")));
            }
            // A place holding a type: its fields are the identity's, which
            // nobody knows until the program runs (DESIGN ›A type is a comptime value‹).
            let out = crate::identities::read::output_type(self.types, lhs);
            if out == self.types.type_
                || out == self.types.dyad_ && self.types.is_storage(self.types.through(lhs))
            {
                return Err(ParseError::TypeKnownOnlyAtRun);
            }
            // A node that is the value has its type's fields; a call stands for what it returns,
            // so its node's own fields are never read (DESIGN ›Reading a path runs nothing‹).
            let runs = matches!(
                crate::identities::read::read_kind(self.types, lhs),
                crate::identities::read::Read::Executable(_)
            );
            if !runs {
                if let Some(field) = self.node_field(lhs, name)? {
                    return Ok((field, 0));
                }
            }
            // `.compile` is the fn type's shared member (DESIGN ›Execution is
            // function application‹); the name-compare stands in for resolution
            // through the type's scope. The leaf is minted now, entry zero; the run patches it in.
            if &self.cx.source[nstart..nstart + nlen] == "compile"
                && dyad::ty(lhs) == self.types.fn_type
            {
                if !unit_call {
                    return Err(ParseError::ExpectedOpen);
                }
                let code = crate::identities::callable::mint(
                    self.rt.store,
                    self.types.callable_,
                    0,
                    self.types.conv_container,
                );
                let types = self.types;
                let ops = [lhs, code, types.ops.compile_];
                return Ok((self.rt.store.alloc_words(types.compile_, &ops), 1));
            }
        }
        // A tape's natives, members of `parsing_tape`'s scope that are not
        // laid-out fields, are built as a call with the receiver's address first.
        if let Some(recv) = crate::identities::tape::receiver_addr(self.rt.store, self.types, lhs) {
            let name = &self.cx.source[nstart..nstart + nlen];
            if let Some((op, leaf)) = crate::identities::tape::member(&self.types.tape, name) {
                let types = self.types;
                if op == types.tape.is_constructed || op == types.tape.spelling {
                    let k = key.ok_or(ParseError::ExpectedIndexBracket)?;
                    let node = crate::identities::tape::build_member(
                        self.rt.store,
                        types,
                        recv,
                        op,
                        leaf,
                        &[k],
                    )?;
                    return Ok((node, 1));
                }
                let args = call.ok_or(ParseError::ExpectedOpen)?;
                if op == types.tape.insert || op == types.tape.remove || op == types.tape.recenter {
                    self.forget_narrowed();
                }
                let node = crate::identities::tape::build_member(
                    self.rt.store,
                    types,
                    recv,
                    op,
                    leaf,
                    &args,
                )?;
                return Ok((node, 1));
            }
        }
        // Through a record pointer, `p@.x` folds the field offset into the deref.
        if dyad::ty(lhs) == self.types.deref_ {
            let (ptr_expr, pointee, base_off) = crate::identities::pointer::deref_parts(lhs);
            if pointee.is_null() || !crate::identities::meta::is_record_type(pointee) {
                return Err(ParseError::UnsupportedOperands);
            }
            let (field, offset, _) = match self.resolve_field(pointee, nstart, nlen) {
                Ok(found) => found,
                Err(e) => {
                    let name = &self.cx.source[nstart..nstart + nlen];
                    return self.share_member_read(pointee, name).map(|n| (n, 0)).ok_or(e);
                }
            };
            let types = self.types;
            return Ok((
                crate::identities::pointer::build_deref(
                    self.rt.store,
                    types,
                    ptr_expr,
                    crate::identities::hole::type_in(field),
                    base_off as usize + offset,
                ),
                0,
            ));
        }
        // A `share` member of the record's type, or a field: the field's offset inside the
        // record's storage, read through the record's binding and the field's.
        let storage = self.types.is_storage(lhs);
        let record_logos = self.types.type_of(lhs);
        if record_logos.is_null()
            || !crate::identities::meta::is_record_type(record_logos)
            || (!storage && self.types.is_hole(lhs))
        {
            return Err(ParseError::UnsupportedOperands);
        }
        let (_, _, binding) = match self.resolve_field(record_logos, nstart, nlen) {
            Ok(found) => found,
            Err(e) => {
                let name = &self.cx.source[nstart..nstart + nlen];
                return self.share_member_access(lhs, &left, record_logos, name, call)?.ok_or(e);
            }
        };
        // A field is bytes inside storage; a node's fields are read above, through its slots.
        if !storage {
            return Err(ParseError::UnsupportedOperands);
        }
        let node =
            crate::identities::instance::build_field(self.rt.store, self.types, lhs, binding);
        // Every binding along the path grants a write into the place: the
        // name the path starts at, then each field.
        let mut path = self.path_of(&left);
        if path.is_empty() && !self.cx.member_root.is_null() {
            path.push(self.cx.member_root);
        }
        path.push(binding);
        self.cx.member_path = path;
        Ok((node, 0))
    }

    /// `.`'s constructor: the member read of `tape[-1]` named by the cell to
    /// the right, plus the `[i]` or `()` cell some reads take (see
    /// [`Parser::field_access`]); every consumed cell is spliced out.
    pub(crate) fn construct_field_access(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // The left is read as it stands: an identity's fields are read off the
        // token before its own constructor wakes (DESIGN ›Text is the quote‹),
        // so a callable to the left is the identity itself, not a call in waiting.
        let Some(left) = tape.at(-1).copied() else {
            return Err(ParseError::MissingOperand);
        };
        let (lhs, root) = (self.operand_dyad(left)?, left.binding(self.types));
        // The member is read by its spelling at the offset it was lexed at, so a
        // keyword there (`.type`) reads as a name.
        let this_read = self.is_node_cell(lhs);
        self.cx.member_asleep = this_read;
        let m = self.cell_at(tape, 1);
        self.cx.member_asleep = false;
        let Some(m) = m? else {
            return Err(ParseError::ExpectedField);
        };
        let mstart = m.start;
        let save = self.cx.pos;
        self.cx.pos = mstart;
        let member = self.lex_spelling();
        self.cx.pos = save;
        let Some((nstart, nlen)) = member else {
            return Err(ParseError::ExpectedField);
        };
        // The optional `[i]` or `(…)` after the member, lexed on demand; a
        // bracket is the member's argument list.
        self.read_arguments(tape, 2)?;
        let index = self.index_at(tape, 2);
        let bracket = tape.at(2).filter(|c| c.is_bracket()).map(|c| c.dyad);
        // SAFETY: a bracket cell is a node from the store.
        let call = bracket.map(|d| unsafe { self.args_of(d) });
        let key = self.index_node_at(tape, 2);
        let plain = index.is_none() && key.is_none() && bracket.is_none();
        let next_is =
            |id: DyadPtr| tape.at(2).is_some_and(|c| !c.constructed && self.cell_identity(c) == id);
        let (writes, dotted) =
            (plain && next_is(self.types.assign), plain && next_is(self.types.dot_));
        let source = self.cx.source;
        // A name its value's parse woke stands constructed and still holds its binding.
        let woke = tape.at(-1).filter(|c| c.constructed && !c.dyad.is_null()).map(|c| c.dyad);
        // SAFETY: a constructed cell's dyad is a dyad from the store.
        let named = woke.filter(|&d| unsafe { dyad::ty(d) } == self.types.binding_).unwrap_or(root);
        let path_root = if named.is_null() { left.root } else { named };
        // SAFETY: `named` and the left cell's root are null or binding dyads; `lhs` a reduced
        // dyad off the tape.
        let reached = match unsafe {
            let member = &source[nstart..nstart + nlen];
            self.check_name_field(named, left.root, lhs, member, writes, dotted)
        } {
            Ok(reached) => reached,
            Err(e) => {
                self.cx.pos = nstart;
                return Err(e);
            }
        };
        self.cx.member_root = root;
        self.cx.member_path.clear();
        let member = Member { start: nstart, len: nlen, index, key, call };
        // SAFETY: `lhs` is a reduced dyad off the tape.
        let access = unsafe { self.field_access_each(lhs, left, member) };
        self.cx.member_root = std::ptr::null_mut();
        let path = std::mem::take(&mut self.cx.member_path);
        let (node, consumed) = access?;
        for _ in 0..(1 + consumed) {
            tape.remove(1);
        }
        tape.remove(-1);
        // A type identity a parse's `tape[0].f` folded to stands as its own
        // unconstructed cell, so it reads its right side as anywhere else.
        // SAFETY: `node` is a node from the store.
        let folded_type = this_read
            && unsafe {
                dyad::ty(node) == self.types.type_
                    && crate::identities::meta::kind_of(node).is_some()
            };
        if folded_type {
            tape.set_dyad(0, node);
        } else {
            let path = if path.is_empty() {
                std::ptr::null_mut()
            } else {
                crate::identities::array::build(self.rt.store, self.types.array_, &path)
            };
            tape.place(node);
            let cell = tape.at_mut(0).expect("placed above");
            cell.path = path;
            if plain {
                cell.target = reached;
                cell.root = path_root;
            }
        }
        Ok(Constructed::Placed)
    }

    /// The comptime index a `[…]` cell carries when its interior is a
    /// non-negative literal: what the reflection reads fold at parse.
    fn index_at(&self, tape: &ParsingTape, offset: isize) -> Option<usize> {
        let key = self.index_node_at(tape, offset)?;
        // SAFETY: the interior is a node from the store.
        unsafe {
            if dyad::ty(key) != self.types.rational {
                return None;
            }
            let i = crate::identities::rational::mold(key)?;
            if i < 0 {
                None
            } else {
                Some(i as usize)
            }
        }
    }

    /// The one value a `[…]` cell's `dyads` hold, prose aside: any expression, for the
    /// reads whose natives run it.
    fn index_node_at(&self, tape: &ParsingTape, offset: isize) -> Option<DyadPtr> {
        let c = tape.at(offset)?;
        if !c.constructed {
            return None;
        }
        let d = c.dyad;
        // SAFETY: a dyad cell is a node from the store; a `square_brackets` node's value
        // holds its `dyads` first, as a scope's does.
        unsafe {
            if dyad::ty(d) != self.types.square_brackets {
                return None;
            }
            let defer_ = self.types.defer_;
            let mut values = crate::identities::scope::exprs_of(d)?.iter().copied().filter(|&e| {
                !crate::identities::numtype::is_comment_type(dyad::ty(e)) && dyad::ty(e) != defer_
            });
            let key = values.next()?;
            values.next().is_none().then_some(key)
        }
    }

    /// `[` is `(` in square brackets: the interior parsed as any bracket's,
    /// closed by its `]`. It lands a `square_brackets` cell (DESIGN ›The
    /// scope's constructor is the driver‹), or right after a tape value the element read
    /// `t[k]`, folded here since a tape value has no constructor of its own yet.
    pub(crate) fn construct_index(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let (scope, key) = self.parse_block(true, LinesEnd::Closer)?;
        let key = key.dyad;
        self.take_closer(self.types.open_sq_)?;
        // An index right after a tape value is the element read, a slot node
        // the identity to its left owns; after a `.` it stays a passive cell.
        if let Some(left) = tape.at(-1).copied() {
            // A fresh spelling to the left is a member name after `.`, never a tape.
            if !left.is_fresh(self.types) && self.is_operand_cell(&left) {
                let lhs = self.operand_dyad(left)?;
                let types = self.types;
                // SAFETY: `lhs` is a reduced dyad from the store.
                let recv =
                    unsafe { crate::identities::tape::receiver_addr(self.rt.store, types, lhs) };
                if let Some(recv) = recv {
                    let slot = crate::identities::tape::build_slot(self.rt.store, types, recv, key);
                    // SAFETY: `slot` is the slot node just built.
                    let node = unsafe { self.narrowed_read(slot) };
                    tape.remove(-1);
                    tape.place(node);
                    return Ok(Constructed::Placed);
                }
                // SAFETY: `lhs` and `key` are reduced dyads from the store.
                let get = unsafe {
                    crate::identities::hashmap::build_get(self.rt.store, types, lhs, key)
                }?;
                if let Some(node) = get {
                    tape.remove(-1);
                    tape.place(node);
                    return Ok(Constructed::Placed);
                }
            }
        }
        // SAFETY: `scope` is the block `parse_block` minted, closed.
        let dyads =
            unsafe { crate::identities::scope::dyads(self.rt.store, self.types.array_, scope) };
        // A list standing as a value gives back itself.
        let sq = self.types.square_brackets;
        let node = self.rt.store.alloc_words(sq, &[dyads, std::ptr::null_mut(), sq]);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// Against `record_logos`'s own scope alone: an enclosing binding of the
    /// same spelling can never shadow or double a field.
    ///
    /// # Safety
    /// `record_logos` must be a record type node from the store.
    unsafe fn resolve_field(
        &mut self,
        record_logos: DyadPtr,
        nstart: usize,
        nlen: usize,
    ) -> Result<(DyadPtr, usize, DyadPtr), ParseError> {
        let source = self.cx.source;
        let name = &source[nstart..nstart + nlen];
        let mut field_scope = ScopeStack::new();
        field_scope.push(crate::identities::meta::record_scope_of(record_logos));
        let resolved = field_scope.resolve(self.trie, name).map_err(ParseError::Resolve)?;
        let field = resolved.identity;
        let fields = crate::identities::array::items(crate::identities::meta::record_fields_of(
            record_logos,
        ));
        if !fields.contains(&field) {
            return Err(ParseError::ExpectedField);
        }
        Ok((field, Binding::read(resolved.binding).offset, resolved.binding))
    }

    /// A function whose declared return type is `type`: it yields a type,
    /// resolved at comptime.
    ///
    /// # Safety
    /// `callee` must be a resolved dyad from the store.
    pub(super) unsafe fn returns_type(&self, callee: DyadPtr) -> bool {
        if callee.is_null() || dyad::ty(callee) != self.types.fn_type {
            return false;
        }
        *(dyad::value(callee) as *const DyadPtr).add(FN_OUTPUT) == self.types.type_
    }

    /// The call runs on the pass and the result bits are the produced type
    /// node's address (DESIGN ›Build and run are one self-directing pass‹); a
    /// run failure or a non-type result is `NonComptimeTypeCall`.
    ///
    /// # Safety
    /// `call` must be a reduced call node from the store.
    pub(super) unsafe fn eval_type_call(&mut self, call: DyadPtr) -> Result<DyadPtr, ParseError> {
        // What stands before the call runs first, so the call reads committed state.
        self.drain()?;
        let bits = self.run_on_pass(call).map_err(|e| match e {
            crate::run::RunError::MintFailed(_) => ParseError::Run(e),
            _ => ParseError::NonComptimeTypeCall,
        })?;
        // A call declared `-> type` yields a node's address (DESIGN ›The store is keyed by address‹).
        let node = bits as usize as DyadPtr;
        if !node.is_null() && crate::identities::is_type_value(self.types, node) {
            Ok(node)
        } else {
            Err(ParseError::NonComptimeTypeCall)
        }
    }

    /// The lhs must give back a pointer: a pointer variable or `&x`, a pointer field place,
    /// a pointer step, a call, or another deref whose pointee is a pointer.
    ///
    /// # Safety
    /// `lhs` must be a reduced dyad from the store.
    pub(crate) unsafe fn build_deref(&mut self, lhs: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The pointer expression is stored as it stands (a use of a name is its binding).
        let Some((crate::identities::read::Read::Pointer(pointee), _)) =
            crate::identities::read::place_layout(
                self.types,
                crate::identities::read::output_type(self.types, lhs),
            )
        else {
            return Err(ParseError::UnsupportedOperands);
        };
        // An `@void` says nothing of how its bytes are read (›`undefined`, `?`, `@void` and
        // `native`‹ [[R135]]).
        if pointee == self.types.void_ {
            return Err(ParseError::UnsupportedOperands);
        }
        let types = self.types;
        Ok(crate::identities::pointer::build_deref(self.rt.store, types, lhs, pointee, 0))
    }

    /// `&`'s constructor: an `addr` node that resolves the place's address at
    /// run/lower time, so a frame-relative local yields a per-activation
    /// address. Of the places, a frame's and a dereference's cell have an address
    /// to hand out; a value nothing names is held first, in a place of its own.
    pub(crate) fn construct_address_of(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let Some(&cell) = tape.at(1) else {
            return Err(ParseError::BadAddressOf);
        };
        // A value not built yet, a call or a literal, is waited for, as `free` waits for it.
        if !cell.constructed && self.ctor_of(self.cell_identity(&cell)).is_some() {
            return Err(ParseError::MissingOperand);
        }
        let operand = self.as_operand(cell)?;
        // SAFETY: `operand` is a dyad from the store.
        let (named, node) = unsafe {
            (
                dyad::ty(operand) == self.types.binding_,
                self.settled_type(self.types.through(operand)),
            )
        };
        // SAFETY: `node` is a resolved dyad from the store.
        let addr = unsafe {
            use crate::identities::read::place_node;
            let types = self.types;
            if types.is_hole(node) {
                return Err(ParseError::BadAddressOf);
            }
            match place_node(self.rt.store, types, node) {
                Some(place) if types.is_storage(place) || dyad::ty(place) == types.deref_ => {
                    if !has_address(types, place) {
                        return Err(ParseError::BadAddressOf);
                    }
                    let pointee = crate::identities::read::output_type(types, place);
                    self.place_address(place, node, pointee)?
                }
                // A node's field, a map entry, a tape cell: a place with no address to hand out.
                Some(_) => return Err(ParseError::BadAddressOf),
                // A name names its value already: one that is no place, `x := 5`, has no address.
                None if named => return Err(ParseError::BadAddressOf),
                None => {
                    self.check_capture(node)?;
                    self.address_of_value(node)?
                }
            }
        };
        tape.remove(1);
        tape.place(addr);
        Ok(Constructed::Placed)
    }

    /// The address of a place's bytes, a frame's or a dereference's cell, as `@pointee`: read when
    /// the code runs, so a frame's place gives each activation's own.
    ///
    /// # Safety
    /// `place` must be a frame's place or a deref node from the store, `node` the operand it was
    /// read from, and `pointee` a type node.
    unsafe fn place_address(
        &mut self,
        place: DyadPtr,
        node: DyadPtr,
        pointee: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        self.check_capture(node)?;
        let types = self.types;
        Ok(crate::identities::pointer::build_addr_as(
            self.rt.store,
            types,
            place,
            std::ptr::null_mut(),
            pointee,
        ))
    }

    /// `&` of a value nothing names: the value lands in an unspelled name of the open scope, as
    /// `:=` lands it, which holds it to the scope's end, and `&` hands out that name's address
    /// (DESIGN ›Holding is decided at the binding site, parameters included‹ [[R61]]).
    ///
    /// # Safety
    /// `value` must be a reduced dyad from the store, no place and no hole.
    unsafe fn address_of_value(&mut self, value: DyadPtr) -> Result<DyadPtr, ParseError> {
        use crate::identities::read::{read_kind, value_type, Read};
        let types = self.types;
        value_type(types, value)?;
        if !has_address(types, value) {
            return Err(ParseError::BadAddressOf);
        }
        // A literal is a constant that no place holds: held, it is a run-time `rational_number`,
        // as `rational_number 1` makes one (DESIGN ›A plain number is a `rational_number`, and a
        // number type's definition converts it‹).
        let value = if read_kind(types, value) == Read::Literal {
            crate::identities::by_copy::build_out(self.rt.store, types, types.through(value))
        } else {
            value
        };
        let scope = self.cx.scopes.current().expect("`&` parses in an open scope");
        let binding = self.mint_binding(std::ptr::null_mut(), scope, b"");
        let declared = self.land_at_binding(binding, value, false, false)?;
        // Only a landing that lays out bytes holds the value, so a refusal here holds nothing.
        if !types.is_storage(binding) {
            return Err(ParseError::BadAddressOf);
        }
        self.note_address_hold(binding);
        let held =
            crate::identities::declare::build(self.rt.store, types, binding, value, declared);
        Ok(crate::identities::pointer::build_addr(self.rt.store, types, binding, held))
    }

    /// Every further `@` cell to the right, then the base type cell, any type
    /// identity (DESIGN ›Pointer types are prefix `@T`‹), built into the
    /// pointer type. What a place of `@T` reads as never depends on `T`.
    pub(crate) fn construct_pointer_type(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let mut depth = 1usize;
        while matches!(self.cell_at(tape, 1)?, Some(c) if !c.constructed && self.cell_identity(&c) == self.types.at_)
        {
            tape.remove(1);
            depth += 1;
        }
        let Some(cell) = self.cell_at(tape, 1)? else {
            return Err(ParseError::UnsupportedOperands);
        };
        let base = self.operand_dyad(cell)?;
        // SAFETY: `base` is a resolved dyad from the store.
        if !unsafe { crate::identities::is_type_value(self.types, base) } {
            return Err(ParseError::UnsupportedOperands);
        }
        tape.remove(1);
        let mut logos = base;
        for _ in 0..depth {
            // SAFETY: `logos` is the base type node, or the pointer type minted in the previous round.
            logos = unsafe {
                crate::identities::pointer::make_pointer_type(
                    self.rt.store,
                    self.types.type_,
                    logos,
                )
            };
        }
        tape.place(logos);
        Ok(Constructed::Placed)
    }

    /// `:`'s constructor (DESIGN ›The dyad's read surface‹): a field of the
    /// binding to the left, the cell taken as it stands, never through the
    /// reading rule; the member is the raw spelling at the cell to the right.
    pub(crate) fn construct_binding_read(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let lhs = match tape.at(-1).copied() {
            Some(cell) => self.as_operand(cell)?,
            None => return Err(ParseError::MissingOperand),
        };
        let Some(m) = self.cell_at(tape, 1)? else {
            return Err(ParseError::ExpectedField);
        };
        let save = self.cx.pos;
        self.cx.pos = m.start;
        let member = self.lex_spelling();
        self.cx.pos = save;
        let Some((nstart, nlen)) = member else {
            return Err(ParseError::ExpectedField);
        };
        let dotted = self
            .cell_at(tape, 2)?
            .is_some_and(|c| !c.constructed && self.cell_identity(&c) == self.types.dot_);
        // SAFETY: `lhs` is a dyad off the tape.
        let node = unsafe { self.binding_read_each(lhs, nstart, nlen, dotted)? };
        tape.remove(1);
        tape.remove(-1);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// A group's read is each member's; a name reads its own binding, never what it holds.
    /// `dotted`: a `.` follows the member.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn binding_read_each(
        &mut self,
        lhs: DyadPtr,
        nstart: usize,
        nlen: usize,
        dotted: bool,
    ) -> Result<DyadPtr, ParseError> {
        let group = if dyad::ty(lhs) == self.types.binding_ {
            None
        } else {
            crate::identities::group::members(self.types, lhs)
        };
        let Some((connective, a, b)) = group else {
            return self.binding_read(lhs, nstart, nlen, dotted);
        };
        let a = self.binding_read_each(a, nstart, nlen, dotted)?;
        let b = self.binding_read_each(b, nstart, nlen, dotted)?;
        crate::identities::group::join(self.rt.store, self.types, connective, a, b)
    }

    /// On a binding, `start` is the declaring line once it is complete, `dyad` the node the name
    /// stands for, and any other field the instance-field read in `binding`'s own scope; on a
    /// constructed node the path answers (DESIGN ›Meta-navigation‹), `start`/`end` null.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn binding_read(
        &mut self,
        lhs: DyadPtr,
        nstart: usize,
        nlen: usize,
        dotted: bool,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let source = self.cx.source;
        let name = &source[nstart..nstart + nlen];
        // A name answers from its own binding; a node reached by path from itself.
        let named = dyad::ty(lhs) == types.binding_;
        let reached = if named { None } else { self.known_node(lhs, std::ptr::null_mut()) };
        let lhs = reached.unwrap_or(lhs);
        if crate::identities::tape::is_cell_read(types, lhs) {
            // `t[k]:name`: the spelling of the binding the cell holds.
            if name == "name" {
                return Ok(crate::identities::tape::build_slot_name(self.rt.store, types, lhs));
            }
            return Err(ParseError::ExpectedField);
        }
        if name == "dyad" && (named || reached.is_some()) {
            return self.dyad_read(lhs, dotted);
        }
        if dyad::ty(lhs) == types.binding_ {
            // `b:start`: the declaring line, once it is complete, is a node
            // like any other (DESIGN ›The dyad's read surface‹).
            let start = Binding::read(lhs).start;
            if name == "start" && !start.is_null() {
                return Ok(self.path_operand(start));
            }
            // The binding is a value of type `binding` laid out in the store's arena, the
            // program frame: its field is a place read through two bindings, as `p.x` is.
            let (_, _, field_binding) = self.resolve_field(types.binding_, nstart, nlen)?;
            let record = self.binding_record_place(lhs);
            return Ok(crate::identities::instance::build_field(
                self.rt.store,
                types,
                record,
                field_binding,
            ));
        }
        let value = match name {
            "scope" => self.cx.scopes.current().unwrap_or(std::ptr::null_mut()),
            "start" | "end" | "gate" => std::ptr::null_mut(),
            _ => {
                // Not a field of a binding: the same error a binding read gives.
                self.resolve_field(types.binding_, nstart, nlen)?;
                return Err(ParseError::ExpectedField);
            }
        };
        Ok(self.address_value(types.dyad_, value))
    }

    /// `x:dyad`, as a `dyad`: the node a name stands for (›`x.type` reads the type word of the node
    /// a name or an expression stands for‹ [[R279]]), or a node reached by path itself (›A
    /// constructed node answers `:` from the path it was reached by‹ [[R320]]). Read by `.`, a
    /// name's view holds its binding, which [`Parser::view_member`] reads; handed on whole, a name
    /// gives the node it denotes, and one whose bytes lie in a frame has none yet.
    ///
    /// # Safety
    /// `lhs` must be a binding dyad or a node from the store.
    unsafe fn dyad_read(&mut self, lhs: DyadPtr, dotted: bool) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let node = if dyad::ty(lhs) != types.binding_ || dotted {
            lhs
        } else if types.is_storage(lhs) {
            return Err(ParseError::NoNodeYet);
        } else {
            Binding::read(lhs).dyad
        };
        if node.is_null() {
            return Err(ParseError::BadReflectRead);
        }
        Ok(self.rt.store.alloc_head(types.dyad_, node.cast()))
    }

    /// The binding's own record as storage: a spelling-less binding over the record's
    /// bytes, which lie in the store's arena, the program frame.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store, its record in the store's arena.
    unsafe fn binding_record_place(&mut self, binding: DyadPtr) -> DyadPtr {
        let types = self.types;
        let offset = (dyad::value(binding) as usize) - (self.rt.store.arena_base() as usize);
        let record = Binding::alloc(
            self.rt.store,
            types.binding_,
            Binding::new(types.binding_, types.root_scope, std::ptr::null_mut()),
        );
        Binding::lay_out(record, types.binding_, types.root_scope, offset);
        record
    }

    /// A pointer-typed literal with its own eight bytes of storage, read at
    /// run time like any pointer variable.
    fn address_value(&mut self, pointee: DyadPtr, addr: DyadPtr) -> DyadPtr {
        crate::identities::pointer::address_value(self.rt.store, self.types, pointee, addr)
    }

    /// `node` reached by path: as a value its address, so using it runs nothing;
    /// the reads after it fold through [`Parser::known_node`].
    fn reach(&mut self, node: DyadPtr) -> DyadPtr {
        self.address_value(self.types.dyad_, node)
    }

    /// The node an `@dyad` value stands for when the parse knows it: an
    /// address no `=` writes, or a name `root` no `=` can move, followed through
    /// its declaring line to the right side as written (DESIGN ›The dyad's read
    /// surface‹: a binding gets you a dyad or a node).
    ///
    /// # Safety
    /// `value` must be a dyad from the store; `root` null or a binding dyad.
    unsafe fn known_node(&self, value: DyadPtr, root: DyadPtr) -> Option<DyadPtr> {
        use crate::identities::{declare, read};
        let types = self.types;
        if !root.is_null() {
            if Binding::has_gate(root, types.mut_) {
                return None;
            }
            let start = Binding::read(root).start;
            if start.is_null()
                || dyad::ty(start) != types.declare_
                || declare::binding_of(start) != root
            {
                return None;
            }
            let rhs = declare::rhs_of(start);
            let next = if dyad::ty(rhs) == types.binding_ { rhs } else { std::ptr::null_mut() };
            return self.known_node(types.through(rhs), next);
        }
        let stored = dyad::value(value);
        if stored.is_null()
            || types.is_storage(value)
            || !matches!(read::read_kind(types, value), read::Read::Pointer(p) if p == types.dyad_)
        {
            return None;
        }
        let node = *(stored as *const DyadPtr);
        (!node.is_null()).then_some(node)
    }

    /// `.type`, read on every value (DESIGN ›`x.type` reads the type word of the node a name or
    /// an expression stands for‹): a tape cell's when the constructor runs, a field's declared
    /// type through `tape[0]`, a node reached by path or an operator expression its own type word,
    /// a view the viewed node's, a name its own and a call the type it returns.
    ///
    /// # Safety
    /// `lhs` must be a reduced dyad from the store.
    unsafe fn type_read(&mut self, lhs: DyadPtr) -> Result<DyadPtr, ParseError> {
        use crate::identities::{tape, this};
        let types = self.types;
        if dyad::ty(lhs) == types.tape.cell_dyad_at || tape::is_cell_read(types, lhs) {
            return Ok(tape::build_cell_type(self.rt.store, types, lhs));
        }
        // An untyped field of the parse's own node has the type of the node it holds, read when
        // the constructor runs.
        if this::is_field_read(types, lhs) {
            if !this::is_fill(types, lhs) {
                return Err(ParseError::BadReflectRead);
            }
            let declared = crate::identities::hole::type_in(
                Binding::read(this::field_binding_of(types, lhs)).dyad,
            );
            return Ok(if declared.is_null() {
                this::build_field_type(self.rt.store, types, lhs)
            } else {
                declared
            });
        }
        if let Some(node) = self.known_node(lhs, self.cx.member_root) {
            return Ok(types.type_of(node));
        }
        if dyad::ty(lhs) == types.dyad_ {
            return self.view_member(lhs, "type");
        }
        // An expression the parse refuses where it is written, as an `if` whose arms give two
        // types, is refused for that reason first.
        let t = crate::identities::read::value_type(types, lhs)?;
        if self.is_operator_node(lhs) {
            return Ok(dyad::ty(lhs));
        }
        if t.is_null() {
            return Err(ParseError::TypeKnownOnlyAtRun);
        }
        Ok(t)
    }

    /// An operator expression, which `.` reads as its node (DESIGN ›Reading a path runs
    /// nothing‹): a node that runs, whose type word is an identity with a parse of its own, the
    /// one that built it from its operands (`x + 2`, `x ^ 3`), or a mint of one. A call's type
    /// word is a function or an inner node with no parse (`pt(3, 4)`, `m[k]`), and the call
    /// stands for what it returns.
    ///
    /// # Safety
    /// `node` must be a reduced dyad from the store.
    unsafe fn is_operator_node(&self, node: DyadPtr) -> bool {
        use crate::identities::read::{read_kind, Read};
        let ty = crate::identities::mint::origin(dyad::ty(node));
        matches!(read_kind(self.types, node), Read::Executable(_))
            && self.identity_head(ty).is_some()
            && !crate::identities::meta::constructor_of(ty).is_null()
    }

    /// Exactly the cell's two fields, `.type` and `.value` (DESIGN ›The dyad's read surface‹), the
    /// value as the address of its bytes, an `@void` (›`undefined`, `?`, `@void` and `native`‹
    /// [[R135]]); nothing else reads through the view, and nothing here writes. A view of a name's
    /// binding, `x:dyad`, reads `.type` as `x.type` does, and a name whose bytes lie in a frame
    /// gives their address there, read when the code runs.
    ///
    /// # Safety
    /// `view` must be a node of type `dyad`.
    unsafe fn view_member(&mut self, view: DyadPtr, name: &str) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let viewed = dyad::head(view) as DyadPtr;
        if viewed.is_null() {
            return Err(ParseError::BadReflectRead);
        }
        let named = dyad::ty(viewed) == types.binding_;
        match name {
            "type" if named => {
                let root = std::mem::replace(&mut self.cx.member_root, viewed);
                let lhs = self.settled_type(types.through(viewed));
                let t = self.type_read(lhs);
                self.cx.member_root = root;
                t
            }
            "type" => Ok(dyad::ty(viewed)),
            "value" if named && types.is_storage(viewed) => {
                self.place_address(viewed, viewed, types.void_)
            }
            "value" => {
                let node = if named { types.through(viewed) } else { viewed };
                Ok(self.address_value(types.void_, dyad::value(node).cast()))
            }
            _ => Err(ParseError::BadReflectRead),
        }
    }

    /// The shared metadata stored once per type: `.arity`, `.roles[i]`,
    /// `.parse_rank`, `.associativity`, `.parse`, `.run`, a `share` member,
    /// `.size_bytes`, `.scope`; a null slot is the honest undefined and errors until `?` exists.
    ///
    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn logos_member(
        &mut self,
        logos: DyadPtr,
        name: &str,
        index: Option<usize>,
    ) -> Result<DyadPtr, ParseError> {
        use crate::identities::meta;
        use crate::identities::numtype::NumType;
        if meta::kind_of(logos).is_none() {
            return Err(ParseError::BadReflectRead);
        }
        let operand_kind = matches!(meta::kind_of(logos), Some(meta::TUPLE_TAG | meta::LIST_TAG));
        match name {
            "arity" if operand_kind => {
                Ok(self.scalar_value(NumType::I64, meta::arity_of(logos) as i64))
            }
            "roles" if operand_kind => {
                let i = index.ok_or(ParseError::ExpectedIndexBracket)?;
                if i >= meta::arity_of(logos) {
                    return Err(ParseError::BadReflectRead);
                }
                Ok(meta::role_of(logos, i))
            }
            "parse_rank" => {
                Ok(self.scalar_value(NumType::F64, meta::parse_rank_of(logos).to_bits() as i64))
            }
            // `lex_rank` is the name's binding's, not the type's, so it falls to
            // the unknown-member error below. Associativity's values are the identities `left` and `right`.
            "associativity" => Ok(match meta::assoc_of(logos) {
                Assoc::Left => self.types.left_,
                Assoc::Right => self.types.right_,
            }),
            // The Logos function a body filled the slot with, or the view of a native leaf.
            "parse" => {
                let c = meta::constructor_of(logos);
                if c.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                if dyad::ty(c) == self.types.fn_type {
                    return Ok(c);
                }
                Ok(self.rt.store.alloc_head(self.types.dyad_, c as *mut u8))
            }
            // The held body is no function until a node's field types construct
            // it, so it is read through a view, as `.parse` reads a native leaf.
            "run" => {
                let held = meta::run_body_of(logos);
                if held.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                Ok(self.rt.store.alloc_head(self.types.dyad_, held as *mut u8))
            }
            "size_bytes" if meta::is_record_type(logos) => {
                Ok(self.scalar_value(NumType::U64, meta::record_size_of(logos) as i64))
            }
            "size_bytes" => match crate::identities::read::place_layout(self.types, logos) {
                Some((
                    crate::identities::read::Read::Scalar(_)
                    | crate::identities::read::Read::Pointer(_),
                    width,
                )) => Ok(self.scalar_value(NumType::U64, width as i64)),
                _ => Err(ParseError::BadReflectRead),
            },
            "scope" if meta::is_record_type(logos) => Ok(self
                .rt
                .store
                .alloc_head(self.types.dyad_, meta::record_scope_of(logos) as *mut u8)),
            _ if self.share_member_of(logos, name).is_some() => {
                Ok(self.share_member_read(logos, name).expect("found just above"))
            }
            _ if self.per_node_field_of(logos, name) => {
                Err(ParseError::PerNodeThroughType(name.to_string()))
            }
            _ => Err(ParseError::BadReflectRead),
        }
    }

    /// A `share` member of `logos`, one place stored with the type, read
    /// through the type or a node alike: the member's own gate decides a
    /// write, since the place is no node's.
    ///
    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn share_member_read(&mut self, logos: DyadPtr, name: &str) -> Option<DyadPtr> {
        let (identity, binding) = self.share_member_of(logos, name)?;
        self.cx.member_path = vec![binding];
        Some(identity)
    }

    /// `lhs.name` read as a `share` member of `logos`, the type `lhs` is read through: the
    /// member, or, called, a `share` function that takes `lhs` as its receiver. `None` when
    /// `logos` has no such member.
    ///
    /// # Safety
    /// `lhs` must be a reduced dyad from the store and `logos` a type identity node.
    unsafe fn share_member_access(
        &mut self,
        lhs: DyadPtr,
        left: &Cell,
        logos: DyadPtr,
        name: &str,
        call: Option<Vec<DyadPtr>>,
    ) -> Result<Option<(DyadPtr, usize)>, ParseError> {
        let Some(member) = self.share_member_read(logos, name) else {
            return Ok(None);
        };
        if let Some(args) = call {
            if self.takes_this(member) {
                self.check_receiver_write(left, member)?;
                // A record laid out in bytes is handed on as the address of its bytes,
                // any other value by a `dyad` view, as a `dyad ?` parameter takes a node.
                let view = if self.types.is_storage(lhs) {
                    let types = self.types;
                    crate::identities::by_copy::build_out(self.rt.store, types, lhs)
                } else {
                    self.rt.store.alloc_head(self.types.dyad_, lhs.cast())
                };
                let mut with_this = vec![view];
                with_this.extend(args);
                self.cx.member_path.clear();
                return self.build_call(member, &with_this).map(|n| Some((n, 1)));
            }
        }
        Ok(Some((member, 0)))
    }

    /// The element type a type of node values built from a bracket holds; stand-in for #152.
    ///
    /// # Safety
    /// `ty` must be a type identity node from the store.
    pub(super) unsafe fn bracket_element_type(&self, ty: DyadPtr) -> Option<DyadPtr> {
        let (identity, _) = self.share_member_of(ty, "element_type")?;
        let element = self.types.through(identity);
        crate::identities::is_type_value(self.types, element).then_some(element)
    }

    /// The identity and binding of `logos`'s `share` member `name`, declared
    /// in the definition body's scope.
    ///
    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn share_member_of(&self, logos: DyadPtr, name: &str) -> Option<(DyadPtr, DyadPtr)> {
        use crate::identities::meta;
        if !meta::is_record_type(logos) {
            return None;
        }
        let body = meta::record_body_of(logos);
        if body.is_null() {
            return None;
        }
        let mut members = ScopeStack::new();
        members.push(body);
        let r = members.resolve(self.trie, name).ok()?;
        Some((r.identity, r.binding))
    }

    /// # Safety
    /// `logos` must be a type identity node from the store.
    unsafe fn per_node_field_of(&self, logos: DyadPtr, name: &str) -> bool {
        use crate::identities::meta;
        if !meta::is_record_type(logos) {
            return false;
        }
        let mut fields = ScopeStack::new();
        fields.push(meta::record_scope_of(logos));
        fields.resolve(self.trie, name).is_ok()
    }

    /// Fresh storage holding `bits` at `nt`'s width: the reflection counts
    /// and measures are ordinary typed values, comparable with literals.
    pub(super) fn scalar_value(
        &mut self,
        nt: crate::identities::numtype::NumType,
        bits: i64,
    ) -> DyadPtr {
        let ty = self.types.numtypes[nt as usize];
        let width = nt.bytes();
        let bytes = bits.to_ne_bytes();
        self.rt.store.alloc_blob(ty, &bytes[..width])
    }
}
