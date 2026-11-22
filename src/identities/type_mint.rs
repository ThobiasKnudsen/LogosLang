// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `type_mint`, the one keeper of every mint, written in Logos: the seed carries
//! identities/type_mint.logos and reads it before any program, giving the root its name, so the
//! name stands wherever a type mints. The formers written in the seed ask it through [`Keeper`], which
//! calls its functions as a program does. ›Every type is minted by its contents through
//! `type_mint`; the name that holds a `type ( … )` gives it its identity‹ [[R383]]

use super::meta;
use crate::dyad::{self, DyadPtr};
use crate::parse::{LinesEnd, Parser, ScopeStack};
use crate::regex_trie::RegexTrie;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

const PATH: &str = "identities/type_mint.logos";
const SOURCE: &str = include_str!("../../identities/type_mint.logos");
const NAME: &str = "type_mint";

/// `type_mint`'s functions, which the formers written in the seed call.
#[derive(Debug, Clone, Copy)]
pub struct Members {
    /// The scope the file's own lines stand in, whose names live for the run.
    pub(crate) file: DyadPtr,
    step: DyadPtr,
    step_name: DyadPtr,
    step_word: DyadPtr,
    kept: DyadPtr,
    keep: DyadPtr,
}

impl Members {
    /// What a core holds until the file is read.
    pub(super) const fn unread() -> Self {
        let null = std::ptr::null_mut();
        Self { file: null, step: null, step_name: null, step_word: null, kept: null, keep: null }
    }

    pub(crate) fn is_read(&self) -> bool {
        !self.step.is_null()
    }
}

/// Reads the file in a scope of its own under the root, as an imported file is read, and gives
/// the root its one `pub` name, so `type_mint` is ambient and nothing else of the file is
/// (›Importing is dropping the text there, wrapped in its own scope‹ [[R306]]).
///
/// # Panics
/// Where the file the seed was built with does not parse or run, or lacks a function the seed
/// calls: a defect of the seed.
pub(super) fn load(store: &mut Store, trie: &mut RegexTrie, core: &Core) -> Members {
    let file = read_file(store, trie, core, PATH, SOURCE);
    let name_in = |scope: DyadPtr, name: &str| {
        let mut members = ScopeStack::new();
        members.push(scope);
        members.resolve(trie, name).unwrap_or_else(|_| panic!("{PATH}: no `{name}`"))
    };
    let keeper = name_in(file, NAME);
    // SAFETY: the file declared `type_mint` as a type with a body.
    let body = unsafe { meta::record_body_of(keeper.identity) };
    let member = |name: &str| {
        let f = name_in(body, name).identity;
        // SAFETY: `f` is the dyad a name of the body holds.
        assert!(unsafe { dyad::ty(f) } == core.fn_type, "{PATH}: `{name}` is no function");
        f
    };
    let members = Members {
        file,
        step: member("step"),
        step_name: member("step_name"),
        step_word: member("step_word"),
        kept: member("kept"),
        keep: member("keep"),
    };
    publish(store, trie, core, PATH, NAME, keeper.binding);
    members
}

/// The file the seed carries at `path`, read in a scope of its own under the root, as an imported
/// file is read; that scope.
///
/// # Panics
/// Where the file does not parse or run: a defect of the seed.
pub(super) fn read_file(
    store: &mut Store,
    trie: &mut RegexTrie,
    core: &Core,
    path: &str,
    source: &str,
) -> DyadPtr {
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let file = scopes.push_section(store, core.scope);
    let mut p = Parser::new(source, store, trie, core, scopes);
    let read = p.read_lines(LinesEnd::TextEnd).and_then(|_| p.finish());
    if let Err(e) = read {
        let message = crate::report::parse_message(&e);
        panic!("{}", crate::report::render(path, source, p.offset(), &message));
    }
    p.hand_exit();
    file
}

/// The root takes `binding`, a name a file the seed carries declared, under `name`.
///
/// # Panics
/// Where the root refuses it: a defect of the seed.
pub(super) fn publish(
    store: &mut Store,
    trie: &mut RegexTrie,
    core: &Core,
    path: &str,
    name: &str,
    binding: DyadPtr,
) {
    let mut root = ScopeStack::new();
    root.push(core.root_scope);
    Parser::new("", store, trie, core, root)
        .publish(&[(name.to_string(), binding)])
        .unwrap_or_else(|e| panic!("{path}: {}", crate::report::parse_message(&e)));
}

/// One argument of a mint's key.
#[derive(Debug, Clone)]
pub(crate) enum KeyPart {
    /// A type, or another node that is one identity wherever it stands, by its address.
    Node(DyadPtr),
    /// A name, by its spelling.
    Name(String),
    /// A whole number: a rank, a whole value, a place in a text.
    Word(u64),
}

/// The place the key `former`, then `key`, reaches in `type_mint`, and the mint kept there.
///
/// # Safety
/// `former` and each node of `key` must be dyads from the store.
///
/// # Panics
/// Before the seed has read `type_mint`'s file: nothing mints through it until then.
pub(crate) unsafe fn find(
    store: &mut Store,
    types: &Core,
    former: DyadPtr,
    key: &[KeyPart],
) -> Result<(u64, Option<DyadPtr>), RunError> {
    let mut keeper = Keeper::over(store, types);
    let place = keeper.place(former, key)?;
    Ok((place, keeper.kept(place)?))
}

/// Has `type_mint` keep `made` at `place`.
///
/// # Safety
/// `place` must be a place [`find`] gave, and `made` a type node from the store.
pub(crate) unsafe fn keep(
    store: &mut Store,
    types: &Core,
    place: u64,
    made: DyadPtr,
) -> Result<(), RunError> {
    Keeper::over(store, types).keep(place, made)
}

