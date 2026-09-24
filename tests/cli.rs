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
fn an_import_runs_the_file_and_prints_its_tail() {
    let out = logos().args(["import", "examples/answer.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn the_drop_model_runs_at_file_scope() {
    let out = logos().args(["import", "tests/fixtures/heap.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn a_parse_error_renders_clickable_with_a_caret() {
    let out = logos().args(["import", "tests/fixtures/unknown_name.logos"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown_name.logos:2:5: error: unknown name"), "stderr: {err}");
    assert!(err.contains("      ^"), "stderr: {err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn an_unreadable_path_fails_cleanly() {
    let out = logos().args(["import", "tests/fixtures/no_such_file.logos"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));
}

#[test]
fn an_import_exposes_pub_and_hides_private() {
    let out = logos().arg("import tests/fixtures/lib_pub.logos, double(21)").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");

    let out = logos().arg("import tests/fixtures/lib_pub.logos, helper(1)").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not in scope"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn importing_a_library_alone_is_silent_and_clean() {
    let out = logos().args(["import", "tests/fixtures/lib_pub.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stdout.is_empty());
}

#[test]
fn an_imported_file_cannot_see_the_import_site() {
    let out = logos().arg("x := 5, import tests/fixtures/uses_missing.logos").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("uses_missing.logos:2:6"), "stderr: {err}");
    assert!(err.contains("not in scope"), "stderr: {err}");
}

#[test]
fn a_relative_import_resolves_against_the_importing_file() {
    let out = logos().args(["import", "tests/fixtures/outer.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn an_import_cycle_is_a_checked_error() {
    let out = logos().args(["import", "tests/fixtures/cycle_a.logos"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("cycle"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn an_import_failure_reports_the_outer_position_then_the_inner_one() {
    let out = logos().arg("import tests/fixtures/broken.logos").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = stderr.lines().collect();
    assert!(
        lines[0].contains("import of `tests/fixtures/broken.logos` failed"),
        "stderr: {stderr}"
    );
    assert!(lines[1].contains("import tests/fixtures/broken.logos"), "stderr: {stderr}");
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
            "import tests/fixtures/subdir/inner.logos, \
             import tests/fixtures/subdir/inner.logos, inner_val + 0",
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn the_repl_imports_once_per_session_and_keeps_pub_names() {
    let (echoes, stderr) =
        repl(b"import examples/answer.logos\nimport tests/fixtures/lib_pub.logos\ndouble(4)\n");
    assert_eq!(echoes, ["42", "8"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_view_reads_the_cell_and_operands_are_ordinary_fields() {
    // `(x + x)` has arity 3: operands[2] is the resolved callable leaf, not an i32.
    let (echoes, stderr) = repl(
        b"x := i32 5\nx:type == i32\n(x + x):type.arity\n\
          (x + x).operands[0]:type == i32\n(x + x).operands[2]:type == i32\n\
          (x + x).operands[0]\n",
    );
    assert_eq!(echoes, ["true", "3", "true", "false", "5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_type_body_fills_its_slots_and_declares_its_members() {
    let (echoes, stderr) = repl(
        b"t := type (parse_rank = *.parse_rank + 1, associativity = right)\n\
          t.parse_rank\nt.associativity == right\nright:type == type\n",
    );
    assert_eq!(echoes, ["71.0", "true", "true"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"g := type (fields = (shared y := 3, shared z := y + 3))\ng.fields.y\ng.fields.z\ng.parse_rank\n",
    );
    assert_eq!(echoes, ["3", "6", "91.0"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"g := type (fields = (shared y := i32 7, shared z := i32 (y + 3)))\n\
          g.fields.y\ng.fields.z\ng.fields.y + 1\n",
    );
    assert_eq!(echoes, ["7", "10", "8"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"p := type (fields = (shared k := 10, v := i32 ?))\nq := p(2)\nq.v\nq.k\np.fields.k\np.size_bytes\n",
    );
    assert_eq!(echoes, ["2", "10", "10", "4"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"t := type (fields = (shared # \xc2\xabnote\xc2\xbb y := 3, shared z := 4 # \xc2\xabtail\xc2\xbb, v := i32 ?))\n\
          t.fields.y\nt.fields.z\nt.size_bytes\n",
    );
    assert_eq!(echoes, ["3", "4", "4"], "stderr: {stderr}");
    let (echoes, stderr) =
        repl(b"h := fn () -> i32 ( p := @i32 ?, p@ )\ng := type (fields = (shared y := h()))\n");
    assert!(
        echoes.is_empty() && stderr.contains("a type body's own declaration failed"),
        "stderr: {stderr}"
    );
    let (echoes, stderr) = repl(b"r := type (lex_rank = 3)\nr:lex_rank\n+:lex_rank\nr.lex_rank\n");
    assert_eq!(echoes, ["3.0", "0.0"], "stderr: {stderr}");
    assert!(!stderr.is_empty(), "`.lex_rank` is no member of a type");
    let (echoes, stderr) = repl(
        b"r := type (lex_rank = 3)\ns := r\ns:lex_rank\ns:lex_rank = 7\ns:lex_rank\nr:lex_rank\n",
    );
    assert_eq!(echoes, ["0.0", "7.0", "3.0"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"f := fn (t := type ?) -> void ( )\nf(type (lex_rank = 1))\n");
    assert!(stderr.contains("lex_rank is the name's"), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"s := type (precedence = 5)\n");
    assert!(echoes.is_empty() && stderr.contains("unknown name"), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"c := type (fields = (shared run = 5))\n");
    assert!(echoes.is_empty() && stderr.contains("`shared run = (…)`"), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"x := 1\np := type (fields = (x := i32 ?))\nq := p(2)\nq.x\nx\n");
    assert_eq!(echoes, ["2", "1"], "stderr: {stderr}");
}

#[test]
fn any_spelling_the_index_can_hold_is_nameable() {
    let (echoes, stderr) = repl(
        b"^ := i32 5\n^ + 1\na := i32 1\nab2 := i32 2\nab2 + a\nmut x := i32 5\nx=-1\nx\n\
          p := type (fields = (v := i32 ?))\nq := p(7)\nr := &q\nrr := &r\nrr@@.v\nx^2\n",
    );
    assert_eq!(echoes, ["6", "3", "-1", "7"], "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:2: error:"), "stderr: {stderr}");
    // caret.logos: 9 + 512 + 18.
    let out = logos().args(["import", "tests/fixtures/caret.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "539\n");
}

#[test]
fn the_power_demo_prints_nine() {
    let out = logos().args(["import", "identities/power.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "9\n");
}

#[test]
#[ignore = "stand-in for #137: the seed cannot run array.logos yet"]
fn the_array_written_in_logos_reads_an_element_and_its_size() {
    let out = logos().args(["import", "./identities/array.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    for (tail, printed) in [("a[1]", "2\n"), ("a.size_bytes()", "12\n")] {
        let src = format!("import ./identities/array.logos, a := array i32 (1, 2, 3), {tail}");
        let out = logos().arg(&src).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), printed, "{src}");
    }
}

/// The power operator with the bare run body, on one line for the REPL.
const POWER: &str = "^ := type ( fields = ( lhs := ?, rhs := i32 ?, output := type ?, \
    shared run = ( mut r := this.output 1, for 0..this.rhs ( r = r * this.lhs ), r ) ), \
    parse_rank = *.parse_rank + 1, associativity = right, \
    parse = ( this.lhs = tape[-1], this.rhs = tape[1], this.output = tape[-1]:type, \
    tape[0] = this, tape.is_constructed[0] = true, tape.remove(1), tape.remove(-1) ) )";

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
fn a_shared_member_is_read_through_a_node_bare_and_through_the_type_by_fields() {
    let point = "point := type ( fields = ( x := i32 ?, y := i32 ?, shared dims := i32 2 ) ), \
                 p := point (1, 2)";
    for (read, expect) in [("p.dims", "2"), ("point.fields.dims", "2"), ("p.x", "1")] {
        let out = logos().args([&format!("{point}, {read}")]).output().unwrap();
        assert!(out.status.success(), "{read}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), expect, "{read}");
    }
    for (read, expect) in [
        ("point.dims", "`.fields.dims`"),
        ("point.x", "a place in each node"),
        ("point.fields.x", "a place in each node"),
        ("point.fields.run", "does not fit"),
        ("point.run", "no `run` of its own"),
    ] {
        let out = logos().args([&format!("{point}, {read}")]).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(expect), "{read}: stderr: {stderr}");
    }
    // The instances' run is one place, read through the type; the type has none of its own.
    let out =
        logos().args([&format!("{POWER}, r := ^.fields.run, ^.fields.run")]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "dyad");
    let out = logos().args([&format!("{POWER}, ^.run")]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no `run` of its own"), "stderr: {stderr}");
}

#[test]
fn a_constructed_run_body_is_kept_on_the_type_across_repl_lines() {
    // A later line is its own parser, so it must find the i32 body built by the first use.
    let (echoes, stderr) = repl(
        format!(
            "{POWER}\nf := fn (x := i32 ?) -> i32 ( x ^ 2 )\nf(2)\n\
             g := fn (x := i32 ?) -> i32 ( x ^ 3 )\ng.compile()\ng(3)\n"
        )
        .as_bytes(),
    );
    assert_eq!(echoes, ["4", "27"], "stderr: {stderr}");
}

#[test]
fn a_run_body_sees_its_definition_and_its_own_locals_only() {
    let before = format!(
        "k := i32 2, {}, f := fn (x := i32 ?) -> i32 ( x ^ 2 ), f(5)",
        POWER.replace("r = r * this.lhs", "r = r * this.lhs * k")
    );
    let out = logos().args([&before]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "100");
    let after = format!(
        "{}, k := i32 2, f := fn (x := i32 ?) -> i32 ( x ^ 2 ), f(5)",
        POWER.replace("r = r * this.lhs", "r = r * this.lhs * k")
    );
    let out = logos().args([&after]).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("the run body of `^` could not be constructed"), "stderr: {stderr}");
    assert!(stderr.contains("run body of `^`:1:"), "stderr: {stderr}");
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
        format!(
            "{POWER}\nq := rational_number 2\nq ^ 3\n\
             h := fn (n := i32 ?) -> rational_number ( 2.5 ^ n )\nh(2)\n"
        )
        .as_bytes(),
    );
    assert_eq!(echoes, ["8", "25/4"], "stderr: {stderr}");
}

#[test]
fn rational_places_and_operators_run_interpreted_and_are_refused_compiled() {
    let (echoes, stderr) = repl(
        b"mut q := rational_number 2\nr := rational_number 3\nq * r\nq / 3\nq = q * 2\nq\n\
          q < 5\nmut x := rational_number ?\nx = 7\nx\nk := fn () -> rational_number ( q )\nk()\n",
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
    // `<=>` beats `<=` `>` by length at equal rank; the core's `..` is spelled `\.\.`.
    let (echoes, stderr) = repl(
        b"regex \xc2\xab[0-9]+[kK]\xc2\xbb := type ()\n(5k):type == type\n\
          regex \xc2\xab<=>\xc2\xbb := type ()\n(<=>):type == type\n\
          regex \xc2\xab\\.\\.\xc2\xbb := type ()\n",
    );
    assert_eq!(echoes, ["true", "true"], "stderr: {stderr}");
    assert!(stderr.contains("shadowed"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"regex \xc2\xab[a-z][0-9]\xc2\xbb := type ()\nregex \xc2\xaba[0-9]\xc2\xbb := type ()\na1\n\
          regex \xc2\xabb[0-9]\xc2\xbb := type (lex_rank = 1)\n(b1):type == type\n",
    );
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
    assert!(stderr.contains("same lex_rank"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"regex \xc2\xab[unclosed\xc2\xbb := type ()\nregex 5\n\
          regex \xc2\xabz[0-9]\xc2\xbb := type (\nregex \xc2\xabz[0-9]\xc2\xbb := type ()\n(z1):type == type\n",
    );
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
    assert!(stderr.contains("does not compile"), "stderr: {stderr}");
    assert!(stderr.contains("must be followed by a"), "stderr: {stderr}");
}

#[test]
fn a_constructor_written_in_logos_runs_during_the_parse() {
    let out = logos().args(["import", "tests/fixtures/squared.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "92\n");
    // A parse that consumes nothing sets its flag and stands as itself.
    let (echoes, stderr) = repl(
        b"noop := type (parse = ( tape.is_constructed[0] = true ))\n\
          t := noop\nt:type == type\nnoop.parse_rank\n",
    );
    assert_eq!(echoes, ["true", "91.0"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"bad := type (parse = ( tape[5] ))\nx := bad\n");
    assert!(stderr.contains("constructor failed"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"named := type (parse = ( tape[0] = tape.spelling[0], tape.is_constructed[0] = true ))\n\
          f := fn () -> void ( named )\n5\n",
    );
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"bad2 := type (parse = ( tape.spelling[5] ))\nx := bad2\n");
    assert!(stderr.contains("constructor failed"), "stderr: {stderr}");
}

#[test]
fn a_binding_carries_its_spelling() {
    let (echoes, stderr) = repl(b"x := 5\nx:name\nif:name\n");
    assert_eq!(echoes, ["x", "if"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"a := i32 1\n(a + 1):name\n");
    assert!(stderr.contains("expected a field name"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"sp := type (parse = ( tape[0] = tape[-1]:name, tape.is_constructed[0] = true, tape.remove(-1) ))\n\
          x := 1\nf := fn () -> void ( x sp )\n5\n",
    );
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
}

#[test]
fn the_slot_words_are_names_only_inside_a_type_body() {
    // Outside a type body they are ordinary spellings; inside, the body's own
    // shadow an outer name and are gone again at the close.
    let (echoes, stderr) = repl(
        b"run := 5\nrun + 1\nparse := i32 2\nparse * 3\n\
          m := type (fields = (a := i32 ?, output := type ?, shared run = ( this.a + this.a )), \
          parse_rank = *.parse_rank + 1, \
          parse = ( this.a = tape[-1], this.output = i32, tape[0] = this, \
          tape.is_constructed[0] = true, tape.remove(-1) ))\n\
          x := i32 3\nx m\nrun\nparse_rank := 4\nparse_rank\n",
    );
    assert_eq!(echoes, ["6", "6", "6", "5", "4"], "stderr: {stderr}");
}

#[test]
fn a_slot_body_is_read_bare() {
    // The hidden `tape` is declared as a parameter is, so an outer `tape` is the shadowing error.
    let (echoes, stderr) = repl(
        b"sp := type (parse = ( tape[0] = tape[-1]:name, tape.is_constructed[0] = true, tape.remove(-1) ))\n\
          x := 1\nf := fn () -> void ( x sp )\n5\n",
    );
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"tape := 1\nsp := type (parse = ( tape.recenter(0) ))\n");
    assert!(stderr.contains("shadowed"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"minus := type (fields = (a := i32 ?, b := i32 ?, output := type ?, shared run = ( this.a - this.b )), \
          parse_rank = +.parse_rank, associativity = left, \
          parse = ( this.a = tape[-1], this.b = tape[1], this.output = i32, tape[0] = this, \
          tape.is_constructed[0] = true, tape.remove(1), tape.remove(-1) ))\n\
          7 minus 2\n10 minus 2 minus 3\nf := fn (x := i32 ?) -> i32 ( x minus 1 )\nf.compile()\nf(9)\nthis\n",
    );
    assert_eq!(echoes, ["5", "5", "8"], "stderr: {stderr}");
    assert!(stderr.contains("`this` is not in scope"), "stderr: {stderr}");
    // A body constructed in the session's section reaches the core names, so a
    // run that calls a function whose body reads an operator constructs.
    let (echoes, stderr) = repl(
        b"sq := fn (a := i32 ?) -> i32 ( a * a )\n\
          squared := type (fields = (a := i32 ?, output := type ?, shared run = ( sq(this.a) )), \
          parse_rank = *.parse_rank + 1, \
          parse = ( this.a = tape[-1], this.output = i32, tape[0] = this, \
          tape.is_constructed[0] = true, tape.remove(-1) ))\n\
          x := i32 4\nx squared\n",
    );
    assert_eq!(echoes, ["16"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"t := type (fields = (a := i32 ?, shared run = ( s := \xc2\xaba ) b\xc2\xbb, \
          # \xc2\xab ) \xc2\xbb (( x[0] ), 5 ))))\n\
          t:type == type\n",
    );
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
    let out = logos()
        .args(["t := type (fields = (shared run = ( x[0], # c ) d\n5 ))),\n\
                t:type == type"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn a_type_body_refuses_what_is_not_its_own() {
    for (src, expect) in [
        (&b"t := type (parse_rank := 5)\n"[..], "shadowed"),
        (b"y := 1\ng := type (y := 3)\n", "shadowed"),
        (b"t := type (fields = (a := i32 ?), fields = (b := i32 ?))\n", "one `fields"),
        (b"g := type (y := 3)\n", "inside `fields"),
        (b"g := type (fields = (shared))\n", "followed by a declaration"),
        (b"f := fn (shared a := i32 ?) -> void ( a )\n", "nowhere else"),
        (b"parse_rank = 3\n", "unknown name"),
        (b"d := i32 5\ndrop = 3\n", "a line of the type body itself"),
        (b"t := type (parse = ( parse_rank = 3 ))\n", "a line of the type body itself"),
        (b"t := type (run = ( 1 ))\n", "no `run` of its own"),
        (b"t := type (fields = (run = 5))\n", "marked"),
        (b"t := type (fields = (shared parse_rank = 5))\n", "other slots"),
        (b"t := type (fields = (shared parse = ( tape.recenter(0) )))\n", "other slots"),
        (b"t := type (fields = (shared fields = (a := i32 ?)))\n", "other slots"),
        (b"t := type (fields = (shared drop = 5))\n", "`drop` slot"),
        (b"t := type (parse = ( this.a = tape[-1] ))\n", "declares none"),
        (b"t := type (fields = (a := ?), parse = ( this.b = tape[-1] ))\n", "no field `b`"),
        (
            b"t := type (fields = (a := ?), parse = ( tape.is_constructed[0] = 5 ))\n",
            "takes a bool",
        ),
        (b"p := type (fields = (y := i32 ?, shared y := 3))\n", "shadowed"),
        (b"p := type (fields = (shared y := 3, y := i32 ?))\n", "shadowed"),
        (b"t := type (fields = (shared := i32 ?))\n", "followed by a declaration"),
        (b"t := type (5)\n", "a type body line"),
        (b"t := type (fields)\n", "a type body line"),
        (b"t := type (associativity = 5)\n", "`left` or `right`"),
        (b"t := type (parse = fn (mut a := i32 ?) -> void ( a = 1 ))\n", "`parse = (…)`"),
        (b"t := type (parse = 5)\n", "`parse = (…)`"),
        (b"t := type (fields = (shared run = 5))\n", "`shared run = (…)`"),
        (b"t := type (fields = (shared run = ( 5 )))\nt(1)\n", "the call form of a type"),
        (b"t := type (fields = (shared run = ( 5 )),\n", "never closed"),
        (
            b"g := fn (n := i32 ?) -> type ( type (parse_rank = n) )\n",
            "known when the type is defined",
        ),
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
    let (echoes, stderr) = repl(b"x := @i32\ny := @i32\nx == y\nz := @@i32\nz == @@i32\n");
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
fn a_collection_member_demands_its_index_brackets() {
    let (_echoes, stderr) = repl(b"x := i32 5\n(x + x).operands(0)\n");
    assert!(stderr.contains("element access is `[…]`"), "stderr: {stderr}");
}

#[test]
fn a_square_bracket_is_a_paren_that_closes_only_itself() {
    let (echoes, stderr) = repl(b"x := i32 5\n(x + x).operands[1 - 1]\n");
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"(1]\n");
    assert!(stderr.contains("never closed"), "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"x := i32 5\n(x + x).operands[0)\n");
    assert!(stderr.contains("never closed"), "stderr: {stderr}");
}

#[test]
fn dot_type_is_a_guided_error() {
    let (_echoes, stderr) = repl(b"x := i32 5\nx.type\n");
    assert!(stderr.contains("x:type"), "stderr: {stderr}");
}

#[test]
fn nothing_reaches_the_cell_as_a_whole() {
    for src in [&b"x := i32 5\nx:dyad\n"[..], b"x := i32 5\nx:value\n"] {
        let (echoes, stderr) = repl(src);
        assert!(echoes.is_empty() && stderr.contains("as a whole"), "stderr: {stderr}");
    }
}

#[test]
fn a_reflect_read_that_does_not_fit_is_an_error() {
    let (_echoes, stderr) = repl(b"x := i32 5\n(x + x):type.roles[5]\n");
    assert!(stderr.contains("does not fit"), "stderr: {stderr}");
}

#[test]
fn an_import_inside_a_fn_body_is_rejected() {
    // The load is a comptime effect; inside a fn body parse and run order do not coincide.
    let (_echoes, stderr) = repl(b"g := fn () -> i32 ( import examples/answer.logos 1 )\n");
    assert!(stderr.contains("loads at parse time"), "stderr: {stderr}");
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
        repl(b"a := alloc 1 of i32 5\na@\nr := ( b := alloc 1 of i32 20, b@ )\nr\n");
    assert_eq!(echoes, ["5", "20"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_repl_reuses_a_name_after_drop() {
    let (echoes, stderr) = repl(b"n := i32 5\ndrop n\nn\nn := i32 6\nn\n");
    assert_eq!(echoes, ["6"], "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:1: error: `n` is dead here"), "stderr: {stderr}");
}

#[test]
fn the_repl_refuses_a_call_whose_body_reads_a_dropped_name() {
    // A call is a use of every outer name the callee's body reads.
    let (echoes, stderr) = repl(
        b"mut n := i32 0\nclimb := fn () -> i32 ( n = n + 1, n )\nclimb()\nclimb()\ndrop n\n\
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
    let out = logos().arg("import tests/fixtures/lib_helper.logos, bump(41)").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn a_failed_repl_line_restores_a_moved_name() {
    let (echoes, stderr) = repl(b"a := alloc 1 of i32 5\nr := ( b := own a, b@ ) + nosuch\na@\n");
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    assert!(stderr.contains("unknown name"), "stderr: {stderr}");
    assert!(!stderr.contains("dead"), "stderr: {stderr}");
}

#[test]
fn an_owning_value_that_nothing_can_free_is_refused() {
    let out = logos().args(["import", "tests/fixtures/unbound_owning.logos"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("must be bound to a name"), "stderr: {err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn the_repl_compiles_a_fn_across_lines() {
    let (echoes, stderr) =
        repl(b"double := fn (x := i64 ?) -> i64 ( x + x )\ndouble.compile()\ndouble(21)\n");
    assert_eq!(echoes, ["42"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn an_else_if_chain_selects_the_matching_arm() {
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
    let (echoes, stderr) =
        repl(b"t := i32\nx := t 7\nt(9)\nf := fn (v := t ?) -> t ( v + v )\nt\nf(x)\n");
    assert_eq!(echoes, ["9", "14"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn logos_is_a_value_reflected_by_dot_logos_and_compared_by_identity() {
    let (echoes, stderr) = repl(
        b"i32 == i32\ni32 == f64\ni32 != f64\ni32:type == logos\ni32:type == i32\n\
          x := i32 5\nx:type == i32\nx:type == f64\nt := logos\ni32:type == t\nlogos:type == logos\n",
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
    let out = logos().args(["import", "examples/type_reflection.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn a_logos_value_prints_its_spelling() {
    let out = logos().args(["import", "tests/fixtures/logos_name.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "i32\n");
}

#[test]
fn a_type_returning_function_resolves_at_comptime() {
    let (echoes, stderr) = repl(
        b"pick := fn (i := i32 ?) -> logos (if (i==0)(i32) else (f64))\n\
          pick(0) == i32\npick(1) == f64\npick(0) == f64\nt := pick(0)\nt == i32\n",
    );
    assert_eq!(echoes, ["true", "true", "false", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_type_returning_fn_example_runs() {
    let out = logos().args(["import", "examples/type_returning_fn.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn file_mode_runs_each_expression_as_it_parses() {
    // The file driver used to parse everything first, so the call read x's zeroed storage and answered i32 rather than f64.
    let out = logos()
        .args(["import", "tests/fixtures/comptime_sees_committed_state.logos"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn a_type_call_with_a_runtime_argument_is_rejected() {
    let (_echoes, stderr) = repl(
        b"pick := fn (i := i32 ?) -> logos (if (i==0)(i32) else (f64))\n\
          g := fn (n := i32 ?) -> i32 ( a := pick(n), 1 )\n",
    );
    assert!(stderr.contains("must be evaluable at parse time"), "stderr: {stderr}");
}

#[test]
fn a_logos_declaration_declares_a_place_of_that_type() {
    let (echoes, stderr) = repl(b"mut a := i32 ?\na:type == i32\na = 9\na\n");
    assert_eq!(echoes, ["true", "9"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_dependent_typed_declaration_takes_a_computed_type() {
    let (echoes, stderr) = repl(
        b"metalogos := fn (i := i32 ?) -> logos (if (i==0)(i32) else (f64))\n\
          mut b := metalogos(1) ?\nb:type == f64\nb = 7\nb\n",
    );
    assert_eq!(echoes, ["true", "7.0"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_logos_declaration_works_after_other_code() {
    let out = logos()
        .args(["import", "tests/fixtures/declared_logos_after_code.logos"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n");
}

#[test]
fn a_logos_declaration_rejects_a_non_type() {
    let (_echoes, stderr) = repl(b"a := 5 ?\n");
    assert!(stderr.contains("expected one expression, found more"), "stderr: {stderr}");
}

#[test]
fn a_logos_declaration_names_the_non_numeric_gap() {
    let (_echoes, stderr) = repl(b"a := bool ?\n");
    assert!(stderr.contains("non-numeric types are not in the seed yet"), "stderr: {stderr}");
}

#[test]
fn a_logos_variable_declares_fills_once_and_becomes_the_type() {
    let (echoes, stderr) =
        repl(b"mut a := logos ?\na:type == logos\na == i32\na = i32\na == i32\ny := a 5\ny\n");
    assert_eq!(echoes, ["true", "false", "true", "5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_logos_box_is_written_as_often_as_you_like() {
    let (echoes, stderr) = repl(b"mut a := logos ?\na = i32\na = f64\na\n");
    assert_eq!(echoes, ["f64"], "stderr: {stderr}");

    let (echoes, stderr) = repl(b"mut a := logos ?\ng := fn () -> i32 ( a = i32, 1 )\ng()\na\n");
    assert_eq!(echoes, ["1", "i32"], "stderr: {stderr}");

    let (_e, stderr) = repl(b"mut a := logos ?\na = 5\n");
    assert!(stderr.contains("must be a type value"), "stderr: {stderr}");
}

#[test]
fn logical_operators_fold_over_bool_literals() {
    let (echoes, stderr) = repl(
        b"true or false\ntrue and true\nnot (true)\n\
          mut a := logos ?\nif (a:type == f32 or a:type == logos) (a = f64) else (a = i32)\na == f64\n",
    );
    assert_eq!(echoes, ["true", "true", "false", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_comptime_if_drops_the_untaken_branch_unparsed() {
    // `a = 9.9` under `a := i32 ?` would be a parse error if it were ever parsed; that this runs proves the branch was skipped.
    let (echoes, stderr) = repl(
        b"mut a := i32 ?\nif (a:type == i32) (a = 9) else (a = 9.9)\na\n\
          mut b := f64 ?\nif (b:type == i32) (b = 1) else if (b:type == f64) (b = 2.5) else (b = 3)\nb\n",
    );
    assert_eq!(echoes, ["9", "2.5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_metatypefn_example_runs() {
    // The expected value tracks the file's current argument; the deep arm is pinned by the metalogos_arm fixture.
    let out = logos().args(["import", "examples/metatypefn.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "9.9\n");
}

#[test]
fn the_metalogos_arm_fills_a_logos_variable() {
    let out = logos().args(["import", "tests/fixtures/metalogos_arm.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "i32\n");
}

#[test]
fn a_declaration_snapshots_its_value_and_reads_are_stable() {
    // The block used to re-run its loop on every read and grow.
    let (echoes, stderr) = repl(
        b"c := (mut sum := i32 0, for i in 0..10 (sum = sum + i), sum)\nc\nc\n\
          mut a := i32 1\nx := a + a\na = 5\nx\n",
    );
    assert_eq!(echoes, ["45", "45", "2"], "stderr: {stderr}");
}

#[test]
fn a_for_loop_needs_no_index_in_both_tiers() {
    let (echoes, stderr) = repl(
        b"f := fn () -> i32 ( mut t := i32 0, for 0..5 ( t = t + 2 ), t )\nf()\nf.compile()\nf()\n",
    );
    assert_eq!(echoes, ["10", "10"], "stderr: {stderr}");
}

#[test]
fn the_no_index_example_counts_the_evens() {
    let out = logos().args(["import", "examples/no_index.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "5");
}

#[test]
fn a_declaration_copies_rather_than_aliases() {
    let (echoes, stderr) = repl(b"y := i32 1\nmut z := y\nz = 5\ny\nz\n");
    assert_eq!(echoes, ["1", "5"], "stderr: {stderr}");
}

#[test]
fn values_render_through_their_type() {
    let (echoes, stderr) =
        repl(b"f32 5.5\nq := f64 2.5\nq + q\ni64 -1\nu8 200\n1 < 2\nnot (1 < 2)\n");
    assert_eq!(echoes, ["5.5", "5.0", "-1", "200", "true", "false"], "stderr: {stderr}");
}

#[test]
fn help_prints_usage_and_version() {
    let out = logos().arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(env!("CARGO_PKG_VERSION")));
    assert!(text.contains("logos import ./file.logos"));
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
    assert!(err.contains("<command line>:1:7: error:"), "stderr: {err}");

    for flag in ["--help", "-h"] {
        let out = logos().arg(flag).output().unwrap();
        assert!(out.status.success(), "{flag}");
        assert!(String::from_utf8_lossy(&out.stdout).contains("usage:"), "{flag}");
    }
}

#[test]
fn the_binding_read_answers_scope_range_and_gate() {
    let (echoes, stderr) = repl(
        b"x := i32 5\nx:end\nx:gate\nx:scope\n(x + x):scope\ny := i32\ny:type == type\ny == i32\n",
    );
    assert_eq!(echoes.len(), 6, "stderr: {stderr}");
    assert_eq!(&echoes[..2], ["0", "0"], "end and gate are null");
    assert_ne!(echoes[2], "0", "x was declared somewhere");
    assert_eq!(echoes[2], echoes[3], "a constructed node's :scope is the scope open at the read");
    assert_eq!(&echoes[4..], ["true", "true"], "an alias is its own binding over the same dyad");
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
    let (_e, via_dot) = repl(b"p := type (fields = (a := i32 ?))\nq := p(1)\nq.scope\n");
    assert_eq!(message(&via_binding), message(&via_dot), "binding: {via_binding}\ndot: {via_dot}");
    assert!(via_binding.contains("not in scope"), "stderr: {via_binding}");
    let (_e, via_binding) = repl(b"x := i32 5\nx:nonexistent\n");
    let (_e, via_dot) = repl(b"p := type (fields = (a := i32 ?))\nq := p(1)\nq.nonexistent\n");
    assert_eq!(message(&via_binding), message(&via_dot), "binding: {via_binding}\ndot: {via_dot}");
    assert!(via_binding.contains("unknown name"), "stderr: {via_binding}");
}

#[test]
fn a_type_is_read_with_the_binding_read() {
    let (_echoes, stderr) = repl(b"x := i32 5\n(dyad x).type\n");
    assert!(!stderr.is_empty(), "the old spelling no longer parses");
    let (echoes, stderr) = repl(b"x := i32 5\nx:type == i32\n");
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
}

#[test]
fn a_tight_read_runs_over_a_keyword_before_its_constructor_wakes() {
    let (echoes, stderr) = repl(
        b"mut x := i32 5\nif:scope\ntype:type == type\nfn:type == type\n\
          f := fn () -> i32 ( if (x < 9) (x = 1) else (x = 2), x )\nf()\n",
    );
    assert_eq!(echoes.len(), 4, "stderr: {stderr}");
    assert_ne!(echoes[0], "0", "if was declared in the session scope");
    assert_eq!(&echoes[1..], ["true", "true", "1"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn assignment_returns_nothing() {
    let (_e, stderr) = repl(b"mut a := i32 1\nmut b := i32 2\na = b = 3\n");
    assert!(stderr.contains("yields no value"), "stderr: {stderr}");
    let (_e, stderr) = repl(b"mut a := i32 1\ny := (a = 2) + 1\n");
    assert!(!stderr.is_empty(), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"p := type (fields = (mut v := i32 ?))\nmut q := p(1)\nq.v = 3\nq.v\nmut a := i32 1\na = a + 1\na\n",
    );
    assert_eq!(echoes, ["3", "2"], "stderr: {stderr}");
}

#[test]
fn a_write_along_a_path_needs_mut_on_every_step() {
    for (src, expect) in [
        (&b"p := type (fields = (mut v := i32 ?))\nq := p(1)\nq.v = 3\n"[..], "`q` is not `mut`"),
        (b"p := type (fields = (v := i32 ?))\nmut q := p(1)\nq.v = 3\n", "`v` is not `mut`"),
        (b"t := type (fields = (shared y := i32 3))\nt.fields.y = 4\n", "`y` is not `mut`"),
        (b"t := type (fields = (shared y := i32 3))\nq := t()\nq.y = 4\n", "`y` is not `mut`"),
        (b"p := type (fields = (immut v := i32 ?))\nmut q := p(1)\nq.v = 3\n", "`v` is `immut`"),
    ] {
        let (_e, stderr) = repl(src);
        assert!(stderr.contains(expect), "{}: stderr: {stderr}", String::from_utf8_lossy(src));
    }
    let (echoes, stderr) = repl(
        b"t := type (fields = (shared mut y := i32 3, v := i32 ?))\nq := t(1)\nt.fields.y = 4\nq.y\nq.y = 5\nt.fields.y\n",
    );
    assert_eq!(echoes, ["4", "5"], "stderr: {stderr}");
}

#[test]
fn a_tight_read_lexes_its_right_cell_on_demand_and_stops_at_a_boundary() {
    let (echoes, stderr) = repl(
        b"x := i32 5\n(x:end, 3)\np := type (fields = (a := i32 ?))\nq := p(1)\n(q.a, 2)\nq.a\n\
          r := &q\nr@.a\npp := &r\npp@@.a\nx:type == i32\n",
    );
    assert_eq!(echoes, ["3", "2", "1", "1", "1", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_type_box_is_an_ordinary_variable() {
    let (echoes, stderr) =
        repl(b"mut a := type ?\na = i32\na == i32\na = f64\na == f64\na == i32\n");
    assert_eq!(echoes, ["true", "true", "false"], "stderr: {stderr}");

    let (echoes, stderr) =
        repl(b"mut a := type ?\na = i32\nx := a 5\nx\na = f64\ny := a 2.5\ny\na\n");
    assert_eq!(echoes, ["5", "2.5", "f64"], "stderr: {stderr}");

    let (echoes, stderr) = repl(b"b := type ?\nz := b ?\n");
    assert!(
        echoes.is_empty() && stderr.contains("known only when the program runs"),
        "stderr: {stderr}"
    );
}

#[test]
fn the_dyad_box_says_what_it_holds() {
    let (echoes, stderr) = repl(b"mut a := dyad ?\na\na = i32\na:type == type\na == i32\na\n");
    assert_eq!(echoes, ["dyad ?", "true", "true", "i32"], "stderr: {stderr}");

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

    let (_e, stderr) = repl(b"mut a := dyad ?\na = 5\n");
    assert!(stderr.contains("must be a type value"), "stderr: {stderr}");
}

#[test]
fn only_a_marked_place_is_written_or_addressed() {
    let pw = "pw := type ( fields = ( mut a := i32 ?, mut b := i32 ?, output := type ?, shared run = ( this.a * this.b ) ), \
              parse_rank = *.parse_rank + 1, associativity = right, \
              parse = ( this.a = tape[-1], this.b = tape[1], this.output = i32, tape[0] = this, \
              tape.is_constructed[0] = true, tape.remove(1), tape.remove(-1) ) )";
    for (src, expect) in [
        ("i32 5 = 3\n", "not an assignable place"),
        ("mut a := 5\na = 6\n", "`5` is a literal with no storage"),
        ("mut a := 5\na = 6\n", "as in `mut x := i32 5`"),
        ("x := &(i32 5)\n", "needs a variable"),
        (&format!("{pw}\np := &(2 pw 3)\n"), "needs a variable"),
    ] {
        let (_e, stderr) = repl(src.as_bytes());
        assert!(stderr.contains(expect), "{src}: stderr: {stderr}");
    }
    let (echoes, stderr) = repl(
        b"mut x := i32 5\nx = 6\np := &x\np@\nw := type (fields = (y := i64 ?))\nq := w(7)\nr := &q\nr@.y\n\
          mut a := type ?\nmut b := type ?\na = i32\nb = a\nb == i32\nmut d := dyad ?\nd = i32\nd:type == type\n",
    );
    assert_eq!(echoes, ["6", "7", "true", "true"], "stderr: {stderr}");
}

#[test]
fn declaring_from_a_box_copies_it() {
    let (echoes, stderr) = repl(
        b"mut a := type ?\na = i32\nmut x := a\nx = f64\na == i32\nx == f64\n\
          mut d := dyad ?\nd = i32\nmut e := d\ne = f64\nd == i32\ne == f64\n",
    );
    assert_eq!(echoes, ["true", "true", "true", "true"], "stderr: {stderr}");
    let (echoes, stderr) = repl(b"t := i32\ny := t 5\ny\n");
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    let (_e, stderr) = repl(b"b := bool ?\n");
    assert!(stderr.contains("not in the seed yet"), "stderr: {stderr}");
}

#[test]
fn a_dyad_is_built_from_a_type_and_a_value() {
    let (echoes, stderr) =
        repl(b"c := dyad (i32, 7)\nc\nc:type == i32\ndyad (i32, 7):type == i32\nc + 1\n");
    assert_eq!(echoes, ["7", "true", "true", "8"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
    let (_e, stderr) = repl(b"dyad (i32)\n");
    assert!(!stderr.is_empty(), "two operands, not one");

    // A bool cell used to carry the node as its value, so every bool dyad read true.
    let (echoes, stderr) = repl(b"dyad (bool, true)\ndyad (bool, false)\n");
    assert_eq!(echoes, ["true", "false"], "stderr: {stderr}");

    for src in [
        &b"dyad (bool, 0)\n"[..],
        b"p := dyad (@i32, 5)\n",
        b"dyad (void, 0)\n",
        b"g := type (fields = (x := i32 ?))\ndyad (g, 5)\n",
    ] {
        let (echoes, stderr) = repl(src);
        assert!(
            echoes.is_empty() && !stderr.is_empty(),
            "{}: stderr: {stderr}",
            String::from_utf8_lossy(src)
        );
    }
}

/// Run one command line and return its stdout, asserting success.
fn line(src: &str) -> String {
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

#[test]
fn an_import_tail_runs_once() {
    assert_eq!(line("import ./tests/fixtures/counter.logos, c@"), "1");
    let (echoes, stderr) =
        repl(b"import ./tests/fixtures/counter.logos\nimport ./tests/fixtures/counter.logos\nc@\n");
    assert_eq!(echoes, ["1", "1", "1"], "stderr: {stderr}");
}

#[test]
fn the_pass_runs_only_as_far_as_it_must_in_order_and_never_twice() {
    let bump = "mut x := i32 0, bump := fn () -> i32 ( x = x + 1, x )";
    assert_eq!(line(&format!("{bump}, bump() + ( x = 10, x )")), "11");
    let add = "f := fn (mut a := i32 ?, b := i32 ?) -> i32 ( a + b )";
    assert_eq!(line(&format!("{bump}, {add}, f(bump(), bump()) * 10 + x")), "32");
    assert_eq!(line("mut x := i32 0, while (x < 3) ( x = x + 1 ), x"), "3");
    // `bump()` once gives 7; twice would give 9.
    let block = "y := ( mut a := type ?, a = i32, bump(), z := a 5, z + x ), y + x";
    assert_eq!(line(&format!("{bump}, {block}")), "7");
    assert_eq!(line("y := ( mut a := type ?, a = i32, mut x := a 5, x ) + 1, y"), "6");
    let (echoes, stderr) = repl(
        b"( mut a := type ?, a = i32, mut x := a 5, x )\nq := ( p := @i32 ?, p@ )\nq := i32 4\nq\n",
    );
    assert_eq!(echoes, ["5", "4"], "stderr: {stderr}");
    assert!(stderr.contains("holds nothing yet"), "stderr: {stderr}");
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
        "twice := type (parse_rank = *.parse_rank + 1, parse = ( tape.insert(1, lex «* 2»), tape.remove(0) ))\n\
          5 twice\n3 + 4 twice\n"
            .as_bytes(),
    );
    assert_eq!(echoes, ["10", "11"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        "t := lex «(a, b)»\n5\n\
          bad := type (parse = ( tape.insert(1, lex «zz»), tape.remove(0) ))\n\
          x := bad\n"
            .as_bytes(),
    );
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    assert!(stderr.contains("unknown name `zz`"), "stderr: {stderr}");
    let (_echoes, stderr) = repl(
        b"y := lex 5\n\
          bad2 := type (parse = ( tape.insert(1, dyad (i32, 1)) ))\n",
    );
    assert!(stderr.contains("`lex` must be followed by a «…» quote"), "stderr: {stderr}");
    assert!(stderr.contains("`insert` splices a tape"), "stderr: {stderr}");
}

#[test]
fn print_writes_its_quote_each_time_it_runs_and_is_a_statement() {
    let out = logos()
        .arg("print «a», f := fn () -> i32 ( print «in f», 7 ), f(), f(), print «»")
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\nin f\nin f\n\n7\n");

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
             f := fn (n := i32 ?) -> i32 ( print «n={n}», n ), f(3), f(4)",
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "answer 42\ntrue 5.5 {x} a\\b\nn=3\nn=4\n4\n");

    // A constructor's print runs at parse time.
    let (echoes, stderr) =
        repl("m := type (parse = ( print «parsing {1 + 1}», tape.remove(0) ))\n1 m\n".as_bytes());
    assert_eq!(echoes, ["parsing 2", "1"], "stderr: {stderr}");

    for (src, message) in [
        ("print «{x»", "this `{` has no `}`"),
        ("print «x}»", "this `}` closes no `{`"),
        ("print «{nope}»", "unknown name `nope`"),
        ("print «{1, 2}»", "expected one expression"),
        ("print «{}»", "nothing to evaluate"),
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
         w := type (parse = ( seen = caller.scope, tape.remove(0) ))\n\
         g := fn () -> i32 ( 1 w )\n\
         seen == here.scope\n\
         seen.back.back == here.scope\n\
         g()\n\
         h := fn () -> i32 ( seen = here.scope, 2 )\n\
         h()\n\
         seen.back.back == here.scope\n"
            .as_bytes(),
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
         s.dyads.size\n\
         s.dyads[0].lhs:name\n\
         s.dyads[0].rhs:type == i32\n\
         s.dyads[1].rhs.lhs:name\n\
         s.back == here.scope\n\
         s == g:start.rhs\n\
         t := s\n\
         t.dyads[3]:name\n\
         n\n\
         here.scope.dyads[1].lhs:name\n\
         ( c := i32 1, here.scope.dyads.size )\n"
            .as_bytes(),
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
fn a_declaration_on_the_command_line_is_its_names_start() {
    let out = logos().arg("x := i32 1, y := x + 2, y:start.rhs.lhs:name").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "x\n");
}

#[test]
fn a_nodes_fields_are_read_by_name_without_running_it() {
    let power = "^ := type ( fields = ( lhs := ?, rhs := i32 ?, output := type ?, \
                 shared run = ( mut r := this.output 1, for 0..this.rhs ( r = r * this.lhs ), r ) ), \
                 parse_rank = *.parse_rank + 1, associativity = right, \
                 parse = ( this.lhs = tape[-1], this.rhs = tape[1], this.output = tape[-1]:type, \
                 tape[0] = this, tape.is_constructed[0] = true, tape.remove(1), tape.remove(-1) ) )";
    for (tail, want) in [
        ("(2 ^ 3).lhs", "2"),
        ("(2 ^ 3).rhs", "3"),
        ("f := fn (x := i32 ?) -> i32 ( (x ^ 3).lhs ), f(5)", "5"),
        ("x := i32 1, (x + 2).lhs", "1"),
    ] {
        let out = logos().arg(format!("{power}, {tail}")).output().unwrap();
        assert!(out.status.success(), "{tail}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{want}\n"), "{tail}");
    }
}

#[test]
fn a_pointer_type_applies_to_any_type() {
    let (echoes, stderr) = repl(
        "mut p := @dyad ?\n\
         p = here.scope\n\
         p == here.scope\n\
         f := fn (s := @dyad ?) -> i32 ( p = s, 1 )\n\
         x := i32 5\n\
         f(x:scope)\n\
         p == here.scope\n\
         g := fn () -> i32 ( f(here.scope) )\n\
         g()\n\
         p.back.back.back == here.scope\n\
         v := @void ?\n\
         t := fn (s := @void ?) -> i32 ( 1 )\n"
            .as_bytes(),
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
         f(q)\n\
         f(px)\n\
         pt := type (fields = (h := @@i32 ?))\n\
         v := pt(q)\n\
         v.h@@\n\
         w := pt(px)\n"
            .as_bytes(),
    );
    assert_eq!(echoes, ["7", "7"], "stderr: {stderr}");
    assert_eq!(stderr.matches("these types do not match").count(), 2, "stderr: {stderr}");
}

#[test]
fn a_constructors_outcome_is_read_off_its_own_cell() {
    let (echoes, stderr) = repl(
        b"r1 := type (parse = ( tape.is_constructed[0] = true, tape.recenter(1) ))\n\
          r2 := type (parse = ( tape.is_constructed[0] = true, tape.recenter(-1) ))\n\
          r3 := type (parse = ( tape.is_constructed[0] = true, tape.recenter(99) ))\n\
          a := r1\nb := r2\nc := r3\na:type == type\nb:type == type\nc:type == type\n",
    );
    assert_eq!(echoes, ["true", "true", "true"], "stderr: {stderr}");
    for src in [
        &b"r4 := type (parse = ( tape.recenter(1) ))\nx := r4\n"[..],
        b"r5 := type (parse = ( tape[0] = tape.spelling[0] ))\nf := fn () -> void ( r5 )\n",
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
        b"as_i32 := type (parse = ( tape[0] = i32 ))\n\
          x := as_i32 5\nx + 1\n",
    );
    assert_eq!(echoes, ["6"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(
        "half := type (parse = ( tape.insert(1, lex «/ 2») ))\n\
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
