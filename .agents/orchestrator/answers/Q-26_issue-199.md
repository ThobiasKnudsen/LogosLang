# Q-26: does `.lhs` written on an expression read the node, or what the expression evaluates to?

## Metadata
- **Status:** answered, relayed 2026-09-30 00:08
- **Priority:** 1 (why: #199's reviewer is paused until you answer; one letter answers it)
- **Asked:** 2026-09-30 00:00
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#199](https://github.com/ThobiasKnudsen/LogosLang/issues/199), a bare parameter `fn (a)` is a `dyad ?`, where your answers also moved a value's type to `.type`; the answer also shapes [#215](https://github.com/ThobiasKnudsen/LogosLang/issues/215), the `:type` → `.type` rename
- **Branch and worktree:** `issue-199-bare-param-dyad`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-199)
- **Asked by:** logoslang-issue-199-review-2 (session 70302031), Opus 5.5
- **Waiting:** the reviewer is paused until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your three `.type` answers](../answers/Q-22_issue-199.md): ".type reads the fields of what is evaluated, not the LG … if f returns a type with the fields x and y then f().x would access that value the same way"

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

For your information, no answer needed: two points of your own words stay as Open lines in DESIGN, and #199 does not wait for them. One is how a node's own kind (`+`) is read ("something else which im not sure of how right now"). The other is which read the fine reflection gate stands at, now that `.type` is not gated.

## 1. `.lhs` on an expression

You said `.type` reads what an expression evaluates to, and that operators name their operands `lhs` and `rhs`. A rule of 24 September reads `.lhs` the other way, on the node: ›Reading a path runs nothing‹, "So `(2 ^ 3).lhs` is `2` and `(x + 2).lhs` reads `x`."

```logos
x := i32 3,
(x + x).type,        # i32: your answer
(x + x).lhs,         # ?   (3 today)
b := x + x,
b:start.rhs.lhs,     # x: the + node reached through the graph; not in question
```

- (a) **`.` written on an expression always reads what it evaluates to.** `(x + x).lhs` is an error, since an `i32` has no field `lhs`. A node's `lhs` and `rhs` are read on the node reached through the graph (`b:start.rhs.lhs`, or `tape[0].lhs` inside a parse). One rule for every `.`: `f().x`, `(x + x).type` and `(x + x).lhs` all read the value.
- (b) **Only `.type` reads the value;** any other field written on an expression reads the node, so `(x + x).lhs` is `x`. Then `.` on an expression does two different jobs, chosen by the field's name.

Recommended: (a), because it is what your `f().x` reason gives: one meaning for `.`.

**Answer 1:** 
a and actually i dont see when (x + x).lhs is needed because where you write that you already know its x. but when you want to inspect LG from some other place which doesnt know its x you would probably start via somename:start or via the scope array of dyads. 
