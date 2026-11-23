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
        ParseError::StrayCloser(closer) => {
            format!("this `{closer}` closes nothing: no bracket is open here")
        }
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
        ParseError::ArmGivesNothing => {
            "every arm of an `if` used as a value must give one, and this arm gives nothing \
             (a statement, a `return`, an `error` or an `abort` gives the `if` no value)"
                .into()
        }
        ParseError::ArmsDiffer => {
            "the arms of this `if`, or the lines that give this block its value, give different \
             types: convert them to one type, as in `if c (i64 (i32 1)) else (i64 2)`"
                .into()
        }
        ParseError::ArmsMakeAndBorrow => {
            "make a value in every arm, or borrow in every arm: one arm of this `if`, or one line \
             that gives this block its value, makes its value and another borrows one, so who \
             owns it would be known only when it runs"
                .into()
        }
        ParseError::LineAfterValue => {
            "this line never runs: the line before it always gives its scope a value, which ends \
             the scope; a value nobody wants is thrown away with `free`, as in `free (f())`"
                .into()
        }
        ParseError::LineAfterLeave => {
            "this line never runs: the line before it always leaves, with a `return`, an `error` \
             or an `abort`"
                .into()
        }
        ParseError::VoidBodyGives => {
            "this line gives a value, and the lines of a `-> void` or `-> void!` function give \
             none; a value nobody wants is thrown away with `free`, as in `free (f())`"
                .into()
        }
        ParseError::KeptScopeGives => {
            "this line gives a value, and a `{ … }` gives the scope itself; a value nobody wants \
             is thrown away with `free`, as in `free (f())`"
                .into()
        }
        ParseError::DeferGives => {
            "this gives a value, and a `defer` gives none: it runs as its scope ends, and nothing \
             takes a value there; a value nobody wants is thrown away with `free`, as in \
             `defer free (f())`"
                .into()
        }
        ParseError::HeldGivenUnmoved => {
            "this block gives a name it holds, and its end frees that name: give it with `move`, \
             as in `if c (move a)`"
                .into()
        }
        ParseError::HeldTakenOnTheWay => {
            "a name a block holds moves out of it as the block's value here, and this line takes \
             the value on the way: name it first, as in `y := if c (a) else (b)`, or write `move`"
                .into()
        }
        ParseError::EndsWithoutValue => {
            "this can end without giving a value, and a value is needed here: an `if` without \
             `else` or a loop gives one only on some paths"
                .into()
        }
        ParseError::NonBoolOperands => "these operands must be bools".into(),
        ParseError::TypeMismatch => {
            "these types do not match: a value converts only into a type known to hold every value \
             of it; a number that may lose part of itself converts into a number type with `lossy`, \
             as `i32 lossy (x)`"
                .into()
        }
        ParseError::UncomputableLiteral => {
            "this literal has no exact value in the type it lands in; into a number type, `lossy` \
             converts it anyway, as `f64 lossy 0.1`"
                .into()
        }
        ParseError::LiteralArithmetic => {
            "this arithmetic on plain numbers has no value: a division by zero, or a result past \
             what a fraction of two i64s holds"
                .into()
        }
        ParseError::ReturnOutsideFunction => "`return` is written only inside a function".into(),
        ParseError::ReturnInDefer => {
            "a `defer` cannot leave its function: it runs as its scope ends, once the way out is \
             already chosen, so it holds no `return`"
                .into()
        }
        ParseError::StatementAsValue => "this gives nothing, and a value is needed here".into(),
        ParseError::BadAssignTarget => "this is not an assignable place".into(),
        ParseError::AssignToLiteral(lit) => format!(
            "`{lit}` is a literal with no storage: `x := {lit}` names the number \
             itself, so `=` has nothing to write into. Declare a type, as in \
             `x := mut i32 5`, to make a place"
        ),
        ParseError::GateNeedsValue => {
            "a gate word (`pub`, `mut`, `static`) gates the value to its right, so it stands right of `:=`: `x := mut i32 5`".into()
        }
        ParseError::DoubleGate => "this gate word already gates the value".into(),
        ParseError::GateBeforeLaterWord => {
            "a gate word takes the nearest value to its right, and a word built after it, such as `try`, `not` or a negating `-`, is none: put that word and its value in brackets, as `x := mut (try alloc 1 of i32 5)` or `mut (not a)`".into()
        }
        ParseError::PointeeNotMutable => {
            "this pointer points at a value that is not `mut`: a write through `@` needs a `@ mut T`, as `&a` of `a := mut i32 5` or `try alloc 1 of mut i32 5` gives".into()
        }
        ParseError::Unwritten(name) => format!(
            "`{name}` is read before it is written: give it a value with `{name} = …` in the same block first"
        ),
        ParseError::NotMutable(name) => {
            format!("`{name}` is not `mut`: a name is written after its declaration only when declared `{name} := mut …`")
        }
        ParseError::LexOutsideScope(name) => format!(
            "`{name}:lex_rank` is written only from the scope that declares `{name}` or a scope inside it"
        ),
        ParseError::ImportTakesFile => {
            "`import` reads the file it is given: write `import file «path»`".into()
        }
        ParseError::AddressNotPlain => {
            "a `file «…»` address is written as plain text: a file whose address is computed \
             while the program runs is read through the `fs` the command line hands down"
                .into()
        }
        ParseError::FileOutsideProject { path, root } => format!(
            "`file «{path}»` is outside this project's folder, {root}: a file reaches only files \
             inside its project's folder, and reads any other while the program runs, through the \
             `fs` the command line hands down"
        ),
        ParseError::NoPubName(name) => {
            format!("the file has no `pub` name `{name}`: only its `pub` names are read")
        }
        ParseError::NotPub(name) => format!(
            "`{name}` is not `pub`, so it is read only inside the scope it is declared in: write \
             `{name} := pub …` to read it from outside"
        ),
        ParseError::ExpectedPattern => "`regex` must be followed by a «…» pattern".into(),
        ParseError::ExpectedQuote(word) => format!("`{word}` must be followed by a «…» quote"),
        ParseError::ErrorNeedsCategory => {
            "an error needs a category: write `error.x «…»` over a category declared with \
             `error.x := «description»`, or `abort «…»` to stop the program"
                .into()
        }
        ParseError::UnknownCategory(name) => {
            format!("unknown category `{name}`: declare it with `{name} := «description»`")
        }
        ParseError::CategoryDeclared(name) => format!(
            "the category `{name}` is already declared and still in scope: a category is \
             declared once where it can be seen, and it lasts as long as its scope"
        ),
        ParseError::CategoryApart(name) => {
            format!("a category's name is one word: write `{name}`, with no space inside it")
        }
        ParseError::CategoryAlone(name) => format!(
            "`{name}` is a category, a name with no value to read: raise it with \
             `{name} «message»`"
        ),
        ParseError::EmptySpelling => {
            "a declared spelling must not be empty: no text could ever lex as it".into()
        }
        ParseError::StrayInterpolationClose => {
            "this `}` closes no `{`: write `\\}` to print a brace".into()
        }
        ParseError::HashmapShape => {
            "`hashmap` is followed by `K -> V`: a key type, `->`, a value type, each `type`, \
             `dyad`, `string` or an integer type, and the value type also a map, `hashmap K2 -> V2`"
                .into()
        }
        ParseError::ScopeKeysAreText => {
            "a `hashmap` that reads `{ … }` keys each entry by its name's spelling, so its key type \
             is `string`: `hashmap string -> V { … }`"
                .into()
        }
        ParseError::PatternIsNoKey(pattern) => format!(
            "`{pattern}` is a pattern, which stands for many spellings, so it is no key of the \
             `hashmap` that reads this `{{ … }}`"
        ),
        ParseError::InsertTakesTape => {
            "`insert` splices a tape: give it a `lex «…»` fragment or a parsing_tape value".into()
        }
        ParseError::CellLeftUnconstructed(name) => format!(
            "the constructor of `{name}` edited the tape but left its own cell unconstructed: \
             place what it built and set `tape.is_constructed[0] = true`, hand the cell to \
             another identity, remove it, or set the flag on the untouched cell to stand as itself"
        ),
        ParseError::BadPattern(why) => format!("this pattern does not compile: {why}"),
        ParseError::FileRead(detail) => format!("cannot read {detail}"),
        ParseError::ImportCycle(path) => format!(
            "reading `{path}` as Logos here closes a cycle: the graph of files reaching files \
             must be a DAG"
        ),
        ParseError::ImportFailed { path, rendered } => {
            format!("reading `{path}` as Logos failed:\n{rendered}")
        }
        ParseError::BadReflectRead => {
            "this read does not fit the node's type".into()
        }
        ParseError::NoMint => "this operation has no mint for these field types".into(),
        ParseError::ExpectedIndexBracket => {
            "expected `[index]` — element access is `[…]`, `(…)` is application".into()
        }
        ParseError::NoNodeYet => {
            "this name has no node yet: its bytes lie in its frame and its type on its binding, \
             so read `x:dyad.type` or `x:dyad.value`"
                .into()
        }
        ParseError::UnsettledInclusion => {
            "`⊆` answers only for a type and itself, between pointer types, or between number \
             types and `bool`; \
             between other types what each can hold is not settled yet"
                .into()
        }
        ParseError::CtorArity => {
            "this construction's argument count does not match the fields".into()
        }
        ParseError::ExpectedIn => "expected `in` after the loop variable".into(),
        ParseError::ExpectedRange => "expected a range like `0..10` here".into(),
        ParseError::BadStep => "a literal loop step must be positive".into(),
        ParseError::BadAddressOf => "`&` needs a variable to take the address of".into(),
        ParseError::BadCast => "a conversion takes exactly one value".into(),
        ParseError::BadDeclaredType => {
            "the declared or assigned type must be a type value"
                .into()
        }
        ParseError::NameNeedsType => {
            "a name needs a type: write it, or `dyad ?` for one whose type changes".into()
        }
        ParseError::FieldNeedsType => {
            "a field needs a type: write it, write `dyad ?` for one whose type changes, or build \
             the mints in code written for them, as `array`'s parse does"
                .into()
        }
        ParseError::DyadTakesNode => {
            "a `dyad ?` place does not take this value yet: it takes a node, or a value of a type \
             whose value moves whole, which it holds in a new node; a field, a cell and a `mut` \
             parameter take only a node yet"
                .into()
        }
        ParseError::WaitingLineHolds(word) => format!(
            "this line needs a type known only when it runs, a `dyad` value's or a type a name or \
             a call gives then, so it is parsed only when it runs, and such a line cannot hold \
             `{word}` yet: a `:=` inside it would declare a name whose type is known only then, \
             and a `return`, `free` or `move` is checked where it is written"
        ),
        ParseError::DyadReadEachPass => {
            "a `while` condition cannot read what a call that gives a `dyad` gives through a \
             bracket or a member yet: such a condition is held only where the call is written \
             bare, and parsing it here would run the call once more than the condition runs; \
             write the call bare, as `while i < g() (…)`, or give its value a name first"
                .into()
        }
        ParseError::RunsBeforeUnbuilt => {
            "the parse needs a value here (a box's, a call's for a type, the node a call that gives \
             a `dyad` gives, or what an import runs), so what its line runs before it runs first; \
             here something before it that runs cannot run first yet, such as what stands \
             before a branch; give the value a name on a line of its own first, as \
             `v := g(), k(h(), v + 1)`"
                .into()
        }
        ParseError::NonNumericDeclaredType => {
            "declarations of non-numeric types are not in the seed yet"
                .into()
        }
        ParseError::TypeBodyLine => {
            "a type body line fills a slot (`parse_rank = …`, `parse = (…)`), declares a field (`x := i32 ?`) or a member (`y := static 3`), or is prose".into()
        }
        ParseError::StaticMisplaced => {
            "`static` makes one place for a type or a function, `y := static 3`, never on a parameter, which each call fills".into()
        }
        ParseError::PubParameter => {
            "a parameter takes no `pub`: only its own function reads it".into()
        }
        ParseError::OnceReadsUnmade => {
            "a value made once, where it is written (`static`, `immediate`), cannot use a name that has no value there yet: a parameter or local of the function around it, or a name of the loop or branch around it".into()
        }
        ParseError::DeferInTypeBody => "a type body cannot own what needs a teardown".into(),
        ParseError::TypeKnownOnlyAtRun => {
            "which type this is is known only when the program runs, and this needs it now".into()
        }
        ParseError::NonOwningIntoOwning => {
            "this place owns what it holds, so what is assigned must own too (`alloc …`, or `move x`)"
                .into()
        }
        ParseError::BadDyadType => {
            "`dyad (T, v)` cannot make a node of this type yet: it takes a type whose value moves \
             whole (a number, a bool, a pointer, a rational, a record kept in its bytes)"
                .into()
        }
        ParseError::RunPlaceNotData => {
            "this place's type is known only when its line runs, and a place is laid out then \
             only for a type whose value moves whole (a number, a bool, a pointer, a rational, a \
             record kept in its bytes)"
                .into()
        }
        ParseError::TooDeep => {
            format!("scopes nested too deep: the parse used up its {} stack", work_stack())
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
        ParseError::SlotNeedsBody(SlotKind::Free) => {
            "`free` is a bare body over the value, `free = (…)`".into()
        }
        ParseError::SlotNeedsBody(_) => {
            "`run` is a bare body over the value's fields, `run = (…)`".into()
        }
        ParseError::NoSuchField(name) => {
            format!("no field `{name}` is declared above in the type body")
        }
        ParseError::NotAField(name) => format!("this value's type has no field `{name}`"),
        ParseError::BareFieldInParse(name) => {
            format!("a parse writes and reads a field through its cell, `tape[0].{name}`, never bare")
        }
        ParseError::StampOutsideParse => {
            "`tape[0].type = T` starts a node only in a parse, on its own cell `tape[0]`".into()
        }
        ParseError::StampNeedsType => {
            "`tape[0].type = T` takes a type: a type's name, or a call that gives one".into()
        }
        ParseError::StaticFnNeedsValue { name, member } => {
            let through = if *member {
                format!("`v.{name}(…)`")
            } else {
                "by the member that holds it".to_string()
            };
            format!(
                "`{name}` works on a value of its type: call it through one, {through}, or bare from that type's `run`, `free` or `static` functions; any other call has no value, so it takes only a function that uses no field"
            )
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
            "a slot is filled on a line of the type body itself: `parse_rank = …`, `run = (…)`".into()
        }
        ParseError::PerNodeThroughType(name) => format!(
            "`{name}` is a place in each node, not stored with the type: read it through a node"
        ),
        ParseError::LexRankNeedsName => {
            "lex_rank is the name's: write it in a declaration, `x := type (lex_rank = …)`, \
             or on the name, `x:lex_rank = …`"
                .into()
        }
        ParseError::ConstructorFailed(msg) => {
            format!("this identity's constructor failed while parsing: {msg}")
        }
        ParseError::Run(e) => format!("run error: {}", run_message(e)),
        ParseError::ImmediateNotHeld => {
            "`immediate` stands as a number, a bool, a type or a node; this expression's value \
             is none of those (a record, a run-time rational or a pointer lives in the run's frame)"
                .into()
        }
        ParseError::NonComptimeTypeCall => {
            "a `-> type` call must be evaluable at parse time; \
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
        ParseError::AddressOfHeld => {
            "this hands out the address of a name its scope holds, whose end frees the value \
             under the address; hand out the name itself, which moves out"
                .into()
        }
        ParseError::MoveOfUnownedPath => {
            "moving out of a field or cell that owns nothing is not in the seed yet; a name \
             moves whatever it holds"
                .into()
        }
        ParseError::UnfollowedPath => {
            "this field is reached through a pointer, an element, a value no name holds, a name \
             that borrows it or a `static` function's own value, so nothing here can end it: \
             `move` and `free` take a field through the name that holds it, `move x.p`"
                .into()
        }
        ParseError::EndThroughBorrow => {
            "this cell is reached through something that only borrows it, a pointer `&` made, a \
             copy of its owner or a parameter without `own`, so nothing here can end it: `move` \
             and `free` take a cell through the pointer that owns it"
                .into()
        }
        ParseError::EndUnderFree(spelled) => {
            let (path, holder) = &**spelled;
            format!(
                "the type of `{holder}` fills its own `free = (…)`, which frees `{path}` with \
                 the rest of `{holder}`: `move` and `free` cannot take `{path}` out alone; free \
                 `{holder}` whole"
            )
        }
        ParseError::MoveOfBorrow => {
            "this name borrows what it holds, so it cannot move it; only its owner can".into()
        }
        ParseError::OwnNeedsPointer => {
            "`own` stands only before a pointer, `own @T ?` or `-> own @T`; a value of any other \
             type is owned with no word written, where its type fills `free`"
                .into()
        }
        ParseError::OwnOutsideType => {
            "`own` marks a hole, `own @T ?`, or a function's result, `-> own @T`; `move x` moves \
             a value"
                .into()
        }
        ParseError::HandsOnMade(owning) => {
            format!("this function hands on a value it made: write `-> {owning}`")
        }
        ParseError::OwnResultBorrows => {
            "this function hands on a borrow, but its result says `-> own`, which hands on a \
             value made here; make or move the value, or write the result without `own`"
                .into()
        }
        ParseError::RunReturnBorrows => {
            "this `return` hands on a borrow, but this `run` hands on the value it makes as its \
             last value, and every `return` must do the same; make or move the value, or hand on \
             only borrows, the last value too"
                .into()
        }
        ParseError::NoSuchSlot(name) => format!(
            "`{name}` names no slot: a type body fills `parse = (…)`, `run = (…)`, \
             `free = (…)` and its ranks, and declares a member of its own, `{name} := static …`"
        ),
        ParseError::FieldsNeedFree(fields) => {
            let named = fields.iter().map(|f| format!("`{f}`")).collect::<Vec<_>>().join(", ");
            let (which, own, it) = match fields.len() {
                1 => ("field", "owns what it holds", "it"),
                _ => ("fields", "own what they hold", "they"),
            };
            format!(
                "the {which} {named} {own}, so {it} must be freed by the type's \
                 `free = (…)`, which this type body does not fill"
            )
        }
        ParseError::KeptEntry(name) => format!(
            "`{name}` cannot be kept in this `{{ … }}`'s value: the seed keeps a name whose value \
             moves whole into a field, as a number, a string, a map, a record or a pointer does"
        ),
        ParseError::KeptReadEnded { name, reader } => format!(
            "`{reader}` reads the names written above it from this `{{ … }}`'s value, so they \
             must last to its `}}`, and `{name}` ends before it"
        ),
        ParseError::OwnParameterNotInSeed => {
            "an `own` parameter, which takes its argument's ownership, is not in the seed yet".into()
        }
        ParseError::UnionNeedsMembers => {
            "a union is written with its members, each a name and a type: \
             `union ( a := i32, b := f64 )`"
                .into()
        }
        ParseError::UnionTooManyMembers => format!(
            "a union holds one of at most {} members",
            crate::identities::union::MOST_MEMBERS
        ),
        ParseError::UnionLine => {
            "a union's line is a member, a name and a type, `a := i32`: a union has no `static` \
             member, fills no slot, and a member has no default"
                .into()
        }
        ParseError::UnionMemberPub => {
            "every member of a union is read from outside it, so a member takes no `pub`; `mut` \
             still lets it be written from outside"
                .into()
        }
        ParseError::HoldsNeedsUnion => {
            "`holds` asks a union which member it holds, and this value is no union".into()
        }
        ParseError::FallibleNeedsType => {
            "`!` makes a type that holds a value or an error, `i32!`, and what stands to its left \
             is no type"
                .into()
        }
        ParseError::TryNeedsFallible => {
            "`try` passes on the error of a value that holds a value or an error, `T!`, and this \
             value is no `T!`"
                .into()
        }
        ParseError::TryOutsideFallible => {
            "`try` passes an error on to the caller, so it is written inside a function that gives \
             a value or an error, `-> i32!`, or at the top level, which hands it to the command \
             line"
                .into()
        }
        ParseError::ErrorInDefer => {
            "a `defer` runs as its scope ends, where nobody receives an error, so it holds no raise \
             and no `try`; `abort «…»` stops the program"
                .into()
        }
        ParseError::HoldsNeedsMember => {
            "`holds` is followed by the name of a member of the union: `v holds a`".into()
        }
        ParseError::NotAMember(named) => {
            let (name, members) = &**named;
            let members = members.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ");
            format!("`{name}` is no member of this union, whose members are {members}")
        }
        ParseError::UnionApplied => {
            "a union holds one member, so it is not made from values in a row: declare an empty \
             place and write the member, `v := mut num ?, v.a = …`"
                .into()
        }
        ParseError::LayoutForm => {
            "`layout` takes the field's type, then the byte the field starts at: \
             `a := layout (mut i32, 4)`"
                .into()
        }
        ParseError::LayoutByte => {
            "the byte `layout` gives a field is a `u64` known where the type is built, such as `4`"
                .into()
        }
        ParseError::LayoutOutsideField => {
            "`layout` gives a field of a type body its type and its byte, and this is no such \
             field: `t := type ( a := layout (mut i32, 4) )`"
                .into()
        }
        ParseError::LayoutTooFar => format!(
            "a field ends past byte {}, the store's whole room, which no value outgrows",
            crate::store::ARENA_CAP
        ),
        ParseError::UnionMemberUnkept(name) => format!(
            "`{name}` cannot be a member of a union: the seed keeps a member whose value moves \
             whole into a place, as a number, a string, a record or a pointer does"
        ),
        ParseError::MoveOfOuterName => {
            "move or free of a name declared outside this loop or function body: \
             the next pass or call would find it dead; move or free it outside, \
             or hand it in as a parameter"
                .into()
        }
        ParseError::MoveOfValue => {
            "`move` moves a value out of a place: a name, a field `p.f`, a dereference `p@` or \
             an element `a[k]`. No name holds this value, so there is nothing to move it out of: \
             bind it as it is, `b := f()` for `b := move (f())`"
                .into()
        }
        ParseError::FreeOfHole => {
            "a hole, `T ?`, holds no value yet, so `free` has nothing to run or free here".into()
        }
        ParseError::EndsPrimordialName(name) => format!(
            "`{name}` is a name the run starts with, which every imported file reads too, so \
             this program cannot end it: give it a name of your own, `t := {name}`, and end that"
        ),
    }
}

