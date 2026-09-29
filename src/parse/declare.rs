// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Declarations: `:=`, `?`, `dyad`, the binding and its gates, what a write must be
//! granted by, ownership marks, and the unwritten-fields rule.

use super::*;
use crate::dyad;

impl<'a> Parser<'a> {
    /// A gate word marks the declaration that just reduced to its right.
    pub(crate) fn gate_declared(&mut self, gate: DyadPtr) -> Result<(), ParseError> {
        let binding = self.cx.last_declared;
        if binding.is_null() {
            return Err(ParseError::GateNeedsDeclaration);
        }
        self.add_gate(binding, gate)
    }

    /// A write into a place reached by a path is granted by every binding
    /// along it, `immut` vetoing first; the constructor's fill of its fresh
    /// node is granted unless `immut` vetoes it; a place reached no such way passes here.
    pub(crate) fn check_path_write(&self, target: &Cell) -> Result<(), ParseError> {
        use crate::identities::this;
        let node = target.dyad;
        // SAFETY: `target` holds a reduced dyad from the store and a null or `array` path; a
        // fill's binding is a binding dyad.
        unsafe {
            if this::is_field_read(self.types, node) && this::is_fill(self.types, node) {
                let binding = this::field_binding_of(self.types, node);
                if Binding::has_gate(binding, self.types.immut_) {
                    return Err(ParseError::Immutable(Box::new(Binding::spelling(binding))));
                }
                return Ok(());
            }
        }
        self.check_gates(&self.path_of(target))
    }

    /// The bindings a write into the cell's node must be granted by: the cell's path, or
    /// for a field read outside the type's own parse the field's binding alone.
    pub(super) fn path_of(&self, cell: &Cell) -> Vec<DyadPtr> {
        use crate::identities::this;
        // SAFETY: a cell's path is null or an `array` node, its dyad null or a dyad from the
        // store, and a field read's binding operand null or a binding dyad.
        unsafe {
            if !cell.path.is_null() {
                return crate::identities::array::items(cell.path).to_vec();
            }
            let node = cell.dyad;
            if !node.is_null()
                && this::is_field_read(self.types, node)
                && !this::is_fill(self.types, node)
            {
                let binding = this::field_binding_of(self.types, node);
                if !binding.is_null() {
                    return vec![binding];
                }
            }
        }
        Vec::new()
    }

    fn check_gates(&self, path: &[DyadPtr]) -> Result<(), ParseError> {
        for &binding in path {
            // SAFETY: the path holds binding dyads from the store.
            unsafe {
                if Binding::has_gate(binding, self.types.immut_) {
                    return Err(ParseError::Immutable(Box::new(Binding::spelling(binding))));
                }
                if !Binding::has_gate(binding, self.types.mut_) {
                    return Err(ParseError::NotMutable(Box::new(Binding::spelling(binding))));
                }
            }
        }
        Ok(())
    }

    /// A `share` function that writes a field of its value meets the gates `receiver.f = …`
    /// would (DESIGN ›There is no `this`‹).
    ///
    /// # Safety
    /// `member` must be a reduced dyad from the store.
    pub(super) unsafe fn check_receiver_write(
        &self,
        receiver: &Cell,
        member: DyadPtr,
    ) -> Result<(), ParseError> {
        let f = self.types.through(member);
        if fn_receiver(self.types, f) & RECEIVER_WRITES == 0 {
            return Ok(());
        }
        let path = self.path_of(receiver);
        if !path.is_empty() {
            self.check_gates(&path)
        } else if !self.cx.member_root.is_null() {
            self.check_gates(&[self.cx.member_root])
        } else {
            Ok(())
        }
    }

    /// `=` over a field of the value a `share` function is reading.
    ///
    /// # Safety
    /// `target` must be a reduced dyad from the store.
    pub(crate) unsafe fn note_receiver_write(&mut self, target: DyadPtr) {
        let target = self.types.through(target);
        if !crate::identities::this::is_field_read(self.types, target) {
            return;
        }
        let this = *(dyad::value(target) as *const DyadPtr);
        if let Some(def) = self.cx.definitions.last_mut() {
            def.wrote_receiver |= !def.in_parse && !this.is_null() && this == def.this_param;
        }
    }

