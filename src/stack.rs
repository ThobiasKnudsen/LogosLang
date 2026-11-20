// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The work stack: the thread every Logos parse and run is on, and the one guard that turns
//! its running out into a checked error. The parser recurses per bracket and the interpreter
//! per node, so what bounds them is the stack left, never a count of calls or brackets.

use std::cell::Cell;

/// The stack of the thread [`on_work_stack`] starts.
pub const WORK_STACK_BYTES: usize = 64 * 1024 * 1024;

/// Kept free under the limit: what the thread's start put above the first frame the work
/// runs in, and what the deepest stretch of the seed between two checks uses.
const RESERVE: usize = 1024 * 1024;

thread_local! {
    static LIMIT: Cell<usize> = const { Cell::new(0) };
}

/// Runs `f` on a new thread of [`WORK_STACK_BYTES`], `thread`'s name kept, whose limit is
/// set; waits for it.
pub fn on_work_stack<T: Send + 'static>(
    thread: std::thread::Builder,
    f: impl FnOnce() -> T + Send + 'static,
) -> std::thread::Result<T> {
    thread
        .stack_size(WORK_STACK_BYTES)
        .spawn(|| {
            LIMIT.set(here() - WORK_STACK_BYTES + RESERVE);
            f()
        })
        .expect("the work thread must start")
        .join()
}

/// The lowest address this thread's stack may reach before a parse or a run faults; 0, no
/// limit, on a thread [`on_work_stack`] did not start.
pub fn limit() -> usize {
    LIMIT.get()
}

/// An address in the caller's frame. The stack grows down on every target the seed builds
/// for, so a lower address is a deeper stack.
#[inline(always)]
pub fn here() -> usize {
    let marker = std::mem::MaybeUninit::<u8>::uninit();
    marker.as_ptr() as usize
}

/// Whether the caller's frame lies under `limit`.
#[inline(always)]
pub fn exhausted(limit: usize) -> bool {
    here() < limit
}
