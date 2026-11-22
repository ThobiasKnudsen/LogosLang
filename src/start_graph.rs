// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The start graph the seed carries inside its binary: the store, the name index and the core a
//! start builds, written with every address as an offset and fixed up at load (›The seed carries
//! its start graph inside its binary‹ [[R386]]). The writer builds the start twice and compares
//! the two: a word that moved by its target's own shift is an address, and anything else that
//! differs is refused, so the graph holds no address the writer did not see.

use std::collections::{HashMap, HashSet};
use std::mem::{offset_of, size_of, MaybeUninit};

use crate::compile::{LowerFn, LowerTable};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, HeldLine, ParsingTape};
use crate::regex_trie::RegexTrie;
use crate::store::{Rebuilt, Sides, Store};
use crate::Core;

const MAGIC: &[u8; 8] = b"logos-sg";
const VERSION: u32 = 1;
/// What the build script hashed this build from: its sources, the files it carries, its
/// toolchain and its settings. A graph another build wrote is never read.
const KEY: &str = env!("LOGOS_BUILD_KEY");

/// A null pointer, and the targets an address can lie in: the arena, the binary's code, and each
/// object the store keeps outside its arena, numbered from `OBJECT` on.
const NULL: u32 = u32::MAX;
const ARENA: u32 = 0;
const NATIVE: u32 = 1;
const OBJECT: u32 = 2;

/// The code address a native entry is written relative to.
#[inline(never)]
fn anchor() -> usize {
    anchor as fn() -> usize as usize
}

/// The store, the name index and the core one start leaves.
pub type Start = (Store, RegexTrie, Core);

/// The start `carried` holds, or a start built from source where it holds none or one another
/// build wrote; `true` with a carried one. Only the binary that wrote `carried` may hand it
/// here: its natives are offsets into that binary's code.
pub fn start(carried: &[u8]) -> (Start, bool) {
    match load(carried) {
        Some(start) => (start, true),
        None => (build(), false),
    }
}

/// A start built from source: the core, then each file the seed carries.
pub fn build() -> Start {
    let mut store = Store::new();
    let mut trie = RegexTrie::new();
    let core = Core::build(&mut store, &mut trie);
    (store, trie, core)
}

/// The graph two starts of this binary left, each built on its own: refused where they differ in
/// anything but where their parts lie.
pub fn write(first: &Start, second: &Start) -> Result<Vec<u8>, String> {
    let sides = [first.0.sides()?, second.0.sides()?];
    let places = [Places::of(&first.0, &sides[0]), Places::of(&second.0, &sides[1])];
    let mut out = Out::new(&places[0]);
    out.buf.extend_from_slice(MAGIC);
    out.u32(VERSION);
    out.bytes(KEY.as_bytes());

    let mut arena = first.0.arena_bytes().to_vec();
    let sites = relocate("the arena", &mut arena, second.0.arena_bytes(), &places)?;
    out.bytes(&arena);
    sites.carry(&mut out);
    let s = &sides[0];
    for count in [s.nodes, s.node_bytes, s.hung, s.tables.len(), s.tapes.len(), s.lines.len()] {
        out.usize(count);
    }

    let parts = [sides_of(first, &sides[0], &places[0])?, sides_of(second, &sides[1], &places[1])?];
    for ((name, a), (_, b)) in parts[0].iter().zip(&parts[1]) {
        if a != b {
            return Err(format!("two starts differ in {name}"));
        }
        out.buf.extend_from_slice(a);
    }

    let mut core = core_bytes(&first.2);
    let sites = relocate("the core", &mut core, &core_bytes(&second.2), &places)?;
    out.bytes(&core);
    sites.carry(&mut out);
    Ok(out.buf)
}