    /// Add `gate` to a name's binding, once.
    pub(crate) fn add_gate(&mut self, binding: DyadPtr, gate: DyadPtr) -> Result<(), ParseError> {
        // SAFETY: `binding` is a binding dyad from the store.
        unsafe {
            if Binding::has_gate(binding, gate) {
                return Err(ParseError::DoubleGate);
            }
            Binding::add_gate(self.rt.store, self.types.array_, binding, gate);
        }
        Ok(())
    }

    pub(super) fn mint_binding(
        &mut self,
        identity: DyadPtr,
        scope: DyadPtr,
        spelling: &[u8],
    ) -> DyadPtr {
        let name =
            crate::identities::string::build_text(self.rt.store, self.types.string_, spelling);
        let mut rec = Binding::new(identity, scope, name);
        rec.unmade = self.cx.runtime_depth > 0 && !self.in_fn_body();
        Binding::alloc(self.rt.store, self.types.binding_, rec)
    }

    /// One binding per declared name, a dyad of type `binding` (DESIGN ›`mut` is
    /// a gate on the binding‹); an error is reported at `at`, the name's own
    /// offset, so the caret lands on the name.
    pub(crate) fn declare_name(
        &mut self,
        name: &str,
        identity: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let scope = self.cx.scopes.current().expect("declare needs an open scope");
        let binding = self.mint_binding(identity, scope, name.as_bytes());
        // SAFETY: `binding` was minted by `Binding::alloc` just above.
        if let Err(e) = unsafe { self.cx.scopes.declare(self.trie, name, binding) } {
            self.cx.pos = at;
            return Err(ParseError::Resolve(e));
        }
        Ok(binding)
    }

    /// The pattern twin of `declare_name`.
    pub(crate) fn declare_pattern(
        &mut self,
        key: &str,
        identity: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let scope = self.cx.scopes.current().expect("declare needs an open scope");
        let binding = self.mint_binding(identity, scope, key.as_bytes());
        // SAFETY: `binding` was minted by `Binding::alloc` just above.
        if let Err(e) = unsafe { self.cx.scopes.declare_pattern(self.trie, key, binding) } {
            self.cx.pos = at;
            return Err(ParseError::Resolve(e));
        }
        Ok(binding)
    }

    /// Checked against its siblings alone; a failure is reported at `at`.
    pub(super) fn declare_field_name(
        &mut self,
        name: &str,
        identity: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let scope = self.cx.scopes.current().expect("declare needs an open scope");
        let binding = self.mint_binding(identity, scope, name.as_bytes());
        // SAFETY: `binding` was minted by `Binding::alloc` just above.
        if let Err(e) = unsafe { self.cx.scopes.declare_field(self.trie, name, binding) } {
            self.cx.pos = at;
            return Err(ParseError::Resolve(e));
        }
        Ok(binding)
    }

    /// Whether `d` is a read of a field declared `own` over a node's type: what it reaches
    /// is owned, for `=`, `move` and `free` to consult.
    pub(crate) fn is_owning_read(&self, d: DyadPtr) -> bool {
        use crate::identities::this;
        // SAFETY: `d` is null or a reduced dyad from the store; a field read's binding operand
        // is null or a binding dyad.
        unsafe {
            !d.is_null() && this::is_field_read(self.types, d) && {
                let binding = this::field_binding_of(self.types, d);
                !binding.is_null() && self.owns_node(binding)
            }
        }
    }

    /// Whether the name owns the node it holds: its binding site inserted the teardown
    /// that runs the node's instances' `free`, and its binding carries `own`.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store.
    pub(crate) unsafe fn owns_node(&self, binding: DyadPtr) -> bool {
        Binding::has_gate(binding, self.types.own_)
    }

    /// The binding site of a node whose type fills the instances' `free`: the name owns
    /// it, and `defer free <place>` goes into the scope where the ownership lands (DESIGN
    /// ›Holding is decided at the binding site‹; the inserted defer is a stand-in).
    ///
    /// # Safety
    /// `binding` must be the binding dyad being declared, `place` its node place, `free`
    /// the instances' `free` of the place's type.
    unsafe fn own_node(&mut self, binding: DyadPtr, place: DyadPtr, free: DyadPtr) {
        Binding::add_gate(self.rt.store, self.types.array_, binding, self.types.own_);
        let teardown = crate::identities::drop_model::build_instance_free(
            self.rt.store,
            self.types,
            place,
            free,
        );
        let defer_node =
            crate::identities::drop_model::build_defer(self.rt.store, self.types, teardown);
        let scope = self.teardown_scope();
        self.cx.open[scope].defers.push(defer_node);
    }

