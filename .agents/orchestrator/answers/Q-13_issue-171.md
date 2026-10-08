# Q-13: #171 is merged; which root starts next?

## Metadata
- **Status:** answered, relayed 2026-09-29 21:55
- **Priority:** 1 (why: the seed has no worker on it now; two short answers)
- **Asked:** 2026-09-29 21:20
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#171](https://github.com/ThobiasKnudsen/LogosLang/issues/171), merged at 1eabb7e
- **Branch and worktree:** `dev`
- **Asked by:** Orchestrator (2), Opus 5.5
- **Waiting:** no worker runs on the seed until you answer; analyses and reviews continue
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [#192](https://github.com/ThobiasKnudsen/LogosLang/issues/192), [#198](https://github.com/ThobiasKnudsen/LogosLang/issues/198), [#203](https://github.com/ThobiasKnudsen/LogosLang/issues/203), [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197): the roots in question

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

## 1. Which root next?

The order you set this morning was #171 → #192 → #127 → #164 → #99/#82. Today added roots, each with its own suggested place:

- **#192**, teardown where a value's life ends: next in your order.
- **The `free`/`move` place check** (in review now, not filed yet): suggested right after #171 and before #192, because #192 rebuilds what `free` does and they share `drop_model.rs`. Its first slice needs no ruling.
- **#198**, one landing check for a value in a typed place: suggested before #127.
- **#203**, one placing rule for a parse's writes: after #198, same code.
- **#197**, `immediate` and held types built once per set: after #171, now free to go.

Roots that share files run one at a time; a root with disjoint files can run beside another.

- (a) **#192 now**, as ordered this morning; the others after.
- (b) **The place check first, when its analysis files it** (minutes away), then #192.
- (c) **#192 and #197 side by side**, if their files are disjoint.

Recommended: (b), because #192 would otherwise rebuild `free` on top of the unchecked reader the place check replaces.

**Answer 1:** 
b

## 2. A helper for minting token records

#171's review noticed that making a token record (the thing `pub`, `mut`, `own` are) is written out by hand at about 17 places; `gate::word` now covers 2. One helper would cover all of them. It is older than #171 and changes no behaviour.

```logos
own.arity    # error: this read does not fit the node's type, like pub.arity
```

- (a) **File it as a leaf issue**, to run beside any root.
- (b) **Leave it.**

Recommended: (a); it is small, and it removes a hand-kept pattern.

**Answer 2:** 
why is the term token record used. what is record used for really? token should be called lexeme instead which is a node not parsed yet. also the term cell is used everywhere which is basically a node. all places where cell occurs should be replaced by node. are there other terms that are synonymous. there are often i dont understand what you mean because terms are used that i havent ruled or superseeded. this should the orchistation agent answer and create an issue for. for the actuall answer now: a
