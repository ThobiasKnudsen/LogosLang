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
fn the_repl_holds_declarations_across_lines_and_survives_errors() {
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
        .write_all(b"x := i32 5\nx = 40\nzz\nx + 2\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The banner line, then the echoes: 5, 40, (error on stderr), 42.
    assert!(stdout.contains("5\n"), "stdout: {stdout}");
    assert!(stdout.contains("40\n"), "stdout: {stdout}");
    assert!(stdout.contains("42\n"), "stdout: {stdout}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("<repl>:1:1: error: unknown name"), "stderr: {stderr}");
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