/// The start `image` holds, its addresses fixed up where this run put its parts; `None` where it
/// is empty or another build wrote it.
pub fn load(image: &[u8]) -> Option<Start> {
    let mut inp = In { buf: image, at: 0, bases: &[] };
    if image.len() < MAGIC.len() || inp.take(MAGIC.len()) != MAGIC {
        return None;
    }
    if inp.u32() != VERSION || inp.bytes() != KEY.as_bytes() {
        return None;
    }
    let arena = inp.bytes();
    let arena_sites = Vec::<(u32, u32)>::uncarry(&mut inp);
    let [nodes, node_bytes, hung, tables, tapes, lines] = [(); 6].map(|_| inp.usize());

    let mut store = Store::new();
    let mut tables: Vec<Box<HashMap<i64, i64>>> = (0..tables).map(|_| Box::default()).collect();
    let mut tapes: Vec<Box<ParsingTape>> = (0..tapes).map(|_| Box::default()).collect();
    let mut lines: Vec<Box<HeldLine>> = (0..lines).map(|_| Box::default()).collect();
    let mut bases = vec![store.arena_base() as usize, anchor()];
    bases.extend(tables.iter().map(|t| &**t as *const HashMap<i64, i64> as usize));
    bases.extend(tapes.iter().map(|t| &**t as *const ParsingTape as usize));
    bases.extend(lines.iter().map(|l| &**l as *const HeldLine as usize));
    inp.bases = &bases;

    #[cfg(test)]
    let node_list = Vec::<DyadPtr>::uncarry(&mut inp);
    #[cfg(not(test))]
    Vec::<DyadPtr>::uncarry(&mut inp);
    for table in &mut tables {
        let entries = inp.usize();
        table.reserve(entries);
        for _ in 0..entries {
            let key = inp.word();
            table.insert(key, inp.word());
        }
    }
    let texts = Vec::<(Box<[u8]>, DyadPtr)>::uncarry(&mut inp);
    for tape in &mut tapes {
        **tape = ParsingTape::uncarry(&mut inp);
    }
    for line in &mut lines {
        **line = HeldLine::uncarry(&mut inp);
    }
    let natives = Vec::<usize>::uncarry(&mut inp)
        .into_iter()
        .map(|offset| anchor().wrapping_add(offset))
        .collect();
    let trie = RegexTrie::uncarry(&mut inp);
    let lower = Vec::<(DyadPtr, usize)>::uncarry(&mut inp);

    let parts = Rebuilt {
        nodes,
        node_bytes,
        hung,
        #[cfg(test)]
        node_list,
        texts,
        natives,
    };
    store.install(arena, tables, tapes, lines, parts);
    let base = store.arena_base();
    for &(at, target) in &arena_sites {
        // SAFETY: each site is eight bytes inside the arena just installed.
        unsafe { fix(base.add(at as usize), bases[target as usize]) };
    }

    let core = inp.bytes();
    let core_sites = Vec::<(u32, u32)>::uncarry(&mut inp);
    if core.len() != size_of::<Core>() {
        return None;
    }
    let mut built = MaybeUninit::<Core>::uninit();
    let at = built.as_mut_ptr() as *mut u8;
    // SAFETY: `core` is the bytes of a `Core` this build wrote, its addresses fixed up below and
    // its `lower` table written whole before it is read.
    let core = unsafe {
        std::ptr::copy_nonoverlapping(core.as_ptr(), at, core.len());
        for &(site, target) in &core_sites {
            fix(at.add(site as usize), bases[target as usize]);
        }
        let lower: LowerTable = lower
            .into_iter()
            .map(|(id, offset)| {
                (id, std::mem::transmute::<usize, LowerFn>(anchor().wrapping_add(offset)))
            })
            .collect();
        std::ptr::addr_of_mut!((*built.as_mut_ptr()).lower).write(lower);
        built.assume_init()
    };
    Some((store, trie, core))
}

/// The eight bytes at `at` hold an offset into the target at `base`; they become its address.
///
/// # Safety
/// `at` must be valid for eight bytes.
unsafe fn fix(at: *mut u8, base: usize) {
    let offset = std::ptr::read_unaligned(at as *const usize);
    std::ptr::write_unaligned(at as *mut usize, base.wrapping_add(offset));
}

/// `Core`'s bytes, its `lower` table's left zero: that table is written as its entries. Every
/// other field of `Core` is a word, so it has no padding.
fn core_bytes(core: &Core) -> Vec<u8> {
    let mut bytes = vec![0u8; size_of::<Core>()];
    // SAFETY: `core` is a whole `Core`, and `bytes` as long.
    unsafe {
        std::ptr::copy_nonoverlapping(
            core as *const Core as *const u8,
            bytes.as_mut_ptr(),
            size_of::<Core>(),
        )
    };
    let lower = offset_of!(Core, lower);
    bytes[lower..lower + size_of::<LowerTable>()].fill(0);
    bytes
}

