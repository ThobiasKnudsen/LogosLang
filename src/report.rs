// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Human-readable diagnostics: byte offsets become `file:line:col`, errors
//! become sentences, and the offending source line is shown with a caret.
//! All diagnostic text lives here; nothing prints a raw node address.

use crate::compile::CompileError;
use crate::parse::{ParseError, ResolveError, SlotKind};
use crate::regex_trie::RegexTrieError;
use crate::run::RunError;

/// The 1-based `(line, column)` of byte `offset`. Columns count characters, not
/// bytes; any offset is safe (past the end clamps, inside a multibyte character
/// counts that character).
pub fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source.as_bytes()[..offset];
    let line = 1 + before.iter().filter(|&&b| b == b'\n').count();
    let line_start = before.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let col = 1 + String::from_utf8_lossy(&before[line_start..]).chars().count();
    (line, col)
}

/// `file:line:col: error: message`, the source line, and a caret under the
/// column. A tab in the line stays a tab in the pad, so the caret aligns
/// however tabs render.
pub fn render(file: &str, source: &str, offset: usize, message: &str) -> String {
    let (line, col) = line_col(source, offset);
    let text = source.lines().nth(line - 1).unwrap_or("");
    let pad: String =
        text.chars().take(col - 1).map(|c| if c == '\t' { '\t' } else { ' ' }).collect();
    // A multi-line message heads the block with its first line and follows the
    // caret with the rest, so a nested rendering reads top down.
    let (first, rest) = match message.split_once('\n') {
        Some((first, rest)) => (first, Some(rest)),
        None => (message, None),
    };
    let mut out = format!("{file}:{line}:{col}: error: {first}\n  {text}\n  {pad}^");
    if let Some(rest) = rest {
        out.push('\n');
        out.push_str(rest);
    }
    out
}

