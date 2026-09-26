// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The identities' tests: whole programs parsed, run, compiled, and compared
//! across the two tiers. A child of `identities`, so every helper is in reach.

use super::*;
use crate::compile::{compile_fn, compile_nullary_i32};
use crate::parse::{Parser, ScopeStack, SlotKind, FN_BCODE, FN_BODY, FN_INPUT, FN_OUTPUT};
use crate::run::Runtime;

fn new_core() -> (Store, RegexTrie, Core) {
    let mut store = Store::new();
    let mut trie = RegexTrie::new();
    let core = Core::build(&mut store, &mut trie);
    (store, trie, core)
}

/// A binding dyad naming `identity`, for a name a test declares by hand; leaked
/// like every test-built dyad. A hand-minted variable must carry the storage
/// mark (`crate::dyad::global_place`), or the reading rule takes it for a literal.
fn test_binding(binding_ty: DyadPtr, identity: DyadPtr) -> DyadPtr {
    let fields =
        Box::into_raw(Box::new(Binding::new(identity, std::ptr::null_mut(), std::ptr::null_mut())));
    Box::into_raw(Box::new(crate::dyad::Dyad { ty: binding_ty, value: fields as *mut u8 }))
}

/// A hand-built binding of a variable the test writes.
fn mut_binding(store: &mut Store, core: &Core, identity: DyadPtr) -> DyadPtr {
    let binding = test_binding(core.binding_, identity);
    // SAFETY: `binding` was just built; its fields are a leaked `Binding`.
    unsafe { Binding::add_gate(store, core.array_, binding, core.mut_) };
    binding
}

