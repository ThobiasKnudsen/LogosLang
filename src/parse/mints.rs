// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The formers written in the seed ask `type_mint` for their mints, by calling its functions as
//! a program does: a key is walked one argument at a time to its place, which keeps the mint.
//! ›Every type is minted by its contents through `type_mint`; the name that holds a `type ( … )`
//! gives it its identity‹ [[R383]]

use super::{ParseError, Parser};
use crate::dyad::DyadPtr;

/// One argument of a mint's key.
#[derive(Debug, Clone)]
pub(super) enum KeyPart {
    /// A type, or another node that is one identity wherever it stands, by its address.
    Node(DyadPtr),
    /// A name, by its spelling.
    Name(String),
    /// A whole number: a rank, a whole value, a place in a text.
    Word(u64),
}

impl Parser<'_> {
    /// `f`, a function of `type_mint`, called through the type as `type_mint.f(…)` is: the value
    /// it works on is none, then `arguments`.
    ///
    /// # Safety
    /// `arguments` must be what `f`'s written parameters take, a word each.
    unsafe fn call_type_mint(&mut self, f: DyadPtr, arguments: &[i64]) -> Result<i64, ParseError> {
        let mut values = Vec::with_capacity(arguments.len() + 1);
        values.push(0);
        values.extend_from_slice(arguments);
        // SAFETY: the caller's contract; `f` is a function `type_mint`'s file declared.
        self.hosted(|rt| unsafe { rt.apply_values(f, &values) }).map_err(ParseError::Run)
    }

    /// The place the key `former`, then `key`, reaches in `type_mint`.
    ///
    /// # Safety
    /// `former` and each node of `key` must be dyads from the store.
    pub(super) unsafe fn mint_place(
        &mut self,
        former: DyadPtr,
        key: &[KeyPart],
    ) -> Result<u64, ParseError> {
        let members = self.types.type_mint;
        let mut place = self.call_type_mint(members.step, &[0, former as i64])?;
        for part in key {
            place = match part {
                KeyPart::Node(node) => self.call_type_mint(members.step, &[place, *node as i64])?,
                KeyPart::Name(name) => {
                    let text = crate::identities::string::build_text(
                        self.rt.store,
                        self.types.string_,
                        name.as_bytes(),
                    );
                    self.call_type_mint(members.step_name, &[place, text as i64])?
                }
                KeyPart::Word(word) => {
                    self.call_type_mint(members.step_word, &[place, *word as i64])?
                }
            };
        }
        Ok(place as u64)
    }

    /// The mint `type_mint` keeps at `place`.
    ///
    /// # Safety
    /// `place` must be a place [`Self::mint_place`] gave.
    pub(super) unsafe fn mint_kept(&mut self, place: u64) -> Result<Option<DyadPtr>, ParseError> {
        let kept =
            self.call_type_mint(self.types.type_mint.kept, &[place as i64])? as usize as DyadPtr;
        Ok((!kept.is_null() && kept != self.types.unknown).then_some(kept))
    }

    /// Has `type_mint` keep `made` at `place`.
    ///
    /// # Safety
    /// As [`Self::mint_kept`]; `made` must be a type node from the store.
    pub(super) unsafe fn mint_keep(&mut self, place: u64, made: DyadPtr) -> Result<(), ParseError> {
        self.call_type_mint(self.types.type_mint.keep, &[place as i64, made as i64]).map(|_| ())
    }
}
