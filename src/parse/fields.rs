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

impl<'a> Parser<'a> {
    /// The cell before is an unbuilt `.` or `:`, whose right side is a member's spelling.
    pub(super) fn after_tight_read(&self, tape: &ParsingTape) -> bool {
        tape.last().is_some_and(|l| {
            !l.constructed && {
                let id = self.cell_identity(l);
                id == self.types.dot_ || id == self.types.colon_
            }
        })
    }

    /// Inside a `drop` or a `share` function a field of the value is named bare (DESIGN
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

    /// `tape[0]:type = T`: the fresh node placed in the parse's own cell, from where its
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
    /// body is about (DESIGN ›There is no `this`‹): a `run`'s node, a `drop`'s or `share`
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
        Ok(node)
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
            // `[field…, null, spec]`: the fields are the nodes the constructor filled,
            // for every node a Logos-written `parse` built.
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
            return Ok(None);
        };
        let field = *(value as *const DyadPtr).add(i);
        if field.is_null() {
            return Err(ParseError::BadReflectRead);
        }
        Ok(Some(self.path_operand(field)))
    }

    /// Whether `t`'s field `name` is declared of a type a Logos `parse` builds.
    ///
    /// # Safety
    /// `t` must be a record type from the store.
    unsafe fn holds_node(&self, t: DyadPtr, name: &str) -> bool {
        use crate::identities::{array, meta};
        let mut fields = ScopeStack::new();
        fields.push(meta::record_scope_of(t));
        fields.resolve(self.trie, name).ok().is_some_and(|r| {
            array::items(meta::record_fields_of(t)).contains(&r.identity)
                && meta::is_node_valued(
                    crate::identities::hole::type_in(r.identity),
                    self.types.fn_type,
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
            // A field holding a node a Logos `parse` built is read when the code runs, even
            // through a node the parse knows: the node it holds may be replaced before then.
            if let Some(t) = crate::identities::node_type_of(self.types, lhs) {
                if self.holds_node(t, name) {
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
                if let Some(t) = crate::identities::node_type_of(self.types, lhs) {
                    if let Some(read) = self.run_node_member(lhs, left, t, name, call.clone())? {
                        return Ok(read);
                    }
                }
            }
            // `(2 ^ 3).lhs`: the node a comptime value was folded from keeps its fields.
            let lhs = if left.origin.is_null() { lhs } else { left.origin };
            if dyad::ty(lhs) == self.types.dyad_ {
                return self.view_member(lhs, name).map(|n| (n, 0));
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
            if matches!(
                crate::identities::read::read_kind(self.types, lhs),
                crate::identities::read::Read::Container(t) if t == self.types.type_ || t == self.types.dyad_
            ) || crate::identities::yields_type(self.types, lhs)
            {
                return Err(ParseError::TypeKnownOnlyAtRun);
            }
            if name == "type" {
                return Err(ParseError::TypeIsColonRead);
            }
            // An operator node's slots are the fields its own type defines:
            // `.operands[i]` fetches one, no view involved; a null slot is the checked error until `?`.
            if name == "operands"
                && !dyad::ty(lhs).is_null()
                && matches!(
                    crate::identities::meta::kind_of(dyad::ty(lhs)),
                    Some(crate::identities::meta::TUPLE_TAG | crate::identities::meta::LIST_TAG)
                )
            {
                let i = index.ok_or(ParseError::ExpectedIndexBracket)?;
                if i >= crate::identities::meta::arity_of(dyad::ty(lhs)) {
                    return Err(ParseError::BadReflectRead);
                }
                let ops = dyad::value(lhs) as *const DyadPtr;
                let operand = *ops.add(i);
                if operand.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                return Ok((operand, 1));
            }
            if let Some(field) = self.node_field(lhs, name)? {
                return Ok((field, 0));
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
                return Ok((
                    self.rt
                        .store
                        .alloc_words(self.types.compile_, &[lhs, code, self.types.ops.compile_]),
                    1,
                ));
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
                let Some(member) = self.share_member_read(record_logos, name) else {
                    return Err(e);
                };
                if let Some(args) = call {
                    if self.takes_this(member) {
                        self.check_receiver_write(&left, member)?;
                        // A record laid out in bytes is handed on as the address of its bytes,
                        // any other value by a `dyad` view, as a `dyad ?` parameter takes a node.
                        let view = if storage {
                            crate::identities::instance::layout(record_logos)?;
                            let types = self.types;
                            crate::identities::by_copy::build_out(self.rt.store, types, lhs)
                        } else {
                            self.rt.store.alloc_head(self.types.dyad_, lhs.cast())
                        };
                        let mut with_this = vec![view];
                        with_this.extend(args);
                        self.cx.member_path.clear();
                        return self.build_call(member, &with_this).map(|n| (n, 1));
                    }
                }
                return Ok((member, 0));
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
        self.cell_at(tape, 2)?;
        let index = self.index_at(tape, 2);
        let bracket = tape.at(2).filter(|c| c.is_bracket()).map(|c| c.dyad);
        // SAFETY: a bracket cell is a node from the store.
        let call = bracket.map(|d| unsafe { self.args_of(d) });
        let key = self.index_node_at(tape, 2);
        let writes = index.is_none()
            && key.is_none()
            && bracket.is_none()
            && tape
                .at(2)
                .is_some_and(|c| !c.constructed && self.cell_identity(c) == self.types.assign);
        let source = self.cx.source;
        // A name its value's parse woke stands constructed and still holds its binding.
        let woke = tape.at(-1).filter(|c| c.constructed && !c.dyad.is_null()).map(|c| c.dyad);
        // SAFETY: a constructed cell's dyad is a dyad from the store.
        let named = woke.filter(|&d| unsafe { dyad::ty(d) } == self.types.binding_).unwrap_or(root);
        // SAFETY: `named` is null or a binding dyad; `lhs` a reduced dyad off the tape.
        let fills = unsafe {
            self.check_field_unwritten(named, lhs, &source[nstart..nstart + nlen], writes)
        };
        if let Err(e) = fills {
            self.cx.pos = nstart;
            return Err(e);
        }
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
            if let Ok(Some(fill)) = fills {
                cell.target = fill;
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
        let (scope, key) = self.parse_block(true)?;
        let key = key.dyad;
        self.expect_close_sq()?;
        // An index right after a tape value is the element read, a slot node
        // the identity to its left owns; after a `.` it stays a passive cell.
        if let Some(left) = tape.at(-1).copied() {
            // A fresh spelling to the left is a member name after `.`, never a tape.
            if !left.is_fresh() && self.is_operand_cell(&left) {
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
        let node =
            self.rt.store.alloc_words(self.types.square_brackets, &[dyads, std::ptr::null_mut()]);
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

    /// The lhs's static type must be a pointer type: a pointer variable or
    /// `&x`, a pointer field place, or another deref whose pointee is a pointer.
    ///
    /// # Safety
    /// `lhs` must be a reduced dyad from the store.
    pub(crate) unsafe fn build_deref(&mut self, lhs: DyadPtr) -> Result<DyadPtr, ParseError> {
        // The pointer expression is stored as it stands (a use of a name is
        // its binding); its type is read through the reading rule.
        let read = self.types.through(lhs);
        let ptr_ty = if dyad::ty(read) == self.types.deref_ {
            crate::identities::pointer::deref_parts(read).1
        } else {
            self.types.type_of(read)
        };
        let pointee = if ptr_ty == self.types.plus || ptr_ty == self.types.minus {
            // A pointer step, `(p + k)@`: its pointee is the stepped pointer's.
            match crate::identities::numtype_of(self.types, read) {
                crate::identities::Operand::Pointer(pointee) => pointee,
                _ => return Err(ParseError::UnsupportedOperands),
            }
        } else {
            // The pointee rides on the reading rule's answer.
            let Some((crate::identities::read::Read::Pointer(pointee), _)) =
                crate::identities::read::place_layout(self.types, ptr_ty)
            else {
                return Err(ParseError::UnsupportedOperands);
            };
            pointee
        };
        let types = self.types;
        Ok(crate::identities::pointer::build_deref(self.rt.store, types, lhs, pointee, 0))
    }

    /// `&`'s constructor: an `addr` node that resolves the place's address at
    /// run/lower time, so a frame-relative local yields a per-activation
    /// address. A comptime binding has no storage.
    pub(crate) fn construct_address_of(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let Some(&cell) = tape.at(1) else {
            return Err(ParseError::BadAddressOf);
        };
        // Keywords, operators, literals: not places.
        if !cell.constructed
            && !cell.is_fresh()
            && self.ctor_of(self.cell_identity(&cell)).is_some()
        {
            return Err(ParseError::BadAddressOf);
        }
        let node = self.operand_dyad(cell)?;
        // SAFETY: `node` is a resolved dyad from the store.
        let addr = unsafe {
            // A place the reading rule can name, marked as storage: a literal's
            // blob is not one, nor a node of a code-carrying type (an operand run, no fields).
            use crate::identities::read::{read_kind, Read};
            let placed = matches!(
                read_kind(self.types, node),
                Read::Scalar(_) | Read::Pointer(_) | Read::Aggregate
            ) && self.types.is_storage(node);
            // A binding's fields are read; `lex_rank` alone is a place a program writes.
            let binding_field = self
                .types
                .binding_field_of(node)
                .is_some_and(|f| Binding::spelling(f) != "lex_rank");
            if !placed || binding_field {
                return Err(ParseError::BadAddressOf);
            }
            self.check_capture(node)?;
            crate::identities::pointer::build_addr(self.rt.store, self.types, node)
        };
        tape.remove(1);
        tape.place(addr);
        Ok(Constructed::Placed)
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
        // SAFETY: `lhs` is a dyad off the tape.
        let node = unsafe { self.binding_read_each(lhs, nstart, nlen)? };
        tape.remove(1);
        tape.remove(-1);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// A group's read is each member's; a name reads its own binding, never what it holds.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn binding_read_each(
        &mut self,
        lhs: DyadPtr,
        nstart: usize,
        nlen: usize,
    ) -> Result<DyadPtr, ParseError> {
        let group = if dyad::ty(lhs) == self.types.binding_ {
            None
        } else {
            crate::identities::group::members(self.types, lhs)
        };
        let Some((connective, a, b)) = group else {
            return self.binding_read(lhs, nstart, nlen);
        };
        let a = self.binding_read_each(a, nstart, nlen)?;
        let b = self.binding_read_each(b, nstart, nlen)?;
        crate::identities::group::join(self.rt.store, self.types, connective, a, b)
    }

    /// On a binding, `type` is the type of the dyad the name stands for,
    /// `start` the declaring line once it is complete, and any other field the
    /// instance-field read in `binding`'s own scope; on a constructed node
    /// `type` is its own and the path answers the rest (DESIGN ›Meta-navigation‹),
    /// `start`/`end` null.
    ///
    /// # Safety
    /// `lhs` must be a valid dyad from the store.
    unsafe fn binding_read(
        &mut self,
        lhs: DyadPtr,
        nstart: usize,
        nlen: usize,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let source = self.cx.source;
        let name = &source[nstart..nstart + nlen];
        // The binding's `dyad` field is storage, not a spelling.
        if name == "dyad" || name == "value" {
            return Err(ParseError::CellNotReachable);
        }
        // A name answers from its own binding; a node reached by path from itself.
        let lhs = if dyad::ty(lhs) == types.binding_ {
            lhs
        } else {
            self.known_node(lhs, std::ptr::null_mut()).unwrap_or(lhs)
        };
        // Read when the constructor runs.
        if dyad::ty(lhs) == types.tape.cell_dyad_at && name == "type" {
            return Ok(crate::identities::tape::build_cell_type(self.rt.store, types, lhs));
        }
        if crate::identities::tape::is_cell_read(types, lhs) {
            if name == "type" {
                return Ok(crate::identities::tape::build_cell_type(self.rt.store, types, lhs));
            }
            // `t[k]:name`: the spelling of the binding the cell holds.
            if name == "name" {
                return Ok(crate::identities::tape::build_slot_name(self.rt.store, types, lhs));
            }
            return Err(ParseError::ExpectedField);
        }
        if dyad::ty(lhs) == types.binding_ {
            if name == "type" {
                // Through a settled box: the type of what the name holds; storage answers
                // with its declared type.
                let cell = self.settled_type(Binding::read(lhs).names(lhs));
                if cell.is_null() {
                    return Err(ParseError::BadReflectRead);
                }
                return Ok(types.type_of(cell));
            }
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
        // `tape[0].f:type` is the field's declared type, not the type of the read that reaches it;
        // an untyped field's type is the written value's, unknown until the constructor runs.
        if crate::identities::this::is_field_read(types, lhs) && name == "type" {
            let declared = if crate::identities::this::is_fill(types, lhs) {
                crate::identities::hole::type_in(
                    Binding::read(crate::identities::this::field_binding_of(types, lhs)).dyad,
                )
            } else {
                std::ptr::null_mut()
            };
            if declared.is_null() {
                return Err(ParseError::BadReflectRead);
            }
            return Ok(declared);
        }
        let value = match name {
            "type" => return Ok(types.type_of(lhs)),
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

    /// Exactly the cell's two fields, `.type` and `.value` (DESIGN ›The dyad's
    /// read surface‹); nothing else reads through the view, and nothing here writes.
    ///
    /// # Safety
    /// `view` must be a node of type `dyad`.
    unsafe fn view_member(&mut self, view: DyadPtr, name: &str) -> Result<DyadPtr, ParseError> {
        let viewed = dyad::head(view) as DyadPtr;
        if viewed.is_null() {
            return Err(ParseError::BadReflectRead);
        }
        match name {
            "type" => Ok(dyad::ty(viewed)),
            // The raw address as a u64; the `@void` spelling waits for pointer-value plumbing.
            "value" => Ok(self.scalar_value(
                crate::identities::numtype::NumType::U64,
                dyad::value(viewed) as usize as i64,
            )),
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
            "type" => Err(ParseError::TypeIsColonRead),
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
