// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Type bodies: the field list, the slot fills and `static` members, a held `type (…)`
//! built at its run, and a run type's specialization and comptime fold.

use super::*;
use crate::dyad;
use crate::identities::type_mint::{self, KeyPart};
use crate::start_graph::{Carry, In, Out};

/// What a `type (…)` body's lines have filled so far: the head the type node
/// takes at the close, its one set of slots, and its values' fields. Also the `{ … }` a `fn`
/// written in it is read in, whose value the `fn` works on as a `static` function does.
#[derive(Clone)]
pub(super) struct OpenType {
    pub(super) scope: DyadPtr,
    /// The five slot words as this definition's own markers, declared into the body's scope.
    slots: [DyadPtr; 5],
    /// The marker a `free = (…)` line declares; `free` itself is the core word.
    free_marker: DyadPtr,
    parse_rank: f64,
    /// From a `lex_rank = …` line; written onto the declaration's binding at
    /// the close.
    lex_rank: Option<f64>,
    assoc: Assoc,
    ctor: DyadPtr,
    /// From a `run = (…)` line: the body lexed once into its cells; a node of
    /// the type constructs it per field-type set.
    run_body: DyadPtr,
    /// The hidden receiver of the body being read, a parse's node or a `free`'s or
    /// `static` function's value; null otherwise.
    pub(super) this_param: DyadPtr,
    /// `this_param` is a parse's node, whose fields are reached through `tape[0]`, never bare.
    pub(super) in_parse: bool,
    /// `this_param` is the value a `free` body tears down, whose fields it frees.
    pub(super) in_free: bool,
    /// The parse body's `tape` parameter while `in_parse`.
    pub(super) tape_param: DyadPtr,
    /// The parse body read so far stamps a type it gets as it runs, so its node's fields are
    /// reached by name from here.
    pub(super) stamps_at_run: bool,
    /// The `static` function being read has read a field of its value.
    pub(super) read_receiver: bool,
    /// The `static` function being read has written a field of its value, itself or through a
    /// bare call.
    pub(super) wrote_receiver: bool,
    /// The fields' scope and the fields declared so far: what a body written
    /// below them reaches by name.
    pub(super) block: (DyadPtr, Vec<DyadPtr>),
    /// From a `free = (…)` line: the `fn` over the value an owner's teardown runs.
    instances_free: DyadPtr,
    /// The type node being defined, its record written at the close: what its name means in its parse.
    pub(super) self_type: DyadPtr,
    /// The `static` members declared so far, in written order.
    statics: Vec<DyadPtr>,
}

impl Carry for OpenType {
    fn carry(&self, out: &mut Out) {
        let OpenType {
            scope,
            slots,
            free_marker,
            parse_rank,
            lex_rank,
            assoc,
            ctor,
            run_body,
            this_param,
            in_parse,
            in_free,
            tape_param,
            stamps_at_run,
            read_receiver,
            wrote_receiver,
            block,
            instances_free,
            self_type,
            statics,
        } = self;
        (*scope, *slots, *free_marker).carry(out);
        (*parse_rank, *lex_rank, *assoc).carry(out);
        (*ctor, *run_body, *this_param).carry(out);
        (*in_parse, *in_free, *tape_param).carry(out);
        (*stamps_at_run, *read_receiver, *wrote_receiver).carry(out);
        block.carry(out);
        (*instances_free, *self_type).carry(out);
        statics.carry(out);
    }

    fn uncarry(inp: &mut In) -> Self {
        let (scope, slots, free_marker) = Carry::uncarry(inp);
        let (parse_rank, lex_rank, assoc) = Carry::uncarry(inp);
        let (ctor, run_body, this_param) = Carry::uncarry(inp);
        let (in_parse, in_free, tape_param) = Carry::uncarry(inp);
        let (stamps_at_run, read_receiver, wrote_receiver) = Carry::uncarry(inp);
        let block = Carry::uncarry(inp);
        let (instances_free, self_type) = Carry::uncarry(inp);
        OpenType {
            scope,
            slots,
            free_marker,
            parse_rank,
            lex_rank,
            assoc,
            ctor,
            run_body,
            this_param,
            in_parse,
            in_free,
            tape_param,
            stamps_at_run,
            read_receiver,
            wrote_receiver,
            block,
            instances_free,
            self_type,
            statics: Carry::uncarry(inp),
        }
    }
}

impl OpenType {
    /// The `{ … }` of `scope`, its value of the record `ty`, read where a `fn` written in it is:
    /// `fields` its names written above the `fn` that hold bytes, the value's first fields in
    /// order. It fills no slot.
    pub(super) fn kept(scope: DyadPtr, fields: Vec<DyadPtr>, ty: DyadPtr) -> Self {
        let none = std::ptr::null_mut();
        OpenType {
            scope,
            slots: [none; 5],
            free_marker: none,
            parse_rank: crate::identities::meta::prec::APPLY,
            lex_rank: None,
            assoc: Assoc::Left,
            ctor: none,
            run_body: none,
            this_param: none,
            in_parse: false,
            in_free: false,
            tape_param: none,
            stamps_at_run: false,
            read_receiver: false,
            wrote_receiver: false,
            block: (scope, fields),
            instances_free: none,
            self_type: ty,
            statics: Vec::new(),
        }
    }

    /// Which of this definition's slot words `id` is, by `SLOT_NAMES` position.
    fn slot_index(&self, id: DyadPtr) -> Option<usize> {
        if id.is_null() {
            return None;
        }
        self.slots.iter().position(|&m| m == id)
    }

    /// The `static`, `run` or `free` body being read uses its value as [`RECEIVER_READS`] and
    /// [`RECEIVER_WRITES`] in `bits` say: by a bare field, or by a bare call of a function that
    /// uses its own (›A bare call of a `static` function works on the value the calling body is
    /// about‹ [[R238]]). A parse's node is no value it uses.
    pub(super) fn note_receiver_use(&mut self, bits: u64) {
        if !self.in_parse {
            self.read_receiver |= bits & RECEIVER_READS != 0;
            self.wrote_receiver |= bits & RECEIVER_WRITES != 0;
        }
    }
}

/// The slot words a type body declares into its own scope, in `SlotKind`
/// order (DESIGN ›A type body describes one level‹). `free` is not here: it is
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
    /// The `free` keyword left of `=`: no spelling in `SLOT_NAMES`, since the
    /// word is the statement's.
    Free = 5,
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

/// What a list of field lines is read for. A parameter takes no `pub`, which a read from outside
/// asks of a member and a field; only a type body's lines fill slots, declare `static` members
/// and give a field a default.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldList {
    Parameters,
    Members,
    TypeBody,
}

/// The name that gives a formed type its identity: its binding, and for a name declared in a held
/// body while it is built, that body's held node and where the name's value starts in its text.
#[derive(Clone, Copy)]
struct OwnName {
    binding: DyadPtr,
    written: Option<(DyadPtr, usize)>,
}

impl OwnName {
    const NONE: OwnName = OwnName { binding: std::ptr::null_mut(), written: None };

    fn is_none(&self) -> bool {
        self.binding.is_null()
    }
}

/// Each field's offset inside a value of `ty`, written onto the field's binding, and the value's
/// extent: a field `bytes` gives a byte starts there, and every other at the next byte its type's
/// alignment allows after the field written before it, as C lays out a struct; the size reaches
/// the end of the field that ends last, rounded up to the widest field's alignment, which is the
/// record's (›Every name of a scope is seen from anywhere in it; written order counts in four
/// cases only‹ [[R372]], ›Inside a type body, `layout` gives a field its type and its byte: `a :=
/// layout (mut i32, 1)`‹ [[R385]], ›`.size_bytes` is a `u64`‹ [[R282]]). `None` where a field
/// would end past the store's whole room, which no value outgrows.
///
/// # Safety
/// `ty` must be a type node from the store, `fields` its field holes and `bindings` their
/// bindings in the same order; `bytes` holds a byte or none for each field, or is empty.
unsafe fn lay_out_fields(
    types: &Core,
    ty: DyadPtr,
    fields: &[DyadPtr],
    bindings: &[DyadPtr],
    bytes: &[Option<u64>],
) -> Option<(Vec<usize>, crate::identities::meta::Extent)> {
    let (mut end, mut size, mut align) = (0usize, 0usize, 1);
    let mut offsets = Vec::with_capacity(fields.len());
    for (k, (&field, &binding)) in fields.iter().zip(bindings).enumerate() {
        let t = crate::identities::hole::type_in(field);
        let field_align = crate::identities::read::place_align(types, t);
        let offset = match bytes.get(k).copied().flatten() {
            Some(byte) => usize::try_from(byte).ok()?,
            None => end.checked_next_multiple_of(field_align)?,
        };
        Binding::set_field_offset(binding, ty, offset);
        end = offset.checked_add(crate::identities::by_copy::place_width(types, t))?;
        if end > crate::store::ARENA_CAP {
            return None;
        }
        size = size.max(end);
        align = align.max(field_align);
        offsets.push(offset);
    }
    let size_bytes = size.checked_next_multiple_of(align)? as u64;
    Some((offsets, crate::identities::meta::Extent { size_bytes, align }))
}

