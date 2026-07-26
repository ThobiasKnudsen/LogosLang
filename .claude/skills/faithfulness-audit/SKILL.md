---
name: faithfulness-audit
description: Audit a spec area or change for faithfulness to DESIGN.md — build a quoted claim inventory, cross-check every source layer for conflicts (blocking), then check code against the ruled spec. Use before implementing in a spec-governed area, when reviewing seed changes, or when any source disagrees with another.
---

# Faithfulness audit

The failure this skill exists to prevent, observed 2026-07-17: a design decision
made in conversation (no per-struct name hashtable; the name index + scope
filtering is the one resolution mechanism) was never recorded, three artifacts
kept teaching the stale direction back (the sketch's `names : hashtable` member,
an issue comment listing it as open work, a reconcilable-but-ambiguous DESIGN
sentence), and multiple sessions then wrote and accepted unfaithful code from
the downstream sources without quote-checking the ruling one.

## Source precedence

1. **DESIGN.md** — the ruling document.
2. **language_sketch.logos** — illustrates DESIGN; where it conflicts, DESIGN rules.
3. Downstream, never authoritative alone: GitHub issues and comments, plans,
   session logs, auto-memory, and the existing code. Staleness is invisible
   from inside a session — a source being newer, more concrete, or written by
   Thobias himself does not promote it.

## Procedure

### 1. Claim inventory
For the area under audit, extract every governing DESIGN.md passage: verbatim
quote + line number. If no passage governs the thing being built, that is
itself a finding (**spec-silent**) — stop and ask before implementing.

### 2. Cross-check the source layers (conflicts BLOCK)
Check each claim against: other DESIGN sections, the sketch, open issue bodies
and comments, and auto-memory. Any disagreement is a **spec-conflict** finding.
Spec-conflicts block implementation in that area: surface them to Thobias as
questions, with both quotes, and do not frame options inside either side's
model. Never tie-break silently.

### 3. Check code against the ruled spec
Only after conflicts are ruled: compare the seed to the claims. Label each
divergence:
- **deliberate-deferred** — a known v1 gap; must carry a code comment naming
  the gap and the tracking issue. Missing label = finding.
- **drift** — code contradicts the spec; fix or file.
- **doc-rot** — spec is behind decided-and-implemented reality; draft the spec
  edit for Thobias's review.

### 4. Report before fixing
Output a table: claim (quoted), source(s), verdict, evidence (file:line).
Surface it to Thobias before applying fixes. Fixes to DESIGN.md and the sketch
use his wording or are explicitly drafted for his approval.

### 5. Record every ruling the same session
A ruling on a conflict, or a rejection made in conversation, is written into
DESIGN.md immediately — the existing pattern is the §Substrate vocabulary line
"Recorded as rejected, to stay rejected: …" — or, if wording must wait, logged
as an explicit pending-spec-edit in the session log AND auto-memory. An
unrecorded decision is a future bug.
