# Q-42: a `{…}` in a quote that is not `print`'s: code or text?

## Metadata
- **Status:** withdrawn unanswered 2026-10-01 01:48, not needed now (Thobias asked to remove the questions not needed): it was asked so the analysis could file, and PWS replaces that filing. The finding is problem P33 in PWS.json; this point comes back in the solution talk for P33's root.
- **Priority:** 3 (why: one analysis waits, and its fix would wait for #233 anyway; one letter)
- **Asked:** 2026-09-30 02:45
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none yet: the analysis `quote-brace-skip` files after your answer. Found by #233's review
- **Branch and worktree:** none, [analysis worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-quote-brace-skip)
- **Asked by:** logoslang-rca-quote-brace-skip-worker (session 4b796d0f), Opus 5.5
- **Waiting:** the analyst is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›`{ }` is text interpolation‹, ›`print «…»` is the output word‹
  - [RCA.md](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-quote-brace-skip/RCA.md): §1-4, 50 probes
  - [#233](https://github.com/ThobiasKnudsen/LogosLang/issues/233), one rule for which closer ends a bracket: in review
  - [#213](https://github.com/ThobiasKnudsen/LogosLang/issues/213), a text value can't be read yet; [#232](https://github.com/ThobiasKnudsen/LogosLang/issues/232), the `«` reader is also the string type (a question); [#234](https://github.com/ThobiasKnudsen/LogosLang/issues/234), five readers of a scope's lines

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

Today only `print` and `error` treat a quote's `{…}` as code. Every other quote keeps its braces as plain text:

```logos
print «a {zz} b»            # error: unknown name `zz`
y := lex «a {zz} b», 5      # 5: the braces are just text
regex «ab{2}» := type (…)   # the pattern is ab{2}, "two b's"
```

So a `print` inside an `if` that doesn't run is never checked, because the skip can't tell which quotes hold code:

```logos
print «a {( 1 ]} b»                                     # error: this bracket is never closed
if false ( print «a {( 1 ]} b» ) else ( print «ok» )    # ok
```

The sources disagree:
- DESIGN ›`{ }` is text interpolation‹: "A string is a scope that builds exactly its braces."
- The seed (`src/identities/string.rs:73`): a string holds "each `{…}` as written", and a test pins `«x {«y»}»` as `x {«y»}`.
- DESIGN ›`print «…»` is the output word‹ sits in between: "`{…}` outside `print` and `error` still waits on live strings." It says the splice waits, not what the braces are meanwhile.

## 1. Outside `print` and `error`, is a `{…}` code or text?

- (a) **Code in every quote, now**, and in a `#` comment. Every `«…»` builds its `{…}` where it is written: names looked up, brackets paired, a stray `}` an error. Where the value can't be turned into text yet, the `{…}` is refused for now, with "write `\{` for a brace". So `y := lex «a {zz} b», 5` says unknown name `zz`, a regex count is written `ab\{2\}`, and the skipped `print` above is refused like the built one. Two tests change (`«x {«y»}»`, and `# «note {a}»` in a held body).
- (b) **Text until live strings, as today.** Only `print` and `error` braces are code. A skipped `print` is checked only when something builds it, or the skip must know `print` and `error` by name (a hand-kept list).

Recommended: (a). It is what DESIGN's rule says, one place (the quote) decides which braces are code, and it fails closed: under (b), `lex «+ {n}»` means one thing today and another once live strings land, with no error in between. `language_sketch.logos:513` already writes code braces outside `print`.

With (a), the analysis files a new root (the quote decides its own braces), which waits for #233. With (b), nothing new is filed: the case goes on #233 as "not reached until live strings".

**Answer 1:** 
