//! The seed's hand-built core identities (V1PLAN Phase 1), one file each.
//!
//! Everything in Logos is an identity, but only the seed's *native* identities
//! are authored in Rust; identities created while a program runs are graph data,
//! never source files. This folder is that bounded native kernel: the node cell
//! ([`dyad`]) and name-resolution pairing ([`id_context`]) the substrate is
//! built from, plus each primitive (`type`, `fn`, `i32`, `rational`, `=`, `+`).
//!
//! Each primitive file defines exactly one identity: its node, its spelling, and
//! its behaviour across the phases (parse `Construct`, run `bcode`, compile
//! lowering). [`Core::build`] wires them into the graph and the per-phase tables.
//! The tables are held per phase (parser/run/compile) rather than on the nodes,
//! so a new run or compile *version* is a new table, not a graph rewrite.

use std::collections::HashMap;

use crate::compile::LowerTable;
use crate::dyad::DyadPtr;
use crate::parse::{Construct, CoreTypes, ParseError};
use crate::run::Bcode;
use crate::regex_trie::RegexTrie;
use crate::store::Store;

pub mod dyad;
pub mod id_context;

#[path = "type.rs"]
mod type_mod;
#[path = "fn.rs"]
mod fn_mod;
#[path = "i32.rs"]
mod i32_mod;
#[path = "return.rs"]
mod return_mod;
#[path = "struct.rs"]
mod struct_mod;
mod assign;
mod paren;
mod plus;
mod rational;
mod scope;

/// The core identities and the per-phase tables that drive them.
pub struct Core {
    /// The `Type : Type` self-loop, the one node whose type is itself.
    pub type_: DyadPtr,
    /// `scope`, the type of a scope node (the graph's spine). Each scope the parser
    /// opens is typed with this.
    pub scope_: DyadPtr,
    /// The scope every core identity is declared in; itself a `scope`-typed node.
    pub root_scope: DyadPtr,
    /// `fn`, the type whose values are functions.
    pub fn_type: DyadPtr,
    /// `i32`, the type of an integer variable/value.
    pub i32_: DyadPtr,
    /// `=` (assignment); a function.
    pub assign: DyadPtr,
    /// `+` (addition); a function.
    pub plus: DyadPtr,
    /// `rational_number` (numeric literal carrier); a data type.
    pub rational: DyadPtr,
    /// `struct`, the type whose constructor derives a layout from a field list
    /// (and whose field list is a function's parameter list).
    pub struct_: DyadPtr,
    /// The parser's table: parse-time behaviour keyed by identity.
    pub metas: HashMap<DyadPtr, Construct>,
    /// One run version: each function identity's `bcode`.
    pub bcode: Bcode,
    /// One compile version: each operation's Cranelift lowering rule.
    pub lower: LowerTable,
}

impl Core {
    /// Hand-build the core graph into `store`, registering spellings in `trie`.
    pub fn build(store: &mut Store, trie: &mut RegexTrie) -> Core {
        // Foundational types first: others reference them.
        let type_ = type_mod::register_root(store);
        let scope_ = scope::register(store, type_);
        let root_scope = store.alloc_raw(scope_, std::ptr::null_mut());
        let fn_type = fn_mod::register(store, type_);

        // Then the behaviour-bearing identities, via a shared build context.
        let mut cx = Cx {
            store,
            trie,
            type_,
            fn_type,
            root_scope,
            metas: HashMap::new(),
            bcode: HashMap::new(),
            lower: HashMap::new(),
        };
        let i32_ = i32_mod::register(&mut cx);
        let rational = rational::register(&mut cx);
        let assign = assign::register(&mut cx);
        let plus = plus::register(&mut cx);
        fn_mod::register_syntax(&mut cx);
        paren::register(&mut cx);
        return_mod::register(&mut cx);
        let struct_ = struct_mod::register(&mut cx);

        let Cx { metas, bcode, lower, .. } = cx;
        Core {
            type_,
            scope_,
            root_scope,
            fn_type,
            i32_,
            assign,
            plus,
            rational,
            struct_,
            metas,
            bcode,
            lower,
        }
    }

    /// The core type handles the parser needs to type the nodes it opens.
    pub fn types(&self) -> CoreTypes {
        CoreTypes { scope: self.scope_, struct_: self.struct_ }
    }
}

/// The shared context each identity registers itself into: the store and name
/// index to build in, the foundational type handles it may reference, and the
/// per-phase tables it fills.
pub(crate) struct Cx<'a> {
    store: &'a mut Store,
    trie: &'a mut RegexTrie,
    type_: DyadPtr,
    fn_type: DyadPtr,
    root_scope: DyadPtr,
    metas: HashMap<DyadPtr, Construct>,
    bcode: Bcode,
    lower: LowerTable,
}

