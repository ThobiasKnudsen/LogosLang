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
#[path = "bool.rs"]
mod bool_mod;
#[path = "if.rs"]
mod if_mod;
mod add;
mod assign;
mod cmp;
mod declare;
mod lt;
mod minus;
mod mul;
mod paren;
mod plus;
pub(crate) mod rational;
mod scope;
mod sub;
mod times;

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
    /// `bool`, the type of a boolean value (a comparison result; an `if` condition).
    pub bool_: DyadPtr,
    /// `=` (assignment); a function.
    pub assign: DyadPtr,
    /// `+` (abstract addition operator); resolves to a concrete op per operand type.
    pub plus: DyadPtr,
    /// `add_i32` (concrete i32 addition); the machine op `+` resolves to.
    pub add_i32: DyadPtr,
    /// `-` (abstract subtraction operator).
    pub minus: DyadPtr,
    /// `sub_i32` (concrete i32 subtraction); the machine op `-` resolves to.
    pub sub_i32: DyadPtr,
    /// `*` (abstract multiplication operator).
    pub times: DyadPtr,
    /// `mul_i32` (concrete i32 multiplication); the machine op `*` resolves to.
    pub mul_i32: DyadPtr,
    /// `<` (abstract less-than comparison); its result is `bool`.
    pub lt: DyadPtr,
    /// `lt_i32` (concrete i32 less-than); the machine op `<` resolves to.
    pub lt_i32: DyadPtr,
    /// `if` (the value-producing conditional); a function.
    pub if_: DyadPtr,
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
        let bool_ = bool_mod::register(&mut cx);
        let rational = rational::register(&mut cx);
        let assign = assign::register(&mut cx);
        // The concrete addition op `+` resolves to, registered before `+` so the
        // abstract operator can point at it.
        let add_i32 = add::register(&mut cx);
        let plus = plus::register(&mut cx);
        // Each abstract arithmetic operator is registered after the concrete op it
        // resolves to, matching `add_i32`/`+`.
        let sub_i32 = sub::register(&mut cx);
        let minus = minus::register(&mut cx);
        let mul_i32 = mul::register(&mut cx);
        let times = times::register(&mut cx);
        let lt_i32 = cmp::register(&mut cx);
        let lt = lt::register(&mut cx);
        let if_ = if_mod::register(&mut cx);
        fn_mod::register_syntax(&mut cx);
        paren::register(&mut cx);
        return_mod::register(&mut cx);
        // `:=` is a parse-time-only token; the driver dispatches on its Construct.
        declare::register(&mut cx);
        let struct_ = struct_mod::register(&mut cx);

        let Cx { metas, bcode, lower, .. } = cx;
        Core {
            type_,
            scope_,
            root_scope,
            fn_type,
            i32_,
            bool_,
            assign,
            plus,
            add_i32,
            minus,
            sub_i32,
            times,
            mul_i32,
            lt,
            lt_i32,
            if_,
            rational,
            struct_,
            metas,
            bcode,
            lower,
        }
    }

    /// The core type handles the parser needs to type the nodes it opens and to
    /// resolve abstract operators.
    pub fn types(&self) -> CoreTypes {
        CoreTypes {
            scope: self.scope_,
            struct_: self.struct_,
            fn_type: self.fn_type,
            i32_: self.i32_,
            bool_: self.bool_,
            rational: self.rational,
            add_i32: self.add_i32,
            plus: self.plus,
            minus: self.minus,
            times: self.times,
            sub_i32: self.sub_i32,
            mul_i32: self.mul_i32,
            lt: self.lt,
            lt_i32: self.lt_i32,
        }
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

/// Build a plain binary application `{ty: op, value: [lhs, rhs]}`, used by `=`
/// (which is not type-resolved). `+` uses its own resolving builder instead (see
/// [`plus`]); the shared `CoreTypes` parameter lets a builder resolve operand types
/// but is unused here.
pub(crate) fn build_binary(
    store: &mut Store,
    _types: &CoreTypes,
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

/// Whether `node` produces a number an arithmetic operator can compute over: an
/// `i32`, a `rational` literal (which molds to i32), the result of another
/// arithmetic operator (`+`/`-`/`*`), or a *call* whose callee is a function. The
/// seed has one numeric machine type, so any fn-typed callee is treated as numeric
/// — a recursive self-call reaches its operator before the callee is bound, so its
/// output type cannot be read yet. A non-numeric operand (a `struct`, …) leaves the
/// operator unresolved ([`ParseError::UnsupportedOperands`]); this widens as
/// `f32`/`u64`/… arrive.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_numeric(types: &CoreTypes, node: DyadPtr) -> bool {
    let ty = (*node).ty;
    if ty == types.i32_
        || ty == types.rational
        || ty == types.plus
        || ty == types.minus
        || ty == types.times
    {
        return true;
    }
    // A call node's `ty` is its callee; a call of a function yields a value.
    !ty.is_null() && (*ty).ty == types.fn_type
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{compile_fn, compile_nullary_i32};
    use crate::parse::{Parser, ScopeStack, FN_BCODE, FN_BODY, FN_INPUT, FN_OUTPUT};
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
            assert_eq!(rational::mold(one), Some(1)); // the rational 1/1 molds to i32 1
            // `+` stayed reflectable (ty is still `+`) and recorded the concrete op
            // it resolved to as its third operand.
            assert_eq!(*sops.add(2), core.add_i32);
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
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `root` is the valid dyad tree just parsed into `store`.
        let result = unsafe { rt.run(root) }.unwrap();
        assert_eq!(result, 1);
        unsafe {
            assert_eq!(std::ptr::read_unaligned(a_val as *const i32), 1);
        }
    }

    #[test]
    fn runs_a_compound_function_by_walking_its_body() {
        // A function with no bcode is interpreted by walking its `body` field. The
        // body `return a + 1` reads an enclosing variable, so the walk resolves `a`
        // and loads it (a non-trivial body, and a valid one: it returns its i32).
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&41i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        let main = {
            let mut p = Parser::new(
                "fn () -> i32 ( return a + 1 )",
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

        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `call`/`main`/body are valid nodes in `store`.
        let result = unsafe { rt.run(call) }.unwrap();
        assert_eq!(result, 42); // a + 1 = 41 + 1
        unsafe {
            assert_eq!(std::ptr::read_unaligned(a_val as *const i32), 41); // a unchanged
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

        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
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
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
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
            let (input, output, body) = (*v.add(FN_INPUT), *v.add(FN_OUTPUT), *v.add(FN_BODY));
            assert_eq!((*input).ty, core.struct_); // input is a struct
            let iops = (*input).value as *const DyadPtr;
            assert!((*iops.add(1)).is_null()); // no fields (scope then terminator)
            assert_eq!(output, core.i32_); // return type i32
            assert!(!body.is_null());
        }

        // Apply it and run: run finds no bcode for `func` and walks its body.
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
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
            let (input, output, body) = (*v.add(FN_INPUT), *v.add(FN_OUTPUT), *v.add(FN_BODY));
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
    fn compiles_and_runs_a_fn_with_arguments() {
        // Step B: compile a two-parameter function and call it compiled, diffed
        // against the interpreter. Parameters lower to the function's arguments, so
        // the same `run(call)` that interpreted `add(40, 2)` now calls native code.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let add = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (x : i32, y : i32) -> i32 ( return x + y )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };

        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "add", add).unwrap();
            let mut p =
                Parser::new("add(40, 2)", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };

        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // Oracle: interpret the call.
        // SAFETY: `call`/`add`/args are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();

        // Compile `add` (installs parameterized bcode); keep the artifact alive.
        // SAFETY: `add` is the fn node just built and outlives the call.
        let _compiled = unsafe { compile_fn(&core.lower, add) }.unwrap();
        // JIT: the same `run(call)` now evaluates the arguments and calls native code.
        let jit = unsafe { rt.run(call) }.unwrap();

        assert_eq!(interp, 42);
        assert_eq!(jit, interp); // compiled parameterized call matches the oracle
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
    fn calls_a_function_with_arguments() {
        // The calling convention (interpreted): define a two-parameter function,
        // call it with arguments, and read the parameters in the body. `add(40, 2)`
        // binds x=40, y=2 in a frame and the body `return x + y` reads them.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // Define `add` (its params live in its own scope).
        let add = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (x : i32, y : i32) -> i32 ( return x + y )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };

        // Declare `add`, then parse and run the call `add(40, 2)`.
        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "add", add).unwrap();
            let mut p =
                Parser::new("add(40, 2)", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };

        // The call node applies `add` to its two arguments.
        unsafe {
            assert_eq!((*call).ty, add);
        }

        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `call`/`add`/args are valid nodes just parsed.
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 42);
    }

    #[test]
    fn calling_with_the_wrong_arity_errors() {
        // Too few arguments for the parameters is a run error, not a bad read.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let add = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (x : i32, y : i32) -> i32 ( return x + y )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };

        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "add", add).unwrap();
            let mut p = Parser::new("add(40)", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };

        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `call`/`add` are valid nodes just parsed.
        assert_eq!(unsafe { rt.run(call) }, Err(crate::run::RunError::ArityMismatch));
    }

    #[test]
    fn fn_body_return_is_optional() {
        // `return` is optional: a body is valued by what it evaluates to, so a bare
        // `( 40 + 2 )` yields 42 just like `( return 40 + 2 )` does.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let func = {
            let mut p = Parser::new(
                "fn () -> i32 ( 40 + 2 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };

        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `call`/`func`/body are valid nodes just parsed.
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 42);
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
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
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
        // Milestone 2: a function run both interpreted and Cranelift-JIT-compiled,
        // results diffed. The interpreter is the oracle. The body `return 40 + 2`
        // yields its i32; here through an explicit `return`, though `return` is
        // optional and a bare trailing expression yields the same value (DESIGN ›A
        // scope's value is what it evaluates to‹; see `fn_body_return_is_optional`).
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

        // The same `run`, two paths on one node: interpret first (no bcode yet),
        // then compile and run again (jumps to the installed bcode). Both diffed.
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);

        // Interpreted: bcode is null, so `run` walks the body.
        let interp = unsafe { rt.run(call) }.unwrap();
        unsafe {
            let bcode = *((*func).value as *const DyadPtr).add(FN_BCODE);
            assert!(bcode.is_null());
        }

        // Compile installs the exec@ on `func`; keep the artifact alive for the run.
        // SAFETY: `func` is the fn node just built and outlives the call.
        let _compiled = unsafe { compile_fn(&core.lower, func) }.unwrap();
        unsafe {
            let bcode = *((*func).value as *const DyadPtr).add(FN_BCODE);
            assert!(!bcode.is_null()); // bcode installed on the node
        }

        // JIT: the same `run(call)` now jumps to the installed bcode.
        let jit = unsafe { rt.run(call) }.unwrap();

        assert_eq!(interp, 42);
        assert_eq!(jit, interp); // the compiled path matches the interpreter oracle
    }

    #[test]
    fn rational_decimal_parses_but_is_uncomputable_as_i32() {
        // A decimal is a valid rational literal (it parses), but computing it as an
        // i32 has no exact answer: run and compile both report UncomputableLiteral
        // rather than crashing or silently truncating.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p = Parser::new("3.14", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap() // parsing a decimal succeeds
        };
        unsafe {
            assert_eq!((*node).ty, core.rational);
            assert_eq!(rational::mold(node), None); // 157/50 has no exact i32
        }

        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `node` is the rational literal just parsed.
        assert_eq!(unsafe { rt.run(node) }, Err(crate::run::RunError::UncomputableLiteral));
        // SAFETY: same node; compilation reports the same outcome as the oracle.
        let compiled = unsafe { compile_nullary_i32(&core.lower, node) };
        assert!(matches!(compiled, Err(crate::compile::CompileError::UncomputableLiteral)));
    }

    #[test]
    fn whole_valued_rationals_still_compute() {
        // `6.0` reduces to 6/1 and molds to 6 — integer literals are the den==1 case.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p = Parser::new("6.0", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `node` is the rational literal just parsed.
        assert_eq!(unsafe { rt.run(node) }.unwrap(), 6);
    }

    #[test]
    fn i32_overflow_matches_between_interpreter_and_jit() {
        // 2_000_000_000 + 2_000_000_000 overflows i32; both paths must wrap to the
        // same i32. The interpreter is the oracle, so it must not widen to i64.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn () -> i32 ( 2000000000 + 2000000000 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `call`/`func`/body are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` is the fn node just built and outlives the call.
        let _compiled = unsafe { compile_fn(&core.lower, func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();

        let expected = i64::from(2_000_000_000i32.wrapping_add(2_000_000_000)); // -294967296
        assert_eq!(interp, expected);
        assert_eq!(jit, interp);
    }

    #[test]
    fn four_param_fn_stays_interpreted_and_refuses_to_compile() {
        // The compiled calling convention supports at most three i32 args, so a
        // 4-param function fails compilation (UnsupportedArity) rather than
        // installing bcode a call cannot invoke; interpreted, it runs fine.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let add4 = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (a : i32, b : i32, c : i32, d : i32) -> i32 ( return a + b + c + d )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        // Compilation refuses the arity up front.
        // SAFETY: `add4` is the fn node just built.
        let result = unsafe { compile_fn(&core.lower, add4) };
        assert!(matches!(result, Err(crate::compile::CompileError::UnsupportedArity(4))));

        // Interpreted, the same function computes (bcode was never installed).
        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "add4", add4).unwrap();
            let mut p =
                Parser::new("add4(1, 2, 3, 4)", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `call`/`add4`/args are valid nodes just parsed.
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 10);
    }

    #[test]
    fn compiling_an_uninitialized_read_errors_instead_of_crashing() {
        // A declared-but-uninitialised i32 (null storage) compiled would bake a load
        // from address 0 and SIGSEGV; instead compilation errors with BadValue, the
        // same outcome the interpreter reaches.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        // `x`: an i32 variable with no storage yet.
        let x = store.alloc_raw(core.i32_, std::ptr::null_mut());
        scopes.declare(&mut trie, "x", x).unwrap();

        let node = {
            let mut p = Parser::new("x", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };
        // Interpreter: clean BadValue.
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `node` is the variable reference just parsed.
        assert_eq!(unsafe { rt.run(node) }, Err(crate::run::RunError::BadValue));
        // Compiler: BadValue, not a baked load from address 0.
        // SAFETY: same node; the lowering guards the null storage.
        let compiled = unsafe { compile_nullary_i32(&core.lower, node) };
        assert!(matches!(compiled, Err(crate::compile::CompileError::BadValue)));
    }

    #[test]
    fn plus_is_abstract_and_resolves_to_a_concrete_op() {
        // `+` is not itself a machine addition: it stays reflectable (its node's
        // type is still `+`) but resolves to a concrete op (add_i32) that it stores
        // in its value, and both run and compile delegate to that op. Nested `+`
        // resolves too, and interpreted matches JIT.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn () -> i32 ( 10 + 20 + 12 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        // The body is a `+` node, reflectable as `+`, carrying its resolved op.
        unsafe {
            let body = *((*func).value as *const DyadPtr).add(FN_BODY);
            assert_eq!((*body).ty, core.plus);
            assert_eq!(*((*body).value as *const DyadPtr).add(2), core.add_i32);
        }

        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `call`/`func`/body are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` is the fn node just built and outlives the call.
        let _compiled = unsafe { compile_fn(&core.lower, func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, 42);
        assert_eq!(jit, interp);
    }

    #[test]
    fn parses_and_runs_bool_literals() {
        // `true`/`false` are `bool`-typed literals: they parse to a `bool` node and
        // both tiers read 1/0. The interpreter's generic data path reads the i32;
        // the `bool` lowering bakes it as a constant.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        for (src, expect) in [("true", 1i64), ("false", 0i64)] {
            let node = {
                let mut s = ScopeStack::new();
                s.push(core.root_scope);
                let mut p = Parser::new(src, &mut store, &mut trie, &core.metas, core.types(), s);
                p.parse_expression().unwrap()
            };
            // SAFETY: `node` is the literal just parsed.
            unsafe {
                assert_eq!((*node).ty, core.bool_);
            }
            let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
            // SAFETY: `node` is a valid `bool` literal.
            assert_eq!(unsafe { rt.run(node) }.unwrap(), expect);
            // SAFETY: same node; the `bool` lowering bakes its constant.
            let compiled = unsafe { compile_nullary_i32(&core.lower, node) }.unwrap();
            assert_eq!(i64::from(unsafe { compiled.call_i32() }), expect);
        }
    }

    /// Parse `src` as a nullary i32 fn body, diff the interpreter against the JIT,
    /// and assert both equal `expect`. The interpreter is the oracle.
    fn diff_nullary_fn(src: &str, expect: i64) {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(src, &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `call`/`func`/body are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` is the fn node just built and outlives the call.
        let _compiled = unsafe { compile_fn(&core.lower, func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, expect, "interpreter: {src}");
        assert_eq!(jit, interp, "jit != interpreter: {src}");
    }

    #[test]
    fn subtraction_and_multiplication_match_between_tiers() {
        diff_nullary_fn("fn () -> i32 ( 10 - 3 )", 7);
        diff_nullary_fn("fn () -> i32 ( 2 * 4 )", 8);
        // `*` binds tighter than `+`: 2 + (3 * 4) = 14, not (2 + 3) * 4 = 20.
        diff_nullary_fn("fn () -> i32 ( 2 + 3 * 4 )", 14);
        // `-` shares `+`'s precedence and is left-associative: (10 - 3) - 2 = 5.
        diff_nullary_fn("fn () -> i32 ( 10 - 3 - 2 )", 5);
    }

    #[test]
    fn minus_and_times_resolve_to_concrete_ops() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn () -> i32 ( 6 - 2 * 3 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        // Body is `-`(6, *(2, 3)): `-` resolved to sub_i32, its rhs `*` to mul_i32.
        unsafe {
            let body = *((*func).value as *const DyadPtr).add(FN_BODY);
            assert_eq!((*body).ty, core.minus);
            let bops = (*body).value as *const DyadPtr;
            assert_eq!(*bops.add(2), core.sub_i32);
            let rhs = *bops.add(1);
            assert_eq!((*rhs).ty, core.times);
            assert_eq!(*((*rhs).value as *const DyadPtr).add(2), core.mul_i32);
        }
    }

    #[test]
    fn multiplication_overflow_matches_between_interpreter_and_jit() {
        // 100000 * 100000 overflows i32; both tiers must wrap to the same i32.
        let expected = i64::from(100_000i32.wrapping_mul(100_000));
        diff_nullary_fn("fn () -> i32 ( 100000 * 100000 )", expected);
    }

    #[test]
    fn less_than_matches_between_tiers() {
        diff_nullary_fn("fn () -> i32 ( 3 < 5 )", 1);
        diff_nullary_fn("fn () -> i32 ( 5 < 3 )", 0);
        diff_nullary_fn("fn () -> i32 ( 5 < 5 )", 0);
        // `<` binds looser than arithmetic: (2 + 3) < (4 * 2) = 5 < 8 = 1.
        diff_nullary_fn("fn () -> i32 ( 2 + 3 < 4 * 2 )", 1);
    }

    #[test]
    fn less_than_is_abstract_and_resolves_to_lt_i32() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn () -> i32 ( 3 < 5 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        // The body stays reflectable as `<` and records the concrete op it resolved to.
        unsafe {
            let body = *((*func).value as *const DyadPtr).add(FN_BODY);
            assert_eq!((*body).ty, core.lt);
            assert_eq!(*((*body).value as *const DyadPtr).add(2), core.lt_i32);
        }
    }

    #[test]
    fn if_with_bool_literal_conditions_match_between_tiers() {
        // A `bool`-literal condition selects a branch; both tiers agree.
        diff_nullary_fn("fn () -> i32 ( if (true) (100) else (200) )", 100);
        diff_nullary_fn("fn () -> i32 ( if (false) (100) else (200) )", 200);
        // A comparison condition, nullary so both operands are constants.
        diff_nullary_fn("fn () -> i32 ( if (0 < 1) (100) else (200) )", 100);
        diff_nullary_fn("fn () -> i32 ( if (1 < 0) (100) else (200) )", 200);
    }

    #[test]
    fn if_over_a_parameter_matches_between_tiers() {
        // The non-recursive control-flow shape: a parameterized `if` fn, each branch
        // diffed interpreted vs JIT. n=0 takes the then-branch, n=5 the else.
        for (arg, expect) in [(0i64, 100i64), (5, 200)] {
            let mut store = Store::new();
            let mut trie = RegexTrie::new();
            let core = Core::build(&mut store, &mut trie);

            let func = {
                let mut s = ScopeStack::new();
                s.push(core.root_scope);
                let mut p = Parser::new(
                    "fn (n : i32) -> i32 ( if (n < 1) (100) else (200) )",
                    &mut store,
                    &mut trie,
                    &core.metas,
                    core.types(),
                    s,
                );
                p.parse_expression().unwrap()
            };
            let call = {
                let mut s = ScopeStack::new();
                s.push(core.root_scope);
                s.declare(&mut trie, "f", func).unwrap();
                let src = format!("f({arg})");
                let mut p =
                    Parser::new(&src, &mut store, &mut trie, &core.metas, core.types(), s);
                p.parse_expression().unwrap()
            };
            let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
            // SAFETY: `call`/`func`/body are valid nodes just parsed.
            let interp = unsafe { rt.run(call) }.unwrap();
            // SAFETY: `func` is the fn node just built and outlives the call.
            let _compiled = unsafe { compile_fn(&core.lower, func) }.unwrap();
            let jit = unsafe { rt.run(call) }.unwrap();
            assert_eq!(interp, expect, "interpreter n={arg}");
            assert_eq!(jit, interp, "jit != interpreter n={arg}");
        }
    }

    #[test]
    fn if_with_a_non_bool_condition_is_rejected() {
        // The condition must be a `bool`; a bare number is not one.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut s = ScopeStack::new();
        s.push(core.root_scope);

        let mut p = Parser::new(
            "fn () -> i32 ( if (1) (100) else (200) )",
            &mut store,
            &mut trie,
            &core.metas,
            core.types(),
            s,
        );
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::NonBoolCondition));
    }

    #[test]
    fn nested_if_matches_between_tiers() {
        // A then-branch that is itself an `if`, exercising nested merge blocks.
        diff_nullary_fn(
            "fn () -> i32 ( if (true) ( if (false) (1) else (2) ) else (3) )",
            2,
        );
    }

    #[test]
    fn declaration_binds_a_name_to_a_value() {
        // `x := 5` binds `x` (declared before its value is parsed); the expression's
        // value is the bound node, and a later `x` resolves to that same node.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let decl = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p =
                Parser::new("x := 5", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        // The placeholder became the value: a rational that molds to 5.
        unsafe {
            assert_eq!((*decl).ty, core.rational);
            assert_eq!(rational::mold(decl), Some(5));
        }

        let x_ref = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new("x", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        assert_eq!(x_ref, decl); // the reference resolves to the bound node
        let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
        // SAFETY: `x_ref` is the bound rational node.
        assert_eq!(unsafe { rt.run(x_ref) }.unwrap(), 5);
    }

    #[test]
    fn redeclaration_in_the_same_scope_is_rejected() {
        // `:=` reuses the no-shadowing check: redeclaring a live name is an error.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p =
                Parser::new("y := 1", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap();
        }
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("y := 2", &mut store, &mut trie, &core.metas, core.types(), s);
        assert_eq!(
            p.parse_expression(),
            Err(crate::parse::ParseError::Resolve(crate::parse::ResolveError::Shadowed))
        );
    }

    #[test]
    fn interpreted_recursive_factorial() {
        // The payoff: a recursive function on the interpreter. `fact` names itself
        // via `:=` (declared before its body is parsed), the body branches on `if`,
        // and each call runs on its own parameter frame. `n * fact(n - 1)` resolves
        // `*` while `fact` is still an unbound placeholder — the fn-typed placeholder
        // is what lets the self-call read as numeric.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fact := fn (n : i32) -> i32 ( if (n < 1) (1) else (n * fact(n - 1)) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap();
        }

        for (arg, expect) in [(0i64, 1i64), (1, 1), (5, 120)] {
            let call = {
                let mut s = ScopeStack::new();
                s.push(core.root_scope);
                let src = format!("fact({arg})");
                let mut p =
                    Parser::new(&src, &mut store, &mut trie, &core.metas, core.types(), s);
                p.parse_expression().unwrap()
            };
            let mut rt = Runtime::new(core.fn_type, core.rational, &core.bcode);
            // SAFETY: `call` applies the bound `fact` to a literal.
            assert_eq!(unsafe { rt.run(call) }.unwrap(), expect, "fact({arg})");
        }
    }

    #[test]
    fn plus_over_non_numeric_operands_is_unresolved() {
        // `+` with a non-numeric operand (a struct value) has no concrete machine op
        // to resolve to, so parsing reports UnsupportedOperands.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let mut p =
            Parser::new("struct () + 1", &mut store, &mut trie, &core.metas, core.types(), scopes);
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::UnsupportedOperands));
    }
}