/// The parts of a start besides its arena and its core, each written against `places`.
fn sides_of(
    start: &Start,
    sides: &Sides,
    places: &Places,
) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
    let mut parts = Vec::new();
    let mut part = |name: &'static str, write: &mut dyn FnMut(&mut Out)| {
        let mut out = Out::new(places);
        write(&mut out);
        match out.unplaced {
            Some(addr) => {
                Err(format!("{name} hold an address no part of the start holds: {addr:#x}"))
            }
            None => {
                parts.push((name, out.buf));
                Ok(())
            }
        }
    };
    #[cfg(test)]
    part("the node list", &mut |out| sides.node_list.to_vec().carry(out))?;
    #[cfg(not(test))]
    part("the node list", &mut |out| Vec::<DyadPtr>::new().carry(out))?;
    part("the tables", &mut |out| {
        for table in &sides.tables {
            let mut entries: Vec<(Vec<u8>, Vec<u8>)> = table
                .iter()
                .map(|(&key, &value)| {
                    let mut k = Out::new(out.places);
                    k.word(key);
                    let mut v = Out::new(out.places);
                    v.word(value);
                    (k.buf, v.buf)
                })
                .collect();
            entries.sort_unstable();
            out.usize(entries.len());
            for (key, value) in entries {
                out.buf.extend_from_slice(&key);
                out.buf.extend_from_slice(&value);
            }
        }
    })?;
    part("the texts", &mut |out| {
        out.usize(sides.texts.len());
        for &(text, node) in &sides.texts {
            out.bytes(text);
            node.carry(out);
        }
    })?;
    part("the tapes", &mut |out| sides.tapes.iter().for_each(|tape| tape.carry(out)))?;
    part("the held lines", &mut |out| sides.lines.iter().for_each(|line| line.carry(out)))?;
    part("the natives", &mut |out| {
        out.usize(sides.natives.len());
        for &entry in sides.natives {
            out.usize(entry.wrapping_sub(out.places.anchor));
        }
    })?;
    part("the name index", &mut |out| start.1.carry(out))?;
    part("the lowerings", &mut |out| {
        let mut lower: Vec<(DyadPtr, usize)> =
            start.2.lower.iter().map(|(&id, &f)| (id, f as usize)).collect();
        lower.sort_unstable_by_key(|&(id, _)| id as usize);
        out.usize(lower.len());
        for (id, entry) in lower {
            id.carry(out);
            out.usize(entry.wrapping_sub(out.places.anchor));
        }
    })?;
    Ok(parts)
}

/// Where one start's parts lie: what an address is read against.
struct Places {
    arena: usize,
    used: usize,
    anchor: usize,
    natives: HashSet<usize>,
    /// Each object the store keeps outside its arena, by its address.
    objects: HashMap<usize, u32>,
    /// The process's mapped memory, where no word the two starts share may point but at a native.
    mapped: Vec<std::ops::Range<usize>>,
}

impl Places {
    fn of(store: &Store, sides: &Sides) -> Places {
        let tables = sides.tables.iter().map(|&t| t as *const HashMap<i64, i64> as usize);
        let tapes = sides.tapes.iter().map(|&t| t as *const ParsingTape as usize);
        let lines = sides.lines.iter().map(|&l| l as *const HeldLine as usize);
        let objects =
            tables.chain(tapes).chain(lines).enumerate().map(|(i, a)| (a, OBJECT + i as u32));
        Places {
            arena: store.arena_base() as usize,
            used: store.arena_bytes().len(),
            anchor: anchor(),
            natives: sides.natives.iter().copied().collect(),
            objects: objects.collect(),
            mapped: mapped(),
        }
    }

    /// The target `addr` lies in and its offset there, for an address that moves with its part.
    fn moved(&self, addr: usize) -> Option<(u32, usize)> {
        if addr.wrapping_sub(self.arena) <= self.used {
            return Some((ARENA, addr - self.arena));
        }
        self.objects.get(&addr).map(|&target| (target, 0))
    }

    /// As [`Self::moved`], and a native's entry, which stays where the binary put it.
    fn place(&self, addr: usize) -> Option<(u32, usize)> {
        if self.natives.contains(&addr) {
            return Some((NATIVE, addr.wrapping_sub(self.anchor)));
        }
        self.moved(addr)
    }

    /// Whether `addr` points into the process's memory outside every part of the start: at its
    /// stack, a block it freed, or the binary's own code and statics, which lie near the anchor
    /// on every system, the memory map listed or not.
    fn stray(&self, addr: usize) -> bool {
        self.place(addr).is_none()
            && (addr.abs_diff(self.anchor) < IMAGE_REACH
                || self.mapped.iter().any(|range| range.contains(&addr)))
    }
}

/// How far from the anchor the binary's own image may reach. Windows moves an executable once
/// per boot, not once per run, so two runs of the writer may share an address into it.
const IMAGE_REACH: usize = 1 << 28;