    /// Where a binding's teardown goes: its own scope, or the root for a `share` place,
    /// which lives as long as the program.
    fn teardown_scope(&self) -> usize {
        if self.cx.once_at == Some(self.cx.frames.len()) {
            0
        } else {
            self.cx.open.len() - 1
        }
    }

    /// `node` has emptied `ended`'s place: its name is dead from here on
    /// (DESIGN ›Memory and concurrency‹).
    pub(crate) fn mark_dead(&mut self, ended: Ended, node: DyadPtr) {
        // SAFETY: `ended.binding` is the binding the resolver returned for the operand.
        unsafe { self.cx.scopes.mark_dead(ended.binding, node) };
    }

    /// `?`'s entry on a binding refuses every read of the value until it is
    /// written: the target of `x = …` passes, and so does `x:…`, which reads
    /// the binding (DESIGN ›Declarations are immutable by default‹).
    pub(super) fn check_unwritten(&mut self, cell: &Cell) -> Result<(), ParseError> {
        let binding = cell.binding(self.types);
        // SAFETY: a non-null cell binding is a binding dyad from the store.
        if binding.is_null() || !unsafe { Binding::has_gate(binding, self.types.unknown) } {
            return Ok(());
        }
        if matches!(self.peek_token(), Some((t, _)) if t == self.types.assign || t == self.types.colon_)
        {
            return Ok(());
        }
        // `v.f`: the field read or write decides, one field at a time.
        // SAFETY: as above.
        if !unsafe { Binding::read(binding) }.unwritten.is_null()
            && matches!(self.peek_token(), Some((t, _)) if t == self.types.dot_)
        {
            return Ok(());
        }
        self.cx.pos = cell.start;
        // SAFETY: as above.
        Err(ParseError::Unwritten(Box::new(unsafe { Binding::spelling(binding) })))
    }

    /// The `=` just built, with the name and field its target `v.f` fills when the
    /// unwritten-fields rule noted one: the item the sibling rule reads.
    pub(crate) fn note_write(&mut self, node: DyadPtr, fill: (DyadPtr, DyadPtr)) {
        // SAFETY: `node` is the `=` node just built.
        if unsafe { dyad::ty(node) } == self.types.tape.write {
            self.forget_narrowed();
        }
        let scope = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        self.cx.last_write = LastWrite { node, scope, fill };
    }

    /// The type a check has narrowed the cell or line to, the innermost check first.
    fn narrowed_to(&self, key: CellKey) -> Option<DyadPtr> {
        self.cx
            .open
            .iter()
            .rev()
            .flat_map(|s| s.narrowed.iter().rev())
            .find(|n| n.0 == key)
            .map(|n| n.1)
    }

    /// `read`, a tape cell or line read, as the value a check narrowed it to, or as it stands.
    ///
    /// # Safety
    /// `read` must be a slot node or a line node the tape file built.
    pub(super) unsafe fn narrowed_read(&mut self, read: DyadPtr) -> DyadPtr {
        use crate::identities::tape;
        let types = self.types;
        let Some(ty) = tape::cell_key(types, read).and_then(|k| self.narrowed_to(k)) else {
            return read;
        };
        if ty == types.type_ {
            tape::build_cell_value(self.rt.store, types, read)
        } else if crate::identities::meta::is_node_valued(ty, types.fn_type) {
            tape::build_cell_node(self.rt.store, types, read, ty)
        } else {
            tape::build_cell_number(self.rt.store, types, read, ty)
        }
    }

    /// After a tape edit a checked index may name another cell.
    pub(super) fn forget_narrowed(&mut self) {
        for s in &mut self.cx.open {
            s.narrowed.clear();
        }
        self.cx.narrow_next = None;
    }

