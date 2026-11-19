# Velnor backlog: issue file format (v1)

One Markdown file per issue: `backlog/<KEY>.md`. A script creates the GitHub issues from these files after the owner confirms, so the header must parse exactly.

## Header (YAML front matter, required fields)

```yaml
---
key: VLN-X-03-2           # stable plan key: VLN-<stream>[-<item>[-<sub>[-<subsub>]]]; never reused
level: L3                 # L1 epic (one per stream), L2 task (one per delivery-plan item), L3 sub-issue, L4 sub-sub-issue
title: "[X03.2] Port the run-service acquirejob client"   # "[<item>.<sub>]" code, then an imperative title of 50–80 chars
parent: VLN-X-03          # the key of the parent; L1 parents are the root "VLN" epic
blocked_by: [VLN-X-01, VLN-P-02-1]   # keys only, any stream; empty list if none
milestone: "D1"           # a delivery-plan milestone id, defined in milestones.md by the manager
stream: X
plan_items: [X03]         # the delivery-plan item(s) this issue implements
size: M                   # XS ≤1 h, S ≤3 h, M ≤8 h, L ≤16 h agent time. Split anything bigger.
impl: {model: claude-sonnet-5-5, effort: medium}     # who implements: see "Choosing models" below
review: {model: claude-opus-5-5, effort: high}       # who reviews: never the same model as impl
deletes: ["crates/velnor-runner/... official-runner wrapper"]  # what this issue removes (decision 18); [] if none
existing: [1324]          # open PRs or issues already doing part of this work; [] if none
owner_decision: null      # an owner question id (e.g. D4) this issue waits for, or null
---
```

## Body, L1 and L2 (parents)
Sections in this order: `## Goal` (2–4 sentences), `## Exit check` (numbered commands with expected output that prove the whole subtree is done), `## Source of truth` (links to design sections, `docs/proposed/velnor-runner-architecture.md#...`), `## Sub-issues` (keys, in order), `## Sequencing` (what can run in parallel), `## Done when` (checklist).

## Body, L3 and L4 (leaves), written so a WEAK model can execute it without guessing
Sections in this order:
1. `## Summary`: one sentence, plus the single command that proves it is done.
2. `## Goal`: what changes, and why it matters for speed or correctness.
3. `## What exists today`: exact paths with line ranges, read from the code at the frozen commit. No absolute local paths.
4. `## Read first`: at most 5 links, design sections or files.
5. `## Test first`: the failing test to write first: the file, the test name and what it asserts.
6. `## Steps`: numbered. Each step names the exact file and what to change, then `Run:` (an exact command from the repository root) and `Expected:` (exact output or exit code). Keep it to 3–12 steps.
7. `## Do not`: the files and behaviours to leave alone, and the shortcuts that are forbidden (for example, never weaken a test, never add a compatibility shim).
8. `## Deletes`: what to delete in this same change, with the exact paths.
9. `## Acceptance`: a checklist with objective checks only (commands and their output).
10. `## If blocked`: do not ask anyone. Push the branch, comment with the last 40 lines of output and what you tried, add the label `blocked`, and stop.
11. `## Review checklist`: what the reviewer model must verify, 3–8 items.

## Choosing models (impl and review)
Pick the cheapest model that will succeed reliably. Strong models only where judgement matters.
- Mechanical edits, renames, config, docs, test scaffolding: `claude-haiku-5-5` low or medium, or `gpt-6-luna` medium.
- Ordinary implementation from precise steps: `claude-sonnet-5-5` medium, or `gpt-6.1-sol` medium.
- Protocol, concurrency, security, correctness-critical (gate, ledger, keys, recovery), tricky ports: `claude-opus-5-5` high, or `gpt-6.2-sol` high.
- Architecture or redesign decisions (the RV items, new traits): `claude-opus-5-5` high, or `claude-fable-5-2` high.
- The reviewer is at least as strong as the implementer, and never the same model. Correctness-critical work is reviewed by a strong model at high effort.
Allowed model ids: claude-haiku-5-5, claude-sonnet-5-5, claude-opus-5-5, claude-fable-5-2, gpt-6-luna, gpt-6.1-sol, gpt-6.2-sol, gpt-6.2-astra, gemini-4-hydrogen, gemini-4-carbon, gemini-4-argon, gemini-4-neon, muse-spark-1.3, muse-wonder-1.0. Efforts: low, medium, high, xhigh, max.