impl<'a> Parser<'a> {
    /// A function's `( field-list )` as a record node whose value is a `RECORD_TAG` record
    /// holding the scope, the `fields` array and the packed `size_bytes`, and the call as its
    /// constructor (›`fn` is not a primitive: `fn (…) -> T` is a function's type, and its body
    /// the value‹ [[R199]]). Fresh field names are read raw, which is why the list has its own
    /// sub-parse.
    pub fn parse_record(&mut self) -> Result<DyadPtr, ParseError> {
        self.parse_record_taking(None).map(|(input, _)| input)
    }

    /// `parse_record` with an unnamed field of type `leading` first, and the fields'
    /// bindings in order.
    pub(super) fn parse_record_taking(
        &mut self,
        leading: Option<DyadPtr>,
    ) -> Result<(DyadPtr, Vec<DyadPtr>), ParseError> {
        let (scope, fields_arr, bindings) = self.parse_field_list(leading)?;
        let input = self.rt.store.alloc_leaf(self.types.type_);
        let call = self.types.call_parse;
        Ok((
            self.record_node(input, scope, fields_arr, &bindings, std::ptr::null_mut(), call)?,
            bindings,
        ))
    }

    /// `input`, a type node with no record yet, made the record node over a field list: its
    /// fields laid out onto their bindings, then the record holding the scope, the fields, their
    /// bindings, their total and `body`, the scope its `static` members are read in, null for none,
    /// and `parse` in its constructor slot.
    fn record_node(
        &mut self,
        input: DyadPtr,
        scope: DyadPtr,
        fields: DyadPtr,
        bindings: &[DyadPtr],
        body: DyadPtr,
        parse: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        // SAFETY: the bindings were just minted for these fields; `input` is the record node.
        let laid = unsafe {
            let holes = crate::identities::array::items(fields);
            lay_out_fields(self.types, input, holes, bindings, &[])
        };
        let (_, extent) = laid.ok_or(ParseError::LayoutTooFar)?;
        let bindings = crate::identities::array::build(self.rt.store, self.types.array_, bindings);
        let record = crate::identities::meta::record_layout(
            self.rt.store,
            scope,
            fields,
            bindings,
            extent,
            body,
            crate::identities::meta::prec::APPLY,
            Assoc::Left,
            parse,
        );
        // SAFETY: `input` was minted above with no record; nothing reads one until now.
        unsafe { dyad::set_head(input, record.cast()) };
        Ok(input)
    }

    /// The record a `{ … }` is a value of: each live name of `scope` that holds bytes a field of
    /// its spelling, type and gates in written order, and the block of the `{ … }` that made it
    /// the record's body, so a name that holds none, a `fn` or a type, reads through the value as
    /// a `static` member does (›`{ … }` outside text is a scope kept as a value, read with `.`‹
    /// [[R374]]). Where every name holds bytes, the record is the mint of its shape, the names'
    /// spellings, gates and types in written order, which `type_mint` keeps (›Every type is minted
    /// by its contents through `type_mint`; the name that holds a `type ( … )` gives it its
    /// identity‹ [[R383]]); a name that holds none is a type of its own (›`fn` is not a
    /// primitive: a function is a type in this same shape‹ [[R199]]), and so is the record read
    /// through it, laid out on `kept.ty` when a `fn` written here needed it. A `fn` that read the
    /// value read its fields by their place among the names then, so those names must still be
    /// the first fields. Returns the record with the names its fields are made from.
    ///
    /// # Safety
    /// `scope` must be the innermost open scope, the block whose lines were just read, and `kept`
    /// what its lines noted.
    pub(super) unsafe fn kept_record(
        &mut self,
        scope: DyadPtr,
        kept: KeptValue,
    ) -> Result<(DyadPtr, Vec<DyadPtr>), ParseError> {
        use crate::identities::{hole, instance, read::carry_of};
        let types = self.types;
        let mut names = Vec::new();
        let mut own = !kept.ty.is_null();
        for name in instance::kept_names(types, scope) {
            if !types.is_storage(name) {
                own = true;
                continue;
            }
            if carry_of(types, types.type_of(name)).is_none() {
                return Err(ParseError::KeptEntry(Box::new(Binding::spelling(name))));
            }
            names.push(name);
        }
        for (reader, read) in &kept.readers {
            if let Some(i) = (0..read.len()).find(|&i| names.get(i) != Some(&read[i])) {
                return Err(ParseError::KeptReadEnded {
                    name: Box::new(Binding::spelling(read[i])),
                    reader: Box::new(Binding::spelling(*reader)),
                });
            }
        }
        let place = if own {
            None
        } else {
            // A name's type as it stands, its gate node with it, is its entry's.
            let mut key = Vec::with_capacity(names.len() * 2);
            for &name in &names {
                key.push(KeyPart::Name(Binding::spelling(name)));
                key.push(KeyPart::Node(Binding::gated(name)));
            }
            let (place, kept) = type_mint::find(self.rt.store, types, types.open_curly_, &key)
                .map_err(ParseError::Run)?;
            if let Some(shape) = kept {
                return Ok((shape, names));
            }
            Some(place)
        };
        let ty = if kept.ty.is_null() { self.rt.store.alloc_leaf(types.type_) } else { kept.ty };
        let field_scope = self.open_scope();
        let mut fields = Vec::with_capacity(names.len());
        let mut bindings = Vec::with_capacity(names.len());
        let declared = names.iter().try_for_each(|&name| {
            let gated = Binding::gated(name);
            let field = hole::build(self.rt.store, types.unknown, gated, std::ptr::null_mut());
            let binding = self.declare_field_name(&Binding::spelling(name), field, self.cx.pos)?;
            fields.push(field);
            bindings.push(binding);
            Ok(())
        });
        self.cx.scopes.pop();
        declared?;
        let fields = crate::identities::array::build(self.rt.store, types.array_, &fields);
        let record =
            self.record_node(ty, field_scope, fields, &bindings, scope, types.derived_parse)?;
        if let Some(place) = place {
            type_mint::keep(self.rt.store, types, place, record).map_err(ParseError::Run)?;
        }
        Ok((record, names))
    }

    /// `{`'s constructor: the lines up to its `}`, the scope kept as a value.
    pub(crate) fn construct_curly(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let (_, value) = self.parse_kept_block()?;
        self.take_closer(self.types.open_curly_)?;
        tape.place(value.dyad);
        Ok(Constructed::Placed)
    }

