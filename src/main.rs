// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `logos`: the command. Everything after `logos` is one line of Logos source
//! run by the one pass; bare `logos` starts the REPL. No subcommands and no
//! compile flags: what to compile is decided inside Logos source.
//! DESIGN ›The command line is Logos source‹.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use seed::identities::Core;
use seed::parse::{Imports, LinesEnd, ParseError, Parser, ScopeStack};
use seed::regex_trie::RegexTrie;
use seed::report;
use seed::run::Runtime;
use seed::store::{Store, StoreStats};

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

/// The REPL's echo rule: a line that gives nothing, a bare fn or a type definition stays
/// silent; everything else echoes. Wider than the command line's: a bare type tail prints
/// its spelling there.
///
/// # Safety
/// `node` must be a valid dyad the REPL's [`seed::identities::read::gives_value`] passed.
unsafe fn is_statement_node(core: &Core, node: seed::dyad::DyadPtr) -> bool {
    let named = core.through(node);
    let logos = core.type_of(named);
    // A bare type definition is a statement; a place holding a type is a value and echoes.
    if logos == core.type_ {
        return seed::identities::read::read_kind(core, named)
            == seed::identities::read::Read::Identity;
    }
    !matches!(seed::identities::read::gives_value(core, node), Ok(true)) || logos == core.fn_type
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
/// the end. Parse errors render with a
/// caret; run errors are message-only, nodes carrying no positions.
fn run_line(source: &str) -> ExitCode {
    let path = "<command line>";
    let mut engine = Engine::new();
    // An environment variable, since the command line takes no flags.
    let core_stats = std::env::var_os("LOGOS_STATS")
        .is_some_and(|v| !v.is_empty())
        .then(|| engine.store.stats());
    let mut scopes = ScopeStack::new();
    scopes.push(engine.core.root_scope);
    // The command line is its own section: its declarations sit above the
    // root, out of an imported file's view.
    scopes.push_section(&mut engine.store, engine.core.scope);

    let types = &engine.core;
    // The compiler rides along so `f.compile()` works in the one pass.
    let mut p = Parser::new(source, &mut engine.store, &mut engine.trie, types, scopes)
        .with_lower(&engine.core.lower);

    let last = match run_program_line(&mut p, types, path, source) {
        Ok(last) => last,
        // What the lines that ran hold and defer ends before the error shows.
        Err(shown) => {
            p.exit_after_fault();
            eprintln!("{shown}");
            return ExitCode::FAILURE;
        }
    };

    // The program's end: what the top level holds and defers ends here, last first.
    if let Err(e) = p.exit() {
        eprintln!("{path}: {}", report::parse_message(&e));
        return ExitCode::FAILURE;
    }
    let imports = p.take_imports();
    drop(p);
    let mut sources: Vec<&str> = vec![source];
    sources.extend(imports.sources());
    if let Some(core) = core_stats {
        print_stats(&engine, core, engine.store.stats(), &sources);
    }

    if let Some((_, shown)) = last {
        println!("{shown}");
    }
    ExitCode::SUCCESS
}

/// A line of the program read whole and run by the one pass: its tail with the text it
/// shows, or none when the tail gives no value or there is none (DESIGN ›The command line is
/// Logos source; the binary stays out of the way‹). An error comes back rendered, what the
/// line holds left for the caller to end.
fn run_program_line(
    p: &mut Parser,
    types: &Core,
    path: &str,
    source: &str,
) -> Result<Option<(seed::dyad::DyadPtr, String)>, String> {
    let tail = match p.read_lines(LinesEnd::TextEnd) {
        Ok(tail) => tail,
        Err(ParseError::Run(e)) => {
            return Err(format!("{path}: run error: {}", report::run_message(&e)))
        }
        Err(e) => return Err(report::render(path, source, p.offset(), &report::parse_message(&e))),
    };
    // Asked before the drain runs the tail, as a use is asked where it is written.
    let tail = match tail {
        // SAFETY: the tail is a line the parser built.
        Some((cell, end)) => match unsafe { seed::identities::read::gives_value(types, cell.dyad) }
        {
            Ok(shown) => shown.then_some(cell.dyad),
            Err(e) => return Err(report::render(path, source, end, &report::parse_message(&e))),
        },
        None => None,
    };
    // The tail is read after the root's own run and before the teardowns, which may free
    // what it points at, and rendered before the pass's runtime goes: an unnamed result
    // lives in that runtime's scratch.
    let message = |e: ParseError| format!("{path}: {}", report::parse_message(&e));
    let ran = p.finish().map_err(message)?;
    let Some(node) = tail else { return Ok(None) };
    let bits = match ran {
        Some((last, bits)) if last == node => bits,
        // A tail the pass never ran, a `[…]` line, runs now.
        // SAFETY: `node` is a line the parser built.
        _ => unsafe { p.value_of(node) }.map_err(message)?,
    };
    // SAFETY: `node` is the parsed dyad whose value `bits` is.
    Ok(Some((node, unsafe { seed::identities::display_value(types, node, bits) })))
}

/// `LOGOS_STATS`: the store after the core, after the line, and their delta
/// against the source that was parsed (the command line and every loaded file).
fn print_stats(engine: &Engine, core: StoreStats, after: StoreStats, sources: &[&str]) {
    let delta = after.since(&core);
    let source_bytes: usize = sources.iter().map(|s| s.len()).sum();
    let code_bytes: usize = sources.iter().map(|s| code_bytes(engine, s)).sum();
    let per = |n: usize, d: usize| if d == 0 { 0.0 } else { n as f64 / d as f64 };
    eprintln!("store core:  {core}");
    eprintln!("store after: {after}");
    eprintln!("store delta: {delta}");
    eprintln!(
        "store ratio: source_bytes={source_bytes} code_bytes={code_bytes} \
         per_source_byte={:.2} per_code_byte={:.2} per_node={:.2}",
        per(delta.total_bytes(), source_bytes),
        per(delta.total_bytes(), code_bytes),
        per(delta.total_bytes(), delta.nodes),
    );
}

/// Bytes of code: every token the lexer reads but a comment, the whitespace between them
/// dropped. The program's own names lex as fresh spellings, which spans the same bytes.
fn code_bytes(engine: &Engine, text: &str) -> usize {
    let mut scopes = ScopeStack::new();
    scopes.push(engine.core.root_scope);
    let Ok(tape) = seed::parse::lex_fragment(&scopes, &engine.trie, text) else {
        return 0;
    };
    let cells = tape.cells();
    cells.iter().filter(|c| c.identity(&engine.core) != engine.core.hash_).map(|c| c.len).sum()
}

/// One persistent store, index and scope; each line a line of the program, its tail
/// echoed; a line that fails reports, leaves no trace, and the loop continues.
fn repl() -> ExitCode {
    println!(
        "logos {} — each line is a line of the program, ctrl-d to exit",
        env!("CARGO_PKG_VERSION")
    );
    let mut engine = Engine::new();
    let mut scopes = ScopeStack::new();
    scopes.push(engine.core.root_scope);
    // The session is its own section, like the command line.
    scopes.push_section(&mut engine.store, engine.core.scope);

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
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
                // The session is one scope, and its end runs what its kept lines hold and defer.
                let session = scopes.current().expect("the session's scope is open");
                let mut rt = Runtime::new(&engine.core, &mut engine.store);
                // SAFETY: `session` is the session's scope node, its places in the program frame.
                if let Err(e) = unsafe { seed::identities::run_scope_exit(&mut rt, session) } {
                    eprintln!("<repl>: run error: {}", report::run_message(&e));
                    return ExitCode::FAILURE;
                }
                return ExitCode::SUCCESS;
            }
        };
        let session = scopes.current().expect("the session's scope is open");
        // SAFETY: `session` is the session's scope node.
        let kept = unsafe { seed::identities::scope_lines(session) };
        let types = &engine.core;
        let (outcome, freed, scopes_back, imports_back) = {
            let mut p = Parser::new(&line, &mut engine.store, &mut engine.trie, types, scopes)
                .with_imports(std::mem::take(&mut imports))
                .with_lower(&engine.core.lower);
            let outcome = run_program_line(&mut p, types, "<repl>", &line);
            let freed = match outcome {
                // A kept line is a line of the session: what it holds and defers joins the
                // session's end.
                Ok(_) => {
                    p.hand_exit();
                    Vec::new()
                }
                // A failed line keeps the ends it reached of an earlier line's names, and what
                // its own lines hold ends as at a fault (DESIGN ›A checked error is a fault: the
                // task that hit it is cancelled‹).
                Err(_) => {
                    let freed = p.end_failed_earlier_holds();
                    p.exit_after_fault();
                    freed
                }
            };
            let imports_back = p.take_imports();
            (outcome, freed, p.into_scopes(), imports_back)
        };
        scopes = scopes_back;
        imports = imports_back;

        match outcome {
            Ok(last) => {
                if let Some((node, shown)) = last {
                    // SAFETY: `node` is the line's tail, a dyad the parser built.
                    if !unsafe { is_statement_node(&engine.core, node) } {
                        println!("{shown}");
                    }
                }
                scopes.commit();
            }
            // A failed line leaves no trace, or a typo would burn its name for the session
            // under the no-shadowing rule.
            Err(shown) => {
                eprintln!("{shown}");
                scopes.rollback(&mut engine.trie, &freed);
                // SAFETY: as above.
                unsafe { seed::identities::keep_scope_lines(session, kept) };
                scopes.truncate(2); // the root and the session's own section
            }
        }
    }
}
