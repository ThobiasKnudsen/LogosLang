// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::dyad;

/// A binding dyad for `identity`, leaked like the dyads below.
/// A name of the program frame holding the tape's handle, as the parser lays a
/// `parsing_tape` place out.
fn tape_name(store: &mut Store, core: &Core, tape: *mut ParsingTape) -> DyadPtr {
    let offset = store.arena_alloc(8);
    // SAFETY: the eight arena bytes were just taken.
    unsafe { std::ptr::write_unaligned(store.arena_at(offset) as *mut u64, tape as usize as u64) };
    let rec = Binding::alloc(
        store,
        core.binding_,
        Binding::new(std::ptr::null_mut(), core.root_scope, std::ptr::null_mut()),
    );
    // SAFETY: `rec` was just built into the store.
    unsafe { Binding::lay_out(rec, core.tape.parsing_tape, core.root_scope, offset) };
    rec
}

fn rec(identity: DyadPtr) -> DyadPtr {
    rec_in(identity, std::ptr::null_mut())
}

/// A leaked binding node: a null type word, then the record.
#[repr(C)]
struct Leaked(DyadPtr, Binding);

fn rec_in(identity: DyadPtr, scope: DyadPtr) -> DyadPtr {
    let rec = Binding::new(identity, scope, std::ptr::null_mut());
    Box::into_raw(Box::new(Leaked(std::ptr::null_mut(), rec))) as DyadPtr
}

/// The fields behind a binding dyad.
fn f(binding: DyadPtr) -> Binding {
    // SAFETY: only `rec`-built dyads reach the trie in these tests.
    unsafe { Binding::read(binding) }
}

/// A distinct sentinel address per tag, never dereferenced.
fn dyad(tag: usize) -> DyadPtr {
    std::ptr::without_provenance_mut(tag)
}

fn dyad_cells(tags: &[usize]) -> Vec<Cell> {
    tags.iter().map(|&t| unsafe { Cell::built(dyad(t)) }).collect()
}

#[test]
fn offset_indexing_is_cursor_relative() {
    let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12, 13]));
    t.set_cursor(2);
    assert_eq!(t.at(0).unwrap().dyad, dyad(12));
    assert_eq!(t.at(-1).unwrap().dyad, dyad(11));
    assert_eq!(t.at(1).unwrap().dyad, dyad(13));
    assert_eq!(t.at(-2).unwrap().dyad, dyad(10));
    assert!(t.at(2).is_none());
    assert!(t.at(-3).is_none());
    assert_eq!(t.cursor(), 2);
}

#[test]
fn insert_left_keeps_cursor_on_same_cell() {
    let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
    t.set_cursor(1);
    t.insert(0, unsafe { Cell::built(dyad(99)) });
    assert_eq!(t.at(0).unwrap().dyad, dyad(11));
    assert_eq!(t.at(-1).unwrap().dyad, dyad(99));
    assert_eq!(t.len(), 4);
    assert_eq!(t.cursor(), 2);
}

#[test]
fn insert_right_leaves_cursor() {
    let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
    t.set_cursor(1);
    t.insert(1, unsafe { Cell::built(dyad(99)) });
    assert_eq!(t.at(0).unwrap().dyad, dyad(11));
    assert_eq!(t.at(1).unwrap().dyad, dyad(99));
    assert_eq!(t.at(2).unwrap().dyad, dyad(12));
}

#[test]
fn remove_left_keeps_cursor_on_same_cell() {
    let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
    t.set_cursor(2);
    let gone = t.remove(-1);
    assert_eq!(gone.unwrap().dyad, dyad(11));
    assert_eq!(t.at(0).unwrap().dyad, dyad(12));
    assert_eq!(t.at(-1).unwrap().dyad, dyad(10));
    assert_eq!(t.len(), 2);
}

#[test]
fn removing_the_center_moves_it_to_the_next_cell_or_past_the_end() {
    let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
    t.set_cursor(1);
    t.remove(0);
    assert_eq!(t.at(0).unwrap().dyad, dyad(12));
    t.remove(0);
    assert!(t.at(0).is_none(), "past the end");
    assert_eq!(t.at(-1).unwrap().dyad, dyad(10), "the tail is still behind the center");
    assert_eq!(t.cursor(), t.len());
    t.insert(1, unsafe { Cell::built(dyad(7)) });
    assert_eq!(t.last().unwrap().dyad, dyad(7));
    assert_eq!(t.len(), 2);
}

