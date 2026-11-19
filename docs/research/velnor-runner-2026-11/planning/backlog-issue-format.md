# Velnor backlog: issue format

One Markdown file per issue: `backlog/<KEY>.md`. A script creates the GitHub issues from these files only after the
owner confirms the [delivery plan](../delivery-plan.md) (ruling R12), so the header must parse exactly. The issues
follow `docs/roadmap.md` and the rules of root issue #574, so the existing roadmap loop can run them unchanged. The
manager writes the files of a phase before that phase's issues are created; the body of W1 below is the model.

## Hierarchy and labels

| Level | What | Parent | Labels |
|---|---|---|---|
| L1 | One epic per phase: `[VELNOR-0] Correctness`, `[VELNOR-1] Reuse`, `[VELNOR-2] Runner and Native`, `[VELNOR-3] Proof` | #574 | `epic`, `fast-ci` |
| L2 | One issue per delivery-plan item (W0–W10, V01, N25, …) | its phase epic | `fast-ci`, one `exec:*`, one `effort:*`; `needs-owner` while a gate without a default is unanswered |
| L3 | A sub-issue of an L2 item | its L2 issue | same as L2 |
| Owner step | `[OWNER] <what is needed>`: an answer to a question without a default (`[OWNER] Answer Q3`), or an action such as W6's ruleset change | #574 | `owner-action`, never `fast-ci` |

- **Size.** One issue is one PR of at most 8 agent-hours. An L2 item above 8 h lists its L3 sub-issues in `children`;
  they are written with it, each has its own header, checks and PR, and the L2 issue closes when they close. Nothing
  is split later, at a first commit.
- **Dependencies** are GitHub's native "blocked by" relationships, created from `blocked_by` and from `gate`. The
  roadmap's ready-issue query reads only those, so nothing else tracks order.
- **Owner gates block natively.** For each id in `gate` that has no default in the owner sheet, and for each owner
  action, the script creates one `[OWNER]` issue and makes every item that names it blocked by it. The ready query
  then never returns a gated item. The `[OWNER]` issue closes only with the owner's answer, or the read-back of the
  action, posted on it. A gate id that has a default creates no blocker: the item runs on the default.
- **Milestones:** one per phase (`VELNOR-0` … `VELNOR-3`), created by the manager after the owner confirms. The existing
  `FAST-*` milestones are left alone.
- **Executor seat** comes from the labels `exec:haiku`, `exec:sonnet` or `exec:opus` and `effort:low`,
  `effort:medium` or `effort:high`. There is no other seat.
- **Reviewer** is named in the last section of the body, as `docs/roadmap.md` expects. It is never a label.

## Header (YAML front matter, required fields)

```yaml
---
key: W1                       # the delivery-plan id; never reused
level: L2                     # L1 phase epic, L2 plan item, L3 sub-issue
title: "[W1] Check rendered helper commands against the pinned helper"   # "[<id>]" then an imperative title, 50–80 chars
parent: VELNOR-0              # the L1 key, or the L2 key for an L3
blocked_by: [W0]              # plan ids only; [] if none
children: []                  # L3 keys; required when size_h is above 8
milestone: VELNOR-0
phase: 0
stream: W                     # W for phase 0, else the plan's stream letters (V, L, H, DG, AN, B, R, A, T, BM, G, RV, P, O, X, C, ST, S, F)
size_h: 8                     # agent-hours of building, at most 8 for an issue that has a PR
impl: {seat: opus, effort: high}             # becomes exec:opus + effort:high
review: {model: opus-5.5, effort: high}      # haiku-5.5 | sonnet-5.5 | opus-5.5 | fable-5.2; effort low..xhigh
gate: [Q3, D9]                # ids from owner-questions.md (Q1–Q19, D7, D8, …); [] if none
release_train: A              # A, B or C when the change ships in a tools release (plan section 4.1); else omit
state_at: 515d4c7ff1          # the origin/main commit "What exists today" was read at
links:                        # every PR or issue that already covers part of this item
  - {ref: "#1768", state: merged, covers: "nextest preflight argv"}
  - {ref: "#1555", state: open, disposition: independent, covers: "catch-all fallout list"}
deletes: ["the per-command version floors in crates/velnor-gen/src/helper_gate.rs"]   # [] if none
done_when:                    # the same checks as the body: a command and the result it must give
  - run: "git grep -nE 'HelperTooOld|BATCHED_HELPER' origin/main -- crates/velnor-gen/src/helper_gate.rs crates/velnor-rust/src/base_cache.rs"
    expect: "exit 1"
  - run: "gh run list -R tailrocks/velnor-new --workflow rust-ci.yml --branch main --event push --status completed -L 3 --json conclusion -q '[.[].conclusion]|unique|join(\",\")'"
    expect: "stdout: success"
---
```

