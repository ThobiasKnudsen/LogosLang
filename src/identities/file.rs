// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `file «path»`: a file on this computer, reached by its address written as text and read in
//! the pass. Its node holds the path as written and the file's bytes, and is given by its
//! address, so using it runs nothing; a word in front reads it.
//! ›`file «path»` and `url «address»` reach a file; `import` sets its `pub` names where it stands‹
//! [[R375]]

use super::{meta, string, Cx};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::store::Store;
use crate::Core;

/// At the rank of a word that reads the text to its right at discovery.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::IMPORT,
        Assoc::Left,
        &["path", "bytes"],
    );
    let file = cx.store.alloc_head(cx.type_, record);
    cx.declare("file", file);
    cx.metas.insert(file, |p, _id, tape| p.construct_file(tape));
    file
}

/// `[path, bytes]`, each a text node.
pub(crate) fn build(store: &mut Store, types: &Core, path: &str, bytes: &[u8]) -> DyadPtr {
    let path = string::build_text(store, types.string_, path.as_bytes());
    let bytes = string::build_text(store, types.string_, bytes);
    store.alloc_words(types.file, &[path, bytes])
}

/// The file's bytes as the pass read them.
///
/// # Safety
/// `node` must be a `file` node from the store.
pub(crate) unsafe fn bytes<'n>(node: DyadPtr) -> &'n [u8] {
    string::text(*(dyad::value(node) as *const DyadPtr).add(1))
}
