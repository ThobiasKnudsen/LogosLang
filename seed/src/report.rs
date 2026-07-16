// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Human-readable diagnostics: byte offsets become `file:line:col`, errors
//! become sentences, and the offending source line is shown with a caret.
//!
//! The parser tracks byte offsets ([`crate::parse::Token`], the cursor) but its
//! errors carry none; the honest v1 position is *where the parser stopped*
//! ([`crate::parse::Parser::offset`]) — the stuck point, which for the common
//! errors (an unknown name, a bad literal) sits exactly at the offending
//! token. Run/compile errors have no path to a source position yet (nodes
//! carry no spans; the derived source-map is future work), so they render as
//! message-only lines. All diagnostic text lives here, in one place: the error
//! enums stay plain data, and nothing ever prints a raw node address.

use crate::compile::CompileError;
use crate::parse::{ParseError, ResolveError};
use crate::regex_trie::RegexTrieError;
use crate::run::RunError;

/// The 1-based `(line, column)` of byte `offset` in `source`. Columns count
/// characters, not bytes (a `«…»` earlier in the line must not shift the
/// caret). Any offset is safe: past-the-end clamps to the end, and an offset
/// inside a multibyte character counts the character it falls in.
pub fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source.as_bytes()[..offset];
    let line = 1 + before.iter().filter(|&&b| b == b'\n').count();
    let line_start = before.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    // Count characters, tolerating an offset that splits a multibyte sequence.
    let col = 1 + String::from_utf8_lossy(&before[line_start..]).chars().count();
    (line, col)
}

/// Render a positioned diagnostic: `file:line:col: error: message` (the
/// editor-clickable prefix), the offending source line, and a caret under the
/// column. A tab in the line stays a tab in the pad, so the caret aligns
/// however tabs render.
pub fn render(file: &str, source: &str, offset: usize, message: &str) -> String {
    let (line, col) = line_col(source, offset);
    let text = source.lines().nth(line - 1).unwrap_or("");
    let pad: String = text
        .chars()
        .take(col - 1)
        .map(|c| if c == '\t' { '\t' } else { ' ' })
        .collect();
    format!("{file}:{line}:{col}: error: {message}\n  {text}\n  {pad}^")
}

/// The human sentence for a parse error.
pub fn parse_message(e: &ParseError) -> String {
    match e {
        ParseError::Resolve(r) => resolve_message(r),
        ParseError::MissingOperand => "an operator is missing an operand".into(),
        ParseError::Trailing => "expected one expression, found more".into(),
        ParseError::Empty => "nothing to evaluate here".into(),
        ParseError::BadLiteral => "this is not a valid literal".into(),
        ParseError::UnclosedBracket => "this `(` is never closed".into(),
        ParseError::ExpectedOpen => "expected a `(` here".into(),
        ParseError::ExpectedField => "expected a field name here".into(),
        ParseError::ExpectedArrow => "expected `->` before the return type".into(),
        ParseError::ExpectedReturnType => "expected a return type after `->`".into(),
        ParseError::UnsupportedOperands => {
            "this operator cannot compute over these operands".into()
        }
        ParseError::NonBoolCondition => "this condition must be a bool".into(),
        ParseError::MissingElse => {
            "an `if` used as a value needs an `else` branch".into()
        }
        ParseError::NonBoolOperands => "these operands must be bools".into(),
        ParseError::TypeMismatch => {
            "these types do not match (cross-type needs an explicit cast)".into()
        }
        ParseError::UncomputableLiteral => {
            "this literal has no exact value in the type it lands in".into()
        }
        ParseError::EarlyReturn => {
            "`return` must be the last expression of its scope".into()
        }
        ParseError::StatementAsValue => {
            "a statement yields no value and cannot stand here".into()
        }
        ParseError::BadAssignTarget => "this is not an assignable place".into(),
        ParseError::CtorArity => {
            "this construction's argument count does not match the fields".into()
        }
        ParseError::ExpectedLoopVar => "expected a loop variable after `for`".into(),
        ParseError::ExpectedIn => "expected `in` after the loop variable".into(),
        ParseError::ExpectedRange => "expected a range like `0..10` here".into(),
        ParseError::BadStep => "a literal loop step must be positive".into(),
        ParseError::BadAddressOf => "`&` needs a variable to take the address of".into(),
        ParseError::BadCast => "a conversion takes exactly one numeric value".into(),
        ParseError::TypedDeclaration => {
            "typed declarations (`name : type`) are not in the seed yet; \
             declare with a value: `name := i32 0`"
                .into()
        }
        ParseError::CapturedLocal => {
            "a nested function cannot use an outer function's local or parameter \
             (no closures yet); pass it in as the nested function's own parameter"
                .into()
        }
    }
}