pub fn parse_message(e: &ParseError) -> String {
    match e {
        ParseError::Resolve(r) => resolve_message(r),
        ParseError::MissingOperand => "an operator is missing an operand".into(),
        ParseError::Trailing => "expected one expression, found more".into(),
        ParseError::Empty => "nothing to evaluate here".into(),
        ParseError::BadLiteral => "this is not a valid literal".into(),
        ParseError::UnclosedBracket => "this bracket is never closed".into(),
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
            "these types do not match (crossing types needs an explicit cast)".into()
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
        ParseError::AssignToLiteral(lit) => format!(
            "`{lit}` is a literal with no storage: `x := {lit}` names the number \
             itself, so `=` has nothing to write into. Declare a type, as in \
             `mut x := i32 5`, to make a place"
        ),
        ParseError::GateNeedsDeclaration => {
            "a gate word (`pub`, `mut`, `immut`) must be followed by a declaration".into()
        }
        ParseError::DoubleGate => "this gate word already stands on the declaration".into(),
        ParseError::Immutable(name) => {
            format!("`{name}` is `immut`: it is written nowhere, not even by its constructor")
        }
        ParseError::Unwritten(name) => format!(
            "`{name}` is read before it is written: give it a value with `{name} = …` in the same block first"
        ),
        ParseError::NotMutable(name) => {
            format!("`{name}` is not `mut`: a name is written after its declaration only when declared `mut {name} := …`")
        }
        ParseError::ExpectedPath => "`import` must be followed by a file path".into(),
        ParseError::ExpectedPattern => "`regex` must be followed by a «…» pattern".into(),
        ParseError::ExpectedQuote(word) => format!("`{word}` must be followed by a «…» quote"),
        ParseError::UnclosedInterpolation => {
            "this `{` has no `}`: write `\\{` to print a brace".into()
        }
        ParseError::StrayInterpolationClose => {
            "this `}` closes no `{`: write `\\}` to print a brace".into()
        }
        ParseError::InsertTakesTape => {
            "`insert` splices a tape: give it a `lex «…»` fragment or a parsing_tape value".into()
        }
        ParseError::CellLeftUnconstructed(name) => format!(
            "the constructor of `{name}` edited the tape but left its own cell unconstructed: \
             place what it built and set `tape.is_constructed[0] = true`, hand the cell to \
             another identity, remove it, or set the flag on the untouched cell to stand as itself"
        ),
        ParseError::BadPattern(why) => format!("this pattern does not compile: {why}"),
        ParseError::ImportInRuntimeBody => {
            "`import` loads at parse time, so it cannot stand inside a \
             function body, loop, or runtime branch"
                .into()
        }
        ParseError::ImportRead(detail) => format!("cannot read {detail}"),
        ParseError::ImportCycle(path) => format!(
            "importing `{path}` here closes an import cycle — the import graph \
             must be a DAG"
        ),
        ParseError::ImportFailed { path, rendered } => {
            format!("import of `{path}` failed:\n{rendered}")
        }
        ParseError::BadReflectRead => {
            "this read does not fit the node's type".into()
        }
        ParseError::ExpectedIndexBracket => {
            "expected `[index]` — element access is `[…]`, `(…)` is application".into()
        }
        ParseError::TypeIsColonRead => {
            "a value's type is not one of its fields — read it with `:`: x:type".into()
        }
        ParseError::CellNotReachable => {
            "nothing reaches a value's cell as a whole — read its type with \
             `x:type` and its fields with `x.f`"
                .into()
        }
        ParseError::CtorArity => {
            "this construction's argument count does not match the fields".into()
        }
        ParseError::ExpectedIn => "expected `in` after the loop variable".into(),
        ParseError::ExpectedRange => "expected a range like `0..10` here".into(),
        ParseError::BadStep => "a literal loop step must be positive".into(),
        ParseError::BadAddressOf => "`&` needs a variable to take the address of".into(),
        ParseError::BadCast => "a conversion takes exactly one numeric value".into(),
        ParseError::BadDeclaredType => {
            "the declared or assigned type must be a type value"
                .into()
        }
        ParseError::NonNumericDeclaredType => {
            "declarations of non-numeric types are not in the seed yet"
                .into()
        }
        ParseError::TypeBodyLine => {
            "a type body line fills a slot (`parse_rank = …`, `fields = (…)`) or is prose".into()
        }
        ParseError::DoubleFields => "a type body has one `fields = (…)` block".into(),
        ParseError::MemberOutsideFieldsBlock => {
            "a type body's own lines only fill slots with `=`; a member, shared or per instance, is declared inside `fields = (…)`".into()
        }
        ParseError::SharedOutsideFieldsBlock => {
            "`shared` marks a member inside `fields = (…)` and stands nowhere else".into()
        }
        ParseError::SharedNeedsDeclaration => {
            "`shared` must be followed by a declaration, `shared name := value`".into()
        }
        ParseError::DeferInTypeBody => "a type body cannot own what needs a teardown".into(),
        ParseError::TypeKnownOnlyAtRun => {
            "which type this is is known only when the program runs, and this needs it now".into()
        }
        ParseError::NonOwningIntoOwning => {
            "this place owns what it holds, so what is assigned must own too (`alloc …`, or `own x`)"
                .into()
        }
        ParseError::BadDyadType => {
            "a dyad of this type cannot be built here: only a number or a bool".into()
        }
        ParseError::TooDeep => {
            format!("scopes nested deeper than {}", crate::parse::MAX_BRACKET_DEPTH)
        }
        ParseError::TypeBodyFailed(msg) => {
            format!("a type body's own declaration failed at the definition: {msg}")
        }
        ParseError::NonComptimeRank => {
            "a parse_rank must be a number known when the type is defined".into()
        }
        ParseError::BadAssociativity => "associativity is `left` or `right`".into(),
        ParseError::SlotNeedsBody(SlotKind::Parse) => {
            "`parse` is a bare body over the tape, `parse = (…)`".into()
        }
        ParseError::SlotNeedsBody(_) => {
            "`run` is a bare body over the instance's fields, `shared run = (…)`".into()
        }
        ParseError::ThisNeedsFieldsBlock => {
            "`this.f` reads a field of the node being built, and this type declares none: write the `fields = (…)` block above the body".into()
        }
        ParseError::ThisFieldUnknown(name) => {
            format!("`this.{name}`: no field `{name}` is declared in the `fields = (…)` block above")
        }
        ParseError::FlagTakesBool => {
            "`tape.is_constructed[k] = …` takes a bool, `true` or `false`".into()
        }
        ParseError::RunTypeApplied => {
            "the call form of a type with a `run` is not in the seed: only its `parse` fills a node's fields and output, so write the node through the type's own spelling".into()
        }
        ParseError::RunBodyFailed { name, rendered } => {
            format!("the run body of `{name}` could not be constructed for these field types:\n{rendered}")
        }
        ParseError::SlotOutsideDefinition => {
            "a slot is filled on a line of the type body itself: `parse_rank = …` on a bare line, `shared run = (…)` inside `fields = (…)`".into()
        }
        ParseError::NoOwnRun => {
            "a type has no `run` of its own: its instances' run is filled with `shared run = (…)` inside `fields = (…)` and read through the type as `t.fields.run`".into()
        }
        ParseError::MemberThroughFields(name) => format!(
            "`{name}` is a member of the type's fields block: read it through the type as `.fields.{name}`, or through a node bare"
        ),
        ParseError::PerNodeThroughType(name) => format!(
            "`{name}` is a place in each node, not stored with the type: read it through a node"
        ),
        ParseError::FieldsSlotNotInSeed => {
            "inside `fields = (…)` only `shared run = (…)` fills a slot yet; the instances' other slots are not in the seed (#133)".into()
        }
        ParseError::DropSlotNotInSeed => "the `drop` slot is not in the seed yet (#133)".into(),
        ParseError::FieldsSlotNeedsShared => {
            "a slot fill inside `fields = (…)` is written `shared run = (…)`; an unmarked fill would be a per-instance default, not in the seed".into()
        }
        ParseError::LexRankNeedsName => {
            "lex_rank is the name's: write it in a declaration, `x := type (lex_rank = …)`, \
             or on the name, `x:lex_rank = …`"
                .into()
        }
        ParseError::ConstructorFailed(msg) => {
            format!("this identity's constructor failed while parsing: {msg}")
        }
        ParseError::Run(e) => format!("run error: {}", run_message(e)),
        ParseError::NonComptimeTypeCall => {
            "a `-> logos` call must be evaluable at parse time; \
             its arguments must be comptime-known"
                .into()
        }
        ParseError::CapturedLocal => {
            "a nested function cannot use an outer function's local or parameter \
             (no closures yet); pass it in as the nested function's own parameter"
                .into()
        }
        ParseError::UnboundOwningValue => {
            "an owning value must be bound to a name, which is where its `free` \
             attaches; bind it first (`a := alloc …`) and pass the name"
                .into()
        }
        ParseError::OwningEscape => {
            "this scope's value is a place it owns, so the value would be freed \
             on the way out; hand ownership over with `own`"
                .into()
        }
        ParseError::OwnershipAcrossReturn => {
            "a function cannot hand ownership out through its return yet: the \
             return logos cannot say it transfers ownership, so the caller would \
             not know it owes a `free`; allocate in the caller and pass a pointer in"
                .into()
        }
        ParseError::OwnOfOuterName => {
            "own or drop of a name declared outside this loop or function body: \
             the next pass or call would find it dead; move or drop it outside, \
             or hand it in as a parameter"
                .into()
        }
    }
}

