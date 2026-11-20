// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests of the `logos` binary: the real executable, real files,
//! real stdin, the surface a user downloads. Integration tests run with the
//! package root as the working directory, so the example and fixture paths
//! are relative.

use std::io::Write;
use std::process::{Command, Stdio};

/// The compiled `logos` binary under test.
fn logos() -> Command {
    Command::new(env!("CARGO_BIN_EXE_logos"))
}

#[test]
fn an_import_runs_the_file_which_prints() {
    let out = logos().arg("import file «examples/answer.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn the_drop_model_runs_at_file_scope() {
    let out = logos().arg("import file «tests/fixtures/heap.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn a_parse_error_renders_clickable_with_a_caret() {
    let out = logos().arg("import file «tests/fixtures/unknown_name.logos»").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown_name.logos:2:5: error: unknown name"), "stderr: {err}");
    assert!(err.contains("      ^"), "stderr: {err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn an_unreadable_path_fails_cleanly() {
    let out = logos().arg("import file «tests/fixtures/no_such_file.logos»").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));
}

#[test]
fn an_import_exposes_pub_and_hides_private() {
    let out =
        logos().arg("import file «tests/fixtures/lib_pub.logos», double(21)").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");

    let out =
        logos().arg("import file «tests/fixtures/lib_pub.logos», helper(1)").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not in scope"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn importing_a_library_alone_is_silent_and_clean() {
    let out = logos().arg("import file «tests/fixtures/lib_pub.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stdout.is_empty());
}

#[test]
fn logos_stats_measures_the_store_on_stderr() {
    let out = logos()
        .env("LOGOS_STATS", "1")
        .arg("import file «examples/answer.logos»")
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
    let err = String::from_utf8_lossy(&out.stderr);
    for field in [
        "store core:",
        "store after:",
        "store delta:",
        "nodes=",
        "node_bytes=",
        "arena_bytes=",
        "arena_allocs=",
        "boxed_bytes=",
        "boxed_allocs=",
        "total_bytes=",
        "source_bytes=",
        "code_bytes=",
        "per_source_byte=",
        "per_code_byte=",
        "per_node=",
    ] {
        assert!(err.contains(field), "missing {field} in stderr: {err}");
    }
}

#[test]
fn an_imported_file_cannot_see_the_import_site() {
    let out =
        logos().arg("x := 5, import file «tests/fixtures/uses_missing.logos»").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("uses_missing.logos:2:6"), "stderr: {err}");
    assert!(err.contains("not in scope"), "stderr: {err}");
}

#[test]
fn a_relative_import_resolves_against_the_importing_file() {
    let out = logos().arg("import file «tests/fixtures/outer.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn an_import_cycle_is_a_checked_error() {
    let out = logos().arg("import file «tests/fixtures/cycle_a.logos»").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("cycle"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn an_import_failure_reports_the_outer_position_then_the_inner_one() {
    let out = logos().arg("import file «tests/fixtures/broken.logos»").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = stderr.lines().collect();
    assert!(
        lines[0].contains("reading `tests/fixtures/broken.logos` as Logos failed"),
        "stderr: {stderr}"
    );
    assert!(lines[1].contains("import file «tests/fixtures/broken.logos»"), "stderr: {stderr}");
    assert!(lines[2].trim_end().ends_with('^'), "stderr: {stderr}");
    assert!(lines[3].starts_with("tests/fixtures/broken.logos:2:"), "stderr: {stderr}");
    assert!(
        lines[4].contains("nothing_here") && lines[5].trim_end().ends_with('^'),
        "stderr: {stderr}"
    );
}

#[test]
fn a_repeat_import_is_idempotent() {
    let out = logos()
        .arg(
            "import file «tests/fixtures/subdir/inner.logos», \
             import file «tests/fixtures/subdir/inner.logos», inner_val + 0",
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn a_bare_path_after_import_is_refused_and_named() {
    let (code, _, stderr) = run_line("import tests/fixtures/lib_pub.logos, double(21)");
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("1:8: error: `import` reads the file it is given"), "{stderr}");
    assert!(stderr.contains("write `import file «path»`"), "{stderr}");
}

#[test]
fn logos_file_gives_the_files_scope_whose_pub_names_are_read_with_a_dot() {
    let lib = "logos file «tests/fixtures/lib_pub.logos»";
    assert_eq!(line(&format!("({lib}).double(21)")), "42");
    assert_eq!(line(&format!("m := {lib}, m.double(4)")), "8");
    // A `pub` function reached so compiles as one an `import` set does.
    assert_eq!(line(&format!("g := ({lib}).double, g.compile(), g(5)")), "10");
    // Only `pub` names: a private one, and the scope's own lines, are not reached.
    for member in ["helper(1)", "dyads"] {
        let (code, _, stderr) = run_line(&format!("({lib}).{member}"));
        assert_eq!(code, Some(1), "{member}: stderr: {stderr}");
        assert!(stderr.contains("the file has no `pub` name"), "{member}: {stderr}");
    }
}

#[test]
fn logos_file_and_import_read_one_loaded_scope() {
    let gated = "file «tests/fixtures/gated.logos»";
    assert_eq!(line(&format!("(logos {gated}).r = 7, import {gated}, r")), "7");
    assert_eq!(line(&format!("import {gated}, r = 9, (logos {gated}).r")), "9");
    let (code, _, stderr) = run_line(&format!("(logos {gated}).s = 7"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("`s` is not `mut`"), "{stderr}");
    // Loaded once per run, whichever reads it first.
    let prints = "file «tests/fixtures/prints_pub.logos»";
    assert_eq!(line(&format!("x := (logos {prints}).v, import {prints}, x + v")), "imp\n4");
}

#[test]
fn logos_with_a_bracket_is_the_plain_group_and_type_reads_no_file() {
    assert_eq!(line("x := logos (1 + 2), x"), "3");
    assert_eq!(line("f := fn () -> i32 ( logos (1 + 2) ), f.compile(), f()"), "3");
    // The bracket gives the file unread: its top level would print `imp`.
    let prints = "tests/fixtures/prints_pub.logos";
    assert_eq!(line(&format!("f := logos (file «{prints}»), print «{{f.path}}»")), prints);
    let (code, _, stderr) = run_line(&format!("type file «{prints}»"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("1:6: error: expected one expression, found more"), "{stderr}");
}

#[test]
fn a_file_address_is_text_so_any_file_name_is_reached() {
    let (hidden, dashed) = ("tests/fixtures/.hidden.logos", "tests/fixtures/my-file.logos");
    assert_eq!(line(&format!("(logos file «{hidden}»).h + (logos file «{dashed}»).k")), "11");
    // A `{…}` would compute the address while the program runs, which only `fs` may.
    let (code, _, stderr) = run_line("file «tests/fixtures/{1}.logos»");
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("1:6: error: a `file «…»` address is written as plain text"));
    assert!(stderr.contains("`fs`"), "{stderr}");
    // `file` alone gives the file, its path as written and its bytes, read in the pass anywhere:
    // in a function body too, on both tiers.
    let dashed_file = format!("f := file «{dashed}»");
    assert_eq!(line(&format!("{dashed_file}, print «{{f.path}}»")), dashed);
    assert!(line(&format!("{dashed_file}, print «{{f.bytes}}»")).ends_with("pub k := i32 6"));
    let g = "g := fn () -> i32 ( f := file «tests/fixtures/lib_pub.logos», print «{f.path}», 1 )";
    assert_eq!(line(&format!("{g}, g()")), "tests/fixtures/lib_pub.logos\n1");
    assert_eq!(line(&format!("{g}, g.compile(), g()")), "tests/fixtures/lib_pub.logos\n1");
}

#[test]
fn a_file_reaches_only_files_inside_its_projects_folder() {
    // The command line read `reads_subdir.logos`, so its folder is the project, which the file
    // it reads keeps: `subdir/reads_up.logos` reaches `../lib_pub.logos`.
    assert_eq!(line("import file «tests/fixtures/reads_subdir.logos», top"), "5");
    // Read by the command line itself, `reads_up.logos` roots the project at `subdir`.
    for (file, at) in [
        ("subdir/reads_up.logos", "reads_up.logos:3:13: error: `file «../lib_pub.logos»`"),
        ("reads_outside.logos", "reads_outside.logos:2:11: error: `file «../../Cargo.toml»`"),
    ] {
        let (code, _, stderr) = run_line(&format!("import file «tests/fixtures/{file}»"));
        assert_eq!(code, Some(1), "{file}: stderr: {stderr}");
        assert!(stderr.contains(at), "{file}: {stderr}");
        assert!(stderr.contains("is outside this project's folder"), "{file}: {stderr}");
        assert!(stderr.contains("through the `fs` the command line hands down"), "{stderr}");
    }
    // The command line itself names any path.
    assert_eq!(line("f := file «Cargo.toml», 1"), "1");
}

#[test]
fn the_repl_imports_once_per_session_and_keeps_pub_names() {
    let (echoes, stderr) =
        repl("import file «examples/answer.logos»\nimport file «tests/fixtures/lib_pub.logos»\ndouble(4)\n");
    assert_eq!(echoes, ["42", "8"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_value_reads_its_type_and_a_reached_node_its_operands() {
    // `(x + x)` has arity 2, its operands: the type it gives back is its mint's.
    let (echoes, stderr) = repl(
        "x := i32 5\nprint «{x.type == i32}»\nb := x + x\nprint «{b:start.rhs.type.arity}»\n\
          print «{b:start.rhs.lhs.type == i32}»\nprint «{b:start.rhs.lhs}»\n\
          b:start.rhs.output_type == i32\n",
    );
    assert_eq!(echoes, ["true", "2", "true", "5", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_type_body_fills_its_slots_and_declares_its_members() {
    let (echoes, stderr) = repl(
        "t := type (static parse_rank = *.parse_rank + 1, static associativity = right)\n\
          print «{t.parse_rank}»\nprint «{t.associativity == right}»\nright.type == type\n",
    );
    assert_eq!(echoes, ["71.0", "true", "true"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        "g := type (static y := 3, static z := y + 3)\nprint «{g.y}»\nprint «{g.z}»\ng.parse_rank\n",
    );
    assert_eq!(echoes, ["3", "6", "91.0"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        "g := type (static y := i32 7, static z := i32 (y + 3))\n\
          print «{g.y}»\nprint «{g.z}»\ng.y + 1\n",
    );
    assert_eq!(echoes, ["7", "10", "8"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        "p := type (static k := 10, v := i32 ?)\nq := p(2)\n\
          print «{q.v}»\nprint «{q.k}»\nprint «{p.k}»\np.size_bytes\n",
    );
    assert_eq!(echoes, ["2", "10", "10", "4"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        "t := type (static # «note» y := 3, static z := 4 # «tail», v := i32 ?)\n\
          print «{t.y}»\nprint «{t.z}»\nt.size_bytes\n",
    );
    assert_eq!(echoes, ["3", "4", "4"], "stderr: {stderr}");
    let (echoes, stderr) =
        repl(b"h := fn () -> i32 ( n := i64 0 - 1, p := alloc n of i32 0, 1 )\ng := type (static y := h())\n");
    assert!(
        echoes.is_empty() && stderr.contains("a type body's own declaration failed"),
        "stderr: {stderr}"
    );
    let (echoes, stderr) = repl(
        "r := type (static lex_rank = 3)\nprint «{r:lex_rank}»\nprint «{+:lex_rank}»\nr.lex_rank\n",
    );
    assert_eq!(echoes, ["3.0", "0.0"], "stderr: {stderr}");
    assert!(!stderr.is_empty(), "`.lex_rank` is no member of a type");
    let (echoes, stderr) = repl(
        "r := type (static lex_rank = 3)\ns := r\nprint «{s:lex_rank}»\ns:lex_rank = 7\n\
          print «{s:lex_rank}»\nr:lex_rank\n",
    );
    assert_eq!(echoes, ["0.0", "7.0", "3.0"], "stderr: {stderr}");
    let (_echoes, stderr) =
        repl(b"f := fn (t := type ?) -> void ( )\nf(type (static lex_rank = 1))\n");
    assert!(stderr.contains("lex_rank is the name's"), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"s := type (precedence = 5)\n");
    assert!(echoes.is_empty() && stderr.contains("unknown name"), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"c := type (static run = 5)\n");
    assert!(echoes.is_empty() && stderr.contains("`static run = (…)`"), "stderr: {stderr}");
    let (echoes, stderr) = repl("x := 1\np := type (x := i32 ?)\nq := p(2)\nprint «{q.x}»\nx\n");
    assert_eq!(echoes, ["2", "1"], "stderr: {stderr}");
}

#[test]
fn any_spelling_the_index_can_hold_is_nameable() {
    let (echoes, stderr) = repl(
        "^ := i32 5\nprint «{^ + 1}»\na := i32 1\nab2 := i32 2\nprint «{ab2 + a}»\n\
          mut x := i32 5\nx=-1\nprint «{x}»\n\
          p := type (v := i32 ?)\nq := p(7)\nr := &q\nrr := &r\nprint «{rr@@.v}»\nx^2\n",
    );
    assert_eq!(echoes, ["6", "3", "-1", "7"], "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:2: error:"), "stderr: {stderr}");
    // caret.logos: 9 + 512 + 18.
    let out = logos().arg("import file «tests/fixtures/caret.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "539\n");
}

#[test]
fn a_type_s_size_bytes_is_a_u64_for_records_numbers_and_pointers() {
    let (echoes, stderr) = repl(
        "print «{i32.size_bytes}»\nprint «{u8.size_bytes}»\nprint «{(@u8).size_bytes}»\n\
          n := u64 3\nprint «{n * i32.size_bytes}»\n\
          p := type (a := i32 ?, b := i64 ?)\nn * p.size_bytes\n",
    );
    assert_eq!(echoes, ["4", "1", "8", "12", "36"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_power_demo_prints_nine() {
    let out = logos().arg("import file «identities/power.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "9\n");
}

#[test]
fn the_power_demo_takes_floats_fractions_and_negative_exponents() {
    // The demo's own `^`, its last line swapped for each case.
    let demo = std::fs::read_to_string("identities/power.logos").unwrap();
    // Given on the command line, its imports resolve against the working directory.
    let body = demo.trim_end().rsplit_once('\n').unwrap().0.replace("file «", "file «identities/");
    for (tail, want) in [
        ("x := f64 2.0, x ^ 3", "8.0"),
        ("x := i32 3, y := f64 2.0, x ^ y", "9.0"),
        ("x := f64 2.0, x ^ 0.5", "1.4142135623730954"),
        ("x := f64 10.0, x ^ 2.5", "316.2277660168377"),
        ("x := f32 2.0, x ^ 0.5", "1.4142135"),
        ("x := f64 2.0, x ^ -2", "0.25"),
        ("x := f64 -2.0, x ^ 3", "-8.0"),
        ("f64(2 ^ -3)", "0.125"),
        ("2 ^ 0.5", "1.4142135623730954"),
        ("g := fn (x := f64 ?) -> f64 ( x ^ 1.5 ), g.compile(), g(f64 4.0)", "8.0"),
        ("f(2) + 2 ^ 3 ^ 2", "521"),
        // Negation ranks with the number types, above `^` (›`-` negates where a space stands
        // before it and none after, or no value stands to its left; any other `-` subtracts‹
        // [[R157]]).
        ("y := i32 3, -y ^ 2", "9"),
        ("y := i32 3, 0 - y ^ 2", "-9"),
        ("-3 ^ 2", "9"),
        ("g := fn (y := i32 ?) -> i32 ( -y ^ 2 ), g.compile(), g(3)", "9"),
        ("x := f64 2.0, x ^ (f64 0.0 / 0.0)", "NaN"),
        ("x := f64 2.0, x ^ (f64 1.0 / 0.0)", "inf"),
        ("x := f64 2.0, x ^ (f64 -1.0 / 0.0)", "0.0"),
        ("x := f64 1.0, x ^ (f64 0.0 / 0.0)", "NaN"),
        ("g := fn (x := f64 ?, y := f64 ?) -> f64 ( x ^ y ), g.compile(), g(f64 2.0, f64 0.0 / 0.0)", "NaN"),
        ("g := fn (x := f64 ?, y := f64 ?) -> f64 ( x ^ y ), g.compile(), g(f64 2.0, f64 1.0 / 0.0)", "inf"),
        ("g := fn (x := f64 ?, y := f64 ?) -> f64 ( x ^ y ), g.compile(), g(f64 2.0, f64 -1.0 / 0.0)", "0.0"),
        ("g := fn (x := f64 ?, y := f64 ?) -> f64 ( x ^ y ), g.compile(), g(f64 2.0, f64 3.0)", "8.0"),
        ("x := f64 2.0, x ^ (f64 -1000000000000000000 * 1000000000000000000)", "0.0"),
        ("x := f64 0.5, x ^ (f64 1000000000000000000 * 1000000000000000000)", "0.0"),
        ("x := f64 2.0, x ^ (f64 1000000000000000000 * 1000000000000000000)", "inf"),
        ("g := fn (x := f64 ?, y := f64 ?) -> f64 ( x ^ y ), g.compile(), g(f64 2.0, f64 -1000000000000000000 * 1000000000000000000)", "0.0"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{body}\n{tail}"));
        assert_eq!((code, stdout.trim()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
    for (tail, expect) in [
        ("x := i32 2, x ^ -1", "a negative exponent of a whole number is not whole"),
        ("x := f64 -8.0, x ^ 0.5", "a negative base has no real power"),
    ] {
        let (code, _, stderr) = run_line(&format!("{body}\n{tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn the_power_demo_converts_a_whole_exponent_so_each_mint_has_one_result_type() {
    let demo = std::fs::read_to_string("identities/power.logos").unwrap();
    let body = demo.trim_end().rsplit_once('\n').unwrap().0.replace("file «", "file «identities/");
    // The last word: whether `a` and `b` are nodes of one mint of `^`.
    let same = "print «{a} {b} {a:start.rhs.type == b:start.rhs.type}»";
    let int_then_float = "9 1.7320508075688774 false";
    for (tail, want) in [
        (format!("x := i32 3, a := x ^ 2, b := x ^ 0.5, {same}"), int_then_float),
        (format!("x := i32 3, b := x ^ 0.5, a := x ^ 2, {same}"), int_then_float),
        (format!("x := i32 3, n := i32 4, a := x ^ n, b := x ^ 2, {same}"), "81 9 true"),
        (format!("x := i32 3, n := i64 4, a := x ^ n, b := x ^ 2, {same}"), "81 9 true"),
        (format!("x := i32 3, n := u8 2, a := x ^ n, b := x ^ 2, {same}"), "9 9 true"),
        (format!("y := f64 2.0, a := y ^ 2, b := y ^ 0.5, {same}"), "4.0 1.4142135623730954 false"),
        ("a := 2 ^ 3, b := 2 ^ (1/3), print «{a} {b}»".to_string(), "8 1.259921049894873"),
        ("g := fn (y := i32 ?, n := i32 ?) -> i32 ( y ^ n ), g(3, 4)".to_string(), "81"),
        (
            "g := fn (y := i32 ?, n := i32 ?) -> i32 ( y ^ n ), g.compile(), g(3, 4)".to_string(),
            "81",
        ),
        ("g := fn (y := i32 ?) -> f64 ( f64(y ^ 2) + y ^ 0.5 ), g(4)".to_string(), "18.0"),
        (
            "g := fn (y := i32 ?) -> f64 ( f64(y ^ 2) + y ^ 0.5 ), g.compile(), g(4)".to_string(),
            "18.0",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{body}\n{tail}"));
        assert_eq!((code, stdout.trim()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn a_types_static_output_type_is_what_its_nodes_give() {
    let t = |output: &str| {
        format!(
            "t := type ( lhs := i32 ?, static output_type := {output}, \
             static run = ( output_type lossy (lhs) * output_type 2 ), static parse_rank = 60, \
             static parse = ( tape[0].type = t, tape[0].lhs = tape[1], tape.is_constructed[0] = true, \
             tape.remove(1) ) )"
        )
    };
    for (src, want) in [
        (format!("{}, x := i32 5, print «{{t x}} {{(t x).output_type}}»", t("i32")), "10 i32"),
        (
            format!(
                "{}, x := i32 5, print «{{t x}} {{(t x).output_type}}»",
                t("if i32.size_bytes == 4 (i64) else (i32)")
            ),
            "10 i64",
        ),
        (format!("{}, f := fn (y := i32 ?) -> f64 ( t y ), f(6)", t("f64")), "12.0"),
        (format!("{}, f := fn (y := i32 ?) -> f64 ( t y ), f.compile(), f(6)", t("f64")), "12.0"),
        (format!("{}, f := fn (y := i32 ?) -> i64 ( t y + 1 ), f.compile(), f(6)", t("i64")), "13"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.trim()), (Some(0), want), "{src}: stderr: {stderr}");
    }
}

/// `q`'s parse stamps the type `get` builds for its operand's type and fills its field `v` by
/// name, looked up in that type when the parse runs.
fn chooser(field: &str, output: &str, run: &str, fill: &str) -> String {
    format!(
        "q := type ( static get := fn (l := type ?) -> type ( type ( {field}, \
         static output_type := {output}, static run = ( {run} ) ) ), static parse_rank = 60, \
         static parse = ( tape[0].type = get(tape[1].type), {fill}, tape.is_constructed[0] = true, \
         tape.remove(1) ) )"
    )
}

/// A postfix operator `name` that mints as `array` does: `get_mint` builds one type per type `t`
/// of the operand to its left, whose field `a` holds the operand, kept in a memo.
fn postfix_chooser(name: &str, output: &str, run: &str) -> String {
    format!(
        "{name} := type ( static mints := hashmap type -> type, \
         static get_mint := fn (t := type ?) -> type ( mut mint := mints[t], \
         if mint != ? return mint, mint = type ( a := t ?, static output_type := {output}, \
         static run = ( {run} ) ), mints[t] = mint, mint ), \
         static parse_rank = *.parse_rank + 1, static associativity = left, \
         static parse = ( tape[0].type = get_mint(tape[-1].type), tape[0].a = tape[-1], \
         tape.is_constructed[0] = true, tape.remove(-1) ) )"
    )
}

#[test]
fn a_parse_stamps_a_type_a_function_chooses_and_fills_its_fields_by_name() {
    let q = chooser("v := l ?", "l", "v + v", "tape[0].v = tape[1]");
    let wide = chooser(
        "v := i64 ?",
        "i64",
        "v * 3",
        "if tape[1].type ⊆ i64 ( tape[0].v = i64 (tape[1]) )",
    );
    for (src, want) in [
        (format!("{q}, x := i32 4, print «{{q x}}»"), "8"),
        (format!("{q}, print «{{q 4}}»"), "8"),
        (format!("{q}, x := f64 1.5, print «{{q x}} {{(q x).output_type}}»"), "3.0 f64"),
        (format!("{q}, f := fn (y := i32 ?) -> i32 ( q y + 1 ), f(5)"), "11"),
        (format!("{q}, f := fn (y := i32 ?) -> i32 ( q y + 1 ), f.compile(), f(5)"), "11"),
        (format!("{wide}, x := i32 4, print «{{q x}}»"), "12"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.trim()), (Some(0), want), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_parse_that_stamps_a_chosen_type_refuses_what_that_type_cannot_take() {
    let missing = chooser("v := l ?", "l", "v", "tape[0].w = tape[1]");
    let narrow = chooser("v := i32 ?", "i32", "v", "tape[0].v = tape[1]");
    let immut = chooser("immut v := l ?", "l", "v", "tape[0].v = tape[1]");
    let stamp = |ty: &str| {
        format!(
            "q := type ( static parse_rank = 60, static parse = ( tape[0].type = {ty}, \
             tape.is_constructed[0] = true, tape.remove(1) ) )"
        )
    };
    for (src, want) in [
        (format!("{missing}, x := i32 4, q x"), "has no field `w`"),
        (format!("{narrow}, x := i64 4, q x"), "these types do not match"),
        (format!("{immut}, x := i32 4, q x"), "`v` is `immut`"),
        (format!("{}, x := i32 4, q x", stamp("3")), "`tape[0].type = T` takes a type"),
        (format!("{}, x := i32 4, q x", stamp("i32")), "with a `run` or a `parse` of its own"),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert!(code != Some(0) && stderr.contains(want), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_lossy_conversion_reads_a_rational_value_when_it_runs() {
    let q = |conversion: &str| {
        format!(
            "q := type ( a := rational_number ?, static output_type := f64, \
             static run = ( {conversion} * 2.0 ), static parse_rank = 60, static parse = ( \
             tape[0].type = q, tape[0].a = tape[1], tape.remove(1), tape.is_constructed[0] = true ) ), \
             q 0.75"
        )
    };
    let (code, stdout, stderr) = run_line(&q("f64 lossy (a)"));
    assert_eq!((code, stdout.as_str()), (Some(0), "1.5\n"), "stderr: {stderr}");
    // A `rational_number` lies inside no float type: 1/3 has no `f64`.
    let (code, _, stderr) = run_line(&q("f64 (a)"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("these types do not match"), "stderr: {stderr}");
}

#[test]
fn a_chooser_hands_its_cell_to_a_type_minted_at_run() {
    let src = "mk := fn (t := type ?) -> type ( type ( static parse = ( tape.is_constructed[0] = true ) ) ), \
               c := type ( static parse_rank = fn.parse_rank, static associativity = right, static parse = ( \
                 if tape[1].type != type error «no», t := tape[1], tape[0] = mk(t), tape.remove(1) ) ), \
               x := c i32, x";
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "type\n");
}

#[test]
fn a_fresh_spelling_on_the_tape_is_a_node_of_type_unknown_name() {
    let k = |body: &str, operand: &str| {
        format!(
            "k := type ( static parse = ( {body}, tape.remove(1), \
             tape.is_constructed[0] = true ) ), k {operand}"
        )
    };
    let is_unknown = "print «{tape[1].type == unknown_name}»";
    for (operand, expect) in [("zz", "true\ntype\n"), ("i32", "false\ntype\n")] {
        let (code, stdout, stderr) = run_line(&k(is_unknown, operand));
        assert_eq!((code, stdout.as_str()), (Some(0), expect), "{operand}: stderr: {stderr}");
    }
    let (code, stdout, stderr) = run_line("unknown_name");
    assert_eq!((code, stdout.as_str()), (Some(0), "type\n"), "stderr: {stderr}");
    // An `unknown_name` holds a spelling, not a scope: reading its dyads is an error, not a crash.
    let (code, _, stderr) = run_line(&k("print «{tape[1].dyads}»", "zz"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("this node is not a scope"), "stderr: {stderr}");
}

#[test]
fn a_tape_read_checked_against_a_number_type_reads_as_that_number() {
    let q = |ty: &str, body: &str| {
        format!(
            "q := type ( v := {ty} ?, static parse_rank = 61, \
             static parse = ( tape[0].type = q, {body}, \
             tape.remove(1), tape.is_constructed[0] = true ) )"
        )
    };
    for (src, expect) in [
        (q("u64", "if not tape[1].dyads[0].type ⊆ u64 error «no», tape[0].v = tape[1].dyads[0] + 1") + ", x := q [5], x.v", "6"),
        (q("i32", "if tape[1].dyads[1].type == i32 ( tape[0].v = tape[1].dyads[1] * 2 ) else ( tape[0].v = i32 0 )") + ", x := q [5, i32 7], x.v", "14"),
        (q("u8", "if tape[1].dyads[0].type ⊆ u8 ( tape[0].v = tape[1].dyads[0] ) else ( tape[0].v = u8 1 )") + ", x := q [300], x.v", "1"),
        (q("i32", "mut s := i32 0, for i in 0..tape[1].dyads.size ( if not tape[1].dyads[i].type ⊆ i32 error «no», s = s + tape[1].dyads[i] ), tape[0].v = s") + ", x := q [4, 5, 6], x.v", "15"),
        (q("u64", "if not tape[1].type ⊆ u64 error «no», tape[0].v = tape[1] * 3") + ", x := q 5, x.v", "15"),
        // A line that is no constant is the line itself, read when the field is.
        (
            "y := i32 3, ".to_owned()
                + &q(
                    "i32",
                    "if not tape[1].dyads[0].type ⊆ i32 error «no», tape[0].v = tape[1].dyads[0]",
                )
                + ", x := q [y], x.v",
            "3",
        ),
    ] {
        let out = logos().arg(&src).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), expect, "{src}");
    }
    // A literal that does not fit fails the check.
    let src = q(
        "u64",
        "if not tape[1].dyads[0].type ⊆ u64 error «not a u64», tape[0].v = tape[1].dyads[0]",
    ) + ", x := q [-5]";
    let out = logos().arg(&src).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && stderr.contains("not a u64"), "{src}: stderr: {stderr}");
}

#[test]
fn a_typed_field_read_through_a_name_gives_its_value_on_both_tiers() {
    let q = |ty: &str, body: &str| {
        format!(
            "q := type ( v := {ty} ?, w := {ty} ?, static parse_rank = 61, \
             static parse = ( tape[0].type = q, {body}, \
             tape.remove(1), tape.is_constructed[0] = true ) )"
        )
    };
    let f = |ty: &str, body: &str, read: &str| {
        format!("{}, f := fn () -> {ty} ( x := q [5], {read} )", q(ty, body))
    };
    let checked = "if not tape[1].dyads[0].type ⊆ u64 error «no», tape[0].v = tape[1].dyads[0] + 1";
    for (src, want) in [
        (f("u64", checked, "x.v"), "6"),
        (f("u64", "tape[0].v = 5 + 1", "x.v + x.v"), "12"),
        (f("i32", "tape[0].v = 0 - 5 + 1", "x.v"), "-4"),
        (f("f64", "tape[0].v = 2.5", "x.v"), "2.5"),
        (f("rational_number", "tape[0].v = 1 / 3", "x.v"), "1/3"),
    ] {
        for call in ["f()", "f.compile(), f()"] {
            let src = format!("{src}, {call}");
            let (code, stdout, stderr) = run_line(&src);
            assert_eq!((code, stdout.trim()), (Some(0), want), "{src}: stderr: {stderr}");
        }
    }
    for call in ["f()", "f.compile(), f()"] {
        let src = format!("{}, {call}", f("u64", "tape[0].v = 1", "x.w"));
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(
            stderr.contains("field 2 of this node's type body was never written"),
            "{src}: {stderr}"
        );
    }
}

#[test]
fn an_instance_takes_its_bracket_as_a_call_built_in_its_parse() {
    // `x[k]` places the call `x.get(k)`, the line `k` its operand, run where it stands.
    let q = "q := type ( n := u64 ?, static get := fn (i := u64 ?) -> u64 ( n + i ), \
             static parse_rank = 61, static parse = ( \
               if tape[0].type == type ( tape[0].type = q, tape[0].n = tape[1], tape.remove(1) ) \
               else ( if tape[1].type == square_brackets ( \
                 if not tape[1].dyads[0].type ⊆ u64 error «no», \
                 tape[0] = tape[0].get(tape[1].dyads[0]), tape.remove(1) ) ), \
               tape.is_constructed[0] = true ) ), x := q u64 10, y := q u64 20";
    for (tail, expect) in [
        ("x[5]", "15"),
        ("x.get(3)", "13"),
        ("x[5] + y[1]", "36"),
        ("f := fn () -> u64 ( x[2] ), f()", "12"),
        ("f := fn () -> u64 ( x[2] + y[0] ), f.compile(), f()", "32"),
        ("f := fn (i := u64 ?) -> u64 ( x[i] ), f(3)", "13"),
        ("f := fn (i := u64 ?) -> u64 ( x[i] + y[i] ), f.compile(), f(4)", "38"),
    ] {
        let src = format!("{q}, {tail}");
        let out = logos().arg(&src).output().unwrap();
        assert!(out.status.success(), "{tail}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), expect, "{tail}");
    }
}

#[test]
fn the_array_fills_its_elements_when_the_program_runs() {
    let array = "import file «./identities/array.logos»";
    for (tail, printed) in [
        ("x := i32 4, y := i32 5, a := array i32 [x, y, 3], a[0] + a[1]", "9"),
        ("x := i32 5, y := i32 6, b := array i32 [x, y, 3], b[0] + b[1] + b[2]", "14"),
        ("g := fn (x := i32 ?) -> i32 ( c := array i32 [x, x + 1], c[1] ), g(7)", "8"),
        // Each run makes its own array: the second does not overwrite the first. An owner
        // takes a new array or a moved one, never a borrow of one the loop's end frees.
        (
            "mut first := array i32 [0, 0], mut second := array i32 [0, 0], \
             for i in 0..2 ( if i == 0 ( first = array i32 [i, 10] ) else ( second = array i32 [i, 10] ) ), \
             first[0] + second[0] * 10",
            "10",
        ),
    ] {
        let out = logos().arg(format!("{array}, {tail}")).output().unwrap();
        assert!(out.status.success(), "{tail}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), printed, "{tail}");
    }
}

#[test]
fn a_function_that_makes_an_array_compiles_and_agrees_with_the_interpreter() {
    let array = "import file «./identities/array.logos»";
    for (def, name, call, printed) in [
        ("f := fn (x := i32 ?) -> i32 ( c := array i32 [x, x], c[1] )", "f", "f(7)", "7"),
        (
            "f := fn (x := i32 ?) -> i32 ( c := array i32 [x, x + 1, 3], c[0] + c[1] + c[2] )",
            "f",
            "f(7)",
            "18",
        ),
        // A new array each pass, its elements the loop's values.
        (
            "f := fn (n := i32 ?) -> i32 ( mut s := i32 0, \
             for i in 0..n ( c := array i32 [i, i * 2], s = s + c[0] + c[1] ), s )",
            "f",
            "f(4)",
            "18",
        ),
        (
            "f := fn (x := i32 ?) -> i32 ( c := array i32 [x, x * 2, x * 3], mut s := i32 0, \
             for i in (u64 0)..(u64 3) ( s = s + c[i] ), s )",
            "f",
            "f(2)",
            "12",
        ),
        // Each call returns its own array: the second does not overwrite the first.
        (
            "t := array i32, g := fn (x := i32 ?) -> own t ( array i32 [x, x + 1] )",
            "g",
            "a := g(5), b := g(9), a[0] + a[1] + b[1]",
            "21",
        ),
    ] {
        for compile in ["", &format!("{name}.compile(), ")] {
            let src = format!("{array}, {def}, {compile}{call}");
            let out = logos().arg(&src).output().unwrap();
            assert!(
                out.status.success(),
                "{src}: stderr: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), printed, "{src}");
        }
    }
    let src =
        format!("{array}, f := fn () -> i32 ( c := array i32 [1, 2], c[5] ), f.compile(), f()");
    let out = logos().arg(&src).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && stderr.contains("index out of range"), "stderr: {stderr}");
}

#[test]
fn an_array_travels_as_its_address() {
    let array = "import file «./identities/array.logos», a := array i32 [1, 2, 3]";
    for (tail, printed) in [
        ("b := a, b[1]", "2"),
        ("f := fn (p := array i32 ?) -> i32 ( p[1] ), f(a)", "2"),
        ("f := fn (p := array i32 ?) -> i32 ( p[1] + p[2] ), f.compile(), f(a)", "5"),
        ("f := fn (p := array i32 ?) -> u64 ( p.size_bytes() + p.size ), f(a)", "15"),
        // `b := a` copies the array: a write through `b` is not seen through `a`, while one
        // through a parameter is.
        ("b := a, b.ptr@ = i32 9, a[0]", "1"),
        ("b := a, b.ptr@ = i32 9, b[0]", "9"),
        ("f := fn (p := array i32 ?) -> i32 ( p[0] = i32 9, 0 ), free (f(a)), a[0]", "9"),
        (
            "t := array i32, mk := fn (v := i32 ?) -> own t ( c := array i32 [v, 1], c ), \
             m := mk(3), n := mk(4), m[0] + n[0]",
            "7",
        ),
        // The return type stops before the body's bracket, so the mint takes no elements there.
        ("mk := fn () -> own array i32 ( array i32 [1, 2] ), m := mk(), m[1]", "2"),
        ("mk := fn (v := i32 ?) -> own array i32 ( array i32 [v, v + 1] ), m := mk(4), m[1]", "5"),
        ("a", "dyad"),
        // One mint per element type, kept by the chooser across its calls.
        ("t := array i32, u := array i32, t == u", "true"),
        ("t := array i32, u := array u8, t == u", "false"),
    ] {
        let out = logos().arg(format!("{array}, {tail}")).output().unwrap();
        assert!(out.status.success(), "{tail}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), printed, "{tail}");
    }
    for tail in [
        "f := fn (p := array i32 ?) -> i32 ( p[1] ), f(5)",
        "f := fn (p := array i32 ?) -> i32 ( p[1] ), f(array u8 [1, 2])",
    ] {
        let out = logos().arg(format!("{array}, {tail}")).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success() && stderr.contains("these types do not match"),
            "{tail}: {stderr}"
        );
    }
}

#[test]
fn a_call_result_takes_its_index_as_a_name_does() {
    let array = "import file «./identities/array.logos», a := array i32 [1, 2, 3], \
                 mk := fn () -> own array i32 ( array i32 [1, 2] )";
    for (tail, printed) in [
        ("mk()[1]", "2"),
        ("mk().at(1)", "2"),
        ("mk()[0] + mk()[1]", "3"),
        ("f := fn () -> i32 ( mk()[1] + mk().at(0) ), f()", "3"),
        ("f := fn () -> i32 ( mk()[1] + mk().at(0) ), f.compile(), f()", "3"),
        ("(a)[2]", "3"),
        ("array i32 [4, 5][1]", "5"),
        // A bracket an identity to its left claims stays that identity's argument.
        ("f := fn (p := array i32 ?) -> i32 ( p[1] ), f(a)", "2"),
    ] {
        let out = logos().arg(format!("{array}, {tail}")).output().unwrap();
        assert!(out.status.success(), "{tail}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), printed, "{tail}");
    }
    let out = logos().arg(format!("{array}, mk()[2]")).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && stderr.contains("index out of range"), "stderr: {stderr}");
}

#[test]
fn a_static_function_called_through_a_record_built_by_application_reads_its_fields() {
    let p = "p := type ( mut x := i32 ?, y := i32 ?, static s := fn () -> i32 ( x + y ), \
             static add := fn (k := i32 ?) -> i32 ( x + y + k ), static d := fn () -> i32 ( s() * 2 ), \
             static bump := fn () -> i32 ( x = x + 10, x ) ), q := p (1, 2)";
    for (tail, want) in [
        ("q.s()", "3\n"),
        ("q.add(10)", "13\n"),
        ("q.d()", "6\n"),
        ("f := fn () -> i32 ( q.s() ), f()", "3\n"),
        ("f := fn () -> i32 ( q.s() ), f.compile(), f()", "3\n"),
        ("f := fn () -> i32 ( r := p (3, 4), r.s() + r.add(1) ), f()", "15\n"),
        ("f := fn () -> i32 ( r := p (3, 4), r.s() + r.add(1) ), f.compile(), f()", "15\n"),
        ("f := fn (k := i32 ?) -> i32 ( q.add(k) ), f.compile(), f(7)", "10\n"),
        ("f := fn (r := p ?) -> i32 ( r.add(5) ), f.compile(), f(q) + f(p (10, 20))", "43\n"),
        // The function writes the record it is called on.
        ("mut m := p (1, 2), m.bump() + m.x", "22\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{p}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
    // Each field is read at its own width.
    let (code, stdout, stderr) = run_line(
        "p := type ( x := u8 ?, y := i64 ?, z := f64 ?, static s := fn () -> f64 ( z ), \
         static t := fn () -> i64 ( y * 2 ), static u := fn () -> u8 ( x ) ), q := p (u8 3, -9, 2.5), \
         f := fn () -> i64 ( q.t() ), f.compile(), print «{q.s()} {f()} {q.u()}»",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "2.5 -18 3\n"), "stderr: {stderr}");
}

#[test]
fn a_static_function_writes_the_value_it_is_called_on() {
    let p = "p := type ( mut x := i32 ?, y := i32 ?, static get := fn () -> i32 ( x + y ), \
             static bump := fn () -> i32 ( x = x + 10, x ), \
             static twice := fn () -> i32 ( free (bump()), bump() ) )";
    for (tail, want) in [
        ("mut m := p (1, 2), free (m.bump()), m.x", "11\n"),
        ("mut m := p (1, 2), free (m.twice()), m.x", "21\n"),
        ("mut m := p (1, 2), f := fn () -> i32 ( m.bump() ), f.compile(), free (f()), m.x", "11\n"),
        ("f := fn () -> i32 ( mut r := p (3, 4), free (r.bump()), r.x ), f()", "13\n"),
        ("f := fn () -> i32 ( mut r := p (3, 4), free (r.bump()), r.x ), f.compile(), f()", "13\n"),
        ("f := fn (mut r := p ?) -> i32 ( free (r.twice()), r.x ), f(p (1, 2))", "21\n"),
        (
            "f := fn (mut r := p ?) -> i32 ( free (r.twice()), r.x ), f.compile(), f(p (1, 2))",
            "21\n",
        ),
        // A function that only reads is called on any value.
        ("m := p (1, 2), m.get()", "3\n"),
        ("f := fn (r := p ?) -> i32 ( r.get() ), f.compile(), f(p (1, 2))", "3\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{p}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
    // A function that writes, itself or through a bare call, meets the gate `m.x = …` meets.
    for (tail, name) in [
        ("m := p (1, 2), m.bump()", "m"),
        ("m := p (1, 2), m.twice()", "m"),
        ("f := fn (r := p ?) -> i32 ( r.bump() ), f(p (1, 2))", "r"),
        ("f := fn () -> i32 ( r := p (3, 4), r.twice() ), f()", "r"),
    ] {
        let (code, _, stderr) = run_line(&format!("{p}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(&format!("`{name}` is not `mut`")), "{tail}: stderr: {stderr}");
    }
    // Each field is written at its own width.
    let (code, stdout, stderr) = run_line(
        "p := type ( mut x := u8 ?, mut y := i64 ?, mut z := f64 ?, \
         static s := fn () -> i64 ( x = x + u8 1, y = y * 2, z = z + 1.0, y ) ), \
         mut q := p (u8 3, -9, 2.5), f := fn () -> i64 ( q.s() ), f.compile(), \
         print «{f()} {q.x} {q.y} {q.z}»",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "-18 4 -18 3.5\n"), "stderr: {stderr}");
    // A node a parse built is written through too, and meets the same gate.
    let q = "q := type ( mut n := u64 ?, static bump := fn () -> u64 ( n = n + 10, n ), \
             static get := fn () -> u64 ( n ), static parse_rank = 61, static parse = ( \
             if tape[0].type == type ( tape[0].type = q, tape[0].n = tape[1], tape.remove(1) ), \
             tape.is_constructed[0] = true ) )";
    for (tail, want) in
        [("mut m := q u64 1, free (m.bump()), m.get()", "11\n"), ("m := q u64 1, m.get()", "1\n")]
    {
        let (code, stdout, stderr) = run_line(&format!("{q}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
    let (code, _, stderr) = run_line(&format!("{q}, m := q u64 1, m.bump()"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("`m` is not `mut`"), "stderr: {stderr}");
}

#[test]
fn a_static_function_writes_only_a_mut_field() {
    let p = |gate: &str| {
        format!(
            "p := type ( {gate}x := i32 ?, static bump := fn () -> i32 ( x = x + 10, x ) ), \
             mut m := p (1), free (m.bump()), m.x"
        )
    };
    let (code, stdout, stderr) = run_line(&p("mut "));
    assert_eq!((code, stdout.as_str()), (Some(0), "11\n"), "stderr: {stderr}");
    for (gate, expect) in [("", "`x` is not `mut`"), ("immut ", "`x` is `immut`")] {
        let (code, _, stderr) = run_line(&p(gate));
        assert_eq!(code, Some(1), "stderr: {stderr}");
        assert!(stderr.contains(expect), "stderr: {stderr}");
    }
}

#[test]
fn the_array_written_in_logos_reads_an_element_and_its_size() {
    let out = logos().arg("import file «./identities/array.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    for (tail, printed) in [("a[1]", "2\n"), ("a.size_bytes()", "12\n")] {
        let src =
            format!("import file «./identities/array.logos», a := array i32 [1, 2, 3], {tail}");
        let out = logos().arg(&src).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), printed, "{src}");
    }
    let array = "import file «./identities/array.logos», a := array i32 [1, 2, 3]";
    for (tail, printed) in [
        ("a[0] + a[2]", "4"),
        ("f := fn () -> i32 ( a[0] + a[2] ), f.compile(), f()", "4"),
        ("b := array u8 [7, 8], b[1]", "8"),
        ("i := u64 1, a[i]", "2"),
        ("f := fn (i := u64 ?) -> i32 ( a[i] ), f(1)", "2"),
        ("f := fn (i := u64 ?) -> i32 ( a[i] ), f.compile(), f(1)", "2"),
        ("j := u64 1, a[j + 1]", "3"),
        ("f := fn (j := u64 ?) -> i32 ( a[j + 1] ), f.compile(), f(0)", "2"),
        ("mut i := u64 0, g := fn () -> i32 ( a[i] ), i = 2, g()", "3"),
        ("t := array i32", ""),
        ("b := a", ""),
        ("b := a, b[1]", "2"),
        ("t := array i32, mut x := t ?, x = [4, 5], x[1]", "5"),
        ("b := array f64 [i32 1, 2], b[0]", "1.0"),
    ] {
        let out = logos().arg(format!("{array}, {tail}")).output().unwrap();
        assert!(out.status.success(), "{tail}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), printed, "{tail}");
    }
    for (tail, expect) in [
        ("a[3]", "index out of range"),
        ("i := u64 3, a[i]", "index out of range"),
        ("f := fn (i := u64 ?) -> i32 ( a[i] ), f(3)", "index out of range"),
        ("f := fn (i := u64 ?) -> i32 ( a[i] ), f.compile(), f(5)", "index out of range"),
        ("a[-1]", "the index type is not within the size type"),
        ("a[1.5]", "the index type is not within the size type"),
        ("b := array u8 [1, 300]", "an element does not fit the element type"),
        // The list is written in square brackets; a parenthesis is a scope argument.
        ("b := array i32 (1, 2)", "the list is written in square brackets, `array i32 [1, 2]`"),
        ("t := array u8, mut x := t ?, x = [4, 300]", "an element does not fit the element type"),
        ("b := array array u8 [[1, 300]]", "an element does not fit the element type"),
        ("b := array f32 [i32 1]", "an element does not fit the element type"),
        ("a[f64 1.0]", "the index is not an integer"),
        // `array ?` is an empty node of `array`, with nothing to run.
        ("n := array ?", "was never written by its constructor"),
    ] {
        let out = logos().arg(format!("{array}, {tail}")).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
}

/// `v := T ?` of a type whose values are nodes is a new empty node, filled field by field.
#[test]
fn an_empty_node_is_filled_field_by_field() {
    let src =
        "t := type ( mut a := i32 0, mut b := i32 0, static parse = ( tape.is_constructed[0] = true ) ), \
               f := fn () -> i32 ( mut v := t ?, v.a = 3, v.b = 4, v.a + v.b ), f()";
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "7");
}

#[test]
fn an_index_of_any_integer_type_reads_inside_the_array() {
    let array = "import file «./identities/array.logos», a := array i32 [1, 2, 3]";
    for (tail, printed) in [
        ("mut s := i32 0, for i in (i32 0)..(i32 3) ( s = s + a[i] ), s", "6"),
        ("mut s := i32 0, for i in 0..3 ( s = s + a[i] ), s", "6"),
        ("for i in (i32 0)..(i32 3) ( a[i] = a[i] * 10 ), a[0] + a[1] + a[2]", "60"),
        ("i := i32 1, a[i]", "2"),
        ("k := u8 1, a[k]", "2"),
        ("k := u8 2, a[k] = 8, a[2]", "8"),
        ("f := fn (i := i32 ?) -> i32 ( a[i] ), f.compile(), f(2)", "3"),
        (
            "f := fn (n := i32 ?) -> i32 ( mut s := i32 0, for i in (i32 0)..n ( s = s + a[i] ), s ), \
             f.compile(), f(3)",
            "6",
        ),
        ("f := fn (i := i8 ?) -> void ( a[i] = 9 ), f.compile(), f(1), a[1]", "9"),
    ] {
        let out = logos().arg(format!("{array}, {tail}")).output().unwrap();
        assert!(out.status.success(), "{tail}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), printed, "{tail}");
    }
    // A negative value is out of range when the read or the write runs; a literal one at parse.
    for (tail, expect) in [
        ("i := i32 0 - 1, a[i]", "index out of range"),
        ("i := i32 0 - 1, a[i] = 4", "index out of range"),
        ("i := i32 3, a[i]", "index out of range"),
        ("f := fn (i := i32 ?) -> i32 ( a[i] ), f(0 - 1)", "index out of range"),
        ("f := fn (i := i32 ?) -> i32 ( a[i] ), f.compile(), f(0 - 1)", "index out of range"),
        ("f := fn (i := i64 ?) -> void ( a[i] = 1 ), f.compile(), f(0 - 2)", "index out of range"),
        ("a[-1]", "the index type is not within the size type"),
    ] {
        let out = logos().arg(format!("{array}, {tail}")).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn an_array_element_is_written_where_the_read_finds_it() {
    let array = "import file «./identities/array.logos», a := array i32 [1, 2, 3]";
    for (tail, printed) in [
        ("a[0] = i32 9, a[0]", "9"),
        ("a[0] = 9, a[0] + a[1]", "11"),
        ("i := u64 2, a[i] = 7, a[2]", "7"),
        ("j := u64 0, a[j + 1] = 5, a[1]", "5"),
        ("a[0] = a[2], a[0]", "3"),
        ("a.at(1) = 4, a[1]", "4"),
        ("b := a, b[0] = i32 7, a[0]", "1"),
        ("b := a, b[0] = i32 7, b[0]", "7"),
        ("b := array u8 [1, 2], b[1] = 200, b[1]", "200"),
        ("f := fn (p := array i32 ?) -> void ( p[1] = 8 ), f(a), a[1]", "8"),
        ("f := fn (p := array i32 ?) -> void ( p[1] = 8 ), f.compile(), f(a), a[1]", "8"),
        ("f := fn (i := u64 ?) -> void ( a[i] = 6 ), f(2), a[2]", "6"),
        ("f := fn (i := u64 ?) -> void ( a[i] = 6 ), f.compile(), f(0), a[0]", "6"),
        (
            "f := fn (p := array i32 ?, i := u64 ?) -> void ( p[i + 1] = 8 ), f.compile(), \
             f(a, 1), a[2]",
            "8",
        ),
        (
            "f := fn (x := i32 ?) -> i32 ( c := array i32 [x, 2], c[0] = x + 10, c[0] ), \
             f.compile(), f(5)",
            "15",
        ),
        // Any call whose body ends in a dereference is a place, not only an array's.
        (
            "p := alloc 2 of i32 4, g := fn (q := @i32 ?, i := u64 ?) -> i32 ( (q + i)@ ), \
             g(p, 1) = 3, (p + 1)@",
            "3",
        ),
        // A name freed before the last line leaves the body's end nothing to run.
        (
            "p := alloc 1 of i32 0, \
             g := fn (r := @i32 ?) -> i32 ( q := alloc 1 of i32 1, free q, r@ ), g(p) = 5, p@",
            "5",
        ),
        ("x := u8 3, a[0] = x, a[0]", "3"),
    ] {
        let out = logos().arg(format!("{array}, {tail}")).output().unwrap();
        assert!(out.status.success(), "{tail}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), printed, "{tail}");
    }
    for (tail, expect) in [
        ("a[3] = 1", "index out of range"),
        ("i := u64 3, a[i] = 1", "index out of range"),
        ("f := fn (i := u64 ?) -> void ( a[i] = 6 ), f(3)", "index out of range"),
        ("f := fn (i := u64 ?) -> void ( a[i] = 6 ), f.compile(), f(5)", "index out of range"),
        (
            "f := fn (i := u64 ?) -> i32 ( c := array i32 [1, 2], c[i] = 9, c[i] ), \
             f.compile(), f(2)",
            "index out of range",
        ),
        ("a[0] = i64 4", "these types do not match"),
        ("x := u32 3, a[0] = x", "these types do not match"),
        ("a[0] = 1.5", "this literal has no exact value"),
        ("b := array u8 [1, 2], b[1] = 300", "this literal has no exact value"),
        ("a[-1] = 1", "the index type is not within the size type"),
        ("g := fn () -> i32 ( 5 ), g() = 3", "this is not an assignable place"),
        // A teardown would free the place before the write lands.
        (
            "g := fn () -> i32 ( q := alloc 1 of i32 3, q@ ), g() = 5",
            "this is not an assignable place",
        ),
    ] {
        let out = logos().arg(format!("{array}, {tail}")).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn a_static_name_in_a_function_is_made_once_at_the_definition() {
    let src = "f := fn () -> i32 ( static n := (print «made», i32 5), n ), \
               print «defined», free (f()), free (f()), f()";
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "made\ndefined\n5\n");
    // A type built per call reads the name, and the second call finds the first's.
    let src = "get := fn (t := type ?) -> type ( static m := hashmap type -> type, mut r := m[t], \
               if r != ? return r, print «miss», r = type ( x := t ?, static seen := m ), \
               m[t] = r, r ), get(i32) == get(i32)";
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "miss\ntrue\n");
    // An owning value is freed when the program ends, not at each call's end.
    let src = "f := fn () -> i32 ( static p := alloc 1 of i32 7, p@ ), f() + f()";
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "14\n");
}

/// The power operator, minting through a function of its own by the base's type, on one line
/// for the REPL.
const POWER: &str = "^ := type ( static power_mints := hashmap type -> type, \
    static get_mint := fn (l := type ?) -> type ( mut mint := power_mints[l], if mint != ? return mint, \
    mint = type ( lhs := l ?, rhs := i32 ?, static output_type := l, \
    static run = ( mut r := output_type 1, for 0..rhs ( r = r * lhs ), r ) ), \
    power_mints[l] = mint, mint ), \
    static parse_rank = *.parse_rank + 1, static associativity = right, \
    static parse = ( tape[0].type = get_mint(tape[-1].type), tape[0].lhs = tape[-1], tape[0].rhs = tape[1], \
    tape.is_constructed[0] = true, tape.remove(1), tape.remove(-1) ) )";

#[test]
fn a_held_run_body_is_constructed_once_per_field_type_set_in_both_tiers() {
    // `a ^ 2` over i32 and `b ^ 2` over i64 each get their own constructed body: 9 + 16.
    let src =
        format!("{POWER}, h := fn (a := i32 ?, b := i64 ?) -> i64 ( i64(a ^ 2) + b ^ 2 ), h(3, 4)");
    let out = logos().args([&src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "25");
    let src = format!(
        "{POWER}, h := fn (a := i32 ?, b := i64 ?) -> i64 ( i64(a ^ 2) + b ^ 2 ), \
         h.compile(), h(3, 4)"
    );
    let out = logos().args([&src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "25");
}

#[test]
fn a_built_node_runs_and_is_never_parsed_again() {
    for (tail, expect) in [
        ("n := 2 ^ 3, n + 1", "9"),
        ("f := fn () -> i32 ( n := 2 ^ 3, n * 2 ), f()", "16"),
        ("2 ^ 3 + 2 ^ 2", "12"),
    ] {
        let out = logos().args([&format!("{POWER}, {tail}")]).output().unwrap();
        assert!(out.status.success(), "{tail}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), expect, "{tail}");
    }
}

#[test]
fn a_static_member_is_read_through_a_node_and_through_the_type() {
    let point = "point := type ( x := i32 ?, y := i32 ?, static dims := i32 2 ), \
                 p := point (1, 2)";
    for (read, expect) in [("p.dims", "2"), ("point.dims", "2"), ("p.x", "1")] {
        let out = logos().args([&format!("{point}, {read}")]).output().unwrap();
        assert!(out.status.success(), "{read}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), expect, "{read}");
    }
    for (read, expect) in [("point.x", "a place in each node"), ("point.run", "does not fit")] {
        let out = logos().args([&format!("{point}, {read}")]).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(expect), "{read}: stderr: {stderr}");
    }
    // The run its values share is one place, read through the type.
    let sq = "sq := type ( a := i32 ?, static output_type := i32, static run = ( a * a ), \
              static parse_rank = 60, static parse = ( tape[0].type = sq, tape[0].a = tape[1], \
              tape.is_constructed[0] = true, tape.remove(1) ) )";
    let out = logos().args([&format!("{sq}, r := sq.run, sq.run")]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "dyad");
}

#[test]
fn a_static_function_called_on_no_value_uses_no_field() {
    let t = "t := type ( n := i64 ?, static k := fn () -> i64 ( 1 ), \
             static inc := fn (a := i64 ?) -> i64 ( a + 1 ), static j := fn () -> i64 ( k() + 1 ), \
             static get := fn () -> i64 ( n ), static via := fn () -> i64 ( get() ), \
             static held_k := fn (a := dyad ?) -> i64 ( a + k() ), \
             static held_get := fn (a := dyad ?) -> i64 ( a + get() ) )";
    for (tail, want) in [
        ("t.k()", "1\n"),
        ("t.inc(i64 5)", "6\n"),
        ("t.j()", "2\n"),
        // A line that waits for the run calls bare as any other line does.
        ("t.held_k(i64 1)", "2\n"),
        ("g := fn () -> i64 ( t.held_k(i64 1) ), g.compile(), g()", "2\n"),
        ("v := t (3), g := fn () -> i64 ( v.held_get(i64 1) ), g.compile(), g()", "4\n"),
        ("g := fn () -> i64 ( t.k() ), g()", "1\n"),
        ("g := fn () -> i64 ( t.k() ), g.compile(), g()", "1\n"),
        ("g := fn (b := i64 ?) -> i64 ( t.inc(b) ), g.compile(), g(i64 5)", "6\n"),
        // By another name the call has no value either.
        ("f := t.k, f()", "1\n"),
        ("f := t.inc, g := fn () -> i64 ( f(i64 1) ), g.compile(), g()", "2\n"),
        // A function that reads a field through a bare call reads it through a value as well.
        ("v := t (3), v.via() + v.j()", "5\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{t}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
    // So is a bare call in a `static` member's value, made at the definition.
    let (code, stdout, stderr) = run_line(
        "t := type ( n := i64 ?, static k := fn () -> i64 ( 1 ), static c := k() + 1 ), t.c",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "2\n"), "stderr: {stderr}");
    // One that reads or writes a field, itself or through a bare call, has no value to use. The
    // refusal names the function as the call spells it, and shows the call through a value only
    // where that is the member's own name.
    let set = "s := type ( mut n := i64 ?, static set := fn () -> i64 ( n = 2, 0 ) )";
    for (src, name, through) in [
        (format!("{t}, t.get()"), "get", "`v.get(…)`"),
        (format!("{t}, t.via()"), "via", "`v.via(…)`"),
        (format!("{t}, t.held_get(i64 1)"), "held_get", "`v.held_get(…)`"),
        (
            format!("{t}, g := fn () -> i64 ( t.held_get(i64 1) ), g.compile(), g()"),
            "held_get",
            "`v.held_get(…)`",
        ),
        (format!("{t}, g := fn () -> i64 ( t.get() ), g()"), "get", "`v.get(…)`"),
        (format!("{set}, s.set()"), "set", "`v.set(…)`"),
        (format!("{t}, f := t.get, f()"), "f", "by the member that holds it"),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        let expect =
            format!("`{name}` works on a value of its type: call it through one, {through},");
        assert!(stderr.contains(&expect), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_static_function_is_called_on_the_record_behind_a_pointer() {
    let t = "u := type ( a := i64 ?, static s := fn () -> i64 ( a ) ), \
             t := type ( mut n := i64 ?, inner := u ?, static k := fn () -> i64 ( 1 ), \
             static get := fn () -> i64 ( n ), static bump := fn () -> i64 ( n = n + 10, n ) ), \
             p := alloc 2 of t (i64 3, u (i64 7))";
    for (tail, want) in [
        ("p@.k()", "1\n"),
        ("p@.get()", "3\n"),
        ("(p + 1)@.get()", "3\n"),
        ("p@.inner.s()", "7\n"),
        ("g := fn (q := @t ?) -> i64 ( q@.get() + q@.inner.s() ), g.compile(), g(p)", "10\n"),
        // The write reaches the record behind the pointer.
        ("free (p@.bump()), p@.n", "13\n"),
        ("g := fn (q := @t ?) -> i64 ( free (q@.bump()), q@.n ), g.compile(), g(p)", "13\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{t}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn a_static_member_holding_any_function_is_called_through_a_member_read() {
    let t = "g := fn (a := i64 ?) -> i64 ( a * 2 ), t := type ( n := i64 ?, static h := g ), \
             v := t (3)";
    for (tail, want) in [
        ("t.h(i64 4)", "8\n"),
        ("v.h(i64 5)", "10\n"),
        ("p := alloc 1 of t (i64 1), p@.h(i64 6)", "12\n"),
        ("k := fn () -> i64 ( t.h(i64 4) ), k.compile(), k()", "8\n"),
        ("k := fn () -> i64 ( w := t (1), w.h(i64 6) ), k.compile(), k()", "12\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{t}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn a_value_runs_its_type_s_parse_as_tape_0() {
    // The type builds the value and hands the cell on; the same parse then
    // reads the value as `tape[0]` and consumes the cell to its right.
    let q = "q := type ( size := i32 ?, static parse_rank = 61, static parse = ( \
             if tape[0].type == type ( tape[0].type = q, tape[0].size = tape[1], tape.remove(1) ) \
             else ( tape[0] = tape[0].size, tape.remove(1), tape.is_constructed[0] = true ) ) )";
    for (src, expect) in [
        (format!("{q}, q i32 5 i32 7"), "5"),
        (format!("{q}, f := fn () -> i32 ( q i32 8 i32 0 ), f()"), "8"),
    ] {
        let out = logos().args([&src]).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), expect, "{src}");
    }
    // A parse that says nothing for the value leaves the handed-on cell unconstructed.
    let out = logos()
        .args(["q := type ( size := i32 ?, static parse_rank = 61, static parse = ( \
                if tape[0].type == type ( tape[0].type = q, tape[0].size = tape[1], tape.remove(1) ) ) ), \
                q i32 5"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("left its own cell unconstructed"), "stderr: {stderr}");
}

#[test]
fn a_static_member_read_through_tape_0_is_the_member_itself() {
    // `tape[0].element_type` folds to the identity, so `alloc … of` has a static pointee.
    let q =
        "q := type ( static element_type := i32, v := i32 ?, static parse_rank = 60, static parse = ( \
             tape[0] = alloc 1 of tape[0].element_type 9, tape.is_constructed[0] = true ) )";
    let out = logos().args([&format!("{q}, a := q, a@ + 1")]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "10");
    let out = logos()
        .args(["q := type ( static element_type := i32, v := i32 ?, static parse_rank = 60, \
                static parse = ( tape[0].type = q, tape[0].element_type = i64, tape.is_constructed[0] = true ) ), q"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not an assignable place"), "stderr: {stderr}");
}

#[test]
fn a_constructed_run_body_is_kept_on_the_type_across_repl_lines() {
    // A later line is its own parser, so it must find the i32 body built by the first use.
    let (echoes, stderr) = repl(
        POWER.to_owned()
            + "\nf := fn (x := i32 ?) -> i32 ( x ^ 2 )\nprint «{f(2)}»\n\
               g := fn (x := i32 ?) -> i32 ( x ^ 3 )\ng.compile()\ng(3)\n",
    );
    assert_eq!(echoes, ["4", "27"], "stderr: {stderr}");
}

#[test]
fn a_run_body_sees_its_definition_and_its_own_locals_only() {
    let sq = "sq := type ( a := i32 ?, static output_type := i32, static run = ( a * a * k ), \
              static parse_rank = 60, static parse = ( tape[0].type = sq, tape[0].a = tape[1], \
              tape.is_constructed[0] = true, tape.remove(1) ) )";
    let before = format!("k := i32 2, {sq}, f := fn (x := i32 ?) -> i32 ( sq x ), f(5)");
    let out = logos().args([&before]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "50");
    let after = format!("{sq}, k := i32 2, f := fn (x := i32 ?) -> i32 ( sq x ), f(5)");
    let out = logos().args([&after]).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("the run body of `sq` could not be constructed"), "stderr: {stderr}");
    assert!(stderr.contains("run body of `sq`:1:"), "stderr: {stderr}");
    assert!(stderr.contains("unknown name `k`"), "stderr: {stderr}");
}

#[test]
fn a_run_body_over_bare_literal_operands_is_a_rational_specialization() {
    // A node whose value fields are all literals folds at construction; `f(2) + 2 ^ 3 ^ 2 + 2 * 3 ^ 2` is 9 + 512 + 18.
    let src = format!("{POWER}, 2 ^ 3");
    let out = logos().args([&src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "8");
    let src = format!(
        "{POWER}, f := fn (x := i32 ?) -> i32 ( x^3 + 1 ), f.compile(), \
         f(2) + 2 ^ 3 ^ 2 + 2 * 3 ^ 2"
    );
    let out = logos().args([&src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "539");
    let (echoes, stderr) = repl(
        POWER.to_owned()
            + "\nq := rational_number 2\nprint «{q ^ 3}»\n\
               h := fn (n := i32 ?) -> rational_number ( 2.5 ^ n )\nh(2)\n",
    );
    assert_eq!(echoes, ["8", "25/4"], "stderr: {stderr}");
}

#[test]
fn rational_places_and_operators_run_interpreted_and_are_refused_compiled() {
    let (echoes, stderr) = repl(
        "mut q := rational_number 2\nr := rational_number 3\nprint «{q * r}»\nprint «{q / 3}»\n\
          q = q * 2\nprint «{q}»\nprint «{q < 5}»\nmut x := rational_number ?\nx = 7\nprint «{x}»\n\
          k := fn () -> rational_number ( q )\nk()\n",
    );
    assert_eq!(echoes, ["6", "2/3", "4", "true", "7", "4"], "stderr: {stderr}");
    // The seed has no rational-to-machine conversion yet, so a rational value in a typed slot is the mismatch.
    for (src, expect) in [
        ("mut q := rational_number 2, q + i32 1", "do not match"),
        ("k := fn () -> i32 ( mut q := rational_number 2, q ), k()", "do not match"),
        ("mut q := rational_number 2, mut z := i32 4, z = q", "do not match"),
        ("g := fn () -> i32 ( mut q := rational_number 2, q = q * 2, 4 ), g.compile()", "compiled"),
    ] {
        let out = logos().args([src]).output().unwrap();
        assert!(!out.status.success(), "{src} succeeded");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_pattern_spelling_is_declared_through_regex() {
    // `<=>` beats `<=` `>` by length at equal rank; the core's `..` enters the index as `\.\.`.
    let (echoes, stderr) = repl(
        "regex «[0-9]+[kK]» := type ()\nprint «{(5k).type == type}»\n\
          regex «<=>» := type ()\nprint «{(<=>).type == type}»\n\
          regex «\\.\\.» := type ()\n",
    );
    assert_eq!(echoes, ["true", "true"], "stderr: {stderr}");
    assert!(stderr.contains("shadowed"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"regex \xc2\xab[a-z][0-9]\xc2\xbb := type ()\nregex \xc2\xaba[0-9]\xc2\xbb := type ()\na1\n\
          regex \xc2\xabb[0-9]\xc2\xbb := type (static lex_rank = 1)\n(b1).type == type\n",
    );
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
    assert!(stderr.contains("same lex_rank"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"regex \xc2\xab[unclosed\xc2\xbb := type ()\nregex 5\n\
          regex \xc2\xabz[0-9]\xc2\xbb := type (\nregex \xc2\xabz[0-9]\xc2\xbb := type ()\n(z1).type == type\n",
    );
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
    assert!(stderr.contains("does not compile"), "stderr: {stderr}");
    assert!(stderr.contains("must be followed by a"), "stderr: {stderr}");
}

#[test]
fn a_literal_spelling_is_declared_by_its_whole_text() {
    // `@` is declared and outranks the fresh-name pattern, so only the quote declares `@!`.
    for (src, out) in [
        ("lex «@!» := i32 5, @!", "5"),
        ("lex «@!» := i32 5, @!:name", "@!"),
        ("lex «~.» := i32 5, ~.:name", "~."),
        ("mut lex «@!» := i32 5, @! = 6, @!", "6"),
        ("f := fn () -> i32 (lex «@!» := i32 5, @!), f()", "5"),
        ("f := fn () -> i32 (lex «@!» := i32 5, @!), f.compile(), f()", "5"),
    ] {
        assert_eq!(line(src), out, "{src}");
    }
    for (src, expect) in [
        ("lex «..» := i32 5, 1", "`..` is already declared"),
        ("lex «@!» := i32 5, lex «@!» := i32 6, 1", "`@!` is already declared"),
        ("lex «12» := i32 5, 1", "`12` is already declared"),
        ("lex «» := i32 5, 1", "must not be empty"),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: {stderr}");
        assert!(stderr.contains(expect), "{src}: {stderr}");
    }
}

#[test]
fn a_pattern_whose_class_can_eat_its_own_literal_still_lexes() {
    let (echoes, stderr) = repl(
        "regex «[a-z]+ing» := type ()\nprint «{(running).type == type}»\n\
          print «{(ring).type == type}»\n\
          regex «[0-9]+(?:[0-9]k)?x» := type ()\n(12kx).type == type\n",
    );
    assert_eq!(echoes, ["true", "true", "true"], "stderr: {stderr}");
}

#[test]
fn a_quantifier_in_a_pattern_covers_its_whole_group_or_escape() {
    for src in [
        "regex «(a[0-9])+» := type (), (a1a2).type == type",
        "regex «(a\\\\.b)+» := type (), (a.ba.b).type == type",
        "regex «q\\\\.+» := type (), (q..).type == type",
        "regex «(a|b)+c» := type (), (abc).type == type",
        "regex «xb{0}c» := type (), (xc).type == type",
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(0), "{src}: {stderr}");
        assert_eq!(stdout, "true\n", "{src}: {stderr}");
    }
    for (src, name) in [
        ("regex «(a\\\\.b)+» := type (), (axb).type == type", "axb"),
        ("regex «x([a-z]+ing)?» := type (), (xab).type == type", "xab"),
    ] {
        let (_, _, stderr) = run_line(src);
        assert!(stderr.contains(&format!("unknown name `{name}`")), "{src}: {stderr}");
    }
}

#[test]
fn a_pattern_is_read_as_the_regex_crate_reads_it() {
    for src in [
        "regex «k\\\\x41» := type (), (kA).type == type",
        "regex «k\\\\u{41}» := type (), (kA).type == type",
        "regex «(?i)abc» := type (), (ABC).type == type",
        "regex «k(?-u:[a-z])» := type (), (kq).type == type",
        "regex «(ab|cd|ef|gh|ij){5}» := type (), (ijijijijij).type == type",
        "regex «(?:a+)?b» := type (), (b).type == type",
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(0), "{src}: {stderr}");
        assert_eq!(stdout, "true\n", "{src}: {stderr}");
    }
    for (src, name) in [
        ("regex «k\\\\x41» := type (), (kx41).type == type", "kx41"),
        ("regex «k\\\\t» := type (), (kt).type == type", "kt"),
        ("regex «k\\\\n» := type (), (kn).type == type", "kn"),
    ] {
        let (_, _, stderr) = run_line(src);
        assert!(stderr.contains(&format!("unknown name `{name}`")), "{src}: {stderr}");
    }
    // A rolled-back pattern that can match nothing as a whole leaves no entry behind.
    let (echoes, stderr) =
        repl("regex «a*» := type ()\nregex «b*» := type (), zz\n(bb).type == type\n".as_bytes());
    assert!(echoes.is_empty(), "echoes: {echoes:?}");
    assert!(stderr.contains("unknown name `bb`"), "stderr: {stderr}");
}

#[test]
fn a_pattern_judges_a_word_boundary_by_the_char_before_it() {
    for src in [
        "regex «q\\B[0-9]+» := type (), (q1).type == type",
        "regex «q-\\b[0-9]+» := type (), (q-1).type == type",
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(0), "{src}: {stderr}");
        assert_eq!(stdout, "true\n", "{src}: {stderr}");
    }
    let (_, _, stderr) = run_line("regex «q\\b[0-9]+» := type (), (q1).type == type");
    assert!(stderr.contains("unknown name `q1`"), "{stderr}");
}

#[test]
fn a_pattern_part_that_matches_nothing_still_lets_it_lex() {
    for src in [
        "regex «^ab» := type (), (ab).type == type",
        "regex «\\bfoo» := type (), (foo).type == type",
        "regex «(?:[0-9]*)x» := type (), (x).type == type",
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(0), "{src}: {stderr}");
        assert_eq!(stdout, "true\n", "{src}: {stderr}");
    }
    // A pattern that can match nothing as a whole lexes only where it matches something.
    let (echoes, stderr) = repl(
        "regex «a*» := type ()\nprint «{(aaa).type == type}»\nb := 3\nprint «{b}»\n\
         regex «x{0,3}» := type ()\n(xx).type == type\n",
    );
    assert_eq!(echoes, ["true", "3", "true"], "stderr: {stderr}");
}

#[test]
fn spellings_ending_at_one_node_are_each_found_by_their_own_key() {
    // A rolled-back declaration leaves nothing behind, though another spelling ends where it did.
    for (program, gone, kept) in [
        ("regex «x|ab» := type ()\nregex «ab|y» := type (), zz\n(y).type == type\n", "y", "ab"),
        ("regex «a*» := type ()\nregex «b*» := type (), zz\n(bb).type == type\n", "bb", "aa"),
    ] {
        let (echoes, stderr) = repl(format!("{program}({kept}).type == type\n").as_bytes());
        assert_eq!(echoes, ["true"], "{program}: stderr: {stderr}");
        assert!(stderr.contains(&format!("unknown name `{gone}`")), "{program}: stderr: {stderr}");
    }
    let (_, _, stderr) =
        run_line("regex «x|ab» := type (), regex «ab|y» := type (), regex «ab|y» := type (), 5");
    assert!(stderr.contains("`ab|y` is already declared"), "stderr: {stderr}");
    let (_, _, stderr) = run_line("regex «x|ab» := type (), regex «ab|y» := type (), ab");
    assert!(stderr.contains("same lex_rank"), "stderr: {stderr}");
}

#[test]
fn a_constructor_written_in_logos_runs_during_the_parse() {
    let out = logos().arg("import file «tests/fixtures/squared.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "92\n");
    // A parse that consumes nothing sets its flag and stands as itself.
    let (echoes, stderr) = repl(
        "noop := type (static parse = ( tape.is_constructed[0] = true ))\n\
          t := noop\nprint «{t.type == type}»\nnoop.parse_rank\n",
    );
    assert_eq!(echoes, ["true", "91.0"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"bad := type (static parse = ( tape[5] = i32 1 ))\nx := bad\n");
    assert!(stderr.contains("constructor failed"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"named := type (static parse = ( tape[0] = tape.spelling[0], tape.is_constructed[0] = true ))\n\
          f := fn () -> void ( named )\n5\n",
    );
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
}

#[test]
fn a_binding_carries_its_spelling() {
    let (echoes, stderr) = repl("x := 5\nprint «{x:name}»\nif:name\n");
    assert_eq!(echoes, ["x", "if"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"a := i32 1\n(a + 1):name\n");
    assert!(stderr.contains("expected a field name"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"sp := type (static parse = ( tape[0] = tape[-1]:name, tape.is_constructed[0] = true, tape.remove(-1) ))\n\
          x := 1\nf := fn () -> void ( x sp )\n5\n",
    );
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
}

#[test]
fn the_slot_words_are_names_only_inside_a_type_body() {
    // Outside a type body they are ordinary spellings; inside, the body's own
    // shadow an outer name and are gone again at the close.
    let (echoes, stderr) = repl(
        "run := 5\nprint «{run + 1}»\nparse := i32 2\nprint «{parse * 3}»\n\
          m := type (a := i32 ?, output_type := type ?, static run = ( a + a ), \
          static parse_rank = *.parse_rank + 1, \
          static parse = ( tape[0].type = m, tape[0].a = tape[-1], tape[0].output_type = i32, \
          tape.is_constructed[0] = true, tape.remove(-1) ))\n\
          x := i32 3\nprint «{x m}»\nprint «{run}»\nparse_rank := 4\nparse_rank\n",
    );
    assert_eq!(echoes, ["6", "6", "6", "5", "4"], "stderr: {stderr}");
}

#[test]
fn a_slot_body_is_read_bare() {
    // The hidden `tape` is declared as a parameter is, so an outer `tape` is the shadowing error.
    let (echoes, stderr) = repl(
        b"sp := type (static parse = ( tape[0] = tape[-1]:name, tape.is_constructed[0] = true, tape.remove(-1) ))\n\
          x := 1\nf := fn () -> void ( x sp )\n5\n",
    );
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"tape := 1\nsp := type (static parse = ( tape.recenter(0) ))\n");
    assert!(stderr.contains("shadowed"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"minus := type (a := i32 ?, b := i32 ?, output_type := type ?, static run = ( a - b ), \
          static parse_rank = +.parse_rank, static associativity = left, \
          static parse = ( tape[0].type = minus, tape[0].a = tape[-1], tape[0].b = tape[1], tape[0].output_type = i32, \
          tape.is_constructed[0] = true, tape.remove(1), tape.remove(-1) ))\n\
          print \xc2\xab{7 minus 2}\xc2\xbb\nprint \xc2\xab{10 minus 2 minus 3}\xc2\xbb\n\
          f := fn (x := i32 ?) -> i32 ( x minus 1 )\nf.compile()\nprint \xc2\xab{f(9)}\xc2\xbb\nthis\n",
    );
    assert_eq!(echoes, ["5", "5", "8"], "stderr: {stderr}");
    assert!(stderr.contains("unknown name `this`"), "stderr: {stderr}");
    // A body constructed in the session's section reaches the core names, so a
    // run that calls a function whose body reads an operator constructs.
    let (echoes, stderr) = repl(
        b"sq := fn (a := i32 ?) -> i32 ( a * a )\n\
          squared := type (a := i32 ?, output_type := type ?, static run = ( sq(a) ), \
          static parse_rank = *.parse_rank + 1, \
          static parse = ( tape[0].type = squared, tape[0].a = tape[-1], tape[0].output_type = i32, \
          tape.is_constructed[0] = true, tape.remove(-1) ))\n\
          x := i32 4\nx squared\n",
    );
    assert_eq!(echoes, ["16"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"t := type (a := i32 ?, static run = ( s := \xc2\xaba ) b\xc2\xbb, \
          # \xc2\xab ) \xc2\xbb (( x[0] ), 5 )))\n\
          t.type == type\n",
    );
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
    let out = logos()
        .args(["t := type (static run = ( x[0], # c ) d\n5 )),\n\
                t.type == type"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn prose_may_stand_before_a_field_a_member_or_a_slot_fill() {
    let src = "t := type (\n\
               # a field\n\
               x := i32 ?,\n\
               # « a member »\n\
               static y := i32 2,\n\
               # a slot\n\
               static parse_rank = 5\n\
               ), q := t (4), q.x + t.y + i32 lossy (t.parse_rank)";
    let (code, stdout, stderr) = run_line(src);
    assert_eq!((code, stdout.as_str()), (Some(0), "11\n"), "stderr: {stderr}");
}

#[test]
fn a_type_body_refuses_what_is_not_its_own() {
    for (src, expect) in [
        (&b"t := type (parse_rank := 5)\n"[..], "shadowed"),
        (b"g := type (static)\n", "followed by a declaration"),
        (b"f := fn (static a := i32 ?) -> void ( a )\n", "never on a parameter"),
        (b"static parse_rank = 3\n", "unknown name"),
        (b"t := type (share y := 3)\n", "unknown name `share`"),
        (b"d := i32 5\nfree = 3\n", "a line of the type body itself"),
        (b"t := type (static parse = ( parse_rank = 3 ))\n", "a line of the type body itself"),
        (b"t := type (static parse = ( static parse_rank = 3 ))\n", "followed by a declaration"),
        (b"t := type (lex_rank = 5)\n", "its fill says so"),
        (b"t := type (parse = ( 1 ))\n", "its fill says so"),
        ("t := type (# «note» parse = ( 1 ))\n".as_bytes(), "its fill says so"),
        (b"x := 1\nt := type (x = 5)\n", "a type body line"),
        (b"t := type (fields = (a := i32 ?))\n", "unknown name `fields`"),
        (b"t := type (static associativity = 5)\n", "`left` or `right`"),
        (b"t := type (static free = 5)\n", "`static free = (…)`"),
        (b"t := type (static drop = ( 0 ))\n", "`drop` names no slot"),
        (b"t := type (static parse = ( tape[0].a = tape[-1] ))\n", "no field `a`"),
        (b"t := type (a := i32 ?, static parse = ( tape[0].b = tape[-1] ))\n", "no field `b`"),
        (
            b"t := type (a := i32 ?, static parse = ( tape.is_constructed[0] = 5 ))\n",
            "takes a bool",
        ),
        (b"p := type (y := i32 ?, static y := 3)\n", "shadowed"),
        (b"p := type (static y := 3, y := i32 ?)\n", "shadowed"),
        (b"t := type (static := i32 ?)\n", "followed by a declaration"),
        (b"t := type (5)\n", "a type body line"),
        (b"t := type (static associativity = 5)\n", "`left` or `right`"),
        (
            b"t := type (static parse = fn (mut a := i32 ?) -> void ( a = 1 ))\n",
            "`static parse = (…)`",
        ),
        (b"t := type (static parse = 5)\n", "`static parse = (…)`"),
        (b"t := type (static run = 5)\n", "`static run = (…)`"),
        (b"t := type (static run = ( 5 ))\nt(1)\n", "the call form of a type"),
        (b"t := type (static run = ( 5 ),\n", "never closed"),
    ] {
        let (_echoes, stderr) = repl(src);
        assert!(stderr.contains(expect), "{}: stderr: {stderr}", String::from_utf8_lossy(src));
    }
}

#[test]
fn an_and_group_of_non_booleans_is_data_not_a_condition() {
    let (_echoes, stderr) = repl(b"x := i32 1\ny := i32 2\nif (x and y) (1) else (2)\n");
    assert!(stderr.contains("must be a bool"), "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"x := i32 1\nx and true\n");
    assert!(stderr.contains("must be bools"), "stderr: {stderr}");
}

#[test]
fn two_spellings_of_a_pointer_type_are_one_type() {
    // Plain pointer types are interned through the pointee's record, one node per pointee.
    let (echoes, stderr) = repl("x := @i32\ny := @i32\nprint «{x == y}»\nz := @@i32\nz == @@i32\n");
    assert_eq!(echoes, ["true", "true"], "stderr: {stderr}");
}

#[test]
fn an_or_group_of_non_booleans_is_data_like_the_and_group() {
    let (_echoes, stderr) = repl(b"x := i32 1\ny := i32 2\nif (x or y) (1) else (2)\n");
    assert!(stderr.contains("must be a bool"), "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"x := i32 1\nx or true\n");
    assert!(stderr.contains("must be bools"), "stderr: {stderr}");
}

#[test]
fn every_operator_applied_to_a_group_applies_to_each_member() {
    let sq = "sq := fn (x := i32 ?) -> i32 ( x * x ), ";
    let rec = "p := type (v := i32 ?), a := p(2), b := p(3), ";
    for (line, want) in [
        ("t := f32, t == (f64 or f32)".to_string(), "true"),
        ("t := f32, t == (f64 and f32)".into(), "false"),
        ("t := i32, not (t == (f64 or f32 or rational_number))".into(), "true"),
        ("x := i32 3, x == (i32 3 or i32 4)".into(), "true"),
        ("x := i32 5, (i32 3 or i32 4) == x".into(), "false"),
        ("x := i32 2, y := i32 2, (x and y) + 1 == 3".into(), "true"),
        ("x := i32 1, y := i32 2, (x and y) + 1 == 3".into(), "false"),
        ("x := i32 1, y := i32 2, (x and y) + 1 == (i32 2 or i32 3)".into(), "true"),
        ("x := i32 3, -(x or x) == -3".into(), "true"),
        ("x := i32 1, (x and x and x).type == i32".into(), "true"),
        ("x := i32 1, t := f32, (x and t).type == i32".into(), "false"),
        (format!("{sq}sq(i32 2 or i32 3) == 4"), "true"),
        (format!("{sq}sq(i32 2 and i32 3) == 4"), "false"),
        ("x := i32 3, i64(x or x) == i64 3".into(), "true"),
        (format!("{rec}(a or b).v == 3"), "true"),
        (format!("{rec}(a and b).v == 3"), "false"),
        (
            "f := fn (x := i32 ?) -> bool ( x == (i32 3 or i32 4) ), f.compile(), f(4)".into(),
            "true",
        ),
        (
            "f := fn (x := i32 ?) -> bool ( not ((x and i32 3) + 1 == 4) ), f.compile(), f(3)"
                .into(),
            "false",
        ),
        (
            "f := fn (x := i32 ?) -> bool ( (x and i32 3).type == i32 ), f.compile(), f(1)".into(),
            "true",
        ),
        (
            format!("{sq}f := fn (x := i32 ?) -> bool ( sq(x or i32 3) == 9 ), f.compile(), f(1)"),
            "true",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&line);
        assert_eq!((code, stdout.trim()), (Some(0), want), "{line}: stderr: {stderr}");
    }
    // A group held as data has nothing to run, compiled or not.
    let (code, _, stderr) =
        run_line("n := fn (x := i32 ?) -> i32 ( y := x and x, 1 ), n.compile(), n(3)");
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("cannot be compiled yet"), "stderr: {stderr}");
}

#[test]
fn a_collection_member_demands_its_index_brackets() {
    let (_echoes, stderr) = repl(b"+.roles(0)\n");
    assert!(stderr.contains("element access is `[…]`"), "stderr: {stderr}");
}

#[test]
fn a_square_bracket_is_a_paren_that_closes_only_itself() {
    let (echoes, stderr) = repl(b"x := i32 5\ns := here.scope\ns.dyads[1 - 1].rhs\n");
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"(1]\n");
    assert!(stderr.contains("never closed"), "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"+.roles[0)\n");
    assert!(stderr.contains("never closed"), "stderr: {stderr}");
    // Held or skipped, a bracket ends where the built one does: at the stray closer.
    for (src, stray) in [
        ("t := type ( a := i32 ?, static run = ( print «a» ] print «b» ) )", "] print"),
        ("t := type ( a := i32 ?, static run = ( k := [1 ), print «b» ) )", "), print"),
        ("f := fn () -> i32 ( t := type ( a := i32 ? ] ), 1 )", "] )"),
        ("f := fn () -> i32 ( t := type ( static k := immediate [1 ) ), 1 )", ") ), 1"),
        ("if false ( 1 ] 2 ) else ( 3 )", "] 2"),
        ("if false ( k := [1 ) ) else ( 3 )", ") ) else"),
        ("if true ( 3 ) else ( 1 ] 2 )", "] 2"),
        ("if true ( 3 ) else ( k := [1 ) )", ") )"),
        ("if true ( 3 ) else if ( 1 ] == 1 ) ( 4 )", "] =="),
        ("if true ( 3 ) else if ( [1 ) == 1 ) ( 4 )", ") =="),
    ] {
        let (echoes, stderr) = repl(format!("{src}\n").as_bytes());
        let col =
            src[..src.find(stray).expect("the stray closer is in the line")].chars().count() + 1;
        let head = format!("<repl>:1:{col}: error: this bracket is never closed");
        assert!(echoes.is_empty() && stderr.contains(&head), "{src}: {echoes:?} {stderr}");
    }
}

#[test]
fn a_held_or_skipped_bracket_is_refused_where_it_is_written_before_anything_runs() {
    let bump = |body: &str, tail: &str| {
        format!(
            "bump := type (a := i32 ?, static run = ( {body} ), static parse_rank = *.parse_rank + 1, \
             static parse = ( tape[0].type = bump, tape[0].a = tape[-1], tape.is_constructed[0] = true, \
             tape.remove(-1) )), {tail}"
        )
    };
    let held_type = "print «a», t := type ( static k := immediate [1 ) ), print «b»";
    for (src, stray) in [
        (bump("print «a» ] print «b»", "3 bump"), "] print"),
        (bump("print «a», k := [1 ), print «b»", "3 bump"), "), print"),
        (bump("print «a», k := [1 ), print «b»", "x := 3 bump, x"), "), print"),
        (bump("print «a», k := [1 ), print «b»", "5"), "), print"),
        (bump("x := ( 1 ], print «b»", "3 bump"), "], print"),
        (bump(held_type, "3 bump"), ") ), print"),
        (bump(held_type, "5"), ") ), print"),
        ("f := fn () -> i32 ( t := type ( static k := immediate [1 ) ), 1 ), 5".into(), ") ), 1"),
        ("if false ( k := [1 ) ) else ( print «ok» )".into(), ") ) else"),
        ("if true ( print «ok» ) else ( k := [1 ) )".into(), ") )"),
        ("f := fn () -> i32 ( if false ( k := [1 ) ) else ( 7 ) ), f()".into(), ") ) else"),
        (
            "f := fn () -> i32 ( if false ( k := [1 ) ) else ( 7 ) ), f.compile(), f()".into(),
            ") ) else",
        ),
        (
            "f := fn () -> i32 ( if true ( k := [1 ) ) else ( 7 ) ), f.compile(), f()".into(),
            ") ) else",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stdout.is_empty(), "{src}: stdout: {stdout}");
        let col =
            src[..src.find(stray).expect("the stray closer is in the program")].chars().count() + 1;
        let head = format!("<command line>:1:{col}: error: this bracket is never closed");
        assert!(stderr.starts_with(&head), "{src}: stderr: {stderr}");
    }
    let nested = bump("x := ((a + 2) * (3)), print «{x}»", "3 bump");
    let (code, stdout, stderr) =
        run_line(&format!("{nested}, if false ( j := zz [(1), [2 (3)]] ) else ( print «ok» )"));
    assert_eq!((code, stdout.as_str()), (Some(0), "15\nok\n"), "stderr: {stderr}");
}

#[test]
fn a_constructor_tells_a_square_bracket_cell_from_a_scope_by_its_type() {
    let probe = "probe := type ( static parse_rank = *.parse_rank + 1, static parse = ( \
                 if (tape[1].type == square_brackets) (print «brackets») else (print «other»), \
                 if (tape[1].type != square_brackets) (print «not brackets»), \
                 tape.remove(1), tape.remove(0) ) )";
    let src = format!(
        "{probe}, probe [1, 2], probe (3), x := i32 4, probe x, square_brackets.type == type"
    );
    let out = logos().args([&src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "brackets\nother\nnot brackets\nother\nnot brackets\ntrue\n"
    );
}

#[test]
fn a_constructor_reads_a_square_bracket_cell_by_its_dyads() {
    let first = "first := type ( static parse_rank = *.parse_rank + 1, static associativity = left, static parse = ( \
                 tape[0] = tape[1].dyads[0], tape.is_constructed[0] = true, tape.remove(1) ) )";
    let count = "count := type ( static parse_rank = *.parse_rank + 1, static parse = ( \
                 if (tape[1].dyads.size == 2) (print «two») else (print «not two»), \
                 tape.remove(1), tape.remove(0) ) )";
    let src = format!("{first}, {count}, count [1, 2], count [3], first [3]");
    let out = logos().args([&src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "two\nnot two\n3\n");
}

#[test]
fn a_logos_constructor_at_discovery_lexes_the_tape_on_demand() {
    // At discovery the read lexes `i32` and it arrives unbuilt; at the boundary the loop built it first.
    for (rank, read) in
        [("90", "type"), ("92", "type"), ("99", "type"), ("*.parse_rank + 1", "i32")]
    {
        let src = format!(
            "r := type ( static parse_rank = {rank}, static parse = ( print «{{tape[1].type == {read}}}», \
             tape.remove(0) ) ), r i32 5"
        );
        let out = logos().args([&src]).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n5\n", "{src}");
    }
    // A name arrives as lexed, a bracket as its scope cell, and what follows is left unlexed.
    let src = "x := i32 4, r := type ( static parse_rank = fn.parse_rank, static parse = ( \
               print «{tape.is_constructed[1]} {tape[2].type == scope} {tape[2].dyads.size}», \
               tape.remove(2), tape.remove(1), tape.remove(0) ) ), r x (1, 2), 7";
    let out = logos().args([src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "false true 2\n7\n");
    // A constructor the read lexed is not woken by it: it runs in its turn if the reader
    // leaves it, and `lex` still works after it.
    let s = "s := type ( static parse_rank = fn.parse_rank, static parse = ( tape[0] = i32 3, \
             tape.is_constructed[0] = true ) )";
    for (r, printed) in [
        ("print «{tape[1].type == type}», tape.remove(0)", "true\n3\n"),
        ("tape.remove(1), tape.remove(0), tape.insert(0, lex «6»)", "6\n"),
    ] {
        let src = format!(
            "{s}, r := type ( static parse_rank = fn.parse_rank, static parse = ( {r} ) ), r s"
        );
        let out = logos().args([&src]).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), printed, "{src}");
    }
    // Past the end of the source, or past a boundary, the read is the constructed `void`.
    for (tail, printed) in
        [("r", "true\n"), ("r i32 5, 1", "true\n1\n"), ("(r i32 5), 1", "true\n1\n")]
    {
        let src = format!(
            "r := type ( static parse_rank = fn.parse_rank, static parse = ( print «{{tape[3].type == void}}», \
             tape.remove(2), tape.remove(1), tape.remove(0) ) ), {tail}"
        );
        let out = logos().args([&src]).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), printed, "{src}");
    }
}

#[test]
fn a_read_where_the_tape_reaches_no_cell_is_a_constructed_void() {
    let r = "r := type ( static parse_rank = fn.parse_rank, static parse = ( \
             print «{tape[1].type == void} {tape.is_constructed[1]} {tape[-1].type == void}», \
             tape.remove(0) ) )";
    for (tail, printed) in [
        ("r", "true true true\n"),
        ("r, 1", "true true true\n1\n"),
        ("(r), 1", "true true true\n1\n"),
        ("x := i32 (r 2)", "false true true\n"),
    ] {
        let out = logos().arg(format!("{r}, {tail}")).output().unwrap();
        assert!(out.status.success(), "{tail}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), printed, "{tail}");
    }
    // Written into a cell, it is a finished expression that yields nothing.
    let w = "w := type ( static parse_rank = fn.parse_rank, static parse = ( tape[0] = tape[1], \
             tape.is_constructed[0] = true ) )";
    let out = logos().arg(format!("{w}, (w), 4")).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "4\n");
    let out = logos().arg(format!("{w}, x := (w), 4")).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("gives nothing"), "stderr: {stderr}");
    // A write is still the checked error.
    let out = logos()
        .arg("v := type ( static parse_rank = fn.parse_rank, static parse = ( tape[1] = i32 3 ) ), v")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("this index is off the tape"), "stderr: {stderr}");
}

#[test]
fn a_cell_a_constructor_reads_from_the_tape_arrives_unbuilt() {
    // `i32` is the type itself, not a conversion of the bracket, which stays for the reader.
    let src = "c := type ( static parse_rank = fn.parse_rank, static parse = ( \
               print «{tape[1].type == type} {tape[2].type == scope} {tape[2].dyads.size}», \
               tape.remove(2), tape.remove(1), tape.remove(0) ) ), c i32 (1, 2, 3), 7";
    let out = logos().args([src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true true 3\n7\n");
    // Left on the tape, `i32` takes its bracket in its own turn.
    let src = "c := type ( static parse_rank = fn.parse_rank, static parse = ( tape.remove(0) ) ), c i32 (5)";
    let out = logos().args([src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}

#[test]
fn inclusion_between_integer_types_follows_their_value_ranges() {
    let (echoes, stderr) = repl(
        "print «{u8 ⊆ u16}»\nprint «{u8 ⊆ i16}»\nprint «{i8 ⊆ u64}»\nprint «{u16 ⊆ i16}»\n\
         print «{u64 ⊆ i64}»\nprint «{i8 ⊆ i64}»\nprint «{i32 ⊆ i32}»\nprint «{bool ⊆ bool}»\n\
         print «{not u8 ⊆ i8}»\nx := u8 3\nnot x.type ⊆ u16\n",
    );
    assert_eq!(
        echoes,
        ["true", "true", "false", "false", "false", "true", "true", "true", "true", "false"],
        "stderr: {stderr}"
    );
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn an_integer_lies_inside_a_float_that_holds_every_value_of_it() {
    let (echoes, stderr) = repl(
        "print «{i32 ⊆ f64}»\nprint «{i32 ⊆ f32}»\nprint «{u16 ⊆ f32}»\nprint «{u32 ⊆ f64}»\n\
         print «{i64 ⊆ f64}»\nprint «{f32 ⊆ f64}»\nprint «{f64 ⊆ f32}»\nprint «{f64 ⊆ i64}»\n\
         print «{bool ⊆ i32}»\ni32 ⊆ bool\n",
    );
    assert_eq!(
        echoes,
        ["true", "false", "true", "true", "false", "true", "false", "false", "false", "false"],
        "stderr: {stderr}"
    );
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_line_checked_against_a_float_reads_as_that_float() {
    let q = "q := type ( static parse_rank = 60, static parse = ( \
             if (not tape[1].type ⊆ f64) (error «not within f64»), \
             print «{tape[1] + 0.5}», tape.remove(1), tape[0] = i32 1, tape.is_constructed[0] = true ) )";
    let out = logos().args([&format!("{q}, q (i32 3)")]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3.5\n1\n");
    let out = logos().args([&format!("{q}, q (i64 3)")]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && stderr.contains("not within f64"), "stderr: {stderr}");
}

#[test]
fn lossy_gives_what_each_number_type_defines_on_both_tiers() {
    let huge = "f64 4000000000.0 * 4000000000.0 * 4000000000.0 * 4000000000.0 * 4000000000.0";
    for (ty, from, value, expect) in [
        ("i32", "f64", "f64 2.5", "2"),
        ("f32", "i32", "i32 16777217", "16777216.0"),
        ("i32", "i64", "i64 5000000000", "705032704"),
        ("i32", "f64", "f64 3000000000", "-1294967296"),
        ("f32", "f64", huge, "inf"),
        ("i32", "i32", "i32 7", "7"),
    ] {
        for compile in ["", "g.compile(), "] {
            let src =
                format!("g := fn (x := {from} ?) -> {ty} ( {ty} lossy (x) ), {compile}g({value})");
            let (code, stdout, stderr) = run_line(&src);
            assert_eq!((code, stdout.trim()), (Some(0), expect), "{src}: stderr: {stderr}");
        }
    }
    for (src, expect) in
        [("f64 lossy 0.1", "0.1"), ("i32 lossy -2.5", "-2"), ("u8 lossy 300", "44")]
    {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!((code, stdout.trim()), (Some(0), expect), "{src}: stderr: {stderr}");
    }
    for compile in ["", "g.compile(), "] {
        for x in ["f64 1.0 / 0.0", "f64 -1.0 / 0.0", "f64 0.0 / 0.0"] {
            let src = format!("g := fn (x := f64 ?) -> i32 ( i32 lossy (x) ), {compile}g({x})");
            let (code, _, stderr) = run_line(&src);
            assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
            assert!(stderr.contains("has no whole number"), "{src}: stderr: {stderr}");
        }
    }
}

#[test]
fn a_value_lands_unwritten_only_in_a_type_that_holds_every_value_of_it() {
    let pt = "pt := type ( a := i64 ?, output_type := type ?, static run = ( a + a ), \
              static parse_rank = *.parse_rank + 1, static parse = ( tape[0].type = pt, \
              tape[0].a = tape[-1], tape[0].output_type = i64, tape.is_constructed[0] = true, \
              tape.remove(-1) ) )";
    for compile in ["", "f.compile(), "] {
        for (src, printed) in [
            ("f := fn () -> f64 ( i32 3 ), {c}f()", "3.0"),
            ("f := fn () -> f64 ( u32 4000000000 ), {c}f()", "4000000000.0"),
            ("f := fn () -> i64 ( mut x := i64 0, y := i32 -9, x = y, x ), {c}f()", "-9"),
            ("f := fn (a := f64 ?) -> f64 ( a ), {c}f(i32 3)", "3.0"),
            ("f := fn (b := i32 ?) -> i64 ( if (b < 1) (return i32 4), i64 2 ), {c}f(0)", "4"),
            ("f := fn () -> f64 ( mut a := f64 0.0, p := &a, p@ = i32 5, a ), {c}f()", "5.0"),
            ("f := fn () -> i64 ( x := i32 3, x pt ), {c}f()", "6"),
        ] {
            let src = format!("{pt}, {}", src.replace("{c}", compile));
            let out = logos().arg(&src).output().unwrap();
            assert!(out.status.success(), "{src}: {}", String::from_utf8_lossy(&out.stderr));
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), printed, "{src}");
        }
    }
    for (src, printed) in [
        ("rec := type ( a := i64 ?, b := f64 ? ), v := rec (i32 3, i32 4), v.b", "4.0"),
        ("m := hashmap i64 -> i64 ?, m[i32 3] = i16 4, m[3]", "4"),
        ("x := i32 3, x pt", "6"),
    ] {
        let out = logos().arg(format!("{pt}, {src}")).output().unwrap();
        assert!(out.status.success(), "{src}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), printed, "{src}");
    }
    for src in [
        "f := fn () -> i32 ( f64 2.5 ), f()",
        "f := fn () -> i32 ( true ), f()",
        "f := fn () -> @i32 ( i32 5 ), f()",
        "f := fn () -> f32 ( i32 3 ), f()",
        "mut x := i32 0, y := i64 9, x = y",
        "mut b := bool ?, b = i32 1",
        "f := fn (a := i64 ?) -> i64 ( a ), f(f64 3.0)",
        "mut a := i32 0, p := &a, p@ = i64 5",
        "rec := type ( a := i64 ?, b := f64 ? ), v := rec (i32 3, i64 4)",
        "m := hashmap i32 -> i32 ?, m[i64 3] = 4",
        "x := f64 3.5, x pt",
        "g := fn (b := dyad ?) -> i32 ( mut x := i32 0, x = b, x ), g(f64 2.5)",
        "g := fn (b := dyad ?) -> i32 ( h := fn (a := i32 ?) -> i32 ( a ), h(b) ), g(i64 5)",
        "g := fn (b := dyad ?) -> i64 ( b pt ), g(f64 3.0)",
        "mut d := dyad ?, d = i32, mut x := i32 0, x = d",
        "mut d := dyad ?, d = i32, f := fn () -> i32 ( d ), f()",
        "g := fn (q := dyad ?) -> i32 ( q ), g(i32)",
    ] {
        let out = logos().arg(format!("{pt}, {src}")).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("these types do not match"), "{src}: stderr: {stderr}");
    }
}

#[test]
fn an_if_lands_each_arm_and_an_operation_meets_in_the_wider_type() {
    for compile in ["", "f.compile(), "] {
        for (src, printed) in [
            (
                "f := fn (c := bool ?) -> i64 ( mut x := i64 0, x = if c (i32 1) else (i64 2), x ), \
                 {c}f(true)",
                "1",
            ),
            ("f := fn (c := bool ?) -> f64 ( if c (1) else (f64 2.5) ), {c}f(true)", "1.0"),
            ("g := fn (a := i64 ?) -> i64 ( a ), f := fn (c := bool ?) -> i64 ( g(if c (i32 1) else (i64 2)) ), {c}f(false)", "2"),
            (
                "f := fn (c := i32 ?) -> i64 ( if c == 1 (i32 1) else if c == 2 (i16 7) else (i64 3) ), \
                 {c}f(2)",
                "7",
            ),
            ("f := fn (a := i32 ?, b := i64 ?) -> i64 ( a * b ), {c}f(3, 4)", "12"),
            ("f := fn (a := f64 ?, b := i32 ?) -> f64 ( a + b ), {c}f(1.5, 1)", "2.5"),
            ("f := fn (a := u8 ?, b := i32 ?) -> bool ( a < b ), {c}f(1, 2)", "true"),
            ("f := fn () -> i64 ( mut s := i64 0, for i in (i32 0)..(i64 3) ( s = s + i ), s ), {c}f()", "3"),
        ] {
            let src = src.replace("{c}", compile);
            let out = logos().arg(&src).output().unwrap();
            assert!(out.status.success(), "{src}: {}", String::from_utf8_lossy(&out.stderr));
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), printed, "{src}");
        }
    }
    for (src, expect) in [
        (
            "c := true, x := if c (i32 1) else (i64 2)",
            "the arms of this `if`, or the lines that give this block its value, give different types",
        ),
        ("c := true, mut x := i64 0, x = if c (i32 1) else (f64 2.0)", "these types do not match"),
        ("f32 1.5 + i32 1", "these types do not match"),
        ("x := u64 1, y := i64 1, x + y", "these types do not match"),
    ] {
        let out = logos().arg(src).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_dyad_place_takes_every_node_and_a_number_in_a_new_node() {
    let array = "import file «./identities/array.logos»";
    for tail in
        ["mut d := dyad ?, d = array i32 [1, 2]", "a := array i32 [1, 2], mut d := dyad ?, d = a"]
    {
        let out = logos().arg(format!("{array}, {tail}, print «taken»")).output().unwrap();
        assert!(out.status.success(), "{tail}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "taken\n", "{tail}");
    }
    for c in ["", "f.compile(), "] {
        for (src, want) in [
            ("mut d := dyad ?, d = 5, d".to_string(), "5"),
            ("mut d := dyad ?, d = i64 5, d = f64 2.5, d".to_string(), "2.5"),
            ("mut d := dyad ?, d = 1/2, d * 4".to_string(), "2"),
            ("mut d := dyad ?, d = i64 5, d + 1".to_string(), "6"),
            ("mut d := dyad ?, d = i64 5, d = d * 2, d".to_string(), "10"),
            ("mut d := dyad ?, d = true, d".to_string(), "true"),
            ("mut d := dyad ?, d = i64 5, e := d, d = i64 6, e".to_string(), "5"),
            (format!("f := fn (q := dyad ?) -> i32 ( 1 ), {c}f(5)"), "1"),
            (format!("f := fn () -> dyad ( i32 5 ), {c}f()"), "5"),
            (format!("f := fn (q := dyad ?) -> dyad ( q ), {c}f(i32 5)"), "5"),
            (format!("f := fn (q := dyad ?) -> dyad ( q ), {c}x := f(i64 7), x"), "7"),
            (format!("f := fn () -> i32 ( mut d := dyad ?, d = i64 5, d = 1/2, 1 ), {c}f()"), "1"),
            ("mut d := dyad ?, d = «a», d".to_string(), "a"),
            (format!("f := fn () -> i32 ( mut d := dyad ?, d = «a», 1 ), {c}f()"), "1"),
            ("mut d := dyad ?, d = alloc 1 of i32 5, d@".to_string(), "5"),
            (format!("f := fn (q := dyad ?) -> i32 ( 1 ), {c}f(alloc 1 of i32 5)"), "1"),
        ] {
            let out = logos().arg(&src).output().unwrap();
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(out.status.success(), "{src}: stderr: {stderr}");
            assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{want}\n"), "{src}");
        }
    }
    // A number in a place no name holds.
    for src in [
        "f := fn (mut q := dyad ?) -> i32 ( 1 ), f(5)",
        "p := alloc 1 of dyad ?, p@ = 5",
        "r := type ( v := dyad ? ), x := r (5)",
    ] {
        let out = logos().arg(src).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("does not take this value yet"), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_dyad_place_ends_the_value_it_holds_and_copies_it_deep() {
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) ), ";
    for c in ["", "f.compile(), "] {
        for (tail, want) in [
            ("mut d := dyad ?, d = bag (1)".to_string(), "freed 1\n"),
            (
                "mut d := dyad ?, d = bag (1), d = bag (2), print «end»".to_string(),
                "freed 1\nend\nfreed 2\n",
            ),
            ("mut d := dyad ?, d = bag (1), e := d, print «end»".to_string(), "end\nfreed 1\nfreed 1\n"),
            ("b := bag (7), mut d := dyad ?, d = b, print «end»".to_string(), "end\nfreed 7\nfreed 7\n"),
            ("d := dyad (bag, bag (6)), print «end»".to_string(), "end\nfreed 6\n"),
            ("f := fn () -> dyad ( bag (9) ), mut y := bag ?, y = f(), print «end»".to_string(), "freed 0\nend\nfreed 9\n"),
            (format!("f := fn (a := dyad ?) -> i32 ( 1 ), {c}f(bag (3))"), "freed 3\n1\n"),
            (
                format!("f := fn (a := dyad ?) -> dyad ( a ), {c}x := f(bag (3)), print «end»"),
                "freed 3\nend\nfreed 3\n",
            ),
            (format!("f := fn () -> dyad ( bag (9) ), {c}x := f(), print «end»"), "end\nfreed 9\n"),
            (
                format!("f := fn () -> dyad ( b := bag (9), b ), {c}x := f(), print «end»"),
                "end\nfreed 9\n",
            ),
            (
                format!("g := fn (q := bag ?) -> i32 ( print «g», q.n ), f := fn () -> dyad ( bag (9) ), {c}g(f())"),
                "g\nfreed 9\n9\n",
            ),
            (
                format!(
                    "g := fn () -> dyad ( print «g», i32 2 ), f := fn () -> dyad ( print «f», \
                     bag (4) ), k := fn (a := bag ?, b := i32 ?) -> i32 ( b ), {c}k(f(), g() + 1)"
                ),
                "f\ng\nfreed 4\n3\n",
            ),
            // A parameter borrows a value a name holds: the call frees the node it lands in, and
            // the name ends the value.
            (
                format!(
                    "b := bag (4), f := fn (a := dyad ?) -> i32 ( 1 ), {c}free (f(b)), print «end»"
                ),
                "end\nfreed 4\n",
            ),
            (
                format!(
                    "mut d := dyad ?, d = bag (5), f := fn (a := dyad ?) -> i32 ( 1 ), {c}free (f(d)), \
                     print «end»"
                ),
                "end\nfreed 5\n",
            ),
            (
                format!(
                    "f := fn (a := dyad ?) -> i32 ( 1 ), {c}g := fn (a := dyad ?) -> i32 ( f(a) ), \
                     free (g(bag (5))), print «end»"
                ),
                "freed 5\nend\n",
            ),
            (
                format!(
                    "b := bag (4), f := fn (a := dyad ?) -> dyad ( a ), {c}x := f(b), print «end»"
                ),
                "end\nfreed 4\nfreed 4\n",
            ),
        ] {
            let src = format!("{bag}{tail}");
            let out = logos().arg(&src).output().unwrap();
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(out.status.success(), "{src}: stderr: {stderr}");
            assert_eq!(String::from_utf8_lossy(&out.stdout), want, "{src}");
        }
    }
}

#[test]
fn an_if_that_makes_a_dyad_in_one_arm_and_borrows_one_in_another_is_refused() {
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) ), ";
    let head =
        format!("{bag}mk := fn () -> dyad ( bag (6) ), mut d := dyad ?, d = bag (5), c := i32 1, ");
    // Refused wherever the value goes, as over `bag`: at a `free`'s operand, and where a `:=`
    // closes the `if`.
    for (tail, at) in [
        ("free (if (c == 1) (dyad (bag, bag (6))) else (d)), print «end»", "(if"),
        ("free (if (c == 1) (mk()) else (d)), print «end»", "(if"),
        ("x := if (c == 1) (dyad (bag, bag (6))) else (d), print «end»", ", print"),
        ("x := if (c == 1) (d) else (mk()), print «end»", ", print"),
        ("x := if (c == 1) (dyad (i32, 6)) else (d), print «end»", ", print"),
        ("a := bag (5), x := if (c == 1) (bag (6)) else (a), print «end»", ", print"),
    ] {
        let src = format!("{head}{tail}");
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        let col = src[..head.len() + tail.find(at).unwrap()].chars().count() + 1;
        let want = format!(
            "<command line>:1:{col}: error: make a value in every arm, or borrow in every arm"
        );
        assert!(stderr.starts_with(&want), "{src}: stderr: {stderr}");
    }
    // Arms alike hand the value on: each bag is freed once.
    for (tail, want) in [
        (
            "free (if (c == 1) (dyad (bag, bag (6))) else (move d)), print «end»",
            "freed 5\nfreed 6\nend\n",
        ),
        (
            "x := if (c == 1) (dyad (bag, bag (6))) else (move d), print «end»",
            "freed 5\nend\nfreed 6\n",
        ),
        (
            "x := if (c == 1) (mk()) else (dyad (bag, bag (7))), print «end»",
            "end\nfreed 6\nfreed 5\n",
        ),
        (
            "mut e := dyad ?, e = bag (7), x := if (c == 1) (d) else (e), print «end»",
            "end\nfreed 5\nfreed 7\nfreed 5\n",
        ),
    ] {
        let src = format!("{head}{tail}");
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, want, "{src}");
    }
    // A type's `run` whose output is `dyad` gives a new node its caller holds, whatever it made.
    let gives_dyad = |run: &str| {
        format!(
            "t := type ( lhs := i32 ?, static output_type := dyad, static run = ( {run} ), \
             static parse_rank = 60, static parse = ( tape[0].type = t, tape[0].lhs = tape[1], \
             tape.is_constructed[0] = true, tape.remove(1) ) ), \
             f := fn (m := i32 ?) -> i32 ( y := t m, 1 ), "
        )
    };
    // A function's value lands in its result: a `dyad` read through into `-> i32` hands on a
    // number, and into `-> bag` the bag its node held, which only `-> own bag` may.
    for c in ["", "f.compile(), "] {
        for (tail, want) in [
            (
                format!("{}{c}free (f(i32 5)), print «end»", gives_dyad("bag (lhs)")),
                "freed 5\nend\n",
            ),
            (
                format!("{}{c}free (f(i32 5)), print «end»", gives_dyad("dyad (bag, bag (lhs))")),
                "freed 5\nend\n",
            ),
            (format!("f := fn (m := i32 ?) -> i32 ( dyad (i32, m) ), {c}f(i32 3)"), "3\n"),
            (format!("f := fn (m := i32 ?) -> i32 ( return dyad (i32, m) ), {c}f(i32 3)"), "3\n"),
            (
                format!("f := fn () -> own bag ( dyad (bag, bag (6)) ), {c}x := f(), print «end»"),
                "end\nfreed 6\n",
            ),
            (
                format!(
                    "f := fn () -> own bag ( return dyad (bag, bag (6)) ), {c}x := f(), print «end»"
                ),
                "end\nfreed 6\n",
            ),
        ] {
            let src = format!("{bag}{tail}");
            let (code, stdout, stderr) = run_line(&src);
            assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
            assert_eq!(stdout, want, "{src}");
        }
    }
    for tail in [
        "f := fn () -> bag ( dyad (bag, bag (6)) ), x := f(), print «end»",
        "f := fn () -> bag ( return dyad (bag, bag (6)) ), x := f(), print «end»",
    ] {
        let src = format!("{bag}{tail}");
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains("hands on a value it made: write `-> own bag`"), "{src}: {stderr}");
    }
}

#[test]
fn a_parameter_moves_nothing_whose_type_ends_its_value_a_dyad_as_a_record() {
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) ), ";
    // A type whose values a Logos `parse` builds, with or without a `free` of its own.
    let node = |free: &str| {
        format!(
            "nd := type ( mut s := u64 0, static parse_rank = dyad.parse_rank, \
             static parse = ( tape.is_constructed[0] = true ){free} ), \
             f := fn (q := nd ?) -> u64 ( c := move q, c.s ), "
        )
    };
    let lend = "mut b := nd ?, b.s = u64 4, f(b)";
    for c in ["", "f.compile(), "] {
        // The call holds what its parameter takes, or borrows it, so the parameter has nothing to
        // move: a `dyad ?` one as a `bag ?` one, whatever the argument.
        for tail in [
            format!("f := fn (a := dyad ?) -> void ( x := move a ), {c}f(i64 5), print «end»"),
            format!(
                "f := fn (a := dyad ?) -> void ( b := move a ), {c}mut d := dyad ?, d = i64 5, \
                 f(d), d"
            ),
            format!("f := fn (a := dyad ?) -> void ( x := move a ), {c}f(bag (3)), print «end»"),
            format!(
                "f := fn (a := dyad ?) -> void ( x := move a ), {c}b := bag (4), f(b), print «end»"
            ),
            format!("f := fn (mut a := dyad ?) -> void ( x := move a ), {c}f(i64 5), print «end»"),
            format!("f := fn (a := bag ?) -> void ( x := move a ), {c}f(bag (3)), print «end»"),
            format!("{}{c}{lend}", node(", static free = ( print «freed» )")),
        ] {
            let src = format!("{bag}{tail}");
            let out = logos().arg(&src).output().unwrap();
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                !out.status.success() && stderr.contains("only its owner can"),
                "{src}: stderr: {stderr}"
            );
        }
        // A value its type ends with its bytes moves as a copy; a name that holds its node moves
        // it.
        for (tail, want) in [
            (format!("f := fn (a := i32 ?) -> i32 ( x := move a, x ), {c}f(i32 5)"), "5\n"),
            (
                format!("f := fn (a := @i32 ?) -> i32 ( x := move a, x@ ), {c}y := i32 6, f(&y)"),
                "6\n",
            ),
            (format!("{}{c}{lend}", node("")), "4\n"),
            (
                format!(
                    "f := fn () -> i32 ( mut d := dyad ?, d = bag (7), e := move d, 1 ), {c}f()"
                ),
                "freed 7\n1\n",
            ),
            (
                "mut d := dyad ?, d = bag (7), e := move d, print «end»".to_string(),
                "end\nfreed 7\n",
            ),
        ] {
            let src = format!("{bag}{tail}");
            let out = logos().arg(&src).output().unwrap();
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(out.status.success(), "{src}: stderr: {stderr}");
            assert_eq!(String::from_utf8_lossy(&out.stdout), want, "{src}");
        }
    }
}

#[test]
fn a_plain_number_is_read_as_the_rational_it_is_whatever_holds_it() {
    let q = |ty: &str| {
        format!(
            "q := type ( v := {ty} ?, static parse_rank = 61, static parse = ( tape[0].type = q, \
             tape[0].v = tape[1].dyads[0], tape.remove(1), tape.is_constructed[0] = true ) )"
        )
    };
    let mut rows = vec![
        ("mut d := dyad ?, d = 1/3, d".to_string(), "1/3\n"),
        ("r := rational_number 1/3, mut d := dyad ?, d = r, d".to_string(), "1/3\n"),
        ("r := 1/3, d := dyad (rational_number, r), d".to_string(), "1/3\n"),
        ("d := dyad (rational_number, 1/3), d".to_string(), "1/3\n"),
        ("r := 1/2, 0".to_string(), "0\n"),
        ("1/2".to_string(), "1/2\n"),
        ("r := 1/2, r".to_string(), "1/2\n"),
        ("print «{1/2}»".to_string(), "1/2\n"),
        ("if (true) 1/2 else 1/3".to_string(), "1/2\n"),
        ("10000000000".to_string(), "10000000000\n"),
        ("r := 5, r + 3000000000".to_string(), "3000000005\n"),
        ("for 0..2 ( x := 5, y := move x, print «{y}» )".to_string(), "5\n5\n"),
        (format!("{}, x := q [10000000000, 7], x.v", q("i64")), "10000000000\n"),
    ];
    for c in ["", "f.compile(), "] {
        rows.extend([
            (format!("f := fn () -> rational_number ( r := 1/2, r ), {c}f()"), "1/2\n"),
            (format!("f := fn () -> rational_number ( if (true) 1/2 else 1/3 ), {c}f()"), "1/2\n"),
            (format!("f := fn () -> void ( print «{{1/2}}» ), {c}f()"), "1/2\n"),
            (format!("f := fn () -> i64 ( 10000000000 ), {c}f()"), "10000000000\n"),
            (format!("f := fn () -> i32 ( r := 1/2, 1 ), {c}f()"), "1\n"),
            (format!("f := fn () -> i32 ( defer free (1/2), 1 ), {c}f()"), "1\n"),
            (format!("f := fn () -> dyad ( mut d := dyad ?, d = 1/3, d ), {c}f()"), "1/3\n"),
            (
                format!(
                    "f := fn (r := rational_number ?) -> dyad ( mut d := dyad ?, d = r, d ), \
                     {c}f(1/3)"
                ),
                "1/3\n",
            ),
        ]);
    }
    for (src, printed) in rows {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{src}: {stderr}");
    }
    // A block's plain number is a `rational_number` too, which no `i32` holds whole.
    for (src, expect) in [
        ("x := ( free (1), 7 ) + i32 1".to_string(), "do not match"),
        (format!("{}, x := q [1/2, 7], x.v", q("i32")), "only a constant is read as its number"),
        (
            format!("{}, x := q [(free (1), 7), 7], x.v", q("i32")),
            "only a constant is read as its number",
        ),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_line_that_reads_a_dyad_name_in_a_body_is_parsed_when_it_runs() {
    for (src, want) in [
        ("f := fn (a := dyad ?) -> i64 ( a ), f(i64 42)", "42"),
        ("f := fn (a := dyad ?) -> i64 ( a * 2 ), f(i64 21)", "42"),
        ("g := fn (a := dyad ?) -> bool ( a.type == i32 ), g(i32 5)", "true"),
        ("g := fn (a := dyad ?) -> bool ( a.type == i32 ), g(i64 5)", "false"),
        ("f := fn (a := dyad ?) -> i64 ( a ), f(42)", "42"),
        ("f := fn (a := dyad ?) -> bool ( a == i32 ), f(i32)", "true"),
        ("f := fn (a := dyad ?) -> bool ( a == i32 ), f(f64)", "false"),
        ("f := fn (a := dyad ?) -> bool ( a != i32 ), f(f64)", "true"),
        ("x := i32 4, g := fn (a := dyad ?) -> bool ( a.type == i32 ), g(x + 1)", "true"),
        (
            "mut n := i32 0, h := fn () -> i64 ( n = n + 1, i64 7 ), \
             f := fn (a := dyad ?) -> i64 ( a ), x := f(h()), n",
            "1",
        ),
        ("mut d := dyad ?, d = i64 5, for 0..3 ( d = d + 1 ), d", "8"),
        ("f := fn (a := dyad ?) -> i64 ( mut d := dyad ?, d = a, d = d * 3, d ), f(i64 2)", "6"),
        ("g := fn () -> dyad ( i64 6 ), f := fn () -> i64 ( g() ), f()", "6"),
        (
            "f := fn (a := dyad ?) -> i64 ( if a > 0 ( f(i64 (a - 1)) + 1 ) else ( 0 ) ), \
             f(i64 4)",
            "4",
        ),
        (
            "f := fn (a := dyad ?) -> i64 ( mut s := i64 0, for 0..3 ( s = s + a ), s ), f(i64 5)",
            "15",
        ),
        ("f := fn (a := dyad ?) -> dyad ( a + 1 ), f(i64 1)", "2"),
        (
            "mut d := dyad ?, d = i64 1, d = f64 2.5, f := fn (a := dyad ?) -> f64 ( a ), f(d)",
            "2.5",
        ),
        // A compiled caller of a function whose lines wait.
        (
            "f := fn (a := dyad ?) -> i64 ( a ), g := fn () -> i64 ( f(i64 3) + f(42) ), \
             g.compile(), g()",
            "45",
        ),
    ] {
        let out = logos().arg(src).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{src}: stderr: {stderr}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{want}\n"), "{src}");
    }
    for (src, refusal) in [
        ("f := fn (a := dyad ?) -> i64 ( a ), f(f64 2.5)", "these types do not match"),
        ("f := fn (a := dyad ?) -> i64 ( a ), f(1/2)", "no exact value"),
        ("f := fn (a := dyad ?) -> i64 ( a + «x» ), f(i64 1)", "cannot compute"),
        ("f := fn (a := dyad ?) -> i64 ( x := a + 1, x ), f(i64 2)", "cannot hold `:=` yet"),
        (
            "f := fn (a := dyad ?) -> i64 ( if a > 0 ( return 1 ), 2 ), f(i64 2)",
            "cannot hold `return` yet",
        ),
        ("f := fn (a := dyad ?) -> i64 ( a ), f.compile(), f(i64 42)", "cannot be compiled yet"),
    ] {
        let out = logos().arg(src).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && stderr.contains(refusal), "{src}: stderr: {stderr}");
    }
    let (echoes, stderr) =
        repl("f := fn (a := dyad ?) -> i64 ( a * 2 )\nprint «{f(i64 21)}»\nf(4)\n");
    assert_eq!(echoes, ["42", "8"], "stderr: {stderr}");
}

#[test]
fn a_line_that_reads_a_dyad_name_is_parsed_where_it_is_written_up_to_the_name() {
    for (src, want) in [
        // `h()` is built where the line is written and runs once each time the line runs.
        (
            "mut n := i64 0, h := fn () -> i64 ( n = n + 1, n ), \
             f := fn (a := dyad ?) -> i64 ( h() * 10 + a ), x := f(i64 2), y := f(i64 3), \
             x + y + n",
            "37",
        ),
        ("f := fn (a := dyad ?) -> i64 ( mut s := i64 0, s = (i64 3 + 1) * a, s ), f(i64 2)", "8"),
        // A `while` condition that reads a `dyad` name reads the node it holds on each pass.
        ("mut d := dyad ?, d = i64 0, while d < 3 ( d = d + 1 ), d", "3"),
        ("mut d := dyad ?, d = i64 0, while # «a» d < 3 ( d = f64 9.5 ), d", "9.5"),
        ("mut d := dyad ?, d = i64 0, while (d < 3) ( d = d + 1 ), d", "3"),
        (
            "f := fn (a := dyad ?) -> i64 ( mut d := dyad ?, d = a, while d < 3 ( d = d + 1 ), d ), \
             f(i64 0)",
            "3",
        ),
        // A member named `type` is no `type (…)`, so what the line reads after it is seen.
        ("h := fn (x := i32 ?, a := dyad ?) -> bool ( x.type == a.type ), h(i32 1, i32 2)", "true"),
        ("h := fn (x := i32 ?, a := dyad ?) -> bool ( x.type == a.type ), h(i32 1, f64 2.0)", "false"),
        (
            "f := fn () -> dyad ( i32 5 ), h := fn (x := i32 ?) -> bool ( x.type == f().type ), \
             h(i32 1)",
            "true",
        ),
    ] {
        let out = logos().arg(src).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{src}: stderr: {stderr}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{want}\n"), "{src}");
    }
    // A name nothing declares, before the read or on a loop body's other line, is reported
    // where the function is written, though it is never called.
    for src in [
        "f := fn (a := dyad ?) -> i64 ( undefinedname + a ), 5",
        "f := fn (a := dyad ?) -> i64 ( mut s := i64 0, for 0..3 ( s = s + a, nope ), s ), 5",
    ] {
        let out = logos().arg(src).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && stderr.contains("unknown name"), "{src}: {stderr}");
        assert!(!stderr.contains("held line"), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_line_of_a_body_written_in_a_type_waits_for_the_run() {
    let t = "t := type ( mut n := i64 ?, static k := fn (a := dyad ?) -> i64 ( a + n ), \
             static is_i32 := fn (a := dyad ?) -> bool ( a.type == i32 ), \
             static last := fn (a := dyad ?) -> i64 ( a ), \
             static add := fn (a := dyad ?) -> void ( n = a + n ), \
             static bump := fn () -> i64 ( n = n + 1, n ), \
             static add_bump := fn (a := dyad ?) -> i64 ( a + bump() ) )";
    // A bare field of the `static` function's value is read through that value when the line runs.
    for (tail, want) in [
        ("v := t (3), v.k(i64 5)", "8"),
        ("v := t (3), w := t (10), v.k(i64 5) + w.k(i64 5)", "23"),
        ("v := t (3), v.is_i32(i32 5)", "true"),
        ("v := t (3), v.is_i32(f64 2.5)", "false"),
        ("v := t (3), v.last(i64 42)", "42"),
        ("mut v := t (3), v.add(i64 5), v.n", "8"),
        ("mut v := t (3), free (v.add_bump(i64 5)), v.n", "4"),
        ("v := t (3), g := fn () -> i64 ( v.k(i64 5) ), g.compile(), g()", "8"),
    ] {
        let src = format!("{t}, {tail}");
        assert_eq!(line(&src), want, "{src}");
    }
    // A field written in a held line, there or in a bare call, is a write of the value the
    // function is called on, which must be `mut`, checked where the call is written.
    for tail in ["v := t (3), v.add(i64 5)", "v := t (3), v.add_bump(i64 5)"] {
        let src = format!("{t}, {tail}");
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains("`v` is not `mut`"), "{src}: stderr: {stderr}");
        assert!(!stderr.contains("held line"), "{src}: stderr: {stderr}");
    }
    let (code, _, stderr) = run_line(&format!("{t}, v := t (3), v.last(f64 2.5)"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("these types do not match"), "stderr: {stderr}");
    // A type's `run` body: a bare call there works on the node's own value.
    let inc = "inc := type ( a := i32 ?, output_type := type ?, \
               static twice := fn () -> i32 ( a * 2 ), static run = ( g() + twice() + a ), \
               static parse_rank = *.parse_rank + 1, static parse = ( tape[0].type = inc, \
               tape[0].a = tape[-1], tape[0].output_type = i32, tape.is_constructed[0] = true, \
               tape.remove(-1) ) )";
    let src =
        format!("g := fn () -> dyad ( i32 1 ), {inc}, f := fn (y := i32 ?) -> i32 ( y inc ), f(5)");
    assert_eq!(line(&src), "16", "{src}");
    // A type's `parse` body: a held line runs when the parse does, and a bare field in one is
    // refused where it is written, as on any line of a parse.
    let parse = |line: &str| {
        format!(
            "mut d := dyad ?, d = i32 1, inc := type ( a := i32 ?, output_type := type ?, \
             static run = ( a ), static parse_rank = *.parse_rank + 1, static parse = ( \
             tape[0].type = inc, tape[0].a = tape[-1], {line}, tape.is_constructed[0] = true, \
             tape.remove(-1) ) )"
        )
    };
    let src = format!(
        "{}, f := fn (y := i32 ?) -> i32 ( y inc ), f(5)",
        parse("tape[0].output_type = d.type")
    );
    assert_eq!(line(&src), "5", "{src}");
    let src = format!("{}, i32 5 inc", parse("tape[0].output_type = i32, d + a"));
    let (code, _, stderr) = run_line(&src);
    assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
    assert!(stderr.contains("never bare") && !stderr.contains("held line"), "stderr: {stderr}");
    // A held stamp's type is known only when the parse runs, so the parse's later lines read the
    // node's fields by name, not as fields of the type being defined.
    let src = "w := type ( v := i32 ?, static output_type := i32, static run = ( v + v ) ), \
               mut d := dyad ?, d = w, q := type ( static parse_rank = 60, static parse = ( \
               tape[0].type = d, tape[0].v = tape[1], tape.is_constructed[0] = true, \
               tape.remove(1) ) ), x := i32 4, print «{q x}»";
    assert_eq!(line(src), "8", "{src}");
    // A type's `free` body reads its value's fields bare, as a `static` function does.
    let src = "mut total := i64 0, g := fn () -> dyad ( i64 1 ), \
               t := type ( n := i64 ?, static free = ( total = total + g() + n ) ), \
               h := fn () -> void ( x := t (3) ), h(), total";
    assert_eq!(line(src), "4", "{src}");
}

#[test]
fn a_held_line_checks_the_gates_of_what_it_writes_where_it_is_written() {
    // The place left of `=` stands before the read, so it is read and its gates are checked
    // where the line is written, though the function is never called.
    for (src, name) in [
        ("f := fn (a := dyad ?) -> i64 ( x := i64 3, x = a + x, x ), 0", "x"),
        ("f := fn (a := dyad ?) -> i64 ( x := i64 3, x = undefinedname + a, x ), 0", "x"),
        ("t := type ( n := i64 ?, static add := fn (a := dyad ?) -> void ( n = a + n ) ), 0", "n"),
        (
            "p := type ( mut x := i64 ? ), \
             f := fn (a := dyad ?) -> i64 ( q := p (1), q.x = a + q.x, q.x ), 0",
            "q",
        ),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(&format!("`{name}` is not `mut`")), "{src}: stderr: {stderr}");
        assert!(!stderr.contains("held line"), "{src}: stderr: {stderr}");
    }
    // The place read there is the one each run writes, in a line held inside another's bracket
    // too, whose parse where it is written stops at its cut as the outer line's does.
    for (src, want) in [
        ("f := fn (a := dyad ?) -> i64 ( if a > 0 ( i64 1 + a ) else ( i64 2 ) ), f(i64 5)", "6"),
        ("f := fn (a := dyad ?) -> i64 ( mut x := i64 3, x = a + x, x ), f(i64 5) + f(i64 1)", "12"),
        (
            "t := type ( mut n := i64 ?, static add := fn (a := dyad ?) -> void ( n = a + n ) ), \
             mut v := t (3), v.add(i64 5), v.add(i64 1), v.n",
            "9",
        ),
        (
            "p := type ( mut x := i64 ? ), \
             f := fn (a := dyad ?) -> i64 ( mut q := p (1), q.x = a + q.x, q.x ), f(i64 5)",
            "6",
        ),
        ("f := fn (a := dyad ?) -> i64 ( mut d := dyad ?, d = a, d ), f(i64 7)", "7"),
        (
            "f := fn (a := dyad ?) -> i64 ( mut x := i64 3, if a > 0 ( x = a + x ) else ( x = x ), \
             x ), f(i64 5)",
            "8",
        ),
        ("f := fn (a := dyad ?) -> i64 ( mut x := i64 3, while x < a ( x = a + x ), x ), f(i64 5)", "8"),
    ] {
        assert_eq!(line(src), want, "{src}");
    }
}

#[test]
fn a_held_line_is_the_first_write_of_a_valueless_place_and_meets_its_read_veto_where_written() {
    let pt = "pt := type ( mut x := i32 ?, mut y := i32 ? ), ";
    for (src, want) in [
        ("f := fn (a := dyad ?) -> i32 ( mut u := i32 ?, u = a, u ), f(i32 5)".to_string(), "5"),
        ("f := fn (a := dyad ?) -> i32 ( mut u := i32 ?, u = a + 1, u ), f(i32 5)".into(), "6"),
        ("f := fn (d := dyad ?) -> i32 ( mut u := type ?, u = d, 1 ), f(i32)".into(), "1"),
        (
            "f := fn (a := dyad ?) -> i32 ( mut u := i32 ?, mut w := i32 ?, u = a + i32 1, w = u, \
             w ), f(i32 5)"
                .into(),
            "6",
        ),
        (
            "f := fn (a := dyad ?) -> i32 ( mut u := i32 ?, u = a, u ), \
             g := fn () -> i32 ( f(i32 5) ), g.compile(), g()"
                .into(),
            "5",
        ),
        (
            format!(
                "{pt}f := fn (a := dyad ?) -> i32 ( mut v := pt ?, v.x = a, v.y = i32 2, v.x + v.y \
                 ), f(i32 5)"
            ),
            "7",
        ),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
    // A read in a held line, the part left to the run included, meets the veto where the line is
    // written: a write after it in text order is no write before it, and a write nested in a
    // group or an `if` arm fills nothing after it.
    for (src, at, name) in [
        ("f := fn (a := dyad ?) -> i32 ( mut u := i32 ?, u = a + u, u ), f(i32 5)".to_string(), "1:56", "u"),
        ("f := fn (a := dyad ?) -> i32 ( mut u := i32 ?, a + u, u = i32 1, u ), f(i32 5)".into(), "1:52", "u"),
        (
            "f := fn (a := dyad ?) -> void ( mut u := i32 ?, print «{a + u}», u = i32 1 ), f(i32 5)"
                .into(),
            "1:61",
            "u",
        ),
        ("f := fn (a := dyad ?) -> i32 ( mut u := i32 ?, if a > 0 ( u = a ), u ), f(i32 5)".into(), "1:68", "u"),
        ("f := fn (a := dyad ?) -> i32 ( mut u := i32 ?, (u = a), u ), f(i32 5)".into(), "1:57", "u"),
        (
            format!(
                "{pt}f := fn (a := dyad ?) -> i32 ( mut v := pt ?, v.x = a + v.x, v.y = i32 2, v.x \
                 ), f(i32 5)"
            ),
            "1:106",
            "v.x",
        ),
        (
            format!(
                "{pt}f := fn (a := dyad ?) -> i32 ( mut v := pt ?, print «{{a + v.x}}», v.x = i32 \
                 1, v.y = i32 2, v.x ), f(i32 5)"
            ),
            "1:108",
            "v.x",
        ),
        (format!("{pt}f := fn (a := dyad ?) -> i32 ( mut v := pt ?, v.x = a, v ), f(i32 5)"), "1:103", "v"),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        let want = format!("{at}: error: `{name}` is read before it is written");
        assert!(stderr.contains(&want), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_quote_part_of_a_body_that_reads_a_dyad_value_gives_it_when_it_runs() {
    for (src, want) in [
        ("f := fn (a := dyad ?) -> void ( print «{a + 1}» ), f(i64 5)", "6"),
        ("f := fn (a := dyad ?) -> void ( print «{i64 1 + a}» ), f(i64 5)", "6"),
        ("f := fn (a := dyad ?) -> void ( if a > 0 ( print «{a + 1}» ) ), f(i64 5)", "6"),
        (
            "f := fn (a := dyad ?) -> void ( print «x {a + 1} y {a * 2}» ), f(i64 5), f(f64 2.5)",
            "x 6 y 10\nx 3.5 y 5.0",
        ),
        ("f := fn (a := dyad ?) -> void ( print «{a > 3}» ), f(i64 5)", "true"),
        ("f := fn (a := dyad ?) -> void ( print «{a + 1/2}» ), f(1/3)", "5/6"),
        (
            "f := fn (a := dyad ?) -> void ( for i in 0..3 ( print «{a + i}» ) ), f(i64 5)",
            "5\n6\n7",
        ),
    ] {
        assert_eq!(line(src), want, "{src}");
    }
    let (code, _, stderr) =
        run_line("f := fn (a := dyad ?) -> void ( error «bad {a + 1}» ), f(i64 5)");
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("bad 6"), "stderr: {stderr}");
    // A part that gives nothing is refused when its line is parsed, as it runs.
    let src = "mut x := i64 0, f := fn (a := dyad ?) -> void ( print «{x = a + 1}» ), f(i64 5)";
    let (code, _, stderr) = run_line(src);
    assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
    assert!(stderr.contains("held line") && stderr.contains("gives nothing"), "stderr: {stderr}");
}

#[test]
fn inclusion_the_design_leaves_open_is_a_checked_error() {
    for src in ["i32 ⊆ type\n", "u8 ⊆ dyad\n", "bool ⊆ type\n"] {
        let (echoes, stderr) = repl(src.as_bytes());
        assert!(echoes.is_empty() && stderr.contains("not settled"), "{src}: stderr: {stderr}");
    }
    let (echoes, stderr) = repl("1 ⊆ 2\n".as_bytes());
    assert!(echoes.is_empty() && stderr.contains("cannot compute"), "stderr: {stderr}");
}

#[test]
fn a_parse_body_checks_that_an_index_type_fits_the_size_type() {
    let q = "q := type ( static size := u64 0, static parse_rank = 60, static parse = ( \
             if (not tape[1].type ⊆ tape[0].size.type) \
             (error «the index type is not within the size type»), \
             tape.remove(1), tape[0] = i32 1, tape.is_constructed[0] = true ) )";
    let out = logos().args([&format!("{q}, q (u32 3)")]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1");
    for (arg, expect) in
        [("i8 3", "not within the size type"), ("f64 3.0", "not within the size type")]
    {
        let out = logos().args([&format!("{q}, q ({arg})")]).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && stderr.contains(expect), "{arg}: stderr: {stderr}");
    }
}

#[test]
fn a_hashmap_is_read_and_written_by_key() {
    let (echoes, stderr) = repl(
        "m := hashmap i32 -> i64\n\
          m[3] = 40\n\
          m[-1] = 3000000000\n\
          print «{m[3] + 2}»\n\
          print «{m[-1]}»\n\
          x := i32 3\n\
          m[x] = m[x] * 2\n\
          print «{m[3]}»\n\
          f := fn (n := i64 ?) -> i64 ( w := hashmap u8 -> i64, w[1] = n, w[1] + w[1] )\n\
          print «{f(5)}»\n\
          print «{f(7)}»\n\
          print «{(hashmap i32 -> i64).type == m.type}»\n\
          (hashmap i32 -> i64).type == (hashmap i64 -> i32).type\n",
    );
    assert_eq!(echoes, ["42", "3000000000", "80", "10", "14", "true", "false"], "stderr: {stderr}");

    // A number cannot be the unknown, so a missing key of a number-valued map is the checked error.
    let (echoes, stderr) = repl(b"m := hashmap i32 -> i64\nm[1] = 1\nm[2]\nm[1]\n");
    assert_eq!(echoes, ["1"], "stderr: {stderr}");
    assert!(stderr.contains("the map holds no value at this key"), "stderr: {stderr}");
}

#[test]
fn a_hashmap_of_types_hands_back_the_unknown_for_a_missing_key() {
    let (echoes, stderr) = repl(
        "mints := hashmap type -> type\n\
          mints[i32] = f64\n\
          mut t := mints[i32]\n\
          print «{t == f64}»\n\
          t = mints[u8]\n\
          print «{t}»\n\
          mints[mints[i32]] = i8\n\
          mut u := mints[f64]\n\
          u == i8\n",
    );
    assert_eq!(echoes, ["true", "type ?", "true"], "stderr: {stderr}");
}

#[test]
fn a_declared_hashmap_is_empty_and_readable_at_once() {
    // Before any write, with or without `?`: no read veto, as `t := i32 ?` has.
    let (echoes, stderr) = repl(
        "m := hashmap type -> type\n\
          mut u := m[i32]\n\
          print «{u}»\n\
          n := hashmap i32 -> i64 ?\n\
          n[1]\n\
          n[1] = 2\n\
          print «{n[1]}»\n\
          f := fn (w := hashmap i32 -> i64 ?) -> i64 ( 1 )\n",
    );
    assert_eq!(echoes, ["type ?", "2"], "stderr: {stderr}");
    assert!(stderr.contains("the map holds no value at this key"), "stderr: {stderr}");
    assert!(!stderr.contains("<repl>:1:"), "no line is refused at parse: {stderr}");
}

#[test]
fn a_hashmap_holds_maps_as_copies_of_their_own() {
    let (echoes, stderr) = repl(
        "pm := hashmap type -> hashmap type -> type\n\
          mut row := hashmap type -> type\n\
          row[i64] = f32\n\
          pm[i32] = row\n\
          row[i64] = f64\n\
          mut back := pm[i32]\n\
          print «{back[i64] == f32}»\n\
          mut empty := hashmap type -> type\n\
          pm[u8] = empty\n\
          empty[i64] = f32\n\
          mut none := pm[u8]\n\
          print «{none[i64] == ?}»\n\
          keep := fn (l := type ?, r := type ?) -> i32 ( mut m := hashmap type -> type, m[r] = f64, pm[l] = m, 0 )\n\
          print «{keep(i16, i8)}»\n\
          mut kept := pm[i16]\n\
          print «{kept[i8] == f64}»\n\
          deep := hashmap type -> hashmap type -> hashmap type -> type\n\
          mut b := hashmap type -> hashmap type -> type\n\
          b[u16] = row\n\
          deep[u32] = b\n\
          mut b2 := deep[u32]\n\
          mut a2 := b2[u16]\n\
          a2[i64] == f64\n",
    );
    assert_eq!(echoes, ["true", "true", "0", "true", "true"], "stderr: {stderr}");
}

/// ›`:=` decides nothing about copy or reference; the type's reading rule does‹: a map named
/// by `:=` or written by `=` is a copy of its own, its maps copied too.
#[test]
fn a_map_is_copied_by_declaration_and_assignment() {
    let m = "m := hashmap i32 -> i32, m[1] = i32 5";
    let pm = "pm := hashmap i32 -> hashmap i32 -> i32, r := hashmap i32 -> i32, r[2] = i32 5, \
              pm[1] = r";
    assert_runs(&[
        (format!("{m}, mut n := m, n[1] = i32 6, m[1]"), "5\n"),
        (format!("{m}, mut n := m, n[1] = i32 6, n[1]"), "6\n"),
        (format!("{m}, mut n := hashmap i32 -> i32, n = m, n[1] = i32 6, m[1]"), "5\n"),
        (format!("{m}, mut n := hashmap i32 -> i32, n = m, n[1] = i32 6, n[1]"), "6\n"),
        (
            format!("{pm}, mut n := pm, mut x := n[1], x[2] = i32 7, n[1] = x, y := pm[1], y[2]"),
            "5\n",
        ),
    ]);
}

#[test]
fn a_maps_places_hold_the_unknown() {
    let memo = "pm := hashmap type -> hashmap type -> type, mut row := pm[i32]";
    for (src, want) in [
        (format!("{memo}, print «{{row == ?}} {{row != ?}} {{? == pm[i8]}}»"), "true false true"),
        ("mut r := hashmap type -> type, print «{r == ?} {r != ?}»".to_string(), "false true"),
        (
            format!(
                "{memo}, if row == ? ( row = hashmap type -> type ), row[i64] = f32, pm[i32] = row, \
                 mut back := pm[i32], print «{{back == ?}} {{back[i64] == f32}}»"
            ),
            "false true",
        ),
        (format!("{memo}, pm[u8] = row, mut kept := pm[u8], print «{{kept == ?}}»"), "true"),
        (
            "mut a := hashmap type -> type, a[i32] = f64, a = hashmap type -> type, \
             print «{a[i32] == ?}»"
                .to_string(),
            "true",
        ),
        (
            "mut n := i64 0, for 0..3 ( mut m := hashmap type -> type, if m[i8] == ? ( n = n + 1 ), \
             m[i8] = i8 ), print «{n}»"
                .to_string(),
            "3",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.trim()), (Some(0), want), "{src}: stderr: {stderr}");
    }
    for src in [format!("{memo}, row[i64] = f32"), format!("{memo}, row[i64] == ?")] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains("this map is the unknown `?`"), "{src}: stderr: {stderr}");
    }
    for src in ["a := hashmap i64 -> i64, a[3]", "x := i32 5, x == ?"] {
        let (code, _, _) = run_line(src);
        assert_eq!(code, Some(1), "{src}");
    }
}

#[test]
fn a_hashmap_checks_its_shape_and_its_key_and_value_types() {
    for line in [
        "hashmap i32 i32",
        "hashmap f64 -> i32",
        "hashmap i32 -> bool",
        "hashmap i32",
        "hashmap hashmap type -> type -> type",
        "hashmap type -> hashmap type -> f64",
    ] {
        let (_echoes, stderr) = repl(format!("{line}\n").as_bytes());
        assert!(stderr.contains("`hashmap` is followed by `K -> V`"), "{line}: {stderr}");
    }
    for (line, expect) in [
        ("m[i32] = 1", "these types do not match"),
        ("m[1] = i32", "these types do not match"),
        ("m[i64 1] = 1", "these types do not match"),
        ("n[1] = 2", "must be a type value"),
        ("n[i32] = 2", "must be a type value"),
    ] {
        let (_echoes, stderr) = repl(
            format!("m := hashmap i32 -> i32\nn := hashmap type -> type\n{line}\n").as_bytes(),
        );
        assert!(stderr.contains(expect), "{line}: {stderr}");
    }
}

#[test]
fn a_constructor_writes_its_ifs_without_brackets() {
    let probe = "probe := type ( static parse_rank = *.parse_rank + 1, static parse = (\n\
                 if tape[1].type == square_brackets print «brackets» else print «other»,\n\
                 if tape[1].type == scope (\n    print «scope»\n),\n\
                 if not tape[1].type == square_brackets\n    print «not brackets»,\n\
                 tape.remove(1), tape.remove(0) ) )";
    let src = format!("{probe}, probe [1, 2], probe (3, 4), x := i32 4, probe x");
    let out = logos().args([&src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "brackets\nother\nscope\nnot brackets\nother\nnot brackets\n"
    );
}

#[test]
fn an_inclusion_ends_a_bare_if_condition_as_a_comparison_does() {
    let src = "x := u8 3, if x.type ⊆ u16 print «inside», \
               if not x.type ⊆ i8\n    print «outside» else print «inside», 7";
    let out = logos().args([src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "inside\noutside\n7\n");
    let src =
        "y := i8 3, if not y.type ⊆ u64\n    error «the index type is not within the size type»";
    let out = logos().args([src]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success() && stderr.contains("not within the size type"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_condition_ending_in_a_type_is_bracketed_before_a_name_body() {
    let out = logos()
        .args(["mut y := i32 0, t := f64, if t == f64 y = 1 else y = 2, y"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "the type takes `y`: {}", String::from_utf8_lossy(&out.stdout));
    let src = "mut y := i32 0, t := f32, \
               if (t == f64) y = 1 else if (t == f32) y = 2 else y = 3, y";
    let out = logos().args([src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
}

#[test]
fn a_condition_ending_in_a_member_ends_before_its_body() {
    for src in [
        "x := i32 3, if i32 == x.type ( print «yes» ), 7",
        "if here.scope == here.scope ( print «yes» ), 7",
    ] {
        let out = logos().args([src]).output().unwrap();
        assert!(out.status.success(), "{src}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\n7\n", "{src}");
    }
}

#[test]
fn a_quote_shows_a_name_read_through_a_path() {
    let src = "g := ( a := i32 1, b := a + 1, b ), s := g:start.rhs, \
               print «{s.dyads[0].lhs:name} and {s.dyads[1].rhs.lhs:name}», \
               error «no {s.dyads[0].lhs:name}»";
    let out = logos().args([src]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a and a\n");
    assert!(String::from_utf8_lossy(&out.stderr).contains("no a"), "{out:?}");
}

#[test]
fn a_binding_has_no_field_type() {
    let (echoes, stderr) = repl(b"x := i32 5\nx:type\n");
    assert!(echoes.is_empty() && stderr.contains("`type` is not in scope"), "stderr: {stderr}");
    let (code, _, stderr) =
        run_line("f := fn (x := i32 ?) -> bool ( x:type == i32 ), f.compile(), f(1)");
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("`type` is not in scope"), "stderr: {stderr}");
}

#[test]
fn a_value_reads_its_type_with_a_dot_on_both_tiers() {
    let sq = postfix_chooser("sq", "t", "a * a");
    let pt = "pt := type ( x := i32 ? ), g := fn (x := i32 ?) -> pt ( pt(x) )";
    for (src, want) in [
        ("x := i32 3, x.type == i32".to_string(), "true"),
        ("f := fn (x := i32 ?) -> bool ( x.type == i32 ), f.compile(), f(1)".into(), "true"),
        // An operator expression stands for its node: `.type` is the node's own type, its
        // operator's mint for its operand types, and what it gives is the mint's `output_type`.
        ("x := i32 3, (x + i32 1).type == i32".into(), "false"),
        ("x := i32 3, (x + i32 1).output_type == i32".into(), "true"),
        ("x := i32 3, (x + i32 1).type == (x + i32 2).type".into(), "true"),
        ("x := i32 3, y := i64 4, (x + i32 1).type == (y + i64 1).type".into(), "false"),
        (
            "f := fn (x := i32 ?) -> bool ( (x + i32 1).type == (x + i32 2).type ), \
             f.compile(), f(3)"
                .into(),
            "true",
        ),
        (
            "f := fn (x := i32 ?, y := i64 ?) -> bool ( (x + i32 1).type == (y + i64 1).type ), \
             f.compile(), f(3, 4)"
                .into(),
            "false",
        ),
        (
            "f := fn (x := f64 ?) -> bool ( (x + 1.0).output_type == f64 ), f.compile(), f(1.0)"
                .into(),
            "true",
        ),
        // A call stands for what it returns.
        (format!("{pt}, g(5).type == pt"), "true"),
        (format!("{pt}, (g(5)).type == pt"), "true"),
        (format!("{pt}, g.compile(), g(5).type == pt"), "true"),
        // The tape operand's type picks the node's output type.
        (format!("{sq}, x := f64 1.5, x sq"), "2.25"),
        (format!("{sq}, f := fn (x := f64 ?) -> f64 ( x sq ), f.compile(), f(1.5)"), "2.25"),
        (format!("{sq}, x := i32 3, (x sq).output_type == i32"), "true"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.trim()), (Some(0), want), "{src}: {stderr}");
    }
}

#[test]
fn a_name_s_dyad_reads_its_type_and_its_bytes_address_on_both_tiers() {
    let pt = "pt := type ( a := i32 ?, b := i32 ? )";
    let inc = "f := fn (a := i32 ?) -> i32 ( a + 1 )";
    for (src, want) in [
        // A name laid out in a frame: its type from its binding, its value the address of its
        // bytes, an `@void`.
        ("x := i32 5, x:dyad.type == i32".to_string(), "true"),
        ("x := i32 5, x:dyad.type == x.type".into(), "true"),
        ("x := i32 5, x:dyad.value == &x".into(), "true"),
        ("x := i32 5, y := i32 5, x:dyad.value == y:dyad.value".into(), "false"),
        ("x := i32 5, x:dyad.value.type == @void".into(), "true"),
        ("f := fn (a := i32 ?) -> bool ( a:dyad.type == i32 ), f(3)".into(), "true"),
        ("f := fn (a := i32 ?) -> bool ( a:dyad.type == i32 ), f.compile(), f(3)".into(), "true"),
        ("f := fn (a := i32 ?) -> bool ( a:dyad.value == &a ), f(3)".into(), "true"),
        ("f := fn (a := i32 ?) -> bool ( a:dyad.value == &a ), f.compile(), f(3)".into(), "true"),
        (
            format!(
                "{pt}, f := fn (q := pt ?) -> bool ( q:dyad.value == &q ), f.compile(), f(pt(1, 2))"
            ),
            "true",
        ),
        // A name of a type or a function, and a node reached by path, have their node.
        (format!("{pt}, pt:dyad.type == type"), "true"),
        (format!("{inc}, f:dyad.type == f.type"), "true"),
        (format!("{inc}, f:dyad.value.type == @void"), "true"),
        (format!("{inc}, d := f:dyad, d.type == f.type"), "true"),
        ("x := i32 5, b := x + 1, b:start.rhs:dyad.type == b:start.rhs.type".into(), "true"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.trim()), (Some(0), want), "{src}: {stderr}");
    }
    for (src, error) in [
        ("x := i32 5, d := x:dyad", "has no node yet"),
        ("f := fn (a := i32 ?) -> i32 ( d := a:dyad, a ), f.compile(), f(1)", "has no node yet"),
        ("x := i32 5, x:value", "unknown name `value`"),
        ("f := fn (a := i32 ?) -> i32 ( a:dyad.value@ ), f.compile(), f(1)", "cannot compute"),
        ("x := i32 5, x:dyad.lhs", "does not fit"),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: {stderr}");
        assert!(stderr.contains(error), "{src}: {stderr}");
    }
    let (echoes, stderr) = repl(b"x := i32 5\nx:dyad\n");
    assert!(echoes.is_empty() && stderr.contains("has no node yet"), "stderr: {stderr}");
}

#[test]
fn a_dot_on_an_if_or_a_statement_reads_its_node_and_uses_no_value_on_both_tiers() {
    let two = "(if c (i32 1) else (i64 2))";
    for body in [
        format!("{two}.type == (if c (u8 1) else (i32 2)).type"),
        format!("{two}.type == (if c (u8 1)).type"),
        format!("(if c (return true) else (i32 2)).type == {two}.type"),
        format!("not ({two}.type == (x + 1).type)"),
        format!("not ({two}.type == i32)"),
        format!("not ({two}.type == (while c (c = false)).type)"),
        format!("(c = true, {two}).type == scope"),
        "(x = 4).type == (x = 5).type and x == 3".into(),
        "(for i in 0..2 (i)).type == (for i in 0..3 (i)).type and x == 3".into(),
    ] {
        let lines = format!("mut c := true, mut x := i32 3, f := fn () -> bool ( {body} )");
        for call in ["f()", "f.compile(), f()"] {
            let src = format!("{lines}, {call}");
            let (code, stdout, stderr) = run_line(&src);
            assert_eq!((code, stdout.trim()), (Some(0), "true"), "{src}: {stderr}");
        }
    }
    for (tail, echo) in [(format!("{two}.type"), "type\n"), (format!("t := {two}.type, 0"), "0\n")]
    {
        let src = format!("mut c := true, {tail}");
        assert_eq!(run_line(&src), (Some(0), echo.to_string(), String::new()), "{src}");
    }
    // Its value used, the `if` is still refused.
    for tail in [two.to_string(), format!("x := {two}, 0")] {
        let (code, _, stderr) = run_line(&format!("mut c := true, {tail}"));
        assert_eq!(code, Some(1), "{tail}");
        assert!(stderr.contains("give different types"), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn a_reflect_read_that_does_not_fit_is_an_error() {
    let (_echoes, stderr) = repl(b"x := i32 5\nb := x + x\nb:start.rhs.type.roles[5]\n");
    assert!(stderr.contains("does not fit"), "stderr: {stderr}");
}

#[test]
fn an_import_inside_a_fn_body_is_rejected() {
    // The load is a comptime effect; inside a fn body parse and run order do not coincide.
    let (_echoes, stderr) = repl("g := fn () -> i32 ( import file «examples/answer.logos», 1 )\n");
    assert!(stderr.contains("`import` reads a file as Logos at parse time"), "stderr: {stderr}");
    let (code, _, stderr) =
        run_line("g := fn () -> i32 ( (logos file «tests/fixtures/lib_pub.logos»).double(2) )");
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("`logos` reads a file as Logos at parse time"), "stderr: {stderr}");
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
        .write_all(
            b"mut x := i32 5\nx = 40\ndouble := fn (a := i32 ?) -> i32 ( a + a )\nzz\ndouble(x) + 2\n",
        )
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
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
fn repl(input: impl AsRef<[u8]>) -> (Vec<String>, String) {
    let mut child = logos()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(input.as_ref()).unwrap();
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
fn the_repl_echoes_a_line_as_the_command_line_does() {
    // Whether a last value shows is one rule; what a type definition or a `fn` shows is
    // ›A value is shown as the text its type's `print` slot gives back‹ [[R305]], so it is
    // only compared here.
    for (program, shown) in [
        ("x := i32 5, x.type", 1),
        ("i32", 1),
        ("t := i32, t", 1),
        ("mut a := type ?, a = i32, a", 1),
        ("type ( a := i32 ? )", 1),
        ("f := fn () -> i32 ( 1 ), f", 1),
        ("x := i32 5", 0),
        ("t := i32", 0),
        ("f := fn () -> i32 ( 1 )", 0),
        ("p := type (v := i32 ?)", 0),
    ] {
        let (code, stdout, stderr) = run_line(program);
        assert_eq!(code, Some(0), "{program}: stderr: {stderr}");
        let printed: Vec<&str> = stdout.lines().collect();
        assert_eq!(printed.len(), shown, "{program}: {stdout}");
        let (echoes, stderr) = repl(format!("{program}\n").as_bytes());
        assert_eq!(echoes, printed, "{program}: stderr: {stderr}");
    }
    for input in ["x := i32 5\nx.type\n", "t := i32\nt\n"] {
        let (echoes, stderr) = repl(input);
        assert_eq!(echoes, ["i32"], "{input}: stderr: {stderr}");
        assert!(stderr.is_empty(), "{input}: stderr: {stderr}");
    }
}

#[test]
fn the_repl_rolls_back_a_failed_lines_declarations() {
    let (echoes, stderr) =
        repl(b"b := )\nb := 5\nf := fn (v := i32 ?) -> i32 ( v )\ng := f(1,2)\ng := f(3)\ng + b\n");
    assert_eq!(echoes, ["8"], "stderr: {stderr}");
    assert!(stderr.contains("nothing to evaluate here"), "stderr: {stderr}");
    assert!(stderr.contains("argument count"), "stderr: {stderr}");
    assert!(!stderr.contains("shadowed"), "stderr: {stderr}");
}

#[test]
fn the_repl_keeps_an_owning_binding_alive_across_lines() {
    let (echoes, stderr) =
        repl("a := alloc 1 of i32 5\nprint «{a@}»\nr := ( b := alloc 1 of i32 20, b@ )\nr\n");
    assert_eq!(echoes, ["5", "20"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_repl_session_frees_each_line_s_value_once_at_its_end() {
    // The session's exit grows a line at a time, text made between its lines.
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) )";
    let lines: String =
        (1..=5).map(|i| format!("v{i} := bag({i})\nt{i} := «text {i}»\n")).collect();
    let (echoes, stderr) = repl(format!("{bag}\n{lines}").as_bytes());
    assert_eq!(echoes, ["freed 5", "freed 4", "freed 3", "freed 2", "freed 1"], "stderr: {stderr}");
}

#[test]
fn the_repl_reuses_a_name_after_free() {
    let (echoes, stderr) = repl(b"n := i32 5\nfree n\nn\nn := i32 6\nn\n");
    assert_eq!(echoes, ["6"], "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:1: error: `n` is dead here"), "stderr: {stderr}");
}

#[test]
fn the_repl_refuses_a_call_whose_body_reads_a_freed_name() {
    // A call is a use of every outer name the callee's body reads.
    let (echoes, stderr) = repl(
        "mut n := i32 0\nclimb := fn () -> i32 ( n = n + 1, n )\n\
          print «{climb()}»\nprint «{climb()}»\nfree n\n\
          climb()\nmut n := i32 10\nclimb()\nclimb2 := fn () -> i32 ( n = n + 1, n )\nclimb2()\n",
    );
    assert_eq!(echoes, ["1", "2", "11"], "stderr: {stderr}");
    assert_eq!(
        stderr.matches("<repl>:1:1: error: `n` is dead here").count(),
        2,
        "stderr: {stderr}"
    );
}

#[test]
fn an_imported_function_may_read_its_own_sections_private_names() {
    // The section is on no caller's stack, but its names live for the run.
    let out =
        logos().arg("import file «tests/fixtures/lib_helper.logos», bump(41)").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn a_failed_repl_line_restores_a_moved_name() {
    let (echoes, stderr) = repl(b"a := alloc 1 of i32 5\nr := ( b := move a, b@ ) + nosuch\na@\n");
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    assert!(stderr.contains("unknown name"), "stderr: {stderr}");
    assert!(!stderr.contains("dead"), "stderr: {stderr}");
}

#[test]
fn a_repl_line_that_faults_keeps_the_end_of_what_it_ended() {
    // Its run freed the value, before the fault or as the fault left: the session neither
    // reads it nor frees it again.
    let (echoes, stderr) =
        repl(b"a := alloc 1 of i32 5\n( free a, error \xc2\xabx\xc2\xbb )\na@\n");
    assert!(echoes.is_empty(), "{echoes:?} stderr: {stderr}");
    assert!(stderr.contains("`a` is dead here"), "stderr: {stderr}");
    let lines = BOX.replacen("), box := ", ")\nbox := ", 1);
    let g = "g := fn () -> i32 ( error «x» )\nc := i32 1";
    for tail in
        ["( free a, error «x» )", "if (c == 1) ( free (g()), free a )", "if (g() == 1) ( free a )"]
    {
        let (echoes, stderr) =
            repl(format!("{lines}\n{g}\na := box (1, 2)\n{tail}\na.size\n").as_bytes());
        assert_eq!(echoes, ["free"], "{tail}: stderr: {stderr}");
        assert!(stderr.contains("`a` is dead here"), "{tail}: stderr: {stderr}");
    }
    // A fault the pass meets before the parse reaches the `free` ends nothing.
    let (echoes, stderr) = repl(
        format!("{lines}\n{g}\na := box (1, 2)\n( free (g()), free a )\n") + "print «{a.size}»\n",
    );
    assert_eq!(echoes, ["2", "free"], "stderr: {stderr}");
}

#[test]
fn a_failed_repl_line_keeps_the_ends_its_pass_ran() {
    // A parse error after the pass ran the `free` keeps the name ended; a fault the pass
    // meets before the `free` ran leaves it to the session.
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) )";
    let fns = "g := fn () -> i32 ( error «x» )\nh := fn () -> i32 ( 1 )";
    let (echoes, stderr) = repl(
        format!("{bag}\n{fns}\n")
            + "a := bag(1)\n( free a, n := immediate h(), nosuch )\na.n\n\
               b := bag(2)\n( n := immediate g(), free b, 1 )\nprint «{b.n}»\n",
    );
    assert_eq!(echoes, ["freed 1", "2", "freed 2"], "stderr: {stderr}");
    assert!(stderr.contains("`a` is dead here"), "stderr: {stderr}");
}

#[test]
fn a_fault_in_the_pass_frees_what_a_nested_line_ended_once() {
    // The scope that holds the name decides, from the lines the pass ran.
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) ), \
               g := fn () -> i32 ( error «x» ), h := fn () -> i32 ( 1 ), a := bag(1)";
    for tail in [
        "x := ( free a, n := immediate g(), 1 )",
        "y := g(), x := ( m := i32 1, free a, n := immediate h(), 1 )",
        "x := ( m := i32 1, free a, n := immediate g(), 1 )",
        "x := ( m := immediate g(), free a, 1 )",
        "x := ( free a, m := immediate h(), i32 1 ) + immediate g()",
        "x := ( m := immediate h(), free a, i32 1 ) + immediate g()",
    ] {
        let out = logos().arg(format!("{bag}, {tail}, 1")).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{tail}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "freed 1\n", "{tail}");
    }
}

#[test]
fn a_block_that_fails_to_parse_frees_what_its_run_lines_hold() {
    // As the top level does: the pass ran `b`'s line before the error.
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) ), \
               h := fn () -> i32 ( 1 )";
    for tail in ["nosuch", "&b", "return b", "return 1, 2"] {
        let src = format!("{bag}, x := ( b := bag(2), n := immediate h(), {tail} )");
        let out = logos().arg(src).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{tail}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "freed 2\n", "{tail}");
    }
}

#[test]
fn the_repl_compiles_a_fn_across_lines() {
    let (echoes, stderr) =
        repl(b"double := fn (x := i64 ?) -> i64 ( x + x )\ndouble.compile()\ndouble(21)\n");
    assert_eq!(echoes, ["42"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_repl_recompiles_a_fn_across_lines() {
    let (echoes, stderr) = repl(
        b"double := fn (x := i64 ?) -> i64 ( x + x )\ndouble.compile()\ndouble.compile()\ndouble(21)\n",
    );
    assert_eq!(echoes, ["42"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_recompiled_callee_is_reached_by_an_earlier_compiled_caller() {
    let out = logos()
        .arg(
            "g := fn (x := i64 ?) -> i64 ( x + 1 ), f := fn (x := i64 ?) -> i64 ( g(x) * 2 ), \
              g.compile(), f.compile(), g.compile(), f(20)",
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn runaway_compiled_recursion_is_a_clean_error() {
    let out = logos()
        .arg("f := fn (n := i32 ?) -> i32 ( f(n + 1) ), f.compile(), f(1)")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("nested too deep: the run used up its 64 MiB stack"), "stderr: {err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn an_else_if_chain_selects_the_matching_arm() {
    let (echoes, stderr) = repl(
        "x := i32 1\nprint «{if (x == 0) (i32 10) else if (x == 1) (i32 20) else (i32 30)}»\n\
          y := i32 2\n\
          print «{if (y == 0) (i32 10) else if (y == 1) (i32 20) else if (y == 2) (i32 30) else (i32 40)}»\n\
          z := i32 9\nprint «{if (z == 0) (i32 10) else if (z == 1) (i32 20) else (i32 30)}»\n\
          if (x == 1) (i32 20) else ( if (x == 2) (i32 30) else (i32 40) )\n",
    );
    assert_eq!(echoes, ["20", "30", "30", "20"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_repl_binds_a_name_to_a_type() {
    let (echoes, stderr) =
        repl("t := i32\nx := t 7\nprint «{t(9)}»\nf := fn (v := t ?) -> t ( v + v )\nprint «{t}»\nf(x)\n");
    assert_eq!(echoes, ["9", "i32", "14"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn type_is_a_value_reflected_by_dot_type_and_compared_by_identity() {
    let (echoes, stderr) = repl(
        "print «{i32 == i32}»\nprint «{i32 == f64}»\nprint «{i32 != f64}»\n\
          print «{i32.type == type}»\nprint «{i32.type == i32}»\n\
          x := i32 5\nprint «{x.type == i32}»\nprint «{x.type == f64}»\nt := type\n\
          print «{i32.type == t}»\nprint «{type.type == type}»\n\
          print «{logos.type == language}»\nprint «{logos == type}»\nlanguage.type == type\n",
    );
    assert_eq!(
        echoes,
        [
            "true", "false", "true", "true", "false", "true", "false", "true", "true", "true",
            "false", "true"
        ],
        "stderr: {stderr}"
    );
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_type_reflection_example_runs() {
    let out = logos().arg("import file «examples/type_reflection.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn a_logos_value_prints_its_spelling() {
    let out = logos().arg("import file «tests/fixtures/logos_name.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "i32\n");
}

#[test]
fn a_type_returning_function_resolves_at_comptime() {
    let (echoes, stderr) = repl(
        "pick := fn (i := i32 ?) -> type (if (i==0)(i32) else (f64))\n\
          print «{pick(0) == i32}»\nprint «{pick(1) == f64}»\nprint «{pick(0) == f64}»\n\
          t := pick(0)\nt == i32\n",
    );
    assert_eq!(echoes, ["true", "true", "false", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_type_returning_function_hands_back_a_type_box() {
    let (echoes, stderr) = repl(
        "f := fn (t := type ?) -> type ( mut x := t, x )\nprint «{f(i32) == i32}»\n\
          y := f(f64)\nz := y 5\nprint «{z}»\n\
          mints := hashmap type -> type\n\
          get := fn (t := type ?) -> type ( mut m := mints[t], if m != ? return m, m = t, mints[t] = m, m )\n\
          print «{get(i32) == get(i32)}»\nprint «{get(i32) == get(f64)}»\nmut u := mints[u8]\nu == ?\n",
    );
    assert_eq!(echoes, ["true", "5.0", "true", "false", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_tape_cell_checked_to_hold_a_type_passes_as_a_type() {
    let g = "g := fn (t := type ?) -> type ( t )";
    let tail = "tape.remove(1), tape.is_constructed[0] = true ) )";
    // After a raising `!=` check, for the rest of the scope; inside an `==` branch.
    for (check, arg) in [
        ("if tape[1].type != type error «no», t := tape[1], print «{g(t) == i64}»,", "i64"),
        ("if tape[1].type == type ( print «{g(tape[1]) == i64}» ),", "i64"),
        ("if tape[1].type != type ( print «no» ) else print «{g(tape[1]) == u8}»,", "u8"),
    ] {
        let src = format!(
            "{g}, r := type ( static parse_rank = fn.parse_rank, static parse = ( {check} {tail}, r {arg}"
        );
        let out = logos().args([&src]).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).lines().next(), Some("true"), "{src}");
    }
    // Unchecked, or checked and then the tape edited, the cell is still refused as a type.
    for body in [
        "t := tape[1], u := g(t),",
        "if tape[1].type != type error «no», tape.remove(1), u := g(tape[1]),",
        "if tape[1].type == type print «x», u := g(tape[1]),",
    ] {
        let src =
            format!("{g}, r := type ( static parse_rank = fn.parse_rank, static parse = ( {body} {tail}, r i32");
        let out = logos().args([&src]).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("must be a type value"), "{src}: stderr: {stderr}");
    }
    // A loop edits the tape after the read was checked: the read checks again when it runs.
    let src = format!(
        "{g}, r := type ( static parse_rank = fn.parse_rank, static parse = ( \
         if tape[1].type != type error «no», mut i := i32 0, \
         while i < 2 ( u := g(tape[1]), tape.remove(1), i = i + 1 ), \
         tape.is_constructed[0] = true ) ), r i32 [1]"
    );
    let out = logos().args([&src]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("it holds something else"), "{src}: stderr: {stderr}");
}

#[test]
fn a_type_body_in_a_function_is_built_per_call() {
    let (echoes, stderr) = repl(
        "mk := fn (t := type ?) -> type ( type ( static e := t ) )\n\
          a := mk(i32)\nb := mk(f64)\nc := mk(i32)\n\
          print «{a.e == i32}»\nprint «{b.e == f64}»\nprint «{a == b}»\nprint «{a == c}»\n\
          mints := hashmap type -> type\n\
          get := fn (t := type ?) -> type ( mut m := mints[t], if m != ? return m, \
          m = type ( static e := t ), mints[t] = m, m )\n\
          print «{get(i32) == get(i32)}»\nprint «{get(i32) == get(f64)}»\nprint «{get(u8).e == u8}»\n\
          g := fn (n := i32 ?) -> type ( type (static parse_rank = n) )\nt := g(3)\nt.parse_rank\n",
    );
    assert_eq!(
        echoes,
        ["true", "true", "false", "false", "true", "false", "true", "3.0"],
        "stderr: {stderr}"
    );
    assert!(stderr.is_empty(), "stderr: {stderr}");

    // The body is read when it runs, so its mistakes are reported at the call.
    let (_echoes, stderr) = repl(b"mk := fn () -> type ( type ( static e := nosuch ) )\nmk()\n");
    assert!(stderr.contains("building this `type (…)` failed"), "stderr: {stderr}");
    assert!(stderr.contains("nosuch"), "stderr: {stderr}");
}

#[test]
fn a_type_body_in_a_function_declares_fields_from_the_call_s_types() {
    let (echoes, stderr) = repl(
        "mk := fn (t := type ?) -> type ( type ( static e := t, p := @e ?, v := t ? ) )\n\
          print «{mk(u8).e == u8}»\nk := fn (t := type ?) -> type ( mk(t) )\nk(i32).e == i32\n",
    );
    assert_eq!(echoes, ["true", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
    // A member declared from the call's type is that type, so a function body may apply it.
    let (echoes, stderr) = repl(
        b"mk := fn (t := type ?) -> type ( type ( static e := t, \
          static f := fn () -> i32 ( e 5 ) ) )\nmk(i32).e == i32\n",
    );
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_type_returning_fn_example_runs() {
    let out = logos().arg("import file «examples/type_returning_fn.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn file_mode_runs_each_expression_as_it_parses() {
    // The file driver used to parse everything first, so the call read x's zeroed storage and answered i32 rather than f64.
    let out = logos()
        .arg("import file «tests/fixtures/comptime_sees_committed_state.logos»")
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn a_type_call_with_a_runtime_argument_yields_a_type_at_run() {
    let (echoes, stderr) = repl(
        "pick := fn (i := i32 ?) -> type (if (i==0)(i32) else (f64))\n\
          g := fn (n := i32 ?) -> i32 ( a := pick(n), if (a == f64) (7) else (3) )\n\
          print «{g(1)}»\nprint «{g(0)}»\n\
          h := fn (n := i32 ?) -> i32 ( x := pick(n) 5, 1 )\n",
    );
    assert_eq!(echoes, ["7", "3"], "stderr: {stderr}");
    assert!(stderr.contains("known only when the program runs"), "stderr: {stderr}");
}

#[test]
fn a_logos_declaration_declares_a_place_of_that_type() {
    let (echoes, stderr) = repl("mut a := i32 ?\na = 9\nprint «{a.type == i32}»\na\n");
    assert_eq!(echoes, ["true", "9"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_dependent_typed_declaration_takes_a_computed_type() {
    let (echoes, stderr) = repl(
        "metalogos := fn (i := i32 ?) -> type (if (i==0)(i32) else (f64))\n\
          mut b := metalogos(1) ?\nb = 7\nprint «{b.type == f64}»\nb\n",
    );
    assert_eq!(echoes, ["true", "7.0"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_logos_declaration_works_after_other_code() {
    let out = logos()
        .arg("import file «tests/fixtures/declared_logos_after_code.logos»")
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n");
}

#[test]
fn a_logos_declaration_rejects_a_non_type() {
    let (_echoes, stderr) = repl(b"a := 5 ?\n");
    assert!(
        stderr.contains("the declared or assigned type must be a type value"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_logos_declaration_names_the_non_numeric_gap() {
    let (_echoes, stderr) = repl(b"a := scope ?\n");
    assert!(stderr.contains("non-numeric types are not in the seed yet"), "stderr: {stderr}");
}

#[test]
fn a_logos_variable_declares_fills_once_and_becomes_the_type() {
    let (echoes, stderr) = repl(
        "mut a := type ?\na = i32\nprint «{a.type == type}»\nprint «{a == i32}»\ny := a 5\ny\n",
    );
    assert_eq!(echoes, ["true", "true", "5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"mut a := type ?\na == i32\n");
    assert!(
        echoes.is_empty() && stderr.contains("`a` is read before it is written"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_logos_box_is_written_as_often_as_you_like() {
    let (echoes, stderr) = repl(b"mut a := type ?\na = i32\na = f64\na\n");
    assert_eq!(echoes, ["f64"], "stderr: {stderr}");

    let (echoes, stderr) =
        repl("mut a := type ?\na = f64\ng := fn () -> i32 ( a = i32, 1 )\nprint «{g()}»\na\n");
    assert_eq!(echoes, ["1", "i32"], "stderr: {stderr}");

    let (_e, stderr) = repl(b"mut a := type ?\na = 5\n");
    assert!(stderr.contains("must be a type value"), "stderr: {stderr}");
}

#[test]
fn logical_operators_fold_over_bool_literals() {
    let (echoes, stderr) = repl(
        "print «{true or false}»\nprint «{true and true}»\nprint «{not (true)}»\n\
          mut a := type ?\na = i32\nif (a.type == f32 or a.type == type) (a = f64) else (a = i32)\na == f64\n",
    );
    assert_eq!(echoes, ["true", "true", "false", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_comptime_if_drops_the_untaken_branch_unparsed() {
    // `a = 9.9` under `a := i32 ?` would be a parse error if it were ever parsed; that this runs proves the branch was skipped.
    let (echoes, stderr) = repl(
        "mut a := i32 ?\na = 0\nif (a.type == i32) (a = 9) else (a = 9.9)\nprint «{a}»\n\
          mut b := f64 ?\nb = 0\nif (b.type == i32) (b = 1) else if (b.type == f64) (b = 2.5) else (b = 3)\nb\n",
    );
    assert_eq!(echoes, ["9", "2.5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_metatypefn_example_runs() {
    // The expected value tracks the file's current argument; the deep arm is pinned by the metalogos_arm fixture.
    let out = logos().arg("import file «examples/metatypefn.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "9.5\n");
}

#[test]
fn the_metalogos_arm_fills_a_logos_variable() {
    let out = logos().arg("import file «tests/fixtures/metalogos_arm.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "i32\n");
}

#[test]
fn a_declaration_snapshots_its_value_and_reads_are_stable() {
    // The block used to re-run its loop on every read and grow.
    let (echoes, stderr) = repl(
        "c := (mut sum := i32 0, for i in 0..10 (sum = sum + i), sum)\nprint «{c}»\nprint «{c}»\n\
          mut a := i32 1\nx := a + a\na = 5\nx\n",
    );
    assert_eq!(echoes, ["45", "45", "2"], "stderr: {stderr}");
}

#[test]
fn a_for_loop_needs_no_index_in_both_tiers() {
    let (echoes, stderr) = repl(
        "f := fn () -> i32 ( mut t := i32 0, for 0..5 ( t = t + 2 ), t )\nprint «{f()}»\nf.compile()\nf()\n",
    );
    assert_eq!(echoes, ["10", "10"], "stderr: {stderr}");
}

#[test]
fn the_no_index_example_counts_the_evens() {
    let out = logos().arg("import file «examples/no_index.logos»").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "5");
}

#[test]
fn a_declaration_copies_rather_than_aliases() {
    let (echoes, stderr) = repl("y := i32 1\nmut z := y\nz = 5\nprint «{y}»\nz\n");
    assert_eq!(echoes, ["1", "5"], "stderr: {stderr}");
}

#[test]
fn values_render_through_their_type() {
    for (input, shown) in [
        ("f32 5.5\n", "5.5"),
        ("q := f64 2.5\nq + q\n", "5.0"),
        ("i64 -1\n", "-1"),
        ("u8 200\n", "200"),
        ("1 < 2\n", "true"),
        ("not (1 < 2)\n", "false"),
    ] {
        let (echoes, stderr) = repl(input);
        assert_eq!(echoes, [shown], "{input}: stderr: {stderr}");
    }
}

#[test]
fn help_prints_usage_and_version() {
    let out = logos().arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(env!("CARGO_PKG_VERSION")));
    assert!(text.contains("logos import file «./file.logos»"));
}

#[test]
fn a_statement_tail_prints_nothing_on_the_command_line_too() {
    // The command line used to print 5 for `x := i32 0, x = 5` where the REPL and a file printed nothing.
    for src in ["mut x := i32 0, x = 5", "mut x := i32 5", "mut x := i32 1, p := &x, p@ = 9"] {
        let out = logos().arg(src).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert!(out.stdout.is_empty(), "{src}: printed {:?}", String::from_utf8_lossy(&out.stdout));
    }
    let out = logos().arg("mut x := i32 0, x = 5, x").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}

#[test]
fn a_line_starting_with_a_dash_is_source_not_a_flag() {
    // Both shapes a shell produces: the words unquoted, joined back into one line, and the line as one argument.
    let out = logos().args(["-5", "+", "3"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-2\n");

    let out = logos().arg("-5+3").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-2\n");

    let out = logos().arg("--nope").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("<command line>:1:3: error: unknown name `nope`"), "stderr: {err}");

    for flag in ["--help", "-h"] {
        let out = logos().arg(flag).output().unwrap();
        assert!(out.status.success(), "{flag}");
        assert!(String::from_utf8_lossy(&out.stdout).contains("usage:"), "{flag}");
    }
}

#[test]
fn the_binding_read_answers_scope_range_and_gate() {
    let (echoes, stderr) = repl(
        "x := i32 5\nprint «{x:end}»\nprint «{x:gate}»\nprint «{x:scope}»\n\
          at_read := (x + x):scope\nprint «{at_read}»\n\
          y := i32\nprint «{y.type == type}»\ny == i32\n",
    );
    assert_eq!(echoes.len(), 6, "stderr: {stderr}");
    assert_eq!(&echoes[..2], ["0", "0"], "end and gate are null");
    assert_ne!(echoes[2], "0", "x was declared somewhere");
    assert_eq!(echoes[2], echoes[3], "a constructed node's :scope is the scope open at the read");
    assert_eq!(&echoes[4..], ["true", "true"], "an alias is its own binding over the same dyad");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_binding_reads_its_frame_and_offset() {
    // A top-level name's bytes are in the program frame, the root scope, at an offset of its own.
    let (echoes, stderr) = repl(
        "x := i32 5\ny := i64 6\ntop := here.scope\n\
          print «{x:frame == top.back}»\nprint «{y:frame == x:frame}»\n\
          print «{y:offset == x:offset}»\nprint «{x:offset < y:offset}»\nx:name\n",
    );
    assert_eq!(echoes, ["true", "true", "false", "true", "x"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_binding_s_fields_are_read_and_only_lex_rank_is_written() {
    // A write into `frame` or `offset` would move where `s` reads; `lex_rank` is the one
    // field a program sets after the declaration. `&` of a field holds a copy of what the line
    // read, so a write through its address moves nothing.
    let (echoes, stderr) = repl(
        "s := i32 5
s:offset = 7
s:frame = s:scope
s:scope = s:scope
s:end = s:scope
          p := &s:offset
p@ = 9
s:lex_rank = 7
print «{s:lex_rank}»
s
",
    );
    assert_eq!(echoes, ["7.0", "5"], "stderr: {stderr}");
    assert_eq!(stderr.matches("not an assignable place").count(), 4, "stderr: {stderr}");
    assert_eq!(stderr.matches("error").count(), 4, "stderr: {stderr}");
}

#[test]
fn a_parameter_and_a_local_are_names_of_the_call_s_frame() {
    // Parameters first, then the locals, packed at their widths: `a` at 0, `b` at 4, `c` at 12;
    // their frame is the function, not the program's.
    let (echoes, stderr) = repl(
        "x := i32 5\nf := fn (a := i32 ?, b := i64 ?) -> u64 ( c := i64 3, a:offset + b:offset + c:offset )\n\
          print «{f(1, 2)}»\ng := fn (a := i32 ?, b := i64 ?) -> bool ( a:frame == b:frame and a:frame != x:frame )\n\
          print «{g(1, 2)}»\nh := fn () -> i64 ( n := i64 0 - 1, n )\nprint «{h()}»\nh.compile()\n\
          print «{h()}»\nu := u64 0 - 1\nu\n",
    );
    assert_eq!(echoes, ["16", "true", "-1", "-1", "18446744073709551615"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_static_function_writes_a_field_of_a_frame_local_record() {
    // The value a `static` function is called through is the record's own bytes, in the
    // caller's frame, so the write lands in the local it was called on.
    let (echoes, stderr) = repl(
        "pt := type (mut a := i32 ?, b := i32 ?, static bump := fn () -> void ( a = a + b ))\n\
          f := fn () -> i32 ( mut p := pt(1, 2), p.bump(), p.bump(), p.a )\nprint «{f()}»\n\
          mut q := pt(10, 5)\nq.bump()\nq.a\n",
    );
    // A `-> void` call gives nothing, so it echoes nothing.
    assert_eq!(echoes, ["5", "15"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_field_the_binding_has_not_is_the_same_error_as_an_undeclared_dot_field() {
    // The message names the spelling, the one thing the two probes differ in, so it is blanked before comparing.
    fn message(stderr: &str) -> String {
        let m = stderr.lines().next().and_then(|l| l.split("error: ").nth(1)).unwrap_or("");
        m.split('`')
            .enumerate()
            .filter(|(i, _)| i % 2 == 0)
            .map(|(_, part)| part)
            .collect::<Vec<_>>()
            .join("`…`")
    }
    let (_e, via_binding) = repl(b"x := i32 5\nx:i32\n");
    let (_e, via_dot) = repl(b"p := type (a := i32 ?)\nq := p(1)\nq.scope\n");
    assert_eq!(message(&via_binding), message(&via_dot), "binding: {via_binding}\ndot: {via_dot}");
    assert!(via_binding.contains("not in scope"), "stderr: {via_binding}");
    let (_e, via_binding) = repl(b"x := i32 5\nx:nonexistent\n");
    let (_e, via_dot) = repl(b"p := type (a := i32 ?)\nq := p(1)\nq.nonexistent\n");
    assert_eq!(message(&via_binding), message(&via_dot), "binding: {via_binding}\ndot: {via_dot}");
    assert!(via_binding.contains("unknown name"), "stderr: {via_binding}");
}

#[test]
fn a_type_is_read_with_the_binding_read() {
    let (_echoes, stderr) = repl(b"x := i32 5\n(dyad x).type\n");
    assert!(!stderr.is_empty(), "the old spelling no longer parses");
    let (echoes, stderr) = repl(b"x := i32 5\nx.type == i32\n");
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
}

#[test]
fn a_tight_read_runs_over_a_keyword_before_its_constructor_wakes() {
    let (echoes, stderr) = repl(
        "mut x := i32 5\nprint «{if:scope}»\nprint «{type.type == type}»\nprint «{fn.type == type}»\n\
          f := fn () -> i32 ( if (x < 9) (x = 1) else (x = 2), x )\nf()\n",
    );
    assert_eq!(echoes.len(), 4, "stderr: {stderr}");
    assert_ne!(echoes[0], "0", "if was declared in the session scope");
    assert_eq!(&echoes[1..], ["true", "true", "1"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_line_that_gives_nothing_echoes_nothing_and_no_value_reads_it() {
    let void_fn = "mut n := i32 1, f := fn () -> void ( n = n + 1 )";
    for tail in ["f()", "for i in 0..3 ( n = n + 1 )", "while n < 4 ( n = n + 1 )", "if (n > 9) 5"]
    {
        let (code, stdout, stderr) = run_line(&format!("{void_fn}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, "", "{tail}");
    }
    for tail in [
        "x := f(), x",
        "print «{f()}»",
        "f() + 1",
        "g := fn () -> i32 ( y := (return n), y ), g()",
        "f().type",
        "g := fn (a := i32 ?) -> i32 ( a ), g(f())",
        "g := fn (a := i32 ?) -> i32 ( a ), g(x := i32 5)",
        "h := fn () -> i32 ( return f() ), h()",
    ] {
        let (code, _, stderr) = run_line(&format!("{void_fn}, {tail}"));
        assert_eq!(code, Some(1), "{tail}");
        assert!(stderr.contains("gives nothing"), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn a_return_outside_every_function_is_refused_where_it_is_written() {
    for (src, operand) in [
        ("x := i32 4, return x", "return"),
        ("return 5, 6", "return"),
        ("mut c := true, if c (return 1), 2", "return"),
        ("for i in 0..3 ( print «{i}», return i )", "return"),
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}");
        assert_eq!(stdout, "", "{src}");
        let at = src.find(operand).expect("the word is in the program");
        let col = src[..at].chars().count() + 1;
        let head =
            format!("<command line>:1:{col}: error: `return` is written only inside a function");
        assert!(stderr.starts_with(&head), "{src}: stderr: {stderr}");
    }
    let (_, _, stderr) = run_line("x := i32 4,\n  return x");
    assert!(stderr.starts_with("<command line>:2:3: error: `return`"), "stderr: {stderr}");
}

#[test]
fn a_return_hands_the_call_its_operand_as_the_last_line_does() {
    for (src, out) in [
        // A call that gives nothing takes nothing, so the operand need give nothing.
        (
            "f := fn (n := i32 ?) -> void ( if (n > 2) (return print «big»), print «small» ), \
             f(5), f(1)",
            "big\nsmall\n",
        ),
        ("f := fn () -> void ( c := i32 1, return if (c == 1) (print «a») ), f()", "a\n"),
        // Each arm goes to the `-> T`, and one may leave by `error`, as at the last line.
        (
            "f := fn (c := bool ?) -> i64 ( return if c (i32 1) else (i64 2) ), \
             print «{f(true)} {f(false)}»",
            "1 2\n",
        ),
        (
            "f := fn (c := bool ?) -> i32 ( return if c (error «no») else (i32 2) ), print «{f(false)}»",
            "2\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, out, "{src}");
    }
}

#[test]
fn the_echo_reads_the_tail_value_as_any_use_does() {
    // Refused before the tail runs, so `ran` never prints; an arm's value passes out through the
    // `if` around it, whatever its sibling gives.
    for tail in [
        "if c (i32 1) else (i64 2)",
        "( y := i32 1, if c (print «ran», i32 1) else (i64 2) )",
        "if c (i32 1) else (i64 2), 3",
        "if c (if c (i32 1) else (i64 2)) else (print «b»)",
        "if c (print «b») else (if c (i32 1) else (i64 2))",
    ] {
        let (code, stdout, stderr) = run_line(&format!("mut c := true, {tail}"));
        assert_eq!(code, Some(1), "{tail}");
        assert_eq!(stdout, "", "{tail}");
        assert!(stderr.contains("give different types"), "{tail}: stderr: {stderr}");
    }
    // The caret stands where the refused line ends, not after the prose below it.
    let (_, _, stderr) = run_line("mut c := true,\nif c (i32 1) else (i64 2),\n# a closing note\n");
    assert!(stderr.starts_with("<command line>:2:"), "stderr: {stderr}");
    for (tail, want) in [
        ("if c (i32 1) else (i32 2)", "1\n"),
        // The arm that runs gives the value, even where another arm gives nothing.
        ("if c (5)", "5\n"),
        ("if c (i32 1) else (print «b»)", "1\n"),
        ("if c (1) else if c (2) else (3)", "1\n"),
        ("x := if c (1) else (if c (2) else (3)), x", "1\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("mut c := true, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, want, "{tail}");
    }
    let (echoes, stderr) =
        repl(b"mut c := true\nif c (i32 1) else (i64 2)\nif c (i32 1) else (i32 2)\n");
    assert_eq!(echoes, ["1"], "stderr: {stderr}");
    assert!(stderr.contains("give different types"), "stderr: {stderr}");
}

#[test]
fn an_arm_that_gives_nothing_is_refused_whatever_it_ends_in() {
    for arm in ["error «no»", "print «b»"] {
        let (code, stdout, stderr) = run_line(&format!("c := true, x := if c (1) else ({arm}), x"));
        assert_eq!(code, Some(1), "{arm}");
        assert_eq!(stdout, "", "{arm}");
        assert!(
            stderr.contains("this arm gives nothing (a statement, a `return` or an `error`"),
            "{arm}: stderr: {stderr}"
        );
    }
}

#[test]
fn assignment_returns_nothing() {
    let (_e, stderr) = repl(b"mut a := i32 1\nmut b := i32 2\na = b = 3\n");
    assert!(stderr.contains("gives nothing"), "stderr: {stderr}");
    let (_e, stderr) = repl(b"mut a := i32 1\ny := (a = 2) + 1\n");
    assert!(!stderr.is_empty(), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        "p := type (mut v := i32 ?)\nmut q := p(1)\nq.v = 3\nprint «{q.v}»\nmut a := i32 1\na = a + 1\na\n",
    );
    assert_eq!(echoes, ["3", "2"], "stderr: {stderr}");
}

#[test]
fn a_write_along_a_path_needs_mut_on_every_step() {
    for (src, expect) in [
        (&b"p := type (mut v := i32 ?)\nq := p(1)\nq.v = 3\n"[..], "`q` is not `mut`"),
        (b"p := type (v := i32 ?)\nmut q := p(1)\nq.v = 3\n", "`v` is not `mut`"),
        (b"t := type (static y := i32 3)\nt.y = 4\n", "`y` is not `mut`"),
        (b"t := type (static y := i32 3)\nq := t()\nq.y = 4\n", "`y` is not `mut`"),
        (b"p := type (immut v := i32 ?)\nmut q := p(1)\nq.v = 3\n", "`v` is `immut`"),
    ] {
        let (_e, stderr) = repl(src);
        assert!(stderr.contains(expect), "{}: stderr: {stderr}", String::from_utf8_lossy(src));
    }
    let (echoes, stderr) = repl(
        "t := type (static mut y := i32 3, v := i32 ?)\nq := t(1)\nt.y = 4\nprint «{q.y}»\nq.y = 5\nt.y\n",
    );
    assert_eq!(echoes, ["4", "5"], "stderr: {stderr}");
}

#[test]
fn a_write_through_a_pointer_needs_mut_on_every_field_after_the_dereference() {
    let t = |gate: &str| format!("t := type ( {gate}n := i64 ? )");
    let u = |a: &str, b: &str| {
        format!(
            "u := type ( {a}a := i64 ?, static bump := fn () -> void ( a = a + 1 ) ), \
             t := type ( {b}b := u ? ), p := alloc 1 of t (u (5))"
        )
    };
    for (src, expect) in [
        (format!("{}, p := alloc 1 of t (5), p@.n = 7", t("")), "`n` is not `mut`"),
        (format!("{}, p := alloc 1 of t (5), p@.n = 7", t("immut ")), "`n` is `immut`"),
        (format!("{}, mut v := t (5), q := &v, q@.n = 7", t("")), "`n` is not `mut`"),
        (format!("{}, p := alloc 2 of t (5), (p + 1)@.n = 7", t("")), "`n` is not `mut`"),
        (
            format!("{}, f := fn (p := @t ?) -> i64 ( p@.n = 7, p@.n ), f.compile()", t("")),
            "`n` is not `mut`",
        ),
        (format!("{}, p@.b.a = 7", u("mut ", "")), "`b` is not `mut`"),
        (format!("{}, p@.b.a = 7", u("", "mut ")), "`a` is not `mut`"),
        (format!("{}, p@.b.bump()", u("mut ", "")), "`b` is not `mut`"),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
    // The name holding the pointer grants nothing: the path starts at the `@`.
    for (src, want) in [
        (format!("{}, p := alloc 1 of t (5), p@.n = 7, p@.n", t("mut ")), "7\n"),
        (format!("{}, p := alloc 2 of t (5), (p + 1)@.n = 7, (p + 1)@.n", t("mut ")), "7\n"),
        (format!("{}, p@.b.a = 7, p@.b.a", u("mut ", "mut ")), "7\n"),
        (format!("{}, p@.b.bump(), p@.b.a", u("mut ", "mut ")), "6\n"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{src}: stderr: {stderr}");
    }
    for compile in ["", "f.compile(), "] {
        let src = format!(
            "{}, f := fn (p := @t ?) -> i64 ( p@.n = 7, p@.n ), {compile}p := alloc 1 of t (5), f(p)",
            t("mut ")
        );
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.as_str()), (Some(0), "7\n"), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_tight_read_lexes_its_right_cell_on_demand_and_stops_at_a_boundary() {
    let (echoes, stderr) = repl(
        "x := i32 5\nprint «{(y := x:end, 3)}»\np := type (a := i32 ?)\nq := p(1)\n\
          print «{(y := q.a, 2)}»\nprint «{q.a}»\n\
          r := &q\nprint «{r@.a}»\npp := &r\nprint «{pp@@.a}»\nx.type == i32\n",
    );
    assert_eq!(echoes, ["3", "2", "1", "1", "1", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_type_box_is_an_ordinary_variable() {
    let (echoes, stderr) = repl(
        "mut a := type ?\na = i32\nprint «{a == i32}»\na = f64\nprint «{a == f64}»\na == i32\n",
    );
    assert_eq!(echoes, ["true", "true", "false"], "stderr: {stderr}");

    let (echoes, stderr) = repl(
        "mut a := type ?\na = i32\nx := a 5\nprint «{x}»\na = f64\ny := a 2.5\nprint «{y}»\na\n",
    );
    assert_eq!(echoes, ["5", "2.5", "f64"], "stderr: {stderr}");

    let (echoes, stderr) = repl(b"b := type ?\nz := b ?\n");
    assert!(
        echoes.is_empty() && stderr.contains("`b` is read before it is written"),
        "stderr: {stderr}"
    );
}

#[test]
fn the_dyad_box_says_what_it_holds() {
    let (echoes, stderr) =
        repl("mut a := dyad ?\na = i32\nprint «{a.type == type}»\nprint «{a == i32}»\na\n");
    assert_eq!(echoes, ["true", "true", "i32"], "stderr: {stderr}");

    let (echoes, stderr) = repl(b"mut a := dyad ?\na = i32\ny := a 5\ny\n");
    assert_eq!(echoes, ["5"], "stderr: {stderr}");

    for src in [
        &b"( mut a := type ?, a = i32, x := a 5, x )"[..],
        b"( mut a := dyad ?, a = i32, x := a 5, x )",
    ] {
        let out = logos().arg(String::from_utf8_lossy(src).as_ref()).output().unwrap();
        assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n", "{}", String::from_utf8_lossy(src));
    }

    // A deferred body is the one place a box fill stays refused: there parse order is not run order.
    let out = logos()
        .arg("f := fn () -> i32 ( mut a := type ?, a = i32, x := a 5, x ), f()")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("known only when the program runs"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let (echoes, stderr) =
        repl("mut a := dyad ?\na = 5\nprint «{a}»\na = i64 7\nprint «{a.type == i64}»\na\n");
    assert_eq!(echoes, ["5", "true", "7"], "stderr: {stderr}");
}

#[test]
fn a_result_type_is_read_as_a_declaration_reads_its_type() {
    for (src, expect) in [
        ("mut t := type ?, t = i32, mk := fn () -> t ( 7 ), m := mk(), m.type", "i32\n"),
        ("mut t := type ?, t = i32, mk := fn () -> t ( 7 ), mk.compile(), m := mk(), m + 1", "8\n"),
        ("mut d := dyad ?, d = i32, mk := fn () -> d ( 7 ), mk() + 1", "8\n"),
    ] {
        let out = logos().arg(src).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), expect, "{src}");
    }

    let (echoes, stderr) = repl(
        "mut t := type ?\nt = i32\nmk := fn () -> t ( 7 )\nm := mk()\nprint «{m.type == i32}»\nm + 1\n",
    );
    assert_eq!(echoes, ["true", "8"], "stderr: {stderr}");

    for src in ["mk := fn () -> 5 ( 7 ), mk()", "mk := fn () -> 5 ( 7 ), mk.compile(), mk()"] {
        let out = logos().arg(src).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{src}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("1:16: error: the declared or assigned type must be"),
            "{src}: {stderr}"
        );
    }
    let (echoes, stderr) = repl(b"mk2 := fn () -> 5 ( 7 )\nmk2()\n");
    assert!(echoes.is_empty(), "echoes: {echoes:?}");
    assert!(stderr.contains("must be a type value"), "stderr: {stderr}");
    assert!(stderr.contains("unknown name `mk2`"), "stderr: {stderr}");

    let out = logos().arg("import file «tests/fixtures/hole_result_type.logos»").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr
            .contains("hole_result_type.logos:1:16: error: the declared or assigned type must be"),
        "stderr: {stderr}"
    );

    let out = logos()
        .arg("o := fn () -> i32 ( mut t := type ?, t = i32, mk := fn () -> t ( 7 ), m := mk(), m + 1 ), o()")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("1:62: error: which type this is is known only when"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_filled_box_names_the_type_it_holds_wherever_one_is_needed() {
    for (src, expect) in [
        ("mut u := type ?, mut d := dyad ?, d = i32, u = d, u", "i32\n"),
        ("mut u := type ?, mut d := dyad ?, d = i32, u = d, x := u 5, x + 1", "6\n"),
        ("mut d := dyad ?, d = i32, mut y := d ?, y = 4, y + 1", "5\n"),
        ("mut d := dyad ?, d = i32, y := dyad (d, 7), y + 1", "8\n"),
        ("mut d := dyad ?, d = i32, mk := fn () -> d ( 7 ), mk.compile(), mk() + 1", "8\n"),
    ] {
        let out = logos().arg(src).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), expect, "{src}");
    }

    for (src, at) in [
        ("x := i32 5, y := x ?, 1", "1:21"),
        ("x := i32 5, y := dyad (x, 7), 1", "1:29"),
        ("x := i32 5, mk := fn () -> x ( 7 ), 1", "1:28"),
        ("x := i32 5, mut u := type ?, u = x, 1", "1:35"),
        ("mut d := dyad ?, d = i32 5, y := d ?, 1", "1:37"),
    ] {
        let out = logos().arg(src).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{src}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!("{at}: error: the declared or assigned type must be")),
            "{src}: {stderr}"
        );
    }

    for param in ["type", "dyad"] {
        let src = format!("f := fn (t := {param} ?) -> i32 ( mk := fn () -> t ( 7 ), 1 ), f(i32)");
        let out = logos().arg(&src).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{src}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("1:47: error: which type this is is known only when"), "{stderr}");
    }
}

#[test]
fn a_call_lands_a_filled_box_as_the_type_it_holds() {
    let g = "g := fn (t := type ?) -> bool ( t == i32 )";
    for tail in [
        "mut d := dyad ?, d = i32, g(d)",
        "g.compile(), mut d := dyad ?, d = i32, g(d)",
        "mut d := type ?, d = i32, g(d)",
        "mut d := dyad ?, d = i32, h := fn () -> bool ( g(d) ), h()",
        "h := fn (d := dyad ?) -> bool ( g(d) ), h(i32)",
        "pt := type ( t := type ? ), mut d := dyad ?, d = i32, p := pt (d), p.t == i32",
    ] {
        let src = format!("{g}, {tail}");
        let out = logos().arg(&src).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n", "{src}");
    }

    let src = "mut d := dyad ?, d = i32, i32 (d)";
    let out = logos().arg(src).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{src}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("1:34: error: these types do not match"), "{stderr}");
}

#[test]
fn only_a_marked_place_is_written_or_addressed() {
    let pw = "pw := type ( mut a := i32 ?, mut b := i32 ?, output_type := type ?, static run = ( a * b ), \
              static parse_rank = *.parse_rank + 1, static associativity = right, \
              static parse = ( tape[0].type = pw, tape[0].a = tape[-1], tape[0].b = tape[1], tape[0].output_type = i32, \
              tape.is_constructed[0] = true, tape.remove(1), tape.remove(-1) ) )";
    for (src, expect) in [
        ("i32 5 = 3\n", "not an assignable place"),
        ("mut a := 5\na = 6\n", "`5` is a literal with no storage"),
        ("mut a := 5\na = 6\n", "as in `mut x := i32 5`"),
        ("mut a := 5\np := &a\n", "needs a variable"),
    ] {
        let (_e, stderr) = repl(src.as_bytes());
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
    let (echoes, stderr) = repl(
        "mut x := i32 5\nx = 6\np := &x\nprint «{p@}»\nw := type (y := i64 ?)\nq := w(7)\nr := &q\n\
          print «{r@.y}»\nmut a := type ?\nmut b := type ?\na = i32\nb = a\nprint «{b == i32}»\n\
          mut d := dyad ?\nd = i32\nd.type == type\n",
    );
    assert_eq!(echoes, ["6", "7", "true", "true"], "stderr: {stderr}");
    // A value nothing names is held in a place of its own, whose address `&` hands out.
    let (echoes, stderr) =
        repl("x := &(i32 5)\nprint «{x@}»\n".to_owned() + pw + "\np := &(2 pw 3)\np@\n");
    assert_eq!(echoes, ["5", "6"], "stderr: {stderr}");
}

#[test]
fn an_element_has_an_address_and_taking_it_checks_the_index() {
    let arr = "import file «./identities/array.logos»";
    assert_eq!(line(&format!("{arr}, a := array i32 [1, 2], p := &(a[1]), p@ = 8, a[1]")), "8");
    assert_eq!(
        line(&format!(
            "{arr}, g := fn () -> i32 ( a := array i32 [1, 2], p := &(a[1]), p@ ), g.compile(), g()"
        )),
        "2"
    );
    for src in [
        format!("{arr}, a := array i32 [1, 2], p := &(a[5]), 1"),
        format!(
            "{arr}, g := fn () -> i32 ( a := array i32 [1, 2], p := &(a[5]), 1 ), g.compile(), g()"
        ),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains("index out of range"), "{src}: stderr: {stderr}");
    }
    let (echoes, stderr) = repl(b"a := alloc 1 of i32 3\np := &(a@)\np@ = 9\na@\n");
    assert_eq!(echoes, ["9"], "stderr: {stderr}");
}

#[test]
fn declaring_from_a_box_copies_it() {
    let (echoes, stderr) = repl(
        "mut a := type ?\na = i32\nmut x := a\nx = f64\nprint «{a == i32}»\nprint «{x == f64}»\n\
          mut d := dyad ?\nd = i32\nmut e := d\ne = f64\nprint «{d == i32}»\ne == f64\n",
    );
    assert_eq!(echoes, ["true", "true", "true", "true"], "stderr: {stderr}");
    let (echoes, stderr) = repl(b"t := i32\ny := t 5\ny\n");
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    let (_e, stderr) = repl(b"b := scope ?\n");
    assert!(stderr.contains("not in the seed yet"), "stderr: {stderr}");
}

#[test]
fn a_dyad_is_built_from_a_type_and_a_value() {
    let (echoes, stderr) = repl(
        "c := dyad (i32, 7)\nprint «{c}»\nprint «{c.type == i32}»\n\
              print «{dyad (i32, 7).type == i32}»\nc + 1\n",
    );
    assert_eq!(echoes, ["7", "true", "true", "8"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
    let (_e, stderr) = repl(b"dyad (i32)\n");
    assert!(!stderr.is_empty(), "two operands, not one");

    // A bool cell used to carry the node as its value, so every bool dyad read true.
    let (echoes, stderr) = repl("print «{dyad (bool, true)}»\ndyad (bool, false)\n");
    assert_eq!(echoes, ["true", "false"], "stderr: {stderr}");

    for src in [
        &b"dyad (bool, 0)\n"[..],
        b"p := dyad (@i32, 5)\n",
        b"dyad (void, 0)\n",
        b"g := type (x := i32 ?)\ndyad (g, 5)\n",
        b"dyad (i32, f64 2.5)\n",
        b"dyad (dyad, 5)\n",
    ] {
        let (echoes, stderr) = repl(src);
        assert!(
            echoes.is_empty() && !stderr.is_empty(),
            "{}: stderr: {stderr}",
            String::from_utf8_lossy(src)
        );
    }
}

#[test]
fn a_dyad_of_a_type_is_a_new_node_each_time_its_line_runs() {
    for (src, want) in [
        ("m := i32 3, d := dyad (i32, m), d", "3"),
        ("d := dyad (i32, i32 4), d", "4"),
        ("d := dyad (i32, 7), d", "7"),
        ("d := dyad (i32, 7), d.type", "i32"),
        ("m := i32 3, (dyad (i64, m)).type", "i64"),
        ("mut t := type ?, t = i32, d := dyad (t, 7), d", "7"),
        ("f := fn (m := i32 ?) -> i32 ( d := dyad (i32, m), d + 1 ), f(i32 3)", "4"),
        ("g := fn (m := i32 ?) -> i32 ( d := dyad (i32, m), 0 ), g.compile(), g(i32 3)", "0"),
    ] {
        assert_eq!(line(src), want, "{src}");
    }
    let mk = "mk := fn (m := i32 ?) -> dyad ( dyad (i32, m) )";
    for c in ["", "mk.compile(), "] {
        for (src, want) in [
            (format!("{mk}, {c}a := mk(i32 3), b := mk(i32 4), print «{{a}} {{b}}»"), "3 4"),
            (format!("{mk}, {c}x := mk(i32 9), x.type"), "i32"),
        ] {
            assert_eq!(line(&src), want, "{src}");
        }
    }
    // A type a body's box holds only when the program runs: stand-in for #292.
    let (code, _, stderr) = run_line("f := fn (t := type ?) -> dyad ( dyad (t, 7) ), f(i32)");
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("known only when the program runs"), "stderr: {stderr}");
}

#[test]
fn an_operator_reads_through_a_dyad_no_name_holds() {
    let g = "g := fn () -> dyad ( i32 5 )";
    for (src, want) in [
        ("dyad (i32, 7) + 1".to_string(), "8"),
        ("1 + dyad (i32, 7)".to_string(), "8"),
        ("dyad (i64, 7) * dyad (i64, 6)".to_string(), "42"),
        ("dyad (rational_number, 1/2) * 4".to_string(), "2"),
        ("x := dyad (i32, 7) == i32 7, x".to_string(), "true"),
        (
            "f := fn (m := i32 ?) -> i32 ( dyad (i32, m) + 1 ), f.compile(), f(i32 3)".to_string(),
            "4",
        ),
        (format!("{g}, g() + 1"), "6"),
        (format!("{g}, print «{{1 + g()}}»"), "6"),
        (format!("{g}, f := fn () -> i32 ( g() + 1 ), f()"), "6"),
        (
            format!(
                "{g}, f := fn () -> i32 ( g() + 1 ), h := fn () -> i32 ( f() ), h.compile(), h()"
            ),
            "6",
        ),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
    // What runs in the call's line runs in the order it is written, the lines before it first,
    // and the part of its line before it too, inside a bracket around the call as well.
    let calls = "h := fn () -> i32 ( print «h», 1 ), g := fn () -> dyad ( print «g», i32 2 )";
    let k1 = "k := fn (a := i32 ?) -> i32 ( print «k», a )";
    for (src, want) in [
        (format!("{calls}, g() + h()"), "g\nh\n3"),
        (format!("{calls}, x := ( free (h()), g() + 1 ), x"), "h\ng\n3"),
        (format!("{calls}, v := g(), h() + v"), "g\nh\n3"),
        (
            format!("{calls}, k := fn (a := i32 ?, b := i32 ?) -> i32 ( a + b ), k(1, g() + 1)"),
            "g\n4",
        ),
        (format!("{calls}, h() + g()"), "h\ng\n3"),
        (format!("{calls}, h() * 2 + g()"), "h\ng\n4"),
        (format!("{calls}, h() + (g() * 2)"), "h\ng\n5"),
        (format!("{calls}, (free (h()), 1) + g()"), "h\ng\n3"),
        (format!("{calls}, g() + h() + g()"), "g\nh\ng\n5"),
        (format!("{calls}, h() + (h() * (g() + 1))"), "h\nh\ng\n4"),
        (format!("{calls}, x := h() + (g() + 1), x"), "h\ng\n4"),
        (format!("{calls}, {k1}, h() + k(g() + 1)"), "h\ng\nk\n4"),
        (format!("{calls}, h() == 1 and g().type == i32"), "h\ng\ntrue"),
        (format!("{calls}, print «{{h() + g()}}»"), "h\ng\n3"),
        (format!("{calls}, print «{{h() + (g() + 1)}}»"), "h\ng\n4"),
        (format!("{calls}, k := fn () -> i32 ( h() + g() ), k()"), "h\ng\n3"),
        (format!("{calls}, k := fn () -> i32 ( h() + (g() + 1) ), k()"), "h\ng\n4"),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
    // An earlier argument runs before the call too, once: the call reads the value it gave.
    let k2 = "k := fn (a := i32 ?, b := i32 ?) -> i32 ( a + b )";
    let k3 = "k3 := fn (a := i32 ?, b := i32 ?, c := i32 ?) -> i32 ( a + b + c )";
    let arrays = "import file «./identities/array.logos»";
    for (src, want) in [
        (format!("{calls}, {k2}, k(h(), g() + 1)"), "h\ng\n4"),
        (format!("{calls}, {k2}, k(h(), (g() + 1))"), "h\ng\n4"),
        (format!("{calls}, {k2}, h() + k(h(), k(h(), g() + 1))"), "h\nh\nh\ng\n6"),
        (format!("{calls}, {k3}, k3(h(), g() + 1, h())"), "h\ng\nh\n5"),
        (format!("{calls}, {k2}, f := fn () -> i32 ( k(h(), g() + 1) ), f()"), "h\ng\n4"),
        (
            format!("{arrays}, {calls}, c := array i32 [h(), g() + 1], print «{{c[0]}} {{c[1]}}»"),
            "h\ng\n1 3",
        ),
        (
            format!(
                "{arrays}, {calls}, ka := fn (a := array i32 ?, b := i32 ?) -> i32 ( a[0] + b ), \
                 ka(array i32 [h(), 2], g() + 1)"
            ),
            "h\ng\n4",
        ),
        (
            format!(
                "{calls}, t := fn () -> string ( print «t», «x» ), \
                 kt := fn (a := string ?, b := i32 ?) -> i32 ( print «{{a}}», b ), kt(t(), g() + 1)"
            ),
            "t\ng\nx\n3",
        ),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
    // An earlier part of a quote runs before the call too, once: the word reads the value it gave.
    for (src, want) in [
        (format!("{calls}, print «{{h()}} {{g() + 1}}»"), "h\ng\n1 3"),
        (format!("{calls}, print «a {{h()}} b {{«x»}} c {{g() + 1}}»"), "h\ng\na 1 b x c 3"),
        (format!("{calls}, print «{{h()}} {{h()}} {{g() + 1}}»"), "h\nh\ng\n1 1 3"),
        (format!("{calls}, {k2}, print «{{h()}} {{k(h(), g() + 1)}}»"), "h\nh\ng\n1 4"),
        (format!("{calls}, print «{{h()}} {{( print «in», g() + 1 )}}»"), "h\nin\ng\n1 3"),
        (
            format!("{calls}, f := fn () -> i32 ( print «{{h()}} {{g() + 1}}», 7 ), f()"),
            "h\ng\n1 3\n7",
        ),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
    let (code, stdout, stderr) = run_line(&format!("{calls}, error «{{h()}} {{g() + 1}}»"));
    assert_eq!((code, stdout.as_str()), (Some(1), "h\ng\n"), "stderr: {stderr}");
    assert!(stderr.contains("1 3"), "stderr: {stderr}");
    // A name read before the call is read before the call writes it.
    let writes = "mut n := i32 1, c := fn () -> dyad ( n = 5, i32 1 )";
    for (src, want) in [
        (format!("{writes}, n + c()"), "2"),
        (format!("{writes}, n = n + c(), n"), "2"),
        (format!("{writes}, x := n + c(), print «{{x}} {{n}}»"), "2 5"),
        (format!("{writes}, f := fn () -> i32 ( n + c() ), f()"), "2"),
        (format!("{writes}, {k2}, k(n, c() + 1)"), "3"),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
    // What stands before the call cannot run first yet: a node with no operation built yet, and
    // what stands before a branch.
    for (src, refusal) in [
        (format!("{calls}, x := [h(), 2], x[0] + (g() + 1)"), "cannot run first yet"),
        (format!("{calls}, x := h() + if true (g() + 1) else (0), x"), "cannot run first yet"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stdout.is_empty(), "{src}: stdout: {stdout}");
        assert!(stderr.contains(refusal), "{src}: {stderr}");
    }
    // A `while` condition that holds the call is held before it is parsed, so the call runs once
    // each time the condition runs, at the top level and in a body; a `[` in it is no body.
    let count = "mut n := i32 0, c := fn () -> dyad ( n = n + 1, i32 3 ), mut i := i32 0";
    let array = "import file «./identities/array.logos», a := array i32 [1, 2, 3]";
    for (src, want) in [
        (format!("{g}, mut i := i32 0, while i < g() ( i = i + 1 ), i"), "5"),
        (format!("{count}, while i < c() ( i = i + 1 ), n"), "4"),
        (format!("{count}, while c() > i ( i = i + 1 ), n"), "4"),
        (format!("{count}, while i < c() and i < 9 ( i = i + 1 ), n"), "4"),
        (format!("{count}, while (i < c()) ( i = i + 1 ), n"), "4"),
        (format!("{array}, {count}, while c() > a[0] + i ( i = i + 1 ), n"), "3"),
        (format!("{array}, {count}, while a[0] + i < c() ( i = i + 1 ), n"), "3"),
        (
            format!(
                "{g}, f := fn () -> i32 ( mut i := i32 0, while i < g() ( i = i + 1 ), i ), f()"
            ),
            "5",
        ),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
    // A call read through a bracket around it is not seen by the scan that holds the condition.
    let (code, _, stderr) =
        run_line(&format!("{g}, mut i := i32 0, while i < (g()) ( i = i + 1 ), i"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("through a bracket or a member"), "stderr: {stderr}");
}

#[test]
fn a_box_read_and_a_call_for_a_type_run_after_the_part_of_their_line_before_them() {
    let h = "mut t := type ?, t = i64, h := fn () -> i32 ( print «h», t = i32, 1 )";
    let mk = "mk := fn () -> type ( print «mk», t )";
    let mk_of = "mk := fn (u := type ?) -> type ( print «mk», u )";
    let d = "mut d := dyad ?, d = i64 5, h := fn () -> i32 ( print «h», d = i32 7, 1 )";
    let is_i32 = "is_i32 := fn (u := type ?) -> bool ( u == i32 )";
    for (src, want) in [
        (format!("{h}, x := h() + t 5, x.type"), "h\ni32"),
        (format!("{h}, x := h() + (t 5), x.type"), "h\ni32"),
        (format!("{h}, x := h() + (1 + (t 5)), x.type"), "h\ni32"),
        (format!("{h}, x := h() + h() + t 5, x.type"), "h\nh\ni32"),
        (format!("{h}, x := h() + t 5, y := t 6, print «{{x}} {{y.type}}»"), "h\n6 i32"),
        (format!("{h}, {mk}, h() == 1 and mk() == i32"), "h\nmk\ntrue"),
        (format!("{h}, {mk}, h() == 1 and (mk() == i32)"), "h\nmk\ntrue"),
        (format!("{h}, {mk_of}, h() == 1 and mk(t) == i32"), "h\nmk\ntrue"),
        (format!("{h}, h() == 1 and t == i32"), "h\ntrue"),
        (format!("{h}, print «{{h()}} {{t}}»"), "h\n1 i32"),
        (format!("{h}, print «{{h()}} {{t}} {{t.type}}»"), "h\n1 i32 type"),
        (format!("{h}, print «{{h()}} {{u := t, u}}»"), "h\n1 i32"),
        (format!("{h}, {is_i32}, print «{{h()}} {{is_i32(t)}}»"), "h\n1 true"),
        (format!("{h}, print «{{h()}} {{t}}», x := t 5, x.type"), "h\n1 i32\ni32"),
        (format!("{d}, x := h() + d, x"), "h\n8"),
        (format!("{d}, x := h() + (1 + d), x"), "h\n9"),
        // An earlier part of a quote runs first too, where the parse reads the value itself, or
        // the type a box holds.
        (format!("{d}, print «{{h()}} {{d}}»"), "h\n1 7"),
        (format!("{d}, print «{{h()}} {{d}} {{h()}} {{d}}»"), "h\nh\n1 7 1 7"),
        (format!("{h}, {mk}, print «{{h()}} {{mk()}}»"), "h\nmk\n1 i32"),
        (format!("{h}, print «{{h()}} {{(t 5).type}}»"), "h\n1 i32"),
        (format!("{h}, print «{{h()}} {{t}} {{t 5}}»"), "h\n1 i32 5"),
        (format!("{h}, print «{{h()}} {{dyad (t, 7)}}»"), "h\n1 7"),
        (format!("{h}, print «{{h()}} {{t.size_bytes}}»"), "h\n1 4"),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
    // A hole the box's type makes, after an earlier part ran, is the run's error a hole alone is.
    let (code, stdout, stderr) = run_line(&format!("{h}, print «{{h()}} {{t ?}}»"));
    assert_eq!((code, stdout.as_str()), (Some(1), "h\n"), "stderr: {stderr}");
    assert!(stderr.contains("a record or hole is not read as one value"), "stderr: {stderr}");
}

#[test]
fn a_constructor_a_member_call_for_a_type_and_an_import_run_after_the_part_of_their_line_before_them(
) {
    let h = "h := fn () -> i32 ( print «h», 1 )";
    let q = "q := type ( v := i32 ?, static parse_rank = 61, static parse = ( print «q», \
             tape[0].type = q, tape[0].v = tape[1].dyads[0], tape.remove(1), \
             tape.is_constructed[0] = true ) )";
    let o = "o := type ( static m := fn () -> type ( print «m», i32 ) )";
    let imp = "import file «./tests/fixtures/prints.logos»";
    let read = "(logos file «./tests/fixtures/prints_pub.logos»).v";
    let array = "import file «./identities/array.logos»";
    let boxed = "mut t := type ?, t = i64, h := fn () -> i32 ( print «h», t = i32, 1 )";
    let count = "count := type ( static parse_rank = *.parse_rank + 1, \
                 static parse = ( tape.remove(1), tape.remove(0) ) )";
    for (src, want) in [
        (format!("{h}, {q}, h() + (q [5, i32 7]).v"), "h\nq\n6"),
        (format!("{h}, {q}, x := h() + (q [5, i32 7]).v, x"), "h\nq\n6"),
        (format!("{h}, {o}, h() == 1 and o.m() == i32"), "h\nm\ntrue"),
        (format!("{h}, {o}, h() == 1 and (o.m() == i32)"), "h\nm\ntrue"),
        (format!("{h}, h() + ({imp} , i32 1)"), "h\nimp\n2"),
        (format!("{h}, h() + {read}"), "h\nimp\n3"),
        (
            format!("{h}, k := fn (a := i32 ?, b := i32 ?) -> i32 ( a + b ), k(h(), {read})"),
            "h\nimp\n3",
        ),
        (format!("{array}, {h}, l := array i32 [4, 5], h() + l[1]"), "h\n6"),
        (format!("{array}, l := array i32 [4, 5], print «{{l[0]}} {{l[1]}}»"), "4 5"),
        // In a later part of a quote, after the earlier parts.
        (format!("{h}, {q}, print «{{h()}} {{(q [5, i32 7]).v}}»"), "h\nq\n1 5"),
        (format!("{h}, {o}, print «{{h()}} {{o.m() == i32}}»"), "h\nm\n1 true"),
        (format!("{h}, print «{{h()}} {{({imp} , i32 1)}}»"), "h\nimp\n1 1"),
        (format!("{h}, print «{{h()}} {{{read}}}»"), "h\nimp\n1 2"),
        (format!("{array}, {h}, l := array i32 [4, 5], print «{{h()}} {{l[1]}}»"), "h\n1 5"),
        (
            format!("{h}, {q}, f := fn () -> i32 ( print «{{h()}} {{(q [5, i32 7]).v}}», 2 ), f()"),
            "q\nh\n1 5\n2",
        ),
        // A line a constructor took whole leaves nothing of it to run.
        (format!("{count}, {boxed}, count [1, 2], x := h() + t 5, x.type"), "h\ni32"),
        // What a run builds on a tape of its own, the line does not run.
        (
            format!(
                "{array}, {boxed}, a := array (array i32) [[1, 2], [3]], \
                 x := h() + a[0][1] + t 5, x.type"
            ),
            "h\ni32",
        ),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
}

#[test]
fn an_unnamed_call_for_a_type_reads_what_follows_it_as_a_name_holding_the_type_does() {
    let mk = "mk := fn () -> type ( i32 )";
    let mk_of = "mk := fn (u := type ?) -> type ( u )";
    let boxed = "mut t := type ?, t = i32, mk := fn () -> type ( t )";
    let o = "o := type ( static m := fn () -> type ( i32 ) )";
    let o_by_name = format!("{o}, k := o.m");
    let pt = "pt := type ( mut a := i32 ? ), mk := fn () -> type ( pt )";
    let h = "h := fn () -> i32 ( print «h», 1 ), mk := fn () -> type ( print «mk», i32 )";
    for (named, unnamed, want) in [
        (format!("{mk}, u := mk(), u 3"), format!("{mk}, mk() 3"), "3"),
        (format!("{boxed}, u := mk(), u 3"), format!("{boxed}, mk() 3"), "3"),
        (format!("{mk}, u := mk(), x := u 3, x.type"), format!("{mk}, x := mk() 3, x.type"), "i32"),
        (
            format!("{mk_of}, u := mk(i64), x := u 3, x.type"),
            format!("{mk_of}, x := mk(i64) 3, x.type"),
            "i64",
        ),
        (format!("{o}, u := o.m(), u 3"), format!("{o}, o.m() 3"), "3"),
        (format!("{mk}, u := mk(), u(7)"), format!("{mk}, mk()(7)"), "7"),
        (format!("{pt}, u := mk(), v := u(4), v.a"), format!("{pt}, v := mk()(4), v.a"), "4"),
        (format!("{mk}, u := mk(), u"), format!("{mk}, mk()"), "i32"),
        (format!("{mk}, u := mk(), u == i32"), format!("{mk}, mk() == i32"), "true"),
        (format!("{mk}, u := mk(), print «{{u}}»"), format!("{mk}, print «{{mk()}}»"), "i32"),
        (
            format!("{mk}, u := mk(), mut x := u ?, x = 4, x"),
            format!("{mk}, mut x := mk() ?, x = 4, x"),
            "4",
        ),
    ] {
        assert_eq!(line(&named), want, "{named}");
        assert_eq!(line(&unnamed), want, "{unnamed}");
    }
    // The call runs where the parse reads it, after the part of its line before it.
    assert_eq!(line(&format!("{h}, x := h() + mk() 5, x")), "h\nmk\n6");
    // Inside a body too, on both tiers.
    for (defs, out, named, unnamed, want) in [
        (mk, "i32", "u := mk(), u 3", "mk() 3", "3"),
        (boxed, "i32", "u := mk(), u 3", "mk() 3", "3"),
        (mk, "type", "u := mk(), x := u 3, x.type", "x := mk() 3, x.type", "i32"),
        (mk_of, "type", "u := mk(i64), x := u 3, x.type", "x := mk(i64) 3, x.type", "i64"),
        (o, "i32", "u := o.m(), u 3", "o.m() 3", "3"),
        (o_by_name.as_str(), "i32", "u := k(), u 3", "k() 3", "3"),
        (mk, "i32", "u := mk(), u(7)", "mk()(7)", "7"),
        (pt, "i32", "u := mk(), v := u(4), v.a", "v := mk()(4), v.a", "4"),
        (mk, "type", "u := mk(), u", "mk()", "i32"),
        (mk, "bool", "u := mk(), u == i32", "mk() == i32", "true"),
        (mk, "i32", "u := mk(), mut x := u ?, x = 4, x", "mut x := mk() ?, x = 4, x", "4"),
        (mk, "i32", "if true ( u := mk(), u 3 ) else ( 4 )", "if true ( mk() 3 ) else ( 4 )", "3"),
    ] {
        for body in [named, unnamed] {
            for call in ["f()", "f.compile(), f()"] {
                let src = format!("{defs}, f := fn () -> {out} ( {body} ), {call}");
                assert_eq!(line(&src), want, "{src}");
            }
        }
    }
    // In a body the call runs where the body is written; what stands before it waits for f's call.
    let src = format!("{h}, f := fn () -> i32 ( h() + mk() 5 ), print «defined», f()");
    assert_eq!(line(&src), "mk\ndefined\nh\n6");
    let om = "o := type ( static m := fn () -> type ( print «m», i32 ) )";
    let src = format!("{om}, f := fn () -> i32 ( o.m() 3 ), print «defined», f()");
    assert_eq!(line(&src), "m\ndefined\n3");
}

#[test]
fn a_member_holding_a_type_reads_what_follows_it_as_a_name_holding_the_type_does() {
    let o = "o := type ( static e := i32 )";
    let pt = "pt := type ( mut a := i32 ? ), o := type ( static e := pt )";
    for (defs, out, named, unnamed, want) in [
        (o, "i32", "u := o.e, u 3", "o.e 3", "3"),
        (o, "type", "u := o.e, x := u 3, x.type", "x := o.e 3, x.type", "i32"),
        (o, "i32", "u := o.e, u(7)", "o.e(7)", "7"),
        (pt, "i32", "u := o.e, v := u(4), v.a", "v := o.e(4), v.a", "4"),
        (o, "type", "u := o.e, u", "o.e", "i32"),
        (o, "bool", "u := o.e, u == i32", "o.e == i32", "true"),
        (o, "i32", "u := o.e, mut x := u ?, x = 4, x", "mut x := o.e ?, x = 4, x", "4"),
    ] {
        for body in [named, unnamed] {
            let src = format!("{defs}, {body}");
            assert_eq!(line(&src), want, "{src}");
            for call in ["f()", "f.compile(), f()"] {
                let src = format!("{defs}, f := fn () -> {out} ( {body} ), {call}");
                assert_eq!(line(&src), want, "{src}");
            }
        }
    }
}

#[test]
fn a_bracket_holding_a_type_reads_what_follows_it_as_a_name_holding_the_type_does() {
    let none = "z := i32 0";
    let mk = "mk := fn () -> type ( i32 )";
    let pt = "pt := type ( mut a := i32 ? )";
    let o = "o := type ( static e := i32 )";
    for (defs, out, named, unnamed, want) in [
        (none, "i32", "u := (i32), u 3", "(i32) 3", "3"),
        (none, "type", "u := (i32), x := u 3, x.type", "x := (i32) 3, x.type", "i32"),
        (none, "i32", "u := i32, v := (u), v 3", "u := i32, (u) 3", "3"),
        (mk, "i32", "u := (mk()), u 3", "(mk()) 3", "3"),
        (mk, "i32", "u := (immediate mk()), u 3", "(immediate mk()) 3", "3"),
        (pt, "i32", "u := (pt), v := u(4), v.a", "v := (pt)(4), v.a", "4"),
        (o, "i32", "u := (o.e), u 3", "(o.e) 3", "3"),
        (none, "type", "u := (i32), u", "(i32)", "i32"),
    ] {
        for body in [named, unnamed] {
            let src = format!("{defs}, {body}");
            assert_eq!(line(&src), want, "{src}");
            for call in ["f()", "f.compile(), f()"] {
                let src = format!("{defs}, f := fn () -> {out} ( {body} ), {call}");
                assert_eq!(line(&src), want, "{src}");
            }
        }
    }
    // A bracket an identity to its left takes stays that identity's argument.
    let k = "k := fn (t := type ?) -> type ( t )";
    assert_eq!(line(&format!("{k}, k (i64) 3")), "3");
    assert_eq!(line(&format!("{k}, x := k (i64) 3, x.type")), "i64");
}

#[test]
fn a_word_that_finds_nothing_to_its_right_stands_as_its_own_value() {
    // Before a boundary, or an operator that reads to its left, each word that reads its own
    // right side declines, its identity standing as its own value.
    for word in [
        "fn",
        "if",
        "while",
        "for",
        "defer",
        "immediate",
        "type",
        "scope",
        "dyad",
        "print",
        "lex",
        "file",
        "error",
        "regex",
        "import",
        "logos",
    ] {
        for src in [format!("x := {word}, x == {word}"), format!("x := ({word}), x == {word}")] {
            assert_eq!(line(&src), "true", "{src}");
        }
        assert_eq!(line(&format!("print «{{{word}}}»")), line(word), "{word}");
    }
    // `.type` hands the type on as a name holding it does, named or not, at top level and in a
    // body on both tiers.
    let a = "a := i32 5";
    let f = "f := fn (a := i32 ?) -> i32 ( a + 1 )";
    let c = "mut c := true";
    let arms = ["(if c (i32 1) else (i64 2))", "(if c (u8 1) else (i32 2))"];
    let if_named = format!("t := {}.type, t == {}.type", arms[0], arms[1]);
    let if_unnamed = format!("{}.type == {}.type", arms[0], arms[1]);
    for (defs, out, named, unnamed, want) in [
        (a, "i32", "t := a.type, t 3", "a.type 3", "3"),
        (f, "bool", "t := f.type, t == f.type", "f.type == f.type", "true"),
        (f, "bool", "t := f.type, t == fn", "f.type == fn", "true"),
        (f, "bool", "t := f.type, t == i32", "f.type == i32", "false"),
        (c, "bool", if_named.as_str(), if_unnamed.as_str(), "true"),
        (
            c,
            "bool",
            "t := (if c (i32 1) else (i64 2)).type, t == if",
            "(if c (i32 1) else (i64 2)).type == if",
            "true",
        ),
    ] {
        for body in [named, unnamed] {
            let src = format!("{defs}, {body}");
            assert_eq!(line(&src), want, "{src}");
            for call in ["g()", "g.compile(), g()"] {
                let src = format!("{defs}, g := fn () -> {out} ( {body} ), {call}");
                assert_eq!(line(&src), want, "{src}");
            }
        }
    }
    // Standing alone, or in a quote, it shows as the word itself does.
    for (src, word) in [
        (format!("{f}, t := f.type, t"), "fn"),
        (format!("{f}, t := f.type, print «{{t}}»"), "fn"),
        (format!("{c}, t := (if c (i32 1) else (i64 2)).type, t"), "if"),
        (format!("{c}, t := (if c (i32 1) else (i64 2)).type, print «{{t}}»"), "if"),
    ] {
        assert_eq!(line(&src), line(word), "{src}");
    }
    // Before the body of a complete condition, and after `->`, it stands; a `-` after it negates.
    assert_eq!(line(&format!("{f}, if f.type == fn ( print «fn» )")), "fn");
    assert_eq!(line(&format!("{f}, g := f, if g.type == f.type ( print «same» )")), "same");
    assert_eq!(line(&format!("{f}, g := fn () -> f.type ( f ), h := g(), h.type == fn")), "true");
    assert_eq!(line("mut x := i32 3, if -x < 0 ( print «neg» )"), "neg");
    // Where its syntax has started, or something else stands to its right, it keeps its error.
    for src in [
        "fn (a := i32 ?) -> i32".to_string(),
        "fn 3".into(),
        "fn -> i32 ( 1 )".into(),
        format!("{f}, t := f.type, t 3"),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: {stderr}");
        assert!(stderr.contains("expected a `(` here"), "{src}: {stderr}");
    }
}

#[test]
fn a_value_the_parse_needs_in_an_argument_list_runs_after_the_earlier_arguments_once() {
    // Each point where the parse needs a value runs the earlier arguments first, each then
    // standing for the value it gave, and the call runs them no more.
    let defs =
        "h := fn () -> i32 ( print «h», 1 ), k := fn (a := i32 ?, b := i32 ?) -> i32 ( a + b )";
    let g = "g := fn () -> i32 ( print «g», 2 )";
    let mk =
        "mk := fn () -> type ( print «mk», i32 ), kt := fn (a := i32 ?, b := type ?) -> i32 ( a )";
    let q = "q := type ( v := i32 ?, static parse_rank = 61, static parse = ( print «q», \
             tape[0].type = q, tape[0].v = tape[1].dyads[0], tape.remove(1), \
             tape.is_constructed[0] = true ) )";
    for (src, want) in [
        (format!("{defs}, d := dyad (i32, 7), k(h(), d + 1)"), "h\n9"),
        (format!("{defs}, mut t := type ?, t = i32, k(h(), t 5)"), "h\n6"),
        (format!("{defs}, d := dyad (i32, 7), k(h(), k(h(), d + 1))"), "h\nh\n10"),
        (format!("{defs}, d := dyad (i32, 7), x := k(h(), d + 1), x"), "h\n9"),
        (format!("{defs}, d := dyad (i32, 7), x := ( free (h()), k(h(), d + 1) ), x"), "h\nh\n9"),
        (format!("{defs}, d := dyad (i32, 7), f := fn () -> i32 ( k(h(), d + 1) ), f()"), "h\n9"),
        (format!("{defs}, {g}, k(h(), immediate g())"), "h\ng\n3"),
        (format!("{defs}, {g}, print «{{h()}} {{immediate g()}}»"), "h\ng\n1 2"),
        (format!("{defs}, {mk}, kt(h(), mk())"), "h\nmk\n1"),
        (format!("{defs}, {g}, {mk}, kt(h(), type ( static s := g() ))"), "h\ng\n1"),
        (format!("{defs}, {q}, k(h(), (q [5, i32 7]).v)"), "h\nq\n6"),
        (
            "mut t := type ?, t = i64, h := fn () -> i64 ( t = i32, 1 ), \
             k := fn (a := i64 ?, b := dyad ?) -> dyad ( b ), x := k(h(), t 5), x.type"
                .to_string(),
            "i32",
        ),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
    // An earlier argument with an end of its own stands in its place too, and the call that holds
    // it, or the line's end, ends it once.
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) ), \
               mb := fn () -> own bag ( print «mb», bag(4) ), \
               kb := fn (a := bag ?, b := i32 ?, c := i32 ?) -> i32 ( b + c )";
    let gd = "gd := fn () -> dyad ( print «g», i32 2 )";
    let k2 = "k2 := fn (a := bag ?, b := i32 ?) -> i32 ( b )";
    for (src, want) in [
        (format!("{defs}, {bag}, d := dyad (i32, 7), kb(mb(), h(), d + 1)"), "mb\nh\nfreed 4\n9"),
        (format!("{defs}, {g}, {bag}, kb(mb(), h(), immediate g())"), "mb\nh\ng\nfreed 4\n3"),
        (format!("{bag}, {gd}, {k2}, k2(mb(), gd() + 1)"), "mb\ng\nfreed 4\n3"),
        (
            format!("{bag}, {gd}, {k2}, f := fn () -> i32 ( k2(mb(), gd() + 1) ), f()"),
            "mb\ng\nfreed 4\n3",
        ),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
    // A fault after it ran ends it too.
    let src = format!("{bag}, {gd}, {k2}, k2(mb(), gd() + «x»)");
    let (code, stdout, stderr) = run_line(&src);
    assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
    assert_eq!(stdout, "mb\ng\nfreed 4\n", "{src}");
    // A loop's condition runs on every pass, its arguments with it.
    let src =
        format!("{defs}, {g}, mut i := i32 0, while i < k(h(), immediate g()) ( i = i + 1 ), i");
    assert_eq!(line(&src), "g\nh\nh\nh\nh\n3", "{src}");
}

#[test]
fn a_comment_leading_a_line_leaves_that_line_what_it_made_and_runs() {
    let calls = "h := fn () -> i32 ( print «h», 1 ), g := fn () -> dyad ( print «g», i32 2 ), \
                 k := fn (a := i32 ?, b := i32 ?) -> i32 ( a + b )";
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) )";
    for (src, want) in [
        (format!("{calls}, k(# «a» h(), g() + 1)"), "h\ng\n4"),
        (format!("{calls}, d := dyad (i32, 7), k(# «a» h(), d + 1)"), "h\n9"),
        (format!("{bag}, # «a» free (bag(3)), print «x»"), "freed 3\nx"),
    ] {
        assert_eq!(line(&src), want, "{src}");
    }
}

#[test]
fn a_dyad_no_name_holds_is_read_through_as_a_named_one_is() {
    let g = "g := fn (q := i32 ?) -> i32 ( q + 1 )";
    let f = "f := fn () -> dyad ( i32 5 )";
    let defs = format!("{g}, {f}");
    for (named, unnamed, want) in [
        ("d := dyad (i32, 7), g(d)", "g(dyad (i32, 7))", "8"),
        ("d := f(), g(d)", "g(f())", "6"),
        (
            "d := dyad (i32, 7), mut x := i32 ?, x = d, x",
            "mut x := i32 ?, x = dyad (i32, 7), x",
            "7",
        ),
        ("d := f(), mut x := i64 ?, x = d, x", "mut x := i64 ?, x = f(), x", "5"),
        ("d := dyad (i32, 7), x := i32 d, x", "x := i32 dyad (i32, 7), x", "7"),
        ("d := dyad (f64, 5.5), x := i32 lossy (d), x", "x := i32 lossy (dyad (f64, 5.5)), x", "5"),
        (
            "d := f(), print «{d.type == i32} {d.type == dyad}»",
            "print «{f().type == i32} {f().type == dyad}»",
            "true false",
        ),
    ] {
        for src in [format!("{defs}, {named}"), format!("{defs}, {unnamed}")] {
            assert_eq!(line(&src), want, "{src}");
        }
    }
    // A node whose type is known where it is written is read when its line runs, on both tiers.
    for (body, call, want) in [
        ("h := fn (m := i32 ?) -> i32 ( dyad (i32, m) )", "h(i32 3)", "3"),
        ("h := fn (m := i32 ?) -> i32 ( g(dyad (i32, m)) )", "h(i32 3)", "4"),
        ("h := fn (m := i32 ?) -> i64 ( mut x := i64 ?, x = dyad (i32, m), x )", "h(i32 3)", "3"),
        ("h := fn () -> rational_number ( dyad (rational_number, 1/2) )", "h()", "1/2"),
    ] {
        for src in
            [format!("{defs}, {body}, {call}"), format!("{defs}, {body}, h.compile(), {call}")]
        {
            assert_eq!(line(&src), want, "{src}");
        }
    }
    // One whose type is known only at run lands there when its line runs, in the order written.
    let calls = "h := fn () -> i32 ( print «h», 1 ), k := fn () -> dyad ( print «k», i32 2 )";
    let sum = "s := fn (a := i32 ?, b := i32 ?) -> i32 ( a + b )";
    assert_eq!(line(&format!("{calls}, {sum}, s(h(), k())")), "h\nk\n3");
    let half = "r := fn () -> dyad ( rational_number 3/4 )";
    for (src, want) in [
        ("q := fn (a := i64 ?) -> i64 ( a ), q(f())", "5"),
        ("x := i64 (f()), x", "5"),
        ("e := fn () -> dyad ( f64 5.5 ), x := i32 lossy (e()), x", "5"),
        ("h := fn () -> rational_number ( r() ), h()", "3/4"),
        ("h := fn () -> i32 ( g(f()) ), h()", "6"),
        ("h := fn () -> i32 ( g(f()) ), m := fn () -> i32 ( h() ), m.compile(), m()", "6"),
    ] {
        let src = format!("{defs}, {half}, {src}");
        assert_eq!(line(&src), want, "{src}");
    }
    let (code, _, stderr) = run_line(&format!("{g}, e := fn () -> dyad ( i64 5 ), g(e())"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("these types do not match"), "stderr: {stderr}");
    // A body's line that reads a call's node through `.` waits for the run.
    assert_eq!(line(&format!("{f}, h := fn () -> bool ( f().type == i32 ), h()")), "true");
    // So does one in a function written in a type's body.
    let t = "t := type ( n := i64 ?, static k := fn () -> bool ( f().type == i32 ) )";
    assert_eq!(line(&format!("{f}, {t}, v := t (3), v.k()")), "true");
}

/// Run one command line and return its stdout, asserting success.
fn line(src: &str) -> String {
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

#[test]
fn an_imported_pub_name_keeps_its_gates() {
    // The importer's name is the file's binding again, gates included.
    assert_eq!(line("import file «./tests/fixtures/gated.logos», r = 9, r"), "9");
    assert_eq!(line("import file «./tests/fixtures/gated.logos», q.bump(), q.a"), "2");
    let (code, _, stderr) = run_line("import file «./tests/fixtures/gated.logos», s = 3, s");
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("`s` is not `mut`"), "stderr: {stderr}");
}

/// Each line fails at parse, the name it reads dead.
fn assert_dead(lines: &[String], name: &str) {
    for src in lines {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(&format!("`{name}` is dead here")), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_pub_name_the_file_ended_is_dead_for_the_importer() {
    let lib = "import file «./tests/fixtures/pub_ended.logos»";
    let tails = ["a@", "free a", "b := move a", "f := fn () -> i32 ( a@ )", &format!("{lib}, a@")];
    assert_dead(&tails.map(|tail| format!("{lib}, {tail}")), "a");
    assert_dead(&[format!("{lib}, q")], "q");
    assert_eq!(line(&format!("{lib}, a := i32 3, a")), "3");
    assert_eq!(line(&format!("{lib}, c")), "7");
    assert_eq!(line(&format!("{lib}, q.y")), "6");
}

#[test]
fn an_imported_name_one_importer_ended_is_dead_for_every_importer() {
    let lib = "import file «./tests/fixtures/owning_pub.logos»";
    let frees = "import file «./tests/fixtures/frees_imported.logos»";
    assert_dead(
        &[
            format!("{lib}, free p, {lib}, p@"),
            format!("{lib}, free p, {lib}, free p"),
            format!("{lib}, {frees}, p@"),
            format!("{frees}, {lib}, p@"),
        ],
        "p",
    );
    let (echoes, stderr) = repl(
        "import file «./tests/fixtures/held.logos»\nfree h\nimport file «./tests/fixtures/held.logos»\nh.n\n",
    );
    assert_eq!(echoes, ["freed 9"], "stderr: {stderr}");
    assert!(stderr.contains("`h` is dead here"), "stderr: {stderr}");
    // A body that runs later or again cannot end it: `import` stands only where it runs once.
    let (code, _, stderr) = run_line(&format!("{lib}, f := fn () -> void ( free p ), 1"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("declared outside this loop or function body"), "stderr: {stderr}");
    // A line that fails at parse takes its `free` back, on every copy of the name.
    let (echoes, stderr) = repl(format!("{lib}\nfree p, zz\n{lib}\np@\n").as_bytes());
    assert_eq!(echoes, ["7"], "stderr: {stderr}");
}

#[test]
fn an_imported_pub_spelling_is_keyed_and_ranked_as_the_file_declared_it() {
    let lib = "import file «./tests/fixtures/pub_spellings.logos»";
    assert_eq!(line(&format!("{lib}, (5k).type == type")), "true");
    assert_eq!(line(&format!("{lib}, {lib}, (5k).type == type")), "true");
    assert_eq!(line(&format!("{lib}, {lib}, @!")), "7");
    assert_eq!(line(&format!("{lib}, r:lex_rank")), "3.0");
    let (code, _, stderr) = run_line(&format!("regex «[0-9]+k» := type (), {lib}"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("`[0-9]+k` is already declared"), "stderr: {stderr}");
}

#[test]
fn an_import_tail_runs_once() {
    assert_eq!(line("import file «./tests/fixtures/counter.logos», c@"), "1");
    // An import is a statement: nothing echoes, and the second import runs nothing.
    let (echoes, stderr) =
        repl("import file «./tests/fixtures/counter.logos»\nimport file «./tests/fixtures/counter.logos»\nc@\n");
    assert_eq!(echoes, ["1"], "stderr: {stderr}");
}

#[test]
fn a_static_name_in_a_file_imported_twice_is_one_place() {
    let out = logos()
        .arg(
            "import file «tests/fixtures/static_importer_a.logos», \
             import file «tests/fixtures/static_importer_b.logos», \
             import file «tests/fixtures/static_counter.logos», print «{a} {b} {count}», 0",
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "loaded\n1 2 2\n0\n");
}

#[test]
fn a_static_name_in_a_top_level_loop_is_made_once() {
    let src = "print «a», mut t := i32 0, \
               for i in 0..3 ( static p := (print «made», alloc 1 of i32 7), t = t + p@ ), \
               print «end», t";
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\nmade\nend\n21\n");
}

#[test]
fn the_pass_runs_only_as_far_as_it_must_in_order_and_never_twice() {
    let bump = "mut x := i32 0, bump := fn () -> i32 ( x = x + 1, x )";
    assert_eq!(line(&format!("{bump}, bump() + ( x = 10, x )")), "11");
    let add = "f := fn (mut a := i32 ?, b := i32 ?) -> i32 ( a + b )";
    assert_eq!(line(&format!("{bump}, {add}, f(bump(), bump()) * 10 + x")), "32");
    assert_eq!(line("mut x := i32 0, while (x < 3) ( x = x + 1 ), x"), "3");
    // `bump()` once gives 7; twice would give 9.
    let block = "y := ( mut a := type ?, a = i32, free (bump()), z := a 5, z + x ), y + x";
    assert_eq!(line(&format!("{bump}, {block}")), "7");
    assert_eq!(line("y := ( mut a := type ?, a = i32, mut x := a 5, x ) + 1, y"), "6");
    let (echoes, stderr) = repl(
        "print «{( mut a := type ?, a = i32, mut x := a 5, x )}»\nq := ( p := @i32 ?, p@ )\nq := i32 4\nq\n",
    );
    assert_eq!(echoes, ["5", "4"], "stderr: {stderr}");
    assert!(stderr.contains("`p` is read before it is written"), "stderr: {stderr}");
}

#[test]
fn immediate_runs_its_expression_as_it_parses_and_stands_as_the_value() {
    let array = "import file «./identities/array.logos»";
    assert_eq!(line(&format!("{array}, n := immediate ( 2 + 3 ), a := array i32 [n], a[0]")), "5");
    assert_eq!(line("x := immediate 2 + 3, x"), "5");
    let bump = "mut c := i32 0, bump := fn () -> i32 ( c = c + 1, c )";
    // It reads to the comma, and runs after what stands before it.
    assert_eq!(line(&format!("{bump}, y := immediate bump() + 10, y * 10 + c")), "111");
    // A body holds the value: `bump` ran once, while the body parsed.
    assert_eq!(
        line(&format!("{bump}, f := fn () -> i32 ( immediate bump() ), f() * 100 + f() * 10 + c")),
        "111"
    );
    assert_eq!(line("b := immediate ( 2 < 3 ), b"), "true");
    assert_eq!(
        line("g := fn (t := type ?) -> type ( t ), u := immediate g(i64), v := u 7, v"),
        "7"
    );
    // A name of the body has no value while the body parses.
    let (_echoes, stderr) = repl(b"h := fn (x := i32 ?) -> i32 ( immediate x + 1 )\n");
    assert!(stderr.contains("made once"), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"immediate 2 + 3\n");
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
}

#[test]
fn a_name_error_names_the_name_and_points_at_it() {
    // `x` is the tenth character of the line.
    let (echoes, stderr) = repl(b"x := i32 1\nf := fn (x:=i32 ?, y:=i32 ?) -> i32 ( x + y )\n");
    assert!(echoes.is_empty(), "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:10: error: `x` is already declared"), "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"zz + 1\nq := i32 1\n( q := 2 )\n");
    assert!(stderr.contains("unknown name `zz`"), "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:3: error: `q` is already declared"), "stderr: {stderr}");
}

#[test]
fn lex_splices_text_built_fragments() {
    // `twice` sits above `+` on the axis, so `3 + 4 twice` doubles the 4.
    let (echoes, stderr) = repl(
        "twice := type (static parse_rank = *.parse_rank + 1, static parse = ( tape.insert(1, lex «* 2»), tape.remove(0) ))\n\
          print «{5 twice}»\n3 + 4 twice\n",
    );
    assert_eq!(echoes, ["10", "11"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        "t := lex «(a, b)»\n\
          bad := type (static parse = ( tape.insert(1, lex «zz»), tape.remove(0) ))\n\
          x := bad\n5\n",
    );
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    assert!(stderr.contains("unknown name `zz`"), "stderr: {stderr}");
    let (_echoes, stderr) = repl(
        b"y := lex 5\n\
          bad2 := type (static parse = ( tape.insert(1, dyad (i32, 1)) ))\n",
    );
    assert!(stderr.contains("`lex` must be followed by a «…» quote"), "stderr: {stderr}");
    assert!(stderr.contains("`insert` splices a tape"), "stderr: {stderr}");
}

#[test]
fn print_writes_its_quote_each_time_it_runs_and_is_a_statement() {
    let out = logos()
        .arg("print «a», f := fn () -> i32 ( print «in f», 7 ), free (f()), free (f()), print «»")
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // No line of the program gives a value, so nothing echoes after the prints.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\nin f\nin f\n\n");

    let (echoes, stderr) = repl("print «hi»\n1 + 1\n".as_bytes());
    assert_eq!(echoes, ["hi", "2"], "stderr: {stderr}");

    let (_echoes, stderr) = repl("print 5\n".as_bytes());
    assert!(stderr.contains("`print` must be followed by a «…» quote"), "stderr: {stderr}");
}

#[test]
fn print_interpolates_braces_as_echo_shows_them_and_escapes_them_with_a_backslash() {
    let out = logos()
        .arg(
            "sum := i32 20, double := fn (n := i32 ?) -> i32 ( return n * 2 ), \
             print «answer {double(sum) + 2}», print «{1 < 2} {f64 5.5} \\{x\\} a\\b», \
             f := fn (n := i32 ?) -> i32 ( print «n={n}», n ), free (f(3)), f(4)",
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "answer 42\ntrue 5.5 {x} a\\b\nn=3\nn=4\n4\n");

    // A constructor's print runs at parse time.
    let (echoes, stderr) = repl(
        "m := type (static parse = ( print «parsing {1 + 1}», tape.remove(0) ))\n1 m\n".as_bytes(),
    );
    assert_eq!(echoes, ["parsing 2", "1"], "stderr: {stderr}");

    for (src, message) in [
        ("print «{x»", "this `{` has no `}`"),
        ("print «x}»", "this `}` closes no `{`"),
        ("print «{nope}»", "unknown name `nope`"),
    ] {
        let out = logos().arg(src).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(message), "{src}: {stderr}");
    }
}

/// DESIGN ›Source becomes runnable by interpretation, one token at a time; compilation is
/// opt-in‹ [[R164]]: "runtime code keeps I/O", so a `print` in a compiled body writes what the
/// interpreted one writes, each `{…}` read in the compiled frame (›`print «…»` is the output
/// word‹ [[R304]]).
#[test]
fn a_print_in_a_compiled_body_writes_what_the_interpreted_one_writes() {
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) )";
    let mut cases = Vec::new();
    for compile in ["", "g.compile(), "] {
        for (program, printed) in [
            ("g := fn (k := i32 ?) -> i32 ( print «hi», k ), {compile}g(1)".to_string(), "hi\n1\n"),
            ("g := fn (k := i32 ?) -> i32 ( print «hi {k}», k ), {compile}g(1)".to_string(), "hi 1\n1\n"),
            (
                "g := fn (k := i32 ?) -> i32 ( for i in 0..3 ( print «turn {i}» ), 0 ), {compile}g(1)"
                    .to_string(),
                "turn 0\nturn 1\nturn 2\n0\n",
            ),
            (
                "g := fn (k := i32 ?) -> i32 ( mut s := i32 0, while s < k ( s = s + 1, \
                 print «s {s}» ), s ), {compile}g(2)"
                    .to_string(),
                "s 1\ns 2\n2\n",
            ),
            (
                "g := fn (k := f64 ?) -> f64 ( print «{k} {k > 1.0} {k * 2.0}», k ), {compile}g(1.5)"
                    .to_string(),
                "1.5 true 3.0\n1.5\n",
            ),
            ("g := fn (k := f32 ?) -> f32 ( print «{k}», k ), {compile}g(f32 2.5)".to_string(), "2.5\n2.5\n"),
            (
                "g := fn (k := u32 ?) -> u32 ( print «{k}», k ), {compile}g(4000000000)".to_string(),
                "4000000000\n4000000000\n",
            ),
            ("g := fn (k := i8 ?) -> i8 ( print «{k}», k ), {compile}g(i8 -7)".to_string(), "-7\n-7\n"),
            (
                "g := fn (k := i64 ?) -> i64 ( print «{k} {-k}», k ), {compile}g(-7)".to_string(),
                "-7 7\n-7\n",
            ),
            (
                "g := fn (k := i32 ?) -> i32 ( print «{k} \\{b\\} {i32} {3} {k == 3} {k:name} \
                 {k.type}», k ), {compile}g(3)"
                    .to_string(),
                "3 {b} i32 3 true k i32\n3\n",
            ),
            (
                "top := i32 9, h := fn (k := i32 ?) -> i32 ( k * 2 ), g := fn (k := i32 ?) -> i32 \
                 ( print «{top} {h(k)} { a := k + 1, a * 2 }», k ), {compile}g(4)"
                    .to_string(),
                "9 8 10\n4\n",
            ),
            (
                "r := rational_number 1/3, g := fn (k := i32 ?) -> i32 ( print «{r}», k ), \
                 {compile}g(3)"
                    .to_string(),
                "1/3\n3\n",
            ),
            (
                "g := fn (k := @i32 ?) -> i32 ( print «{k@}», k@ ), x := i32 5, {compile}g(&x)"
                    .to_string(),
                "5\n5\n",
            ),
            (
                "g := fn (k := i32 ?) -> i32 ( if (k > 2) ( print «big {k}» ) else \
                 ( print «small {k}» ), k ), {compile}free (g(4)), g(1)"
                    .to_string(),
                "big 4\nsmall 1\n1\n",
            ),
            (
                "g := fn (k := i32 ?) -> i32 ( defer print «bye {k}», print «hi {k}», k ), \
                 {compile}g(1)"
                    .to_string(),
                "hi 1\nbye 1\n1\n",
            ),
            (
                format!(
                    "{bag}, g := fn (k := i32 ?) -> i32 ( b := bag(k), print «made {{b.n}}», \
                     b.n + 1 ), {{compile}}g(4)"
                ),
                "made 4\nfreed 4\n5\n",
            ),
        ] {
            cases.push((program.replace("{compile}", compile), printed));
        }
    }
    assert_runs(&cases);
}

#[test]
fn a_quote_counts_its_pairs_by_depth_and_reads_five_escapes() {
    for (src, printed) in [
        ("print «a «b» c»", "a «b» c\n"),
        ("print «{«a»}»", "a\n"),
        // A plain string's `{…}` waits on live strings: it stays as written.
        ("print «{«b{«c»}»}»", "b{«c»}\n"),
        ("print «a\\»b»", "a»b\n"),
        ("print «a\\«b»", "a«b\n"),
        ("print «a\\\\b»", "a\\b\n"),
        ("print «\\{a\\} \\q»", "{a} \\q\n"),
        ("t := lex «print «a «b» c»», 5", "5\n"),
        ("t := type (static greet := fn () -> void ( print «{«a»}» )), 5", "5\n"),
        ("# «a «b» c», 5", "5\n"),
        ("regex «a\\»» := type (), 5", "5\n"),
        ("regex «[0-9]+\\\\.k» := type (), (5.k).type == type", "true\n"),
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(0), "{src}: {stderr}");
        assert_eq!(stdout, printed, "{src}: {stderr}");
    }
    let (_, stdout, _) = run_line("f := fn () -> void ( print «{«a»}» ), f()");
    assert!(stdout.starts_with("a\n"), "{stdout}");
    // The escapes are applied before the pattern is read, so `5\.k` is no longer its spelling.
    let (_, _, stderr) = run_line("regex «[0-9]+\\\\.k» := type (), (5\\.k).type == type");
    assert!(stderr.contains("1:34: error: unknown name `\\`"), "{stderr}");
    let (_, _, stderr) = run_line("error «{«a»}»");
    assert!(stderr.contains("run error: a"), "{stderr}");
    // The quote holds `a «b» c` whole.
    let (code, stdout, stderr) = run_line("s := «a «b» c», print «{s}»");
    assert_eq!((code, stdout.as_str()), (Some(0), "a «b» c\n"), "{stderr}");
    let (_, _, stderr) = run_line("import file «tests/fixtures/a«b».logos»");
    assert!(stderr.contains("cannot read ./tests/fixtures/a«b».logos"), "{stderr}");

    let (echoes, stderr) = repl("print «{«a»}»\nprint «a «b» c»\n".as_bytes());
    assert_eq!(echoes, ["a", "a «b» c"], "stderr: {stderr}");
}

#[test]
fn a_bracket_inside_a_nested_quote_or_its_comment_is_text_in_every_body() {
    let run_body = |body: &str| {
        format!(
            "bump := type (a := i32 ?, static run = ( {body} ), static parse_rank = *.parse_rank + 1, \
             static parse = ( tape[0].type = bump, tape[0].a = tape[-1], tape.is_constructed[0] = true, \
             tape.remove(-1) )), 3 bump"
        )
    };
    for (src, printed) in [
        ("f := fn () -> void ( print «a «b» ) c» ), f()".to_string(), "a «b» ) c\n"),
        ("if false ( print «a «b» ) c» ) else ( print «ok» )".to_string(), "ok\n"),
        (run_body("print «a «b» ) c»"), "a «b» ) c\n"),
        ("f := fn () -> void (\n# «a «b» ) c»\nprint «ok»\n), f()".to_string(), "ok\n"),
        (run_body("\n# «a «b» ) c»\nprint «hi»\n"), "hi\n"),
        (run_body("\n# a ) b\nprint «hi»\n"), "hi\n"),
        ("# «a «b» c»\nprint «ok»".to_string(), "ok\n"),
        (
            "pt := type (\n# «a «b» ) c»\na := i32 ?,\n# the rank\nstatic parse_rank = *.parse_rank + 1,\n\
             # «before the fill»\nstatic parse = ( tape[0].type = pt, tape[0].a = tape[-1], \
             tape.is_constructed[0] = true, tape.remove(-1) )\n), q := 4 pt, q.a"
                .to_string(),
            "4\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: {stderr}");
        assert!(stdout.starts_with(printed), "{src}: {stdout} {stderr}");
    }
}

/// A type `h` whose `parse` splices `fragment` in its own place.
fn splicer(rank: &str, fragment: &str) -> String {
    format!(
        "h := type (static parse_rank = {rank}, \
         static parse = ( tape.insert(1, lex «{fragment}»), tape.remove(0) ))"
    )
}

#[test]
fn a_hash_is_one_token_with_its_text_wherever_it_is_lexed() {
    let splice = |fragment: &str| splicer("*.parse_rank + 1", fragment);
    // A spliced `#` reads its own cell, never the rest of the outer line.
    let (_, stdout, stderr) = run_line(&format!("{}, h, print «after»", splice("#")));
    assert_eq!(stdout, "after\n", "{stderr}");
    let (_, stdout, stderr) =
        run_line(&format!("{},\nh, print «same line»,\nprint «next line»", splice("#")));
    assert_eq!(stdout, "same line\nnext line\n", "{stderr}");
    // The comment in a fragment is one cell, no code.
    for (fragment, printed) in
        [("* 2 # note", "10\n"), ("+ 1 # + 1", "6\n"), ("* 2 # «a «b» ) c»", "10\n")]
    {
        let (code, stdout, stderr) = run_line(&format!("{}, 5 h", splice(fragment)));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{fragment}: {stderr}");
    }
}

#[test]
fn a_comment_built_in_the_boundary_s_build_loop_is_read_through_before_the_next_cell() {
    // Below `(`, `h` runs at the boundary and its `#` is built in the build loop; at `fn`'s rank, at discovery.
    for rank in ["*.parse_rank + 1", "fn.parse_rank"] {
        let h = |fragment: &str| splicer(rank, fragment);
        for (fragment, line, printed) in [
            ("* 2 #", "5 h", "10\n"),
            ("+ 1 #", "5 h", "6\n"),
            ("* #", "5 h 2", "10\n"),
            ("# «x» * 2", "5 h", "10\n"),
            ("* 2 #", "c := 5 h", ""),
            ("#", "5, h", "5\n"),
            ("* 2 #", "f := fn () -> i32 ( 5 h ), f()", "10\n"),
            ("* #", "f := fn () -> i32 ( 5 h 2 ), f()", "10\n"),
            ("* 2 #", "f := fn () -> i32 ( 5 h ), f.compile(), f()", "10\n"),
            ("* #", "f := fn () -> i32 ( 5 h 2 ), f.compile(), f()", "10\n"),
        ] {
            let src = format!("{}, {line}", h(fragment));
            let (code, stdout, stderr) = run_line(&src);
            assert_eq!((code, stdout.as_str()), (Some(0), printed), "{src}: {stderr}");
        }
        let (echoes, stderr) = repl(format!("{}\n5 h\n", h("* 2 #")).as_bytes());
        assert_eq!(echoes, ["10"], "{rank}: {stderr}");
    }
    let out =
        logos().arg("import file «tests/fixtures/spliced_comment.logos», ten").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n");
}

#[test]
fn a_comment_is_a_line_of_the_scope_its_text_is_in() {
    let q = "q := type ( v := i32 ?, static parse_rank = 61, static parse = ( tape[0].type = q, \
             tape[0].v = tape[1].dyads[0], tape.remove(1), tape.is_constructed[0] = true ) )";
    let narrowed = "q := type ( v := i32 ?, static parse_rank = 61, static parse = ( \
                    tape[0].type = q, if tape[1].dyads[1].type == i32 # «c» \
                    ( tape[0].v = tape[1].dyads[1] * 2 ) else ( tape[0].v = i32 0 ), \
                    tape.remove(1), tape.is_constructed[0] = true ) ), x := q [5, i32 7], x.v";
    let mut rows = vec![
        // In a condition: a line of the condition's own scope, read past for its value.
        ("if 5 * 2 # «c» == 10 ( free (1), here.scope.dyads.size ) else ( 0 )".to_owned(), "1\n"),
        ("if (5 * 2 # «c» == 10) ( free (1), here.scope.dyads.size ) else ( 0 )".to_owned(), "1\n"),
        ("if 5 * 2 # «c» == 10 ( print «yes» ) else ( print «no» )".to_owned(), "yes\n"),
        ("if false # «c» ( 1 ) else ( free (2), here.scope.dyads.size )".to_owned(), "1\n"),
        (
            "x := i32 3, if x == 0 ( u64 0 ) else if x == 3 # «c» ( free (1), here.scope.dyads.size ) \
             else ( u64 9 )"
                .to_owned(),
            "1\n",
        ),
        (
            "mut i := i32 0, mut n := u64 0, while i < 1 # «c» \
             ( i = i + 1, n = here.scope.dyads.size ), n"
                .to_owned(),
            "1\n",
        ),
        ("if (print «side», true) # «c» ( 1 ) else ( 2 )".to_owned(), "side\n1\n"),
        (narrowed.to_owned(), "14\n"),
        // Inside a line: a line of the scope the line stands in, after it.
        ("f := fn () -> u64 # «c» ( free (1), here.scope.dyads.size ), f()".to_owned(), "1\n"),
        ("f := fn (n := i32 ? # «c») -> u64 ( free (1), here.scope.dyads.size ), f(1)".to_owned(), "1\n"),
        (
            "mut n := u64 0, for i in 0..1 # «c» ( free (1), n = here.scope.dyads.size ), n".to_owned(),
            "1\n",
        ),
        ("x := 5 # «c» * 2, here.scope.dyads.size".to_owned(), "2\n"),
        ("x := immediate 5 # «c» * 2, here.scope.dyads.size".to_owned(), "2\n"),
        ("mut y := i32 1, y = 5 # «c» * 2, here.scope.dyads.size".to_owned(), "3\n"),
        ("defer print «a» # «c», here.scope.dyads.size".to_owned(), "a\n2\n"),
        ("t := type ( v := i32 ? # «c» ), ( free (1), here.scope.dyads.size )".to_owned(), "1\n"),
        // Written in the source, the comment follows its expression in the bracket.
        (format!("{q}, x := q [5 * 2 # «c», 7], x.v"), "10\n"),
    ];
    for compile in ["", "f.compile(), "] {
        rows.push((
            format!(
                "f := fn (n := i32 ?) -> u64 ( if n == 5 # «c» ( free (1), here.scope.dyads.size ) \
                 else ( u64 0 ) ), {compile}f(5)"
            ),
            "1\n",
        ));
        rows.push((
            format!(
                "mut n := u64 0, f := fn () -> u64 ( for i in 0..1 # «c» \
                 ( free (1), n = here.scope.dyads.size ), n ), {compile}f()"
            ),
            "1\n",
        ));
    }
    // Spliced, it follows its expression as the written one does, at discovery and at the boundary.
    for rank in ["fn.parse_rank", "*.parse_rank + 1"] {
        rows.push((format!("{}, {q}, x := q [5 h, 7], x.v", splicer(rank, "* 2 #")), "10\n"));
    }
    for (src, printed) in rows {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{src}: {stderr}");
    }
    let line = "if 5 * 2 # «c» == 10 ( free (1), here.scope.dyads.size ) else ( 0 )\n";
    let (echoes, stderr) = repl(line.as_bytes());
    assert_eq!(echoes, ["1"], "{stderr}");
}

#[test]
fn a_comment_before_a_bare_condition_s_value_is_read_through() {
    let mut rows = vec![
        ("if # «a» true ( 1 ) else ( u64 0 )".to_owned(), "1\n"),
        ("mut i := i32 0, while # «a» i < 2 ( i = i + 1 ), i".to_owned(), "2\n"),
        ("if # «a» # «b» true ( 1 ) else ( u64 0 )".to_owned(), "1\n"),
        ("if # «a» (true) ( 1 ) else ( u64 0 )".to_owned(), "1\n"),
        ("if # «a» true ( free (1), here.scope.dyads.size ) else ( u64 0 )".to_owned(), "1\n"),
        ("x := i32 3, if x == # «a» i32 3 ( print «yes» ) else ( print «no» )".to_owned(), "yes\n"),
        ("x := i32 3, if x.type == # «a» i32 ( 10 ) else ( 20 )".to_owned(), "10\n"),
        (
            "mut i := i32 0, mut n := u64 0, while # «c» i < 1 \
             ( i = i + 1, n = here.scope.dyads.size ), n"
                .to_owned(),
            "1\n",
        ),
    ];
    for compile in ["", "f.compile(), "] {
        rows.push((
            format!("f := fn () -> u64 ( if # «a» true ( 1 ) else ( u64 0 ) ), {compile}f()"),
            "1\n",
        ));
        rows.push((
            format!(
                "f := fn () -> i32 ( mut i := i32 0, while # «a» i < 2 ( i = i + 1 ), i ), \
                 {compile}f()"
            ),
            "2\n",
        ));
        rows.push((
            format!(
                "f := fn (n := i32 ?) -> u64 ( if # «c» n == 5 ( free (1), here.scope.dyads.size ) \
                 else ( u64 0 ) ), {compile}f(5)"
            ),
            "1\n",
        ));
    }
    for (src, printed) in rows {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{src}: {stderr}");
    }
}

/// ›An `if` body needs no brackets‹ [[R265]]: the condition is the first complete expression, and
/// a quote, whose record names no left side, starts the body; a comment after a complete part
/// still leaves the cell after it to decide.
#[test]
fn a_bare_condition_ends_before_a_quote() {
    let mut cases: Vec<(String, &str)> = [
        ("if (true) «a» else «b»", "a\n"),
        ("c := true, if c «a» else «b»", "a\n"),
        ("c := true, if c # «c» and false «a» else «b»", "b\n"),
        ("mut c := true, if c mut y := 1, 0", "0\n"),
    ]
    .into_iter()
    .map(|(src, want)| (src.to_string(), want))
    .collect();
    for c in ["", "f.compile(), "] {
        for (src, want) in [
            (format!("f := fn () -> i32 ( free (if (true) «a» else «b»), 1 ), {c}f()"), "1\n"),
            (format!("f := fn () -> string ( if (true) «a» else «b» ), {c}f()"), "a\n"),
            (format!("f := fn (c := bool ?) -> string ( if c «a» else «b» ), {c}f(false)"), "b\n"),
        ] {
            cases.push((src, want));
        }
    }
    assert_runs(&cases);
}

#[test]
fn the_boundary_s_comment_lift_reads_a_built_cell_with_no_node_without_faulting() {
    // `h1` marks the unknown `zz` built, which leaves its cell with no node, and `h2`
    // removes it at the next step: the lift between the two reads that cell.
    let src = "h1 := type (static parse_rank = *.parse_rank + 2, \
               static parse = ( tape.is_constructed[1] = true, tape.remove(0) )), \
               h2 := type (static parse_rank = *.parse_rank + 1, \
               static parse = ( tape.remove(1), tape.remove(0) )), \
               x := 7, x h2 h1 zz";
    let (code, _, stderr) = run_line(src);
    // A panic on the work thread exits 1, as a Logos error does.
    assert!(code.is_some() && !stderr.contains("panicked"), "{code:?}: {stderr}");
}

#[test]
fn a_body_lexed_once_holds_the_scopes_of_its_quotes() {
    let run_body = |body: &str| {
        format!(
            "mut n := i32 1, bump := type (a := i32 ?, static run = ( {body} ), \
             static parse_rank = *.parse_rank + 1, static parse = ( tape[0].type = bump, \
             tape[0].a = tape[-1], tape.is_constructed[0] = true, tape.remove(-1) )), 3 bump"
        )
    };
    for (src, printed) in [
        (run_body("print «{a}»"), "3\n"),
        (run_body("print «{n} {a + 1} {«)»} {«x {«y»}»}»"), "1 4 ) x {«y»}\n"),
        (run_body("\n# «note {a}»\nprint «a={a}»\n"), "a=3\n"),
        (
            "f := fn () -> i32 ( t := type ( static k := immediate ( print «v {1}», i32 1 ) ), 1 ), f()"
                .to_string(),
            "v 1\n1\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: {stderr}");
        assert!(stdout.starts_with(printed), "{src}: {stdout} {stderr}");
    }
    let (_, _, stderr) = run_line(&run_body("error «a is {a}»"));
    assert!(stderr.contains("run error: a is 3"), "{stderr}");
}

#[test]
fn an_unclosed_quote_or_brace_is_reported_at_its_opener() {
    for (src, message) in [
        ("print «abc", "1:7: error: this `«` has no `»`"),
        ("print «a {b»", "1:10: error: this `{` has no `}`"),
        ("x := «a «b»", "1:6: error: this `«` has no `»`"),
        ("f := fn () -> void ( print «a ), f()", "1:28: error: this `«` has no `»`"),
        ("if false ( print «a ) else ( 1 )", "1:18: error: this `«` has no `»`"),
        ("t := type ( # «a )", "1:15: error: this `«` has no `»`"),
        ("# «{a»\n5", "1:4: error: this `{` has no `}`"),
        ("# «x {»} y», 5", "1:7: error: this `»` closes no `«`"),
        ("f := fn () -> void ( # «x {»} y»\n1 ), 5", "1:28: error: this `»` closes no `«`"),
        ("t := type (a := i32 ?, static run = ( # «x {»} y»\n1 )), 5", "1:45: error: this `»`"),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: {stderr}");
        assert!(stderr.contains(message), "{src}: {stderr}");
    }
    // A `lex` quote lexes its text when it runs, the escapes applied first.
    let (_, _, stderr) = run_line("t := lex «x \\«», 5");
    assert!(stderr.contains("this `«` has no `»`"), "{stderr}");
}

/// ›Unknown spellings are two pattern identities‹ [[R252]]: `«` is an identity apart from
/// `string`, the type of the values it makes, so a name of the type stands as the type, as
/// `t := i32` does, and no reader of a quote is handed the name, wherever it is lexed.
#[test]
fn a_name_of_the_text_type_stands_as_the_type() {
    let t = "t := «a».type";
    let mut cases: Vec<(String, &str)> = [
        "free (t), 7",
        "free (5), free (t), 7",
        "x := lex «t», 7",
        "if false ( t ), 7",
        "p := type ( x := t ? ), 7",
        "p := @t ?, 7",
        "r := t:lex_rank, 7",
        "w := type ( n := i32 ?, static run = ( t, n ) ), 7",
        "f := fn () -> i32 ( x := t, 7 ), f()",
        "f := fn () -> i32 ( x := t, 7 ), f.compile(), f()",
    ]
    .into_iter()
    .map(|tail| (format!("{t}, {tail}"), "7\n"))
    .collect();
    cases.push((format!("{t}, t == «b».type"), "true\n"));
    cases.push((format!("{t}, print «{{t == «b».type}}», 7"), "true\n7\n"));
    cases.push(("g := fn () -> type ( «a».type ), t := g(), free (t), 7".to_string(), "7\n"));
    for (src, want) in [
        (format!("{t}, t == string"), "true\n"),
        (format!("{t}, t == bool"), "false\n"),
        ("f := fn () -> bool ( return «a».type == string ), f()".to_string(), "true\n"),
        (
            "f := fn () -> bool ( return «a».type == string ), f.compile(), f()".to_string(),
            "true\n",
        ),
    ] {
        cases.push((src, want));
    }
    cases.push(("s := string ?, 0".to_string(), "0\n"));
    cases.push(("mut s := string ?, s = «a», s".to_string(), "a\n"));
    assert_runs(&cases);
    // A `»` after the name is no quote's end.
    let (code, _, stderr) = run_line(&format!("{t}, (t 1 + 2»), 7"));
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("1:24: error: unknown name `»`"), "{stderr}");
    let (_, shown, _) = run_line(&format!("{t}, t"));
    let (echoes, stderr) = repl(format!("{t}\n") + "print «{7}»\nprint «{t}»\n8\n");
    assert_eq!(echoes, ["7", shown.trim_end(), "8"], "stderr: {stderr}");
}

/// ›Text literals are plain values; `#` is the one comment constructor‹ [[R160]]: a text node and
/// a place of text read as the node's address, wherever they stand, on both tiers, and show the
/// text they hold.
#[test]
fn a_text_node_and_a_place_of_text_read_as_the_node() {
    let mut cases: Vec<(String, &str)> = [
        ("s := «a», 0", "0\n"),
        ("free («a»), 0", "0\n"),
        ("( free («a»), 0 )", "0\n"),
        ("for 0..2 ( free («a») ), 0", "0\n"),
        ("t := type ( static k := «a» ), 0", "0\n"),
        ("free (regex «[0-9]+»), 0", "0\n"),
        ("f := fn (a := dyad ?) -> i32 ( 1 ), f(«a»)", "1\n"),
        ("x := 5, x:name == «x»", "false\n"),
        ("«a»", "a\n"),
        ("s := «a», s", "a\n"),
        ("if (true) ( «a» ) else ( «b» )", "a\n"),
        ("immediate «a»", "a\n"),
        ("import file «tests/fixtures/pub_text.logos», s", "a\n"),
        (
            "sq := type ( a := dyad ?, static parse = ( n := tape[-1]:name, \
             print «{n} {tape.spelling[-1]}», tape[0].type = sq, tape[0].a = tape[-1], \
             tape.is_constructed[0] = true, tape.remove(-1) ) ), x := i32 3, free (x sq), 0",
            "x x\n0\n",
        ),
    ]
    .into_iter()
    .map(|(src, want)| (src.to_string(), want))
    .collect();
    for c in ["", "f.compile(), "] {
        for (src, want) in [
            (format!("f := fn () -> i32 ( free («a»), 1 ), {c}f()"), "1\n"),
            (format!("f := fn () -> i32 ( s := «a», 1 ), {c}f()"), "1\n"),
            (format!("f := fn () -> i32 ( defer free («a»), 1 ), {c}f()"), "1\n"),
            (format!("f := fn (a := string ?) -> i32 ( print «{{a}}», 1 ), {c}f(«hi»)"), "hi\n1\n"),
            (format!("f := fn () -> string ( «a» ), {c}f()"), "a\n"),
        ] {
            cases.push((src, want));
        }
    }
    assert_runs(&cases);
    let (echoes, stderr) = repl("print «{«a»}»\ns := «a»\ns\n");
    assert_eq!(echoes, ["a", "a"], "stderr: {stderr}");
}

#[test]
fn error_aborts_the_run_with_its_message() {
    let out = logos()
        .arg(
            "f := fn (x := i32 ?) -> i32 ( if (x > 2) (error «too big: {x}») else (x) ), \
             print «{f(1)}», free (f(5)), print «never»",
        )
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "a plain failure exit, not a signal");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("run error: too big: 5"), "stderr: {stderr}");

    let out = logos().arg("t := type (static parse = (error «not here»)), t").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not here"), "stderr: {stderr}");

    let (echoes, stderr) = repl("error «boom»\nerror 5\n1 + 1\n".as_bytes());
    assert_eq!(echoes, ["2"], "stderr: {stderr}");
    assert!(stderr.contains("run error: boom"), "stderr: {stderr}");
    assert!(stderr.contains("`error` must be followed by a «…» quote"), "stderr: {stderr}");
}

#[test]
fn error_interpolates_braces_exactly_as_print_does() {
    let out = logos()
        .arg(
            "n := 3, f := fn (i := i32 ?) -> i32 ( if (i >= n) (error «index {i} is past {n - 1}») else (i) ), \
             free (f(1)), f(4)",
        )
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("run error: index 4 is past 2"), "stderr: {stderr}");

    let out = logos().arg("error «{1 < 2} {f64 5.5} \\{x\\}»").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("run error: true 5.5 {x}"), "stderr: {stderr}");

    for (src, message) in [
        ("error «{x»", "this `{` has no `}`"),
        ("error «x}»", "this `}` closes no `{`"),
        ("error «{nope}»", "unknown name `nope`"),
    ] {
        let out = logos().arg(src).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(message), "{src}: {stderr}");
    }
}

#[test]
fn caller_scope_is_the_use_site_and_here_scope_the_body() {
    let (echoes, stderr) = repl(
        "mut seen := here.scope\n\
         w := type (static parse = ( seen = caller.scope, tape.remove(0) ))\n\
         g := fn () -> i32 ( 1 w )\n\
         top := here.scope\n\
         print «{seen == top}»\n\
         print «{seen.back.back == top}»\n\
         print «{g()}»\n\
         h := fn () -> i32 ( seen = here.scope, 2 )\n\
         print «{h()}»\n\
         seen.back.back == here.scope\n",
    );
    assert_eq!(echoes, ["false", "true", "1", "2", "true"], "stderr: {stderr}");
    // Stand-in: outside a constructor `caller.scope` is the checked error, not the per-call read.
    let (_echoes, stderr) = repl(b"caller.scope\n");
    assert!(stderr.contains("`caller` can be read only inside a constructor"), "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"caller\n");
    assert!(stderr.contains("read `caller.scope`"), "stderr: {stderr}");
}

#[test]
fn a_scope_is_read_by_path_and_reading_runs_nothing() {
    let (echoes, stderr) = repl(
        "mut n := i32 0\n\
         x := i32 1\n\
         g := ( a := i32 1, b := a + 1, n = n + 1, b )\n\
         s := g:start.rhs\n\
         top := here.scope\n\
         print «{s.dyads.size}»\n\
         print «{s.dyads[0].lhs:name}»\n\
         print «{s.dyads[0].rhs.type == i32}»\n\
         print «{s.dyads[1].rhs.lhs:name}»\n\
         print «{s.back == top}»\n\
         print «{s == g:start.rhs}»\n\
         t := s\n\
         print «{t.dyads[3]:name}»\n\
         print «{n}»\n\
         print «{top.dyads[1].lhs:name}»\n\
         ( c := i32 1, here.scope.dyads.size )\n",
    );
    assert_eq!(
        echoes,
        ["4", "a", "true", "a", "true", "true", "b", "1", "x", "1"],
        "stderr: {stderr}"
    );

    let (_echoes, stderr) = repl(
        b"g := ( a := i32 1, a )\n\
          g:start.rhs.dyads[2]\n\
          k := i32 0\n\
          g:start.rhs.dyads[k]\n\
          mut m := g:start.rhs\n\
          m.dyads\n",
    );
    assert_eq!(stderr.matches("this read does not fit the node's type").count(), 2, "{stderr}");
    assert!(stderr.contains("this operator cannot compute over these operands"), "{stderr}");
}

#[test]
fn a_constructor_reads_the_scope_cell_to_its_right_line_by_line() {
    let lib = "import file «tests/fixtures/scope_cell_lines.logos»";
    for (line, want) in
        [("first (7, 8, 9) + last (7, 8, 9)", "16"), ("first (scope (free (1), 2), 3)", "2")]
    {
        let out = logos().arg(format!("{lib}, {line}")).output().unwrap();
        assert!(out.status.success(), "{line}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{want}\n"), "{line}");
    }
    for (line, want) in [
        ("first 5", "this node is not a scope"),
        ("first ()", "index 0 is past the end (0 items)"),
        ("last ()", "an index cannot be negative"),
    ] {
        let out = logos().arg(format!("{lib}, {line}")).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{line}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(want), "{line}: {stderr}");
    }
}

#[test]
fn scope_by_name_is_the_bracket_and_alone_is_the_type() {
    for (line, want) in [
        ("scope (free (1), 2)", "2"),
        ("g := scope ( a := i32 1, a ), g:start.rhs.dyads.size", "2"),
        ("g := scope ( a := i32 1, a ), g:start.rhs.dyads[0].lhs:name", "a"),
        ("f := fn () -> i32 ( scope ( 4 ) ), f.compile(), f()", "4"),
        ("scope == scope", "true"),
        ("t := scope, t == scope", "true"),
        ("here.scope.type == scope", "true"),
        ("x := i32 1, x:scope == here.scope", "true"),
    ] {
        let out = logos().arg(line).output().unwrap();
        assert!(out.status.success(), "{line}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{want}\n"), "{line}");
    }
}

#[test]
fn a_declaration_on_the_command_line_is_its_names_start() {
    let out = logos().arg("x := i32 1, y := x + 2, y:start.rhs.lhs:name").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "x\n");
}

#[test]
fn a_nodes_fields_are_read_by_name_without_running_it() {
    let power = POWER;
    let sq = "sq := type ( a := i32 ?, static output_type := i32, static tag := i32 7, \
              static run = ( a * a ), static parse = ( tape[0].type = sq, tape[0].a = tape[-1], \
              tape.is_constructed[0] = true, tape.remove(-1) ) )";
    let mk = "pt := type ( x := i32 ? ), mk := type ( a := i32 ?, output_type := type ?, \
              static tag := i32 7, static run = ( pt(a) ), static parse = ( tape[0].type = mk, \
              tape[0].a = tape[-1], tape[0].output_type = pt, tape.is_constructed[0] = true, \
              tape.remove(-1) ) )";
    for (tail, want) in [
        ("f := fn (x := i32 ?) -> i32 ( b := x ^ 3, b:start.rhs.lhs ), f(5)", "5"),
        ("f := fn (x := i32 ?) -> i32 ( b := x ^ 3, b:start.rhs.lhs ), f.compile(), f(5)", "5"),
        ("x := i32 1, c := x + 2, c:start.rhs.lhs", "1"),
        // Written straight on an operator expression, `.` reads its node.
        ("f := fn (x := i32 ?) -> i32 ( (x ^ 3).lhs ), f(5)", "5"),
        ("f := fn (x := i32 ?) -> i32 ( (x ^ 3).lhs ), f.compile(), f(5)", "5"),
        ("f := fn (x := i32 ?) -> i32 ( (x + 3).lhs ), f.compile(), f(5)", "5"),
        ("x := i32 1, (x + 2).lhs", "1"),
        ("x := i32 1, (x + 2).rhs", "2"),
    ] {
        let out = logos().arg(format!("{power}, {tail}")).output().unwrap();
        assert!(out.status.success(), "{tail}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{want}\n"), "{tail}");
    }
    // A `static` member of the node's own type.
    for tail in
        ["x := i32 3, (x sq).tag", "f := fn (x := i32 ?) -> i32 ( (x sq).tag ), f.compile(), f(3)"]
    {
        let out = logos().arg(format!("{sq}, {tail}")).output().unwrap();
        assert!(out.status.success(), "{tail}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n", "{tail}");
    }
    // A name stands for the number it holds, which has no `lhs`; an all-literal `2 ^ 3` folds
    // to its number before `.` reads it.
    for tail in ["x := i32 1, c := x + 2, c.lhs", "(2 ^ 3).lhs"] {
        let out = logos().arg(format!("{power}, {tail}")).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && out.stdout.is_empty(), "{tail}: {stderr}");
        assert!(stderr.contains("cannot compute"), "{tail}: {stderr}");
    }
    // `&x` is typed by `&`'s mint, not by the `@i32` it gives; `y mk` is typed `mk`, not by the
    // record its `run` gives, which a copy carries.
    for (src, want) in [
        ("x := i32 5, (&x).type == @i32".to_string(), "false"),
        ("f := fn () -> bool ( x := i32 5, (&x).type == @i32 ), f.compile(), f()".into(), "false"),
        ("x := i32 5, (&x).output_type == @i32".into(), "true"),
        (
            "f := fn () -> bool ( x := i32 5, (&x).output_type == @i32 ), f.compile(), f()".into(),
            "true",
        ),
        (format!("{mk}, y := i32 3, (y mk).a"), "3"),
        (format!("{mk}, f := fn (y := i32 ?) -> i32 ( (y mk).a ), f.compile(), f(3)"), "3"),
        (format!("{mk}, y := i32 3, (y mk).output_type == pt"), "true"),
    ] {
        let out = logos().arg(&src).output().unwrap();
        assert!(out.status.success(), "{src}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{want}\n"), "{src}");
    }
    // `mk` reads to its left, so held or handed on by `.type` it wakes with nothing there:
    // stand-in for #402.
    for src in [
        format!("{mk}, t := mk, 1"),
        format!("{mk}, mk.tag"),
        format!("{mk}, y := i32 3, (y mk).type == pt"),
        format!("{mk}, y := i32 3, (y mk).type.tag"),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: {stderr}");
        assert!(
            stderr.contains("this gives nothing, and a value is needed here"),
            "{src}: {stderr}"
        );
    }
    // A field the node's type does not have is the error that says so, never a read of the
    // record the node gives when it runs.
    for src in [
        "x := i32 1, (x + 2).nope".to_string(),
        format!("{sq}, x := i32 3, (x sq).nope"),
        "x := i32 5, (&x).lhs".into(),
        "x := i32 5, (&x).nope".into(),
        "f := fn () -> i32 ( x := i32 5, (&x).lhs ), f.compile(), f()".into(),
        format!("{mk}, y := i32 3, (y mk).nope"),
        format!("{mk}, y := i32 3, (y mk).x"),
        format!("{mk}, f := fn (y := i32 ?) -> i32 ( (y mk).x ), f.compile(), f(3)"),
    ] {
        let out = logos().arg(&src).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && out.stdout.is_empty(), "{src}: {stderr}");
        assert!(stderr.contains("does not fit the node's type"), "{src}: {stderr}");
    }
    // A name holds the record.
    let out = logos().arg(format!("{mk}, y := i32 3, z := y mk, z.x")).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
}

/// ›An operation mints through a chooser, one mint per set of operand types, so each mint's `run`
/// body is parsed once‹ [[R301]]: `&` mints per pointee type and negation per operand type, as
/// `+` mints, and negation is its own identity (›`-` negates where a space stands before it and
/// none after, or no value stands to its left; any other `-` subtracts‹ [[R157]]), on both tiers.
#[test]
fn every_operation_is_typed_by_its_mint() {
    let mut cases = Vec::new();
    for (body, want) in [
        ("(&x).type == (&w).type", "false\n"),
        ("(&x).type == (&y).type", "true\n"),
        ("(x + x).type == (w + w).type", "false\n"),
        ("(-x).type == (x - y).type", "false\n"),
        ("(-x).type == (-y).type", "true\n"),
        ("(-x).type == (-w).type", "false\n"),
        ("(-x).output_type == i32", "true\n"),
        ("(-x).operand == x", "true\n"),
    ] {
        cases.push((format!("x := i32 3, y := i32 4, w := i64 5, {body}"), want));
        let f = format!("f := fn (x := i32 ?, y := i32 ?, w := i64 ?) -> bool ( {body} )");
        cases.push((format!("{f}, f(3, 4, 5)"), want));
        cases.push((format!("{f}, f.compile(), f(3, 4, 5)"), want));
    }
    // Negation over each kind of number: an integer wraps, a float flips its sign, a rational
    // value is negated exactly.
    for (ty, arg, want) in
        [("u8", "3", "253\n"), ("i8", "i8 -128", "-128\n"), ("f64", "0.0", "-0.0\n")]
    {
        let f = format!("f := fn (m := {ty} ?) -> {ty} ( -m )");
        cases.push((format!("{f}, f({arg})"), want));
        cases.push((format!("{f}, f.compile(), f({arg})"), want));
    }
    cases.push(("m := rational_number 3, n := -m, n".into(), "-3\n"));
    // `move` is typed by its mint for the type it hands on.
    for (gives, body, want) in [
        ("bool", "(move x).type == (move w).type", "false\n"),
        ("bool", "(move x).type == (move y).type", "true\n"),
        ("bool", "(move x).output_type == i32", "true\n"),
        ("i32", "z := move x, z", "3\n"),
    ] {
        let f = format!("f := fn () -> {gives} ( x := i32 3, y := i32 4, w := i64 5, {body} )");
        cases.push((format!("{f}, f()"), want));
        cases.push((format!("{f}, f.compile(), f()"), want));
    }
    cases.push(("p := alloc 1 of i32 5, q := move p, q@".into(), "5\n"));
    // `alloc` is typed by its mint for its count's type and the cells' type, interpreted only.
    for (body, want) in [
        ("(alloc 1 of i32 5).type == (alloc 1 of i32 6).type", "true\n"),
        ("(alloc 1 of i32 5).type == (alloc 1 of i64 6).type", "false\n"),
        ("(alloc 1 of i32 5).type == (alloc i64 1 of i32 6).type", "false\n"),
        ("(alloc 2 of i32 5).count", "2\n"),
        ("p := alloc 2 of i32 5, (p + 1)@", "5\n"),
    ] {
        cases.push((body.into(), want));
    }
    // A map's compare against `?` is a node of `==`'s or `!=`'s mint.
    let map = "m := hashmap i64 -> i64, n := hashmap i64 -> hashmap i64 -> i64";
    for (body, want) in [
        ("print «{m == ?} {? != m} {n[1] == ?}»", "false true true\n"),
        ("(m == ?).output_type == bool", "true\n"),
        ("(m == ?).type == (m != ?).type", "false\n"),
        ("(m == ?).type == (? == m).type", "false\n"),
        ("(m == ?).rhs == ?", "true\n"),
    ] {
        cases.push((format!("{map}, {body}"), want));
    }
    assert_runs(&cases);
    for src in
        ["x := i32 3, (-x).lhs", "f := fn (x := i32 ?) -> i32 ( (-x).lhs ), f.compile(), f(3)"]
    {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{src}: stderr: {stderr}");
        assert!(stderr.contains("does not fit the node's type"), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_pointer_type_applies_to_any_type() {
    let (echoes, stderr) = repl(
        "mut p := @dyad ?\n\
         p = here.scope\n\
         top := here.scope\n\
         print «{p == top}»\n\
         f := fn (s := @dyad ?) -> i32 ( p = s, 1 )\n\
         x := i32 5\n\
         print «{f(x:scope)}»\n\
         print «{p == top}»\n\
         g := fn () -> i32 ( f(here.scope) )\n\
         print «{g()}»\n\
         print «{p.back.back.back == top}»\n\
         v := @void ?\n\
         t := fn (s := @void ?) -> i32 ( 1 )\n",
    );
    assert_eq!(echoes, ["true", "1", "true", "1", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
    // A place or literal after `@` is refused; its bytes used to be read as a record and crash.
    let (_echoes, stderr) = repl(b"x := i32 5\nq := @x ?\nq := @5 ?\n");
    assert_eq!(
        stderr.matches("this operator cannot compute over these operands").count(),
        2,
        "stderr: {stderr}"
    );
    // A scope's link up is `.back`; `.scope` on a scope is a field it does not have.
    let (_echoes, stderr) = repl(b"mut p := @dyad ?\np = here.scope\np.scope\nhere.scope.scope\n");
    assert_eq!(
        stderr.matches("this operator cannot compute over these operands").count(),
        2,
        "stderr: {stderr}"
    );
}

#[test]
fn a_pointer_parameter_takes_a_pointer_of_the_same_type_however_spelled() {
    // The seed mints a pointer type per spelling, so two `@@i32` pointees are two nodes describing one type.
    let (echoes, stderr) = repl(
        "x := i32 7\n\
         px := &x\n\
         q := &px\n\
         f := fn (p := @@i32 ?) -> i32 ( p@@ )\n\
         print «{f(q)}»\n\
         f(px)\n\
         pt := type (h := @@i32 ?)\n\
         v := pt(q)\n\
         print «{v.h@@}»\n\
         w := pt(px)\n",
    );
    assert_eq!(echoes, ["7", "7"], "stderr: {stderr}");
    assert_eq!(stderr.matches("these types do not match").count(), 2, "stderr: {stderr}");
}

#[test]
fn a_constructors_outcome_is_read_off_its_own_cell() {
    let (echoes, stderr) = repl(
        "r1 := type (static parse = ( tape.is_constructed[0] = true, tape.recenter(1) ))\n\
          r2 := type (static parse = ( tape.is_constructed[0] = true, tape.recenter(-1) ))\n\
          r3 := type (static parse = ( tape.is_constructed[0] = true, tape.recenter(99) ))\n\
          a := r1\nb := r2\nc := r3\nprint «{a.type == type}»\nprint «{b.type == type}»\n\
          c.type == type\n",
    );
    assert_eq!(echoes, ["true", "true", "true"], "stderr: {stderr}");
    for src in [
        &b"r4 := type (static parse = ( tape.recenter(1) ))\nx := r4\n"[..],
        b"r5 := type (static parse = ( tape[0] = tape.spelling[0] ))\nf := fn () -> void ( r5 )\n",
    ] {
        let (_echoes, stderr) = repl(src);
        assert!(
            stderr.contains("left its own cell unconstructed"),
            "{}: stderr: {stderr}",
            String::from_utf8_lossy(src)
        );
    }
    // `as_i32` hands its cell to `i32` with the flag false, so `as_i32 5` is `i32 5`.
    let (echoes, stderr) = repl(
        b"as_i32 := type (static parse = ( tape[0] = i32 ))\n\
          x := as_i32 5\nx + 1\n",
    );
    assert_eq!(echoes, ["6"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(
        "half := type (static parse = ( tape.insert(1, lex «/ 2») ))\n\
          8 half\n"
            .as_bytes(),
    );
    assert!(
        stderr.contains(
            "the constructor of `half` edited the tape but left its own cell unconstructed"
        ),
        "stderr: {stderr}"
    );
}

/// One command line run: its exit code (`None` for a signal), stdout and stderr.
fn run_line(src: &str) -> (Option<i32>, String, String) {
    let out = logos().arg(src).output().unwrap();
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (out.status.code(), text(&out.stdout), text(&out.stderr))
}

#[test]
fn a_parse_body_writes_a_field_as_a_node_and_the_run_reads_it() {
    let (code, stdout, stderr) = run_line(
        "probe := type ( v := i32 ?, static run = ( print «{v}» ), \
         static parse_rank = *.parse_rank + 1, \
         static parse = ( tape[0].type = probe, tape[0].v = 7, tape.is_constructed[0] = true ) ), probe",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "7\n"), "stderr: {stderr}");
    for (tail, want) in [("probe", "7\n"), ("probe + 1", "8\n")] {
        let (code, stdout, stderr) = run_line(&format!(
            "probe := type ( v := i32 ?, output_type := type ?, static run = ( v ), \
             static parse_rank = *.parse_rank + 1, \
             static parse = ( tape[0].type = probe, tape[0].v = 7, tape[0].output_type = i32, \
             tape.is_constructed[0] = true ) ), {tail}"
        ));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "stderr: {stderr}");
    }
    let (code, stdout, stderr) = run_line(
        "q := type ( size := u64 ?, static parse_rank = 60, \
         static parse = ( tape[0].type = q, tape[0].size = 3, tape[0] = tape[0].size, \
         tape.is_constructed[0] = true ) ), q",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "3\n"), "stderr: {stderr}");
}

#[test]
fn a_number_field_of_the_parse_node_holds_the_node_placed_into_it() {
    // `tape[0].n` is the node `n` holds, typed `u64`, so it takes part in arithmetic.
    let (code, stdout, stderr) = run_line(
        "q := type ( n := u64 ?, m := u64 ?, static parse_rank = 60, static parse = ( \
         tape[0].type = q, tape[0].n = tape[1].dyads.size, tape[0].m = tape[0].n * 2, \
         tape[0] = tape[0].m, tape.remove(1), \
         tape.is_constructed[0] = true ) ), q (1, 2, 3)",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "6\n"), "stderr: {stderr}");
    let (code, _, stderr) = run_line(
        "q := type ( n := u64 ?, static parse_rank = 60, \
         static parse = ( tape[0].type = q, tape[0].n = i32 3, tape.is_constructed[0] = true ) ), q",
    );
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("these types do not match"), "stderr: {stderr}");
}

#[test]
fn a_node_a_parse_places_inside_a_function_reads_that_functions_frame() {
    // A parse runs in a call of its own, and `f`'s body runs the node the parse built: what read
    // the parse's own names was run as it was placed, and what reads `f`'s reads `f`'s frame.
    let pr = "pr := type ( lhs := i32 ?, rhs := i32 ?, output_type := type ?, \
              static run = ( lhs + rhs ), static parse_rank = *.parse_rank + 1, static parse = ( \
              tape[0].type = pr, tape[0].lhs = tape[-1], tape[0].rhs = tape[0].lhs, \
              tape[0].output_type = i32, tape.is_constructed[0] = true, tape.remove(-1) ) )";
    let sq = "sq := type ( static parse_rank = *.parse_rank + 1, static parse = ( n := i32 40, \
              tape[0] = n + 1, tape.is_constructed[0] = true, tape.remove(-1) ) )";
    let inc = "inc := type ( static parse_rank = *.parse_rank + 1, static parse = ( \
               if tape[-1].type == i32 ( tape[0] = tape[-1] + 1, tape.is_constructed[0] = true ), \
               tape.remove(-1) ) )";
    let locals = "a := i32 91, b := i32 92, c := i32 93, d := i32 94";
    for (src, want) in [
        (format!("{pr}, f := fn (x := i32 ?) -> i32 ( x pr ), f(4)"), "8\n"),
        (format!("{pr}, f := fn (x := i32 ?) -> i32 ( x pr ), f.compile(), f(4)"), "8\n"),
        (format!("{pr}, y := i32 4, y pr"), "8\n"),
        (format!("{sq}, f := fn (y := i32 ?) -> i32 ( {locals}, y sq ), f(3)"), "41\n"),
        (
            format!("{sq}, f := fn (y := i32 ?) -> i32 ( {locals}, y sq ), f.compile(), f(3)"),
            "41\n",
        ),
        (format!("{inc}, f := fn (y := i32 ?) -> i32 ( y inc ), f(3)"), "4\n"),
        (format!("{inc}, f := fn (y := i32 ?) -> i32 ( y inc ), f.compile(), f(3)"), "4\n"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_parse_places_tape_lines_and_its_fields_as_nodes_and_runs_its_own_names() {
    let w = "w := fn (a := i32 ?) -> i32 ( a + 1 )";
    // `sq` writes `v` from the line before it; `cell` writes the line's cell.
    let sq = |rank: &str, body: &str| {
        format!(
            "{w}, sq := type ( v := i32 ?, output_type := type ?, static run = ( v ), \
             static parse_rank = {rank}, static parse = ( tape[0].type = sq, {body}, \
             tape[0].output_type = i32, tape.is_constructed[0] = true, tape.remove(-1) ) )"
        )
    };
    let field = |body: &str| sq("*.parse_rank + 1", &format!("if tape[-1].type == i32 ( {body} )"));
    let cell = |body: &str| {
        format!(
            "{w}, sq := type ( static parse_rank = *.parse_rank + 1, static parse = ( {body}, \
             tape.remove(-1) ) )"
        )
    };
    let narrowed = |body: &str| {
        cell(&format!("if tape[-1].type == i32 ( {body}, tape.is_constructed[0] = true )"))
    };
    let local = |body: &str| cell(&format!("n := i32 40, {body}, tape.is_constructed[0] = true"));
    let any = |body: &str| {
        format!(
            "sq := type ( v := i32 ?, static output_type := i32, static run = ( v ), \
             static parse_rank = *.parse_rank + 1, static parse = ( n := i32 40, tape[0].type = sq, \
             {body}, tape.is_constructed[0] = true, tape.remove(-1) ) )"
        )
    };
    let in_f = "f := fn (y := i32 ?) -> i32 ( y sq )";
    let counted = |body: &str| {
        format!(
            "q := type ( n := dyad ?, m := dyad ?, static parse_rank = 60, \
             static parse = ( tape[0].type = q, \
             tape[0].n = tape[1].dyads.size, {body}, tape[0] = tape[0].m, tape.remove(1), \
             tape.is_constructed[0] = true ) )"
        )
    };
    let checked_n = counted("if tape[0].n.type ⊆ u64 ( tape[0].m = tape[0].n * 2 )");
    let wide = "p := type ( rhs := i64 ?, static output_type := i64, static run = ( rhs ), \
                static parse_rank = 60, static parse = ( tape[0].type = p, \
                if tape[1].type ⊆ i64 ( tape[0].rhs = i64 (tape[1]) ), \
                tape.remove(1), tape.is_constructed[0] = true ) )";
    for (src, want) in [
        // A narrowed line into a number field, at any depth.
        (field("tape[0].v = w(tape[-1])") + ", x := i32 7, x sq", "8"),
        (field("tape[0].v = tape[-1]") + ", x := i32 7, x sq", "7"),
        (field("tape[0].v = tape[-1] + 1") + ", x := i32 7, x sq", "8"),
        (field("tape[0].v = w(w(tape[-1]))") + ", x := i32 7, x sq", "9"),
        (
            sq("+.parse_rank - 1", "if tape[-1].type == i32 ( tape[0].v = w(tape[-1]) )")
                + ", x := i32 7, x + 1 sq",
            "9",
        ),
        (field("tape[0].v = w(tape[-1])") + &format!(", {in_f}, f(3)"), "4"),
        (field("tape[0].v = w(tape[-1])") + &format!(", {in_f}, f.compile(), f(3)"), "4"),
        // A narrowed line into the cell, below a call's first level.
        (narrowed("tape[0] = w(w(tape[-1]))") + ", x := i32 7, x sq", "9"),
        (
            local("if tape[-1].type == i32 ( tape[0] = w(tape[-1] + n) )") + ", x := i32 7, x sq",
            "48",
        ),
        (narrowed("tape[0] = tape[-1]") + ", x := i32 7, x sq", "7"),
        (narrowed("tape[0] = tape[-1] + 1") + ", x := i32 7, x sq", "8"),
        (
            cell(
                "if tape[-1].type ⊆ i32 ( tape[0] = tape[-1] + 1, tape.is_constructed[0] = true )",
            ) + ", 7 sq",
            "8",
        ),
        // The parse's own names run as they are placed.
        (local("tape[0] = n + 1") + ", x := i32 7, x sq", "41"),
        (local("tape[0] = w(n)") + ", x := i32 7, x sq", "41"),
        (any("tape[0].v = n") + ", x := i32 7, x sq", "40"),
        (any("tape[0].v = n + 1") + ", x := i32 7, x sq", "41"),
        (
            "q := type ( n := u64 ?, static output_type := u64, static run = ( n ), \
             static parse_rank = 60, static parse = ( tape[0].type = q, \
             tape[0].n = tape[1].dyads.size, tape.remove(1), tape.is_constructed[0] = true ) ), \
             q (1, 2, 3)"
                .to_string(),
            "3",
        ),
        (
            "q := type ( v := u64 ?, static output_type := u64, static run = ( v ), \
             static parse_rank = 61, static parse = ( tape[0].type = q, \
             if not tape[1].dyads[0].type ⊆ u64 error «no», tape[0].v = tape[1].dyads[0] + 1, \
             tape.remove(1), tape.is_constructed[0] = true ) ), q [5]"
                .to_string(),
            "6",
        ),
        // An unnarrowed cell into a number field is filled per evaluation.
        (
            sq("*.parse_rank + 1", "tape[0].v = tape[-1]")
                + ", mut x := i32 7, f := fn () -> i32 ( x sq ), x = 10, f()",
            "10",
        ),
        // A `dyad` field is a number only inside a check of its type.
        (format!("{checked_n}, q (1, 2, 3)"), "6"),
        (format!("{checked_n}, f := fn () -> u64 ( q (1, 2, 3) ), f()"), "6"),
        (format!("{checked_n}, f := fn () -> u64 ( q (1, 2, 3) ), f.compile(), f()"), "6"),
        // A conversion placed over a line holds that line.
        (format!("n := i32 5, {wide}, p n"), "5"),
        (format!("{wide}, p 5"), "5"),
        // Over a literal line the field holds the constant a written `i64 (5)` makes, so the
        // node folds.
        (format!("{wide}, a := p 5, print «{{a}} {{a:start.rhs.type}}»"), "5 i64"),
        (format!("{wide}, f := fn (n := i32 ?) -> i64 ( p n ), f(5)"), "5"),
        (format!("{wide}, f := fn (n := i32 ?) -> i64 ( p n ), f.compile(), f(5)"), "5"),
        (format!("{wide}, f := fn () -> i64 ( p 5 ), f()"), "5"),
        (format!("{wide}, f := fn () -> i64 ( p 5 ), f.compile(), f()"), "5"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.trim()), (Some(0), want), "{src}: stderr: {stderr}");
    }
    let constant = "this is read as `i32`, but it holds no literal or constant of it";
    let used = ", q (1, 2, 3)";
    for (src, refused) in [
        // An unnarrowed line is no number.
        (
            cell("tape[0] = w(tape[-1]), tape.is_constructed[0] = true") + ", x := i32 7, x sq",
            "do not match",
        ),
        (sq("*.parse_rank + 1", "tape[0].v = w(tape[-1])") + ", x := i32 7, x sq", "do not match"),
        // While the parse runs, only a constant is read as its number.
        (
            "big := fn (a := i32 ?) -> bool ( a > 5 ), ".to_string()
                + &narrowed("if big(tape[-1]) ( tape[0] = i32 1 ) else ( tape[0] = i32 0 )")
                + ", x := i32 7, x sq",
            constant,
        ),
        (
            "x := i32 3, q := type ( n := i32 ?, static parse_rank = 60, static parse = ( \
             tape[0].type = q, tape[0].n = tape[1], if tape[0].n > 2 ( tape[0] = u64 7 ) \
             else ( tape[0] = u64 8 ), tape.remove(1), tape.is_constructed[0] = true ) ), q x"
                .to_string(),
            constant,
        ),
        // A `?` field unchecked, or after a write of it or a tape edit, is no number.
        (counted("tape[0].m = tape[0].n * 2") + used, "cannot compute over these operands"),
        (
            counted("if tape[0].n.type ⊆ u64 ( tape[0].n = 2, tape[0].m = tape[0].n * 2 )") + used,
            "cannot compute over these operands",
        ),
        (
            counted("if tape[0].n.type ⊆ u64 ( tape.remove(1), tape[0].m = tape[0].n * 2 )") + used,
            "cannot compute over these operands",
        ),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(refused), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_static_function_names_its_fields_bare() {
    let (code, _, stderr) =
        run_line("x := type ( n := u64 ?, static twice := fn () -> u64 ( n * 2 ) ), 1");
    assert_eq!(code, Some(0), "stderr: {stderr}");
    // Only the type's own functions reach a field by name, and a parse never does.
    for (src, expect) in [
        ("f := fn () -> u64 ( n ), 1", "unknown name `n`"),
        ("x := type ( n := u64 ?, static parse = ( n ) ), 1", "`tape[0].n`, never bare"),
        ("f := fn () -> u64 ( this.n ), 1", "unknown name `this`"),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_free_body_fills_the_values_free() {
    let (code, stdout, stderr) =
        run_line("x := type ( n := u64 ?, static free = ( print «gone» ) ), 1");
    assert_eq!((code, stdout.as_str()), (Some(0), "1\n"), "stderr: {stderr}");
    for (src, expect) in [
        ("x := type ( static free = 5 ), 1", "`static free = (…)`"),
        ("x := type ( mut p := own @i32 ? ), 1", "must be freed by the type's"),
        ("x := type ( p := own i32 ? ), 1", "written over a pointer"),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

/// A collection in the array's shape whose values' `free` prints `free` before it frees, so a
/// run shows each teardown: `box (…)` is a node whose run makes a new `boxed`, and `box`
/// with no bracket is `boxed`.
const BOX: &str = "boxed := type ( \
    mut p := own @i32 ?, \
    mut size := u64 0, \
    static free = ( print «free», free p ), \
    static parse_rank = dyad.parse_rank, \
    static parse = ( tape.is_constructed[0] = true ) \
), \
box := type ( \
    elements := scope ?, \
    output_type := type ?, \
    static run = ( \
        mut v := output_type ?, \
        v.p = mut alloc elements.dyads.size of i32 ?, \
        for i in 0..elements.dyads.size ( \
            (v.p + i)@ = elements.dyads[i], \
            v.size = i + 1 \
        ), \
        move v \
    ), \
    static parse_rank = dyad.parse_rank, \
    static associativity = left, \
    static parse = ( \
        if tape[1].type == scope ( \
            for i in 0..tape[1].dyads.size ( \
                if not (tape[1].dyads[i].type ⊆ i32) error «not an i32» \
            ), \
            tape[0].type = box, \
            tape[0].elements = tape[1], \
            tape[0].output_type = boxed, \
            tape.remove(1) \
        ) else ( tape[0] = boxed ), \
        tape.is_constructed[0] = true \
    ) \
)";

#[test]
fn the_owner_s_scope_end_runs_the_instances_free_once() {
    for (tail, printed) in [
        // Made in a function and not returned: freed as the call ends, each call its own.
        (
            "f := fn () -> i32 ( a := box (6, 1), 3 ), print «before», free (f()), free (f()), \
             print «after»",
            "before\nfree\nfree\nafter\n",
        ),
        (
            "f := fn () -> i32 ( a := box (6, 1), 3 ), f.compile(), print «before», free (f()), \
             print «after»",
            "before\nfree\nafter\n",
        ),
        // The value given moves out: the caller's name is the owner.
        (
            "mk := fn () -> own box ( a := box (9, 1), a ), \
             g := fn () -> i32 ( m := mk(), print «got», (m.p + 0)@ ), free (g()), print «after»",
            "got\nfree\nafter\n",
        ),
        ("mk := fn () -> own box ( box (9, 1) ), m := mk(), print «got»", "got\nfree\n"),
        // A copy is its name's own: each name frees its own value. An argument borrows and frees
        // nothing: one free, by the owner.
        ("a := box (5, 1), b := a, print «copied»", "copied\nfree\nfree\n"),
        (
            "a := box (5, 1), f := fn (q := box ?) -> i32 ( (q.p + 0)@ ), free (f(a)), print «after»",
            "after\nfree\n",
        ),
        ("a := box (7, 1), b := move a, print «moved»", "moved\nfree\n"),
        // `=` frees the value it displaces and takes a copy of its own.
        ("mut a := box (7, 1), b := box (8, 1), a = b, print «set»", "free\nset\nfree\nfree\n"),
        // An early `free` tears down now, and the scope end finds nothing left.
        ("a := box (7, 1), free a, print «after»", "free\nafter\n"),
        // A copy moves out as its name's last value: the callee frees its own as it returns.
        (
            "mk := fn () -> own box ( a := box (9, 1), b := a, b ), m := mk(), print «got»",
            "free\ngot\nfree\n",
        ),
        // An `if` whose every arm makes or moves the value hands it on as either arm would.
        ("c := true, x := if c (box (1, 2)) else (box (3, 4)), print «{x.size}»", "2\nfree\n"),
        (
            "c := true, a := box (1, 2), b := box (3, 4, 5), x := if c (move a) else (move b), \
             print «mid»",
            "free\nmid\nfree\n",
        ),
        ("c := true, free (if c (box (1, 2)) else (box (3, 4))), print «after»", "free\nafter\n"),
        (
            "c := true, mk := fn () -> own box ( if c (box (1, 2)) else (box (3, 4)) ), m := mk(), \
             print «got»",
            "got\nfree\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{BOX}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert!(stdout.starts_with(printed), "{tail}: stdout: {stdout}");
        assert_eq!(stdout.matches("free").count(), printed.matches("free").count(), "{tail}");
    }
    for (tail, expect) in [
        ("a := box (7, 1), free a, a", "`a` is dead here"),
        (
            "f := fn (q := box ?) -> i32 ( c := move q, 1 ), a := box (7, 1), f(a)",
            "only its owner can",
        ),
        ("mk := fn () -> box ( a := box (9, 1), b := a, b ), m := mk()", "write `-> own box`"),
    ] {
        let (code, _, stderr) = run_line(&format!("{BOX}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn a_call_holds_a_value_passed_straight_and_frees_it_as_it_returns() {
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ), \
               static get := fn () -> i32 ( n ) )";
    for (src, printed) in [
        // Made or moved in the argument: the parameter holds it, on either tier.
        (format!("{BOX}, f := fn (q := box ?) -> i32 ( 1 ), free (f(box (1, 2))), print «after»"), "free\nafter\n"),
        (
            format!("{BOX}, f := fn (q := box ?) -> i32 ( 1 ), f.compile(), free (f(box (1, 2))), print «after»"),
            "free\nafter\n",
        ),
        (
            format!(
                "{BOX}, f := fn (q := box ?) -> i32 ( 1 ), g := fn () -> i32 ( f(box (1, 2)) ), \
                 g.compile(), free (g()), print «after»"
            ),
            "free\nafter\n",
        ),
        (
            format!("{BOX}, b := box (1, 2), f := fn (q := box ?) -> i32 ( 1 ), free (f(move b)), print «after»"),
            "free\nafter\n",
        ),
        (
            format!(
                "{BOX}, mk := fn () -> own box ( box (1, 2) ), f := fn (q := box ?) -> i32 ( 1 ), \
                 free (f(mk())), print «after»"
            ),
            "free\nafter\n",
        ),
        (format!("{BOX}, f := fn (q := dyad ?) -> i32 ( 1 ), free (f(box (1, 2))), print «after»"), "free\nafter\n"),
        (
            format!(
                "{BOX}, c := true, f := fn (q := box ?) -> i32 ( 1 ), \
                 free (f(if c (box (1, 2)) else (box (3, 4)))), print «after»"
            ),
            "free\nafter\n",
        ),
        // A record in its bytes, freed over the bytes the call passed; the last argument first.
        (
            format!("{bag}, f := fn (p := bag ?, q := bag ?) -> i32 ( p.n + q.n ), print «{{f(bag(1), bag(2))}}»"),
            "freed 2\nfreed 1\n3\n",
        ),
        (
            format!(
                "{bag}, f := fn (q := bag ?) -> i32 ( q.n ), g := fn () -> i32 ( f(bag(1)) ), \
                 g.compile(), free (g()), print «after»"
            ),
            "freed 1\nafter\n",
        ),
        // A name passed, or a receiver, is a borrow: its owner's end frees it, once.
        (
            format!("{BOX}, b := box (1, 2), f := fn (q := box ?) -> i32 ( 1 ), free (f(b)), print «after»"),
            "after\nfree\n",
        ),
        (format!("{bag}, x := bag(1), free (x.get()), print «after»"), "after\nfreed 1\n"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{src}");
    }
    // A fault in the callee, or in a later argument, ends what the call holds, on either tier.
    let raise = "f := fn (q := box ?) -> i32 ( error «boom» )";
    let nan = "f := fn (q := box ?, x := f64 ?) -> i32 ( i32 lossy (x) ), f.compile()";
    let later =
        "f2 := fn (q := box ?, n := i32 ?) -> i32 ( 1 ), g := fn () -> i32 ( error «boom» )";
    for (tail, expect) in [
        (format!("{later}, f2(box (1, 2), g())"), "boom"),
        (format!("{later}, b := box (1, 2), f2(move b, g())"), "boom"),
        (format!("{later}, f2.compile(), f2(box (1, 2), g())"), "boom"),
        (format!("{later}, h := fn () -> i32 ( f2(box (1, 2), g()) ), h.compile(), h()"), "boom"),
        (format!("{later}, f2.compile(), h := fn () -> i32 ( f2(box (1, 2), g()) ), h.compile(), h()"), "boom"),
        (format!("{raise}, f(box (1, 2))"), "boom"),
        (format!("{raise}, g := fn () -> i32 ( f(box (1, 2)) ), g.compile(), g()"), "boom"),
        (format!("{nan}, f(box (1, 2), f64 0.0 / 0.0)"), "cannot convert"),
        (
            format!("{nan}, g := fn (x := f64 ?) -> i32 ( f(box (1, 2), x) ), g.compile(), g(f64 0.0 / 0.0)"),
            "cannot convert",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{BOX}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, "free\n", "{tail}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
    // Nothing holds it yet: a record's field and a parameter `=` may write over.
    for tail in [
        "holder := type ( b := box ? ), h := holder(box (1, 2))",
        "f := fn (mut q := box ?) -> i32 ( 1 ), f(box (1, 2))",
    ] {
        let (code, _, stderr) = run_line(&format!("{BOX}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(
            stderr.contains("an owning value must be bound to a name"),
            "{tail}: stderr: {stderr}"
        );
    }
}

#[test]
fn the_program_s_end_runs_its_defers_and_what_it_holds_last_first() {
    for (src, printed) in [
        ("defer print «hi», 5".to_string(), "hi\n5\n"),
        ("defer print «a», defer print «b», 1".to_string(), "b\na\n1\n"),
        (
            "mut n := i32 1, f := fn () -> void ( print «{n}» ), defer f(), n = 3, 7".to_string(),
            "3\n7\n",
        ),
        (format!("{BOX}, a := box (1, 2), defer print «d», print «made»"), "made\nd\nfree\n"),
        (
            "import file «./tests/fixtures/deferring.logos», print «main», k".to_string(),
            "main\nfile ends\n1\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{src}");
    }
    // A line that gives a value ends the session as a program ends: its defers, then the value.
    let (echoes, stderr) = repl("defer print «bye»\n1\n");
    assert_eq!(echoes, ["bye", "1"], "stderr: {stderr}");
    // An imported name is the file's own place: ending it ends the file's hold, once.
    for src in [
        "import file «./tests/fixtures/owning_pub.logos», p@",
        "import file «./tests/fixtures/owning_pub.logos», free p, 1",
        "import file «./tests/fixtures/owning_pub.logos», q := move p, q@",
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert!(matches!(stdout.trim(), "7" | "1"), "{src}: {stdout}");
    }
    let (echoes, stderr) = repl("import file «./tests/fixtures/owning_pub.logos»\nfree p\n1\n");
    assert_eq!(echoes, ["1"], "stderr: {stderr}");
}

#[test]
fn a_fault_ends_every_live_scope_as_its_end_would() {
    let array = "import file «./identities/array.logos»";
    for (tail, printed, error) in [
        (
            "f := fn () -> i32 ( a := box (1, 2), defer print «bye», error «stop» ), f()",
            "bye\nfree\n",
            "stop",
        ),
        ("a := box (1, 2), defer print «bye», error «stop»", "bye\nfree\n", "stop"),
        // Only what was declared before the fault ends.
        ("a := box (1, 2), error «stop», b := box (3, 4), print «never»", "free\n", "stop"),
        // The arm that did not move the name frees it on the fault's way out.
        (
            "c := i32 0, a := box (1, 2), if (c == 1) ( b := move a ) else ( error «stop» ), 1",
            "free\n",
            "stop",
        ),
        ("f := fn () -> i32 ( ( a := box (1, 2), error «in» ) ), f()", "free\n", "in"),
        // Lines the pass ran early, in a block the fault unwinds.
        (
            "g := fn () -> i32 ( error «stop» ), x := ( a := box (1, 2), n := immediate g(), 1 ), 1",
            "free\n",
            "stop",
        ),
        (
            "g := fn () -> i32 ( error «stop» ), \
             x := ( a := box (1, 2), free a, n := immediate g(), 1 ), 1",
            "free\n",
            "stop",
        ),
        (
            "f := fn (n := i32 ?) -> i32 ( a := box (1, 2), b := array i32 [1, 2], b[n] ), \
             f.compile(), f(5)",
            "free\n",
            "index out of range",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {BOX}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(1), printed), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(error), "{tail}: stderr: {stderr}");
    }
    // An imported file that faults ends its own scope before the import fails.
    let (code, stdout, stderr) = run_line("import file «./tests/fixtures/faulting.logos», 1");
    assert_eq!((code, stdout.as_str()), (Some(1), "file cleanup\nfreed\n"), "stderr: {stderr}");
    assert!(stderr.contains("file stops"), "stderr: {stderr}");
    // The stopped function's `defer` runs, the line after the fault does not.
    let (echoes, stderr) = repl(
        "p := alloc 1 of i32 5\n\
         g := fn () -> i32 ( error «stop» )\n\
         f := fn () -> i32 ( defer (p@ = p@ + 100), free (g()), p@ = 9, 1 )\n\
         f()\n\
         p@\n"
            .as_bytes(),
    );
    assert_eq!(echoes.last().map(String::as_str), Some("105"), "{echoes:?} {stderr}");
    assert!(stderr.contains("stop"), "stderr: {stderr}");
}

#[test]
fn a_jump_into_compiled_code_keeps_a_fault_parked_before_it() {
    let nan = "a float that is not a number (NaN)";
    let range = "index out of range";
    let checked = "import file «./identities/array.logos», \
         h := fn (x := f64 ?) -> i32 ( i32 lossy (x) ), h.compile(), \
         bag := type ( mut v := f64 ?, static free = ( print «freed {h(v)}» ) ), z := f64 0.0";
    for (before, after, printed, error) in [
        // The fault's cleanup enters compiled code.
        (
            "h := fn (k := i32 ?) -> i32 ( k ), h.compile(), \
             bag := type ( mut n := i32 ?, static free = ( print «freed {h(n)}» ) ), \
             g := fn (x := f64 ?) -> i32 ( b := bag(4), n := i32 lossy (x), n )"
                .to_string(),
            "z := f64 0.0, g(z / z)",
            "freed 4\n",
            nan,
        ),
        // A call through the interpreter after the fault enters compiled code.
        (
            "h := fn (k := i32 ?) -> i32 ( k ), h.compile(), \
             say := fn (k := i32 ?) -> i32 ( h(k) ), \
             g := fn (x := f64 ?) -> i32 ( n := i32 lossy (x), say(n) )"
                .to_string(),
            "z := f64 0.0, g(z / z)",
            "",
            nan,
        ),
        // The cleanup's compiled call faults too: the first fault is the one shown.
        (
            format!(
                "{checked}, \
                 g := fn (n := i32 ?) -> i32 ( a := bag(z / z), b := array i32 [1, 2], b[n] )"
            ),
            "g(5)",
            "",
            range,
        ),
        (
            format!(
                "{checked}, \
                 g := fn (n := i32 ?) -> i32 ( a := bag(f64 1.5), b := array i32 [1, 2], b[n] )"
            ),
            "g(5)",
            "freed 1\n",
            range,
        ),
    ] {
        let interpreted = run_line(&format!("{before}, {after}"));
        let compiled = run_line(&format!("{before}, g.compile(), {after}"));
        assert_eq!(compiled, interpreted, "{before}");
        let (code, stdout, stderr) = interpreted;
        assert_eq!((code, stdout.as_str()), (Some(1), printed), "{before}: stderr: {stderr}");
        assert!(stderr.contains(error), "{before}: stderr: {stderr}");
    }
}

/// ›A checked error is a fault: the task that hit it is cancelled‹ [[R141]]: "no further
/// steps", each scope it leaves ended as at its end, and a `free` that fails during that cleanup
/// stopping its scope's cleanup, on both tiers.
#[test]
fn compiled_code_stops_at_a_fault_as_the_interpreter_does() {
    let nan = "a float that is not a number (NaN)";
    let range = "index out of range";
    let checked = "import file «./identities/array.logos», \
         h := fn (x := f64 ?) -> i32 ( i32 lossy (x) ), h.compile(), \
         bag := type ( mut v := f64 ?, static free = ( print «freed {h(v)}» ) ), z := f64 0.0";
    let say = "say := fn (k := i32 ?) -> i32 ( print «say {k}», k )";
    let fault = "z := f64 0.0, g(z / z)";
    for (before, after, printed, error) in [
        // Nothing after the fault runs: a `print`, a call, a declaration.
        (
            "g := fn (x := f64 ?) -> i32 ( n := i32 lossy (x), print «n {n}», n )".to_string(),
            fault,
            "",
            nan,
        ),
        (
            format!("{say}, g := fn (x := f64 ?) -> i32 ( n := i32 lossy (x), say(n) )"),
            fault,
            "",
            nan,
        ),
        (
            format!("{RECORD}, g := fn (x := f64 ?) -> i32 ( n := i32 lossy (x), d := bag(3), n )"),
            fault,
            "",
            nan,
        ),
        // What was declared before it ends, a `defer` and an `if` arm's included.
        (
            format!(
                "{RECORD}, g := fn (x := f64 ?) -> i32 ( b := bag(4), n := i32 lossy (x), \
                 d := bag(3), n )"
            ),
            fault,
            "freed 4\n",
            nan,
        ),
        (
            format!(
                "{say}, g := fn (x := f64 ?) -> i32 ( defer free (say(1)), n := i32 lossy (x), \
                 say(n) )"
            ),
            fault,
            "say 1\n",
            nan,
        ),
        (
            format!(
                "{RECORD}, g := fn (x := f64 ?) -> i32 ( a := bag(1), y := if (x > 0.0) \
                 ( b := bag(2), i32 lossy (x / 0.0) ) else ( i32 0 ), d := bag(3), y )"
            ),
            "g(f64 1.0)",
            "freed 2\nfreed 1\n",
            nan,
        ),
        // A null pointer, and the stack's limit.
        (
            "g := fn (q := @@i32 ?) -> i32 ( n := q@@, print «n {n}», n )".to_string(),
            "p := alloc 1 of @i32 ?, g(p)",
            "",
            "this pointer holds nothing yet",
        ),
        (
            "g := fn (n := i32 ?) -> i32 ( k := g(n + 1), print «back», k )".to_string(),
            "g(0)",
            "",
            "nested too deep: the run used up its 64 MiB stack",
        ),
        // A `free` that faults in the cleanup stops its scope's cleanup, and a call's.
        (
            format!(
                "{checked}, g := fn (n := i32 ?) -> i32 ( a := bag(f64 1.5), c := bag(z / z), \
                 b := array i32 [1, 2], b[n] )"
            ),
            "g(5)",
            "",
            range,
        ),
        (
            format!(
                "{checked}, g := fn (n := i32 ?) -> i32 ( a := bag(f64 1.5), \
                 x := ( c := bag(z / z), b := array i32 [1, 2], b[n] ), x )"
            ),
            "g(5)",
            "freed 1\n",
            range,
        ),
        (
            format!(
                "{checked}, k := fn (p := bag ?, q := bag ?, n := i32 ?) -> i32 \
                 ( b := array i32 [1, 2], b[n] ), \
                 g := fn (n := i32 ?) -> i32 ( k(bag(f64 1.5), bag(z / z), n) )"
            ),
            "g(5)",
            "",
            range,
        ),
        // Three scopes deep: each ends once, and a fault in the middle one's end skips only it.
        (
            format!(
                "{RECORD}, g := fn (x := f64 ?) -> i32 ( a := bag(1), y := ( b := bag(2), \
                 w := ( c := bag(3), i32 lossy (x) ), w ), y )"
            ),
            fault,
            "freed 3\nfreed 2\nfreed 1\n",
            nan,
        ),
        (
            format!(
                "{checked}, g := fn (n := i32 ?) -> i32 ( a := bag(f64 1.5), \
                 y := ( b := bag(f64 2.5), d := bag(z / z), \
                 w := ( c := bag(f64 3.5), e := array i32 [1, 2], e[n] ), w ), y )"
            ),
            "g(5)",
            "freed 3\nfreed 1\n",
            range,
        ),
        // A fault in a `defer` stops the end it runs in.
        (
            format!(
                "{RECORD}, h := fn (x := f64 ?) -> i32 ( i32 lossy (x) ), \
                 g := fn (x := f64 ?) -> i32 ( a := bag(1), defer free (h(x)), b := bag(2), 3 )"
            ),
            fault,
            "freed 2\n",
            nan,
        ),
        // A node the failing line ended and did not reach is freed, one it reached is not.
        (
            format!(
                "{BOX}, f := fn () -> i32 ( error «stop» ), \
                 g := fn () -> i32 ( a := box (1, 2), x := f() + (free a, i32 1), x )"
            ),
            "print «before», g()",
            "before\nfree\n",
            "run error: stop",
        ),
        (
            format!(
                "{BOX}, f := fn () -> i32 ( error «stop» ), \
                 g := fn () -> i32 ( a := box (1, 2), x := (free a, i32 1) + f(), x )"
            ),
            "print «before», g()",
            "before\nfree\n",
            "run error: stop",
        ),
    ] {
        let interpreted = run_line(&format!("{before}, {after}"));
        let compiled = run_line(&format!("{before}, g.compile(), {after}"));
        assert_eq!(compiled, interpreted, "{before}");
        let (code, stdout, stderr) = interpreted;
        assert_eq!((code, stdout.as_str()), (Some(1), printed), "{before}: stderr: {stderr}");
        assert!(stderr.contains(error), "{before}: stderr: {stderr}");
    }
}

#[test]
fn assignment_frees_the_value_it_displaces() {
    for (tail, printed) in [
        ("mut x := box (1, 2), x = box (3, 4), print «after»", "free\nafter\nfree\n"),
        (
            "f := fn () -> i32 ( mut x := box (1, 2), x = box (3, 4), 1 ), f.compile(), free (f()), \
             print «after»",
            "free\nfree\nafter\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{BOX}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{tail}");
    }
}

/// A plain record whose type fills `static free`, its free printing the one field.
const RECORD: &str = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) )";

#[test]
fn a_plain_record_s_owner_runs_its_free_once() {
    for (tail, printed) in [
        // The scope's end runs it; the program's before the tail shows.
        ("a := bag(1), 1", "freed 1\n1\n"),
        ("f := fn () -> i32 ( a := bag(2), 1 ), f()", "freed 2\n1\n"),
        (
            "f := fn () -> i32 ( a := bag(3), 1 ), print «before», free (f()), free (f()), print «after»",
            "before\nfreed 3\nfreed 3\nafter\n",
        ),
        (
            "f := fn () -> i32 ( a := bag(4), 1 ), f.compile(), print «before», free (f()), print «after»",
            "before\nfreed 4\nafter\n",
        ),
        // `bag ?` owns too: the end runs its free over whatever was written.
        (
            "f := fn () -> i32 ( mut a := bag ?, a.n = 5, 1 ), free (f()), print «after»",
            "freed 5\nafter\n",
        ),
        // An early `free` tears down now, and the end finds nothing held.
        ("a := bag(6), free a, print «after»", "freed 6\nafter\n"),
        ("a := bag(7), if true ( free a ), print «after»", "freed 7\nafter\n"),
        ("c := i32 0, a := bag(15), if (c == 1) ( free a ), print «after»", "freed 15\nafter\n"),
        // A copy is its name's own: each name frees its own value, `free` of the copy now. An
        // argument borrows and frees nothing: one free, by the owner.
        ("a := bag(8), b := a, print «copied»", "copied\nfreed 8\nfreed 8\n"),
        ("a := bag(14), b := a, free b, print «after»", "freed 14\nafter\nfreed 14\n"),
        (
            "a := bag(9), f := fn (q := bag ?) -> i32 ( q.n ), free (f(a)), print «after»",
            "after\nfreed 9\n",
        ),
        // The last value moves out: the caller's name is the owner.
        ("mk := fn () -> own bag ( bag(10) ), m := mk(), print «got»", "got\nfreed 10\n"),
        ("mk := fn () -> own bag ( a := bag(11), a ), m := mk(), print «got»", "got\nfreed 11\n"),
        // A copy moves out as its name's last value: the callee frees its own as it returns.
        (
            "mk := fn () -> own bag ( a := bag(12), b := a, b ), m := mk(), print «got»",
            "freed 12\ngot\nfreed 12\n",
        ),
        ("free (bag(16)), print «after»", "freed 16\nafter\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
    for (tail, expect) in [
        ("a := bag(1), free a, a", "`a` is dead here"),
        ("a := bag(1), b := a, free b, b", "`b` is dead here"),
        ("a := bag(1), b := move a, a.n", "`a` is dead here"),
    ] {
        let (code, _, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
    // A later REPL line frees what an earlier one holds, and the session's end frees it no more.
    let (echoes, stderr) = repl(format!("{RECORD}\na := bag(13)\nfree a\n").as_bytes());
    assert_eq!(echoes, ["freed 13"], "stderr: {stderr}");
}

/// ›A last value moves out‹ [[R66]]: a line that gives its block a name the block holds moves
/// it out on the paths where it gives it, with no `move` written, and the paths that go on free
/// it where the `if` ends.
#[test]
fn a_block_moves_out_a_name_it_holds_on_the_paths_that_give_it() {
    let block = "x := ( a := bag(1), if c (a), bag(2) ), print «end»";
    let f = "f := fn (c := bool ?) -> own bag ( a := bag(1), if c (a), bag(2) )";
    for (tail, printed) in [
        (format!("c := true, {block}"), "end\nfreed 1\n"),
        (format!("c := false, {block}"), "freed 1\nend\nfreed 2\n"),
        (format!("{f}, x := f(true), print «end»"), "end\nfreed 1\n"),
        (format!("{f}, x := f(false), print «end»"), "freed 1\nend\nfreed 2\n"),
        (format!("{f}, f.compile(), x := f(true), print «end»"), "end\nfreed 1\n"),
        (format!("{f}, f.compile(), x := f(false), print «end»"), "freed 1\nend\nfreed 2\n"),
        (
            "c := true, x := ( a := bag(1), if c (a) else (bag(2)) ), print «end»".into(),
            "end\nfreed 1\n",
        ),
        ("x := ( a := bag(1), ( k := i32 1, a ) ), print «end»".into(), "end\nfreed 1\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{tail}");
    }
    // A reader to the left takes the value, so the arm borrows; one to the right is refused.
    let (code, stdout, stderr) = run_line(
        "c := true, a := alloc 1 of i32 7, b := alloc 1 of i32 8, y := if c (a) else (b), y@",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "7\n"), "stderr: {stderr}");
    for (src, expect) in [
        (
            "c := true, x := ( a := alloc 1 of i32 7, b := alloc 1 of i32 8, if c (a) else (b) + 1 ), 0",
            "this line takes the value on the way",
        ),
        ("x := ( a := alloc 1 of i32 7, &a ), 0", "hands out the address of a name its scope holds"),
        (
            "x := ( a := alloc 1 of i32 7, for i in 0..3 ( if (i == 1) (a) ), alloc 1 of i32 9 ), 0",
            "give it with `move`",
        ),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

/// DESIGN ›`move` is the act, `own` the gate word, `free` the end‹ [[R58]]: `move a` of a plain
/// record copies its bytes into the name or parameter that takes it, which frees it once, and
/// `a` is dead from that line, on both tiers.
#[test]
fn a_plain_record_moves_by_its_bytes() {
    let g = "g := fn (q := bag ?) -> i32 ( print «in g», q.n )";
    let pick = "f := fn (c := bool ?) -> i32 ( a := bag(1), x := if c (bag(3)) else (move a), \
                print «end», x.n )";
    for (tail, printed) in [
        ("a := bag(1), b := move a, print «end»".to_string(), "end\nfreed 1\n"),
        ("a := bag(1), b := move a, c := move b, print «end»".into(), "end\nfreed 1\n"),
        (
            "f := fn () -> i32 ( a := bag(2), b := move a, print «in», 1 ), f.compile(), \
             free (f()), print «after»"
                .into(),
            "in\nfreed 2\nafter\n",
        ),
        (format!("{g}, a := bag(3), free (g(move a)), print «end»"), "in g\nfreed 3\nend\n"),
        (
            format!(
                "{g}, h := fn () -> i32 ( a := bag(3), free (g(move a)), print «end», 1 ), \
                 h.compile(), free (h()), print «after»"
            ),
            "in g\nfreed 3\nend\nafter\n",
        ),
        // The arm that does not move `a` frees it at the `if`'s close.
        (
            format!("{pick}, free (f(false)), free (f(true)), print «after»"),
            "end\nfreed 1\nfreed 1\nend\nfreed 3\nafter\n",
        ),
        (
            format!("{pick}, f.compile(), free (f(false)), free (f(true)), print «after»"),
            "end\nfreed 1\nfreed 1\nend\nfreed 3\nafter\n",
        ),
        (
            "mk := fn () -> own bag ( a := bag(5), move a ), m := mk(), print «got»".into(),
            "got\nfreed 5\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
    let pt = "pt := type ( x := i32 ?, y := i32 ? )";
    for tail in [
        "a := pt(1, 2), b := move a, b.y",
        "f := fn () -> i32 ( a := pt(1, 2), b := move a, b.y ), f.compile(), f()",
        "g := fn (q := pt ?) -> i32 ( b := move q, b.y ), g(pt(1, 2))",
    ] {
        let (code, stdout, stderr) = run_line(&format!("{pt}, {tail}"));
        assert_eq!((code, stdout.trim()), (Some(0), "2"), "{tail}: stderr: {stderr}");
    }
    for (tail, expect) in [
        ("a := bag(1), b := move a, a.n", "`a` is dead here"),
        (
            "f := fn () -> i32 ( a := bag(1), b := move a, a.n ), f.compile(), f()",
            "`a` is dead here",
        ),
        // A parameter only borrows what the caller holds, so the caller's end frees it.
        (
            "g := fn (q := bag ?) -> i32 ( b := move q, b.n ), a := bag(3), g(a)",
            "this name borrows what it holds",
        ),
        ("a := bag(1), for i in 0..2 ( b := move a ), 1", "declared outside this loop"),
    ] {
        let (code, _, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
    // A record field stays where it is: its holder fills `free`, which frees it with the rest,
    // and a holder that does not is refused where it is defined.
    let owns_nothing = "a field or cell that owns nothing is not in the seed yet";
    for (holder, expect) in [
        ("h := type ( mut b := bag ?, z := i32 ? )", "the field `b` owns what it holds"),
        ("h := type ( mut b := bag ?, static free = ( free b ) )", "fills its own `static free"),
    ] {
        for tail in [
            "mut v := h ?, v.b = bag(1), w := move v.b, 1",
            "f := fn () -> i32 ( mut v := h ?, v.b = bag(1), w := move v.b, 1 ), f.compile(), f()",
        ] {
            let (code, stdout, stderr) = run_line(&format!("{RECORD}, {holder}, {tail}"));
            assert_eq!((code, stdout.as_str()), (Some(1), ""), "{tail}: stderr: {stderr}");
            assert!(stderr.contains(expect), "{holder}, {tail}: stderr: {stderr}");
        }
    }
    let tail = "h := type ( p := pt ?, z := i32 ? ), v := h(pt(1, 2), 3), w := move v.p, w.y";
    let (code, _, stderr) = run_line(&format!("{pt}, {tail}"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains(owns_nothing), "stderr: {stderr}");
    let (echoes, stderr) =
        repl(format!("{RECORD}\na := bag(1)\nb := move a\nprint «mid»\n").as_bytes());
    assert_eq!(echoes, ["mid", "freed 1"], "stderr: {stderr}");
}

/// ›A type whose fields carry teardowns must write its own destructor‹ [[R59]]: a field that owns
/// what it holds, an `own` field or a record whose type fills `free`, is refused at its type's
/// definition unless the type fills `free`; that `free` frees it once, and `=` frees the record it
/// displaces, on both tiers.
#[test]
fn a_type_frees_the_records_its_fields_hold() {
    for (tail, expect) in [
        ("h := type ( mut b := bag ?, z := i32 ? ), 1", "the field `b` owns what it holds"),
        (
            "h := type ( mut p := own @i32 ?, mut b := bag ? ), 1",
            "the fields `p`, `b` own what they hold",
        ),
        (
            "h := type ( mut b := bag ?, static free = ( free b ) ), k := type ( c := h ? ), 1",
            "the field `c` owns what it holds",
        ),
        ("f := fn () -> i32 ( h := type ( b := bag ? ), 1 ), f()", "the field `b` owns"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
    // A record whose type fills no `free` gives its holder nothing to free.
    let (code, stdout, stderr) = run_line(
        "m := type ( k := i32 ? ), h := type ( b := m ?, z := i32 ? ), v := h(m(1), 2), v.z",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "2\n"), "stderr: {stderr}");
    let h = "h := type ( mut b := bag ?, z := i32 ?, static free = ( print «h», free b ) )";
    let body = "a := bag(1), mut v := h(a, 2), v.b = bag(3), v.b.n = 7, print «end»";
    for (tail, printed) in [
        ("a := bag(1), v := h(a, 2), print «end»".to_string(), "end\nh\nfreed 1\nfreed 1\n"),
        (body.into(), "freed 1\nend\nh\nfreed 7\nfreed 1\n"),
        (format!("f := fn () -> i32 ( {body}, 0 ), f()"), "freed 1\nend\nh\nfreed 7\nfreed 1\n0\n"),
        (
            format!("f := fn () -> i32 ( {body}, 0 ), f.compile(), f()"),
            "freed 1\nend\nh\nfreed 7\nfreed 1\n0\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {h}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
}

/// DESIGN ›A value's teardown runs where its life ends; the ending identity reads the type's
/// `free` slot‹ [[R57]]: a line at its end frees what it made that nothing took, after its own
/// reads, on both tiers.
#[test]
fn a_line_frees_at_its_end_what_it_made_that_nothing_took() {
    let mk = "mk := fn (k := i32 ?) -> own bag ( bag(k) )";
    for (tail, printed) in [
        ("free (bag(1)), print «after»".to_string(), "freed 1\nafter\n"),
        (
            "f := fn () -> i32 ( free (bag(2)), 1 ), print «before», free (f()), print «after»".into(),
            "before\nfreed 2\nafter\n",
        ),
        (
            "f := fn () -> i32 ( free (bag(2)), 1 ), f.compile(), print «before», free (f()), print «after»"
                .into(),
            "before\nfreed 2\nafter\n",
        ),
        (format!("{mk}, free (mk(3)), print «after»"), "freed 3\nafter\n"),
        (
            format!("{mk}, g := fn () -> i32 ( free (mk(3)), 1 ), g.compile(), free (g()), print «after»"),
            "freed 3\nafter\n",
        ),
        // A loop body's value passes out, so a pass throws away what it made with `free`.
        ("for i in 0..2 ( free (bag(i)) ), print «end»".into(), "freed 0\nfreed 1\nend\n"),
        (
            "g := fn () -> i32 ( for i in 0..2 ( free (bag(i)) ), 1 ), g.compile(), g()".into(),
            "freed 0\nfreed 1\n1\n",
        ),
        // So does an `if`'s without `else`, and a branch that does not run makes nothing to free.
        ("c := true, if c ( free (bag(4)) ), print «after»".into(), "freed 4\nafter\n"),
        (
            "g := fn (c := bool ?) -> i32 ( if c ( free (bag(4)) ), 1 ), g.compile(), print «{g(false)}», g(true)"
                .into(),
            "1\nfreed 4\n1\n",
        ),
        (
            "g := fn () -> i32 ( for i in 0..3 ( if (i == 1) ( free (bag(i)) ) ), 1 ), g.compile(), g()"
                .into(),
            "freed 1\n1\n",
        ),
        // And a `-> void` function.
        ("f := fn () -> void ( free (bag(5)) ), f(), print «after»".into(), "freed 5\nafter\n"),
        ("f := fn () -> void ( free (bag(5)) ), f.compile(), f(), print «after»".into(), "freed 5\nafter\n"),
        // A block's value is its own, which its binder takes; its other lines end theirs.
        ("x := ( bag(6) ), print «after»".into(), "after\nfreed 6\n"),
        ("c := true, x := if c ( bag(7) ) else ( bag(8) ), print «after»".into(), "after\nfreed 7\n"),
        ("x := ( free (bag(9)), i32 5 ), print «x {x}»".into(), "freed 9\nx 5\n"),
        // A `defer` is a line of its own when its scope ends.
        (
            "f := fn () -> i32 ( defer free (bag(1)), 2 ), free (f()), print «after»".into(),
            "freed 1\nafter\n",
        ),
        (
            "f := fn () -> i32 ( defer free (bag(1)), 2 ), f.compile(), free (f()), print «after»"
                .into(),
            "freed 1\nafter\n",
        ),
        // A `.` reads the node and runs nothing, so nothing is made to free.
        ("(bag(1)).type == bag".into(), "true\n"),
        ("f := fn () -> bool ( (bag(1)).type == bag ), f.compile(), f()".into(), "true\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
    let (echoes, stderr) = repl(
        format!("{RECORD}\nx := ( free (bag(9)), i32 5 )\nprint «after»\nc := true\nif c ( free (bag(4)) )\n")
            .as_bytes(),
    );
    assert_eq!(echoes, ["freed 9", "after", "freed 4"], "stderr: {stderr}");
}

/// DESIGN ›A value's teardown runs where its life ends; the ending identity reads the type's
/// `free` slot‹ [[R57]], with values whose fields are read: a line frees its values when it
/// ends, a `return` or a fault leaving it included.
#[test]
fn a_line_frees_what_it_made_where_it_ends() {
    let sized = BOX.replace("print «free»", "print «free {size}»");
    let mk = "mk := fn () -> own boxed ( box (1, 2) )";
    let two_three = "two := fn () -> own boxed ( box (1, 2) ), \
                     three := fn () -> own boxed ( box (1, 2, 3) )";
    let r = "r := fn (k := i32 ?) -> u64 ( if k > 0 ( return mk().size ), 0 )";
    let array = "import file «./identities/array.logos», a := array u64 [1, 2]";
    let w = "mut i := u64 0, while (mk().size > i) ( i = i + 1 )";
    let bare_w = "mut i := u64 0, while mk().size > i ( i = i + 1 )";
    for (tail, printed) in [
        // The last made first: R57's Open line, its choice (a), until Question 67 is answered.
        (
            format!("{two_three}, x := two().size + three().size, print «x {{x}}»"),
            "free 3\nfree 2\nx 5\n",
        ),
        (
            format!(
                "{two_three}, g := fn () -> u64 ( two().size + three().size ), g.compile(), \
                 print «x {{g()}}»"
            ),
            "free 3\nfree 2\nx 5\n",
        ),
        // After the line's own reads.
        (format!("{two_three}, print «{{two().size}} {{three().size}}»"), "2 3\nfree 3\nfree 2\n"),
        // A `return` leaves the line, so it frees what the line made.
        (format!("{mk}, {r}, print «got {{r(5)}}»"), "free 2\ngot 2\n"),
        (format!("{mk}, {r}, r.compile(), print «got {{r(5)}}»"), "free 2\ngot 2\n"),
        // A `while` condition is a scope of its own, bracketed or not, which ends each pass.
        (format!("{mk}, {w}, print «end {{i}}»"), "free 2\nfree 2\nfree 2\nend 2\n"),
        (
            format!("{mk}, g := fn () -> u64 ( {w}, i ), g.compile(), g()"),
            "free 2\nfree 2\nfree 2\n2\n",
        ),
        (format!("{mk}, {bare_w}, print «end {{i}}»"), "free 2\nfree 2\nfree 2\nend 2\n"),
        (
            format!("{mk}, g := fn () -> u64 ( {bare_w}, i ), g.compile(), g()"),
            "free 2\nfree 2\nfree 2\n2\n",
        ),
        // So is an `if` condition, which ends before an arm runs.
        (format!("{mk}, if mk().size > 1 ( print «in» ), print «end»"), "free 2\nin\nend\n"),
        (format!("{mk}, if (mk().size > 1) ( print «in» ), print «end»"), "free 2\nin\nend\n"),
        (
            format!(
                "{mk}, g := fn () -> u64 ( if mk().size > 1 ( print «in» ), 7 ), g.compile(), g()"
            ),
            "free 2\nin\n7\n",
        ),
        // A one-line bracket stands as its line, so a `.` still reads the line's node.
        (format!("{mk}, (mk().size + 1).output_type == u64"), "true\n"),
        (
            format!(
                "{mk}, f := fn () -> bool ( (mk().size + 1).output_type == u64 ), f.compile(), f()"
            ),
            "true\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{sized}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
    // A fault leaves the line: what it made ends before the error shows.
    for tail in [
        format!("{mk}, {array}, x := mk().size + a[5], print «after»"),
        format!("{mk}, {array}, g := fn () -> u64 ( mk().size + a[5] ), g.compile(), g()"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{sized}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(1), "free 2\n"), "{tail}: stderr: {stderr}");
        assert!(stderr.contains("index out of range"), "{tail}: stderr: {stderr}");
    }
}

/// ›The pass runs only as far as it must, in order, and never twice‹ [[R288]] and ›Holding is
/// decided at the binding site, parameters included‹ [[R61]]: a part of a line the pass runs
/// before a point that needs a value, a `.` read among them, leaves what it made beside the
/// value it gave to the line's end, which frees it last made first.
#[test]
fn what_a_part_the_pass_ran_early_made_ends_where_its_line_ends() {
    let sized = BOX.replace("print «free»", "print «free {size}»");
    let defs = "two := fn () -> own boxed ( print «two», box (1, 2) ), \
                three := fn () -> own boxed ( box (1, 2, 3) ), mut tt := type ?, tt = u64";
    let k = "p := fn () -> u64 ( print «p», 0 ), k := fn (a := u64 ?, b := u64 ?) -> u64 ( a + b )";
    let q = "q := type ( v := u64 ?, static parse_rank = 61, static parse = ( print «q», \
             tape[0].type = q, tape[0].v = tape[1].dyads[0], tape.remove(1), \
             tape.is_constructed[0] = true ) )";
    for (tail, printed) in [
        (format!("{defs}, x := two().size + tt 5, print «x {{x}}»"), "two\nfree 2\nx 7\n"),
        (
            format!("{defs}, x := (two().size + three().size) + tt 5, print «x {{x}}»"),
            "two\nfree 3\nfree 2\nx 10\n",
        ),
        // An argument the pass ran early is a part of the line the call stands in.
        (
            format!("{defs}, {k}, x := k(two().size, tt 5) + p(), print «x {{x}}»"),
            "two\np\nfree 2\nx 7\n",
        ),
        (
            format!("{defs}, {k}, x := k(two().size, three().size) + p(), print «x {{x}}»"),
            "two\np\nfree 3\nfree 2\nx 5\n",
        ),
        // A constructor written in Logos runs after it.
        (
            format!("{defs}, {q}, x := two().size + (q [5, u64 7]).v, print «x {{x}}»"),
            "two\nq\nfree 2\nx 7\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{sized}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
    // A fault leaves the line, in the early run or after it: what the run made ends first.
    let array = "import file «./identities/array.logos», a := array u64 [1, 2]";
    for tail in [
        format!("{defs}, {array}, x := (two().size + a[5]) + tt 5, print «after»"),
        format!("{defs}, {array}, x := two().size + tt 5 + a[5], print «after»"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{sized}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(1), "two\nfree 2\n"), "{tail}: stderr: {stderr}");
        assert!(stderr.contains("index out of range"), "{tail}: stderr: {stderr}");
    }
}

/// DESIGN ›Pointer types are prefix `@T`; dereference is postfix `x@`; `&x` is address-of‹
/// [[R162]].
#[test]
fn the_address_of_a_value_nothing_names_holds_it_to_the_scope_s_end() {
    let g = "g := fn () -> i32 ( p := &bag(4), p@.n )";
    for (tail, printed) in [
        ("p := &bag(1), print «after», p@.n", "after\nfreed 1\n1\n"),
        (&format!("{g}, print «before», free (g()), print «after»"), "before\nfreed 4\nafter\n"),
        (
            &format!("{g}, g.compile(), print «before», free (g()), print «after»"),
            "before\nfreed 4\nafter\n",
        ),
        (
            "for i in 0..2 ( p := &bag(i), print «in {p@.n}» ), print «end»",
            "in 0\nfreed 0\nin 1\nfreed 1\nend\n",
        ),
        ("x := if true ( p := &bag(2), p@.n ) else ( 3 ), print «x {x}»", "freed 2\nx 2\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
    // The scope's end frees it, so its address may not leave the scope, as a named one may not.
    for tail in ["f := fn () -> @bag ( &bag(4) ), 1", "f := fn () -> @bag ( b := bag(4), &b ), 1"] {
        let (code, _, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains("hands out the address of a name its scope holds"), "{tail}");
    }
}

/// DESIGN ›Holding is decided at the binding site, parameters included‹ [[R61]]: a call's
/// argument list is never the scope `&` holds a value to.
#[test]
fn the_address_of_a_value_in_a_call_s_arguments_holds_it_to_the_scope_the_call_stands_in() {
    let f = "f := fn (q := @bag ?) -> i32 ( q@.n )";
    let f2 = "f := fn (q := @bag ?, k := i32 ?) -> i32 ( q@.n + k )";
    let g = "g := fn () -> i32 ( f(&bag(4)) )";
    let sum = "g := fn () -> i32 ( mut s := i32 0, for i in 0..2 ( s = s + f(&bag(i)) ), s )";
    let get = "holder := type ( k := i32 ?, static get := fn (q := @bag ?) -> i32 ( k + q@.n ) )";
    let r = "r := fn (n := i32 ?) -> i32 ( return f(&bag(n)) )";
    let r_if = "r := fn (n := i32 ?) -> i32 ( if f(&bag(n)) > 2 (return f(&bag(n + 10))), 0 )";
    let r_in = "r := fn (n := i32 ?) -> i32 ( f(&bag(n), ( if (n > 2) (return 9), 4 )) )";
    let w = "mut i := i32 0, while (f(&bag(i)) < 2) ( i = i + 1 )";
    let bare_w = "mut i := i32 0, while f(&bag(i)) < 2 ( i = i + 1 )";
    let bare_if = "g := fn () -> i32 ( if f(&bag(1)) < 2 ( print «in» ), print «end», 0 )";
    for (tail, printed) in [
        (format!("{f}, f(&bag(3))"), "freed 3\n3\n"),
        (format!("{f}, f.compile(), f(&bag(3))"), "freed 3\n3\n"),
        (format!("{f}, x := f(&bag(3)), print «after {{x}}»"), "after 3\nfreed 3\n"),
        (
            format!("{f}, {g}, print «before», free (g()), print «after»"),
            "before\nfreed 4\nafter\n",
        ),
        (
            format!(
                "{f}, f.compile(), {g}, g.compile(), print «before», free (g()), print «after»"
            ),
            "before\nfreed 4\nafter\n",
        ),
        (
            format!("{f}, for i in 0..2 ( x := f(&bag(i)), print «got {{x}}» ), print «end»"),
            "got 0\nfreed 0\ngot 1\nfreed 1\nend\n",
        ),
        (format!("{f}, {sum}, g.compile(), g()"), "freed 0\nfreed 1\n1\n"),
        (format!("{f}, h := fn (k := i32 ?) -> i32 ( k + 1 ), h(f(&bag(3)))"), "freed 3\n4\n"),
        (format!("{f}, x := f(&bag(3)) + f(&bag(4)), print «x {{x}}»"), "x 7\nfreed 4\nfreed 3\n"),
        // A list of several arguments hands it on too.
        (format!("{f2}, f(&bag(3), 5)"), "freed 3\n8\n"),
        (format!("{f2}, f.compile(), f(&bag(3), 5)"), "freed 3\n8\n"),
        (format!("{get}, h := holder(1), h.get(&bag(6))"), "freed 6\n7\n"),
        (
            format!(
                "{get}, g := fn () -> i32 ( h := holder(1), h.get(&bag(6)) ), g.compile(), g()"
            ),
            "freed 6\n7\n",
        ),
        // A `while` condition, bracketed or not, is the scope its call stands in, ended each pass.
        (format!("{f}, {w}, print «end {{i}}»"), "freed 0\nfreed 1\nfreed 2\nend 2\n"),
        (
            format!("{f}, g := fn () -> i32 ( {w}, i ), g.compile(), g()"),
            "freed 0\nfreed 1\nfreed 2\n2\n",
        ),
        (format!("{f}, {bare_w}, print «end {{i}}»"), "freed 0\nfreed 1\nfreed 2\nend 2\n"),
        (
            format!("{f}, g := fn () -> i32 ( {bare_w}, i ), g.compile(), g()"),
            "freed 0\nfreed 1\nfreed 2\n2\n",
        ),
        // So is an `if` condition, ended before an arm runs.
        (format!("{f}, if f(&bag(1)) < 2 ( print «in» ), print «end»"), "freed 1\nin\nend\n"),
        (format!("{f}, if (f(&bag(1)) < 2) ( print «in» ), print «end»"), "freed 1\nin\nend\n"),
        (format!("{f}, {bare_if}, g()"), "freed 1\nin\nend\n0\n"),
        (format!("{f}, {bare_if}, g.compile(), g()"), "freed 1\nin\nend\n0\n"),
        (
            format!("{f}, x := if f(&bag(1)) < 2 ( 10 ) else ( 20 ), print «x {{x}}»"),
            "freed 1\nx 10\n",
        ),
        (
            format!("{f}, c := 1, if c == 2 ( print «a» ) else if f(&bag(3)) < 5 ( print «b» )"),
            "freed 3\nb\n",
        ),
        // A `return` leaves before its line ends, so it frees what the line holds itself.
        (format!("{f}, {r}, print «got {{r(5)}}», print «end»"), "freed 5\ngot 5\nend\n"),
        (format!("{f}, {r_if}, print «got {{r(5)}}»"), "freed 5\nfreed 15\ngot 15\n"),
        (format!("{f}, {r_if}, r.compile(), print «got {{r(5)}}»"), "freed 5\nfreed 15\ngot 15\n"),
        (format!("{f}, {r_if}, print «got {{r(1)}}»"), "freed 1\ngot 0\n"),
        (
            format!("{f2}, {r_in}, print «got {{r(5)}}», print «got {{r(1)}}»"),
            "freed 5\ngot 9\nfreed 1\ngot 5\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
    let (echoes, stderr) =
        repl(format!("{RECORD}\n{f}\nx := f(&bag(3))\n") + "print «{x}»\nprint «after»\n");
    assert_eq!(echoes, ["3", "after", "freed 3"], "stderr: {stderr}");

    // A bracket no call takes is the scope that holds it, so its address may not leave.
    for tail in [
        format!("{f}, f(( b := bag(3), &b ))"),
        format!("{f}, f(( &bag(3) ))"),
        "x := ( &bag(3) ), 1".to_string(),
        "f := fn (q := @bag ?, r := @bag ?) -> i32 ( q@.n ), f(b := bag(3), &b)".to_string(),
    ] {
        let (code, _, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains("hands out the address of a name its scope holds"), "{tail}");
    }
}

/// DESIGN ›`move` is the act, `own` the gate word, `free` the end‹: "`free p@` does the same for
/// a cell an owning pointer reaches, a record kept in the cell's bytes included", on both tiers.
#[test]
fn free_of_a_cell_runs_the_free_of_the_record_it_holds() {
    let mk =
        "mk := fn () -> own @bag ( alloc 1 of bag (5) ), g := fn () -> i32 ( free (mk())@, 1 )";
    for (tail, printed) in [
        ("p := alloc 1 of bag (5), free p@, 0".to_string(), "freed 5\n0\n"),
        (
            "p := alloc 2 of bag (5), (p + 1)@.n = 6, free (p + 1)@, free (p + 2 - 2)@, 0"
                .to_string(),
            "freed 6\nfreed 5\n0\n",
        ),
        (
            "g := fn () -> i32 ( p := alloc 1 of bag (5), free p@, 1 ), g()".to_string(),
            "freed 5\n1\n",
        ),
        (format!("{mk}, g()"), "freed 5\n1\n"),
        (format!("{mk}, g.compile(), g()"), "freed 5\n1\n"),
        // The block's own `free` runs nothing for the records its cells hold.
        ("p := alloc 1 of bag (5), free p, 0".to_string(), "0\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
}

/// DESIGN ›`move` is the act, `own` the gate word, `free` the end‹: "Something that only borrows
/// a value cannot end it: through a pointer `&` made (`pp := &a, free pp@`) … `free` and `move`
/// are refused where they are written". A cell with nothing to end keeps the inert `free`.
#[test]
fn a_cell_reached_through_a_borrow_is_not_ended_there() {
    let borrowed = "this cell is reached through something that only borrows it";
    for tail in [
        "a := bag(1), pp := &a, free pp@, 0",
        "a := bag(1), pp := &a, b := move pp@, 0",
        "a := alloc 1 of i32 5, pp := &a, free pp@, 0",
        "a := alloc 1 of i32 5, pp := &a, b := move pp@, 0",
        "f := fn (p := @bag ?) -> i32 ( free p@, 1 )",
        "id := fn (q := @bag ?) -> @bag ( q ), p := alloc 1 of bag (5), free (id(p))@, 0",
        "import file «./identities/array.logos», t := array i32, tt := array t, \
         g := fn (h := tt ?) -> i32 ( free (h.ptr + 0)@, 1 )",
    ] {
        let (code, _, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(borrowed), "{tail}: {stderr}");
    }
    // A copy of an owning pointer owns its own block, so its cells are its own to end.
    for (tail, printed) in [
        ("p := alloc 1 of bag (5), q := p, free q@, 0", "freed 5\n0\n"),
        ("p := alloc 2 of bag (5), q := p, free (q + 1)@, 0", "freed 5\n0\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
    for (tail, printed) in [
        ("a := i32 5, pp := &a, free pp@, print «after»", "after\n"),
        ("f := fn (p := @i32 ?) -> i32 ( free p@, 1 ), a := i32 5, f(&a)", "1\n"),
        ("f := fn (p := @i32 ?) -> i32 ( free p@, 1 ), f.compile(), a := i32 5, f(&a)", "1\n"),
    ] {
        let (code, stdout, stderr) = run_line(tail);
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
}

/// `h0` makes with `-> RESULT`, and each `h1` to `hN` hands on the one before from both arms.
fn own_chain(result: &str, depth: usize) -> String {
    let mut src = format!("c := i32 1, h0 := fn () -> {result} ( bag(1) )");
    for i in 1..=depth {
        let before = i - 1;
        src.push_str(&format!(
            ", h{i} := fn () -> {result} ( if (c == 1) (h{before}()) else (h{before}()) )"
        ));
    }
    src
}

#[test]
fn a_call_through_own_hands_on_its_made_value_at_any_depth() {
    for depth in [1, 10, 40] {
        let tail = format!("{}, x := h{depth}(), print «after»", own_chain("own bag", depth));
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), "after\nfreed 1\n"), "stderr: {stderr}");
        let borrow = format!(
            "{}, y := bag(4), x := if (c == 1) (h{depth}()) else (y)",
            own_chain("own bag", depth)
        );
        let (code, _, stderr) = run_line(&format!("{RECORD}, {borrow}"));
        assert_eq!(code, Some(1), "depth {depth}: stderr: {stderr}");
        assert!(stderr.contains("make a value in every arm"), "depth {depth}: {stderr}");
    }
    let (code, _, stderr) = run_line(&format!("{RECORD}, {}", own_chain("bag", 1)));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("write `-> own bag`"), "{stderr}");
    let compiled = "mk := fn () -> own bag ( bag(3) ), h := fn () -> i32 ( b := mk(), b.n ), \
                    h.compile(), h()";
    let (code, stdout, stderr) = run_line(&format!("{RECORD}, {compiled}"));
    assert_eq!((code, stdout.as_str()), (Some(0), "freed 3\n3\n"), "stderr: {stderr}");
}

#[test]
fn a_return_hands_on_its_value_as_the_last_line_does() {
    let g = "g := bag(9), mk := fn (c := i32 ?) ->";
    for (tail, refused) in [
        (
            format!("{g} own bag ( if (c == 1) (return g), bag(4) ), x := mk(1)"),
            "hands on a borrow",
        ),
        (format!("{g} bag ( if (c == 1) (return bag(3)), g ), x := mk(1)"), "write `-> own bag`"),
    ] {
        let (code, _, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(refused), "{tail}: {stderr}");
    }
    let made = "mk := fn (c := i32 ?) -> own bag ( if (c == 1) (return bag(3)), bag(4) ), \
                x := mk(1), print «got»";
    let (code, stdout, stderr) = run_line(&format!("{RECORD}, {made}"));
    assert_eq!((code, stdout.as_str()), (Some(0), "got\nfreed 3\n"), "stderr: {stderr}");
    // A name the function holds moves out, as a last line's does, and the `return` frees the
    // function's other names as it leaves.
    for (src, printed) in [
        (
            format!(
                "{RECORD}, mk := fn () -> own bag ( a := bag(1), b := bag(2), return a ), \
                 x := mk(), print «got {{x.n}}»"
            ),
            "freed 2\ngot 1\nfreed 1\n",
        ),
        (
            format!("{BOX}, mk := fn () -> own box ( b := box (1, 2), return b ), w := mk(), print «after»"),
            "after\nfree\n",
        ),
        (
            format!(
                "{BOX}, mk := fn () -> own box ( b := box (1, 2), return b ), mk.compile(), \
                 w := mk(), print «after»"
            ),
            "after\nfree\n",
        ),
        (
            format!(
                "{BOX}, mk := fn (c := i32 ?) -> own box ( b := box (1, 2), \
                 if (c == 1) (return b) else (print «no»), box (3, 4, 5) ), \
                 w := mk(1), print «{{w.size}}», v := mk(2), print «{{v.size}}»"
            ),
            "2\nno\nfree\n3\nfree\nfree\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{src}");
    }
    for (src, refused) in [
        (
            format!("{RECORD}, mk := fn () -> bag ( a := bag(1), return a ), x := mk()"),
            "write `-> own bag`",
        ),
        (
            format!(
                "{BOX}, mk := fn () -> own box ( b := box (1, 2), \
                 for i in 0..3 ( if (i == 1) (return b) ), b ), w := mk()"
            ),
            "declared outside this loop",
        ),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(refused), "{src}: {stderr}");
    }
    // A type's `run` whose last value is made is marked `own`, and its `return` is held to it.
    let run = "g := box (1, 2), pk := type ( elements := scope ?, output_type := type ?, \
               static run = ( if (elements.dyads.size == 3) (return g), mut v := output_type ?, \
               v.p = mut alloc 1 of i32 ?, v.size = 1, move v ), \
               static parse_rank = dyad.parse_rank, static associativity = left, \
               static parse = ( tape[0].type = pk, tape[0].elements = tape[1], \
               tape[0].output_type = boxed, tape.remove(1), tape.is_constructed[0] = true ) ), \
               x := pk (7, 8, 9), print «{x.size}»";
    let (code, _, stderr) = run_line(&format!("{BOX}, {run}"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("this `return` hands on a borrow"), "{stderr}");
}

#[test]
fn a_plain_record_s_owning_field_keeps_its_pointer_in_its_own_bytes() {
    let r = "r := type ( mut p := own @i32 ?, static free = ( free p ), \
             static set := fn () -> i32 ( a := alloc 1 of i32 5, p = move a, 1 ) ), mut x := r ?";
    for (tail, printed) in [
        ("free (x.set()), 2", "2"),
        ("a := alloc 1 of i32 5, x.p = move a, b := alloc 1 of i32 6, x.p = move b, x.p@", "6"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{r}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.trim(), printed, "{tail}");
    }
    // The record's own free frees the field; nothing takes it out alone.
    for tail in [
        "a := alloc 1 of i32 5, x.p = move a, free x.p, 2",
        "a := alloc 1 of i32 5, x.p = move a, b := move x.p, b@",
    ] {
        let (code, _, stderr) = run_line(&format!("{r}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains("the type of `x` fills its own `static free"), "{tail}: {stderr}");
    }
}

#[test]
fn an_if_frees_what_one_arm_moved_at_the_end_of_the_other() {
    for (tail, printed) in [
        (
            "f := fn (c := i32 ?) -> i32 ( a := box (1, 2), if (c == 1) ( b := move a, 1 ), 3 ), \
             f.compile(), free (f(0)), free (f(1)), print «after»",
            "free\nfree\nafter\n",
        ),
        (
            "f := fn (c := i32 ?) -> i32 ( a := box (1, 2), \
             if (c == 1) ( b := move a, 1 ) else ( return 2 ) ), free (f(0)), free (f(1)), print «after»",
            "free\nfree\nafter\n",
        ),
        (
            "c := i32 0, a := box (1, 2), if (c == 1) ( free a ) else ( print «kept» ), \
             print «after»",
            "kept\nfree\nafter\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{BOX}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{tail}");
    }
    // A REPL line ends a name an earlier line holds: the session's end frees it no more.
    let lines = BOX.replacen("), box := ", ")\nbox := ", 1);
    for (tail, last) in [
        ("a := box (1, 2)\nb := move a\nprint «end»\n", "free"),
        ("a := box (1, 2)\nfree a\nprint «end»\n", "end"),
        ("a := box (1, 2)\nc := i32 0\nif (c == 1) ( b := move a )\nprint «end»\n", "end"),
    ] {
        let (echoes, stderr) = repl(format!("{lines}\n{tail}").as_bytes());
        assert_eq!(
            echoes.iter().filter(|l| *l == "free").count(),
            1,
            "{tail}: {echoes:?} {stderr}"
        );
        assert_eq!(echoes.last().map(String::as_str), Some(last), "{tail}: {echoes:?}");
    }
}

#[test]
fn and_and_or_run_both_sides() {
    for (tail, printed) in [
        (
            "c := i32 0, a := box (1, 2), x := (c == 1) and ( free a, c == 0 ), print «after {x}»",
            "free\nafter false\n",
        ),
        (
            "c := i32 1, a := box (1, 2), x := (c == 1) or ( free a, c == 0 ), print «after {x}»",
            "free\nafter true\n",
        ),
        (
            "f := fn (c := i32 ?) -> i32 ( a := box (1, 2), x := (c == 1) and ( free a, c == 0 ), 7 ), \
             f.compile(), print «after {f(0)}»",
            "free\nafter 7\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{BOX}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{tail}");
    }
}

#[test]
fn a_return_frees_what_its_line_ends_after_it_compiled_or_not() {
    let ret = "(if (c == 1) (return 5), i32 2)";
    for (body, other) in [
        (format!("x := {ret} + (free a, i32 1)"), 3),
        (format!("x := ( free ({ret}), i32 7 ) + (free a, i32 1)"), 8),
        (format!("x := ( free ({ret} + (free a, i32 1)), i32 7 )"), 7),
    ] {
        for compile in ["", "f.compile(), "] {
            let (code, stdout, stderr) = run_line(&format!(
                "{BOX}, f := fn (c := i32 ?) -> i32 ( a := box (1, 2), {body}, x ), \
                 {compile}print «got {{f(1)}}», print «got {{f(0)}}»"
            ));
            assert_eq!(code, Some(0), "{compile}{body}: stderr: {stderr}");
            assert_eq!(stdout, format!("free\ngot 5\nfree\ngot {other}\n"), "{compile}{body}");
        }
    }
}

/// DESIGN ›A value's teardown runs where its life ends; the ending identity reads the type's
/// `free` slot‹ [[R57]] and ›A last value moves out‹ [[R66]]: "the teardowns of the scopes it
/// leaves run on the way out", and an `if`'s and a call's, in the same order on both tiers.
#[test]
fn a_return_ends_what_each_scope_if_and_call_it_leaves_holds_compiled_or_not() {
    let fns = "say := fn (k := i32 ?) -> void ( print «say {k}» ), \
               f := fn (q := bag ?, m := i32 ?) -> i32 ( q.n + m ), \
               p := fn (q := @bag ?, k := i32 ?) -> i32 ( q@.n + k )";
    for (g, calls, printed) in [
        (
            "g := fn () -> i32 ( b := bag(4), return b.n + 1 )",
            "print «got {g()}»",
            "freed 4\ngot 5\n",
        ),
        (
            "g := fn (k := i32 ?) -> i32 ( a := bag(1), for i in 0..3 ( b := bag(10 + i), \
             if (i == k) ( c := bag(20 + i), return a.n + b.n + c.n ), say(i) ), say(9), 0 )",
            "print «got {g(1)}», print «got {g(5)}»",
            "say 0\nfreed 10\nfreed 21\nfreed 11\nfreed 1\ngot 33\n\
             say 0\nfreed 10\nsay 1\nfreed 11\nsay 2\nfreed 12\nsay 9\nfreed 1\ngot 0\n",
        ),
        (
            "g := fn (c := i32 ?) -> i32 ( a := bag(1), mut k := i32 0, \
             while k < 3 ( b := bag(10 + k), if (k == c) (return k), k = k + 1 ), 9 )",
            "print «got {g(1)}», print «got {g(7)}»",
            "freed 10\nfreed 11\nfreed 1\ngot 1\nfreed 10\nfreed 11\nfreed 12\nfreed 1\ngot 9\n",
        ),
        (
            "g := fn (c := i32 ?) -> i32 ( a := bag(1), defer say(0), \
             ( b := bag(2), if (c == 1) (return 5) ), 9 )",
            "print «got {g(1)}», print «got {g(0)}»",
            "freed 2\nsay 0\nfreed 1\ngot 5\nfreed 2\nsay 0\nfreed 1\ngot 9\n",
        ),
        (
            "g := fn (c := i32 ?) -> i32 ( a := bag(1), x := ( b := bag(2), \
             if (c == 1) (return 5), i32 7 ), d := bag(3), x )",
            "print «got {g(1)}», print «got {g(0)}»",
            "freed 2\nfreed 1\ngot 5\nfreed 2\nfreed 3\nfreed 1\ngot 7\n",
        ),
        // The arm the `return` leaves frees what the other arm freed.
        (
            "g := fn (c := i32 ?) -> i32 ( a := bag(7), if (c > 0) ( b := bag(8), \
             if (c == 1) (return 5) else (free a), say(1) ) else ( say(2) ), 9 )",
            "print «got {g(1)}», print «got {g(2)}», print «got {g(0)}»",
            "freed 7\nfreed 8\ngot 5\nfreed 7\nsay 1\nfreed 8\ngot 9\nsay 2\nfreed 7\ngot 9\n",
        ),
        // A condition the `return` leaves frees what either arm would have.
        (
            "g := fn (c := i32 ?) -> i32 ( a := bag(7), \
             x := if ((if (c == 1) (return 5), i32 2) == 2) (free a, i32 1) else (i32 3), x )",
            "print «got {g(1)}», print «got {g(0)}»",
            "freed 7\ngot 5\nfreed 7\ngot 1\n",
        ),
        // A call frees the arguments it holds before the one the `return` leaves.
        (
            "g := fn (c := i32 ?) -> i32 ( f(bag(1), (if (c == 1) (return 7), i32 2)) )",
            "print «got {g(1)}», print «got {g(0)}»",
            "freed 1\ngot 7\nfreed 1\ngot 3\n",
        ),
        (
            "g := fn (c := i32 ?) -> i32 ( a := bag(1), \
             x := f(bag(2), f(bag(3), (if (c == 1) (return 5), i32 4))), x )",
            "print «got {g(1)}», print «got {g(0)}»",
            "freed 3\nfreed 2\nfreed 1\ngot 5\nfreed 3\nfreed 2\nfreed 1\ngot 9\n",
        ),
        // What `&` holds, named or not, in the `return`'s line or in a call's arguments.
        (
            "g := fn () -> i32 ( b := bag(4), return p(&b, 1) )",
            "print «got {g()}»",
            "freed 4\ngot 5\n",
        ),
        ("g := fn () -> i32 ( return p(&bag(4), 1) )", "print «got {g()}»", "freed 4\ngot 5\n"),
        (
            "g := fn (n := i32 ?) -> i32 ( a := bag(1), p(&bag(n), ( if (n > 2) (return 9), 4 )) )",
            "print «got {g(5)}», print «got {g(1)}»",
            "freed 5\nfreed 1\ngot 9\nfreed 1\nfreed 1\ngot 5\n",
        ),
    ] {
        for compile in ["", "g.compile(), "] {
            let src = format!("{RECORD}, {fns}, {g}, {compile}{calls}");
            let (code, stdout, stderr) = run_line(&src);
            assert_eq!((code, stdout.as_str()), (Some(0), printed), "{compile}{g}: {stderr}");
        }
    }
}

/// ›A scope's value is what it evaluates to, and `return` is an optional early exit from the
/// enclosing function‹ [[R207]]: a `defer` holds no `return`, at any depth, and gives no value;
/// each is refused where it is written, so nothing runs on either tier.
#[test]
fn a_defer_holds_no_return_and_gives_no_value() {
    let leaves = "a `defer` cannot leave its function";
    let gives = "this gives a value, and a `defer` gives none";
    for (g, word, error) in [
        ("g := fn () -> i32 ( defer ( return 7 ), return 3 )", "return 7", leaves),
        ("g := fn () -> i32 ( defer return 7, 3 )", "return 7", leaves),
        (
            "g := fn () -> i32 ( a := bag(1), defer ( return 7 ), b := bag(2), 3 )",
            "return 7",
            leaves,
        ),
        ("g := fn (c := i32 ?) -> i32 ( defer ( if (c == 1) (return 7) ), 3 )", "return 7", leaves),
        (
            "g := fn () -> i32 ( defer ( for i in 0..3 ( if (i == 1) (return 7) ) ), 3 )",
            "return 7",
            leaves,
        ),
        ("g := fn () -> i32 ( x := ( defer ( return 7 ), i32 5 ), x )", "return 7", leaves),
        ("g := fn () -> i32 ( defer ( defer ( return 7 ) ), 3 )", "return 7", leaves),
        // The line after the `return` is the `return`'s to answer for, not the `defer`'s.
        ("g := fn () -> i32 ( defer ( return 7, print «x» ), 3 )", "return 7", leaves),
        ("g := fn () -> i32 ( defer ( 7 ), 3 )", "( 7 )", gives),
        ("g := fn () -> i32 ( defer 7, 3 )", "7,", gives),
        ("g := fn () -> i32 ( defer bag(1), 3 )", "bag(1),", gives),
        ("g := fn (c := i32 ?) -> i32 ( defer ( if (c == 1) (7) ), 3 )", "( if", gives),
        ("g := fn () -> i32 ( defer ( for i in 0..3 ( if (i == 1) (i) ) ), 3 )", "( for", gives),
    ] {
        for compile in ["", "g.compile(), "] {
            let src = format!("{RECORD}, {g}, {compile}print «got {{g()}}»");
            let (code, stdout, stderr) = run_line(&src);
            assert_eq!((code, stdout.as_str()), (Some(1), ""), "{compile}{g}: {stderr}");
            let at = src.find(word).expect("the word is in the program");
            let col = src[..at].chars().count() + 1;
            let head = format!("<command line>:1:{col}: error: {error}");
            assert!(stderr.starts_with(&head), "{compile}{g}: stderr: {stderr}");
        }
    }
    // A function written in a `defer` leaves itself, and a value nobody wants is thrown away.
    for compile in ["", "g.compile(), "] {
        let src = format!(
            "{RECORD}, g := fn () -> i32 ( a := bag(1), defer ( h := fn () -> i32 ( return 5 ), \
             print «h {{h()}}» ), defer free (bag(2)), 3 ), {compile}print «got {{g()}}»"
        );
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(
            (code, stdout.as_str()),
            (Some(0), "freed 2\nh 5\nfreed 1\ngot 3\n"),
            "{compile}: {stderr}"
        );
    }
}

#[test]
fn a_line_that_fails_frees_a_node_it_ended_and_did_not_reach() {
    let g = "g := fn () -> i32 ( error «stop» )";
    for line in [
        "x := g() + (free a, i32 1)",
        "x := (free a, i32 1) + g()",
        "x := g() + (b := move a, i32 1)",
        "x := (b := move a, i32 1) + g()",
    ] {
        for tail in [
            format!("f := fn () -> i32 ( a := box (1, 2), {line}, x ), print «before», f()"),
            format!("print «before», a := box (1, 2), {line}, 1"),
        ] {
            let (code, stdout, stderr) = run_line(&format!("{BOX}, {g}, {tail}"));
            assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
            assert!(stderr.contains("run error: stop"), "{tail}: stderr: {stderr}");
            assert_eq!(stdout, "before\nfree\n", "{tail}");
        }
    }
    // A REPL line that stopped before its `free` leaves the name live, so the session's end
    // frees it; one that reached it leaves the name ended.
    let session =
        format!("{}\n{g}\na := box (1, 2)\n", BOX.replacen("), box := type", ")\nbox := type", 1));
    for (line, echoes) in [
        ("x := g() + (free a, i32 1)", ["after", "free"]),
        ("x := (free a, i32 1) + g()", ["free", "after"]),
    ] {
        let (echoed, stderr) = repl(format!("{session}{line}\nprint «after»\n").as_bytes());
        assert_eq!(echoed, echoes, "{line}: stderr: {stderr}");
        assert!(stderr.contains("run error: stop"), "{line}: stderr: {stderr}");
    }
}

#[test]
fn an_array_holds_arrays_as_their_addresses() {
    let array =
        "import file «./identities/array.logos», x := array i32 [1, 2], y := array i32 [3, 4], \
                 t := array i32, b := array t [move x, move y]";
    for (tail, printed) in [
        ("b[1][0]", "3"),
        ("b[0][1] + b[1][1]", "6"),
        ("b.size", "2"),
        ("b[1].size", "2"),
        ("b[0][1] = i32 9, b[0][1]", "9"),
        ("f := fn () -> i32 ( b[1][0] ), f.compile(), f()", "3"),
        ("f := fn (p := array t ?) -> i32 ( p[1][1] ), f(b)", "4"),
        ("f := fn (p := array t ?) -> i32 ( p[1][1] ), f.compile(), f(b)", "4"),
        ("u := array t, u == array t", "true"),
        // A copy is its name's own, so it moves in.
        ("z := array i32 [5], w := z, c := array t [move w], c[0][0]", "5"),
        // A named line puts a copy in: the outer array and the name each own their own.
        ("z := array i32 [5], c := array t [z], c[0][0] = i32 9, z[0]", "5"),
        ("z := array i32 [5], c := array t [z], c[0][0] = i32 9, c[0][0]", "9"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.trim(), printed, "{tail}");
    }
    // A name moved in is dead; a borrow cannot be moved.
    for (tail, expect) in [
        ("x[0]", "`x` is dead here"),
        ("f := fn (p := t ?) -> u64 ( c := array t [move p], c.size )", "only its owner can"),
    ] {
        let (code, _, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
}

/// A value whose field holds an array, built in the array's shape: `holder ()` is a node
/// whose run makes a new `held`, and `holder` with no bracket is `held`, whose parse
/// reads nothing.
const HOLDER: &str = "import file «./identities/array.logos», t := array i32, \
    held := type ( \
        mut items := t ?, \
        static count := fn () -> u64 ( items.size ), \
        static second := fn () -> i32 ( items[1] ), \
        static parse_rank = dyad.parse_rank, \
        static parse = ( tape.is_constructed[0] = true ) ), \
    holder := type ( \
        output_type := type ?, \
        static run = ( mut v := output_type ?, v.items = array i32 [4, 5, 6], v ), \
        static parse_rank = dyad.parse_rank, \
        static associativity = left, \
        static parse = ( \
            if tape[1].type == scope ( \
                tape[0].type = holder, tape[0].output_type = held, tape.remove(1) \
            ) else ( tape[0] = held ), \
            tape.is_constructed[0] = true \
        ) )";

#[test]
fn a_field_holding_an_array_reads_as_that_array() {
    for (tail, printed) in [
        ("h := holder (), h.count()", "3"),
        ("h := holder (), h.second()", "5"),
        ("h := holder (), h.items[1]", "5"),
        ("h := holder (), h.items.size", "3"),
        ("h := holder (), h.items.at(2)", "6"),
        ("h := holder (), h.items[0] = i32 9, h.items[0] + h.items[1]", "14"),
        // A name declared from the field holds a copy of the array, its own.
        ("h := holder (), c := h.items, c[0] = i32 7, h.items[0]", "4"),
        ("h := holder (), c := h.items, c[0] = i32 7, c[0]", "7"),
        ("f := fn (p := t ?) -> i32 ( p[2] ), h := holder (), f(h.items)", "6"),
        ("f := fn (p := t ?) -> i32 ( p[2] ), f.compile(), h := holder (), f(h.items)", "6"),
        ("f := fn (h := holder ?) -> i32 ( h.items[0] = i32 9, h.items[0] ), f(holder ())", "9"),
        ("f := fn () -> u64 ( h := holder (), h.items.size ), f()", "3"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{HOLDER}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.trim(), printed, "{tail}");
    }
    let (code, _, stderr) = run_line(&format!(
        "{HOLDER}, u := array u8, g := type ( mut items := t ?, \
         static put := fn () -> u64 ( items = array u8 [1], 0 ) )"
    ));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("these types do not match"), "stderr: {stderr}");
}

/// `BOX` and a `bag` whose owning fields hold an array and a box, its free printing «bag»,
/// in the array's shape: `bag ()` makes a new `bagged`.
fn bag_line(tail: &str) -> String {
    format!(
        "import file «./identities/array.logos», {BOX}, t := array i32, \
         bagged := type ( \
             mut items := own t ?, \
             mut b := own boxed ?, \
             static free = ( print «bag», free items, free b ), \
             static parse_rank = dyad.parse_rank, \
             static parse = ( tape.is_constructed[0] = true ) ), \
         bag := type ( \
             output_type := type ?, \
             static run = ( \
                 mut v := output_type ?, \
                 v.items = array i32 [4, 5, 6], v.b = box (7, 1), move v ), \
             static parse_rank = dyad.parse_rank, \
             static associativity = left, \
             static parse = ( \
                 if tape[1].type == scope ( \
                     tape[0].type = bag, tape[0].output_type = bagged, tape.remove(1) \
                 ) else ( tape[0] = bagged ), \
                 tape.is_constructed[0] = true \
             ) ), {tail}"
    )
}

#[test]
fn a_field_under_a_type_that_fills_free_is_refused_where_it_is_written() {
    let refused =
        |holder: &str| format!("the type of `{holder}` fills its own `static free = (…)`");
    for (tail, holder) in [
        ("g := bag (), y := move g.b, 1", "g"),
        ("g := bag (), free g.b, 1", "g"),
        ("g := bag (), free g.items, 1", "g"),
        ("g := bag (), y := move g.b.p, 1", "g"),
        ("g := bag (), free g.b.size, 1", "g"),
        ("c := i32 1, g := bag (), if (c == 1) ( y := move g.b, 1 ), 1", "g"),
        ("f := fn () -> i32 ( g := bag (), y := move g.b, 1 ), f()", "g"),
        ("f := fn () -> i32 ( g := bag (), free g.b, 1 ), f.compile(), f()", "g"),
        ("b := box (5, 6), y := move b.p, y@", "b"),
        ("g := fn () -> i32 ( b := box (5, 6), y := move b.p, y@ ), g.compile(), g()", "b"),
        // A copy is its name's own, refused as its type fills `free`.
        ("g := bag (), h := g, y := move h.b, 1", "h"),
    ] {
        let (code, stdout, stderr) = run_line(&bag_line(tail));
        assert_eq!(code, Some(1), "{tail}: stdout: {stdout}");
        assert!(stderr.contains(&refused(holder)), "{tail}: stderr: {stderr}");
    }
    // The refused line ends nothing: `g` is freed whole, once.
    let (code, stdout, _) = run_line(&bag_line("g := bag (), y := move g.b, 1"));
    assert_eq!((code, stdout.as_str()), (Some(1), "bag\nfree\n"));
    // Where nothing can end the field, that is said first.
    let unfollowed = "nothing here can end it";
    for (tail, expect) in [
        ("g := bag (), for i in 0..2 ( y := move g.b )", "declared outside this loop"),
        ("l := array bagged [bag (), bag ()], y := move l[0].b, 1", unfollowed),
        ("l := array bagged [bag (), bag ()], free l[0].b, 1", unfollowed),
        ("f := fn (q := bagged ?) -> i32 ( y := move q.b, 1 ), 1", unfollowed),
        (
            "r := type ( mut p := own @i32 ?, static free = ( free p ), \
             static s := fn () -> i32 ( free p, 2 ) )",
            unfollowed,
        ),
        (
            "r := type ( mut p := own @i32 ?, static free = ( free p ), \
             static s := fn () -> i32 ( a := move p, 2 ) )",
            unfollowed,
        ),
        ("p := type ( x := i32 ?, static s := fn () -> i32 ( free x, 1 ) )", unfollowed),
    ] {
        let (code, _, stderr) = run_line(&bag_line(tail));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
    // An earlier REPL line's owner: the session's end frees it whole, once.
    let (echoes, stderr) =
        repl(format!("{}\ng := bag ()\ny := move g.b\nprint «end»\n", bag_line("print «ready»")));
    assert!(stderr.contains(&refused("g")), "{stderr}");
    assert_eq!(echoes[echoes.len() - 3..], ["end", "bag", "free"], "{echoes:?}");
}

/// `g`, an `outer` whose field `b` borrows an `inner`; `{inner}` and `{outer}` stand in each
/// type's body, so either can fill `free`.
fn nested_line(inner: &str, outer: &str, tail: &str) -> String {
    format!(
        "inner := type ( mut n := i32 0, {inner}static parse_rank = dyad.parse_rank, \
             static parse = ( tape.is_constructed[0] = true ) ), \
         outer := type ( mut b := inner ?, mut m := i32 0, {outer}\
             static parse_rank = dyad.parse_rank, \
             static parse = ( tape.is_constructed[0] = true ) ), \
         mut g := outer ?, mut i := inner ?, g.b = i, {tail}"
    )
}

#[test]
fn a_freed_field_ends_its_path_and_its_holder_as_a_whole() {
    // No type on the path fills `free`: the path ends at its `free`.
    for (tail, printed) in [("free g.b.n, g.m", "0\n"), ("free g.b, g.m", "0\n")] {
        let (code, stdout, stderr) = run_line(&nested_line("", "", tail));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{tail}");
    }
    for (tail, expect) in [
        ("free g.b.n, g.b.n", "`g.b.n` is dead here"),
        ("free g.b.n, g.b", "`g.b` is dead here"),
        ("free g.b.n, free g.b", "`g.b` is dead here"),
        ("free g.b.n, g", "`g` is dead here"),
        ("free g.b, free g.b", "`g.b` is dead here"),
        ("y := move g.b.n, 1", "owns nothing"),
    ] {
        let (code, _, stderr) = run_line(&nested_line("", "", tail));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
    // A type on the path that fills `free` refuses it; the first such holder is named.
    let free = |word: &str| format!("static free = ( print «{word}» ), ");
    for (inner, outer, holder) in [
        (free("inner"), String::new(), "g.b"),
        (String::new(), free("outer"), "g"),
        (free("inner"), free("outer"), "g"),
    ] {
        let (code, _, stderr) = run_line(&nested_line(&inner, &outer, "free g.b.n, g.m"));
        assert_eq!(code, Some(1), "{holder}: stderr: {stderr}");
        let refused = format!("the type of `{holder}` fills its own `static free = (…)`");
        assert!(stderr.contains(&refused), "{holder}: stderr: {stderr}");
    }
}

#[test]
fn an_owning_field_is_freed_once_by_the_owner_s_free() {
    for (tail, printed) in [
        ("g := bag (), print «made»", "made\nbag\nfree\n"),
        ("g := bag (), free g, print «after»", "bag\nfree\nafter\n"),
        (
            "f := fn () -> i32 ( g := bag (), g.items[2] ), free (f()), print «after»",
            "bag\nfree\nafter\n",
        ),
        ("g := bag (), h := g, print «copied»", "copied\nbag\nfree\nbag\nfree\n"),
        // `=` frees the box it displaces before the write.
        ("g := bag (), x := box (8, 1), g.b = move x, print «set»", "free\nset\nbag\nfree\n"),
        ("l := array bagged [bag (), bag ()], print «made»", "made\nbag\nfree\nbag\nfree\n"),
        ("g := bag (), l := array bagged [move g], print «made»", "made\nbag\nfree\n"),
        ("g := bag (), l := array bagged [g], print «made»", "made\nbag\nfree\nbag\nfree\n"),
        ("mut a := own box ?, a = box (3, 1), print «set»", "set\nfree\n"),
    ] {
        let (code, stdout, stderr) = run_line(&bag_line(tail));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert!(stdout.starts_with(printed), "{tail}: stdout: {stdout}");
        assert_eq!(stdout.matches("free").count(), printed.matches("free").count(), "{tail}");
        assert_eq!(stdout.matches("bag").count(), printed.matches("bag").count(), "{tail}");
    }
    for (tail, printed) in [
        ("g := bag (), g.items[1] + g.items.at(2)", "11"),
        ("g := bag (), g.items[0] = i32 9, g.items[0]", "9"),
        ("f := fn (p := t ?) -> u64 ( p.size ), g := bag (), f(g.items)", "3"),
        ("g := bag (), x := array i32 [1, 2], g.items = move x, g.items[1]", "2"),
        ("g := bag (), x := array i32 [1], y := x, g.items = move y, g.items[0]", "1"),
        ("g := bag (), x := array i32 [1], g.items = x, g.items[0] = i32 9, x[0]", "1"),
        ("l := array bagged [bag (), bag ()], l[1].items[0] = i32 8, l[1].items[0]", "8"),
    ] {
        let (code, stdout, stderr) = run_line(&bag_line(tail));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.lines().last(), Some(printed), "{tail}: {stdout}");
    }
    for (tail, expect) in [
        ("f := fn (p := t ?) -> i32 ( g := bag (), g.items = move p, 1 )", "only its owner can"),
        ("nd := type ( mut items := own t ? )", "must be freed by the type's `static free = (…)`"),
        ("g := bag (), x := array i32 [1], g.items = own x", "`move x` moves a value"),
        ("mk := fn () -> t ( array i32 [1] ), 1", "write `-> own t`"),
        ("nd := type ( mut n := own i32 ? )", "or a type whose body fills `free`"),
        ("f := fn (p := own t ?) -> i32 ( 1 )", "an `own` parameter"),
    ] {
        let (code, _, stderr) = run_line(&bag_line(tail));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn a_chooser_takes_the_type_the_chooser_right_of_it_leaves() {
    let array = "import file «./identities/array.logos»";
    for (tail, printed) in [
        ("t := array array i32, u := array (array i32), t == u", "true"),
        ("t := array array i32, u := array i32, v := array u, t == v", "true"),
        ("t := array array i32, u := array i32, t == u", "false"),
        (
            "x := array i32 [1, 2], y := array i32 [3, 4], b := array array i32 [move x, move y], \
             b[1][0]",
            "3",
        ),
        ("x := array i32 [1, 2], b := array (array i32) [move x], b[0][1]", "2"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.trim(), printed, "{tail}");
    }
}

#[test]
fn a_nested_list_builds_each_element_with_the_element_type() {
    let array = "import file «./identities/array.logos»";
    for (tail, printed) in [
        ("a := array array i32 [[1, 2], [3, 4]], a[1][0]", "3"),
        ("a := array array i32 [[1, 2], [3, 4]], a[0][1] + a[1][1]", "6"),
        ("a := array array i32 [[1, 2], [3, 4]], a.size", "2"),
        ("a := array array i32 [[1, 2], [3, 4, 5]], a[1].size", "3"),
        ("a := array array i32 [[1, 2], [3, 4]], a[0][1] = i32 9, a[0][1] + a[1][0]", "12"),
        ("a := array (array i32) [[1, 2], [3, 4]], a[1][1]", "4"),
        ("a := array array array i32 [[[1]], [[2, 3]]], a[1][0][1]", "3"),
        ("x := i32 5, a := array array i32 [[x, 2], [3, x]], a[1][1]", "5"),
        ("x := array i32 [7], a := array array i32 [[1], move x], a[1][0]", "7"),
        (
            "mk := fn () -> own array array i32 ( array array i32 [[1, 2], [3, 4]] ), \
             m := mk(), m[1][0]",
            "3",
        ),
        (
            "mk := fn (v := i32 ?) -> own array array i32 ( array array i32 [[v, 2], [3, v + 1]] ), \
             m := mk(4), n := mk(6), m[1][1] + n[1][1]",
            "12",
        ),
        (
            "mk := fn (v := i32 ?) -> own array array i32 ( array array i32 [[v, 2], [3, v + 1]] ), \
             mk.compile(), m := mk(4), m[1][1]",
            "5",
        ),
        (
            "f := fn (v := i32 ?) -> i32 ( a := array array i32 [[v, 2], [3, v + 1]], a[1][1] ), \
             f.compile(), f(4)",
            "5",
        ),
        (
            "mut s := i32 0, for i in 0..3 ( a := array array i32 [[i], [i * 2]], \
             s = s + a[0][0] + a[1][0] ), s",
            "9",
        ),
        ("mk := fn () -> own array array i32 ( array array i32 [[1, 2], [3, 4]] ), mk()[1][1]", "4"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.trim(), printed, "{tail}");
    }
    for (tail, expect) in [
        ("a := array array i32 [[1, 2], 3]", "an element does not fit the element type"),
        ("a := array array i32 [(free (1), 2)]", "the list is written in square brackets"),
        ("x := i64 5, a := array array i32 [[1], [x]]", "an element does not fit the element type"),
        (
            "p := type ( static parse = ( tape.construct(i32, tape[1]), tape.is_constructed[0] = true ) )",
            "unknown name `construct`",
        ),
    ] {
        let (code, _, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn the_outer_array_s_free_frees_each_element_once() {
    let array = "import file «./identities/array.logos»";
    for (tail, printed) in [
        ("a := array boxed [box (1, 2), box (3, 4)], print «made»", "made\nfree\nfree\n"),
        (
            "f := fn () -> i32 ( a := array boxed [box (1, 2), box (3, 4)], 7 ), \
             print «before», free (f()), print «after»",
            "before\nfree\nfree\nafter\n",
        ),
        (
            "f := fn () -> i32 ( a := array boxed [box (1, 2)], 7 ), f.compile(), \
             print «before», free (f()), print «after»",
            "before\nfree\nafter\n",
        ),
        ("x := box (1, 2), a := array boxed [move x], print «made»", "made\nfree\n"),
        ("a := array boxed [box (5, 6)], free a, print «after»", "free\nafter\n"),
        (
            "a := array array boxed [[box (1, 2)], [box (3, 4), box (5, 6)]], print «made»",
            "made\nfree\nfree\nfree\n",
        ),
        (
            "f := fn () -> i32 ( a := array array boxed [[box (1, 2)], [box (3, 4)]], 1 ), \
             free (f()), free (f()), print «after»",
            "free\nfree\nfree\nfree\nafter\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {BOX}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert!(stdout.starts_with(printed), "{tail}: stdout: {stdout}");
        assert_eq!(stdout.matches("free").count(), printed.matches("free").count(), "{tail}");
    }
}

#[test]
fn free_of_an_element_runs_its_free_there_and_the_array_skips_it() {
    let array = "import file «./identities/array.logos»";
    for tail in [
        "x := box (1, 2), arr := array boxed [move x], free (arr[0]), print «after»",
        "x := box (1, 2), arr := array boxed [move x], free arr[0], print «after»",
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {BOX}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, "free\nafter\n", "{tail}");
    }
    // A compiled body holding the cell's free declines to compile.
    for free in ["free (arr[0])", "free (arr.ptr + 0)@"] {
        let g = format!(
            "g := fn () -> i32 ( x := box (1, 2), arr := array boxed [move x], {free}, 1 )"
        );
        let (code, stdout, stderr) = run_line(&format!("{array}, {BOX}, {g}, g.compile(), g()"));
        assert_eq!(code, Some(1), "{free}: stderr: {stderr}");
        assert!(stdout.is_empty(), "{free}: stdout: {stdout}");
        assert!(stderr.contains("cannot be compiled yet"), "{free}: stderr: {stderr}");
    }
    // The cell is reached whatever its type, bounds check included; over `i32` nothing is freed.
    for tail in [
        "a := array i32 [1, 2], free (a[5]), 1",
        "x := box (1, 2), arr := array boxed [move x], free (arr[5]), 1",
        "g := fn () -> i32 ( a := array i32 [1, 2], free (a[5]), 1 ), g.compile(), g()",
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {BOX}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stdout: {stdout}");
        assert!(stderr.contains("index out of range"), "{tail}: stderr: {stderr}");
    }
    for (tail, printed) in [
        ("a := array i32 [1, 2], free (a[0]), a[0]", "1\n"),
        (
            "x := i32 3, get := fn (q := @i32 ?) -> i32 ( print «ran», q@ ), free (get(&x)), x",
            "ran\n3\n",
        ),
        // A plain record's field lies in its bytes, not in a node's slot.
        ("p := type ( x := i32 ?, y := i32 ? ), q := p (1, 2), free q.x, q.y", "2\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{tail}");
    }
}

#[test]
fn free_of_a_value_runs_it_and_frees_what_it_made() {
    let array = "import file «./identities/array.logos»";
    for (tail, printed) in [
        ("free (box (1, 2)), print «after»", "free\nafter\n"),
        ("free box (1, 2), print «after»", "free\nafter\n"),
        ("mk := fn () -> own boxed ( box (1, 2) ), free (mk()), print «after»", "free\nafter\n"),
        (
            "g := fn () -> i32 ( free (box (1, 2)), 7 ), g.compile(), print «before», free (g()), \
             print «after»",
            "before\nfree\nafter\n",
        ),
        // A borrow's free is its owner's, at the owner's end.
        (
            "b := box (1, 2), id := fn (q := boxed ?) -> boxed ( q ), free (id(b)), print «after»",
            "after\nfree\n",
        ),
        ("f := fn () -> i32 ( print «ran», 7 ), free (f()), print «after»", "ran\nafter\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {BOX}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{tail}");
    }
    // A field of a node the parse knows is followed from its name, whose type fills `free`.
    let (code, _, stderr) = run_line(&format!("{array}, {BOX}, b := box (1, 2), free b.p, 1"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("the type of `b` fills its own `static free"), "stderr: {stderr}");
}

#[test]
fn free_and_move_refuse_what_they_cannot_take_where_it_stands() {
    // Each error stands at the operand it refuses.
    for (src, operand, message) in [
        (
            "f := fn () -> i32 ( 7 ), b := move (f()), 1",
            "(f())",
            "`move` moves a value out of a place",
        ),
        ("free (own @i32 ?), 1", "(own", "a hole, `T ?`, holds no value yet"),
        (
            "g := fn () -> i32 ( free (own @i32 ?), 1 ), g.compile(), g()",
            "(own",
            "a hole, `T ?`, holds no value yet",
        ),
        ("free i32, 1", "i32", "`i32` is a name the run starts with"),
        (
            "c := i32 1, p := alloc 1 of i32 7, free (if (c == 1) (alloc 1 of i32 5) else (p)), 1",
            "(if",
            "make a value in every arm, or borrow in every arm",
        ),
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stdout.is_empty(), "{src}: stdout: {stdout}");
        let col = src.find(operand).expect("the operand is in the program") + 1;
        let head = format!("<command line>:1:{col}: error: {message}");
        assert!(stderr.starts_with(&head), "{src}: stderr: {stderr}");
    }
}

#[test]
fn the_repl_runs_a_freed_value_and_refuses_a_hole_and_a_moved_value() {
    let (echoes, stderr) = repl(
        b"f := fn () -> i32 ( print \xc2\xabran\xc2\xbb, 7 )\nfree (f())\nfree (own @i32 ?)\n\
          b := move (f())\nfree (alloc 1 of i32 5)\n1\n",
    );
    assert_eq!(echoes, ["ran", "1"], "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:6: error: a hole"), "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:11: error: `move` moves a value"), "stderr: {stderr}");
}

#[test]
fn an_array_is_freed_by_its_owner_and_a_borrow_outliving_it_reads_nothing() {
    let array = "import file «./identities/array.logos», a := array i32 [1, 2, 3]";
    for (tail, printed) in [
        ("free a, 5", "5"),
        ("b := a, f := fn (p := array i32 ?) -> i32 ( p[1] ), f(b)", "2"),
        // A copy is its own array, so it outlives the one it was made from.
        ("b := a, free a, b[0]", "1"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.trim(), printed, "{tail}");
    }
    // Until the borrow checker, a call's result may borrow the argument the call frees as it
    // returns; the free emptied the array's pointer, so a read at the first cell is refused.
    for (tail, expect) in [
        ("free a, a[0]", "`a` is dead here"),
        (
            "f := fn (q := array i32 ?) -> array i32 ( q ), w := f(array i32 [1, 2]), w[0]",
            "holds nothing",
        ),
    ] {
        let (code, _, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
}

/// A type with no `run` whose own `parse` builds its nodes.
const COUNTED: &str = "q := type ( n := u64 ?, k := u64 7, \
    static twice := fn () -> u64 ( n * 2 ), static parse_rank = 60, static parse = ( \
    tape[0].type = q, tape[0].n = tape[1].dyads.size, tape.remove(1), tape.is_constructed[0] = true ) )";

#[test]
fn a_node_a_parse_built_is_declared_and_its_fields_read() {
    for (tail, want) in [
        ("a := q (1, 2, 3), a.n + a.k", "10\n"),
        ("f := fn () -> u64 ( a := q (1, 2), a.n ), f()", "2\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{COUNTED}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn a_member_function_called_through_a_node_reads_that_node() {
    for (tail, want) in [
        ("a := q (1, 2, 3), a.twice()", "6\n"),
        ("a := q (1, 2, 3), b := q (1, 2), a.twice() + b.twice()", "10\n"),
        ("f := fn () -> u64 ( a := q (1, 2, 3, 4), a.twice() ), f()", "8\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{COUNTED}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn a_value_left_unwritten_is_filled_one_field_at_a_time() {
    let pt = "pt := type ( mut x := i32 ?, mut y := i32 ? )";
    let bx = "bx := type ( mut p := own @i32 ?, mut size := u64 0, static free = ( free p ), \
              static parse_rank = dyad.parse_rank, static parse = ( tape.is_constructed[0] = true ) )";
    for (src, want) in [
        (format!("{pt}, f := fn () -> i32 ( mut v := pt ?, v.x = 3, v.y = 4, v.x + v.y ), f()"), "7\n"),
        (format!("{pt}, mut v := pt ?, v.x = 3, v.y = 4, v.x * v.y"), "12\n"),
        // A field with a default already holds it.
        (
            format!("{bx}, f := fn (n := u64 ?) -> u64 ( mut v := bx ?, v.p = mut alloc n of i32 ?, v.size ), f(2)"),
            "0\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{src}: stderr: {stderr}");
    }
    for (src, expect) in [
        (format!("{pt}, f := fn () -> i32 ( mut v := pt ?, v.x = 3, v.y ), f()"), "`v.y` is read before"),
        (
            format!("{pt}, g := fn (q := pt ?) -> i32 ( 1 ), f := fn () -> i32 ( mut v := pt ?, v.x = 3, g(v) ), f()"),
            "`v` is read before",
        ),
        // A write inside a branch does not fill it.
        (
            format!("{pt}, f := fn () -> i32 ( mut v := pt ?, if true ( v.x = 3 ), v.y = 1, v.x ), f()"),
            "`v.x` is read before",
        ),
        (format!("{bx}, f := fn () -> u64 ( mut v := bx ?, v.size = 2, move v ), f()"), "`v` is read before"),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_bare_static_call_works_on_the_value_the_body_is_about() {
    let quad = COUNTED.replace(
        "static twice := fn () -> u64 ( n * 2 ),",
        "static twice := fn () -> u64 ( n * 2 ), static quad := fn () -> u64 ( twice() * 2 ),",
    );
    let (code, stdout, stderr) = run_line(&format!("{quad}, a := q (1, 2, 3), a.quad()"));
    assert_eq!((code, stdout.as_str()), (Some(0), "12\n"), "stderr: {stderr}");
    // From a run: a value holding this run's fields, so an operand written as an
    // expression, or a function's parameter, is read as its value.
    let inc = "inc := type ( a := i32 ?, output_type := type ?, \
        static twice := fn () -> i32 ( a * 2 ), static run = ( twice() + 1 ), \
        static parse_rank = *.parse_rank + 1, static parse = ( tape[0].type = inc, \
        tape[0].a = tape[-1], tape[0].output_type = i32, tape.is_constructed[0] = true, \
        tape.remove(-1) ) )";
    for (tail, want) in [
        ("x := i32 5, x inc", "11\n"),
        ("f := fn (y := i32 ?) -> i32 ( (y + 1) inc ), f(20)", "43\n"),
        ("f := fn (y := i32 ?) -> i32 ( y inc ), f.compile(), f(20) + f(1)", "44\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{inc}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
    // A parse has no value to hand one that reads a field, itself or through a bare call;
    // outside the type it is no name.
    for (src, expect) in [
        (
            "q := type ( n := u64 ?, static get := fn () -> u64 ( n ), static parse_rank = 60, \
             static parse = ( tape[0].type = q, tape[0].n = get(), tape.is_constructed[0] = true ) ), q"
                .to_string(),
            "`get` works on a value of its type: call it through one, `v.get(…)`,",
        ),
        (
            "q := type ( n := u64 ?, static get := fn () -> u64 ( n ), \
             static via := fn () -> u64 ( get() ), static parse_rank = 60, \
             static parse = ( tape[0].type = q, tape[0].n = via(), tape.is_constructed[0] = true ) ), q"
                .to_string(),
            "`via` works on a value of its type: call it through one, `v.via(…)`,",
        ),
        (format!("{COUNTED}, twice()"), "`twice` is not in scope"),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_parse_stamps_its_own_cell_before_it_writes_a_field() {
    for (src, expect) in [
        (
            "q := type ( n := u64 ?, static parse_rank = 60, static parse = ( \
             tape[0].n = 3, tape[0].type = q, tape.is_constructed[0] = true ) ), q",
            "still holds the type",
        ),
        (
            "r := type ( n := u64 ? ), q := type ( static parse_rank = 60, static parse = ( \
             tape[0].type = r, tape.is_constructed[0] = true ) ), q",
            "with a `run` or a `parse` of its own",
        ),
        (
            "q := type ( static parse_rank = 60, static parse = ( \
             tape[1].type = q, tape.is_constructed[0] = true ) ), q 5",
            "on its own cell",
        ),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_field_the_constructor_never_wrote_is_a_checked_error() {
    for src in [
        // The node is used as a value with its field `b` unwritten.
        "t := type (a := i32 ?, b := i32 ?, output_type := type ?, static run = ( a ), \
         static parse_rank = *.parse_rank + 1, \
         static parse = ( tape[0].type = t, tape[0].a = tape[-1], tape[0].output_type = i32, \
         tape.is_constructed[0] = true, tape.remove(-1) )), x := i32 4, x t",
        // The unwritten field itself is read in the parse body.
        "q := type ( size := u64 ?, static parse_rank = 60, \
         static parse = ( tape[0].type = q, tape[0] = tape[0].size, tape.is_constructed[0] = true ) ), q",
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains("was never written by its constructor"), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_fields_type_reads_its_declared_type() {
    let (code, stdout, stderr) = run_line(
        "q := type ( size := u64 ?, static parse_rank = 60, \
         static parse = ( tape[0].type = q, tape[0].size = 3, tape[0] = tape[0].size.type, \
         tape.is_constructed[0] = true ) ), q",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "u64\n"), "stderr: {stderr}");
    // A `dyad` field has the type of the node it holds.
    let q = "q := type ( size := dyad ?, static parse_rank = 60, \
             static parse = ( tape[0].type = q, tape[0].size = 3, \
             if tape[0].size.type == rational_number ( tape[0] = u64 1 ) else ( tape[0] = u64 2 ), \
             tape.is_constructed[0] = true ) )";
    for use_ in ["q", "f := fn () -> u64 ( q ), f()", "f := fn () -> u64 ( q ), f.compile(), f()"] {
        let src = format!("{q}, {use_}");
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.as_str()), (Some(0), "1\n"), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_new_name_without_its_type_is_refused_where_it_is_written() {
    let name = "a name needs a type: write it, or `dyad ?` for one whose type changes";
    let field = "a field needs a type: write it, write `dyad ?` for one whose type changes, or \
                 write a function that builds the mints, as `array` does";
    for (src, at, message) in [
        ("mut x := ?, x = i32 5, x", "1:5", name),
        ("f := fn () -> i32 ( mut x := ?, x = i32 5, x ), f.compile(), f()", "1:25", name),
        ("show := fn (a) -> void ( print «{a}» ), show(i32 5)", "1:13", name),
        ("f := fn (a := ?) -> i64 ( a ), f.compile(), f(i64 42)", "1:10", name),
        ("pt := type ( a := ? ), p := pt(i32 1), 0", "1:14", field),
        ("pt := type ( b := i32 ?, a ), 0", "1:26", field),
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{src}: stderr: {stderr}");
        assert!(stderr.contains(&format!("{at}: error: {message}\n")), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_field_default_fills_each_new_node() {
    let q = "q := type ( mut size := u64 5, output_type := type ?, \
             static run = ( size + 1 ), static parse_rank = 60, \
             static parse = ( tape[0].type = q, ";
    let end = "tape[0].output_type = u64, tape.is_constructed[0] = true ) )";
    for (src, want) in [
        (format!("{q}{end}, q"), "6\n"),
        (format!("{q}tape[0].size = 9, {end}, q"), "10\n"),
        (format!("{q}{end}, f := fn () -> u64 ( q ), f.compile(), f()"), "6\n"),
        (
            "q := type ( size := u64 0, static parse_rank = 60, \
             static parse = ( tape[0].type = q, tape[0] = tape[0].size, tape.is_constructed[0] = true ) ), q"
                .to_string(),
            "0\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{src}: stderr: {stderr}");
    }
}

/// DESIGN ›Operands travel on the stack‹: a plain record crosses a call by copy, in, out,
/// and straight on into another call, on both tiers and with the caller compiled too.
#[test]
fn a_plain_record_passes_into_and_out_of_a_function_by_copy() {
    let types = "p := type ( mut x := i32 ?, mut y := i32 ? ), \
                 w := type ( a := i64 ?, b := i64 ?, c := i64 ? )";
    let f = "f := fn (v := p ?) -> i32 ( v.x )";
    let mk = "mk := fn () -> p ( p (3, 4) )";
    let cases: &[(&str, &[&str], &str, &str, &str)] = &[
        ("", &[], "a := p (1, 2), a.x", "i32", "1"),
        (f, &["f"], "a := p (1, 2), f(a)", "i32", "1"),
        (f, &["f"], "f(p (1, 2))", "i32", "1"),
        (mk, &["mk"], "b := mk(), b.x", "i32", "3"),
        (mk, &["mk"], "b := mk(), b.x * 10 + b.y", "i32", "34"),
        ("f := fn (v := @p ?) -> i32 ( v@.x )", &["f"], "a := p (1, 2), f(&a)", "i32", "1"),
        (
            "f := fn (q := w ?) -> i64 ( q.a + q.b + q.c )",
            &["f"],
            "q := w (1, 20, 300), f(q)",
            "i64",
            "321",
        ),
        (
            "f := fn (n := i32 ?, q := w ?, v := p ?, m := i64 ?) -> i64 ( i64(n) + q.c + i64(v.y) + m )",
            &["f"],
            "f(1, w (1, 2, 30), p (5, 600), 4000)",
            "i64",
            "4631",
        ),
        (
            "mkw := fn (k := i64 ?) -> w ( w (k, k + 1, k + 2) )",
            &["mkw"],
            "r := mkw(10), r.a * 100 + r.b * 10 + r.c",
            "i64",
            "1122",
        ),
        (
            "f := fn (mut v := p ?) -> i32 ( v.x = 9, v.x )",
            &["f"],
            "a := p (1, 2), f(a) + a.x * 100",
            "i32",
            "109",
        ),
        (
            "g := fn (v := p ?) -> i32 ( v.y ), mk := fn () -> p ( p (3, 4) )",
            &["mk", "g"],
            "g(mk())",
            "i32",
            "4",
        ),
        (
            "id := fn (v := p ?) -> p ( v )",
            &["id"],
            "a := p (7, 8), b := id(id(a)), b.x + b.y",
            "i32",
            "15",
        ),
        (
            "pick := fn (k := i32 ?, u := p ?, v := p ?) -> p ( if k > 0 return u, v )",
            &["pick"],
            "b := pick(1, p (1, 2), p (3, 4)), c := pick(0, p (1, 2), p (3, 4)), b.y * 10 + c.y",
            "i32",
            "24",
        ),
        (
            "fact := fn (n := i32 ?, acc := p ?) -> p ( \
             if n == 0 (acc) else (fact(n - 1, p (acc.x * n, acc.y + 1))) )",
            &["fact"],
            "r := fact(5, p (1, 0)), r.x * 100 + r.y",
            "i32",
            "12005",
        ),
    ];
    for &(defs, compiled, tail, ty, want) in cases {
        let compiles: String = compiled.iter().map(|name| format!("{name}.compile(), ")).collect();
        let defs = if defs.is_empty() { String::new() } else { format!("{defs}, ") };
        for src in [
            format!("{types}, {defs}{tail}"),
            format!("{types}, {defs}{compiles}{tail}"),
            format!("{types}, {defs}h := fn () -> {ty} ( {tail} ), h.compile(), h()"),
            format!("{types}, {defs}{compiles}h := fn () -> {ty} ( {tail} ), h.compile(), h()"),
        ] {
            let (code, stdout, stderr) = run_line(&src);
            assert_eq!((code, stdout.trim_end()), (Some(0), want), "{src}: stderr: {stderr}");
        }
    }
    // Filled one field at a time, then passed whole; a `T ?` record local does not compile yet.
    for compile in ["", "f.compile(), "] {
        let src = format!("{types}, {f}, {compile}mut u := p ?, u.x = 1, u.y = 2, f(u)");
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.as_str()), (Some(0), "1\n"), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_record_argument_or_result_is_checked_at_parse() {
    let p = "p := type ( mut x := i32 ?, mut y := i32 ? ), q := type ( z := i32 ?, t := i32 ? )";
    for (tail, reason) in [
        ("f := fn (v := p ?) -> i32 ( v.x ), f(5)", "do not match"),
        ("f := fn (v := p ?) -> i32 ( v.x ), f(q (1, 2))", "do not match"),
        ("mk := fn () -> p ( q (1, 2) ), 1", "do not match"),
        ("mk := fn () -> p ( 5 ), 1", "do not match"),
        (
            "f := fn (v := p ?) -> i32 ( v.x ), mut u := p ?, u.x = 1, f(u)",
            "read before it is written",
        ),
    ] {
        let (code, _, stderr) = run_line(&format!("{p}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(reason), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn a_run_body_takes_a_plain_record_field_by_copy() {
    let fx = "p := type ( x := i32 ?, y := i32 ? ), fx := type ( a := p ?, output_type := type ?, \
              static run = ( a.y ), static parse_rank = 60, static parse = ( tape[0].type = fx, \
              tape[0].a = tape[1], tape[0].output_type = i32, tape.is_constructed[0] = true, \
              tape.remove(1) ) ), v := p (1, 2)";
    for tail in ["fx v", "g := fn () -> i32 ( fx v ), g.compile(), g()"] {
        let (code, stdout, stderr) = run_line(&format!("{fx}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), "2\n"), "{tail}: stderr: {stderr}");
    }
}

/// Runs each program and checks its whole stdout.
fn assert_runs(cases: &[(String, &str)]) {
    for (src, want) in cases {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!((code, stdout.as_str()), (Some(0), *want), "{src}: stderr: {stderr}");
    }
}

/// DESIGN ›`=` sits beside `:=`, and returns nothing‹: `=` writes a whole record through the
/// address its left side gives, from a constructor, a name, a call or itself, on both tiers.
#[test]
fn equals_writes_a_whole_record_into_its_place() {
    let defs = "q := type ( x := i32 ? ), w := q(4), mk := fn () -> q ( q(5) )";
    let mut cases = Vec::new();
    for (body, want) in [
        ("mut v := q(1), v = q(3), v.x", "3\n"),
        ("mut v := q(1), v = w, v.x", "4\n"),
        ("mut v := q(1), v = mk(), v.x", "5\n"),
        ("mut v := q(1), v = v, v.x", "1\n"),
        ("mut v := q(1), p := &v, p@ = q(3), v.x", "3\n"),
        ("mut v := q(0), mut i := i32 0, while i < 3 ( v = q(i), i = i + 1 ), v.x", "2\n"),
    ] {
        cases.push((format!("{defs}, {body}"), want));
        cases.push((format!("{defs}, f := fn () -> i32 ( {body} ), f()"), want));
        cases.push((format!("{defs}, f := fn () -> i32 ( {body} ), f.compile(), f()"), want));
    }
    // Into a hole, a parameter and a field that fits it; a `T ?` record local does not compile yet.
    let param = "f := fn (mut v := q ?) -> i32 ( v = q(3), v.x )";
    cases.extend([
        (format!("{defs}, mut v := q ?, v = q(3), v.x"), "3\n"),
        (format!("{defs}, {param}, f(q(1))"), "3\n"),
        (format!("{defs}, {param}, f.compile(), f(q(1))"), "3\n"),
        (
            "q2 := type ( mut x := i32 ?, mut y := i32 ? ), o := type ( mut a := q2 ?, mut b := i32 ? ), \
             mut v := o ?, v.b = 5, v.a = q2(3, 4), v.a.y * 10 + v.b"
                .to_string(),
            "45\n",
        ),
    ]);
    assert_runs(&cases);
}

/// DESIGN ›`:=` decides nothing about copy or reference; the type's reading rule does‹: a plain
/// record's name gets a copy of its own, from a name, a parameter or a dereference.
#[test]
fn a_declaration_copies_a_plain_record() {
    let q = "q := type ( mut x := i32 ? ), pt := type ( mut a := i64 ?, mut b := i32 ? )";
    let mut cases = Vec::new();
    for (ty, body, want) in [
        ("i32", "v := q(3), mut w := v, w.x = 9, v.x", "3\n"),
        ("bool", "v := q(3), w := v, &v == &w", "false\n"),
        ("i32", "v := q(3), w := v, mut u := w, u.x = 4, v.x * 100 + w.x * 10 + u.x", "334\n"),
        ("i32", "v := pt(1, 2), p := &v, w := p@, w.b", "2\n"),
    ] {
        cases.push((format!("{q}, {body}"), want));
        cases.push((format!("{q}, f := fn () -> {ty} ( {body} ), f()"), want));
        cases.push((format!("{q}, f := fn () -> {ty} ( {body} ), f.compile(), f()"), want));
    }
    for (f, call) in [
        ("f := fn (a := q ?) -> i32 ( mut b := a, b.x = 9, a.x )", "f(q(3))"),
        ("f := fn (mut v := q ?) -> i32 ( w := v, v.x = 9, w.x )", "f(q(3))"),
    ] {
        cases.push((format!("{q}, {f}, {call}"), "3\n"));
        cases.push((format!("{q}, {f}, f.compile(), {call}"), "3\n"));
    }
    // A copy of a value whose type fills `free` owns its copy: each name frees its own.
    cases.push((
        "r := type ( mut n := i32 ?, static free = ( print «freed {n}» ) ), w := r(2), y := w, 0"
            .to_string(),
        "freed 2\nfreed 2\n0\n",
    ));
    assert_runs(&cases);
}

/// ›`:=` decides nothing about copy or reference; the type's reading rule does‹: `=` into a
/// place that holds its value frees the value it displaces and takes a copy of a named owner.
#[test]
fn an_assignment_copies_a_value_that_owns_memory() {
    let r = "r := type ( mut n := i32 ?, static free = ( print «freed {n}» ) )";
    let body = "mut v := r(1), w := r(2), v = w, v.n = 3, 0";
    let freed = "freed 1\nfreed 2\nfreed 3\n0\n";
    let array = "import file «./identities/array.logos»";
    assert_runs(&[
        (format!("{r}, {body}"), freed),
        (format!("{r}, f := fn () -> i32 ( {body} ), f()"), freed),
        (format!("{r}, f := fn () -> i32 ( {body} ), f.compile(), f()"), freed),
        (
            format!(
                "{array}, a := array i32 [1, 2], mut b := array i32 [3], b = a, b[0] = i32 9, a[0]"
            ),
            "1\n",
        ),
        (
            format!(
                "{array}, a := array i32 [1, 2], mut b := array i32 [3], b = a, b[0] = i32 9, b[0]"
            ),
            "9\n",
        ),
        (
            "p := alloc 1 of i32 5, mut q := alloc 1 of i32 6, q = p, q@ = i32 9, p@".to_string(),
            "5\n",
        ),
    ]);
}

/// ›Explicit heap: `alloc n` and `alloc n of T v`‹: each cell is built from the value, the first
/// taking it and every other one owning a copy of its own.
#[test]
fn alloc_gives_each_cell_a_value_of_its_own() {
    let array = "import file «./identities/array.logos», p := alloc 2 of array i32 [1, 2]";
    let boxes = format!("{BOX}, p := alloc 2 of box (1, 2), ((p + 1)@.p + 0)@ = i32 9");
    assert_runs(&[
        (format!("{array}, (p + 1)@[0] = i32 9, p@[0]"), "1\n"),
        (format!("{array}, (p + 1)@[0] = i32 9, (p + 1)@[0]"), "9\n"),
        (format!("{boxes}, print «{{(p@.p + 0)@}} {{((p + 1)@.p + 0)@}}»"), "1 9\n"),
        (format!("{boxes}, free p@, free (p + 1)@, print «freed»"), "free\nfree\nfreed\n"),
        // A named owner is copied into every cell, the first included.
        (
            "import file «./identities/array.logos», a := array i32 [1, 2], p := alloc 2 of a, \
             p@[0] = i32 9, a[0]"
                .to_string(),
            "1\n",
        ),
        (
            format!(
                "{BOX}, x := box (1, 2), p := alloc 1 of x, (p@.p + 0)@ = i32 9, \
                 print «{{(x.p + 0)@}}»"
            ),
            "1\nfree\n",
        ),
    ]);
}

/// DESIGN ›Explicit heap: `alloc n` and `alloc n of T v`‹: a cell through a pointer is read and
/// written whole for every type a place of it has, a plain record, a rational, `type`, `dyad`.
#[test]
fn a_cell_through_a_pointer_moves_any_type_with_a_layout() {
    let pt = "pt := type ( mut a := i64 ?, mut b := i32 ? ), g := fn (v := pt ?) -> i32 ( v.b )";
    let mut cases = vec![
        (format!("{pt}, p := alloc 1 of pt ?, p@.b = 7, p@.b"), "7\n"),
        (format!("{pt}, p := alloc 2 of pt (1, 2), (p + 1)@.b"), "2\n"),
        (format!("{pt}, p := alloc 2 of pt (1, 2), (p + 1)@ = pt(5, 6), p@.b * 10 + (p + 1)@.b"), "26\n"),
        (format!("{pt}, f := fn (p := @pt ?) -> pt ( p@ ), w := pt(1, 2), r := f(&w), r.b"), "2\n"),
        (
            format!("{pt}, f := fn (p := @pt ?) -> pt ( p@ ), f.compile(), w := pt(1, 2), r := f(&w), r.b"),
            "2\n",
        ),
        ("p := alloc 1 of rational_number ?, p@ = rational_number 1/2, p@".to_string(), "1/2\n"),
        ("p := alloc 2 of rational_number (1/3), (p + 1)@".to_string(), "1/3\n"),
        (
            "f := fn (p := @rational_number ?) -> i32 ( p@ = rational_number 1/2, 1 ), \
             q := alloc 1 of rational_number ?, free (f(q)), q@"
                .to_string(),
            "1/2\n",
        ),
        (
            "f := fn (p := @rational_number ?, r := rational_number ?) -> i32 ( p@ = r, 1 ), \
             f.compile(), q := alloc 1 of rational_number ?, free (f(q, 1/2)), q@"
                .to_string(),
            "1/2\n",
        ),
        (
            "f := fn (p := @rational_number ?) -> rational_number ( p@ ), f.compile(), \
             q := alloc 1 of rational_number ?, q@ = rational_number 2/3, f(q)"
                .to_string(),
            "2/3\n",
        ),
        ("p := alloc 1 of type ?, p@ = i32, t := p@, t".to_string(), "i32\n"),
        ("p := alloc 1 of dyad ?, p@ = i32, d := p@, d == i32".to_string(), "true\n"),
        (
            "pt := type ( mut t := type ?, mut b := i32 ? ), mut v := pt ?, v.t = i32, v.b = 1, \
             p := &v, p@.t = i64, v.t"
                .to_string(),
            "i64\n",
        ),
        (
            "pt := type ( mut t := type ?, mut b := i32 ? ), mut v := pt ?, v.t = i32, v.b = 1, \
             p := &v, p@.t == i32"
                .to_string(),
            "true\n",
        ),
    ];
    for (body, want) in [
        ("w := pt(1, 2), p := &w, g(p@)", "2\n"),
        ("w := pt(1, 2), p := &w, mut v := pt(0, 0), v = p@, v.b", "2\n"),
    ] {
        cases.push((format!("{pt}, {body}"), want));
        cases.push((format!("{pt}, f := fn () -> i32 ( {body} ), f.compile(), f()"), want));
    }
    assert_runs(&cases);
}

/// ›A type's `parse` decides what converts into it; unwritten, a value converts only into a type
/// that holds it exactly‹ [[R155]]: a type takes the name to its right as it takes a number.
#[test]
fn a_type_converts_the_name_to_its_right() {
    let mut cases = vec![
        ("f := fn (n := i32 ?) -> i32 ( x := alloc 1 of i32 n, x@ ), f(4)".to_string(), "4\n"),
        ("r := rational_number 1/3, rational_number r".to_string(), "1/3\n"),
        (
            "f := fn (r := rational_number ?) -> rational_number ( rational_number r ), f(1/3)"
                .to_string(),
            "1/3\n",
        ),
        ("m := i32 3, i64 m == i64 3".to_string(), "true\n"),
    ];
    for (f, call, want) in [
        ("fn (n := i32 ?) -> i64 ( i64 n )", "f(4)", "4\n"),
        ("fn (n := i32 ?) -> f64 ( f64 n )", "f(4)", "4.0\n"),
        ("fn (n := f64 ?) -> i32 ( i32 lossy n )", "f(2.5)", "2\n"),
        ("fn (n := i32 ?, m := i32 ?) -> i64 ( i64 n * i64 m )", "f(4, 5)", "20\n"),
    ] {
        cases.push((format!("f := {f}, {call}"), want));
        cases.push((format!("f := {f}, f.compile(), {call}"), want));
    }
    assert_runs(&cases);
}

/// ›A parse is ranked by what it takes from its right‹ [[R116]]: a number type sits below `(`,
/// so `.`, `@`, a call and an `if` are built before it, on both tiers.
#[test]
fn a_number_type_takes_the_value_built_at_discovery_before_it() {
    let pt = "pt := type (a := i32 ?, b := i32 ?)";
    let g = "g := fn (n := i32 ?) -> i32 ( n )";
    let mut cases = vec![
        (format!("{pt}, p := pt(7, 8), i64 p.a"), "7\n"),
        (format!("{pt}, p := pt(7, 8), i32 lossy p.a"), "7\n"),
        (format!("{pt}, p := pt(7, 8), x := alloc 1 of i32 p.a, x@"), "7\n"),
        ("q := i32 5, r := &q, i64 r@".to_string(), "5\n"),
        (format!("{g}, i64 g(3)"), "3\n"),
        ("c := true, x := i64 if c (i32 1) else (i64 2), x".to_string(), "1\n"),
        (
            "pt := type (a := rational_number ?, b := i32 ?), p := pt(1/3, 8), rational_number p.a"
                .to_string(),
            "1/3\n",
        ),
        ("m := i32 3, i64 m + i64 1".to_string(), "4\n"),
        ("m := i32 3, q := &i64 m, q@".to_string(), "3\n"),
        ("i32 -5".to_string(), "-5\n"),
        ("x := 5, x.type == rational_number".to_string(), "true\n"),
        ("(1/3).type == rational_number".to_string(), "true\n"),
        ("rational_number 1.05".to_string(), "21/20\n"),
        ("i32.parse_rank == rational_number.parse_rank".to_string(), "true\n"),
    ];
    for (f, call, want) in [
        (format!("{pt}, f := fn (q := pt ?) -> i64 ( i64 q.a )"), "f(pt(7, 8))", "7\n"),
        ("f := fn (r := @i32 ?) -> i64 ( i64 r@ )".to_string(), "q := i32 5, f(&q)", "5\n"),
        (format!("{g}, f := fn () -> i64 ( i64 g(3) )"), "f()", "3\n"),
        ("f := fn (m := i32 ?) -> i64 ( i64 m + i64 1 )".to_string(), "f(3)", "4\n"),
        (
            "f := fn (c := bool ?) -> i64 ( i64 if c (i32 1) else (i64 2) )".to_string(),
            "f(false)",
            "2\n",
        ),
        ("f := fn () -> i32 ( i32 -5 )".to_string(), "f()", "-5\n"),
    ] {
        cases.push((format!("{f}, {call}"), want));
        cases.push((format!("{f}, f.compile(), {call}"), want));
    }
    assert_runs(&cases);
}

/// ›A parse is ranked by what it takes from its right‹ [[R116]]: number types, `rational_number`
/// and `lossy` associate right, so of two in a row the one nearer the value is built first, and
/// `lossy` makes the value the type to its left takes, on both tiers (›A type's `parse` decides
/// what converts into it; unwritten, a value converts only into a type that holds it exactly‹
/// [[R155]]).
#[test]
fn number_types_and_lossy_take_the_value_to_their_right_rightmost_first() {
    let mut cases = vec![("i32 lossy rational_number 1/3".to_string(), "0\n")];
    for (f, call, want) in [
        ("fn () -> i64 ( i64 i32 5 )", "f()", "5\n"),
        ("fn () -> f64 ( f64 i32 5 )", "f()", "5.0\n"),
        ("fn () -> i32 ( i32 lossy f64 2.5 )", "f()", "2\n"),
        ("fn () -> i32 ( i32 lossy i64 5000000000 )", "f()", "705032704\n"),
        ("fn (x := f64 ?) -> i32 ( i32 lossy (x) )", "f(2.5)", "2\n"),
        ("fn (x := f64 ?) -> i32 ( if i32 2 == i32 lossy (x) (1) else (0) )", "f(2.5)", "1\n"),
    ] {
        cases.push((format!("f := {f}, {call}"), want));
        cases.push((format!("f := {f}, f.compile(), {call}"), want));
    }
    assert_runs(&cases);
}

/// ›A type's `parse` decides what converts into it; unwritten, a value converts only into a type
/// that holds it exactly‹ [[R155]]: `lossy` needs a number type to its left and a value to its
/// right, and is refused where it is written.
#[test]
fn lossy_without_both_sides_is_refused_where_it_is_written() {
    let refused = "`lossy` stands between a number type and the value it converts";
    for (src, at) in [
        ("x := lossy (f64 2.5), 0", "1:6:"),
        ("lossy 2.5", "1:1:"),
        ("x := i32 lossy, 0", "1:10:"),
        ("rational_number lossy 2.5", "1:17:"),
        ("f := fn () -> i32 ( x := lossy (f64 2.5), 0 ), f.compile(), f()", "1:26:"),
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{src}: stderr: {stderr}");
        assert!(stderr.contains(&format!("{at} error: {refused}")), "{src}: stderr: {stderr}");
    }
}

/// What a number type's lower rank leaves refused: an `if` arm the type cannot hold, a type a
/// `-` subtracts from (›`-` negates where a space stands before it and none after, or no value
/// stands to its left; any other `-` subtracts‹ [[R157]]), a bracket read through `.`, a type
/// that cannot hold the type to its right, and a type left of `=` reading past it.
#[test]
fn a_number_type_below_open_refuses_what_it_cannot_take() {
    let mismatch = "these types do not match";
    let operator = "this operator cannot compute over these operands";
    for (src, want) in [
        ("c := true, x := i64 if c (f64 1.5) else (i64 2), x", mismatch),
        ("c := true, x := i64 if c (u64 1) else (i64 2), x", mismatch),
        (
            "f := fn (c := bool ?) -> i64 ( i64 if c (u64 1) else (i64 2) ), f.compile(), f(true)",
            mismatch,
        ),
        ("m := i32 3, i64 - m", operator),
        ("m := i32 3, i64-m", operator),
        ("f := fn (m := i32 ?) -> i64 ( i64 - m ), f.compile(), f(3)", operator),
        ("x := i32 5, i32 (x).type", mismatch),
        ("i32 i64 5", mismatch),
        ("i32 = 4", "`i32` is not `mut`"),
        ("mut x := f64 0.0, f64 = 2.5, x", "`f64` is not `mut`"),
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{src}: stderr: {stderr}");
        assert!(stderr.contains(want), "{src}: stderr: {stderr}");
    }
}

/// ›`-` negates where a space stands before it and none after, or no value stands to its left;
/// any other `-` subtracts‹ [[R157]]: negation and subtraction are two identities, told apart
/// where the `-` stands, and negation is built before the number type to its left, on both tiers.
#[test]
fn a_minus_negates_or_subtracts_by_where_it_stands() {
    let mut cases = vec![
        ("-5".to_string(), "-5\n"),
        ("i32 -5".to_string(), "-5\n"),
        ("m := i32 3, i32 -m".to_string(), "-3\n"),
        ("m := i32 3, (-m)".to_string(), "-3\n"),
        ("m := i32 3, - -m".to_string(), "3\n"),
        ("m := i32 3, 2 - -m".to_string(), "5\n"),
        ("m := f64 2.5, i32 lossy -m".to_string(), "-2\n"),
        ("mut x := i32 0, x=-1, x".to_string(), "-1\n"),
        (
            "import file «./identities/array.logos», a := array i32 [-1, 2], a[0]".to_string(),
            "-1\n",
        ),
    ];
    for sub in ["1-m", "1 - m", "1- m"] {
        cases.push((format!("m := i32 3, {sub}"), "-2\n"));
    }
    for (body, call, want) in [
        ("( i32 -m )", "f(3)", "-3\n"),
        ("( 1-m )", "f(3)", "-2\n"),
        ("( 1 - m )", "f(3)", "-2\n"),
        ("( 1- m )", "f(3)", "-2\n"),
        ("( (-m) )", "f(3)", "-3\n"),
        ("( - -m )", "f(3)", "3\n"),
        ("( -m * 3 )", "f(3)", "-9\n"),
        ("( -(m * 3) )", "f(3)", "-9\n"),
        ("( -5 + m )", "f(3)", "-2\n"),
        ("( mut x := i32 0, x=-m, x )", "f(3)", "-3\n"),
    ] {
        let f = format!("f := fn (m := i32 ?) -> i32 {body}");
        cases.push((format!("{f}, {call}"), want));
        cases.push((format!("{f}, f.compile(), {call}"), want));
    }
    let deref = "f := fn (q := @i32 ?) -> i32 ( q@-1 )";
    cases.push((format!("{deref}, x := i32 5, f(&x)"), "4\n"));
    cases.push((format!("{deref}, f.compile(), x := i32 5, f(&x)"), "4\n"));
    assert_runs(&cases);
    for (src, at) in [
        ("m := i32 3, 1 -m", "1:15:"),
        ("f := fn (m := i32 ?) -> i32 ( 1 -m ), f.compile(), f(3)", "1:33:"),
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{src}: stderr: {stderr}");
        let found = format!("{at} error: expected one expression, found more");
        assert!(stderr.contains(&found), "{src}: stderr: {stderr}");
    }
}

/// DESIGN ›`=` sits beside `:=`, and returns nothing‹: the one `run` frees the value it
/// displaces by its type's `free`, on both tiers.
#[test]
fn equals_frees_the_record_it_displaces() {
    let r = "r := type ( mut n := i32 ?, static free = ( print «freed {n}» ) )";
    let body = "mut x := r(1), x = r(2), x.n";
    assert_runs(&[
        (format!("{r}, mut x := r(1), x = r(2), print «{{x.n}}»"), "freed 1\n2\nfreed 2\n"),
        (format!("{r}, f := fn () -> i32 ( {body} ), f()"), "freed 1\nfreed 2\n2\n"),
        (format!("{r}, f := fn () -> i32 ( {body} ), f.compile(), f()"), "freed 1\nfreed 2\n2\n"),
    ]);
}

/// A record whose type fills `free` is refused through a pointer, never written over the value
/// whose `free` would then never run.
#[test]
fn a_held_record_through_a_pointer_is_refused() {
    let src = "r := type ( mut n := i32 ?, static free = ( print «freed {n}» ) ), w := r(2), \
               mut x := r(1), p := &x, p@ = w, 0";
    let (code, stdout, stderr) = run_line(src);
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{src}: stderr: {stderr}");
    assert!(stderr.contains("through a pointer is not in the seed yet"), "stderr: {stderr}");
}

/// DESIGN ›A scope lays out its declarations; a use reaches the offset through its binding‹: a
/// field takes the bytes a place of its type takes, sixteen for a `rational_number` and a
/// record's own size for a record, so writing one never reaches the next, and `size_bytes` is
/// their sum, packed.
#[test]
fn a_field_takes_the_width_a_place_of_its_type_takes() {
    let ab = "pt := type ( mut a := rational_number ?, mut b := i32 ? )";
    let get = "f := fn (p := pt ?) -> rational_number ( p.a )";
    let mut cases = vec![
        ("pt := type ( a := rational_number ?, b := i32 ? ), pt.size_bytes".to_string(), "20\n"),
        (
            "q := type ( x := i64 ?, y := i64 ? ), pt := type ( a := q ?, b := i32 ? ), pt.size_bytes"
                .to_string(),
            "20\n",
        ),
        (
            "pt := type ( c := i32 ?, a := rational_number ?, b := i32 ? ), pt.size_bytes".to_string(),
            "24\n",
        ),
        (
            "pt := type ( a := i8 ?, b := i64 ?, c := i16 ? ), p := pt(1, 2, 3), \
             print «{p.a} {p.b} {p.c} {pt.size_bytes}»"
                .to_string(),
            "1 2 3 11\n",
        ),
        (format!("{ab}, mut v := pt ?, v.b = 7, v.a = rational_number 1/2, v.b"), "7\n"),
        (format!("{ab}, mut v := pt ?, v.a = rational_number 1/2, v.b = 7, v.a"), "1/2\n"),
        (
            "pt := type ( mut b := i32 ?, mut a := rational_number ? ), mut v := pt ?, v.b = 7, \
             v.a = rational_number 1/2, v.b"
                .to_string(),
            "7\n",
        ),
        (
            "pt := type ( mut c := i32 ?, mut a := rational_number ?, mut b := i32 ? ), mut v := pt ?, \
             v.c = 3, v.b = 7, v.a = rational_number 1/2, print «{v.c} {v.b}»"
                .to_string(),
            "3 7\n",
        ),
        (
            "pt := type ( mut a := rational_number ? ), f := fn () -> i32 ( mut v := pt ?, \
             mut w := i32 9, v.a = rational_number 1/2, w ), f()"
                .to_string(),
            "9\n",
        ),
        (
            "pt := type ( mut a := rational_number ? ), f := fn () -> pt ( mut v := pt ?, \
             v.a = rational_number 1/2, v ), r := f(), r.a"
                .to_string(),
            "1/2\n",
        ),
        (
            "pt := type ( mut a := i64 ?, mut b := i64 ? ), o := type ( mut a := pt ?, mut z := i64 ? ), \
             mut v := o ?, v.z = 9, v.a = pt(1, 2), print «{v.a.a} {v.a.b} {v.z}»"
                .to_string(),
            "1 2 9\n",
        ),
        (
            "o := type ( mut a := rational_number ?, mut z := i64 ? ), mut v := o ?, v.z = 9, \
             v.a = rational_number 1/3, print «{v.a} {v.z}»"
                .to_string(),
            "1/3 9\n",
        ),
        (
            format!("{ab}, p := alloc 1 of pt ?, p@.b = 7, p@.a = rational_number 1/2, print «{{p@.a}} {{p@.b}}»"),
            "1/2 7\n",
        ),
        (
            "pt := type ( mut b := i32 ?, mut a := rational_number ? ), p := alloc 1 of pt ?, \
             p@.a = rational_number 1/2, p@.a"
                .to_string(),
            "1/2\n",
        ),
    ];
    for compile in ["", "f.compile(), "] {
        cases.push((
            format!(
                "{ab}, {get}, {compile}mut v := pt ?, v.b = 7, v.a = rational_number 1/2, f(v)"
            ),
            "1/2\n",
        ));
        cases.push((
            format!(
                "{ab}, f := fn (v := pt ?) -> i32 ( v.b ), {compile}mut v := pt ?, v.b = 7, \
                 v.a = rational_number 1/2, f(v)"
            ),
            "7\n",
        ));
        cases.push((
            format!(
                "{ab}, f := fn (p := @pt ?) -> rational_number ( p@.a ), {compile}mut v := pt ?, \
                 v.b = 7, v.a = rational_number 1/2, f(&v)"
            ),
            "1/2\n",
        ));
    }
    assert_runs(&cases);
}

/// DESIGN ›`X (…)` is one spelling, and X's constructor decides what the bracket is‹: a type's
/// constructor fills its holes positionally, each argument moved whole into its field at the
/// offset the field's binding carries, a rational, a type and a record too, on both tiers.
#[test]
fn a_constructor_fills_each_field_at_its_place() {
    let q = "q := type ( x := i32 ? )";
    let mut cases = vec![
        (
            "pt := type ( a := rational_number ?, b := i32 ? ), p := pt(rational_number 1/2, 7), \
             print «{p.a} {p.b}»"
                .to_string(),
            "1/2 7\n",
        ),
        ("pt := type ( t := type ? ), p := pt(i32), p.t".to_string(), "i32\n"),
        (
            format!("{q}, pt := type ( a := q ?, b := i32 ? ), p := pt(q(1), 3), print «{{p.a.x}} {{p.b}}»"),
            "1 3\n",
        ),
    ];
    for compile in ["", "f.compile(), "] {
        cases.push((
            format!(
                "pt := type ( b := i32 ?, a := rational_number ? ), \
                 f := fn (r := rational_number ?) -> rational_number ( p := pt(7, r), p.a ), \
                 {compile}f(rational_number 1/2)"
            ),
            "1/2\n",
        ));
        cases.push((
            format!(
                "{q}, pt := type ( a := q ?, b := i32 ? ), \
                 f := fn () -> i32 ( p := pt(q(4), 3), p.a.x + p.b ), {compile}f()"
            ),
            "7\n",
        ));
        cases.push((
            format!(
                "pt := type ( t := type ? ), f := fn () -> i32 ( p := pt(i32), 3 ), {compile}f()"
            ),
            "3\n",
        ));
    }
    assert_runs(&cases);
}

/// DESIGN ›There is no `this`‹: a `static` function over a plain record reads and writes each
/// field at the offset its binding carries, a rational, a type and a record as whole values.
#[test]
fn a_static_function_reaches_each_field_of_a_plain_record_whole() {
    let ab = "mut a := rational_number ?, mut b := i32 ?";
    let set = "mut v := pt ?, v.a = rational_number 1/2, v.b = 7";
    assert_runs(&[
        (
            format!("pt := type ( {ab}, static get := fn () -> rational_number ( a ) ), {set}, v.get()"),
            "1/2\n",
        ),
        (format!("pt := type ( {ab}, static getb := fn () -> i32 ( b ) ), {set}, v.getb()"), "7\n"),
        (
            format!(
                "pt := type ( {ab}, static set := fn () -> void ( a = rational_number 3/4 ) ), {set}, \
                 v.set(), print «{{v.a}} {{v.b}}»"
            ),
            "3/4 7\n",
        ),
        (
            format!(
                "pt := type ( {ab}, static both := fn (r := rational_number ?) -> void ( a = r, b = 3 ) ), \
                 {set}, v.both(rational_number 1/5), print «{{v.a}} {{v.b}}»"
            ),
            "1/5 3\n",
        ),
        (
            "pt := type ( mut a := rational_number ?, mut c := rational_number ?, \
             static cp := fn () -> void ( c = a ) ), mut v := pt ?, v.a = rational_number 1/2, \
             v.c = rational_number 1/3, v.cp(), print «{v.a} {v.c}»"
                .to_string(),
            "1/2 1/2\n",
        ),
        (
            "pt := type ( mut t := type ?, mut u := type ?, static swap := fn () -> void ( u = t ) ), \
             mut v := pt ?, v.t = i32, v.u = i64, v.swap(), v.u"
                .to_string(),
            "i32\n",
        ),
        // Packed, `t` lies at an odd offset.
        (
            "pt := type ( mut c := i8 ?, mut t := type ?, mut u := type ?, \
             static swap := fn () -> void ( u = t ) ), mut v := pt ?, v.c = 1, v.t = i32, v.u = i64, \
             v.swap(), print «{v.c} {v.u}»"
                .to_string(),
            "1 i32\n",
        ),
        (
            "q := type ( x := i32 ? ), pt := type ( mut a := q ?, mut b := i32 ?, \
             static seta := fn () -> void ( a = q(9) ) ), mut v := pt ?, v.a = q(1), v.b = 2, v.seta(), \
             print «{v.a.x} {v.b}»"
                .to_string(),
            "9 2\n",
        ),
        (
            format!(
                "pt := type ( {ab}, static get := fn () -> rational_number ( a ) ), \
                 f := fn () -> rational_number ( mut v := pt ?, v.a = rational_number 1/3, v.get() ), f()"
            ),
            "1/3\n",
        ),
        // A node a Logos `parse` built keeps a rational field as its operand node.
        (
            "pt := type ( a := rational_number ?, output_type := type ?, static run = ( a ), \
             static parse_rank = *.parse_rank + 1, static parse = ( tape[0].type = pt, \
             tape[0].a = rational_number 1/2, tape[0].output_type = rational_number, \
             tape.is_constructed[0] = true ) ), pt"
                .to_string(),
            "1/2\n",
        ),
    ]);
}

#[test]
fn the_repl_writes_and_copies_whole_records() {
    let (echoes, stderr) = repl(
        "q := type ( mut x := i32 ? )\nmut v := q ?\nv = q(3)\nprint «{v.x}»\nw := q(4)\nmut u := w\n\
          u.x = 9\nprint «{w.x}»\npt := type ( mut a := i64 ?, mut b := i32 ? )\np := alloc 1 of pt ?\n\
          p@.b = 7\nprint «{p@.b}»\nr := &v\nr@ = q(5)\nv.x\n",
    );
    assert_eq!(echoes, ["3", "4", "7", "5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_name_used_inside_its_own_declaration_is_a_checked_error() {
    // The binding exists from the `:=` on; the node comes with the value.
    let out = logos().arg("x := x + 1").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains("`x` is used inside its own declaration, before its value is built"),
        "stderr: {stderr}"
    );
    // Overflowed the stack when the name copied the scope it stood in.
    let out = logos().arg("s := ( a := 1, s ), s").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(stderr.contains("`s` is used inside its own declaration"), "stderr: {stderr}");
    // A `fn` points the binding at its node before the body parses.
    let out = logos()
        .arg("f := fn (n := i32 ?) -> i32 ( if n == 0 (1) else (n * f(n - 1)) ), f(4)")
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "24\n");
    // A name declared again after `free` gets a new binding.
    let out = logos().arg("x := 5, free x, x := 6, x").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n");
}

#[test]
fn a_node_gives_back_the_type_its_parse_wrote_on_both_tiers() {
    for (src, want) in [
        ("x := i32 1, a := x == 1, a", "true\n"),
        ("x := i32 1, a := x == 1, b := true, a and b", "true\n"),
        ("f := fn () -> bool ( true ), b := f(), print «{b}», b.type", "true\nbool\n"),
        ("f := fn () -> bool ( true ), b := f(), if b ( i32 1 ) else ( i32 2 )", "1\n"),
        ("g := fn (a := i32 ?) -> bool ( a < 1 ), not g(0)", "false\n"),
        ("true == true", "true\n"),
        ("x := i32 3, print «{(&x).output_type == @i32}», p := &x, p.type == @i32", "true\ntrue\n"),
        (
            "mut c := true, a := alloc 1 of i32 5, b := alloc 1 of i32 6, \
             x := if c (move a) else (move b), y := if c (alloc 1 of i32 7) else (alloc 1 of i32 8), \
             print «{x@} {y@}», free x, free y",
            "5 7\n",
        ),
        ("p := alloc 1 of i32 0, f := fn (q := @i32 ?) -> @i32 ( q ), f(p).type == @i32", "true\n"),
        (
            "mk := fn () -> own @i32 ( alloc 1 of i32 7 ), x := mk(), print «{x@}», free x",
            "7\n",
        ),
        // A list standing as a value gives back itself, never nothing.
        ("t := ([1, 2]).type, t == square_brackets", "true\n"),
        (
            "f := fn (c := i32 ?) -> bool ( a := c == 1, mut b := not a, b = not b, b ), \
             f.compile(), print «{f(1)} {f(2)}»",
            "true false\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, want, "{src}");
    }
    // A comparison gives back a `bool`, which is no number and no `i32`.
    for (src, error) in [
        ("x := i32 1, (x == 1) + 1", "this operator cannot compute over these operands"),
        ("x := i32 1, mut a := i32 0, a = x == 1, a", "these types do not match"),
        ("x := i32 1, mut a := x == 1, a = i32 1, a", "these types do not match"),
        // A `return` gives its own line nothing.
        ("f := fn (x := f64 ?) -> f64 ( (return x) + 1.0 ), f(2.5)", "gives nothing"),
        ("f := fn (x := f64 ?) -> f64 ( y := (return x), y ), f(2.5)", "gives nothing"),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}");
        assert!(stderr.contains(error), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_bool_hole_is_a_bool_place_on_both_tiers() {
    for (src, want) in [
        ("mut a := bool ?, a = true, a", "true\n"),
        (
            "f := fn (x := i32 ?) -> bool ( mut a := bool ?, a = x == 1, a ), \
             f.compile(), print «{f(1)} {f(2)}»",
            "true false\n",
        ),
        ("pt := type ( ok := bool ?, n := i32 ? ), p := pt(true, 3), p.ok", "true\n"),
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, want, "{src}");
    }
    for (src, error) in [
        ("a := bool ?, a", "`a` is read before it is written"),
        ("mut a := bool ?, a = i32 1, a", "these types do not match"),
        // A plain number is no `bool`, though a `bool` is stored as an `i32`.
        ("mut a := bool ?, a = 1, a", "no exact value in the type it lands in"),
        ("mut a := true, a = 2, a", "no exact value in the type it lands in"),
        ("pt := type ( ok := bool ?, n := i32 ? ), p := pt(7, 3), p.ok", "no exact value"),
        ("f := fn (b := bool ?) -> i32 ( if b (1) else (2) ), f(2)", "no exact value"),
        ("b := alloc 1 of bool ?, b@ = 7, b@", "no exact value"),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}");
        assert!(stderr.contains(error), "{src}: stderr: {stderr}");
    }
}

#[test]
fn free_and_move_read_the_type_a_dereference_gives_back() {
    let r = "r := type ( mut p := own @i32 ?, static free = ( free p ) )";
    let q = "q := alloc 1 of own @i32 ?, a := alloc 1 of i32 5, q@ = move a";
    let x = "mut x := r ?, a := alloc 1 of i32 5, x.p = move a";
    let src = format!("{q}, free q@, q@@");
    let (code, _, stderr) = run_line(&src);
    assert_eq!(code, Some(1), "{src}");
    assert!(stderr.contains("this pointer holds nothing yet"), "{src}: stderr: {stderr}");
    // A field through a pointer is ended on its owner, `move x.p`.
    for src in [
        format!("{r}, {x}, pp := &x, free pp@.p, x.p@"),
        format!("{r}, f := fn (pp := @r ?) -> i32 ( free pp@.p, 1 ), {x}, f(&x), x.p@"),
        format!("{r}, {x}, pp := &x, b := move pp@.p, b@"),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}");
        assert!(stderr.contains("reached through a pointer"), "{src}: stderr: {stderr}");
    }
    let (code, _, stderr) = run_line(&format!("{r}, {x}, b := move x.p, b@"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("the type of `x` fills its own `static free"), "stderr: {stderr}");
    for (src, want) in
        [(format!("{q}, b := move q@, b@"), "5\n"), ("x := 5, move x".to_string(), "5\n")]
    {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, want, "{src}");
    }
}

#[test]
fn a_generic_body_is_keyed_by_the_type_its_operand_gives_back() {
    let isbool = postfix_chooser("isbool", "i32", "if (a.type == bool) (i32 1) else (i32 2)");
    let pass = postfix_chooser("pass", "t", "a");
    let at = "at := fn (q := @i32 ?) -> @i32 ( q )";
    let pt = "pt := type (u := i32 ?, v := i32 ?)";
    for (src, want) in [
        (
            format!(
                "{isbool}, big := fn (v := i32 ?) -> bool ( v > 1 ), n := i32 5, big(n) isbool"
            ),
            "1\n",
        ),
        (format!("{isbool}, (i32 1 < i32 2) isbool"), "1\n"),
        (
            format!(
                "{isbool}, g := fn (n := i32 ?) -> i32 ( (n < i32 2) isbool ), g.compile(), g(1)"
            ),
            "1\n",
        ),
        (format!("{pass}, x := i32 5, (x < i32 7) pass"), "true\n"),
        (format!("{pass}, f := fn () -> bool ( true pass ), f.compile(), f()"), "true\n"),
        (
            format!(
                "{pass}, u := u8 7, x := i32 5, y := f64 2.5, \
                 print «{{u pass}} {{x pass pass}} {{y pass}} {{42 pass}}»"
            ),
            "7 5 2.5 42\n",
        ),
        (format!("{pass}, n := i32 5, p := &n, q := p pass, q@"), "5\n"),
        (
            format!(
                "{pass}, f := fn (q := @i32 ?) -> @i32 ( q pass ), f.compile(), n := i32 5, f(&n)@"
            ),
            "5\n",
        ),
        (format!("{pass}, {at}, n := i32 5, (at(&n) pass)@"), "5\n"),
        (
            format!(
                "{pass}, {at}, f := fn (q := @i32 ?) -> i32 ( (at(q) pass)@ ), f.compile(), \
                 n := i32 5, f(&n)"
            ),
            "5\n",
        ),
        (format!("{pass}, {pt}, r := pt(3, 4), q := r pass, print «{{q.u}} {{q.v}}»"), "3 4\n"),
        (format!("{pass}, {pt}, q := pt(3, 4) pass, q.v"), "4\n"),
        (
            format!(
                "{pass}, {pt}, g := fn (r := pt ?) -> i32 ( q := r pass, q.v ), g.compile(), \
                 g(pt(3, 4))"
            ),
            "4\n",
        ),
        (format!("{pass}, mut t := type ?, t = i32, q := t pass, q"), "i32\n"),
        (format!("{pass}, ty := fn () -> type ( i32 ), (ty() pass) == i32"), "true\n"),
        (format!("{pass}, (i32 pass) == i32"), "true\n"),
        (format!("{pass}, g := fn (u := type ?) -> type ( u pass ), g.compile(), g(i32)"), "i32\n"),
        (
            format!("{pass}, g := fn (b := bool ?) -> bool ( b pass ), g.compile(), g(true)"),
            "true\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, want, "{src}");
    }
    // An operand that gives nothing gets a mint that gives nothing, refused where a value is needed.
    let src = format!("{pass}, mut x := i32 5, q := (x = i32 3) pass, q");
    let (code, _, stderr) = run_line(&src);
    assert_eq!(code, Some(1), "{src}: {stderr}");
    assert!(stderr.contains("this gives nothing"), "{src}: {stderr}");
}

#[test]
fn a_type_field_is_a_box_its_run_reads_when_it_runs() {
    let parse = |name: &str, fill: &str| {
        format!(
            "static parse_rank = 60, static parse = ( tape[0].type = {name}, {fill}, \
             tape.is_constructed[0] = true, tape.remove(1) )"
        )
    };
    let pass = format!(
        "pass := type ( a := type ?, static output_type := type, static run = ( a ), {} )",
        parse("pass", "tape[0].a = tape[1]")
    );
    let mk = format!(
        "mk := type ( a := type ?, static output_type := i64, \
         static run = ( x := a 7, i64 (x) ), {} )",
        parse("mk", "tape[0].a = tape[1]")
    );
    let sq = format!(
        "sq := type ( a := i32 ?, output_type := type ?, static run = ( a * a ), {} )",
        parse("sq", "tape[0].a = i32 4, tape[0].output_type = tape[1]")
    );
    for (src, want) in [
        (format!("{pass}, (pass i32) == i32"), "true\n"),
        (format!("{pass}, mut t := type ?, t = i32, (pass t) == i32"), "true\n"),
        (format!("{pass}, f := fn (u := type ?) -> type ( pass u ), f(i32)"), "i32\n"),
        (format!("{pass}, f := fn (u := type ?) -> type ( pass u ), f.compile(), f(i32)"), "i32\n"),
        (format!("{pass}, mut d := dyad ?, d = i32, (pass d) == i32"), "true\n"),
        (format!("{pass}, f := fn (d := dyad ?) -> bool ( (pass d) == f64 ), f(f64)"), "true\n"),
        (format!("{sq}, mut t := type ?, t = i64, sq t"), "16\n"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, want, "{src}");
    }
    let at_run = "which type this is is known only when the program runs, and this needs it now";
    for (src, error) in [
        (format!("{pass}, pass (i32 5)"), "the declared or assigned type must be a type value"),
        (format!("{mk}, mk i32"), at_run),
        (format!("{sq}, f := fn (u := type ?) -> i32 ( sq u ), f(i32)"), at_run),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}");
        assert!(stderr.contains(error) && !stderr.contains("run error"), "{src}: {stderr}");
    }
}

#[test]
fn an_input_with_nothing_in_it_has_no_value_on_the_command_line_and_in_a_file() {
    for src in [
        "# hi",
        "# «x»",
        "# a\n# b",
        "",
        "   ",
        "import file «tests/fixtures/only_prose.logos»",
        "import file «tests/fixtures/empty.logos»",
    ] {
        assert_eq!(run_line(src), (Some(0), String::new(), String::new()), "{src:?}");
    }
    // The same lines read as a block, after other lines, and before a value.
    for (src, want) in [
        ("( # note\n), 7", "7"),
        ("x := 1,\n# hi", ""),
        ("1, # done", "1"),
        ("( y := i32 2, y + 1 )", "3"),
        ("x := 1, y := 2, x + y", "3"),
        ("import file «tests/fixtures/only_prose.logos», 1", "1"),
        ("import file «tests/fixtures/empty.logos», 1", "1"),
    ] {
        assert_eq!(line(src), want, "{src:?}");
    }
}

#[test]
fn a_closer_no_bracket_opened_is_one_error_naming_it() {
    for (src, message) in [
        ("1 )", "<command line>:1:3: error: this `)` closes nothing: no bracket is open here"),
        ("1 ], 5", "<command line>:1:3: error: this `]` closes nothing: no bracket is open here"),
        (")", "<command line>:1:1: error: this `)` closes nothing: no bracket is open here"),
        (
            "import file «tests/fixtures/stray_closer.logos», 2",
            "stray_closer.logos:1:3: error: this `)` closes nothing: no bracket is open here",
        ),
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stdout: {stdout}");
        assert!(stderr.contains(message), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_repl_line_is_a_line_of_the_program() {
    let (echoes, stderr) = repl(
        "# hello\n\
          # «x»\n\
          \n   \n\
          x := 1, y := 2\n\
          print «{x + y}»\n\
          print «{1}», # done\n\
          z := i32 1,\n\
          print «{z}»\n\
          import file «tests/fixtures/only_prose.logos», print «{7}»\n\
          w := i32 1, print «{5}»\n\
          free (6), v := i32 1\n\
          v\n",
    );
    assert_eq!(echoes, ["3", "1", "1", "7", "5", "1"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"1 )\n)\n2\n");
    assert_eq!(echoes, ["2"], "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:3: error: this `)` closes nothing"), "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:1: error: this `)` closes nothing"), "stderr: {stderr}");
}

/// ›The command line is Logos source; the binary stays out of the way‹ [[R302]]: a line that
/// gives a value shows it and ends the session, as a program ends with its value.
#[test]
fn a_repl_line_that_gives_a_value_ends_the_session() {
    for (input, shown) in [
        ("x := i32 5\nx + 1\nprint «after»\n", "6"),
        ("x := i32 1, 5\nprint «after»\n", "5"),
        ("a := true\nif a (i32 1)\nprint «after»\n", "1"),
        ("f := fn () -> i32 ( 2 )\nf()\nf()\n", "2"),
    ] {
        let (echoes, stderr) = repl(input);
        assert_eq!(echoes, [shown], "{input}: stderr: {stderr}");
    }
    // A line that gives nothing, an empty line and a failed line go on.
    let (echoes, stderr) = repl(
        "x := i32 5\nprint «{x + 1}»\n\n   \nzz\nx := i32 6\na := false\nif a (i32 1)\n\
         free (x + 1)\nx + 2\nprint «after»\n",
    );
    assert_eq!(echoes, ["6", "7"], "stderr: {stderr}");
    assert!(stderr.contains("unknown name `zz`"), "stderr: {stderr}");
    assert!(stderr.contains("`x` is already declared"), "stderr: {stderr}");
}

#[test]
fn a_failed_repl_line_leaves_no_trace() {
    // Neither declaration of a line that fails is known after it.
    let (echoes, stderr) = repl(b"x := i32 1, y := zz\nx\ny\nx := i32 3, y := i32 4\nx + y\n");
    assert_eq!(echoes, ["7"], "stderr: {stderr}");
    assert!(stderr.contains("unknown name `zz`"), "stderr: {stderr}");
    assert!(stderr.contains("unknown name `x`"), "stderr: {stderr}");
    assert!(stderr.contains("unknown name `y`"), "stderr: {stderr}");
    // What the line's own lines hold ends at its fault; what an earlier line holds ends only
    // where the failing line reached its `free`.
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) )";
    let (echoes, stderr) = repl(
        format!("{bag}\n")
            + "c := bag(7), error «boom»\nc\nk := bag(1)\nfree k, error «x»\n\
               m := bag(2)\nerror «y», free m\nprint «{m.n}»\n",
    );
    assert_eq!(echoes, ["freed 7", "freed 1", "2", "freed 2"], "stderr: {stderr}");
    assert!(stderr.contains("<repl>: run error: boom"), "stderr: {stderr}");
    assert!(stderr.contains("unknown name `c`"), "stderr: {stderr}");
    // Its lines leave the session's scope.
    let (echoes, stderr) = repl(b"a := i32 1\nb := i32 2, zz\nhere.scope.dyads.size\n");
    assert_eq!(echoes, ["1"], "stderr: {stderr}");
}

#[test]
fn a_file_imported_on_a_failed_repl_line_stays_loaded_for_the_session() {
    let held = "import file «./tests/fixtures/held.logos»";
    // Neither a fault, a failed parse after the import, nor a `free` the line parsed and never
    // ran ends the file: the session does, once.
    for failing in [
        format!("{held}, error «boom»"),
        format!("{held}, zz"),
        format!("{held}, error «boom», free h"),
        format!("{held}, free h, zz"),
    ] {
        let (echoes, stderr) = repl(format!("{failing}\n{held}\n") + "print «{h.n}»\n");
        assert_eq!(echoes, ["9", "freed 9"], "{failing}: stderr: {stderr}");
    }
    // A `free` the line ran before its fault ended the value there.
    let (echoes, stderr) = repl(format!("{held}, free h, error «boom»\n1\n").as_bytes());
    assert_eq!(echoes, ["freed 9", "1"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        "import file «./tests/fixtures/owning_pub.logos», error «boom»\n\
          import file «./tests/fixtures/owning_pub.logos», p@\n",
    );
    assert_eq!(echoes, ["7"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        "import file «./tests/fixtures/counter.logos», error «boom»\n\
          import file «./tests/fixtures/counter.logos»\nc@\n",
    );
    assert_eq!(echoes, ["1"], "stderr: {stderr}");
    // The line's own holds still end at its fault, and the names it published roll back.
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) )";
    let line_holds = format!("c := bag(1), {held}, d := bag(2), error «boom»");
    let (echoes, stderr) = repl(format!("{bag}\n{line_holds}\nh.n\n").as_bytes());
    assert_eq!(echoes, ["freed 2", "freed 1", "freed 9"], "stderr: {stderr}");
    assert!(stderr.contains("`h` is not in scope here"), "stderr: {stderr}");
    // On the command line the run ends at the fault, and the file with it.
    let (code, stdout, _) = run_line("import file «./tests/fixtures/deferring.logos», error «x»");
    assert_eq!((code, stdout.as_str()), (Some(1), "file ends\n"));
}

#[test]
fn a_quote_s_braces_are_a_scope_whose_text_is_its_tail() {
    for (src, printed) in [
        ("print «a{ y := i32 2, y + 1 }b»", "a3b\n"),
        ("print «a{ # c\n}b»", "ab\n"),
        ("print «a{ # «c» }b»", "ab\n"),
        ("print «a{}b»", "ab\n"),
        ("print «{free (1), 2}»", "2\n"),
        ("print «{}»", "\n"),
        ("f := fn () -> void ( print «a{ y := i32 2, y + 1 }b» ), f()", "a3b\n"),
        ("f := fn () -> void ( print «a{ # c\n}b» ), f()", "ab\n"),
        ("f := fn () -> void ( print «a{ # «c» }b», print «a{}b» ), f()", "ab\nab\n"),
        // A comment in the braces is a line of their scope, not of the one around them.
        ("print «{5 # «c» * 2}», here.scope.dyads.size", "10\n1\n"),
        ("( print «{5 # «c» * 2}», here.scope.dyads.size )", "10\n1\n"),
    ] {
        assert_eq!(run_line(src), (Some(0), printed.to_string(), String::new()), "{src:?}");
    }
    for (src, message) in [("error «a{ y := 2, y }b»", "a2b"), ("error «a{ # «c» }b»", "ab")]
    {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}");
        assert!(stderr.contains(&format!("run error: {message}")), "{src}: stderr: {stderr}");
    }
    // A tail that gives nothing is no tail missing: a value is read there.
    for src in ["print «a{ x := i32 1 }b»", "print «a{ y := i32 1, print «in» }b»"] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}");
        assert!(stderr.contains("gives nothing, and a value is needed here"), "{src}: {stderr}");
    }
    // Its names are its own, and a closer in it closes nothing.
    let (_, _, stderr) = run_line("print «a{ y := i32 2, y }b», y");
    assert!(stderr.contains("`y` is not in scope here"), "stderr: {stderr}");
    let (_, _, stderr) = run_line("print «{ 1 ) }»");
    assert!(stderr.contains("1:12: error: this `)` closes nothing"), "stderr: {stderr}");
}

#[test]
fn an_array_element_lands_by_the_element_type() {
    let array = "import file «./identities/array.logos»";
    for (src, printed) in [
        (format!("{array}, a := array bool [true, false], a[1]"), "false\n"),
        (format!("{array}, a := array (array i32) [[1, 2], [3]], a[0][1]"), "2\n"),
    ] {
        assert_eq!(run_line(&src), (Some(0), printed.to_string(), String::new()), "{src}");
    }
    // Refused, not a crash.
    let src = format!("{array}, pt := type ( a := i32 ?, b := i32 ? ), a := array pt [pt(1, 2)]");
    let (code, _, stderr) = run_line(&src);
    assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
    assert!(stderr.contains("these types do not match"), "{src}: stderr: {stderr}");
}

#[test]
fn a_curly_bracket_keeps_its_scope_as_a_value_whose_names_are_read_with_a_dot() {
    for compile in ["", "f.compile(), "] {
        for (body, printed) in [
            ("m := { a := i32 1, b := i32 2 }, m.a + m.b", "3"),
            ("m := {a := i32 1}, m.a", "1"),
            ("m := { a := i32 1, b := a + 1 }, m.b", "2"),
            ("mut m := { mut a := i32 1, b := i32 2 }, m.a = 9, m.a * m.b", "18"),
            ("m := { inner := { x := i32 7 } }, m.inner.x", "7"),
            ("m := { a := i32 3, s := «hi» }, m.a", "3"),
            ("m := { a := k, b := k * 2 }, m.a + m.b", "15"),
            ("m := { a := i32 1 }, n := m, n.a", "1"),
            ("m := { }, k", "5"),
        ] {
            let src = format!("f := fn (k := i32 ?) -> i32 ( {body} ), {compile}f(5)");
            assert_eq!(run_line(&src), (Some(0), format!("{printed}\n"), String::new()), "{src}");
        }
    }
    for (src, printed) in [
        ("m := { s := «hi», n := 3 }, print «{m.s} {m.n}»", "hi 3"),
        ("m := { a := 1/3 }, m.a + 1/6", "1/2"),
        ("m := { f := fn (x := i32 ?) -> i32 ( x + 1 ) }, m.f(3)", "4"),
        ("m := { x := i32 1, free x, x := i32 2 }, m.x", "2"),
        ("mut m := { mut a := i32 1 }, n := m, m.a = 7, n.a", "1"),
        ("print «a {1 + 2} b»", "a 3 b"),
    ] {
        assert_eq!(run_line(src), (Some(0), format!("{printed}\n"), String::new()), "{src}");
    }
    let (echoes, stderr) = repl(b"m := { a := i32 1, b := i32 2 }\nm.a + m.b\n");
    assert_eq!(echoes, ["3"], "stderr: {stderr}");
    for (src, expect) in [
        ("mut m := { a := i32 1 }, m.a = 5", "`a` is not `mut`"),
        ("m := { mut a := i32 1 }, m.a = 5", "`m` is not `mut`"),
        ("m := { a := i32 1 }, a", "`a` is not in scope here"),
        ("m := { a := i32 1 ), m.a", "1:19: error: this bracket is never closed"),
        ("x := i32 1 }", "1:12: error: this `}` closes nothing"),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_curly_bracket_value_ends_its_entries_last_declared_first() {
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) ), ";
    let run = |src: String, want: &str| {
        let out = logos().arg(&src).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{src}: stderr: {stderr}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), want, "{src}");
    };
    let same = |want: &'static str| (want, want);
    for c in ["", "f.compile(), "] {
        for (body, (then, otherwise)) in [
            (
                "m := { b := bag (1), c := bag (2), a := i32 7 }, print «end {m.b.n} {m.c.n} {m.a}»",
                same("end 1 2 7\nfreed 2\nfreed 1\n"),
            ),
            (
                "m := { b := bag (1), n := { c := bag (2), d := bag (3) }, e := bag (4) }, print «end»",
                same("end\nfreed 4\nfreed 3\nfreed 2\nfreed 1\n"),
            ),
            ("m := { e := dyad (bag, bag (6)), b := bag (1) }, print «end»", same("end\nfreed 1\nfreed 6\n")),
            ("m := { b := bag (1) }, k := m, print «end»", same("end\nfreed 1\nfreed 1\n")),
            ("m := { e := dyad (bag, bag (6)) }, k := m, print «end»", same("end\nfreed 6\nfreed 6\n")),
            ("m := { b := bag (1) }, k := move m, print «end»", same("end\nfreed 1\n")),
            ("m := { b := bag (1), c := bag (2) }, free m, print «end»", same("freed 2\nfreed 1\nend\n")),
            ("free ({ b := bag (1), c := bag (2) }), print «end»", same("freed 2\nfreed 1\nend\n")),
            // ›`move` and `free` take a field path too‹: the value's end ends the other entries.
            (
                "m := { b := bag (1), c := bag (2) }, free m.b, print «end {m.c.n}»",
                same("freed 1\nend 2\nfreed 2\n"),
            ),
            (
                "m := { b := bag (1), c := bag (2) }, k := move m.b, print «end {k.n}»",
                same("end 1\nfreed 1\nfreed 2\n"),
            ),
            (
                "m := { b := bag (1), n := { c := bag (2), d := bag (3) } }, free m.n.c, print «end»",
                same("freed 2\nend\nfreed 3\nfreed 1\n"),
            ),
            (
                "m := { e := dyad (bag, bag (6)), b := bag (1) }, free m.e, print «end»",
                same("freed 6\nend\nfreed 1\n"),
            ),
            ("mut m := { mut b := bag (1) }, m.b = bag (3), print «end»", same("freed 1\nend\nfreed 3\n")),
            (
                "m := { b := bag (1), c := bag (2) }, if t ( free m.b ) else ( print «kept» ), print «end»",
                ("freed 1\nend\nfreed 2\n", "kept\nfreed 1\nend\nfreed 2\n"),
            ),
            (
                "m := { b := bag (1), c := bag (2) }, if t ( return 9 ), free m.b, print «end»",
                ("freed 2\nfreed 1\n", "freed 1\nend\nfreed 2\n"),
            ),
            (
                "m := { b := bag (1), c := bag (2) }, k := move m.c, if t ( return 9 ), print «end»",
                ("freed 2\nfreed 1\n", "end\nfreed 2\nfreed 1\n"),
            ),
        ] {
            let src = format!(
                "{bag}f := fn (t := bool ?) -> i32 ( {body}, 0 ), {c}x := f(true), print «-», y := f(false)"
            );
            run(src, &format!("{then}-\n{otherwise}"));
        }
        // A fault inside the bracket ends the entries made before it.
        let src = format!(
            "{bag}g := fn () -> i32 ( error «stop» ), \
             f := fn () -> i32 ( m := {{ b := bag (1), x := g(), c := bag (2) }}, 0 ), {c}f()"
        );
        let out = logos().arg(&src).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{src}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "freed 1\n", "{src}");
    }
    run(format!("{bag}m := {{ b := bag (1) }}, print «end»"), "end\nfreed 1\n");
    run(
        "m := { p := alloc 1 of i32 5, a := i32 1 }, q := move m.p, print «end {q@}»".into(),
        "end 5\n",
    );
    let head = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) )\n";
    for (lines, want) in [
        (
            "m := { b := bag (1), c := bag (2) }\nk := bag (7)\nfree m.b\nprint «mid»\n",
            ["freed 1", "mid", "freed 7", "freed 2"].as_slice(),
        ),
        (
            "m := { b := bag (1), c := bag (2) }\nk := move m.b\nprint «mid»\n",
            &["mid", "freed 1", "freed 2"],
        ),
        (
            "m := { b := bag (1), n := { c := bag (2), d := bag (3) } }\nfree m.n\nprint «mid»\n",
            &["freed 3", "freed 2", "mid", "freed 1"],
        ),
        (
            "m := { b := bag (1), n := { c := bag (2), d := bag (3) } }\nfree m.n.c\nfree m.b\n",
            &["freed 2", "freed 1", "freed 3"],
        ),
    ] {
        let (echoes, stderr) = repl(format!("{head}{lines}").as_bytes());
        assert_eq!(echoes, want, "{lines}: stderr: {stderr}");
    }
}

/// ›A type body describes one level‹ [[R222]]: a field named bare is read as the field inside a
/// `static` initializer too, which checks only names for a value made once.
#[test]
fn a_field_named_bare_inside_a_static_initializer_is_read() {
    let src = "static w := ( t := type ( x := i32 ?, static f := fn () -> i32 ( x + 1 ) ), \
               u := t (4), u.f() ), w";
    assert_eq!(run_line(src), (Some(0), "5\n".to_string(), String::new()), "{src}");
}

/// ›`{ … }` outside text is a scope kept as a value, read with `.`‹ [[R374]]: a `fn` written in it
/// reads its sibling names through the value it is reached by, as a `static` function reads its
/// value's fields.
#[test]
fn a_fn_entry_of_a_curly_bracket_reads_its_sibling_names_through_the_value() {
    for compile in ["", "f.compile(), "] {
        for (body, printed) in [
            ("m := { a := i32 1, get := fn () -> i32 ( a + 1 ) }, m.get()", "2"),
            ("m := { a := i32 2, b := i32 3, get := fn (c := i32 ?) -> i32 ( a * b + c ) }, m.get(k)", "11"),
            ("m := { a := k, get := fn () -> i32 ( a * 3 ) }, m.get()", "15"),
            ("m := { a := i32 5, twice := fn () -> i32 ( a * 2 ), get := fn () -> i32 ( twice() + 1 ) }, m.get()", "11"),
            ("m := { a := i32 2, sum := fn (x := i32 ?) -> i32 ( if x == 0 ( a ) else ( sum(x - 1) + a ) ) }, m.sum(3)", "8"),
            ("x := i32 4, m := { p := &x, get := fn () -> i32 ( p@ ) }, m.get()", "4"),
            ("mut m := { mut a := k, inc := fn () -> void ( a = a + 1 ) }, m.inc(), m.inc(), m.a", "7"),
            ("m := { a := i32 1, get := fn () -> i32 ( a + 1 ) }, n := m, p := &n, n.get() + p@.get()", "4"),
            ("m := { a := i32 1, seven := fn () -> i32 ( 7 ), b := seven() }, m.b", "7"),
        ] {
            let src = format!("f := fn (k := i32 ?) -> i32 ( {body} ), {compile}f(5)");
            assert_eq!(run_line(&src), (Some(0), format!("{printed}\n"), String::new()), "{src}");
        }
    }
    let bag = "bag := type ( mut n := i32 ?, static free = ( print «freed {n}» ) ), ";
    for (src, printed) in [
        ("m := { a := i32 1, f := fn () -> i32 ( a + 1 ) }, m.f()".to_string(), "2\n"),
        ("m := { a := i32 1, f := fn () -> i32 ( a + 1 ) }, m.f.compile(), m.f()".into(), "2\n"),
        ("m := { x := f64 2.5, f := fn () -> f64 ( x * 2 ) }, m.f.compile(), m.f()".into(), "5.0\n"),
        ("m := { a := i64 1, f := fn (d := dyad ?) -> i64 ( d + a ) }, m.f(i64 5)".into(), "6\n"),
        (format!("{bag}m := {{ b := bag (4), a := i32 3, f := fn () -> i32 ( a ) }}, print «{{m.f()}}»"), "3\nfreed 4\n"),
        ("static m := { a := i32 1, f := fn () -> i32 ( a + 1 ) }, m.f()".into(), "2\n"),
    ] {
        assert_eq!(run_line(&src), (Some(0), printed.to_string(), String::new()), "{src}");
    }
    // Before its `}`, a bare call of an entry reads the names from the scope being built.
    for compile in ["", "f.compile(), "] {
        for (body, printed) in [
            ("a := i32 1, f := fn () -> i32 ( a + 1 ), {compile}b := f() }, m.b", "2"),
            ("x := f64 2.5, f := fn () -> f64 ( x * 2 ), {compile}y := f() }, m.y", "5.0"),
            ("a := i32 5, g := fn () -> i32 ( a * 2 ), f := fn () -> i32 ( g() + 1 ), {compile}b := f() }, m.b", "11"),
            ("a := i32 2, f := fn (x := i32 ?) -> i32 ( if x == 0 ( a ) else ( f(x - 1) + a ) ), {compile}b := f(3) }, m.b", "8"),
            ("p := alloc 1 of i32 7, f := fn () -> i32 ( p@ ), {compile}b := f() }, m.b", "7"),
            ("a := i32 1, f := fn () -> i32 ( a + 1 ), {compile}b := f(), t := type ( x := i32 ? ) }, m.b", "2"),
            ("a := i32 1, f := fn () -> i32 ( a + 1 ), {compile}b := f(), static s := b * 10 }, m.s", "20"),
            ("a := i32 1, f := fn () -> i32 ( a + 1 ), {compile}b := f() }, m.f() + m.b", "4"),
        ] {
            let src = format!("m := {{ {}", body.replace("{compile}", compile));
            assert_eq!(run_line(&src), (Some(0), format!("{printed}\n"), String::new()), "{src}");
        }
    }
    for (src, printed) in [
        ("m := { mut a := i32 1, inc := fn () -> void ( a = a + 1 ), inc(), inc() }, m.a".to_string(), "3\n"),
        ("static m := { a := i32 1, f := fn () -> i32 ( a + 1 ), b := f() }, m.b".into(), "2\n"),
        ("m := { a := i32 1, f := fn () -> i32 ( a + 1 ), print «{f()}» }, m.a".into(), "2\n1\n"),
        (format!("{bag}m := {{ b := bag (4), a := i32 3, f := fn () -> i32 ( a ), c := f() }}, print «{{m.c}}»"), "3\nfreed 4\n"),
    ] {
        assert_eq!(run_line(&src), (Some(0), printed.to_string(), String::new()), "{src}");
    }
    let (echoes, stderr) = repl(b"m := { a := i32 1, f := fn () -> i32 ( a + 1 ) }\nm.f()\n");
    assert_eq!(echoes, ["2"], "stderr: {stderr}");
    let (echoes, stderr) =
        repl(b"m := { a := i32 1, f := fn () -> i32 ( a + 1 ), b := f() }\nm.b\n");
    assert_eq!(echoes, ["2"], "stderr: {stderr}");
    for (src, expect) in [
        (
            "m := { mut a := i32 1, inc := fn () -> void ( a = a + 1 ) }, m.inc()",
            "`m` is not `mut`",
        ),
        (
            "mut m := { a := i32 1, inc := fn () -> void ( a = a + 1 ) }, m.inc()",
            "`a` is not `mut`",
        ),
        // A call that reaches no value has none to read: by another name, or from an entry of
        // another `{ … }`, which may run once the names are gone.
        ("m := { a := i32 1, f := fn () -> i32 ( a + 1 ) }, h := m.f, h()", "`h` works on a value"),
        (
            "m := { a := i32 7, f := fn () -> i32 ( a ), n := { g := fn () -> i32 ( f() ) } }, m.n.g()",
            "`f` works on a value",
        ),
        // Inside a function the names lie in its frame, as a block's do.
        (
            "g := fn (k := i32 ?) -> i32 ( m := { a := k, f := fn () -> i32 ( a + 1 ), b := f() }, m.b ), g(4)",
            "a nested function cannot use an outer function's local or parameter",
        ),
        (
            "m := { a := i32 1, inc := fn () -> void ( a = a + 1 ), inc() }, m.a",
            "`a` is not `mut`",
        ),
        (
            "m := { a := i32 1, b := i32 2, f := fn () -> i32 ( b ), c := move a }, 0",
            "1:69: error: `f` reads the names written above it from this `{ … }`'s value, so they \
             must last to its `}`, and `a` ends before it",
        ),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

/// ›A type body describes one level‹ [[R222]]: a field named bare in a `static` function, a `free`
/// or a `fn` entry of a `{ … }` reads as the same field read through the value, on both tiers:
/// text and a type as what they hold, a record's field with a `.`, and a write through it meets
/// the gates `v.p.x = …` would.
#[test]
fn a_field_named_bare_reads_as_the_field_read_through_the_value() {
    let pt = "pt := type ( mut x := i32 ?, y := i32 ?, static sum := fn () -> i32 ( x + y ) ), ";
    for compile in ["", "v.f.compile(), "] {
        for (head, call, printed) in [
            ("t := type ( s := string ?, static f := fn () -> void ( print «{s}!» ) ), v := t («hi»)", "v.f()", "hi!"),
            ("t := type ( s := string ?, static f := fn () -> string ( s ) ), v := t («hi»)", "v.f()", "hi"),
            ("t := type ( k := type ?, static f := fn () -> void ( print «{k}» ) ), v := t (i32)", "v.f()", "i32"),
            ("t := type ( k := type ?, static f := fn () -> type ( k ) ), v := t (i32)", "v.f()", "i32"),
            ("q := type ( p := pt ?, static f := fn () -> i32 ( p.x ) ), v := q (pt (1, 2))", "v.f()", "1"),
            ("q := type ( p := pt ?, static f := fn () -> i32 ( p.y + 1 ) ), v := q (pt (1, 2))", "v.f()", "3"),
            ("q := type ( p := pt ?, static f := fn () -> i32 ( p.sum() ) ), v := q (pt (1, 2))", "v.f()", "3"),
            ("q := type ( p := pt ?, static f := fn () -> pt ( p ) ), v := q (pt (1, 2))", "v.f().y", "2"),
            (
                "l := type ( a := pt ?, b := pt ? ), q := type ( r := l ?, static f := fn () -> i32 ( r.b.y ) ), \
                 v := q (l (pt (1, 2), pt (3, 4)))",
                "v.f()",
                "4",
            ),
            ("v := { s := «hi», f := fn () -> void ( print «{s}!» ) }", "v.f()", "hi!"),
            ("v := { p := pt (1, 2), f := fn () -> i32 ( p.x ) }", "v.f()", "1"),
        ] {
            let src = format!("{pt}{head}, {compile}{call}");
            assert_eq!(run_line(&src), (Some(0), format!("{printed}\n"), String::new()), "{src}");
        }
    }
    let bag =
        "bag := type ( s := string ?, p := pt ?, static free = ( print «freed {s} {p.y}» ) ), ";
    for compile in ["", "f.compile(), "] {
        let src =
            format!("{pt}{bag}f := fn () -> i32 ( b := bag («x», pt (1, 2)), 1 ), {compile}f()");
        assert_eq!(run_line(&src), (Some(0), "freed x 2\n1\n".into(), String::new()), "{src}");
    }
    let writes = |p: &str, v: &str| {
        format!(
            "{pt}q := type ( {p}p := pt ?, static f := fn () -> void ( p.x = 9 ) ), \
             {v}v := q (pt (1, 2)), v.f(), v.p.x"
        )
    };
    let src = writes("mut ", "mut ");
    assert_eq!(run_line(&src), (Some(0), "9\n".into(), String::new()), "{src}");
    for (src, expect) in [
        (writes("mut ", ""), "`v` is not `mut`"),
        (writes("", "mut "), "`p` is not `mut`"),
        (
            format!("{pt}q := type ( p := pt ?, static f := fn () -> i32 ( p.z ) ), 0"),
            "unknown name `z`",
        ),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

/// ›Reading a path runs nothing‹ [[R273]]: a call stands for what it returns, so `.` after it
/// reads a field of the record it returns, or calls its type's `static` function on it, as `.`
/// after a name of the record does; a record a type makes is read so too.
#[test]
fn a_dot_reads_a_field_of_a_record_no_name_holds() {
    let head = "pt := type ( x := i32 ?, y := i32 ?, static sum := fn () -> i32 ( x + y ) ), \
                g := fn (n := i32 ?) -> pt ( pt (n * 2, n) ), ";
    for compile in ["", "h.compile(), "] {
        for (body, printed) in [
            ("g(5).x", "10"),
            ("g(5).y + 1", "6"),
            ("pt (3, 4).y", "4"),
            ("g(5).sum()", "15"),
            ("pt (1, 2).sum()", "3"),
        ] {
            let src = format!("{head}h := fn () -> i32 ( {body} ), {compile}h()");
            assert_eq!(run_line(&src), (Some(0), format!("{printed}\n"), String::new()), "{src}");
        }
    }
    let src = format!("{head}g(5).z");
    let (code, _, stderr) = run_line(&src);
    assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
    assert!(stderr.contains("unknown name `z`"), "{src}: stderr: {stderr}");
}

#[test]
fn a_hashmap_reads_a_curly_bracket_each_name_s_spelling_a_text_key() {
    for (src, printed) in [
        ("mut h := hashmap string -> i32 { a := 1, b := 2 }, h[«a»]", "1"),
        ("mut h := hashmap string -> i32 { a := 1, b := 2 }, h[«new york»] = 3, h[«new york»] + h[«b»]", "5"),
        ("h := hashmap string -> i32 { a := 1, b := a + 1 }, h[«b»]", "2"),
        ("h := hashmap string -> i32 { }, h == ?", "false"),
        ("h := hashmap string -> type { t := i32, u := f64 }, h[«u»] == f64", "true"),
        ("h := hashmap string -> string { a := «x» }, print «{h[«a»]}»", "x"),
        ("h := hashmap string -> hashmap string -> i32 { a := hashmap string -> i32 { x := 7 } }, r := h[«a»], r[«x»]", "7"),
        ("f := fn (k := i32 ?) -> i32 ( h := hashmap string -> i32 { a := k, b := k * 2 }, h[«b»] ), f(4)", "8"),
        ("mut h := hashmap string -> i32 { a := 1 }, mut n := h, n[«a»] = 9, h[«a»]", "1"),
        ("mut h := hashmap string -> i32, h[«x»] = 4, k := «x», h[k]", "4"),
    ] {
        assert_eq!(run_line(src), (Some(0), format!("{printed}\n"), String::new()), "{src}");
    }
    for (src, expect) in [
        ("h := hashmap i32 -> i32 { a := 1 }", "keys each entry by its name's spelling"),
        ("h := hashmap string -> i32 { regex «x+» := i32 }", "`x+` is a pattern"),
        ("h := hashmap string -> i32 { a := 1.5 }", "no exact value in the type it lands in"),
        ("h := hashmap string -> i32 { a := 1 }, h[«z»]", "the map holds no value at this key"),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_tape_line_no_check_narrowed_is_checked_through_a_record_pointer() {
    let q = "pt := type ( mut a := u64 ?, mut b := u64 ? ), \
             q := type ( v := u64 ?, static parse_rank = 61, static parse = ( tape[0].type = q, \
             p := alloc 1 of pt ?, p@.a = tape[1].dyads[0], tape[0].v = p@.a, \
             tape.remove(1), tape.is_constructed[0] = true ) )";
    let src = format!("{q}, x := q [5], x.v");
    assert_eq!(run_line(&src), (Some(0), "5\n".to_string(), String::new()), "{src}");
    let src = format!("{q}, x := q [«a»]");
    let (code, _, stderr) = run_line(&src);
    assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
    assert!(stderr.contains("holds no literal or constant of it"), "{src}: stderr: {stderr}");
}
