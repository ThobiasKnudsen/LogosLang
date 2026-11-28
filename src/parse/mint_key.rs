// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! A held `type ( … )`'s key in `type_mint`: the walk where it is written, which finds the reads its
//! construction makes that can change and its lines with an effect, and the key it is found by
//! where it is reached, its place and those reads' values (›Every type is minted by its contents
//! through `type_mint`; the name that holds a `type ( … )` gives it its identity‹ [[R383]]).

use std::collections::{HashMap, HashSet};

use super::*;
use crate::binding::Frame;
use crate::dyad;
use crate::identities::held_type::Construction;
use crate::identities::read::{place_layout, Read};
use crate::identities::type_mint::{self, KeyPart};
use crate::identities::{gate, hole, meta};

/// What a held `type ( … )` reached again asks of its top body's build; stand-in for R383.
#[derive(Debug, Default)]
pub(super) enum PlaceKey {
    /// Built whole, with no key of its place.
    #[default]
    None,
    /// Once the lines that end at or before byte `stop` of the text have run, the `static`
    /// members they made join `key`, which then finds the mint or the place to keep the build at.
    Pending { stop: usize, key: Vec<KeyPart> },
    /// The mint kept for the key at this place, so the rest of the body is not built.
    Found(u64, DyadPtr),
    /// No mint is kept for the key: the build is kept at this place.
    Missed(u64),
}

/// What the walk where a held `type ( … )` is written found in the code that runs when it is
/// constructed: its lines at any depth and the bodies of the functions they call, but no body of a
/// `fn` written in it, which runs later.
pub(super) struct Walk {
    /// Every name declared outside the type that the code reads, directly or through a call.
    pub(super) outer: Vec<DyadPtr>,
    /// Those of them that can change, in the order the key reads them.
    reads: Vec<DyadPtr>,
    /// Where the last line with an effect ends in the text; none where no line has one.
    pub(super) effect_end: Option<usize>,
    /// The code reads what the walk cannot follow, so the body is built at every reach.
    pub(super) unseen: bool,
}

impl Walk {
    pub(super) fn construction(&self) -> Construction<'_> {
        if self.unseen {
            Construction::Unseen
        } else {
            Construction::Keyed { reads: &self.reads, effect_end: self.effect_end }
        }
    }
}

/// The walk over the cells of one held body, `( … )` with its brackets.
struct Walker<'w, 'a> {
    p: &'w Parser<'a>,
    cells: &'w [Cell],
    /// The name its declaration gives the type, which its lines read as the type itself.
    own_name: DyadPtr,
    /// Each cell's identity; null for a cell built already.
    ids: Vec<DyadPtr>,
    /// For a bracket's opener its closer, and for its closer its opener.
    pair: HashMap<usize, usize>,
    /// The opener of the innermost bracket each cell stands in.
    around: Vec<Option<usize>>,
    /// For each cell, how many bodies that run later, a `fn`'s, it stands in.
    later: Vec<u32>,
    /// The spellings each bracket declares, seen from anywhere inside it. A type body's own lines
    /// declare members, which no bare name reaches, so they are not among them.
    declared: HashMap<usize, HashSet<&'w str>>,
    /// The body of each `static fn` line of the type, by its name: what a call through `self` runs.
    siblings: HashMap<&'w str, (usize, usize)>,
    walk: Walk,
}

impl<'a> Parser<'a> {
    /// The walk where the held body `cells` is written, for `own_name`, the binding its declaration
    /// gives the type, or null.
    pub(super) fn walk_construction(&self, cells: &[Cell], own_name: DyadPtr) -> Walk {
        let mut walker = Walker::over(self, cells, own_name);
        walker.walk_lines();
        walker.walk
    }

    /// Whether a read of `binding` can give another value at another reach of a `type ( … )` that
    /// reads it from outside: a parameter, local or loop variable of an enclosing body, a `mut`
    /// name, or one not yet written.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store.
    unsafe fn can_change(&self, binding: DyadPtr) -> bool {
        let types = self.types;
        let fields = Binding::read(binding);
        fields.unread
            || fields.unmade
            || gate::name_has(types, binding, types.mut_)
            || matches!(fields.storage(types), Some(Frame::Call(_)))
    }

