// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `logos`: the command. Everything after `logos` is one line of Logos
//! source, run by the one pass (#58, DESIGN ›The command line is Logos
//! source‹): `logos import ./file.logos` imports a file — it runs top to
//! bottom, and the importer reaches only its `pub` names — and
//! `logos 'import ./file.logos, main(«a»)'` calls one. The line's tail value
//! prints (an import's value is its file's tail). Bare `logos` starts the
//! interactive REPL: one persistent scope, one expression per line, each
//! value echoed. The superseded `logos file.logos` spelling is gone.
//!
//! Deliberately this small (settled, July 2026): no subcommands and no compile
//! flags, ever — the build system, linking, and what-to-compile decisions live
//! *inside* Logos source, not in this binary. The CLI's whole job is handing
//! source to the interpreter. Printing the tail value stands in for output
//! until FFI (#45) gives programs real effects.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use seed::identities::Core;
use seed::parse::{Imports, ParseError, Parser, ScopeStack};
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

/// Whether `node` is a statement for the REPL's echo policy — a declaration,
/// an assignment, a bare fn/logos, a compile, or an import: silent.
///
/// # Safety
/// `node` must be a valid dyad.
unsafe fn is_statement_node(core: &Core, node: seed::dyad::DyadPtr) -> bool {
    let logos = (*node).ty;
    logos == core.declare_
        || logos == core.assign
        || logos == core.storeptr_
        || logos == core.fn_type
        || logos == core.compile_
        || logos == core.import_
        || logos == core.type_
}

/// Whether an imported file's tail is a true statement, with no value worth
/// printing as the line's tail. Narrower than the REPL's echo policy: a bare
/// logos tail keeps printing its spelling, exactly as the retired file mode
/// printed it.
///
/// # Safety
/// `node` must be a valid dyad.
unsafe fn is_silent_tail(core: &Core, node: seed::dyad::DyadPtr) -> bool {
    let logos = (*node).ty;
    logos == core.declare_
        || logos == core.assign
        || logos == core.storeptr_
        || logos == core.compile_
        || logos == core.import_
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => repl(),
        [flag] if flag == "--help" || flag == "-h" => {
            print!("{}", help());
            ExitCode::SUCCESS
        }
        [first, ..] if first.starts_with('-') => {
            eprintln!("usage: logos [code…]   (or `logos --help`)");
            ExitCode::from(2)
        }
        line => run_line(&line.join(" ")),
    }
}