pub(crate) fn resolve_message(e: &ResolveError) -> String {
    match e {
        ResolveError::Unknown(n) if n.is_empty() => "unknown name".into(),
        ResolveError::Unknown(n) => format!("unknown name `{n}`"),
        ResolveError::OutOfScope(n) => format!("`{n}` is not in scope here"),
        ResolveError::Shadowed(n) => format!(
            "`{n}` is already declared and still in scope; declaring it again would \
             leave the first one shadowed (a name ended by `drop {n}` may be declared again)"
        ),
        ResolveError::Dead(n) => format!("`{n}` is dead here: it was moved or dropped above"),
        ResolveError::Tied => {
            "two spellings match here with the same lex_rank; give one of them a higher lex_rank"
                .into()
        }
        ResolveError::Index(RegexTrieError::NodeNotFound) => "unknown name".into(),
        ResolveError::Index(RegexTrieError::BadPattern(p)) => {
            format!("this name's pattern is invalid: {p}")
        }
    }
}

/// Message-only: nodes carry no source positions yet.
pub fn run_message(e: &RunError) -> String {
    match e {
        RunError::NotRunnable(_) => "this is not runnable".into(),
        RunError::NotAFunction(_) => "only a function can be compiled".into(),
        RunError::NoLeaf => "this node was built with no operation to run".into(),
        RunError::NoActivation => "a function's local was read outside any call".into(),
        RunError::Uninitialized => "a value here has no storage to read".into(),
        RunError::NoWholeRead => {
            "a record, text or hole is not read as one value; read a field or take its address"
                .into()
        }
        RunError::MalformedFn(_) => "a parameter of this function has no frame slot".into(),
        RunError::NotText => "`lex` takes a string".into(),
        RunError::Output(why) => format!("print could not write to stdout: {why}"),
        RunError::NoTape => "there is no tape behind this receiver".into(),
        RunError::OffTape => "this index is off the tape".into(),
        RunError::NoName => "this cell holds no name".into(),
        RunError::NoFragment => "insert takes a tape fragment".into(),
        RunError::NoThis => "`this` holds no node here".into(),
        RunError::BadIndex(k) => format!("an index cannot be negative ({k})"),
        RunError::BadCount(n) => format!("an alloc count cannot be negative ({n})"),
        RunError::NotDerefable => "only a scalar or a pointer is read through a pointer".into(),
        RunError::NoLayout(_) => "this type has no field layout to construct".into(),
        RunError::EmptyScope => "a scope with nothing in it has no value".into(),
        RunError::NotANode(a) => format!("the address {a:#x} is not a node of the store"),
        RunError::NotAScope(_) => "this node is not a scope".into(),
        RunError::OutOfMemory => "the allocator refused this alloc".into(),
        RunError::NoDestructor(_) => "this place has no destructor to run".into(),
        RunError::UncomputableLiteral => {
            "a literal here has no exact value in the type it lands in".into()
        }
        RunError::ArityMismatch => "a call's argument count does not match its function".into(),
        RunError::CompilerUnavailable => {
            "compile() is not available here (parse-time evaluation runs without the compiler)"
                .into()
        }
        RunError::CompileFailed(msg) => format!("compile() failed: {msg}"),
        RunError::Faulted(msg) => format!("the interpreter stopped inside compiled code: {msg}"),
        RunError::NoLexer => "`lex` can run only where the parser runs it".into(),
        RunError::Lex(why) => format!("this text will not lex: {why}"),
        RunError::NoCaller => {
            "`caller` can be read only inside a constructor the parser runs".into()
        }
        RunError::CallerSpot => {
            "`caller` alone is not a value the seed reads; read `caller.scope`".into()
        }
        RunError::NullPointer => "this pointer holds nothing yet".into(),
        RunError::CallDepth => {
            format!(
                "calls nested deeper than {}: is this recursion ending?",
                crate::run::MAX_CALL_DEPTH
            )
        }
    }
}

/// Message-only, as [`run_message`].
pub fn compile_message(e: &CompileError) -> String {
    match e {
        CompileError::NotLowerable(_) => "this cannot be compiled yet".into(),
        CompileError::Uninitialized => "a value here has no storage to compile against".into(),
        CompileError::NoActivation => "a function's local is compiled outside any call".into(),
        CompileError::NotDerefable => "only a scalar or a pointer is read through a pointer".into(),
        CompileError::NoLayout(_) => "this type has no field layout to construct".into(),
        CompileError::EmptyScope => "a scope with nothing in it has no value".into(),
        CompileError::Internal(what) => format!("the seed broke its own rule: {what}"),
        CompileError::UncomputableLiteral => {
            "a literal here has no exact value in the type it lands in".into()
        }
        CompileError::ArityMismatch => "a call's argument count does not match its function".into(),
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
        assert_eq!(line_col(src, src.len()), (3, 6));
        assert_eq!(line_col(src, 9999), (3, 6));
    }

    #[test]
    fn columns_count_characters_not_bytes() {
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
        let msg = compile_message(&CompileError::NotLowerable(std::ptr::null_mut()));
        assert!(!msg.contains("0x"));
    }
}
