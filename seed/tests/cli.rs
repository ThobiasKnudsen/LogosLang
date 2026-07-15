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
    // type): it works by juxtaposition, as a conversion, and in a fn
    // signature; declaring it and echoing the bare type are silent.
    let (echoes, stderr) = repl(
        b"t := i32\nx := t 7\nt(9)\nf := fn (v : t) -> t ( v + v )\nt\nf(x)\n",
    );
    assert_eq!(echoes, ["9", "14"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn type_is_a_value_reflected_by_dot_type_and_compared_by_identity() {
    // Roadmap #30: `type` is a first-class value. `.type` yields a value's type, and
    // `==`/`!=` compare types by identity (types are interned, so pointer identity is
    // type identity). Every result is a bool, so it echoes; declarations stay silent.
    let (echoes, stderr) = repl(
        b"i32 == i32\ni32 == f64\ni32 != f64\ni32.type == type\ni32.type == i32\n\
          x := i32 5\nx.type == i32\nx.type == f64\nt := type\ni32.type == t\ntype.type == type\n",
    );
    assert_eq!(
        echoes,
        ["true", "false", "true", "true", "false", "true", "false", "true", "true"],
        "stderr: {stderr}"
    );
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_type_reflection_example_runs() {
    let out = logos().arg("examples/type_reflection.logos").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn a_type_value_prints_its_spelling() {
    // A program whose value is a type prints the type's name, not the raw bit
    // container (roadmap #30). The value rides out of a scope (comment + expression).
    let out = logos().arg("tests/fixtures/type_name.logos").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "i32\n");
}

#[test]
fn a_typed_declaration_names_the_gap() {
    // `name : type` is settled design but not in the seed; the error must say
    // that, at the name, instead of calling the fresh name unknown.
    let (echoes, stderr) = repl(b"a : i32\n");
    assert!(echoes.is_empty());
    assert!(stderr.contains("<repl>:1:1: error: typed declarations"), "stderr: {stderr}");
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
    // The CLI shows a value through its static type, not the raw i64 container:
    // floats with a decimal point, unsigned at width, bool as true/false, and a
    // negative literal juxtaposed onto a type (`i64 -1`).
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
