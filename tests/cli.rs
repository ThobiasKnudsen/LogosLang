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
fn logos_stats_measures_the_store_on_stderr() {
    let out =
        logos().env("LOGOS_STATS", "1").args(["import", "examples/answer.logos"]).output().unwrap();
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
fn a_value_reads_its_type_and_a_reached_node_its_operands() {
    // `(x + x)` has arity 4: its third slot is the resolved callable leaf, its fourth the
    // type it gives back.
    let (echoes, stderr) = repl(
        b"x := i32 5\nx.type == i32\nb := x + x\nb:start.rhs.type.arity\n\
          b:start.rhs.lhs.type == i32\nb:start.rhs.lhs\nb:start.rhs.output_type == i32\n",
    );
    assert_eq!(echoes, ["true", "4", "true", "5", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_type_body_fills_its_slots_and_declares_its_members() {
    let (echoes, stderr) = repl(
        b"t := type (share parse_rank = *.parse_rank + 1, share associativity = right)\n\
          t.parse_rank\nt.associativity == right\nright.type == type\n",
    );
    assert_eq!(echoes, ["71.0", "true", "true"], "stderr: {stderr}");
    let (echoes, stderr) =
        repl(b"g := type (share y := 3, share z := y + 3)\ng.y\ng.z\ng.parse_rank\n");
    assert_eq!(echoes, ["3", "6", "91.0"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"g := type (share y := i32 7, share z := i32 (y + 3))\n\
          g.y\ng.z\ng.y + 1\n",
    );
    assert_eq!(echoes, ["7", "10", "8"], "stderr: {stderr}");
    let (echoes, stderr) =
        repl(b"p := type (share k := 10, v := i32 ?)\nq := p(2)\nq.v\nq.k\np.k\np.size_bytes\n");
    assert_eq!(echoes, ["2", "10", "10", "4"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"t := type (share # \xc2\xabnote\xc2\xbb y := 3, share z := 4 # \xc2\xabtail\xc2\xbb, v := i32 ?)\n\
          t.y\nt.z\nt.size_bytes\n",
    );
    assert_eq!(echoes, ["3", "4", "4"], "stderr: {stderr}");
    let (echoes, stderr) =
        repl(b"h := fn () -> i32 ( n := i64 0 - 1, p := alloc n of i32 0, 1 )\ng := type (share y := h())\n");
    assert!(
        echoes.is_empty() && stderr.contains("a type body's own declaration failed"),
        "stderr: {stderr}"
    );
    let (echoes, stderr) =
        repl(b"r := type (share lex_rank = 3)\nr:lex_rank\n+:lex_rank\nr.lex_rank\n");
    assert_eq!(echoes, ["3.0", "0.0"], "stderr: {stderr}");
    assert!(!stderr.is_empty(), "`.lex_rank` is no member of a type");
    let (echoes, stderr) = repl(
        b"r := type (share lex_rank = 3)\ns := r\ns:lex_rank\ns:lex_rank = 7\ns:lex_rank\nr:lex_rank\n",
    );
    assert_eq!(echoes, ["0.0", "7.0", "3.0"], "stderr: {stderr}");
    let (_echoes, stderr) =
        repl(b"f := fn (t := type ?) -> void ( )\nf(type (share lex_rank = 1))\n");
    assert!(stderr.contains("lex_rank is the name's"), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"s := type (precedence = 5)\n");
    assert!(echoes.is_empty() && stderr.contains("unknown name"), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"c := type (share run = 5)\n");
    assert!(echoes.is_empty() && stderr.contains("`share run = (…)`"), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"x := 1\np := type (x := i32 ?)\nq := p(2)\nq.x\nx\n");
    assert_eq!(echoes, ["2", "1"], "stderr: {stderr}");
}

#[test]
fn any_spelling_the_index_can_hold_is_nameable() {
    let (echoes, stderr) = repl(
        b"^ := i32 5\n^ + 1\na := i32 1\nab2 := i32 2\nab2 + a\nmut x := i32 5\nx=-1\nx\n\
          p := type (v := i32 ?)\nq := p(7)\nr := &q\nrr := &r\nrr@@.v\nx^2\n",
    );
    assert_eq!(echoes, ["6", "3", "-1", "7"], "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:2: error:"), "stderr: {stderr}");
    // caret.logos: 9 + 512 + 18.
    let out = logos().args(["import", "tests/fixtures/caret.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "539\n");
}

#[test]
fn a_type_s_size_bytes_is_a_u64_for_records_numbers_and_pointers() {
    let (echoes, stderr) = repl(
        b"i32.size_bytes\nu8.size_bytes\n(@u8).size_bytes\nn := u64 3\nn * i32.size_bytes\n\
          p := type (a := i32 ?, b := i64 ?)\nn * p.size_bytes\n",
    );
    assert_eq!(echoes, ["4", "1", "8", "12", "36"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_power_demo_prints_nine() {
    let out = logos().args(["import", "identities/power.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "9\n");
}

#[test]
fn the_power_demo_takes_floats_fractions_and_negative_exponents() {
    // The demo's own `^`, its last line swapped for each case.
    let demo = std::fs::read_to_string("identities/power.logos").unwrap();
    // Given on the command line, its imports resolve against the working directory.
    let body =
        demo.trim_end().rsplit_once('\n').unwrap().0.replace("import ", "import identities/");
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
fn a_conversion_reads_a_rational_value_when_it_runs() {
    let (code, stdout, stderr) = run_line(
        "q := type ( a := ?, output_type := type ?, share run = ( f64(a) * 2.0 ), \
         share parse_rank = 60, share parse = ( tape[0].type = q, tape[0].a = tape[1], \
         tape[0].output_type = f64, tape.remove(1), tape.is_constructed[0] = true ) ), q 0.75",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "1.5\n"), "stderr: {stderr}");
}

#[test]
fn a_chooser_hands_its_cell_to_a_type_minted_at_run() {
    let src = "mk := fn (t := type ?) -> type ( type ( share parse = ( tape.is_constructed[0] = true ) ) ), \
               c := type ( share parse_rank = fn.parse_rank, share associativity = right, share parse = ( \
                 if tape[1].type != type error «no», t := tape[1], tape[0] = mk(t), tape.remove(1) ) ), \
               x := c i32, x";
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "type\n");
}

#[test]
fn a_tape_read_checked_against_a_number_type_reads_as_that_number() {
    let q = |ty: &str, body: &str| {
        format!(
            "q := type ( v := {ty} ?, share parse_rank = 61, \
             share parse = ( tape[0].type = q, {body}, \
             tape.remove(1), tape.is_constructed[0] = true ) )"
        )
    };
    for (src, expect) in [
        (q("u64", "if not tape[1].dyads[0].type ⊆ u64 error «no», tape[0].v = tape[1].dyads[0] + 1") + ", x := q [5], x.v", "6"),
        (q("i32", "if tape[1].dyads[1].type == i32 ( tape[0].v = tape[1].dyads[1] * 2 ) else ( tape[0].v = i32 0 )") + ", x := q [5, i32 7], x.v", "14"),
        (q("u8", "if tape[1].dyads[0].type ⊆ u8 ( tape[0].v = tape[1].dyads[0] ) else ( tape[0].v = u8 1 )") + ", x := q [300], x.v", "1"),
        (q("i32", "mut s := i32 0, for i in 0..tape[1].dyads.size ( if not tape[1].dyads[i].type ⊆ i32 error «no», s = s + tape[1].dyads[i] ), tape[0].v = s") + ", x := q [4, 5, 6], x.v", "15"),
        (q("u64", "if not tape[1].type ⊆ u64 error «no», tape[0].v = tape[1] * 3") + ", x := q 5, x.v", "15"),
    ] {
        let out = logos().arg(&src).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), expect, "{src}");
    }
    // A literal that does not fit fails the check; a line that is no constant is refused.
    for (src, expect) in [
        (
            q(
                "u64",
                "if not tape[1].dyads[0].type ⊆ u64 error «not a u64», tape[0].v = tape[1].dyads[0]",
            ) + ", x := q [-5]",
            "not a u64",
        ),
        (
            "y := i32 3, ".to_owned()
                + &q(
                    "i32",
                    "if not tape[1].dyads[0].type ⊆ i32 error «no», tape[0].v = tape[1].dyads[0]",
                )
                + ", x := q [y]",
            "holds no literal or constant",
        ),
    ] {
        let out = logos().arg(&src).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && stderr.contains(expect), "{src}: stderr: {stderr}");
    }
}

#[test]
fn an_instance_takes_its_bracket_as_a_call_built_in_its_parse() {
    // `x[k]` places the call `x.get(k)`, the line `k` its operand, run where it stands.
    let q = "q := type ( n := u64 ?, share get := fn (i := u64 ?) -> u64 ( n + i ), \
             share parse_rank = 61, share parse = ( \
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
    let array = "import ./identities/array.logos";
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
    let array = "import ./identities/array.logos";
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
            "t := array i32, g := fn (x := i32 ?) -> t ( array i32 [x, x + 1] )",
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
    let array = "import ./identities/array.logos, a := array i32 [1, 2, 3]";
    for (tail, printed) in [
        ("b := a, b[1]", "2"),
        ("f := fn (p := array i32 ?) -> i32 ( p[1] ), f(a)", "2"),
        ("f := fn (p := array i32 ?) -> i32 ( p[1] + p[2] ), f.compile(), f(a)", "5"),
        ("f := fn (p := array i32 ?) -> u64 ( p.size_bytes() + p.size ), f(a)", "15"),
        // `b := a` shares the array: a write through `b` is seen through `a`.
        ("b := a, b.ptr@ = i32 9, a[0]", "9"),
        (
            "t := array i32, mk := fn (v := i32 ?) -> t ( c := array i32 [v, 1], c ), \
             m := mk(3), n := mk(4), m[0] + n[0]",
            "7",
        ),
        // The return type stops before the body's bracket, so the mint takes no elements there.
        ("mk := fn () -> array i32 ( array i32 [1, 2] ), m := mk(), m[1]", "2"),
        ("mk := fn (v := i32 ?) -> array i32 ( array i32 [v, v + 1] ), m := mk(4), m[1]", "5"),
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
    let array = "import ./identities/array.logos, a := array i32 [1, 2, 3], \
                 mk := fn () -> array i32 ( array i32 [1, 2] )";
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
fn a_share_function_called_through_a_record_built_by_application_reads_its_fields() {
    let p = "p := type ( mut x := i32 ?, y := i32 ?, share s := fn () -> i32 ( x + y ), \
             share add := fn (k := i32 ?) -> i32 ( x + y + k ), share d := fn () -> i32 ( s() * 2 ), \
             share bump := fn () -> i32 ( x = x + 10, x ) ), q := p (1, 2)";
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
        "p := type ( x := u8 ?, y := i64 ?, z := f64 ?, share s := fn () -> f64 ( z ), \
         share t := fn () -> i64 ( y * 2 ), share u := fn () -> u8 ( x ) ), q := p (u8 3, -9, 2.5), \
         f := fn () -> i64 ( q.t() ), f.compile(), print «{q.s()} {f()} {q.u()}»",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "2.5 -18 3\n"), "stderr: {stderr}");
}

#[test]
fn a_share_function_writes_the_value_it_is_called_on() {
    let p = "p := type ( mut x := i32 ?, y := i32 ?, share get := fn () -> i32 ( x + y ), \
             share bump := fn () -> i32 ( x = x + 10, x ), share twice := fn () -> i32 ( bump(), bump() ) )";
    for (tail, want) in [
        ("mut m := p (1, 2), m.bump(), m.x", "11\n"),
        ("mut m := p (1, 2), m.twice(), m.x", "21\n"),
        ("mut m := p (1, 2), f := fn () -> i32 ( m.bump() ), f.compile(), f(), m.x", "11\n"),
        ("f := fn () -> i32 ( mut r := p (3, 4), r.bump(), r.x ), f()", "13\n"),
        ("f := fn () -> i32 ( mut r := p (3, 4), r.bump(), r.x ), f.compile(), f()", "13\n"),
        ("f := fn (mut r := p ?) -> i32 ( r.twice(), r.x ), f(p (1, 2))", "21\n"),
        ("f := fn (mut r := p ?) -> i32 ( r.twice(), r.x ), f.compile(), f(p (1, 2))", "21\n"),
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
         share s := fn () -> i64 ( x = x + u8 1, y = y * 2, z = z + 1.0, y ) ), \
         mut q := p (u8 3, -9, 2.5), f := fn () -> i64 ( q.s() ), f.compile(), \
         print «{f()} {q.x} {q.y} {q.z}»",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "-18 4 -18 3.5\n"), "stderr: {stderr}");
    // A node a parse built is written through too, and meets the same gate.
    let q = "q := type ( mut n := u64 ?, share bump := fn () -> u64 ( n = n + 10, n ), \
             share get := fn () -> u64 ( n ), share parse_rank = 61, share parse = ( \
             if tape[0].type == type ( tape[0].type = q, tape[0].n = tape[1], tape.remove(1) ), \
             tape.is_constructed[0] = true ) )";
    for (tail, want) in
        [("mut m := q u64 1, m.bump(), m.get()", "11\n"), ("m := q u64 1, m.get()", "1\n")]
    {
        let (code, stdout, stderr) = run_line(&format!("{q}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "{tail}: stderr: {stderr}");
    }
    let (code, _, stderr) = run_line(&format!("{q}, m := q u64 1, m.bump()"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("`m` is not `mut`"), "stderr: {stderr}");
}

#[test]
fn a_share_function_writes_only_a_mut_field() {
    let p = |gate: &str| {
        format!(
            "p := type ( {gate}x := i32 ?, share bump := fn () -> i32 ( x = x + 10, x ) ), \
             mut m := p (1), m.bump(), m.x"
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
    let out = logos().args(["import", "./identities/array.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    for (tail, printed) in [("a[1]", "2\n"), ("a.size_bytes()", "12\n")] {
        let src = format!("import ./identities/array.logos, a := array i32 [1, 2, 3], {tail}");
        let out = logos().arg(&src).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), printed, "{src}");
    }
    let array = "import ./identities/array.logos, a := array i32 [1, 2, 3]";
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
        "t := type ( mut a := i32 0, mut b := i32 0, share parse = ( tape.is_constructed[0] = true ) ), \
               f := fn () -> i32 ( mut v := t ?, v.a = 3, v.b = 4, v.a + v.b ), f()";
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "7");
}

#[test]
fn an_index_of_any_integer_type_reads_inside_the_array() {
    let array = "import ./identities/array.logos, a := array i32 [1, 2, 3]";
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
    let array = "import ./identities/array.logos, a := array i32 [1, 2, 3]";
    for (tail, printed) in [
        ("a[0] = i32 9, a[0]", "9"),
        ("a[0] = 9, a[0] + a[1]", "11"),
        ("i := u64 2, a[i] = 7, a[2]", "7"),
        ("j := u64 0, a[j + 1] = 5, a[1]", "5"),
        ("a[0] = a[2], a[0]", "3"),
        ("a.at(1) = 4, a[1]", "4"),
        ("b := a, b[0] = i32 7, a[0]", "7"),
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
        ("x := u8 3, a[0] = x", "these types do not match"),
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
fn a_shared_name_in_a_function_is_made_once_at_the_definition() {
    let src = "f := fn () -> i32 ( share n := (print «made», i32 5), n ), \
               print «defined», f(), f(), f()";
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "made\ndefined\n5\n");
    // A type built per call reads the name, and the second call finds the first's.
    let src = "get := fn (t := type ?) -> type ( share m := hashmap type -> type, mut r := m[t], \
               if r != ? return r, print «miss», r = type ( x := t ?, share seen := m ), \
               m[t] = r, r ), get(i32) == get(i32)";
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "miss\ntrue\n");
    // An owning value is freed when the program ends, not at each call's end.
    let src = "f := fn () -> i32 ( share p := alloc 1 of i32 7, p@ ), f() + f()";
    let out = logos().arg(src).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "14\n");
}

/// The power operator with the bare run body, on one line for the REPL.
const POWER: &str = "^ := type ( lhs := ?, rhs := i32 ?, output_type := type ?, \
    share run = ( mut r := output_type 1, for 0..rhs ( r = r * lhs ), r ), \
    share parse_rank = *.parse_rank + 1, share associativity = right, \
    share parse = ( tape[0].type = ^, tape[0].lhs = tape[-1], tape[0].rhs = tape[1], tape[0].output_type = tape[-1].type, \
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
fn a_shared_member_is_read_through_a_node_and_through_the_type() {
    let point = "point := type ( x := i32 ?, y := i32 ?, share dims := i32 2 ), \
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
    let out = logos().args([&format!("{POWER}, r := ^.run, ^.run")]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "dyad");
}

#[test]
fn a_value_runs_its_type_s_parse_as_tape_0() {
    // The type builds the value and hands the cell on; the same parse then
    // reads the value as `tape[0]` and consumes the cell to its right.
    let q = "q := type ( size := ?, share parse_rank = 61, share parse = ( \
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
        .args(["q := type ( size := ?, share parse_rank = 61, share parse = ( \
                if tape[0].type == type ( tape[0].type = q, tape[0].size = tape[1], tape.remove(1) ) ) ), \
                q i32 5"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("left its own cell unconstructed"), "stderr: {stderr}");
}

#[test]
fn a_shared_member_read_through_tape_0_is_the_member_itself() {
    // `tape[0].element_type` folds to the identity, so `alloc … of` has a static pointee.
    let q =
        "q := type ( share element_type := i32, v := ?, share parse_rank = 60, share parse = ( \
             tape[0] = alloc 1 of tape[0].element_type 9, tape.is_constructed[0] = true ) )";
    let out = logos().args([&format!("{q}, a := q, a@ + 1")]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "10");
    let out = logos()
        .args(["q := type ( share element_type := i32, v := ?, share parse_rank = 60, \
                share parse = ( tape[0].type = q, tape[0].element_type = i64, tape.is_constructed[0] = true ) ), q"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not an assignable place"), "stderr: {stderr}");
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
        POWER.replace("r = r * lhs", "r = r * lhs * k")
    );
    let out = logos().args([&before]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "100");
    let after = format!(
        "{}, k := i32 2, f := fn (x := i32 ?) -> i32 ( x ^ 2 ), f(5)",
        POWER.replace("r = r * lhs", "r = r * lhs * k")
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
        b"regex \xc2\xab[0-9]+[kK]\xc2\xbb := type ()\n(5k).type == type\n\
          regex \xc2\xab<=>\xc2\xbb := type ()\n(<=>).type == type\n\
          regex \xc2\xab\\.\\.\xc2\xbb := type ()\n",
    );
    assert_eq!(echoes, ["true", "true"], "stderr: {stderr}");
    assert!(stderr.contains("shadowed"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"regex \xc2\xab[a-z][0-9]\xc2\xbb := type ()\nregex \xc2\xaba[0-9]\xc2\xbb := type ()\na1\n\
          regex \xc2\xabb[0-9]\xc2\xbb := type (share lex_rank = 1)\n(b1).type == type\n",
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
fn a_pattern_whose_class_can_eat_its_own_literal_still_lexes() {
    let (echoes, stderr) = repl(
        b"regex \xc2\xab[a-z]+ing\xc2\xbb := type ()\n(running).type == type\n(ring).type == type\n\
          regex \xc2\xab[0-9]+(?:[0-9]k)?x\xc2\xbb := type ()\n(12kx).type == type\n",
    );
    assert_eq!(echoes, ["true", "true", "true"], "stderr: {stderr}");
}

#[test]
fn a_constructor_written_in_logos_runs_during_the_parse() {
    let out = logos().args(["import", "tests/fixtures/squared.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "92\n");
    // A parse that consumes nothing sets its flag and stands as itself.
    let (echoes, stderr) = repl(
        b"noop := type (share parse = ( tape.is_constructed[0] = true ))\n\
          t := noop\nt.type == type\nnoop.parse_rank\n",
    );
    assert_eq!(echoes, ["true", "91.0"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"bad := type (share parse = ( tape[5] = i32 1 ))\nx := bad\n");
    assert!(stderr.contains("constructor failed"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"named := type (share parse = ( tape[0] = tape.spelling[0], tape.is_constructed[0] = true ))\n\
          f := fn () -> void ( named )\n5\n",
    );
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
}

#[test]
fn a_binding_carries_its_spelling() {
    let (echoes, stderr) = repl(b"x := 5\nx:name\nif:name\n");
    assert_eq!(echoes, ["x", "if"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"a := i32 1\n(a + 1):name\n");
    assert!(stderr.contains("expected a field name"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"sp := type (share parse = ( tape[0] = tape[-1]:name, tape.is_constructed[0] = true, tape.remove(-1) ))\n\
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
          m := type (a := i32 ?, output_type := type ?, share run = ( a + a ), \
          share parse_rank = *.parse_rank + 1, \
          share parse = ( tape[0].type = m, tape[0].a = tape[-1], tape[0].output_type = i32, \
          tape.is_constructed[0] = true, tape.remove(-1) ))\n\
          x := i32 3\nx m\nrun\nparse_rank := 4\nparse_rank\n",
    );
    assert_eq!(echoes, ["6", "6", "6", "5", "4"], "stderr: {stderr}");
}

#[test]
fn a_slot_body_is_read_bare() {
    // The hidden `tape` is declared as a parameter is, so an outer `tape` is the shadowing error.
    let (echoes, stderr) = repl(
        b"sp := type (share parse = ( tape[0] = tape[-1]:name, tape.is_constructed[0] = true, tape.remove(-1) ))\n\
          x := 1\nf := fn () -> void ( x sp )\n5\n",
    );
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"tape := 1\nsp := type (share parse = ( tape.recenter(0) ))\n");
    assert!(stderr.contains("shadowed"), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"minus := type (a := i32 ?, b := i32 ?, output_type := type ?, share run = ( a - b ), \
          share parse_rank = +.parse_rank, share associativity = left, \
          share parse = ( tape[0].type = minus, tape[0].a = tape[-1], tape[0].b = tape[1], tape[0].output_type = i32, \
          tape.is_constructed[0] = true, tape.remove(1), tape.remove(-1) ))\n\
          7 minus 2\n10 minus 2 minus 3\nf := fn (x := i32 ?) -> i32 ( x minus 1 )\nf.compile()\nf(9)\nthis\n",
    );
    assert_eq!(echoes, ["5", "5", "8"], "stderr: {stderr}");
    assert!(stderr.contains("unknown name `this`"), "stderr: {stderr}");
    // A body constructed in the session's section reaches the core names, so a
    // run that calls a function whose body reads an operator constructs.
    let (echoes, stderr) = repl(
        b"sq := fn (a := i32 ?) -> i32 ( a * a )\n\
          squared := type (a := i32 ?, output_type := type ?, share run = ( sq(a) ), \
          share parse_rank = *.parse_rank + 1, \
          share parse = ( tape[0].type = squared, tape[0].a = tape[-1], tape[0].output_type = i32, \
          tape.is_constructed[0] = true, tape.remove(-1) ))\n\
          x := i32 4\nx squared\n",
    );
    assert_eq!(echoes, ["16"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"t := type (a := i32 ?, share run = ( s := \xc2\xaba ) b\xc2\xbb, \
          # \xc2\xab ) \xc2\xbb (( x[0] ), 5 )))\n\
          t.type == type\n",
    );
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
    let out = logos()
        .args(["t := type (share run = ( x[0], # c ) d\n5 )),\n\
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
               share y := i32 2,\n\
               # a slot\n\
               share parse_rank = 5\n\
               ), q := t (4), q.x + t.y + i32 (t.parse_rank)";
    let (code, stdout, stderr) = run_line(src);
    assert_eq!((code, stdout.as_str()), (Some(0), "11\n"), "stderr: {stderr}");
}

#[test]
fn a_type_body_refuses_what_is_not_its_own() {
    for (src, expect) in [
        (&b"t := type (parse_rank := 5)\n"[..], "shadowed"),
        (b"g := type (share)\n", "followed by a declaration"),
        (b"f := fn (share a := i32 ?) -> void ( a )\n", "never on a parameter"),
        (b"share parse_rank = 3\n", "unknown name"),
        (b"d := i32 5\nfree = 3\n", "a line of the type body itself"),
        (b"t := type (share parse = ( parse_rank = 3 ))\n", "a line of the type body itself"),
        (b"t := type (share parse = ( share parse_rank = 3 ))\n", "followed by a declaration"),
        (b"t := type (lex_rank = 5)\n", "its fill says so"),
        (b"t := type (parse = ( 1 ))\n", "its fill says so"),
        ("t := type (# «note» parse = ( 1 ))\n".as_bytes(), "its fill says so"),
        (b"x := 1\nt := type (x = 5)\n", "a type body line"),
        (b"t := type (fields = (a := i32 ?))\n", "unknown name `fields`"),
        (b"t := type (share associativity = 5)\n", "`left` or `right`"),
        (b"t := type (share free = 5)\n", "`share free = (…)`"),
        (b"t := type (share drop = ( 0 ))\n", "`drop` names no slot"),
        (b"t := type (share parse = ( tape[0].a = tape[-1] ))\n", "no field `a`"),
        (b"t := type (a := ?, share parse = ( tape[0].b = tape[-1] ))\n", "no field `b`"),
        (b"t := type (a := ?, share parse = ( tape.is_constructed[0] = 5 ))\n", "takes a bool"),
        (b"p := type (y := i32 ?, share y := 3)\n", "shadowed"),
        (b"p := type (share y := 3, y := i32 ?)\n", "shadowed"),
        (b"t := type (share := i32 ?)\n", "followed by a declaration"),
        (b"t := type (5)\n", "a type body line"),
        (b"t := type (share associativity = 5)\n", "`left` or `right`"),
        (
            b"t := type (share parse = fn (mut a := i32 ?) -> void ( a = 1 ))\n",
            "`share parse = (…)`",
        ),
        (b"t := type (share parse = 5)\n", "`share parse = (…)`"),
        (b"t := type (share run = 5)\n", "`share run = (…)`"),
        (b"t := type (share run = ( 5 ))\nt(1)\n", "the call form of a type"),
        (b"t := type (share run = ( 5 ),\n", "never closed"),
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
        ("t := type ( a := i32 ?, share run = ( print «a» ] print «b» ) )", "] print"),
        ("t := type ( a := i32 ?, share run = ( k := [1 ), print «b» ) )", "), print"),
        ("f := fn () -> i32 ( t := type ( a := i32 ? ] ), 1 )", "] )"),
        ("f := fn () -> i32 ( t := type ( share k := immediate [1 ) ), 1 )", ") ), 1"),
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
            "bump := type (a := i32 ?, share run = ( {body} ), share parse_rank = *.parse_rank + 1, \
             share parse = ( tape[0].type = bump, tape[0].a = tape[-1], tape.is_constructed[0] = true, \
             tape.remove(-1) )), {tail}"
        )
    };
    let held_type = "print «a», t := type ( share k := immediate [1 ) ), print «b»";
    for (src, stray) in [
        (bump("print «a» ] print «b»", "3 bump"), "] print"),
        (bump("print «a», k := [1 ), print «b»", "3 bump"), "), print"),
        (bump("print «a», k := [1 ), print «b»", "x := 3 bump, x"), "), print"),
        (bump("print «a», k := [1 ), print «b»", "5"), "), print"),
        (bump("x := ( 1 ], print «b»", "3 bump"), "], print"),
        (bump(held_type, "3 bump"), ") ), print"),
        (bump(held_type, "5"), ") ), print"),
        ("f := fn () -> i32 ( t := type ( share k := immediate [1 ) ), 1 ), 5".into(), ") ), 1"),
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
    let probe = "probe := type ( share parse_rank = *.parse_rank + 1, share parse = ( \
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
    let first = "first := type ( share parse_rank = *.parse_rank + 1, share associativity = left, share parse = ( \
                 tape[0] = tape[1].dyads[0], tape.is_constructed[0] = true, tape.remove(1) ) )";
    let count = "count := type ( share parse_rank = *.parse_rank + 1, share parse = ( \
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
            "r := type ( share parse_rank = {rank}, share parse = ( print «{{tape[1].type == {read}}}», \
             tape.remove(0) ) ), r i32 5"
        );
        let out = logos().args([&src]).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n5\n", "{src}");
    }
    // A name arrives as lexed, a bracket as its scope cell, and what follows is left unlexed.
    let src = "x := i32 4, r := type ( share parse_rank = fn.parse_rank, share parse = ( \
               print «{tape.is_constructed[1]} {tape[2].type == scope} {tape[2].dyads.size}», \
               tape.remove(2), tape.remove(1), tape.remove(0) ) ), r x (1, 2), 7";
    let out = logos().args([src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "false true 2\n7\n");
    // A constructor the read lexed is not woken by it: it runs in its turn if the reader
    // leaves it, and `lex` still works after it.
    let s = "s := type ( share parse_rank = fn.parse_rank, share parse = ( tape[0] = i32 3, \
             tape.is_constructed[0] = true ) )";
    for (r, printed) in [
        ("print «{tape[1].type == type}», tape.remove(0)", "true\n3\n"),
        ("tape.remove(1), tape.remove(0), tape.insert(0, lex «6»)", "6\n"),
    ] {
        let src = format!(
            "{s}, r := type ( share parse_rank = fn.parse_rank, share parse = ( {r} ) ), r s"
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
            "r := type ( share parse_rank = fn.parse_rank, share parse = ( print «{{tape[3].type == void}}», \
             tape.remove(2), tape.remove(1), tape.remove(0) ) ), {tail}"
        );
        let out = logos().args([&src]).output().unwrap();
        assert!(out.status.success(), "{src}: stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), printed, "{src}");
    }
}

#[test]
fn a_read_where_the_tape_reaches_no_cell_is_a_constructed_void() {
    let r = "r := type ( share parse_rank = fn.parse_rank, share parse = ( \
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
    let w = "w := type ( share parse_rank = fn.parse_rank, share parse = ( tape[0] = tape[1], \
             tape.is_constructed[0] = true ) )";
    let out = logos().arg(format!("{w}, (w), 4")).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "4\n");
    let out = logos().arg(format!("{w}, x := (w), 4")).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("gives nothing"), "stderr: {stderr}");
    // A write is still the checked error.
    let out = logos()
        .arg("v := type ( share parse_rank = fn.parse_rank, share parse = ( tape[1] = i32 3 ) ), v")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("this index is off the tape"), "stderr: {stderr}");
}

#[test]
fn a_cell_a_constructor_reads_from_the_tape_arrives_unbuilt() {
    // `i32` is the type itself, not a conversion of the bracket, which stays for the reader.
    let src = "c := type ( share parse_rank = fn.parse_rank, share parse = ( \
               print «{tape[1].type == type} {tape[2].type == scope} {tape[2].dyads.size}», \
               tape.remove(2), tape.remove(1), tape.remove(0) ) ), c i32 (1, 2, 3), 7";
    let out = logos().args([src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true true 3\n7\n");
    // Left on the tape, `i32` takes its bracket in its own turn.
    let src = "c := type ( share parse_rank = fn.parse_rank, share parse = ( tape.remove(0) ) ), c i32 (5)";
    let out = logos().args([src]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}

#[test]
fn inclusion_between_integer_types_follows_their_value_ranges() {
    let (echoes, stderr) = repl(
        "u8 ⊆ u16\nu8 ⊆ i16\ni8 ⊆ u64\nu16 ⊆ i16\nu64 ⊆ i64\ni8 ⊆ i64\ni32 ⊆ i32\n\
         bool ⊆ bool\nnot u8 ⊆ i8\nx := u8 3\nnot x.type ⊆ u16\n"
            .as_bytes(),
    );
    assert_eq!(
        echoes,
        ["true", "true", "false", "false", "false", "true", "true", "true", "true", "false"],
        "stderr: {stderr}"
    );
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn inclusion_the_design_leaves_open_is_a_checked_error() {
    for src in ["f32 ⊆ f64\n", "i32 ⊆ f64\n", "bool ⊆ i32\n", "i32 ⊆ type\n", "u8 ⊆ dyad\n"]
    {
        let (echoes, stderr) = repl(src.as_bytes());
        assert!(echoes.is_empty() && stderr.contains("not settled"), "{src}: stderr: {stderr}");
    }
    let (echoes, stderr) = repl("1 ⊆ 2\n".as_bytes());
    assert!(echoes.is_empty() && stderr.contains("cannot compute"), "stderr: {stderr}");
}

#[test]
fn a_parse_body_checks_that_an_index_type_fits_the_size_type() {
    let q = "q := type ( share size := u64 0, share parse_rank = 60, share parse = ( \
             if (not tape[1].type ⊆ tape[0].size.type) \
             (error «the index type is not within the size type»), \
             tape.remove(1), tape[0] = i32 1, tape.is_constructed[0] = true ) )";
    let out = logos().args([&format!("{q}, q (u32 3)")]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1");
    for (arg, expect) in [("i8 3", "not within the size type"), ("f64 3.0", "not settled")] {
        let out = logos().args([&format!("{q}, q ({arg})")]).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && stderr.contains(expect), "{arg}: stderr: {stderr}");
    }
}

#[test]
fn a_hashmap_is_read_and_written_by_key() {
    let (echoes, stderr) = repl(
        b"m := hashmap i32 -> i64\n\
          m[3] = 40\n\
          m[-1] = 3000000000\n\
          m[3] + 2\n\
          m[-1]\n\
          x := i32 3\n\
          m[x] = m[x] * 2\n\
          m[3]\n\
          f := fn (n := i64 ?) -> i64 ( w := hashmap u8 -> i64, w[1] = n, w[1] + w[1] )\n\
          f(5)\n\
          f(7)\n\
          (hashmap i32 -> i64).type == m.type\n\
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
        b"mints := hashmap type -> type\n\
          mints[i32] = f64\n\
          mut t := mints[i32]\n\
          t == f64\n\
          t = mints[u8]\n\
          t\n\
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
        b"m := hashmap type -> type\n\
          mut u := m[i32]\n\
          u\n\
          n := hashmap i32 -> i64 ?\n\
          n[1]\n\
          n[1] = 2\n\
          n[1]\n\
          f := fn (w := hashmap i32 -> i64 ?) -> i64 ( 1 )\n",
    );
    assert_eq!(echoes, ["type ?", "2"], "stderr: {stderr}");
    assert!(stderr.contains("the map holds no value at this key"), "stderr: {stderr}");
    assert!(!stderr.contains("<repl>"), "no line is refused at parse: {stderr}");
}

#[test]
fn a_hashmap_checks_its_shape_and_its_key_and_value_types() {
    for line in ["hashmap i32 i32", "hashmap f64 -> i32", "hashmap i32 -> bool", "hashmap i32"] {
        let (_echoes, stderr) = repl(format!("{line}\n").as_bytes());
        assert!(stderr.contains("`hashmap` is followed by `K -> V`"), "{line}: {stderr}");
    }
    for line in ["m[i32] = 1", "m[1] = i32", "n[1] = 2", "n[i32] = 2"] {
        let (_echoes, stderr) = repl(
            format!("m := hashmap i32 -> i32\nn := hashmap type -> type\n{line}\n").as_bytes(),
        );
        assert!(stderr.contains("these types do not match"), "{line}: {stderr}");
    }
}

#[test]
fn a_constructor_writes_its_ifs_without_brackets() {
    let probe = "probe := type ( share parse_rank = *.parse_rank + 1, share parse = (\n\
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
    let sq = "sq := type ( a := ?, output_type := type ?, share run = ( a * a ), \
              share parse = ( tape[0].type = sq, tape[0].a = tape[-1], \
              tape[0].output_type = tape[-1].type, tape.is_constructed[0] = true, tape.remove(-1) ) )";
    for (src, want) in [
        ("x := i32 3, x.type == i32".to_string(), "true"),
        ("f := fn (x := i32 ?) -> bool ( x.type == i32 ), f.compile(), f(1)".into(), "true"),
        ("x := i32 3, (x + i32 1).type == i32".into(), "true"),
        (
            "f := fn (x := f64 ?) -> bool ( (x + 1.0).type == f64 ), f.compile(), f(1.0)".into(),
            "true",
        ),
        // The tape operand's type picks the node's output type.
        (format!("{sq}, x := f64 1.5, x sq"), "2.25"),
        (format!("{sq}, f := fn (x := f64 ?) -> f64 ( x sq ), f.compile(), f(1.5)"), "2.25"),
        (format!("{sq}, x := i32 3, (x sq).type == i32"), "true"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!((code, stdout.trim()), (Some(0), want), "{src}: {stderr}");
    }
    // An untyped parameter's type is known only per call: a checked error, never a crash.
    for param in ["a", "a := ?"] {
        for tail in ["f(1)", "f.compile(), f(1)"] {
            let src = format!("f := fn ({param}) -> bool ( a.type == i32 ), {tail}");
            let (code, _, stderr) = run_line(&src);
            assert_eq!(code, Some(1), "{src}: {stderr}");
            assert!(stderr.contains("known only when the program runs"), "{src}: {stderr}");
        }
    }
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
    let (_echoes, stderr) = repl(b"x := i32 5\nb := x + x\nb:start.rhs.type.roles[5]\n");
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
fn a_repl_session_frees_each_line_s_value_once_at_its_end() {
    // The session's exit grows a line at a time, text made between its lines.
    let bag = "bag := type ( mut n := i32 ?, share free = ( print «freed {n}» ) )";
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
        b"mut n := i32 0\nclimb := fn () -> i32 ( n = n + 1, n )\nclimb()\nclimb()\nfree n\n\
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
    for tail in ["( free a, error «x» )", "if (c == 1) ( g(), free a )", "if (g() == 1) ( free a )"]
    {
        let (echoes, stderr) =
            repl(format!("{lines}\n{g}\na := box (1, 2)\n{tail}\na.size\n").as_bytes());
        assert_eq!(echoes, ["free"], "{tail}: stderr: {stderr}");
        assert!(stderr.contains("`a` is dead here"), "{tail}: stderr: {stderr}");
    }
    // A fault the pass meets before the parse reaches the `free` ends nothing.
    let (echoes, stderr) =
        repl(format!("{lines}\n{g}\na := box (1, 2)\n( g(), free a )\na.size\n").as_bytes());
    assert_eq!(echoes, ["2", "free"], "stderr: {stderr}");
}

#[test]
fn a_failed_repl_line_keeps_the_ends_its_pass_ran() {
    // A parse error after the pass ran the `free` keeps the name ended; a fault the pass
    // meets before the `free` ran leaves it to the session.
    let bag = "bag := type ( mut n := i32 ?, share free = ( print «freed {n}» ) )";
    let fns = "g := fn () -> i32 ( error «x» )\nh := fn () -> i32 ( 1 )";
    let (echoes, stderr) = repl(
        format!(
            "{bag}\n{fns}\na := bag(1)\n( free a, n := immediate h(), nosuch )\na.n\n\
             b := bag(2)\n( n := immediate g(), free b, 1 )\nb.n\n"
        )
        .as_bytes(),
    );
    assert_eq!(echoes, ["freed 1", "2", "freed 2"], "stderr: {stderr}");
    assert!(stderr.contains("`a` is dead here"), "stderr: {stderr}");
}

#[test]
fn a_fault_in_the_pass_frees_what_a_nested_line_ended_once() {
    // The scope that holds the name decides, from the lines the pass ran.
    let bag = "bag := type ( mut n := i32 ?, share free = ( print «freed {n}» ) ), \
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
    let bag = "bag := type ( mut n := i32 ?, share free = ( print «freed {n}» ) ), \
               h := fn () -> i32 ( 1 )";
    for tail in ["nosuch", "&b", "return b", "return 1, 2"] {
        let src = format!("{bag}, x := ( b := bag(2), n := immediate h(), {tail} )");
        let out = logos().arg(src).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{tail}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "freed 2\n", "{tail}");
    }
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
    assert!(err.contains("calls nested deeper than 10000"), "stderr: {err}");
    assert!(out.stdout.is_empty());
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
        b"i32 == i32\ni32 == f64\ni32 != f64\ni32.type == logos\ni32.type == i32\n\
          x := i32 5\nx.type == i32\nx.type == f64\nt := logos\ni32.type == t\nlogos.type == logos\n",
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
fn a_type_returning_function_hands_back_a_type_box() {
    let (echoes, stderr) = repl(
        b"f := fn (t := type ?) -> type ( mut x := t, x )\nf(i32) == i32\ny := f(f64)\nz := y 5\nz\n\
          mints := hashmap type -> type\n\
          get := fn (t := type ?) -> type ( mut m := mints[t], if m != ? return m, m = t, mints[t] = m, m )\n\
          get(i32) == get(i32)\nget(i32) == get(f64)\nmut u := mints[u8]\nu == ?\n",
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
            "{g}, r := type ( share parse_rank = fn.parse_rank, share parse = ( {check} {tail}, r {arg}"
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
            format!("{g}, r := type ( share parse_rank = fn.parse_rank, share parse = ( {body} {tail}, r i32");
        let out = logos().args([&src]).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("these types do not match"), "{src}: stderr: {stderr}");
    }
    // A loop edits the tape after the read was checked: the read checks again when it runs.
    let src = format!(
        "{g}, r := type ( share parse_rank = fn.parse_rank, share parse = ( \
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
        b"mk := fn (t := type ?) -> type ( type ( share e := t ) )\n\
          a := mk(i32)\nb := mk(f64)\nc := mk(i32)\n\
          a.e == i32\nb.e == f64\na == b\na == c\n\
          mints := hashmap type -> type\n\
          get := fn (t := type ?) -> type ( mut m := mints[t], if m != ? return m, \
          m = type ( share e := t ), mints[t] = m, m )\n\
          get(i32) == get(i32)\nget(i32) == get(f64)\nget(u8).e == u8\n\
          g := fn (n := i32 ?) -> type ( type (share parse_rank = n) )\nt := g(3)\nt.parse_rank\n",
    );
    assert_eq!(
        echoes,
        ["true", "true", "false", "false", "true", "false", "true", "3.0"],
        "stderr: {stderr}"
    );
    assert!(stderr.is_empty(), "stderr: {stderr}");

    // The body is read when it runs, so its mistakes are reported at the call.
    let (_echoes, stderr) = repl(b"mk := fn () -> type ( type ( share e := nosuch ) )\nmk()\n");
    assert!(stderr.contains("building this `type (…)` failed"), "stderr: {stderr}");
    assert!(stderr.contains("nosuch"), "stderr: {stderr}");
}

#[test]
fn a_type_body_in_a_function_declares_fields_from_the_call_s_types() {
    let (echoes, stderr) = repl(
        b"mk := fn (t := type ?) -> type ( type ( share e := t, p := @e ?, v := t ? ) )\n\
          mk(u8).e == u8\nk := fn (t := type ?) -> type ( mk(t) )\nk(i32).e == i32\n",
    );
    assert_eq!(echoes, ["true", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
    // A member declared from the call's type is that type, so a function body may apply it.
    let (echoes, stderr) = repl(
        b"mk := fn (t := type ?) -> type ( type ( share e := t, \
          share f := fn () -> i32 ( e 5 ) ) )\nmk(i32).e == i32\n",
    );
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
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
fn a_type_call_with_a_runtime_argument_yields_a_type_at_run() {
    let (echoes, stderr) = repl(
        b"pick := fn (i := i32 ?) -> logos (if (i==0)(i32) else (f64))\n\
          g := fn (n := i32 ?) -> i32 ( a := pick(n), if (a == f64) (7) else (3) )\n\
          g(1)\ng(0)\n\
          h := fn (n := i32 ?) -> i32 ( x := pick(n) 5, 1 )\n",
    );
    assert_eq!(echoes, ["7", "3"], "stderr: {stderr}");
    assert!(stderr.contains("known only when the program runs"), "stderr: {stderr}");
}

#[test]
fn a_logos_declaration_declares_a_place_of_that_type() {
    let (echoes, stderr) = repl(b"mut a := i32 ?\na = 9\na.type == i32\na\n");
    assert_eq!(echoes, ["true", "9"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_dependent_typed_declaration_takes_a_computed_type() {
    let (echoes, stderr) = repl(
        b"metalogos := fn (i := i32 ?) -> logos (if (i==0)(i32) else (f64))\n\
          mut b := metalogos(1) ?\nb = 7\nb.type == f64\nb\n",
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
    let (_echoes, stderr) = repl(b"a := scope ?\n");
    assert!(stderr.contains("non-numeric types are not in the seed yet"), "stderr: {stderr}");
}

#[test]
fn a_logos_variable_declares_fills_once_and_becomes_the_type() {
    let (echoes, stderr) =
        repl(b"mut a := logos ?\na = i32\na.type == logos\na == i32\ny := a 5\ny\n");
    assert_eq!(echoes, ["true", "true", "5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"mut a := logos ?\na == i32\n");
    assert!(
        echoes.is_empty() && stderr.contains("`a` is read before it is written"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_logos_box_is_written_as_often_as_you_like() {
    let (echoes, stderr) = repl(b"mut a := logos ?\na = i32\na = f64\na\n");
    assert_eq!(echoes, ["f64"], "stderr: {stderr}");

    let (echoes, stderr) =
        repl(b"mut a := logos ?\na = f64\ng := fn () -> i32 ( a = i32, 1 )\ng()\na\n");
    assert_eq!(echoes, ["1", "i32"], "stderr: {stderr}");

    let (_e, stderr) = repl(b"mut a := logos ?\na = 5\n");
    assert!(stderr.contains("must be a type value"), "stderr: {stderr}");
}

#[test]
fn logical_operators_fold_over_bool_literals() {
    let (echoes, stderr) = repl(
        b"true or false\ntrue and true\nnot (true)\n\
          mut a := logos ?\na = i32\nif (a.type == f32 or a.type == logos) (a = f64) else (a = i32)\na == f64\n",
    );
    assert_eq!(echoes, ["true", "true", "false", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_comptime_if_drops_the_untaken_branch_unparsed() {
    // `a = 9.9` under `a := i32 ?` would be a parse error if it were ever parsed; that this runs proves the branch was skipped.
    let (echoes, stderr) = repl(
        b"mut a := i32 ?\na = 0\nif (a.type == i32) (a = 9) else (a = 9.9)\na\n\
          mut b := f64 ?\nb = 0\nif (b.type == i32) (b = 1) else if (b.type == f64) (b = 2.5) else (b = 3)\nb\n",
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
        b"x := i32 5\nx:end\nx:gate\nx:scope\n(x + x):scope\ny := i32\ny.type == type\ny == i32\n",
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
        b"x := i32 5\ny := i64 6\nx:frame == here.scope.back\ny:frame == x:frame\n\
          y:offset == x:offset\nx:offset < y:offset\nx:name\n",
    );
    assert_eq!(echoes, ["true", "true", "false", "true", "x"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_binding_s_fields_are_read_and_only_lex_rank_is_written() {
    // A write into `frame` or `offset` would move where `s` reads; `lex_rank` is the one
    // field a program sets after the declaration.
    let (echoes, stderr) = repl(
        b"s := i32 5
s:offset = 7
s:frame = s:scope
s:scope = s:scope
s:end = s:scope
          p := &s:offset
s:lex_rank = 7
s:lex_rank
s
",
    );
    assert_eq!(echoes, ["7.0", "5"], "stderr: {stderr}");
    assert_eq!(stderr.matches("not an assignable place").count(), 4, "stderr: {stderr}");
    assert_eq!(stderr.matches("`&` needs a variable").count(), 1, "stderr: {stderr}");
}

#[test]
fn a_parameter_and_a_local_are_names_of_the_call_s_frame() {
    // Parameters first, then the locals, packed at their widths: `a` at 0, `b` at 4, `c` at 12;
    // their frame is the function, not the program's.
    let (echoes, stderr) = repl(
        b"x := i32 5\nf := fn (a := i32 ?, b := i64 ?) -> u64 ( c := i64 3, a:offset + b:offset + c:offset )\n\
          f(1, 2)\ng := fn (a := i32 ?, b := i64 ?) -> bool ( a:frame == b:frame and a:frame != x:frame )\n\
          g(1, 2)\nh := fn () -> i64 ( n := i64 0 - 1, n )\nh()\nh.compile()\nh()\nu := u64 0 - 1\nu\n",
    );
    assert_eq!(echoes, ["16", "true", "-1", "-1", "18446744073709551615"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_share_function_writes_a_field_of_a_frame_local_record() {
    // The value a `share` function is called through is the record's own bytes, in the
    // caller's frame, so the write lands in the local it was called on.
    let (echoes, stderr) = repl(
        b"pt := type (mut a := i32 ?, b := i32 ?, share bump := fn () -> void ( a = a + b ))\n\
          f := fn () -> i32 ( mut p := pt(1, 2), p.bump(), p.bump(), p.a )\nf()\n\
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
        b"mut x := i32 5\nif:scope\ntype.type == type\nfn.type == type\n\
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
    for tail in ["x := f(), x", "print «{f()}»", "f() + 1", "y := (return n), y", "f().type"] {
        let (code, _, stderr) = run_line(&format!("{void_fn}, {tail}"));
        assert_eq!(code, Some(1), "{tail}");
        assert!(stderr.contains("gives nothing"), "{tail}: stderr: {stderr}");
    }
}

#[test]
fn the_echo_reads_the_tail_value_as_any_use_does() {
    // Refused before the tail runs, so `ran` never prints.
    for tail in [
        "if c (i32 1) else (i64 2)",
        "( y := i32 1, if c (print «ran», i32 1) else (i64 2) )",
        "(if c (i32 1) else (i64 2)).type",
    ] {
        let (code, stdout, stderr) = run_line(&format!("mut c := true, {tail}"));
        assert_eq!(code, Some(1), "{tail}");
        assert_eq!(stdout, "", "{tail}");
        assert!(stderr.contains("give different types"), "{tail}: stderr: {stderr}");
    }
    for (tail, want) in [
        ("if c (i32 1) else (i32 2)", "1\n"),
        ("if c (5)", ""),
        ("if c (i32 1) else (print «b»)", ""),
        ("if c (i32 1) else (i64 2), 3", "3\n"),
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
        b"p := type (mut v := i32 ?)\nmut q := p(1)\nq.v = 3\nq.v\nmut a := i32 1\na = a + 1\na\n",
    );
    assert_eq!(echoes, ["3", "2"], "stderr: {stderr}");
}

#[test]
fn a_write_along_a_path_needs_mut_on_every_step() {
    for (src, expect) in [
        (&b"p := type (mut v := i32 ?)\nq := p(1)\nq.v = 3\n"[..], "`q` is not `mut`"),
        (b"p := type (v := i32 ?)\nmut q := p(1)\nq.v = 3\n", "`v` is not `mut`"),
        (b"t := type (share y := i32 3)\nt.y = 4\n", "`y` is not `mut`"),
        (b"t := type (share y := i32 3)\nq := t()\nq.y = 4\n", "`y` is not `mut`"),
        (b"p := type (immut v := i32 ?)\nmut q := p(1)\nq.v = 3\n", "`v` is `immut`"),
    ] {
        let (_e, stderr) = repl(src);
        assert!(stderr.contains(expect), "{}: stderr: {stderr}", String::from_utf8_lossy(src));
    }
    let (echoes, stderr) = repl(
        b"t := type (share mut y := i32 3, v := i32 ?)\nq := t(1)\nt.y = 4\nq.y\nq.y = 5\nt.y\n",
    );
    assert_eq!(echoes, ["4", "5"], "stderr: {stderr}");
}

#[test]
fn a_tight_read_lexes_its_right_cell_on_demand_and_stops_at_a_boundary() {
    let (echoes, stderr) = repl(
        b"x := i32 5\n(x:end, 3)\np := type (a := i32 ?)\nq := p(1)\n(q.a, 2)\nq.a\n\
          r := &q\nr@.a\npp := &r\npp@@.a\nx.type == i32\n",
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
        echoes.is_empty() && stderr.contains("`b` is read before it is written"),
        "stderr: {stderr}"
    );
}

#[test]
fn the_dyad_box_says_what_it_holds() {
    let (echoes, stderr) = repl(b"mut a := dyad ?\na = i32\na.type == type\na == i32\na\n");
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

    let (_e, stderr) = repl(b"mut a := dyad ?\na = 5\n");
    assert!(stderr.contains("must be a type value"), "stderr: {stderr}");
}

#[test]
fn only_a_marked_place_is_written_or_addressed() {
    let pw = "pw := type ( mut a := i32 ?, mut b := i32 ?, output_type := type ?, share run = ( a * b ), \
              share parse_rank = *.parse_rank + 1, share associativity = right, \
              share parse = ( tape[0].type = pw, tape[0].a = tape[-1], tape[0].b = tape[1], tape[0].output_type = i32, \
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
        b"mut x := i32 5\nx = 6\np := &x\np@\nw := type (y := i64 ?)\nq := w(7)\nr := &q\nr@.y\n\
          mut a := type ?\nmut b := type ?\na = i32\nb = a\nb == i32\nmut d := dyad ?\nd = i32\nd.type == type\n",
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
    let (_e, stderr) = repl(b"b := scope ?\n");
    assert!(stderr.contains("not in the seed yet"), "stderr: {stderr}");
}

#[test]
fn a_dyad_is_built_from_a_type_and_a_value() {
    let (echoes, stderr) =
        repl(b"c := dyad (i32, 7)\nc\nc.type == i32\ndyad (i32, 7).type == i32\nc + 1\n");
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
        b"g := type (x := i32 ?)\ndyad (g, 5)\n",
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
fn an_imported_pub_name_keeps_its_gates() {
    // The importer's name is the file's binding again, gates included.
    assert_eq!(line("import ./tests/fixtures/gated.logos, r = 9, r"), "9");
    assert_eq!(line("import ./tests/fixtures/gated.logos, q.bump(), q.a"), "2");
    let (code, _, stderr) = run_line("import ./tests/fixtures/gated.logos, s = 3, s");
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("`s` is not `mut`"), "stderr: {stderr}");
}

#[test]
fn an_import_tail_runs_once() {
    assert_eq!(line("import ./tests/fixtures/counter.logos, c@"), "1");
    // An import is a statement: nothing echoes, and the second import runs nothing.
    let (echoes, stderr) =
        repl(b"import ./tests/fixtures/counter.logos\nimport ./tests/fixtures/counter.logos\nc@\n");
    assert_eq!(echoes, ["1"], "stderr: {stderr}");
}

#[test]
fn a_shared_name_in_a_file_imported_twice_is_one_place() {
    let out = logos()
        .arg(
            "import tests/fixtures/shared_importer_a.logos, \
             import tests/fixtures/shared_importer_b.logos, \
             import tests/fixtures/shared_counter.logos, print «{a} {b} {count}», 0",
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "loaded\n1 2 2\n0\n");
}

#[test]
fn a_shared_name_in_a_top_level_loop_is_made_once() {
    let src = "print «a», mut t := i32 0, \
               for i in 0..3 ( share p := (print «made», alloc 1 of i32 7), t = t + p@ ), \
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
    let block = "y := ( mut a := type ?, a = i32, bump(), z := a 5, z + x ), y + x";
    assert_eq!(line(&format!("{bump}, {block}")), "7");
    assert_eq!(line("y := ( mut a := type ?, a = i32, mut x := a 5, x ) + 1, y"), "6");
    let (echoes, stderr) = repl(
        b"( mut a := type ?, a = i32, mut x := a 5, x )\nq := ( p := @i32 ?, p@ )\nq := i32 4\nq\n",
    );
    assert_eq!(echoes, ["5", "4"], "stderr: {stderr}");
    assert!(stderr.contains("`p` is read before it is written"), "stderr: {stderr}");
}

#[test]
fn immediate_runs_its_expression_as_it_parses_and_stands_as_the_value() {
    let array = "import ./identities/array.logos";
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
    let (echoes, stderr) = repl(b"immediate 2 + 3\nimmediate\n");
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    assert!(stderr.contains("nothing to evaluate here"), "stderr: {stderr}");
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
        "twice := type (share parse_rank = *.parse_rank + 1, share parse = ( tape.insert(1, lex «* 2»), tape.remove(0) ))\n\
          5 twice\n3 + 4 twice\n"
            .as_bytes(),
    );
    assert_eq!(echoes, ["10", "11"], "stderr: {stderr}");
    let (echoes, stderr) = repl(
        "t := lex «(a, b)»\n5\n\
          bad := type (share parse = ( tape.insert(1, lex «zz»), tape.remove(0) ))\n\
          x := bad\n"
            .as_bytes(),
    );
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    assert!(stderr.contains("unknown name `zz`"), "stderr: {stderr}");
    let (_echoes, stderr) = repl(
        b"y := lex 5\n\
          bad2 := type (share parse = ( tape.insert(1, dyad (i32, 1)) ))\n",
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
    // The program's tail is its last line, a statement, so nothing echoes after the prints.
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
             f := fn (n := i32 ?) -> i32 ( print «n={n}», n ), f(3), f(4)",
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "answer 42\ntrue 5.5 {x} a\\b\nn=3\nn=4\n4\n");

    // A constructor's print runs at parse time.
    let (echoes, stderr) = repl(
        "m := type (share parse = ( print «parsing {1 + 1}», tape.remove(0) ))\n1 m\n".as_bytes(),
    );
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
        ("t := type (share greet := fn () -> void ( print «{«a»}» )), 5", "5\n"),
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
    // The quote holds `a «b» c` whole; the text value's read is still missing.
    let (_, _, stderr) = run_line("s := «a «b» c», print «{s}»");
    assert!(stderr.contains("run error: a record, text or hole is not read"), "{stderr}");
    let (_, _, stderr) = run_line("import «tests/fixtures/a«b».logos»");
    assert!(stderr.contains("cannot read ./tests/fixtures/a«b».logos"), "{stderr}");

    let (echoes, stderr) = repl("print «{«a»}»\nprint «a «b» c»\n".as_bytes());
    assert_eq!(echoes, ["a", "a «b» c"], "stderr: {stderr}");
}

#[test]
fn a_bracket_inside_a_nested_quote_or_its_comment_is_text_in_every_body() {
    let run_body = |body: &str| {
        format!(
            "bump := type (a := i32 ?, share run = ( {body} ), share parse_rank = *.parse_rank + 1, \
             share parse = ( tape[0].type = bump, tape[0].a = tape[-1], tape.is_constructed[0] = true, \
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
            "pt := type (\n# «a «b» ) c»\na := i32 ?,\n# the rank\nshare parse_rank = *.parse_rank + 1,\n\
             # «before the fill»\nshare parse = ( tape[0].type = pt, tape[0].a = tape[-1], \
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
        "h := type (share parse_rank = {rank}, \
         share parse = ( tape.insert(1, lex «{fragment}»), tape.remove(0) ))"
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
    let out = logos().arg("import tests/fixtures/spliced_comment.logos, ten").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n");
}

#[test]
fn the_boundary_s_comment_lift_reads_a_built_cell_with_no_node_without_faulting() {
    // `h1` marks the unknown `zz` built, which leaves its cell with no node, and `h2`
    // removes it at the next step: the lift between the two reads that cell.
    let src = "h1 := type (share parse_rank = *.parse_rank + 2, \
               share parse = ( tape.is_constructed[1] = true, tape.remove(0) )), \
               h2 := type (share parse_rank = *.parse_rank + 1, \
               share parse = ( tape.remove(1), tape.remove(0) )), \
               x := 7, x h2 h1 zz";
    let (code, _, stderr) = run_line(src);
    // A panic on the work thread exits 1, as a Logos error does.
    assert!(code.is_some() && !stderr.contains("panicked"), "{code:?}: {stderr}");
}

#[test]
fn a_body_lexed_once_holds_the_scopes_of_its_quotes() {
    let run_body = |body: &str| {
        format!(
            "mut n := i32 1, bump := type (a := i32 ?, share run = ( {body} ), \
             share parse_rank = *.parse_rank + 1, share parse = ( tape[0].type = bump, \
             tape[0].a = tape[-1], tape.is_constructed[0] = true, tape.remove(-1) )), 3 bump"
        )
    };
    for (src, printed) in [
        (run_body("print «{a}»"), "3\n"),
        (run_body("print «{n} {a + 1} {«)»} {«x {«y»}»}»"), "1 4 ) x {«y»}\n"),
        (run_body("\n# «note {a}»\nprint «a={a}»\n"), "a=3\n"),
        (
            "f := fn () -> i32 ( t := type ( share k := immediate ( print «v {1}», 1 ) ), 1 ), f()"
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
        ("t := type (a := i32 ?, share run = ( # «x {»} y»\n1 )), 5", "1:44: error: this `»`"),
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: {stderr}");
        assert!(stderr.contains(message), "{src}: {stderr}");
    }
    // A `lex` quote lexes its text when it runs, the escapes applied first.
    let (_, _, stderr) = run_line("t := lex «x \\«», 5");
    assert!(stderr.contains("this `«` has no `»`"), "{stderr}");
}

#[test]
fn error_aborts_the_run_with_its_message() {
    let out = logos()
        .arg(
            "f := fn (x := i32 ?) -> i32 ( if (x > 2) (error «too big: {x}») else (x) ), \
             print «{f(1)}», f(5), print «never»",
        )
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "a plain failure exit, not a signal");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("run error: too big: 5"), "stderr: {stderr}");

    let out = logos().arg("t := type (share parse = (error «not here»)), t").output().unwrap();
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
             f(1), f(4)",
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
         w := type (share parse = ( seen = caller.scope, tape.remove(0) ))\n\
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
         s.dyads[0].rhs.type == i32\n\
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
fn a_constructor_reads_the_scope_cell_to_its_right_line_by_line() {
    let lib = "import tests/fixtures/scope_cell_lines.logos";
    for (line, want) in
        [("first (7, 8, 9) + last (7, 8, 9)", "16"), ("first (scope (1, 2), 3)", "2")]
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
        ("scope (1, 2, 3)", "3"),
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
    let power = "^ := type ( lhs := ?, rhs := i32 ?, output_type := type ?, \
                 share run = ( mut r := output_type 1, for 0..rhs ( r = r * lhs ), r ), \
                 share parse_rank = *.parse_rank + 1, share associativity = right, \
                 share parse = ( tape[0].type = ^, tape[0].lhs = tape[-1], tape[0].rhs = tape[1], tape[0].output_type = tape[-1].type, \
                 tape.is_constructed[0] = true, tape.remove(1), tape.remove(-1) ) )";
    for (tail, want) in [
        ("f := fn (x := i32 ?) -> i32 ( b := x ^ 3, b:start.rhs.lhs ), f(5)", "5"),
        ("f := fn (x := i32 ?) -> i32 ( b := x ^ 3, b:start.rhs.lhs ), f.compile(), f(5)", "5"),
        ("x := i32 1, c := x + 2, c:start.rhs.lhs", "1"),
    ] {
        let out = logos().arg(format!("{power}, {tail}")).output().unwrap();
        assert!(out.status.success(), "{tail}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{want}\n"), "{tail}");
    }
    // Written straight on an expression, `.` reads the number it evaluates to, as on a name.
    for tail in [
        "(2 ^ 3).lhs",
        "f := fn (x := i32 ?) -> i32 ( (x ^ 3).lhs ), f(5)",
        "f := fn (x := i32 ?) -> i32 ( (x + 3).lhs ), f.compile(), f(5)",
        "x := i32 1, (x + 2).lhs",
        "x := i32 1, c := x + 2, c.lhs",
    ] {
        let out = logos().arg(format!("{power}, {tail}")).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && out.stdout.is_empty(), "{tail}: {stderr}");
        assert!(stderr.contains("cannot compute"), "{tail}: {stderr}");
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
         pt := type (h := @@i32 ?)\n\
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
        b"r1 := type (share parse = ( tape.is_constructed[0] = true, tape.recenter(1) ))\n\
          r2 := type (share parse = ( tape.is_constructed[0] = true, tape.recenter(-1) ))\n\
          r3 := type (share parse = ( tape.is_constructed[0] = true, tape.recenter(99) ))\n\
          a := r1\nb := r2\nc := r3\na.type == type\nb.type == type\nc.type == type\n",
    );
    assert_eq!(echoes, ["true", "true", "true"], "stderr: {stderr}");
    for src in [
        &b"r4 := type (share parse = ( tape.recenter(1) ))\nx := r4\n"[..],
        b"r5 := type (share parse = ( tape[0] = tape.spelling[0] ))\nf := fn () -> void ( r5 )\n",
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
        b"as_i32 := type (share parse = ( tape[0] = i32 ))\n\
          x := as_i32 5\nx + 1\n",
    );
    assert_eq!(echoes, ["6"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(
        "half := type (share parse = ( tape.insert(1, lex «/ 2») ))\n\
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
    let (code, _, stderr) = run_line(
        "probe := type ( v := i32 ?, share run = ( v ), \
         share parse_rank = *.parse_rank + 1, \
         share parse = ( tape[0].type = probe, tape[0].v = 7, tape.is_constructed[0] = true ) ), probe",
    );
    assert_eq!(code, Some(0), "stderr: {stderr}");
    for (tail, want) in [("probe", "7\n"), ("probe + 1", "8\n")] {
        let (code, stdout, stderr) = run_line(&format!(
            "probe := type ( v := i32 ?, output_type := type ?, share run = ( v ), \
             share parse_rank = *.parse_rank + 1, \
             share parse = ( tape[0].type = probe, tape[0].v = 7, tape[0].output_type = i32, \
             tape.is_constructed[0] = true ) ), {tail}"
        ));
        assert_eq!((code, stdout.as_str()), (Some(0), want), "stderr: {stderr}");
    }
    let (code, stdout, stderr) = run_line(
        "q := type ( size := u64 ?, share parse_rank = 60, \
         share parse = ( tape[0].type = q, tape[0].size = 3, tape[0] = tape[0].size, \
         tape.is_constructed[0] = true ) ), q",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "3\n"), "stderr: {stderr}");
}

#[test]
fn a_number_field_is_read_and_written_by_value_in_a_parse_body() {
    // `tape[0].n` is the u64 it holds, so it takes part in arithmetic; the value a
    // run-time right side yields is what the field keeps after the parse returns.
    let (code, stdout, stderr) = run_line(
        "q := type ( n := u64 ?, m := u64 ?, share parse_rank = 60, share parse = ( \
         tape[0].type = q, tape[0].n = tape[1].dyads.size, tape[0].m = tape[0].n * 2, \
         tape[0] = tape[0].m, tape.remove(1), \
         tape.is_constructed[0] = true ) ), q (1, 2, 3)",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "6\n"), "stderr: {stderr}");
    let (code, _, stderr) = run_line(
        "q := type ( n := u64 ?, share parse_rank = 60, \
         share parse = ( tape[0].type = q, tape[0].n = i32 3, tape.is_constructed[0] = true ) ), q",
    );
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("these types do not match"), "stderr: {stderr}");
}

#[test]
fn a_share_function_names_its_fields_bare() {
    let (code, _, stderr) =
        run_line("x := type ( n := u64 ?, share twice := fn () -> u64 ( n * 2 ) ), 1");
    assert_eq!(code, Some(0), "stderr: {stderr}");
    // Only the type's own functions reach a field by name, and a parse never does.
    for (src, expect) in [
        ("f := fn () -> u64 ( n ), 1", "unknown name `n`"),
        ("x := type ( n := u64 ?, share parse = ( n ) ), 1", "`tape[0].n`, never bare"),
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
        run_line("x := type ( n := u64 ?, share free = ( print «gone» ) ), 1");
    assert_eq!((code, stdout.as_str()), (Some(0), "1\n"), "stderr: {stderr}");
    for (src, expect) in [
        ("x := type ( share free = 5 ), 1", "`share free = (…)`"),
        ("x := type ( mut p := own @i32 ? ), 1", "must be freed by the type's"),
        ("x := type ( p := own i32 ? ), 1", "over a pointer hole"),
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
    share free = ( print «free», free p ), \
    share parse_rank = dyad.parse_rank, \
    share parse = ( tape.is_constructed[0] = true ) \
), \
box := type ( \
    elements := scope ?, \
    output_type := type ?, \
    share run = ( \
        mut v := output_type ?, \
        v.p = mut alloc elements.dyads.size of i32 ?, \
        for i in 0..elements.dyads.size ( \
            (v.p + i)@ = elements.dyads[i], \
            v.size = i + 1 \
        ), \
        move v \
    ), \
    share parse_rank = dyad.parse_rank, \
    share associativity = left, \
    share parse = ( \
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
            "f := fn () -> i32 ( a := box (6, 1), 3 ), print «before», f(), f(), print «after»",
            "before\nfree\nfree\nafter\n",
        ),
        (
            "f := fn () -> i32 ( a := box (6, 1), 3 ), f.compile(), print «before», f(), print «after»",
            "before\nfree\nafter\n",
        ),
        // The last value moves out: the caller's name is the owner.
        (
            "mk := fn () -> box ( a := box (9, 1), a ), \
             g := fn () -> i32 ( m := mk(), print «got», (m.p + 0)@ ), g(), print «after»",
            "got\nfree\nafter\n",
        ),
        ("mk := fn () -> box ( box (9, 1) ), m := mk(), print «got»", "got\nfree\n"),
        // A borrow frees nothing: one free, by the owner.
        ("a := box (5, 1), b := a, print «borrowed»", "borrowed\nfree\n"),
        (
            "a := box (5, 1), f := fn (q := box ?) -> i32 ( (q.p + 0)@ ), f(a), print «after»",
            "after\nfree\n",
        ),
        ("a := box (7, 1), b := move a, print «moved»", "moved\nfree\n"),
        // An early `free` tears down now, and the scope end finds nothing left.
        ("a := box (7, 1), free a, print «after»", "free\nafter\n"),
        // A returned borrow is no owner: the callee's own array is freed as it returns.
        ("mk := fn () -> box ( a := box (9, 1), b := a, b ), m := mk(), print «got»", "free\ngot\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{BOX}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert!(stdout.starts_with(printed), "{tail}: stdout: {stdout}");
        assert_eq!(stdout.matches("free").count(), printed.matches("free").count(), "{tail}");
    }
    for (tail, expect) in [
        ("a := box (7, 1), free a, a", "`a` is dead here"),
        ("a := box (7, 1), b := a, c := move b", "only its owner can"),
        ("mut a := box (7, 1), b := box (8, 1), a = b", "what is assigned must own too"),
    ] {
        let (code, _, stderr) = run_line(&format!("{BOX}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
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
            "import ./tests/fixtures/deferring.logos, print «main», k".to_string(),
            "main\nfile ends\n1\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{src}");
    }
    let (echoes, stderr) = repl(b"defer print \xc2\xabbye\xc2\xbb\n1\n");
    assert_eq!(echoes.last().map(String::as_str), Some("bye"), "stderr: {stderr}");
    // An imported name is the file's own place: ending it ends the file's hold, once.
    for src in [
        "import ./tests/fixtures/owning_pub.logos, p@",
        "import ./tests/fixtures/owning_pub.logos, free p, 1",
        "import ./tests/fixtures/owning_pub.logos, q := move p, q@",
    ] {
        let (code, stdout, stderr) = run_line(src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert!(matches!(stdout.trim(), "7" | "1"), "{src}: {stdout}");
    }
    let (echoes, stderr) = repl(b"import ./tests/fixtures/owning_pub.logos\nfree p\n1\n");
    assert_eq!(echoes, ["1"], "stderr: {stderr}");
}

#[test]
fn a_fault_ends_every_live_scope_as_its_end_would() {
    let array = "import ./identities/array.logos";
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
        ("f := fn () -> i32 ( ( a := box (1, 2), error «in» ), 1 ), f()", "free\n", "in"),
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
    let (code, stdout, stderr) = run_line("import ./tests/fixtures/faulting.logos, 1");
    assert_eq!((code, stdout.as_str()), (Some(1), "file cleanup\nfreed\n"), "stderr: {stderr}");
    assert!(stderr.contains("file stops"), "stderr: {stderr}");
    // The stopped function's `defer` runs, the line after the fault does not.
    let (echoes, stderr) = repl(
        "p := alloc 1 of i32 5\n\
         g := fn () -> i32 ( error «stop» )\n\
         f := fn () -> i32 ( defer (p@ = p@ + 100), g(), p@ = 9, 1 )\n\
         f()\n\
         p@\n"
            .as_bytes(),
    );
    assert_eq!(echoes.last().map(String::as_str), Some("105"), "{echoes:?} {stderr}");
    assert!(stderr.contains("stop"), "stderr: {stderr}");
}

#[test]
fn assignment_frees_the_value_it_displaces() {
    for (tail, printed) in [
        ("mut x := box (1, 2), x = box (3, 4), print «after»", "free\nafter\nfree\n"),
        (
            "f := fn () -> i32 ( mut x := box (1, 2), x = box (3, 4), 1 ), f.compile(), f(), \
             print «after»",
            "free\nfree\nafter\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{BOX}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{tail}");
    }
}

/// A plain record whose type fills `share free`, its free printing the one field.
const RECORD: &str = "bag := type ( mut n := i32 ?, share free = ( print «freed {n}» ) )";

#[test]
fn a_plain_record_s_owner_runs_its_free_once() {
    for (tail, printed) in [
        // The scope's end runs it; the program's before the tail shows.
        ("a := bag(1), 1", "freed 1\n1\n"),
        ("f := fn () -> i32 ( a := bag(2), 1 ), f()", "freed 2\n1\n"),
        (
            "f := fn () -> i32 ( a := bag(3), 1 ), print «before», f(), f(), print «after»",
            "before\nfreed 3\nfreed 3\nafter\n",
        ),
        (
            "f := fn () -> i32 ( a := bag(4), 1 ), f.compile(), print «before», f(), print «after»",
            "before\nfreed 4\nafter\n",
        ),
        // `bag ?` owns too: the end runs its free over whatever was written.
        (
            "f := fn () -> i32 ( mut a := bag ?, a.n = 5, 1 ), f(), print «after»",
            "freed 5\nafter\n",
        ),
        // An early `free` tears down now, and the end finds nothing held.
        ("a := bag(6), free a, print «after»", "freed 6\nafter\n"),
        ("a := bag(7), if true ( free a ), print «after»", "freed 7\nafter\n"),
        ("c := i32 0, a := bag(15), if (c == 1) ( free a ), print «after»", "freed 15\nafter\n"),
        // A borrow frees nothing: one free, by the owner; `free` of the borrow ends its name.
        ("a := bag(8), b := a, print «borrowed»", "borrowed\nfreed 8\n"),
        ("a := bag(14), b := a, free b, print «after»", "after\nfreed 14\n"),
        (
            "a := bag(9), f := fn (q := bag ?) -> i32 ( q.n ), f(a), print «after»",
            "after\nfreed 9\n",
        ),
        // The last value moves out: the caller's name is the owner.
        ("mk := fn () -> bag ( bag(10) ), m := mk(), print «got»", "got\nfreed 10\n"),
        ("mk := fn () -> bag ( a := bag(11), a ), m := mk(), print «got»", "got\nfreed 11\n"),
        // A returned borrow is no owner: the callee's own record is freed as it returns.
        (
            "mk := fn () -> bag ( a := bag(12), b := a, b ), m := mk(), print «got»",
            "freed 12\ngot\n",
        ),
        ("free (bag(16)), print «after»", "freed 16\nafter\n"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), printed), "{tail}: stderr: {stderr}");
    }
    for (tail, expect) in [
        ("a := bag(1), free a, a", "`a` is dead here"),
        ("a := bag(1), b := a, free b, b", "`b` is dead here"),
        ("a := bag(1), b := move a, 1", "moving a plain record is not in the seed yet"),
    ] {
        let (code, _, stderr) = run_line(&format!("{RECORD}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
    // A later REPL line frees what an earlier one holds, and the session's end frees it no more.
    let (echoes, stderr) = repl(format!("{RECORD}\na := bag(13)\nfree a\n").as_bytes());
    assert_eq!(echoes, ["freed 13"], "stderr: {stderr}");
}

#[test]
fn a_plain_record_s_owning_field_keeps_its_pointer_in_its_own_bytes() {
    let r = "r := type ( mut p := own @i32 ?, share free = ( free p ), \
             share set := fn () -> i32 ( a := alloc 1 of i32 5, p = move a, 1 ), \
             share s := fn () -> i32 ( free p, 2 ) ), mut x := r ?";
    for (tail, printed) in [
        ("x.set(), x.s()", "2"),
        ("a := alloc 1 of i32 5, x.p = move a, x.s()", "2"),
        ("x.s()", "2"),
        ("a := alloc 1 of i32 5, x.p = move a, b := alloc 1 of i32 6, x.p = move b, x.p@", "6"),
        // Moved out, the field is empty, so the record's own free frees nothing twice.
        ("a := alloc 1 of i32 5, x.p = move a, b := move x.p, b@", "5"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{r}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.trim(), printed, "{tail}");
    }
}

#[test]
fn an_if_frees_what_one_arm_moved_at_the_end_of_the_other() {
    for (tail, printed) in [
        (
            "f := fn (c := i32 ?) -> i32 ( a := box (1, 2), if (c == 1) ( b := move a, 1 ), 3 ), \
             f.compile(), f(0), f(1), print «after»",
            "free\nfree\nafter\n",
        ),
        (
            "f := fn (c := i32 ?) -> i32 ( a := box (1, 2), \
             if (c == 1) ( b := move a, 1 ) else ( return 2 ), 3 ), f(0), f(1), print «after»",
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
        (format!("x := ( {ret}, 7 ) + (free a, i32 1)"), 8),
        (format!("x := ( {ret} + (free a, i32 1), 7 )"), 7),
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
    let array = "import ./identities/array.logos, x := array i32 [1, 2], y := array i32 [3, 4], \
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
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.trim(), printed, "{tail}");
    }
    // The outer array owns its elements, so a name is moved in; a borrow cannot be.
    for (tail, expect) in [
        ("x[0]", "`x` is dead here"),
        ("z := array i32 [5], c := array t [z]", "write `move x` to move it in"),
        ("z := array i32 [5], w := z, c := array t [move w]", "only its owner can"),
    ] {
        let (code, _, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
}

/// A value whose field holds an array, built in the array's shape: `holder ()` is a node
/// whose run makes a new `held`, and `holder` with no bracket is `held`, whose parse
/// reads nothing.
const HOLDER: &str = "import ./identities/array.logos, t := array i32, \
    held := type ( \
        mut items := t ?, \
        share count := fn () -> u64 ( items.size ), \
        share second := fn () -> i32 ( items[1] ), \
        share parse_rank = dyad.parse_rank, \
        share parse = ( tape.is_constructed[0] = true ) ), \
    holder := type ( \
        output_type := type ?, \
        share run = ( mut v := output_type ?, v.items = array i32 [4, 5, 6], v ), \
        share parse_rank = dyad.parse_rank, \
        share associativity = left, \
        share parse = ( \
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
        ("h := holder (), c := h.items, c[0] = i32 7, h.items[0]", "7"),
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
         share put := fn () -> u64 ( items = array u8 [1], 0 ) )"
    ));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("these types do not match"), "stderr: {stderr}");
}

/// `BOX` and a `bag` whose owning fields hold an array and a box, its free printing «bag»,
/// in the array's shape: `bag ()` makes a new `bagged`.
fn bag_line(tail: &str) -> String {
    format!(
        "import ./identities/array.logos, {BOX}, t := array i32, \
         bagged := type ( \
             mut items := own t ?, \
             mut b := own boxed ?, \
             share free = ( print «bag», free items, free b ), \
             share parse_rank = dyad.parse_rank, \
             share parse = ( tape.is_constructed[0] = true ) ), \
         bag := type ( \
             output_type := type ?, \
             share run = ( \
                 mut v := output_type ?, \
                 v.items = array i32 [4, 5, 6], v.b = box (7, 1), move v ), \
             share parse_rank = dyad.parse_rank, \
             share associativity = left, \
             share parse = ( \
                 if tape[1].type == scope ( \
                     tape[0].type = bag, tape[0].output_type = bagged, tape.remove(1) \
                 ) else ( tape[0] = bagged ), \
                 tape.is_constructed[0] = true \
             ) ), {tail}"
    )
}

#[test]
fn an_owning_field_is_freed_once_by_the_owner_s_free() {
    for (tail, printed) in [
        ("g := bag (), print «made»", "made\nbag\nfree\n"),
        ("g := bag (), free g, print «after»", "bag\nfree\nafter\n"),
        ("f := fn () -> i32 ( g := bag (), g.items[2] ), f(), print «after»", "bag\nfree\nafter\n"),
        ("g := bag (), h := g, print «borrowed»", "borrowed\nbag\nfree\n"),
        // A move out of the field leaves the bag's free nothing to free there.
        ("g := bag (), y := move g.b, print «moved»", "moved\nfree\nbag\n"),
        // `=` frees the box it displaces before the write.
        ("g := bag (), x := box (8, 1), g.b = move x, print «set»", "free\nset\nbag\nfree\n"),
        ("l := array bagged [bag (), bag ()], print «made»", "made\nbag\nfree\nbag\nfree\n"),
        ("g := bag (), l := array bagged [move g], print «made»", "made\nbag\nfree\n"),
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
        ("l := array bagged [bag (), bag ()], l[1].items[0] = i32 8, l[1].items[0]", "8"),
    ] {
        let (code, stdout, stderr) = run_line(&bag_line(tail));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.lines().last(), Some(printed), "{tail}: {stdout}");
    }
    for (tail, expect) in [
        ("g := bag (), x := array i32 [1], g.items = x", "what is assigned must own too"),
        ("g := bag (), x := array i32 [1], y := x, g.items = move y", "only its owner can"),
        ("nd := type ( mut items := own t ? )", "must be freed by the type's `share free = (…)`"),
        ("g := bag (), x := array i32 [1], g.items = own x", "`move x` moves a value"),
        ("mk := fn () -> own @i32 ( alloc 1 of i32 7 ), 1", "`-> own @T` is not in the seed"),
        ("nd := type ( mut n := own i32 ? )", "or a hole of a type"),
        ("f := fn (p := own t ?) -> i32 ( 1 )", "an `own` parameter"),
    ] {
        let (code, _, stderr) = run_line(&bag_line(tail));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
    // A compiled body holding a free or move out of the field declines to compile;
    // interpreted, the same body frees each node once.
    for line in ["free g.b", "y := move g.b"] {
        let f = format!("f := fn () -> i32 ( g := bag (), {line}, 1 )");
        let (code, stdout, stderr) = run_line(&bag_line(&format!("{f}, f.compile(), f()")));
        assert_eq!(code, Some(1), "{line}: stdout: {stdout}");
        assert!(stdout.is_empty(), "{line}: stdout: {stdout}");
        assert!(stderr.contains("cannot be compiled yet"), "{line}: stderr: {stderr}");
        let (code, stdout, stderr) = run_line(&bag_line(&format!("{f}, f(), print «after»")));
        assert_eq!(code, Some(0), "{line}: stderr: {stderr}");
        assert_eq!(stdout, "free\nbag\nafter\n", "{line}");
    }
}

#[test]
fn a_chooser_takes_the_type_the_chooser_right_of_it_leaves() {
    let array = "import ./identities/array.logos";
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
    let array = "import ./identities/array.logos";
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
            "mk := fn () -> array array i32 ( array array i32 [[1, 2], [3, 4]] ), \
             m := mk(), m[1][0]",
            "3",
        ),
        (
            "mk := fn (v := i32 ?) -> array array i32 ( array array i32 [[v, 2], [3, v + 1]] ), \
             m := mk(4), n := mk(6), m[1][1] + n[1][1]",
            "12",
        ),
        (
            "mk := fn (v := i32 ?) -> array array i32 ( array array i32 [[v, 2], [3, v + 1]] ), \
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
        ("mk := fn () -> array array i32 ( array array i32 [[1, 2], [3, 4]] ), mk()[1][1]", "4"),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.trim(), printed, "{tail}");
    }
    for (tail, expect) in [
        ("a := array array i32 [[1, 2], 3]", "an element does not fit the element type"),
        ("a := array array i32 [(1, 2)]", "the list is written in square brackets"),
        ("x := i64 5, a := array array i32 [[1], [x]]", "an element does not fit the element type"),
        (
            "p := type ( share parse = ( tape.construct(i32, tape[1]), tape.is_constructed[0] = true ) )",
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
    let array = "import ./identities/array.logos";
    for (tail, printed) in [
        ("a := array boxed [box (1, 2), box (3, 4)], print «made»", "made\nfree\nfree\n"),
        (
            "f := fn () -> i32 ( a := array boxed [box (1, 2), box (3, 4)], 7 ), \
             print «before», f(), print «after»",
            "before\nfree\nfree\nafter\n",
        ),
        (
            "f := fn () -> i32 ( a := array boxed [box (1, 2)], 7 ), f.compile(), \
             print «before», f(), print «after»",
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
             f(), f(), print «after»",
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
    let array = "import ./identities/array.logos";
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
        (
            "p := type ( x := i32 ?, y := i32 ?, share s := fn () -> i32 ( free x, y ) ), \
             q := p (1, 2), q.s()",
            "2\n",
        ),
    ] {
        let (code, stdout, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout, printed, "{tail}");
    }
}

#[test]
fn free_of_a_value_runs_it_and_frees_what_it_made() {
    let array = "import ./identities/array.logos";
    for (tail, printed) in [
        ("free (box (1, 2)), print «after»", "free\nafter\n"),
        ("free box (1, 2), print «after»", "free\nafter\n"),
        ("mk := fn () -> boxed ( box (1, 2) ), free (mk()), print «after»", "free\nafter\n"),
        (
            "g := fn () -> i32 ( free (box (1, 2)), 7 ), g.compile(), print «before», g(), \
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
    // A field of a node the parse knows is a place: its block is freed at `free b.p`.
    let (code, _, stderr) =
        run_line(&format!("{array}, {BOX}, b := box (1, 2), free b.p, (b.p + 0)@"));
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("this pointer holds nothing yet"), "stderr: {stderr}");
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
            "c := i32 1, free (if (c == 1) (alloc 1 of i32 5) else (alloc 1 of i32 6)), 1",
            "(if",
            "`free` runs a value and then its type's `free`",
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
    let array = "import ./identities/array.logos, a := array i32 [1, 2, 3]";
    for (tail, printed) in
        [("free a, 5", "5"), ("b := a, f := fn (p := array i32 ?) -> i32 ( p[1] ), f(b)", "2")]
    {
        let (code, stdout, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(0), "{tail}: stderr: {stderr}");
        assert_eq!(stdout.trim(), printed, "{tail}");
    }
    // Until the borrow checker, a borrow may outlive its owner; the free emptied the
    // array's pointer, so a read at the first cell is refused (one further in is not).
    for (tail, expect) in
        [("free a, a[0]", "`a` is dead here"), ("b := a, free a, b[0]", "holds nothing")]
    {
        let (code, _, stderr) = run_line(&format!("{array}, {tail}"));
        assert_eq!(code, Some(1), "{tail}: stderr: {stderr}");
        assert!(stderr.contains(expect), "{tail}: stderr: {stderr}");
    }
}

/// A type with no `run` whose own `parse` builds its nodes.
const COUNTED: &str = "q := type ( n := u64 ?, k := u64 7, \
    share twice := fn () -> u64 ( n * 2 ), share parse_rank = 60, share parse = ( \
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
    let bx = "bx := type ( mut p := own @i32 ?, mut size := u64 0, share free = ( free p ), \
              share parse_rank = dyad.parse_rank, share parse = ( tape.is_constructed[0] = true ) )";
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
fn a_bare_share_call_works_on_the_value_the_body_is_about() {
    let quad = COUNTED.replace(
        "share twice := fn () -> u64 ( n * 2 ),",
        "share twice := fn () -> u64 ( n * 2 ), share quad := fn () -> u64 ( twice() * 2 ),",
    );
    let (code, stdout, stderr) = run_line(&format!("{quad}, a := q (1, 2, 3), a.quad()"));
    assert_eq!((code, stdout.as_str()), (Some(0), "12\n"), "stderr: {stderr}");
    // From a run: a value holding this run's fields, so an operand written as an
    // expression, or a function's parameter, is read as its value.
    let inc = "inc := type ( a := i32 ?, output_type := type ?, \
        share twice := fn () -> i32 ( a * 2 ), share run = ( twice() + 1 ), \
        share parse_rank = *.parse_rank + 1, share parse = ( tape[0].type = inc, \
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
    // A parse has no value to hand one that reads a field; outside the type it is no name.
    for (src, expect) in [
        (
            "q := type ( n := u64 ?, share get := fn () -> u64 ( n ), share parse_rank = 60, \
             share parse = ( tape[0].type = q, tape[0].n = get(), tape.is_constructed[0] = true ) ), q"
                .to_string(),
            "reads no field",
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
            "q := type ( n := u64 ?, share parse_rank = 60, share parse = ( \
             tape[0].n = 3, tape[0].type = q, tape.is_constructed[0] = true ) ), q",
            "still holds the type",
        ),
        (
            "r := type ( n := u64 ? ), q := type ( share parse_rank = 60, share parse = ( \
             tape[0].type = r, tape.is_constructed[0] = true ) ), q",
            "not in the seed yet",
        ),
        (
            "q := type ( share parse_rank = 60, share parse = ( \
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
        "t := type (a := i32 ?, b := i32 ?, output_type := type ?, share run = ( a ), \
         share parse_rank = *.parse_rank + 1, \
         share parse = ( tape[0].type = t, tape[0].a = tape[-1], tape[0].output_type = i32, \
         tape.is_constructed[0] = true, tape.remove(-1) )), x := i32 4, x t",
        // The unwritten field itself is read in the parse body.
        "q := type ( size := u64 ?, share parse_rank = 60, \
         share parse = ( tape[0].type = q, tape[0] = tape[0].size, tape.is_constructed[0] = true ) ), q",
    ] {
        let (code, _, stderr) = run_line(src);
        assert_eq!(code, Some(1), "{src}: stderr: {stderr}");
        assert!(stderr.contains("was never written by its constructor"), "{src}: stderr: {stderr}");
    }
}

#[test]
fn a_fields_type_reads_its_declared_type() {
    let (code, stdout, stderr) = run_line(
        "q := type ( size := u64 ?, share parse_rank = 60, \
         share parse = ( tape[0].type = q, tape[0].size = 3, tape[0] = tape[0].size.type, \
         tape.is_constructed[0] = true ) ), q",
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "u64\n"), "stderr: {stderr}");
    // An untyped field has no type until the constructor runs and writes it.
    let (code, _, stderr) = run_line(
        "q := type ( size := ?, share parse_rank = 60, \
         share parse = ( tape[0].type = q, tape[0].size = 3, tape[0] = tape[0].size.type, \
         tape.is_constructed[0] = true ) ), q",
    );
    assert_eq!(code, Some(1), "stderr: {stderr}");
    assert!(stderr.contains("does not fit the node's type"), "stderr: {stderr}");
}

#[test]
fn a_field_default_fills_each_new_node() {
    let q = "q := type ( mut size := u64 5, output_type := type ?, \
             share run = ( size + 1 ), share parse_rank = 60, \
             share parse = ( tape[0].type = q, ";
    let end = "tape[0].output_type = u64, tape.is_constructed[0] = true ) )";
    for (src, want) in [
        (format!("{q}{end}, q"), "6\n"),
        (format!("{q}tape[0].size = 9, {end}, q"), "10\n"),
        (format!("{q}{end}, f := fn () -> u64 ( q ), f.compile(), f()"), "6\n"),
        (
            "q := type ( size := u64 0, share parse_rank = 60, \
             share parse = ( tape[0].type = q, tape[0] = tape[0].size, tape.is_constructed[0] = true ) ), q"
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
              share run = ( a.y ), share parse_rank = 60, share parse = ( tape[0].type = fx, \
              tape[0].a = tape[1], tape[0].output_type = i32, tape.is_constructed[0] = true, \
              tape.remove(1) ) ), v := p (1, 2)";
    for tail in ["fx v", "g := fn () -> i32 ( fx v ), g.compile(), g()"] {
        let (code, stdout, stderr) = run_line(&format!("{fx}, {tail}"));
        assert_eq!((code, stdout.as_str()), (Some(0), "2\n"), "{tail}: stderr: {stderr}");
    }
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
        ("x := i32 3, print «{(&x).type == @i32}», p := &x, p.type == @i32", "true\ntrue\n"),
        (
            "mut c := true, a := alloc 1 of i32 5, b := alloc 1 of i32 6, \
             x := if c (move a) else (move b), y := if c (alloc 1 of i32 7) else (alloc 1 of i32 8), \
             print «{x@} {y@}», free x, free y",
            "5 7\n",
        ),
        ("f := fn () -> @i32 ( alloc 1 of i32 0 ), f().type == @i32", "true\n"),
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
        ("x := f64 2.5, (return x) + 1.0", "gives nothing"),
        ("x := f64 2.5, y := (return x), y", "gives nothing"),
        ("f := fn (x := f64 ?) -> f64 ( (return x) + 1.0 ), f(2.5)", "gives nothing"),
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
    let r = "r := type ( mut p := own @i32 ?, share free = ( free p ) )";
    let q = "q := alloc 1 of own @i32 ?, a := alloc 1 of i32 5, q@ = move a";
    let x = "mut x := r ?, a := alloc 1 of i32 5, x.p = move a";
    for src in [
        format!("{r}, {x}, pp := &x, free pp@.p, x.p@"),
        format!("{r}, f := fn (pp := @r ?) -> i32 ( free pp@.p, 1 ), {x}, f(&x), x.p@"),
        format!("{q}, free q@, q@@"),
    ] {
        let (code, _, stderr) = run_line(&src);
        assert_eq!(code, Some(1), "{src}");
        assert!(stderr.contains("this pointer holds nothing yet"), "{src}: stderr: {stderr}");
    }
    for (src, want) in [
        (format!("{r}, {x}, pp := &x, b := move pp@.p, b@"), "5\n"),
        (format!("{q}, b := move q@, b@"), "5\n"),
        (
            format!("{BOX}, g := fn () -> i32 ( b := box (5, 6), y := move b.p, y@ ), g()"),
            "free\n5\n",
        ),
        ("x := 5, move x".to_string(), "5\n"),
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, want, "{src}");
    }
}

#[test]
fn a_generic_body_is_keyed_by_the_type_its_operand_gives_back() {
    let isbool = "isbool := type ( a := ?, output_type := type ?, \
                  share run = ( if (a.type == bool) (i32 1) else (i32 2) ), \
                  share parse_rank = *.parse_rank + 1, share associativity = left, \
                  share parse = ( tape[0].type = isbool, tape[0].a = tape[-1], \
                  tape[0].output_type = i32, tape.is_constructed[0] = true, tape.remove(-1) ) )";
    let pass = "pass := type ( a := ?, output_type := type ?, share run = ( a ), \
                share parse_rank = *.parse_rank + 1, share associativity = left, \
                share parse = ( tape[0].type = pass, tape[0].a = tape[-1], \
                tape[0].output_type = tape[-1].type, tape.is_constructed[0] = true, \
                tape.remove(-1) ) )";
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
    ] {
        let (code, stdout, stderr) = run_line(&src);
        assert_eq!(code, Some(0), "{src}: stderr: {stderr}");
        assert_eq!(stdout, want, "{src}");
    }
}
