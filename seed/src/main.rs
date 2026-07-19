// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `logos`: the command. `logos file.logos` evaluates the file's top-level
//! scope in order, Python-style — declarations are inert at run time, there is
//! no main function to find, and the file's value (its tail expression) is
//! printed. Bare `logos` starts the interactive REPL: one persistent scope, one
//! expression per line, each value echoed.
//!
//! Deliberately this small (settled, July 2026): no subcommands and no compile
//! flags, ever — the build system, linking, and what-to-compile decisions live
//! *inside* Logos files, not in this binary. The CLI's whole job is handing
//! source to the interpreter. Printing the tail value stands in for output
//! until FFI (#45) gives programs real effects.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use seed::identities::Core;
use seed::parse::{ParseError, Parser, ScopeStack};
use seed::regex_trie::RegexTrie;
use seed::report;
use seed::run::Runtime;
use seed::store::Store;

/// The engine a run needs: the store and name index the graph lives in, plus
/// the core identities. One per process; the REPL reuses it across lines.
struct Engine {
    store: Store,
    trie: RegexTrie,
    core: Core,
}

impl Engine {
    fn new() -> Engine {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        Engine { store, trie, core }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => repl(),
        [flag] if flag == "--help" || flag == "-h" => {
            print!("{}", help());
            ExitCode::SUCCESS
        }
        [path] if !path.starts_with('-') => run_file(path),
        _ => {
            eprintln!("usage: logos [file.logos]   (or `logos --help`)");
            ExitCode::from(2)
        }
    }
}