    /// Whether a place of type `ty` holds no address, so a name that cannot change holds a value
    /// that cannot change: a number, a type, or a record of such.
    ///
    /// # Safety
    /// `ty` must be null or a type node from the store.
    unsafe fn holds_no_address(&self, ty: DyadPtr) -> bool {
        let types = self.types;
        match place_layout(types, ty) {
            Some((Read::Scalar(_) | Read::Rational, _)) => true,
            Some((Read::Container(t), _)) => t == types.type_ || t == types.string_,
            Some((Read::Aggregate, _)) => {
                let fields = crate::identities::array::items(meta::record_fields_of(ty));
                fields.iter().all(|&field| self.holds_no_address(hole::type_in(field)))
            }
            _ => false,
        }
    }

    /// The key part of the value `binding` holds now: a number by its bits, a type or a function by
    /// its address; `None` for any other value. Stand-in for Question 306.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store, its place readable by the innermost call.
    unsafe fn value_key(&self, binding: DyadPtr) -> Option<KeyPart> {
        let types = self.types;
        let held = types.through(Binding::read(binding).names(binding));
        if held.is_null() {
            return None;
        }
        if !types.is_storage(held) {
            let node =
                crate::identities::is_type_value(types, held) || dyad::ty(held) == types.fn_type;
            return node.then_some(KeyPart::Node(held));
        }
        let at = self.rt.place_addr(held)?;
        match place_layout(types, types.type_of(held))? {
            (Read::Scalar(nt), _) => {
                Some(KeyPart::Word(crate::identities::numtype::read_scalar_nt(nt, at) as u64))
            }
            (Read::Container(t), _) if t == types.type_ => {
                Some(KeyPart::Node(std::ptr::read_unaligned(at as *const DyadPtr)))
            }
            _ => None,
        }
    }

    /// The key of the reads that can change, by their values now; `None` where one holds a value
    /// with no key.
    ///
    /// # Safety
    /// As [`Self::value_key`], for each of `reads`.
    pub(super) unsafe fn read_key(&self, reads: &[DyadPtr]) -> Option<Vec<KeyPart>> {
        reads.iter().map(|&binding| self.value_key(binding)).collect()
    }

    /// The `static` members the open type body has made so far, each by its spelling and value, a
    /// function written in the body by nothing more, since each build writes it the same; `None`
    /// where one holds a value with no key.
    ///
    /// # Safety
    /// The open definition's statics must be binding dyads of the body being built.
    unsafe fn statics_key(&self) -> Option<Vec<KeyPart>> {
        let types = self.types;
        let def = self.cx.definitions.last()?;
        let mut key = Vec::new();
        for &binding in &def.statics {
            key.push(KeyPart::Name(Binding::spelling(binding)));
            let held = types.through(Binding::read(binding).names(binding));
            if !held.is_null() && dyad::ty(held) == types.fn_type && self.written_in(held, def) {
                continue;
            }
            key.push(self.value_key(binding)?);
        }
        Some(key)
    }

    /// Whether the function `f` is written in the type body `def` is reading.
    ///
    /// # Safety
    /// `f` must be a `fn` node from the store.
    pub(super) unsafe fn written_in(&self, f: DyadPtr, def: &OpenType) -> bool {
        let input = *(dyad::value(f) as *const DyadPtr).add(FN_INPUT);
        let mut at = meta::record_scope_of(input);
        while !at.is_null() && at != def.scope {
            at = crate::identities::scope::parent_of(at);
        }
        !at.is_null()
    }

    /// At a line boundary of a held build's top body past its last line with an effect: the lines
    /// read so far run, and the key their `static` members complete is looked up. True where it
    /// finds a mint and the build stops there, as it does outside a test, which builds the rest to
    /// compare it with the mint.
    pub(super) fn place_check(&mut self) -> Result<bool, ParseError> {
        let due = match self.cx.place_key {
            PlaceKey::Pending { stop, .. } => self.cx.pos >= stop,
            _ => false,
        };
        if !due || self.cx.definitions.len() != 1 {
            return Ok(false);
        }
        let PlaceKey::Pending { mut key, .. } = std::mem::take(&mut self.cx.place_key) else {
            return Ok(false);
        };
        self.drain_body()?;
        // SAFETY: the open definition is the held body being built; its statics are its own.
        let Some(statics) = (unsafe { self.statics_key() }) else {
            return Ok(false);
        };
        key.extend(statics);
        let types = self.types;
        // SAFETY: the held node's text is a string node; the key's nodes are dyads from the store.
        let (place, kept) = unsafe {
            let former = crate::identities::held_type::parts(self.cx.held).text;
            type_mint::find(self.rt.store, types, former, &key).map_err(ParseError::Run)?
        };
        self.cx.place_key = match kept {
            Some(kept) => PlaceKey::Found(place, kept),
            None => PlaceKey::Missed(place),
        };
        Ok(kept.is_some() && !cfg!(test))
    }

