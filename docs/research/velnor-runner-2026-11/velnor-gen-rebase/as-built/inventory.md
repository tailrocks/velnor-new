# Inventory: open PRs, branches, roadmap issues, milestones, project 2

| Field | Value |
|---|---|
| Model | Claude Sonnet 5.5 (`claude-sonnet-5-5`) |
| Client | Claude Code 2.1.325 (general-purpose subagent) |
| Reasoning effort | high |
| Date | 2026-11-20 |
| Repository state | main at `4fc0ede34e`–`a619478fb2` unless the text says otherwise; #1747 (old engine deleted), #1274, #1335 and #1748 merged after it |

Read-only evidence for the velnor-gen rebase round. Written by a subagent from the code, `gh` and git; unedited except for this header and replacing home-directory paths with `~`.

---

# D inventory: tailrocks/velnor-new (read-only, generated 2026-11-20)

Main tip used: `a619478fb2` (#1263, 2026-11-20 00:45 +0700); the clone was fetched at the start. Nothing on GitHub was changed.

## 0. Findings that change the plan

1. **#1263 is already merged** (a619478fb2, 2026-11-19T17:46Z). Only **7** PRs are open: #1608, #1335, #1320, #1297, #1274, #1200, #1026.
2. **Only 15 branches exist on the remote** (`git ls-remote --heads origin`; `gh api repos/.../branches` agrees: 15, including `main`). The "~1097 remote branches" are not branches: `git branch -r` lists 1095 refs, of which only 15 are `origin/*`. The other 1080 are local PR-head mirror refs in the shared git dir (`pull/N` 1021, `prh/N` 47, `pr/N` 10, plus `keep628` and `mmrv1344-head`). They are not on GitHub and are not cleanup targets on the remote.
3. **Class (a) "merged into main by ancestry" is empty** (`git branch -r --merged origin/main` lists only `origin/main`): all 937 merged PRs were squash-merged, so ancestry never shows them. Use PR state, not ancestry.
4. Of 1112 PRs ever opened: 937 MERGED, 168 CLOSED (unmerged), 7 OPEN (before #1263 merged, 8 open).
5. Six remote branches still exist with finished PRs and can be deleted by the owner (after the keep-ref rule in docs/roadmap.md: save as `refs/keep/<name>`): `cleanup/runner` (#1605 merged), `fast/1007-floor-0.1.81` (#1046 closed, redone as #1713), `fast/890-retryable-roots` (#1313 closed), `fast/p3-image-jobs` (#1318 closed), `fast/runner-host-test-tail` (#1349 closed), `peer/consumer-nonrust-repairs` (#1168 closed). One remote branch has no PR: `handoff/roadmap-2026-11-15` (HANDOFF.md only, 4.6 days old, 3 ahead / 422 behind).
6. Layout on main after #1290/#1594: one Cargo workspace; all crates are in `crates/velnor-*` (32 crate directories). `generator/` and `ci-helper/` now hold only docs/samples/AGENTS files; `local/` still exists. Four of the open PRs are written against the old `generator/crates/*` and `ci-helper/crates/*` paths, so they conflict on renames (git suggests the moves). #1200 and #1608 are the only ones written for the new layout (or touching only files git can map).
7. Old engine: #1550 (legacy workflows) and #1420 (Rust CI lane code) are on main. The old engine still exists for non-Rust stacks (alint, jactionlint, images, qualification); #608 "Delete the existing engine's Rust CI path" and #604 are still open in the roadmap. Main CI/CD tools pinned to tools-v0.1.95 (#1742); velnor-gen floor is velnor-ci 0.1.81+ (#1713).

## 1. Open PRs

Mergeability was computed locally with `git merge-tree --write-tree origin/main <head>` (GitHub's own `mergeable` was UNKNOWN for most and CONFLICTING for the rest; the local result is the authority and differs for #1608).

| PR | Title | Author | Branch to base | Updated (UTC) | vs main (local) | CI at last run | Size | Behind main |
|---|---|---|---|---|---|---|---|---|
| #1608 | release gate: require Required from the push run on the default branch (#1555) | donbeave | fast/1555-tools-gate to main | 2026-11-19 15:44 | clean locally (GitHub says CONFLICTING/DIRTY, stale); 0 conflicts | red: 7 fail (Alint, Cache, Required, rust-ci_helper, rust-crates_velnor_runner, rust-generator, rust-root), 4 cancelled, 8 pass | +35/-13, 4 files | 79 |
| #1335 | [P1] velnor-gen: emit a lane-guard step after filtered feature lanes (#1290) | donbeave | fast/1290-lane-guard-gen to main | 2026-11-19 17:36 | CONFLICTING: 39 conflicts (1 content in .velnor/config.toml, io/mod.rs, lane_guard.rs add/add; rest rename/delete) | 3 cancelled, 1 pass (no real run) | +2569/-87, 44 files | 236 |
| #1320 | (#1073) per-root inputs audit; no double workspace build | donbeave | fast/1073-per-root-audit to main | 2026-11-19 14:07 | CONFLICTING: 9 conflicts (3 content: shared_sources.rs, model steps.rs, render steps.rs; rest renames) | 1 pass only | +1317/-239, 30 files | 144 |
| #1297 | fix(deps): update cargo minor/patch | renovate[bot] | renovate/cargo-minorpatch to main | 2026-11-19 08:18 | CONFLICTING: 11 (Cargo.lock x2, 4 crate Cargo.toml, ci-helper/Cargo.lock deleted on main) | 46 pass, 3 cancelled | +457/-346, 25 files | 294 |
| #1274 | feat(generator): check the workspaces of a group in one job set | donbeave | pool/644 to main | 2026-11-19 10:47 | CONFLICTING: 34 (content in plan.rs and io/plan.rs; rest renames) | red: 3 fail (Required, Select source qualification, Source qualification), 46 cancelled | +2249/-688, 51 files | 232 |
| #1200 | fix(mise): preserve platform-specific tool locks | donbeave | peer/1104-mise-platform-pins to main | 2026-11-19 17:46 | MERGEABLE, no conflicts; merge state BLOCKED (checks) | red: 4 fail (Alint, Cargo Deny, Required, rust), 3 skipped, 4 pass | +1639/-272, 33 files | 2 |
| #1026 | fix(inputs): restore sibling packages and custom target directories | donbeave | peer/1018-1011-inputs to main (was stacked on #1035 / fast/607-base-cache) | 2026-11-19 02:02 | CONFLICTING: 44 (content in io/cold.rs; rest renames/deletes) | red: 11 fail (Alint, Required, Source qualification, 4 Rust velnor-* jobs...), 37 pass | +5792/-505, 66 files, 96 commits ahead | 285 |

Not open any more: **#1263** "fix(runner): refuse a second daemon when daemon.lock is unlinked" MERGED 2026-11-19T17:46Z.

### What each changes, relevance after the cutover, recommended disposition

- **#1608** (part of #1555): restores the strict `Required`-check gate in `.github/workflows/release-rust-ci-tools.yml` (push-run on default branch, `rust-ci.yml`), plus golden copy, MANIFEST.sha256 and `release_workflow` test (now `crates/velnor-gen/tests/it/release_workflow.rs`). Fully relevant: the release workflow is the velnor-gen generated/dogfood one. Red CI is likely main's own red at that time (main was fixed by #1728). **Disposition: rebase and finish** (rebase is trivial; rerun CI; regenerate goldens if the workflow file moved since).
- **#1335** (part of #1290): generator half of lane-guard (`CargoCommand::LaneGuard` in model, emission in rust, rendering in render, goldens) after filtered `[[test_lane]]`. The helper half (`velnor-ci lane-guard`, #1326) is already on main (add/add on `lane_guard.rs`), and the PR was stacked on #1308 (merged). Needs velnor-ci 0.1.82; floor now 0.1.81+, tools at 0.1.95. **Disposition: rewrite on velnor-gen**: drop the helper files, replay model/rust/render changes onto `crates/velnor-*` and regenerate goldens.
- **#1320** (closes #1073): `velnor-ci inputs audit --root DIR` and generator change that runs per-root audit in each lane and removes the separate `rust-inputs-audit` job; gated on helper >= 0.1.82. Relevant (#1073 and #1305 open; the "older-helper branches" were deleted by #1713, so the compatibility gating can be simplified). **Disposition: rewrite on velnor-gen** (mechanical path remap `ci-helper/crates/*` and `generator/crates/*` to `crates/*`, resolve 3 content conflicts, drop the version-gated old output branch).
- **#1297** (Renovate): bump 25 cargo deps (tokio, toml, nix, base64, der, hcl, sigstore-verify, ...) across the old per-box lockfiles; conflicts in Cargo.lock, and `ci-helper/Cargo.lock` no longer exists. **Disposition: close as superseded; let Renovate recreate** against the single workspace (or tick its rebase box). Cargo Deny on main also fails (#1218 RUSTSEC-2026-0543 chrono via turso), which affects any dependency PR.
- **#1274** (closes #644, FAST-3.6): `[[workspace_groups]]` in `.velnor/rust-ci.toml`: one job set for several Cargo roots; touches velnor-rust (groups.rs, root_jobs, members, units), velnor-ci plan/io, CONFIG.md, docs/rust-ci-cache-keys.md. Relevant (#644 open, a live product request: a project with 14 workspaces). Plan code in velnor-ci and velnor-rust has since moved (feature lanes #1308, source-diff selection #1723, etc.). **Disposition: rewrite on velnor-gen** (re-apply on the new tree; largest rewrite risk; the CI failures are Source qualification, probably environment, recheck).
- **#1200** (closes #1104): shared mise platform matching, preserves request/lock identities, fail-closed on ambiguous locks (velnor-ci, velnor-gen, velnor-model, velnor-rust), 13 new tests, mutation-verified. Already on the new `crates/` layout, 2 commits behind. Failures: Alint, Cargo Deny, rust, Required (Cargo Deny is the known #1218 main failure; others need a look). **Disposition: rebase and finish** (highest-value, closest to merge). Follow-ups already filed in #1700.
- **#1026** (closes #1018 and #1011): `inputs save/restore --target-dir`, snapshot of reachable package directories, cleanup-target validation (47 files in velnor-ci, 17 in generator, +5.8k lines; docs/proposed). Relevant (both issues open, in-progress label), but giant, 96 commits, stacked, red CI, review follow-ups in #1730. **Disposition: rewrite on velnor-gen / split**: replay in two PRs (velnor-ci input handling, then generator rendering gated on current floor) on the `crates/` layout.

## 2. Branches

Full table (1094 rows, one per ref in `refs/remotes`, excluding `origin`, `origin/HEAD`, `origin/main`): `~/.cache/ci-ambition/regen/D-branches.tsv`. Columns: ref, class, ref_kind, prs, last_commit_date, age_days, age_bucket, ahead_of_main, behind_main, top_level_paths (paths filled for classes c and d only; ahead/behind from `%(ahead-behind:origin/main)`).

Counts by class:

| Class | Count | Notes |
|---|---|---|
| (a) merged into main by ancestry | 0 | squash merges; only `origin/main` itself |
| (b) PR merged | 898 | 848 `pull/N`, 40 `prh/N`, 9 `pr/N` local mirrors, 1 live branch (`origin/cleanup/runner`, #1605) |
| (b) PR closed unmerged | 175 | 167 `pull/N`, 3 `prh/N`, 5 live branches (see finding 5) |
| (c) open PR | 18 refs, 7 distinct PRs | live: 7 branches (#1608, #1200, #1320, #1335, #1274, #1026, #1297; `renovate/cargo-minorpatch` also had closed #1240); the rest are `pull/`, `prh/`, `pr/` mirrors, some stale vs the live branch (e.g. `pull/1200` is 23 ahead of generator/ci-helper paths while the live branch is 29 ahead in `crates/`) |
| (d) no PR | 3 | below |

(c) live remote branches:

| Branch | PR | Last commit | Ahead | Behind | Top-level paths |
|---|---|---|---|---|---|

| fast/1555-tools-gate | #1608:OPEN | 2026-11-19 | 1 | 79 | fixtures(2),.github(1),generator(1) |
| peer/1104-mise-platform-pins | #1200:OPEN | 2026-11-20 | 29 | 2 | crates(30),.velnor(1),Cargo.lock(1),Cargo.toml(1) |
| fast/1073-per-root-audit | #1320:OPEN | 2026-11-19 | 7 | 144 | generator(21),ci-helper(9) |
| fast/1290-lane-guard-gen | #1335:OPEN | 2026-11-19 | 7 | 236 | generator(33),ci-helper(10),.velnor(1) |
| pool/644 | #1274:OPEN | 2026-11-19 | 7 | 232 | generator(35),ci-helper(15),docs(1) |
| peer/1018-1011-inputs | #1026:OPEN | 2026-11-19 | 96 | 285 | ci-helper(48),generator(17),docs(1) |
| renovate/cargo-minorpatch | #1240:CLOSED,#1297:OPEN | 2026-11-18 | 3 | 294 | crates(18),ci-helper(2),generator(2),local(2),Cargo.lock(1) |

(d) no PR (grouped by age; there is no branch older than 14 days):

| Age bucket | Ref | Kind | Last commit | Ahead | Behind | Top-level paths |
|---|---|---|---|---|---|---|
| <3d | mmrv1344-head | local-ref(other) | 2026-11-19 | 3 | 235 | crates(6) |
| 3-14d | origin/handoff/roadmap-2026-11-15 | remote-branch(live) | 2026-11-15 | 3 | 422 | HANDOFF.md(1) |
| 3-14d | keep628 | local-ref(other) | 2026-11-13 | 3 | 468 | local(13),.alint.yml(1),.velnor(1),Cargo.toml(1) |

Notes: `mmrv1344-head` is a local mirror of PR #1344's head (the PR number is not in the name; PR #1344 is merged/closed). `keep628` is a `refs/keep`-style safety ref for issue #628. Neither is on the remote. Open-PR mirror refs under `pull/`, `prh/`, `pr/` can be ignored for planning; use the live branches.

## 3. Roadmap (#574)

`docs/roadmap.md` (132 lines, main tip) is a process document: it says the roadmap runs unattended (Opus session delegating to executor subagents on `fast/<issue>-<name>` branches, labels `exec:*`, `effort:*`, `in-progress`, `blocked`, `needs-owner`, `owner-action`), gives the "ready issue" GraphQL query over label `fast-ci`, the project board commands, the never-stop rules ([OWNER] follow-up issues; `blocked` after one stronger-model retry) and the never-do list (no goals change, no `--admin`, no force push, no deletion without a `refs/keep/` save). Finished when #574 is closed. Its description of the five boxes (`generator/`, `ci-helper/`, `crates/` engine, runner, `local/`) is **outdated** after the single-workspace restructure (#1290/#1594); it still says the "existing engine keeps generating CI for other languages... Rust path deleted once the new generator owns Rust (#626)".

**#574 "[FAST] Roadmap: the fastest Rust CI a generator can produce"**: open, labels `fast-ci, epic`, no milestone, no assignee, 91 direct sub-issues (76 closed, 15 open), last updated 2026-11-18. Sub-issues nest (epics #578, #579, #581, #626, #627 have their own). "Old engine" / "velnor-gen" column is a keyword heuristic over title and body (velnor-gen, generator/, velnor-ci, ci-helper vs old engine, velnor-actions-, orchestrator, ...), so treat "both" as mixed.

Open issues in the #574 tree or labelled `fast-ci` (42):

| # | Updated | Parent | Blocked by (open) | Labels | Milestone | Assignee | References | Title |
|---|---|---|---|---|---|---|---|---|
| #574 | 2026-11-18 |  |  | fast-ci,epic |  | none | both | [FAST] Roadmap: the fastest Rust CI a generator can produce |
| #578 | 2026-11-14 | 574 |  | fast-ci,epic | FAST-3 Job shape | none | velnor-gen/ci | [FAST-3] Job shape: one job by default, more only when it pays |
| #579 | 2026-11-12 | 574 |  | fast-ci,epic | FAST-4 Cache | none | both | [FAST-4] Cache: a restored build that cargo treats as fresh |
| #581 | 2026-11-12 | 574 |  | fast-ci,epic | FAST-6 Guard and adopt | none | neither | [FAST-6] Guard and document |
| #601 | 2026-11-19 | 578 |  | fast-ci,exec:sonnet,effort:high | FAST-3 Job shape | none | velnor-gen/ci | [FAST-3.3] Test groups with needs |
| #604 | 2026-11-19 | 626 | 610,601,623 | fast-ci,exec:opus,effort:high,needs-owner | FAST-5 Dogfood | none | both | [FAST-5.3] Switch velnor-new's Rust CI to the new generator |
| #608 | 2026-11-19 | 626 |  | fast-ci,exec:opus,effort:medium | FAST-5 Dogfood | none | old-engine | [FAST-5.5] Delete the existing engine's Rust CI path |
| #609 | 2026-11-19 | 579 |  | fast-ci,exec:opus,effort:high,in-progress | FAST-4 Cache | none | velnor-gen/ci | [FAST-4.5] PR delta layer: the second push of a PR reuses the first |
| #610 | 2026-11-19 | 579 |  | fast-ci,exec:sonnet,effort:medium,in-progress | FAST-4 Cache | none | velnor-gen/ci | [FAST-4.4] Budget manager: stay under 8 GB without losing the base |
| #622 | 2026-11-19 | 581 |  | fast-ci,exec:sonnet,effort:medium,in-progress | FAST-6 Guard and adopt | none | velnor-gen/ci | [FAST-6.1] Speed budget: fail loudly when CI gets slower |
| #623 | 2026-11-16 | 626 | 609 | fast-ci,exec:sonnet,effort:medium,needs-owner | FAST-5 Dogfood | none | velnor-gen/ci | [FAST-5.1] velnor-lab runs the generator for real every night |
| #624 | 2026-11-16 | 581 | 608 | fast-ci,exec:sonnet,effort:medium | FAST-6 Guard and adopt | none | both | [FAST-6.2] Documentation: one page on how Velnor keeps Rust CI fast |
| #626 | 2026-11-12 | 574 |  | fast-ci,epic | FAST-5 Dogfood | none | both | [FAST-5] Dogfood: velnor-new's own Rust CI comes from the new generator |
| #627 | 2026-11-18 | 574 |  | fast-ci,epic | FAST-L Local GitHub | none | both | [FAST-L] Local GitHub: the official runner, a stand-in service and our own cache, all on one machine |
| #644 | 2026-11-19 | 578 |  | fast-ci,exec:sonnet,effort:high | FAST-3 Job shape | none | velnor-gen/ci | [FAST-3.6] Workspaces that share a job |
| #645 | 2026-11-19 | 578 |  | fast-ci,exec:sonnet,effort:medium | FAST-3 Job shape | none | velnor-gen/ci | [FAST-3.7] Checking out only what Rust needs |
| #646 | 2026-11-16 | 578 |  | fast-ci,exec:opus,effort:high,needs-owner | FAST-3 Job shape | none | velnor-gen/ci | [FAST-3.8] Private dependencies: fetch with a short-lived token, then build offline |
| #648 | 2026-11-16 | 581 | 623 | fast-ci,exec:opus,effort:high,needs-owner | FAST-6 Guard and adopt | none | velnor-gen/ci | [FAST-6.3] Projects receive new Velnor versions automatically |
| #649 | 2026-11-16 | 581 | 648,608 | fast-ci,exec:sonnet,effort:high,needs-owner | FAST-6 Guard and adopt | none | both | [FAST-6.4] Adopt the new generator in the owner's Rust projects |
| #662 | 2026-11-13 | 574 |  | owner-action |  | none | old-engine | [OWNER] Run the Qualification workflow once on the scale set after #554 |
| #680 | 2026-11-14 | 574 |  | owner-action |  | none | old-engine | [OWNER] Approve the existing engine's exemption for the hand-written release workflow |
| #682 | 2026-11-14 | 574 |  | owner-action |  | none | velnor-gen/ci | [OWNER] Allow only admins to create rust-ci-v* tags |
| #729 | 2026-11-16 | 574 |  | fast-ci,exec:sonnet,effort:high,blocked |  | none | velnor-gen/ci | [FAST-REL.9] Prove the whole release flow on velnor-lab |
| #732 | 2026-11-14 | 574 |  | owner-action |  | none | velnor-gen/ci | [OWNER] Switch the release-tags ruleset to the App-only bypass (for FAST-REL.11) |
| #733 | 2026-11-16 | 574 | 732 | fast-ci,exec:opus,effort:medium |  | none | velnor-gen/ci | [FAST-REL.11] Generate this repository's release.yml and delete the #676 exemption |
| #890 | 2026-11-16 |  |  | fast-ci,exec:sonnet,effort:medium |  | none | velnor-gen/ci | [FAST-B3.33] Make each release root retryable in the tag job |
| #931 | 2026-11-16 |  | 949 | fast-ci,exec:sonnet,effort:medium |  | none | velnor-gen/ci | Detect bypass actors added to the tag immutability rulesets |
| #934 | 2026-11-17 |  | 1007 | fast-ci,exec:sonnet,effort:low |  | none | velnor-gen/ci | Require velnor-ci 0.1.80+ for release generation once it is released |
| #966 | 2026-11-17 |  |  | fast-ci,exec:sonnet,effort:medium,blocked |  | none | both | Wire the in-cluster crates.io mirror explicitly in velnor-gen config |
| #1007 | 2026-11-16 |  |  | fast-ci,in-progress |  | none | velnor-gen/ci | Require velnor-ci 0.1.81+ for generation (release and the Required gate) once it is released |
| #1011 | 2026-11-17 | 574 |  | fast-ci,in-progress |  | none | velnor-gen/ci | inputs save/restore ignore a custom build.target-dir |
| #1018 | 2026-11-17 | 574 |  | fast-ci,in-progress |  | none | velnor-gen/ci | inputs snapshot scope misses tracked path dependencies outside a nested root |
| #1047 | 2026-11-16 |  |  | fast-ci,in-progress |  | none | both | Docs drift: statements in the contracts that no longer match the code |
| #1066 | 2026-11-17 |  |  | fast-ci,in-progress |  | none | neither | Qualification dispatch succeeds when every job is skipped |
| #1073 | 2026-11-18 | 574 |  | fast-ci,in-progress |  | none | velnor-gen/ci | Scheduled inputs audit builds every workspace again |
| #1104 | 2026-11-17 | 574 |  | fast-ci,in-progress |  | none | velnor-gen/ci | prepare's pinned-tool check mismatches mise OS filters and lock rows |
| #1161 | 2026-11-18 |  |  | fast-ci |  | none | velnor-gen/ci | fast-ci: list tests without a second discovery pass; lift the 32 KiB package-map cap |
| #1209 | 2026-11-18 |  |  | fast-ci |  | none | neither | Lab proof: changed-source selection adds the image job (follow-up to #1063) |
| #1217 | 2026-11-19 |  |  | fast-ci,blocked |  | none | neither | Measure runner minutes for argument-expanded COPY selection (#1199 follow-up) |
| #1218 | 2026-11-18 |  |  | fast-ci |  | none | old-engine | Cargo Deny fails on main: chrono unmaintained (RUSTSEC-2026-0543) via turso |
| #1220 | 2026-11-19 |  |  | fast-ci,in-progress |  | none | neither | Steps after repository code in the same job can't be trusted; put the trust boundary at the job |
| #1290 | 2026-11-19 |  |  | fast-ci |  | none | both | P1: velnor-new runs entirely on velnor-gen (Rust CI + Rust releases) |

Observations: no open issue has an assignee (0 of 215); 18 of 215 have a milestone. 22 issues carry `in-progress`, 3 `blocked` (#729, #966, #1217), 6 `needs-owner`, 5 `owner-action` (four of them in the #574 tree: #662, #680, #682, #732). The remaining roadmap work after the cutover is: FAST-3 job shape (#601, #644, #645, #646), FAST-4 cache (#609, #610), FAST-5 dogfood (#604, #608, #623), FAST-6 (#622, #624, #648, #649), FAST-REL (#729, #733), and #1290 "P1: velnor-new runs entirely on velnor-gen" (owner priority 1 of 2026-11-18). #604 and #608 (switch Rust CI, delete old engine Rust path) are open even though #1523/#1420/#1550 landed; they are probably closable or need their Done-when boxes rechecked. #1555 "Main fallout from the fast-merge phase" is the most recently updated issue.

### Milestones (`gh api repos/tailrocks/velnor-new/milestones?state=all`)

| # | Title | State | Open | Closed |
|---|---|---|---|---|
| 1 | FAST-0 Measure and decide | open | 0 | 6 |
| 2 | FAST-1 Foundation | open | 0 | 7 |
| 3 | FAST-2 Selection | open | 0 | 7 |
| 4 | FAST-3 Job shape | open | 5 | 11 |
| 5 | FAST-4 Cache | open | 4 | 4 |
| 6 | FAST-5 Dogfood | open | 4 | 2 |
| 7 | FAST-6 Guard and adopt | open | 5 | 0 |
| 8 | FAST-L Local GitHub | open | 1 | 8 |
| 9 | FAST-R Old work | open | 0 | 10 |

Milestones 1, 2, 3, 9 are complete but still open (0 open issues). No due dates.

### Org project tailrocks/2 "Velnor fast Rust CI"

Public, open, 160 items, readme points at #574 and docs/roadmap.md, 14 fields: Title, Assignees, Status (single select: Todo / In Progress / Done), Labels, Linked pull requests, Milestone, Repository, Reviewers, Parent issue, Sub-issues progress, Created, Updated, Closed, Stage (single select: FAST-0 Measure, FAST-1 Foundation, FAST-2 Selection, FAST-3 Job shape, FAST-4 Cache, FAST-5 Dogfood, FAST-6 Guard, FAST-L Local, FAST-R Old work). Views: Board (board, `-label:epic`), Now (table, `is:open label:in-progress`), Next (table, `is:open -is:blocked -label:epic -label:in-progress -label:blocked -label:owner-action`), Waiting (`is:open is:blocked -label:epic`), Needs you (`is:open label:owner-action,blocked`), Done (`is:closed`), Everything (no filter). Project id PVT_kwDOBLJ97c4BrWyl.

## 4. Open issues

Total open issues: **215** (PRs excluded). Breakdown: 99 "Deferred review findings from #N", 17 "Follow-ups from #N review", 38 labelled `fast-ci`, 61 other (incl. epics #1526/#1692, Dependency Dashboard #1233). 173 of 215 were updated on 2026-11-19, so "most recently updated" is dominated by the review-finding bot batch. Labels in use: fast-ci 38, in-progress 22, exec:sonnet 13, effort:medium 10, epic 8, effort:high 8, needs-owner 6, exec:opus 6, owner-action 5, blocked 3, effort:low 1.

40 most recently updated:

| # | Updated (UTC) | Labels | Title |
|---|---|---|---|
| #1555 | 2026-11-19T17:40 |  | Main fallout from the fast-merge phase (fix in one cleanup wave) |
| #1741 | 2026-11-19T17:32 |  | Deferred review findings from #1464 |
| #1526 | 2026-11-19T17:31 | epic | [Epic] Deferred review findings from the fast-delivery phase |
| #1233 | 2026-11-19T17:24 |  | Dependency Dashboard |
| #1521 | 2026-11-19T17:18 | in-progress | Stop polling a created scale set after its delete and settle the session header |
| #1733 | 2026-11-19T17:15 |  | runner-qualification: read_logs is exported only for the log_frames test |
| #1572 | 2026-11-19T17:15 |  | Deferred review findings from #1322 |
| #1506 | 2026-11-19T17:13 | in-progress | Reactivating a connection after its created scale set was deleted leaves it unpreparable |
| #1500 | 2026-11-19T17:13 | in-progress | Delete the remote scale set whose id #1402 retains after a refused create |
| #1730 | 2026-11-19T17:07 |  | Follow-ups from #1026 review |
| #1647 | 2026-11-19T17:03 |  | Deferred review findings from #1462 |
| #1726 | 2026-11-19T17:02 |  | Deferred review findings from the #1352 release-stack port |
| #1705 | 2026-11-19T16:54 |  | Follow-ups from #1397 review |
| #1717 | 2026-11-19T16:48 |  | Deferred review findings from #1275 |
| #1716 | 2026-11-19T16:48 |  | Deferred review findings from #1273 |
| #610 | 2026-11-19T16:44 | fast-ci,exec:sonnet,effort:medium,in-progress | [FAST-4.4] Budget manager: stay under 8 GB without losing the base |
| #1580 | 2026-11-19T16:37 |  | Deferred review findings from #1322 |
| #1541 | 2026-11-19T16:31 |  | Deferred review findings from #1515 |
| #1702 | 2026-11-19T16:29 |  | Follow-ups from #1107 review |
| #1700 | 2026-11-19T16:25 |  | Follow-ups from #1200 review |
| #1699 | 2026-11-19T16:25 |  | Follow-ups from #1263 review |
| #1698 | 2026-11-19T16:24 |  | Follow-ups from #1308 review |
| #1697 | 2026-11-19T16:23 |  | Follow-ups from #1259 review |
| #1519 | 2026-11-19T16:17 |  | velnor-local: follow-ups deferred from #1415 (branch pin, implicit actions:write, cache guard scope) |
| #1485 | 2026-11-19T16:16 |  | Declared workflows: stricter validation of .velnor/workflows.toml (follow-ups from #1462) |
| #1693 | 2026-11-19T16:15 |  | Deferred review findings from #1509 |
| #1692 | 2026-11-19T16:10 | epic | [Epic] Deferred review findings from the fast-delivery phase (part 2) |
| #1690 | 2026-11-19T16:07 |  | Deferred review findings from #1612 |
| #1689 | 2026-11-19T16:07 |  | Deferred review findings from #1611 |
| #1688 | 2026-11-19T16:07 |  | Deferred review findings from #1610 |
| #1687 | 2026-11-19T16:07 |  | Deferred review findings from #1607 |
| #1686 | 2026-11-19T16:07 |  | Deferred review findings from #1604 |
| #1685 | 2026-11-19T16:07 |  | Deferred review findings from #1602 |
| #1684 | 2026-11-19T16:07 |  | Deferred review findings from #1596 |
| #1683 | 2026-11-19T16:07 |  | Deferred review findings from #1592 |
| #1682 | 2026-11-19T16:07 |  | Deferred review findings from #1591 |
| #1681 | 2026-11-19T16:07 |  | Deferred review findings from #1588 |
| #1680 | 2026-11-19T16:07 |  | Deferred review findings from #1587 |
| #1679 | 2026-11-19T16:07 |  | Deferred review findings from #1581 |
| #1678 | 2026-11-19T16:06 |  | Deferred review findings from #1576 |
