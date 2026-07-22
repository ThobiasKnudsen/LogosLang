// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests of the `logos` binary: the real executable, real files,
//! real stdin — the surface a user downloads. Integration tests run with the
//! package root as the working directory, so the example and fixture paths
//! are relative.

use std::io::Write;
use std::process::{Command, Stdio};

/// The compiled `logos` binary under test.
fn logos() -> Command {
    Command::new(env!("CARGO_BIN_EXE_logos"))
}

#[test]
fn runs_a_file_top_to_bottom_and_prints_its_value() {
    let out = logos().arg("examples/answer.logos").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn a_parse_error_renders_clickable_with_a_caret() {
    let out = logos().arg("tests/fixtures/unknown_name.logos").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown_name.logos:2:5: error: unknown name"), "stderr: {err}");
    assert!(err.contains("      ^"), "stderr: {err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn an_unreadable_path_fails_cleanly() {
    let out = logos().arg("tests/fixtures/no_such_file.logos").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));
}

#[test]
fn the_repl_echoes_values_but_not_declarations_or_assignments() {
    let mut child = logos()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"x := i32 5\nx = 40\ndouble := fn (a : i32) -> i32 ( a + a )\nzz\ndouble(x) + 2\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Declarations and the assignment are silent (they still ran: the call
    // reads x = 40 through the declared double); only the tail expression
    // echoes. Strip the banner and prompts, keep the echoes.
    let echoes: Vec<&str> = stdout
        .lines()
        .skip(1) // the banner
        .map(|l| l.trim_start_matches("» "))
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(echoes, ["82"], "stdout: {stdout}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("<repl>:1:1: error: unknown name"), "stderr: {stderr}");
}

/// Run the REPL over `input` and return (echoed value lines, stderr).
fn repl(input: &[u8]) -> (Vec<String>, String) {
    let mut child = logos()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let echoes = String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1) // the banner
        .map(|l| l.trim_start_matches("» ").to_string())
        .filter(|l| !l.is_empty())
        .collect();
    (echoes, String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn the_repl_rolls_back_a_failed_lines_declarations() {
    // A failed line — parse error or run error — must not burn its name: the
    // same spelling declares cleanly on the next line instead of reporting
    // "shadowed" for the rest of the session.
    let (echoes, stderr) = repl(
        b"b := )\nb := 5\nf := fn (v : i32) -> i32 ( v )\ng := f(1,2)\ng := f(3)\ng + b\n",
    );
    assert_eq!(echoes, ["8"], "stderr: {stderr}");
    assert!(stderr.contains("nothing to evaluate here"), "stderr: {stderr}");
    assert!(stderr.contains("argument count"), "stderr: {stderr}");
    assert!(!stderr.contains("shadowed"), "stderr: {stderr}");
}

#[test]
fn the_repl_compiles_a_fn_across_lines() {
    // `f.compile()` on one line installs the machine code; the call on the
    // next line jumps to it. The compile itself is a silent statement, so the
    // only echo is the call's value.
    let (echoes, stderr) = repl(
        b"double := fn (x : i64) -> i64 ( x + x )\ndouble.compile()\ndouble(21)\n",
    );
    assert_eq!(echoes, ["42"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn an_else_if_chain_selects_the_matching_arm() {
    // `else if` is sugar for a nested `if` in the else slot, so a chain picks the
    // first matching arm with no hand-written `else ( if … )`. Each reachable arm
    // is exercised — a middle `else if`, a later one, the final `else` — and the
    // explicit nested form yields the same value the sugar does.
    let (echoes, stderr) = repl(
        b"x := i32 1\nif (x == 0) (i32 10) else if (x == 1) (i32 20) else (i32 30)\n\
          y := i32 2\nif (y == 0) (i32 10) else if (y == 1) (i32 20) else if (y == 2) (i32 30) else (i32 40)\n\
          z := i32 9\nif (z == 0) (i32 10) else if (z == 1) (i32 20) else (i32 30)\n\
          if (x == 1) (i32 20) else ( if (x == 2) (i32 30) else (i32 40) )\n",
    );
    assert_eq!(echoes, ["20", "30", "30", "20"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_repl_binds_a_name_to_a_type() {
    // `t := i32` makes `t` another spelling of `i32` (a `:=` value may be a
    // logos): it works by juxtaposition, as a conversion, and in a fn
    // signature; declaring it and echoing the bare logos are silent.
    let (echoes, stderr) = repl(
        b"t := i32\nx := t 7\nt(9)\nf := fn (v : t) -> t ( v + v )\nt\nf(x)\n",
    );
    assert_eq!(echoes, ["9", "14"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn logos_is_a_value_reflected_by_dot_logos_and_compared_by_identity() {
    // Roadmap #30: `logos` is a first-class value. `.logos` yields a value's logos, and
    // `==`/`!=` compare logos by identity (logos are interned, so pointer identity is
    // logos identity). Every result is a bool, so it echoes; declarations stay silent.
    let (echoes, stderr) = repl(
        b"i32 == i32\ni32 == f64\ni32 != f64\ni32.logos == logos\ni32.logos == i32\n\
          x := i32 5\nx.logos == i32\nx.logos == f64\nt := logos\ni32.logos == t\nlogos.logos == logos\n",
    );
    assert_eq!(
        echoes,
        ["true", "false", "true", "true", "false", "true", "false", "true", "true"],
        "stderr: {stderr}"
    );
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_logos_reflection_example_runs() {
    let out = logos().arg("examples/logos_reflection.logos").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn a_logos_value_prints_its_spelling() {
    // A program whose value is a logos prints the logos's name, not the raw bit
    // container (roadmap #30). The value rides out of a scope (comment + expression).
    let out = logos().arg("tests/fixtures/logos_name.logos").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "i32\n");
}

#[test]
fn a_type_returning_function_resolves_at_comptime() {
    // Roadmap #30 Phase 2: a `-> logos` call is run during parsing and becomes the
    // concrete logos it yields, so it flows through `==` and `:=` like any logos.
    let (echoes, stderr) = repl(
        b"pick := fn (i:i32) -> logos (if (i==0)(i32) else (f64))\n\
          pick(0) == i32\npick(1) == f64\npick(0) == f64\nt := pick(0)\nt == i32\n",
    );
    assert_eq!(echoes, ["true", "true", "false", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_logos_returning_fn_example_runs() {
    let out = logos().arg("examples/logos_returning_fn.logos").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn file_mode_runs_each_expression_as_it_parses() {
    // Build and run are one pass: a top-level expression runs the moment it is
    // parsed, so parse-time evaluation (a `-> logos` call reading an earlier
    // binding) sees committed state and file mode agrees with the REPL. Before,
    // the file driver parsed everything first and ran afterward, so the call
    // read x's zeroed storage instead of 5 and answered i32 rather than f64.
    let out =
        logos().arg("tests/fixtures/comptime_sees_committed_state.logos").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn a_type_call_with_a_runtime_argument_is_rejected() {
    // A `-> logos` call is comptime-only; an argument not known at parse time (here a
    // function parameter) is reported, not silently mis-evaluated.
    let (_echoes, stderr) = repl(
        b"pick := fn (i:i32) -> logos (if (i==0)(i32) else (f64))\n\
          g := fn (n:i32) -> i32 ( a := pick(n)  1 )\n",
    );
    assert!(stderr.contains("must be evaluable at parse time"), "stderr: {stderr}");
}

#[test]
fn a_logos_declaration_declares_a_place_of_that_type() {
    // `a : i32` introduces the name with its logos slot set and its value
    // undefined (zeroed until phase bits land): the declaration is silent,
    // `.logos` reflects the declared logos, `=` fills the value, reads load it.
    let (echoes, stderr) = repl(b"a : i32\na.logos == i32\na = 9\na\n");
    assert_eq!(echoes, ["true", "9"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_dependent_typed_declaration_takes_a_computed_type() {
    // `b : metalogos(1)` — the declared logos is the result of running a
    // `-> logos` function at parse time (roadmap #30): the dependent
    // declaration is the same declaration, its logos just computed.
    let (echoes, stderr) = repl(
        b"metalogos := fn (i:i32) -> logos (if (i==0)(i32) else (f64))\n\
          b : metalogos(1)\nb.logos == f64\nb = 7\nb\n",
    );
    assert_eq!(echoes, ["true", "7.0"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_logos_declaration_works_after_other_code() {
    let out = logos().arg("tests/fixtures/declared_logos_after_code.logos").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n");
}

#[test]
fn a_logos_declaration_rejects_a_non_type() {
    let (_echoes, stderr) = repl(b"a : 5\n");
    assert!(stderr.contains("must be a logos value"), "stderr: {stderr}");
}

#[test]
fn a_logos_declaration_names_the_non_numeric_gap() {
    // Storage for `a : bool` (and struct/pointer/void declarations) is not in
    // the seed yet; the error names the gap instead of mis-storing. (`a : logos`
    // is no longer a gap — it declares a logos variable.)
    let (_echoes, stderr) = repl(b"a : bool\n");
    assert!(stderr.contains("non-numeric logos are not in the seed yet"), "stderr: {stderr}");
}

#[test]
fn a_logos_variable_declares_fills_once_and_becomes_the_type() {
    // `a : logos` declares a logos variable (an undefined logos); `a = i32` fills
    // it at parse — comptime rebinding — after which the name is a full
    // spelling of the logos: `==` folds, juxtaposition builds typed values.
    let (echoes, stderr) = repl(
        b"a : logos\na.logos == logos\na == i32\na = i32\na == i32\ny := a 5\ny\n",
    );
    assert_eq!(echoes, ["true", "false", "true", "5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_logos_variable_fill_is_define_once_and_comptime_only() {
    // A second fill finds a real logos, not the placeholder, and is an ordinary
    // (rejected) assignment; a fill inside a fn body is rejected explicitly —
    // it would rebind at parse, where parse and run do not coincide.
    let (_e1, stderr1) = repl(b"a : logos\na = i32\na = f64\n");
    assert!(stderr1.contains("not an assignable place"), "stderr: {stderr1}");
    let (_e2, stderr2) = repl(b"a : logos\ng := fn () -> i32 ( a = i32  1 )\n");
    assert!(stderr2.contains("where parsing and running coincide"), "stderr: {stderr2}");
    let (_e3, stderr3) = repl(b"a : logos\na = 5\n");
    assert!(stderr3.contains("must be a logos value"), "stderr: {stderr3}");
}

#[test]
fn logical_operators_fold_over_bool_literals() {
    // and/or/not over bare bool literals fold at parse (pure, nothing lost) —
    // what keeps a comptime chain comptime; runtime operands still build nodes.
    let (echoes, stderr) = repl(
        b"true or false\ntrue and true\nnot (true)\n\
          a : logos\nif (a.logos == f32 or a.logos == logos) (a = f64) else (a = i32)\na == f64\n",
    );
    assert_eq!(echoes, ["true", "true", "false", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_comptime_if_drops_the_untaken_branch_unparsed() {
    // The condition folds to a bool literal at parse time (`a.logos == i32`), so
    // the `if` resolves during parsing and the untaken branch's tokens are
    // dropped unlexed: `a = 9.9` under `a : i32` would be a parse error
    // (UncomputableLiteral) if it were ever parsed — the proof it was skipped
    // is that this runs at all. A comptime-false chain link falls through to
    // the branch whose condition holds.
    let (echoes, stderr) = repl(
        b"a : i32\nif (a.logos == i32) (a = 9) else (a = 9.9)\na\n\
          b : f64\nif (b.logos == i32) (b = 1) else if (b.logos == f64) (b = 2.5) else (b = 3)\nb\n",
    );
    assert_eq!(echoes, ["9", "2.5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_metalogosfn_example_runs() {
    // The station #30 north-star, end to end: a `-> logos` fn computes the logos,
    // `a : metalogos(…)` declares with it, and a comptime `if` dispatches on
    // `a.logos`, skipping the untaken branches unparsed. The expected value
    // tracks the file's current argument (2 → f64 → the middle arm assigns 9.9;
    // the deep arm is pinned separately by the metalogos_arm fixture).
    let out = logos().arg("examples/metalogosfn.logos").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "9.9\n");
}

#[test]
fn the_metalogos_arm_fills_a_logos_variable() {
    // The deep arm the example reaches with argument 3: `a : metalogos(3)` is
    // `a : logos` — a logos variable — and the comptime chain's last arm fills it
    // with the logos i32, so the program's value IS a logos and prints `i32`.
    let out = logos().arg("tests/fixtures/metalogos_arm.logos").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "i32\n");
}

#[test]
fn a_declaration_snapshots_its_value_and_reads_are_stable() {
    // `:=` evaluates its value once, into the name's own storage; reading the
    // name is a plain load, never a re-evaluation. A block that sums 0..10 to 45
    // stays 45 across reads (it was re-running the loop and growing before), and
    // a later mutation of an input does not change the snapshot.
    let (echoes, stderr) = repl(
        b"c := (sum := i32 0, for i in 0..10 (sum = sum + i) sum)\nc\nc\n\
          a := i32 1\nx := a + a\na = 5\nx\n",
    );
    assert_eq!(echoes, ["45", "45", "2"], "stderr: {stderr}");
}

#[test]
fn a_declaration_copies_rather_than_aliases() {
    // `z := y` snapshots y's value into fresh storage; writing z must not write y.
    let (echoes, stderr) = repl(b"y := i32 1\nz := y\nz = 5\ny\nz\n");
    assert_eq!(echoes, ["1", "5"], "stderr: {stderr}");
}

#[test]
fn values_render_through_their_type() {
    // The CLI shows a value through its static logos, not the raw i64 container:
    // floats with a decimal point, unsigned at width, bool as true/false, and a
    // negative literal juxtaposed onto a logos (`i64 -1`).
    let (echoes, stderr) = repl(
        b"f32 5.5\nq := f64 2.5\nq + q\ni64 -1\nu8 200\n1 < 2\nnot (1 < 2)\n",
    );
    assert_eq!(echoes, ["5.5", "5.0", "-1", "200", "true", "false"], "stderr: {stderr}");
}

#[test]
fn help_prints_usage_and_version() {
    let out = logos().arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(env!("CARGO_PKG_VERSION")));
    assert!(text.contains("logos <file.logos>"));
}

#[test]
fn an_unknown_flag_is_a_usage_error() {
    let out = logos().arg("--nope").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