    /// The key a held `type ( … )` reached now is found by, from the walk where it is written: its
    /// place and the values of its reads that can change. With no effect it is looked up here;
    /// with one, the build completes it once the lines with the effect have run. Stand-in for R383.
    ///
    /// # Safety
    /// `node` must be a held type node from the store; the call it was written in must be the
    /// innermost activation.
    pub(super) unsafe fn reach_key(
        &mut self,
        node: DyadPtr,
    ) -> Result<PlaceKey, crate::run::RunError> {
        let held = crate::identities::held_type::parts(node);
        let Construction::Keyed { reads, effect_end } = held.construction else {
            return Ok(PlaceKey::None);
        };
        let Some(key) = self.read_key(reads) else {
            return Ok(PlaceKey::None);
        };
        if let Some(stop) = effect_end {
            return Ok(PlaceKey::Pending { stop, key });
        }
        let (place, kept) = type_mint::find(self.rt.store, self.types, held.text, &key)?;
        Ok(match kept {
            Some(kept) => PlaceKey::Found(place, kept),
            None => PlaceKey::Missed(place),
        })
    }

    /// What the held build that gave `built` gives once its key is settled: the mint its key
    /// found, or the build, kept at its place where the key is new. A test's build at a found mint
    /// is the type built again, whose contents must be the mint's as they were when it was kept.
    ///
    /// # Safety
    /// `built` must be the type node the held build's top body gave.
    pub(super) unsafe fn settle_key(
        &mut self,
        built: DyadPtr,
    ) -> Result<DyadPtr, crate::run::RunError> {
        #[cfg(test)]
        let contents = std::mem::take(&mut self.cx.built_contents);
        match std::mem::take(&mut self.cx.place_key) {
            PlaceKey::Found(_place, kept) => {
                #[cfg(test)]
                if let Some(kept_contents) = self.rt.store.kept_contents.get(&_place) {
                    assert_eq!(
                        &contents, kept_contents,
                        "a held `type ( … )` built again at its key's hit differs from its mint"
                    );
                }
                Ok(kept)
            }
            PlaceKey::Missed(place) => {
                type_mint::keep(self.rt.store, self.types, place, built)?;
                #[cfg(test)]
                self.rt.store.kept_contents.insert(place, contents);
                Ok(built)
            }
            PlaceKey::None | PlaceKey::Pending { .. } => Ok(built),
        }
    }
}

impl<'w, 'a> Walker<'w, 'a> {
    fn over(p: &'w Parser<'a>, cells: &'w [Cell], own_name: DyadPtr) -> Self {
        let ids: Vec<DyadPtr> = cells
            .iter()
            .map(|c| if c.constructed { std::ptr::null_mut() } else { p.cell_identity(c) })
            .collect();
        let mut pair = HashMap::new();
        let mut around = Vec::with_capacity(cells.len());
        let mut open = Vec::new();
        for (k, &id) in ids.iter().enumerate() {
            if p.is_closer(id) {
                if let Some(opener) = open.pop() {
                    pair.insert(opener, k);
                    pair.insert(k, opener);
                }
            }
            around.push(open.last().copied());
            if p.is_opener(id) {
                open.push(k);
            }
        }
        let walk = Walk {
            outer: Vec::new(),
            reads: Vec::new(),
            effect_end: None,
            unseen: !open.is_empty() || !pair.contains_key(&0),
        };
        let mut walker = Walker {
            p,
            cells,
            own_name,
            ids,
            pair,
            around,
            later: vec![0; cells.len()],
            declared: HashMap::new(),
            siblings: HashMap::new(),
            walk,
        };
        walker.find_bodies();
        walker.find_declarations();
        walker
    }

    fn id(&self, k: usize) -> DyadPtr {
        self.ids.get(k).copied().unwrap_or(std::ptr::null_mut())
    }

    fn tight(&self, k: usize) -> bool {
        self.cells.get(k).is_some_and(|c| self.p.is_unbuilt_tight_read(c))
    }

    /// Whether the bracket opened at `opener` is a type body, whose lines declare members.
    fn type_body(&self, opener: usize) -> bool {
        let types = self.p.types;
        opener == 0
            || self.id(opener) == types.open_
                && [types.type_, types.union.union].contains(&self.id(opener - 1))
    }