#[test]
fn unconstructed_and_constructed_cells_coexist() {
    // Told apart by the tape's own flag, never by the dyad.
    let mut t = ParsingTape::new();
    t.insert(0, unsafe { Cell::lexed(dyad(3), "abc", 0, 3) });
    t.insert(1, unsafe { Cell::built(dyad(7)) });
    assert_eq!(t.is_constructed(0), Some(false));
    assert_eq!(t.is_constructed(1), Some(true));
    assert_eq!(t.own_text(), Some("abc"));
    assert_eq!(t.spelling(1), Some(""), "a built cell answers the empty text");
    assert_eq!(t.at(1).unwrap().dyad, dyad(7));
    assert_eq!(t.len(), 2);
}

#[test]
fn a_cell_is_rewritable_in_place_until_placed() {
    let mut t = ParsingTape::from_cells(vec![
        unsafe { Cell::lexed(dyad(1), "a b c", 0, 1) },
        unsafe { Cell::lexed(dyad(2), "a b c", 2, 1) },
        unsafe { Cell::lexed(dyad(3), "a b c", 4, 1) },
    ]);
    t.set_cursor(1);
    t.at_mut(1).unwrap().len = 2;
    assert_eq!(t.at(1).unwrap().len, 2);
    t.reduce_here(dyad(9));
    assert_eq!(t.len(), 1);
    let c = *t.at(0).unwrap();
    assert!(c.constructed);
    assert_eq!((c.dyad, c.start, c.len), (dyad(9), 0, 6));
    t.recenter(0);
    assert_eq!(t.cursor(), 0);
}

/// Parse one expression and run it on a runtime with the lexer attached.
fn go(
    src: &str,
    store: &mut crate::store::Store,
    trie: &mut crate::regex_trie::RegexTrie,
    types: &Core,
    scopes: ScopeStack,
) -> (i64, ScopeStack) {
    let mut p = Parser::new(src, store, trie, types, scopes);
    let node = p.parse_expression().unwrap_or_else(|e| panic!("{src}: {e:?}"));
    let scopes = p.into_scopes();
    let mut rt = crate::run::Runtime::new(types, store);
    // SAFETY: `node` was just parsed into the store.
    let v = rt.hosting(&scopes, trie, None, |rt| unsafe { rt.run(node) });
    (v.unwrap_or_else(|e| panic!("{src}: {e:?}")), scopes)
}

#[test]
fn a_scope_carries_its_enclosing_scope() {
    let mut store = crate::store::Store::new();
    let mut trie = crate::regex_trie::RegexTrie::new();
    let core = crate::identities::Core::build(&mut store, &mut trie);
    let types = &core;
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let mut p = Parser::new("(1, 2, (3, 4))", &mut store, &mut trie, types, scopes);
    let outer = p.parse_expression().unwrap();
    // SAFETY: the nodes were just parsed into the store.
    unsafe {
        use crate::identities::scope::{exprs_of, parent_of};
        assert_eq!(dyad::ty(outer), types.scope);
        let inner = exprs_of(outer).expect("a sequence")[2];
        assert_eq!(dyad::ty(inner), types.scope);
        assert_eq!(parent_of(inner), outer);
        assert_eq!(parent_of(outer), core.root_scope);
        assert!(parent_of(core.root_scope).is_null(), "the arche encloses nothing");
    }
}

#[test]
fn here_is_where_the_line_is_written() {
    let mut store = crate::store::Store::new();
    let mut trie = crate::regex_trie::RegexTrie::new();
    let core = crate::identities::Core::build(&mut store, &mut trie);
    let types = &core;
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);
    let (root, scopes) = go("here.scope", &mut store, &mut trie, types, scopes);
    assert_eq!(root as usize, core.root_scope as usize);
    let (above, scopes) = go("here.scope.back", &mut store, &mut trie, types, scopes);
    assert_eq!(above, 0, "the arche has no enclosing scope");
    // Past the arche is the checked error, never a dereference.
    let mut p = Parser::new("here.scope.back.back", &mut store, &mut trie, types, scopes);
    let node = p.parse_expression().unwrap();
    let scopes = p.into_scopes();
    let mut rt = crate::run::Runtime::new(types, &mut store);
    // SAFETY: `node` was just parsed into the store.
    let err = rt.hosting(&scopes, &trie, None, |rt| unsafe { rt.run(node) }).unwrap_err();
    assert_eq!(err, crate::run::RunError::NullPointer);
    // `caller.scope` outside a constructor is the checked error.
    let mut p = Parser::new("caller.scope", &mut store, &mut trie, types, scopes);
    let node = p.parse_expression().unwrap();
    let scopes = p.into_scopes();
    let mut rt = crate::run::Runtime::new(types, &mut store);
    // SAFETY: `node` was just parsed into the store.
    let err = rt.hosting(&scopes, &trie, None, |rt| unsafe { rt.run(node) }).unwrap_err();
    assert_eq!(err, crate::run::RunError::NoCaller);
}

