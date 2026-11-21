// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The parser's checked errors; report.rs renders them.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Name resolution failed; the spelling is in the reason.
    Resolve(ResolveError),
    /// A Logos-written constructor failed while running; carries the
    /// rendered run error.
    ConstructorFailed(Box<String>),
    /// An item the pass ran failed (DESIGN ›Build and run are one
    /// self-directing pass‹).
    Run(crate::run::RunError),
    /// A line of a `type (…)` body that neither fills a slot, declares a
    /// field or member, nor is prose.
    TypeBodyLine,
    /// `static` on a parameter or a union's member, which each value holds.
    StaticMisplaced,
    /// `pub` on a parameter, which only its function reads.
    PubParameter,
    /// A value made once at the definition (`static`, `immediate`) read a name that
    /// holds no value there: a parameter or local of the function around it, or a
    /// name declared in the loop or branch around it.
    OnceReadsUnmade,
    /// A field named bare inside a parse, where it goes through `tape[0]`.
    BareFieldInParse(Box<String>),
    /// `tape[0].type = …` anywhere but on a parse's own cell.
    StampOutsideParse,
    /// `tape[0].type = T` for a `T` that gives no type.
    StampNeedsType,
    /// A `static` function that works on a value, called where no value is at hand: `name` as
    /// the call spells it, `member` when that is the name of the member holding it.
    StaticFnNeedsValue { name: Box<String>, member: bool },
    /// A slot word left of `=` where no type is being defined.
    SlotOutsideDefinition,
    /// `t.x` where `x` is a place per node, not stored with the type.
    PerNodeThroughType(String),
    /// A binding in a type body inserted a teardown, which no scope exit runs.
    DeferInTypeBody,
    /// A type body's own declaration failed while running at the definition;
    /// carries the rendered run error.
    TypeBodyFailed(Box<String>),
    /// Scopes nested deeper than the parse's stack holds.
    TooDeep,
    /// The pass needed a type identity and found a place that will only hold
    /// one when the program runs (DESIGN ›A type is a comptime value‹).
    TypeKnownOnlyAtRun,
    /// `=` into an owning place of what owns nothing there: a `&` or a plain
    /// `@T`, or, through a pointer's cell, which takes no copy, a borrowed owner.
    NonOwningIntoOwning,
    /// `dyad (T, v)` for a `T` whose value does not move whole into a new node.
    BadDyadType,
    /// `x := T ?` for a `T` known only when its line runs, whose value does not move whole into
    /// the new node laid out then.
    RunPlaceNotData,
    /// `parse_rank = …` whose value is not a number known at the definition.
    NonComptimeRank,
    /// `associativity = …` with something other than `left` or `right`.
    BadAssociativity,
    /// `lex_rank = …` in a type body that is not a declaration's value: the
    /// rank is the name's, and here there is no name.
    LexRankNeedsName,
    /// `parse = …` or `run = …` with anything but a bracket on its right.
    SlotNeedsBody(SlotKind),
    /// A type with a `run` applied to arguments, `sq(3)`: only its `parse`
    /// fills a node's fields and output, so the call form builds no node.
    RunTypeApplied,
    /// A held run body could not be constructed for a field-type set; carries
    /// the type's spelling at the use and the failure rendered against the
    /// body's text.
    RunBodyFailed { name: String, rendered: String },
    /// `tape[0].f` naming no field declared above; carries `f`.
    NoSuchField(Box<String>),
    /// `v.f` over a value whose type lays out no fields; carries `f`.
    NotAField(Box<String>),
    /// `tape.is_constructed[k] = v` with a `v` that is no bool.
    FlagTakesBool,
    /// An operator lacked a reduced operand on one side.
    MissingOperand,
    /// The tape did not reduce to a single dyad (a dangling operator or operand).
    Trailing,
    /// The input held no expression.
    Empty,
    /// A numeric literal's digits did not parse.
    BadLiteral,
    /// An opener whose closer never came, or a mismatched one (`)` for `[`).
    UnclosedBracket,
    /// A closer where a scope's lines read to the end of the text; carries its spelling.
    StrayCloser(Box<String>),
    /// A construct that requires a `(` (a `record`/parameter list) was not
    /// followed by one.
    ExpectedOpen,
    /// A field list expected a field name where it found neither a name nor `)`.
    ExpectedField,
    /// A fn signature's parameter list was not followed by `->`.
    ExpectedArrow,
    /// A fn signature's `->` was not followed by a return type.
    ExpectedReturnType,
    /// An abstract operator could not resolve a concrete machine op for its
    /// operand types.
    UnsupportedOperands,
    /// An `if` or `while` condition was not a `bool`.
    NonBoolCondition,
    /// An `if` without an `else` where a value is required: with no false
    /// branch it yields unit.
    MissingElse,
    /// An arm of an `if` whose value is used gives nothing, as one ending in `return`, a raise
    /// or `abort`.
    ArmGivesNothing,
    /// The arms of an `if` whose value is used give different types, and no type
    /// written in front of it says which one it gives.
    ArmsDiffer,
    /// One arm of an `if` whose value is used makes its value and another borrows one: who
    /// owns it would show only when it runs.
    ArmsMakeAndBorrow,
    /// A line written after one that always gives its scope a value, which never runs.
    LineAfterValue,
    /// A line written after one that always leaves, a `return`, a raise or an `abort`, which
    /// never runs.
    LineAfterLeave,
    /// A line of a `-> void` function's body gives a value.
    VoidBodyGives,
    /// A line of a `{ … }` gives a value, where the scope itself is the value.
    KeptScopeGives,
    /// A `defer` gives a value, which nothing takes when its scope ends.
    DeferGives,
    /// A value is needed, and some path ends without giving one: a block one of whose lines
    /// gives only on some paths, a loop.
    EndsWithoutValue,
    /// A line gives a name a block holds out of it, and a reader to its right takes the value on
    /// the way, so the name has moved where nothing hands it on.
    HeldTakenOnTheWay,
    /// A line gives a name its block holds as the block's value through a construct the seed
    /// does not move it out of, so the block's end would free it under the value.
    HeldGivenUnmoved,
    /// A logical operator applied to a non-`bool` operand.
    NonBoolOperands,
    /// A value lands in a type that is not its own and is not known to hold every value of it.
    TypeMismatch,
    /// A number literal had no exact value in the type it was committed to.
    UncomputableLiteral,
    /// Arithmetic on plain numbers with no value: a division by zero, or past an `i64` fraction.
    LiteralArithmetic,
    /// `lossy` not between a number type and the value it converts.
    LossyOutOfPlace,
    /// A `return` with no function around it to leave.
    ReturnOutsideFunction,
    /// A `return` in a `defer`, at any depth.
    ReturnInDefer,
    /// A node that gives nothing (a statement, a `-> void` call) stood where a value is
    /// required.
    StatementAsValue,
    /// An assignment target that is not a typed numeric variable: a comptime
    /// binding has no machine storage to write.
    BadAssignTarget,
    /// The target is a bare literal: `x := 5` binds the name to the number
    /// itself, so there is no storage for `x = 6` to write (DESIGN ›Numeric
    /// literals are uncommitted‹).
    /// Carries the literal's spelling.
    AssignToLiteral(Box<String>),
    /// A gate word (`pub`) where a line's name stands, left of `:=` or of a slot fill.
    GateNeedsValue,
    /// A value was gated twice by one word (`x := pub pub …`).
    DoubleGate,
    /// `x = …` on a name not declared `mut`; carries the name.
    NotMutable(Box<String>),
    /// `x:lex_rank = …` written outside the scope that declares `x`; carries the name.
    LexOutsideScope(Box<String>),
    /// A read of a name declared `T ?` before a sibling write filled it; carries the name.
    Unwritten(Box<String>),
    /// An `import` was not followed by the `file «…»` it reads.
    ImportTakesFile,
    /// A `file «…»` address holding a `{…}`: an address computed when the program runs.
    AddressNotPlain,
    /// A file read from inside a project names a file outside its project's folder.
    FileOutsideProject {
        /// The path as written.
        path: String,
        /// The project's folder.
        root: String,
    },
    /// `.name` over a file's scope where the file has no `pub` name `name`; carries the name.
    NoPubName(Box<String>),
    /// `.name` from outside the scope `name` is declared in, a file's, a type's or a `{ … }`'s,
    /// where it is no `pub` name; carries the name.
    NotPub(Box<String>),
    /// A `regex` was not followed by a `«…»` quote.
    ExpectedPattern,
    /// A `lex`, `print`, `abort` or a category's `:=` was not followed by a `«…»` quote;
    /// carries the word.
    ExpectedQuote(&'static str),
    /// `error` with something to its right that names no category.
    ErrorNeedsCategory,
    /// A raise over a category no open scope declares; carries the category as written.
    UnknownCategory(Box<String>),
    /// `error.X :=` over a category in reach; carries the category.
    CategoryDeclared(Box<String>),
    /// A category standing where a value is read; carries the category.
    CategoryAlone(Box<String>),
    /// A category's path written with space inside it; carries the category.
    CategoryApart(Box<String>),
    /// `lex «» := …`: a spelling of no text, which no lex could ever match.
    EmptySpelling,
    /// A `}` in a `print` quote with no `{` before it.
    StrayInterpolationClose,
    /// `tape.insert(k, …)` was handed something that is not a tape: `insert`
    /// splices a tape into a tape (DESIGN ›Text is the quote‹).
    InsertTakesTape,
    /// `hashmap` not followed by `K -> V`, or a key or value type whose values are
    /// not one word read by value.
    HashmapShape,
    /// `hashmap K -> V { … }` with a key type other than `string`: each key is a name's spelling.
    ScopeKeysAreText,
    /// A pattern declared in the `{ … }` a `hashmap` reads, which stands for many spellings and so
    /// is no key; carries its pattern.
    PatternIsNoKey(Box<String>),
    /// A constructor edited the tape and returned with its own cell still
    /// unconstructed and still its own identity: neither a decline nor a
    /// construction. Carries the cell's spelling.
    CellLeftUnconstructed(Box<String>),
    /// The pattern a `regex «…»` quotes does not compile, or is empty;
    /// reported at the definition, since the index compiles a branch only on
    /// first lookup.
    BadPattern(String),
    /// A `file «…»` could not be read: the joined path and why.
    FileRead(String),
    /// The named file is already loading: the graph of files reaching files must be a DAG.
    ImportCycle(String),
    ImportFailed {
        /// The path as written after `file`.
        path: String,
        /// The fully rendered inner report (file:line:col, caret and all).
        rendered: String,
    },
    /// A reflection read that does not fit the node's type, an unknown member
    /// on a view, or a read whose honest answer is undefined (a null
    /// constructor slot).
    BadReflectRead,
    /// `op.mint(T, …)` over field types `op`'s chooser makes no mint for.
    NoMint,
    /// A collection member (`.roles`) without its `[index]`: the
    /// bare collection as a first-class value waits for the array type.
    ExpectedIndexBracket,
    /// `x:dyad` handed on whole for a name whose bytes lie in a frame, with no node of their own.
    NoNodeYet,
    /// `⊆` over two different types that are not both integer types.
    UnsettledInclusion,
    /// A record construction's argument count did not match its field count.
    CtorArity,
    /// A `for` followed by a fresh spelling and then not by `in`: a fresh
    /// spelling can only be the loop variable.
    ExpectedIn,
    /// A `for`'s range was malformed: a missing `..`, or a range part that is
    /// not a primary (a bare full expression would consume the body's `(`).
    ExpectedRange,
    /// A `for`'s literal step was not positive: with the end-exclusive
    /// condition it could never terminate.
    BadStep,
    /// An `&` of something without storage to point at.
    BadAddressOf,
    /// A numeric conversion `T(value)` with not exactly one operand.
    BadCast,
    /// A declaration's type position, or a type variable's fill, held
    /// something that does not evaluate to a type.
    BadDeclaredType,
    /// A new name or parameter without its type: `x := ?`, `fn (a)`, `fn (a := ?)`.
    NameNeedsType,
    /// A field of a type without its type: `a := ?`, or `a` alone on its line.
    FieldNeedsType,
    /// A value lands in a `dyad ?` place that it can neither be held as nor moved whole into a
    /// new node: one that is no node where nothing holds the place, or one with an end of its own
    /// that a parameter would only borrow.
    DyadTakesNode,
    /// A line that waits for the run to parse it holds `:=`, `return`, `free` or `move`, whose
    /// checks the parse makes where they are written; carries the word.
    WaitingLineHolds(Box<String>),
    /// A `while` condition reads the node a call that gives a `dyad` gives where the scan that
    /// holds such a condition before it is parsed does not see it: through a bracket or a
    /// member's call.
    DyadReadEachPass,
    /// A point where the parse needs a value (a box read, a call for a type, a `-> dyad` call an
    /// operator reads, an import) stands after something its own line runs that the pass cannot
    /// run first: what stands before a branch, or a node whose operation is not built yet.
    RunsBeforeUnbuilt,
    /// A typed declaration of a non-numeric type: the declared-type storage
    /// for those is not in the seed yet.
    NonNumericDeclaredType,
    /// A `-> type` call could not be resolved at parse time: its arguments
    /// were not comptime-known, or it did not yield a type.
    NonComptimeTypeCall,
    /// An `immediate` whose value the graph cannot hold: a record or a run-time
    /// rational, which live in the run's frame, or a pointer.
    ImmediateNotHeld,
    /// A nested function reached a local or parameter of an enclosing
    /// function: a closure capture, which v1 does not support; it would read
    /// the wrong frame at run time.
    CapturedLocal,
    /// A value with an end passed to a call whose parameter cannot hold it
    /// (`drop_model::param_holds`), so nothing would end it.
    UnboundOwningValue,
    /// A scope's value or a `return` is `&x` of a name a scope it leaves holds: that
    /// scope's end frees the value under the address.
    AddressOfHeld,
    /// `move b` where `b` borrows the value with an end it names: only the owner can move it.
    MoveOfBorrow,
    /// `move p.f` or `move p@` of a field or cell that owns nothing. stand-in for #66
    MoveOfUnownedPath,
    /// `move` or `free` of a field the parse cannot follow back to a name that holds it:
    /// through a pointer, an element, a value no name holds or a borrow, or of a `static`
    /// function's own value.
    UnfollowedPath,
    /// `move` or `free` of a cell reached through something that only borrows it: a pointer
    /// `&` made, a copy of an owner or a parameter without `own`, or a name that borrows the node
    /// whose pointer reaches it.
    EndThroughBorrow,
    /// `move` or `free` of a field path under a holder whose type fills its own `free`, which
    /// frees the field with the rest: the path, then that holder, each spelled.
    EndUnderFree(Box<(String, String)>),
    /// `own` before a type that is no plain pointer.
    OwnNeedsPointer,
    /// `own` before anything but a hole or a function's result: the word names a state, the
    /// act is `move`.
    OwnOutsideType,
    /// A function hands on a value made in it, as its last value or through a `return`, and its
    /// result, a pointer spelled as written here, lacks `own`.
    HandsOnMade(String),
    /// A function whose result says `-> own @T` hands on a borrow, as its last value or through a
    /// `return`.
    OwnResultBorrows,
    /// A `return` in a type's `run` hands on a borrowed pointer, where the `run`'s last value is
    /// a pointer made there and so marks it to hand on what it makes.
    RunReturnBorrows,
    /// `w = …` in a type body where `w` is no slot and nothing declared.
    NoSuchSlot(String),
    /// A type body whose fields, named here, own what they hold, and no `free = (…)` to
    /// free it.
    FieldsNeedFree(Vec<String>),
    /// A name of a `{ … }` whose value its scope's end runs a teardown for, or that does not
    /// move whole into a field; carries the name.
    KeptEntry(Box<String>),
    /// A name written above a `fn` of a `{ … }` that reads the value ends before the `}`, so the
    /// value lays out no field where the `fn` reads it.
    KeptReadEnded { name: Box<String>, reader: Box<String> },
    /// `own @T ?` on a parameter, which would consume the caller's argument.
    OwnParameterNotInSeed,
    /// `union` with no `( … )` after it, or with no member in it.
    UnionNeedsMembers,
    /// A union of more members than its tag tells apart.
    UnionTooManyMembers,
    /// A line of a `union ( … )` that is no member: a `static`, a slot fill or a default.
    UnionLine,
    /// `pub` before a union's member, which every scope reads.
    UnionMemberPub,
    /// A member of a union, named here, whose value does not move whole into a place.
    UnionMemberUnkept(Box<String>),
    /// A union type applied to values, as a type with fields is.
    UnionApplied,
    /// `holds` after a value that is no union.
    HoldsNeedsUnion,
    /// `holds` with no member's name after it.
    HoldsNeedsMember,
    /// A name after `holds` that is no member of the union, and the union's members.
    NotAMember(Box<(String, Vec<String>)>),
    /// A `move` or `free` inside a loop or `fn` body names a place declared
    /// outside it: the loop would read a dead name on its next pass, and a
    /// function may own only what its parameters hand it (DESIGN ›`move` and `free` are
    /// static: the parse marks the name dead‹).
    MoveOfOuterName,
    /// `move` of a value no name holds: there is no place to move it out of.
    MoveOfValue,
    /// `free` of a hole, which holds no value.
    FreeOfHole,
    /// `move` or `free` of a name the run starts with (`i32`), which every section reads;
    /// carries the name.
    EndsPrimordialName(Box<String>),
}

impl ParseError {
    /// A refusal from `Parser::type_known_now` of an operand that does name a type, which only
    /// running gives: the program's run, or that of a part of its line that cannot run first.
    pub(crate) fn type_given_by_running(&self) -> bool {
        matches!(self, ParseError::TypeKnownOnlyAtRun | ParseError::RunsBeforeUnbuilt)
    }
}