    /// Marks each `fn`'s body as one that runs later, and notes the `static fn` lines of the type.
    fn find_bodies(&mut self) {
        let types = self.p.types;
        for k in 0..self.cells.len() {
            if self.id(k) != types.fn_type {
                continue;
            }
            let Some(&params_close) =
                self.pair.get(&(k + 1)).filter(|_| self.id(k + 1) == types.open_)
            else {
                continue;
            };
            // The result type is read up to the first `(` beside the `fn`, the body's.
            let body = (params_close + 1..self.cells.len())
                .take_while(|&j| self.around[j] == self.around[k] && self.id(j) != types.sep_)
                .find(|&j| self.id(j) == types.open_);
            let Some((open, close)) = body.and_then(|j| Some((j, *self.pair.get(&j)?))) else {
                continue;
            };
            for later in &mut self.later[open..=close] {
                *later += 1;
            }
            if self.around[k] == Some(0) {
                let line = self.line_start(k);
                if self.id(line + 1) == types.declare_tok {
                    self.siblings.insert(self.cells[line].spelling(), (open, close));
                }
            }
        }
    }

    /// The first cell of the line of the top body `k` stands in.
    fn line_start(&self, k: usize) -> usize {
        let mut j = k;
        while j > 1 && !(self.id(j - 1) == self.p.types.sep_ && self.around[j - 1] == Some(0)) {
            j -= 1;
        }
        j
    }

    /// The spellings each bracket declares: a name before `:=`, a bare parameter, a `for`'s
    /// variable. A function's parameters are seen from its body.
    fn find_declarations(&mut self) {
        let types = self.p.types;
        let cells = self.cells;
        for k in 0..cells.len() {
            let Some(at) = self.around[k] else {
                continue;
            };
            let spelling = cells[k].spelling();
            let declares = self.id(k + 1) == types.declare_tok;
            let fn_params = at > 0 && self.id(at - 1) == types.fn_type;
            let bare_param = fn_params
                && cells[k].is_fresh(types)
                && [types.sep_, types.open_].contains(&self.id(k - 1))
                && (self.id(k + 1) == types.sep_ || self.p.is_closer(self.id(k + 1)));
            if (declares || bare_param) && !self.type_body(at) {
                self.declared.entry(at).or_default().insert(spelling);
                if fn_params {
                    if let Some(body) = self.body_after(at) {
                        self.declared.entry(body).or_default().insert(spelling);
                    }
                }
            }
            let loop_variable = self.id(k) == types.for_
                && cells.get(k + 1).is_some_and(|c| c.is_fresh(types))
                && self.id(k + 2) == types.in_;
            if loop_variable {
                let body = (k + 3..cells.len())
                    .take_while(|&j| self.around[j] == Some(at) && self.id(j) != types.sep_)
                    .find(|&j| self.id(j) == types.open_);
                if let Some(body) = body {
                    self.declared.entry(body).or_default().insert(cells[k + 1].spelling());
                }
            }
        }
    }

    /// The opener of the body of the `fn` whose parameters open at `params`.
    fn body_after(&self, params: usize) -> Option<usize> {
        let close = *self.pair.get(&params)?;
        (close + 1..self.cells.len())
            .take_while(|&j| {
                self.around[j] == self.around[params] && self.id(j) != self.p.types.sep_
            })
            .find(|&j| self.id(j) == self.p.types.open_)
    }

    /// Whether the fresh spelling at `k` names what a bracket around it declares, or a word each
    /// type body declares.
    fn declared_inside(&self, k: usize) -> bool {
        let spelling = self.cells[k].spelling();
        if spelling == SELF_WORD || spelling == "layout" {
            return true;
        }
        let mut at = self.around[k];
        while let Some(opener) = at {
            if self.declared.get(&opener).is_some_and(|names| names.contains(spelling)) {
                return true;
            }
            at = self.around[opener];
        }
        false
    }

    /// Walks each line of the top body, noting where the last line with an effect ends.
    fn walk_lines(&mut self) {
        let Some(&end) = self.pair.get(&0) else {
            return;
        };
        let mut start = 1;
        for k in 1..=end {
            if k < end && !(self.id(k) == self.p.types.sep_ && self.around[k] == Some(0)) {
                continue;
            }
            let mut called = HashSet::new();
            if self.walk_cells(start, k, 0, &mut called) && k > start {
                self.walk.effect_end = Some(self.cells[k - 1].end());
            }
            start = k + 1;
        }
    }