    /// A statement `x = …` built in the block that declared `x := T ?` lifts
    /// `?`'s read veto from that line on; a write nested in a group, an `if`,
    /// a loop or a `fn` body is no statement of that block, so it does not.
    ///
    /// # Safety
    /// `item` must be a reduced dyad from the store.
    pub unsafe fn fill_if_sibling_write(&mut self, item: DyadPtr) {
        let LastWrite { node, scope, fill } = self.cx.last_write;
        if item.is_null() || item != node {
            return;
        }
        if !fill.0.is_null() {
            let (binding, field) = fill;
            let rec = Binding::read(binding);
            if rec.scope != scope || rec.unwritten.is_null() {
                return;
            }
            let left: Vec<DyadPtr> = crate::identities::array::items(rec.unwritten)
                .iter()
                .copied()
                .filter(|&f| f != field)
                .collect();
            if left.is_empty() {
                Binding::set_unwritten(binding, std::ptr::null_mut());
                Binding::remove_gate(self.rt.store, self.types.array_, binding, self.types.unknown);
            } else {
                let arr = crate::identities::array::build(self.rt.store, self.types.array_, &left);
                Binding::set_unwritten(binding, arr);
            }
            return;
        }
        let target = *(dyad::value(item) as *const DyadPtr);
        if target.is_null()
            || dyad::ty(target) != self.types.binding_
            || !Binding::has_gate(target, self.types.unknown)
            || Binding::read(target).scope != scope
        {
            return;
        }
        Binding::remove_gate(self.rt.store, self.types.array_, target, self.types.unknown);
        Binding::set_unwritten(target, std::ptr::null_mut());
    }

    /// `v := T ?` of a type with fields: each field waits for its own write, those with a
    /// default already holding it when `defaults` says the place starts with them.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; `t` a record type.
    unsafe fn leave_fields_unwritten(&mut self, binding: DyadPtr, t: DyadPtr, defaults: bool) {
        let fields = crate::identities::array::items(crate::identities::meta::record_fields_of(t));
        let unwritten: Vec<DyadPtr> = fields
            .iter()
            .copied()
            .filter(|&f| !defaults || crate::identities::hole::default_in(f).is_null())
            .collect();
        if unwritten.is_empty() {
            return;
        }
        if !Binding::has_gate(binding, self.types.unknown) {
            Binding::add_gate(self.rt.store, self.types.array_, binding, self.types.unknown);
        }
        let arr = crate::identities::array::build(self.rt.store, self.types.array_, &unwritten);
        Binding::set_unwritten(binding, arr);
    }

    /// `v.f` where `v` is a name `?` left with fields unwritten: a write of `f` is let
    /// through and noted for the sibling rule, a read of an unwritten `f` refused.
    ///
    /// # Safety
    /// `root` must be null or a binding dyad from the store; `lhs` a reduced dyad.
    pub(super) unsafe fn check_field_unwritten(
        &self,
        root: DyadPtr,
        lhs: DyadPtr,
        name: &str,
        writes: bool,
    ) -> Result<Option<(DyadPtr, DyadPtr)>, ParseError> {
        if root.is_null() {
            return Ok(None);
        }
        let unwritten = Binding::read(root).unwritten;
        if unwritten.is_null() {
            return Ok(None);
        }
        let t = self.types.type_of(lhs);
        let mut scope = ScopeStack::new();
        scope.push(crate::identities::meta::record_scope_of(t));
        let fields = crate::identities::array::items(crate::identities::meta::record_fields_of(t));
        let Some(field) = scope
            .resolve(self.trie, name)
            .ok()
            .and_then(|r| fields.iter().copied().find(|&f| f == r.identity))
        else {
            return Ok(None);
        };
        if writes {
            return Ok(Some((root, field)));
        }
        if crate::identities::array::items(unwritten).contains(&field) {
            return Err(ParseError::Unwritten(Box::new(format!(
                "{}.{name}",
                Binding::spelling(root)
            ))));
        }
        Ok(None)
    }

    /// `name := value`, or `share name := value`: one place for every context
    /// that reaches the line, every call of the function around it and every
    /// pass of the loop around it, its value made once here at the definition;
    /// the line itself then only names it.
    pub(crate) fn construct_decl(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if !self.marked_share(tape) {
            return self.declare_here(tape);
        }
        let saved_init = self.cx.once_at.replace(self.cx.frames.len());
        let saved_runtime_depth = std::mem::replace(&mut self.cx.runtime_depth, 0);
        let built = self.declare_here(tape);
        self.cx.once_at = saved_init;
        self.cx.runtime_depth = saved_runtime_depth;
        if !matches!(built?, Constructed::Placed) {
            return Ok(Constructed::Decline);
        }
        let node = tape.at(0).expect("the declaration was just placed").dyad;
        self.drain()?;
        let mark = self.rt.stack_mark();
        // SAFETY: `node` is the declare node just built; its binding is a binding dyad from the store.
        unsafe {
            self.run_on_pass(node).map_err(ParseError::Run)?;
            let named = self.types.through(crate::identities::declare::binding_of(node));
            crate::identities::declare::set_declared(node, named);
        }
        self.rt.stack_release(mark);
        Ok(Constructed::Placed)
    }