#[test]
fn lex_hands_back_the_tape_unconstructed() {
    use crate::identities::Core;
    use crate::regex_trie::RegexTrie;
    use crate::store::Store;

    let mut store = Store::new();
    let mut trie = RegexTrie::new();
    let core = Core::build(&mut store, &mut trie);
    let types = &core;
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let plus = lex_fragment(&scopes, &trie, "+").unwrap();
    assert_eq!(plus.len(), 1);
    let cell = *plus.at(0).unwrap();
    assert!(!cell.constructed);
    assert_eq!(cell.identity(types), types.plus, "the cell points at `+`'s binding");
    assert!(!cell.binding(types).is_null());
    assert_eq!(plus.spelling(0), Some("+"));

    let group = lex_fragment(&scopes, &trie, "(a, b)").unwrap();
    let cells = group.cells();
    assert_eq!(cells.len(), 5, "a group is its tokens, the bracket not woken");
    assert!(cells.iter().all(|c| !c.constructed));
    assert_eq!(cells[0].identity(types), types.open_);
    assert_eq!(cells[2].identity(types), types.sep_);
    assert_eq!(cells[4].identity(types), types.close_);
    assert!(cells[1].is_fresh() && cells[3].is_fresh(), "names nothing declared are fresh");
    assert!(cells[1].dyad.is_null(), "a fresh spelling has no node");
    assert_eq!(cells[1].spelling(), "a");
    assert_eq!(cells[3].spelling(), "b");
    assert_eq!(group.cursor(), 0, "the fragment is centered on its first cell");

    let five = lex_fragment(&scopes, &trie, " 5 ").unwrap();
    assert_eq!(five.len(), 1);
    assert_eq!(five.at(0).unwrap().identity(types), types.rational);
    assert_eq!(five.spelling(0), Some("5"), "`lex «5»` carries «5»");

    let none = lex_fragment(&scopes, &trie, "  ").unwrap();
    assert!(none.is_empty());
    assert!(lex_fragment(&scopes, &trie, "«").is_err(), "nothing spells a lone quote mark");
}

#[test]
fn a_handle_outlives_the_center_and_counts_edits() {
    let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11, 12]));
    t.set_cursor(1);
    let own = t.mark();
    let edits = t.edits();
    t.recenter(1);
    assert_eq!(t.cell_at_mark(own).map(|c| c.dyad), Some(dyad(11)));
    assert_eq!(t.edits(), edits, "moving the center is not an edit");
    t.restore(own);
    assert_eq!(t.at(0).unwrap().dyad, dyad(11));
    t.set_dyad(0, dyad(7));
    assert!(t.cell_at_mark(own).unwrap().constructed, "a write keeps the flag");
    assert_eq!(t.cell_at_mark(own).unwrap().dyad, dyad(7));
    assert_eq!(t.edits(), edits + 1, "a write is an edit");
    t.set_constructed(0, false);
    assert!(!t.cell_at_mark(own).unwrap().constructed, "the flag is its own write");
    assert_eq!(t.edits(), edits + 2, "a flag write is an edit");
    t.insert(1, unsafe { Cell::built(dyad(8)) });
    assert_eq!(t.edits(), edits + 3, "a splice is an edit");
    t.remove(0);
    assert!(t.cell_at_mark(own).is_none(), "a removed cell is gone by its handle");
    assert_eq!(t.edits(), edits + 4);
}