/// The ranges of the process's memory map; none where the system does not list them.
fn mapped() -> Vec<std::ops::Range<usize>> {
    let Ok(maps) = std::fs::read_to_string("/proc/self/maps") else {
        return Vec::new();
    };
    let range = |line: &str| {
        let (start, end) = line.split_whitespace().next()?.split_once('-')?;
        Some(usize::from_str_radix(start, 16).ok()?..usize::from_str_radix(end, 16).ok()?)
    };
    maps.lines().filter_map(range).collect()
}

/// The sites in `a` that hold an address, found by comparing it with `b`, the same part of
/// another start: each `(offset, target)`, its eight bytes in `a` rewritten as the offset into the
/// target. A byte that differs between the two and lies in no such site is refused, and so is a
/// word they share that is a stray address ([`Places::stray`]).
fn relocate(
    part: &str,
    a: &mut [u8],
    b: &[u8],
    places: &[Places; 2],
) -> Result<Vec<(u32, u32)>, String> {
    if a.len() != b.len() {
        return Err(format!(
            "two starts differ in the size of {part}: {} and {}",
            a.len(),
            b.len()
        ));
    }
    let word =
        |bytes: &[u8], at: usize| usize::from_ne_bytes(bytes[at..at + 8].try_into().unwrap());
    let mut sites = Vec::new();
    let mut covered = vec![false; a.len()];
    let mut at = 0;
    while at + 8 <= a.len() {
        let (x, y) = (word(a, at), word(b, at));
        let site = if x == y {
            if places[0].stray(x) {
                return Err(format!("{part} holds the stray address {x:#x} at byte {at:#x}"));
            }
            places[0].natives.contains(&x).then(|| (NATIVE, x.wrapping_sub(places[0].anchor)))
        } else {
            match (places[0].moved(x), places[1].moved(y)) {
                (Some(p), Some(q)) if p == q => Some(p),
                _ => None,
            }
        };
        let Some((target, offset)) = site else {
            at += 1;
            continue;
        };
        a[at..at + 8].copy_from_slice(&offset.to_ne_bytes());
        covered[at..at + 8].fill(true);
        sites.push((at as u32, target));
        at += 8;
    }
    if let Some(i) = (0..b.len()).find(|&i| !covered[i] && a[i] != b[i]) {
        return Err(format!("two starts differ in {part} at byte {i:#x}, where no address lies"));
    }
    Ok(sites)
}

/// A part of a start written out, each address as its target and its offset there.
pub(crate) struct Out<'a> {
    buf: Vec<u8>,
    places: &'a Places,
    /// The first address that lies in no part of the start.
    unplaced: Option<usize>,
}

impl<'a> Out<'a> {
    fn new(places: &'a Places) -> Self {
        Out { buf: Vec::new(), places, unplaced: None }
    }

    pub(crate) fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_ne_bytes());
    }

    pub(crate) fn usize(&mut self, v: usize) {
        self.buf.extend_from_slice(&(v as u64).to_ne_bytes());
    }

    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        self.usize(bytes.len());
        self.buf.extend_from_slice(bytes);
    }

    pub(crate) fn ptr<T: ?Sized>(&mut self, p: *const T) {
        let addr = p as *const u8 as usize;
        if addr == 0 {
            self.u32(NULL);
            return;
        }
        let Some((target, offset)) = self.places.place(addr) else {
            self.unplaced.get_or_insert(addr);
            self.u32(NULL);
            return;
        };
        self.u32(target);
        self.usize(offset);
    }

    /// A word that is an address or a number: one that lies in a part of the start is written as
    /// an address, any other as itself.
    pub(crate) fn word(&mut self, w: i64) {
        match self.places.place(w as usize).filter(|_| w != 0) {
            Some((target, offset)) => {
                self.u8(1);
                self.u32(target);
                self.usize(offset);
            }
            None => {
                if self.places.stray(w as usize) {
                    self.unplaced.get_or_insert(w as usize);
                }
                self.u8(0);
                self.usize(w as usize);
            }
        }
    }
}

/// A part of a start read back, each address fixed up against `bases`, one per target.
pub(crate) struct In<'a> {
    buf: &'a [u8],
    at: usize,
    bases: &'a [usize],
}

