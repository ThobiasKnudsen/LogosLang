// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The identities' tests: whole programs parsed, run, compiled and compared
//! across the two tiers. A child of `identities`, so `super::*` is the core
//! and every `pub(crate)` helper is in reach (#104).

use super::*;
use crate::compile::{compile_fn, compile_nullary_i32};
use crate::parse::{Parser, ScopeStack, FN_BCODE, FN_BODY, FN_INPUT, FN_OUTPUT};
use crate::run::Runtime;

/// A fresh store and name index with the core built into them: what every
/// test starts from (#108: this line stood in eighty-two tests).
fn new_core() -> (Store, RegexTrie, Core) {
    let mut store = Store::new();
    let mut trie = RegexTrie::new();
    let core = Core::build(&mut store, &mut trie);
    (store, trie, core)
}

/// A record dyad of type `record` naming `identity`, for a name a test
/// declares by hand (its scope is set by `ScopeStack::declare`). Leaked,
/// like every test-built dyad: the process exits.
/// A variable a test mints by hand must carry the storage mark the parser
/// puts on every place it allocates (`alloc_local` -> `global_place`): the
/// reading rule tells a place from a literal's untagged storage by that
/// mark, and `=` and `&` accept only places (#82). Build the storage as
/// `crate::dyad::global_place(store.alloc_bytes(..))`. A null value stays
/// legitimate for a variable a test declares but never writes — reading it
/// is the checked `BadValue` those tests pin.
fn test_record(record_ty: DyadPtr, identity: DyadPtr) -> DyadPtr {
    let fields =
        Box::into_raw(Box::new(Record::new(identity, std::ptr::null_mut(), std::ptr::null_mut())));
    Box::into_raw(Box::new(crate::dyad::Dyad { ty: record_ty, value: fields as *mut u8 }))
}