#[test]
fn a_fragment_splices_in_order_with_its_spellings() {
    let frag = vec![unsafe { Cell::lexed(dyad(1), "x y", 0, 1) }, unsafe {
        Cell::lexed(dyad(2), "x y", 2, 1)
    }];
    let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11]));
    t.splice(1, frag.clone());
    let ids: Vec<_> = t.cells().iter().map(|c| c.dyad).collect();
    assert_eq!(ids, vec![dyad(10), dyad(1), dyad(2), dyad(11)]);
    assert_eq!(t.spelling(1), Some("x"));
    assert_eq!(t.spelling(2), Some("y"));
    assert_eq!(t.cursor(), 0, "the center keeps pointing at the same cell");

    let mut t = ParsingTape::from_cells(dyad_cells(&[10]));
    t.splice(0, frag.clone());
    let ids: Vec<_> = t.cells().iter().map(|c| c.dyad).collect();
    assert_eq!(ids, vec![dyad(1), dyad(2), dyad(10)]);

    let mut empty = ParsingTape::new();
    empty.splice(3, frag.clone());
    assert_eq!(empty.len(), 2);
    assert_eq!(empty.cursor(), 0, "on an empty tape the first cell becomes the center");

    let mut t = ParsingTape::from_cells(dyad_cells(&[10, 11]));
    let own = t.cells();
    t.splice(2, own);
    let ids: Vec<_> = t.cells().iter().map(|c| c.dyad).collect();
    assert_eq!(ids, vec![dyad(10), dyad(11), dyad(10), dyad(11)]);
}

#[test]
fn a_logos_constructor_runs_when_its_identity_appears() {
    // A postfix `squared` whose constructor is written in Logos, one above `*`'s parse_rank.
    use crate::identities::Core;
    use crate::regex_trie::RegexTrie;
    use crate::store::Store;

    let mut store = Store::new();
    let mut trie = RegexTrie::new();
    let core = Core::build(&mut store, &mut trie);
    let types = &core;
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let (_, s) = go("sq := fn (a := i32 ?) -> i32 ( a * a )", &mut store, &mut trie, types, scopes);
    let (_, s) = go(
        "squared := type ( \
            a := i32 ?, output_type := type ?, share run = ( sq(a) ), \
            share parse_rank = *.parse_rank + 1, \
            share parse = ( tape[0].type = squared, tape[0].a = tape[-1], tape[0].output_type = i32, \
                      tape.is_constructed[0] = true, tape.remove(-1) ) )",
        &mut store,
        &mut trie,
        types,
        s,
    );

    let (_, s) = go("x := i32 5", &mut store, &mut trie, types, s);
    let (v, s) = go("x squared", &mut store, &mut trie, types, s);
    assert_eq!(v, 25);
    let (v, s) = go("x squared + 1", &mut store, &mut trie, types, s);
    assert_eq!(v, 26, "squared binds tighter than +");
    let (v, s) = go("2 * x squared", &mut store, &mut trie, types, s);
    assert_eq!(v, 50, "and tighter than *");
    let (_, s) =
        go("f := fn (y := i32 ?) -> i32 ( y squared + 1 )", &mut store, &mut trie, types, s);
    let (v, _s) = go("f(3)", &mut store, &mut trie, types, s);
    assert_eq!(v, 10, "the node a constructor built calls like any other");
}

#[test]
fn a_constructor_writes_a_cell_from_logos() {
    // The write and the flag are two lines: `t[0] = g` repoints, `t.is_constructed[0] = true` marks.
    use crate::identities::Core;
    use crate::regex_trie::RegexTrie;
    use crate::store::Store;

    let mut store = Store::new();
    let mut trie = RegexTrie::new();
    let core = Core::build(&mut store, &mut trie);
    let types = &core;
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let tape = {
        let mut tape = ParsingTape::new();
        let mut p = Parser::new("a + b", &mut store, &mut trie, types, scopes);
        p.lex_segment(&mut tape).unwrap();
        scopes = p.into_scopes();
        tape.set_cursor(1);
        Box::into_raw(Box::new(tape))
    };
    // SAFETY: `tape` is a live box the natives write through.
    unsafe {
        let plus = (*tape).at(0).unwrap().dyad;
        let rec = tape_name(&mut store, &core, tape);
        scopes.declare(&mut trie, "t", rec).unwrap();

        let (v, s) = go("t[0].type", &mut store, &mut trie, types, scopes);
        assert_eq!(v as DyadPtr, core.type_, "an identity's type is the root");

        let (_, s) = go(
            "g := fn (p := i32 ?, q := i32 ?) -> i32 ( p + q )",
            &mut store,
            &mut trie,
            types,
            s,
        );
        let (_, s) = go("t[0] = g", &mut store, &mut trie, types, s);
        let c = *(*tape).at(0).unwrap();
        assert!(!c.constructed, "a write never sets the flag");
        assert_ne!(c.dyad, plus, "the pointer is replaced");
        assert_eq!(
            types.through(c.dyad),
            types.through(s.resolve(&trie, "g").unwrap().binding),
            "the cell now names g"
        );
        let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
        assert_eq!(v, 0);
        let (_, s) = go("t.is_constructed[0] = true", &mut store, &mut trie, types, s);
        assert!((*tape).at(0).unwrap().constructed, "the flag is the constructor's line");
        let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
        assert_eq!(v, 1);

        let (_, s) = go("t.remove(1)", &mut store, &mut trie, types, s);
        let (_, _s) = go("t.remove(-1)", &mut store, &mut trie, types, s);
        assert_eq!((*tape).len(), 1);

        drop(Box::from_raw(tape));
    }
}

