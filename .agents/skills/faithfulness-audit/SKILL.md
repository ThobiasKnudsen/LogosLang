---
name: faithfulness-audit
description: Audit a spec area or change for faithfulness to the rules of `LogosLang` on the PolyLogue server — build a quoted claim inventory, cross-check every source layer for conflicts (blocking), then check code against the ruled spec. Use before implementing in a spec-governed area, when reviewing seed changes, or when any source disagrees with another.
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

1. **The rules of `LogosLang` on the PolyLogue server** (the `polylogue` tools,
   AGENTS.md *Faithfulness in this repo*) — the ruling document.
2. **language_sketch.logos** — illustrates the rules; where it conflicts, the rules win.
3. Downstream, never authoritative alone: GitHub issues and comments, plans,
   auto-memory, and the existing code. Staleness is invisible
   from inside a session — a source being newer, more concrete, or written by
   Thobias himself does not promote it.

## Procedure

### 1. Claim inventory
For the area under audit, find every governing rule with `rules` (by its words and the
code's names) and read each one whole with `rule`: verbatim quote + heading + id. If no
rule governs the thing being built, that is
itself a finding (**spec-silent**) — stop and ask before implementing.

### 2. Cross-check the source layers (conflicts BLOCK)
Check each claim against: other rules, the sketch, open issue bodies
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
Surface it to Thobias before applying fixes. Fixes to the rules and the sketch
use his wording or are explicitly drafted for his approval.

### 5. Record every ruling the same session
A ruling on a conflict, or a rejection made in conversation, is written at the
rule it touches immediately, as AGENTS.md *Faithfulness in this repo* says: a
**Ruled** line, a **Rejected, to stay rejected** line, or a new rule. An
unrecorded decision is a future bug.