/// The human sentence for a name-resolution error.
fn resolve_message(e: &ResolveError) -> String {
    match e {
        ResolveError::Unknown => "unknown name".into(),
        ResolveError::OutOfScope => "this name is not in scope here".into(),
        ResolveError::Ambiguous => "this name is ambiguous here".into(),
        ResolveError::Shadowed => "this name is shadowed here".into(),
        ResolveError::Index(RegexTrieError::NodeNotFound) => {
            "unknown name".into()
        }
        ResolveError::Index(RegexTrieError::BadPattern(p)) => {
            format!("this name's pattern is invalid: {p}")
        }
    }
}

/// The human sentence for a run error (message-only: nodes carry no source
/// positions yet).
pub fn run_message(e: &RunError) -> String {
    match e {
        RunError::NotRunnable(_) => "this is not runnable".into(),
        RunError::BadValue => "a value here has no storage to read".into(),
        RunError::UncomputableLiteral => {
            "a literal here has no exact value in its context type".into()
        }
        RunError::ArityMismatch => {
            "a call's argument count does not match its function".into()
        }
        RunError::CompiledArity => {
            "compiled calls take at most three arguments in v1".into()
        }
    }
}

/// The human sentence for a compile error (message-only, as [`run_message`]).
pub fn compile_message(e: &CompileError) -> String {
    match e {
        CompileError::NotLowerable(_) => "this cannot be compiled yet".into(),
        CompileError::BadValue => "a value here has no storage to compile against".into(),
        CompileError::UncomputableLiteral => {
            "a literal here has no exact value in its context type".into()
        }
        CompileError::UnsupportedArity(n) => {
            format!("compiled functions take at most three parameters in v1 (this one has {n})")
        }
        CompileError::UncompiledCallee(_) => {
            "a call target is not compiled yet (declare functions before their callers)".into()
        }
        CompileError::ArityMismatch => {
            "a call's argument count does not match its function".into()
        }
        CompileError::Cranelift(msg) => format!("the backend rejected this: {msg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_is_one_based_and_counts_chars() {
        let src = "first\nsecond line\nthird";
        assert_eq!(line_col(src, 0), (1, 1));
        assert_eq!(line_col(src, 4), (1, 5));
        assert_eq!(line_col(src, 6), (2, 1));
        assert_eq!(line_col(src, 13), (2, 8));
        assert_eq!(line_col(src, src.len()), (3, 6)); // past the last char
        assert_eq!(line_col(src, 9999), (3, 6)); // clamps
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        // `«` is 2 bytes; the caret must land on the char after it, not two on.
        let src = "x := «a»\nzz";
        let off = src.find("»").unwrap();
        assert_eq!(line_col(src, off), (1, 8));
    }

    #[test]
    fn render_shows_the_line_and_a_caret() {
        let src = "x := i32 40\nx + zz";
        let off = src.find("zz").unwrap();
        let got = render("f.logos", src, off, "unknown name");
        assert_eq!(got, "f.logos:2:5: error: unknown name\n  x + zz\n      ^");
    }

    #[test]
    fn render_preserves_tabs_in_the_caret_pad() {
        let src = "\tx + zz";
        let off = src.find("zz").unwrap();
        let got = render("f.logos", src, off, "unknown name");
        assert_eq!(got, "f.logos:1:6: error: unknown name\n  \tx + zz\n  \t    ^");
    }

    #[test]
    fn messages_never_print_node_addresses() {
        let msg = run_message(&RunError::NotRunnable(std::ptr::null_mut()));
        assert!(!msg.contains("0x"));
        let msg = compile_message(&CompileError::UncompiledCallee(std::ptr::null_mut()));
        assert!(!msg.contains("0x"));
    }
}
