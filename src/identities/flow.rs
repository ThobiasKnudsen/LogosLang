// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! What a line gives the scope it stands in, read where the program is written and run when it
//! runs: ›A scope's value is what it evaluates to, and `return` is an optional early exit from the
//! enclosing function‹ [[R207]]. A scope gives the value of the first of its lines that gives
//! one, and a value passes out through a block, an `if`'s arm and a loop's body standing as a
//! line.

use super::{array, numtype, read, scope};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::ParseError;
use crate::run::{RunError, Runtime};
use crate::Core;

/// How a line ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Flow {
    /// Some path through it gives its scope a value.
    pub gives: bool,
    /// Some path through it ends with no value and goes on to the next line.
    pub falls: bool,
}

impl Flow {
    pub(crate) const STATEMENT: Flow = Flow { gives: false, falls: true };
    pub(crate) const VALUE: Flow = Flow { gives: true, falls: false };
    /// A `return` or an `error`: every path leaves.
    pub(crate) const LEAVES: Flow = Flow { gives: false, falls: false };

    /// Every path ends the scope, so a line written after it never runs.
    pub(crate) fn ends_scope(self) -> bool {
        !self.falls
    }

    /// The scope's value where the scope is used as one: refused where a path gives none.
    fn as_value(self) -> Result<(), ParseError> {
        match self {
            Flow { gives: false, .. } => Err(ParseError::StatementAsValue),
            Flow { falls: true, .. } => Err(ParseError::EndsWithoutValue),
            _ => Ok(()),
        }
    }
}

/// Told each slot of a node that gives the value as itself, in line order, once each.
pub(crate) trait Given {
    fn value(&mut self, slot: *mut DyadPtr) -> Result<(), ParseError>;
}

impl<F: FnMut(*mut DyadPtr) -> Result<(), ParseError>> Given for F {
    fn value(&mut self, slot: *mut DyadPtr) -> Result<(), ParseError> {
        self(slot)
    }
}

/// How `node` ends as a line of a scope.
///
/// # Safety
/// `node` must be null or a reduced dyad from the store.
pub(crate) unsafe fn of(types: &Core, node: DyadPtr) -> Flow {
    let mut node = node;
    walk(types, &mut node, &mut |_| Ok(())).unwrap_or(Flow::STATEMENT)
}

/// How the node in `slot` ends as a line, `given` told every node that gives its value. A
/// name stands for a value, never for lines, so a value passes out only through the nodes
/// written there.
///
/// # Safety
/// `slot` must hold null or a reduced dyad from the store, and stay valid, as every slot below
/// it, while the walk runs.
pub(crate) unsafe fn walk(
    types: &Core,
    slot: *mut DyadPtr,
    given: &mut dyn Given,
) -> Result<Flow, ParseError> {
    let node = *slot;
    if node.is_null() || is_prose(types, node) {
        return Ok(Flow::STATEMENT);
    }
    let ty = dyad::ty(node);
    if ty == types.return_ || ty == types.error.error {
        return Ok(Flow::LEAVES);
    }
    match passing(types, node) {
        Some(Passing::Lines(lines)) => {
            let mut gives = false;
            for line in line_slots(lines) {
                let flow = walk(types, line, given)?;
                gives |= flow.gives;
                if flow.ends_scope() {
                    return Ok(Flow { gives, falls: false });
                }
            }
            Ok(Flow { gives, falls: true })
        }
        Some(Passing::Arms(then, els)) => {
            let then = walk(types, then, given)?;
            let els = match els {
                Some(els) => walk(types, els, given)?,
                None => Flow::STATEMENT,
            };
            Ok(Flow { gives: then.gives || els.gives, falls: then.falls || els.falls })
        }
        Some(Passing::Body(body)) => {
            let body = walk(types, body, given)?;
            Ok(Flow { gives: body.gives, falls: true })
        }
        None if read::output_type(types, node) == types.void_ => Ok(Flow::STATEMENT),
        None => {
            given.value(slot)?;
            Ok(Flow::VALUE)
        }
    }
}

/// Prose, and a `defer`, which runs at the scope's end: no line of the scope's run.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn is_prose(types: &Core, node: DyadPtr) -> bool {
    let ty = dyad::ty(node);
    numtype::is_comment_type(ty) || ty == types.defer_
}

/// Where a node a value passes out through holds what gives it.
enum Passing {
    /// A block's lines, an `array`; null where it has none.
    Lines(DyadPtr),
    /// An `if`'s arms; `None` for an `else` it does not have.
    Arms(*mut DyadPtr, Option<*mut DyadPtr>),
    /// A loop's body, which may end without running.
    Body(*mut DyadPtr),
}

type RunGiven = unsafe fn(&mut Runtime, DyadPtr) -> Result<Option<i64>, RunError>;
type LowerGiven = unsafe fn(&mut Lowerer, DyadPtr) -> Result<(), CompileError>;

