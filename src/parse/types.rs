// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Type bodies: the field list, the slot fills and share members, a held `type (…)`
//! built at its run, and a run type's specialization and comptime fold.

use super::*;
use crate::dyad;

/// What a `type (…)` body's lines have filled so far: the head the type node
/// takes at the close, its one set of slots, and its values' fields.
pub(super) struct OpenType {
    pub(super) scope: DyadPtr,
    /// The five slot words as this definition's own markers, declared into the body's scope.
    slots: [DyadPtr; 5],
    /// The marker a `drop = (…)` line declares; `drop` itself is the core word.
    drop_marker: DyadPtr,
    parse_rank: f64,
    /// From a `lex_rank = …` line; written onto the declaration's binding at
    /// the close.
    lex_rank: Option<f64>,
    assoc: Assoc,
    ctor: DyadPtr,
    /// From a `run = (…)` line: the body lexed once into its cells; a node of
    /// the type constructs it per field-type set.
    run_body: DyadPtr,
    /// The hidden receiver of the body being read, a parse's node or a `drop`'s or
    /// `share` function's value; null otherwise.
    pub(super) this_param: DyadPtr,
    /// `this_param` is a parse's node, whose fields are reached through `tape[0]`, never bare.
    pub(super) in_parse: bool,
    /// The parse body's `tape` parameter while `in_parse`.
    pub(super) tape_param: DyadPtr,
    /// The declaration this body is the value of: inside its parse, its name is `self_type`.
    pub(super) binding: DyadPtr,
    /// The `share` function being read has read a field of its value.
    pub(super) read_receiver: bool,
    /// The `share` function being read has written a field of its value, itself or through a
    /// bare call.
    pub(super) wrote_receiver: bool,
    /// The fields' scope and the fields declared so far: what a body written
    /// below them reaches by name.
    pub(super) block: (DyadPtr, Vec<DyadPtr>),
    /// From a `share drop = (…)` line: the `fn` over the value an owner's teardown runs.
    instances_drop: DyadPtr,
    /// A field declared `own` over a node's type, whose teardown the type's drop must write.
    owns_node_field: bool,
    /// The type node being defined, its record written at the close: what its name means in its parse.
    pub(super) self_type: DyadPtr,
}

/// The slot words a type body declares into its own scope, in `SlotKind`
/// order (DESIGN ›A type body describes one level‹). `drop` is not here: it is
/// the statement keyword, which `=` takes as the slot's name.
pub const SLOT_NAMES: [&str; 5] = ["parse_rank", "lex_rank", "associativity", "parse", "run"];

/// One of the slots, by `SLOT_NAMES` position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    ParseRank = 0,
    LexRank = 1,
    Associativity = 2,
    Parse = 3,
    Run = 4,
    /// The `drop` keyword left of `=`: no spelling in `SLOT_NAMES`, since the
    /// word is the statement's.
    Drop = 5,
}

impl SlotKind {
    fn of(i: usize) -> Self {
        match i {
            0 => SlotKind::ParseRank,
            1 => SlotKind::LexRank,
            2 => SlotKind::Associativity,
            3 => SlotKind::Parse,
            _ => SlotKind::Run,
        }
    }
}

/// A scalar at its own width; anything else an 8-byte container.
///
/// # Safety
/// `logos` must be null or a type node from the store.
unsafe fn field_width(logos: DyadPtr) -> u64 {
    if crate::identities::numtype::is_scalar_type(logos) {
        crate::identities::numtype::of_type_node(logos).bytes() as u64
    } else {
        8
    }
}

impl<'a> Parser<'a> {
    /// `( field-list )` as a record node whose value is a `RECORD_TAG` record
    /// holding the scope, the `fields` array and the packed `size_bytes`. Fresh
    /// field names are read raw, which is why the list has its own sub-parse.
    pub fn parse_record(&mut self) -> Result<DyadPtr, ParseError> {
        self.parse_record_taking(None)
    }

    /// `parse_record` with an unnamed field of type `leading` first.
    pub(super) fn parse_record_taking(
        &mut self,
        leading: Option<DyadPtr>,
    ) -> Result<DyadPtr, ParseError> {
        let record_logos = self.types.type_;
        let (scope, fields_arr, size_bytes) = self.parse_field_list(false, leading)?;
        let record = crate::identities::meta::record_layout(
            self.rt.store,
            scope,
            fields_arr,
            size_bytes,
            std::ptr::null_mut(),
            crate::identities::meta::prec::APPLY,
            Assoc::Left,
        );
        Ok(self.rt.store.alloc_raw(record_logos, record.cast()))
    }

    /// A `fn`'s parameter list checks each name against every open scope, since
    /// the body reopens the list's scope; a type body's fields against their
    /// siblings alone (`relaxed`), where the list is the body itself, its
    /// slot fills, `share` members and prose among the fields (DESIGN ›A type
    /// body describes one level‹).
    fn parse_field_list(
        &mut self,
        relaxed: bool,
        leading: Option<DyadPtr>,
    ) -> Result<(DyadPtr, DyadPtr, u64), ParseError> {
        if !relaxed {
            self.expect_open()?;
        }
        // Field names are declared into the record's own scope.
        let scope = self.open_scope();

        let mut fields = Vec::new();
        if let Some(ty) = leading {
            fields.push(self.rt.store.alloc_raw(ty, std::ptr::null_mut()));
        }
        let lines = self.field_lines(relaxed, scope, &mut fields);

        self.cx.scopes.pop();
        lines?;
        if !relaxed {
            self.expect_close()?;
        }

        // Fields pack in declaration order at the width rule parameters claim
        // frame offsets by.
        // SAFETY: each field is the dyad just built, its type null or a type node.
        let size_bytes: u64 = fields.iter().map(|&f| unsafe { field_width(dyad::ty(f)) }).sum();
        let fields_arr = crate::identities::array::build(self.rt.store, self.types.array_, &fields);
        Ok((scope, fields_arr, size_bytes))
    }

