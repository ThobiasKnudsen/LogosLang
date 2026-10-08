# LogosLang: facts for every agent the orchestrator starts

- Speed probes, run in every review round (DESIGN.md ›Reading a program takes time in step with its size; no walk runs once per path‹). Each probe is one command line to `target/release/logos`, built in a file under the orchestrator's `tmp/` folder, run on the branch and on `dev` with `timeout 60` and timed:
  - A chain of functions, at 10 and at 20 functions: `c := i32 1, f0 := fn () -> @i32 ( alloc 1 of i32 5 ), f1 := fn () -> @i32 ( if (c == 1) (f0()) else (f0()) ), f2 := …, 7`, each `f_i` calling `f_{i-1}` in both arms. It prints `7`; the time must grow about twofold, not a thousandfold.
  - A wide `if`, at 200 and at 2,000 arms: `g := fn () -> @i32 ( if (x == 1) (alloc 1 of i32 1) else if (x == 2) (alloc 1 of i32 2) … else (alloc 1 of i32 2000) )` after `x := i32 1`. The time must grow about tenfold, not a hundredfold.