pub(crate) fn resolve_message(e: &ResolveError) -> String {
    match e {
        ResolveError::Unknown(n) if n.is_empty() => "unknown name".into(),
        ResolveError::Unknown(n) => format!("unknown name `{n}`"),
        ResolveError::OutOfScope(n) => format!("`{n}` is not in scope here"),
        ResolveError::Shadowed(n) => format!(
            "`{n}` is already declared and still in scope; declaring it again would \
             leave the first one shadowed (a name ended by `free {n}` may be declared again)"
        ),
        ResolveError::Dead(n) => format!("`{n}` is dead here: it was moved or freed above"),
        ResolveError::Unbuilt(n) => {
            format!("`{n}` is used inside its own declaration, before its value is built")
        }
        ResolveError::Tied => {
            "two spellings match here with the same lex_rank; give one of them a higher lex_rank"
                .into()
        }
        ResolveError::Unmatched { bracket: '{', .. } => {
            "this `{` has no `}`: write `\\{` for a brace as text".into()
        }
        ResolveError::Unmatched { bracket: '»', .. } => {
            "this `»` closes no `«`: a `{…}` in a quote is code".into()
        }
        ResolveError::Unmatched { .. } => "this `«` has no `»`: write `\\«` for one as text".into(),
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
            "a record or hole is not read as one value; read a field or take its address"
                .into()
        }
        RunError::NotText => "`lex` takes a string".into(),
        RunError::Output(why) => format!("print could not write to stdout: {why}"),
        RunError::Aborted(message) => message.to_string(),
        RunError::Raised(raised) => format!("{}: {}", raised.0, raised.1),
        RunError::NoTape => "there is no tape behind this receiver".into(),
        RunError::OffTape => "this index is off the tape".into(),
        RunError::MissingKey => {
            "the map holds no value at this key, and a value of its value type cannot be the \
             unknown `?`"
                .into()
        }
        RunError::UnknownMap => {
            "this map is the unknown `?`, so no map stands here to read or write: write one \
             first, `m = hashmap K -> V`"
                .into()
        }
        RunError::NoName => "this cell holds no name".into(),
        RunError::CellNotAType => {
            "this cell was checked to hold a type, but the tape changed since and it holds something else"
                .into()
        }
        RunError::NotAConstant(ty) => format!(
            "this is read as `{ty}`, but it holds no literal or constant of it: \
             while the parse runs, only a constant is read as its value"
        ),
        RunError::CellNotANode => {
            "this cell was checked against a type, but the tape changed since and it holds no value of it"
                .into()
        }
        RunError::NoFragment => "insert takes a tape fragment".into(),
        RunError::ConstructOffPass => "`tape.construct` builds nodes only on the tape a running \
                                       parse was handed, never on a `lex «…»` fragment"
            .into(),
        RunError::ConstructFrom(from) => format!(
            "`tape.construct(from, to)` builds the nodes right of the parse's own, so `from` is 1 \
             or more (here {from})"
        ),
        RunError::NoThis => "no value holds this field here".into(),
        RunError::FieldBeforeStamp => {
            "`tape[0]` still holds the type: `tape[0].type = T` makes it a new node first".into()
        }
        RunError::StampNotANodeType => "`tape[0].type = T` makes a node of a type written \
                                        `type (…)` with a `run` or a `parse` of its own"
            .into(),
        RunError::NoFieldNamed(name) => {
            format!("the type `tape[0]` was made a node of has no field `{name}`")
        }
        RunError::UnfilledField(i) => format!(
            "field {} of this node's type body was never written by its constructor",
            i + 1
        ),
        RunError::BadIndex(k) => format!("an index cannot be negative ({k})"),
        RunError::PastEnd { index, size } => {
            format!("index {index} is past the end ({size} items)")
        }
        RunError::BadCount(n) => format!("an alloc count cannot be negative ({n})"),
        RunError::NotDerefable => "only a scalar or a pointer is read through a pointer".into(),
        RunError::NoLayout(_) => "this type has no field layout to construct".into(),
        RunError::EmptyScope => "a scope with nothing in it has no value".into(),
        RunError::NotAScope(_) => "this node is not a scope".into(),
        RunError::OutOfMemory => "the allocator refused this alloc".into(),
        RunError::NoDestructor(_) => "this place has no destructor to run".into(),
        RunError::NoWholeNumber => "a float that is not a number (NaN) or is infinite has no \
                                    whole number, so `lossy` cannot convert it into an integer \
                                    type"
            .into(),
        RunError::RationalArithmetic => {
            "this arithmetic on `rational_number` values has no value: a division by zero, or a \
             result past what a fraction of two i64s holds"
                .into()
        }
        RunError::ArityMismatch => "a call's argument count does not match its function".into(),
        RunError::CompilerUnavailable => {
            "compile() is not available here (parse-time evaluation runs without the compiler)"
                .into()
        }
        RunError::CompileFailed(msg) => format!("compile() failed: {msg}"),
        RunError::Faulted(msg) => format!("the interpreter stopped inside compiled code: {msg}"),
        RunError::NoLexer => "`lex` can run only where the parser runs it".into(),
        RunError::NoParser => {
            "a `type (…)` in a body, and a line that reads a `dyad` value, are parsed when they \
             run, which needs the parser running"
                .into()
        }
        RunError::MintFailed(rendered) => format!("building this `type (…)` failed:\n{rendered}"),
        RunError::WaitFailed(rendered) => {
            format!(
                "a line that needs a type known only when it runs is parsed then, and here it \
                 failed:\n{rendered}"
            )
        }
        RunError::StoodTaken => {
            "a value the pass ran early was read again after its line ended it or a call took it"
                .into()
        }
        RunError::NotKept => "a value its line holds was read where no line holds it".into(),
        RunError::Lex(why) => format!("this text will not lex: {why}"),
        RunError::NoCaller => {
            "`caller` can be read only inside a constructor the parser runs".into()
        }
        RunError::CallerSpot => {
            "`caller` alone is not a value the seed reads; read `caller.scope`".into()
        }
        RunError::UnsettledInclusion => {
            "`⊆` answers only for a type and itself, between pointer types, or between number \
             types and `bool`; \
             between other types what each can hold is not settled yet"
                .into()
        }
        RunError::NullPointer => "this pointer holds nothing yet".into(),
        RunError::UnionHolds(members) => match &**members {
            (Some(held), read) => format!(
                "this union holds `{held}`, not `{read}`: a member is read only while the union \
                 holds it"
            ),
            (None, read) => format!(
                "this union holds no member yet, so `{read}` is not there to read: write a \
                 member first"
            ),
        },
        RunError::HeldNodeReplaced => {
            "this line was read for the type of what its `dyad` held, but by the time it reached \
             the `dyad` it held a node of another type"
                .into()
        }
        RunError::Parse(e) => parse_message(e),
        RunError::Return(_) => "`return` found no function to leave".into(),
        RunError::TooDeep => {
            format!(
                "nested too deep: the run used up its {} stack; is this recursion ending?",
                work_stack()
            )
        }
    }
}

