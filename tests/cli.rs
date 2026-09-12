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
fn an_import_runs_the_file_and_prints_its_tail() {
    // #58: the command line is Logos source — `logos import ./file.logos`
    // runs the file top to bottom, and the line's value is the file's tail.
    let out = logos().args(["import", "examples/answer.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn the_drop_model_runs_at_file_scope() {
    // The drop model (issue #49) through the real driver: a top-level `alloc`,
    // an `own` move, and a block whose own alloc frees at its exit. The driver
    // drains the top level's teardowns at program exit, so the program prints
    // its tail value (42) and exits clean — no leak, no crash.
    let out = logos().args(["import", "tests/fixtures/heap.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn a_parse_error_renders_clickable_with_a_caret() {
    // The inner report keeps the imported file's own coordinates and caret,
    // wrapped in the import-failed frame.
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
    // Pub-only exposure (#58 over #33): the importer reaches `pub double`,
    // and the unmarked `helper` stays invisible — fail-closed.
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
    // A declaration-tailed file has no value to print; the run still counts.
    let out = logos().args(["import", "tests/fixtures/lib_pub.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stdout.is_empty());
}

#[test]
fn an_imported_file_cannot_see_the_import_site() {
    // The fresh view (ruled August 2026): the imported scope resolves ambient
    // names and its own imports only — never the command line's declarations.
    let out = logos().arg("x := 5, import tests/fixtures/uses_missing.logos").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("uses_missing.logos:2:6"), "stderr: {err}");
    assert!(err.contains("not in scope"), "stderr: {err}");
}

#[test]
fn a_relative_import_resolves_against_the_importing_file() {
    // outer.logos imports ./subdir/inner.logos — relative to ITS folder, not
    // the working directory (ruled August 2026).
    let out = logos().args(["import", "tests/fixtures/outer.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn an_import_cycle_is_a_checked_error() {
    // The import graph must be a DAG (ruled August 2026).
    let out = logos().args(["import", "tests/fixtures/cycle_a.logos"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("cycle"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_repeat_import_is_idempotent() {
    // Once per run: the second import finds the loaded file and republishing
    // the same identities is not a shadowing error.
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
    // The REPL threads one import registry across lines (a session is a run):
    // the answer echoes through the import's tail, the library import is
    // silent, and its pub fn stays callable on a later line.
    let (echoes, stderr) =
        repl(b"import examples/answer.logos\nimport tests/fixtures/lib_pub.logos\ndouble(4)\n");
    assert_eq!(echoes, ["42", "8"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_view_reads_the_cell_and_operands_are_ordinary_fields() {
    // #52: the view exposes exactly the cell's two fields (`.logos`, `.value`)
    // — the dyad logos defines nothing else — while an operator node's
    // slots are fields ITS logos defines: `.operands` is that collection and
    // `[i]` fetches an element from it (element access is `[…]`, application
    // is `(…)`): `(x + x).operands[0]`, no view involved. Reads fold at
    // parse (comptime reflection). The honest cell surface shows the op slot
    // too: `(x + x)` has arity 3, and operands[2] is the resolved callable
    // leaf, not an i32.
    let (echoes, stderr) = repl(
        b"x := i32 5\nx:dyad.type == i32\n(x + x):dyad.type.arity\n\
          (x + x).operands[0]:dyad.type == i32\n(x + x).operands[2]:dyad.type == i32\n\
          (x + x).operands[0]\n",
    );
    assert_eq!(echoes, ["true", "3", "true", "false", "5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_type_body_fills_its_slots_and_declares_its_members() {
    // DESIGN ›The constructor is a field‹ (#61): the six slots `type`
    // declares are filled with `=` — the parse_rank spelled relative, the
    // associativity one of the two identities `left` and `right` (of type
    // `type`, like a keyword, ruled 9 September 2026) — bare `:=` lines are
    // the type's own members, read `g.y`, and `instance (…)` holds the
    // per-instance fields, checked against their siblings alone.
    let (echoes, stderr) = repl(
        b"t := type (parse_rank = *.parse_rank + 1, associativity = right)\n\
          t.parse_rank\nt.associativity == right\nright:dyad.type == type\n",
    );
    assert_eq!(echoes, ["71.0", "true", "true"], "stderr: {stderr}");
    let (echoes, stderr) = repl(b"g := type (y := 3, z := y + 3)\ng.y\ng.z\ng.parse_rank\n");
    assert_eq!(echoes, ["3", "6", "91.0"], "stderr: {stderr}");
    // A member's initializer runs at the definition, so a *typed* member holds
    // its value too — the body's own declarations run as they are parsed, the
    // one pass over a type body as over a file. Before that, only the untyped
    // comptime form worked, because it folds at parse, and `g.y` read the
    // zeroed place (#87).
    let (echoes, stderr) = repl(b"g := type (y := i32 7, z := i32 (y + 3))\ng.y\ng.z\ng.y + 1\n");
    assert_eq!(echoes, ["7", "10", "8"], "stderr: {stderr}");
    // A declaration that faults at the definition says so, and names the type
    // body it was in.
    let (echoes, stderr) = repl(b"h := fn () -> i32 ( p := @i32 ?, p@ )\ng := type (y := h())\n");
    assert!(
        echoes.is_empty() && stderr.contains("a type body's own declaration failed"),
        "stderr: {stderr}"
    );
    // `lex_rank` (ruled 10 September 2026, #113): the order among pattern
    // spellings competing at one text position, a stored slot the seed reads
    // for nothing yet; 0 on every identity whose body did not set it.
    let (echoes, stderr) = repl(b"r := type (lex_rank = 3)\nr.lex_rank\n+.lex_rank\n");
    assert_eq!(echoes, ["3.0", "0.0"], "stderr: {stderr}");
    // The superseded spelling is no slot: inside a body it is an unknown name.
    let (echoes, stderr) = repl(b"s := type (precedence = 5)\n");
    assert!(echoes.is_empty() && stderr.contains("unknown name"), "stderr: {stderr}");
    // `code` holds a function and nothing else (#63).
    let (echoes, stderr) = repl(b"c := type (code = 5)\n");
    assert!(echoes.is_empty() && stderr.contains("must be a function"), "stderr: {stderr}");
    let (echoes, stderr) = repl(b"x := 1\np := type (instance (x := i32 ?))\nq := p(2)\nq.x\nx\n");
    assert_eq!(echoes, ["2", "1"], "stderr: {stderr}");
}

#[test]
fn any_spelling_the_index_can_hold_is_nameable() {
    // DESIGN ›The scope's constructor is the driver‹ (ruled 10 September 2026,
    // #110): a spelling nothing declared lexes through one of two fresh
    // patterns, a word or a symbol run, ranked below every declared spelling;
    // the highest-ranked candidate wins, the longest at equal rank. So `^` is
    // a name like any other, a declared `a` does not cut the fresh `ab2`, a
    // declared `@` beats the run `@@`, `=` and `-` glue without a rank set,
    // and a fresh symbol used as an operand is the leftover-cell error at its
    // own column.
    let (echoes, stderr) = repl(
        b"^ := i32 5\n^ + 1\na := i32 1\nab2 := i32 2\nab2 + a\nx := i32 5\nx=-1\nx\n\
          p := type (instance (v := i32 ?))\nq := p(7)\nr := &q\nrr := &r\nrr@@.v\nx^2\n",
    );
    assert_eq!(echoes, ["6", "3", "-1", "7"], "stderr: {stderr}");
    // The fresh `^` is the leftover cell of its line, reported at its column.
    assert!(stderr.contains("<repl>:1:2: error:"), "stderr: {stderr}");
    // The demo's operator with its `code` slot (#63): `^` defined with
    // `type`, right-associative, above `*`, its constructor and its code in
    // Logos, used glued (`x^3`) inside a function that compiles. 9 + 512 + 18.
    let out = logos().args(["import", "tests/fixtures/caret.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "539\n");
}

#[test]
fn a_pattern_spelling_is_declared_through_regex() {
    // DESIGN ›The scope's constructor is the driver‹ (ruled 10 September 2026,
    // #114): `regex «…»` reads its own quote and yields a recognizer, which
    // `:=` enters into the index as written. So a spelling that is a pattern
    // (`5k`), or a compound of known symbols (`<=>`, beating `<=` `>` by
    // length at equal rank), becomes a name like any other; the same key live
    // twice is the shadowing error (the core's `..` is spelled `\.\.`).
    let (echoes, stderr) = repl(
        b"regex \xc2\xab[0-9]+[kK]\xc2\xbb := type ()\n(5k):dyad.type == type\n\
          regex \xc2\xab<=>\xc2\xbb := type ()\n(<=>):dyad.type == type\n\
          regex \xc2\xab\\.\\.\xc2\xbb := type ()\n",
    );
    assert_eq!(echoes, ["true", "true"], "stderr: {stderr}");
    assert!(stderr.contains("shadowed"), "stderr: {stderr}");
    // Two declared patterns of equal rank matching the same length is the
    // inconsistency the definitions must correct; the seed reports it where
    // text hits both. A `lex_rank` on one of them settles it.
    let (echoes, stderr) = repl(
        b"regex \xc2\xab[a-z][0-9]\xc2\xbb := type ()\nregex \xc2\xaba[0-9]\xc2\xbb := type ()\na1\n\
          regex \xc2\xabb[0-9]\xc2\xbb := type (lex_rank = 1)\n(b1):dyad.type == type\n",
    );
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
    assert!(stderr.contains("same lex_rank"), "stderr: {stderr}");
    // A pattern that does not compile is refused at its quote; `regex` with
    // no quote is refused at the word; a failed line rolls its pattern back.
    let (echoes, stderr) = repl(
        b"regex \xc2\xab[unclosed\xc2\xbb := type ()\nregex 5\n\
          regex \xc2\xabz[0-9]\xc2\xbb := type (\nregex \xc2\xabz[0-9]\xc2\xbb := type ()\n(z1):dyad.type == type\n",
    );
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
    assert!(stderr.contains("does not compile"), "stderr: {stderr}");
    assert!(stderr.contains("must be followed by a"), "stderr: {stderr}");
}

#[test]
fn a_constructor_written_in_logos_runs_during_the_parse() {
    // Issue #61's done-when: a file defines a type with `parse_rank = …`,
    // `associativity = …`, and a `constructor = fn (tape) -> void (…)`, and a
    // later appearance in the same file runs that constructor during the
    // parse — here a postfix `squared` and an infix `plus2`, the nodes they
    // build calling like any other, interpreted and compiled.
    let out = logos().args(["import", "tests/fixtures/squared.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "92\n");
    // A constructor that touches nothing declines: the identity stands as
    // its own value (DESIGN ›The scope's constructor is the driver‹).
    let (echoes, stderr) = repl(
        b"noop := type (constructor = fn (tape := parsing_tape ?) -> void ( tape.recenter(0) ))\n\
          t := noop\nt:dyad.type == type\nnoop.parse_rank\n",
    );
    assert_eq!(echoes, ["true", "91.0"], "stderr: {stderr}");
    // A constructor that fails is the checked error, reported at the appearance.
    let (_echoes, stderr) = repl(
        b"bad := type (constructor = fn (tape := parsing_tape ?) -> void ( tape[5] ))\nx := bad\n",
    );
    assert!(stderr.contains("constructor failed"), "stderr: {stderr}");
}

#[test]
fn a_type_body_refuses_what_is_not_its_own() {
    // `:=` on a slot name is the no-shadowing error; a member may not shadow
    // an outer name; `instance` belongs in a body, once; a line that would
    // only run is refused; the slot values are checked.
    for (src, expect) in [
        (&b"t := type (parse_rank := 5)\n"[..], "shadowed"),
        (b"y := 1\ng := type (y := 3)\n", "shadowed"),
        (b"instance (x := i32 ?)\n", "belongs inside a type body"),
        (b"t := type (instance (a := i32 ?), instance (b := i32 ?))\n", "one `instance"),
        (b"t := type (5)\n", "a type body line"),
        // `instance` without its bracket declares nothing: it used to stand as
        // a bare value and pass as a silent no-op (#87).
        (b"t := type (instance)\n", "a type body line"),
        (b"t := type (associativity = 5)\n", "`left` or `right`"),
        (b"t := type (constructor = fn (a := i32 ?) -> void ( a = 1 ))\n", "parsing_tape"),
        (
            b"t := type (destructor = fn (tape := parsing_tape ?) -> void ( tape.remove(1) ))\n",
            "destructor",
        ),
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
    // `and` over two non-booleans is a group the consuming operator
    // distributes over (DESIGN, 7 September 2026; the seed's `append`), so
    // `if` refuses it as a condition, and a boolean beside a non-boolean is
    // still the operand error.
    let (_echoes, stderr) = repl(b"x := i32 1\ny := i32 2\nif (x and y) (1) else (2)\n");
    assert!(stderr.contains("must be a bool"), "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"x := i32 1\nx and true\n");
    assert!(stderr.contains("must be bools"), "stderr: {stderr}");
}

#[test]
fn a_collection_member_demands_its_index_brackets() {
    // Element access is `[…]`; the call form is refused with a teaching
    // message, and the bare collection as a value waits for the array logos.
    let (_echoes, stderr) = repl(b"x := i32 5\n(x + x).operands(0)\n");
    assert!(stderr.contains("element access is `[…]`"), "stderr: {stderr}");
}

#[test]
fn a_square_bracket_is_a_paren_that_closes_only_itself() {
    // `[` is `(` in square brackets (ruled 9 September 2026): its interior is
    // any expression, and each opener takes only its own closer.
    let (echoes, stderr) = repl(b"x := i32 5\n(x + x).operands[1 - 1]\n");
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"(1]\n");
    assert!(stderr.contains("never closed"), "stderr: {stderr}");
    let (_echoes, stderr) = repl(b"x := i32 5\n(x + x).operands[0)\n");
    assert!(stderr.contains("never closed"), "stderr: {stderr}");
}

#[test]
fn dot_logos_off_the_view_is_a_guided_error() {
    // `.` reads only the fields a logos defines — about the value — so
    // `x.type` no longer exists; the error teaches the view spelling.
    let (_echoes, stderr) = repl(b"x := i32 5\nx.type\n");
    assert!(stderr.contains("dyad view"), "stderr: {stderr}");
}

#[test]
fn a_reflect_read_that_does_not_fit_is_an_error() {
    // The ruled checked error: the view has no member beyond the two cell
    // fields, so `.operands` through it has nothing to read.
    let (_echoes, stderr) = repl(b"x := i32 5\nx:dyad.operands[0]\n");
    assert!(stderr.contains("does not fit"), "stderr: {stderr}");
}

#[test]
fn an_import_inside_a_fn_body_is_rejected() {
    // The load is a comptime effect; inside a fn body parse and run order do
    // not coincide, so it is rejected like a logos variable's fill.
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
            b"x := i32 5\nx = 40\ndouble := fn (a := i32 ?) -> i32 ( a + a )\nzz\ndouble(x) + 2\n",
        )
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
    let (echoes, stderr) =
        repl(b"b := )\nb := 5\nf := fn (v := i32 ?) -> i32 ( v )\ng := f(1,2)\ng := f(3)\ng + b\n");
    assert_eq!(echoes, ["8"], "stderr: {stderr}");
    assert!(stderr.contains("nothing to evaluate here"), "stderr: {stderr}");
    assert!(stderr.contains("argument count"), "stderr: {stderr}");
    assert!(!stderr.contains("shadowed"), "stderr: {stderr}");
}

#[test]
fn the_repl_keeps_an_owning_binding_alive_across_lines() {
    // A REPL binding lives for the whole session, so its teardown belongs at
    // session exit, not end of line: `a` is still readable on a later line, and
    // a block's own allocation is freed at the block's exit as in file mode
    // (file and REPL are one pass and must agree).
    let (echoes, stderr) = repl(b"a := alloc i32 5\na@\nr := ( b := alloc i32 20, b@ )\nr\n");
    assert_eq!(echoes, ["5", "20"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_repl_reuses_a_name_after_drop() {
    // DESIGN ›Name resolution is scope-filtered‹ (3 September 2026): a session
    // name is recycled by `drop n` then `n := …`, and between the two the dead
    // name refuses a read instead of reporting "shadowed" forever.
    let (echoes, stderr) = repl(b"n := i32 5\ndrop n\nn\nn := i32 6\nn\n");
    assert_eq!(echoes, ["6"], "stderr: {stderr}");
    assert!(stderr.contains("<repl>:1:1: error: this name is dead here"), "stderr: {stderr}");
}

#[test]
fn a_failed_repl_line_restores_a_moved_name() {
    // A line that moves `a` out and then fails is rolled back whole: the dead
    // mark lifts with the line's declarations, so `a` reads on the next line
    // as if the line had never been typed.
    let (echoes, stderr) = repl(b"a := alloc i32 5\nr := ( b := own a, b@ ) + nosuch\na@\n");
    assert_eq!(echoes, ["5"], "stderr: {stderr}");
    assert!(stderr.contains("unknown name"), "stderr: {stderr}");
    assert!(!stderr.contains("dead"), "stderr: {stderr}");
}

#[test]
fn an_owning_value_that_nothing_can_free_is_refused() {
    // The fail-closed edge of the drop model: an owning value with no name to
    // attach its teardown to is refused at parse rather than leaked at run.
    let out = logos().args(["import", "tests/fixtures/unbound_owning.logos"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("must be bound to a name"), "stderr: {err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn the_repl_compiles_a_fn_across_lines() {
    // `f.compile()` on one line installs the machine code; the call on the
    // next line jumps to it. The compile itself is a silent statement, so the
    // only echo is the call's value.
    let (echoes, stderr) =
        repl(b"double := fn (x := i64 ?) -> i64 ( x + x )\ndouble.compile()\ndouble(21)\n");
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
    let (echoes, stderr) =
        repl(b"t := i32\nx := t 7\nt(9)\nf := fn (v := t ?) -> t ( v + v )\nt\nf(x)\n");
    assert_eq!(echoes, ["9", "14"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn logos_is_a_value_reflected_by_dot_logos_and_compared_by_identity() {
    // Roadmap #30: `logos` is a first-class value. `.logos` yields a value's logos, and
    // `==`/`!=` compare logos by identity (logos are interned, so pointer identity is
    // logos identity). Every result is a bool, so it echoes; declarations stay silent.
    let (echoes, stderr) = repl(
        b"i32 == i32\ni32 == f64\ni32 != f64\ni32:dyad.type == logos\ni32:dyad.type == i32\n\
          x := i32 5\nx:dyad.type == i32\nx:dyad.type == f64\nt := logos\ni32:dyad.type == t\nlogos:dyad.type == logos\n",
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
    // A program whose value is a logos prints the logos's name, not the raw bit
    // container (roadmap #30). The value rides out of a scope (comment + expression).
    let out = logos().args(["import", "tests/fixtures/logos_name.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "i32\n");
}

#[test]
fn a_type_returning_function_resolves_at_comptime() {
    // Roadmap #30 Phase 2: a `-> logos` call is run during parsing and becomes the
    // concrete logos it yields, so it flows through `==` and `:=` like any logos.
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
    // Build and run are one pass: a top-level expression runs the moment it is
    // parsed, so parse-time evaluation (a `-> logos` call reading an earlier
    // binding) sees committed state and file mode agrees with the REPL. Before,
    // the file driver parsed everything first and ran afterward, so the call
    // read x's zeroed storage instead of 5 and answered i32 rather than f64.
    let out = logos()
        .args(["import", "tests/fixtures/comptime_sees_committed_state.logos"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

#[test]
fn a_type_call_with_a_runtime_argument_is_rejected() {
    // A `-> logos` call is comptime-only; an argument not known at parse time (here a
    // function parameter) is reported, not silently mis-evaluated.
    let (_echoes, stderr) = repl(
        b"pick := fn (i := i32 ?) -> logos (if (i==0)(i32) else (f64))\n\
          g := fn (n := i32 ?) -> i32 ( a := pick(n), 1 )\n",
    );
    assert!(stderr.contains("must be evaluable at parse time"), "stderr: {stderr}");
}

#[test]
fn a_logos_declaration_declares_a_place_of_that_type() {
    // `a := i32 ?` introduces the name with its logos slot set and its value
    // undefined (zeroed until phase bits land): the declaration is silent,
    // `.logos` reflects the declared logos, `=` fills the value, reads load it.
    let (echoes, stderr) = repl(b"a := i32 ?\na:dyad.type == i32\na = 9\na\n");
    assert_eq!(echoes, ["true", "9"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_dependent_typed_declaration_takes_a_computed_type() {
    // `b := metalogos(1) ?` — the declared logos is the result of running a
    // `-> logos` function at parse time (roadmap #30): the dependent
    // declaration is the same declaration, its logos just computed.
    let (echoes, stderr) = repl(
        b"metalogos := fn (i := i32 ?) -> logos (if (i==0)(i32) else (f64))\n\
          b := metalogos(1) ?\nb:dyad.type == f64\nb = 7\nb\n",
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
    // `5 ?`: a hole after a value is no declared type — two cells where one
    // must stand, the leftover error.
    let (_echoes, stderr) = repl(b"a := 5 ?\n");
    assert!(stderr.contains("expected one expression, found more"), "stderr: {stderr}");
}

#[test]
fn a_logos_declaration_names_the_non_numeric_gap() {
    // Storage for `a := bool ?` (and struct/pointer/void declarations) is not in
    // the seed yet; the error names the gap instead of mis-storing. (`a := logos ?`
    // is no longer a gap — it declares a logos variable.)
    let (_echoes, stderr) = repl(b"a := bool ?\n");
    assert!(stderr.contains("non-numeric types are not in the seed yet"), "stderr: {stderr}");
}

#[test]
fn a_logos_variable_declares_fills_once_and_becomes_the_type() {
    // `a := logos ?` declares a logos variable (an undefined logos); `a = i32` fills
    // it at parse — comptime rebinding — after which the name is a full
    // spelling of the logos: `==` folds, juxtaposition builds typed values.
    let (echoes, stderr) =
        repl(b"a := logos ?\na:dyad.type == logos\na == i32\na = i32\na == i32\ny := a 5\ny\n");
    assert_eq!(echoes, ["true", "false", "true", "5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_logos_variable_fill_is_define_once_and_comptime_only() {
    // A second fill finds a real logos, not the placeholder, and is an ordinary
    // (rejected) assignment; a fill inside a fn body is rejected explicitly —
    // it would rebind at parse, where parse and run do not coincide.
    let (_e1, stderr1) = repl(b"a := logos ?\na = i32\na = f64\n");
    assert!(stderr1.contains("not an assignable place"), "stderr: {stderr1}");
    let (_e2, stderr2) = repl(b"a := logos ?\ng := fn () -> i32 ( a = i32, 1 )\n");
    assert!(stderr2.contains("where parsing and running coincide"), "stderr: {stderr2}");
    let (_e3, stderr3) = repl(b"a := logos ?\na = 5\n");
    assert!(stderr3.contains("must be a type value"), "stderr: {stderr3}");
}

#[test]
fn logical_operators_fold_over_bool_literals() {
    // and/or/not over bare bool literals fold at parse (pure, nothing lost) —
    // what keeps a comptime chain comptime; runtime operands still build nodes.
    let (echoes, stderr) = repl(
        b"true or false\ntrue and true\nnot (true)\n\
          a := logos ?\nif (a:dyad.type == f32 or a:dyad.type == logos) (a = f64) else (a = i32)\na == f64\n",
    );
    assert_eq!(echoes, ["true", "true", "false", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_comptime_if_drops_the_untaken_branch_unparsed() {
    // The condition folds to a bool literal at parse time (`a:dyad.type == i32`), so
    // the `if` resolves during parsing and the untaken branch's tokens are
    // dropped unlexed: `a = 9.9` under `a := i32 ?` would be a parse error
    // (UncomputableLiteral) if it were ever parsed — the proof it was skipped
    // is that this runs at all. A comptime-false chain link falls through to
    // the branch whose condition holds.
    let (echoes, stderr) = repl(
        b"a := i32 ?\nif (a:dyad.type == i32) (a = 9) else (a = 9.9)\na\n\
          b := f64 ?\nif (b:dyad.type == i32) (b = 1) else if (b:dyad.type == f64) (b = 2.5) else (b = 3)\nb\n",
    );
    assert_eq!(echoes, ["9", "2.5"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn the_metatypefn_example_runs() {
    // The station #30 north-star, end to end: a `-> logos` fn computes the logos,
    // `a := metalogos(…) ?` declares with it, and a comptime `if` dispatches on
    // `a.logos`, skipping the untaken branches unparsed. The expected value
    // tracks the file's current argument (2 → f64 → the middle arm assigns 9.9;
    // the deep arm is pinned separately by the metalogos_arm fixture).
    let out = logos().args(["import", "examples/metatypefn.logos"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "9.9\n");
}

#[test]
fn the_metalogos_arm_fills_a_logos_variable() {
    // The deep arm the example reaches with argument 3: `a := metalogos(3) ?` is
    // `a := logos ?` — a logos variable — and the comptime chain's last arm fills it
    // with the logos i32, so the program's value IS a logos and prints `i32`.
    let out = logos().args(["import", "tests/fixtures/metalogos_arm.logos"]).output().unwrap();
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
        b"c := (sum := i32 0, for i in 0..10 (sum = sum + i), sum)\nc\nc\n\
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
fn a_line_starting_with_a_dash_is_source_not_a_flag() {
    // DESIGN ›The command line is Logos source‹: "Everything after `logos`
    // is one line of Logos code" and "There are no build or compile flags".
    // Prefix `-` is the negation identity there as everywhere else, so a
    // leading `-` reaches the parser (#90) and a would-be flag is an ordinary
    // parse error at its own position, never a usage message.
    // Both shapes a shell produces: the words unquoted, which the binary joins
    // back into one line, and the line as a single argument.
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

    // The two exact spellings are still flags, matched whole and not by prefix.
    for flag in ["--help", "-h"] {
        let out = logos().arg(flag).output().unwrap();
        assert!(out.status.success(), "{flag}");
        assert!(String::from_utf8_lossy(&out.stdout).contains("usage:"), "{flag}");
    }
}

#[test]
fn the_record_read_answers_scope_range_and_gate() {
    // DESIGN ›The dyad's read surface‹ (7–8 September 2026): `:` reads a
    // name's record — `dyad`, `scope`, `start`, `end`, `gate` — bypassing its
    // reading rule. `end` and `gate` are null here (alive; no gates in
    // v0.1.0), `scope` is the session scope, and a constructed node answers
    // `:scope` from its path (›Meta-navigation‹): the same scope.
    let (echoes, stderr) = repl(
        b"x := i32 5\nx:end\nx:gate\nx:scope\n(x + x):scope\ny := i32\ny:dyad.type == type\ny == i32\n",
    );
    assert_eq!(echoes.len(), 6, "stderr: {stderr}");
    assert_eq!(&echoes[..2], ["0", "0"], "end and gate are null");
    assert_ne!(echoes[2], "0", "x was declared somewhere");
    assert_eq!(echoes[2], echoes[3], "a constructed node's :scope is the scope open at the read");
    assert_eq!(&echoes[4..], ["true", "true"], "an alias is its own record over the same dyad");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_field_the_record_has_not_is_the_same_error_as_an_undeclared_dot_field() {
    // `a:type` is a checked error, "exactly as a `.` read the type does not
    // declare is" (DESIGN ›The dyad's read surface‹): the record has no such
    // field, and the read resolves the name against `record`'s own scope —
    // the same resolution a `.` read runs against a user record's scope, so
    // the two report the same way, whether the spelling exists elsewhere
    // (`type`, `scope`: out of scope) or nowhere (`nonexistent`: unknown).
    fn message(stderr: &str) -> String {
        stderr.lines().next().and_then(|l| l.split("error: ").nth(1)).unwrap_or("").to_string()
    }
    let (_e, via_record) = repl(b"x := i32 5\nx:type\n");
    let (_e, via_dot) = repl(b"p := type (instance (a := i32 ?))\nq := p(1)\nq.scope\n");
    assert_eq!(message(&via_record), message(&via_dot), "record: {via_record}\ndot: {via_dot}");
    assert!(via_record.contains("not in scope"), "stderr: {via_record}");
    let (_e, via_record) = repl(b"x := i32 5\nx:nonexistent\n");
    let (_e, via_dot) = repl(b"p := type (instance (a := i32 ?))\nq := p(1)\nq.nonexistent\n");
    assert_eq!(message(&via_record), message(&via_dot), "record: {via_record}\ndot: {via_dot}");
    assert!(via_record.contains("unknown name"), "stderr: {via_record}");
}

#[test]
fn the_dyad_view_is_spelled_with_the_record_read() {
    // `(dyad a)` as a second spelling of `a:dyad` is superseded (DESIGN, 8
    // September 2026): `dyad` is the cell type, inert on the tape.
    let (_echoes, stderr) = repl(b"x := i32 5\n(dyad x).type\n");
    assert!(!stderr.is_empty(), "the old spelling no longer parses");
    let (echoes, stderr) = repl(b"x := i32 5\nx:dyad.type == i32\n");
    assert_eq!(echoes, ["true"], "stderr: {stderr}");
}

#[test]
fn a_tight_read_runs_over_a_keyword_before_its_constructor_wakes() {
    // DESIGN ›Text is the quote‹ and the 8 September 2026 ruling: `:` and `.`
    // sit above the identities that read their own right side, so `if:scope`
    // reads the keyword's record and `if.constructor` its field; a keyword
    // used as a keyword still reads its right side.
    let (echoes, stderr) = repl(
        b"x := i32 5\nif:scope\ntype:dyad.type == type\nfn:dyad.type == type\n\
          f := fn () -> i32 ( if (x < 9) (x = 1) else (x = 2), x )\nf()\n",
    );
    assert_eq!(echoes.len(), 4, "stderr: {stderr}");
    assert_ne!(echoes[0], "0", "if was declared in the session scope");
    assert_eq!(&echoes[1..], ["true", "true", "1"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn assignment_returns_nothing() {
    // `=` beside `:=`, driving its right side, and returning nothing (ruled 8
    // September 2026): `a = b = c` assigns nothing to `a`, an error; an `=`
    // in a value position is the statement-as-value error.
    let (_e, stderr) = repl(b"a := i32 1\nb := i32 2\na = b = 3\n");
    assert!(stderr.contains("yields no value"), "stderr: {stderr}");
    let (_e, stderr) = repl(b"a := i32 1\ny := (a = 2) + 1\n");
    assert!(!stderr.is_empty(), "stderr: {stderr}");
    let (echoes, stderr) = repl(
        b"p := type (instance (v := i32 ?))\nq := p(1)\nq.v = 3\nq.v\na := i32 1\na = a + 1\na\n",
    );
    assert_eq!(echoes, ["3", "2"], "stderr: {stderr}");
}

#[test]
fn a_tight_read_lexes_its_right_cell_on_demand_and_stops_at_a_boundary() {
    // DESIGN ›The scope's constructor is the driver‹ (8-9 September 2026):
    // `:`, `.`, and `@` construct at discovery, their right cell lexed lazily
    // by the `tape[1]` read inside the constructor; a lazy read never crosses
    // a `,` or a closer, and an inner `@` constructs before the outer one.
    let (echoes, stderr) = repl(
        b"x := i32 5\n(x:end, 3)\np := type (instance (a := i32 ?))\nq := p(1)\n(q.a, 2)\nq.a\n\
          r := &q\nr@.a\npp := &r\npp@@.a\nx:dyad.type == i32\n",
    );
    assert_eq!(echoes, ["3", "2", "1", "1", "1", "true"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_dyad_is_built_from_a_type_and_a_value() {
    // DESIGN ›Feasibility‹: "`dyad (type, value)` construction from Logos"
    // (#60). `dyad (i32, 7)` is a store-owned i32 cell, `dyad` alone the
    // cell type; the view reads the built cell's two fields.
    let (echoes, stderr) =
        repl(b"c := dyad (i32, 7)\nc\nc:dyad.type == i32\ndyad (i32, 7):dyad.type == i32\nc + 1\n");
    assert_eq!(echoes, ["7", "true", "true", "8"], "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
    let (_e, stderr) = repl(b"dyad (i32)\n");
    assert!(!stderr.is_empty(), "two operands, not one");

    // A bool cell carries the literal's own 0/1 byte. It used to carry the
    // *node* as its value, so every bool dyad read true because a node address
    // is nonzero — `dyad (bool, false)` included (#85).
    let (echoes, stderr) = repl(b"dyad (bool, true)\ndyad (bool, false)\n");
    assert_eq!(echoes, ["true", "false"], "stderr: {stderr}");

    // Every other type reads its value as bytes at its own width, and handing
    // it a node was type confusion: `dyad (@i32, 5)` read the rational node's
    // bytes as an i32 and a write through it corrupted the store. Nothing
    // builds such a cell, so it is refused rather than guessed.
    for src in [
        &b"dyad (bool, 0)\n"[..],
        b"p := dyad (@i32, 5)\n",
        b"dyad (void, 0)\n",
        b"g := type (instance (x := i32 ?))\ndyad (g, 5)\n",
    ] {
        let (echoes, stderr) = repl(src);
        assert!(
            echoes.is_empty() && !stderr.is_empty(),
            "{}: stderr: {stderr}",
            String::from_utf8_lossy(src)
        );
    }
}
