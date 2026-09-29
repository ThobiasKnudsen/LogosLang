## Project notes: LogosLang

- The work is the Rust seed of the Logos language. Probe a behaviour by running a Logos program through `target/release/logos`, on both tiers where a compiled path exists (`compile f`), and give the program with its exact output wherever you report one.
- Run `/faithfulness-audit` before spec-governed work. DESIGN.md is one rule per `###` heading: point at a rule by its heading ›like this‹, never by line number. What a ruling supersedes goes to DESIGN_HISTORY.md under the same heading, with why.
- Commits are signed off: `git commit -s` (the DCO guard checks it).
- `.vscode/` in the main checkout is Thobias's and untracked; leave it alone.
- Explain to Thobias in plain words, with a Logos example wherever one fits.