/// A type whose nodes a value passes out through: where a node holds what gives it, and how it
/// runs and lowers as a line, handing that value on.
struct Passer {
    shape: unsafe fn(DyadPtr) -> Passing,
    run: RunGiven,
    lower: LowerGiven,
}

fn passer(types: &Core, ty: DyadPtr) -> Option<Passer> {
    unsafe fn lines(node: DyadPtr) -> Passing {
        Passing::Lines(scope::exprs_array(node))
    }
    unsafe fn arms(node: DyadPtr) -> Passing {
        let (then, els) = super::if_mod::arm_slots(node);
        Passing::Arms(then, (!(*els).is_null()).then_some(els))
    }
    unsafe fn for_body(node: DyadPtr) -> Passing {
        Passing::Body((dyad::value(node) as *mut DyadPtr).add(super::for_mod::BODY))
    }
    unsafe fn while_body(node: DyadPtr) -> Passing {
        Passing::Body((dyad::value(node) as *mut DyadPtr).add(super::while_mod::BODY))
    }
    let (shape, run, lower): (unsafe fn(DyadPtr) -> Passing, RunGiven, LowerGiven) =
        if ty == types.scope {
            (lines, scope::run_given, scope::lower_given)
        } else if ty == types.if_ {
            (arms, super::if_mod::run_given, super::if_mod::lower_given)
        } else if ty == types.for_ {
            (for_body, super::for_mod::run_given, super::for_mod::lower_given)
        } else if ty == types.while_ {
            (while_body, super::while_mod::run_given, super::while_mod::lower_given)
        } else {
            return None;
        };
    Some(Passer { shape, run, lower })
}

/// # Safety
/// `node` must be a reduced dyad from the store.
unsafe fn passing(types: &Core, node: DyadPtr) -> Option<Passing> {
    passer(types, dyad::ty(node)).map(|p| (p.shape)(node))
}

/// # Safety
/// `lines` must be null or an `array` node from the store, unchanged while the slots are read.
unsafe fn line_slots(lines: DyadPtr) -> impl Iterator<Item = *mut DyadPtr> {
    let (len, data) = if lines.is_null() { (0, std::ptr::null()) } else { array::parts(lines) };
    (0..len).map(move |i| data.cast_mut().add(i))
}

/// The lines of `lines` a value passes out of, up to the one that ends the scope: each that
/// gives on some path, and the one that ends it, as a `return`.
///
/// # Safety
/// `lines` must be null or an `array` node of reduced dyads from the store.
pub(crate) unsafe fn giving_lines(types: &Core, lines: DyadPtr) -> Vec<DyadPtr> {
    let mut giving = Vec::new();
    for line in line_slots(lines) {
        let flow = of(types, *line);
        if flow.gives || flow.ends_scope() {
            giving.push(*line);
        }
        if flow.ends_scope() {
            break;
        }
    }
    giving
}

/// The type of the value a block or an `if` gives where it is used as one, or why it gives
/// none: an `if` needs its `else` and each arm a value, and a block a value on every path that
/// does not leave. Every node that gives it gives one type, and every one makes its value or
/// every one borrows one, as `if` arms do (›`if` reads its own right side‹).
///
/// # Safety
/// `node` must be a scope or `if` node from the store.
pub(crate) unsafe fn value_type(types: &Core, node: DyadPtr) -> Result<DyadPtr, ParseError> {
    value_read(types, node).map(|v| v.ty)
}

/// A value as one walk reads it, each node it passes through read once, so a chain of `else if`
/// arms reads in time in step with its length (›Reading a program takes time in step with its
/// size; no walk runs once per path‹).
#[derive(Clone, Copy)]
struct ValueRead {
    ty: DyadPtr,
    /// Whether it is made (`Some(true)`) or borrowed, as
    /// [`super::drop_model::handed_ends`] reads it.
    made: Option<bool>,
}

/// # Safety
/// `node` must be a scope or `if` node from the store.
unsafe fn value_read(types: &Core, node: DyadPtr) -> Result<ValueRead, ParseError> {
    if dyad::ty(node) == types.if_ {
        let (then, els) = super::if_mod::arm_slots(node);
        if (*els).is_null() {
            return Err(ParseError::MissingElse);
        }
        // An arm that gives nothing makes the `if` give nothing, whichever arm it is, so a
        // sibling arm's two types are no use of a value.
        let mut arms = Vec::with_capacity(2);
        for arm in [*then, *els] {
            match arm_read(types, arm) {
                Ok(v) => arms.push(v),
                Err(ParseError::StatementAsValue) => return Err(ParseError::ArmGivesNothing),
                Err(ParseError::ArmsDiffer) => {}
                Err(e) => return Err(e),
            }
        }
        return match arms[..] {
            [a, b] if super::same_type(a.ty, b.ty) => {
                let made = super::drop_model::arms_end_alike(a.made, b.made)?;
                Ok(ValueRead { made, ..a })
            }
            _ => Err(ParseError::ArmsDiffer),
        };
    }
    let (flow, read) = given_read(types, node)?;
    flow.as_value()?;
    Ok(read.expect("a block that gives on some path has a node that gives"))
}