impl<'a> In<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let bytes = &self.buf[self.at..self.at + n];
        self.at += n;
        bytes
    }

    pub(crate) fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }

    pub(crate) fn u32(&mut self) -> u32 {
        u32::from_ne_bytes(self.take(4).try_into().unwrap())
    }

    pub(crate) fn usize(&mut self) -> usize {
        u64::from_ne_bytes(self.take(8).try_into().unwrap()) as usize
    }

    pub(crate) fn bytes(&mut self) -> &'a [u8] {
        let n = self.usize();
        self.take(n)
    }

    pub(crate) fn ptr<T>(&mut self) -> *mut T {
        let target = self.u32();
        if target == NULL {
            return std::ptr::null_mut();
        }
        self.bases[target as usize].wrapping_add(self.usize()) as *mut T
    }

    pub(crate) fn word(&mut self) -> i64 {
        if self.u8() == 0 {
            return self.usize() as i64;
        }
        let target = self.u32();
        self.bases[target as usize].wrapping_add(self.usize()) as i64
    }
}

/// A value a start graph carries: written with its addresses as offsets, read back fixed up.
pub(crate) trait Carry: Sized {
    fn carry(&self, out: &mut Out);
    fn uncarry(inp: &mut In) -> Self;
}

impl<T> Carry for *mut T {
    fn carry(&self, out: &mut Out) {
        out.ptr(*self);
    }

    fn uncarry(inp: &mut In) -> Self {
        inp.ptr()
    }
}

impl Carry for bool {
    fn carry(&self, out: &mut Out) {
        out.u8(*self as u8);
    }

    fn uncarry(inp: &mut In) -> Self {
        inp.u8() != 0
    }
}

impl Carry for u8 {
    fn carry(&self, out: &mut Out) {
        out.u8(*self);
    }

    fn uncarry(inp: &mut In) -> Self {
        inp.u8()
    }
}

impl Carry for u32 {
    fn carry(&self, out: &mut Out) {
        out.u32(*self);
    }

    fn uncarry(inp: &mut In) -> Self {
        inp.u32()
    }
}

impl Carry for u64 {
    fn carry(&self, out: &mut Out) {
        out.usize(*self as usize);
    }

    fn uncarry(inp: &mut In) -> Self {
        inp.usize() as u64
    }
}

impl Carry for usize {
    fn carry(&self, out: &mut Out) {
        out.usize(*self);
    }

    fn uncarry(inp: &mut In) -> Self {
        inp.usize()
    }
}

impl Carry for f64 {
    fn carry(&self, out: &mut Out) {
        out.usize(self.to_bits() as usize);
    }

    fn uncarry(inp: &mut In) -> Self {
        f64::from_bits(inp.usize() as u64)
    }
}

impl Carry for String {
    fn carry(&self, out: &mut Out) {
        out.bytes(self.as_bytes());
    }

    fn uncarry(inp: &mut In) -> Self {
        String::from_utf8(inp.bytes().to_vec()).expect("a start graph's text is the text written")
    }
}

impl Carry for Box<[u8]> {
    fn carry(&self, out: &mut Out) {
        out.bytes(self);
    }

    fn uncarry(inp: &mut In) -> Self {
        inp.bytes().into()
    }
}

impl Carry for Assoc {
    fn carry(&self, out: &mut Out) {
        out.u8(matches!(self, Assoc::Right) as u8);
    }

    fn uncarry(inp: &mut In) -> Self {
        if inp.u8() == 0 {
            Assoc::Left
        } else {
            Assoc::Right
        }
    }
}

impl<T: Carry> Carry for Option<T> {
    fn carry(&self, out: &mut Out) {
        match self {
            Some(v) => {
                out.u8(1);
                v.carry(out);
            }
            None => out.u8(0),
        }
    }

    fn uncarry(inp: &mut In) -> Self {
        (inp.u8() != 0).then(|| T::uncarry(inp))
    }
}

impl<T: Carry> Carry for Box<T> {
    fn carry(&self, out: &mut Out) {
        (**self).carry(out);
    }

    fn uncarry(inp: &mut In) -> Self {
        Box::new(T::uncarry(inp))
    }
}

impl<T: Carry> Carry for Vec<T> {
    fn carry(&self, out: &mut Out) {
        out.usize(self.len());
        self.iter().for_each(|v| v.carry(out));
    }

    fn uncarry(inp: &mut In) -> Self {
        let n = inp.usize();
        (0..n).map(|_| T::uncarry(inp)).collect()
    }
}

impl<T: Carry, const N: usize> Carry for [T; N] {
    fn carry(&self, out: &mut Out) {
        self.iter().for_each(|v| v.carry(out));
    }

