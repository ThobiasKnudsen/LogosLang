//! The hand-built core graph (V1PLAN Phase 1).
//!
//! The seed builds its foundational identities directly in Rust rather than
//! parsing them, so they are correct by construction. This is the minimal set the
//! `a = a + 1` smoke test needs: the `Type : Type` self-loop, a root scope, and
//! the `=`, `+`, and `rational_number` identities with their native parse-time
//! behaviour ([`Construct`]). Each such identity is itself a type (its `ty` is
//! `type`), carries a spelling in the name index, and has an entry in `metas`.
//!
//! `metas` is a side table keyed by identity: a self-hosted Logos would carry
//! this as `shared` metadata on the type node itself, but in the seed the native
//! `Construct`s live here (see DESIGN ›core identities ... correct by
//! construction‹). This is a first slice: it grows to `type`/`fn`/`struct` and
//! the full primitive set as later phases need them.

use std::collections::HashMap;

use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::lex::RegexTrie;
use crate::parse::{Assoc, Construct, ParseError};
use crate::store::Store;

/// The core identities and the parse-time metadata that drives them.
pub struct Core {
    /// The `Type : Type` self-loop, the one node whose type is itself.
    pub type_: DyadPtr,
    /// The scope every core identity is declared in.
    pub root_scope: DyadPtr,
    /// `=` (assignment).
    pub assign: DyadPtr,
    /// `+` (addition).
    pub plus: DyadPtr,
    /// `rational_number` (numeric literal carrier).
    pub rational: DyadPtr,
    /// Parse-time behaviour keyed by identity.
    pub metas: HashMap<DyadPtr, Construct>,
}

impl Core {
    /// Hand-build the core graph into `store`, registering spellings in `trie`.
    pub fn build(store: &mut Store, trie: &mut RegexTrie) -> Core {
        // Type : Type, the fixed point whose layout is the seed's a-priori knowledge.
        let type_ = store.alloc_raw(std::ptr::null_mut(), std::ptr::null_mut());
        unsafe {
            (*type_).ty = type_;
        }

        // The root scope; every core identity is declared here.
        let root_scope = store.alloc_raw(type_, std::ptr::null_mut());

        // Operator and literal identities. Each is a type (its `ty` is `type`).
        let assign = store.alloc_raw(type_, std::ptr::null_mut());
        let plus = store.alloc_raw(type_, std::ptr::null_mut());
        let rational = store.alloc_raw(type_, std::ptr::null_mut());

        // Register spellings in the name index under the root scope.
        trie.insert("=", IdContext::new(assign, root_scope));
        trie.insert("+", IdContext::new(plus, root_scope));
        trie.insert("-?[0-9]+", IdContext::new(rational, root_scope));

        // `=` binds loosest and is right-associative; `+` binds tighter, left.
        let mut metas: HashMap<DyadPtr, Construct> = HashMap::new();
        metas.insert(
            assign,
            Construct::Infix { precedence: 1.0, assoc: Assoc::Right, build: build_binary },
        );
        metas.insert(
            plus,
            Construct::Infix { precedence: 2.0, assoc: Assoc::Left, build: build_binary },
        );
        metas.insert(rational, Construct::Atom(build_rational));

        Core { type_, root_scope, assign, plus, rational, metas }
    }
}

/// Build a binary application `{ty: op, value: {lhs, rhs}}`.
fn build_binary(
    store: &mut Store,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let operands = store.alloc_operands(&[lhs, rhs]);
    Ok(store.alloc_raw(op, operands))
}

/// Build a rational literal `{ty: rational, value: <digit bytes>}` from its span.
fn build_rational(store: &mut Store, rational: DyadPtr, span: &str) -> Result<DyadPtr, ParseError> {
    let value = store.alloc_bytes(span.as_bytes());
    Ok(store.alloc_raw(rational, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Parser, ScopeStack};

    #[test]
    fn parses_a_equals_a_plus_one() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        // Declare the variable `a` in the root scope.
        let a = store.alloc_raw(core.type_, std::ptr::null_mut());
        scopes.declare(&mut trie, "a", a).unwrap();

        // Parse; the parser borrows store/trie for its lifetime, so scope it.
        let root = {
            let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core.metas, scopes);
            p.parse_expression().unwrap()
        };

        // Expect the tree =(a, +(a, 1)).
        unsafe {
            assert_eq!((*root).ty, core.assign);
            let top = (*root).value as *const DyadPtr;
            assert_eq!(*top, a); // =.lhs is the variable a
            let sum = *top.add(1); // =.rhs is the + application
            assert_eq!((*sum).ty, core.plus);
            let sops = (*sum).value as *const DyadPtr;
            assert_eq!(*sops, a); // +.lhs is a
            let one = *sops.add(1); // +.rhs is the literal
            assert_eq!((*one).ty, core.rational);
            assert_eq!(std::slice::from_raw_parts((*one).value, 1), b"1");
        }
    }
}