/// Build a binary application `{ty: op, value: {lhs, rhs}}`. The shared `Infix`
/// constructor: `=` and `+` differ only in precedence/associativity, not shape.
pub(crate) fn build_binary(
    store: &mut Store,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let operands = store.alloc_operands(&[lhs, rhs]);
    Ok(store.alloc_raw(op, operands))
}

/// The two `dyad@` operands of a binary application node.
///
/// # Safety
/// `node.value` must point at an operand struct of at least two `dyad@` fields,
/// as produced by [`build_binary`].
pub(crate) unsafe fn operands(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
    (*p, *p.add(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{compile_fn, compile_nullary_i32};
    use crate::parse::{Parser, ScopeStack};
    use crate::run::Runtime;

    #[test]
    fn parses_a_equals_a_plus_one() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        // Declare the variable `a` in the root scope.
        let a = store.alloc_raw(core.i32_, std::ptr::null_mut());
        scopes.declare(&mut trie, "a", a).unwrap();

        let root = {
            let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core.metas, core.types(), scopes);
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
            let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core.metas, core.types(), scopes);
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
    fn runs_a_compound_function_by_walking_its_body() {
        // A function with no bcode is interpreted by walking its `body` field.
        // Parse `main`, a nullary function whose body mutates the outer `a`, then
        // run an application of it: run finds no bcode for `main` and walks body.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        let main = {
            let mut p = Parser::new(
                "fn () -> i32 ( a = a + 1 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };
        // A nullary application of `main`: its type is `main`.
        let call = store.alloc_raw(main, std::ptr::null_mut());

        let mut rt = Runtime::new(core.fn_type, &core.bcode);
        // SAFETY: `call`/`main`/body are valid nodes in `store`.
        let result = unsafe { rt.run(call) }.unwrap();
        assert_eq!(result, 1);
        unsafe {
            assert_eq!(std::ptr::read_unaligned(a_val as *const i32), 1);
        }
    }

    #[test]
    fn runs_a_returning_scope() {
        // `( return 40 + 2 )`: the brackets delimit a scope; `return` yields its
        // value. Runs to 42.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p = Parser::new("( return 40 + 2 )", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };

        let mut rt = Runtime::new(core.fn_type, &core.bcode);
        // SAFETY: `node` is the valid dyad tree just parsed.
        let result = unsafe { rt.run(node) }.unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn nested_scopes_and_bare_return() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // Bare `return` (no brackets) yields to the top-level expression.
        let bare = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new("return 7", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(core.fn_type, &core.bcode);
        assert_eq!(unsafe { rt.run(bare) }.unwrap(), 7);

        // Nested brackets group correctly.
        let nested = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new("( ( return 5 ) )", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        assert_eq!(unsafe { rt.run(nested) }.unwrap(), 5);
    }

    #[test]
    fn unclosed_bracket_is_an_error() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let mut p = Parser::new("( return 1", &mut store, &mut trie, &core.metas, core.types(), scopes);
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::UnclosedBracket));
    }

    #[test]
    fn parses_and_runs_a_fn() {
        // The real fn surface `fn ( params ) -> ret ( body )`. A nullary function
        // returning i32; applying and running it walks the body -> 42.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let func = {
            let mut p = Parser::new(
                "fn () -> i32 ( return 40 + 2 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };

        // Node shape: `{ty: fn, value -> [input, output, body]}` with an empty
        // input struct, an i32 return type, and a body (the `return`).
        unsafe {
            assert_eq!((*func).ty, core.fn_type);
            let v = (*func).value as *const DyadPtr;
            let (input, output, body) = (*v, *v.add(1), *v.add(2));
            assert_eq!((*input).ty, core.struct_); // input is a struct
            let iops = (*input).value as *const DyadPtr;
            assert!((*iops.add(1)).is_null()); // no fields (scope then terminator)
            assert_eq!(output, core.i32_); // return type i32
            assert!(!body.is_null());
        }

        // Apply it and run: run finds no bcode for `func` and walks its body.
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, &core.bcode);
        // SAFETY: `call`/`func`/body are valid nodes in `store`.
        let result = unsafe { rt.run(call) }.unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn parses_a_fn_with_a_param_visible_in_the_body() {
        // A parameter is declared in the input struct's scope and resolves inside
        // the body: `fn (x : i32) -> i32 ( return x )` parses to a body `return(x)`
        // whose operand is the `x` parameter field. (Running it needs the calling
        // convention — param frame slots — which is later; here we check parsing.)
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let func = {
            let mut p = Parser::new(
                "fn (x : i32) -> i32 ( return x )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };

        unsafe {
            assert_eq!((*func).ty, core.fn_type);
            let v = (*func).value as *const DyadPtr;
            let (input, output, body) = (*v, *v.add(1), *v.add(2));
            assert_eq!(output, core.i32_);
            // The single parameter `x`, an i32 field in the input struct.
            let iops = (*input).value as *const DyadPtr;
            let x_field = *iops.add(1); // [scope, x, null]
            assert_eq!((*x_field).ty, core.i32_);
            // The body `return x` resolved `x` to that parameter field.
            let return_operand = (*body).value as DyadPtr;
            assert_eq!(return_operand, x_field);
        }
    }

    #[test]
    fn fn_without_arrow_is_an_error() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let mut p = Parser::new(
            "fn () ( return 1 )",
            &mut store,
            &mut trie,
            &core.metas,
            core.types(),
            scopes,
        );
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::ExpectedArrow));
    }

    #[test]
    fn parses_an_empty_struct() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p = Parser::new("struct ()", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };

        // `{ty: struct, value -> [scope, null]}`: a scope, zero fields.
        unsafe {
            assert_eq!((*node).ty, core.struct_);
            let ops = (*node).value as *const DyadPtr;
            assert!(!(*ops).is_null()); // scope at index 0
            assert!((*ops.add(1)).is_null()); // terminator: no fields
        }
    }

    #[test]
    fn parses_a_struct_with_typed_fields() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p =
                Parser::new("struct (x : i32, y : i32)", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };

        // Two `:` declaration fields, each typed i32 with an undefined value.
        let (scope, fx, fy) = unsafe {
            assert_eq!((*node).ty, core.struct_);
            let ops = (*node).value as *const DyadPtr;
            assert!((*ops.add(3)).is_null()); // terminator after two fields
            (*ops, *ops.add(1), *ops.add(2))
        };
        unsafe {
            assert_eq!((*fx).ty, core.i32_);
            assert!((*fx).value.is_null());
            assert_eq!((*fy).ty, core.i32_);
            assert!((*fy).value.is_null());
        }

        // The field names are declared in the struct's own scope (index 0).
        let mut inner = ScopeStack::new();
        inner.push(scope);
        assert_eq!(inner.resolve(&trie, "x").unwrap().identity, fx);
        assert_eq!(inner.resolve(&trie, "y").unwrap().identity, fy);
    }

    #[test]
    fn parses_a_bare_name_field() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p = Parser::new("struct (t)", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };

        // A bare name: one field with an undefined type slot.
        let (scope, ft) = unsafe {
            let ops = (*node).value as *const DyadPtr;
            assert!((*ops.add(2)).is_null()); // terminator after one field
            (*ops, *ops.add(1))
        };
        unsafe {
            assert!((*ft).ty.is_null()); // bare name: type undefined
            assert!((*ft).value.is_null());
        }

        let mut inner = ScopeStack::new();
        inner.push(scope);
        assert_eq!(inner.resolve(&trie, "t").unwrap().identity, ft);
    }

    #[test]
    fn struct_without_parens_is_an_error() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let mut p = Parser::new("struct 40", &mut store, &mut trie, &core.metas, core.types(), scopes);
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::ExpectedOpen));
    }

    #[test]
    fn scopes_are_typed_scope() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // `scope` is a type (its own type is `type`), and the root scope is one.
        unsafe {
            assert_eq!((*core.scope_).ty, core.type_);
            assert_eq!((*core.root_scope).ty, core.scope_);
        }

        // A struct opens its own `scope`-typed node (value[0]).
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let node = {
            let mut p =
                Parser::new("struct (x : i32)", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };
        unsafe {
            let scope = *((*node).value as *const DyadPtr);
            assert_eq!((*scope).ty, core.scope_);
        }
    }

    #[test]
    fn jit_matches_the_interpreter() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        let root = {
            let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core.metas, core.types(), scopes);
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

    #[test]
    fn milestone_2_fn_runs_interpreted_and_jit_identically() {
        // Milestone 2: the smoke test wrapped in a function, run both interpreted
        // and Cranelift-JIT-compiled, results and the side effect on `a` diffed.
        // The interpreter is the oracle.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        // `fn () -> i32 ( a = a + 1 )`: the smoke test, wrapped in a function.
        let func = {
            let mut p = Parser::new(
                "fn () -> i32 ( a = a + 1 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };

        // Oracle: interpret an application of the function, from a = 0.
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, &core.bcode);
        // SAFETY: `call`/`func`/body are valid nodes just parsed into `store`.
        let interp = unsafe { rt.run(call) }.unwrap();
        let interp_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };

        // Reset a to 0, JIT-compile the function's body, call, and diff.
        unsafe { std::ptr::write_unaligned(a_val as *mut i32, 0) };
        // SAFETY: `func` is the fn node just built; `a`'s storage outlives the call.
        let compiled = unsafe { compile_fn(&core.lower, func) }.unwrap();
        let jit = unsafe { compiled.call_i32() };
        let jit_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };

        assert_eq!(interp, 1);
        assert_eq!(i64::from(jit), interp); // same result
        assert_eq!(jit_a, interp_a); // same side effect on a
        assert_eq!(jit_a, 1);
    }
}