    /// `union`'s constructor: `( members )` as the union of those members, the mint `type_mint`
    /// keeps under `union` and each member's spelling and gated type in written order (›A union
    /// holds one of its members at a time: `union ( a := T, b := U )`‹ [[R379]]). For the name
    /// that `num := type union ( … )` declares it gives the name's own dyad over that mint's
    /// record (›Every type is minted by its contents through `type_mint`; the name that holds a
    /// `type ( … )` gives it its identity‹ [[R383]]).
    pub(crate) fn construct_union(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        // With nothing to its right `union` stands as itself, as every word that reads its right
        // side does.
        if !self.finds_right_side(tape) {
            return Ok(Constructed::Decline);
        }
        if !self.at_open() {
            return Err(ParseError::UnionNeedsMembers);
        }
        let own_name = self.name_given(&["type", "union"]);
        let opened_at = self.cx.pos;
        self.expect_open()?;
        let depth = self.cx.scopes.depth();
        let scope = self.open_scope();
        let (mut members, mut bindings, mut bytes) = (Vec::new(), Vec::new(), Vec::new());
        let lines =
            self.field_lines(FieldList::Members, scope, &mut members, &mut bindings, &mut bytes);
        // As a type's body stands below its fields, where a member is looked up through a value.
        let body = self.open_scope();
        while self.cx.scopes.depth() > depth {
            self.cx.scopes.pop();
        }
        lines.map_err(|e| match e {
            ParseError::StaticMisplaced | ParseError::BadDeclaredType => ParseError::UnionLine,
            other => other,
        })?;
        self.expect_close()?;
        if members.is_empty() || members.len() > crate::identities::union::MOST_MEMBERS {
            let e = match members.len() {
                0 => ParseError::UnionNeedsMembers,
                _ => ParseError::UnionTooManyMembers,
            };
            return Err(self.fail_at(opened_at, e));
        }
        // SAFETY: the members are holes just built with their bindings, their types type nodes.
        let node = unsafe {
            self.check_members(&members, &bindings).map_err(|e| self.fail_at(opened_at, e))?;
            let mint = self.mint_union(scope, body, &members, &bindings)?;
            self.named_type(own_name, mint, std::ptr::null_mut())?
        };
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// `layout`'s constructor, read in a type body: `( T, byte )` as the node that gives a field
    /// the type `T`, with the gate words written before it, and the byte it starts at in each
    /// value, a `u64` known here (›Inside a type body, `layout` gives a field its type and its
    /// byte: `a := layout (mut i32, 1)`‹ [[R385]]).
    pub(crate) fn construct_layout(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        if !self.finds_right_side(tape) {
            return Ok(Constructed::Decline);
        }
        let at = self.cx.pos;
        if !self.at_open() {
            return Err(ParseError::LayoutForm);
        }
        self.expect_open()?;
        let outer = std::mem::replace(&mut self.cx.field_hole, false);
        let parts = self.layout_parts();
        self.cx.field_hole = outer;
        let (gated, byte) = parts?;
        if !self.at_close() {
            return Err(self.fail_at(at, ParseError::LayoutForm));
        }
        self.expect_close()?;
        let u64_ = self.types.numtypes[crate::identities::NumType::U64 as usize];
        let byte = self.rt.store.alloc_blob(u64_, &byte.to_ne_bytes());
        let node = self.rt.store.alloc_words(self.types.layout_, &[gated, byte]);
        tape.place(node);
        Ok(Constructed::Placed)
    }

    /// The two lines of `layout ( … )`: the gated type and the byte.
    fn layout_parts(&mut self) -> Result<(DyadPtr, u64), ParseError> {
        let types = self.types;
        let (words, after) = self.gates_ahead();
        let start = self.cx.pos;
        self.cx.pos = after;
        let cell = self.parse_expression_cell()?;
        // SAFETY: a parsed cell holds a reduced dyad from the store.
        let ty = unsafe { crate::identities::type_identity_of(types, cell.dyad) };
        let ty = ty.ok_or_else(|| self.fail_at(start, ParseError::LayoutForm))?;
        let mut gates = cell.gates;
        for word in words {
            // SAFETY: a cell's gates are null or a gate mint, and `word` is a gate word.
            let joined =
                unsafe { crate::identities::gate::joined(self.rt.store, types, gates, word) };
            gates = joined.ok_or_else(|| self.fail_at(start, ParseError::DoubleGate))?;
        }
        // SAFETY: `ty` is a type node, `gates` null or a gate mint.
        let gated = unsafe { crate::identities::gate::gated(self.rt.store, types, gates, ty) };
        if !self.consume_separator() {
            return Err(self.fail_at(start, ParseError::LayoutForm));
        }
        let start = self.cx.pos;
        let cell = self.parse_expression_cell()?;
        let u64_ = types.numtypes[crate::identities::NumType::U64 as usize];
        // SAFETY: as above; `u64_` is a number type node.
        let byte = unsafe { crate::identities::tape::constant_value(types, cell.dyad, u64_) };
        let byte = byte.map_err(|_| self.fail_at(start, ParseError::LayoutByte))?;
        Ok((gated, byte as u64))
    }

    /// `T!`, a value of `T` or an error: `union ( value := T, err := error )`, that very mint,
    /// with no type of its own (›Target: errors are values (`T!`), handled by `if`, passed on by
    /// `try`…‹ [[R140]]). The members are declared as
    /// the written union's lines declare theirs.
    pub(crate) fn construct_fallible(
        &mut self,
        tape: &mut ParsingTape,
    ) -> Result<Constructed, ParseError> {
        let Some(left) = tape.at(-1).copied() else {
            return Err(ParseError::MissingOperand);
        };
        let lhs = self.operand_dyad(left)?;
        // SAFETY: `lhs` is a reduced dyad off the tape.
        let Some(value) = (unsafe { crate::identities::type_identity_of(self.types, lhs) }) else {
            return Err(self.fail_at(left.start, ParseError::FallibleNeedsType));
        };
        let mint = self.fallible_of(value, left.start)?;
        tape.remove(-1);
        tape.place(mint);
        Ok(Constructed::Gave)
    }

    /// `value!`, `union ( value := value, err := error )` (›Target: errors are values (`T!`),
    /// handled by `if`, passed on by `try`…‹ [[R140]]); an error is placed at `at`.
    pub(crate) fn fallible_of(&mut self, value: DyadPtr, at: usize) -> Result<DyadPtr, ParseError> {
        let depth = self.cx.scopes.depth();
        let scope = self.open_scope();
        let (mut members, mut bindings) = (Vec::new(), Vec::new());
        let declared = [("value", value), ("err", self.types.error.value)]
            .into_iter()
            .try_for_each(|(name, ty)| {
                let member = crate::identities::hole::build(
                    self.rt.store,
                    self.types.unknown,
                    ty,
                    std::ptr::null_mut(),
                );
                let binding = self.mint_binding(member, scope, name.as_bytes());
                self.enter_binding_in(scope, name, binding, at)?;
                members.push(member);
                bindings.push(binding);
                Ok(())
            });
        let body = self.open_scope();
        while self.cx.scopes.depth() > depth {
            self.cx.scopes.pop();
        }
        declared?;
        // SAFETY: the members are holes just built with their bindings, their types type nodes.
        unsafe {
            self.check_members(&members, &bindings).map_err(|e| self.fail_at(at, e))?;
            self.mint_union(scope, body, &members, &bindings)
        }
    }

    /// The union of `members`, declared in `scope` by `bindings` with `body` below them: the mint
    /// `type_mint` keeps under their names and types, made the first time (›Every type is minted
    /// by its contents through `type_mint`; the name that holds a `type ( … )` gives it its
    /// identity‹ [[R383]]).
    ///
    /// # Safety
    /// `members` must be holes from the store, each over a type node, and `bindings` their
    /// bindings in the same order.
    unsafe fn mint_union(
        &mut self,
        scope: DyadPtr,
        body: DyadPtr,
        members: &[DyadPtr],
        bindings: &[DyadPtr],
    ) -> Result<DyadPtr, ParseError> {
        let types = self.types;
        let mut key = Vec::with_capacity(members.len() * 2);
        for (&member, &binding) in members.iter().zip(bindings) {
            key.push(KeyPart::Name(Binding::spelling(binding)));
            key.push(KeyPart::Node(crate::identities::hole::gated_type_in(member)));
        }
        let (place, kept) = type_mint::find(self.rt.store, types, types.union.union, &key)
            .map_err(ParseError::Run)?;
        if let Some(mint) = kept {
            return Ok(mint);
        }
        let mint = self.rt.store.alloc_leaf(types.type_);
        let extent = crate::identities::union::lay_out(types, mint, members, bindings);
        let members = crate::identities::array::build(self.rt.store, types.array_, members);
        let bindings = crate::identities::array::build(self.rt.store, types.array_, bindings);
        let record = crate::identities::meta::formed_record(
            self.rt.store,
            types.union.union,
            scope,
            members,
            bindings,
            extent,
            body,
            types.derived_parse,
        );
        dyad::set_head(mint, record.cast());
        type_mint::keep(self.rt.store, types, place, mint).map_err(ParseError::Run)?;
        Ok(mint)
    }

    /// A union holds only members whose values move whole into its room, or a `void` one, which
    /// holds nothing (`void!`, ›Target: a constructor returns `void!`, so a user's syntax error is
    /// a recoverable value‹ [[R142]]), and a member's type takes no `pub`, every member being read
    /// from outside.
    ///
    /// # Safety
    /// `members` must be holes from the store, each over a type node, and `bindings` their
    /// bindings in the same order.
    unsafe fn check_members(
        &self,
        members: &[DyadPtr],
        bindings: &[DyadPtr],
    ) -> Result<(), ParseError> {
        for (&member, &binding) in members.iter().zip(bindings) {
            // ›A union holds one of its members at a time: `union ( a := T, b := U )`‹ [[R379]]
            let gated = crate::identities::hole::gated_type_in(member);
            if crate::identities::gate::has(gated, self.types.pub_) {
                return Err(ParseError::UnionMemberPub);
            }
            let ty = crate::identities::hole::type_in(member);
            if ty != self.types.void_ && crate::identities::read::carry_of(self.types, ty).is_none()
            {
                return Err(ParseError::UnionMemberUnkept(Box::new(Binding::spelling(binding))));
            }
        }
        Ok(())
    }

    /// A `fn`'s parameter list, in a scope of its own that the body reopens. Also the fields'
    /// bindings in order, the leading one's spelling-less; their offsets are the caller's to write
    /// once the record node exists.
    fn parse_field_list(
        &mut self,
        leading: Option<DyadPtr>,
    ) -> Result<(DyadPtr, DyadPtr, Vec<DyadPtr>), ParseError> {
        self.expect_open()?;
        let scope = self.open_scope();

        let mut fields = Vec::new();
        let mut bindings = Vec::new();
        if let Some(ty) = leading {
            let field = crate::identities::hole::build(
                self.rt.store,
                self.types.unknown,
                ty,
                std::ptr::null_mut(),
            );
            fields.push(field);
            bindings.push(self.mint_binding(field, scope, b""));
        }
        let mut bytes = Vec::new();
        let lines =
            self.field_lines(FieldList::Parameters, scope, &mut fields, &mut bindings, &mut bytes);
        self.cx.scopes.pop();
        lines?;
        self.expect_close()?;
        let fields_arr = crate::identities::array::build(self.rt.store, self.types.array_, &fields);
        Ok((scope, fields_arr, bindings))
    }

    /// The lines of a parameter list, of a union's members, or of a type body (DESIGN ›A type
    /// body describes one level‹): its fields among its slot fills, `static` members and prose.
    /// Each name is declared into `scope`, below the type's own in a type body, against every
    /// name open there. `bytes` gets, for each field, the byte a type body's `layout` gives it.
    fn field_lines(
        &mut self,
        list: FieldList,
        scope: DyadPtr,
        fields: &mut Vec<DyadPtr>,
        bindings: &mut Vec<DyadPtr>,
        bytes: &mut Vec<Option<u64>>,
    ) -> Result<(), ParseError> {
        let relaxed = list == FieldList::TypeBody;
        loop {
            if relaxed {
                // A type body's line has run as it was read, its `static` made there.
                self.cx.open.last_mut().expect("the type body's scope is open").runs.clear();
            }
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
                if self.prose_at()?.is_some() {
                    let at = self.cx.pos;
                    self.skip_prose()?;
                    if self.at_slot_fill() {
                        self.cx.pos = at;
                        self.lift_prose()?;
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
            // A gate word gates a value, so none stands where a line's name does.
            if word.is_some_and(|id| self.is_gate_word(id)) {
                self.cx.pos = start;
                return Err(ParseError::GateNeedsValue);
            }
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
            let assigns = matches!(self.peek_token(), Some((id, _)) if id == self.types.assign);
            if relaxed {
                let declares =
                    matches!(self.peek_token(), Some((id, _)) if id == self.types.declare_tok);
                let def = self.cx.definitions.last().expect("a relaxed field list is a type body");
                if word.is_some_and(|id| id == self.types.free_ || def.slot_index(id).is_some()) {
                    // A slot is one place per type already, so its fill takes no gate word.
                    if assigns {
                        self.slot_line(start)?;
                        continue;
                    }
                    self.cx.pos = start;
                    return Err(if declares {
                        ParseError::Resolve(ResolveError::Shadowed(name.to_string()))
                    } else {
                        ParseError::TypeBodyLine
                    });
                }
            }
            if !self.consume_token(self.types.declare_tok) {
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
                        // `=` writes what exists, so a fresh word before it names no slot.
                        None if assigns => ParseError::NoSuchSlot(name.to_string()),
                        None => ParseError::Resolve(ResolveError::Unknown(name.to_string())),
                        Some(_) => ParseError::TypeBodyLine,
                    });
                }
                return Err(self.untyped(start, relaxed));
            }
            // `name := static …`: one place stored with the type, not a field of the layout;
            // never a parameter, which each call fills.
            if self.static_ahead() {
                self.cx.pos = start;
                if !relaxed {
                    return Err(ParseError::StaticMisplaced);
                }
                self.static_member(start)?;
                continue;
            }
            // `name := T ?` declares the field's type through the hole `?` built, and so does
            // `name := T`. A default, `size := u64 0`, is a constant: the field's dyad is that
            // value, and each new node starts with it in the field's slot.
            let mut default = std::ptr::null_mut();
            let outer = std::mem::replace(&mut self.cx.field_hole, relaxed);
            let value = self.parse_expression_cell().and_then(|c| self.declared_value(c));
            self.cx.field_hole = outer;
            let cell = value?;
            // A field line's comments are the body's, as a `static` line's are; a parameter's
            // go to the line the `fn` stands in.
            if relaxed {
                let body = self.cx.definitions.last().expect("a relaxed field list is a type body");
                let open = self.cx.open.last_mut().expect("the type body's scope is open");
                for comment in std::mem::take(&mut open.prose) {
                    // SAFETY: the pending bindings were minted by this parser's declares.
                    unsafe { self.cx.scopes.settle_item(body.scope, comment) };
                }
            }
            let value = cell.dyad;
            if self.is_bare_unknown(&cell) {
                return Err(self.untyped(start, relaxed));
            }
            let mut gates = cell.gates;
            let mut byte = None;
            // SAFETY: `value` is a reduced dyad just parsed.
            let laid = unsafe { crate::identities::layout_of(self.types, value) };
            let logos = if let Some((gated, at)) = laid {
                if !relaxed {
                    self.cx.pos = start;
                    return Err(ParseError::LayoutOutsideField);
                }
                byte = Some(at);
                // SAFETY: a `layout` node holds a type node, and a cell's gates are null or a
                // gate mint.
                unsafe {
                    let (ty, own) = crate::identities::gate::parts(gated);
                    gates = crate::identities::gate::union(self.rt.store, self.types, gates, own);
                    ty
                }
            } else if cell.hole {
                // A type whose nodes run has no place for a field or a parameter: its `T ?` is an
                // empty node, not a hole.
                // SAFETY: `value` is the place `?` just built.
                if !unsafe { self.types.is_hole(value) } {
                    self.cx.pos = start;
                    return Err(ParseError::BadDeclaredType);
                }
                // SAFETY: as above.
                unsafe { crate::identities::hole::type_in(value) }
            } else {
                // SAFETY: `value` is a reduced dyad just parsed.
                let held = unsafe { self.types.through(value) };
                if relaxed
                    // SAFETY: as above.
                    && unsafe {
                        matches!(
                            crate::identities::read::read_kind(self.types, held),
                            crate::identities::read::Read::Scalar(_)
                        ) && !self.types.is_storage(held)
                    }
                {
                    // SAFETY: as above.
                    let ty = unsafe { dyad::ty(held) };
                    default = held;
                    ty
                } else {
                    self.cx.pos = start;
                    return Err(ParseError::BadDeclaredType);
                }
            };
            // An `own` parameter consumes its argument, which no call does yet.
            // SAFETY: `logos` is a type node.
            let owning = unsafe {
                crate::identities::numtype::is_pointer_type(logos)
                    && !crate::identities::meta::destructor_of(logos).is_null()
            };
            if list == FieldList::Parameters && owning {
                self.cx.pos = start;
                return Err(ParseError::OwnParameterNotInSeed);
            }
            let field =
                crate::identities::hole::build(self.rt.store, self.types.unknown, logos, default);
            // The field's name is not stored on the record: declaring it puts
            // a binding in the one name index (DESIGN ›Name resolution is scope-filtered‹).
            let binding = self.mint_binding(field, scope, name.as_bytes());
            self.enter_binding_in(scope, name, binding, start)?;
            // SAFETY: a cell's gates are null or a gate mint.
            let public =
                unsafe { crate::identities::gate::words_of(gates) }.contains(&self.types.pub_);
            if public && list == FieldList::Parameters {
                self.cx.pos = start;
                return Err(ParseError::PubParameter);
            }
            self.gate_name(binding, gates);
            fields.push(field);
            bindings.push(binding);
            bytes.push(byte);
            if !self.consume_separator() {
                break;
            }
        }
        Ok(())
    }