/// The `--help` text, versioned so the release archives self-identify.
fn help() -> String {
    format!(
        "logos {} — the Logos language\n\n\
         usage:\n\
         \x20 logos <code…>        run the arguments as one line of Logos source\n\
         \x20 logos                start the interactive REPL\n\
         \x20 logos --help         show this help\n\n\
         Everything after `logos` is Logos source, run by the one pass:\n\
         \x20 logos import ./file.logos\n\
         \x20 logos 'import ./file.logos, main(«hello»)'\n\
         `import` loads a file: it runs top to bottom, and the importer\n\
         reaches only its `pub` names. The line's tail value is printed.\n\
         There are no build or compile flags: the code itself decides what\n\
         compiles.\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// Run the command line as one line of Logos source: one pass — each
/// expression runs the moment it is parsed (DESIGN ›Build and run are one
/// self-directing pass‹), so everything the parser itself evaluates (an
/// `import`, a `-> logos` call reading an earlier binding) sees committed
/// state, and the command line and REPL agree. The line's tail value prints
/// at the end — for an import node, the imported file's own tail. Parse
/// errors render with line:col and a caret; run errors are message-only
/// (nodes carry no source positions yet).
fn run_line(source: &str) -> ExitCode {
    let path = "<command line>";
    let mut engine = Engine::new();
    let mut scopes = ScopeStack::new();
    scopes.push(engine.core.root_scope);
    // The command line is its own section (ruled August 2026): its
    // declarations land in a scope above the root, so an imported file's
    // fresh view — the root plus the file's own section — cannot see them.
    let user_section = engine.store.alloc_raw(engine.core.scope_, std::ptr::null_mut());
    scopes.push(user_section);

    let types = engine.core.types();
    // The compiler rides along so `f.compile()` works in the one pass; the
    // engine (core + store) outlives the runtime, per `with_compiler`'s
    // contract.
    let mut rt = Runtime::new(engine.core.fn_type, engine.core.rational)
        .with_compiler(&engine.core.lower, types)
        .with_defer_type(engine.core.defer_);
    let mut p = Parser::new(source, &mut engine.store, &mut engine.trie, types, scopes)
        .with_lower(&engine.core.lower);

    // The tail: the last non-comment expression and its value, printed at the
    // end (prose is invisible to value flow, so a trailing comment never
    // becomes the line's value). An import node's value displays through the
    // imported file's own tail; a declaration-only import counts as ran work
    // with nothing to print.
    let mut last = None;
    let mut ran_something = false;
    while let Some(item) = p.parse_next() {
        let node = match item {
            Ok(node) => node,
            Err(e) => {
                eprintln!(
                    "{}",
                    report::render(path, source, p.offset(), &report::parse_message(&e))
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
                unsafe {
                    if (*node).ty == engine.core.import_ {
                        ran_something = true;
                        // A value tail is the import's value; a statement tail
                        // (a library ending in a declaration) shows nothing.
                        let tail = seed::identities::import::tail_of(node);
                        if !tail.is_null() && !is_silent_tail(&engine.core, tail) {
                            last = Some((tail, bits));
                        }
                    } else if (*node).ty != types.comment_ {
                        ran_something = true;
                        last = Some((node, bits));
                    }
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
            report::render(path, source, end, "unexpected `)` — no scope is open here")
        );
        return ExitCode::FAILURE;
    }

    // The top level's own scope exit (issue #49): run the teardowns top-level
    // owning bindings inserted, LIFO, at program end. A nested scope ran its own
    // at its exit; these are the file's, freeing what top-level `alloc`s owned.
    for defer_node in p.take_pending_defers().into_iter().rev() {
        // SAFETY: `defer_node` is a `defer` node just built into the store; the
        // runtime works off raw handles into the store, which is still alive.
        if let Err(e) = unsafe { seed::identities::run_deferred(&mut rt, defer_node) } {
            eprintln!("{path}: run error: {}", report::run_message(&e));
            return ExitCode::FAILURE;
        }
    }

    match last {
        Some((node, bits)) => {
            // SAFETY: `node` is the parsed dyad whose value `bits` is.
            println!("{}", unsafe { seed::identities::display_value(&types, node, bits) });
            ExitCode::SUCCESS
        }
        // A line that did real work with no value to show (a declaration-only
        // import) exits clean and silent; a genuinely empty line is an error.
        None if ran_something => ExitCode::SUCCESS,
        None => {
            eprintln!(
                "{}",
                report::render(path, source, end, &report::parse_message(&ParseError::Empty))
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
    // The session is its own section, like the command line (ruled August
    // 2026): its declarations land above the root, invisible to imported files.
    let user_section = engine.store.alloc_raw(engine.core.scope_, std::ptr::null_mut());
    scopes.push(user_section);

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    // The session's own teardowns (issue #49): a REPL binding lives for the whole
    // session, so its `defer free` belongs at session exit — the REPL's top-level
    // scope exit, the same drain the file driver runs at program end (file mode
    // and the REPL are one pass and must agree). Each line's parser is fresh, so
    // its pending teardowns are collected here as the line is accepted.
    let mut session_defers: Vec<seed::dyad::DyadPtr> = Vec::new();
    // A session is one run, so once-per-run imports must hold across lines:
    // the one registry threads through each line's fresh parser.
    let mut imports = Imports::default();
    loop {
        print!("» ");
        let _ = std::io::stdout().flush();
        let line = match lines.next() {
            Some(Ok(line)) => line,
            _ => {
                println!();
                // Session exit: run the accumulated teardowns, newest first.
                let mut rt = Runtime::new(engine.core.fn_type, engine.core.rational)
                    .with_defer_type(engine.core.defer_);
                for defer_node in session_defers.into_iter().rev() {
                    // SAFETY: each is a `defer` node in the engine's store, which
                    // is still alive here.
                    if let Err(e) = unsafe { seed::identities::run_deferred(&mut rt, defer_node) } {
                        eprintln!("<repl>: run error: {}", report::run_message(&e));
                        return ExitCode::FAILURE;
                    }
                }
                return ExitCode::SUCCESS;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let types = engine.core.types();
        let (parsed, end, line_defers, scopes_back, imports_back) = {
            let mut p = Parser::new(&line, &mut engine.store, &mut engine.trie, types, scopes)
                .with_imports(std::mem::take(&mut imports))
                .with_lower(&engine.core.lower);
            let parsed = p.parse_expression();
            let end = p.offset();
            // Teardowns this line's bindings inserted; kept only if the line is
            // accepted, so a failed line leaves no trace here either.
            let line_defers = p.take_pending_defers();
            let imports_back = p.take_imports();
            (parsed, end, line_defers, p.into_scopes(), imports_back)
        };
        scopes = scopes_back;
        imports = imports_back;

        // A failed line must leave no trace: roll its declarations back out of
        // the name index and close any scopes an error left open, or a typo
        // would burn its name for the rest of the session ("shadowed" forever
        // under the no-shadowing rule).
        let fail = |scopes: &mut ScopeStack, trie: &mut RegexTrie| {
            scopes.rollback(trie);
            scopes.truncate(2); // the root and the session's own section
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
        // node, a bare fn, record, or logos a declaration statement. An import
        // echoes through the imported file's tail (its value), so a
        // declaration-tailed or declaration-only import stays silent.
        // SAFETY: `node` is the valid dyad just parsed.
        let display_node = unsafe {
            if (*node).ty == engine.core.import_ {
                let tail = seed::identities::import::tail_of(node);
                if tail.is_null() {
                    node
                } else {
                    tail
                }
            } else {
                node
            }
        };
        // SAFETY: `display_node` is a valid dyad (the node or its import tail).
        let is_statement = unsafe { is_statement_node(&engine.core, display_node) };

        // The compiler rides along so `f.compile()` works across lines: the
        // installed bcode lives in the engine's store and the compiled
        // artifact is process-lived, so a fresh per-line runtime is fine.
        // The line is accepted, so its bindings' teardowns join the session's,
        // to run at exit. A teardown over a place whose binding never ran sees a
        // null place and no-ops, so keeping them is the fail-closed side.
        session_defers.extend(line_defers);

        let mut rt =
            Runtime::new(engine.core.fn_type, engine.core.rational)
                .with_compiler(&engine.core.lower, types)
                .with_defer_type(engine.core.defer_);
        // SAFETY: `node` and everything it reaches live in the engine's store,
        // which outlives the loop. Statements still run — for their effect —
        // they just do not echo.
        match unsafe { rt.run(node) } {
            // SAFETY: `display_node` is a valid dyad whose value `bits` is
            // (an import's run yields its tail's bits).
            Ok(bits) if !is_statement => {
                println!("{}", unsafe {
                    seed::identities::display_value(&types, display_node, bits)
                })
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