    /// Walks the cells `from..to` that stand in `level` bodies that run later, and in no body
    /// inside those; true where one has an effect.
    fn walk_cells(
        &mut self,
        from: usize,
        to: usize,
        level: u32,
        called: &mut HashSet<&'w str>,
    ) -> bool {
        let mut effect = false;
        for k in from..to {
            if self.later[k] <= level && !self.cells[k].constructed {
                effect |= self.cell(k, called);
            }
        }
        effect
    }

    fn cell(&mut self, k: usize, called: &mut HashSet<&'w str>) -> bool {
        let types = self.p.types;
        let id = self.id(k);
        let structural = [types.sep_, types.declare_tok].contains(&id)
            || self.p.is_opener(id)
            || self.p.is_closer(id);
        let declares = self.id(k + 1) == types.declare_tok || k > 0 && self.id(k - 1) == types.for_;
        if structural || declares || k > 0 && self.tight(k - 1) {
            return false;
        }
        if id == types.print.print || id == types.alloc_ {
            return true;
        }
        if id == types.assign {
            return self.write_seen(k);
        }
        if id == types.at_ {
            self.walk.unseen |= !self.type_after(k);
            return false;
        }
        let cell = self.cells[k];
        let binding = cell.binding(types);
        if binding.is_null() {
            if cell.spelling() == SELF_WORD {
                return self.sibling_call(k, called);
            }
            if cell.is_fresh(types) && !self.declared_inside(k) {
                self.walk.unseen = true;
            }
            return false;
        }
        // SAFETY: a lexed cell's binding is a binding dyad from the store.
        let fields = unsafe { Binding::read(binding) };
        if fields.pattern {
            return false;
        }
        if binding == self.own_name || fields.body_word || cell.spelling() == SELF_WORD {
            return self.sibling_call(k, called);
        }
        self.note(binding);
        if self.tight(k + 1) {
            return self.member_chain(k, binding);
        }
        self.outer_cell(k, binding)
    }

    fn note(&mut self, binding: DyadPtr) {
        if !self.walk.outer.contains(&binding) {
            self.walk.outer.push(binding);
        }
    }

    fn read(&mut self, binding: DyadPtr) {
        if !self.walk.reads.contains(&binding) {
            self.walk.reads.push(binding);
        }
    }

    /// `self.h(…)` of a `static fn` line of the type: the call runs its body now.
    fn sibling_call(&mut self, k: usize, called: &mut HashSet<&'w str>) -> bool {
        if !self.tight(k + 1) || self.id(k + 3) != self.p.types.open_ {
            return false;
        }
        let name = self.cells[k + 2].spelling();
        let Some(&(open, close)) = self.siblings.get(name) else {
            return false;
        };
        if !called.insert(name) {
            return false;
        }
        self.walk_cells(open + 1, close, self.later[open], called)
    }

    /// `name.m.…` from a name declared outside the type: a `static` member of a type is followed
    /// to what it holds; any other member is a read the walk cannot follow.
    fn member_chain(&mut self, k: usize, binding: DyadPtr) -> bool {
        let p = self.p;
        let types = p.types;
        // SAFETY: `binding` is a binding dyad from the store.
        if unsafe { p.can_change(binding) } {
            self.walk.unseen = true;
            return false;
        }
        // SAFETY: as above.
        let mut at = unsafe { types.through(binding) };
        let mut j = k;
        while self.tight(j + 1) {
            if self.id(j + 1) == types.colon_ || j + 2 >= self.cells.len() {
                self.walk.unseen = true;
                return false;
            }
            let name = self.cells[j + 2].spelling();
            // SAFETY: `at` is a dyad from the store.
            let Some((member, member_binding)) = (unsafe { meta::is_record_type(at) }
                .then(|| unsafe { p.static_member_of(at, name) }))
            .flatten() else {
                self.walk.unseen = true;
                return false;
            };
            self.note(member_binding);
            j += 2;
            // SAFETY: `member_binding` is a binding dyad and `member` a dyad from the store.
            unsafe {
                if p.can_change(member_binding) {
                    self.read(member_binding);
                    self.walk.unseen |= self.tight(j + 1);
                    return false;
                }
                if dyad::ty(member) == types.fn_type {
                    self.walk.unseen |= self.tight(j + 1);
                    return self.fold(member);
                }
            }
            at = member;
        }
        // SAFETY: `at` is a dyad from the store.
        self.walk.unseen |= unsafe { self.runs_logos(at) };
        false
    }