    fn uncarry(inp: &mut In) -> Self {
        std::array::from_fn(|_| T::uncarry(inp))
    }
}

impl<A: Carry, B: Carry> Carry for (A, B) {
    fn carry(&self, out: &mut Out) {
        self.0.carry(out);
        self.1.carry(out);
    }

    fn uncarry(inp: &mut In) -> Self {
        let a = A::uncarry(inp);
        (a, B::uncarry(inp))
    }
}

impl<A: Carry, B: Carry, C: Carry> Carry for (A, B, C) {
    fn carry(&self, out: &mut Out) {
        self.0.carry(out);
        self.1.carry(out);
        self.2.carry(out);
    }

    fn uncarry(inp: &mut In) -> Self {
        let a = A::uncarry(inp);
        let b = B::uncarry(inp);
        (a, b, C::uncarry(inp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{LinesEnd, Parser, ScopeStack};

    /// What the value `src` gives shows, run over `start` as the command line runs it.
    fn shown(start: &mut Start, src: &str) -> String {
        let (store, trie, core) = start;
        let core = &*core;
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        scopes.push_section(store, core.scope);
        let mut p = Parser::new(src, store, trie, core, scopes).with_lower(&core.lower);
        p.read_lines(LinesEnd::TextEnd).expect("the program parses");
        let (node, bits) = p.finish().expect("the program runs").expect("it gives a value");
        // SAFETY: `node` is the dyad the parser built whose value `bits` is.
        let shown = unsafe { crate::identities::display_value(core, node, bits) };
        p.end_tail().expect("its last line ends");
        shown.expect("its value shows")
    }

    const PROGRAMS: [&str; 4] = [
        "1 + 1",
        "a := i32 7, f := fn (n := i32 ?) -> i32 ( n * a ), f.compile(), f(6)",
        "error.too_big := «a number is too big», \
         half := fn (n := i32 ?) -> i32! ( if n > 100 ( error.too_big «{n} is too big» ), n / 2 ), \
         r := half(300), «{r holds err} {r.err.size} {r.err[0].message}»",
        "point := type ( x := pub i32 ?, y := pub i32 ? ), p := point(3, 4), p.x * p.y",
    ];

    #[test]
    fn a_start_from_the_carried_graph_is_the_start_built_from_source() {
        let image = write(&build(), &build()).expect("two starts write one graph");
        let loaded = load(&image).expect("this build reads the graph it wrote");
        let again = write(&loaded, &build()).expect("a carried start writes its graph");
        assert!(again == image, "the carried start writes the graph it was read from");
    }

    #[test]
    fn a_program_runs_alike_over_the_carried_graph_and_the_built_one() {
        let image = write(&build(), &build()).expect("two starts write one graph");
        for program in PROGRAMS {
            let built = shown(&mut build(), program);
            // Two carried starts at once, each fixed up to its own arena.
            let mut first = load(&image).expect("this build reads the graph it wrote");
            let mut second = load(&image).expect("this build reads the graph it wrote");
            assert_eq!(shown(&mut first, program), built, "{program}");
            assert_eq!(shown(&mut second, program), built, "{program}");
        }
    }

    #[test]
    fn a_graph_another_build_wrote_is_not_read() {
        let mut image = write(&build(), &build()).expect("two starts write one graph");
        let key = MAGIC.len() + 4 + 8;
        image[key] ^= 1;
        assert!(load(&image).is_none());
        assert!(load(&[]).is_none());
    }

    #[test]
    fn a_start_holding_an_address_into_the_binary_is_refused() {
        static IN_THE_BINARY: u8 = 0;
        let addr = &IN_THE_BINARY as *const u8 as u64;
        let [mut first, mut second] = [build(), build()];
        for (store, _, core) in [&mut first, &mut second] {
            let u64_ty = core.numtypes[crate::identities::numtype::NumType::U64 as usize];
            store.alloc_blob(u64_ty, &addr.to_ne_bytes());
        }
        let refused = write(&first, &second).expect_err("an address into the binary is refused");
        assert!(refused.contains("stray address"), "{refused}");
        let mut empty = Places::of(&first.0, &first.0.sides().expect("a start's sides"));
        empty.mapped.clear();
        assert!(empty.stray(addr as usize), "refused where the memory map is not listed");
    }

    #[test]
    fn a_start_a_program_ran_in_is_refused() {
        let mut ran = build();
        shown(&mut ran, "x := i32 1, x + 1");
        assert!(write(&ran, &build()).is_err());
    }
}
