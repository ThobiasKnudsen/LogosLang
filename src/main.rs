// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `logos`: the command. Everything after `logos` is one line of Logos source
//! run by the one pass; bare `logos` starts the REPL. No subcommands and no
//! compile flags: what to compile is decided inside Logos source.
//! DESIGN ›The command line is Logos source‹.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use seed::identities::Core;
use seed::parse::{Imports, ParseError, Parser, ScopeStack};
use seed::regex_trie::RegexTrie;
use seed::report;
use seed::run::Runtime;
use seed::store::Store;

/// One per process; the REPL reuses it across lines.
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

/// The echo rule: a declaration, an assignment, a bare fn or type definition,
/// a compile, or an import is a statement and stays silent; everything else echoes.
///
/// # Safety
/// `node` must be a valid dyad.
unsafe fn is_statement_node(core: &Core, node: seed::dyad::DyadPtr) -> bool {
    let (named, logos) = tail_type(core, node);
    // A bare type definition is a statement; a place holding a type is a value and echoes.
    if logos == core.type_ {
        return seed::identities::read::read_kind(core, named)
            == seed::identities::read::Read::Identity;
    }
    is_silent_type(core, logos) || logos == core.fn_type || logos == core.drop_
}

/// The dyad a line's tail names (through its ran item and its record) and its type.
///
/// # Safety
/// `node` must be a valid dyad.
unsafe fn tail_type(
    core: &Core,
    node: seed::dyad::DyadPtr,
) -> (seed::dyad::DyadPtr, seed::dyad::DyadPtr) {
    let node = seed::identities::ran::expr_of(core, node);
    let named = seed::record::through(core.record_, node);
    (named, (*named).ty)
}

/// The statement types with no value worth printing in any mode.
fn is_silent_type(core: &Core, logos: seed::dyad::DyadPtr) -> bool {
    logos == core.declare_
        || logos == core.assign
        || logos == core.storeptr_
        || logos == core.compile_
        || logos == core.import_
}

/// Whether an imported file's tail prints nothing. Narrower than the REPL's
/// rule: a bare type tail still prints its spelling.
///
/// # Safety
/// `node` must be a valid dyad.
unsafe fn is_silent_tail(core: &Core, node: seed::dyad::DyadPtr) -> bool {
    is_silent_type(core, tail_type(core, node).1)
}

fn main() -> ExitCode {
    // A thread of the seed's stack size, so the depth guards decide, not the build profile.
    std::thread::Builder::new()
        .stack_size(seed::WORK_STACK_BYTES)
        .spawn(work)
        .expect("the work thread must start")
        .join()
        .unwrap_or(ExitCode::FAILURE)
}

fn work() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => repl(),
        [flag] if flag == "--help" || flag == "-h" => {
            print!("{}", help());
            ExitCode::SUCCESS
        }
        // No arm rejects a leading `-`: `logos '-5 + 3'` is Logos source; only
        // an exact `--help` or `-h` is a flag.
        line => run_line(&line.join(" ")),
    }
}

/// Versioned, so the release archives self-identify.
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