#[test]
fn the_tape_affordances_are_reachable_from_logos() {
    use crate::identities::Core;
    use crate::regex_trie::RegexTrie;
    use crate::store::Store;

    let mut store = Store::new();
    let mut trie = RegexTrie::new();
    let core = Core::build(&mut store, &mut trie);
    let types = &core;
    let mut scopes = ScopeStack::new();
    scopes.push(core.root_scope);

    let tape = {
        let mut tape = ParsingTape::new();
        let mut p = Parser::new("a + b", &mut store, &mut trie, types, scopes);
        p.lex_segment(&mut tape).unwrap();
        scopes = p.into_scopes();
        tape.set_cursor(0);
        Box::into_raw(Box::new(tape))
    };
    // SAFETY: `tape` is a live box; the natives and the checks below read through the same handle.
    unsafe {
        assert_eq!((*tape).len(), 3);
        let plus = (*tape).at(1).unwrap().dyad;
        assert!(!(*tape).at(1).unwrap().constructed, "a lexed cell is unconstructed");

        let rec = tape_name(&mut store, &core, tape);
        scopes.declare(&mut trie, "t", rec).unwrap();

        let (v, s) = go("t.is_constructed[1]", &mut store, &mut trie, types, scopes);
        assert_eq!(v, 0);
        let (v, s) = go("t.spelling[1]", &mut store, &mut trie, types, s);
        assert_eq!(crate::identities::string::text(v as DyadPtr), b"+");
        let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
        assert_eq!(crate::identities::string::text(v as DyadPtr), b"a");
        let (v, s) = go("t[1]:name", &mut store, &mut trie, types, s);
        assert_eq!(crate::identities::string::text(v as DyadPtr), b"+");
        let (v, s) = go("t.remove(1)", &mut store, &mut trie, types, s);
        assert_eq!(v as DyadPtr, plus, "remove yields the cell it took");
        assert_eq!((*tape).len(), 2);

        let (a_rec, b_rec) = ((*tape).at(0).unwrap().dyad, (*tape).at(1).unwrap().dyad);
        let (_, s) = go("i := i64 1", &mut store, &mut trie, types, s);
        let (v, s) = go("t[i]", &mut store, &mut trie, types, s);
        assert_eq!(v as DyadPtr, b_rec, "an index may be a name");
        let (_, s) = go("t.recenter(1)", &mut store, &mut trie, types, s);
        let (v, s) = go("t[-1]", &mut store, &mut trie, types, s);
        assert_eq!(v as DyadPtr, a_rec, "a negative index reads the left context");
        let (v, s) = go("t[i - 2]", &mut store, &mut trie, types, s);
        assert_eq!(v as DyadPtr, a_rec, "an index may be an expression");
        let (_, s) = go("t.recenter(-1)", &mut store, &mut trie, types, s);

        let (_, s) = go("t[0] = dyad (i32, 7)", &mut store, &mut trie, types, s);
        let c0 = *(*tape).at(0).unwrap();
        assert!(!c0.constructed, "a write replaces the pointer and nothing more");
        assert_eq!(dyad::ty(c0.dyad), core.i32_);
        let (v, s) = go("t[0]", &mut store, &mut trie, types, s);
        assert_eq!(v as DyadPtr, c0.dyad, "the element read yields the cell");
        let (_, s) = go("t.is_constructed[0] = true", &mut store, &mut trie, types, s);
        let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
        assert_eq!(v, 1, "the flag is the constructor's own write");
        let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
        assert_eq!(
            crate::identities::string::text(v as DyadPtr),
            b"a",
            "the text stays after the cell is constructed"
        );

        let (_, s) = go("t.insert(0, lex «9»)", &mut store, &mut trie, types, s);
        assert_eq!((*tape).len(), 3);
        assert_eq!((*tape).at(0).unwrap().dyad, c0.dyad, "the center stays on its cell");
        let (_, s) = go("t.recenter(-1)", &mut store, &mut trie, types, s);
        let (v, s) = go("t.is_constructed[0]", &mut store, &mut trie, types, s);
        assert_eq!(v, 0, "a spliced cell is unconstructed");
        let (v, s) = go("t[0]", &mut store, &mut trie, types, s);
        assert_eq!(dyad::ty(v as DyadPtr), core.binding_, "the spliced cell is a binding");
        assert_eq!(types.through(v as DyadPtr), core.rational, "the number pattern's");
        let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
        assert_eq!(
            crate::identities::string::text(v as DyadPtr),
            b"9",
            "a spliced cell keeps the text it was lexed from"
        );
        let (_, s) = go("t[0] = dyad (i32, 9)", &mut store, &mut trie, types, s);
        let (v, s) = go("t[0]", &mut store, &mut trie, types, s);
        assert_eq!(dyad::ty(v as DyadPtr), core.i32_, "the written cell, now the center");
        assert_ne!(v as DyadPtr, c0.dyad);
        let (v, s) = go("t.spelling[0]", &mut store, &mut trie, types, s);
        assert_eq!(crate::identities::string::text(v as DyadPtr), b"9");
        let mut p = Parser::new("t.insert(0, dyad (i32, 9))", &mut store, &mut trie, types, s);
        assert_eq!(p.parse_expression().unwrap_err(), ParseError::InsertTakesTape);
        let s = p.into_scopes();

        // A use of a name handed in is its binding; the flag is untouched.
        let (_, s) = go("t[0] = t", &mut store, &mut trie, types, s);
        assert!(!(*tape).at(0).unwrap().constructed);
        assert_eq!((*tape).at(0).unwrap().dyad, rec);

        // Through a function taking the tape as a pointer parameter.
        let (_, s) = go(
            "g := fn (p := @parsing_tape ?) -> void ( p@.remove(0) )",
            &mut store,
            &mut trie,
            types,
            s,
        );
        let (_, _s) = go("g(&t)", &mut store, &mut trie, types, s);
        assert_eq!((*tape).len(), 2);

        drop(Box::from_raw(tape));
    }
}