/// The mint under `former` and the nodes of `key`: the one kept, or the one `make` builds the
/// first time, kept from then on; `None` where `make` builds none.
///
/// # Safety
/// `former` and each node of `key` must be dyads from the store, or null, and `make` must give
/// a type node from the store.
///
/// # Panics
/// Where `type_mint`'s own functions fail: a defect of the seed.
pub(crate) unsafe fn mint_of(
    store: &mut Store,
    types: &Core,
    former: DyadPtr,
    key: &[DyadPtr],
    make: impl FnOnce(&mut Store) -> Option<DyadPtr>,
) -> Option<DyadPtr> {
    if !types.type_mint.is_read() {
        let former = if former == types.at_ { std::ptr::null_mut() } else { former };
        return early(store, former, key, make);
    }
    let parts: Vec<KeyPart> = key.iter().map(|&node| KeyPart::Node(node)).collect();
    let (place, kept) = find(store, types, former, &parts).expect("`type_mint` walks a key");
    if kept.is_some() {
        return kept;
    }
    let mint = make(store)?;
    keep(store, types, place, mint).expect("`type_mint` keeps a mint");
    Some(mint)
}

/// As [`mint_of`] before `type_mint` stands, which is built from mints itself: the store lists
/// each one, and [`keep_early`] hands the list over. `former` is null for `@`, whose identity is
/// registered after the first pointer types are made: stand-in for #466.
///
/// # Safety
/// As [`mint_of`].
pub(crate) unsafe fn early(
    store: &mut Store,
    former: DyadPtr,
    key: &[DyadPtr],
    make: impl FnOnce(&mut Store) -> Option<DyadPtr>,
) -> Option<DyadPtr> {
    if let Some(mint) = store.early_mint(former, key) {
        return Some(mint);
    }
    let mint = make(store)?;
    store.note_early_mint(former, key, mint);
    Some(mint)
}

/// Hands `type_mint`, once it stands, the mints the seed made before.
///
/// # Panics
/// As [`mint_of`].
pub(super) fn keep_early(store: &mut Store, types: &Core) {
    let early = store.take_early_mints();
    let mut keeper = Keeper::over(store, types);
    for (former, key, mint) in early {
        let former = if former.is_null() { types.at_ } else { former };
        let parts: Vec<KeyPart> = key.iter().map(|&node| KeyPart::Node(node)).collect();
        // SAFETY: `former`, the key's nodes and `mint` are what `early` noted.
        unsafe {
            let place = keeper.place(former, &parts).expect("`type_mint` walks a key");
            keeper.keep(place, mint).expect("`type_mint` keeps a mint");
        }
    }
}

/// `type_mint` as the seed's formers call it, over a runtime of its own: its functions read and
/// write only the places of their own type, so they need nothing of the run that asks.
struct Keeper<'a> {
    rt: Runtime<'a>,
}

impl Drop for Keeper<'_> {
    fn drop(&mut self) {
        self.rt.leave_stack();
    }
}

impl<'a> Keeper<'a> {
    fn over(store: &'a mut Store, types: &'a Core) -> Self {
        assert!(types.type_mint.is_read(), "a type is minted before `type_mint` stands");
        Keeper { rt: Runtime::for_a_call(types, store) }
    }

    /// `f`, a function of `type_mint`, called through the type as `type_mint.f(…)` is: the value
    /// it works on is none, then `arguments`.
    ///
    /// # Safety
    /// `arguments` must be what `f`'s written parameters take, a word each.
    unsafe fn call(&mut self, f: DyadPtr, arguments: &[i64]) -> Result<i64, RunError> {
        let mut values = Vec::with_capacity(arguments.len() + 1);
        values.push(0);
        values.extend_from_slice(arguments);
        self.rt.apply_values(f, &values)
    }

    /// The place the key `former`, then `key`, reaches.
    ///
    /// # Safety
    /// `former` and each node of `key` must be dyads from the store.
    unsafe fn place(&mut self, former: DyadPtr, key: &[KeyPart]) -> Result<u64, RunError> {
        let members = self.rt.types().type_mint;
        let mut place = self.call(members.step, &[0, former as i64])?;
        for part in key {
            place = match part {
                KeyPart::Node(node) => self.call(members.step, &[place, *node as i64])?,
                KeyPart::Name(name) => {
                    let string = self.rt.types().string_;
                    let text = super::string::build_text(self.rt.store(), string, name.as_bytes());
                    self.call(members.step_name, &[place, text as i64])?
                }
                KeyPart::Word(word) => self.call(members.step_word, &[place, *word as i64])?,
            };
        }
        Ok(place as u64)
    }

    /// The mint kept at `place`.
    ///
    /// # Safety
    /// `place` must be a place [`Self::place`] gave.
    unsafe fn kept(&mut self, place: u64) -> Result<Option<DyadPtr>, RunError> {
        let (kept, unknown) = (self.rt.types().type_mint.kept, self.rt.types().unknown);
        let kept = self.call(kept, &[place as i64])? as usize as DyadPtr;
        Ok((!kept.is_null() && kept != unknown).then_some(kept))
    }

    /// Keeps `made` at `place`.
    ///
    /// # Safety
    /// As [`Self::kept`]; `made` must be a type node from the store.
    unsafe fn keep(&mut self, place: u64, made: DyadPtr) -> Result<(), RunError> {
        let keep = self.rt.types().type_mint.keep;
        self.call(keep, &[place as i64, made as i64]).map(|_| ())
    }
}
