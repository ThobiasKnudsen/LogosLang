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
    /// `share` on a parameter, or anywhere but at the start of a type body's line.
    ShareMisplaced,
    /// `parse = (…)` without `share`: a slot is stored once per type.
    SlotFillNeedsShare,
    /// A value made once at the definition (`share`, `immediate`) read a name that
    /// holds no value there: a parameter or local of the function around it, or a
    /// name declared in the loop or branch around it.
    OnceReadsUnmade,
    /// `share` not followed by a `name := value` declaration or a slot fill.
    ShareNeedsDeclaration,
    /// A field named bare inside a parse, where it goes through `tape[0]`.
    BareFieldInParse(Box<String>),
    /// `tape[0]:type = …` anywhere but on a parse's own cell.
    StampOutsideParse,
    /// `tape[0]:type = T` for a `T` other than the type being defined.
    StampOtherType,
    /// A `share` function that works on a value, called bare where no value is at hand.
    ShareFnNeedsValue(Box<String>),
    /// A slot word left of `=` where no type is being defined.
    SlotOutsideDefinition,
    /// `t.x` where `x` is a place per node, not stored with the type.
    PerNodeThroughType(String),
    /// A binding in a type body inserted a teardown, which no scope exit runs.
    DeferInTypeBody,
    /// A type body's own declaration failed while running at the definition;
    /// carries the rendered run error.
    TypeBodyFailed(Box<String>),
    /// Scopes nested deeper than `MAX_BRACKET_DEPTH`.
    TooDeep,
    /// The pass needed a type identity and found a place that will only hold
    /// one when the program runs (DESIGN ›A type is a comptime value‹).
    TypeKnownOnlyAtRun,
    /// `=` into an owning place whose right side does not own: two teardowns
    /// over one block, or one over memory the store owns.
    NonOwningIntoOwning,
    /// `dyad (T, v)` for a `T` whose values are bytes at a width the seed
    /// cannot fill from a value node.
    BadDyadType,
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
    /// A logical operator applied to a non-`bool` operand.
    NonBoolOperands,
    /// A binary operator's operands were two different concrete numeric
    /// types: cross-type arithmetic needs an explicit cast.
    TypeMismatch,
    /// A number literal had no exact value in the type it was committed to.
    UncomputableLiteral,
    /// A `return` before the tail with no function around it to leave.
    EarlyReturn,
    /// A unit-valued statement (a `while` loop) stood where a value is required.
    StatementAsValue,
    /// An assignment target that is not a typed numeric variable: a comptime
    /// binding has no machine storage to write.
    BadAssignTarget,
    /// The target is a bare literal: `x := 5` binds the name to the number
    /// itself, so there is no storage for `x = 6` to write (DESIGN ›Numeric
    /// literals are uncommitted‹).
    /// Carries the literal's spelling.
    AssignToLiteral(Box<String>),
    /// A gate word (`pub`) not followed by a declaration: a gate fills a
    /// declare node's gate slot.
    GateNeedsDeclaration,
    /// A declaration was gated twice (`pub pub x := …`).
    DoubleGate,
    /// `x = …` on a name not declared `mut`; carries the name.
    NotMutable(Box<String>),
    /// A write into a name or field declared `immut`; carries the name.
    Immutable(Box<String>),
    /// A read of a name declared `T ?` before a sibling write filled it; carries the name.
    Unwritten(Box<String>),
    /// An `import` was not followed by a path token.
    ExpectedPath,
    /// A `regex` was not followed by a `«…»` quote.
    ExpectedPattern,
    /// A `lex`, `print` or `error` was not followed by a `«…»` quote; carries the word.
    ExpectedQuote(&'static str),
    /// A `{` in a `print` quote with no `}` after it.
    UnclosedInterpolation,
    /// A `}` in a `print` quote with no `{` before it.
    StrayInterpolationClose,
    /// `tape.insert(k, …)` was handed something that is not a tape: `insert`
    /// splices a tape into a tape (DESIGN ›Text is the quote‹).
    InsertTakesTape,
    /// `hashmap` not followed by `K -> V`, or a key or value type whose values are
    /// not one word read by value.
    HashmapShape,
    /// A constructor edited the tape and returned with its own cell still
    /// unconstructed and still its own identity: neither a decline nor a
    /// construction. Carries the cell's spelling.
    CellLeftUnconstructed(Box<String>),
    /// The pattern a `regex «…»` quotes does not compile, or is empty;
    /// reported at the definition, since the index compiles a branch only on
    /// first lookup.
    BadPattern(String),
    /// An `import` inside a deferred-or-repeated body: the load happens once,
    /// at parse, so `import` belongs where parse order and run order coincide.
    ImportInRuntimeBody,
    /// The imported file could not be read: the joined path and the OS error.
    ImportRead(String),
    /// The named file is already loading: the import graph must be a DAG.
    ImportCycle(String),
    ImportFailed {
        /// The path as written at the import site.
        path: String,
        /// The fully rendered inner report (file:line:col, caret and all).
        rendered: String,
    },
    /// A reflection read that does not fit the node's type, an unknown member
    /// on a view, or a read whose honest answer is undefined (a null
    /// constructor slot).
    BadReflectRead,
    /// A collection member (`.operands`, `.roles`) without its `[index]`: the
    /// bare collection as a first-class value waits for the array type.
    ExpectedIndexBracket,
    /// `.type` on something that is not a dyad: a value's type is never one of
    /// its own fields; `:` reads it, `x:type`.
    TypeIsColonRead,
    /// `:dyad` or `:value`: nothing reaches a value's cell as a whole.
    CellNotReachable,
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
    /// A numeric conversion `logos(value)` with not exactly one operand, or a
    /// non-numeric one.
    BadCast,
    /// A declaration's type position, or a type variable's fill, held
    /// something that does not evaluate to a type.
    BadDeclaredType,
    /// A typed declaration of a non-numeric type: the declared-type storage
    /// for those is not in the seed yet.
    NonNumericDeclaredType,
    /// A `-> logos` call could not be resolved at parse time: its arguments
    /// were not comptime-known, or it did not yield a type.
    NonComptimeTypeCall,
    /// An `immediate` whose value the graph cannot hold: a record or a run-time
    /// rational, which live in the run's frame, or a pointer.
    ImmediateNotHeld,
    /// A nested function reached a local or parameter of an enclosing
    /// function: a closure capture, which v1 does not support; it would read
    /// the wrong frame at run time.
    CapturedLocal,
    /// An owning value stood where no name binds it: the teardown attaches at
    /// the binding site (DESIGN ›Holding is decided at the binding site‹),
    /// so it would leak. Fail-closed until ownership-gated parameters.
    UnboundOwningValue,
    /// A `return` hands out a place a scope it leaves owns: the teardowns run on
    /// the way out, so the value would already be freed. A last value moves out instead.
    OwningEscape,
    /// A `return` hands out an owning value, which only a last value moves out to the caller.
    OwnershipAcrossReturn,
    /// `move b` where `b` borrows the node it names: only the owner can move it.
    MoveOfBorrow,
    /// A name as a line of a list a type's `parse` builds from, where the value's type fills
    /// a `free`: the built value owns its lines, so the name must be moved in.
    LineNotMoved,
    /// `own` in a type position over something other than a pointer hole, `own @T ?`.
    OwnNeedsPointer,
    /// `own` before anything but a hole: the word names a state, the act is `move`.
    OwnOutsideType,
    /// `share w = …` in a type body where `w` is no slot and nothing declared.
    NoSuchSlot(String),
    /// A type body with an `own` field and no `free = (…)` to free it.
    OwningFieldNeedsFree,
    /// `own @T ?` on a parameter, which would consume the caller's argument.
    OwnParameterNotInSeed,
    /// A `move` or `free` inside a loop or `fn` body names a place declared
    /// outside it: the loop would read a dead name on its next pass, and a
    /// function may own only what its parameters hand it (DESIGN ›`move` and `free` are
    /// static: the parse marks the name dead‹).
    MoveOfOuterName,
}