#[test]
fn resolves_a_name_declared_in_an_open_scope() {
    let mut trie = RegexTrie::new();
    let mut scopes = ScopeStack::new();
    scopes.push(dyad(100));
    let id = dyad(1);
    unsafe { scopes.declare(&mut trie, "a", rec(id)) }.unwrap();
    assert_eq!(scopes.resolve(&trie, "a").unwrap().identity, id);
}

#[test]
fn same_name_in_sibling_scopes_resolves_the_open_one() {
    let mut trie = RegexTrie::new();
    let mut scopes = ScopeStack::new();
    let (outer, inner) = (dyad(100), dyad(101));

    scopes.push(outer);
    unsafe { scopes.declare(&mut trie, "x", rec(dyad(1))) }.unwrap();
    scopes.pop();

    scopes.push(inner);
    unsafe { scopes.declare(&mut trie, "x", rec(dyad(2))) }.unwrap();
    assert_eq!(scopes.resolve(&trie, "x").unwrap().identity, dyad(2));

    scopes.pop();
    scopes.push(outer);
    assert_eq!(scopes.resolve(&trie, "x").unwrap().identity, dyad(1));
}

#[test]
fn out_of_scope_is_distinct_from_unknown() {
    let mut trie = RegexTrie::new();
    let mut scopes = ScopeStack::new();
    scopes.push(dyad(100));
    unsafe { scopes.declare(&mut trie, "y", rec(dyad(1))) }.unwrap();
    scopes.pop();

    assert_eq!(scopes.resolve(&trie, "y"), Err(ResolveError::OutOfScope("y".into())));
    assert_eq!(scopes.resolve(&trie, "nope"), Err(ResolveError::Unknown("nope".into())));
}

#[test]
fn shadowing_is_rejected() {
    let mut trie = RegexTrie::new();
    let mut scopes = ScopeStack::new();
    let (outer, inner) = (dyad(100), dyad(101));

    scopes.push(outer);
    unsafe { scopes.declare(&mut trie, "a", rec(dyad(1))) }.unwrap();
    assert_eq!(
        unsafe { scopes.declare(&mut trie, "a", rec(dyad(2))) },
        Err(ResolveError::Shadowed("a".into()))
    );
    scopes.push(inner);
    assert_eq!(
        unsafe { scopes.declare(&mut trie, "a", rec(dyad(3))) },
        Err(ResolveError::Shadowed("a".into()))
    );
}