fn work_stack() -> String {
    format!("{} MiB", crate::stack::WORK_STACK_BYTES / (1024 * 1024))
}

/// Message-only, as [`run_message`].
pub fn compile_message(e: &CompileError) -> String {
    match e {
        CompileError::NotLowerable(_) => "this cannot be compiled yet".into(),
        CompileError::ParsedAtRun => {
            "a line of this function is parsed only when it runs, since a type it needs is known \
             only then, and compiled code cannot parse: the function stays interpreted"
                .into()
        }
        CompileError::Uninitialized => "a value here has no storage to compile against".into(),
        CompileError::NoActivation => "a function's local is compiled outside any call".into(),
        CompileError::NotDerefable => "only a scalar or a pointer is read through a pointer".into(),
        CompileError::NoLayout(_) => "this type has no field layout to construct".into(),
        CompileError::EmptyScope => "a scope with nothing in it has no value".into(),
        CompileError::Internal(what) => format!("the seed broke its own rule: {what}"),
        CompileError::ArityMismatch => "a call's argument count does not match its function".into(),
        CompileError::Cranelift(msg) => format!("the backend rejected this: {msg}"),
        CompileError::TooDeep => {
            format!(
                "this body nests too deep to compile here: the compile used up its {} stack",
                work_stack()
            )
        }
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
