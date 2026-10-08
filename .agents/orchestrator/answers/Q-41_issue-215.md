# Q-41: after #192, which runs first: the `.type` rename (#215) or #82's first slice?

## Metadata
- **Status:** answered, relayed 2026-10-01 01:16
- **Priority:** 2 (why: nothing waits yet; once #192's review is clean, the next builder needs this; one letter)
- **Asked:** 2026-09-30 02:40; cut to one question on 2026-09-30 after your answer in chat
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#215](https://github.com/ThobiasKnudsen/LogosLang/issues/215), the `.type` rename; [#82](https://github.com/ThobiasKnudsen/LogosLang/issues/82), a node carries its type
- **Branch and worktree:** none
- **Asked by:** Orchestrator (2), for the order of work
- **Waiting:** the next builder after #192 merges
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [#192](https://github.com/ThobiasKnudsen/LogosLang/issues/192), teardown where a value's life ends: built, in review now
  - [the order you set, recorded on #82](https://github.com/ThobiasKnudsen/LogosLang/issues/82#issuecomment-5902373996)

<!-- Write your answer on the **Answer** line under the question, or anywhere else in the file, and save. -->

Your answer in chat, "update the issues accordingly based on your findings", set the order, and it is recorded on #82:

1. Finish #192 (teardown where a value's life ends).
2. #82's first slice, alone: every built-in's parse writes the type its node gives back, and the five Rust functions that guess it today go.
3. The per-site roots in their present order: the parse line (#197's seed change, #223, #234 then #235, #230) beside the value line (#198, then #211 slice 2, #212, #203, #227, #229, then #218 and #224). #226, the DESIGN-only run, right after #192 and #197's DESIGN record merge, as before.
4. #127 (`fn` is a type), then #164 (one definition per identity, then the IR).

That settles the old questions 1 and 3. One is left.

## #215 and #82's first slice both touch most files, so each runs alone. Which goes first after #192?

- (a) **#215 first** (`a:type` becomes `a.type` everywhere). Every later change, #82's slice included, is written with `.type`, as #171 went first for `move`/`free`. #82's slice starts one rename later.
- (b) **#82's first slice first.** The deeper fix starts sooner; #215 then also renames the `:type` reads in the slice's new tests.

Recommended: (a). The rename is mechanical, and putting every later change in the final spelling is why #215 was placed right after #192.

**Answer:** 
a
