# Q-45: What is NULL for a value kept in its own bytes, like `bag(1)`? And does a `return` check NULL as a failing line does?

## Metadata
- **Status:** answered 2026-10-01 02:52; answers 1 and 2 relayed to #192, answers 3 to 5 recorded on [#240](https://github.com/ThobiasKnudsen/LogosLang/issues/240#issuecomment-5922486750), the word "node" for a block on [#205](https://github.com/ThobiasKnudsen/LogosLang/issues/205#issuecomment-5922495654)
- **Priority:** 0 (why: #192 is the first root in your order, #215 and #82 wait for it, and its third example needs this before it merges; one letter each)
- **Asked:** 2026-10-01 01:23
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#192](https://github.com/ThobiasKnudsen/LogosLang/issues/192), a value's teardown runs where its life ends
- **Branch and worktree:** `issue-192-teardown-at-life-end`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-192)
- **Asked by:** logoslang-issue-192-review-2 (session 247213f5), Opus 5.5
- **Waiting:** answers 1 and 2 are built on #192. Question 3 no longer holds #192 back: #192 merges with a stand-in, and question 3 is filed as [#240](https://github.com/ThobiasKnudsen/LogosLang/issues/240), a record kept in its own bytes has no NULL. Its fix waits for your answer.
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-43_issue-192.md](file:///home/o/Personal/Code/LogosLang/.agents/orchestrator/answers/Q-43_issue-192.md): Q-43, your NULL-check answers this question comes from
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›`free` and `move` end a name; no drop flag‹, ›A filled `share free` is the value's teardown; one name owns each value‹, ›A field is filled at run, per evaluation; its type decides how the operand is used‹, ›A last value moves out‹, ›A value's teardown runs where its life ends; the ending identity reads the type's `free` slot‹

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

Your Q-43 answer 3 was "a": `free` and `move` write NULL into the place they end, and a line that fails frees every place that is not NULL. That option said: "Every value with a `free` slot then needs a NULL form: a pointer has one, but a value stored directly in its place needs one too." This question asks which NULL form.

## 1. What is NULL for a value kept in its place's own bytes?

A record's place holds the record's bytes, not a pointer to them (›A field is filled at run, per evaluation‹: "At run it is a place in the node's frame holding what the declared type says: the operand's value at that type"). Here is the `bag` from Q-43's third example:

```logos
bag := type ( mut n := i32 ?, share free = ( print «freed {n}» ) ),
a := bag(1),     # a's own 4 bytes hold n = 1
free a           # must write NULL into those 4 bytes
```

Every 4-byte pattern is a real `bag`, and `bag(0)` is all zeros. So no pattern is left over to mean NULL.

- (a) **All-zero bytes are NULL.** `free a` zeroes `a`. This is cheap. But when a line fails, a live `bag(0)` looks empty and is never freed, so its `freed 0` never prints. On a line that finishes, the same `bag(0)` is freed.
- (b) **A value whose type fills `free` always lives in a block of its own.** `a := bag(1)` puts the bag in its own block, and `a` holds the address. NULL is then 0, as for every pointer, so every value with a `free` has the same NULL. `b := a` hands out the same address, which is what ›A filled `share free` is the value's teardown; one name owns each value‹ already says: "`b := a` and `f(a)` **borrow**". The reviewer says `array` values already work this way in the seed. Cost: one block per such value, and one more hop for each field read. It also changes something the reviewer sees on the branch today: `mut a := bag ?`, never written, prints `freed 0` at its end. Under (b) it frees nothing, as an unwritten `own @i32 ?` does.
- (c) **The type names its NULL.** A type kept in its bytes that fills `free` also says which of its values means NULL, the way Rust keeps `None` of a file handle as -1. Cost: more work for the type's author, and a type like `bag`, where every value is real, has nothing to give.
- (d) **One hidden byte beside such a value.** It is set at `:=`, cleared by `free` and `move`, and read only when a line fails. Cost: that byte is the drop flag that ›`free` and `move` end a name; no drop flag‹ says does not exist: "there is no run-time drop flag".

Recommended: (b), by the reviewer and by me. It makes your answer 1 true as you wrote it, "all things that could possibly be freed when error occurs is not NULL", for every value with a `free`. It also removes the seed's second teardown path, the one for records kept in their bytes.

**Answer 1:** 
i said that every pointer should become NULL after its freed which lex«@».type.free should be defined as doing. no other fields needs to be written to NULL, right?

## 2. A `return` in the middle of a line, before that line's `free`: is the name freed?

The reviewer found this on the branch, in both tiers:

```logos
bag := type ( mut n := i32 ?, share free = ( print «freed {n}» ) ),
f := fn (c := i32 ?) -> i32 (
    a := bag(1),
    x := (if (c == 1) ( return 5 ) else ( i32 2 )) + ( free a, i32 1 ),
    x
),
print «{f(1)}»
# prints 5, never "freed 1"; f(0) prints "freed 1" then 3, as it should
```

DESIGN says `a` must be freed here. The `return` runs before `free a`, so `a` still holds its bag, and "the teardowns of the scopes it leaves run on the way out" (›A last value moves out‹), "for each value a place of the scope still holds" (›A value's teardown runs where its life ends; the ending identity reads the type's `free` slot‹). The seed skips it because the parse counts `a` as ended by that line. What DESIGN does not say is how the `return` knows that `a` is still held. Your Q-43 answer says lines that do not fail still check nothing, and a `return` is not a failure.

- (a) **A `return` checks NULL, as a failing line does.** The scopes it leaves free every held place that is not NULL. One rule covers every line that stops before its end, by a fault or by a `return`, and the same code serves both. Cost: one NULL check per held name at each `return`. A line that runs to its end still checks nothing.
- (b) **The parse works it out from the order inside the line.** Here `a` must be freed, because its `free` comes after the `return`. Nothing is checked at run time. This is the "order of the line" option you turned down for failing lines in Q-43 (question 3). Cost: more parse bookkeeping, and a `free` inside a nested block on the `return`'s path must not be counted twice, which is easy to get wrong.
- (c) **Refuse a `return` on a line that ends a name after it.** `a` would have to be freed on its own line first. Cost: some code must be rewritten.

Recommended: (a), by the reviewer and by me. It is your Q-43 NULL rule widened from "a line that fails" to "a line that does not reach its end". With answer 1's (b), it works the same way for every value with a `free`.

**Answer 2:** 
the life of a ends inside the if branch where return is so since a:end is at that place (i know it could be at other places as well) it should be freed when return exists the scope

## 3. Your answer 1: right for memory, with one gap

Answer 2 is relayed: the parse puts an end of `a` at the `return`, and the `return` frees `a` as it leaves, with nothing checked at run time.

You asked in answer 1: "no other fields needs to be written to NULL, right?" Right, for memory. `@`'s `free` writes NULL into the pointer it frees, and freeing a NULL pointer does nothing. A record's `free` frees its pointer fields, so if it runs a second time it finds them NULL and frees nothing. No other field needs NULL for that.

The gap: when a line fails, the teardown cannot tell whether a record kept in its bytes was already freed on that line, so its `free` can run a second time. That is harmless when the `free` only frees pointers. A `free` that does anything else does it twice:

```logos
bag := type ( mut n := i32 ?, share free = ( print «freed {n}» ) ),
g := fn () -> i32 ( error «stop» ),
f := fn () -> i32 ( a := bag(1), x := ( free a, g() ), x ),
f()
# `free a` prints "freed 1"; then g fails, the teardown of f's scope finds `a`,
# which cannot be NULL, and frees it again: "freed 1" a second time
```

The same holds for `close` on a file handle, which ›A type whose fields carry teardowns must write its own destructor‹ names beside `free`: a handle kept as a plain number would be closed twice.

- (a) **Yes, only pointers get NULL.** A record's `free` may run a second time on a failing line, so a `free` that does more than free pointers must be safe to run twice. ›`free` and `move` end a name; no drop flag‹ ("no teardown runs over an emptied place") gets a Ruled line naming this exception.
- (b) **Yes, only pointers get NULL, and a record with a `free` is kept behind one** (question 1's (b)). `a := bag(1)` is then a pointer, `free a` makes `a` NULL like any pointer, and a failing line skips it. No exception in DESIGN, and your rule covers every value with a `free`. Cost: one block per such value.

Recommended: (b). It is your rule, "every pointer should become NULL after its freed", applied to the one kind of value that had no pointer.

**Answer 3:** 
b, but i see now with this example that its actually not ideal that a free runs twice even though there are no pointers to free, but it at least should work for now, so a problem should be recorded that sometimes free could be called multiple times on one name when it isnt a pointer which is not ideal

## 4. Answer 3: which one did you mean?

Under (b), `free` never runs twice, so your second half ("it at least should work for now", "free could be called multiple times") reads like (a):

```logos
f := fn () -> i32 ( a := bag(1), x := ( free a, g() ), x ),
f()
# (b): "freed 1" once: `a` holds the bag's address, `free a` writes NULL into `a`, the teardown skips it
# (a): "freed 1" twice
```

- (a) **(b), built in #240.** `free` never runs twice on one name, so no problem is left to record; #240 keeps why. Until #240 lands, #192's stand-in decides.
- (b) **(a) for now.** A record's `free` may run twice on a failing line, and DESIGN says so in a Ruled line. The double run is recorded as a problem in PWS, and its solution talk decides the lasting fix, maybe (b).

**Answer 4:** 
i think i see how this would be solved nicely maybe. when some node is freed the type, which is a pointer is sat to NULL as well. you should not need its type either if im right

## 5. Your idea: `free` writes NULL into the type word

You are right that the type is not needed after `free`, and a node has a type word to write NULL into: "A node is one 8-byte reference to a block whose first word is the type" (›A dyad is a type and a value: one block, the type word first, and its identity is its address‹).

The catch is that a name's own bytes are not a node. `a := bag(1)` keeps only `n`'s 4 bytes at `a`'s offset in the frame, and `a`'s type comes from its binding (›A scope lays out its declarations; a use reaches the offset through its binding‹: "a node's value word carries no mark"). There is no type word there to write NULL into. So your idea needs one of these two:

```logos
a := bag(1),
free a
# (a): a holds an address; the block [bag][n] is freed, and NULL goes into a
# (b): a's place is [bag][n] in the frame itself; free writes NULL over [bag]
```

- (a) **The bag lives in a block of its own** (question 3's (b)). `a` holds its address, and `free a` writes NULL into `a`, as for every pointer. The block's own type word cannot hold the NULL, because the block is given back. Cost: one block per such value, and one more hop for each field read.
- (b) **The place keeps the type word before the bytes** (your idea, closest). `a`'s place in the frame is `[bag][n]`: 8 more bytes per place whose type fills `free`, but no block and no extra hop. `free a` writes NULL over `[bag]`, and a failing line skips a place whose type word is NULL. Cost: that word is the run-time flag ›`free` and `move` end a name; no drop flag‹ says does not exist ("there is no run-time drop flag"). That rule would get a Ruled line saying the type word is the flag.

No recommendation: (a) trades a block and a hop for no new rule; (b) trades 8 bytes per place and a Ruled line for no block.

On the problem you asked me to record: a problem needs a program that shows `free` running twice today. #192's reviewer is running your example on its branch in both tiers. If either tier runs `free` twice, I record it as a problem; if not, the concern stays on #240, where this ruling lands.

**Answer 5:** 
yes freeing the type from the block (which should not be named block but node. add that to the renaming root cause) should not be allowed since it corrupts the node. basically the root cause problem as i see it is that a name has onlyone :end but it could have multiple in many cases, and if im right being able to express all places it could end like in rust would likely solve this. but i dont know if rust is doing it exactly this way though. i just know the borrow checker is rewritten to only need some few facts about each name to borrow check everything in an extensible way
 