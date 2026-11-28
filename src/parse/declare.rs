// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Declarations: `:=`, `?`, `dyad`, the binding, what a write must be granted by, ownership
//! marks, and the unwritten-fields rule.

use super::*;
use crate::dyad;
use crate::identities::drop_model::{teardown_of, teardown_of_type, ExitItem, Teardown};

/// What follows a member in a path, which decides whether the path reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Next {
    /// `=`: the path is written.
    Write,
    /// `.` and another member; `checked_after` where that step checks the path again.
    Dot { checked_after: bool },
    /// Anything else: the place is read.
    Read,
}

impl<'a> Parser<'a> {
    /// A write into a name is granted by its own binding, and into a place reached by a path by
    /// every binding along it; the constructor's fill of its fresh node is granted; a write of a
    /// name's `lex_rank` is decided by the scope it is written from; a place reached no such way
    /// passes here.
    pub(crate) fn check_write(&self, target: &Cell) -> Result<(), ParseError> {
        use crate::identities::read::{place_of, Place};
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
            let fill = this::is_field_read(self.types, node) && this::is_fill(self.types, node);
            if fill || this::is_named(self.types, node) {
                return Ok(());
            }
            if target.path.is_null() && dyad::ty(node) == self.types.binding_ {
                return self.check_gates(&[node]);
            }
            // A call's place lies behind the dereference its body ends in, so the path before the
            // call grants nothing (›`a[k]` is an application, exactly as `a(k)`‹ [[R227]]).
            if let Some(Place::Call(_)) = place_of(self.types, node) {
                return self.check_through_pointer(node);
            }
            self.check_gates(&self.path_of(target))?;
            self.check_through_pointer(node)
        }
    }

    /// A write into a place a path reaches from a dereference, `p@ = …` or `p@.f.g = …`, is
    /// granted by what the pointer points at, as each field after it grants its own step
    /// (›An access along a path needs every gate on the path to grant it‹ [[R122]]).
    ///
    /// # Safety
    /// `node` must be null or a reduced dyad from the store.
    unsafe fn check_through_pointer(&self, node: DyadPtr) -> Result<(), ParseError> {
        use crate::identities::{pointer, this};
        let types = self.types;
        let mut at = node;
        while !at.is_null() {
            if dyad::ty(at) == types.deref_ {
                let into_result = crate::identities::union::member_of_place(types, at)
                    .is_some_and(|place| self.is_result_place(place));
                if into_result || pointer::writes_through(types, at) {
                    return Ok(());
                }
                return Err(ParseError::PointeeNotMutable);
            }
            let Some((into, _)) = this::field_step(types, at) else { break };
            at = into;
        }
        Ok(())
    }

    /// A write at a name's lex level, admitted from the scope that declares it and the scopes
    /// below; `mut` guards the value and has no part in it. ›Two levels: the binding
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
            || !Binding::read(place).typed_at_run
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
        // The body writes its own result, and a member of a `T!` one, which no caller may, but
        // not its tag (›A scope's value is what it evaluates to, and `return` is an optional early
        // exit from the enclosing function‹ [[R207]]); what lies inside grants as it does anywhere.
        let path = match path {
            [place, rest @ ..] if self.is_result_place(*place) => {
                use crate::identities::union;
                // SAFETY: a result place is a binding dyad laid out with its type.
                let ty = unsafe { Binding::read(*place).dyad };
                // SAFETY: `ty` is a union type where `is_union` holds.
                let member = |line: &DyadPtr| unsafe {
                    union::is_union(self.types, ty) && union::member_bindings(ty).contains(line)
                };
                match rest {
                    [line, inside @ ..] if member(line) => inside,
                    _ => rest,
                }
            }
            _ => path,
        };
        for &binding in path {
            // SAFETY: the path holds binding dyads from the store.
            unsafe {
                if !crate::identities::gate::name_has(self.types, binding, self.types.mut_) {
                    return Err(ParseError::NotMutable(Box::new(Binding::spelling(binding))));
                }
            }
        }
        Ok(())
    }

    /// A `static` function that writes a field of its value meets the gates `receiver.f = …`
    /// would (›A `static` function writes the value it is called on, and respects `mut`‹
    /// [[R239]]).
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
            self.check_gates(&path)?;
        } else if !self.cx.member_root.is_null() {
            self.check_gates(&[self.cx.member_root])?;
        }
        self.check_through_pointer(receiver.dyad)
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

    /// One binding per declared name, a dyad of type `binding` (›One binding per declared
    /// name‹ [[R119]]); an error is reported at `at`, the name's own offset, so the caret lands
    /// on the name.
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
        if self.in_type_body_lines() {
            return self.declare_member(scope, spelling, binding, at);
        }
        self.enter_binding(spelling, binding, at)
    }

    /// The line being read is one of a type body's own, whose names are its `static` members.
    pub(super) fn in_type_body_lines(&self) -> bool {
        self.cx.definitions.last().is_some_and(|def| self.cx.scopes.current() == Some(def.scope))
    }

    /// `binding`, a member of the type `scope` is a scope of, entered under `spelling` there and
    /// checked against its siblings alone: in a type body, every member its fields' scope and its
    /// own scope hold (›Members are dot-only; strictness has no exceptions‹ [[R336]]). A failure
    /// is reported at `at`.
    pub(super) fn declare_member(
        &mut self,
        scope: DyadPtr,
        spelling: &str,
        binding: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        let siblings = match self.cx.definitions.last() {
            Some(def) if def.block.0 == scope || def.scope == scope => vec![def.block.0, def.scope],
            _ => vec![scope],
        };
        // SAFETY: the caller hands a binding dyad from the store.
        let declared = unsafe {
            self.cx.scopes.declare_member(self.trie, spelling, binding, scope, &siblings)
        };
        if let Err(e) = declared {
            self.cx.pos = at;
            return Err(ParseError::Resolve(e));
        }
        Ok(binding)
    }

    /// Enter a minted binding under its spelling in the open scope; a failure is reported
    /// at `at`.
    pub(crate) fn enter_binding(
        &mut self,
        spelling: &str,
        binding: DyadPtr,
        at: usize,
    ) -> Result<DyadPtr, ParseError> {
        // SAFETY: the caller hands a binding dyad from the store.
        if let Err(e) = unsafe { self.cx.scopes.declare(self.trie, spelling, binding) } {
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

    /// Whether the name owns the node it holds, as its binding site decided.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store.
    pub(crate) unsafe fn owns_node(&self, binding: DyadPtr) -> bool {
        Binding::read(binding).holds
    }

    /// Whether the binding site made the name an owner: of a block, its place an owning
    /// pointer's, of a node, or of what its `dyad ?` place holds.
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
        Binding::set_holds(binding);
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
        let mut path = ended.binding;
        for &field in &ended.path {
            path = self.path_below(ended.binding, path, field);
        }
        self.cx.scopes.mark_dead(path, node);
    }

    /// The path binding under `path`, the name `name` or a path binding under it, that ends at the
    /// field binding `field`, made the first time a path reaches it.
    ///
    /// # Safety
    /// `name` must be a name's binding dyad, `path` it or a path binding under it.
    unsafe fn path_below(&mut self, name: DyadPtr, path: DyadPtr, field: DyadPtr) -> DyadPtr {
        let identity = Binding::read(field).dyad;
        let below = Binding::path(path, identity);
        if !below.is_null() {
            return below;
        }
        let name = Binding::read(name);
        let spelled = format!("{}.{}", Binding::spelling(path), Binding::spelling(field));
        let below = self.mint_binding(identity, name.scope, spelled.as_bytes());
        Binding::set_start(below, name.start);
        Binding::add_path(self.rt.store, self.types.array_, path, below);
        below
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
        if !unsafe { Binding::read(binding) }.unread
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
        if self.is_result_place(binding) {
            return Err(ParseError::ResultUnwritten);
        }
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
        // No later step of a held line is checked where it is written, so this one checks for them.
        let next = match self.next_identity() {
            Some(id) if id == self.types.assign => Next::Write,
            Some(id) if id == self.types.dot_ => Next::Dot { checked_after: false },
            _ => Next::Read,
        };
        // SAFETY: `binding` is a binding dyad from the store, which `.` reads as its own place.
        unsafe {
            self.check_name_field(
                binding,
                std::ptr::null_mut(),
                binding,
                &source[start..start + len],
                next,
                std::ptr::null_mut(),
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
        // A path is the place's when every step of it was a member by name, from a name.
        // SAFETY: a cell's path is null or an `array` node.
        let from_name = !place.root.is_null()
            && !place.path.is_null()
            && unsafe { crate::identities::array::items(place.path) }.first() == Some(&place.root);
        let path = if from_name { place.path } else { std::ptr::null_mut() };
        self.cx.last_write = LastWrite { node, scope, place: place.dyad, fill: place.target, path };
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
        let LastWrite { node, scope, place, fill, path } = self.cx.last_write;
        if item.is_null() || item != node {
            return;
        }
        let (name, fields) = if path.is_null() {
            (fill.0, vec![fill.1])
        } else {
            let path = crate::identities::array::items(path);
            (path[0], path[1..].to_vec())
        };
        if !name.is_null() {
            if Binding::read(name).scope == scope {
                self.fill_path(name, &fields);
            }
            return;
        }
        if place.is_null()
            || dyad::ty(place) != self.types.binding_
            || !Binding::read(place).unread
            || Binding::read(place).scope != scope
        {
            return;
        }
        Binding::set_unread(place, false);
        Binding::set_unwritten(place, std::ptr::null_mut());
    }

    /// A sibling write of the place the field bindings `fields` reach from the name `name`: each
    /// place along it still unwritten under the one above is tracked by its own path binding, its
    /// type's fields unwritten from the first write into it; the last field is filled, and each
    /// place it completes is filled in the place above it (›A value of several fields is filled
    /// one field at a time‹ [[R105]]). A place already written is left as it is.
    ///
    /// # Safety
    /// `name` must be a name's binding dyad; `fields` field bindings along its value.
    unsafe fn fill_path(&mut self, name: DyadPtr, fields: &[DyadPtr]) {
        let Some((&last, through)) = fields.split_last() else { return };
        let mut places = vec![name];
        for &field in through {
            let place = *places.last().expect("starts with the name");
            let identity = Binding::read(field).dyad;
            let unwritten = Binding::read(place).unwritten;
            if unwritten.is_null()
                || !crate::identities::array::items(unwritten).contains(&identity)
            {
                return;
            }
            let below = self.path_below(name, place, field);
            if Binding::read(below).unwritten.is_null() {
                let ty = crate::identities::hole::type_in(identity);
                if !crate::identities::meta::is_record_type(ty) {
                    return;
                }
                self.leave_fields_unwritten(below, ty, false);
            }
            places.push(below);
        }
        for (&place, &field) in
            places.iter().rev().zip(std::iter::once(&last).chain(through.iter().rev()))
        {
            if !self.fill_field(place, field) {
                return;
            }
        }
    }

    /// Fills the field `field` of the place `place` tracks, or every field of a union, which is one
    /// place: a write of any member fills it. Whether the place is now written whole.
    ///
    /// # Safety
    /// `place` must be a binding dyad from the store, `field` a field binding of its value.
    unsafe fn fill_field(&mut self, place: DyadPtr, field: DyadPtr) -> bool {
        let unwritten = Binding::read(place).unwritten;
        if unwritten.is_null() {
            return true;
        }
        let whole = crate::identities::union::is_union(self.types, Binding::read(field).frame);
        let identity = Binding::read(field).dyad;
        let left: Vec<DyadPtr> = crate::identities::array::items(unwritten)
            .iter()
            .copied()
            .filter(|&f| !whole && f != identity)
            .collect();
        if left.is_empty() {
            Binding::set_unwritten(place, std::ptr::null_mut());
            Binding::set_unread(place, false);
            return true;
        }
        let arr = crate::identities::array::build(self.rt.store, self.types.array_, &left);
        Binding::set_unwritten(place, arr);
        false
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
        Binding::set_unread(binding, true);
        let arr = crate::identities::array::build(self.rt.store, self.types.array_, &unwritten);
        Binding::set_unwritten(binding, arr);
    }

    /// `v.f` where `v` is a name, `root`, or a field path from the name `chain`, `left_path` the
    /// bindings it was reached by, the name first: the name and the field's binding it reaches
    /// when `v` is a name, nulls otherwise. A read of a field still unwritten is refused at the
    /// path's last step, a write let through: no step of a write path is a read (›A value of
    /// several fields is filled one field at a time‹ [[R105]]). Where a path under `v` ended, `v.f`
    /// is refused if `f`'s did, and so is `v.f` read whole, with no `.` after it, if a path under
    /// it did; a member that is no field, which takes `v` whole, is refused too.
    ///
    /// # Safety
    /// `root` and `chain` must be null or binding dyads from the store; `lhs` a reduced dyad;
    /// `left_path` null or an `array` of binding dyads.
    pub(super) unsafe fn check_name_field(
        &self,
        root: DyadPtr,
        chain: DyadPtr,
        lhs: DyadPtr,
        name: &str,
        next: Next,
        left_path: DyadPtr,
    ) -> Result<(DyadPtr, DyadPtr), ParseError> {
        let dotted = matches!(next, Next::Dot { .. });
        let none = (std::ptr::null_mut(), std::ptr::null_mut());
        let parent = if root.is_null() { self.reached_path(lhs, chain) } else { root };
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
            scope.resolve_member(self.trie, name).ok().filter(|r| fields.contains(&r.identity))
        };
        if !parent.is_null() && Binding::has_ended_path(parent) {
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
        // The name the path starts at and the fields it steps through, this one last; a member
        // that is no field takes the place before it whole.
        let mut fields = Vec::new();
        let from = if root.is_null() {
            let before = if left_path.is_null() {
                &[][..]
            } else {
                crate::identities::array::items(left_path)
            };
            match before.split_first() {
                Some((&first, rest)) if !chain.is_null() && first == chain => {
                    fields.extend_from_slice(rest);
                    chain
                }
                _ => std::ptr::null_mut(),
            }
        } else {
            root
        };
        fields.extend(field.as_ref().map(|f| f.binding));
        let reads = match (&field, next) {
            (None, _) | (Some(_), Next::Read) => true,
            (Some(_), Next::Write) => false,
            (Some(_), Next::Dot { checked_after }) => !checked_after,
        };
        if reads && !from.is_null() && !fields.is_empty() {
            if let Some(path) = self.unwritten_along(from, &fields) {
                return Err(ParseError::Unwritten(Box::new(path)));
            }
        }
        match field.filter(|_| !root.is_null()) {
            Some(field) => Ok((root, field.binding)),
            None => Ok(none),
        }
    }

    /// The first place along the path from the name `from` through the field bindings `fields`
    /// that sibling writes have not filled, spelled `v.a.b`: one still unwritten under the place
    /// above it, or the last, written only in part; `None` where the path reaches written bytes.
    ///
    /// # Safety
    /// `from` must be a binding dyad from the store, `fields` field bindings along its value.
    unsafe fn unwritten_along(&self, from: DyadPtr, fields: &[DyadPtr]) -> Option<String> {
        let mut place = from;
        let mut spelled = Binding::spelling(from);
        for &field in fields {
            let identity = Binding::read(field).dyad;
            let unwritten = Binding::read(place).unwritten;
            if unwritten.is_null()
                || !crate::identities::array::items(unwritten).contains(&identity)
            {
                return None;
            }
            spelled.push('.');
            spelled.push_str(&Binding::spelling(field));
            let below = Binding::path(place, identity);
            if below.is_null() || Binding::read(below).unwritten.is_null() {
                return Some(spelled);
            }
            place = below;
        }
        Some(spelled)
    }

    /// `name := value`, or `name := static value`: one place for every context
    /// that reaches the line, every call of the function around it and every
    /// pass of the loop around it, its value made once here at the definition;
    /// the line itself then only names it.
    pub(crate) fn construct_decl(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // A type body's member is made as its line is read, with the type.
        if self.in_type_body_lines() || !self.static_ahead() {
            return self.declare_here(tape);
        }
        let before = self.line_made();
        let built = self.parse_in_pass(|p| p.declare_here(tape));
        if !matches!(built?, Constructed::Placed) {
            return Ok(Constructed::Decline);
        }
        let node = tape.at(0).expect("the declaration was just placed").dyad;
        self.drain()?;
        let plan = self.take_made_since(&before);
        let depth = self.cx.open.len() - 1;
        let mark = self.rt.stack_mark();
        // SAFETY: `node` is the declare node just built, `plan` what its value made; its binding
        // is a binding dyad from the store.
        unsafe {
            self.run_held(depth, node, &plan, |p| p.run_on_pass(node).map_err(ParseError::Run))?;
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

    /// The result place of the function being read, the word `return` at `at` read as a name
    /// (›A scope's value is what it evaluates to, and `return` is an optional early exit from the
    /// enclosing function‹ [[R207]]): a place of its result type in its call frame, declared the
    /// first time the body reads it, unwritten until a write fills it, and held by the body, so
    /// a `return X` or a raise after a write ends what it holds.
    pub(super) fn result_place(&mut self, at: usize) -> Result<DyadPtr, ParseError> {
        match self.receiver() {
            Receiver::CommandLine => {
                return Err(self.fail_at(at, ParseError::ReturnOutsideFunction))
            }
            Receiver::Deferred => return Err(self.fail_at(at, ParseError::ReturnInDefer)),
            Receiver::Caller { .. } | Receiver::Nobody => {}
        }
        let depth = self.cx.frames.len();
        if self.cx.once_at == Some(depth) {
            return Err(self.fail_at(at, ParseError::OnceReadsUnmade));
        }
        // A line held for the run is read over stand-ins for the frames open where it was written.
        if depth <= self.cx.held_depth {
            return Err(self.fail_at(at, ParseError::ResultPlaceInHeldLine));
        }
        let types = self.types;
        let frame = self.cx.frames.last_mut().expect("a caller is a function's");
        if frame.slot_body {
            return Err(self.fail_at(at, ParseError::ResultPlaceInSlot));
        }
        // SAFETY: an open frame is a `fn` node, its signature written before its body parses.
        let output = unsafe { *(dyad::value(frame.frame) as *const DyadPtr).add(super::FN_OUTPUT) };
        if output == types.void_ {
            return Err(self.fail_at(at, ParseError::ResultPlaceOfVoid));
        }
        // SAFETY: a function's result is a type node from the store.
        if unsafe { crate::identities::meta::is_node_valued(output, types.fn_type) } {
            return Err(self.fail_at(at, ParseError::ResultPlaceOfNode));
        }
        if !frame.result.is_null() {
            let place = frame.result;
            // SAFETY: a result place is a binding dyad from the store.
            if unsafe { Binding::read(place) }.is_dead() {
                let dead = ResolveError::Dead("return".into());
                return Err(self.fail_at(at, ParseError::Resolve(dead)));
            }
            return Ok(place);
        }
        let body = frame.open_below;
        let scope = self.cx.open[body].scope.expect("a function body is a block");
        let binding = self.mint_binding(output, scope, b"return");
        // SAFETY: `output` is the function's result type, a type node from the store, and
        // `binding` the binding just minted.
        unsafe {
            let width = crate::identities::by_copy::place_width(types, output);
            let frame = self.cx.frames.last_mut().expect("read above");
            Binding::lay_out(binding, output, frame.frame, frame.size);
            frame.size += width;
            frame.result = binding;
            Binding::set_unread(binding, true);
            if crate::identities::meta::is_record_type(output) {
                self.leave_fields_unwritten(binding, output, false);
            }
            let ends = teardown_of_type(types, output);
            if ends != Teardown::Nothing {
                if ends.of_record() {
                    Binding::set_holds(binding);
                }
                // The frame starts zeroed, whose end runs nothing, so it is held from this line on.
                let open = &mut self.cx.open[body];
                open.exit.push(ExitItem { what: binding, from: open.lines, end: None });
            }
        }
        Ok(binding)
    }

    /// Whether the lexed `cell` is the word `return` read as a name, the result place: a read
    /// `.` or `holds`, or a write `=`, stands after it, where `return X` would take a value.
    pub(super) fn reads_result_place(&mut self, cell: &Cell) -> bool {
        let types = self.types;
        self.cell_identity(cell) == types.return_
            && matches!(self.next_identity(),
                Some(id) if id == types.dot_ || id == types.union.holds || id == types.assign)
    }

    /// Whether `binding` is the result place of a function being read.
    pub(super) fn is_result_place(&self, binding: DyadPtr) -> bool {
        !binding.is_null() && self.cx.frames.iter().any(|f| f.result == binding)
    }

    /// The gates of the value the name `binding` took, written before it, are the gates of the
    /// name's type (›Gates are type nodes: `x := mut i32 5`; the record holds no gates‹
    /// [[R381]]).
    pub(super) fn gate_name(&mut self, binding: DyadPtr, mint: DyadPtr) {
        use crate::identities::gate::{add_to_name, words_of};
        // SAFETY: `mint` is null or a gate mint, and `binding` is a binding dyad from the store.
        unsafe {
            for gate in words_of(mint) {
                add_to_name(self.rt.store, self.types, binding, gate);
            }
        }
    }

    /// Whether the cell at the center is the first of the value being read, only gate words
    /// standing before it.
    pub(crate) fn opens_value(&self, tape: &ParsingTape) -> bool {
        let mut k = -1;
        while let Some(cell) = tape.at(k) {
            if cell.constructed || !self.is_gate_word(cell.identity(self.types)) {
                return false;
            }
            k -= 1;
        }
        true
    }

    /// The gate words that open the value at the cursor, as written, and where they end; nothing
    /// is consumed.
    pub(super) fn gates_ahead(&mut self) -> (Vec<DyadPtr>, usize) {
        let at = self.cx.pos;
        let (mut words, mut end) = (Vec::new(), at);
        loop {
            let word = self.lex_spelling().and_then(|(start, len)| {
                let source = self.cx.source;
                self.cx.scopes.resolve(self.trie, &source[start..start + len]).ok()
            });
            match word.map(|r| r.identity) {
                Some(id) if self.is_gate_word(id) => {
                    words.push(id);
                    end = self.cx.pos;
                }
                _ => break,
            }
        }
        self.cx.pos = at;
        (words, end)
    }

    /// The gate words that open the value at the cursor include `static`; nothing is consumed.
    pub(super) fn static_ahead(&mut self) -> bool {
        self.gates_ahead().0.contains(&self.types.static_)
    }

    /// The value of the `:=` at the cursor where it waits for the run, held from after the gate
    /// words that open it, whether it is a place with no value, and the gate mint of those words,
    /// which gate the place the run fills; `None`, the cursor where it was, for a value parsed
    /// here.
    fn waiting_gated(&mut self) -> Result<Option<(Cell, bool, DyadPtr)>, ParseError> {
        let at = self.cx.pos;
        let (words, after) = self.gates_ahead();
        self.cx.pos = after;
        let Some((held, hole)) = self.waiting_value()? else {
            self.cx.pos = at;
            return Ok(None);
        };
        let mut gates = std::ptr::null_mut();
        for gate in words {
            // SAFETY: `gates` is null or a gate mint, and `gate` is a gate word.
            let joined =
                unsafe { crate::identities::gate::joined(self.rt.store, self.types, gates, gate) };
            gates = joined.ok_or_else(|| self.fail_at(at, ParseError::DoubleGate))?;
        }
        Ok(Some((held, hole, gates)))
    }

    /// A word that gates the value to its right.
    pub(super) fn is_gate_word(&self, id: DyadPtr) -> bool {
        crate::identities::gate::words(self.types).iter().any(|&(word, _)| word == id)
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
        let mut held_gates = std::ptr::null_mut();
        let value = self.waiting_gated().and_then(|held| match held {
            Some((held, hole, gates)) => {
                (typed_at_run, unwritten, held_gates) = (true, hole, gates);
                Ok(held)
            }
            None => self.parse_expression_cell(),
        });
        self.cx.filling.pop();
        self.cx.filling_at.pop();
        self.cx.last_declared = binding;
        self.cx.pending_binding = std::ptr::null_mut();
        let cell = if typed_at_run { value? } else { self.declared_value(value?)? };
        if self.is_bare_unknown(&cell) {
            return Err(self.untyped(tok.start, false));
        }
        // The gates the value's own gate words wrote are the name's (›Gates are type nodes:
        // `x := mut i32 5`; the record holds no gates‹ [[R381]]); the value lands without them.
        // SAFETY: a parsed cell holds a reduced dyad from the store.
        let (value, gates) = unsafe { crate::identities::gate::value_parts(self.types, cell.dyad) }
            .unwrap_or((cell.dyad, held_gates));
        // SAFETY: `gates` is null or a gate mint.
        let written =
            unsafe { crate::identities::gate::words_of(gates) }.contains(&self.types.mut_);
        // SAFETY: `binding` is the binding just declared; `value` a reduced dyad from the store.
        let declared = unsafe { self.land_at_binding(binding, value, cell.hole, written) }?;
        self.gate_name(binding, gates);
        // A place whose type its line gives when it runs keeps that type, which a write converts
        // into ([`Self::land_in_run_type`]); one laid out then holds no value until a write fills
        // it, as one declared `T ?` does.
        // SAFETY: `binding` is the binding just declared.
        unsafe {
            if typed_at_run {
                Binding::set_typed_at_run(binding);
            }
            if unwritten {
                Binding::set_unread(binding, true);
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
        use crate::identities::gate;
        if cell.hole || cell.type_value {
            return Ok(cell);
        }
        // A type a line computes, gated: the place is made of the type, with both sets of gates.
        // SAFETY: a parsed cell holds a reduced dyad from the store.
        let (value, written) = unsafe { gate::value_parts(self.types, cell.dyad) }
            .unwrap_or((cell.dyad, std::ptr::null_mut()));
        let place = self.empty_place_of(value).map_err(|e| self.fail_at(cell.start, e))?;
        let Some((place, own)) = place else { return Ok(cell) };
        // SAFETY: `written` and `own` are null or gate mints, `value` a reduced dyad and `place` a
        // node just built.
        let place = unsafe {
            let gates = gate::union(self.rt.store, self.types, written, own);
            let byte = gate::byte_of(self.types, value);
            gate::give_at(self.rt.store, self.types, gates, byte, place, false)
        }
        .map_err(|e| self.fail_at(cell.start, e))?;
        let mut cell = cell;
        cell.dyad = place;
        cell.hole = true;
        cell.constructed = true;
        Ok(cell)
    }

    /// The place `T ?` makes of the type `value` gives, read as `?` reads the type to its left,
    /// with the gates of a gated `T`; `None` for a value whose own type is not `type`, and for `?`
    /// itself.
    pub(super) fn empty_place_of(
        &mut self,
        value: DyadPtr,
    ) -> Result<Option<(DyadPtr, DyadPtr)>, ParseError> {
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

    /// `type`, a word of the one rank (›Every identity has a `parse_rank`, and `(` works in two
    /// steps‹ [[R259]]): the `( … )` right after it is its own, the body of a new type
    /// ([`Self::parse_type_body`]); `type ?` is the valueless `type` place; `type T` takes `T` as
    /// a type takes a value ([`Self::type_operand`]) and gives the type `T` as a value of `type`,
    /// as `i32 1` is a value of `i32`, so a name given it holds `T` itself (›The ground is one
    /// identity, spelled `type`; `logos` is a language‹ [[R134]]): `type @i32`, `type pick(1)`.
    /// A box the pass has filled gives what it holds; a type known only when the program runs
    /// stays the value it is, which lands in a `type` place. With nothing to take, `type` stands
    /// as itself.
    pub(crate) fn construct_type(
        &mut self,
        id: DyadPtr,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if let Some(body) = self.own_bracket(tape) {
            let node = self.read_own_bracket(tape, body, |p| p.parse_type_body(id))?;
            tape.place_type_value(node);
            return Ok(Constructed::Placed);
        }
        let Some(operand) = self.type_operand(tape, 1)? else {
            return Ok(Constructed::Decline);
        };
        if self.takes_unknown(tape, &operand, id)? {
            return Ok(Constructed::Placed);
        }
        let value = self.as_operand(operand)?;
        tape.remove(1);
        let types = self.types;
        // The gates a gate word put on a type a place holds join the type's own.
        // SAFETY: `value` is a reduced dyad from the store.
        let (value, gates) = unsafe { crate::identities::gate::value_parts(types, value) }
            .unwrap_or((value, std::ptr::null_mut()));
        // SAFETY: `value` is a reduced dyad from the store, and `type` a type node.
        let landed = unsafe { crate::identities::land(self.rt.store, types, value, types.type_) }
            .map_err(|e| self.fail_at(operand.start, e))?;
        // SAFETY: `landed` is a reduced dyad from the store.
        let known = unsafe {
            let read = self.settled_type(types.through(landed));
            crate::identities::type_identity_of(types, read)
        };
        let held = match known {
            // SAFETY: `t` is a type node, and `gates` null or a gate mint.
            Some(t) => unsafe { crate::identities::gate::gated(self.rt.store, types, gates, t) },
            None if gates.is_null() => landed,
            None => return Err(self.fail_at(operand.start, ParseError::TypeKnownOnlyAtRun)),
        };
        tape.place_type_value(held);
        Ok(Constructed::Placed)
    }

    /// A declaration's right side is `?` alone, with no type to its left.
    pub(super) fn is_bare_unknown(&self, cell: &Cell) -> bool {
        // SAFETY: a parsed cell holds a reduced dyad from the store.
        !cell.hole && unsafe { self.types.through(cell.dyad) } == self.types.unknown
    }

    /// A new name, field or parameter written without its type, the checked error where it is
    /// written (DESIGN ›`?` is one value, the unknown; `key := T ?` declares without a value‹
    /// [[R103]]); a field's error also names the code that builds the mints.
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
    /// ›Holding is decided at the binding site, parameters included‹ [[R61]]). `written` says the
    /// name's gates let `=` write it after its declaration. Returns what the declare node runs.
    ///
    /// # Safety
    /// `binding` must be the binding being declared; `value` a reduced dyad from the store.
    pub(super) unsafe fn land_at_binding(
        &mut self,
        binding: DyadPtr,
        value: DyadPtr,
        hole: bool,
        written: bool,
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
            let empty_node =
                hole && crate::identities::meta::is_node_valued(declared, self.types.fn_type);
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
                // Whatever is written into it later is the name's to end: `a := r ?` of a plain
                // record whose body fills `free`, `a := own @T ?` and `d := dyad ?`.
                match teardown_of_type(self.types, t) {
                    t if t.of_record() => self.hold_node(binding),
                    Teardown::Nothing => {}
                    _ => self.hold(binding),
                }
                if !crate::identities::hashmap::is_hashmap(self.types, t) {
                    Binding::set_unread(binding, true);
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
                && !crate::identities::gate::is_node(read)
                && !written
            {
                // A type held, `type i32`, by a name nothing writes: the name is another spelling
                // of it (›Every type is minted by its contents through `type_mint`; the name that
                // holds a `type ( … )` gives it its identity‹ [[R383]]). A `mut` name holds it in a
                // `type` place instead, which `=` writes (›A `type ?` place is a box, written any
                // number of times‹ [[R291]]), and so does a gated type, `type mut i32`, since a
                // gate node where the binding points is the name's own.
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
            } else if let Teardown::Block(_) = teardown_of(self.types, value) {
                // An owning value lands in a place of an owning `@pointee`, which the scope holds,
                // the gates under the pointer kept.
                let ty = crate::identities::read::output_type(self.types, value);
                let ty = crate::identities::type_identity_of(self.types, ty).unwrap_or(ty);
                let owning_ty = crate::identities::pointer::make_owning_pointer_type(
                    self.rt.store,
                    self.types,
                    crate::identities::numtype::gated_pointee_of(ty),
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
        let (node, gates) = unsafe {
            let args = self.args_of(bracket.dyad);
            let [ty, value] = args[..] else {
                return Err(ParseError::CtorArity);
            };
            // A box the pass has not filled holds its type only when the program runs, and the
            // line does not wait for it: stand-in for #292.
            let ty = self.type_known_now(ty)?;
            // A gated type gives a node of the type it gates, gated as `mut i32 5` is.
            let (ty, gates) = crate::identities::gate::parts(ty);
            let node =
                crate::identities::dyad_place::build_of_type(self.rt.store, types, ty, value)?;
            // The node holds the value, and ends it with itself.
            self.take(value);
            (node, gates)
        };
        // SAFETY: `gates` is null or a gate mint, and `node` a node just built.
        let node =
            unsafe { crate::identities::gate::give(self.rt.store, types, gates, node, false) }?;
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
        // The place of a gated type, `x.type ?`, has that type's gates.
        let node = self.gated_place(t)?;
        tape.remove(-1);
        tape.place(node);
        tape.at_mut(0).expect("placed above").hole = true;
        Ok(Constructed::Placed)
    }

    /// [`Self::valueless_place`] of `t`, gated as `t` is: `mut i32 ?` is `i32 ?` gated `mut`.
    fn gated_place(&mut self, t: DyadPtr) -> Result<DyadPtr, ParseError> {
        use crate::identities::gate;
        let (node, gates) = self.valueless_place(t)?;
        // SAFETY: `gates` is null or a gate mint, `t` a type node and `node` a node just built.
        unsafe {
            let byte = gate::byte_of(self.types, t);
            gate::give_at(self.rt.store, self.types, gates, byte, node, false)
        }
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
        let node = self.gated_place(t)?;
        tape.remove(1);
        tape.place(node);
        tape.at_mut(0).expect("placed above").hole = true;
        Ok(true)
    }

    /// What `T ?` makes of the type `t`, the valueless form a declaration takes, whose type
    /// `hole::declared_type` reads: an empty node of a type whose nodes run (›`array` is a
    /// chooser‹ [[R223]]), else the valueless marker; with the gate mint of a gated `t`, which
    /// [`Self::gated_place`] gates the place with.
    pub(super) fn valueless_place(&mut self, t: DyadPtr) -> Result<(DyadPtr, DyadPtr), ParseError> {
        let types = self.types;
        // SAFETY: `t` is a type node from the store.
        unsafe {
            let (under, gates) = crate::identities::gate::parts(t);
            if crate::identities::meta::is_record_type(under)
                && !crate::identities::meta::run_body_of(under).is_null()
            {
                return Ok((crate::identities::this::empty_node(self.rt.store, under), gates));
            }
            // The valueless marker `[T][null]`: the name `:=` gives it is laid out at the type's
            // width by the reading rule, so allocation and read cannot disagree.
            if crate::identities::read::place_layout(types, under).is_none() && !self.cx.field_hole
            {
                return Err(ParseError::NonNumericDeclaredType);
            }
            let hole = crate::identities::hole::build(
                self.rt.store,
                types.unknown,
                t,
                std::ptr::null_mut(),
            );
            Ok((hole, gates))
        }
    }
}
