# LogosLang: facts for every agent the orchestrator starts

- Words that tripped Thobias. Use one with him only if the same sentence says what it means; every word he asks about is added here (his choice, Q90, 9 October 2026):
  - "local": a name declared inside a function, its parameters and the names its body declares.
  - "slot": never use it; say "field" (Thobias, Q91: "slot is just another word it seems which is unecessary").
  - "held": say what holds what, in plain words.
- Never name an issue, problem, question, solution, cause or commit by its number alone: the same sentence says what it is. Thobias, 9 October 2026: "what is annoying is how often you refer to issues or other number without explaining what they are."

- Speed probes, run in every review round (DESIGN.md ›Reading a program takes time in step with its size; no walk runs once per path‹). Each probe is one command line to `target/release/logos`, built in a file under the orchestrator's `tmp/` folder, run on the branch and on `dev` with `timeout 60` and timed:
  - A chain of functions, at 10 and at 20 functions: `c := i32 1, h0 := fn () -> @i32 ( alloc 1 of i32 5 ), h1 := fn () -> @i32 ( if (c == 1) (h0()) else (h0()) ), h2 := …, 7`, each `h_i` calling `h_{i-1}` in both arms. It prints `7`; the time must grow about twofold, not a thousandfold. Name probe functions `h…`, never `f…`: `f32` and `f64` are type names, and a chain that reaches them redeclares the float type.
  - A wide `if` at the top level, at 200 and at 2,000 arms: `c := i32 1, x := if (c == 1) (alloc 1 of i32 1) else if (c == 2) (alloc 1 of i32 2) … else (alloc 1 of i32 2000), print «{x@}»`. The time must grow about tenfold, not a hundredfold. Run it at the top level and inside a function body: once it grew with the square of its length at the top level alone.
  - A function that hands on what it made is written `-> own @i32` (›A last value moves out‹), and `-> @i32 ( alloc … )` is the checked error, so the chain probe uses `-> own @i32`.
  - The agents' shell is zsh: an unquoted `$a` that holds several words stays one argument, so `for a in "chain 10 own @i32"; do bash speed6.sh $B $a; done` passes one. Write each probe call's arguments out.
  - One command-line argument holds at most 128 KiB on Linux ("Argument list too long"): the `-> own @i32` chain fits about 1,400 functions, not 2,000; a plain `-> i32` chain with no `alloc` is shorter per function and slows the same way.
- To see where a slow read spends its time: ptrace attach is refused on this machine, so run `logos` as gdb's child, send it SIGINT, then `thread 2` and `bt`; the parse runs on thread 2.