    /// A function body's own lines, not a type's built inside it at run.
    fn in_fn_body(&self) -> bool {
        self.cx.frames.len() > self.cx.held_depth
    }

    /// The bytes a declaration lays out belong to the program frame: outside every
    /// function, in a held `type (…)` body built at run, in a `share` initializer.
    pub(super) fn in_program_frame(&self) -> bool {
        !self.in_fn_body() || self.cx.once_at == Some(self.cx.frames.len())
    }

    /// The name's storage, laid out on the binding itself: `width` bytes of `ty` in the
    /// program frame, or the next bytes of the open function's frame. Returns the binding,
    /// what every reader of the name goes through.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store; `ty` null or a type node.
    pub(super) unsafe fn place_for(
        &mut self,
        binding: DyadPtr,
        ty: DyadPtr,
        width: usize,
    ) -> DyadPtr {
        let (frame, offset) = if self.in_program_frame() {
            (self.types.root_scope, self.rt.store.arena_alloc(width))
        } else {
            let frame = self.cx.frames.last_mut().expect("in_program_frame saw an open function");
            let offset = frame.size;
            frame.size += width;
            (frame.frame, offset)
        };
        Binding::lay_out(binding, ty, frame, offset);
        binding
    }

    /// A type body's own lines, where a line fills a slot.
    pub(crate) fn in_type_body_lines(&self) -> bool {
        self.cx.definitions.last().is_some_and(|def| self.cx.scopes.current() == Some(def.scope))
    }

    /// The gate words left of the name being declared include `share`.
    fn marked_share(&self, tape: &ParsingTape) -> bool {
        let t = self.types;
        let mut k = -2;
        while let Some(cell) = tape.at(k) {
            if cell.constructed {
                return false;
            }
            let id = cell.identity(t);
            if id == t.share_ {
                return true;
            }
            if id != t.mut_ && id != t.pub_ && id != t.immut_ {
                return false;
            }
            k -= 1;
        }
        false
    }

