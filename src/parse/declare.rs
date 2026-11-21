// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Declarations: `:=`, `?`, `dyad`, the binding and its gates, what a write must be
//! granted by, ownership marks, and the unwritten-fields rule.

use super::*;
use crate::dyad;
use crate::identities::drop_model::{teardown_of, teardown_of_type, ExitItem, Teardown};

impl<'a> Parser<'a> {
    /// A gate word marks the declaration that just reduced to its right.
    pub(crate) fn gate_declared(&mut self, gate: DyadPtr) -> Result<(), ParseError> {
        let binding = self.cx.last_declared;
        if binding.is_null() {
            return Err(ParseError::GateNeedsDeclaration);
        }
        self.add_gate(binding, gate)
    }

    /// A write into a name is granted by its own binding, and into a place reached by a path by
    /// every binding along it, `immut` vetoing first; the constructor's fill of its fresh node is
    /// granted unless `immut` vetoes it; a write of a name's `lex_rank` is decided as a write at
    /// its `:gate`; a place reached no such way passes here.
    pub(crate) fn check_write(&self, target: &Cell) -> Result<(), ParseError> {
        use crate::identities::this;
        let node = target.dyad;
        // SAFETY: `target` holds a reduced dyad from the store and a null or `array` path; a
        // fill's binding and an `x:f` read's are binding dyads.
        unsafe {
            if let Some(binding) = self.types.binding_read_of(node) {
                if crate::identities::read::is_lex_rank_field(self.types, node) {
                    return self.check_lex_write(binding);
                }
            }
            if this::is_field_read(self.types, node) && this::is_fill(self.types, node) {
                let binding = this::field_binding_of(self.types, node);
                if Binding::has_gate(binding, self.types.immut_) {
                    return Err(ParseError::Immutable(Box::new(Binding::spelling(binding))));
                }
                return Ok(());
            }
            // A fill of a field of a type stamped as the parse runs: its `immut` is read then.
            if this::is_named(self.types, node) {
                return Ok(());
            }
            if target.path.is_null() && dyad::ty(node) == self.types.binding_ {
                return self.check_gates(&[node]);
            }
        }
        self.check_gates(&self.path_of(target))
    }

    /// A write at a name's lex level, admitted from the scope that declares it and the scopes
    /// below; `mut` and `immut` guard the value and have no part in it. ›Two levels: the binding
    /// is the lex level, the dyad is the value level‹ [[R284]]
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store.
    unsafe fn check_lex_write(&self, binding: DyadPtr) -> Result<(), ParseError> {
        if self.cx.scopes.is_open(Binding::read(binding).scope) {
            Ok(())
        } else {
            Err(ParseError::LexOutsideScope(Box::new(Binding::spelling(binding))))
        }
    }

    /// `value`, written into `place`: where `place` is a name whose type its declaring line gives
    /// when it runs, `value` landed in the type of the node it holds now, as into a place
    /// declared with that type (›A type known only when the program runs still works; what needs
    /// it is parsed when its line runs‹ [[R377]]); any other value as it is. The pass knows no
    /// such type, so there the write is the checked error: its line waits for the run.
    ///
    /// # Safety
    /// `place` and `value` must be reduced dyads from the store.
    pub(crate) unsafe fn land_in_run_type(
        &mut self,
        place: DyadPtr,
        value: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        if place.is_null()
            || dyad::ty(place) != types.binding_
            || !Binding::has_gate(place, types.waiting.line)
        {
            return Ok(value);
        }
        let held = self.settled_type(place);
        if held == place {
            return Err(ParseError::TypeKnownOnlyAtRun);
        }
        crate::identities::land(self.rt.store, types, value, dyad::ty(held))
    }

