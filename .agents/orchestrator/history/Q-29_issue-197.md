# Q-29: #197: a `type (…)` in a function that waits for the call: built partly too, or waits whole?

## Metadata
- **Status:** answered, relayed 2026-09-30 01:14
- **Priority:** 1 (why: the last point the #197 review waits for; one letter each)
- **Asked:** 2026-09-30 00:57
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197), when `immediate` runs and where a body is built
- **Branch and worktree:** `issue-197-build-where-written`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-197)
- **Asked by:** logoslang-issue-197-review-1 (session 34384c84), Opus 5.5
- **Waiting:** the #197 review waits for this
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-28](../history/Q-28_issue-197.md): your answers 1 to 3, now recorded; this is the one case they leave open

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

## 1. A `type (…)` in a function that needs the call's value: built partly, or waits whole?

Your answer 3 made an `immediate` that hits a `?` an error where it is written. One case is left, because your words fit both answers: a `type (…)` written inside a function that needs the call's value.

```logos
mk := fn (t := type ?) -> type ( type ( share e := immediate t ) )
# (a) error where it is written: `t` holds nothing when `mk` is defined
# (b) fine: at each call the type is built with that call's `t`, so mk(i32).e == i32
error and immediate is actually unecessary here because share needs it to be immediate anyways. its almost like share could replace immediate in all cases it seems. also something different: i came to see that the second written type isnt actually needed because the first type should interpret the scope with fields anyways. that probably doesnt work now though. but this basically implies that fn doesnt need to name a type explisitly after ->. it can also infer it. either there is only one return type, or mints are needed per return type + arguemnt types set. 

mk2 := fn (t := type ?) -> type ( type ( share e := t, share g := immediate ( print «hi», 1 ) ) )
print «defined», a := mk2(i32), b := mk2(f64)
# (a) hi defined: this `immediate` needs nothing from the call, so it runs once, where it is written
# (b) defined hi hi: the type waits whole for `t`, and each new `t` builds it again
```
error because t isnt defined in share e := t. but but other than that a is correct. 

- (a) **Built partly too:** one rule for every body: an `immediate` runs where it is written, or is an error there. `share e := t`, without `immediate`, still gives one type per `t`, so nothing is lost.
- (b) **Waits whole:** an `immediate` in such a type runs again at each build: the case your answer 3 removed for `fn` and `run` bodies.

Recommended: (a), because it is your answer 3's reason with no exception. Its cost: the 29 September example, where `immediate t` in `mk` was legal, goes.

**Answer 1:** 
a but ive added some comments under each example. 

## 2. To confirm: a `type (…)` in a body is built where it is written only when everything it reads already has its value there

On 29 September you chose "built where written, since everything is known". The record then said "one that reads no name of its body", which goes further than your reason:

```logos
mut t := type ?, f := fn () -> type ( type ( share e := t ) ), t = i32, f().e == i32
# prints true today; under "reads no name of its body" the type would be built when `f` is defined, while `t` still holds nothing
```

The review wrote it back as "only when everything it reads holds its value there".

- (a) **Yes**, that is what you meant.
- (b) **No:** say what you meant.

**Answer 2:** 
f depends on t existing and when t changes the output of f also changes. that is how functions already work