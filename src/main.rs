// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `logos`: the command. Everything after `logos` is one line of Logos source
//! run by the one pass; bare `logos` starts the REPL. No subcommands and no
//! compile flags: what to compile is decided inside Logos source.
//! DESIGN ›The command line is Logos source‹.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use seed::identities::dyad::DyadPtr;
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

fn main() -> ExitCode {
    seed::stack::on_work_stack(std::thread::Builder::new(), work).unwrap_or(ExitCode::FAILURE)
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
         \x20 logos import file «./file.logos»\n\
         \x20 logos 'import file «./file.logos», main(«hello»)'\n\
         \x20 logos '(logos file «./file.logos»).main(«hello»)'\n\
         `import` reads a file as Logos: it runs top to bottom, once, and\n\
         its `pub` names are set where the `import` stands; `logos` reads\n\
         it the same way and gives its scope, whose `pub` names are read\n\
         with `.`. The value of the first line that gives one is printed,\n\
         and the lines after it never run.\n\
         There are no build or compile flags: the code itself decides what\n\
         compiles.\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// Run the command line as one line of Logos source; the value its first line that gives one
/// gave prints at the end. Parse errors render with a
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
        let after = engine.store.stats();
        print_stats(&mut engine, core, after, &sources);
    }

    if let Some(shown) = last {
        println!("{shown}");
    }
    ExitCode::SUCCESS
}

/// A line of the program read whole and run by the one pass: the text the value its first
/// line that gives one gave shows, or none when no line gives one. The command line and the
/// REPL both echo what this gives (›The command line is Logos source; the binary stays out of
/// the way‹ [[R302]], ›A scope's value is what it evaluates to, and `return` is an optional early
/// exit from the enclosing function‹ [[R207]]). An error comes back rendered, what the line holds
/// left for the caller to end.
fn run_program_line(
    p: &mut Parser,
    types: &Core,
    path: &str,
    source: &str,
) -> Result<Option<String>, String> {
    let tail = match p.read_lines(LinesEnd::TextEnd) {
        Ok(tail) => tail,
        Err(ParseError::Run(e)) => {
            return Err(format!("{path}: run error: {}", report::run_message(&e)))
        }
        Err(e) => return Err(report::render(path, source, p.offset(), &report::parse_message(&e))),
    };
    // The value is read after the root's own run and before the teardowns, which may free
    // what it points at, and rendered before the pass's runtime goes: an unnamed result
    // lives in that runtime's scratch.
    let message = |e: ParseError| format!("{path}: {}", report::parse_message(&e));
    let (node, bits) = match p.finish().map_err(message)? {
        Some(given) => given,
        None => match tail.map(|(cell, _)| cell.dyad) {
            // A `[…]` last line, which the pass never runs, gives its value now.
            // SAFETY: `node` is a line the parser built.
            Some(node) if unsafe { p.gives_unrun(node) } => {
                p.end_tail().map_err(message)?;
                // SAFETY: as above.
                (node, unsafe { p.value_of(node) }.map_err(message)?)
            }
            _ => {
                p.end_tail().map_err(message)?;
                return Ok(None);
            }
        },
    };
    // SAFETY: `node` is the parsed dyad whose value `bits` is.
    let shown = unsafe { seed::identities::display_value(types, node, bits) };
    // The last line ends once its value is shown.
    p.end_tail().map_err(message)?;
    Ok(Some(shown))
}

/// `LOGOS_STATS`: the store after the core, after the line, and their delta
/// against the source that was parsed (the command line and every loaded file).
/// Counting the code lexes it once more, after `after` was read.
fn print_stats(engine: &mut Engine, core: StoreStats, after: StoreStats, sources: &[&str]) {
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
fn code_bytes(engine: &mut Engine, text: &str) -> usize {
    let mut scopes = ScopeStack::new();
    scopes.push(engine.core.root_scope);
    let lexed =
        seed::parse::lex_fragment(&scopes, &engine.trie, &mut engine.store, &engine.core, text);
    let Ok(tape) = lexed else {
        return 0;
    };
    let cells = tape.cells();
    cells.iter().filter(|c| c.identity(&engine.core) != engine.core.hash_).map(|c| c.len).sum()
}

/// One persistent store, index and scope; each line a line of the program. A line that gives a
/// value ends the session with it, as a program ends with its value; a line that fails reports,
/// leaves no trace, and the loop continues.
fn repl() -> ExitCode {
    println!(
        "logos {} — each line is a line of the program, which ends at the first line that gives \
         a value or at ctrl-d",
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
        let session = scopes.current().expect("the session's scope is open");
        let line = match lines.next() {
            Some(Ok(line)) => line,
            _ => {
                println!();
                return end_session(&mut engine, session, None);
            }
        };
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
                // A failed line keeps the ends it reached of an earlier line's names, what a
                // file it imported holds stays the session's, and what its own lines hold ends
                // as at a fault (DESIGN ›A checked error is a fault: the task that hit it is
                // cancelled‹).
                Err(_) => {
                    let freed = p.end_failed_earlier_holds();
                    // SAFETY: `session` is the session's scope node.
                    unsafe { p.hand_imported_exit(session) };
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
                scopes.commit();
                // The session is one scope, and a line that gives a value ends it
                // (›The command line is Logos source; the binary stays out of the way‹ [[R302]]).
                if last.is_some() {
                    return end_session(&mut engine, session, last);
                }
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

/// The session's end, as `run_line`'s: what its kept lines hold and defer ends, last first,
/// then the value that ended it shows.
fn end_session(engine: &mut Engine, session: DyadPtr, shown: Option<String>) -> ExitCode {
    let mut rt = Runtime::new(&engine.core, &mut engine.store);
    // SAFETY: `session` is the session's scope node, its places in the program frame.
    if let Err(e) = unsafe { seed::identities::run_scope_exit(&mut rt, session) } {
        eprintln!("<repl>: run error: {}", report::run_message(&e));
        return ExitCode::FAILURE;
    }
    if let Some(shown) = shown {
        println!("{shown}");
    }
    ExitCode::SUCCESS
}