    /// The bindings a write into the cell's node must be granted by: the cell's path, or
    /// for a field read outside the type's own parse the field's binding and those of the fields
    /// it is read through, `p.x` both `p`'s and `x`'s.
    pub(super) fn path_of(&self, cell: &Cell) -> Vec<DyadPtr> {
        use crate::identities::this;
        // SAFETY: a cell's path is null or an `array` node, its dyad null or a dyad from the
        // store, and a field read's binding operand null or a binding dyad.
        unsafe {
            if !cell.path.is_null() {
                return crate::identities::array::items(cell.path).to_vec();
            }
            let node = cell.dyad;
            if node.is_null()
                || !this::is_field_read(self.types, node)
                || this::is_fill(self.types, node)
            {
                return Vec::new();
            }
            let mut path = Vec::new();
            let mut at = node;
            while let Some((into, binding)) = this::field_step(self.types, at) {
                if !binding.is_null() {
                    path.push(binding);
                }
                at = into;
            }
            path.reverse();
            path
        }
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

    /// A `static` function that writes a field of its value meets the gates `receiver.f = …`
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

    /// `=` over a field of the value a `static` function is reading.
    ///
    /// # Safety
    /// `target` must be a reduced dyad from the store.
    pub(crate) unsafe fn note_receiver_write(&mut self, target: DyadPtr) {
        let target = self.types.through(target);
        if !crate::identities::this::is_field_read(self.types, target) {
            return;
        }
        // A field reached through another, `p.x`, lies in the value too.
        let mut this = target;
        while let Some((into, _)) = crate::identities::this::field_step(self.types, this) {
            this = into;
        }
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
        self.declare_spelling(name, false, identity, at)
    }

    /// `declare_name` for a spelling that is a name or, with `pattern`, a pattern.
    pub(crate) fn declare_spelling(
        &mut self,
        spelling: &str,
        pattern: bool,
        identity: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let scope = self.cx.scopes.current().expect("declare needs an open scope");
        let binding = self.mint_binding(identity, scope, spelling.as_bytes());
        // SAFETY: `binding` was minted by `Binding::alloc` just above.
        unsafe { Binding::set_pattern(binding, pattern) };
        self.enter_binding(spelling, binding, at)
    }

    /// Enter a minted binding under its spelling in the open scope; a failure is reported
    /// at `at`.
    pub(crate) fn enter_binding(
        &mut self,
        spelling: &str,
        binding: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let scope = self.cx.scopes.current().expect("declare needs an open scope");
        self.enter_binding_in(scope, spelling, binding, at)
    }

    /// `enter_binding` into `scope`, an open scope that need not be the innermost.
    pub(super) fn enter_binding_in(
        &mut self,
        scope: DyadPtr,
        spelling: &str,
        binding: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        // SAFETY: the caller hands a binding dyad from the store.
        if let Err(e) = unsafe { self.cx.scopes.declare_in(self.trie, spelling, binding, scope) } {
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

    /// Whether `d` is a read of a field that owns what it holds (`copy::field_needs_owner`), for
    /// `=`, `move` and `free` to consult.
    pub(crate) fn is_owning_read(&self, d: DyadPtr) -> bool {
        use crate::identities::{copy, this};
        // SAFETY: `d` is null or a reduced dyad from the store; a field's binding is null or a
        // binding dyad.
        unsafe {
            !d.is_null()
                && this::field_step(self.types, d)
                    .is_some_and(|(_, b)| !b.is_null() && copy::field_needs_owner(self.types, b))
        }
    }

    /// Whether the name owns the node it holds: its binding carries `own`.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store.
    pub(crate) unsafe fn owns_node(&self, binding: DyadPtr) -> bool {
        Binding::has_gate(binding, self.types.own_)
    }

    /// Whether the binding site made the name an owner: of a block, its place an owning
    /// pointer's, of a node, its binding carrying `own`, or of what its `dyad ?` place holds.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store.
    pub(crate) unsafe fn name_holds(&self, binding: DyadPtr) -> bool {
        use crate::identities::drop_model::{holds_dyad, is_owning_place};
        is_owning_place(self.types, binding)
            || self.owns_node(binding)
            || holds_dyad(self.types, binding)
    }

    /// The binding site of a node whose type fills the instances' `free`: the name owns it
    /// and its scope holds it.
    ///
    /// # Safety
    /// `binding` must be the binding dyad being declared, laid out as a node's place.
    unsafe fn hold_node(&mut self, binding: DyadPtr) {
        Binding::add_gate(self.rt.store, self.types.array_, binding, self.types.own_);
        self.hold(binding);
    }

    /// The binding site made `binding` an owner: the scope that holds it runs its value's
    /// teardown at its end (DESIGN ›Holding is decided at the binding site, parameters
    /// included‹). That scope is the name's own, or the program for a `static` place, which
    /// lives as long as the program.
    pub(super) fn hold(&mut self, binding: DyadPtr) {
        let at =
            if self.cx.once_at == Some(self.cx.frames.len()) { 0 } else { self.cx.open.len() - 1 };
        let open = &mut self.cx.open[at];
        open.exit.push(ExitItem { what: binding, from: open.lines + 1, end: None });
    }

    /// The open scope whose end holds `place`, innermost first.
    pub(super) fn holder_of(&self, place: DyadPtr) -> Option<usize> {
        // SAFETY: held places and `place` are dyads from the store.
        self.cx.open.iter().rposition(|s| s.exit.iter().any(|h| unsafe { self.holds(h, place) }))
    }

    /// Whether the exit item holds `place` still: the same name, or a name that holds over the
    /// same bytes, as an imported `pub` name over the file's, or the same entry of a `{ … }`
    /// value. A borrow over the same bytes, a plain record's second name, is another name.
    ///
    /// # Safety
    /// `place` must be a dyad from the store.
    pub(super) unsafe fn holds(&self, item: &ExitItem, place: DyadPtr) -> bool {
        use crate::identities::drop_model::same_place;
        let types = self.types;
        let same_entry = || {
            dyad::ty(place) == types.field_
                && dyad::ty(item.what) == types.field_
                && same_place(types, item.what, place)
                && types.type_of(item.what) == types.type_of(place)
        };
        item.end.is_none()
            && (item.what == place
                || dyad::ty(place) == types.binding_
                    && self.name_holds(place)
                    && same_place(types, item.what, place)
                || same_entry())
    }

    /// `node` has emptied `ended`'s place: its name is dead from here on (DESIGN ›`move` and
    /// `free` are static: the parse marks the name dead‹). Each scope from the one that held it
    /// inward settles, when its line closes, that it holds the name up to that line, and an
    /// `if` reads which of its arms ended it.
    pub(crate) fn mark_dead(&mut self, ended: Ended, node: DyadPtr) {
        if self.cx.dropping > 0 {
            return;
        }
        if !ended.is_name() {
            // SAFETY: `ended` holds the name's binding and the field bindings its path reads.
            unsafe {
                self.end_held_path(&ended);
                self.end_path(ended, node);
            }
            return;
        }
        // SAFETY: `ended.binding` is the binding the resolver returned for the operand.
        unsafe { self.cx.scopes.mark_dead(ended.binding, node) };
        // A borrow ends its name alone.
        // SAFETY: as above.
        if unsafe { self.name_holds(ended.binding) } {
            self.note_ended(ended.binding);
        }
    }

    /// The held `place` ended here: each scope from the one that holds it inward settles that
    /// at its line's close, and the log tells an `if` which arm ended it. Two arms of an `if`
    /// may each end it, so each line notes it once.
    fn note_ended(&mut self, place: DyadPtr) {
        let note = |list: &mut Vec<DyadPtr>| {
            if !list.contains(&place) {
                list.push(place);
            }
        };
        let holder = self.holder_of(place).unwrap_or_else(|| {
            // An owner no open scope holds is an earlier REPL line's, whose items the
            // session's scope keeps: held from outside everything this line opened.
            note(&mut self.cx.open[0].ended_earlier);
            0
        });
        for open in &mut self.cx.open[holder..] {
            note(&mut open.ended);
        }
        self.cx.ended_log.push((place, holder));
    }

    /// The field path `ended` reaches an entry of a `{ … }` value an open scope holds: that entry
    /// ends here as a held name does, so the value's end ends only its other entries (›`move` and
    /// `free` take a field path too‹ [[R75]]). Nothing for any other path.
    ///
    /// # Safety
    /// `ended.binding` must be a name's binding dyad and `ended.path` field bindings.
    unsafe fn end_held_path(&mut self, ended: &Ended) {
        use crate::identities::{drop_model, instance};
        if !self.name_holds(ended.binding) {
            return;
        }
        let mut place = ended.binding;
        for &field in &ended.path {
            if !drop_model::ends_entry(self.types, place, field) {
                return;
            }
            place = match self.held_entry(place, field) {
                Some(entry) => entry,
                // Held by an earlier REPL line: the session splits its hold when the line ends.
                None => instance::build_field(self.rt.store, self.types, place, field),
            };
        }
        self.note_ended(place);
    }

    /// The place of entry `field` of `place` as an open scope holds it: the hold of a `{ … }`
    /// value held whole first becomes one hold per entry with an end, in written order where the
    /// value's stood, so the scope's end still ends them last declared first, and a `return`
    /// that would end the value ends those entries. `None` where no open scope holds the entry.
    ///
    /// # Safety
    /// `place` must be a name's binding or an entry place this returned, `field` an entry of its
    /// value with an end ([`crate::identities::drop_model::ends_entry`]).
    unsafe fn held_entry(&mut self, place: DyadPtr, field: DyadPtr) -> Option<DyadPtr> {
        use crate::identities::{drop_model, instance};
        let types = self.types;
        let is_entry = |what: DyadPtr| {
            // SAFETY: an exit item holds a binding, a `defer` or a `field` place.
            dyad::ty(what) == types.field_
                && unsafe { instance::field_parts(what) } == (place, field)
        };
        let split = self.cx.open.iter().flat_map(|s| &s.exit);
        if let Some(item) = split.filter(|h| h.end.is_none()).find(|h| is_entry(h.what)) {
            return Some(item.what);
        }
        let holder = self.holder_of(place)?;
        let at = self.cx.open[holder].exit.iter().position(|h| self.holds(h, place))?;
        let whole = self.cx.open[holder].exit[at];
        let entries = drop_model::held_entries(self.rt.store, types, place)?;
        let items = entries.iter().map(|&what| ExitItem { what, from: whole.from, end: None });
        self.cx.open[holder].exit.splice(at..=at, items);
        for r in self.cx.frames.iter_mut().flat_map(|f| &mut f.returns) {
            if let Some(k) = r.live.iter().position(|&n| n == whole.what) {
                r.live.splice(k..=k, entries.iter().copied());
            }
        }
        entries.into_iter().find(|&e| instance::field_parts(e).1 == field)
    }

    /// `node` has ended the field path `ended`: each path binding along it under the name's
    /// entry, made on the first end that reaches it, and the last is dead from here, so the name
    /// and every path above the last are dead as a whole (DESIGN ›`move` and `free` take a field
    /// path too‹).
    ///
    /// # Safety
    /// `ended.binding` must be a name's binding dyad and `ended.path` field bindings; `node` a
    /// `move` or `free` node over the path's place.
    unsafe fn end_path(&mut self, ended: Ended, node: DyadPtr) {
        let name = Binding::read(ended.binding);
        let mut path = ended.binding;
        for &field in &ended.path {
            let identity = Binding::read(field).dyad;
            let mut below = Binding::path(path, identity);
            if below.is_null() {
                let spelled = format!("{}.{}", Binding::spelling(path), Binding::spelling(field));
                below = self.mint_binding(identity, name.scope, spelled.as_bytes());
                Binding::set_start(below, name.start);
                Binding::add_path(self.rt.store, self.types.array_, path, below);
            }
            path = below;
        }
        self.cx.scopes.mark_dead(path, node);
    }

    /// The path binding a field path `lhs` from the name `root` reaches under its entry, null
    /// where it is no field path or no `move` or `free` reached it.
    ///
    /// # Safety
    /// `lhs` must be a reduced dyad from the store; `root` null or a binding dyad.
    unsafe fn reached_path(&self, lhs: DyadPtr, root: DyadPtr) -> DyadPtr {
        use crate::identities::this::{field_path, FieldPath};
        let held = self.settled_type(root);
        let FieldPath::From(name, fields) = field_path(self.types, lhs, root, held) else {
            return std::ptr::null_mut();
        };
        let mut path = name;
        for field in fields {
            if path.is_null() {
                break;
            }
            path = Binding::path(path, Binding::read(field).dyad);
        }
        path
    }

    /// `?`'s entry on a binding refuses every read of the value until it is
    /// written: the target of `x = …` passes, and so does `x:…`, which reads
    /// the binding (DESIGN ›Declarations are immutable by default‹). A name with an ended path
    /// is dead as a whole: only `x.…` and `x:…` pass (DESIGN ›`move` and `free` take a field
    /// path too‹).
    pub(super) fn check_unwritten(&mut self, cell: &Cell) -> Result<(), ParseError> {
        self.check_read(cell.binding(self.types), cell.start)
    }

    /// [`Self::check_unwritten`] of a use of `binding` written at `at`, the cursor after it.
    fn check_read(&mut self, binding: DyadPtr, at: usize) -> Result<(), ParseError> {
        if binding.is_null() {
            return Ok(());
        }
        let types = self.types;
        let next = self.next_identity();
        let next_is = |id: DyadPtr| next == Some(id);
        // SAFETY: a non-null cell binding is a binding dyad from the store.
        if unsafe { Binding::has_ended_path(binding) }
            && !next_is(types.dot_)
            && !next_is(types.colon_)
        {
            self.cx.pos = at;
            // SAFETY: as above.
            let name = unsafe { Binding::spelling(binding) };
            return Err(ParseError::Resolve(ResolveError::Dead(name)));
        }
        // SAFETY: as above.
        if !unsafe { Binding::has_gate(binding, types.unknown) }
            || next_is(types.assign)
            || next_is(types.colon_)
        {
            return Ok(());
        }
        // `v.f`: the field read or write decides, one field at a time.
        // SAFETY: as above.
        if !unsafe { Binding::read(binding) }.unwritten.is_null() && next_is(types.dot_) {
            return Ok(());
        }
        self.cx.pos = at;
        // SAFETY: as above.
        Err(ParseError::Unwritten(Box::new(unsafe { Binding::spelling(binding) })))
    }

    /// A use of `binding` in a line held for the run, written at `at` with the cursor after it,
    /// and a field `v.f` read after it: checked where the line is written as the lexer and `.`
    /// check them, since a line that writes after it changes the binding before the held line
    /// runs (›A binding's fields are read at elaboration, at the line of the read; running code
    /// never reads a binding‹ [[R286]]).
    pub(super) fn check_held_read(
        &mut self,
        binding: DyadPtr,
        at: usize,
    ) -> Result<(), ParseError> {
        self.check_read(binding, at)?;
        if !self.consume_token(self.types.dot_) {
            return Ok(());
        }
        let Some((start, len)) = self.lex_spelling() else { return Ok(()) };
        let source = self.cx.source;
        let next = self.next_identity();
        let (writes, dotted) = (next == Some(self.types.assign), next == Some(self.types.dot_));
        // SAFETY: `binding` is a binding dyad from the store, which `.` reads as its own place.
        unsafe {
            self.check_name_field(
                binding,
                std::ptr::null_mut(),
                binding,
                &source[start..start + len],
                writes,
                dotted,
            )
        }
        .map(|_| ())
        .inspect_err(|_| self.cx.pos = start)
    }

    /// The `=` just built, or the line just held whose `=` writes when it runs, with the place it
    /// writes as `=` read it: the item the sibling rule reads.
    pub(crate) fn note_write(&mut self, node: DyadPtr, place: &Cell) {
        let ids = &self.types.this;
        // SAFETY: `node` is the `=` node or the held line just built.
        let ty = unsafe { dyad::ty(node) };
        if ty == self.types.tape.write {
            self.forget_narrowed();
        } else if ty == ids.write || ty == ids.store {
            // SAFETY: a field write keeps the value and the field's index first.
            let key = unsafe { crate::identities::this::field_key(self.types, node) };
            for s in &mut self.cx.open {
                s.narrowed.retain(|n| Some(n.0) != key);
            }
        }
        let scope = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        self.cx.last_write = LastWrite { node, scope, place: place.dyad, fill: place.target };
    }

    /// The type a check has narrowed the cell, line or field to, the innermost check first.
    pub(super) fn narrowed_to(&self, key: CellKey) -> Option<DyadPtr> {
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

    /// A statement `x = …` in the block that declared `x := T ?`, built or held for the run,
    /// lifts `?`'s read veto from that line on; a write nested in a group, an `if`, a loop or a
    /// `fn` body is no statement of that block, so it does not.
    ///
    /// # Safety
    /// `item` must be a reduced dyad from the store.
    pub unsafe fn fill_if_sibling_write(&mut self, item: DyadPtr) {
        let LastWrite { node, scope, place, fill } = self.cx.last_write;
        if item.is_null() || item != node {
            return;
        }
        if !fill.0.is_null() {
            let (binding, field) = fill;
            let rec = Binding::read(binding);
            if rec.scope != scope || rec.unwritten.is_null() {
                return;
            }
            // A union is one place: a write of any member fills it.
            let whole = crate::identities::union::is_union(self.types, Binding::read(field).frame);
            let field = Binding::read(field).dyad;
            let left: Vec<DyadPtr> = crate::identities::array::items(rec.unwritten)
                .iter()
                .copied()
                .filter(|&f| !whole && f != field)
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
        if place.is_null()
            || dyad::ty(place) != self.types.binding_
            || !Binding::has_gate(place, self.types.unknown)
            || Binding::read(place).scope != scope
        {
            return;
        }
        Binding::remove_gate(self.rt.store, self.types.array_, place, self.types.unknown);
        Binding::set_unwritten(place, std::ptr::null_mut());
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

    /// `v.f` where `v` is a name, `root`, or a field path from the name `chain`: the name and
    /// the field's binding it reaches when `v` is a name, nulls otherwise. A read of a field
    /// `v := T ?` left unwritten is refused, a write let through. Where a path under `v` ended,
    /// `v.f` is refused if `f`'s did, and so is `v.f` read whole, with no `.` after it
    /// (`dotted`), if a path under it did; a member that is no field, which takes `v` whole, is
    /// refused too.
    ///
    /// # Safety
    /// `root` and `chain` must be null or binding dyads from the store; `lhs` a reduced dyad.
    pub(super) unsafe fn check_name_field(
        &self,
        root: DyadPtr,
        chain: DyadPtr,
        lhs: DyadPtr,
        name: &str,
        writes: bool,
        dotted: bool,
    ) -> Result<(DyadPtr, DyadPtr), ParseError> {
        let none = (std::ptr::null_mut(), std::ptr::null_mut());
        let parent = if root.is_null() { self.reached_path(lhs, chain) } else { root };
        if parent.is_null() {
            return Ok(none);
        }
        let t = if root.is_null() {
            crate::identities::read::output_type(self.types, lhs)
        } else {
            self.types.type_of(lhs)
        };
        let field = if !crate::identities::meta::is_record_type(t) {
            None
        } else {
            let mut scope = ScopeStack::new();
            scope.push(crate::identities::meta::record_scope_of(t));
            let fields =
                crate::identities::array::items(crate::identities::meta::record_fields_of(t));
            scope.resolve(self.trie, name).ok().filter(|r| fields.contains(&r.identity))
        };
        if Binding::has_ended_path(parent) {
            let dead = match &field {
                None => Some(Binding::spelling(parent)),
                Some(r) => {
                    let path = Binding::path(parent, r.identity);
                    let ended = !path.is_null()
                        && (Binding::read(path).is_dead()
                            || !dotted && Binding::has_ended_path(path));
                    ended.then(|| Binding::spelling(path))
                }
            };
            if let Some(dead) = dead {
                return Err(ParseError::Resolve(ResolveError::Dead(dead)));
            }
        }
        let Some(field) = field.filter(|_| !root.is_null()) else {
            return Ok(none);
        };
        let unwritten = Binding::read(root).unwritten;
        if !writes
            && !unwritten.is_null()
            && crate::identities::array::items(unwritten).contains(&field.identity)
        {
            return Err(ParseError::Unwritten(Box::new(format!(
                "{}.{name}",
                Binding::spelling(root)
            ))));
        }
        Ok((root, field.binding))
    }

    /// `name := value`, or `static name := value`: one place for every context
    /// that reaches the line, every call of the function around it and every
    /// pass of the loop around it, its value made once here at the definition;
    /// the line itself then only names it.
    pub(crate) fn construct_decl(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if !self.marked_static(tape) {
            return self.declare_here(tape);
        }
        let built = self.parse_in_pass(|p| p.declare_here(tape));
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
    /// function, in a held `type (…)` body built at run, in a `static` initializer.
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

    /// The gate words left of the name being declared include `static`.
    fn marked_static(&self, tape: &ParsingTape) -> bool {
        let t = self.types;
        let mut k = -2;
        while let Some(cell) = tape.at(k) {
            if cell.constructed {
                return false;
            }
            let id = cell.identity(t);
            if id == t.static_ {
                return true;
            }
            if !self.is_gate_word(id) {
                return false;
            }
            k -= 1;
        }
        false
    }

    /// A word written left of the name a `:=` declares.
    pub(super) fn is_gate_word(&self, id: DyadPtr) -> bool {
        let t = self.types;
        [t.static_, t.mut_, t.pub_, t.immut_].contains(&id)
    }

    /// `name := value`: the binding is made before the value parses and points at the
    /// value's node once it exists (a `fn` publishes it before its body). Legal only
    /// opening its expression; anywhere else it declines.
    fn declare_here(&mut self, tape: &mut ParsingTape) -> Result<Constructed, ParseError> {
        // The name is the cell to the left: a spelling, the node `regex «…»` placed, whose
        // text is a pattern, or the node `lex «…»` placed, whose whole text is one spelling
        // (DESIGN ›A pattern identity is declared `regex «…» := type (…)`‹); anything else
        // declines.
        let Some(tok) = tape.at(-1).copied() else {
            return Ok(Constructed::Decline);
        };
        // Copied out: the cell's text is independent of the `&mut self` the
        // declaration and value parse need.
        let (name, pattern): (String, bool) = if tok.constructed {
            // SAFETY: a constructed cell holds a node from the store; a `regex` node's value
            // is a text blob.
            let quoted = unsafe {
                if dyad::ty(tok.dyad) == self.types.regex_ {
                    Some((crate::identities::string::text(tok.dyad), true))
                } else {
                    crate::identities::lex::quoted(self.types, tok.dyad).map(|text| (text, false))
                }
            };
            let Some((text, pattern)) = quoted else {
                return Ok(Constructed::Decline);
            };
            (String::from_utf8_lossy(text).into_owned(), pattern)
        } else {
            (tok.spelling().to_string(), false)
        };
        if name.is_empty() {
            self.cx.pos = tok.start;
            return Err(ParseError::EmptySpelling);
        }
        let name: &str = &name;
        // The binding is the name's home before its node exists (DESIGN ›An unknown spelling is
        // a node of type `unknown_name`; `:=` makes the binding, and the node comes with the
        // value‹ [[R251]]).
        let declared = self.declare_spelling(name, pattern, std::ptr::null_mut(), tok.start);
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
        let (mut typed_at_run, mut unwritten) = (false, false);
        let value = self.waiting_value().and_then(|held| match held {
            Some((held, hole)) => {
                (typed_at_run, unwritten) = (true, hole);
                Ok(held)
            }
            None => self.parse_expression_cell(),
        });
        self.cx.filling.pop();
        self.cx.filling_at.pop();
        self.cx.last_declared = binding;
        self.cx.pending_binding = std::ptr::null_mut();
        let cell = if typed_at_run { value? } else { self.declared_value(value?)? };
        let value = cell.dyad;
        if self.is_bare_unknown(&cell) {
            return Err(self.untyped(tok.start, false));
        }
        // SAFETY: `binding` is the binding just declared; `value` a reduced dyad from the store.
        let declared = unsafe { self.land_at_binding(binding, value, cell.hole, cell.owning) }?;
        // A place whose type its line gives when it runs keeps that type, which a write converts
        // into ([`Self::land_in_run_type`]); one laid out then holds no value until a write fills
        // it, as one declared `T ?` does.
        // SAFETY: `binding` is the binding just declared.
        unsafe {
            let (store, array) = (&mut *self.rt.store, self.types.array_);
            if typed_at_run {
                Binding::add_gate(store, array, binding, self.types.waiting.line);
            }
            if unwritten {
                Binding::add_gate(store, array, binding, self.types.unknown);
            }
        }
        let node =
            crate::identities::declare::build(self.rt.store, self.types, binding, value, declared);
        tape.remove(-1); // the name token, consumed
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// What `:=` makes of its right side, read as a type and the value after it, for a name, a
    /// field and a parameter alike (›`?` is one value, the unknown; `key := T ?` declares without
    /// a value‹ [[R103]]): a type with nothing after it gives the place `T ?` makes of it, a type
    /// a line computes too; a value stands as it is, the one `type` made of what stood after it
    /// (`type i32`, `type (…)`) among them.
    pub(super) fn declared_value(&mut self, cell: Cell) -> Result<Cell, ParseError> {
        if cell.hole || cell.type_value {
            return Ok(cell);
        }
        let place = self.empty_place_of(cell.dyad).map_err(|e| self.fail_at(cell.start, e))?;
        let Some(place) = place else { return Ok(cell) };
        let mut cell = cell;
        cell.dyad = place;
        cell.hole = true;
        cell.constructed = true;
        Ok(cell)
    }

    /// The place `T ?` makes of the type `value` gives, read as `?` reads the type to its left;
    /// `None` for a value whose own type is not `type`, and for `?` itself.
    pub(super) fn empty_place_of(&mut self, value: DyadPtr) -> Result<Option<DyadPtr>, ParseError> {
        let types = self.types;
        // SAFETY: a parsed value is null or a reduced dyad from the store.
        let gives_type = unsafe {
            !value.is_null()
                && types.through(value) != types.unknown
                && crate::identities::read::output_type(types, value) == types.type_
        };
        if !gives_type {
            return Ok(None);
        }
        let t = self.type_known_now(value)?;
        self.valueless_place(t).map(Some)
    }

    /// `type T`: the type `T` as a value of `type`, as `i32 1` is a value of `i32`, so a name
    /// given it holds `T` itself (›The ground is one identity, spelled `type`; `logos` is a
    /// language‹ [[R134]]). `T` is the whole right side, a type a line computes too:
    /// `type array i32`, `type pick(1)`. A box the pass has filled gives what it holds; a type
    /// known only when the program runs stays the value it is, which lands in a `type` place.
    /// `type ?` is the valueless `type` place, and with nothing to its right `type` stands as
    /// itself.
    pub(crate) fn construct_type_value(
        &mut self,
        id: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if !self.finds_right_side(tape) {
            return Ok(Constructed::Decline);
        }
        let Some(operand) = self.value_after(tape)? else {
            return Ok(Constructed::Decline);
        };
        if self.is_bare_unknown(&operand) {
            let place = self.valueless_place(id)?;
            tape.place(place);
            tape.at_mut(0).expect("placed above").hole = true;
            return Ok(Constructed::Placed);
        }
        let value = self.as_operand(operand)?;
        let types = self.types;
        // SAFETY: `value` is a reduced dyad from the store, and `type` a type node.
        let held = unsafe {
            crate::identities::land(self.rt.store, types, value, types.type_)
                .map(|landed| {
                    let read = self.settled_type(types.through(landed));
                    crate::identities::type_identity_of(types, read).unwrap_or(landed)
                })
                .map_err(|e| self.fail_at(operand.start, e))?
        };
        tape.place_type_value(held);
        Ok(Constructed::Placed)
    }

    /// The value after the type at the center, taken off the source: the rest of its expression,
    /// read as `:=` reads its own right side. Inside a read that ends at a bracket, a condition
    /// or a range, it is the one cell before that bracket, which only the read around it can
    /// tell from the value's own.
    fn value_after(&mut self, tape: &mut ParsingTape) -> Result<Option<Cell>, ParseError> {
        if self.cx.lex_mode.is_none() {
            return self.parse_expression_cell().map(Some);
        }
        let cell = self.cell_at(tape, 1)?;
        if cell.is_some() {
            tape.remove(1);
        }
        Ok(cell)
    }

    /// A declaration's right side is `?` alone, with no type to its left.
    pub(super) fn is_bare_unknown(&self, cell: &Cell) -> bool {
        // SAFETY: a parsed cell holds a reduced dyad from the store.
        !cell.hole && unsafe { self.types.through(cell.dyad) } == self.types.unknown
    }

    /// A new name, field or parameter written without its type, the checked error where it is
    /// written (DESIGN ›`?` is one value, the unknown; `key := T ?` declares without a value‹
    /// [[R103]]); a field's error also names a function that builds the mints.
    pub(super) fn untyped(&mut self, at: usize, field: bool) -> ParseError {
        self.cx.pos = at;
        if field {
            ParseError::FieldNeedsType
        } else {
            ParseError::NameNeedsType
        }
    }

    /// Where `value` lands for `binding`, and whether the open scope holds it: the binding
    /// site's one decision, which `:=` and `&` of a value nothing names both ask (DESIGN
    /// ›Holding is decided at the binding site, parameters included‹ [[R61]]). Returns what the declare
    /// node runs.
    ///
    /// # Safety
    /// `binding` must be the binding being declared; `value` a reduced dyad from the store.
    pub(super) unsafe fn land_at_binding(
        &mut self,
        binding: DyadPtr,
        value: DyadPtr,
        hole: bool,
        owning: bool,
    ) -> Result<DyadPtr, ParseError> {
        let value = if hole {
            value
        } else {
            crate::identities::read::value_type(self.types, value)?;
            crate::identities::copy::taken(self.rt.store, self.types, value)
        };
        // A bare name as the value is its binding (a use); the declaration inspects
        // the dyad behind it and keeps `value` as what the initializer stores.
        let read = self.types.through(value);
        let declared = if hole {
            crate::identities::hole::declared_type(self.types.unknown, value)
        } else {
            std::ptr::null_mut()
        };
        Ok({
            let empty_node = hole
                && !owning
                && crate::identities::meta::is_node_valued(declared, self.types.fn_type);
            if empty_node {
                // `v := T ?` of a type whose values are nodes: a new empty node each time the
                // declaration runs, its fields filled one by one as a parse fills `tape[0]`.
                let t = declared;
                let template = crate::identities::this::empty_node(self.rt.store, t);
                let node = crate::identities::this::build_copy(self.rt.store, self.types, template);
                let place = self.place_for(binding, t, 8);
                let init = crate::identities::build_init(self.rt.store, self.types, place, node)?;
                if teardown_of_type(self.types, t) != Teardown::Nothing {
                    self.hold_node(binding);
                }
                // A field with a default starts with it, the empty node's slot holding it.
                self.leave_fields_unwritten(binding, t, true);
                init
            } else if hole {
                // `x := i32 ?`: the name gets the hole's type at the type's width; nothing
                // initializes it, and `?`'s entry refuses a read until a sibling write fills
                // it. A hashmap's zeroed place is already its empty map, so nothing is
                // unknown to refuse. A type whose nodes run has no value until one is written,
                // so its empty node runs nothing here.
                let t = declared;
                let width = crate::identities::by_copy::place_width(self.types, t);
                let place = self.place_for(binding, t, width);
                // Whatever is written into it later is the name's to end: `a := own t ?`, `a := r ?`
                // of a plain record whose body fills `free`, `a := own @T ?` and `d := dyad ?`.
                match teardown_of_type(self.types, t) {
                    t if t.of_record() => self.hold_node(binding),
                    Teardown::Nothing => {}
                    _ => self.hold(binding),
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
                // Both make their value into the name's own bytes: the target slot. A record
                // just made or moved out, of a type whose body fills `free` or a `{ … }` whose
                // entries have ends, makes the name its owner.
                let t = crate::identities::read::output_type(self.types, read);
                let width = crate::identities::by_copy::place_width(self.types, t);
                let place = self.place_for(binding, t, width);
                *(dyad::value(read) as *mut DyadPtr) = place;
                if teardown_of(self.types, value).of_record() {
                    self.hold_node(binding);
                    self.take(value);
                }
                value
            } else if crate::identities::read::read_kind(self.types, read)
                == crate::identities::read::Read::Identity
            {
                // A type held, `type i32`: the name is another spelling of it.
                self.cx.scopes.rebind(binding, read);
                read
            } else if let Some(t) = crate::identities::node_output(self.types, value) {
                if crate::identities::read::read_kind(self.types, read)
                    == crate::identities::read::Read::Node
                {
                    // The node the parse already built: the name denotes it.
                    Binding::set_dyad(binding, read);
                    read
                } else {
                    // A node a Logos `parse` builds is held by its address; a value just made,
                    // moved or copied makes the name its owner.
                    let place = self.place_for(binding, t, 8);
                    let init =
                        crate::identities::build_init(self.rt.store, self.types, place, value)?;
                    if let Teardown::Node(_) = teardown_of(self.types, value) {
                        self.hold_node(binding);
                        self.take(value);
                    }
                    init
                }
            } else if let Teardown::Block(pointee) = teardown_of(self.types, value) {
                // An owning value lands in a place of an owning `@pointee`, which the scope holds.
                let owning_ty = crate::identities::pointer::make_owning_pointer_type(
                    self.rt.store,
                    self.types.type_,
                    pointee,
                    self.types.ops.teardown_,
                );
                // A pointer is 8 bytes (U64-wide), whatever it points at.
                let place = self.place_for(binding, owning_ty, 8);
                let init = crate::identities::build_init(self.rt.store, self.types, place, value)?;
                self.hold(binding);
                self.take(value);
                init
            } else if let Some((ty_node, width)) =
                crate::identities::copy_place(self.rt.store, self.types, value)
            {
                // A run-time value is a copy (DESIGN ›`:=` decides nothing about copy or
                // reference; the type's reading rule does‹): fresh per-call storage of its type,
                // the name bound to it, the value kept as a re-runnable initializer, so a read is
                // a plain load and a loop-body local re-initializes on entry. A value just made
                // or moved makes the name its owner, as above.
                let place = self.place_for(binding, ty_node, width);
                let init = crate::identities::build_init(self.rt.store, self.types, place, value)?;
                match teardown_of(self.types, value) {
                    t if t.of_record() => {
                        self.hold_node(binding);
                        self.take(value);
                    }
                    Teardown::Dyad => {
                        self.hold(binding);
                        self.take(value);
                    }
                    // A `dyad` borrowed from another place lands as a copy of its own.
                    _ if ty_node == self.types.dyad_ => self.hold(binding),
                    _ => {}
                }
                init
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
        })
    }

    /// `dyad (T, v)`: `v` lands in `T` where the line is written, and each run of the line makes
    /// a new node of `T` holding it (›`dyad (T, v)` makes a new node of `T` holding `v` each time
    /// its line runs‹ [[R371]]). Without a bracket, `dyad` stands as its value.
    pub(crate) fn construct_dyad(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let types = self.types;
        let Some(bracket) = self.cell_at(tape, 1)?.filter(|c| c.is_bracket()) else {
            return Ok(Constructed::Decline);
        };
        // SAFETY: the bracket cell holds a node from the store; its arguments are reduced dyads.
        let node = unsafe {
            let args = self.args_of(bracket.dyad);
            let [ty, value] = args[..] else {
                return Err(ParseError::CtorArity);
            };
            // A box the pass has not filled holds its type only when the program runs, and the
            // line does not wait for it: stand-in for #292.
            let ty = self.type_known_now(ty)?;
            let node =
                crate::identities::dyad_place::build_of_type(self.rt.store, types, ty, value)?;
            // The node holds the value, and ends it with itself.
            self.take(value);
            node
        };
        tape.remove(1);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// `?`'s constructor (›`?` is one value, the unknown; `key := T ?` declares without a value‹
    /// [[R103]]): with a finished value to its left, which must be a type, that type's valueless
    /// place, the cell to the left consumed. Otherwise the one unknown stands as its value, and an
    /// identity to its left still to take its turn takes it as its operand and decides what it is
    /// there: `x != ?` compares with it, `i32 ?` is `i32`'s place.
    pub(crate) fn construct_hole(
        &mut self,
        id: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let base = match tape.at(-1).copied() {
            // A fresh name to the left is left for the boundary's own report.
            Some(cell) if !cell.is_fresh(self.types) && self.is_operand_cell(&cell) => {
                let d = self.operand_dyad(cell)?;
                Some(self.type_known_now(d)?)
            }
            _ => None,
        };
        let Some(t) = base else {
            let value = self.stand_as_value(tape, id);
            tape.place(value);
            return Ok(Constructed::Placed);
        };
        let node = self.valueless_place(t)?;
        tape.remove(-1);
        tape.place(node);
        tape.at_mut(0).expect("placed above").hole = true;
        Ok(Constructed::Placed)
    }

    /// The type `t` at the center, whose parse takes the value to its right, finding `?` alone
    /// there, `operand`: its valueless place takes both cells (›`?` is one value, the unknown;
    /// `key := T ?` declares without a value‹ [[R103]]). `false`, the tape untouched, for any other
    /// operand.
    pub(crate) fn takes_unknown(
        &mut self,
        tape: &mut ParsingTape,
        operand: &Cell,
        t: DyadPtr,
    ) -> Result<bool, ParseError> {
        if !self.is_bare_unknown(operand) {
            return Ok(false);
        }
        let node = self.valueless_place(t)?;
        tape.remove(1);
        tape.place(node);
        tape.at_mut(0).expect("placed above").hole = true;
        Ok(true)
    }

    /// What `T ?` makes of the type `t`, the valueless form a declaration takes, whose type
    /// `hole::declared_type` reads: an empty node of a type whose nodes run (›`array` is a
    /// chooser‹ [[R223]]), else the valueless marker.
    pub(super) fn valueless_place(&mut self, t: DyadPtr) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        // SAFETY: `t` is a type node from the store.
        unsafe {
            if crate::identities::meta::is_record_type(t)
                && !crate::identities::meta::run_body_of(t).is_null()
            {
                return Ok(crate::identities::this::empty_node(self.rt.store, t));
            }
            // The valueless marker `[T][null]`: the name `:=` gives it is laid out at the type's
            // width by the reading rule, so allocation and read cannot disagree.
            if crate::identities::read::place_layout(types, t).is_none() && !self.cx.field_hole {
                return Err(ParseError::NonNumericDeclaredType);
            }
        }
        Ok(crate::identities::hole::build(self.rt.store, types.unknown, t, std::ptr::null_mut()))
    }
}