    /// `name := value`: the binding is made before the value parses and points at the
    /// value's node once it exists (a `fn` publishes it before its body). Legal only
    /// opening its expression; anywhere else it declines.
    fn declare_here(&mut self, tape: &mut ParsingTape) -> Result<Constructed, ParseError> {
        // The name is the cell to the left: a spelling, or the node `regex «…»`
        // placed, whose text is a pattern; anything else declines.
        let Some(tok) = tape.at(-1).copied() else {
            return Ok(Constructed::Decline);
        };
        let pattern: Option<Vec<u8>> = if tok.constructed {
            // SAFETY: a constructed cell holds a node from the store.
            if unsafe { dyad::ty(tok.dyad) } == self.types.regex_ {
                // SAFETY: a `regex` node's value is a text blob.
                Some(unsafe { crate::identities::string::text(tok.dyad) }.to_vec())
            } else {
                return Ok(Constructed::Decline);
            }
        } else {
            None
        };
        // Copied out: the cell's text is independent of the `&mut self` the
        // declaration and value parse need.
        let name: String = match &pattern {
            Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            None => tok.spelling().to_string(),
        };
        let name: &str = &name;
        // The binding is the name's home before its node exists (DESIGN ›An unknown spelling
        // stays text on the tape; `:=` makes the binding, and the node comes with the value‹).
        let declared = if pattern.is_some() {
            self.declare_pattern(name, std::ptr::null_mut(), tok.start)
        } else {
            self.declare_name(name, std::ptr::null_mut(), tok.start)
        };
        let binding = match declared {
            Ok(binding) => binding,
            Err(e) => {
                // The stuck point is the name itself (it is what shadows).
                self.cx.pos = tok.start;
                return Err(e);
            }
        };
        // A `fn` literal opening the value points the binding at its node before its body
        // parses; the binding is also the one a `lex_rank = …` line in the value writes.
        self.cx.pending_binding = binding;
        self.cx.filling.push(binding);
        self.cx.filling_at.push(self.cx.pos);
        let value = self.parse_expression_cell();
        self.cx.filling.pop();
        self.cx.filling_at.pop();
        self.cx.last_declared = binding;
        self.cx.pending_binding = std::ptr::null_mut();
        let cell = value?;
        let value = cell.dyad;
        // A bare name as the value is its binding (a use); the declaration inspects
        // the dyad behind it and keeps `value` as what the initializer stores.
        // SAFETY: `value` is a dyad from the store.
        let read = unsafe { self.types.through(value) };
        // A box on the right is decided first; a rational value gets a place of
        // `rational_number`.
        // SAFETY: `read` is a dyad from the store.
        let rational = unsafe { crate::identities::rational::is_rational_value(self.types, read) };
        // SAFETY: `read` is a reduced dyad from the store.
        let box_ty = match unsafe { crate::identities::read::read_kind(self.types, read) } {
            _ if rational => Some(self.types.rational),
            crate::identities::read::Read::Container(t)
                if t == self.types.type_ || t == self.types.dyad_ =>
            {
                Some(t)
            }
            // SAFETY: as above.
            _ if unsafe { dyad::ty(read) } == self.types.tape.cell_value => Some(self.types.type_),
            // SAFETY: as above.
            _ => unsafe { crate::identities::hashmap::box_of(self.types, read) },
        };
        // SAFETY: `binding`, `value` and `read` are dyads from the store.
        let declared = unsafe {
            let empty_node = cell.hole
                && !cell.owning
                && crate::identities::meta::is_node_valued(
                    crate::identities::hole::type_in(value),
                    self.types.fn_type,
                );
            if empty_node {
                // `v := T ?` of a type whose values are nodes: a new empty node each time the
                // declaration runs, its fields filled one by one as a parse fills `tape[0]`.
                let t = crate::identities::hole::type_in(value);
                let template = crate::identities::this::empty_node(self.rt.store, t);
                let node = crate::identities::this::build_copy(self.rt.store, self.types, template);
                let place = self.place_for(binding, t, 8);
                let init = crate::identities::build_init(self.rt.store, self.types, place, node)?;
                let free = crate::identities::meta::instances_free_of(t);
                if !free.is_null() {
                    self.own_node(binding, place, free);
                }
                // A field with a default starts with it, the empty node's slot holding it.
                self.leave_fields_unwritten(binding, t, true);
                init
            } else if cell.hole {
                // `x := i32 ?`: the name gets the hole's type at the type's width; nothing
                // initializes it, and `?`'s entry refuses a read until a sibling write fills
                // it. A hashmap's zeroed place is already its empty map, so nothing is
                // unknown to refuse.
                let t = crate::identities::hole::type_in(value);
                let width = crate::identities::read::place_layout(self.types, t)
                    .map_or(8, |(_, width)| width);
                let place = self.place_for(binding, t, width);
                // `a := own t ?`: the owner of whatever node is written into it later.
                if cell.owning {
                    let free = crate::identities::meta::instances_free_of(t);
                    self.own_node(binding, place, free);
                }
                // `a := own @T ?`: the owner of whatever block is written into it later.
                if crate::identities::drop_model::is_owning_place(self.types, place) {
                    let free_node = crate::identities::drop_model::build_teardown(
                        self.rt.store,
                        self.types,
                        self.types.free_,
                        place,
                    )?;
                    let defer_node = crate::identities::drop_model::build_defer(
                        self.rt.store,
                        self.types,
                        free_node,
                    );
                    let scope = self.teardown_scope();
                    self.cx.open[scope].defers.push(defer_node);
                }
                if !crate::identities::hashmap::is_hashmap(self.types, t) {
                    Binding::add_gate(
                        self.rt.store,
                        self.types.array_,
                        binding,
                        self.types.unknown,
                    );
                    // `T ?` of a record leaves zeroed bytes, not the defaults, so every field
                    // waits for its write.
                    if crate::identities::meta::is_record_type(t) {
                        self.leave_fields_unwritten(binding, t, false);
                    }
                }
                place
            } else if dyad::ty(read) == self.types.construct_
                || dyad::ty(read) == self.types.by_copy.result
            {
                // Both make their value into the name's own bytes: the target slot.
                let t = crate::identities::by_copy::made_type(self.types, read);
                let width = crate::identities::read::place_layout(self.types, t)
                    .map_or(8, |(_, width)| width)
                    .max(1);
                let place = self.place_for(binding, t, width);
                *(dyad::value(read) as *mut DyadPtr) = place;
                value
            } else if crate::identities::read::read_kind(self.types, read)
                == crate::identities::read::Read::Identity
            {
                // A type value: the name becomes another spelling of the type.
                self.cx.scopes.rebind(binding, read);
                read
            } else if let Some(t) = box_ty {
                // A box on the right: reads are copy by default, so `x` gets
                // its own box and a copy of what `a` holds; a rational's is sixteen bytes.
                let width = crate::identities::read::place_layout(self.types, t)
                    .map_or(8, |(_, width)| width);
                let place = self.place_for(binding, t, width);
                crate::identities::build_init(self.rt.store, self.types, place, read)?
            } else if let Some(t) =
                crate::identities::node_type_of(self.types, value).filter(|_| {
                    crate::identities::read::read_kind(self.types, read)
                        != crate::identities::read::Read::Node
                })
            {
                // A node a Logos `parse` builds is held by its address: `b := a` borrows it,
                // and a value just made or moved makes the name its owner.
                let place = self.place_for(binding, t, 8);
                let init = crate::identities::build_init(self.rt.store, self.types, place, value)?;
                let free = crate::identities::meta::instances_free_of(t);
                if !free.is_null() && crate::identities::drop_model::moves_out(self.types, value) {
                    self.own_node(binding, place, free);
                }
                init
            } else if crate::identities::drop_model::is_owning_value(self.types, value) {
                // An owning value lands in a place here, the one site that
                // knows the name it binds, so the teardown attaches here (DESIGN
                // ›Explicit heap‹): an owning `@pointee` place, the value in it, `defer free <place>` in this scope.
                let pointee = crate::identities::drop_model::owning_pointee_of(self.types, value)
                    .expect("is_owning_value implies a pointee");
                let owning_ty = crate::identities::pointer::make_owning_pointer_type(
                    self.rt.store,
                    self.types.type_,
                    pointee,
                    self.types.ops.teardown_,
                );
                // A pointer is 8 bytes (U64-wide), whatever it points at.
                let place = self.place_for(binding, owning_ty, 8);
                let init = crate::identities::build_init(self.rt.store, self.types, place, value)?;
                let free_node = crate::identities::drop_model::build_teardown(
                    self.rt.store,
                    self.types,
                    self.types.free_,
                    place,
                )?;
                let defer_node = crate::identities::drop_model::build_defer(
                    self.rt.store,
                    self.types,
                    free_node,
                );
                let scope = self.teardown_scope();
                self.cx.open[scope].defers.push(defer_node);
                init
            } else if dyad::ty(read) != self.types.rational
                && matches!(
                    crate::identities::numtype_of(self.types, value),
                    crate::identities::Operand::Concrete(_)
                        | crate::identities::Operand::Pointer(_)
                )
            {
                // A runtime numeric or pointer value is snapshotted: fresh
                // per-call storage, the name bound to it, the value kept as a
                // re-runnable initializer, so a read is a plain load and a loop-body local re-initializes on entry.
                let (ty_node, width) =
                    crate::identities::scalar_binding_type(self.rt.store, self.types, value);
                let place = self.place_for(binding, ty_node, width);
                crate::identities::build_init(self.rt.store, self.types, place, value)?
            } else if self.types.frame_of(read).is_some() && dyad::ty(read) == self.types.binding_ {
                // `x := y` over storage no rule above copies: the name is a second name for
                // the same bytes.
                let b = Binding::read(read);
                Binding::lay_out(binding, b.dyad, b.frame, b.offset);
                read
            } else {
                // The name denotes the node the value built, never a copy of it.
                Binding::set_dyad(binding, read);
                read
            }
        };
        let node = crate::identities::declare::build(
            self.rt.store,
            self.types.declare_,
            self.types.ops.declare_,
            binding,
            value,
            declared,
        );
        tape.remove(-1); // the name token, consumed
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// `dyad (T, v)` builds a store-owned cell of type `T` (DESIGN ›Feasibility‹):
    /// a numeric `T` takes a literal `v` committed to its width, any other `T`
    /// takes `v` as the node the value points at. Without a bracket, `dyad` stands as its value.
    pub(crate) fn construct_dyad(
        &mut self,
        id: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let types = self.types;
        let Some(bracket) = self.cell_at(tape, 1)?.filter(|c| c.is_bracket()) else {
            let value = self.stand_as_value(tape, id);
            tape.place(value);
            return Ok(Constructed::Placed);
        };
        // SAFETY: the bracket cell holds a node from the store; its arguments are reduced dyads.
        let cell = unsafe {
            let args = self.args_of(bracket.dyad);
            let [ty, value] = args[..] else {
                return Err(ParseError::CtorArity);
            };
            let ty = types.through(ty);
            if !crate::identities::is_type_value(types, ty) {
                return Err(ParseError::BadDeclaredType);
            }
            let read = types.through(value);
            // What a cell of `ty` holds is the reading rule's answer for a
            // place of `ty`: a scalar is storage at its width.
            let scalar = matches!(
                crate::identities::read::place_layout(types, ty),
                Some((crate::identities::read::Read::Scalar(_), _))
            );
            if scalar && ty != types.bool_ {
                if dyad::ty(read) != types.rational {
                    return Err(ParseError::UnsupportedOperands);
                }
                crate::identities::commit_literal_to(self.rt.store, types, read, ty)?
            } else if scalar {
                // `bool` is physically an i32 0/1 in storage; only `true` and
                // `false` have bits at parse.
                if dyad::ty(read) != types.bool_ {
                    return Err(ParseError::UnsupportedOperands);
                }
                let bits = std::ptr::read_unaligned(dyad::value(read) as *const i32);
                self.rt.store.alloc_blob(types.bool_, &bits.to_ne_bytes())
            } else {
                // No other type has a whole value a cell can hold from a
                // node: refused rather than guessed.
                return Err(ParseError::BadDyadType);
            }
        };
        tape.remove(1);
        tape.place(cell);
        Ok(Constructed::Placed)
    }

    /// `?`'s constructor (DESIGN ›Declarations are immutable by default‹): the
    /// one unknown standing as its value, or with a type to its left that
    /// type's valueless place, the cells to the left consumed.
    pub(crate) fn construct_hole(
        &mut self,
        id: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let types = self.types;
        let base = match tape.at(-1).copied() {
            Some(cell) => {
                // A fresh name to the left is not a type; leave it for the
                // boundary's own report. An operator still waiting for its
                // turn (`x != ?`) is no type either.
                let left = self.cell_identity(&cell);
                let waiting = !cell.constructed
                    && !cell.is_fresh()
                    && self.ctor_of(left).is_some()
                    && self.precedence_of_cell(left) < crate::identities::meta::prec::HOLE;
                if cell.is_fresh() || waiting {
                    None
                } else {
                    // A box the pass has already filled declares with the
                    // type it holds.
                    let d = self.operand_dyad(cell)?;
                    let d = self.settled_type(d);
                    // SAFETY: `d` is a resolved dyad from the store.
                    unsafe {
                        // A place holding a type cannot say what a hole's
                        // layout is (DESIGN ›A type is a comptime value‹).
                        match crate::identities::read::read_kind(types, d) {
                            crate::identities::read::Read::Identity => Some(d),
                            crate::identities::read::Read::Container(t)
                                if t == types.type_ || t == types.dyad_ =>
                            {
                                return Err(ParseError::TypeKnownOnlyAtRun);
                            }
                            _ if crate::identities::yields_type(types, d) => {
                                return Err(ParseError::TypeKnownOnlyAtRun);
                            }
                            _ => None,
                        }
                    }
                }
            }
            None => None,
        };
        let mut hole = false;
        let node = match base {
            None => self.stand_as_value(tape, id),
            Some(t)
                // SAFETY: `t` is a type node from the store.
                if unsafe {
                    crate::identities::meta::is_record_type(t)
                        && !crate::identities::meta::run_body_of(t).is_null()
                } =>
            {
                // A type whose nodes run has no place: the hole is an empty node of it.
                tape.remove(-1);
                // SAFETY: as above.
                unsafe { crate::identities::this::empty_node(self.rt.store, t) }
            }
            Some(t) => {
                // The valueless marker `[T][null]`: the name `:=` gives it is laid out at
                // the type's width by the reading rule, so allocation and read cannot
                // disagree; a `bool` place is refused since its literals cannot yet be stored into one.
                // SAFETY: `t` is a type node from the store.
                if t == types.bool_ {
                    return Err(ParseError::NonNumericDeclaredType);
                }
                // SAFETY: `t` is a type node from the store.
                if unsafe { crate::identities::read::place_layout(types, t) }.is_none()
                    && !self.cx.field_hole
                {
                    return Err(ParseError::NonNumericDeclaredType);
                }
                tape.remove(-1);
                hole = true;
                crate::identities::hole::build(self.rt.store, types.unknown, t, std::ptr::null_mut())
            }
        };
        tape.place(node);
        tape.at_mut(0).expect("placed above").hole = hole;
        Ok(Constructed::Placed)
    }
}
