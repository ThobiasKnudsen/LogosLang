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
fn a_typed_declaration_names_the_gap() {
    // `name : type` is settled design but not in the seed; the error must say
    // that, at the name, instead of calling the fresh name unknown.
    let (echoes, stderr) = repl(b"a : i32\n");
    assert!(echoes.is_empty());
    assert!(stderr.contains("<repl>:1:1: error: typed declarations"), "stderr: {stderr}");
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