/// What a line gives where nothing lands it, at a program's or a file's top level: every node
/// that gives its value one type, made alike or borrowed alike, as a block's.
///
/// # Safety
/// `line` must be a reduced dyad from the store.
pub(crate) unsafe fn check_line_value(types: &Core, line: DyadPtr) -> Result<(), ParseError> {
    given_read(types, line).map(|_| ())
}

/// How `node` ends as a line, and the value its nodes that give one give together, `None` where
/// none does.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
unsafe fn given_read(types: &Core, node: DyadPtr) -> Result<(Flow, Option<ValueRead>), ParseError> {
    let mut seen: Option<ValueRead> = None;
    let mut differ = false;
    let mut node = node;
    let flow = walk(types, &mut node, &mut |slot: *mut DyadPtr| {
        let leaf = arm_read(types, *slot)?;
        seen = Some(match seen {
            Some(v) if !super::same_type(v.ty, leaf.ty) => {
                differ = true;
                v
            }
            Some(v) => {
                ValueRead { made: super::drop_model::arms_end_alike(v.made, leaf.made)?, ..v }
            }
            None => leaf,
        });
        Ok(())
    })?;
    if differ {
        return Err(ParseError::ArmsDiffer);
    }
    Ok((flow, seen))
}

/// A node that gives a value as itself, or an arm of an `if` used as a value, read as a value: a
/// `return` or an `error` gives none.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
unsafe fn arm_read(types: &Core, node: DyadPtr) -> Result<ValueRead, ParseError> {
    if !node.is_null() && types.storage_type(node).is_none() {
        let ty = dyad::ty(node);
        if ty == types.if_ || ty == types.scope {
            return value_read(types, node);
        }
        if ty == types.return_ || ty == types.error.error {
            return Err(ParseError::StatementAsValue);
        }
    }
    let ty = read::value_type(types, node)?;
    Ok(ValueRead { ty, made: super::drop_model::handed_ends(types, node)? })
}

/// Writes over each node that gives the value of the node in `slot` what `leaf` makes of it.
///
/// # Safety
/// As [`walk`].
pub(crate) unsafe fn rewrite_given(
    types: &Core,
    slot: *mut DyadPtr,
    leaf: &mut dyn FnMut(DyadPtr) -> Result<DyadPtr, ParseError>,
) -> Result<Flow, ParseError> {
    walk(types, slot, &mut |at: *mut DyadPtr| {
        *at = leaf(*at)?;
        Ok(())
    })
}

/// The first node that gives `node`'s value as itself, which the echo reads the value's type
/// from; `node` itself where none does.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn first_given(types: &Core, node: DyadPtr) -> DyadPtr {
    let mut first = None;
    let mut at = node;
    let _ = walk(types, &mut at, &mut |slot: *mut DyadPtr| {
        first.get_or_insert(*slot);
        Ok(())
    });
    first.unwrap_or(node)
}

/// Whether a `return` stands in a line of `node` that runs, or in one of its arms or bodies.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn holds_return(types: &Core, node: DyadPtr) -> bool {
    if node.is_null() {
        return false;
    }
    if dyad::ty(node) == types.return_ {
        return true;
    }
    match passing(types, node) {
        Some(Passing::Lines(lines)) => line_slots(lines).any(|line| holds_return(types, *line)),
        Some(Passing::Arms(then, els)) => {
            holds_return(types, *then) || els.is_some_and(|els| holds_return(types, *els))
        }
        Some(Passing::Body(body)) => holds_return(types, *body),
        None => false,
    }
}

/// Runs `node` as a line: the value it gives its scope, `None` where it goes on without one.
/// A block, an `if` and a loop give what a line inside them gave; any other node its own value
/// where it has one.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn run_line(rt: &mut Runtime, node: DyadPtr) -> Result<Option<i64>, RunError> {
    let types = rt.types();
    if node.is_null() || is_prose(types, node) {
        return Ok(None);
    }
    match passer(types, dyad::ty(node)) {
        Some(p) => rt.run_given(node, p.run),
        None => {
            let gives = read::output_type(types, node) != types.void_;
            rt.run(node).map(|v| gives.then_some(v))
        }
    }
}

/// [`run_line`]'s lowering: the value `node` gives passes out to the innermost target. A block,
/// an `if` or a loop whose value a line holds to its end is not lowered, its value passing out.
///
/// # Safety
/// `node` must be null or a reduced dyad from the store.
pub(crate) unsafe fn lower_line(lw: &mut Lowerer, node: DyadPtr) -> Result<(), CompileError> {
    let types = lw.types();
    if node.is_null() || is_prose(types, node) {
        return Ok(());
    }
    match passer(types, dyad::ty(node)) {
        Some(_) if lw.planned(node) => Err(CompileError::NotLowerable(node)),
        Some(p) => (p.lower)(lw, node),
        None => {
            let gives = read::output_type(types, node) != types.void_;
            let value = lw.lower(node)?;
            if gives {
                lw.give(value)?;
            }
            Ok(())
        }
    }
}