    fn field_lines(
        &mut self,
        relaxed: bool,
        scope: DyadPtr,
        fields: &mut Vec<DyadPtr>,
    ) -> Result<(), ParseError> {
        loop {
            if self.at_close() {
                break;
            }
            if relaxed {
                if self.at_end() {
                    return Err(ParseError::UnclosedBracket);
                }
                if let Some(def) = self.cx.definitions.last_mut() {
                    def.block = (scope, fields.clone());
                }
                // Prose before a slot fill is lifted beside it, as on any line;
                // before a field or member it is read past.
                if self.at_prose() {
                    let at = self.cx.pos;
                    self.skip_prose();
                    if self.at_slot_word() {
                        return Err(ParseError::SlotFillNeedsShare);
                    }
                    if self.at_share_slot() {
                        self.cx.pos = at;
                        self.lift_prose()?;
                        self.lex_spelling();
                        self.slot_line(self.cx.pos)?;
                    }
                    continue;
                }
            }
            let (start, len) = self.lex_spelling().ok_or(ParseError::ExpectedField)?;
            // `self.cx.source` is `&'a str` (Copy), so the slice is independent
            // of the `&mut self` needed below.
            let source = self.cx.source;
            let name = &source[start..start + len];
            let word = self.cx.scopes.resolve(self.trie, name).ok().map(|r| r.identity);
            // One place stored with the type, not a field of the layout; never a
            // parameter, which each call fills (DESIGN ›Two muts, and the storage partition‹).
            if word == Some(self.types.share_) {
                if !relaxed {
                    self.cx.pos = start;
                    return Err(ParseError::ShareMisplaced);
                }
                if self.at_slot_word() {
                    self.slot_line(self.cx.pos)?;
                } else {
                    self.share_member(start)?;
                }
                continue;
            }
            // `mut` or `immut` before a field or parameter gates its binding, as before any name.
            let gate = word.filter(|&w| w == self.types.mut_ || w == self.types.immut_);
            let (start, name) = if gate.is_some() {
                let (start, len) = self.lex_spelling().ok_or(ParseError::ExpectedField)?;
                (start, &source[start..start + len])
            } else {
                (start, name)
            };
            // A literal, `5`, matches a pattern identity and names nothing.
            if relaxed {
                if let Ok(r) = self.cx.scopes.resolve(self.trie, name) {
                    // SAFETY: a resolved binding is a binding dyad from the store.
                    if unsafe { Binding::spelling(r.binding) } != name {
                        self.cx.pos = start;
                        return Err(ParseError::TypeBodyLine);
                    }
                }
            }
            if relaxed && gate.is_none() {
                let declares =
                    matches!(self.peek_token(), Some((id, _)) if id == self.types.declare_tok);
                let def = self.cx.definitions.last().expect("a relaxed field list is a type body");
                if word.is_some_and(|id| id == self.types.drop_ || def.slots.contains(&id)) {
                    self.cx.pos = start;
                    return Err(if declares {
                        ParseError::Resolve(ResolveError::Shadowed(name.to_string()))
                    } else {
                        ParseError::SlotFillNeedsShare
                    });
                }
            }
            // `name := T ?` declares the field's type through the hole `?`
            // built; a bare name leaves the type slot undefined. A default,
            // `size := u64 0`, is a constant: the field's dyad is that value,
            // and each new node starts with it in the field's slot.
            let mut default = std::ptr::null_mut();
            let mut owns_node = false;
            let logos = if self.consume_token(self.types.declare_tok) {
                let outer = std::mem::replace(&mut self.cx.field_hole, relaxed);
                let value = self.parse_expression_cell();
                self.cx.field_hole = outer;
                let cell = value?;
                let value = cell.dyad;
                owns_node = cell.owning;
                if cell.hole {
                    // SAFETY: `value` is the place `?` just built.
                    unsafe { dyad::ty(value) }
                } else {
                    // SAFETY: `value` is a reduced dyad just parsed.
                    let held = unsafe { self.types.through(value) };
                    if held == self.types.unknown {
                        std::ptr::null_mut() // `name := ?`: no type yet
                    } else if relaxed
                        // SAFETY: as above.
                        && unsafe {
                            matches!(
                                crate::identities::read::read_kind(self.types, held),
                                crate::identities::read::Read::Scalar(_)
                            ) && !crate::dyad::is_place(dyad::value(held))
                        }
                    {
                        // SAFETY: as above.
                        let (ty, value) = unsafe { (dyad::ty(held), dyad::value(held)) };
                        default = value;
                        ty
                    } else {
                        self.cx.pos = start;
                        return Err(ParseError::BadDeclaredType);
                    }
                }
            } else {
                // A bare field name stands alone on its line: `x = 5` in a type
                // body writes no slot `type` declares.
                let alone = self.at_close()
                    || matches!(
                        self.peek_token(),
                        Some((id, _)) if id == self.types.sep_
                    );
                if relaxed && !alone {
                    self.cx.pos = start;
                    return Err(match word {
                        None => ParseError::Resolve(ResolveError::Unknown(name.to_string())),
                        Some(_) => ParseError::TypeBodyLine,
                    });
                }
                std::ptr::null_mut()
            };
            // An `own` parameter consumes its argument, which no call does yet.
            // SAFETY: `logos` is null or a type node.
            let owning = !logos.is_null()
                && unsafe {
                    crate::identities::numtype::is_pointer_type(logos)
                        && !crate::identities::meta::destructor_of(logos).is_null()
                };
            if !relaxed && (owning || owns_node) {
                self.cx.pos = start;
                return Err(ParseError::OwnParameterNotInSeed);
            }
            let field = self.rt.store.alloc_raw(logos, default);
            // The field's name is not stored on the record: declaring it puts
            // a binding in the one name index (DESIGN ›Name resolution is scope-filtered‹).
            let binding = if relaxed {
                // One block, one no-shadowing rule: a field is checked against
                // the `share` members too, which live in the type's own scope.
                let body =
                    self.cx.definitions.last().expect("a relaxed field list is a type body").scope;
                if self.cx.scopes.declared_in(self.trie, name, body).map_err(ParseError::Resolve)? {
                    self.cx.pos = start;
                    return Err(ParseError::Resolve(ResolveError::Shadowed(name.to_string())));
                }
                self.declare_field_name(name, field, start)?
            } else {
                self.declare_name(name, field, start)?
            };
            if let Some(gate) = gate {
                self.add_gate(binding, gate)?;
            }
            if owns_node {
                self.add_gate(binding, self.types.own_)?;
                if let Some(def) = self.cx.definitions.last_mut() {
                    def.owns_node_field = true;
                }
            }
            fields.push(field);
            if !self.consume_separator() {
                break;
            }
        }
        Ok(())
    }

