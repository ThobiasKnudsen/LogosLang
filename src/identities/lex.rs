// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `lex`: the lexer as an identity (#62; DESIGN ›Text is the quote‹: "The
//! one spelling is `lex «…»`: the lexer itself, entered as an ordinary
//! identity, applied to a string … `lex` returns a tape fragment — … the
//! fragment is a `parsing_tape` value, the cells with their flags and their
//! spellings, and `insert` splices a tape into a tape … the cells the lexer
//! would have put on the frontier, each a pointer to the record the trie
//! resolved at the lex site … unconstructed, no constructor woken … text
//! that names nothing lexes to a fresh dyad with both slots `undefined`, its
//! spelling kept tape-side … There is no comptime/runtime distinction for
//! `lex`: it turns any string into unconstructed identities whenever it
//! runs, and the fragment is graph owned like any other value of the scope
//! that made it").
//!
//! `lex` reads its own quote at discovery, as `regex` does
//! ([`crate::parse::Parser::construct_lex`]), and places a node `[text, op]`
//! that runs the lexer: [`run`] lexes the text against the scopes open when
//! it runs — for a constructor, the appearance's — and hands back the
//! fragment's handle, a `parsing_tape` value, so a Logos constructor splices
//! it with `tape.insert(k, lex «…»)`. The runtime carries the lexer only
//! while the parser runs something ([`crate::run::Runtime::attach_lexer`])
//! and owns the fragments it makes for the pass. Nothing here lowers.

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// The handles: the identity and its run leaf.
#[derive(Debug, Clone, Copy)]
pub struct LexIds {
    pub lex: DyadPtr,
    pub leaf: DyadPtr,
}

/// Register `lex`: an operand-record identity at the rank of a word that
/// consumes raw text to its right at discovery, the rank `regex` and
/// `import` have, whose node `[text, op]` runs the lexer.
pub(crate) fn register(cx: &mut Cx, cs: &Callables) -> LexIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        crate::parse::Assoc::Left,
        &["text", "op"],
    );
    let lex = cx.store.alloc_raw(cx.type_, record);
    cx.declare("lex", lex);
    cx.metas.insert(lex, |p, _id, tape| p.construct_lex(tape));
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    LexIds { lex, leaf }
}

/// Build the node `lex «…»` places: `[text, op]`, `text` the string node the
/// quote built.
pub(crate) fn build(store: &mut Store, types: &Core, text: DyadPtr) -> DyadPtr {
    let value = store.alloc_operands(&[text, types.lex.leaf]);
    store.alloc_raw(types.lex.lex, value)
}

/// Whether `node`, read through a use of a name, is a `lex «…»` node: the
/// one value a tape fragment has in the seed, yielded by its run.
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
pub(crate) unsafe fn is_fragment(types: &Core, node: DyadPtr) -> bool {
    let d = types.through(node);
    !d.is_null() && (*d).ty == types.lex.lex
}

/// Run: lex the text into a fresh fragment and yield its handle. The text
/// operand is a string node as it stands (the quote), or a node that runs to
/// one (`tape.spelling[k]`, the seed's form of a string value); the text is
/// read off the node's bytes, which live for the store.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `lex` node from the store; its text operand is a
    // reduced dyad.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let string_ty = rt.types().string_;
        let mut text = rt.through(*ops);
        if text.is_null() {
            return Err(RunError::NotText);
        }
        if (*text).ty != string_ty {
            text = rt.run(*ops)? as DyadPtr;
        }
        if text.is_null() || (*text).ty != string_ty || (*text).value.is_null() {
            return Err(RunError::NotText);
        }
        let bytes = super::string::text(text);
        let text = std::str::from_utf8(bytes).map_err(|_| RunError::NotText)?;
        Ok(rt.lex(text)? as i64)
    }
}
