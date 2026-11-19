# Velnor backlog: issue format

One Markdown file per issue: `backlog/<KEY>.md`. A script creates the GitHub issues from these files only after the
owner confirms the [delivery plan](../delivery-plan.md) (ruling R12), so the header must parse exactly. The issues
follow `docs/roadmap.md` and the rules of root issue #574, so the existing roadmap loop can run them unchanged.

## Hierarchy and labels

| Level | What | Parent | Labels |
|---|---|---|---|
| L1 | One epic per phase: `[VELNOR-0] Correctness`, `[VELNOR-1] Trust and reuse`, `[VELNOR-2] Runner and Native`, `[VELNOR-3] Proof` | #574 | `epic`, `fast-ci` |
| L2 | One issue per delivery-plan item (W1–W10, V01, N25, …) | its phase epic | `fast-ci`, one `exec:*`, one `effort:*`; `needs-owner` while a gate is unanswered |
| L3 | A sub-issue when an L2 item is over 8 agent-hours, split at its first commit | its L2 issue | same as L2 |
| Owner step | `[OWNER] <what is needed>`, for example W6's ruleset change | #574 | `owner-action`, never `fast-ci` |

- **Dependencies** are GitHub's native "blocked by" relationships, created from `blocked_by`. The roadmap's ready-issue
  query reads them, so nothing else tracks order.
- **Milestones:** one per phase (`VELNOR-0` … `VELNOR-3`), created by the manager after the owner confirms. The existing
  `FAST-*` milestones are left alone.
- **Executor seat** comes from the labels `exec:haiku`, `exec:sonnet` or `exec:opus` and `effort:low`,
  `effort:medium` or `effort:high`. There is no other seat.
- **Reviewer** is named in the last section of the body, as `docs/roadmap.md` expects. Fable 5.2 and the efforts
  `xhigh` and `max` exist only there, never as labels.

## Header (YAML front matter, required fields)

```yaml
---
key: W3                       # the delivery-plan id; never reused
level: L2                     # L1 phase epic, L2 plan item, L3 sub-issue
title: "[W3] Plan the generator check and delete the engine leftovers"   # "[<id>]" then an imperative title, 50–80 chars
parent: VELNOR-0              # the L1 key, or the L2 key for an L3
blocked_by: [W1]              # plan ids only; [] if none
milestone: VELNOR-0
phase: 0
stream: W                     # W for phase 0, else the plan's stream letters (V, L, H, DG, AN, R, A, T, BM, G, RV, P, O, X, C, ST, S, F)
size_h: 8                     # agent-hours of building; L2 above 8 must list L3 children
impl: {seat: sonnet, effort: high}           # becomes exec:sonnet + effort:high
review: {model: opus-5.5, effort: high}      # haiku-5.5 | sonnet-5.5 | opus-5.5 | fable-5.2; effort low..max
gate: []                      # owner question ids from owner-questions.md (Q1–Q17, D7, D8, …); [] if none
state_at: 6fa056da02          # the main commit the "What exists today" section was read at
links:                        # every PR or issue that already covers part of this item
  - {ref: "#1757", state: merged, covers: "deleted run-authority and admit-preview"}
  - {ref: "#1778", state: merged, covers: "hardened plan --mode, write-request; confirm no caller before deleting"}
deletes: [".velnor/config.toml", "crates/velnor-ci/src/plan_request/"]   # what this change removes; [] if none
done_when:                    # the same checks as the body, as commands that read main or GitHub
  - "test ! -e .velnor/config.toml"
  - "rg -l 'plan_request|write_request' crates/velnor-ci/src  # prints nothing"
---
```

`links.state` is one of `open`, `merged`, `closed`. Add `disposition` for an open PR: `land-first`, `absorb`,
`supersede` (close it with a link), or `independent`.

## The "done when" rule

Every check reads **state**: the files on `origin/main`, the live ruleset or settings, workflow runs on main, labels,
or the state of a linked issue. A check never asks whether "this PR" did something. Three consequences:

1. Work that another session lands first closes the item. The executor proves it with the checks' output and closes
   the issue without opening a PR.
2. An item that is partly done says so in `links` and in "What exists today", and its steps cover only the remainder.
3. A check that needs a real run on `velnor-lab` names the run it expects; the PR then says `Refs #<n>` instead of
   `Closes #<n>`, and the issue closes when the numbers are posted (#574, rule 4).

## Body, L1 (phase epic)

Sections in this order: `## Goal` (2–4 sentences), `## Done when` (the checks that prove the whole phase),
`## Source of truth` (links to the delivery plan section and the rulings), `## Items` (keys in plan order),
`## Owner questions` (the Q ids this phase waits for, with their defaults).

## Body, L2 and L3, written so the named seat can execute it without guessing

1. `## Summary`: one sentence, and the single command that proves it is done.
2. `## Before you start`: run every check under "Done when" against `origin/main` and read each linked PR. If all
   pass, post the output and close the issue. If some pass, tick them and do only the rest.
3. `## Goal`: what changes and why it matters for correctness or speed.
4. `## What exists today`: exact paths with line ranges, read at `state_at`. Name what linked PRs already landed.
   No absolute local paths.
5. `## Read first`: at most 5 links (plan section, ruling, files).
6. `## Test first`: the failing test to write first: file, test name, what it asserts.
7. `## Steps`: 3–12 numbered steps. Each names the exact file and change, then `Run:` (a command from the repository
   root) and `Expected:` (exact output or exit code).
8. `## Do not`: files and behaviour to leave alone; forbidden shortcuts (weakening a test, a compatibility shim,
   hand-merging a generated file instead of regenerating it).
9. `## Deletes`: what this change removes, with exact paths.
10. `## Done when`: checkboxes, one per `done_when` entry, each with the command and its expected output.
11. `## If blocked`: as `docs/roadmap.md` says: retry once on the next stronger seat (Opus retries once at high
    effort), then push the branch, comment with the last 40 lines of output and what was tried, add `blocked`, remove
    `in-progress`, and stop. A box only the owner can prove becomes an `[OWNER]` issue.
12. `## Review`: the reviewer's model and effort, then 3–8 things to verify, rerunning the "Done when" checks it can.

## Choosing seats

Follow #574's model table:

| Seat | Use for |
|---|---|
| Haiku 5.5 (`exec:haiku`) | Mechanical work with exact instructions and a command that proves it |
| Sonnet 5.5 (`exec:sonnet`) | A change the issue specifies fully. The default, at medium effort |
| Opus 5.5 (`exec:opus`) | An open design choice, a security boundary, an unfamiliar tool or protocol, an experiment that decides later work |

- `effort:high` where a mistake would be silently wrong: the gate, selection, reuse, cache keys, credentials and
  releases.
- The reviewer uses the executor's seat or a cheaper one at low effort, unless the item is critical. A critical item
  (the same list as `effort:high`) gets Opus 5.5 at high effort.
- Fable 5.2 at xhigh reviews only soundness and trust-boundary items: in the current plan W2, W10, V01, V04, DG03, N26,
  L03, V07, V08 and C05. If the owner answers Q11 "labels only", these fall back to Opus 5.5 high.
- An executor moves to a stronger seat only after a documented failure, and the PR says so.

Allowed values: `impl.seat` is `haiku`, `sonnet` or `opus`; `impl.effort` is `low`, `medium` or `high`;
`review.model` is `haiku-5.5`, `sonnet-5.5`, `opus-5.5` or `fable-5.2`; `review.effort` is `low`, `medium`, `high`,
`xhigh` or `max`.