#[test]
fn parses_a_equals_a_plus_one() {
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();

    let root = {
        let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    unsafe {
        assert_eq!((*root).ty, core.assign);
        let top = (*root).value as *const DyadPtr;
        assert_eq!(core.through(*top), a);
        let sum = *top.add(1);
        assert_eq!((*sum).ty, core.plus);
        let sops = (*sum).value as *const DyadPtr;
        assert_eq!(core.through(*sops), a);
        let one = *sops.add(1);
        assert_eq!((*one).ty, core.i32_);
        assert_eq!(std::ptr::read_unaligned((*one).value as *const i32), 1);
        assert_eq!(*sops.add(2), core.ops.arith_leaf(numtype::ArithOp::Add, NumType::I32));
    }
}

#[test]
fn runs_a_equals_a_plus_one() {
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();

    let root = {
        let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

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
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&41i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();

    let main = {
        let mut p =
            Parser::new("fn () -> i32 ( return a + 1 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(main, std::ptr::null_mut());

    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`main`/body are valid nodes in `store`.
    let result = unsafe { rt.run(call) }.unwrap();
    assert_eq!(result, 42);
    unsafe {
        assert_eq!(std::ptr::read_unaligned(a_val as *const i32), 41);
    }
}

#[test]
fn runs_a_returning_scope() {
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

    let bare = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("return 7", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    assert_eq!(unsafe { rt.run(bare) }.unwrap(), 7);

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
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let func = {
        let mut p =
            Parser::new("fn () -> i32 ( return 40 + 2 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    unsafe {
        assert_eq!((*func).ty, core.fn_type);
        let v = (*func).value as *const DyadPtr;
        let (input, output, body) = (*v.add(FN_INPUT), *v.add(FN_OUTPUT), *v.add(FN_BODY));
        assert_eq!((*input).ty, core.type_);
        assert!(array::items(meta::record_fields_of(input)).is_empty());
        assert_eq!(output, core.i32_);
        assert!(!body.is_null());
    }

    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func`/body are valid nodes in `store`.
    let result = unsafe { rt.run(call) }.unwrap();
    assert_eq!(result, 42);
}

#[test]
fn parses_a_fn_with_a_param_visible_in_the_body() {
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
        let x_field = array::items(meta::record_fields_of(input))[0];
        assert_eq!((*x_field).ty, core.i32_);
        let return_operand = *((*body).value as *const DyadPtr);
        assert_eq!(core.through(return_operand), x_field);
    }
}

#[test]
fn compiles_and_runs_a_fn_with_arguments() {
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
        unsafe { s.declare(&mut trie, "add", test_binding(core.binding_, add)) }.unwrap();
        let mut p = Parser::new("add(40, 2)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };

    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`add`/args are valid nodes just parsed.
    let interp = unsafe { rt.run(call) }.unwrap();

    // SAFETY: `add` is the fn node just built and outlives the call.
    let _compiled = unsafe { compile_fn(rt.store, &core.lower, &core, add) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();

    assert_eq!(interp, 42);
    assert_eq!(jit, interp);
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
        unsafe { s.declare(&mut trie, "add", test_binding(core.binding_, add)) }.unwrap();
        let mut p = Parser::new("add(40, 2)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };

    unsafe {
        assert_eq!((*call).ty, add);
    }

    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`add`/args are valid nodes just parsed.
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 42);
}

#[test]
fn calling_with_the_wrong_arity_errors() {
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
        unsafe { s.declare(&mut trie, "add", test_binding(core.binding_, add)) }.unwrap();
        let mut p = Parser::new("add(40)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };

    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`add` are valid nodes just parsed.
    assert_eq!(unsafe { rt.run(call) }, Err(crate::run::RunError::ArityMismatch));
}

#[test]
fn fn_body_return_is_optional() {
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
        let mut p =
            Parser::new("logos (x := i32 ?, y := i32 ?)", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

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
        let mut p = Parser::new("logos (t)", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    // A bare name is carried as the 8-byte container.
    let (scope, ft) = unsafe {
        let fields = array::items(meta::record_fields_of(node));
        assert_eq!(fields.len(), 1);
        assert_eq!(meta::record_size_of(node), 8);
        (meta::record_scope_of(node), fields[0])
    };
    unsafe {
        assert!((*ft).ty.is_null());
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

    let mut p = Parser::new("logos", &mut store, &mut trie, &core, scopes);
    let value = p.parse_expression().unwrap();
    assert_eq!(unsafe { core.through(value) }, core.type_);
}

#[test]
fn scopes_are_typed_scope() {
    let (mut store, mut trie, core) = new_core();

    unsafe {
        assert_eq!((*core.scope).ty, core.type_);
        assert_eq!((*core.root_scope).ty, core.scope);
    }

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let node = {
        let mut p = Parser::new("logos (x := i32 ?)", &mut store, &mut trie, &core, scopes);
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
    unsafe { scopes.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();

    let root = {
        let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `root` is the valid tree just parsed.
    let interp = unsafe { rt.run(root) }.unwrap();
    let interp_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };

    unsafe { std::ptr::write_unaligned(a_val as *mut i32, 0) };
    // SAFETY: `root`/`a` live in `store`, which outlives the call.
    let compiled = unsafe { compile_nullary_i32(&core.lower, &core, root) }.unwrap();
    let jit = unsafe { compiled.call() };
    let jit_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };

    assert_eq!(interp, 1);
    assert_eq!(jit, interp);
    assert_eq!(jit_a, interp_a);
    assert_eq!(jit_a, 1);
}

#[test]
fn assign_to_a_wide_variable_stores_at_full_width_both_tiers() {
    // A 4-byte store would drop the high word and leave 705_032_704.
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&0i64.to_ne_bytes());
    let a = store.alloc_raw(core.numtypes[NumType::I64 as usize], crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();

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
    // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
    let interp = unsafe { rt.run(call) }.unwrap();
    let interp_a = unsafe { std::ptr::read_unaligned(a_val as *const i64) };

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
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let func = {
        let mut p =
            Parser::new("fn () -> i32 ( return 40 + 2 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };

    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);

    let interp = unsafe { rt.run(call) }.unwrap();
    unsafe {
        let bcode = *((*func).value as *const DyadPtr).add(FN_BCODE);
        assert!(bcode.is_null());
    }

    // SAFETY: `func` is the fn node just built and outlives the call.
    let _compiled = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    unsafe {
        let bcode = *((*func).value as *const DyadPtr).add(FN_BCODE);
        assert!(!bcode.is_null());
    }

    let jit = unsafe { rt.run(call) }.unwrap();

    assert_eq!(interp, 42);
    assert_eq!(jit, interp);
}

#[test]
fn rational_decimal_parses_but_is_uncomputable_as_i32() {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let node = {
        let mut p = Parser::new("3.14", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    unsafe {
        assert_eq!((*node).ty, core.rational);
        assert_eq!(rational::mold(node), None);
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
    // A concrete operand is required: two comptime literals would fold to an exact rational, not wrap.
    let expected = i64::from(2_000_000_000i32.wrapping_add(2_000_000_000));
    diff_var_fn(NumType::I32, 2_000_000_000, "fn () -> i32 ( a + a )", expected);
}

#[test]
fn a_four_parameter_fn_compiles_and_agrees_with_the_interpreter() {
    diff_typed_call(
        "fn (a := i32 ?, b := i32 ?, c := i32 ?, d := i32 ?) -> i32 ( return a + b + c + d )",
        "f(1, 2, 3, 4)",
        10,
    );
}

#[test]
fn compiling_an_uninitialized_read_errors_instead_of_crashing() {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let x = store.alloc_raw(core.i32_, std::ptr::null_mut());
    unsafe { scopes.declare(&mut trie, "x", test_binding(core.binding_, x)) }.unwrap();

    let node = {
        let mut p = Parser::new("x", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `node` is the variable reference just parsed.
    assert_eq!(unsafe { rt.run(node) }, Err(crate::run::RunError::Uninitialized));
    // SAFETY: same node; the lowering guards the null storage.
    let compiled = unsafe { compile_nullary_i32(&core.lower, &core, node) };
    assert!(matches!(compiled, Err(crate::compile::CompileError::Uninitialized)));
}

#[test]
fn plus_is_abstract_and_resolves_to_a_concrete_op() {
    let (mut store, mut trie, core) = new_core();

    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&10i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();

    let func = {
        let mut p =
            Parser::new("fn () -> i32 ( a + 20 + 12 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
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
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let root = {
        let mut p = Parser::new(
            "double := fn (x := i32 ?) -> i32 ( x + x ),\npoint := logos (a := i32 ?),\ndouble(21)",
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

    // SAFETY: the sequence's first expression is the fn declaration.
    // The type body after the declaration ran what stood before it, so the declaration stands as its ran form.
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
    let (mut store, mut trie, core) = new_core();

    for (src, expect) in [("true", 1i64), ("false", 0i64)] {
        let node = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(src, &mut store, &mut trie, &core, s);
            p.parse_expression().unwrap()
        };
        // SAFETY: `node` is the literal just parsed, a use of `true`'s binding.
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

/// Diff the interpreter against the JIT on a nullary i32 fn body; both must equal `expect`.
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
    // `*` binds tighter than `+` (14); `-` is left-associative (5).
    diff_nullary_fn("fn () -> i32 ( 2 + 3 * 4 )", 14);
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
    // A concrete operand is required: two comptime literals would fold to an exact rational, not wrap.
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
    diff_nullary_fn("fn () -> i32 ( 0 - 1 < 0 )", 1);
    diff_nullary_fn("fn () -> i32 ( 2 + 3 == 10 - 5 )", 1);
}

#[test]
fn comparison_siblings_are_bool_conditions_for_if() {
    diff_nullary_fn("fn () -> i32 ( if (5 > 3) (100) else (200) )", 100);
    diff_nullary_fn("fn () -> i32 ( if (2 == 3) (100) else (200) )", 200);
    diff_nullary_fn("fn () -> i32 ( if (3 <= 3) (100) else (200) )", 100);
    diff_nullary_fn("fn () -> i32 ( if (3 >= 4) (100) else (200) )", 200);
    diff_nullary_fn("fn () -> i32 ( if (7 != 7) (100) else (200) )", 200);
}

#[test]
fn an_if_needs_no_brackets_in_both_tiers() {
    diff_nullary_fn("fn () -> i32 ( if 5 > 3 100 else 200 )", 100);
    diff_nullary_fn("fn () -> i32 ( if 2 == 3 100 else if 3 == 3 300 else 200 )", 300);
    diff_nullary_fn("fn () -> i32 ( if 2 == 3 (100) else if 3 > 4 (300) else 200 )", 200);
    let chain = "fn (n := i32 ?) -> i32 ( if n > 2 7 else if n > 0 (1) else 0 )";
    diff_typed_call(chain, "f(5)", 7);
    diff_typed_call(chain, "f(1)", 1);
    diff_typed_call(chain, "f(0)", 0);
    diff_typed_call("fn (n := i32 ?) -> i32 ( if n > 2 return 7, n + 1 )", "f(5)", 7);
    diff_typed_call("fn (n := i32 ?) -> i32 ( if n > 2 return 7, n + 1 )", "f(1)", 2);
    let assign = "fn (n := i32 ?) -> i32 ( mut k := i32 0, if n > 2 k = 1 else k = 2, k )";
    diff_typed_call(assign, "f(5)", 1);
    diff_typed_call(assign, "f(1)", 2);
    // A bare `else` binds to the nearest `if`.
    diff_typed_call("fn (n := i32 ?) -> i32 ( if n > 0 if n > 5 1 else 2 else 3 )", "f(3)", 2);
}

#[test]
fn an_if_condition_is_its_first_complete_expression() {
    assert_eq!(run_script("mut m := type ?, m = ?, if m != ? (1) else (2)"), 2);
    assert_eq!(run_script("mut m := type ?, m = i32, if m != ? 1 else 2"), 1);
    // Right of a comparison a `(` is the body, though the identity could take it.
    assert_eq!(run_script("x := i32 1, if x:type == scope (10) else (20)"), 20);
    assert_eq!(run_script("x := i32 1, if x:type == i32 (10) else (20)"), 10);
    assert_eq!(run_script("x := i32 1, p := &x, if p:type == @i32 (10) else (20)"), 10);
    // A value word is still the type's, and a condition not yet complete keeps its `(`.
    assert_eq!(run_script("x := i32 1, if x == i32 1 (10) else (20)"), 10);
    assert_eq!(run_script("f := fn (a := i32 ?) -> i32 (a), if f(1) == 1 (10) else (20)"), 10);
    assert_eq!(run_script("x := i32 1, if not (x == 2) (10) else (20)"), 10);
    // The identity left of a `(` decides: a fn value takes it as its call, a type right of
    // `+` as its conversion.
    let f = "f := fn (a := i32 ?) -> i32 (a), ";
    assert_eq!(run_script(&format!("{f}if 1 == f(1) (10) else (20)")), 10);
    assert_eq!(run_script(&format!("{f}if 1 == f(2) (10) else (20)")), 20);
    assert_eq!(run_script(&format!("{f}if 1 == (f(1)) (10) else (20)")), 10);
    assert_eq!(run_script(&format!("{f}if 1 == f(2) 10 else if 2 == f(2) 30 else 20")), 30);
    assert_eq!(run_script("x := i32 1, if x + i32 (1) == 2 (10) else (20)"), 10);
    // Anything yielding a bool is a condition; a complete one that does not is refused.
    let b = "b := fn (a := i32 ?) -> bool (a == 1), ";
    assert_eq!(run_script(&format!("{b}if b(1) (10) else (20)")), 10);
    assert_eq!(run_script(&format!("{b}if not b(2) 10 else 20")), 10);
    assert_eq!(
        script_parse_err(&format!("{f}if f(1) (10) else (20)")),
        ParseError::NonBoolCondition
    );
    // Bare bodies on their own lines, as identities/array.logos writes them.
    assert_eq!(run_script("x := i32 1,\nif not x:type == i32\n    error «not i32»,\n5"), 5);
    assert!(run_script_result("x := i32 1,\nif x:type == i32\n    error «is i32»,\n5").is_err());
    // A bare branch is a scope, as a bracket is.
    assert!(matches!(script_parse_err("x := i32 1, if x == 1 y := 2, y"), ParseError::Resolve(_)));
    assert_eq!(script_parse_err("if 1 == 1 else 2"), ParseError::Empty);
}

#[test]
fn a_call_in_a_bare_condition_matches_between_tiers() {
    let src = "f := fn (a := i32 ?) -> i32 (a),\n\
        g := fn (x := i32 ?) -> i32 ( if 1 == f(x) (10) else if f(x) == 2 30 else (20) ),\n\
        before := g(1) * 100 + g(2),\n\
        g.compile(),\n\
        before * 10000 + g(1) * 100 + g(2)";
    assert_eq!(run_script(src), 10_30_10_30);
}

/// `≈` written in Logos, ranked with `==`: its record says it reads a left operand, and
/// its node's function yields a bool.
fn logos_comparison(field: &str, rank: &str) -> String {
    format!(
        "≈ := type (\n\
            lhs := {field} ?,\n\
            rhs := {field} ?,\n\
            output_type := type ?,\n\
            run = ( this.lhs == this.rhs ),\n\
            parse_rank = {rank},\n\
            parse = (\n\
                this.lhs = tape[-1],\n\
                this.rhs = tape[1],\n\
                this.output_type = bool,\n\
                tape[0] = this,\n\
                tape.is_constructed[0] = true,\n\
                tape.remove(1),\n\
                tape.remove(-1)\n\
            )\n\
        ),\n"
    )
}

#[test]
fn a_comparison_written_in_logos_ends_a_bare_condition() {
    let eq = logos_comparison("i32", "==.parse_rank");
    assert_eq!(run_script(&format!("{eq}x := i32 3, if x ≈ 3 (10) else (20)")), 10);
    assert_eq!(run_script(&format!("{eq}x := i32 3, if x ≈ 4 10 else 20")), 20);
    let g = "g := fn (y := i32 ?) -> i32 ( mut n := i32 0, while not n ≈ y n = n + 1, n ),\n";
    assert_eq!(run_script(&format!("{eq}{g}before := g(4), g.compile(), before * 10 + g(4)")), 44);
    let types = logos_comparison("type", "==.parse_rank");
    assert_eq!(run_script(&format!("{types}x := i32 1, if x:type ≈ i32 (10) else (20)")), 10);
    assert_eq!(run_script(&format!("{types}x := i32 1, if x:type ≈ scope (10) else (20)")), 20);
}

/// Where a condition's bool cannot be known before its operator is built: a type standing
/// right of an infix ranked outside the comparisons takes the `(` after it, as right of `+`.
#[test]
fn a_type_right_of_an_infix_outside_the_comparisons_takes_its_bracket() {
    let plus_ranked = logos_comparison("type", "+.parse_rank");
    assert_eq!(
        script_parse_err(&format!("{plus_ranked}x := i32 1, if x:type ≈ i32 (10) else (20)")),
        ParseError::NonBoolCondition
    );
    assert_eq!(
        run_script(&format!("{plus_ranked}x := i32 1, if (x:type ≈ i32) (10) else (20)")),
        10
    );
}

#[test]
fn a_while_body_needs_no_brackets() {
    assert_eq!(run_script("mut i := i32 0, while i < 5 i = i + 1, i"), 5);
    assert_eq!(run_script("mut i := i32 0, while i < 5 (i = i + 1), i"), 5);
    assert_eq!(run_script("mut i := i32 0, while (i < 5) (i = i + 1), i"), 5);
    let f = "f := fn (a := i32 ?) -> i32 (a), ";
    assert_eq!(run_script(&format!("{f}mut i := i32 0, while f(i) < 5 i = i + 1, i")), 5);
    assert_eq!(run_script(&format!("{f}mut i := i32 0, while 5 > f(i) (i = i + 2), i")), 6);
    // Nested, and a bare `while` as a bare `if` branch ending at the `else`.
    assert_eq!(
        run_script(
            "mut i := i32 0, mut n := i32 0,\n\
             while i < 3 (i = i + 1, mut j := i32 0, while j < 2 j = j + 1, n = n + j), n"
        ),
        6
    );
    assert_eq!(run_script("mut i := i32 0, if i == 0 while i < 3 i = i + 1 else i = 9, i"), 3);
    assert_eq!(script_parse_err("while 1 2"), ParseError::NonBoolCondition);
    diff_nullary_fn("fn () -> i32 ( mut i := i32 0, while i < 5 i = i + 1, i )", 5);
    diff_nullary_fn(
        "fn () -> i32 ( mut a := i32 0, mut i := i32 0, while i < 5 ( a = a + i, i = i + 1 ), a )",
        10,
    );
}

#[test]
fn comparison_siblings_resolve_to_their_concrete_ops() {
    let (mut store, mut trie, core) = new_core();

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
    diff_nullary_fn("fn () -> i32 ( 1 < 2 and 3 < 4 )", 1);
    diff_nullary_fn("fn () -> i32 ( 1 < 2 and 3 > 4 )", 0);
    diff_nullary_fn("fn () -> i32 ( 1 > 2 or 3 < 4 )", 1);
    diff_nullary_fn("fn () -> i32 ( not (1 < 2) )", 0);
    // Comparisons bind tighter than `and`, which binds tighter than `or`.
    diff_nullary_fn("fn () -> i32 ( 1 < 2 and 3 > 4 or 5 < 6 )", 1);
    diff_nullary_fn("fn () -> i32 ( not (1 < 2) or not (3 < 4) )", 0);
    diff_nullary_fn("fn () -> i32 ( if (1 < 2 and 3 < 4) (100) else (200) )", 100);
}

#[test]
fn logical_operators_short_circuit_on_the_interpreter() {
    // Observed via a right operand that would error but is skipped.
    let (mut store, mut trie, core) = new_core();
    {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let y = store.alloc_raw(core.i32_, std::ptr::null_mut());
        unsafe { s.declare(&mut trie, "y", test_binding(core.binding_, y)) }.unwrap();
    }
    let mut rt = Runtime::new(&core, &mut store);

    let bad = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("y < 1", rt.store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    assert_eq!(unsafe { rt.run(bad) }, Err(crate::run::RunError::Uninitialized));

    let and_sc = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("false and y < 1", rt.store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    assert_eq!(unsafe { rt.run(and_sc) }.unwrap(), 0);

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
    diff_nullary_fn("fn () -> i32 ( if (true) (100) else (200) )", 100);
    diff_nullary_fn("fn () -> i32 ( if (false) (100) else (200) )", 200);
    diff_nullary_fn("fn () -> i32 ( if (0 < 1) (100) else (200) )", 100);
    diff_nullary_fn("fn () -> i32 ( if (1 < 0) (100) else (200) )", 200);
}

#[test]
fn if_over_a_parameter_matches_between_tiers() {
    for (arg, expect) in [(0i64, 100i64), (5, 200)] {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (mut n := i32 ?) -> i32 ( if (n < 1) (100) else (200) )",
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
            unsafe { s.declare(&mut trie, "f", test_binding(core.binding_, func)) }.unwrap();
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
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);

    let mut p =
        Parser::new("fn () -> i32 ( if (1) (100) else (200) )", &mut store, &mut trie, &core, s);
    assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::NonBoolCondition));
}

#[test]
fn nested_if_matches_between_tiers() {
    diff_nullary_fn("fn () -> i32 ( if (true) ( if (false) (1) else (2) ) else (3) )", 2);
}

#[test]
fn else_less_if_is_a_unit_statement_both_tiers() {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&41i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();
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
    // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 0, "unit (interpreted)");
    assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 42);
    unsafe { std::ptr::write_unaligned(a_val as *mut i32, 100) };
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 0);
    assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 100);
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
    diff_typed_call("fn (c := i32 ?) -> void ( if (c < 1) (42) )", "f(0)", 0);
    diff_typed_call("fn (c := i32 ?) -> void ( if (c < 1) (42) )", "f(5)", 0);
}

#[test]
fn an_else_less_if_is_rejected_in_value_positions() {
    assert_eq!(parse_err("fn () -> i32 ( if (1 < 2) (1) )"), ParseError::MissingElse);
    assert_eq!(parse_err("( if (1 < 2) (1) ) + 1"), ParseError::UnsupportedOperands);
}

#[test]
fn the_else_binds_to_the_outer_if_across_a_bracketed_branch() {
    // Were the `else` the inner if's, nothing would run and `a` would stay 5.
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&5i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();
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
    // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 0);
    assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 7);
    unsafe { std::ptr::write_unaligned(a_val as *mut i32, 5) };
    // SAFETY: `func`/`a` live in `store`, which outlives the call.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, func) }.unwrap();
    assert_eq!(unsafe { rt.run(call) }.unwrap(), 0);
    assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 7);
}

#[test]
fn assignment_commits_a_literal_to_the_targets_type() {
    diff_var_fn(NumType::I64, 0, "fn () -> i64 ( a = 5000000000, a )", 5_000_000_000);
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { s.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();
    let mut p = Parser::new("a = 3.5", &mut store, &mut trie, &core, s);
    assert_eq!(p.parse_expression(), Err(ParseError::UncomputableLiteral));
}

#[test]
fn compiled_calls_between_compiled_functions_are_width_general() {
    // 2e9 * 3 crosses i32, so an i32-assuming boundary would truncate.
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
        unsafe { s.declare(&mut trie, "mul", test_binding(core.binding_, mul)) }.unwrap();
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
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&2.5f64.to_bits().to_ne_bytes());
    let a = store.alloc_raw(core.numtypes[NumType::F64 as usize], crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();

    let g = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p =
            Parser::new("fn (x := f64 ?) -> f64 ( x + 0.5 )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    unsafe { scopes.declare(&mut trie, "g", test_binding(core.binding_, g)) }.unwrap();
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
    // SAFETY: `s_fn` is the declare node just parsed.
    let s_fn = unsafe { declare::declared_of(s_fn) };

    let call = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("s(3)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call` applies the bound `s` to a literal.
    let interp = unsafe { rt.run(call) }.unwrap();
    // SAFETY: `s_fn` is the fn node just built and outlives every call.
    let _c = unsafe { compile_fn(rt.store, &core.lower, &core, s_fn) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();
    assert_eq!(interp, 4_000_000_000, "interpreter");
    assert_eq!(jit, interp, "compiled recursion != interpreter");
}

#[test]
fn compiled_call_with_wrong_arity_refuses_to_compile() {
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
        unsafe { s.declare(&mut trie, "add", test_binding(core.binding_, add)) }.unwrap();
        let mut p = Parser::new("fn () -> i32 ( add(40) )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    // SAFETY: `outer` is the fn node just built.
    let result = unsafe { compile_fn(&mut store, &core.lower, &core, outer) };
    assert!(matches!(result, Err(crate::compile::CompileError::ArityMismatch)));
}

#[test]
fn call_arguments_commit_to_parameter_types() {
    diff_typed_call("fn (x := i64 ?) -> i64 ( x )", "f(3000000000)", 3_000_000_000);
    diff_typed_call("fn (x := f64 ?) -> f64 ( x + 0.5 )", "f(2.5)", 3.0f64.to_bits() as i64);
}

#[test]
fn an_argument_that_does_not_fit_its_parameter_is_rejected() {
    let (mut store, mut trie, core) = new_core();
    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("fn (x := i32 ?) -> i32 ( x )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    unsafe { s.declare(&mut trie, "f", test_binding(core.binding_, func)) }.unwrap();
    let mut p = Parser::new("f(2.5)", &mut store, &mut trie, &core, s);
    assert_eq!(p.parse_expression(), Err(ParseError::UncomputableLiteral));
}

#[test]
fn recursive_i64_factorial_matches_between_tiers() {
    // The self-call must read i64 from the published signature; an unbound placeholder would default to i32.
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
    diff_nullary_fn("fn () -> i32 ( # the answer\n a := 40, # forty\n a + 2 )", 42);
}

#[test]
fn while_loop_sums_both_tiers() {
    diff_nullary_fn(
        "fn () -> i32 ( mut a := i32 0, mut i := i32 0, while (i < 5) ( a = a + i, i = i + 1 ), a )",
        10,
    );
}

#[test]
fn while_false_never_runs_its_body() {
    diff_nullary_fn("fn () -> i32 ( mut a := i32 7, a = 7, while (a < 0) (a = 0), a )", 7);
}

#[test]
fn for_loop_sums_a_range_both_tiers() {
    diff_nullary_fn("fn () -> i32 ( mut s := i32 0, for i in 0..10 ( s = s + i ), s )", 45);
    diff_nullary_fn("fn () -> i32 ( mut s := i32 0, for i in 0..10..2 ( s = s + i ), s )", 20);
    diff_nullary_fn("fn () -> i32 ( mut s := i32 7, for i in 5..5 ( s = 0 ), s )", 7);
}

#[test]
fn for_loop_endpoints_resolve_a_common_type() {
    diff_typed_call(
        "fn (n := i64 ?) -> i64 ( mut s := i64 0, s = 0, for i in n..(n + 3) ( s = s + i ), s )",
        "f(5000000000)",
        15_000_000_003,
    );
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new(
        "fn (mut a := i32 ?, b := i64 ?) -> void ( for i in a..b ( a = 0 ) )",
        &mut store,
        &mut trie,
        &core,
        s,
    );
    assert_eq!(p.parse_expression(), Err(ParseError::TypeMismatch));
}

#[test]
fn for_loop_with_a_runtime_non_positive_step_runs_zero_iterations() {
    diff_typed_call(
        "fn (d := i32 ?) -> i32 ( mut s := i32 3, s = 3, for i in 0..10..d ( s = 0 ), s )",
        "f(0)",
        3,
    );
}

#[test]
fn for_loop_whose_step_overshoots_the_width_ends_both_tiers() {
    // A wrapped counter would satisfy `i < end` again; the step that does not fit ends the loop after one iteration.
    diff_typed_call(
        "fn (d := i32 ?) -> i32 ( mut s := i32 0, for i in 2147483640..2147483647..d ( s = s + 1 ), s )",
        "f(10)",
        1,
    );
}

#[test]
fn for_loop_without_an_index_both_tiers() {
    diff_nullary_fn("fn () -> i32 ( mut s := i32 0, for 0..10 ( s = s + 1 ), s )", 10);
    diff_nullary_fn("fn () -> i32 ( mut s := i32 0, for 0..10..2 ( s = s + 1 ), s )", 5);
    diff_typed_call(
        "fn (n := i32 ?) -> i32 ( mut s := i32 0, s = 0, for 0..n ( s = s + 2 ), s )",
        "f(4)",
        8,
    );
    diff_typed_call(
        "fn (n := i32 ?) -> i32 ( mut s := i32 0, s = 0, for n..(n + 3) ( s = s + 1 ), s )",
        "f(4)",
        3,
    );
}

#[test]
fn for_loop_shapes_are_checked() {
    assert_eq!(parse_err("for i in 0..10..0 ( 1 )"), ParseError::BadStep);
    assert_eq!(parse_err("for i in 10..0..-1 ( 1 )"), ParseError::BadStep);
    assert_eq!(parse_err("fn () -> i32 ( for i in 0..3 ( 1 ) )"), ParseError::StatementAsValue);
    assert_eq!(parse_err("for i in 0..3 ( return 1 )"), ParseError::EarlyReturn);
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
fn a_return_inside_a_loop_leaves_the_function() {
    assert_eq!(parse_err("while (1 < 2) (return 1)"), ParseError::EarlyReturn);
    diff_typed_call(
        "fn (n := i32 ?) -> i32 ( mut k := i32 0, while (k < 100) ( k = k + 1, if (k == n) (return k * 2) ), 0 )",
        "f(7)",
        14,
    );
    diff_typed_call(
        "fn (n := i32 ?) -> i32 ( for i in 0..n ( if (i == 3) (return i * 10) ), 0 - 1 )",
        "f(9)",
        30,
    );
    diff_typed_call(
        "fn (n := i32 ?) -> i32 ( for i in 0..n ( if (i == 3) (return i * 10) ), 0 - 1 )",
        "f(2)",
        -1,
    );
}

#[test]
fn juxtaposition_types_a_literal() {
    diff_typed_call("fn () -> i64 ( x := i64 5000000000, x )", "f()", 5_000_000_000);
    assert_eq!(parse_err("i32 3.5"), ParseError::UncomputableLiteral);
    // `f(i32 3)` is one argument, the typed value; `f(i32, 3)` would be two.
    diff_typed_call("fn (x := i32 ?) -> i32 ( x + 1 )", "f(i32 3)", 4);
}

#[test]
fn division_and_remainder_match_between_tiers() {
    diff_typed_call("fn (x := i32 ?, y := i32 ?) -> i32 ( x / y )", "f(10, 3)", 3);
    diff_typed_call("fn (x := i32 ?, y := i32 ?) -> i32 ( x / y )", "f(-10, 3)", -3);
    diff_typed_call("fn (x := i32 ?, y := i32 ?) -> i32 ( x % y )", "f(10, 3)", 1);
    diff_typed_call("fn (x := i32 ?, y := i32 ?) -> i32 ( x % y )", "f(-10, 3)", -1);
    diff_typed_call("fn (x := i32 ?) -> i32 ( x - 4 / 2 )", "f(10)", 8);
}

#[test]
fn division_by_zero_saturates_to_max_both_tiers() {
    // Signed MIN/-1 saturates to MAX too; MIN % -1 is the well-defined 0.
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
    diff_typed_call("fn () -> f64 ( f64(1 / 3 * 3) )", "f()", 1.0f64.to_bits() as i64);
    diff_typed_call("fn () -> i32 ( i32(10 / 3) )", "f()", 3);
    diff_typed_call("fn () -> i32 ( 10 % 3 )", "f()", 1);
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
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `node` is the string literal just parsed.
    assert_eq!(unsafe { rt.run(node) }, Err(crate::run::RunError::NoWholeRead));
    assert_eq!(parse_err("«a» + 1"), ParseError::UnsupportedOperands);
}

#[test]
fn comments_are_reflectable_nodes_invisible_to_value_flow() {
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
        unsafe { core.through(p.parse_expression().unwrap()) }
    };
    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn () -> i32 ( mut x := i32 41, x = 41, incr(&x), x )",
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
    // The explicit `p = &x` makes the body idempotent: the interpreted run leaves p on y.
    diff_nullary_fn(
        "fn () -> i32 ( mut x := i32 10, mut y := i32 20, x = 10, y = 20, mut p := &x, p = &x, mut s := i32 0, s = p@, p = &y, s + p@ )",
        30,
    );
}

#[test]
fn pointer_chains_and_field_pointers_work_both_tiers() {
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
    let (mut store, mut trie, core) = new_core();
    {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("holder := logos (r := @i32 ?)", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap();
    }
    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn () -> i32 ( mut x := i32 7, x = 7, h := holder(&x), h.r@ + 1 )",
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
    assert_eq!(parse_err("( mut x := i32 1, x@ )"), ParseError::UnsupportedOperands);
    assert_eq!(
        parse_err("( mut x := i32 1, mut p := &x, p * 2 )"),
        ParseError::UnsupportedOperands
    );
    assert_eq!(parse_err("( mut x := i32 1, mut p := &x, p = 5 )"), ParseError::TypeMismatch);
    assert_eq!(parse_err("( y := 5, &y )"), ParseError::BadAddressOf);
    assert_eq!(parse_err("fn () -> i32 ( mut x := i32 1, x = 1, &x )"), ParseError::TypeMismatch);
}

#[test]
fn a_compiled_store_into_a_node_box_takes_its_width_from_the_leaf() {
    // The compiler used to re-derive the width from the target's logos, which a node box has none of, and panicked.
    for tail in ["", "f.compile(), "] {
        assert_eq!(
            run_script(&format!(
                "mut a := type ?, a = f64, f := fn () -> i32 ( a = i32, 5 ),\n{tail}f(), a == i32"
            )),
            1,
            "type box, {tail:?}"
        );
        assert_eq!(
            run_script(&format!(
                "mut a := dyad ?, a = i32, f := fn () -> i32 ( a = f64, 5 ),\n{tail}f(), a == f64"
            )),
            1,
            "dyad box, {tail:?}"
        );
    }
}

#[test]
fn a_bool_literal_agrees_across_tiers() {
    for tail in ["", "f.compile(), "] {
        assert_eq!(run_script(&format!("f := fn () -> bool ( true ),\n{tail}f()")), 1);
        assert_eq!(run_script(&format!("f := fn () -> bool ( false ),\n{tail}f()")), 0);
    }
}

#[test]
fn a_type_read_agrees_across_tiers() {
    assert_eq!(
        run_script(
            "x := i32 5, f := fn () -> bool ( x:type == i32 ), a := f(), f.compile(), a == f()"
        ),
        1
    );
}

#[test]
fn a_bare_parameter_compares_by_address() {
    assert_eq!(run_script("f := fn (a) -> bool ( a == i32 ), f(i32)"), 1);
    assert_eq!(run_script("f := fn (a) -> bool ( a == i32 ), f(f64)"), 0);
    assert_eq!(run_script("f := fn (a) -> bool ( a != i32 ), f(f64)"), 1);
}

#[test]
fn a_type_is_a_value_a_place_can_hold() {
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
    assert_eq!(
        run_script(
            "f := fn (t := type ?) -> bool ( t == f64 ),\ng := fn (u := type ?) -> bool ( f(u) ),\ng(f64)"
        ),
        1
    );
    assert_eq!(
        parse_err_after(&["f := fn (t := type ?) -> i32 ( 1 )"], "f(5)"),
        ParseError::TypeMismatch
    );

    assert_eq!(
        parse_err("fn (t := type ?) -> i32 ( x := t ?, 1 )"),
        ParseError::TypeKnownOnlyAtRun
    );
    assert_eq!(
        parse_err("fn (t := type ?) -> f64 ( t.parse_rank )"),
        ParseError::TypeKnownOnlyAtRun
    );

    assert_eq!(run_script("i32 == i32"), 1);
    assert_eq!(
        run_script(
            "f := fn (b := i32 ?) -> type ( if (b < 1) (i32) else (f64) ),\nt := f(0), x := t 5, x"
        ),
        5
    );
    // Declaring from a filled type box needs the top level's pass; that case is `a_type_box_is_an_ordinary_variable` in tests/cli.rs.
}

#[test]
fn a_type_returning_body_must_hand_back_a_type() {
    // A number handed back as a node address used to take the process down.
    assert_eq!(parse_err("fn () -> type ( 5 )"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(&["x := i32 5"], "fn () -> type ( x )"), ParseError::TypeMismatch);
    assert_eq!(parse_err("fn () -> type ( return 5 )"), ParseError::TypeMismatch);
    assert_eq!(
        parse_err("fn (b := i32 ?) -> type ( if (b < 1) (i32) else (5) )"),
        ParseError::TypeMismatch
    );

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
    // On the seed's own stack size: a test thread's 2 MiB would abort the runner before the guard fires.
    let work = std::thread::Builder::new()
        .stack_size(crate::WORK_STACK_BYTES)
        .spawn(|| {
            let deep = "(".repeat(crate::parse::MAX_BRACKET_DEPTH + 1);
            assert_eq!(parse_err(&format!("{deep}1")), ParseError::TooDeep);
            assert_eq!(run_script("(((((1)))))"), 1);

            let runaway = "f := fn (n := i32 ?) -> i32 ( f(n + 1) ), f(1)";
            assert_eq!(run_script_result(runaway), Err(crate::run::RunError::CallDepth));
            let ending = "f := fn (n := i32 ?) -> i32 ( if (n < 1) (0) else (f(n - 1)) ), f(9000)";
            assert_eq!(run_script(ending), 0);
            let runaway_compiled = "f := fn (n := i32 ?) -> i32 ( f(n + 1) ), f.compile(), f(1)";
            assert_eq!(run_script_result(runaway_compiled), Err(crate::run::RunError::CallDepth));
            let ending_compiled =
                "f := fn (n := i32 ?) -> i32 ( if (n < 1) (0) else (f(n - 1)) ), f.compile(), f(9000)";
            assert_eq!(run_script(ending_compiled), 0);
        })
        .expect("the work thread must start");
    work.join().expect("depth guards hold");
}

#[test]
fn the_unknown_is_one_value_a_type_or_dyad_place_holds() {
    assert_eq!(run_script("mut a := type ?, a = ?, a == ?"), 1);
    assert_eq!(run_script("mut a := type ?, a = ?, a != ?"), 0);
    assert_eq!(run_script("mut a := type ?, a = i32, a == ?"), 0);
    assert_eq!(run_script("mut a := type ?, a = i32, a != ?"), 1);
    assert_eq!(run_script("mut d := dyad ?, d = ?, d == ?"), 1);
    assert_eq!(run_script("x := ?, x == ?"), 1);
    assert_eq!(run_script("f := fn (t := type ?) -> bool ( t != ? ),\nf(i32)"), 1);
    assert_eq!(run_script("f := fn (t := type ?) -> bool ( t != ? ),\nf(?)"), 0);
    assert_eq!(run_script("mut m := type ?, m = ?, if (m != ?) (1) else (2)"), 2);
    // A number place refuses it: the plain mismatch.
    assert_eq!(script_parse_err("mut x := i32 5, x = ?"), ParseError::TypeMismatch);
    assert_eq!(script_parse_err("x := i32 5, x == ?"), ParseError::UnsupportedOperands);
}

#[test]
fn a_valueless_place_is_read_only_after_a_sibling_write() {
    let unwritten = |name: &str| ParseError::Unwritten(Box::new(name.into()));
    assert_eq!(script_parse_err("x := i32 ?, x"), unwritten("x"));
    assert_eq!(script_parse_err("mut x := i32 ?, x = x + 1, x"), unwritten("x"));
    assert_eq!(run_script("mut x := i32 ?, x = 4, x + 1"), 5);
    assert_eq!(run_script("mut x := i32 ?, x:type == i32"), 1);
    // A write nested in a group, an `if`, a loop or a `fn` body does not fill.
    assert_eq!(script_parse_err("mut x := i32 ?, (x = 4), x"), unwritten("x"));
    assert_eq!(script_parse_err("mut x := i32 ?, if (true) (x = 4), x"), unwritten("x"));
    assert_eq!(script_parse_err("mut x := i32 ?, for 0..2 ( x = 1 ), x"), unwritten("x"));
    assert_eq!(
        script_parse_err("mut x := i32 ?, f := fn () -> i32 ( x = 4, 1 ), f(), x"),
        unwritten("x")
    );
    // Text position decides: a body read before the write is refused, one after passes.
    assert_eq!(
        script_parse_err("mut x := i32 ?, g := fn () -> i32 ( x ), x = 1, g()"),
        unwritten("x")
    );
    assert_eq!(run_script("mut x := i32 ?, x = 1, g := fn () -> i32 ( x ), g()"), 1);
}

#[test]
fn a_read_or_write_through_an_unwritten_pointer_is_refused_at_parse() {
    let unwritten = |name: &str| ParseError::Unwritten(Box::new(name.into()));
    assert_eq!(script_parse_err("p := @i32 ?, p@"), unwritten("p"));
    assert_eq!(script_parse_err("p := @i32 ?, p@ = 5, 1"), unwritten("p"));
    assert_eq!(script_parse_err("f := fn () -> i32 ( p := @i32 ?, p@ ), f()"), unwritten("p"));
    assert_eq!(script_parse_err("f := fn () -> f64 ( p := @f64 ?, p@ ), f()"), unwritten("p"));

    assert_eq!(run_script("c := i32 7, q := &c, q@"), 7);
    assert_eq!(
        run_script("f := fn () -> i32 ( c := i32 1, q := &c, q@ = 9, c ), f.compile(), f()"),
        9
    );
}

#[test]
fn address_of_a_parameter_works_both_tiers() {
    diff_typed_call("fn (a := i32 ?) -> i32 ( q := &a, q@ )", "f(7)", 7);
    diff_typed_call("fn (a := i32 ?) -> i32 ( q := &a, q@ = 5, a )", "f(7)", 5);
}

#[test]
fn a_pointer_steps_by_whole_cells_both_tiers() {
    // Cell k of the span `alloc n of T v` made is `(p + k)@`, for any cell width.
    assert_eq!(
        run_script("a := alloc 3 of i32 7, b := a + 1, b@ = 9, (a + 2)@ = 11, a@ + (a + 1)@ * 10 + (b + 1)@ * 100 + (b - 1)@ * 1000"),
        8197
    );
    assert_eq!(
        run_script("a := alloc 4 of i64 3, k := i32 3, (a + k)@ = 40, i := u8 1, (a + i)@ = 5, (a + 3)@ + (a + 1)@ + (a + 2)@"),
        48
    );
    assert_eq!(
        run_script("holder := logos (r := @i32 ?), a := alloc 3 of i32 4, h := holder(a), (h.r + 2)@ = 8, (h.r + 2)@ + a@"),
        12
    );
    let walk = "f := fn (p := @i32 ?, n := i32 ?) -> i32 ( for i in 0..n ( (p + i)@ = i * 10 ), mut s := i32 0, for i in 0..n ( s = s + (p + n - 1 - i)@ ), s )";
    assert_eq!(run_script(&format!("{walk}, a := alloc 4 of i32 0, f(a, 4)")), 60);
    assert_eq!(run_script(&format!("{walk}, f.compile(), a := alloc 4 of i32 0, f(a, 4)")), 60);
}

#[test]
fn a_pointer_steps_only_by_an_integer_on_its_right() {
    let defs = &["p := alloc 1 of i32 0", "q := alloc 1 of i32 0"];
    assert_eq!(parse_err_after(defs, "p * 2"), ParseError::UnsupportedOperands);
    assert_eq!(parse_err_after(defs, "p + q"), ParseError::UnsupportedOperands);
    assert_eq!(parse_err_after(defs, "1 + p"), ParseError::UnsupportedOperands);
    assert_eq!(parse_err_after(defs, "p + f64 1"), ParseError::UnsupportedOperands);
    assert_eq!(parse_err_after(defs, "p + 1.5"), ParseError::UncomputableLiteral);
}

#[test]
fn parameter_reassignment_works_both_tiers() {
    diff_typed_call("fn (mut a := i32 ?) -> i32 ( a = a + 1, a )", "f(41)", 42);
}

/// Parse `src` as a whole script and run it with the compiler attached, so `f.compile()` works.
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

fn script_parse_err(src: &str) -> ParseError {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let mut p = Parser::new(src, &mut store, &mut trie, &core, scopes);
    p.parse_sequence().unwrap_err()
}

fn run_script(src: &str) -> i64 {
    run_script_result(src).unwrap()
}

#[test]
fn interpreted_recursion_stacks_frames() {
    // 1..=500 sums right only if no call's frame aliases another's.
    let src = "f := fn (n := i64 ?) -> i64 ( mut m := i64 0, m = n, if (n == 0) (0) else (m + f(n - 1)) ),\nf(500)";
    assert_eq!(run_script(src), 125_250);
}

#[test]
fn nested_calls_in_argument_position_release_frames_lifo() {
    // The callee's frame is claimed before its arguments evaluate; the inner calls must not clobber it.
    let src = "g := fn (x := i64 ?) -> i64 ( x + x ),\nf := fn (a := i64 ?, b := i64 ?) -> i64 ( a + b ),\nf(g(3), g(4) + g(0))";
    assert_eq!(run_script(src), 14);
}

#[test]
fn a_bare_parameter_carries_the_container() {
    assert_eq!(run_script("f := fn (a) -> i64 ( a ),\nf(42)"), 42);
}

#[test]
fn compile_member_installs_code_the_next_call_jumps_to() {
    assert_eq!(
        run_script("double := fn (x := i64 ?) -> i64 ( x + x ),\ndouble.compile(),\ndouble(21)"),
        42
    );
    assert_eq!(
        run_script(
            "fact := fn (n := i64 ?) -> i64 ( if (n < 2) (1) else (n * fact(n - 1)) ),\nfact.compile(),\nfact(20)"
        ),
        2_432_902_008_176_640_000
    );
}

#[test]
fn compile_member_before_and_after_agree() {
    assert_eq!(
        run_script(
            "double := fn (x := i64 ?) -> i64 ( x + x ),\na := double(21),\ndouble.compile(),\na + double(21)"
        ),
        84
    );
}

/// A power operator defined in Logos, spelled with a word so the script needs no fresh symbol.
const POW_TYPE: &str = "pw := type (\n\
     a := ?, b := i32 ?, output_type := type ?, run = ( mut r := this.output_type 1, for 0..this.b ( r = r * this.a ), r ),\n\
     parse_rank = *.parse_rank + 1,\n\
     associativity = right,\n\
     parse = (\n\
         if tape[-1]:type == void error «pw takes a left operand»,\n\
         this.a = tape[-1],\n\
         this.b = tape[1],\n\
         this.output_type = tape[-1]:type,\n\
         tape[0] = this,\n\
         tape.is_constructed[0] = true,\n\
         tape.remove(1),\n\
         tape.remove(-1)\n\
     )\n\
    ),\n";

#[test]
fn a_node_of_a_run_type_runs_the_function_built_for_its_fields() {
    assert_eq!(run_script(&format!("{POW_TYPE}2 pw 10 + 1")), 1025);
    assert_eq!(run_script(&format!("{POW_TYPE}2 pw 3 pw 2")), 512);
    // A type with a run and no parse of its own has no call form.
    assert_eq!(
        parse_err_after(&["sq2 := type ( a := i32 ?, run = ( this.a * this.a ) )"], "sq2(5)"),
        ParseError::RunTypeApplied
    );
}

#[test]
fn a_parse_is_ranked_by_what_it_takes_from_its_right() {
    let (mut store, mut trie, core) = new_core();
    for def in [
        "q := type ( parse_rank = dyad.parse_rank, parse = ( tape.is_constructed[0] = true ) )",
        "chooser := type ( parse_rank = fn.parse_rank, parse = ( tape.is_constructed[0] = true ) )",
    ] {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        Parser::new(def, &mut store, &mut trie, &core, s).parse_expression().unwrap();
    }
    let [q, chooser] = ["q", "chooser"].map(|name| use_of(&mut store, &mut trie, &core, name));
    // SAFETY: each is the binding of a type with a record head.
    unsafe {
        let [q, chooser] = [q, chooser].map(|b| Binding::read(b).dyad);
        let apply = meta::parse_rank_of(core.dyad_);
        assert_eq!(meta::parse_rank_of(q), apply);
        assert_eq!(meta::parse_rank_of(chooser), meta::parse_rank_of(core.fn_type));
        assert!(meta::parse_rank_of(chooser) > apply);
    }
}

#[test]
fn a_node_of_a_run_type_compiles_as_a_call_of_its_function() {
    // Only the caller compiles; the node's function stays interpreted behind the boundary.
    assert_eq!(
        run_script(&format!(
            "{POW_TYPE}f := fn (x := i32 ?) -> i32 ( x pw 3 + 1 ),\na := f(2),\nf.compile(),\na + f(2)"
        )),
        18
    );
    assert_eq!(
        run_script(&format!(
            "{POW_TYPE}f := fn (x := i32 ?) -> i32 ( x pw 3 + 1 ),\nf.compile(),\nf(2)"
        )),
        9
    );
}

#[test]
fn a_body_slot_takes_a_bracket_and_a_run_type_has_no_place() {
    assert_eq!(parse_err("t := type (run = 5)"), ParseError::SlotNeedsBody(SlotKind::Run));
    assert_eq!(parse_err("t := type (parse = 5)"), ParseError::SlotNeedsBody(SlotKind::Parse));
    assert_eq!(
        parse_err("t := type (parse = fn (tape := parsing_tape ?) -> void ( tape.recenter(0) ))"),
        ParseError::SlotNeedsBody(SlotKind::Parse)
    );
    // `pw` is an infix still waiting for its operands, not a type applied to `?`.
    assert!(matches!(parse_err_after(&[POW_TYPE], "p := pw ?"), ParseError::ConstructorFailed(_)));
}

#[test]
fn a_compiled_caller_reaches_an_uncompiled_callee_through_the_interpreter() {
    assert_eq!(
        run_script(
            "g := fn (x := i32 ?) -> i32 ( x + 1 ),\nf := fn (x := i32 ?) -> i32 ( g(x) * 2 ),\nf.compile(),\na := f(3),\ng.compile(), f.compile(),\na + f(3)"
        ),
        16
    );
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
    // This used to panic on the type root's record tag.
    assert_eq!(
        run_script(
            "metatype := fn (i := i32 ?) -> logos ( if (i == 0) (i32) else (f64) ),\nmetatype.compile(),\nsame := metatype(0) == i32,\nother := metatype(1) == f64,\nif (same and other) (i64 1) else (i64 0)"
        ),
        1
    );
}

#[test]
fn compile_member_on_four_params_compiles_and_runs() {
    let v = run_script_result(
        "f := fn (a := i64 ?, b := i64 ?, c := i64 ?, d := i64 ?) -> i64 ( a + b + c + d ),\nf.compile(),\nf(1, 2, 3, 4)",
    );
    assert_eq!(v, Ok(10));
}

#[test]
fn an_addressed_local_stays_in_memory_under_promotion() {
    // A promoted `a` would return the stale 5.
    diff_typed_call("fn (x := i64 ?) -> i64 ( a := i64 5, p := &a, p@ = 7, a + x )", "f(1)", 8);
}

#[test]
fn a_promoted_loop_matches_the_interpreter() {
    assert_eq!(
        run_script(
            "sum_to := fn (n := i64 ?) -> i64 ( mut i := i64 0, mut s := i64 0, while (i < n) ( s = s + i, i = i + 1 ), s ),\nbefore := sum_to(1000),\nsum_to.compile(),\nbefore + sum_to(1000)"
        ),
        999_000
    );
}

#[test]
fn compile_is_reserved_only_on_fn_typed_values() {
    assert_eq!(run_script("point := logos (compile := i32 ?),\np := point(7),\np.compile"), 7);
}

#[test]
fn an_argument_must_be_the_parameters_logos() {
    let defs: &[&str] =
        &["f := fn (a := i32 ?) -> i32 ( a )", "g := fn () -> i32 ( 1 )", "x := i64 5"];
    assert_eq!(parse_err_after(defs, "f(i32)"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "f(g)"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "f(«s»)"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "f(x)"), ParseError::TypeMismatch);

    assert_eq!(run_script("f := fn (a := i32 ?) -> i32 ( a ), f(7)"), 7);
    assert_eq!(run_script("f := fn (a := i32 ?) -> i32 ( a ), f(3 + 4)"), 7);
    assert_eq!(run_script("y := i32 2, f := fn (a := i32 ?) -> i32 ( a ), f(y + 1)"), 3);
    assert_eq!(run_script("h := fn (a) -> i32 ( 1 ), h(i32)"), 1);
}

#[test]
fn a_literal_into_a_pointer_parameter_is_rejected() {
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

/// Declare `point := logos (x := i32 ?, y := i32 ?)` in the root scope.
fn declare_point(store: &mut Store, trie: &mut RegexTrie, core: &Core) {
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new("point := logos (mut x := i32 ?, y := i32 ?)", store, trie, core, s);
    p.parse_expression().unwrap();
}

#[test]
fn record_instances_construct_read_and_write_fields_both_tiers() {
    let (mut store, mut trie, core) = new_core();
    declare_point(&mut store, &mut trie, &core);

    let func = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn () -> i32 ( mut p := point(3, 4), p.x = p.x + 36, p.x + p.y + 2 )",
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
    // u8, i64, i32 pack in declaration order at offsets 0, 1, 9.
    let (mut store, mut trie, core) = new_core();
    {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "cell := logos (a := u8 ?, b := i64 ?, c := i32 ?)",
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
        unsafe { s.declare(&mut trie, "f", test_binding(core.binding_, func)) }.unwrap();
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
    assert_eq!(
        parse_err("( mut x := 5, x = 7 )"),
        ParseError::AssignToLiteral(Box::new("5".into()))
    );
    assert_eq!(
        parse_err("( mut x := 5/2, x = 7 )"),
        ParseError::AssignToLiteral(Box::new("5/2".into()))
    );
    assert_eq!(parse_err("( 5 = 7 )"), ParseError::AssignToLiteral(Box::new("5".into())));
}

#[test]
fn sequences_run_in_order_and_yield_the_trailing_expression() {
    diff_var_fn(NumType::I32, 0, "fn () -> i32 ( a = 10, a = a + 1, a + 1 )", 12);
}

#[test]
fn the_comma_is_an_optional_readability_separator() {
    diff_var_fn(NumType::I32, 0, "fn () -> i32 ( a = 10, a = a + 1, a + 1 )", 12);
}

#[test]
fn block_local_declarations_do_not_leak() {
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
fn an_early_return_leaves_the_function_and_outside_one_is_refused() {
    assert_eq!(parse_err("( return 1, 2 )"), ParseError::EarlyReturn);
    diff_nullary_fn("fn () -> i32 ( if (true) (return 1) else (0), 2 )", 1);
    diff_typed_call("fn (n := i32 ?) -> i32 ( if (n > 2) (return 7), n + 1 )", "f(5)", 7);
    diff_typed_call("fn (n := i32 ?) -> i32 ( if (n > 2) (return 7), n + 1 )", "f(1)", 2);
    // The literal commits to the declared result, as the tail's does.
    diff_typed_call(
        "fn (n := i64 ?) -> i64 ( if (n > 2) (return 5000000000), n )",
        "f(3)",
        5_000_000_000,
    );
    // Through a block bound to a name: the function is left, not the block.
    diff_typed_call("fn (n := i32 ?) -> i32 ( x := ( return 5 ), x + 1 )", "f(0)", 5);
    assert_eq!(
        parse_err("fn (n := i32 ?) -> type ( if (n > 2) (return 5), i32 )"),
        ParseError::TypeMismatch
    );
}

#[test]
fn an_early_return_hands_out_no_place_a_scope_it_leaves_frees() {
    assert_eq!(
        parse_err("fn (n := i32 ?) -> i32 ( p := alloc 1 of i32 7, if (n > 2) (return p), 0 )"),
        ParseError::OwningEscape
    );
    assert_eq!(
        parse_err("fn (n := i32 ?) -> i32 ( ( p := alloc 1 of i32 7, if (n > 2) (return p) ), 0 )"),
        ParseError::OwningEscape
    );
    assert_eq!(
        parse_err("fn (n := i32 ?) -> i32 ( if (n > 2) (return alloc 1 of i32 7), 0 )"),
        ParseError::OwnershipAcrossReturn
    );
}

#[test]
fn a_tail_return_in_a_sequence_still_yields() {
    diff_nullary_fn("fn () -> i32 ( 1 + 1, return 40 + 2 )", 42);
}

#[test]
fn adjacent_minus_is_subtraction() {
    // The literal regex is unsigned, so `a-1` lexes as `a`, `-`, `1`.
    diff_var_fn(NumType::I32, 43, "fn () -> i32 ( a-1 )", 42);
}

#[test]
fn negative_literals_via_prefix_minus() {
    diff_typed_call("fn (x := i32 ?) -> i32 ( x )", "f(-1)", -1);
    diff_nullary_fn("fn () -> i32 ( i32(-5) )", -5);
    diff_nullary_fn("fn () -> i32 ( 0 - -3 )", 3);
}

#[test]
fn declaration_binds_a_name_to_a_value() {
    let (mut store, mut trie, core) = new_core();

    let decl = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("x := 5", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let bound = unsafe { declare::declared_of(decl) };
    unsafe {
        assert_eq!((*decl).ty, core.declare_);
        assert_eq!(crate::binding::Binding::spelling(declare::binding_of(decl)), "x");
        assert_eq!((*bound).ty, core.rational);
        assert_eq!(rational::mold(bound), Some(5));
    }

    let x_ref = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("x", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    assert_eq!(unsafe { core.through(x_ref) }, bound);
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `x_ref`/`decl` are valid nodes just parsed.
    unsafe {
        assert_eq!(rt.run(x_ref).unwrap(), 5);
        assert_eq!(rt.run(decl).unwrap(), 0);
    }
    // SAFETY: `x_ref` is the binding of `x`.
    unsafe {
        assert!(!Binding::has_gate(x_ref, core.pub_) && !Binding::has_gate(x_ref, core.mut_));
    }
}

/// The binding a use of `name` points at.
fn use_of(store: &mut Store, trie: &mut RegexTrie, core: &Core, name: &str) -> DyadPtr {
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new(name, store, trie, core, s);
    p.parse_expression().unwrap()
}

#[test]
fn a_type_read_reaches_roles_at_the_graph_level() {
    let (mut store, mut trie, core) = new_core();

    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let x_val = store.alloc_bytes(&5i32.to_ne_bytes());
    let x = store.alloc_raw(core.i32_, crate::dyad::global_place(x_val));
    unsafe { s.declare(&mut trie, "x", test_binding(core.binding_, x)) }.unwrap();

    let mut p = Parser::new("(x + x):type.roles[0]", &mut store, &mut trie, &core, s);
    let role = p.parse_expression().unwrap();
    // SAFETY: `role` is the role-name string node the read just yielded.
    unsafe {
        assert_eq!(crate::identities::string::text(role), b"lhs");
    }
}

#[test]
fn pub_marks_the_names_binding() {
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
        let bound = declare::declared_of(decl);
        assert_eq!((*bound).ty, core.rational);
        assert_eq!(rational::mold(bound), Some(5));
    }
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
    let x = use_of(&mut store, &mut trie, &core, "x");
    // SAFETY: `x` is the binding of `x`.
    unsafe {
        assert!(Binding::has_gate(x, core.pub_) && !Binding::has_gate(x, core.mut_));
    }
}

#[test]
fn pub_gates_a_typed_declaration() {
    let (mut store, mut trie, core) = new_core();

    let decl = {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("pub x := i32 5", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    // SAFETY: `decl` is the declare node just parsed.
    unsafe {
        assert_eq!((*decl).ty, core.declare_);
    }
    let x = use_of(&mut store, &mut trie, &core, "x");
    // SAFETY: `x` is the binding of `x`.
    unsafe {
        assert!(Binding::has_gate(x, core.pub_));
    }
}

#[test]
fn pub_gates_a_fn_declaration() {
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
        let f = declare::declared_of(decl);
        assert_eq!((*f).ty, core.fn_type);
    }
    let double = use_of(&mut store, &mut trie, &core, "double");
    // SAFETY: `double` is the binding of `double`.
    unsafe {
        assert!(Binding::has_gate(double, core.pub_));
    }
}

#[test]
fn shared_stands_first_on_the_members_binding() {
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new(
        "t := type (shared y := i32 3, shared mut z := i32 4, v := i32 ?)",
        &mut store,
        &mut trie,
        &core,
        s,
    );
    let decl = p.parse_expression().unwrap();
    // SAFETY: the type and the bindings of its members were just parsed.
    unsafe {
        let t = declare::declared_of(decl);
        let mut body = ScopeStack::new();
        body.push(crate::identities::meta::record_body_of(t));
        let y = body.resolve(&trie, "y").unwrap().binding;
        let z = body.resolve(&trie, "z").unwrap().binding;
        assert!(Binding::has_gate(y, core.shared_) && !Binding::has_gate(y, core.mut_));
        assert_eq!(
            crate::identities::array::items(Binding::read(z).gate),
            &[core.shared_, core.mut_]
        );
    }
    assert_eq!(parse_err("f := fn (shared x := i32 ?) -> i32 ( x )"), ParseError::SharedMisplaced);
}

#[test]
fn a_shared_name_at_top_level_is_one_place_for_the_run() {
    assert_eq!(run_script("shared mut n := i32 0,\nn = n + 1,\nn = n + 1,\nn"), 2);
    assert_eq!(run_script("shared m := hashmap i32 -> i32,\nm[2] = 4,\nm[2]"), 4);
    // In a loop the line names one place across the passes, made once.
    assert_eq!(
        run_script(
            "mut t := i32 0,\nfor i in 0..5 ( shared mut k := i32 0, k = k + 1, t = k ),\nt"
        ),
        5
    );
    assert_eq!(
        run_script(
            "mut j := i32 0, mut t := i32 0,\n\
             while j < 4 ( shared mut k := i32 0, k = k + 1, t = k, j = j + 1 ),\nt"
        ),
        4
    );
    // An unmarked name in the same loop is made again on every pass.
    assert_eq!(
        run_script(
            "mut t := i32 0,\n\
             for i in 0..3 ( shared mut k := i32 0, mut x := i32 0, k = k + 1, x = x + 1, t = t + k * 10 + x ),\nt"
        ),
        63
    );
    // Its value is made at the definition, so it reads names made before it.
    assert_eq!(
        run_script(
            "a := i32 7,\nmut t := i32 0,\nfor i in 0..3 ( shared k := a + 1, t = t + k ),\nt"
        ),
        24
    );
    assert_eq!(
        script_parse_err("for i in 0..3 ( shared k := i, k )"),
        ParseError::SharedInitReadsUnmade
    );
    assert_eq!(
        script_parse_err("for i in 0..3 ( x := i32 1, shared k := x, k )"),
        ParseError::SharedInitReadsUnmade
    );
    assert_eq!(
        script_parse_err("mut c := i32 1,\nif c == 1 ( x := i32 5, shared k := x, k )"),
        ParseError::SharedInitReadsUnmade
    );
}

#[test]
fn a_shared_name_in_a_function_is_one_place_across_its_calls() {
    let counter = "f := fn () -> i32 ( shared mut n := i32 0, n = n + 1, n ),\n";
    assert_eq!(run_script(&format!("{counter}f(), f(), f()")), 3);
    assert_eq!(run_script(&format!("{counter}f.compile(), f(), f(), f()")), 3);
    // Each function owns its own place, whatever the name.
    assert_eq!(
        run_script(
            "f := fn () -> i32 ( shared mut n := i32 0, n = n + 1, n ),\n\
             g := fn () -> i32 ( shared mut n := i32 100, n = n + 1, n ),\n\
             f(), g(), f(), g() + f()"
        ),
        105
    );
    // A loop body inside the function names the function's place too.
    assert_eq!(
        run_script(
            "f := fn () -> i32 ( mut t := i32 0, for i in 0..3 ( shared mut n := i32 0, n = n + 1, t = n ), t ),\n\
             f(), f()"
        ),
        6
    );
    // Without `mut` the name is not written, but a map behind it is.
    assert_eq!(
        run_script(
            "f := fn (k := i32 ?, put := i32 ?) -> i32 ( shared m := hashmap i32 -> i32, if put == 1 ( m[k] = k * k ), m[k] ),\n\
             f(3, 1), f(4, 1), f(3, 0)"
        ),
        9
    );
    assert!(matches!(
        script_parse_err("f := fn () -> i32 ( shared n := i32 0, n = 1, n )"),
        ParseError::NotMutable(_)
    ));
    assert_eq!(
        script_parse_err("f := fn (k := i32 ?) -> i32 ( shared n := k, n )"),
        ParseError::SharedInitReadsUnmade
    );
    assert_eq!(
        script_parse_err("f := fn () -> i32 ( x := i32 1, shared n := x, n )"),
        ParseError::SharedInitReadsUnmade
    );
}

#[test]
fn a_name_is_written_only_if_declared_mut() {
    assert_eq!(run_script("mut x := i32 5,\nx = 6,\nx"), 6);
    assert_eq!(run_script("pub mut x := i32 5,\nx = 6,\nx"), 6);
    assert_eq!(
        parse_err_after(&["x := i32 5"], "x = 6"),
        ParseError::NotMutable(Box::new("x".into()))
    );
    assert_eq!(
        parse_err_after(&["x := i32 ?"], "x = 6"),
        ParseError::NotMutable(Box::new("x".into()))
    );
    assert_eq!(parse_err("mut mut x := 5"), ParseError::DoubleGate);
    assert_eq!(
        parse_err_after(&["immut x := i32 5"], "x = 6"),
        ParseError::Immutable(Box::new("x".into()))
    );
    assert_eq!(
        parse_err_after(&["immut mut x := i32 5"], "x = 6"),
        ParseError::Immutable(Box::new("x".into()))
    );
    assert_eq!(
        parse_err("f := fn (immut a := i32 ?) -> i32 ( a = 1, a )"),
        ParseError::Immutable(Box::new("a".into()))
    );
    assert_eq!(
        parse_err("t := type (immut a := i32 ?, parse = ( this.a = tape[-1], tape[0] = this ))"),
        ParseError::Immutable(Box::new("a".into()))
    );
    // An `immut` sibling never written blocks nothing: the other fill parses.
    assert_eq!(
        run_script(
            "t := type (a := i32 ?, immut b := i32 ?, parse = ( this.a = tape[-1], tape[0] = this, tape.is_constructed[0] = true, tape.remove(-1) )),\n1"
        ),
        1
    );
    assert_eq!(parse_err("mut 5"), ParseError::GateNeedsDeclaration);
    assert_eq!(run_script("f := fn (mut a := i32 ?) -> i32 ( a = a + 1, a ),\nf(4)"), 5);
    assert_eq!(
        parse_err("f := fn (a := i32 ?) -> i32 ( a = a + 1, a )"),
        ParseError::NotMutable(Box::new("a".into()))
    );
}

#[test]
fn pub_without_a_declaration_is_an_error() {
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
    let (mut store, mut trie, core) = new_core();

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
    // SAFETY: `fact` is the declare node just parsed.
    let fact = unsafe { declare::declared_of(fact) };

    let cases = [(0i64, 1i64), (1, 1), (5, 120), (7, 5040)];

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

    // SAFETY: `fact` is the fn node just built and outlives every call.
    let _compiled = unsafe { compile_fn(rt.store, &core.lower, &core, fact) }.unwrap();
    // SAFETY: reading the installed bcode slot of the fn node.
    unsafe {
        let bcode = *((*fact).value as *const DyadPtr).add(FN_BCODE);
        assert!(!bcode.is_null(), "bcode installed");
    }

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

/// Parse `defs`, take the last as the recursive fn `name`, and assert every
/// case interpreted, then compiled. A local shared across activations would
/// make a write, recurse, read case disagree.
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

    let mut rt = Runtime::new(&core, &mut store);
    for &(arg, expect) in cases {
        let call = parse_call(rt.store, &mut trie, arg);
        assert_eq!(rt.run(call).unwrap(), expect, "interpreter {name}({arg})");
    }

    let _compiled = compile_fn(rt.store, &core.lower, &core, f).unwrap();
    for &(arg, expect) in cases {
        let call = parse_call(rt.store, &mut trie, arg);
        assert_eq!(rt.run(call).unwrap(), expect, "jit {name}({arg})");
    }
}

#[test]
fn recursion_with_a_local_is_per_activation_both_tiers() {
    // If `x` shared one blob across activations the inner calls would leave it 0.
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
    // A baked, shared address would read the innermost frame: 0.
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
    // SAFETY: `point` is defined first, then the recursive `h` uses it.
    unsafe {
        assert_recursion_both_tiers(
            &[
                "point := logos ( x := i32 ?, y := i32 ? )",
                "h := fn (n := i32 ?) -> i32 ( pt := point(n, 0), if (n < 1) (0) else (h(n - 1), pt.x) )",
            ],
            "h",
            &[(0, 0), (2, 2), (5, 5)],
        );
    }
}

#[test]
fn recursion_with_a_local_accumulator_both_tiers() {
    // SAFETY: a self-contained recursive definition and literal calls.
    unsafe {
        assert_recursion_both_tiers(
            &["fact := fn (n := i32 ?) -> i32 ( mut acc := i32 1, if (n < 1) (acc) else (acc = n * fact(n - 1), acc) )"],
            "fact",
            &[(0, 1), (1, 1), (5, 120), (7, 5040)],
        );
    }
}

#[test]
fn recursion_with_a_typed_declaration_local_both_tiers() {
    // SAFETY: a self-contained recursive definition and literal calls.
    unsafe {
        assert_recursion_both_tiers(
            &["f := fn (n := i32 ?) -> i32 ( mut a := i32 ?, a = n, if (n < 1) (a) else (f(n - 1), a) )"],
            "f",
            &[(0, 0), (1, 1), (5, 5)],
        );
    }
}

#[test]
fn a_nested_function_cannot_capture_an_outer_local() {
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
    // `inner(a)` passes the outer parameter by value, not a capture.
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
    assert_eq!(
        parse_err("outer := fn (a := i32 ?) -> i32 ( inner := fn () -> i32 ( a ), inner() )"),
        ParseError::CapturedLocal,
    );
    assert_eq!(
        parse_err("outer := fn (a := i32 ?) -> i32 ( in2 := fn () -> i32 ( p := &a, p@ ), in2() )"),
        ParseError::CapturedLocal,
    );
}

/// Parse `defs` in order, then the error `src` fails with.
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
    let defs = &["mut a := i64 1", "mut b := i32 2"];
    assert_eq!(parse_err_after(defs, "a = b"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "b = a"), ParseError::TypeMismatch);
}

#[test]
fn assignment_rejects_pointer_type_mismatches() {
    let defs = &["x := i32 7", "y := f64 2.5", "mut p := &x", "mut b := i32 2"];
    assert_eq!(parse_err_after(defs, "p = &y"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "p = b"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "b = p"), ParseError::TypeMismatch);
    assert_eq!(parse_err_after(defs, "p@ = y"), ParseError::TypeMismatch);
}

#[test]
fn assignment_accepts_a_matching_pointer_and_rewires() {
    let (mut store, mut trie, core) = new_core();
    let mut rt = Runtime::new(&core, &mut store);
    let mut result = 0;
    for line in ["x := i32 7", "y := i32 9", "mut p := &x", "p@", "p = &y", "p@"] {
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
        unsafe { s.declare(&mut trie, "add", test_binding(core.binding_, add)) }.unwrap();
        let mut p = Parser::new("fn () -> i32 ( add(40, 2) )", &mut store, &mut trie, &core, s);
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(outer, std::ptr::null_mut());

    // Compile `add` first so `outer`'s call has a machine address to bake.
    // SAFETY: `add` is the fn node just built and outlives every call.
    let _compiled_add = unsafe { compile_fn(&mut store, &core.lower, &core, add) }.unwrap();

    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`outer`/`add` are valid nodes; `_compiled_add` is alive.
    let interp = unsafe { rt.run(call) }.unwrap();

    // SAFETY: `outer` is the fn node just built; both compiled artifacts are alive.
    let _compiled_outer = unsafe { compile_fn(rt.store, &core.lower, &core, outer) }.unwrap();
    let jit = unsafe { rt.run(call) }.unwrap();

    assert_eq!(interp, 42);
    assert_eq!(jit, interp);
}

/// Declare `fn_src` as `f`, parse `call_src`, and diff the interpreter against the JIT; both must equal `expect`.
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
        unsafe { s.declare(&mut trie, "f", test_binding(core.binding_, func)) }.unwrap();
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
    diff_typed_call("fn (x := u8 ?, y := u8 ?) -> u8 ( x + y )", "f(200, 100)", 44);
    diff_typed_call("fn (x := i16 ?, y := i16 ?) -> i16 ( x - y )", "f(3, 10)", -7);
    diff_typed_call(
        "fn (x := u32 ?, y := u32 ?) -> u32 ( x + y )",
        "f(1000000000, 2000000000)",
        3_000_000_000,
    );
}

#[test]
fn signed_vs_unsigned_comparison_matches_between_tiers() {
    // 255 has no exact i8, so the i8 side takes it through the explicit wrapping cast.
    diff_typed_call("fn (x := i8 ?) -> i32 ( if (x < 1) (100) else (200) )", "f(i8(255))", 100);
    diff_typed_call("fn (x := u8 ?) -> i32 ( if (x < 1) (100) else (200) )", "f(255)", 200);
}

/// Diff a nullary fn between tiers, its body reading an enclosing variable `a` of
/// type `nt` initialised to the low bytes of `init`. Floats cannot ride the
/// argument path, so a float operand must be a stored variable.
fn diff_var_fn(nt: NumType, init: i64, fn_src: &str, expect: i64) {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&init.to_ne_bytes()[..nt.bytes()]);
    let a = store.alloc_raw(core.numtypes[nt as usize], crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();
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
    // f32 bits ride the low 32 of the container, a different ABI path than f64.
    diff_var_fn(
        NumType::F32,
        i64::from(2.5f32.to_bits()),
        "fn () -> f32 ( a + 1.5 )",
        i64::from(4.0f32.to_bits()),
    );
}

#[test]
fn f64_comparison_matches_between_tiers() {
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
    diff_typed_call("fn (x := i8 ?) -> i32 ( i32(x) )", "f(i8(255))", -1);
    diff_typed_call("fn (x := i32 ?) -> i8 ( i8(x) )", "f(300)", 44);
    diff_typed_call("fn (x := i8 ?) -> u8 ( u8(x) )", "f(i8(255))", 255);
    diff_typed_call("fn (x := i32 ?) -> i32 ( i32(x) )", "f(42)", 42);
    diff_var_fn(
        NumType::U32,
        3_000_000_000,
        "fn () -> i32 ( i32(a) )",
        3_000_000_000i64 - 4_294_967_296,
    );
    diff_var_fn(NumType::U64, 300, "fn () -> u8 ( u8(a) )", 44);
}

#[test]
fn casts_between_int_and_float_match_between_tiers() {
    // float -> int truncates toward zero and saturates out of range, matching Rust `as`.
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
    diff_var_fn(
        NumType::F64,
        1.5f64.to_bits() as i64,
        "fn () -> f32 ( f32(a) )",
        i64::from(1.5f32.to_bits()),
    );
}

#[test]
fn casts_fold_literal_operands() {
    diff_typed_call("fn () -> i32 ( i32(3) )", "f()", 3);
    diff_typed_call("fn () -> i32 ( i32(3.5) )", "f()", 3);
    diff_typed_call("fn () -> u8 ( u8(300) )", "f()", 44);
    diff_typed_call("fn () -> f64 ( f64(2) )", "f()", 2.0f64.to_bits() as i64);
}

#[test]
fn malformed_casts_are_rejected() {
    assert_eq!(parse_err("i32()"), ParseError::BadCast);
    assert_eq!(parse_err("i32(1, 2)"), ParseError::BadCast);
    assert_eq!(parse_err("i32(logos ())"), ParseError::BadCast);
}

#[test]
fn void_function_yields_unit_both_tiers() {
    diff_typed_call("fn () -> void ( 42 )", "f()", 0);
}

#[test]
fn void_function_runs_its_body_for_effect() {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let a_val = store.alloc_bytes(&41i32.to_ne_bytes());
    let a = store.alloc_raw(core.i32_, crate::dyad::global_place(a_val));
    unsafe { scopes.declare(&mut trie, "a", mut_binding(&mut store, &core, a)) }.unwrap();
    let func = {
        let mut p =
            Parser::new("fn () -> void ( a = a + 1 )", &mut store, &mut trie, &core, scopes);
        p.parse_expression().unwrap()
    };
    let call = store.alloc_raw(func, std::ptr::null_mut());
    let mut rt = Runtime::new(&core, &mut store);
    // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
    let interp = unsafe { rt.run(call) }.unwrap();
    let interp_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };
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
    diff_typed_call("fn () -> i64 ( i64(1000000 * 1000000) )", "f()", 1_000_000_000_000);
    diff_typed_call("fn () -> i64 ( 2000000000 + 2000000000 )", "f()", 4_000_000_000);
    diff_typed_call("fn () -> i64 ( 2000000000 * 2 )", "f()", 4_000_000_000);
    diff_typed_call("fn () -> i64 ( return 2000000000 + 2000000000 )", "f()", 4_000_000_000);
    diff_typed_call("fn () -> f64 ( f64(0.5 + 0.25) )", "f()", 0.75f64.to_bits() as i64);
}

#[test]
fn comptime_rational_comparison_folds() {
    // An i32 compare could not commit `3000000000`.
    diff_typed_call("fn () -> i32 ( if (3000000000 < 4000000000) (1) else (0) )", "f()", 1);
    diff_typed_call("fn () -> i32 ( if (5 > 3) (1) else (0) )", "f()", 1);
}

#[test]
fn a_comptime_rational_that_overflows_i64_is_rejected() {
    // The seed's rationals are i64 fractions.
    assert_eq!(parse_err("i64(10000000000 * 10000000000)"), ParseError::UncomputableLiteral);
}

#[test]
fn comptime_rational_commits_through_if_branches() {
    let fn_src = "fn (c := i32 ?) -> i64 ( if (c < 1) (2000000000 + 2000000000) else (0) )";
    diff_typed_call(fn_src, "f(0)", 4_000_000_000);
    diff_typed_call(fn_src, "f(5)", 0);
    let fn_src = "fn (c := i32 ?) -> i64 ( if (c < 1) (0) else (2000000000 + 2000000000) )";
    diff_typed_call(fn_src, "f(5)", 4_000_000_000);
}

#[test]
fn different_concrete_types_are_a_mismatch() {
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
    let (mut store, mut trie, core) = new_core();
    let mut s = ScopeStack::new();
    s.push(core.root_scope);
    let mut p = Parser::new("fn (x := i32 ?) -> i32 ( x + 1.5 )", &mut store, &mut trie, &core, s);
    assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::UncomputableLiteral));
}

#[test]
fn plus_over_non_numeric_operands_is_unresolved() {
    let (mut store, mut trie, core) = new_core();
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let mut p = Parser::new("logos () + 1", &mut store, &mut trie, &core, scopes);
    assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::UnsupportedOperands));
}