    /// `share name := value` in a type body: one place stored with the type,
    /// declared in the body scope (DESIGN ›Two muts, and the storage
    /// partition‹). The fields and the members stay one scope for no-shadowing.
    fn share_member(&mut self, at: usize) -> Result<(), ParseError> {
        let body = match self.cx.definitions.last() {
            Some(def) => def.scope,
            None => {
                self.cx.pos = at;
                return Err(ParseError::ShareMisplaced);
            }
        };
        if self.consume_token(self.types.declare_tok) {
            self.cx.pos = at;
            return Err(ParseError::ShareNeedsDeclaration);
        }
        let field_scope = self.cx.scopes.pop().expect("the field list's scope is open");
        self.cx.last_declared = std::ptr::null_mut();
        let outer_member = self.cx.member_fn_depth.replace(self.cx.frames.len());
        let items = self.share_line();
        self.cx.member_fn_depth = outer_member;
        self.cx.scopes.push(field_scope);
        // The line's one declaration, a member or a slot fill, with its prose
        // lifted out beside it; anything else is not what `share` marks.
        let mut declared = None;
        for item in items? {
            // SAFETY: `item` is a reduced dyad just parsed.
            let ty = unsafe { dyad::ty(item) };
            if ty == self.types.comment_ {
                // SAFETY: the pending bindings were minted by this parser's declares.
                unsafe { self.cx.scopes.settle_item(body, item) };
            } else if ty == self.types.declare_ && declared.is_none() {
                declared = Some(item);
            } else {
                self.cx.pos = at;
                return Err(ParseError::ShareNeedsDeclaration);
            }
        }
        let Some(item) = declared else {
            self.cx.pos = at;
            return Err(ParseError::ShareNeedsDeclaration);
        };
        {
            // SAFETY: a declaration that fills no slot is `:=`'s, its lhs the binding.
            let name = unsafe { Binding::spelling(crate::identities::declare::binding_of(item)) };
            if self
                .cx
                .scopes
                .declared_in(self.trie, &name, field_scope)
                .map_err(ParseError::Resolve)?
            {
                self.cx.pos = at;
                return Err(ParseError::Resolve(ResolveError::Shadowed(name)));
            }
            let binding = self.cx.last_declared;
            if binding.is_null() {
                self.cx.pos = at;
                return Err(ParseError::ShareNeedsDeclaration);
            }
            self.add_gate(binding, self.types.share_)?;
        }
        // SAFETY: the pending bindings were minted by this parser's declares.
        unsafe { self.cx.scopes.settle_item(body, item) };
        Ok(())
    }

    /// The first `parse_next` lexes and constructs the segment and queues
    /// what it yielded; the rest are taken while queued, before anything
    /// further is lexed, so none is left for the enclosing body's loop.
    fn share_line(&mut self) -> Result<Vec<DyadPtr>, ParseError> {
        let mut items = Vec::new();
        let Some(first) = self.parse_next() else {
            return Ok(items);
        };
        items.push(first?);
        while !self.cx.queued.is_empty() {
            items.push(self.parse_next().expect("a queued item comes out first")?);
        }
        Ok(items)
    }

    /// A slot fill in a type body, `parse = (…)` or `drop = (…)`, parsed over
    /// the body's scope from the slot word at `start`.
    fn slot_line(&mut self, start: usize) -> Result<(), ParseError> {
        let body = self.cx.definitions.last().expect("a slot line is in a type body").scope;
        self.cx.pos = start;
        let field_scope = self.cx.scopes.pop().expect("the field list's scope is open");
        let items = self.share_line();
        self.cx.scopes.push(field_scope);
        let mut filled = false;
        for item in items? {
            // SAFETY: `item` is a reduced dyad just parsed.
            let ty = unsafe { dyad::ty(item) };
            let fill = ty == self.types.declare_ && self.is_slot_fill(item);
            if !(fill && !filled || ty == self.types.comment_) {
                self.cx.pos = start;
                return Err(ParseError::TypeBodyLine);
            }
            filled |= fill;
            // SAFETY: the pending bindings were minted by this parser's declares.
            unsafe { self.cx.scopes.settle_item(body, item) };
        }
        Ok(())
    }

    fn at_end(&mut self) -> bool {
        self.skip_whitespace();
        if self.cx.feed.is_some() {
            return self.feed_index().is_none();
        }
        self.cx.pos >= self.cx.source.len()
    }

    fn at_prose(&mut self) -> bool {
        self.skip_whitespace();
        self.cx.source.as_bytes().get(self.cx.pos) == Some(&b'#')
    }

    /// Past each comment at the cursor, the line form to its newline and the
    /// string form, `# «…»`, past its quote.
    fn skip_prose(&mut self) {
        let source = self.cx.source;
        let bytes = source.as_bytes();
        while self.at_prose() {
            self.cx.pos += 1;
            while matches!(bytes.get(self.cx.pos), Some(b' ' | b'\t')) {
                self.cx.pos += 1;
            }
            if source[self.cx.pos..].starts_with('«') {
                match self.cx.scopes.lex(self.trie, &source[self.cx.pos..]) {
                    Ok(r) => self.cx.pos += r.matched,
                    Err(_) => return,
                }
            } else {
                while bytes.get(self.cx.pos).is_some_and(|&b| b != b'\n') {
                    self.cx.pos += 1;
                }
            }
        }
    }