/// Run the command line as one line of Logos source; its tail value prints at
/// the end (an import's: the imported file's tail). Parse errors render with a
/// caret; run errors are message-only, nodes carrying no positions.
fn run_line(source: &str) -> ExitCode {
    let path = "<command line>";
    let mut engine = Engine::new();
    let mut scopes = ScopeStack::new();
    scopes.push(engine.core.root_scope);
    // The command line is its own section: its declarations sit above the
    // root, out of an imported file's view.
    scopes.push_section(&mut engine.store, engine.core.scope);

    let types = &engine.core;
    // The compiler rides along so `f.compile()` works in the one pass.
    let mut p = Parser::new(source, &mut engine.store, &mut engine.trie, types, scopes)
        .with_lower(&engine.core.lower);

    // The tail is the last non-comment item: prose never becomes the line's value.
    let mut last = None;
    let mut ran_something = false;
    while let Some(item) = p.parse_next() {
        let node = match item {
            Ok(node) => {
                // SAFETY: `node` is the line `parse_next` just returned.
                unsafe { p.close_item(node) };
                node
            }
            Err(ParseError::Run(e)) => {
                eprintln!("{path}: run error: {}", report::run_message(&e));
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    report::render(path, source, p.offset(), &report::parse_message(&e))
                );
                return ExitCode::FAILURE;
            }
        };
        // SAFETY: `node` is the valid dyad just parsed.
        unsafe {
            if (*node).ty == engine.core.import_ {
                ran_something = true;
                let tail = seed::identities::import::tail_of(node);
                if !tail.is_null() && !is_silent_tail(&engine.core, tail) {
                    last = Some(tail);
                }
            } else if (*node).ty != types.comment_ {
                ran_something = true;
                // The same silence test an import's tail takes, so the command
                // line and the REPL agree.
                if !is_silent_tail(&engine.core, node) {
                    last = Some(node);
                }
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
    // The tail is read after the root's own run and before the teardowns,
    // which may free what it points at.
    if let Err(e) = p.finish() {
        eprintln!("{path}: {}", report::parse_message(&e));
        return ExitCode::FAILURE;
    }
    let last = match last {
        // SAFETY: `node` is a valid dyad the parser built.
        Some(node) => match unsafe { p.value_of(node) } {
            Ok(bits) => Some((node, bits)),
            Err(e) => {
                eprintln!("{path}: {}", report::parse_message(&e));
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };

    // The top level's scope exit: its owning bindings' teardowns run LIFO here.
    if let Err(e) = p.exit() {
        eprintln!("{path}: {}", report::parse_message(&e));
        return ExitCode::FAILURE;
    }

    match last {
        Some((node, bits)) => {
            // SAFETY: `node` is the parsed dyad whose value `bits` is.
            println!("{}", unsafe { seed::identities::display_value(types, node, bits) });
            ExitCode::SUCCESS
        }
        // Real work with nothing to show exits clean; a genuinely empty line is an error.
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

/// One persistent store, index and scope; one expression per line, each value
/// echoed; an error reports and the loop continues.
fn repl() -> ExitCode {
    println!("logos {} — one expression per line, ctrl-d to exit", env!("CARGO_PKG_VERSION"));
    let mut engine = Engine::new();
    let mut scopes = ScopeStack::new();
    scopes.push(engine.core.root_scope);
    // The session is its own section, like the command line.
    scopes.push_section(&mut engine.store, engine.core.scope);

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    // A REPL binding lives for the session, so its teardowns run at session
    // exit; each line's parser is fresh, so they collect here.
    let mut session_defers: Vec<seed::dyad::DyadPtr> = Vec::new();
    // Once-per-run imports must hold across lines: one registry threads through
    // each line's fresh parser.
    let mut imports = Imports::default();
    loop {
        print!("» ");
        let _ = std::io::stdout().flush();
        let line = match lines.next() {
            Some(Ok(line)) => line,
            _ => {
                println!();
                let mut rt = Runtime::new(&engine.core, &mut engine.store);
                for defer_node in session_defers.into_iter().rev() {
                    // SAFETY: each is a `defer` node in the engine's store, still alive here.
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

        let types = &engine.core;
        // The value is read while the parser is alive, and only for a line that parsed whole.
        let (parsed, end, value, line_defers, scopes_back, imports_back) = {
            let mut p = Parser::new(&line, &mut engine.store, &mut engine.trie, types, scopes)
                .with_imports(std::mem::take(&mut imports))
                .with_lower(&engine.core.lower);
            let parsed = p.parse_expression();
            let end = p.offset();
            let value = match parsed {
                Ok(node) if line[end..].trim_start().is_empty() => {
                    // SAFETY: `node` was just parsed into the engine's store.
                    Some(unsafe { p.value_of(node) })
                }
                _ => None,
            };
            let line_defers = p.take_pending_defers();
            let imports_back = p.take_imports();
            (parsed, end, value, line_defers, p.into_scopes(), imports_back)
        };
        scopes = scopes_back;
        imports = imports_back;

        // A failed line must leave no trace, or a typo would burn its name for
        // the session under the no-shadowing rule.
        let fail = |scopes: &mut ScopeStack, trie: &mut RegexTrie| {
            scopes.rollback(trie);
            scopes.truncate(2); // the root and the session's own section
        };

        let node = match parsed {
            Ok(node) => node,
            Err(e) => {
                eprintln!("{}", report::render("<repl>", &line, end, &report::parse_message(&e)));
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

        // Echo rule: statements are silent, value expressions echo. An import
        // echoes through its file's tail, so a declaration-tailed import stays silent.
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

        // Kept even for a binding that never ran: its teardown sees a null
        // place and no-ops, the fail-closed side.
        session_defers.extend(line_defers);

        match value {
            Some(Ok(bits)) if !is_statement => {
                // SAFETY: `display_node` is a valid dyad whose value `bits` is.
                println!("{}", unsafe {
                    seed::identities::display_value(types, display_node, bits)
                })
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => {
                eprintln!("{}", report::parse_message(&e));
                fail(&mut scopes, &mut engine.trie);
                continue;
            }
            // Unreachable: every line that parsed whole had its value read above.
            None => {}
        }
        // SAFETY: `node` is the line just parsed; its pending records are this line's declares.
        unsafe { scopes.close_item(&mut engine.store, engine.core.array_, node) };
        scopes.commit();
    }
}