    /// `name := static value` in a type body, read from the name at `at`: one place stored with
    /// the type, declared in the body scope (DESIGN ›Two muts, and the storage partition‹). The
    /// fields and the members stay one scope for no-shadowing.
    fn static_member(&mut self, at: usize) -> Result<(), ParseError> {
        let body = self.cx.definitions.last().expect("a member is declared in a type body").scope;
        self.cx.last_declared = std::ptr::null_mut();
        let outer_member = self.cx.member_fn_depth.replace(self.cx.frames.len());
        let items = self.static_line();
        self.cx.member_fn_depth = outer_member;
        // The line's one declaration, with its prose lifted out beside it.
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
                return Err(ParseError::TypeBodyLine);
            }
        }
        let binding = self.cx.last_declared;
        let Some(item) = declared.filter(|_| !binding.is_null()) else {
            self.cx.pos = at;
            return Err(ParseError::TypeBodyLine);
        };
        self.cx.definitions.last_mut().expect("read above").statics.push(binding);
        // SAFETY: the pending bindings were minted by this parser's declares.
        unsafe { self.cx.scopes.settle_item(body, item) };
        Ok(())
    }

    /// The first `parse_next` lexes and constructs the segment and queues
    /// what it yielded; the rest are taken while queued, before anything
    /// further is lexed, so none is left for the enclosing body's loop.
    fn static_line(&mut self) -> Result<Vec<DyadPtr>, ParseError> {
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

    /// A slot fill in a type body, `parse = (…)` or `free = (…)`, parsed over
    /// the body's scope from the slot word at `start`.
    fn slot_line(&mut self, start: usize) -> Result<(), ParseError> {
        let body = self.cx.definitions.last().expect("a slot line is in a type body").scope;
        self.cx.pos = start;
        let items = self.static_line();
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

    /// The `#` token at the cursor, `(start, len)`, its text with it.
    fn prose_at(&mut self) -> Result<Option<(usize, usize)>, ParseError> {
        self.skip_whitespace();
        let source = self.cx.source;
        let start = self.cx.pos;
        if start >= source.len() {
            return Ok(None);
        }
        match self.token_at(start, false) {
            Ok(r) if r.identity == self.types.hash_ => Ok(Some((start, r.matched))),
            Err(e @ ParseError::Resolve(ResolveError::Unmatched { .. })) => Err(e),
            _ => Ok(None),
        }
    }

    fn skip_prose(&mut self) -> Result<(), ParseError> {
        while let Some((start, len)) = self.prose_at()? {
            self.cx.pos = start + len;
        }
        Ok(())
    }

    /// The comments at the cursor, settled into the body beside the line after them.
    fn lift_prose(&mut self) -> Result<(), ParseError> {
        let body = self.cx.definitions.last().expect("read inside a type body").scope;
        while let Some((start, len)) = self.prose_at()? {
            self.cx.pos = start + len;
            let node =
                self.construct_leaf(self.types.hash_, start, len)?.ok_or(ParseError::BadLiteral)?;
            // SAFETY: `node` is the comment node just built.
            unsafe { self.cx.scopes.settle_item(body, node) };
        }
        Ok(())
    }

    /// The next word is a slot word or `free` and `=` follows it: a slot's fill; nothing is
    /// consumed.
    fn at_slot_fill(&mut self) -> bool {
        if !self.at_slot_word() {
            return false;
        }
        let at = self.cx.pos;
        self.lex_spelling();
        let fill = matches!(self.peek_token(), Some((id, _)) if id == self.types.assign);
        self.cx.pos = at;
        fill
    }

    /// The next word is a slot word or `free`; nothing is consumed.
    fn at_slot_word(&mut self) -> bool {
        let at = self.cx.pos;
        let word = self.lex_spelling().and_then(|(start, len)| {
            let source = self.cx.source;
            self.cx.scopes.resolve(self.trie, &source[start..start + len]).ok().map(|r| r.identity)
        });
        self.cx.pos = at;
        let def = self.cx.definitions.last().expect("read inside a type body");
        word.is_some_and(|id| id == self.types.free_ || def.slot_index(id).is_some())
    }

    /// A slot fill's item declares the definition's own marker.
    fn is_slot_fill(&self, item: DyadPtr) -> bool {
        // SAFETY: `item` is a declare node from the store.
        let declared = unsafe { crate::identities::declare::declared_of(item) };
        self.cx.definitions.last().is_some_and(|def| {
            def.slot_index(declared).is_some()
                || (!declared.is_null() && def.free_marker == declared)
        })
    }

    /// One body for one level (DESIGN ›A type body describes one level‹): its
    /// lines fill the type's slots, declare its values' fields, and declare
    /// its `static` members; every other line must be prose, since nothing runs a type body later.
    /// In a body that runs later it is held, and built by the same code each time its node runs.
    pub fn parse_type_body(&mut self, id: DyadPtr) -> Result<DyadPtr, ParseError> {
        let own_name = self.name_given(&["type"]);
        if self.cx.runtime_depth > 0 {
            return self.hold_type_body(own_name.binding);
        }
        let node = self.rt.store.alloc_leaf(id);
        if !own_name.is_none() {
            // The body's lines name the type they are building (DESIGN ›An unknown spelling is
            // a node of type `unknown_name`; `:=` makes the binding, and the node comes with
            // the value‹ [[R251]]).
            // SAFETY: the binding is the one `:=` is filling, live for the whole drive.
            unsafe { Binding::set_dyad(own_name.binding, node) };
        }
        self.build_type_body(node, own_name)
    }

    /// Builds the type body at the cursor into `self_type`, a type node with no record yet, for
    /// `own_name`, none where no name holds the type.
    fn build_type_body(
        &mut self,
        self_type: DyadPtr,
        own_name: OwnName,
    ) -> Result<DyadPtr, ParseError> {
        self.expect_open()?;
        let opened_at = self.cx.pos;
        let depth = self.cx.scopes.depth();
        // The slot words are the fields `type` declares (identities/type.logos):
        // names on the body's lines and nowhere outside them, in a scope of
        // their own so that a member read through the type never finds them.
        // Each is a marker of this definition, declared against its siblings
        // alone, so that a body inside another's shadows its words. `layout`
        // stands beside them, one identity in every body.
        let words = self.open_scope();
        let slots = SLOT_NAMES.map(|_| {
            let head = crate::identities::meta::record(
                self.rt.store,
                crate::identities::meta::TYPEREC_TAG,
                crate::identities::meta::prec::INERT,
            );
            self.rt.store.alloc_head(self.types.type_, head)
        });
        let body_words = SLOT_NAMES.iter().copied().zip(slots);
        for (name, marker) in body_words.chain([("layout", self.types.layout_)]) {
            let binding = self.mint_binding(marker, words, name.as_bytes());
            // SAFETY: `binding` was minted by `Binding::alloc` just above.
            let declared = unsafe {
                Binding::set_body_word(binding);
                self.cx.scopes.declare_field(self.trie, name, binding)
            };
            if let Err(e) = declared {
                self.cx.scopes.pop();
                return Err(ParseError::Resolve(e));
            }
        }
        let head = crate::identities::meta::record(
            self.rt.store,
            crate::identities::meta::TYPEREC_TAG,
            crate::identities::meta::prec::INERT,
        );
        let free_marker = self.rt.store.alloc_head(self.types.type_, head);
        // The fields' scope stands below the type's own, so the body's lines are read in the
        // type's scope with its fields open around them.
        let field_scope = self.open_scope();
        let scope = self.open_scope();
        self.cx.definitions.push(OpenType {
            scope,
            slots,
            free_marker,
            parse_rank: crate::identities::meta::prec::APPLY,
            lex_rank: None,
            assoc: Assoc::Left,
            ctor: std::ptr::null_mut(),
            run_body: std::ptr::null_mut(),
            this_param: std::ptr::null_mut(),
            in_parse: false,
            in_free: false,
            tape_param: std::ptr::null_mut(),
            stamps_at_run: false,
            read_receiver: false,
            wrote_receiver: false,
            block: (field_scope, Vec::new()),
            instances_free: std::ptr::null_mut(),
            self_type,
            statics: Vec::new(),
        });
        // A `fn` literal on a slot's right side must not claim the enclosing
        // declaration's binding.
        let suppressed = self.take_pending_binding();
        // The body's declarations are pending until its close: a type is
        // comptime and its members are stored at the definition. Everything
        // before the body runs first, and a failure among its lines is the body's.
        let outer = self.drain();
        self.cx.open.push(OpenScope::default());
        let saved_depth = std::mem::replace(&mut self.cx.runtime_depth, 0);
        let (mut fields, mut bindings, mut bytes) = (Vec::new(), Vec::new(), Vec::new());
        let lines = outer.and_then(|_| {
            let list = FieldList::TypeBody;
            self.field_lines(list, field_scope, &mut fields, &mut bindings, &mut bytes)
        });
        let lines = lines.and_then(|()| {
            self.drain().map_err(|e| match e {
                ParseError::Run(r) => {
                    ParseError::TypeBodyFailed(Box::new(crate::report::run_message(&r)))
                }
                other => other,
            })
        });
        self.cx.runtime_depth = saved_depth;
        let held = self.cx.open.pop().expect("pushed above").exit;
        self.restore_pending_binding(suppressed);
        let def = self.cx.definitions.pop().expect("pushed above");
        while self.cx.scopes.depth() > depth {
            self.cx.scopes.pop();
        }
        lines?;
        if !held.is_empty() {
            return Err(ParseError::DeferInTypeBody);
        }
        self.expect_close()?;
        // SAFETY: the bindings were just minted for these fields; `def.self_type` is the type node.
        let laid = unsafe { lay_out_fields(self.types, def.self_type, &fields, &bindings, &bytes) };
        let (offsets, extent) =
            laid.ok_or_else(|| self.fail_at(opened_at, ParseError::LayoutTooFar))?;
        // SAFETY: `def` was just closed over these fields and their names.
        let contents = unsafe { self.type_contents(&def, &fields, &bindings, &offsets, opened_at) };
        let fields = crate::identities::array::build(self.rt.store, self.types.array_, &fields);
        let bindings = crate::identities::array::build(self.rt.store, self.types.array_, &bindings);
        let layout = crate::identities::meta::record_layout(
            self.rt.store,
            field_scope,
            fields,
            bindings,
            extent,
            scope,
            def.parse_rank,
            def.assoc,
            self.types.derived_parse,
        );
        let node = def.self_type;
        // SAFETY: `node` was minted at the open with no record; nothing reads one until now.
        unsafe { dyad::set_head(node, layout.cast()) };
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
        if def.instances_free.is_null() {
            // SAFETY: `node` was just given its record layout; each field's binding is a binding dyad.
            let unfreed: Vec<String> = unsafe {
                crate::identities::copy::owning_fields(self.types, node)
                    .map(|b| Binding::spelling(b))
                    .collect()
            };
            if !unfreed.is_empty() {
                return Err(ParseError::FieldsNeedFree(unfreed));
            }
        } else {
            // SAFETY: `node` was just built with a record layout; the free is the fill's fn.
            unsafe { crate::identities::meta::install_instances_free(node, def.instances_free) };
        }
        // `type_mint` itself is built before it keeps anything.
        let Some(contents) = contents.filter(|_| self.types.type_mint.is_read()) else {
            return Ok(node);
        };
        // SAFETY: the key's nodes are types, gates and functions of the body just built.
        unsafe {
            let types = self.types;
            let (place, kept) = type_mint::find(self.rt.store, types, types.type_, &contents)
                .map_err(ParseError::Run)?;
            let mint = match kept {
                Some(mint) => mint,
                None => {
                    let mint = self.mint_of(node, own_name);
                    type_mint::keep(self.rt.store, types, place, mint).map_err(ParseError::Run)?;
                    mint
                }
            };
            self.named_type(own_name, mint, node)
        }
    }

    /// The name that gives the type being read here its identity: the name of the declaration
    /// whose whole right side up to the cursor is `words`, the former's own spelling with `type`
    /// before it where the former gives a type, after the gate words that gate the value; none
    /// where no declaration holds it so (›Every type is minted by its contents through
    /// `type_mint`; the name that holds a `type ( … )` gives it its identity‹ [[R383]]).
    fn name_given(&self, words: &[&str]) -> OwnName {
        let gate = |word: &&str| {
            let found = self.cx.scopes.resolve(self.trie, word).ok();
            found.is_some_and(|r| r.matched == word.len() && self.is_gate_word(r.identity))
        };
        match (self.cx.filling.last(), self.cx.filling_at.last()) {
            (Some(&binding), Some(&at))
                if self.cx.source.get(at..self.cx.pos).is_some_and(|t| {
                    t.split_whitespace().skip_while(gate).eq(words.iter().copied())
                }) =>
            {
                let written = (!self.cx.held.is_null()).then_some((self.cx.held, at));
                OwnName { binding, written }
            }
            _ => OwnName::NONE,
        }
    }

    /// The mint `type_mint` keeps for the type just built at `node`: `node` itself where no
    /// name holds it, and beside a name's own dyad a dyad of its own over the same record, so a
    /// type no name holds is never the name's.
    ///
    /// # Safety
    /// `node` must be a type node carrying its record.
    unsafe fn mint_of(&mut self, node: DyadPtr, own_name: OwnName) -> DyadPtr {
        if own_name.is_none() {
            return node;
        }
        self.rt.store.alloc_head(self.types.type_, dyad::head(node))
    }

    /// What a formed type gives where its contents are the mint `mint`: the mint, or for the
    /// name that holds it the name's own dyad over the mint's record, which a value's type word
    /// then points at. A written name has one dyad per mint, which `type_mint` keeps under where
    /// the name is written, so a body that runs again finds it again; the first build makes it,
    /// of `node` where the builder has a dyad for the name already (›Every type is minted by its
    /// contents through `type_mint`; the name that holds a `type ( … )` gives it its identity‹
    /// [[R383]]).
    ///
    /// # Safety
    /// `mint` must be a type node carrying a record, `own_name`'s binding a binding dyad, and
    /// `node` null or a type node of the mint's contents.
    unsafe fn named_type(
        &mut self,
        own_name: OwnName,
        mint: DyadPtr,
        node: DyadPtr,
    ) -> Result<DyadPtr, ParseError> {
        if own_name.is_none() {
            return Ok(mint);
        }
        let types = self.types;
        // A held body makes its names' bindings again at each build, so a name written in one is
        // found by the held node and the place in its text.
        let (former, key) = match own_name.written {
            Some((held, at)) => (held, vec![KeyPart::Word(at as u64), KeyPart::Node(mint)]),
            None => (own_name.binding, vec![KeyPart::Node(mint)]),
        };
        let (place, kept) =
            type_mint::find(self.rt.store, types, former, &key).map_err(ParseError::Run)?;
        if let Some(named) = kept {
            return Ok(named);
        }
        let record = dyad::head(mint);
        let named = if node.is_null() {
            self.rt.store.alloc_head(types.type_, record)
        } else {
            dyad::set_head(node, record);
            node
        };
        type_mint::keep(self.rt.store, types, place, named).map_err(ParseError::Run)?;
        Ok(named)
    }

    /// The contents of the `type ( … )` body just built, as its key in `type_mint` after the
    /// former: each field's spelling, gates, type, byte and whole default; each `static` member's
    /// spelling and gates with the type it holds, the whole value it holds, or the function it
    /// holds; the rank and the side; and, where a body is written in it, a slot's or a
    /// function's, the place the `type ( … )` itself is written, since a body is the same only
    /// where it is written. `None` for a type that is never found again: one that holds a
    /// writable `static`, a value with no key (a float, a record, a pointer, a text), or a body
    /// while it is built where it is read, which happens once (›Every type is minted by its
    /// contents through `type_mint`; the name that holds a `type ( … )` gives it its identity‹
    /// [[R383]]).
    ///
    /// # Safety
    /// `def` must be the definition `parse_type_body` just closed, `fields` and `bindings` its
    /// fields and their names, and `offsets` where each field lies.
    unsafe fn type_contents(
        &self,
        def: &OpenType,
        fields: &[DyadPtr],
        bindings: &[DyadPtr],
        offsets: &[usize],
        opened_at: usize,
    ) -> Option<Vec<KeyPart>> {
        use crate::identities::read::{place_layout, Read};
        use crate::identities::{hole, meta, numtype, scope};
        let types = self.types;
        let gates = |binding: DyadPtr| {
            use crate::identities::gate::{set_of, word_of_name};
            // SAFETY: `binding` is a binding dyad of the body just built.
            unsafe { set_of(word_of_name(types, binding)) }.into_iter().map(KeyPart::Node)
        };
        // SAFETY: `ty` is a type node and `at` a place of it.
        let whole = |ty: DyadPtr, at: *const u8| match unsafe { place_layout(types, ty) } {
            Some((Read::Scalar(nt), _)) if !nt.is_float() => {
                // SAFETY: as above.
                Some(KeyPart::Word(unsafe { numtype::read_scalar_nt(nt, at) } as u64))
            }
            _ => None,
        };
        // A function's value and result typed by the type being built are the same in every build.
        let own = |ty: DyadPtr| {
            if ty == def.self_type {
                KeyPart::Word(0)
            } else {
                KeyPart::Node(ty)
            }
        };
        let mut key = Vec::new();
        for ((&field, &binding), &offset) in fields.iter().zip(bindings).zip(offsets) {
            // A field's type as it was declared, its gate node with it.
            let ty = hole::type_in(field);
            key.push(KeyPart::Name(Binding::spelling(binding)));
            key.push(KeyPart::Node(hole::gated_type_in(field)));
            key.push(KeyPart::Word(offset as u64));
            let default = hole::default_in(field);
            if !default.is_null() {
                key.push(whole(ty, dyad::value(default))?);
            }
        }
        let mut written_here =
            !def.ctor.is_null() || !def.run_body.is_null() || !def.instances_free.is_null();
        for &binding in &def.statics {
            if crate::identities::gate::name_has(types, binding, types.mut_) {
                return None;
            }
            key.push(KeyPart::Name(Binding::spelling(binding)));
            key.extend(gates(binding));
            // A member still unwritten holds no value to tell it by.
            if Binding::read(binding).unread {
                return None;
            }
            let held = types.through(Binding::read(binding).names(binding));
            if types.is_storage(held) {
                let ty = types.type_of(held);
                key.push(KeyPart::Node(ty));
                key.push(whole(ty, self.rt.place_addr(held)?)?);
            } else if dyad::ty(held) == types.fn_type {
                key.push(KeyPart::Node(types.fn_type));
                let parts = dyad::value(held) as *const DyadPtr;
                let input = *parts.add(FN_INPUT);
                let mut at = meta::record_scope_of(input);
                while !at.is_null() && at != def.scope {
                    at = scope::parent_of(at);
                }
                if at.is_null() {
                    key.push(KeyPart::Node(held));
                } else {
                    written_here = true;
                    let params = crate::identities::array::items(meta::record_fields_of(input));
                    key.extend(params.iter().map(|&p| own(hole::type_in(p))));
                    key.push(own(*parts.add(FN_OUTPUT)));
                }
            } else if crate::identities::is_type_value(types, held) {
                key.push(KeyPart::Node(types.type_));
                key.push(KeyPart::Node(held));
            } else {
                return None;
            }
        }
        key.push(KeyPart::Word(def.parse_rank.to_bits()));
        key.push(KeyPart::Word(matches!(def.assoc, Assoc::Right).into()));
        if key.iter().any(|part| matches!(part, KeyPart::Node(node) if node.is_null())) {
            return None;
        }
        if written_here {
            if self.cx.held.is_null() {
                return None;
            }
            key.push(KeyPart::Word(self.cx.held as u64));
            key.push(KeyPart::Word(opened_at as u64));
        }
        Some(key)
    }

    /// A `type (…)` in a body that runs later reads that run's values, so it is
    /// lexed now and built each time it runs (DESIGN ›Deferral is authored‹), for `own_name`,
    /// the name its declaration gives the type, or null.
    fn hold_type_body(&mut self, own_name: DyadPtr) -> Result<DyadPtr, ParseError> {
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
        let fragment = lex_fragment(&self.cx.scopes, self.trie, self.rt.store, types, held)
            .map_err(ParseError::Resolve)?;
        self.refuse_held_shadowing(&fragment.held_cells(), start - 1)?;
        let cells = self.rt.store.keep_tape(fragment);
        let scope = self.cx.scopes.current().unwrap_or(std::ptr::null_mut());
        let frames: Vec<DyadPtr> = self.cx.frames.iter().map(|f| f.frame).collect();
        Ok(crate::identities::held_type::build(
            self.rt.store,
            types,
            text,
            cells,
            scope,
            &frames,
            own_name,
        ))
    }

    /// A name a held body declares, written left of a `:=` on any line of it, is refused where
    /// the body is written when its cell lexed to a live name of that spelling, as a nested
    /// `fn`'s is: the names are in the text (›Fields are read with a dot outside their scope‹
    /// [[R234]]). `at` is where the held text starts in the source.
    fn refuse_held_shadowing(&mut self, cells: &[Cell], at: usize) -> Result<(), ParseError> {
        let types = self.types;
        for (k, cell) in cells.iter().enumerate() {
            let declared = cells
                .get(k + 1)
                .is_some_and(|c| !c.constructed && self.cell_identity(c) == types.declare_tok);
            let member = k.checked_sub(1).is_some_and(|j| self.is_unbuilt_tight_read(&cells[j]));
            let binding = cell.binding(types);
            if !declared || member || binding.is_null() {
                continue;
            }
            // A literal, `5`, matches a pattern identity and names nothing.
            // SAFETY: a lexed cell's binding is a binding dyad from the store.
            if unsafe { Binding::spelling(binding) } != cell.spelling() {
                continue;
            }
            self.cx.pos = at + cell.start;
            return Err(ParseError::Resolve(ResolveError::Shadowed(cell.spelling().to_string())));
        }
        Ok(())
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
        let held = crate::identities::held_type::parts(node);
        let open_fns = held.frames;
        let bytes = crate::identities::string::text(held.text);
        let text: &'a str = std::str::from_utf8(bytes).expect("copied from the source text");
        let cells = (*held.cells).held_cells();
        let mut chain = Vec::new();
        let mut at = held.scope;
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
        let depth = open_fns.len();
        let frames = open_fns
            .iter()
            .map(|&f| OpenFn {
                frame: f,
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
            held: node,
            feed: Some(Feed { cells, next: 0, base_depth, in_run_order: true }),
            ..Context::nested(&self.cx, text, nested)
        };
        let self_type = self.rt.store.alloc_leaf(self.types.type_);
        let g = self.enter(cx);
        let own_name = OwnName { binding: held.name, written: None };
        let built = g.0.build_type_body(self_type, own_name);
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

    /// A use of one of the slot words, or of `free`; whether the fill reaches
    /// a type being defined is `filling_definition`'s question.
    ///
    /// # Safety
    /// `target` must be a dyad from the store.
    pub(crate) unsafe fn slot_of(&self, target: DyadPtr) -> Option<SlotKind> {
        // The binding of the word's use, or the identity itself when its
        // constructor stood aside (`free`).
        // SAFETY: `target` is a reduced dyad from the store.
        unsafe {
            let id = if dyad::ty(target) == self.types.binding_ {
                Binding::read(target).names(target)
            } else {
                target
            };
            if id == self.types.free_ {
                return Some(SlotKind::Free);
            }
            self.cx.definitions.iter().rev().find_map(|def| def.slot_index(id)).map(SlotKind::of)
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
        let nt = match unsafe { crate::identities::operand_of(self.types, value) } {
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
        if matches!(kind, SlotKind::Parse | SlotKind::Run | SlotKind::Free) {
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
            SlotKind::Parse | SlotKind::Run | SlotKind::Free => {
                unreachable!("body slots and `free` returned above")
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
            SlotKind::Free => def.free_marker,
            kind => def.slots[kind as usize],
        };
        crate::identities::declare::build(self.rt.store, types, target, rhs, marker)
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
                // It gives `void!`, so a `try` or a raise in it hands the driver its error
                // (›Target: a constructor returns `void!`, so a user's syntax error is a
                // recoverable value‹ [[R142]]).
                let output = self.fallible_of(types.void_, at)?;
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
                // SAFETY: `input` was just built, `params` its bindings; no declaration is published to.
                let f = unsafe {
                    self.fn_over_body(
                        types.fn_type,
                        input,
                        &params,
                        output,
                        None,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                let def = self.cx.definitions.last_mut().expect("checked above");
                def.this_param = std::ptr::null_mut();
                def.in_parse = false;
                def.stamps_at_run = false;
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
                let fragment = lex_fragment(&self.cx.scopes, self.trie, self.rt.store, types, held)
                    .map_err(ParseError::Resolve)?;
                let cells = self.rt.store.keep_tape(fragment);
                let body = crate::identities::run_body::build(self.rt.store, types, text, cells);
                self.cx.definitions.last_mut().expect("checked above").run_body = body;
                Ok(self.slot_declare(SlotKind::Run, target, body))
            }
            SlotKind::Free => {
                // One parameter, unnamed: the value an owner's teardown hands in.
                let at = self.cx.pos;
                let (input, params) = self.hidden_param_record(&[(None, types.dyad_)], at)?;
                let def = self.cx.definitions.last_mut().expect("checked above");
                def.this_param = params[0];
                def.in_free = true;
                // SAFETY: `input` was just built, `params` its bindings; no declaration is published to.
                let f = unsafe {
                    self.fn_over_body(
                        types.fn_type,
                        input,
                        &params,
                        types.void_,
                        None,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                let def = self.cx.definitions.last_mut().expect("checked above");
                def.this_param = std::ptr::null_mut();
                def.in_free = false;
                def.instances_free = f?;
                let f = def.instances_free;
                Ok(self.slot_declare(SlotKind::Free, target, f))
            }
            _ => unreachable!("`=` reads a bare body for `parse`, `run` and `free` only"),
        }
    }

    /// A parameter record no text spelled: each name declared in its own scope as a body's
    /// word, checked against every open scope, since the body reopens it; the bindings, one per
    /// field, spelling-less for an unnamed one, for `fn_over_body` to lay out. `at` is the error
    /// position.
    pub(super) fn hidden_param_record(
        &mut self,
        params: &[(Option<&str>, DyadPtr)],
        at: usize,
    ) -> Result<(DyadPtr, Vec<DyadPtr>), ParseError> {
        let scope = self.open_scope();
        let mut fields = Vec::with_capacity(params.len());
        let mut bindings = Vec::with_capacity(params.len());
        let mut declared = Ok(());
        for &(name, ty) in params {
            let field = crate::identities::hole::build(
                self.rt.store,
                self.types.unknown,
                ty,
                std::ptr::null_mut(),
            );
            // A parameter with no name is a place the body reaches another
            // way: a receiver, or a run body's field of type `type`.
            let binding = self.mint_binding(field, scope, name.unwrap_or_default().as_bytes());
            if let Some(name) = name {
                // SAFETY: `binding` was minted by `Binding::alloc` just above.
                let entered = unsafe { self.cx.scopes.declare_body_word(self.trie, name, binding) };
                if let Err(e) = entered {
                    self.cx.pos = at;
                    declared = Err(ParseError::Resolve(e));
                }
            }
            fields.push(field);
            bindings.push(binding);
        }
        self.cx.scopes.pop();
        declared?;
        let fields_arr = crate::identities::array::build(self.rt.store, self.types.array_, &fields);
        let input = self.rt.store.alloc_leaf(self.types.type_);
        let call = self.types.call_parse;
        Ok((
            self.record_node(input, scope, fields_arr, &bindings, std::ptr::null_mut(), call)?,
            bindings,
        ))
    }

    /// The run of a node whose type holds its `run` as a lexed body (DESIGN
    /// ›Deferral is authored‹): the function for its field-type set, found on
    /// the type or built now, held in the node; a field whose type is unknown leaves it
    /// unresolved.
    ///
    /// # Safety
    /// `node` must be the node `run_logos_ctor` minted for a type whose record carries a run
    /// body.
    pub(crate) unsafe fn resolve_specialization(
        &mut self,
        node: DyadPtr,
        at: usize,
        spelling: &str,
    ) -> Result<Option<DyadPtr>, ParseError> {
        use crate::identities::{meta, run_body};
        let ty = dyad::ty(node);
        let held = meta::run_body_of(ty);
        if held.is_null() {
            return Ok(None);
        }
        let key = match self.field_type_key(ty, node) {
            Ok(Some(key)) => key,
            Ok(None) => return Ok(None),
            Err(e) => {
                self.cx.pos = at;
                return Err(e);
            }
        };
        let mut spec = run_body::lookup(held, &key);
        if spec.is_null() {
            spec = self.construct_run_body(ty, held, &key, at, spelling)?;
        }
        run_body::set_spec(node, spec);
        // The node is a call of the function: a use of every outer name the
        // body reads, checked before a comptime node runs it.
        if let Err(e) = self.check_call_reads(spec) {
            self.cx.pos = at;
            return Err(e);
        }
        if let Some(folded) = self.fold_comptime_node(ty, node, spec)? {
            return Ok(Some(folded));
        }
        // A run whose result is copied out is copied where it runs: into the name that
        // takes it, else scratch.
        let out = *(dyad::value(spec) as *const DyadPtr).add(FN_OUTPUT);
        if crate::identities::read::carry_of(self.types, out)
            .and_then(crate::identities::read::Carry::bytes)
            .is_none()
        {
            return Ok(None);
        }
        Ok(Some(crate::identities::by_copy::build_result(
            self.rt.store,
            self.types,
            std::ptr::null_mut(),
            node,
            out,
        )))
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
            if crate::identities::hole::type_in(field) == types.type_ {
                continue;
            }
            let slot = types.through(*slots.add(i));
            let comptime = match read_kind(types, slot) {
                Read::Scalar(_) | Read::Rational => !types.is_storage(slot),
                // A literal handed on by `rational_number`, or into a rational field.
                Read::Executable(_) if dyad::ty(slot) == types.by_copy.out => {
                    let expr = types.through(*(dyad::value(slot) as *const DyadPtr));
                    dyad::ty(expr) == types.rational && !types.is_storage(expr)
                }
                _ => false,
            };
            if !comptime {
                return Ok(None);
            }
        }
        let out = *(dyad::value(spec) as *const DyadPtr).add(FN_OUTPUT);
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

    /// The node's field types (`run_body::field_types`), once each typed field holds its
    /// operand at its type, the field `output_type` the type its run gives, and each bracket
    /// field the bracket as an argument. `None` while a field is unfilled.
    ///
    /// # Safety
    /// As `resolve_specialization`; `ty` the node's type's origin.
    unsafe fn field_type_key(
        &mut self,
        ty: DyadPtr,
        node: DyadPtr,
    ) -> Result<Option<Vec<DyadPtr>>, ParseError> {
        let types = self.types;
        let fields = crate::identities::array::items(crate::identities::meta::record_fields_of(ty));
        let slots = dyad::value(node) as *mut DyadPtr;
        let output = self.output_field(ty);
        for (i, &field) in fields.iter().enumerate() {
            let declared = crate::identities::hole::type_in(field);
            let slot = *slots.add(i);
            if slot.is_null() {
                return Ok(None);
            }
            if output == Some(i) {
                // The run is built for the type it gives, so that type is needed now.
                *slots.add(i) = self.type_known_now(slot)?;
            } else if declared != types.square_brackets && declared != types.scope {
                let value = self.held_type_or(slot);
                *slots.add(i) = crate::identities::land(self.rt.store, types, value, declared)?;
            } else if crate::identities::tape::bracket_kind(types, slot).is_some()
                && dyad::ty(slot) != types.tape.bracket_arg
            {
                *slots.add(i) =
                    crate::identities::tape::build_bracket_arg(self.rt.store, types, slot);
            }
        }
        crate::identities::run_body::field_types(types, ty, node, output)
    }
}