#[test]
fn parses_a_equals_a_plus_one() {
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    // Declare the variable `a` in the root scope.
    let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();

    let root = {
        let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    // Expect the tree =(a, +(a, 1)).
    unsafe {
        assert_eq!((*root).ty, core.assign);
        let top = (*root).value as *const DyadPtr;
        // =.lhs is a use of `a`: its record, naming the variable.
        assert_eq!(core.through(*top), a);
        let sum = *top.add(1); // =.rhs is the + application
        assert_eq!((*sum).ty, core.plus);
        let sops = (*sum).value as *const DyadPtr;
        assert_eq!(core.through(*sops), a); // +.lhs is a use of a: its record
                                            // +.rhs is the literal `1`, committed to i32 (the type resolved from `a`).
        let one = *sops.add(1);
        assert_eq!((*one).ty, core.i32_);
        assert_eq!(std::ptr::read_unaligned((*one).value as *const i32), 1);
        // `+` stayed reflectable (type is still `+`) and stored the resolved
        // concrete op in its op slot.
        assert_eq!(*sops.add(2), core.ops.arith_leaf(numtype::ArithOp::Add, NumType::I32));
    }
}

#[test]
fn runs_a_equals_a_plus_one() {
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    // `a` is an i32 variable initialised to 0.
    let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();

    let root = {
        let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    // run `a = a + 1`: yields 1 and leaves a holding 1.
    let mut rt = Runtime::new(&core, &mut store);
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
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&41i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();

    let main = {
        let mut p =
            Parser::new("fn () -> i32 ( return a + 1 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    // A nullary application of `main`: its type is `main`.
    let call = store.alloc_raw(main, std::ptr::null_mut());

    let mut rt = Runtime::new(&core, &mut store);
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
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let node = {
        let mut p = Parser::new("( return 40 + 2 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `node` is the valid dyad tree just parsed.
    let result = unsafe { rt.run(node) }.unwrap();
    assert_eq!(result, 42);
}

#[test]
fn nested_scopes_and_bare_return() {
    let (mut store, mut trie, core) = new_core();

    // Bare `return` (no brackets) yields to the top-level expression.
    let bare = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("return 7", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    assert_eq!(unsafe { rt.run(bare) }.unwrap(), 7);

    // Nested brackets group correctly.
    let nested = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("( ( return 5 ) )", rt.store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    assert_eq!(unsafe { rt.run(nested) }.unwrap(), 5);
}

#[test]
fn unclosed_bracket_is_an_error() {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let mut p = Parser::new("( return 1", &mut store, &mut trie, &core, scopes);
    assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::UnclosedBracket));
}

#[test]
fn parses_and_runs_a_fn() {
    // The real fn surface `fn ( params ) -> ret ( body )`. A nullary function
    // returning i32; applying and running it walks the body -> 42.
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let func = {
        let mut p =
            Parser::new("fn () -> i32 ( return 40 + 2 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    // Node shape: `{type: fn, value -> [input, output, body, bcode]}` with an empty
    // input record, an i32 return type, and a body (the `return`).
    unsafe {
        assert_eq!((*func).ty, core.fn_type);
        let v = (*func).value as *const DyadPtr;
        let (input, output, body) = (*v.add(FN_INPUT), *v.add(FN_OUTPUT), *v.add(FN_BODY));
        assert_eq!((*input).ty, core.type_); // input is a record
        assert!(array::items(meta::record_fields_of(input)).is_empty()); // no params
        assert_eq!(output, core.i32_); // return logos i32
        assert!(!body.is_null());
    }

    // Apply it and run: run finds no bcode for `func` and walks its body.
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func`/body are valid nodes in `store`.
    let result = unsafe { rt.run(call) }.unwrap();
    assert_eq!(result, 42);
}

#[test]
fn parses_a_fn_with_a_param_visible_in_the_body() {
    // A parameter is declared in the input record's scope and resolves inside
    // the body: `fn (x : i32) -> i32 ( return x )` parses to a body `return(x)`
    // whose operand is the `x` parameter field. (Running it needs the calling
    // convention — param frame slots — which is later; here we check parsing.)
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let func = {
        let mut p = Parser::new(
            "fn (x := i32 ?) -> i32 ( return x )",
            &mut store,
            &mut trie,
            &core,
            scopes,
        );
        p.parse_expression().unwrap()
    };

    unsafe {
        assert_eq!((*func).ty, core.fn_type);
        let v = (*func).value as *const DyadPtr;
        let (input, output, body) = (*v.add(FN_INPUT), *v.add(FN_OUTPUT), *v.add(FN_BODY));
        assert_eq!(output, core.i32_);
        // The single parameter `x`, an i32 field in the input record.
        let x_field = array::items(meta::record_fields_of(input))[0];
        assert_eq!((*x_field).ty, core.i32_);
        // The body `return x` resolved `x` to that parameter field
        // (`return` is `[value, op]`; the operand is its first slot).
        let return_operand = *((*body).value as *const DyadPtr);
        assert_eq!(core.through(return_operand), x_field);
    }
}

#[test]
fn compiles_and_runs_a_fn_with_arguments() {
    // Step B: compile a two-parameter function and call it compiled, diffed
    // against the interpreter. Parameters lower to the function's arguments, so
    // the same `run(call)` that interpreted `add(40, 2)` now calls native code.
    let (mut store, mut trie, core) = new_core();

    let add = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn (x := i32 ?, y := i32 ?) -> i32 ( return x + y )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };

    let call = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        unsafe { s.declare(&mut trie, "add", test_record(core.record_, add)) }.unwrap();
        let mut p = Parser::new("add(40, 2)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };

    let mut rt = Runtime::new(&core, &mut store);
    // Oracle: interpret the call.
    // SAFETY: `call`/`add`/args are valid nodes just parsed.
    let interp = unsafe { rt.run(call) }.unwrap();

    // Compile `add` (installs parameterized bcode); keep the artifact alive.
    // SAFETY: `add` is the fn node just built and outlives the call.
    let _compiled = unsafe { compile_fn(rt.store, &core.lower, &core, add) }.unwrap();
    // JIT: the same `run(call)` now evaluates the arguments and calls native code.
    let jit = unsafe { rt.run(call) }.unwrap();

    assert_eq!(interp, 42);
    assert_eq!(jit, interp); // compiled parameterized call matches the oracle
}

#[test]
fn fn_without_arrow_is_an_error() {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let mut p = Parser::new("fn () ( return 1 )", &mut store, &mut trie, &core, scopes);
    assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::ExpectedArrow));
}

#[test]
fn calls_a_function_with_arguments() {
    // The calling convention (interpreted): define a two-parameter function,
    // call it with arguments, and read the parameters in the body. `add(40, 2)`
    // binds x=40, y=2 in a frame and the body `return x + y` reads them.
    let (mut store, mut trie, core) = new_core();

    // Define `add` (its params live in its own scope).
    let add = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn (x := i32 ?, y := i32 ?) -> i32 ( return x + y )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };

    // Declare `add`, then parse and run the call `add(40, 2)`.
    let call = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        unsafe { s.declare(&mut trie, "add", test_record(core.record_, add)) }.unwrap();
        let mut p = Parser::new("add(40, 2)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };

    // The call node applies `add` to its two arguments.
    unsafe {
        assert_eq!((*call).ty, add);
    }

    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`add`/args are valid nodes just parsed.
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 42);
}

#[test]
fn calling_with_the_wrong_arity_errors() {
    // Too few arguments for the parameters is a run error, not a bad read.
    let (mut store, mut trie, core) = new_core();

    let add = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn (x := i32 ?, y := i32 ?) -> i32 ( return x + y )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };

    let call = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        unsafe { s.declare(&mut trie, "add", test_record(core.record_, add)) }.unwrap();
        let mut p = Parser::new("add(40)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };

    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`add` are valid nodes just parsed.
    assert_eq!(unsafe { rt.run(call) }, Err(crate::run::RunError::ArityMismatch));
}

#[test]
fn fn_body_return_is_optional() {
    // `return` is optional: a body is valued by what it evaluates to, so a bare
    // `( 40 + 2 )` yields 42 just like `( return 40 + 2 )` does.
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let func = {
        let mut p = Parser::new("fn () -> i32 ( 40 + 2 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func`/body are valid nodes just parsed.
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 42);
}

#[test]
fn parses_an_empty_record() {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let node = {
        let mut p = Parser::new("logos ()", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    // The stored layout (issue #47): a scope, an empty fields array, zero
    // size.
    unsafe {
        assert_eq!((*node).ty, core.type_);
        assert!(!meta::record_scope_of(node).is_null());
        assert!(array::items(meta::record_fields_of(node)).is_empty());
        assert_eq!(meta::record_size_of(node), 0);
    }
}

#[test]
fn parses_a_record_with_typed_fields() {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let node = {
        let mut p = Parser::new(
            "logos (instance = (x := i32 ?, y := i32 ?))",
            &mut store,
            &mut trie,
            &core,
            scopes,
        );
        p.parse_expression().unwrap()
    };

    // Two `:` declaration fields, each typed i32 with an undefined value.
    // No name is stored on the record: the spellings live in the shared
    // name index alone (the scope-filtered resolution below is the one
    // mechanism; a per-record names store is recorded as rejected). The
    // record stores the layout: fields array and packed size (issue #47).
    let (scope, fx, fy) = unsafe {
        assert_eq!((*node).ty, core.type_);
        let fields = array::items(meta::record_fields_of(node));
        assert_eq!(fields.len(), 2);
        assert_eq!(meta::record_size_of(node), 8); // two i32s, packed
        (meta::record_scope_of(node), fields[0], fields[1])
    };
    unsafe {
        assert_eq!((*fx).ty, core.i32_);
        assert!((*fx).value.is_null());
        assert_eq!((*fy).ty, core.i32_);
        assert!((*fy).value.is_null());
    }

    // The field names are declared in the record's own scope (index 0).
    let mut inner = ScopeStack::new();
    inner.push(scope);
    assert_eq!(inner.resolve(&trie, "x").unwrap().identity, fx);
    assert_eq!(inner.resolve(&trie, "y").unwrap().identity, fy);
}

#[test]
fn parses_a_bare_name_field() {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let node = {
        let mut p = Parser::new("logos (instance = (t))", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    // A bare name: one field with an undefined type slot, carried as the
    // 8-byte container in the stored size.
    let (scope, ft) = unsafe {
        let fields = array::items(meta::record_fields_of(node));
        assert_eq!(fields.len(), 1);
        assert_eq!(meta::record_size_of(node), 8);
        (meta::record_scope_of(node), fields[0])
    };
    unsafe {
        assert!((*ft).ty.is_null()); // bare name: logos undefined
        assert!((*ft).value.is_null());
    }

    let mut inner = ScopeStack::new();
    inner.push(scope);
    assert_eq!(inner.resolve(&trie, "t").unwrap().identity, ft);
}

#[test]
fn bare_logos_without_parens_yields_the_classifier() {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    // No `(` after `logos` means no record path: the merged constructor
    // declines the right and yields the classifier itself (a `40` after it
    // would be a second cell in the segment — the leftover error).
    let mut p = Parser::new("logos", &mut store, &mut trie, &core, scopes);
    // A bare name is a use: its record, naming the classifier.
    let value = p.parse_expression().unwrap();
    assert_eq!(unsafe { core.through(value) }, core.type_);
}

#[test]
fn scopes_are_typed_scope() {
    let (mut store, mut trie, core) = new_core();

    // `scope` is a type (its own type is `logos`), and the root scope is one.
    unsafe {
        assert_eq!((*core.scope).ty, core.type_);
        assert_eq!((*core.root_scope).ty, core.scope);
    }

    // A record opens its own `scope`-typed node (stored in its record).
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let node = {
        let mut p =
            Parser::new("logos (instance = (x := i32 ?))", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    unsafe {
        let scope = meta::record_scope_of(node);
        assert_eq!((*scope).ty, core.scope);
    }
}

#[test]
fn jit_matches_the_interpreter() {
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();

    let root = {
        let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    // Oracle: the interpreter, from a = 0.
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `root` is the valid tree just parsed.
    let interp = unsafe { rt.run(root) }.unwrap();
    let interp_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };

    // Reset a to 0, then JIT-compile and call, and diff against the oracle.
    unsafe { std::ptr::write_unaligned(a_val as *mut i32, 0) };
    // SAFETY: `root`/`a` live in `store`, which outlives the call.
    let compiled = unsafe { compile_nullary_i32(&core.lower, &core, root) }.unwrap();
    let jit = unsafe { compiled.call() };
    let jit_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };

    assert_eq!(interp, 1);
    assert_eq!(jit, interp); // same result
    assert_eq!(jit_a, interp_a); // same side effect on a
    assert_eq!(jit_a, 1);
}

#[test]
fn assign_to_a_wide_variable_stores_at_full_width_both_tiers() {
    // A STORED variable wider than i32 must be written at its full width, not
    // truncated to i32. `a := i64 ?` starts at 0; `a = a + 5_000_000_000` must leave
    // the full 5e9 (0x1_2A05F200) — a 4-byte store would drop the high word and
    // leave 705_032_704. This is the case params (frame-bound i64) never exercised:
    // real backing storage assigned through.
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&0i64.to_ne_bytes());
    let a = store.alloc_raw(core.numtypes[NumType::I64 as usize], crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();

    // A nullary `-> i64` fn so the compiled return type is the declared i64
    // (`compile_fn` reads `FN_OUTPUT`); its body assigns into the enclosing `a`.
    let func = {
        let mut p = Parser::new(
            "fn () -> i64 ( a = a + 5000000000, a )",
            &mut store,
            &mut trie,
            &core,
            scopes,
        );
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(func, std::ptr::null_mut());

    let mut rt = Runtime::new(&core, &mut store);
    // Interpreted oracle: 0 + 5e9, and `a` now holds it.
    // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
    let interp = unsafe { rt.run(call) }.unwrap();
    let interp_a = unsafe { std::ptr::read_unaligned(a_val as *const i64) };

    // Reset `a`, compile the fn (installs bcode), run the same call — now it jumps
    // to the compiled body — and diff the result and the side effect on `a`.
    unsafe { std::ptr::write_unaligned(a_val as *mut i64, 0) };
    // SAFETY: `func`/`a` live in `store`, which outlives the call.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    let jit_a = unsafe { std::ptr::read_unaligned(a_val as *const i64) };

    assert_eq!(interp, 5_000_000_000, "interpreter result");
    assert_eq!(interp_a, 5_000_000_000, "interpreter side effect on a");
    assert_eq!(jit, interp, "jit result != interpreter");
    assert_eq!(jit_a, interp_a, "jit side effect on a != interpreter");
}

#[test]
fn milestone_2_fn_runs_interpreted_and_jit_identically() {
    // Milestone 2: a function run both interpreted and Cranelift-JIT-compiled,
    // results diffed. The interpreter is the oracle. The body `return 40 + 2`
    // yields its i32; here through an explicit `return`, though `return` is
    // optional and a bare trailing expression yields the same value (DESIGN ›A
    // scope's value is what it evaluates to‹; see `fn_body_return_is_optional`).
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let func = {
        let mut p =
            Parser::new("fn () -> i32 ( return 40 + 2 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    // The same `run`, two paths on one node: interpret first (no bcode yet),
    // then compile and run again (jumps to the installed bcode). Both diffed.
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);

    // Interpreted: bcode is null, so `run` walks the body.
    let interp = unsafe { rt.run(call) }.unwrap();
    unsafe {
        let bcode = *((*func).value as *const DyadPtr).add(FN_BCODE);
        assert!(bcode.is_null());
    }

    // Compile installs the exec@ on `func`; keep the artifact alive for the run.
    // SAFETY: `func` is the fn node just built and outlives the call.
    let _compiled = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
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
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let node = {
        let mut p = Parser::new("3.14", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap() // parsing a decimal succeeds
    };
    unsafe {
        assert_eq!((*node).ty, core.rational);
        assert_eq!(rational::mold(node), None); // 157/50 has no exact i32
    }

    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `node` is the rational literal just parsed.
    assert_eq!(unsafe { rt.run(node) }, Err(crate::run::RunError::UncomputableLiteral));
    // SAFETY: same node; compilation reports the same outcome as the oracle.
    let compiled = unsafe { compile_nullary_i32(&core.lower, &core, node) };
    assert!(matches!(compiled, Err(crate::compile::CompileError::UncomputableLiteral)));
}

#[test]
fn whole_valued_rationals_still_compute() {
    // `6.0` reduces to 6/1 and molds to 6 — integer literals are the den==1 case.
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let node = {
        let mut p = Parser::new("6.0", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `node` is the rational literal just parsed.
    assert_eq!(unsafe { rt.run(node) }.unwrap(), 6);
}

#[test]
fn i32_overflow_matches_between_interpreter_and_jit() {
    // `a + a` on an i32 variable = 2_000_000_000 overflows i32; both tiers must wrap
    // to the same i32 (the interpreter must not widen to i64). A concrete operand is
    // required: two comptime literals would fold to an exact rational, not wrap.
    let expected = i64::from(2_000_000_000i32.wrapping_add(2_000_000_000)); // -294967296
    diff_var_fn(NumType::I32, 2_000_000_000, "fn () -> i32 ( a + a )", expected);
}

#[test]
fn four_param_fn_stays_interpreted_and_refuses_to_compile() {
    // The compiled calling convention supports at most three i32 args, so a
    // 4-param function fails compilation (UnsupportedArity) rather than
    // installing bcode a call cannot invoke; interpreted, it runs fine.
    let (mut store, mut trie, core) = new_core();

    let add4 = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn (a := i32 ?, b := i32 ?, c := i32 ?, d := i32 ?) -> i32 ( return a + b + c + d )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    // Compilation refuses the arity up front.
    // SAFETY: `add4` is the fn node just built.
    let result = unsafe { compile_fn(&mut store, &core.lower, &core, add4) };
    assert!(matches!(result, Err(crate::compile::CompileError::UnsupportedArity(4))));

    // Interpreted, the same function computes (bcode was never installed).
    let call = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        unsafe { s.declare(&mut trie, "add4", test_record(core.record_, add4)) }.unwrap();
        let mut p = Parser::new("add4(1, 2, 3, 4)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`add4`/args are valid nodes just parsed.
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 10);
}

#[test]
fn compiling_an_uninitialized_read_errors_instead_of_crashing() {
    // A declared-but-uninitialised i32 (null storage) compiled would bake a load
    // from address 0 and SIGSEGV; instead compilation errors with BadValue, the
    // same outcome the interpreter reaches.
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    // `x`: an i32 variable with no storage yet.
    let x = store.alloc_raw(core.i32_, std::ptr::null_mut());
    unsafe { scopes.declare(&mut trie, "x", test_record(core.record_, x)) }.unwrap();

    let node = {
        let mut p = Parser::new("x", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    // Interpreter: clean BadValue.
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `node` is the variable reference just parsed.
    assert_eq!(unsafe { rt.run(node) }, Err(crate::run::RunError::BadValue));
    // Compiler: BadValue, not a baked load from address 0.
    // SAFETY: same node; the lowering guards the null storage.
    let compiled = unsafe { compile_nullary_i32(&core.lower, &core, node) };
    assert!(matches!(compiled, Err(crate::compile::CompileError::BadValue)));
}

#[test]
fn plus_is_abstract_and_resolves_to_a_concrete_op() {
    // `+` is not itself a machine addition: it stays reflectable (its node's logos
    // is still `+`) but resolves to a concrete op it stores in its value, and both
    // run and compile delegate to it. A concrete operand (`a`) keeps it a `+` node —
    // two comptime literals would fold instead. Nested `+` resolves too.
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&10i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();

    let func = {
        let mut p =
            Parser::new("fn () -> i32 ( a + 20 + 12 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    // The body is a `+` node, reflectable as `+`, carrying its resolved op.
    unsafe {
        let body = *((*func).value as *const DyadPtr).add(FN_BODY);
        assert_eq!((*body).ty, core.plus);
        assert_eq!(
            *((*body).value as *const DyadPtr).add(2),
            core.ops.arith_leaf(numtype::ArithOp::Add, NumType::I32)
        );
    }

    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func`/body are valid nodes just parsed.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `func` is the fn node just built and outlives the call.
    let _compiled = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, 42);
    assert_eq!(jit, interp);
}

#[test]
fn a_whole_file_runs_top_to_bottom_like_a_script() {
    // The CLI model (settled, July 2026): `logos file.logos` evaluates the
    // file's top-level scope in order, Python-style — no main function. A
    // declaration statement (a fn literal or a record type standing as an
    // expression) is inert at run time — its work happened at parse — and
    // yields unit; the file's value is its tail expression's.
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let root = {
        let mut p = Parser::new(
            "double := fn (x := i32 ?) -> i32 ( x + x ),\npoint := logos (instance = (a := i32 ?)),\ndouble(21)",
            &mut store,
            &mut trie,
            &core,
            scopes,
        );
        p.parse_sequence().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `root` is the sequence just parsed; its exprs are valid.
    let interp = unsafe { rt.run(root) }.unwrap();
    assert_eq!(interp, 42);

    // The compiled tier agrees: declarations lower to unit, and the whole
    // file compiles once its fn is compiled.
    // SAFETY: the sequence's first expression is the fn declaration; the
    // bound fn is its declared slot.
    // The type body that follows the declaration ran what stood before
    // it, so the declaration stands in the sequence as its ran form.
    let func = unsafe {
        let arr = *((*root).value as *const DyadPtr);
        let first = crate::identities::array::items(arr)[0];
        declare::declared_of(ran::expr_of(&core, first))
    };
    // SAFETY: `func` is the fn node just parsed and outlives the calls.
    let _fc = unsafe { compile_fn(&mut store, &core.lower, &core, func) }.unwrap();
    let compiled = unsafe { compile_nullary_i32(&core.lower, &core, root) }.unwrap();
    // SAFETY: the artifacts are alive; the baked storage outlives the call.
    assert_eq!(unsafe { compiled.call() }, 42);
}

#[test]
fn parses_and_runs_bool_literals() {
    // `true`/`false` are `bool`-typed literals: they parse to a `bool` node and
    // both tiers read 1/0. The interpreter's generic data path reads the i32;
    // the `bool` lowering bakes it as a constant.
    let (mut store, mut trie, core) = new_core();

    for (src, expect) in [("true", 1i64), ("false", 0i64)] {
        let node = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(src, &mut store, &mut trie, &core, s);
            p.parse_expression().unwrap()
        };
        // SAFETY: `node` is the literal just parsed — a use of `true`'s
        // name, its record, read through.
        unsafe {
            assert_eq!((*core.through(node)).ty, core.bool_);
        }
        let mut rt = Runtime::new(&core, &mut store);
        // SAFETY: `node` is a valid `bool` literal.
        assert_eq!(unsafe { rt.run(node) }.unwrap(), expect);
        // SAFETY: same node; the `bool` lowering bakes its constant.
        let compiled = unsafe { compile_nullary_i32(&core.lower, &core, node) }.unwrap();
        assert_eq!(unsafe { compiled.call() }, expect);
    }
}

/// Parse `src` as a nullary i32 fn body, diff the interpreter against the JIT,
/// and assert both equal `expect`. The interpreter is the oracle.
fn diff_nullary_fn(src: &str, expect: i64) {
    let (mut store, mut trie, core) = new_core();

    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(src, &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func`/body are valid nodes just parsed.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `func` is the fn node just built and outlives the call.
    let _compiled = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
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
    // `-` shares `+`'s parse_rank and is left-associative: (10 - 3) - 2 = 5.
    diff_nullary_fn("fn () -> i32 ( 10 - 3 - 2 )", 5);
}

#[test]
fn minus_and_times_resolve_to_concrete_ops() {
    let (mut store, mut trie, core) = new_core();

    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p =
            Parser::new("fn (a := i32 ?) -> i32 ( a - a * 3 )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    // Body is `-`(a, *(a, 3)): `-` resolved to sub_i32, its rhs `*` to mul_i32. A
    // concrete operand keeps each a node — two literals would fold.
    unsafe {
        let body = *((*func).value as *const DyadPtr).add(FN_BODY);
        assert_eq!((*body).ty, core.minus);
        let bops = (*body).value as *const DyadPtr;
        assert_eq!(*bops.add(2), core.ops.arith_leaf(numtype::ArithOp::Sub, NumType::I32));
        let rhs = *bops.add(1);
        assert_eq!((*rhs).ty, core.times);
        assert_eq!(
            *((*rhs).value as *const DyadPtr).add(2),
            core.ops.arith_leaf(numtype::ArithOp::Mul, NumType::I32)
        );
    }
}

#[test]
fn multiplication_overflow_matches_between_interpreter_and_jit() {
    // `a * a` on an i32 variable = 100000 overflows i32; both tiers wrap to the same
    // i32. A concrete operand is required: `100000 * 100000` folds to an exact
    // rational (10^10) that has no i32 and so would not model the wrap.
    let expected = i64::from(100_000i32.wrapping_mul(100_000));
    diff_var_fn(NumType::I32, 100_000, "fn () -> i32 ( a * a )", expected);
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
    let (mut store, mut trie, core) = new_core();

    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p =
            Parser::new("fn (a := i32 ?) -> i32 ( a < 5 )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    // The body stays reflectable as `<` and records the concrete op it resolved to
    // (a concrete operand keeps it a node; two literals would fold to a bool).
    unsafe {
        let body = *((*func).value as *const DyadPtr).add(FN_BODY);
        assert_eq!((*body).ty, core.lt);
        assert_eq!(
            *((*body).value as *const DyadPtr).add(2),
            core.ops.cmp_leaf(numtype::CmpOp::Lt, NumType::I32)
        );
    }
}

#[test]
fn comparison_siblings_match_between_tiers() {
    // >, ==, <=, >=, != each yield the i32 0/1, diffed interpreter vs JIT.
    diff_nullary_fn("fn () -> i32 ( 5 > 3 )", 1);
    diff_nullary_fn("fn () -> i32 ( 3 > 5 )", 0);
    diff_nullary_fn("fn () -> i32 ( 5 > 5 )", 0);
    diff_nullary_fn("fn () -> i32 ( 4 == 4 )", 1);
    diff_nullary_fn("fn () -> i32 ( 4 == 5 )", 0);
    diff_nullary_fn("fn () -> i32 ( 3 <= 3 )", 1);
    diff_nullary_fn("fn () -> i32 ( 4 <= 3 )", 0);
    diff_nullary_fn("fn () -> i32 ( 2 <= 3 )", 1);
    diff_nullary_fn("fn () -> i32 ( 3 >= 3 )", 1);
    diff_nullary_fn("fn () -> i32 ( 2 >= 3 )", 0);
    diff_nullary_fn("fn () -> i32 ( 4 >= 3 )", 1);
    diff_nullary_fn("fn () -> i32 ( 4 != 5 )", 1);
    diff_nullary_fn("fn () -> i32 ( 4 != 4 )", 0);
    // Signed comparison, with a computed negative operand.
    diff_nullary_fn("fn () -> i32 ( 0 - 1 < 0 )", 1);
    // Comparisons bind looser than arithmetic: (2+3) == (10-5) = 5 == 5 = 1.
    diff_nullary_fn("fn () -> i32 ( 2 + 3 == 10 - 5 )", 1);
}

#[test]
fn comparison_siblings_are_bool_conditions_for_if() {
    // Each comparison's result is a `bool`, so it is a valid `if` condition.
    diff_nullary_fn("fn () -> i32 ( if (5 > 3) (100) else (200) )", 100);
    diff_nullary_fn("fn () -> i32 ( if (2 == 3) (100) else (200) )", 200);
    diff_nullary_fn("fn () -> i32 ( if (3 <= 3) (100) else (200) )", 100);
    diff_nullary_fn("fn () -> i32 ( if (3 >= 4) (100) else (200) )", 200);
    diff_nullary_fn("fn () -> i32 ( if (7 != 7) (100) else (200) )", 200);
}

#[test]
fn comparison_siblings_resolve_to_their_concrete_ops() {
    // Each abstract operator stays reflectable and records the concrete op it
    // resolved to at operand index 2 (the trie longest-matches `<=`/`>=`/`==`
    // over `<`/`>`/`=`).
    let (mut store, mut trie, core) = new_core();

    // A concrete operand (`a`) keeps each a node; two literals would fold to a bool.
    use numtype::CmpOp;
    let cases: [(&str, DyadPtr, DyadPtr); 5] = [
        ("fn (a := i32 ?) -> i32 ( a > 2 )", core.gt, core.ops.cmp_leaf(CmpOp::Gt, NumType::I32)),
        ("fn (a := i32 ?) -> i32 ( a == 2 )", core.eq, core.ops.cmp_leaf(CmpOp::Eq, NumType::I32)),
        ("fn (a := i32 ?) -> i32 ( a <= 2 )", core.le, core.ops.cmp_leaf(CmpOp::Le, NumType::I32)),
        ("fn (a := i32 ?) -> i32 ( a >= 2 )", core.ge, core.ops.cmp_leaf(CmpOp::Ge, NumType::I32)),
        ("fn (a := i32 ?) -> i32 ( a != 2 )", core.ne, core.ops.cmp_leaf(CmpOp::Ne, NumType::I32)),
    ];
    for (src, abstract_op, concrete) in cases {
        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(src, &mut store, &mut trie, &core, s);
            p.parse_expression().unwrap()
        };
        // SAFETY: `func` is the fn node just parsed.
        unsafe {
            let body = *((*func).value as *const DyadPtr).add(FN_BODY);
            assert_eq!((*body).ty, abstract_op, "abstract op for `{src}`");
            assert_eq!(
                *((*body).value as *const DyadPtr).add(2),
                concrete,
                "concrete op for `{src}`"
            );
        }
    }
}

#[test]
fn logical_operators_match_between_tiers() {
    // `and`/`or` truth tables and `not`, diffed interpreter vs JIT.
    diff_nullary_fn("fn () -> i32 ( true and true )", 1);
    diff_nullary_fn("fn () -> i32 ( true and false )", 0);
    diff_nullary_fn("fn () -> i32 ( false and true )", 0);
    diff_nullary_fn("fn () -> i32 ( false and false )", 0);
    diff_nullary_fn("fn () -> i32 ( true or true )", 1);
    diff_nullary_fn("fn () -> i32 ( true or false )", 1);
    diff_nullary_fn("fn () -> i32 ( false or true )", 1);
    diff_nullary_fn("fn () -> i32 ( false or false )", 0);
    diff_nullary_fn("fn () -> i32 ( not (true) )", 0);
    diff_nullary_fn("fn () -> i32 ( not (false) )", 1);
    // Over comparisons (their natural operands).
    diff_nullary_fn("fn () -> i32 ( 1 < 2 and 3 < 4 )", 1);
    diff_nullary_fn("fn () -> i32 ( 1 < 2 and 3 > 4 )", 0);
    diff_nullary_fn("fn () -> i32 ( 1 > 2 or 3 < 4 )", 1);
    diff_nullary_fn("fn () -> i32 ( not (1 < 2) )", 0);
    // Precedence: comparisons tighter than `and` tighter than `or`, so
    // `1<2 and 3>4 or 5<6` = `((1<2) and (3>4)) or (5<6)` = (T and F) or T = T.
    diff_nullary_fn("fn () -> i32 ( 1 < 2 and 3 > 4 or 5 < 6 )", 1);
    // Logical results are bool, so they nest and serve as `if` conditions.
    diff_nullary_fn("fn () -> i32 ( not (1 < 2) or not (3 < 4) )", 0);
    diff_nullary_fn("fn () -> i32 ( if (1 < 2 and 3 < 4) (100) else (200) )", 100);
}

#[test]
fn logical_operators_short_circuit_on_the_interpreter() {
    // `and`/`or` short-circuit: the right operand is not evaluated when the left
    // decides the result. Observed via a right operand that would error (an
    // uninitialized read) but is skipped.
    let (mut store, mut trie, core) = new_core();
    // `y`: a declared-but-uninitialized i32; reading it is a BadValue.
    {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let y = store.alloc_raw(core.i32_, std::ptr::null_mut());
        unsafe { s.declare(&mut trie, "y", test_record(core.record_, y)) }.unwrap();
    }
    let mut rt = Runtime::new(&core, &mut store);

    // The right operand alone errors — this is what short-circuiting must skip.
    let bad = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("y < 1", rt.store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    assert_eq!(unsafe { rt.run(bad) }, Err(crate::run::RunError::BadValue));

    // `false and (y < 1)`: false left operand skips the erroring read → 0.
    let and_sc = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("false and y < 1", rt.store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    assert_eq!(unsafe { rt.run(and_sc) }.unwrap(), 0);

    // `true or (y < 1)`: true left operand skips the erroring read → 1.
    let or_sc = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("true or y < 1", rt.store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    assert_eq!(unsafe { rt.run(or_sc) }.unwrap(), 1);
}

#[test]
fn logical_operators_reject_non_bool_operands() {
    // `and`/`or`/`not` require `bool` operands; a number is not one.
    for src in ["true and 1", "1 or false", "not (1)"] {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(src, &mut store, &mut trie, &core, s);
        assert_eq!(
            p.parse_expression(),
            Err(crate::parse::ParseError::NonBoolOperands),
            "`{src}` should be rejected",
        );
    }
}

#[test]
fn not_takes_a_bare_operand() {
    // `not x` reads the constructed cell to its right — a bool literal
    // folds now — and the parenthesized form still works (the seed's old
    // `not (x)`-only rule was a parsing simplification, not a keep).
    let (mut store, mut trie, core) = new_core();
    for src in ["not true", "not (true)"] {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(src, &mut store, &mut trie, &core, s);
        let node = p.parse_expression().unwrap();
        // SAFETY: `node` is the folded bool literal just built.
        assert_eq!(
            unsafe { crate::parse::bool_literal_value(&core, node) },
            Some(false),
            "`{src}`"
        );
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
                "fn (n := i32 ?) -> i32 ( if (n < 1) (100) else (200) )",
                &mut store,
                &mut trie,
                &core,
                s,
            );
            p.parse_expression().unwrap()
        };
        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            unsafe { s.declare(&mut trie, "f", test_record(core.record_, func)) }.unwrap();
            let src = format!("f({arg})");
            let mut p = Parser::new(&src, &mut store, &mut trie, &core, s);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(&core, &mut store);
        // SAFETY: `call`/`func`/body are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` is the fn node just built and outlives the call.
        let _compiled = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, expect, "interpreter n={arg}");
        assert_eq!(jit, interp, "jit != interpreter n={arg}");
    }
}

#[test]
fn if_with_a_non_bool_condition_is_rejected() {
    // The condition must be a `bool`; a bare number is not one.
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);

    let mut p =
        Parser::new("fn () -> i32 ( if (1) (100) else (200) )", &mut store, &mut trie, &core, s);
    assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::NonBoolCondition));
}

#[test]
fn nested_if_matches_between_tiers() {
    // A then-branch that is itself an `if`, exercising nested merge blocks.
    diff_nullary_fn("fn () -> i32 ( if (true) ( if (false) (1) else (2) ) else (3) )", 2);
}

#[test]
fn else_less_if_is_a_unit_statement_both_tiers() {
    // An else-less `if` runs its then-branch for its effect when taken and
    // yields unit either way. From a = 41 the branch is taken (41 < 100) and
    // bumps a to 42; from a = 100 it is not, and a stays. Both tiers diffed on
    // the unit result and the effect.
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&41i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();
    let func = {
        let mut p = Parser::new(
            "fn () -> void ( if (a < 100) (a = a + 1) )",
            &mut store,
            &mut trie,
            &core,
            scopes,
        );
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // Interpreted: taken bumps a to 42; from 100, not taken, a stays.
    // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 0, "unit (interpreted)");
    assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 42);
    unsafe { std::ptr::write_unaligned(a_val as *mut i32, 100) };
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 0);
    assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 100);
    // Compiled: the same effect and unit on both paths.
    unsafe { std::ptr::write_unaligned(a_val as *mut i32, 41) };
    // SAFETY: `func`/`a` live in `store`, which outlives the call.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 0, "unit (compiled)");
    assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 42);
    unsafe { std::ptr::write_unaligned(a_val as *mut i32, 100) };
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 0);
    assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 100);
}

#[test]
fn else_less_if_in_a_void_fn_yields_unit_both_tiers() {
    // Taken or not, an else-less `if` yields unit through a `-> void` fn.
    diff_typed_call("fn (c := i32 ?) -> void ( if (c < 1) (42) )", "f(0)", 0);
    diff_typed_call("fn (c := i32 ?) -> void ( if (c < 1) (42) )", "f(5)", 0);
}

#[test]
fn an_else_less_if_is_rejected_in_value_positions() {
    // A numeric fn tail needs a value on both paths, so an else-less `if`
    // cannot be one; nor can it feed an operator.
    assert_eq!(parse_err("fn () -> i32 ( if (1 < 2) (1) )"), ParseError::MissingElse);
    assert_eq!(parse_err("( if (1 < 2) (1) ) + 1"), ParseError::UnsupportedOperands);
}

#[test]
fn the_else_binds_to_the_outer_if_across_a_bracketed_branch() {
    // The dangling-else question: the inner `if` is else-less inside its
    // bracketed branch, so the bracket ends its reach and the `else` belongs to
    // the outer `if`. With a = 5 the outer condition is false and the
    // else-branch must run (a becomes 7); were the `else` the inner if's,
    // nothing would run.
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&5i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();
    let func = {
        let mut p = Parser::new(
            "fn () -> void ( if (a < 1) ( if (a < 1) (a = a + 1) ) else (a = a + 2) )",
            &mut store,
            &mut trie,
            &core,
            scopes,
        );
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // Interpreted: outer condition false -> else-branch -> a = 7.
    // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 0);
    assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 7);
    // Compiled: reset and diff the same effect.
    unsafe { std::ptr::write_unaligned(a_val as *mut i32, 5) };
    // SAFETY: `func`/`a` live in `store`, which outlives the call.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 0);
    assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 7);
}

#[test]
fn assignment_commits_a_literal_to_the_targets_type() {
    // The rhs literal commits to the variable's declared type at parse time, so
    // an i64 target takes a value past i32 exactly, in both tiers.
    diff_var_fn(NumType::I64, 0, "fn () -> i64 ( a = 5000000000, a )", 5_000_000_000);
    // And a literal with no exact value in the target is a parse error.
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { s.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();
    let mut p = Parser::new("a = 3.5", &mut store, &mut trie, &core, s);
    assert_eq!(p.parse_expression(), Err(ParseError::UncomputableLiteral));
}

#[test]
fn compiled_calls_between_compiled_functions_are_width_general() {
    // The outer's compiled call passes i64 containers and reads back an i64
    // result: 2e9 * 3 = 6e9 crosses i32, so an i32-assuming boundary would
    // truncate. Oracle: outer interpreted over the compiled callee.
    let (mut store, mut trie, core) = new_core();

    let mul = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn (x := i64 ?, y := i64 ?) -> i64 ( x * y )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    let outer = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        unsafe { s.declare(&mut trie, "mul", test_record(core.record_, mul)) }.unwrap();
        let mut p =
            Parser::new("fn () -> i64 ( mul(2000000000, 3) )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(outer, std::ptr::null_mut());

    // SAFETY: `mul` is the fn node just built and outlives every call.
    let _c_mul = unsafe { compile_fn(&mut store, &core.lower, &core, mul) }.unwrap();
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`outer` are valid nodes; the callee's artifact is alive.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `outer` is the fn node just built; both artifacts stay alive.
    let _c_outer = unsafe { compile_fn(rt.store, &core.lower, &core, outer) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();

    assert_eq!(interp, 6_000_000_000, "interpreter over compiled callee");
    assert_eq!(jit, interp, "fully compiled chain != oracle");
}

#[test]
fn compiled_calls_pass_floats_across_the_boundary() {
    // An f64 argument rides the container as raw bits (a bitcast, not an
    // extend), so a float callee is the other ABI shape worth its own diff.
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&2.5f64.to_bits().to_ne_bytes());
    let a = store.alloc_raw(core.numtypes[NumType::F64 as usize], crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();

    let g = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p =
            Parser::new("fn (x := f64 ?) -> f64 ( x + 0.5 )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    unsafe { scopes.declare(&mut trie, "g", test_record(core.record_, g)) }.unwrap();
    let outer = {
        let mut p = Parser::new("fn () -> f64 ( g(a) )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(outer, std::ptr::null_mut());

    // SAFETY: `g` is the fn node just built and outlives every call.
    let _c_g = unsafe { compile_fn(&mut store, &core.lower, &core, g) }.unwrap();
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`outer`/`a` are valid nodes; the callee's artifact is alive.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `outer` is the fn node just built; both artifacts stay alive.
    let _c_outer = unsafe { compile_fn(rt.store, &core.lower, &core, outer) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();

    assert_eq!(interp, 3.0f64.to_bits() as i64, "interpreter over compiled callee");
    assert_eq!(jit, interp, "fully compiled chain != oracle");
}

#[test]
fn compiled_self_recursion_is_width_general() {
    // A recursive i64 fn compiled whole: the self-call passes and returns i64
    // containers, and the base case alone exceeds i32.
    let (mut store, mut trie, core) = new_core();

    let s_fn = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "s := fn (n := i64 ?) -> i64 ( if (n < 1) (2000000000 + 2000000000) else (s(n - 1)) )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    // The expression is the declaration; the bound fn is its declared slot.
    // SAFETY: `s_fn` is the declare node just parsed.
    let s_fn = unsafe { declare::declared_of(s_fn) };

    let call = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("s(3)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // Oracle: interpret the recursion (bcode not installed yet).
    // SAFETY: `call` applies the bound `s` to a literal.
    let interp = unsafe { rt.run(call) }.unwrap();
    // Compile (installs bcode; the artifact must outlive the compiled call).
    // SAFETY: `s_fn` is the fn node just built and outlives every call.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, s_fn) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, 4_000_000_000, "interpreter");
    assert_eq!(jit, interp, "compiled recursion != interpreter");
}

#[test]
fn compiled_call_with_wrong_arity_refuses_to_compile() {
    // `add(40)` for a two-parameter callee: the interpreter errors at run time;
    // compiling the caller refuses up front instead of baking a bad call.
    let (mut store, mut trie, core) = new_core();

    let add = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn (x := i32 ?, y := i32 ?) -> i32 ( x + y )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    // SAFETY: `add` is the fn node just built.
    let _c_add = unsafe { compile_fn(&mut store, &core.lower, &core, add) }.unwrap();
    let outer = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        unsafe { s.declare(&mut trie, "add", test_record(core.record_, add)) }.unwrap();
        let mut p = Parser::new("fn () -> i32 ( add(40) )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    // SAFETY: `outer` is the fn node just built.
    let result = unsafe { compile_fn(&mut store, &core.lower, &core, outer) };
    assert!(matches!(result, Err(crate::compile::CompileError::ArityMismatch)));
}

#[test]
fn call_arguments_commit_to_parameter_types() {
    // An argument literal lands in the parameter's typed slot: an i64 parameter
    // takes a value past i32 exactly, and a float parameter takes a decimal —
    // neither squeezes through an i32 default.
    diff_typed_call("fn (x := i64 ?) -> i64 ( x )", "f(3000000000)", 3_000_000_000);
    diff_typed_call("fn (x := f64 ?) -> f64 ( x + 0.5 )", "f(2.5)", 3.0f64.to_bits() as i64);
}

#[test]
fn an_argument_that_does_not_fit_its_parameter_is_rejected() {
    // A decimal into an integer parameter has no exact value: a parse error at
    // the call site, not a runtime surprise.
    let (mut store, mut trie, core) = new_core();
    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("fn (x := i32 ?) -> i32 ( x )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    unsafe { s.declare(&mut trie, "f", test_record(core.record_, func)) }.unwrap();
    let mut p = Parser::new("f(2.5)", &mut store, &mut trie, &core, s);
    assert_eq!(p.parse_expression(), Err(ParseError::UncomputableLiteral));
}

#[test]
fn recursive_i64_factorial_matches_between_tiers() {
    // The published-signature hint: inside `n * fact(n - 1)` the self-call must
    // read i64 from the declaration's signature (an unbound placeholder would
    // default to i32 and mismatch `n`). 20! = 2.4e18 needs the width.
    let (mut store, mut trie, core) = new_core();

    let fact = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fact := fn (n := i64 ?) -> i64 ( if (n < 1) (1) else (n * fact(n - 1)) )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    // The expression is the declaration; the bound fn is its declared slot.
    // SAFETY: `fact` is the declare node just parsed.
    let fact = unsafe { declare::declared_of(fact) };
    let call = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("fact(20)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call` applies the bound `fact` to a literal.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `fact` outlives every call; the artifact stays alive for the run.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, fact) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, 2_432_902_008_176_640_000, "interpreter 20!");
    assert_eq!(jit, interp, "compiled 20! != interpreter");
}

#[test]
fn hash_comments_are_trivia() {
    // `#` runs to the end of its line, as in the sketch's own files; comments
    // weave through a sequence without separating anything themselves.
    diff_nullary_fn("fn () -> i32 ( # the answer\n a := 40, # forty\n a + 2 )", 42);
}

#[test]
fn while_loop_sums_both_tiers() {
    // The loop shape: block-local typed variables (the sketch's `sum := i32 0`
    // juxtaposition, with real storage), a while statement mutating them, the
    // trailing read. `:=` re-initializes on each run (its snapshot
    // initializer), so the interpreted run and the compiled rerun agree from
    // any prior state — no manual re-zeroing needed.
    diff_nullary_fn(
        "fn () -> i32 ( a := i32 0, i := i32 0, while (i < 5) ( a = a + i, i = i + 1 ), a )",
        10,
    );
}

#[test]
fn while_false_never_runs_its_body() {
    diff_nullary_fn("fn () -> i32 ( a := i32 7, a = 7, while (a < 0) (a = 0), a )", 7);
}

#[test]
fn for_loop_sums_a_range_both_tiers() {
    // `for i in 0..10` is end-exclusive (0 through 9, per the old prototype's
    // ((end-start)/delta).ceil() count): sum = 45. `s := i32 0` re-initializes
    // on each run, so the interpreted and compiled runs agree without a manual
    // reset.
    diff_nullary_fn("fn () -> i32 ( s := i32 0, for i in 0..10 ( s = s + i ), s )", 45);
    // With a step: 0, 2, 4, 6, 8 sum to 20.
    diff_nullary_fn("fn () -> i32 ( s := i32 0, for i in 0..10..2 ( s = s + i ), s )", 20);
    // An empty range runs zero iterations.
    diff_nullary_fn("fn () -> i32 ( s := i32 7, for i in 5..5 ( s = 0 ), s )", 7);
}

#[test]
fn for_loop_endpoints_resolve_a_common_type() {
    // i64 endpoints past i32's range: the loop variable is i64 and the sum
    // crosses i32. 5e9..5e9+3 sums to 15e9 + 3.
    diff_typed_call(
        "fn (n := i64 ?) -> i64 ( s := i64 0, s = 0, for i in n..(n + 3) ( s = s + i ), s )",
        "f(5000000000)",
        15_000_000_003,
    );
    // Mismatched concrete endpoint logos are rejected.
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new(
        "fn (a := i32 ?, b := i64 ?) -> void ( for i in a..b ( a = 0 ) )",
        &mut store,
        &mut trie,
        &core,
        s,
    );
    assert_eq!(p.parse_expression(), Err(ParseError::TypeMismatch));
}

#[test]
fn for_loop_with_a_runtime_non_positive_step_runs_zero_iterations() {
    // The step guard both tiers emit: a runtime step of 0 (or negative) skips
    // the loop instead of wrapping forever.
    diff_typed_call(
        "fn (d := i32 ?) -> i32 ( s := i32 3, s = 3, for i in 0..10..d ( s = 0 ), s )",
        "f(0)",
        3,
    );
}

#[test]
fn for_loop_whose_step_overshoots_the_width_ends_both_tiers() {
    // #111: the increment past i32::MAX ended nowhere before, the counter
    // wrapping negative and satisfying `i < end` again for two billion
    // more iterations. Now the step that does not fit ends the loop, so
    // exactly one iteration runs in each tier.
    diff_typed_call(
        "fn (d := i32 ?) -> i32 ( s := i32 0, for i in 2147483640..2147483647..d ( s = s + 1 ), s )",
        "f(10)",
        1,
    );
}

#[test]
fn for_loop_without_an_index_both_tiers() {
    // `for a..b (body)` with no index is the same loop without the variable
    // (DESIGN ›The scope's constructor is the driver‹, ruled 17 September
    // 2026; #129): the counter is a block-local no name reaches, and both
    // tiers run the one node shape. Ten runs; five with a step of 2.
    diff_nullary_fn("fn () -> i32 ( s := i32 0, for 0..10 ( s = s + 1 ), s )", 10);
    diff_nullary_fn("fn () -> i32 ( s := i32 0, for 0..10..2 ( s = s + 1 ), s )", 5);
    // A range over a name: the cell after `for` is a use, not the variable,
    // as the end and as the start.
    diff_typed_call(
        "fn (n := i32 ?) -> i32 ( s := i32 0, s = 0, for 0..n ( s = s + 2 ), s )",
        "f(4)",
        8,
    );
    diff_typed_call(
        "fn (n := i32 ?) -> i32 ( s := i32 0, s = 0, for n..(n + 3) ( s = s + 1 ), s )",
        "f(4)",
        3,
    );
}

#[test]
fn for_loop_shapes_are_checked() {
    // A literal non-positive step; a for as a value; a return in the body;
    // a fresh spelling without `in`; a non-primary endpoint; a range with
    // no `..` where `in` itself or a known name stands first.
    assert_eq!(parse_err("for i in 0..10..0 ( 1 )"), ParseError::BadStep);
    assert_eq!(parse_err("for i in 10..0..-1 ( 1 )"), ParseError::BadStep);
    assert_eq!(parse_err("fn () -> i32 ( for i in 0..3 ( 1 ) )"), ParseError::StatementAsValue);
    assert_eq!(parse_err("fn () -> void ( for i in 0..3 ( return 1 ) )"), ParseError::EarlyReturn);
    assert_eq!(parse_err("for i 0..3 ( 1 )"), ParseError::ExpectedIn);
    assert_eq!(parse_err("for i in 0 ( 1 )"), ParseError::ExpectedRange);
    assert_eq!(parse_err("fn () -> i32 ( for 0..3 ( 1 ) )"), ParseError::StatementAsValue);
    assert_eq!(parse_err("for in 0..3 ( 1 )"), ParseError::ExpectedRange);
    assert_eq!(parse_err_after(&["x := i32 0"], "for x 0..3 ( 1 )"), ParseError::ExpectedRange);
    assert_eq!(parse_err("for (1)"), ParseError::ExpectedRange);
}

#[test]
fn while_condition_must_be_bool() {
    assert_eq!(parse_err("while (1) (2)"), ParseError::NonBoolCondition);
}

#[test]
fn a_while_loop_is_not_a_value() {
    assert_eq!(parse_err("fn () -> i32 ( while (1 < 2) (3) )"), ParseError::StatementAsValue);
    assert_eq!(parse_err("( while (1 < 2) (3) ) + 1"), ParseError::UnsupportedOperands);
}

#[test]
fn a_return_inside_a_while_body_is_rejected() {
    // v1 has no unwinding to exit a loop with; running the return without
    // exiting would be silently wrong.
    assert_eq!(parse_err("fn () -> void ( while (1 < 2) (return 1) )"), ParseError::EarlyReturn);
}

#[test]
fn juxtaposition_types_a_literal() {
    // `i64 5000000000` is the anonymous typed value (DESIGN ›written by
    // juxtaposition‹): the literal commits exactly to the type before it.
    diff_typed_call("fn () -> i64 ( x := i64 5000000000, x )", "f()", 5_000_000_000);
    // An exact commit, not a wrapping cast: a decimal into i32 is an error.
    assert_eq!(parse_err("i32 3.5"), ParseError::UncomputableLiteral);
    // The settled separator doctrine: `f(i32 3)` is ONE argument (the typed
    // value), where `f(i32, 3)` would be two.
    diff_typed_call("fn (x := i32 ?) -> i32 ( x + 1 )", "f(i32 3)", 4);
}

#[test]
fn division_and_remainder_match_between_tiers() {
    diff_typed_call("fn (x := i32 ?, y := i32 ?) -> i32 ( x / y )", "f(10, 3)", 3);
    // Truncates toward zero, matching the interpreter and Rust.
    diff_typed_call("fn (x := i32 ?, y := i32 ?) -> i32 ( x / y )", "f(-10, 3)", -3);
    diff_typed_call("fn (x := i32 ?, y := i32 ?) -> i32 ( x % y )", "f(10, 3)", 1);
    diff_typed_call("fn (x := i32 ?, y := i32 ?) -> i32 ( x % y )", "f(-10, 3)", -1);
    // `/` binds like `*`: 10 - 4/2 = 8 (the literal quotient folds exactly).
    diff_typed_call("fn (x := i32 ?) -> i32 ( x - 4 / 2 )", "f(10)", 8);
}

#[test]
fn division_by_zero_saturates_to_max_both_tiers() {
    // Settled: a zero divisor yields the type's MAX — a loud sentinel, easier
    // to discover than 0 — and signed MIN/-1, the other impossible quotient,
    // saturates to MAX too. MIN % -1 is the well-defined 0.
    diff_typed_call(
        "fn (x := i32 ?, y := i32 ?) -> i32 ( x / y )",
        "f(10, 0)",
        i64::from(i32::MAX),
    );
    diff_typed_call(
        "fn (x := i32 ?, y := i32 ?) -> i32 ( x % y )",
        "f(10, 0)",
        i64::from(i32::MAX),
    );
    diff_typed_call("fn (x := u8 ?, y := u8 ?) -> u8 ( x / y )", "f(7, 0)", 255);
    diff_typed_call(
        "fn (x := i32 ?, y := i32 ?) -> i32 ( x / y )",
        "f(-2147483648, -1)",
        i64::from(i32::MAX),
    );
    diff_typed_call("fn (x := i32 ?, y := i32 ?) -> i32 ( x % y )", "f(-2147483648, -1)", 0);
}

#[test]
fn float_division_is_ieee_and_float_remainder_is_rejected() {
    diff_typed_call(
        "fn (x := f64 ?, y := f64 ?) -> f64 ( x / y )",
        "f(1.0, 2.0)",
        0.5f64.to_bits() as i64,
    );
    // IEEE: x / 0.0 is inf, in both tiers, no sentinel needed.
    diff_typed_call(
        "fn (x := f64 ?, y := f64 ?) -> f64 ( x / y )",
        "f(1.0, 0.0)",
        f64::INFINITY.to_bits() as i64,
    );
    // No Cranelift float remainder; `%` over floats is rejected at parse.
    assert_eq!(parse_err("fn (x := f64 ?) -> f64 ( x % 2.0 )"), ParseError::UnsupportedOperands);
}

#[test]
fn comptime_division_is_exact_fractions() {
    // 1/3 is an exact fraction at comptime, so (1/3)*3 is exactly 1 — no
    // float could say that — and explicit truncation is the cast.
    diff_typed_call("fn () -> f64 ( f64(1 / 3 * 3) )", "f()", 1.0f64.to_bits() as i64);
    diff_typed_call("fn () -> i32 ( i32(10 / 3) )", "f()", 3);
    diff_typed_call("fn () -> i32 ( 10 % 3 )", "f()", 1);
    // A literal zero divisor has no comptime value.
    assert_eq!(parse_err("i32(1 / 0)"), ParseError::UncomputableLiteral);
    assert_eq!(parse_err("i32(1 % 0)"), ParseError::UncomputableLiteral);
}

#[test]
fn string_literals_parse_and_are_inert() {
    let (mut store, mut trie, core) = new_core();
    let node = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("«hello world»", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    // SAFETY: `node` is the string literal just parsed.
    unsafe {
        assert_eq!((*node).ty, core.string_);
        assert_eq!(crate::identities::string::text(node), b"hello world");
    }
    // Inert: no scalar to read, and no operator accepts it.
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `node` is the string literal just parsed.
    assert_eq!(unsafe { rt.run(node) }, Err(crate::run::RunError::BadValue));
    assert_eq!(parse_err("«a» + 1"), ParseError::UnsupportedOperands);
}

#[test]
fn comments_are_reflectable_nodes_invisible_to_value_flow() {
    // A statement-level `#` builds a comment node: real graph structure whose
    // substance is a string node (both the raw-line and `«…»` forms), never
    // run, never a scope's tail — the 40 + 2 stays the value in both tiers.
    let (mut store, mut trie, core) = new_core();
    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn () -> i32 ( # the answer\n 40 + 2\n # «checked twice» )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    // The body is the sequence [comment, 42, comment] — behind its array —
    // and the tail for typing and value is the 42, committed through the
    // trailing prose.
    unsafe {
        let body = *((*func).value as *const DyadPtr).add(FN_BODY);
        assert_eq!((*body).ty, core.scope);
        let arr = *((*body).value as *const DyadPtr);
        assert_eq!((*arr).ty, core.array_);
        let exprs = crate::identities::array::items(arr);
        let [c1, mid, c2] = exprs else {
            panic!("the sequence should hold exactly three expressions");
        };
        let (c1, mid, c2) = (*c1, *mid, *c2);
        assert_eq!((*c1).ty, core.comment_);
        assert_eq!(crate::identities::string::text((*c1).value.cast()), b"the answer");
        assert_eq!((*mid).ty, core.i32_);
        assert_eq!((*c2).ty, core.comment_);
        assert_eq!(crate::identities::string::text((*c2).value.cast()), b"checked twice");
    }
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func`/body are valid nodes just parsed.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `func` outlives the call; the artifact stays alive.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, 42);
    assert_eq!(jit, interp);
}

#[test]
fn a_scope_of_only_prose_has_no_value() {
    // A bracket holding only prose is a scope with nothing to run: unit,
    // the value-less `()` of a call with no arguments — never an error.
    // (The `#` runs to the end of its line, so the closer sits on the
    // next one.)
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new("( # just a note\n)", &mut store, &mut trie, &core, s);
    let node = p.parse_expression().unwrap();
    // SAFETY: `node` is the scope just parsed; its value is `[exprs, op]`.
    unsafe {
        assert_eq!((*node).ty, core.scope);
        let arr = *((*node).value as *const DyadPtr);
        let exprs = crate::identities::array::items(arr);
        assert_eq!(exprs.len(), 1, "the prose alone");
        assert!(numtype::is_comment_type((*exprs[0]).ty));
    }
}

#[test]
fn pointers_mutate_caller_state_through_calls_both_tiers() {
    // The headline: a pointer parameter (@i32), the address of a caller
    // variable, and a store-through — the callee mutates the caller's x,
    // interpreted and fully compiled.
    let (mut store, mut trie, core) = new_core();
    {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "incr := fn (p := @i32 ?) -> void ( p@ = p@ + 1 )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap();
    }
    let incr = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("incr", &mut store, &mut trie, &core, s);
        // A bare name is its record; the fn node is what it names.
        unsafe { core.through(p.parse_expression().unwrap()) }
    };
    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn () -> i32 ( x := i32 41, x = 41, incr(&x), x )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func`/`incr` are valid nodes just parsed.
    let interp = unsafe { rt.run(call) }.unwrap();
    // Compile the callee first (the caller's call bakes its address).
    // SAFETY: both fn nodes outlive the calls; the artifacts stay alive.
    let _c_incr = unsafe { compile_fn(rt.store, &core.lower, &core, incr) }.unwrap();
    let _c_func = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, 42);
    assert_eq!(jit, interp);
}

#[test]
fn pointer_variables_rewire_both_tiers() {
    // p := &x aliases x; p = &y rewires; p@ reads through the current target.
    // The explicit `p = &x` makes the body idempotent (declarations
    // initialize at parse, and the interpreted run leaves p on y).
    diff_nullary_fn(
        "fn () -> i32 ( x := i32 10, y := i32 20, x = 10, y = 20, p := &x, p = &x, s := i32 0, s = p@, p = &y, s + p@ )",
        30,
    );
}

#[test]
fn pointer_chains_and_field_pointers_work_both_tiers() {
    // A record pointer with q@.x (the postfix-deref ergonomics the syntax was
    // chosen for), a field pointer &pt.y, and double indirection pp@@.x.
    let (mut store, mut trie, core) = new_core();
    declare_point(&mut store, &mut trie, &core);

    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn () -> i32 ( pt := point(3, 4), q := &pt, q@.x = q@.x + 10, fp := &pt.y, fp@ = fp@ + 1, pp := &q, pp@@.x + pt.y )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // x: 3 + 10 = 13 (via q@.x); y: 4 + 1 = 5 (via fp@); 13 + 5 = 18.
    // SAFETY: `call`/`func` are valid nodes just parsed.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `func` outlives the call; the artifact stays alive.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, 18);
    assert_eq!(jit, interp);
}

#[test]
fn record_pointer_fields_hold_addresses_both_tiers() {
    // A pointer-typed field (@i32): constructed from &x, read through h.r@.
    let (mut store, mut trie, core) = new_core();
    {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "holder := logos (instance = (r := @i32 ?))",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap();
    }
    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn () -> i32 ( x := i32 7, x = 7, h := holder(&x), h.r@ + 1 )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func` are valid nodes just parsed.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `func` outlives the call; the artifact stays alive.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, 8);
    assert_eq!(jit, interp);
}

#[test]
fn pointer_misuse_is_rejected() {
    // Deref of a non-pointer; pointer arithmetic; a literal into a pointer
    // variable; & of a comptime binding; a pointer as a numeric function's
    // tail. (& of a parameter is no longer misuse: a parameter is a frame
    // place with storage, like a local — see `address_of_a_parameter_works`.)
    assert_eq!(parse_err("( x := i32 1, x@ )"), ParseError::UnsupportedOperands);
    assert_eq!(parse_err("( x := i32 1, p := &x, p + 1 )"), ParseError::UnsupportedOperands);
    assert_eq!(parse_err("( x := i32 1, p := &x, p = 5 )"), ParseError::TypeMismatch);
    assert_eq!(parse_err("( y := 5, &y )"), ParseError::BadAddressOf);
    assert_eq!(parse_err("fn () -> i32 ( x := i32 1, x = 1, &x )"), ParseError::TypeMismatch);
}

#[test]
fn a_compiled_store_into_a_node_box_takes_its_width_from_the_leaf() {
    // #82, asymmetry 6. `assign::build` bakes the store leaf for the
    // target's width into the node's op slot (`store_leaf(I64)` for a
    // `type ?` or `dyad ?` box), and the interpreter honours it. The
    // compiler re-derived the width from the target's logos instead, and
    // a node box has no numeric tag: `numtype_of_type` reached
    // `NumType::from_tag(18)` and panicked. A compiled body writing an
    // outer box is the reachable shape — the box is global, the store is
    // inside the body. Both tiers now read the leaf, so both answer 1.
    for tail in ["", "f.compile(), "] {
        assert_eq!(
            run_script(&format!(
                "a := type ?, f := fn () -> i32 ( a = i32, 5 ),\n{tail}f(), a == i32"
            )),
            1,
            "type box, {tail:?}"
        );
        assert_eq!(
            run_script(&format!(
                "a := dyad ?, f := fn () -> i32 ( a = f64, 5 ),\n{tail}f(), a == f64"
            )),
            1,
            "dyad box, {tail:?}"
        );
    }
}

#[test]
fn a_bool_literal_agrees_across_tiers() {
    // Pinned before the compiler's data path is routed through the
    // reading rule (#82): `bool::lower` bakes the literal as an immediate,
    // the `Scalar` arm will load it from its storage. Same value either way.
    for tail in ["", "f.compile(), "] {
        assert_eq!(run_script(&format!("f := fn () -> bool ( true ),\n{tail}f()")), 1);
        assert_eq!(run_script(&format!("f := fn () -> bool ( false ),\n{tail}f()")), 0);
    }
}

#[test]
fn a_dyad_view_lowers_to_the_node_it_views() {
    // #82, asymmetry 2, closed: the interpreter yields a view's address,
    // and the compiler now has the `Address` arm and bakes the same
    // address. Before the reading rule it declined ("cannot be compiled
    // yet"); the two tiers answer alike.
    assert_eq!(
        run_script("x := i32 5, f := fn () -> i64 ( x:dyad ), a := f(), f.compile(), a == f()"),
        1
    );
}

#[test]
fn a_bare_parameter_compares_by_address() {
    // #82 step 7. A bare parameter "accepts any dyad" (DESIGN ›A function's
    // surface‹) and holds its argument's address in its container, so
    // `a == i32` asks whether that argument IS the identity `i32`. The old
    // `is_node_valued` list admitted only `type`- and `dyad`-typed operands
    // and refused this as uncomputable; the reading rule sees a container.
    assert_eq!(run_script("f := fn (a) -> bool ( a == i32 ), f(i32)"), 1);
    assert_eq!(run_script("f := fn (a) -> bool ( a == i32 ), f(f64)"), 0);
    assert_eq!(run_script("f := fn (a) -> bool ( a != i32 ), f(f64)"), 1);
}

#[test]
fn a_type_is_a_value_a_place_can_hold() {
    // DESIGN ›A type is a comptime value‹ (12 September 2026): "a type
    // value is a node address like any other value, so it may be passed to
    // a function, held in a place, and compared ... a place holding a type
    // is therefore an ordinary place". The seed already did all three
    // through an untyped `fn (a)` parameter; what crashed was the spelling
    // that admitted what was happening, because every reader took the
    // frame tag for a record (#75).
    //
    // Passed in, held, and compared — interpreted and compiled alike,
    // since a compiled comparison of two type values is an ordinary
    // 64-bit equality.
    for tail in ["", "f.compile(), "] {
        assert_eq!(
            run_script(&format!("f := fn (t := type ?) -> bool ( t == i32 ),\n{tail}f(i32)")),
            1
        );
        assert_eq!(
            run_script(&format!("f := fn (t := type ?) -> bool ( t == i32 ),\n{tail}f(f64)")),
            0
        );
        assert_eq!(
            run_script(&format!(
                "f := fn (t := type ?, u := type ?) -> bool ( t != u ),\n{tail}f(i32, f64)"
            )),
            1
        );
    }
    // And handed on to another function that declares it the same way.
    assert_eq!(
        run_script(
            "f := fn (t := type ?) -> bool ( t == f64 ),\ng := fn (u := type ?) -> bool ( f(u) ),\ng(f64)"
        ),
        1
    );
    // A `type` parameter takes a type and nothing else.
    assert_eq!(
        parse_err_after(&["f := fn (t := type ?) -> i32 ( 1 )"], "f(5)"),
        ParseError::TypeMismatch
    );

    // What stays in the pass is the elaboration. A hole whose *layout*
    // would come from a runtime type is the thing DESIGN keeps refused,
    // and a field of the identity cannot be read before the identity is
    // known (that read is #52's reflection).
    assert_eq!(
        parse_err("fn (t := type ?) -> i32 ( x := t ?, 1 )"),
        ParseError::TypeKnownOnlyAtRun
    );
    assert_eq!(
        parse_err("fn (t := type ?) -> f64 ( t.parse_rank )"),
        ParseError::TypeKnownOnlyAtRun
    );

    // Untouched: an identity in hand still folds its comparison at parse,
    // and a `-> type` function still resolves in the pass.
    assert_eq!(run_script("i32 == i32"), 1);
    assert_eq!(
        run_script(
            "f := fn (b := i32 ?) -> type ( if (b < 1) (i32) else (f64) ),\nt := f(0), x := t 5, x"
        ),
        5
    );
    // A box holding a type declares with what it holds, but only where the
    // pass has already run the assignment that filled it — which is the
    // real top level, not this helper (it parses the whole sequence before
    // running any of it). That case is `a_type_box_is_an_ordinary_variable`
    // in tests/cli.rs, against the binary.
}

#[test]
fn a_type_returning_body_must_hand_back_a_type() {
    // #76: `eval_type_call` reads a `-> type` call's result bits as a node
    // address, so a body handing back a number handed back an address that
    // was never a node and the process died. The body is where that is
    // knowable, so it is refused at the definition, every tail leaf of it.
    assert_eq!(parse_err("fn () -> type ( 5 )"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(&["x := i32 5"], "fn () -> type ( x )"), ParseError::TypeMismatch);
    // Through the tail positions: a `return`, and both arms of an `if`.
    assert_eq!(parse_err("fn () -> type ( return 5 )"), ParseError::TypeMismatch);
    assert_eq!(
        parse_err("fn (b := i32 ?) -> type ( if (b < 1) (i32) else (5) )"),
        ParseError::TypeMismatch
    );

    // What must still pass: a type, and a type from either arm.
    assert_eq!(run_script("f := fn () -> type ( i32 ), g := f(), x := g 5, x"), 5);
    assert_eq!(
        run_script(
            "f := fn (b := i32 ?) -> type ( if (b < 1) (i32) else (i64) ),\ng := f(0), x := g 7, x"
        ),
        7
    );
}

#[test]
fn runaway_depth_is_a_checked_error_not_an_abort() {
    // #80: the parser recurses per open bracket and the interpreter per
    // call, so a wall of `(` and a recursion with no base case both used
    // to overflow the Rust stack and abort the process with no
    // diagnostic. Both are now checked, and both limits are generous:
    // what a person writes is nowhere near them.
    //
    // On the seed's own stack, the one the binary hands itself, so the
    // guard fires before the stack does. A test thread's default 2 MiB is
    // a quarter of the main thread's and would abort the whole runner.
    let work = std::thread::Builder::new()
        .stack_size(crate::WORK_STACK_BYTES)
        .spawn(|| {
            let deep = "(".repeat(crate::parse::MAX_BRACKET_DEPTH + 1);
            assert_eq!(parse_err(&format!("{deep}1")), ParseError::TooDeep);
            assert_eq!(run_script("(((((1)))))"), 1);

            let runaway = "f := fn (n := i32 ?) -> i32 ( f(n + 1) ), f(1)";
            assert_eq!(run_script_result(runaway), Err(crate::run::RunError::CallDepth));
            // A recursion that ends is untouched, at a depth well past
            // where the unguarded interpreter used to die in a debug build.
            let ending = "f := fn (n := i32 ?) -> i32 ( if (n < 1) (0) else (f(n - 1)) ), f(9000)";
            assert_eq!(run_script(ending), 0);
        })
        .expect("the work thread must start");
    work.join().expect("depth guards hold");
}

#[test]
fn a_read_or_write_through_the_hole_is_the_checked_error_both_tiers() {
    // DESIGN ›Both slots of a dyad follow one lifecycle‹: "`undefined` is
    // the hole ... and reading it is a checked error, never undefined
    // behavior", and DESIGN ›Errors are values‹ rules what a checked error
    // is in v0.1.0 — "not a value but a fault ... the run aborts with its
    // message". A `p := @i32 ?` pointer holds nothing, so a read or a
    // write through it is that fault, never the access at address 0 that
    // took the process down with a SIGSEGV (#74).
    use crate::run::RunError::NullPointer;
    assert_eq!(run_script_result("p := @i32 ?, p@"), Err(NullPointer));
    assert_eq!(run_script_result("p := @i32 ?, p@ = 5, 1"), Err(NullPointer));

    // Compiled, the same guard is the null arm of the lowering: the fault
    // the native parks is the error the call hands back, so both tiers
    // answer alike. A float pointee takes the same arm.
    assert_eq!(
        run_script_result("f := fn () -> i32 ( p := @i32 ?, p@ ), f.compile(), f()"),
        Err(NullPointer)
    );
    assert_eq!(
        run_script_result("f := fn () -> i32 ( p := @i32 ?, p@ = 5, 1 ), f.compile(), f()"),
        Err(NullPointer)
    );
    assert_eq!(
        run_script_result("f := fn () -> f64 ( p := @f64 ?, p@ ), f.compile(), f()"),
        Err(NullPointer)
    );

    // The guard costs a live pointer nothing but the branch: reads and
    // writes through a real address still work on both tiers.
    assert_eq!(run_script("c := i32 7, q := &c, q@"), 7);
    assert_eq!(
        run_script("f := fn () -> i32 ( c := i32 1, q := &c, q@ = 9, c ), f.compile(), f()"),
        9
    );
}

#[test]
fn address_of_a_parameter_works_both_tiers() {
    // A parameter is a frame place — a field of the call's activation
    // record — so `&a` yields its per-call address and `q@` reads the
    // argument back through it, on both tiers alike.
    diff_typed_call("fn (a := i32 ?) -> i32 ( q := &a, q@ )", "f(7)", 7);
    // Writing through the pointer writes the parameter's slot.
    diff_typed_call("fn (a := i32 ?) -> i32 ( q := &a, q@ = 5, a )", "f(7)", 5);
}

#[test]
fn parameter_reassignment_works_both_tiers() {
    // A parameter's slot is written like a local's: reassignment is
    // ordinary storage, agreed on by interpreter and JIT. (Before
    // parameters had frame slots, this was a runtime error interpreted and
    // a wild store compiled.)
    diff_typed_call("fn (a := i32 ?) -> i32 ( a = a + 1, a )", "f(41)", 42);
}

/// Parse `src` as a whole script (a top-level sequence) and run it with the
/// compiler attached (so `f.compile()` works), returning the run result.
fn run_script_result(src: &str) -> Result<i64, crate::run::RunError> {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let root = {
        let mut p = Parser::new(src, &mut store, &mut trie, &core, scopes).with_lower(&core.lower);
        p.parse_sequence().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store).with_compiler(&core.lower);
    // SAFETY: `root` is the sequence just parsed; its exprs are valid.
    unsafe { rt.run(root) }
}

/// [`run_script_result`], unwrapped: the script's tail value.
fn run_script(src: &str) -> i64 {
    run_script_result(src).unwrap()
}

#[test]
fn interpreted_recursion_stacks_frames() {
    // Deep interpreted recursion: each call claims its own frame from the
    // activation stack, with the parameter and the local at distinct
    // per-call slots. The sum 1..=500 = 125250 is right only if no call's
    // frame aliases another's — 500 live frames stacked at once.
    let src = "f := fn (n := i64 ?) -> i64 ( m := i64 0, m = n, if (n == 0) (0) else (m + f(n - 1)) ),\nf(500)";
    assert_eq!(run_script(src), 125_250);
}

#[test]
fn nested_calls_in_argument_position_release_frames_lifo() {
    // An argument that is itself a call: the callee's frame is claimed
    // before the arguments evaluate, the inner calls claim and release
    // theirs above it, and the outer frame is still intact when its
    // parameters are written. g(3) = 6 and g(4) = 8, so f sees 14.
    let src = "g := fn (x := i64 ?) -> i64 ( x + x ),\nf := fn (a := i64 ?, b := i64 ?) -> i64 ( a + b ),\nf(g(3), g(4) + g(0))";
    assert_eq!(run_script(src), 14);
}

#[test]
fn a_bare_parameter_carries_the_container() {
    // A bare `name` parameter (DESIGN: accepts any type-value dyad) has no
    // declared type; its frame slot carries the full i64 bit-container and
    // a read yields it back.
    assert_eq!(run_script("f := fn (a) -> i64 ( a ),\nf(42)"), 42);
}

#[test]
fn compile_member_installs_code_the_next_call_jumps_to() {
    // `f.compile()` (DESIGN: "The `fn` logos carries two shared functions:
    // `compile` … and `run`"): write the body, compile, and the next call
    // runs the machine code — same answer the body walk gives.
    assert_eq!(
        run_script("double := fn (x := i64 ?) -> i64 ( x + x ),\ndouble.compile(),\ndouble(21)"),
        42
    );
    // Compiled recursion through the member: the self-call jumps too.
    assert_eq!(
        run_script(
            "fact := fn (n := i64 ?) -> i64 ( if (n < 2) (1) else (n * fact(n - 1)) ),\nfact.compile(),\nfact(20)"
        ),
        2_432_902_008_176_640_000
    );
}

#[test]
fn compile_member_before_and_after_agree() {
    // A call before the compile walks the body; after, it jumps. Both
    // answers in one script, summed: 42 + 42.
    assert_eq!(
        run_script(
            "double := fn (x := i64 ?) -> i64 ( x + x ),\na := double(21),\ndouble.compile(),\na + double(21)"
        ),
        84
    );
}

/// A power operator defined with `type`, its constructor and its code in
/// Logos: the shape of the v0.1.0 demo (#63), spelled with a word so the
/// script needs no fresh symbol.
const POW_TYPE: &str = "pw := type (\n\
     instance = ( a := ?, b := ?, shared run = fn (a := i32 ?, b := i32 ?) -> i32 ( r := i32 1, for i in 0..b ( r = r * a ), r ) ),\n\
     parse_rank = *.parse_rank + 1,\n\
     associativity = right,\n\
     parse = (\n\
         this.a = tape[-1],\n\
         this.b = tape[1],\n\
         tape[0] = this,\n\
         tape.is_constructed[0] = true,\n\
         tape.remove(1),\n\
         tape.remove(-1)\n\
     )\n\
    ),\n";

/// A code-carrying type with no constructor of its own: applied like a fn.
const SQ_TYPE: &str =
    "sq2 := type ( instance = ( shared run = fn (a := i32 ?) -> i32 ( a * a ) ) ),\n";

#[test]
fn a_type_with_code_runs_as_a_call_of_it() {
    // DESIGN ›Execution is function application‹ (#63): "if the type
    // carries a `code`, run that function on it … A node typed `^` thus
    // runs and compiles exactly as a node typed `f` does". The infix node
    // the constructor builds and the applied form `pw(2, 3)` are one call;
    // the result types through `+`; `2 pw 3 pw 2` associates right. A
    // type with a code and no constructor of its own is applied like a
    // function: `sq2(5)` is the call (`pw(2, 3)` runs `pw`'s own infix
    // constructor instead, "X's constructor decides what the bracket is").
    assert_eq!(run_script(&format!("{POW_TYPE}2 pw 10 + 1")), 1025);
    assert_eq!(run_script(&format!("{POW_TYPE}2 pw 3 pw 2")), 512);
    assert_eq!(run_script(&format!("{SQ_TYPE}sq2(5) + 1")), 26);
}

#[test]
fn a_type_with_code_compiles_as_a_call_of_it() {
    // The caller compiles to a direct call once the code is compiled (the
    // uncompiled-callee boundary is #65); both tiers agree. `.run` is the
    // fn itself, which is what `pw.run.compile()` compiles.
    assert_eq!(
        run_script(&format!(
            "{POW_TYPE}f := fn (x := i32 ?) -> i32 ( x pw 3 + 1 ),\na := f(2),\npw.run.compile(), f.compile(),\na + f(2)"
        )),
        18
    );
    assert_eq!(
        run_script(&format!(
            "{SQ_TYPE}f := fn (x := i32 ?) -> i32 ( sq2(x) + 1 ),\nsq2.run.compile(), f.compile(),\nf(4)"
        )),
        17
    );
    // The pinned demo's shape: only the caller compiles, the operator's
    // code stays interpreted behind the boundary (#65).
    assert_eq!(
        run_script(&format!(
            "{POW_TYPE}f := fn (x := i32 ?) -> i32 ( x pw 3 + 1 ),\nf.compile(),\nf(2)"
        )),
        9
    );
}

#[test]
fn a_code_slot_holds_a_function_and_types_no_place() {
    // `code = 5` is refused; a hole typed by a code-carrying type has no
    // place, exactly as `f ?` has none (a value of it is a call node).
    assert_eq!(parse_err("t := type (instance = (shared run = 5))"), ParseError::BadRunSlot);
    assert_eq!(parse_err_after(&[POW_TYPE], "p := pw ?"), ParseError::NonNumericDeclaredType);
}

#[test]
fn a_compiled_caller_reaches_an_uncompiled_callee_through_the_interpreter() {
    // DESIGN ›The callable ground‹ (#65): "`compile` never fails on an
    // uncompiled Logos callee — the call is emitted as a jump into the
    // interpreter … Compile order therefore decides the call's shape". f
    // compiled first walks g's body through the boundary; compiling g and
    // then f again makes the call direct; both answers agree.
    assert_eq!(
        run_script(
            "g := fn (x := i32 ?) -> i32 ( x + 1 ),\nf := fn (x := i32 ?) -> i32 ( g(x) * 2 ),\nf.compile(),\na := f(3),\ng.compile(), f.compile(),\na + f(3)"
        ),
        16
    );
    // A chain with only the outermost compiled, a nullary callee among
    // them (no argument slot), and a void callee run for effect.
    assert_eq!(
        run_script(
            "h := fn () -> i32 ( 5 ),\ng := fn (x := i32 ?) -> i32 ( h() + x ),\nf := fn (x := i32 ?) -> i32 ( g(x) * 2 ),\nf.compile(),\nf(1)"
        ),
        12
    );
    assert_eq!(
        run_script(
            "v := fn (x := i32 ?) -> void ( x + 1 ),\nf := fn (x := i32 ?) -> i32 ( v(x), x * 2 ),\nf.compile(),\nf(4)"
        ),
        8
    );
}

#[test]
fn a_second_compile_replaces_the_entry() {
    // Compiling again compiles again (the recompile that lifts a boundary,
    // #65); the value is the same and nothing errors.
    assert_eq!(
        run_script(
            "double := fn (x := i64 ?) -> i64 ( x + x ),\ndouble.compile(),\ndouble.compile(),\ndouble(21)"
        ),
        42
    );
}

#[test]
fn compile_member_is_a_statement_not_a_value() {
    assert_eq!(
        parse_err("fn () -> i32 ( g := fn () -> i32 ( 1 ), g.compile() )"),
        ParseError::StatementAsValue
    );
}

#[test]
fn a_type_returning_fn_compiles_and_serves_comptime_calls() {
    // A type value is a node address, so a `-> logos` function is integers
    // in, an integer out — it compiles (type nodes bake as i64 address
    // immediates, run's own rule), and the parse-time comptime evaluation
    // of later `metatype(...)` calls jumps to the installed code. This
    // used to panic on the type root's record tag.
    assert_eq!(
        run_script(
            "metatype := fn (i := i32 ?) -> logos ( if (i == 0) (i32) else (f64) ),\nmetatype.compile(),\nsame := metatype(0) == i32,\nother := metatype(1) == f64,\nif (same and other) (i64 1) else (i64 0)"
        ),
        1
    );
}

#[test]
fn compile_member_on_four_params_reports_cleanly() {
    // The compiled convention carries at most three arguments in v1;
    // `.compile()` on a wider fn is a clean run error, and the function
    // stays interpreted.
    let e = run_script_result(
        "f := fn (a := i64 ?, b := i64 ?, c := i64 ?, d := i64 ?) -> i64 ( a + b + c + d ),\nf.compile()",
    )
    .unwrap_err();
    assert!(matches!(e, crate::run::RunError::CompileFailed(_)), "got {e:?}");
}

#[test]
fn an_addressed_local_stays_in_memory_under_promotion() {
    // Register promotion must not lift a local whose address is taken: the
    // write through `p` targets `a`'s frame slot, and the following read
    // of `a` must see it on both tiers. A promoted `a` would return the
    // stale 5.
    diff_typed_call("fn (x := i64 ?) -> i64 ( a := i64 5, p := &a, p@ = 7, a + x )", "f(1)", 8);
}

#[test]
fn a_promoted_loop_matches_the_interpreter() {
    // The register-promotion showcase: loop counter and accumulator are
    // clean scalars (no address taken), so the compiled loop runs them in
    // registers — and the value still matches the interpreted walk.
    assert_eq!(
        run_script(
            "sum_to := fn (n := i64 ?) -> i64 ( i := i64 0, s := i64 0, while (i < n) ( s = s + i, i = i + 1 ), s ),\nbefore := sum_to(1000),\nsum_to.compile(),\nbefore + sum_to(1000)"
        ),
        999_000
    );
}

#[test]
fn compile_is_reserved_only_on_fn_typed_values() {
    // A record field named `compile` still resolves as an ordinary field:
    // the member intercept fires only when the lhs is fn-typed, so the
    // spelling is not globally reserved (unlike `.logos`).
    assert_eq!(
        run_script("point := logos (instance = (compile := i32 ?)),\np := point(7),\np.compile"),
        7
    );
}

#[test]
fn an_argument_must_be_the_parameters_logos() {
    // DESIGN ›A function's surface‹: the parameter list "*is* a record
    // type, its fields written `name := T ?`" and "the caller's positional
    // arguments are the parameter list's holes, in order" — so filling a
    // hole is the store a typed place takes, the same check `=` makes, and
    // crossing logos needs an explicit cast. Before this, every argument
    // that was not a rational literal passed untouched: `f(i32)` ran the
    // body over the type node's address and `f(g)` over 0 (#83).
    let defs: &[&str] =
        &["f := fn (a := i32 ?) -> i32 ( a )", "g := fn () -> i32 ( 1 )", "x := i64 5"];
    assert_eq!(parse_err_after(defs, "f(i32)"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "f(g)"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "f(«s»)"), ParseError::TypeMismatch);
    // A width that does not match is a crossing like any other.
    assert_eq!(parse_err_after(defs, "f(x)"), ParseError::TypeMismatch);

    // What the check must not refuse: a literal still molds to the
    // parameter (that is the branch above it), an expression of the right
    // logos passes, and a named binding of the right logos passes.
    assert_eq!(run_script("f := fn (a := i32 ?) -> i32 ( a ), f(7)"), 7);
    assert_eq!(run_script("f := fn (a := i32 ?) -> i32 ( a ), f(3 + 4)"), 7);
    assert_eq!(run_script("y := i32 2, f := fn (a := i32 ?) -> i32 ( a ), f(y + 1)"), 3);
    // A bare parameter takes any dyad, so it is not checked.
    assert_eq!(run_script("h := fn (a) -> i32 ( 1 ), h(i32)"), 1);
}

#[test]
fn a_literal_into_a_pointer_parameter_is_rejected() {
    // f(0) against p : @i32 would dereference address 0; the call site
    // rejects it instead.
    let (mut store, mut trie, core) = new_core();
    {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "f := fn (p := @i32 ?) -> void ( p@ = 1 )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap();
    }
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new("f(0)", &mut store, &mut trie, &core, s);
    assert_eq!(p.parse_expression(), Err(ParseError::TypeMismatch));
}

/// Declare `point := logos (instance = (x := i32 ?, y := i32 ?))` in the root scope.
fn declare_point(store: &mut Store, trie: &mut RegexTrie, core: &Core) {
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p =
        Parser::new("point := logos (instance = (x := i32 ?, y := i32 ?))", store, trie, core, s);
    p.parse_expression().unwrap();
}

#[test]
fn record_instances_construct_read_and_write_fields_both_tiers() {
    // The logos applied to its field values constructs the instance
    // (point(3, 4), the type-constructor doctrine); `.` resolves to a place
    // inside the instance's storage, so reads and writes are ordinary numeric
    // paths. Construction re-runs per call, so both tiers start from (3, 4).
    let (mut store, mut trie, core) = new_core();
    declare_point(&mut store, &mut trie, &core);

    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn () -> i32 ( p := point(3, 4), p.x = p.x + 36, p.x + p.y + 2 )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // 3 + 36 = 39; 39 + 4 + 2 = 45.
    // SAFETY: `call`/`func` are valid nodes just parsed.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `func` outlives the call; the artifact stays alive.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, 45);
    assert_eq!(jit, interp);
}

#[test]
fn record_fields_lay_out_mixed_widths() {
    // u8, i64, i32 pack in declaration order (offsets 0, 1, 9); a runtime
    // argument reaches its field, and each field reads back at its own width.
    let (mut store, mut trie, core) = new_core();
    {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "cell := logos (instance = (a := u8 ?, b := i64 ?, c := i32 ?))",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap();
    }
    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn (n := i64 ?) -> i64 ( q := cell(200, n, 7), q.b + i64(q.a) + i64(q.c) )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    let call = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        unsafe { s.declare(&mut trie, "f", test_record(core.record_, func)) }.unwrap();
        let mut p = Parser::new("f(5000000000)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func` are valid nodes just parsed.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `func` outlives the call; the artifact stays alive.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, 5_000_000_207);
    assert_eq!(jit, interp);
}

#[test]
fn construction_and_field_access_are_checked() {
    let (mut store, mut trie, core) = new_core();
    declare_point(&mut store, &mut trie, &core);

    let mut check = |src: &str, expect: ParseError| {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(src, &mut store, &mut trie, &core, s);
        assert_eq!(p.parse_expression(), Err(expect), "`{src}`");
    };
    // Wrong argument count; a literal with no exact field value; an instance
    // used as a value; a construction as a numeric tail; an unknown field.
    check("point(1)", ParseError::CtorArity);
    check("point(1.5, 2)", ParseError::UncomputableLiteral);
    check("point(1, 2) + 1", ParseError::UnsupportedOperands);
    check("fn () -> i32 ( p := point(1, 2) )", ParseError::StatementAsValue);
    check(
        "( p := point(1, 2), p.z )",
        ParseError::Resolve(crate::parse::ResolveError::Unknown("z".into())),
    );
}

#[test]
fn assigning_into_a_comptime_binding_is_rejected() {
    // `x := 5` binds a comptime rational (no machine storage); writing its
    // value slot would corrupt the fraction, so `=` demands a typed variable.
    // The refusal names the literal, so the message can say what to declare.
    assert_eq!(parse_err("( x := 5, x = 7 )"), ParseError::AssignToLiteral(Box::new("5".into())));
    assert_eq!(
        parse_err("( x := 5/2, x = 7 )"),
        ParseError::AssignToLiteral(Box::new("5/2".into()))
    );
    // A literal standing bare as the target is the same refusal.
    assert_eq!(parse_err("( 5 = 7 )"), ParseError::AssignToLiteral(Box::new("5".into())));
}

#[test]
fn sequences_run_in_order_and_yield_the_trailing_expression() {
    // A scope's body is a sequence of self-delimiting expressions with no
    // separator; its value is the trailing one. The body assigns first, so the
    // interpreted and compiled runs are both deterministic from any start.
    diff_var_fn(NumType::I32, 0, "fn () -> i32 ( a = 10, a = a + 1, a + 1 )", 12);
}

#[test]
fn the_comma_is_an_optional_readability_separator() {
    // The same sequence with `,` written between the expressions: the comma
    // marks a boundary the expressions already imply.
    diff_var_fn(NumType::I32, 0, "fn () -> i32 ( a = 10, a = a + 1, a + 1 )", 12);
}

#[test]
fn block_local_declarations_do_not_leak() {
    // `( x := 5, x + 1 )` declares `x` in the block's own scope: the block
    // computes with it (both literals fold to 6), and after the block closes
    // the name is a genuine out-of-scope use, not an unknown one.
    let (mut store, mut trie, core) = new_core();

    let node = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("( x := 5, x + 1 )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `node` is the sequence just parsed.
    assert_eq!(unsafe { rt.run(node) }.unwrap(), 6);
    // SAFETY: same node; the sequence lowering yields its trailing value.
    let compiled = unsafe { compile_nullary_i32(&core.lower, &core, node) }.unwrap();
    assert_eq!(unsafe { compiled.call() }, 6);

    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new("x", &mut store, &mut trie, &core, s);
    assert_eq!(
        p.parse_expression(),
        Err(crate::parse::ParseError::Resolve(crate::parse::ResolveError::OutOfScope("x".into())))
    );
}

#[test]
fn an_early_return_in_a_sequence_is_rejected() {
    // v1 `return` is the tail yield; running one mid-sequence without exiting
    // would be silently wrong, so it is rejected — directly or inside an `if`.
    assert_eq!(parse_err("( return 1, 2 )"), ParseError::EarlyReturn);
    assert_eq!(
        parse_err("fn () -> i32 ( if (true) (return 1) else (0), 2 )"),
        ParseError::EarlyReturn
    );
}

#[test]
fn a_tail_return_in_a_sequence_still_yields() {
    diff_nullary_fn("fn () -> i32 ( 1 + 1, return 40 + 2 )", 42);
}

#[test]
fn adjacent_minus_is_subtraction() {
    // The literal regex is unsigned, so `a-1` lexes as `a`, `-`, `1` —
    // subtraction — never as the statement `a` followed by the literal `-1`.
    diff_var_fn(NumType::I32, 43, "fn () -> i32 ( a-1 )", 42);
}

#[test]
fn negative_literals_via_prefix_minus() {
    // A `-` with no left operand negates the following numeric literal at
    // parse time: as an argument, under a cast, and doubled (0 - -3 = 3).
    diff_typed_call("fn (x := i32 ?) -> i32 ( x )", "f(-1)", -1);
    diff_nullary_fn("fn () -> i32 ( i32(-5) )", -5);
    diff_nullary_fn("fn () -> i32 ( 0 - -3 )", 3);
}

#[test]
fn declaration_binds_a_name_to_a_value() {
    // `x := 5` binds `x` (declared before its value is parsed). The
    // expression is a *declare node* — real graph structure carrying the
    // spelling, the binding, and its native — a statement yielding unit;
    // a later `x` resolves to the bound node itself.
    let (mut store, mut trie, core) = new_core();

    let decl = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("x := 5", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    // The declare node carries the name and the bound value: a rational
    // that molds to 5, held by the fixpointed placeholder.
    let bound = unsafe { declare::declared_of(decl) };
    unsafe {
        assert_eq!((*decl).ty, core.declare_);
        let name_node = *((*decl).value as *const DyadPtr);
        assert_eq!(crate::identities::string::text(name_node), b"x");
        assert_eq!((*bound).ty, core.rational);
        assert_eq!(rational::mold(bound), Some(5));
    }

    let x_ref = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("x", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    // The reference is x's record; it names the bound node.
    assert_eq!(unsafe { core.through(x_ref) }, bound);
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `x_ref`/`decl` are valid nodes just parsed.
    unsafe {
        assert_eq!(rt.run(x_ref).unwrap(), 5);
        // The declaration itself is a statement: it yields unit.
        assert_eq!(rt.run(decl).unwrap(), 0);
    }
    // Unmarked is fail-closed: the gate slot of a bare declaration is null.
    unsafe {
        assert!(declare::gate_of(decl).is_null());
    }
}

#[test]
fn the_view_reads_roles_and_raw_value_at_the_graph_level() {
    // #52 reads whose values are strings or addresses assert at the graph
    // level (strings are inert at the surface, so the REPL cannot echo
    // them): `.logos.roles[i]` is the role-name string, and `.value` is a
    // u64 address value.
    let (mut store, mut trie, core) = new_core();

    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let x_val = store.alloc_bytes(&5i32.to_ne_bytes());
    let x = store.alloc_raw(core.i32_, crate::dyad::global_place(x_val));
    unsafe { s.declare(&mut trie, "x", test_record(core.record_, x)) }.unwrap();

    let mut p = Parser::new("(x + x):dyad.type.roles[0]", &mut store, &mut trie, &core, s);
    let role = p.parse_expression().unwrap();
    // SAFETY: `role` is the role-name string node the read just yielded.
    unsafe {
        assert_eq!(crate::identities::string::text(role), b"lhs");
    }
    let mut s = p.into_scopes();
    s.truncate(1);

    let mut p = Parser::new("x:dyad.value", &mut store, &mut trie, &core, s);
    let value = p.parse_expression().unwrap();
    // SAFETY: `value` is the u64 value node the read just built.
    unsafe {
        assert_eq!((*value).ty, core.numtypes[NumType::U64 as usize]);
        let mut rt = Runtime::new(&core, &mut store);
        // The raw value slot of a place holds its storage *marked* as a
        // place (`GLOBAL_TAG`, 44ea208): that mark is part of what is in the
        // slot, and the view shows the slot, not the address behind it.
        assert_eq!(rt.run(value).unwrap(), crate::dyad::global_place(x_val) as i64);
    }
}

#[test]
fn pub_fills_a_declarations_gate_slot() {
    // `pub x := 5` (#33): the gate word parses the declaration to its
    // right and fills the declare node's gate slot with the `pub`
    // identity — the deviation lives in the declaration's structure,
    // where #58's import will read it. The binding itself is unchanged.
    let (mut store, mut trie, core) = new_core();

    let decl = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("pub x := 5", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    // SAFETY: `decl` is the declare node just parsed.
    unsafe {
        assert_eq!((*decl).ty, core.declare_);
        assert_eq!(declare::gate_of(decl), core.pub_);
        let bound = declare::declared_of(decl);
        assert_eq!((*bound).ty, core.rational);
        assert_eq!(rational::mold(bound), Some(5));
    }
    // The marked declaration still runs as a statement, and the name
    // resolves within its own section exactly as unmarked.
    let x_ref = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("x + 1", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `decl`/`x_ref` are valid nodes just parsed.
    unsafe {
        assert_eq!(rt.run(decl).unwrap(), 0);
        assert_eq!(rt.run(x_ref).unwrap(), 6);
    }
}

#[test]
fn pub_gates_a_typed_declaration() {
    // `pub x : i32`: the typed declaration reduces to a declare node too,
    // so the same gate word marks it.
    let (mut store, mut trie, core) = new_core();

    let decl = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("pub x := i32 ?", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    // SAFETY: `decl` is the declare node just parsed.
    unsafe {
        assert_eq!((*decl).ty, core.declare_);
        assert_eq!(declare::gate_of(decl), core.pub_);
    }
}

#[test]
fn pub_gates_a_fn_declaration() {
    // The shape #58 exposes across a section boundary: a pub fn.
    let (mut store, mut trie, core) = new_core();

    let decl = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "pub double := fn (x := i32 ?) -> i32 ( x + x )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    // SAFETY: `decl` is the declare node just parsed; its binding is the fn.
    unsafe {
        assert_eq!((*decl).ty, core.declare_);
        assert_eq!(declare::gate_of(decl), core.pub_);
        let f = declare::declared_of(decl);
        assert_eq!((*f).ty, core.fn_type);
    }
}

#[test]
fn pub_without_a_declaration_is_an_error() {
    // A gate that marks nothing is a lie in the source, not a no-op.
    let (mut store, mut trie, core) = new_core();

    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new("pub 5", &mut store, &mut trie, &core, s);
    assert!(matches!(p.parse_expression(), Err(crate::parse::ParseError::GateNeedsDeclaration)));
}

#[test]
fn double_pub_is_an_error() {
    let (mut store, mut trie, core) = new_core();

    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new("pub pub x := 5", &mut store, &mut trie, &core, s);
    assert!(matches!(p.parse_expression(), Err(crate::parse::ParseError::DoubleGate)));
}

#[test]
fn redeclaration_in_the_same_scope_is_rejected() {
    // `:=` reuses the no-shadowing check: redeclaring a live name is an error.
    let (mut store, mut trie, core) = new_core();
    {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("y := 1", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap();
    }
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new("y := 2", &mut store, &mut trie, &core, s);
    assert_eq!(
        p.parse_expression(),
        Err(crate::parse::ParseError::Resolve(crate::parse::ResolveError::Shadowed("y".into())))
    );
}

#[test]
fn interpreted_recursive_factorial() {
    // The payoff: a recursive function on the interpreter. `fact` names itself
    // via `:=` (declared before its body is parsed), the body branches on `if`,
    // and each call runs on its own parameter frame. `n * fact(n - 1)` resolves
    // `*` while `fact` is still an unbound placeholder — the fn-typed placeholder
    // is what lets the self-call read as numeric.
    let (mut store, mut trie, core) = new_core();

    {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fact := fn (n := i32 ?) -> i32 ( if (n < 1) (1) else (n * fact(n - 1)) )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap();
    }

    for (arg, expect) in [(0i64, 1i64), (1, 1), (5, 120)] {
        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let src = format!("fact({arg})");
            let mut p = Parser::new(&src, &mut store, &mut trie, &core, s);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(&core, &mut store);
        // SAFETY: `call` applies the bound `fact` to a literal.
        assert_eq!(unsafe { rt.run(call) }.unwrap(), expect, "fact({arg})");
    }
}

#[test]
fn compiled_recursive_factorial_matches_the_interpreter() {
    // Milestone 3: a recursive function on *both* tiers. Compiling `fact` turns
    // its body's `fact(n-1)` into a direct machine `call` to itself, so the whole
    // recursion runs in compiled code. Diffed against the interpreter oracle.
    let (mut store, mut trie, core) = new_core();

    // Parsing the definition returns the bound `fact` node (the fn).
    let fact = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fact := fn (n := i32 ?) -> i32 ( if (n < 1) (1) else (n * fact(n - 1)) )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };
    // The expression is the declaration; the bound fn is its declared slot.
    // SAFETY: `fact` is the declare node just parsed.
    let fact = unsafe { declare::declared_of(fact) };

    let cases = [(0i64, 1i64), (1, 1), (5, 120), (7, 5040)];

    // Oracle: interpret each call (bcode not yet installed → body walk).
    let mut rt = Runtime::new(&core, &mut store);
    for &(arg, expect) in &cases {
        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let src = format!("fact({arg})");
            let mut p = Parser::new(&src, rt.store, &mut trie, &core, s);
            p.parse_expression().unwrap()
        };
        // SAFETY: `call` applies the bound `fact` to a literal.
        assert_eq!(unsafe { rt.run(call) }.unwrap(), expect, "interpreter fact({arg})");
    }

    // Compile `fact` once; the self-call is installed as a machine call.
    // SAFETY: `fact` is the fn node just built and outlives every call.
    let _compiled = unsafe { compile_fn(rt.store, &core.lower, &core, fact) }.unwrap();
    // SAFETY: reading the installed bcode slot of the fn node.
    unsafe {
        let bcode = *((*fact).value as *const DyadPtr).add(FN_BCODE);
        assert!(!bcode.is_null(), "bcode installed");
    }

    // JIT: the same calls now dispatch to compiled code, which recurses natively.
    for &(arg, expect) in &cases {
        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let src = format!("fact({arg})");
            let mut p = Parser::new(&src, rt.store, &mut trie, &core, s);
            p.parse_expression().unwrap()
        };
        // SAFETY: `_compiled` is alive; `call` applies the compiled `fact`.
        assert_eq!(unsafe { rt.run(call) }.unwrap(), expect, "jit fact({arg})");
    }
}

/// Parse `defs` in order (declarations into the shared root scope), take the
/// last as the recursive function `name`, and assert every `(arg, expect)`
/// holds *interpreted* and then *compiled* — the oracle pattern. A local that
/// shared one storage across activations (the pre-activation-record bug) makes
/// a write→recurse→read case disagree, so these lock the fix on both tiers.
///
/// # Safety
/// `defs` must be valid definitions and `name` the last one's function.
unsafe fn assert_recursion_both_tiers(defs: &[&str], name: &str, cases: &[(i64, i64)]) {
    let (mut store, mut trie, core) = new_core();

    let mut last = std::ptr::null_mut();
    for def in defs {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(def, &mut store, &mut trie, &core, s);
        last = p.parse_expression().unwrap();
    }
    let f = declare::declared_of(last);

    let parse_call = |store: &mut Store, trie: &mut RegexTrie, arg: i64| {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let src = format!("{name}({arg})");
        let mut p = Parser::new(&src, store, trie, &core, s);
        p.parse_expression().unwrap()
    };

    // Oracle: interpret every case (no bcode yet → body walk).
    let mut rt = Runtime::new(&core, &mut store);
    for &(arg, expect) in cases {
        let call = parse_call(rt.store, &mut trie, arg);
        assert_eq!(rt.run(call).unwrap(), expect, "interpreter {name}({arg})");
    }

    // Compile the function, then rerun every case: the self-call becomes a
    // machine call and the locals live in the compiled stack frame.
    let _compiled = compile_fn(rt.store, &core.lower, &core, f).unwrap();
    for &(arg, expect) in cases {
        let call = parse_call(rt.store, &mut trie, arg);
        assert_eq!(rt.run(call).unwrap(), expect, "jit {name}({arg})");
    }
}

#[test]
fn recursion_with_a_local_is_per_activation_both_tiers() {
    // The activation-record payoff. `f` writes its local `x := n`, recurses
    // (each call re-initializing *its own* `x`), and returns `x` afterward. If
    // `x` shared one blob across activations, the inner calls would leave it 0
    // and `f(n)` would return 0; with per-call frames it is the caller's `n`.
    // SAFETY: a self-contained recursive definition and literal calls.
    unsafe {
        assert_recursion_both_tiers(
            &["f := fn (n := i32 ?) -> i32 ( x := n, if (n < 1) (0) else (f(n - 1), x) )"],
            "f",
            &[(0, 0), (1, 1), (3, 3), (5, 5)],
        );
    }
}

#[test]
fn recursion_takes_the_address_of_a_per_activation_local_both_tiers() {
    // `&x` must yield *this activation's* `x`, not a parse-time constant: `p`
    // points at the local, the call recurses, and `p@` reads back the caller's
    // value. A baked (shared) address would read the innermost frame → 0.
    // SAFETY: as above; `&x`/`p@` over a per-call local.
    unsafe {
        assert_recursion_both_tiers(
            &["g := fn (n := i32 ?) -> i32 ( x := n, p := &x, if (n < 1) (0) else (g(n - 1), p@) )"],
            "g",
            &[(0, 0), (1, 1), (4, 4)],
        );
    }
}

#[test]
fn recursion_with_a_record_local_is_per_activation_both_tiers() {
    // A record instance is a per-call local too: `pt := point(n, 0)`, recurse,
    // then read `pt.x`. Shared storage would clobber it to the innermost value.
    // SAFETY: `point` is defined first, then the recursive `h` uses it.
    unsafe {
        assert_recursion_both_tiers(
            &[
                "point := logos ( instance = ( x := i32 ?, y := i32 ? ) )",
                "h := fn (n := i32 ?) -> i32 ( pt := point(n, 0), if (n < 1) (0) else (h(n - 1), pt.x) )",
            ],
            "h",
            &[(0, 0), (2, 2), (5, 5)],
        );
    }
}

#[test]
fn recursion_with_a_local_accumulator_both_tiers() {
    // Coverage of the accumulator shape (`acc` written from the recursive
    // result, then read) on both tiers: a recursive factorial that carries its
    // product in a per-call local.
    // SAFETY: a self-contained recursive definition and literal calls.
    unsafe {
        assert_recursion_both_tiers(
            &["fact := fn (n := i32 ?) -> i32 ( acc := i32 1, if (n < 1) (acc) else (acc = n * fact(n - 1), acc) )"],
            "fact",
            &[(0, 1), (1, 1), (5, 120), (7, 5040)],
        );
    }
}

#[test]
fn recursion_with_a_typed_declaration_local_both_tiers() {
    // A typed declaration (`a := i32 ?`, no initializer) is a per-call local
    // too: it reads zero on entry — the undefined approximation, identical
    // on both tiers because the JIT zeroes the frame slot exactly as the
    // interpreter zeroes its activation buffer — and each activation then
    // writes its own copy, which the read-after-recursion checks.
    // SAFETY: a self-contained recursive definition and literal calls.
    unsafe {
        assert_recursion_both_tiers(
            &["f := fn (n := i32 ?) -> i32 ( a := i32 ?, a = a + n, if (n < 1) (a) else (f(n - 1), a) )"],
            "f",
            &[(0, 0), (1, 1), (5, 5)],
        );
    }
}

#[test]
fn a_nested_function_cannot_capture_an_outer_local() {
    // v1 has no closures: a nested fn that reads (or takes the address of) an
    // enclosing fn's local is a clean CapturedLocal parse error, not a wrong
    // read of some other frame at run time.
    assert_eq!(
        parse_err(
            "outer := fn (a := i32 ?) -> i32 ( x := a, inner := fn () -> i32 ( x ), inner() )"
        ),
        ParseError::CapturedLocal,
    );
    assert_eq!(
        parse_err(
            "outer := fn (a := i32 ?) -> i32 ( x := a, in2 := fn () -> i32 ( p := &x, p@ ), in2() )"
        ),
        ParseError::CapturedLocal,
    );
}

#[test]
fn a_nested_function_with_its_own_locals_runs() {
    // The capture guard must not over-reject: a nested fn using only its own
    // parameter and locals is fine — those are its own frame. `inner(a)`
    // passes the outer parameter by value (read in the outer body), not a
    // capture. outer(5) = inner(5) + 1 = (5 + 5) + 1 = 11.
    let (mut store, mut trie, core) = new_core();
    {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "outer := fn (a := i32 ?) -> i32 ( inner := fn (b := i32 ?) -> i32 ( y := b, y + b ), inner(a) + 1 )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap();
    }
    let call = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("outer(5)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call` applies the bound `outer` to a literal.
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 11, "outer(5)");
}

#[test]
fn a_nested_function_cannot_capture_an_outer_parameter() {
    // A parameter is per-call state exactly like a frame local, so a nested
    // fn reading (or taking the address of) an enclosing fn's parameter is
    // the same capture — caught at parse as CapturedLocal, not surfacing as
    // an unrelated no-storage error at run time.
    assert_eq!(
        parse_err("outer := fn (a := i32 ?) -> i32 ( inner := fn () -> i32 ( a ), inner() )"),
        ParseError::CapturedLocal,
    );
    assert_eq!(
        parse_err("outer := fn (a := i32 ?) -> i32 ( in2 := fn () -> i32 ( p := &a, p@ ), in2() )"),
        ParseError::CapturedLocal,
    );
}

/// Parse `defs` in order (declarations into the shared root scope), then
/// expect `src` to fail to parse with the returned error.
fn parse_err_after(defs: &[&str], src: &str) -> ParseError {
    let (mut store, mut trie, core) = new_core();
    for def in defs {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(def, &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap();
    }
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new(src, &mut store, &mut trie, &core, s);
    p.parse_expression().unwrap_err()
}

#[test]
fn assignment_rejects_a_cross_type_right_side() {
    // No implicit coercion (DESIGN ›Numeric literals are uncommitted until
    // context type them‹): a non-literal right side must already BE the
    // target's logos; crossing is explicit (`i64(b)`). Only a literal commits
    // to the target (the typed slot), which the suite covers elsewhere.
    let defs = &["a := i64 1", "b := i32 2"];
    assert_eq!(parse_err_after(defs, "a = b"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "b = a"), ParseError::TypeMismatch);
}

#[test]
fn assignment_rejects_pointer_type_mismatches() {
    // The pointer target's crossings: a pointer to the wrong pointee, a
    // plain value into a pointer (a wild address in the making), a pointer
    // into a plain value, and a store-through of the wrong logos.
    let defs = &["x := i32 7", "y := f64 2.5", "p := &x", "b := i32 2"];
    assert_eq!(parse_err_after(defs, "p = &y"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "p = b"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "b = p"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "p@ = y"), ParseError::TypeMismatch);
}

#[test]
fn assignment_accepts_a_matching_pointer_and_rewires() {
    // The sanctioned rewiring: `p = &y` with a matching pointee re-aims the
    // pointer, and `p@` reads the new target.
    let (mut store, mut trie, core) = new_core();
    let mut rt = Runtime::new(&core, &mut store);
    let mut result = 0;
    for line in ["x := i32 7", "y := i32 9", "p := &x", "p@", "p = &y", "p@"] {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(line, rt.store, &mut trie, &core, s);
        let node = p.parse_expression().unwrap();
        // SAFETY: `node` is the reduced dyad just parsed.
        result = unsafe { rt.run(node) }.unwrap();
    }
    assert_eq!(result, 9, "p@ after p = &y");
}

#[test]
fn compiled_function_calls_another_compiled_function() {
    // A compiled body that calls another already-compiled function, via a machine
    // `call_indirect` to the callee's baked address. Diffed against the oracle.
    let (mut store, mut trie, core) = new_core();

    let add = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn (x := i32 ?, y := i32 ?) -> i32 ( x + y )",
            &mut store,
            &mut trie,
            &core,
            s,
        );
        p.parse_expression().unwrap()
    };

    let outer = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        unsafe { s.declare(&mut trie, "add", test_record(core.record_, add)) }.unwrap();
        let mut p = Parser::new("fn () -> i32 ( add(40, 2) )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(outer, std::ptr::null_mut());

    // Compile `add` first so `outer`'s call has a machine address to bake.
    // SAFETY: `add` is the fn node just built and outlives every call.
    let _compiled_add = unsafe { compile_fn(&mut store, &core.lower, &core, add) }.unwrap();

    let mut rt = Runtime::new(&core, &mut store);
    // Oracle: interpret `outer` (its body calls the already-compiled `add`).
    // SAFETY: `call`/`outer`/`add` are valid nodes; `_compiled_add` is alive.
    let interp = unsafe { rt.run(call) }.unwrap();

    // Compile `outer`: `add(40, 2)` becomes a call_indirect to `add`'s address.
    // SAFETY: `outer` is the fn node just built; both compiled artifacts are alive.
    let _compiled_outer = unsafe { compile_fn(rt.store, &core.lower, &core, outer) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();

    assert_eq!(interp, 42);
    assert_eq!(jit, interp);
}

/// Parse a typed function `fn_src`, declare it `f`, parse a call `call_src`, and
/// diff the interpreter against the JIT, asserting both equal `expect`.
fn diff_typed_call(fn_src: &str, call_src: &str, expect: i64) {
    let (mut store, mut trie, core) = new_core();
    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(fn_src, &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let call = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        unsafe { s.declare(&mut trie, "f", test_record(core.record_, func)) }.unwrap();
        let mut p = Parser::new(call_src, &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func` are valid nodes just parsed.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `func` is the fn node just built; the artifact outlives the call.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, expect, "interpreter: {fn_src} / {call_src}");
    assert_eq!(jit, interp, "jit != interpreter: {fn_src} / {call_src}");
}

#[test]
fn integer_width_arithmetic_matches_between_tiers() {
    // i64 multiplication that overflows i32 (10^10) but fits i64 — proves the width.
    diff_typed_call(
        "fn (x := i64 ?, y := i64 ?) -> i64 ( x * y )",
        "f(100000, 100000)",
        10_000_000_000,
    );
    diff_typed_call(
        "fn (x := i64 ?, y := i64 ?) -> i64 ( x + y )",
        "f(1000000, 2000000)",
        3_000_000,
    );
    // u8 addition wraps at 256: 200 + 100 = 44.
    diff_typed_call("fn (x := u8 ?, y := u8 ?) -> u8 ( x + y )", "f(200, 100)", 44);
    // i16 subtraction, signed negative result.
    diff_typed_call("fn (x := i16 ?, y := i16 ?) -> i16 ( x - y )", "f(3, 10)", -7);
    // u32 sum above i32's range (3e9) stays positive (zero-extended), unlike i32.
    diff_typed_call(
        "fn (x := u32 ?, y := u32 ?) -> u32 ( x + y )",
        "f(1000000000, 2000000000)",
        3_000_000_000,
    );
}

#[test]
fn signed_vs_unsigned_comparison_matches_between_tiers() {
    // The same byte 0xFF compares differently as i8 (-1) and u8 (255). 255 has
    // no exact i8, so the i8 side takes it through the explicit wrapping cast.
    diff_typed_call("fn (x := i8 ?) -> i32 ( if (x < 1) (100) else (200) )", "f(i8(255))", 100);
    diff_typed_call("fn (x := u8 ?) -> i32 ( if (x < 1) (100) else (200) )", "f(255)", 200);
}

/// Diff a nullary `fn () -> ... ( body )` between the interpreter and the JIT, where
/// `body` reads an enclosing variable `a` of numeric type `nt` initialised to the
/// low `nt.bytes()` of `init` (its bit-container). Floats can't ride the i32-mold
/// argument path, so a float operand has to be a real stored variable, not a call
/// argument; the body only reads `a`, so no reset between runs is needed.
fn diff_var_fn(nt: NumType, init: i64, fn_src: &str, expect: i64) {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&init.to_ne_bytes()[..nt.bytes()]);
    let a = store.alloc_raw(core.numtypes[nt as usize], crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();
    let func = {
        let mut p = Parser::new(fn_src, &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `func`/`a` live in `store`, which outlives the call.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, expect, "interpreter: {fn_src}");
    assert_eq!(jit, interp, "jit != interpreter: {fn_src}");
}

#[test]
fn f64_arithmetic_matches_between_tiers() {
    // f64 add with a molded `1.5` beside a typed f64 variable (2.5): 2.5 + 1.5 = 4.0.
    // Result is the f64 bit-container, so both tiers must reinterpret bits the same
    // way across the ABI (interpreter read/mold vs JIT bitcast).
    diff_var_fn(
        NumType::F64,
        2.5f64.to_bits() as i64,
        "fn () -> f64 ( a + 1.5 )",
        4.0f64.to_bits() as i64,
    );
    diff_var_fn(
        NumType::F64,
        2.5f64.to_bits() as i64,
        "fn () -> f64 ( a - 0.5 )",
        2.0f64.to_bits() as i64,
    );
}

#[test]
fn f32_arithmetic_matches_between_tiers() {
    // f32 add: the f32 bits ride the low 32 of the container (zero-extended), a
    // different ABI path than f64, so it is worth its own diff.
    diff_var_fn(
        NumType::F32,
        i64::from(2.5f32.to_bits()),
        "fn () -> f32 ( a + 1.5 )",
        i64::from(4.0f32.to_bits()),
    );
}

#[test]
fn f64_comparison_matches_between_tiers() {
    // f64 `<` (an fcmp), true and false, gated through `if` so the result is the
    // i32 bool the comparison yields. a = 2.5.
    diff_var_fn(
        NumType::F64,
        2.5f64.to_bits() as i64,
        "fn () -> i32 ( if (a < 3.0) (100) else (200) )",
        100,
    );
    diff_var_fn(
        NumType::F64,
        2.5f64.to_bits() as i64,
        "fn () -> i32 ( if (a < 2.0) (100) else (200) )",
        200,
    );
}

/// Parse `src` at expression scope and return the error it fails with.
fn parse_err(src: &str) -> ParseError {
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new(src, &mut store, &mut trie, &core, s);
    p.parse_expression().unwrap_err()
}

#[test]
fn casts_between_integer_widths_match_between_tiers() {
    // Widen sign-extends (i8 -1 kept as i32 -1); narrow drops high bits
    // (i32 300 -> i8 44); a same-width reinterpret keeps the bits (u32 3e9 -> i32
    // is the negative reading); a same-logos cast is the operand unchanged. 255
    // has no exact i8, so reaching an i8 parameter takes an explicit `i8(255)`
    // (the wrapping cast), never a bare literal.
    diff_typed_call("fn (x := i8 ?) -> i32 ( i32(x) )", "f(i8(255))", -1);
    diff_typed_call("fn (x := i32 ?) -> i8 ( i8(x) )", "f(300)", 44);
    diff_typed_call("fn (x := i8 ?) -> u8 ( u8(x) )", "f(i8(255))", 255);
    diff_typed_call("fn (x := i32 ?) -> i32 ( i32(x) )", "f(42)", 42);
    // u32 3e9 reinterpreted as i32: 3_000_000_000 - 2^32.
    diff_var_fn(
        NumType::U32,
        3_000_000_000,
        "fn () -> i32 ( i32(a) )",
        3_000_000_000i64 - 4_294_967_296,
    );
    // u64 300 -> u8 wraps to 44.
    diff_var_fn(NumType::U64, 300, "fn () -> u8 ( u8(a) )", 44);
}

#[test]
fn casts_between_int_and_float_match_between_tiers() {
    // int -> float is exact for small values; float -> int truncates toward zero and
    // *saturates* out of range (matching Rust `as`, so both tiers agree).
    diff_typed_call("fn (x := i32 ?) -> f64 ( f64(x) )", "f(3)", 3.0f64.to_bits() as i64);
    diff_var_fn(NumType::F64, 3.7f64.to_bits() as i64, "fn () -> i32 ( i32(a) )", 3);
    diff_var_fn(NumType::F64, (-3.7f64).to_bits() as i64, "fn () -> i32 ( i32(a) )", -3);
    diff_var_fn(
        NumType::F64,
        1e20f64.to_bits() as i64,
        "fn () -> i32 ( i32(a) )",
        i64::from(i32::MAX),
    );
    diff_var_fn(
        NumType::F64,
        (-1e20f64).to_bits() as i64,
        "fn () -> i32 ( i32(a) )",
        i64::from(i32::MIN),
    );
    // f64 -> f32 demote.
    diff_var_fn(
        NumType::F64,
        1.5f64.to_bits() as i64,
        "fn () -> f32 ( f32(a) )",
        i64::from(1.5f32.to_bits()),
    );
}

#[test]
fn casts_fold_literal_operands() {
    // A literal operand converts at parse time with `as` semantics: an integer stays
    // exact, a decimal truncates toward zero, a float target takes the value.
    diff_typed_call("fn () -> i32 ( i32(3) )", "f()", 3);
    diff_typed_call("fn () -> i32 ( i32(3.5) )", "f()", 3);
    diff_typed_call("fn () -> u8 ( u8(300) )", "f()", 44);
    diff_typed_call("fn () -> f64 ( f64(2) )", "f()", 2.0f64.to_bits() as i64);
}

#[test]
fn malformed_casts_are_rejected() {
    assert_eq!(parse_err("i32()"), ParseError::BadCast); // no operand
    assert_eq!(parse_err("i32(1, 2)"), ParseError::BadCast); // too many operands
    assert_eq!(parse_err("i32(logos ())"), ParseError::BadCast); // non-numeric operand
}

#[test]
fn void_function_yields_unit_both_tiers() {
    // A `-> void` fn discards its body value and returns unit (0 bits) in both tiers.
    diff_typed_call("fn () -> void ( 42 )", "f()", 0);
}

#[test]
fn void_function_runs_its_body_for_effect() {
    // The void body still executes: `a = a + 1` bumps the enclosing variable, and
    // the fn returns unit. Diffed between tiers on both the return and the effect.
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&41i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", test_record(core.record_, a)) }.unwrap();
    let func = {
        let mut p =
            Parser::new("fn () -> void ( a = a + 1 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // Interpreted: yields unit 0, leaves a = 42.
    // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
    let interp = unsafe { rt.run(call) }.unwrap();
    let interp_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };
    // Reset a, compile (installs bcode), run again — jumps to the compiled body.
    unsafe { std::ptr::write_unaligned(a_val as *mut i32, 41) };
    // SAFETY: `func`/`a` live in `store`, which outlives the call.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    let jit_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };
    assert_eq!(interp, 0, "void yields unit (interpreted)");
    assert_eq!(jit, 0, "void yields unit (compiled)");
    assert_eq!(interp_a, 42, "body ran (interpreted)");
    assert_eq!(jit_a, 42, "body ran (compiled)");
}

#[test]
fn both_literal_arithmetic_stays_rational() {
    // `1 + 2` is not committed to i32 at parse time; it folds to a rational literal
    // (exact), committing only when context type it.
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let node = {
        let mut p = Parser::new("1 + 2", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    // SAFETY: `node` is the folded literal just parsed.
    unsafe { assert_eq!((*node).ty, core.rational, "1 + 2 stays a rational literal") };
}

#[test]
fn comptime_rational_arithmetic_folds_exactly_and_commits_on_context() {
    // Two comptime literals stay rational and fold exactly: `1000000 * 1000000` is
    // 10^12 (not an i32 overflow), committing to i64 through the cast; `2e9 + 2e9`
    // commits to the i64 return type; a decimal fold reduces the fraction exactly.
    diff_typed_call("fn () -> i64 ( i64(1000000 * 1000000) )", "f()", 1_000_000_000_000);
    diff_typed_call("fn () -> i64 ( 2000000000 + 2000000000 )", "f()", 4_000_000_000);
    diff_typed_call("fn () -> i64 ( 2000000000 * 2 )", "f()", 4_000_000_000);
    // The commit reaches through an explicit `return` too (not just a bare tail).
    diff_typed_call("fn () -> i64 ( return 2000000000 + 2000000000 )", "f()", 4_000_000_000);
    // 0.5 + 0.25 = 3/4 exactly, then to f64 bits.
    diff_typed_call("fn () -> f64 ( f64(0.5 + 0.25) )", "f()", 0.75f64.to_bits() as i64);
}

#[test]
fn comptime_rational_comparison_folds() {
    // Comparing two comptime literals folds to a `bool`, so it works even for values
    // with no i32 — an i32 compare could not commit `3000000000`.
    diff_typed_call("fn () -> i32 ( if (3000000000 < 4000000000) (1) else (0) )", "f()", 1);
    diff_typed_call("fn () -> i32 ( if (5 > 3) (1) else (0) )", "f()", 1);
}

#[test]
fn a_comptime_rational_that_overflows_i64_is_rejected() {
    // The seed's rationals are i64 fractions; an exact product past i64 has no
    // representation, a clean error rather than a wrong wrapped value.
    assert_eq!(parse_err("i64(10000000000 * 10000000000)"), ParseError::UncomputableLiteral);
}

#[test]
fn comptime_rational_commits_through_if_branches() {
    // An `if` in tail position is a value slot too: a large comptime rational in
    // either branch commits to the i64 return type (it would otherwise fail the i32
    // mold shim). This also exercises the width-general `if` lowering (i64 branches).
    let fn_src = "fn (c := i32 ?) -> i64 ( if (c < 1) (2000000000 + 2000000000) else (0) )";
    diff_typed_call(fn_src, "f(0)", 4_000_000_000); // then-branch taken
    diff_typed_call(fn_src, "f(5)", 0); // else-branch taken
                                        // The else-branch commits too.
    let fn_src = "fn (c := i32 ?) -> i64 ( if (c < 1) (0) else (2000000000 + 2000000000) )";
    diff_typed_call(fn_src, "f(5)", 4_000_000_000);
}

#[test]
fn different_concrete_types_are_a_mismatch() {
    // Cross-logos arithmetic needs an explicit cast; there is no implicit coercion.
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new(
        "fn (x := i32 ?, y := i64 ?) -> i64 ( x + y )",
        &mut store,
        &mut trie,
        &core,
        s,
    );
    assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::TypeMismatch));
}

#[test]
fn a_literal_that_does_not_fit_its_committed_type_is_rejected() {
    // `1.5` beside an i32 has no exact i32, so committing it fails at parse time.
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new("fn (x := i32 ?) -> i32 ( x + 1.5 )", &mut store, &mut trie, &core, s);
    assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::UncomputableLiteral));
}

#[test]
fn plus_over_non_numeric_operands_is_unresolved() {
    // `+` with a non-numeric operand (a record value) has no concrete machine op
    // to resolve to, so parsing reports UnsupportedOperands.
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let mut p = Parser::new("logos () + 1", &mut store, &mut trie, &core, scopes);
    assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::UnsupportedOperands));
}