#[test]
fn rollback_undoes_journalled_declarations() {
    let mut trie = RegexTrie::new();
    let mut scopes = ScopeStack::new();
    scopes.push(dyad(100));
    unsafe { scopes.declare(&mut trie, "keep", rec(dyad(1))) }.unwrap();
    scopes.commit();
    unsafe { scopes.declare(&mut trie, "gone", rec(dyad(2))) }.unwrap();

    scopes.rollback(&mut trie, &[]);
    assert_eq!(scopes.resolve(&trie, "keep").unwrap().identity, dyad(1));
    assert_eq!(scopes.resolve(&trie, "gone"), Err(ResolveError::Unknown("gone".into())));
    unsafe { scopes.declare(&mut trie, "gone", rec(dyad(3))) }.unwrap();
    assert_eq!(scopes.resolve(&trie, "gone").unwrap().identity, dyad(3));
}

#[test]
fn rebind_points_a_spelling_at_the_original_identity() {
    let mut trie = RegexTrie::new();
    let mut scopes = ScopeStack::new();
    scopes.push(dyad(100));
    let alias = rec(dyad(1));
    unsafe { scopes.declare(&mut trie, "alias", alias) }.unwrap();
    unsafe { scopes.rebind(alias, dyad(2)) };
    assert_eq!(scopes.resolve(&trie, "alias").unwrap().identity, dyad(2));
    // The declare's journal entry still covers the rebound binding.
    scopes.rollback(&mut trie, &[]);
    assert_eq!(scopes.resolve(&trie, "alias"), Err(ResolveError::Unknown("alias".into())));
}

#[test]
fn truncate_restores_a_known_depth() {
    let mut scopes = ScopeStack::new();
    scopes.push(dyad(100));
    scopes.push(dyad(101));
    scopes.push(dyad(102));
    scopes.truncate(1);
    assert_eq!(scopes.depth(), 1);
    assert_eq!(scopes.current(), Some(dyad(100)));
    assert!(!scopes.is_open(dyad(101)));
}

#[test]
fn a_dead_name_resolves_as_dead_and_may_be_redeclared() {
    let mut trie = RegexTrie::new();
    let mut scopes = ScopeStack::new();
    let scope = dyad(100);
    scopes.push(scope);
    let a1 = rec(dyad(1));
    unsafe { scopes.declare(&mut trie, "a", a1) }.unwrap();
    unsafe { scopes.mark_dead(a1, dyad(50)) };

    assert_eq!(scopes.resolve(&trie, "a"), Err(ResolveError::Dead("a".into())));
    unsafe { scopes.declare(&mut trie, "a", rec(dyad(2))) }.unwrap();
    assert_eq!(scopes.resolve(&trie, "a").unwrap().identity, dyad(2));
    // The dead entry is still indexed: its range is what reflection reads.
    let m = trie.get("a").unwrap();
    assert_eq!(m.bindings.len(), 2);
    assert!(m.bindings.iter().any(|&c| f(c).dyad == dyad(1) && f(c).end == dyad(50)));
}

#[test]
fn rollback_restores_a_dead_mark() {
    let mut trie = RegexTrie::new();
    let mut scopes = ScopeStack::new();
    let scope = dyad(100);
    scopes.push(scope);
    let a1 = rec(dyad(1));
    unsafe { scopes.declare(&mut trie, "a", a1) }.unwrap();
    scopes.commit();

    unsafe { scopes.mark_dead(a1, dyad(50)) };
    unsafe { scopes.declare(&mut trie, "a", rec(dyad(2))) }.unwrap();
    scopes.rollback(&mut trie, &[]);

    assert_eq!(scopes.resolve(&trie, "a").unwrap().identity, dyad(1));
    assert_eq!(trie.get("a").unwrap().bindings.len(), 1);
}

#[test]
fn rollback_keeps_the_dead_mark_of_a_name_the_line_s_run_freed() {
    let mut trie = RegexTrie::new();
    let mut scopes = ScopeStack::new();
    scopes.push(dyad(100));
    let a1 = rec(dyad(1));
    unsafe { scopes.declare(&mut trie, "a", a1) }.unwrap();
    scopes.commit();

    unsafe { scopes.mark_dead(a1, dyad(50)) };
    scopes.rollback(&mut trie, &[a1]);

    assert_eq!(scopes.resolve(&trie, "a"), Err(ResolveError::Dead("a".into())));
}

