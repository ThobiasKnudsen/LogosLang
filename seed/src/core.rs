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

use cranelift_codegen::ir::Value;

use crate::compile::{CompileError, LowerTable, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::lex::RegexTrie;
use crate::parse::{Assoc, Construct, ParseError};
use crate::run::{Bcode, RunError, Runtime};
use crate::store::Store;

/// The core identities and the metadata that drives them at parse time (`metas`)
/// and run time (`bcode`).
pub struct Core {
    /// The `Type : Type` self-loop, the one node whose type is itself.
    pub type_: DyadPtr,
    /// The scope every core identity is declared in.
    pub root_scope: DyadPtr,
    /// `fn`, the type whose values are functions. `run` recognizes a function by
    /// its type being this.
    pub fn_type: DyadPtr,
    /// `i32`, the type of an integer variable/value.
    pub i32_: DyadPtr,
    /// `=` (assignment); a function.
    pub assign: DyadPtr,
    /// `+` (addition); a function.
    pub plus: DyadPtr,
    /// `rational_number` (numeric literal carrier); a data type.
    pub rational: DyadPtr,
    /// Parse-time behaviour keyed by identity (the parser's table).
    pub metas: HashMap<DyadPtr, Construct>,
    /// One run version: each function identity's `bcode`. Held as a table so a
    /// new interpreter is a new table over the same identities, not a graph
    /// rewrite.
    pub bcode: Bcode,
    /// One compile version: each operation's Cranelift lowering rule. Same
    /// rationale as `bcode` (a lowering rule is the compiler's knowledge).
    pub lower: LowerTable,
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

        // Data types (their `ty` is `type`). `fn` is the type whose values are
        // functions; `i32` and `rational` carry scalar data.
        let fn_type = store.alloc_raw(type_, std::ptr::null_mut());
        let i32_ = store.alloc_raw(type_, std::ptr::null_mut());
        let rational = store.alloc_raw(type_, std::ptr::null_mut());

        // `=` and `+` are *functions* (their type is `fn`); their run/compile
        // behaviour lives in the tables below, not on the node.
        let assign = store.alloc_raw(fn_type, std::ptr::null_mut());
        let plus = store.alloc_raw(fn_type, std::ptr::null_mut());

        // Register spellings in the name index under the root scope.
        trie.insert("=", IdContext::new(assign, root_scope));
        trie.insert("+", IdContext::new(plus, root_scope));
        trie.insert("-?[0-9]+", IdContext::new(rational, root_scope));

        // Parse time: `=` binds loosest and is right-associative; `+` binds
        // tighter, left.
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

        // Run time: this version's bcode for each function identity.
        let mut bcode: Bcode = HashMap::new();
        bcode.insert(assign, run_assign);
        bcode.insert(plus, run_plus);

        // Compile time: this version's Cranelift lowering rule per primitive.
        let mut lower: LowerTable = HashMap::new();
        lower.insert(i32_, lower_i32);
        lower.insert(rational, lower_rational);
        lower.insert(plus, lower_plus);
        lower.insert(assign, lower_assign);

        Core { type_, root_scope, fn_type, i32_, assign, plus, rational, metas, bcode, lower }
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

/// Build a rational literal `{ty: rational, value: <i32 bytes>}` from its span.
/// v1 molds the literal to a concrete `i32` eagerly (the general coerce-to-the
/// -sibling-operand's-type path is later); v1 scalars are `i32`.
fn build_rational(store: &mut Store, rational: DyadPtr, span: &str) -> Result<DyadPtr, ParseError> {
    let n: i32 = span.parse().map_err(|_| ParseError::BadLiteral)?;
    let value = store.alloc_bytes(&n.to_ne_bytes());
    Ok(store.alloc_raw(rational, value))
}

/// The two `dyad@` operands of a binary application node.
///
/// # Safety
/// `node.value` must point at an operand struct of at least two `dyad@` fields,
/// as produced by [`build_binary`].
unsafe fn operands(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
    (*p, *p.add(1))
}

// --- function bcode (each core function's implementation for the run table) --
//
// Data leaves (an `i32` variable, a `rational` literal) are not functions; `run`
// reads them through their layout, so they need no bcode here.

/// Add: run both operands and sum them.
fn run_plus(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid application dyad, so its operands are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        Ok(rt.run(lhs)? + rt.run(rhs)?)
    }
}

/// Assign: run the right operand, write it into the left operand's `i32` storage,
/// and yield the assigned value.
fn run_assign(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid application dyad, so its operands are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        let value = rt.run(rhs)?;
        let slot = (*lhs).value as *mut i32;
        if slot.is_null() {
            return Err(RunError::BadValue);
        }
        std::ptr::write_unaligned(slot, value as i32);
        Ok(value)
    }
}

// --- the lowering rules (each core primitive's Cranelift compile) -------------

/// Lower an `i32` variable to a load from its baked storage address.
fn lower_i32(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    let addr = unsafe { (*node).value };
    Ok(lw.load_i32(addr))
}

/// Lower a molded rational literal to an `i32` immediate.
fn lower_rational(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    let v = unsafe { std::ptr::read_unaligned((*node).value as *const i32) };
    Ok(lw.const_i32(v))
}

/// Lower addition to `iadd` over the lowered operands.
fn lower_plus(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid application dyad, so its operands are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = lw.lower(lhs)?;
        let r = lw.lower(rhs)?;
        Ok(lw.add(l, r))
    }
}

/// Lower assignment to a store into the left operand's baked storage, yielding
/// the assigned value.
fn lower_assign(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid application dyad, so its operands are valid nodes.
    unsafe {
        let (lhs, rhs) = operands(node);
        let v = lw.lower(rhs)?;
        lw.store_i32((*lhs).value, v);
        Ok(v)
    }
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
            assert_eq!(std::ptr::read_unaligned((*one).value as *const i32), 1);
        }
    }

    #[test]
    fn runs_a_equals_a_plus_one() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        // `a` is an i32 variable initialised to 0.
        let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        let root = {
            let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core.metas, scopes);
            p.parse_expression().unwrap()
        };

        // run `a = a + 1`: yields 1 and leaves a holding 1.
        let mut rt = Runtime::new(core.fn_type, &core.bcode);
        // SAFETY: `root` is the valid dyad tree just parsed into `store`.
        let result = unsafe { rt.run(root) }.unwrap();
        assert_eq!(result, 1);
        unsafe {
            assert_eq!(std::ptr::read_unaligned(a_val as *const i32), 1);
        }
    }

    #[test]
    fn jit_matches_the_interpreter() {
        use crate::compile::compile_nullary_i32;

        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        let root = {
            let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core.metas, scopes);
            p.parse_expression().unwrap()
        };

        // Oracle: the interpreter, from a = 0.
        let mut rt = Runtime::new(core.fn_type, &core.bcode);
        // SAFETY: `root` is the valid tree just parsed.
        let interp = unsafe { rt.run(root) }.unwrap();
        let interp_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };

        // Reset a to 0, then JIT-compile and call, and diff against the oracle.
        unsafe { std::ptr::write_unaligned(a_val as *mut i32, 0) };
        // SAFETY: `root`/`a` live in `store`, which outlives the call.
        let compiled = unsafe { compile_nullary_i32(&core.lower, root) }.unwrap();
        let jit = unsafe { compiled.call_i32() };
        let jit_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };

        assert_eq!(interp, 1);
        assert_eq!(i64::from(jit), interp); // same result
        assert_eq!(jit_a, interp_a); // same side effect on a
        assert_eq!(jit_a, 1);
    }
}