- `links.state` is one of `open`, `merged`, `closed`. An open PR also has a `disposition`: `land-first`, `absorb`,
  `supersede` (close it with a link) or `independent`.
- `expect` is `exit <n>`, `stdout: <exact text>` or `run id: <what the run must show>`. A search that must find
  nothing expects `exit 1`, so a tool error (exit 2) fails the check instead of passing as "no output".
- `review` for the ten Fable† items of the plan is `{model: opus-5.5, effort: high}` until the owner answers Q11
  with (a); the script then writes `{model: fable-5.2, effort: xhigh}`.

## The "done when" rule

Every check reads **state**: the files on `origin/main`, the live ruleset or settings, workflow runs on main, labels,
or the state of a linked issue. A check never asks whether "this PR" did something. Three consequences:

1. Work that another session lands first closes the item. The executor proves it with the checks' output and closes
   the issue without opening a PR.
2. An item that is partly done says so in `links` and in "What exists today", and its steps cover only the remainder.
3. A check that needs a real run on `velnor-lab` names the run it expects. The lab pins the release train that carries
   the change; the PR says `Refs #<n>` instead of `Closes #<n>`, and the issue closes when the run ids are posted
   (#574, rule 4).

## Body, L1 (phase epic)

Sections in this order: `## Goal` (2–4 sentences), `## Done when` (the checks that prove the whole phase),
`## Source of truth` (links to the delivery plan section and the rulings), `## Items` (keys in plan order),
`## Owner questions` (the Q ids this phase waits for, with their defaults).

## Body, L2 and L3, written so the named seat can execute it without guessing

1. `## Summary`: one sentence, and the single command that proves it is done.
2. `## Before you start`: fetch `origin/main` and record the commit. Run every "Done when" check against it and read
   each linked PR. If all pass, post the output and close the issue. If some pass, tick them and do only the rest.
3. `## Goal`: what changes and why it matters for correctness or speed.
4. `## What exists today`: exact paths with line ranges, read at `state_at`. No absolute local paths.
5. `## Read first`: at most 5 links (plan section, ruling, files).
6. `## Test plan`: each test with the behaviour it proves, its positive and negative cases, and why no existing test
   covers it. An independent subagent approves the plan before any test is written (`AGENTS.md`).
7. `## Steps`: 3–12 numbered steps. Each names the exact file and change, then `Run:` (a command from the repository
   root) and `Expected:` (exact output or exit code).
8. `## Red and green`: for each new test, the command that shows it red on the code before the change and green
   after it. A second subagent runs both and posts the output.
9. `## Do not`: files and behaviour to leave alone; forbidden shortcuts (weakening a test, a compatibility shim,
   hand-merging a generated file instead of regenerating it).
10. `## Deletes`: what this change removes, with exact paths.
11. `## Done when`: checkboxes, one per `done_when` entry, each with the command and its expected result.
12. `## If blocked`: as `docs/roadmap.md` says: retry once on the next stronger seat (Opus retries once at high
    effort), then push the branch, comment with the last 40 lines of output and what was tried, add `blocked`, remove
    `in-progress`, and stop.
13. `## Review`: the reviewer's model and effort, then 3–8 things to verify, rerunning the "Done when" checks it can.

## Example body: W1

```markdown
## Summary
velnor-gen refuses to render a helper command the pinned helper cannot parse, and three tests keep main's known
causes of red from merging. Proof: the two `done_when` commands in the header.

## Before you start
Run: `git fetch -q origin main && git rev-parse --short origin/main`
Expected: a commit id; write it into this issue as the commit you read.
Run: `git show origin/main:.velnor/rust-ci.lock | sed -n 1p`
Expected: `version = "0.1.98"` or later.
Run: `git grep -cE 'HelperTooOld|BATCHED_HELPER' origin/main -- crates/velnor-gen/src/helper_gate.rs crates/velnor-rust/src/base_cache.rs`
Expected: exit 0 with two counts (the floors still exist). Exit 1 means another session removed them: tick box 1.
Then run every "Done when" check. Tick what passes; close the issue if all pass.

## Test plan (approve before writing)
1. Schema, negative: a schema that lacks one flag the renderer emits makes `generate` fail and name the flag.
   Positive: the real schema of the pinned helper accepts every rendered command of this repository.
2. End to end: render this repository's configuration, run the pinned helper's `plan` and its nextest preflight on
   the rendered arguments. Negative: an argument pair nextest rejects fails the test by name.
3. Freshness: after a manifest edit without `velnor-gen generate`, `velnor-gen check` and `velnor-ci plan` both fail
   and name the file. Positive: after `generate`, both pass.
Why new: the version floors compare numbers and never run the helper.
Post the plan on this issue. A second subagent replies "approved" or names what is missing.

## Steps
1. `crates/velnor-ci/src/cli.rs`: add a command that prints the command schema as JSON.
   Run: `cargo run -q -p velnor-ci -- schema | jq -e '.commands | length > 0'`  Expected: `true`, exit 0.
2. `crates/velnor-gen/src/io/pin.rs`: `pin-helper` stores the schema in `.velnor/rust-ci.lock` with the binary digest.
3. `crates/velnor-gen/src/helper_gate.rs`: check every rendered argv against the stored schema; delete the floors.
   `crates/velnor-rust/src/base_cache.rs`: delete `BATCHED_HELPER`.
4. Write the three tests of the plan under `crates/velnor-gen/tests/it/` and `crates/velnor-ci/tests/it/`.
   Run: `cargo nextest run -p velnor-gen -p velnor-ci`  Expected: exit 0.
5. Run: `cargo run -q -p velnor-gen -- check`  Expected: exit 0.

## Red and green
For each test: `git stash` the fix, run the test (Expected: exit 100, the test named in the failure), `git stash pop`,
run it again (Expected: exit 0). The reviewer's subagent posts both outputs.

## Do not
Do not keep a version floor "for older pins": a lock without a schema is refused. Do not edit `rust-ci.yml` by hand.
Do not weaken an existing preflight test.

## Deletes
The eleven `*HelperTooOld` floors in `crates/velnor-gen/src/helper_gate.rs`; `BATCHED_HELPER` in
`crates/velnor-rust/src/base_cache.rs`.

## Done when
- [ ] the header's first command exits 1
- [ ] after release train A is pinned, the header's second command prints `success`, and each of those three runs
      has a nextest summary line with a test count above 0:
      `gh run view <id> --log | rg -m1 'Summary \[.*\] +[1-9][0-9]* tests? run'` exits 0
- [ ] `git show origin/main:.velnor/rust-ci.lock | rg -c '^\[schema'` exits 0

## Review
Opus 5.5, high effort. Verify: the schema comes from the released binary, not from source; a missing schema is an
error; the three tests were seen red and green; no floor remains; `velnor-gen check` passes on the PR head.
```

The exact command name (`schema`) and the lock's table name are the executor's to settle in step 1; the header and
the checks then use the settled names.

## Owner actions

An `[OWNER]` issue gives the exact commands, one operation each, and the field-level read-back that proves it. W6 is
the model: the delivery plan holds its payload commands (built from the live ruleset, so all six rules survive), the
separate `allow_auto_merge` setting, and the `diff` that must print nothing. The waiver format for a signed reduction
is in W10.

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
- The ten Fable† items (W2, W10, N26, V01, V04, DG03, L03, V07, V08, C05) are reviewed by Opus 5.5 at high effort,
  or by Fable 5.2 at xhigh if the owner answers Q11 with (a).
- An executor moves to a stronger seat only after a documented failure, and the PR says so.

Allowed values: `impl.seat` is `haiku`, `sonnet` or `opus`; `impl.effort` is `low`, `medium` or `high`;
`review.model` is `haiku-5.5`, `sonnet-5.5`, `opus-5.5` or `fable-5.2`; `review.effort` is `low`, `medium`, `high`
or `xhigh`.