    /// A name declared outside the type, not followed by a member. A fixed name that holds an
    /// address is not followed to what it points at, which can change.
    fn outer_cell(&mut self, k: usize, binding: DyadPtr) -> bool {
        let p = self.p;
        let types = p.types;
        // SAFETY: `binding` is a binding dyad and its dyad one from the store.
        unsafe {
            if p.can_change(binding) {
                self.read(binding);
                self.walk.unseen |= self.used_as_code(k, binding);
                return false;
            }
            if types.is_storage(binding) && !p.holds_no_address(types.type_of(binding)) {
                self.walk.unseen = true;
                return false;
            }
            let held = types.through(binding);
            if !held.is_null() && dyad::ty(held) == types.fn_type {
                return self.fold(held);
            }
            self.walk.unseen |= self.runs_logos(held);
        }
        false
    }

    /// Whether `ty` is a type whose values a `parse` or a `run` written in Logos makes or runs,
    /// which the walk does not follow.
    ///
    /// # Safety
    /// `ty` must be null or a dyad from the store.
    unsafe fn runs_logos(&self, ty: DyadPtr) -> bool {
        meta::is_record_type(ty)
            && (!meta::written_parse_of(ty, self.p.types.fn_type).is_null()
                || !meta::run_body_of(ty).is_null())
    }

    /// Whether the changing name at `k` is used as code, so what runs depends on its value: a
    /// type converting what follows it, or a function called.
    ///
    /// # Safety
    /// `binding` must be a binding dyad from the store.
    unsafe fn used_as_code(&self, k: usize, binding: DyadPtr) -> bool {
        let types = self.p.types;
        let next = self.id(k + 1);
        let ends = next.is_null() || next == types.sep_ || self.p.is_closer(next);
        let held = types.through(Binding::read(binding).names(binding));
        let typed = if types.is_storage(held) { types.type_of(held) } else { dyad::ty(held) };
        if typed == types.fn_type || !held.is_null() && dyad::ty(held) == types.fn_type {
            return next == types.open_;
        }
        let a_type = typed == types.type_ || crate::identities::is_type_value(types, held);
        a_type && !ends && next != types.unknown
    }

    /// Whether `@` at `k` forms a pointer type, `@i32` or `@ mut T`, rather than reading through
    /// a pointer, `p@`.
    fn type_after(&self, k: usize) -> bool {
        let p = self.p;
        let types = p.types;
        let Some(next) = self.cells.get(k + 1) else {
            return false;
        };
        let id = self.id(k + 1);
        let binding = next.binding(types);
        // SAFETY: a cell's identity and binding are null or dyads from the store.
        unsafe {
            p.is_gate_word(id)
                || crate::identities::is_type_value(types, id)
                || !binding.is_null()
                    && (binding == self.own_name
                        || types.is_storage(binding) && types.type_of(binding) == types.type_)
                || next.is_fresh(types) && next.spelling() == SELF_WORD
        }
    }

    /// Whether the `=` at `k` writes a place a reader outside the type sees: one through a pointer
    /// or an index, or a name declared outside the type.
    fn write_seen(&self, k: usize) -> bool {
        let types = self.p.types;
        let mut j = k;
        loop {
            if j == 0 {
                return true;
            }
            j -= 1;
            let id = self.id(j);
            if self.p.is_closer(id) || id == types.at_ {
                return true;
            }
            if j > 0 && self.tight(j - 1) {
                j -= 1;
                continue;
            }
            let cell = &self.cells[j];
            let binding = cell.binding(types);
            if binding.is_null() {
                return !cell.is_fresh(types) || !self.declared_inside(j);
            }
            // SAFETY: a lexed cell's binding is a binding dyad from the store.
            let word = unsafe { Binding::read(binding).body_word };
            return !(word || binding == self.own_name || cell.spelling() == SELF_WORD);
        }
    }

    /// A call of the function `f`: its effect is the line's, and the outer names its body reads
    /// are the code's.
    ///
    /// # Safety
    /// `f` must be a `fn` node from the store.
    unsafe fn fold(&mut self, f: DyadPtr) -> bool {
        let p = self.p;
        let types = p.types;
        // A function whose body is still being read has no list of its reads yet.
        if p.cx.frames.iter().any(|open| open.frame == f) {
            self.walk.unseen = true;
            return false;
        }
        for &binding in fn_outer(f) {
            self.note(binding);
            if p.can_change(binding) {
                self.read(binding);
            } else if types.is_storage(binding) && !p.holds_no_address(types.type_of(binding)) {
                self.walk.unseen = true;
            }
        }
        fn_effect(types, f) || fn_receiver(types, f) & RECEIVER_WRITES != 0
    }
}