#[test]
fn two_spellings_of_equal_rank_matching_the_same_length_are_a_tie() {
    // Two patterns entered as the core enters its own (raw keys).
    let mut trie = RegexTrie::new();
    let mut scopes = ScopeStack::new();
    let sc = dyad(9);
    scopes.push(sc);
    trie.insert("a[0-9]", rec_in(dyad(1), sc));
    trie.insert("[a-z]1", rec_in(dyad(2), sc));
    assert_eq!(scopes.resolve(&trie, "a1"), Err(ResolveError::Tied));
    assert_eq!(scopes.resolve(&trie, "a2").unwrap().identity, dyad(1));
    assert_eq!(scopes.resolve(&trie, "b1").unwrap().identity, dyad(2));
    // A longer match at equal rank wins, as `:=` wins over `:`.
    trie.insert("a1x", rec_in(dyad(3), sc));
    assert_eq!(scopes.resolve(&trie, "a1x").unwrap().identity, dyad(3));
}

#[test]
fn settle_patches_start_and_end_to_the_body_item() {
    let mut trie = RegexTrie::new();
    let mut scopes = ScopeStack::new();
    let scope = dyad(100);
    scopes.push(scope);
    let a1 = rec(dyad(1));
    unsafe { scopes.declare(&mut trie, "a", a1) }.unwrap();
    let a = |trie: &RegexTrie| f(trie.get("a").unwrap().bindings[0]);
    assert!(a(&trie).start.is_null());

    unsafe { scopes.settle_item(scope, dyad(10)) };
    assert_eq!(a(&trie).start, dyad(10));
    assert!(a(&trie).end.is_null());

    unsafe { scopes.mark_dead(a1, dyad(50)) };
    assert_eq!(a(&trie).end, dyad(50), "provisional: the move/free node");
    unsafe { scopes.settle_item(scope, dyad(11)) };
    assert_eq!(a(&trie).end, dyad(11), "settled: the body item");
    assert_eq!(a(&trie).start, dyad(10), "start untouched by the end's settle");

    // A rebind keeps the range, and the pending endpoint holds the binding.
    let b_rec = rec(dyad(3));
    unsafe { scopes.declare(&mut trie, "b", b_rec) }.unwrap();
    unsafe { scopes.rebind(b_rec, dyad(4)) };
    unsafe { scopes.settle_item(scope, dyad(12)) };
    let b = f(trie.get("b").unwrap().bindings[0]);
    assert_eq!((b.dyad, b.start), (dyad(4), dyad(12)));
}

#[test]
fn a_barrier_between_a_name_and_the_current_scope_is_detected() {
    let mut scopes = ScopeStack::new();
    let (outer, body, inner) = (dyad(100), dyad(101), dyad(102));
    scopes.push(outer);
    scopes.push_barrier();
    scopes.push(body);
    scopes.push(inner);
    assert!(scopes.crosses_barrier(outer));
    assert!(!scopes.crosses_barrier(body));
    assert!(!scopes.crosses_barrier(inner));
    scopes.pop();
    scopes.pop();
    scopes.pop_barrier();
    assert!(!scopes.crosses_barrier(outer));
}

#[test]
fn truncating_past_a_body_drops_its_barrier() {
    // Or every later top-level `move`/`free` is refused for the rest of the session.
    let mut scopes = ScopeStack::new();
    let (outer, body) = (dyad(100), dyad(101));
    scopes.push(outer);
    scopes.push_barrier();
    scopes.push(body);
    assert!(scopes.crosses_barrier(outer));
    scopes.truncate(1);
    assert!(!scopes.crosses_barrier(outer), "the closed body's barrier is gone");
}

#[test]
fn two_live_bindings_resolve_to_the_innermost_scope() {
    let mut trie = RegexTrie::new();
    let (a, b) = (dyad(100), dyad(101));
    trie.insert("z", rec_in(dyad(1), a));
    trie.insert("z", rec_in(dyad(2), b));

    let mut scopes = ScopeStack::new();
    scopes.push(a);
    scopes.push(b);
    assert_eq!(scopes.resolve(&trie, "z").map(|r| r.identity), Ok(dyad(2)));
    scopes.pop();
    assert_eq!(scopes.resolve(&trie, "z").map(|r| r.identity), Ok(dyad(1)));
}