    /// The comments at the cursor, settled into the body beside the line after them.
    fn lift_prose(&mut self) -> Result<(), ParseError> {
        let body = self.cx.definitions.last().expect("read inside a type body").scope;
        while self.at_prose() {
            self.cx.pos += 1;
            let node = self.comment_after_hash()?;
            // SAFETY: `node` is the comment node just built.
            unsafe { self.cx.scopes.settle_item(body, node) };
        }
        Ok(())
    }

    /// The next words are `share` and a slot word or `drop`; nothing is consumed.
    fn at_share_slot(&mut self) -> bool {
        let at = self.cx.pos;
        let word = self.lex_spelling().and_then(|(start, len)| {
            let source = self.cx.source;
            self.cx.scopes.resolve(self.trie, &source[start..start + len]).ok().map(|r| r.identity)
        });
        let slot = word == Some(self.types.share_) && self.at_slot_word();
        self.cx.pos = at;
        slot
    }

    /// The next word is a slot word or `drop`; nothing is consumed.
    fn at_slot_word(&mut self) -> bool {
        let at = self.cx.pos;
        let word = self.lex_spelling().and_then(|(start, len)| {
            let source = self.cx.source;
            self.cx.scopes.resolve(self.trie, &source[start..start + len]).ok().map(|r| r.identity)
        });
        self.cx.pos = at;
        let def = self.cx.definitions.last().expect("read inside a type body");
        word.is_some_and(|id| id == self.types.drop_ || def.slots.contains(&id))
    }

    /// A slot fill's item declares the definition's own marker.
    fn is_slot_fill(&self, item: DyadPtr) -> bool {
        // SAFETY: `item` is a declare node from the store.
        let declared = unsafe { crate::identities::declare::declared_of(item) };
        self.cx
            .definitions
            .last()
            .is_some_and(|def| def.slots.contains(&declared) || def.drop_marker == declared)
    }

    /// One body for one level (DESIGN ›A type body describes one level‹): its
    /// lines fill the type's slots, declare its values' fields, and declare
    /// its `share` members; every other line must be prose, since nothing runs a type body later.
    pub fn parse_type_body(&mut self, id: DyadPtr) -> Result<DyadPtr, ParseError> {
        if self.cx.runtime_depth > 0 {
            return self.hold_type_body();
        }
        // The declaration whose whole right side is this `type (…)` names it.
        let own_name = match (self.cx.filling.last(), self.cx.filling_at.last()) {
            (Some(&binding), Some(&at))
                if self.cx.source.get(at..self.cx.pos).is_some_and(|t| t.trim() == "type") =>
            {
                binding
            }
            _ => std::ptr::null_mut(),
        };
        self.expect_open()?;
        let depth = self.cx.scopes.depth();
        // The slot words are the fields `type` declares (identities/type.logos):
        // names on the body's lines and nowhere outside them, in a scope of
        // their own so that a member read through the type never finds them.
        // Each is a marker of this definition, declared against its siblings
        // alone, so that a body inside another's shadows its words.
        let words = self.open_scope();
        let slots = SLOT_NAMES.map(|_| {
            let head = crate::identities::meta::record(
                self.rt.store,
                crate::identities::meta::TYPEREC_TAG,
                crate::identities::meta::prec::INERT,
            );
            self.rt.store.alloc_raw(self.types.type_, head)
        });
        for (name, &marker) in SLOT_NAMES.iter().zip(&slots) {
            let binding = self.mint_binding(marker, words, name.as_bytes());
            // SAFETY: `binding` was minted by `Binding::alloc` just above.
            if let Err(e) = unsafe { self.cx.scopes.declare_field(self.trie, name, binding) } {
                self.cx.scopes.pop();
                return Err(ParseError::Resolve(e));
            }
        }
        let head = crate::identities::meta::record(
            self.rt.store,
            crate::identities::meta::TYPEREC_TAG,
            crate::identities::meta::prec::INERT,
        );
        let drop_marker = self.rt.store.alloc_raw(self.types.type_, head);
        let self_type = self.rt.store.alloc_raw(id, std::ptr::null_mut());
        let scope = self.open_scope();
        self.cx.definitions.push(OpenType {
            scope,
            slots,
            drop_marker,
            parse_rank: crate::identities::meta::prec::APPLY,
            lex_rank: None,
            assoc: Assoc::Left,
            ctor: std::ptr::null_mut(),
            run_body: std::ptr::null_mut(),
            this_param: std::ptr::null_mut(),
            in_parse: false,
            tape_param: std::ptr::null_mut(),
            binding: own_name,
            read_receiver: false,
            wrote_receiver: false,
            block: (scope, Vec::new()),
            instances_drop: std::ptr::null_mut(),
            owns_node_field: false,
            self_type,
        });
        // A `fn` literal on a slot's right side must not claim the enclosing
        // declaration's placeholder.
        let suppressed = self.take_pending_fn();
        // The body's declarations are pending until its close: a type is
        // comptime and its members are stored at the definition. Everything
        // before the body runs first, and a failure among its lines is the body's.
        let outer = self.drain();
        self.cx.open.push(OpenScope::default());
        let saved_depth = std::mem::replace(&mut self.cx.runtime_depth, 0);
        let lines = outer.and_then(|_| self.parse_field_list(true, None)).and_then(|layout| {
            self.drain().map(|_| layout).map_err(|e| match e {
                ParseError::Run(r) => {
                    ParseError::TypeBodyFailed(Box::new(crate::report::run_message(&r)))
                }
                other => other,
            })
        });
        self.cx.runtime_depth = saved_depth;
        let defers = self.cx.open.pop().expect("pushed above").defers;
        self.restore_pending_fn(suppressed);
        let def = self.cx.definitions.pop().expect("pushed above");
        while self.cx.scopes.depth() > depth {
            self.cx.scopes.pop();
        }
        let (field_scope, fields, size_bytes) = lines?;
        if !defers.is_empty() {
            return Err(ParseError::DeferInTypeBody);
        }
        self.expect_close()?;
        let layout = crate::identities::meta::record_layout(
            self.rt.store,
            field_scope,
            fields,
            size_bytes,
            scope,
            def.parse_rank,
            def.assoc,
        );
        let node = def.self_type;
        // SAFETY: `node` was minted at the open with no record; nothing reads one until now.
        unsafe { dyad::set_value(node, layout.cast()) };
        if let Some(rank) = def.lex_rank {
            // The rank is the name's, not the type's: it goes on the binding of
            // the declaration this body is the value of.
            let Some(&binding) = self.cx.filling.last() else {
                return Err(ParseError::LexRankNeedsName);
            };
            // SAFETY: `binding` is a binding dyad `:=` minted before driving its value, live for the whole drive.
            unsafe { Binding::set_lex_rank(binding, rank) };
        }
        if !def.ctor.is_null() {
            // SAFETY: `node` was just built; nothing has read its slot.
            unsafe { crate::identities::meta::install_constructor(node, def.ctor) };
        }
        if !def.run_body.is_null() {
            // SAFETY: `node` was just built; `def.run_body` is the `lex` node the fill built.
            unsafe { crate::identities::meta::install_run_body(node, def.run_body) };
        }
        // A type whose fields own what they point to writes the teardown that frees it
        // (DESIGN ›A type whose fields carry teardowns must write its own destructor‹).
        // SAFETY: `fields` is the block's array node, its items field dyads typed null or by a type node.
        let owning_field = unsafe {
            crate::identities::array::items(fields).iter().any(|&f| {
                let ty = dyad::ty(f);
                !ty.is_null()
                    && crate::identities::numtype::is_pointer_type(ty)
                    && !crate::identities::meta::destructor_of(ty).is_null()
            })
        };
        if (owning_field || def.owns_node_field) && def.instances_drop.is_null() {
            return Err(ParseError::OwningFieldNeedsDrop);
        }
        if !def.instances_drop.is_null() {
            // SAFETY: `node` was just built with a record layout; the drop is the fill's fn.
            unsafe { crate::identities::meta::install_instances_drop(node, def.instances_drop) };
        }
        Ok(node)
    }