/// The `--help` text, versioned so the release archives self-identify.
fn help() -> String {
    format!(
        "logos {} — the Logos language\n\n\
         usage:\n\
         \x20 logos <file.logos>   run a file top to bottom (like a script; no main)\n\
         \x20 logos                start the interactive REPL\n\
         \x20 logos --help         show this help\n\n\
         The file's top-level scope evaluates in order; declarations are inert\n\
         at run time, and the file's value (its tail expression) is printed.\n\
         There are no build or compile flags: the code itself decides what\n\
         compiles.\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// Run a file: one pass — each top-level expression runs the moment it is
/// parsed (DESIGN ›Build and run are one self-directing pass‹), so everything
/// the parser itself evaluates (a `-> type` call reading an earlier binding)
/// sees committed state, and file and REPL agree. The file's value is its tail
/// expression's, printed at the end. Parse errors render with file:line:col and
/// a caret; run errors are message-only (nodes carry no source positions yet).
fn run_file(path: &str) -> ExitCode {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("logos: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut engine = Engine::new();
    let mut scopes = ScopeStack::new();
    scopes.push(engine.core.root_scope);

    let types = engine.core.types();
    // The compiler rides along so `f.compile()` works in the one pass; the
    // engine (core + store) outlives the runtime, per `with_compiler`'s
    // contract.
    let mut rt = Runtime::new(engine.core.fn_type, engine.core.rational, engine.core.struct_)
        .with_compiler(&engine.core.lower, types);
    let mut p = Parser::new(&source, &mut engine.store, &mut engine.trie, types, scopes);

    // The tail: the last non-comment expression and its value, printed at the
    // end (prose is invisible to value flow, so a trailing comment never
    // becomes the file's value).
    let mut last = None;
    while let Some(item) = p.parse_next() {
        let node = match item {
            Ok(node) => node,
            Err(e) => {
                eprintln!(
                    "{}",
                    report::render(path, &source, p.offset(), &report::parse_message(&e))
                );
                return ExitCode::FAILURE;
            }
        };
        // SAFETY: `node` and everything it reaches were just parsed into the
        // store, which lives for the rest of this function. The runtime works
        // off raw handles, so running interleaves with the open parse.
        match unsafe { rt.run(node) } {
            Ok(bits) => {
                // SAFETY: `node` is the valid dyad just parsed.
                if unsafe { (*node).ty != types.comment_ } {
                    last = Some((node, bits));
                }
            }
            Err(e) => {
                eprintln!("{path}: run error: {}", report::run_message(&e));
                return ExitCode::FAILURE;
            }
        }
    }
    // A stray `)` ends the item loop without being consumed.
    let end = p.offset();
    if !source[end..].trim_start().is_empty() {
        eprintln!(
            "{}",
            report::render(path, &source, end, "unexpected `)` — no scope is open here")
        );
        return ExitCode::FAILURE;
    }

    match last {
        Some((node, bits)) => {
            // SAFETY: `node` is the parsed dyad whose value `bits` is.
            println!("{}", unsafe { seed::identities::display_value(&types, node, bits) });
            ExitCode::SUCCESS
        }
        // An empty or prose-only file has no value, as before.
        None => {
            eprintln!(
                "{}",
                report::render(path, &source, end, &report::parse_message(&ParseError::Empty))
            );
            ExitCode::FAILURE
        }
    }
}

/// The REPL: one persistent store/name-index/scope, one expression per line,
/// each value echoed. Declarations on earlier lines stay resolvable; an error
/// reports and the loop continues.
fn repl() -> ExitCode {
    println!("logos {} — one expression per line, ctrl-d to exit", env!("CARGO_PKG_VERSION"));
    let mut engine = Engine::new();
    let mut scopes = ScopeStack::new();
    scopes.push(engine.core.root_scope);

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        print!("» ");
        let _ = std::io::stdout().flush();
        let line = match lines.next() {
            Some(Ok(line)) => line,
            _ => {
                println!();
                return ExitCode::SUCCESS;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let types = engine.core.types();
        let (parsed, end, scopes_back) = {
            let mut p = Parser::new(&line, &mut engine.store, &mut engine.trie, types, scopes);
            let parsed = p.parse_expression();
            let end = p.offset();
            (parsed, end, p.into_scopes())
        };
        scopes = scopes_back;

        // A failed line must leave no trace: roll its declarations back out of
        // the name index and close any scopes an error left open, or a typo
        // would burn its name for the rest of the session ("shadowed" forever
        // under the no-shadowing rule).
        let fail = |scopes: &mut ScopeStack, trie: &mut RegexTrie| {
            scopes.rollback(trie);
            scopes.truncate(1);
        };

        let node = match parsed {
            Ok(node) => node,
            Err(e) => {
                eprintln!(
                    "{}",
                    report::render("<repl>", &line, end, &report::parse_message(&e))
                );
                fail(&mut scopes, &mut engine.trie);
                continue;
            }
        };
        if !line[end..].trim_start().is_empty() {
            eprintln!(
                "{}",
                report::render("<repl>", &line, end, "one expression per line in the REPL")
            );
            fail(&mut scopes, &mut engine.trie);
            continue;
        }

        // Echo policy (settled): statements are silent — only value
        // expressions echo, like Python. The graph says which is which: a
        // declaration is a declare node, an assignment an assign/storeptr
        // node, a bare fn, struct, or type a declaration statement.
        // SAFETY: `node` is the valid dyad just parsed.
        let is_statement = unsafe {
            let ty = (*node).ty;
            ty == engine.core.declare_
                || ty == engine.core.assign
                || ty == engine.core.storeptr_
                || ty == engine.core.fn_type
                || ty == engine.core.struct_
                || ty == engine.core.compile_
                || ty == types.type_
        };

        // The compiler rides along so `f.compile()` works across lines: the
        // installed bcode lives in the engine's store and the compiled
        // artifact is process-lived, so a fresh per-line runtime is fine.
        let mut rt =
            Runtime::new(engine.core.fn_type, engine.core.rational, engine.core.struct_)
                .with_compiler(&engine.core.lower, types);
        // SAFETY: `node` and everything it reaches live in the engine's store,
        // which outlives the loop. Statements still run — for their effect —
        // they just do not echo.
        match unsafe { rt.run(node) } {
            // SAFETY: `node` is the valid dyad just parsed, whose value `bits` is.
            Ok(bits) if !is_statement => {
                println!("{}", unsafe { seed::identities::display_value(&types, node, bits) })
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("run error: {}", report::run_message(&e));
                fail(&mut scopes, &mut engine.trie);
                continue;
            }
        }
        scopes.commit();
    }
}
