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
use seed::parse::{Parser, ScopeStack};
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

/// Run a file: parse its whole body as one sequence, evaluate it, print its
/// value. Parse errors render with file:line:col and a caret; run errors are
/// message-only (nodes carry no source positions yet).
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
    let (root, end) = {
        let mut p = Parser::new(
            &source,
            &mut engine.store,
            &mut engine.trie,
            &engine.core.metas,
            types,
            scopes,
        );
        match p.parse_sequence() {
            Ok(root) => (root, p.offset()),
            Err(e) => {
                eprintln!("{}", report::render(path, &source, p.offset(), &report::parse_message(&e)));
                return ExitCode::FAILURE;
            }
        }
    };
    // A stray `)` breaks the sequence loop without being consumed.
    if !source[end..].trim_start().is_empty() {
        eprintln!(
            "{}",
            report::render(path, &source, end, "unexpected `)` — no scope is open here")
        );
        return ExitCode::FAILURE;
    }

    let mut rt = Runtime::new(engine.core.fn_type, engine.core.rational, engine.core.struct_);
    // SAFETY: `root` and everything it reaches were just parsed into the store,
    // which lives for the rest of this function.
    match unsafe { rt.run(root) } {
        Ok(bits) => {
            println!("{bits}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{path}: run error: {}", report::run_message(&e));
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
            let mut p = Parser::new(
                &line,
                &mut engine.store,
                &mut engine.trie,
                &engine.core.metas,
                types,
                scopes,
            );
            let parsed = p.parse_expression();
            let end = p.offset();
            (parsed, end, p.into_scopes())
        };
        scopes = scopes_back;

        let node = match parsed {
            Ok(node) => node,
            Err(e) => {
                eprintln!(
                    "{}",
                    report::render("<repl>", &line, end, &report::parse_message(&e))
                );
                continue;
            }
        };
        if !line[end..].trim_start().is_empty() {
            eprintln!(
                "{}",
                report::render("<repl>", &line, end, "one expression per line in the REPL")
            );
            continue;
        }

        // Echo policy (settled): statements are silent — only value
        // expressions echo, like Python. The graph says which is which: a
        // declaration is a declare node, an assignment an assign/storeptr
        // node, a bare fn or struct literal a declaration statement.
        // SAFETY: `node` is the valid dyad just parsed.
        let is_statement = unsafe {
            let ty = (*node).ty;
            ty == engine.core.declare_
                || ty == engine.core.assign
                || ty == engine.core.storeptr_
                || ty == engine.core.fn_type
                || ty == engine.core.struct_
        };

        let mut rt =
            Runtime::new(engine.core.fn_type, engine.core.rational, engine.core.struct_);
        // SAFETY: `node` and everything it reaches live in the engine's store,
        // which outlives the loop. Statements still run — for their effect —
        // they just do not echo.
        match unsafe { rt.run(node) } {
            Ok(bits) if !is_statement => println!("{bits}"),
            Ok(_) => {}
            Err(e) => eprintln!("run error: {}", report::run_message(&e)),
        }
    }
}
