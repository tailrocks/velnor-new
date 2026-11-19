# Disposition of the second final reviews (PR #1751)

Reviews: `final2-sol/review.md` (GPT-6.2-Sol, findings S1–S16 and four minor groups SM1–SM4) and
`final2-opus/review.md` (Claude Opus 5.5, findings O1–O16; O17 is "verified correct, no action").

Verified against `origin/main` at `515d4c7ff1` (2026-11-19 19:44 UTC) and live GitHub at that time, read-only.
Main moved to `e4e7c9a0be` while this was written; the documents keep `515d4c7ff1` as their read point.

Result: 36 findings. 32 accepted, 4 partially accepted (S3, S12, S16, O16), none rejected. Two proposed remedies
inside accepted findings were not taken, with evidence (the required-workflow rule in S1/O3; "one or two releases"
in O5).

## Evidence gathered for this pass

| Fact | Evidence |
|---|---|
| A draft merges on a green check that ran nothing | Run 44599847337 (PR #1799): `Required` success, only the draft notice step ran. Commit `c2c7b67480` has two `Required` check runs from app 15368, one success (135827936943) and one failure (135828454961). Real run 44599865034 failed after the merge; push run 44599906946 failed in `velnor-ci plan` |
| Tools releases were published from red commits | 0.1.97: release run 44596703249 success, push run 44596682603 failure, same commit `153d88998d`. 0.1.98: release run 44599294031 success; push run 44599278133 failed at `velnor-ci plan` with Test skipped |
| The 564 s run failed and ran no tests | Run 44584072510 at `57b65af0db`: job `rust` 18:05:37Z–18:15:01Z, conclusion failure, failed steps Lint, Test and both local-mode steps |
| Changed since the documents were written | #1770 merged (`c5c2bb1e90`), #1792 merged (`29d96c9839`), #1794 pinned 0.1.97, #1801 pinned 0.1.98 (lock and manifests at 0.1.98), #1608 and #1297 open, issues #1758, #1555 and #1745 open |
| #1770's mechanism | `rust-ci.yml` lines 24 and 631–636 on main: ignored events get the job name `Required (ignored event)` and a concurrency group suffixed `-ignored-<run id>` |
| W3 alone would turn `Required` red | `crates/velnor-gen/src/drifts.rs` (`Owner::Nobody => (!engine_configured).then(...)`), `io/mod.rs` (`engine_configured` from `.velnor/config.toml`; `check` exits 1 on any drift); `ci.yml` starts with "# Hand-maintained" |
| `[[inputs]]` is matched before global triggers | `crates/velnor-rust/src/classify.rs` lines 35–52 |
| `cargo deny` in `policy` is conditional | `crates/velnor-rust/src/policy.rs` (`dependency = everything \|\| …`); `select_event.rs`: a scheduled run selects everything, so deny already runs nightly |
| The push-after-queue lookup is one Boolean per run | `crates/velnor-ci/src/merge_group.rs` lines 72–84 |
| `release-gate` has no workflow or attempt selector | `crates/velnor-ci/src/cli_release.rs` (`--repo`, `--sha`, `--pr-only-check`); the generated release gate is an ancestry `compare` only |
| The generator-check step is a bare `velnor-gen check` | `crates/velnor-render/src/helper_steps.rs` `generator_check_step` |
| `image` is a command group with one subcommand | `crates/velnor-ci/src/cli_args.rs` `ImageCommand::VerifyCandidate` |
| `ci.yml` has four jobs and no sample lint | Alint, Cargo Deny, Cargo Machete, Jactionlint; `lint-generator-samples` exists only in old test fixtures |
| Hard decisions do not bind the generator | `docs/hard-decisions.md` lines 14–19 |
| The 90 s / 3 min targets are not in goals.md | `docs/goals.md` lines 47–61; `decisions.md` line 595 (H5) is their source |
| Decision 12 on an unlabelled PR | `owner-decisions.md` line 20: "An unlabelled PR never gets a green required check" |
| A required-workflow rule is not available | GitHub docs: ruleset workflows are configured at organisation or enterprise level. `gh api orgs/tailrocks` plan `free`; `gh api orgs/tailrocks/rulesets` answers 403 "Upgrade to GitHub Team" |
| Live ruleset and settings | Ruleset 24396608: six rules, fields as quoted in the plan's section 3; `allow_auto_merge` false |
| Commit counts | `0eb2d76a5c..a75aae380f` is 28 commits in 62 minutes; `0eb2d76a5c..515d4c7ff1` is 48 commits in 95 minutes |
| Roadmap readiness | `docs/roadmap.md` lines 51–54 and 105–112: the ready query reads native blockers, `blocked` and `in-progress` only; an owner follow-up closes the original issue |

## Findings

Files: RU = `velnor-gen-rebase/rulings.md`, DP = `delivery-plan.md`, OQ = `owner-questions.md`, EX = `explainer.md`,
IF = `planning/backlog-issue-format.md`, TSV = `planning/backlog-assignments.tsv` (all under
`docs/research/velnor-runner-2026-11/`), AR = `docs/proposed/velnor-runner-architecture.md`,
UX = `docs/proposed/velnor-user-experience.md`.

| Id | Reviewer | Severity | Disposition | What changed and where, or the evidence |
|---|---|---|---|---|
| S1 | Sol | Blocker | Accepted | R4 rewritten: in-workflow checks (W3, W10) are accident guards; a base-controlled verdict comes only from the trusted verifier N26, moved into phase 0; W6 has two steps and may claim "a PR cannot weaken its gate" only after step 2. W6 now needs W2–W5 and D7. Waiver format specified (SSH signature, base-branch `allowed_signers`, both contract digests, reductions, expiry, void on rebase). RU R2/R4; DP §4 graph, W3, W6, W10, N26; AR §2.2, §3, §9.3, §13; UX §2.3, §3.1, §4; EX §3.4; OQ Q12, D7. The "required workflow" remedy is offered only as Q12 option (c): the organisation is on the Free plan and its rulesets API answers 403 |
| S2 | Sol | Blocker | Accepted | W7 rewritten: per-obligation records, full resolved key, latest attempt, tests for unequal selection, skipped or cancelled step, older attempt and different commit; shadow over 20 merges with at least one valid would-hit and the hit rate; Q8 is a precondition. RU R5; DP W7; AR §7.1, §7.4, §7.5; OQ Q8, Q13 |
| S3 | Sol | Blocker | Partially accepted | "Keep green after unlabel" and the lookup by SHA are removed: unlabel turns `Required` red (decision 12). Base retarget stays a trigger; ignored events use #1770's path, also for an unrelated `unlabeled`. Lab cases added (green then unlabel, unrelated label during a run, base change, two PRs on one commit, stale completion, check subject and queue admission). RU R3; DP W2; AR §9.2, §7.4; UX §2.1; EX §3.3; OQ Q2. Partial: state per PR (label, head, tested tree, base, run generation) cannot be held by an Actions check, which belongs to a commit; the documents say so and assign it to the App (AR §9.2, §9.3) |
| S4 | Sol | Blocker | Accepted | N25 and V04 need trusted records, the full key, V12 and V14, and Q8; unclassified obligations run fresh; shadow samples are would-hit comparisons at the count Q8 sets (C6's 3,000, or 300 with its bound stated). Q8 reworded on goals lines 40–41 and carries the goals sentence; "phase 1 needs no goals change" removed. DP §5 intro, N25, V04; RU R5; OQ Q8; AR §6.2, §7.1 |
| S5 | Sol | Major | Accepted | Owner gates and owner actions become `[OWNER]` issues that block natively, so the unchanged ready query never returns a gated item (IF "Hierarchy and labels"; DP §2, §13; RU R12). D7 is a precondition of W6 and N26. W8 no longer waits for W7: the baseline runs with reuse off. One graph in DP §4 with a reason per edge; DP §8 follows it |
| S6 | Sol | Major | Accepted | W5: workflow file, push to the default branch, exact commit, latest attempt, named job, fail closed; tests for a same-name job in another workflow and a pending latest attempt; other callers migrate in the same PR; release bootstrap in DP §4.1. Evidence rows for 0.1.97 and 0.1.98. DP §3, W5, §4.1; RU R2, R10; AR §3, §9.5; UX §2.4 |
| S7 | Sol | Major | Accepted | W1 keeps the schema check and adds end-to-end render, plan and preflight tests and a metadata-freshness test; causes listed with run ids; older pins are not supported (a lock without a schema is refused); done-when names attempt 1, the nextest summary line and the cache key that run saved. DP W1; RU R2; IF example |
| S8 | Sol | Major | Accepted | W9: global triggers win over `[[inputs]]` (a mapping that covers one is a configuration error); `.github/workflows/**` is never narrowed; planted-change tests; the replay publishes unknown and select-everything counts apart; the 5% target is gone. DP W9; RU R8; AR §3, §4, §5; UX §4.1; EX §4 |
| S9 | Sol | Major | Accepted | V01 has two publishers (push runs on main; the verifier for PR runs it verified); a PR run writes nothing a queue run can read; V04 stays in shadow until the lab shows a PR-to-queue hit through that path. DP V01, V04; RU R5; AR §7.1, §7.3; EX §3.5 |
| S10 | Sol | Major | Accepted | V05 keeps a persisted first-parent frontier and audits every landed commit, also one whose push run reused results; lag visible to the release gate. C1 restated for obligations the owner defers past landing (Q13). DP V05; AR §1.2 C1, §7.6; RU R5 |
| S11 | Sol | Major | Accepted | W6 gives three separate operations with read-back; the payload is derived from the live ruleset with `jq` so every rule and field survives; `allow_auto_merge` is a separate PATCH; "Merge when ready" noted; lab case with two overlapping entries (A red, B green) under ALLGREEN; the per-entry diff claim is marked as read from code until observed. DP W6 and its command block; AR §3, §9.4; UX §3.1; OQ Q3, Q4 |
| S12 | Sol | Major | Partially accepted | IF rewritten: state probe against a fetched ref, typed `expect` (exit code or stdout), test-plan approval, red and green verification by a second subagent, owner actions with read-back, release train field, a full W1 body. W2, W9, W10 and N26 have sub-issues up front. L01 no longer defines W2's lab proof. W3 says the rendered step is a bare `velnor-gen check` and changes it. W4 decides the `ci.yml` dispatch inputs with a proof step. Partial: the other eleven issue files are not written here; IF makes the manager write a phase's files before its issues are created, and DP carries each item's checks with expected results |
| S13 | Sol | Major | Accepted | W8: three runs only prove the collector; cohorts (event × cache × selection); clock and attempt rule; a median needs 20 runs and a P90 needs 50 with its interval; queue wait, ejections, runner-minutes and false failures recorded; `[budget]` fields set only from cohorts with enough runs. N27 is a separate pin-regression check with a threshold fixed in advance. S1 formula corrected to 1−(1−p)². DP W8, N27; RU R9; AR §1.4, §12; EX §8 |
| S14 | Sol | Major | Accepted | Each number carries run, revision, helper, topology and conclusion (RU R8; DP §3; OQ Q13; EX §2, §4). "Every run cold" removed. The 2.9 / 6.2 min figures are labelled as the old layout with failed or cancelled runs. The 16% and 67–83% replay figures are removed from AR §9.4. Q13 states the added queue time and no longer says "not slower". Cheap anchors added to W9: one lane listing, `policy` not waiting, base commit fetched on demand |
| S15 | Sol | Major | Accepted | N18 moves to phase 2 after V08 and deletes the base-commit path only after a paired replay. W9 (c) fetches the base commit on demand meanwhile. DP phase 2 V table, §8, §9; RU R5, R13; AR §5 row 11, §11.3; EX §6 |
| S16 | Sol | Major | Partially accepted | Exceptions to decisions 9 and 10 recorded with their end (OQ "Already decided"; RU R10; AR §9.5). Native cache and storage limits are Q19 (OQ; DP C09, ST03, §6; AR §6.2, §8). The post-cutover snapshot, the failed-run shares and the replay figures left AR; PR numbers and a helper version left UX's examples. Partial: AR keeps the `[M]`-tagged experiment numbers it already had on main (50 such tags there); N21 covers the rest under Q10 |
| SM1 | Sol | Minor | Accepted | Refreshed at `515d4c7ff1`: #1770 and #1792 merged, 0.1.98 pinned, #1758 and #1555 open, the tools release still ancestry-gated. DP header, §1, §3, W1, W2, W5, §11, §12; RU "What changed", R12; EX §2, §10 |
| SM2 | Sol | Minor | Accepted | Queue history corrected (289 pre-cutover `CI` group runs; none of `Rust CI`); `tag-ruleset-drift.yml` added to the lists; `[ci]` table named in W7; `[[inputs]]` wording fixed. RU, R11; DP §3, W7; EX §2, §9; AR §2.1 |
| SM3 | Sol | Minor | Accepted | Q11 has one answer (default Opus 5.5 high; Fable 5.2 xhigh named in the text on answer (a); no label). D4 blocks only X06b and is asked when it is scheduled. D7, Q8 aligned with the importer rule. OQ Q11, Q14, "Later"; IF header notes and "Choosing seats"; DP §2, §7, X rows; AR §14; TSV |
| SM4 | Sol | Minor | Accepted | Totals, stream table, seat table and the TSV are computed from the item tables by `panel/plan-totals.py` (not in the repository). Review hours are shown with half hours: 17.5 + 31.0 + 94.5 + 23.5 = 166.5; 1284 + 166.5 = 1450.5. AN10's regex pipes are escaped and its expectation is `exit 1`. DP §1, §9, AN10; TSV |
| O1 | Opus | Blocker | Accepted | W0 added (2 h, Sonnet high, Opus high review), before W1. The bypass freeze is a precondition (Q3 part 1, no default). Until W2, every phase-0 PR carries `ci:auto` and its reviewer checks that the gate step and the tests ran. The label change is part of W2, not an owner chore. RU R2, R3; DP §4, W0, W2, §13; OQ Q3, Q2; UX §3.1, §5.1; AR §3; EX §3.2 |
| O2 | Opus | Major | Accepted | R2 restated: several causes, one enabling condition. AR §3 and §13, UX §7 and EX §2 follow. W1's done-when no longer rests on the schema alone |
| O3 | Opus | Major | Accepted | As S1. Option (b), pulling N26 into phase 0, is taken. Option (a), a ruleset `workflows` rule, is not available on the organisation's plan (403) and is Q12 option (c). The three lab cases (deleted check step, a second `Required` job, a helper downgrade) and a forged status case are in N26's done-when, where they can be red. `engine_configured` hiding unmarked files is in DP §3. D15 now treats an older pin as a signed reduction |
| O4 | Opus | Major | Accepted | W3 is blocked by W4; "`velnor-gen check` exits 0 on main" is W3's first check. DP graph, W3; RU R2; AR §3; EX §3.2 |
| O5 | Opus | Major | Accepted, one remedy adjusted | D9 moved to the phase-0 questions as the second question; release trains A, B and C with a "release and pin" rule, bootstrap and repair release; wall time in DP §4.2 and §1. The risk row that tied pin bumps to N27 is gone (N27 is phase 1). Three trains, not "one or two": W0 and W1 must be live before the label flow lands, and W10, N26 and W7 follow the ruleset |
| O6 | Opus | Major | Accepted | The 564 s run is labelled failed, no tests, helper 0.1.95 everywhere it appears; Q13 gives the cost of each option; "not slower" removed; rung 2 saves lint and test time only. The re-measurement on a green full push run is a W8 check |
| O7 | Opus | Major | Accepted | W4 keeps deny in `policy` for PRs and adds `cargo deny check advisories` on every push to main and on the nightly schedule, never reused; lab case with an advisory and no dependency change. Note: the scheduled run already selects everything, so deny runs nightly today; W4 proves it with a run id. DP W4, §3; RU R2; AR §3; UX §1 |
| O8 | Opus | Major | Accepted | The 90 s / 4 min and 3 / 8 min figures are "proposed targets" with their source; goals.md's own targets are listed and measured by W8; Q18 asks for both the status of the proposals and acceptance of the measured cost under goals line 60. RU R9; OQ Q18; DP §1, W8; AR §1.4, §6.2; UX §6; EX header, §5 |
| O9 | Opus | Major | Accepted | As S8, with the excluded paths named (`rust-ci.yml`, `.velnor/rust-ci.{toml,lock}`, `.velnor/verify.lock`, `actions/setup-velnor/**`) and the "edit to `rust-ci.yml` selects everything" test |
| O10 | Opus | Major | Accepted | One answer per id, OQ as the sheet and RU as the source. Q11: see SM3. Q17: after Q9 (AR §9.5, §11.1, §14; UX §9; EX §3.7; RU R10). Q8: every rung, no default, re-cited on goals lines 40–41 (OQ, AR §6.2 and §14, DP W7 and N25). Q4: ALLGREEN only (AR §14). D7: W6 and N26 (DP gates, OQ). Q6: needed by W2 (OQ). D9 wording fixed in "Closed by events" |
| O11 | Opus | Major | Accepted | The SHA lookup is removed from AR §9.2 and UX §2.1; W2 keeps #1770's no-publish path for unrelated `labeled` and `unlabeled` events; lab case "an unrelated label removed during a run does not cancel it" |
| O12 | Opus | Minor | Accepted | All five rows refreshed; one reading point, `515d4c7ff1`, in RU, DP and the IF example |
| O13 | Opus | Minor | Accepted | EX §2 no longer says a hand edit is detected and lists `tag-ruleset-drift.yml`; the `.velnor/config.toml` correction is fixed in RU R11 and EX §9; the sample lint is removed from DP W4, AR §3 and UX §1 and §4.1; W3 and AR name the `image` command group; the commit count is 48 in 95 minutes at the new anchor |
| O14 | Opus | Minor | Accepted | One order: RU R2 lists the order of work, DP §4 holds the graph with reasons, DP §8 and AR §3 repeat it (W0 → W1, W4 → W3, W2–W5 → W6, W10 → W7) |
| O15 | Opus | Minor | Accepted | W7 is Opus high; W6's review is Opus high; the TSV's seat and review columns are counted from the items instead of a default; the IF stream list includes B |
| O16 | Opus | Major | Partially accepted | (2) one split rule: sub-issues are written with the item, never "at the first commit" (DP §2, IF). (3) checks are commands with expected results; "#1555 is closed" dropped; the lab pins the release train; wall-clock gates estimated in DP §4.2. (4) `[ci]` named. (5) payload commands and read-back diff in DP W6. (6) waiver format in DP W10. Partial on (1): IF holds the W1 body as the model; the remaining issue files are the manager's step before creation (see S12) |
| O17 | Opus | — | No action | Its totals (166 items, 1279 h, 42 h critical path) are superseded: 167 items, 1284 h, 52 h |

## Document sizes

The brief asked for each document to stay within about 10% of its length. The architecture does (+9%). The others
grew more, because the accepted findings add content that did not exist: W0, N26 in phase 0, release trains, typed
checks and the W6 commands in the plan; Q18, Q19 and the reworded Q3, Q8, Q12 and D9 in the sheet; the W1 body in the
issue format.

| File | Before | After |
|---|---|---|
| AR | 90,662 | 98,881 |
| UX | 25,183 | 28,177 |
| DP | 68,837 | 88,947 |
| OQ | 18,764 | 25,608 |
| EX | 21,609 | 25,450 |
| RU | 14,034 | 20,657 |
| IF | 7,570 | 13,512 |
| TSV | 1,065 | 2,443 |

## Commit groups

Every file is touched by several fixes, so the groups are by file. Numbers in the last column are the brief's
"key fixes".

| Commit | Files | Key fixes carried |
|---|---|---|
| 1. `research(velnor-gen): rulings state what phase 0 can guarantee` | `docs/research/velnor-runner-2026-11/velnor-gen-rebase/rulings.md` | 1, 2, 3, 4, 5, 6, 7, 8, 9, 10 |
| 2. `plan(velnor-gen): phase 0 with W0, the verifier and release trains` | `docs/research/velnor-runner-2026-11/delivery-plan.md`, `docs/research/velnor-runner-2026-11/planning/backlog-assignments.tsv` | 1, 2, 3, 4, 5, 6, 7, 8, 9, 10 (totals, AN10, refreshed facts), 11 (W6 commands, waiver, sub-issues) |
| 3. `plan(velnor-gen): issue format with native owner blockers and a W1 body` | `docs/research/velnor-runner-2026-11/planning/backlog-issue-format.md` | 10 (Q11 seats), 11 |
| 4. `research(velnor-gen): owner sheet and explainer follow the rulings` | `docs/research/velnor-runner-2026-11/owner-questions.md`, `docs/research/velnor-runner-2026-11/explainer.md` | 1, 2, 4, 5, 8, 10 (Q4, Q8, Q11, Q17, D4, D7, Q6, Q18, Q19) |
| 5. `design(velnor-gen): architecture and user experience follow the rulings` | `docs/proposed/velnor-runner-architecture.md`, `docs/proposed/velnor-user-experience.md` | 1, 3, 4, 5, 7, 8 (S1 formula), 9, 10 (dated evidence moved out) |

The TSV and the totals in DP §9 come from `~/.cache/ci-ambition/panel/plan-totals.py <delivery-plan.md> --tsv <tsv>`,
which parses the plan's item tables and fails on a duplicate id or an item no group covers.

## Still open for the manager

- Main is at `e4e7c9a0be`, past the documents' read point. No fact was re-read after `515d4c7ff1`.
- Three claims rest on GitHub behaviour that only the lab can show, and the documents mark them so: that
  `workflow_run` fires for a `merge_group` run and the status lands on the group head (N26); which commit a
  pull-request check is attached to and what two PRs on one commit see (W2); the ranges of two overlapping queue
  entries under ALLGREEN (W6).
- The release that carries W0 must come from a green commit of main, and main is red now. The plan's "repair
  release" rule covers it and needs the owner.