    /// A `type (…)` in a body that runs later reads that run's values, so it is
    /// lexed now and built each time it runs (DESIGN ›Deferral is authored‹).
    fn hold_type_body(&mut self) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let (start, len) = self.body_text_extent()?;
        let source = self.cx.source;
        let text = crate::identities::string::build_text(
            self.rt.store,
            types.string_,
            &source.as_bytes()[start - 1..start + len + 1],
        );
        // SAFETY: `text` is the string node just built; its bytes live for the store.
        let held = unsafe { crate::identities::string::text(text) };
        let held = std::str::from_utf8(held).expect("copied from the source text");
        let mut fragment = self.lex_body_fragment(held)?;
        // A slot word of a type around this one names that type's slot; the held body's
        // own are declared when it is built, so its cells wait to be resolved then.
        for (i, cell) in fragment.cells().iter().enumerate() {
            let id = cell.identity(types);
            if !cell.is_fresh() && self.cx.definitions.iter().any(|d| d.slots.contains(&id)) {
                let fresh = self.rt.store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
                fragment.set_dyad(i as isize, fresh);
            }
        }
        let cells = Box::into_raw(Box::new(fragment));
        let scope = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        Ok(crate::identities::held_type::build(
            self.rt.store,
            types,
            text,
            cells,
            scope,
            self.cx.frames.len(),
        ))
    }

    /// Build the held `type (…)` `node` now, inside the run that reached it: its
    /// cells constructed over the scopes open where it was written, its reads
    /// of that function's names reading this call's frame. Everything the
    /// pass had open is set aside and restored.
    ///
    /// # Safety
    /// `node` must be a held type node from the store; the call it was written
    /// in must be the innermost activation.
    pub(super) unsafe fn construct_held_type(
        &mut self,
        node: DyadPtr,
    ) -> Result<DyadPtr, crate::run::RunError> {
        let (text, cells, scope, depth) = crate::identities::held_type::parts(node);
        let bytes = crate::identities::string::text(text);
        let text: &'a str = std::str::from_utf8(bytes).expect("copied from the source text");
        let mut cells = (*cells).cells();
        for cell in &mut cells {
            if cell.is_fresh() {
                cell.dyad = self.rt.store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
            }
        }
        let mut chain = Vec::new();
        let mut at = scope;
        while !at.is_null() {
            chain.push(at);
            at = crate::identities::scope::parent_of(at);
        }
        let mut nested = ScopeStack::new();
        for &scope in chain.iter().rev() {
            nested.push(scope);
        }
        let base_depth = nested.depth();
        // One frame per function open where the body was written, so a read of
        // that function's names is no capture; nothing is claimed in them.
        let frames = (0..depth)
            .map(|_| OpenFn {
                size: 0,
                below: 0,
                outer: Vec::new(),
                open_below: 0,
                returns: Vec::new(),
            })
            .collect();

        let cx = Context {
            frames,
            held_depth: depth,
            feed: Some(Feed { cells, next: 0, base_depth, in_run_order: true }),
            ..Context::nested(&self.cx, text, nested)
        };
        let type_ = self.types.type_;
        let g = self.enter(cx);
        let built = g.0.parse_type_body(type_);
        let built_pos = g.0.cx.pos;
        drop(g);

        // A type of node values minted while a run type's parse runs is the value that type's
        // node makes, so a bracket taken into its place is built by one; stand-in for #152.
        if let Ok(ty) = built {
            let reader = self.cx.reader;
            if crate::identities::meta::is_record_type(reader)
                && !crate::identities::meta::run_body_of(reader).is_null()
                && crate::identities::meta::is_node_valued(ty, self.types.fn_type)
                && crate::identities::meta::is_record_type(ty)
            {
                // SAFETY: `ty` carries the record its body just built; `reader` is a type node.
                unsafe { crate::identities::meta::install_bracket_builder(ty, reader) };
            }
        }
        built.map_err(|e| {
            crate::run::RunError::MintFailed(Box::new(crate::report::render(
                "type body",
                text,
                built_pos,
                &crate::report::parse_message(&e),
            )))
        })
    }

    /// A use of one of the slot words, or of `drop`; whether the fill reaches
    /// a type being defined is `filling_definition`'s question.
    ///
    /// # Safety
    /// `target` must be a dyad from the store.
    pub(crate) unsafe fn slot_of(&self, target: DyadPtr) -> Option<SlotKind> {
        // The binding of the word's use, or the identity itself when its
        // constructor stood aside (`drop`).
        // SAFETY: `target` is a reduced dyad from the store.
        unsafe {
            let id = if dyad::ty(target) == self.types.binding_ {
                Binding::read(target).dyad
            } else {
                target
            };
            if id == self.types.drop_ {
                return Some(SlotKind::Drop);
            }
            self.cx
                .definitions
                .iter()
                .rev()
                .find_map(|def| def.slots.iter().position(|&m| m == id))
                .map(SlotKind::of)
        }
    }

    /// The current scope is the innermost open definition's own (DESIGN ›The
    /// constructor is a field‹): a `parse_rank = 3` inside a constructor's
    /// body is that function's own business, refused.
    pub(crate) fn filling_definition(&self) -> bool {
        self.cx.definitions.last().is_some_and(|def| self.cx.scopes.current() == Some(def.scope))
    }

    /// A literal molds to f64 without running; a concrete expression runs
    /// now, after everything parsed before it (a rank may read a variable).
    /// Anything else is `NonComptimeRank`.
    fn rank_value(&mut self, value: DyadPtr, read: DyadPtr) -> Result<f64, ParseError> {
        use crate::identities::numtype::NumType;
        // SAFETY: `value` is a reduced dyad from the store.
        let nt = match unsafe { crate::identities::numtype_of(self.types, value) } {
            crate::identities::Operand::Literal => None,
            crate::identities::Operand::Concrete(nt) => Some(nt),
            _ => return Err(ParseError::NonComptimeRank),
        };
        let bits = match nt {
            None => crate::identities::rational::mold_to(read, NumType::F64)
                .ok_or(ParseError::UncomputableLiteral)?,
            Some(_) => {
                self.drain()?;
                // SAFETY: `value` is a reduced dyad from the store.
                unsafe { self.run_on_pass(value) }.map_err(|_| ParseError::NonComptimeRank)?
            }
        };
        Ok(match nt {
            None | Some(NumType::F64) => f64::from_bits(bits as u64),
            Some(NumType::F32) => f64::from(f32::from_bits(bits as u32)),
            Some(_) => bits as f64,
        })
    }

    /// A slot `type` declared is filled with `=` (DESIGN ›The constructor is
    /// a field‹): the parse_rank must be known at the definition; a body slot
    /// takes a bracket, `slot_body_fill`'s; the fill is a silent statement.
    ///
    /// # Safety
    /// `value` must be a dyad from the store.
    pub(crate) unsafe fn slot_fill(
        &mut self,
        kind: SlotKind,
        target: DyadPtr,
        value: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        if matches!(kind, SlotKind::Parse | SlotKind::Run | SlotKind::Drop) {
            return Err(ParseError::SlotNeedsBody(kind));
        }
        // SAFETY: `value` is a reduced dyad from the store.
        let read = unsafe { types.through(value) };
        // Computed before the open definition is borrowed, since computing it
        // may run what stands before it.
        let rank = match kind {
            SlotKind::ParseRank | SlotKind::LexRank => Some(self.rank_value(value, read)?),
            _ => None,
        };
        let def = self.cx.definitions.last_mut().expect("slot_of found an open definition");
        match kind {
            SlotKind::ParseRank => {
                if let Some(rank) = rank {
                    def.parse_rank = rank;
                }
            }
            SlotKind::LexRank => {
                if let Some(rank) = rank {
                    def.lex_rank = Some(rank);
                }
            }
            SlotKind::Associativity => {
                let assoc = if read == types.left_ {
                    Assoc::Left
                } else if read == types.right_ {
                    Assoc::Right
                } else {
                    return Err(ParseError::BadAssociativity);
                };
                def.assoc = assoc;
            }
            SlotKind::Parse | SlotKind::Run | SlotKind::Drop => {
                unreachable!("body slots and `drop` returned above")
            }
        }
        Ok(self.slot_declare(kind, target, value))
    }

    /// The line's item for a slot fill: a declare node over the slot word's
    /// use and the right side, declaring the definition's marker, which is how
    /// the body's close tells a fill from a member declaration.
    fn slot_declare(&mut self, kind: SlotKind, target: DyadPtr, rhs: DyadPtr) -> DyadPtr {
        let types = self.types;
        let def = self.cx.definitions.last().expect("a slot is filled inside a definition");
        let marker = match kind {
            SlotKind::Drop => def.drop_marker,
            kind => def.slots[kind as usize],
        };
        crate::identities::declare::build(
            self.rt.store,
            types.declare_,
            types.ops.declare_,
            target,
            rhs,
            marker,
        )
    }

    /// A slot body read bare, no parameter list (DESIGN ›Execution is function
    /// application‹): `parse` over a hidden `tape`/node record, `run` held as
    /// its lexed tape and constructed per field-type set (DESIGN ›Deferral is authored‹).
    pub(crate) fn slot_body_fill(
        &mut self,
        kind: SlotKind,
        target: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        match kind {
            SlotKind::Parse => {
                let at = self.cx.pos;
                // `tape`, centred on the appearance, and unnamed, the node `tape[0]`
                // reaches: fresh when the type itself appeared, the value when a value did.
                let (input, params) = self.hidden_param_record(
                    &[(Some("tape"), types.tape.parsing_tape), (None, types.dyad_)],
                    at,
                )?;
                let def = self.cx.definitions.last_mut().expect("checked above");
                def.this_param = params[1];
                def.tape_param = params[0];
                def.in_parse = true;
                // SAFETY: `input` was just built, its parameters unplaced; no declaration's placeholder is being filled.
                let f = unsafe {
                    self.fn_over_body(types.fn_type, input, types.void_, std::ptr::null_mut())
                };
                let def = self.cx.definitions.last_mut().expect("checked above");
                def.this_param = std::ptr::null_mut();
                def.in_parse = false;
                def.ctor = f?;
                let f = def.ctor;
                Ok(self.slot_declare(SlotKind::Parse, target, f))
            }
            SlotKind::Run => {
                // The body with its brackets is copied into the store (a
                // source may not outlive the type, the store does) and lexed
                // once, now, against the scopes open here (DESIGN ›Deferral is authored‹).
                let (start, len) = self.body_text_extent()?;
                let source = self.cx.source;
                let text = crate::identities::string::build_text(
                    self.rt.store,
                    types.string_,
                    &source.as_bytes()[start - 1..start + len + 1],
                );
                // SAFETY: `text` is the string node just built; its bytes live for the store.
                let held = unsafe { crate::identities::string::text(text) };
                let held = std::str::from_utf8(held).expect("copied from the source text");
                let fragment = self.lex_body_fragment(held)?;
                let cells = Box::into_raw(Box::new(fragment));
                let body = crate::identities::run_body::build(self.rt.store, types, text, cells);
                self.cx.definitions.last_mut().expect("checked above").run_body = body;
                Ok(self.slot_declare(SlotKind::Run, target, body))
            }
            SlotKind::Drop => {
                // One parameter, unnamed: the value an owner's teardown hands in.
                let at = self.cx.pos;
                let (input, params) = self.hidden_param_record(&[(None, types.dyad_)], at)?;
                let def = self.cx.definitions.last_mut().expect("checked above");
                def.this_param = params[0];
                // SAFETY: `input` was just built, its parameter unplaced; no declaration's placeholder is being filled.
                let f = unsafe {
                    self.fn_over_body(types.fn_type, input, types.void_, std::ptr::null_mut())
                };
                let def = self.cx.definitions.last_mut().expect("checked above");
                def.this_param = std::ptr::null_mut();
                def.instances_drop = f?;
                let f = def.instances_drop;
                Ok(self.slot_declare(SlotKind::Drop, target, f))
            }
            _ => unreachable!("`=` reads a bare body for `parse`, `run` and `drop` only"),
        }
    }

    /// A parameter record no text spelled: each name declared in its own
    /// scope and checked against every open scope, since the body reopens it;
    /// value slots left null for `fn_over_body` to place. `at` is the error position.
    pub(super) fn hidden_param_record(
        &mut self,
        params: &[(Option<&str>, DyadPtr)],
        at: usize,
    ) -> Result<(DyadPtr, Vec<DyadPtr>), ParseError> {
        let scope = self.open_scope();
        let mut fields = Vec::with_capacity(params.len());
        let mut declared = Ok(());
        for &(name, ty) in params {
            let field = self.rt.store.alloc_raw(ty, std::ptr::null_mut());
            // A parameter with no name is a place the body reaches another
            // way: a receiver, or a run body's field of type `type`.
            if let Some(name) = name {
                declared = declared.and_then(|()| self.declare_name(name, field, at).map(|_| ()));
            }
            fields.push(field);
        }
        self.cx.scopes.pop();
        declared?;
        let fields_arr = crate::identities::array::build(self.rt.store, self.types.array_, &fields);
        // SAFETY: each `ty` is a type node from the store.
        let size_bytes = fields.iter().map(|&f| unsafe { field_width(dyad::ty(f)) }).sum();
        let record = crate::identities::meta::record_layout(
            self.rt.store,
            scope,
            fields_arr,
            size_bytes,
            std::ptr::null_mut(),
            crate::identities::meta::prec::APPLY,
            Assoc::Left,
        );
        Ok((self.rt.store.alloc_raw(self.types.type_, record.cast()), fields))
    }

    /// The run of a node whose type holds its `run` as a lexed body (DESIGN
    /// ›Deferral is authored‹): the function for its field-type set, found on
    /// the type or built now; a field whose type is unknown leaves the slot empty.
    ///
    /// # Safety
    /// `node` must be the node `run_logos_ctor` minted for a type whose
    /// record carries a run body, `[field…, null, spec]`.
    pub(crate) unsafe fn resolve_specialization(
        &mut self,
        node: DyadPtr,
        at: usize,
        spelling: &str,
    ) -> Result<Option<DyadPtr>, ParseError> {
        let ty = dyad::ty(node);
        let held = crate::identities::meta::run_body_of(ty);
        if held.is_null() {
            return Ok(None);
        }
        let Some(key) = self.field_type_key(ty, node)? else {
            return Ok(None);
        };
        let mut spec = crate::identities::run_body::lookup(held, &key);
        if spec.is_null() {
            spec = self.construct_run_body(ty, held, &key, at, spelling)?;
        }
        crate::identities::run_body::set_spec(node, spec);
        // The node is a call of the function: a use of every outer name the
        // body reads, checked before a comptime node runs it.
        if let Err(e) = self.check_call_reads(spec) {
            self.cx.pos = at;
            return Err(e);
        }
        if let Some(folded) = self.fold_comptime_node(ty, node, spec)? {
            return Ok(Some(folded));
        }
        // A run whose result is copied out gets the slot it is copied into.
        let out = *(dyad::value(spec) as *const DyadPtr).add(FN_OUTPUT);
        let Some(width) = crate::identities::by_copy::record_width(self.types, out) else {
            return Ok(None);
        };
        let slot = self.alloc_local(out, width);
        Ok(Some(crate::identities::by_copy::build_result(self.rt.store, self.types, slot, node)))
    }

    /// A node every value field of which is a literal is comptime, its
    /// constructor a partial evaluator (DESIGN ›Deferral is authored‹): it runs
    /// now and its literal stands in its place, so the result molds where it lands.
    ///
    /// # Safety
    /// As `resolve_specialization`; `spec` the node's function.
    unsafe fn fold_comptime_node(
        &mut self,
        ty: DyadPtr,
        node: DyadPtr,
        spec: DyadPtr,
    ) -> Result<Option<DyadPtr>, ParseError> {
        use crate::identities::read::{read_kind, Read};
        let types = self.types;
        let fields = crate::identities::array::items(crate::identities::meta::record_fields_of(ty));
        let slots = dyad::value(node) as *const DyadPtr;
        for (i, &field) in fields.iter().enumerate() {
            if dyad::ty(field) == types.type_ {
                continue;
            }
            let slot = types.through(*slots.add(i));
            let comptime = match read_kind(types, slot) {
                Read::Literal => true,
                Read::Scalar(_) => !crate::dyad::is_place(dyad::value(slot)),
                // A literal handed on by `rational_number`, or into a rational field.
                Read::Executable(_) if dyad::ty(slot) == types.by_copy.out => {
                    let expr = types.through(*(dyad::value(slot) as *const DyadPtr));
                    dyad::ty(expr) == types.rational && !crate::dyad::is_place(dyad::value(expr))
                }
                _ => false,
            };
            if !comptime {
                return Ok(None);
            }
        }
        let fields = dyad::value(spec) as *const DyadPtr;
        if fields.is_null() {
            return Ok(None);
        }
        let out = *fields.add(FN_OUTPUT);
        let numeric = crate::identities::is_numtype_node(types, out);
        if !numeric && out != types.rational {
            return Ok(None);
        }
        let failed = |e: crate::run::RunError| {
            ParseError::ConstructorFailed(Box::new(crate::report::run_message(&e)))
        };
        let folded = if numeric {
            let bits = self.run_on_pass(node).map_err(failed)?;
            self.scalar_value(crate::identities::numtype::of_type_node(out), bits)
        } else {
            // A rational result is copied out: here into a buffer, then a literal.
            let mut bytes = [0u8; 16];
            self.run_on_pass_into(spec, node, bytes.as_mut_ptr()).map_err(failed)?;
            let (num, den) = crate::identities::rational::read_at(bytes.as_ptr());
            crate::identities::rational::build_literal(self.rt.store, types.rational, num, den)
        };
        Ok(Some(folded))
    }

    /// One type per instance field in order (DESIGN ›Execution is function
    /// application‹): a typed field's type, a `type` field's held type, or for
    /// an untyped hole the written value's type. `None` while one is unknown.
    ///
    /// # Safety
    /// As `resolve_specialization`; `ty` the node's type.
    unsafe fn field_type_key(
        &mut self,
        ty: DyadPtr,
        node: DyadPtr,
    ) -> Result<Option<Vec<DyadPtr>>, ParseError> {
        use crate::identities::{numtype_of, Operand};
        let types = self.types;
        let fields = crate::identities::array::items(crate::identities::meta::record_fields_of(ty));
        let slots = dyad::value(node) as *mut DyadPtr;
        let mut key = Vec::with_capacity(fields.len());
        for (i, &field) in fields.iter().enumerate() {
            let declared = dyad::ty(field);
            let slot = *slots.add(i);
            if slot.is_null() {
                return Ok(None);
            }
            let entry = if declared == types.type_ {
                let held = types.through(slot);
                if !crate::identities::is_type_value(types, held) {
                    return Ok(None);
                }
                held
            } else if !declared.is_null()
                // A field typed as a bracket holds its lines, evaluated as an untyped one's.
                && declared != types.square_brackets
                && declared != types.scope
            {
                // A literal stays as it stands in a rational field: its bytes are the value's.
                if matches!(numtype_of(types, slot), Operand::Literal) {
                    let lit = types.through(slot);
                    if crate::identities::is_numtype_node(types, declared) {
                        *slots.add(i) = crate::identities::commit_literal_to(
                            self.rt.store,
                            types,
                            lit,
                            declared,
                        )?;
                    }
                }
                declared
            } else if crate::identities::rational::is_rational_value(types, slot) {
                types.rational
            } else if let Some(bracket) = self.bracket_type(slot) {
                if dyad::ty(slot) != types.tape.bracket_arg {
                    *slots.add(i) =
                        crate::identities::tape::build_bracket_arg(self.rt.store, types, slot)?;
                }
                bracket
            } else {
                match numtype_of(types, slot) {
                    Operand::Concrete(nt) => types.numtypes[nt as usize],
                    // The literal's own type; its bytes are the value the call copies.
                    Operand::Literal => types.rational,
                    Operand::Pointer(_) | Operand::NonNumeric => return Ok(None),
                }
            };
            key.push(entry);
        }
        Ok(Some(key))
    }

    /// The kind of bracket an untyped field holds, written or already wrapped to run per node.
    ///
    /// # Safety
    /// `slot` must be a dyad from the store.
    unsafe fn bracket_type(&self, slot: DyadPtr) -> Option<DyadPtr> {
        let types = self.types;
        let slot = types.through(slot);
        let bracket = if dyad::ty(slot) == types.tape.bracket_arg {
            *(dyad::value(slot) as *const DyadPtr)
        } else {
            slot
        };
        let ty = dyad::ty(bracket);
        (ty == types.scope || ty == types.square_brackets).then_some(ty)
    }
}
