# Measurements: CI before and after the velnor-gen cutover

| Field | Value |
|---|---|
| Model | Claude Sonnet 5.5 (`claude-sonnet-5-5`) |
| Client | Claude Code 2.1.325 (general-purpose subagent) |
| Reasoning effort | high |
| Date | 2026-11-20 |
| Repository state | main at `4fc0ede34e`–`a619478fb2` unless the text says otherwise; #1747 (old engine deleted), #1274, #1335 and #1748 merged after it |

Read-only evidence for the velnor-gen rebase round. Written by a subagent from the code, `gh` and git; unedited except for this header and replacing home-directory paths with `~`.

---

# C. CI measurements, tailrocks/velnor-new, 2026-11-16 17:00Z to 2026-11-19 17:50Z

Measured read-only with `gh` on 2026-11-19 ~17:50Z (2026-11-20 00:50 +07). Raw data and the analysis scripts are in
`~/.cache/ci-ambition/regen/c-data/` (runs.json, timelines.json, req.json, jobs/, an_*.py).

## 0. Cutover and how to read the numbers

| Item | Value |
|---|---|
| Cutover commit | `053fefa6a3` "ci: cut over to velnor-gen (Required from rust-ci.yml, trim engine ci.yml) (#1523)" committed 2026-11-19 20:31 +07 = **13:31:05Z** |
| Follow-ups | `533fc35623` (#1550) delete legacy engine workflows, 20:37 +07; `5d175c6b7f` (#1420) delete old engine Rust CI lane code, 21:54 +07 |
| Merge queue last real use | last merge_group run 2026-11-19 12:26Z (old `CI`); last enqueue 12:12Z; last queue-merged PR 10:38Z. Ruleset v58581881 at 12:30Z dropped `Required verdict`. |
| Window | 3.0 days of data. **Pre-cutover = 2026-11-16 17:00Z to 2026-11-19 13:31Z (68.5 h). Post-cutover = 13:31Z to 17:50Z (only 4.3 h).** Post numbers are a small sample and from a day when main was being repaired. |
| Run duration definition | `updated_at - run_started_at` of the run (completed success/failure only) unless stated "created to last job end". |

Headline: after the cutover CI is fast to answer (Rust CI PR run P50 1.1 min, heavy-lane runs P50 2.9 min) and has no hosted-runner queue, but it
fails almost always (Rust CI on PR: 2 success of 268; on main push: 4 success of 172), and PRs are merged anyway, directly and within a minute of the last
push, so the merge queue and the `Required` gate are bypassed. Throughput went from 2.7 to 41 merged PRs per hour. The speed numbers below are real;
the gate is not currently gating.

## 1. Merged PRs in the window (412 merged since 2026-11-16 00:41Z; 360 inside the 17:00Z window, 187 pre + 173 post)

Last push = created_at of the first `pull_request` workflow run on the PR's final head SHA (for 359 of 360 PRs; fallback commit date).
Ready = last ReadyForReview event, else PR creation. Label `ci:run`/`ci:auto` was applied on only 4 PRs, so label to merge is not measurable.

| Metric (minutes) | Pre-cutover n | P50 | P90 | Post-cutover n | P50 | P90 |
|---|---|---|---|---|---|---|
| Last push to merge | 187 | **121.8** | **700.8** | 172 | **0.6** | **3.8** |
| Ready/open to merge | 187 | 448.9 | 1365.8 | 173 | 28.9 | 1483.9 |
| PR created to merge | 187 | 530.0 | 1445.9 | 173 | 110.8 | 1715.6 |
| Enqueue to merge (PRs that went through the queue) | 144 | 27.4 | 474.0 | 16 (stale, see below) | 1352 | 1842 |
| Last push to enqueue | 135 | 85.0 | 814.0 | 0 | - | - |
| Merged PRs per hour | 187 / 68.5 h = 2.7 | | | 173 / 4.2 h = 41 | | |

Baseline (2026-11-18): push-to-merge P50 85 / P90 457. The pre-cutover window here (Nov 16 to 19) is slower: P50 122 / P90 701.

Merge queue usage:
- Pre-cutover: 144 of 187 PRs merged through the queue (77%). 520 enqueue events for the 412 merged PRs: heavy churn. Removal reasons across the window:
  manual 318, merged 165, merge_conflict 22, failed_checks 13, checks_timed_out 3. Peak 62 enqueues in one hour (04:00Z Nov 19).
- merge_group `CI` runs: 289 (all pre-cutover), duration P50 26.8 / P90 76.6 min, 169 success / 104 failure / 16 cancelled (36% failed).
- Post-cutover: **zero merge_group runs, zero enqueue events**. 157 of 173 PRs merged "direct". The 16 flagged as "queue" were enqueued before 12:12Z, dequeued, and merged directly hours later.
- The ruleset still contains a `merge_queue` rule (HEADGREEN, max_entries_to_build 2, min wait 5 min, check timeout 90 min) and `Rust CI` still lists `merge_group: {}`, but nothing uses them.

Why post-cutover merge is 0.6 min after the last push: every one of the 173 post-cutover PRs was merged by `donbeave`; for 167 of 173 the
`Required` rollup on the head commit contained a FAILURE (115 failure only, 13 failure+skipped, 23 failure+success from different runs, 16 older queue entries), no `Required` check for 5, SUCCESS only for 1.
The ruleset requires `Required` and has a bypass actor (RepositoryRole 5 = repository admin, mode `pull_request`), so these merges can only be admin bypass merges (inference: GitHub's audit log is not readable here).

## 2. Workflow runs by event (all runs created in the window; 6177 runs)

Duration P50/P90 over success+failure runs only. "Other" = skipped/in progress.

### Post-cutover (13:31Z to 17:50Z, 4.3 h)

| Workflow | Event | Runs | Dur P50 | P90 | Success | Failure | Cancelled | Skipped/other |
|---|---|---|---|---|---|---|---|---|
| Rust CI | pull_request | 268 | 1.1 | 5.8 | 2 | 143 | 108 | 15 skipped |
| Rust CI | push | 172 | 1.2 | 9.7 | 4 | 166 | 0 | 2 in progress |
| CI (ci.yml, alint/deny/machete/jactionlint) | pull_request | 295 | 0.8 | 2.2 | 12 | 205 | 78 | 0 |
| CI | push | 172 | 0.8 | 1.4 | 3 | 138 | 31 | 0 |
| Source qualification | pull_request | 10 | 13.6 | 22.1 | 0 | 10 | 0 | 0 |
| Release Rust CI tools | push | 10 | 4.1 | 4.8 | 9 | 1 | 0 | 0 |
| Rust cache cleanup | workflow_run | 4 | 0.2 | 0.2 | 0 | 2 | 2 | 0 |
| Cache prune (old) | workflow_run | 17 | 0.4 | 0.5 | 6 | 0 | 1 | 10 skipped |
| State seal (old) | push | 3 | 1.0 | 1.0 | 0 | 1 | 2 | 0 |
| velnor-actions generator release jobs (old) | push | 5 | - | - | 0 | 0 | 5 | 0 |

Rust CI post-cutover, by what ran (created to last job end, from per-job data): PR runs where heavy lanes ran n=141, P50 2.9 / P90 6.2 min (58 failed, 83 cancelled by a newer push);
PR runs where plan skipped every lane n=127, P50 0.8 / P90 1.2 (85 still failed, because the `velnor-ci plan`/gate step or `Cache: Check cache waste` failed);
push runs with heavy lanes n=74, P50 7.5 / P90 11.3 (72 failed); plan-only push n=98, P50 0.9.
`CI` (ci.yml) failures: the Alint step, e.g. "Rust source exceeds 400 physical lines (rust-quality-contract section 5)", and main itself is red.
Rust CI failure causes in a random sample of 60 failed runs (failing step counts): `rust-plan: velnor-ci plan` 33 (e.g. helper-install SHA-256 mismatch in setup-velnor), `Required: velnor-ci gate` 36,
Lint in crates/generator/ci_helper lanes ~58 combined, Test ~40, Format 17, `Cache: Check cache waste` 17.

### Pre-cutover (2026-11-16 17:00Z to 2026-11-19 13:31Z, 68.5 h)

| Workflow | Event | Runs | Dur P50 | P90 | Success | Failure | Cancelled | Skipped/other |
|---|---|---|---|---|---|---|---|---|
| CI (old engine) | pull_request | 1256 | 26.8 | 73.2 | 495 | 358 | 403 | 0 |
| CI | merge_group | 289 | 26.8 | 76.6 | 169 | 104 | 16 | 0 |
| CI | push | 164 | 19.6 | 66.0 | 75 | 26 | 63 | 0 |
| Source qualification | pull_request | 1206 | 37.8 | 99.3 | 723 | 293 | 190 | 0 |
| Job image candidate | push | 1602 | 12.4 | 37.1 | 703 | 18 | 881 | 0 |
| State seal | push | 164 | 18.1 | 48.5 | 95 | 15 | 54 | 0 |
| velnor-actions generator release jobs | push | 164 | 136.2 | 298.8 | 89 | 22 | 21 | 32 skipped, 1 other |
| Cache prune | workflow_run | 333 | 1.7 | 12.4 | 210 | 0 | 38 | 85 skipped |
| Qualification | workflow_dispatch | 9 | 18.5 | 21.2 | 7 | 0 | 2 | 0 |
| Velnor product releases | workflow_dispatch | 11 | 11.4 | 42.8 | 1 | 6 | 4 | 0 |
| Release Rust CI tools | push / schedule | 5 / 3 | 11.2 / 1.0 | 14.1 / 6.8 | 3 / 0 | 1 / 3 | 1 / 0 | 0 |
| Rust CI (early, pre-switch) | pull_request | 15 | 1.6 | 5.5 | 0 | 9 | 6 | 0 |

PR-event CI cancellation pre-cutover: 403 of 1256 (32%), queue churn from re-pushes. Pre-cutover about 3600 workflow runs for ~187 merged PRs (~19 runs per merged PR, mostly
`Job image candidate`, `Source qualification`, `CI` on PRs), post-cutover 1300 runs/4.3 h but `Job image candidate`, `State seal` and most `Source qualification` runs no longer fire.
No schedule runs of Rust CI or workflow_dispatch of Rust CI occurred in the window. `qualification-gen.yml` and `rust-cache-trim.yml` produced no runs of their own in the window except the `Rust cache cleanup` workflow_run rows above.

## 3. Representative runs: per-job queue, run time, runner labels, critical path

Queue = job `created_at` to `started_at`. Label `ubuntu-26.04-scale-set` is the self-hosted Velnor scale set. Hosted pre-cutover jobs carry label `ubuntu-26.04` with runner name "GitHub Actions NNN".
No `Rust CI` merge_group run exists (it never ran, the queue was abandoned before the first one); the pre-cutover `CI` merge_group data in 3.3 stands in.

### 3.1 Rust CI, pull_request (all jobs self-hosted, `ubuntu-26.04-scale-set`)

Run 44540312154 (regen-only change, success, wall 56 s). Plan selected nothing.

| Job | Queue s | Run s | Result |
|---|---|---|---|
| rust-plan | 13 | 16 | success |
| Required | 2 | 11 | success |
| Cache | 2 | 23 | success (runs after plan, ends last: critical path = plan 30 s then Cache 56 s) |
| policy, rust-root, rust-local, rust-helper, rust-ci_helper, rust-generator, rust-crates_velnor_runner, rust-inputs-audit, Cache maintenance | - | - | skipped by plan |

Run 44536064168 (feature PR, failure, wall 712 s, 13:42Z). Critical path: rust-plan (31 s) then rust-root (queue 15 s, run 601 s, ends at 647 s) then Required + Cache (queue 36 s each, ends 712 s).

| Job | Queue s | Run s | Result |
|---|---|---|---|
| rust-plan | 2 | 15 | success |
| rust-helper | 13 | 94 | failure |
| policy | 14 | 78 | failure |
| rust-generator | 14 | 143 | failure |
| rust-local | 14 | 130 | success |
| rust-crates_velnor_runner | 14 | 320 | failure |
| rust-root | 15 | **601** | failure (critical path) |
| rust-ci_helper | 16 | 101 | failure |
| Required / Cache | 36 / 36 | 12 / 29 | failure |
| rust-inputs-audit, Cache maintenance | - | - | skipped |

Run 44545389418 (failure, wall 290 s): plan 20 s, then policy/rust-root/rust-ci_helper/rust-crates/rust-generator in parallel (queue 21 to 26 s, run 97 to 210 s), critical path rust-crates_velnor_runner (ends 267 s), Required 7 s.
The helper/local/inputs-audit/Cache maintenance jobs were skipped by plan.

### 3.2 Rust CI, push on main

Run 44545511926 (regen-only commit, success, wall 180 s): rust-plan queued 73 s (the longest queue seen), ran 20 s; all 7 lane jobs + policy skipped; Cache maintenance, Required, Cache ran (queues 17/20/47 s).

Run 44534242943 (the cutover commit itself, failure, wall 526 s): plan 31 s queue + 18 s; policy, rust-root, rust-ci_helper, rust-helper, rust-local, rust-crates_velnor_runner, rust-generator all started 82 to 109 s after run creation (queue 19 to 59 s);
critical path rust-crates_velnor_runner (381 s run, ends 471 s) then Required (11 s) and Cache (21 s); rust-root failed after 46 s, rust-ci_helper 257 s.
Across 160 push runs: queue P50 6 to 13 s, P90 20 to 57 s; skip rates by plan 63% for each lane (101 of 160 skipped for rust-root, rust-ci_helper, crates_velnor_runner, generator, helper).

### 3.3 Pre-cutover `CI` merge_group (40 sampled runs, stands in for the missing Rust CI merge_group), hosted vs self-hosted

| Job group | Runner | Queue P50 | Queue P90 | Run P50 |
|---|---|---|---|---|
| Alint, Cargo Deny, Cargo Machete, Jactionlint, Ledger CLI, Plan, Required verdict (gates) | GitHub-hosted for ~half the runs (19 of 40), scale set for the rest | 287 to 368 s | 1632 to 2318 s (27 to 39 min) | 15 to 62 s |
| 28 per-crate Rust lanes + generator-samples | Velnor scale set | 11 to 21 s | 199 to 454 s | 51 to 126 s (velnor-actions-cli 434 s, runner-host 518 s, runner-host local-mode 606 s, maintenance-helpers 596 s) |

Run wall (created to last job end), sampled: merge_group P50 30.4 / P90 88.0 min; pull_request P50 26.5 / P90 105.9; push P50 27.0 / P90 76.2.

Share of job time that is queue (sum queue / (queue + run), sampled jobs):

| Period | Workflow / event | Hosted jobs | Self-hosted jobs |
|---|---|---|---|
| Pre | CI / merge_group | 97% (2988 of 3075 min) | 47% |
| Pre | CI / pull_request | 96% (2741 of 2855 min) | 40% |
| Pre | CI / push | 93% (1918 of 2053 min) | not meaningful (some jobs have started_at before created_at) |
| Post | Rust CI / pull_request | 0 hosted jobs | 15% (214 of 1396 min) |
| Post | Rust CI / push | 0 hosted jobs | 13% (195 of 1480 min) |

Post-cutover every Rust CI job runs on `ubuntu-26.04-scale-set` (1584 PR jobs + 1076 push jobs sampled, 0 on GitHub-hosted); queue P50 2 to 13 s, P90 13 to 57 s. The 79% hosted-gate queue wait of the baseline is gone for Rust CI.
The remaining queue is the scale-set spin-up (rust-plan queue up to 73 s when many runs start together); `rust-root` is the long pole (run P50 67 s PR / 371 s push, P90 365 / 560 s).

## 4. Required checks, rulesets, merge queue settings

| Setting | Value |
|---|---|
| Classic branch protection on main | none (HTTP 404 "Branch not protected") |
| Repo merge settings | squash only (allow_merge_commit false, allow_rebase false), allow_auto_merge **false**, public repo |
| Ruleset `protect-main` (id 24396608), enforcement active, target default branch | rules: deletion, non_fast_forward, required_linear_history, pull_request, required_status_checks, merge_queue |
| pull_request rule | 0 required approvals, dismiss stale reviews on push, require thread resolution, squash only |
| required_status_checks | **`Required` only** (strict = false, do_not_enforce_on_create = false). Since 2026-11-19 12:30Z (v58581881); before that `Required` + `Required verdict` (v58469928, 11-18 21:39Z) |
| merge_queue | method SQUASH, grouping HEADGREEN, max_entries_to_build 2, max_entries_to_merge 10, min_entries_to_merge 1, min wait 5 min, check_response_timeout 90 min. Before 2026-11-16 15:50 +07 it was ALLGREEN, build 5, merge 5, wait 0 |
| Bypass | RepositoryRole 5 (admin), mode `pull_request` |
| Other rulesets | protect-tags, release-tags, sdk-preview-owner-creation (target tag) |
| Producer of `Required` | `rust-ci.yml` job `Required` (generated, `required_check = "Required"`). `ci.yml` no longer emits `Required` or `Required verdict` |

Consequences: `Required` fails on nearly every run (226 of 268 PR runs), so merging is only possible through the admin bypass (observed). `Rust CI` has `merge_group: {}`, but since the queue is not used no `Required` check is ever created on a queue commit.
`ci.yml` (alint, cargo-deny, cargo-machete, jactionlint) is not required by the ruleset any more (its jobs fail on most runs too: 205 of 295 PR runs).

## 5. Is the same content built multiple times?

Counted per merged PR: runs on the PR's final head SHA (`pull_request`), merge_group runs on `gh-readonly-queue/main/pr-N-*`, and `push` runs on the squash merge commit.

| Period (merged PRs) | Workflow | PR runs per PR | merge_group runs per PR | push runs per PR | Summed run wall minutes per PR (pr / mg / push) | PRs with both a PR run and a main push run |
|---|---|---|---|---|---|---|
| Pre, queue era (151) | CI (heavy engine) | 1.19 | 1.47 | 0.85 | 44.0 / 51.2 / 21.8 = 117 | 127 |
| Post (173) | Rust CI | 1.29 | 0.00 | 0.99 | 2.5 / 0 / 3.9 = 6.4 | 165 |
| Post (173) | CI (ci.yml lint jobs) | 1.35 | 0.09 | 0.99 | 1.5 / 3.3 / 0.8 = 5.6 | 172 |

Answer: pre-cutover the content was verified three times (PR, merge_group, main push, about 117 min of run wall per merged PR, plus `Source qualification` on PRs at 1206 runs and `Job image candidate` at 1602 pushes).
Post-cutover it is verified twice, by the PR run on the head SHA and the push run on the merge commit, because the merge_group leg disappeared (not because of dedup logic). With direct merges the PR head SHA and the squash commit have the same tree only when the PR was based on current main;
the push run is usually a re-run of nearly identical content, and `velnor-ci plan` skips 52% of lanes on PR runs and 63% on push runs, so the second pass is cheaper (3.9 vs 2.5 min summed wall, but failing runs stop early so this understates a green path: heavy-lane push runs P50 7.5 min).
The 83 cancelled heavy PR runs and 78+31 cancelled `CI` runs are re-push supersession, not duplication of the same SHA.
Cluster CPU (92 to 95% baseline) was not measurable with gh.

## 6. Caveats

- Post-cutover sample is 4.3 h, on a day with repeated fix-forward commits to main (`fix(ci): make main's Rust CI and CI green` at 17:07Z; 21 of 21 Rust CI push runs in the 17:00Z hour still failed). Success-path durations are therefore under-sampled (only 2 PR and 4 push Rust CI successes, all regen/docs-only, 56 to 180 s).
- Run duration uses `updated_at - run_started_at`, so re-run attempts and the Required job tail are included; per-job numbers use `created_at`/`started_at`/`completed_at` from the jobs API.
- Pre-cutover job tables are a random sample of 40 runs per event (seed 1) of completed success/failure `CI` runs before 12:30Z; post-cutover covers all 440 Rust CI runs.
- "Last push" is a proxy (first run created on the final head SHA); PRs merged in under 1 minute are real but mean no CI result was awaited.
- Admin-bypass merging is inferred from mergedBy + failing `Required` + ruleset; GitHub does not expose the bypass flag through these endpoints.

## 7. Commands used (all read-only GETs plus GraphQL queries)

```bash
# cutover
cd ~/.cache/ci-ambition/main-tip
git log --format='%h %cI %s' -n 400 | grep -n '#1523\|#1550\|cut over'
git show -s --format='%H%n%cI%n%B' 053fefa6a3

# rulesets / protection / repo settings
gh api repos/tailrocks/velnor-new/rulesets
gh api repos/tailrocks/velnor-new/rulesets/24396608
gh api repos/tailrocks/velnor-new/rules/branches/main
gh api repos/tailrocks/velnor-new/branches/main/protection        # 404 not protected
gh api repos/tailrocks/velnor-new --jq '{allow_auto_merge,allow_squash_merge,allow_merge_commit,allow_rebase_merge}'
gh api repos/tailrocks/velnor-new/rulesets/24396608/history
gh api repos/tailrocks/velnor-new/rulesets/24396608/history/58581881   # also 58469928 58385420 57958569
gh api repos/tailrocks/velnor-new/actions/workflows --paginate --jq '.workflows[]|[.id,.name,.path,.state]|@tsv'

# all workflow runs in the window (API caps one query at 1000 results, so 6 h slices, split recursively; see fetch_runs.py)
gh api 'repos/tailrocks/velnor-new/actions/runs?created=2026-11-19T12:00:00Z..2026-11-19T17:50:00Z&per_page=100' --paginate --jq '.workflow_runs[]'
gh api 'repos/tailrocks/velnor-new/actions/runs?event=merge_group&per_page=5'

# merged PRs
gh pr list -R tailrocks/velnor-new --state merged --search 'merged:>=2026-11-16' --limit 1000 \
  --json number,title,createdAt,mergedAt,isDraft,headRefOid,mergeCommit,labels,additions,deletions,autoMergeRequest
# timelines (15 PRs per GraphQL call, fetch_tl.py): PullRequestCommit, HeadRefForcePushedEvent, ReadyForReviewEvent, LabeledEvent,
#   AddedToMergeQueueEvent, RemovedFromMergeQueueEvent, MergedEvent via timelineItems(first:100,itemTypes:[...])
gh api graphql -f query='query{repository(owner:"tailrocks",name:"velnor-new"){ p123: pullRequest(number:123){ number mergedAt headRefOid mergeCommit{oid} timelineItems(first:100,itemTypes:[ADDED_TO_MERGE_QUEUE_EVENT,REMOVED_FROM_MERGE_QUEUE_EVENT,MERGED_EVENT,READY_FOR_REVIEW_EVENT,LABELED_EVENT,PULL_REQUEST_COMMIT]){ nodes{__typename} } } }}'
# Required status on each post-11:00Z merged PR (fetch_req.py): commits(last:1){nodes{commit{statusCheckRollup{contexts(first:40){...}}}}}

# per-job data (queue/run/labels) for 440 Rust CI runs + 120 sampled CI runs
gh api "repos/tailrocks/velnor-new/actions/runs/<RUN_ID>/jobs?per_page=100" --paginate \
  --jq '.jobs[]|{id,name,status,conclusion,created_at,started_at,completed_at,labels,runner_name,run_id}'
# failing steps for a 60-run sample
gh api "repos/tailrocks/velnor-new/actions/runs/<RUN_ID>/jobs?per_page=100" --jq '[.jobs[]|select(.conclusion=="failure")|{job:.name,steps:[.steps[]|select(.conclusion=="failure")|.name]}]'
gh run view 44544626205 -R tailrocks/velnor-new --log-failed      # plan failure cause
gh run list -R tailrocks/velnor-new -w "Rust CI" -L 12 --json databaseId,event,conclusion,createdAt,headBranch,displayTitle
```

Analysis scripts (Python, read the saved JSON): `an_runs.py` (section 2), `an_prs.py` (section 1), `an_jobs.py` / `an_share.py` / `show.py` (section 3), `an_dup.py` (section 5).
