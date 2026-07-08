//! `fn`: the type whose values are functions. `run` recognizes a function by its
//! type being this. v1 `fn` carries no fields yet: its instances' behaviour lives
//! in the run/compile tables, and a body/input/output layout arrives with
//! compound user functions.

use crate::dyad::DyadPtr;
use crate::store::Store;

/// Create the `fn` type (its own type is `type`) and return it.
pub(super) fn register(store: &mut Store, type_: DyadPtr) -> DyadPtr {
    store.alloc_raw(type_, std::ptr::null_mut())
}
