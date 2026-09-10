# Contributing to LogosLang

Thank you for looking. LogosLang is Apache-2.0, and contributions are welcome on the
terms below. Two of them are load-bearing and are checked by CI: every commit carries a
sign-off, and [`DESIGN.md`](./DESIGN.md) rules.

## Sign your commits (Developer Certificate of Origin)

Every commit must carry a `Signed-off-by` line naming its author:

```
Signed-off-by: Your Name <your.email@example.com>
```

Git writes it for you when you pass `-s`:

```sh
git commit -s -m "seed: the thing you did"
```

Forgot it on the last commit? `git commit --amend -s --no-edit`. On several commits?
`git rebase --signoff origin/dev`.

That line is a statement, not a formality. By adding it you certify the
[Developer Certificate of Origin](./DCO), version 1.1, which is reproduced in full in
this repository: that you wrote the change, or that you have the right to submit
somebody else's work under this project's license. It is the lightweight alternative to
a contributor agreement. Nothing to sign, nothing to post, no account anywhere. It
exists so the project can show, commit by commit, that every line in it was given
deliberately by someone entitled to give it.

Use a real name and an email you can be reached at. `pr.dco` in CI checks that each
commit in a pull request has a sign-off whose email matches the commit's author. Commits
authored before 11 September 2026 predate the rule and are exempt.

If your employer owns your work, get their clearance before you sign off. That is
exactly the situation the certificate is there to record.

### Signing off automatically

The repository ships an opt-in hook that appends the line for you:

```sh
git config core.hooksPath .githooks
```

It only adds the sign-off when one is missing, and only for your own commits. You can
still write it by hand or keep using `-s`.

## What CI requires

A pull request has to pass, and these run on every one:

| Check | What it does |
|---|---|
| `ci / rust` | `cargo fmt --check`, clippy with warnings as errors, build, `cargo test --release`, and the release smoke test |
| `ci / dco` | every commit in the pull request is signed off |
| `docs / test` | the docs guard's own self-test |
| `docs / validate` | the docs versioning model, including the freeze on released snapshots |

Run the first and the last two locally before you push:

```sh
cargo test --release
bash .github/scripts/docs-check.sh validate
bash .github/scripts/docs-check.test.sh
bash .github/scripts/dco-check.test.sh
```

`main` is protected and takes a pull request, never a direct push. Work on a branch off
`dev`.

## DESIGN.md rules

LogosLang is a spec-governed project. [`DESIGN.md`](./DESIGN.md) is the ruling document;
`language_sketch.logos` illustrates it. Issues, plans, comments, and the existing code
are downstream of it and can be stale.

- Before changing anything the spec governs, find the passage in `DESIGN.md` that
  licenses the change and quote it in the pull request. No passage means the change is
  not ready: open an issue and ask.
- If two sources disagree, stop and say so in the issue with both quotes. Do not pick a
  side quietly, and do not assume the newer one wins.
- A passage that no longer holds is kept and marked `superseded`. A live one carries
  `ruled` or `settled` and a date. Grep for either to see what is in force.

## The shape of a good change

- One logical change per pull request, and a subject line that says what it does:
  `seed: …` for the Rust bootstrap, `DESIGN: …` for the spec, `docs: …`, `ci: …`.
- New behaviour comes with a test. The seed's tests live in `tests/`.
- Public spelling changes touch `DESIGN.md`, the seed, `language_sketch.logos`, and the
  docs together, so no reader is left with the old word.
- Do not edit `docs/vX.Y.Z/` for a version at or below the newest release tag. Those are
  frozen, and the guard will fail the pull request.

## The name

The code is Apache-2.0 and yours to fork. The name is not: see
[`TRADEMARK.md`](./TRADEMARK.md). Fork freely, and give a fork that changes the language
its own name.

## Reporting something sensitive

For a security issue, email **thobknu@gmail.com** rather than opening a public issue.
