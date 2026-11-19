# Rust-only redesign: rulings on the contested questions

Date: 2026-11-19. This document rules on Q1–Q12, Q14–Q17 and Q19 of `rust-only-comparison.md`. Q13 is closed by owner decision 7 and Q18 is with the owner; neither is ruled here.

**Method.** Votes were not counted. Each ruling rests on evidence checked for this document: GitHub documentation fetched today (sources D1–D16 at the end), code at `path:line`, read-only `gh api` calls against tailrocks/velnor-new, the official runner at its latest release tag, and reproductions run under `~/.cache/ci-ambition/adjudicate/` (`q5`, `q7`, `q11`, `q14`, `q16`). That directory is local to the adjudication machine, so each reproduction is described here fully enough to repeat. Confidence is *settled* (documented or reproduced), *likely* (the evidence points one way and one link is unobserved) or *needs a lab test*. A lab-test ruling names the experiment and a default that stays correct if the fact goes the other way. The adjudicator runs on the same model as one panel member (Fable 5.2); Q1, Q2, Q10 and Q11 overturn parts of that member's answer.

## Summary

| Q | Ruling | Confidence | Credited |
|---|---|---|---|
| Q1 | A skipped job passes a required check, also when it is skipped through `needs`. A skipped reusable-workflow caller leaves the inner check unreported: observed, undocumented, never to be the lock. | settled; likely | all nine; Fable, Neon (the observation); Opus, Astra, Sol (not relying on it) |
| Q2 | One required context: an App commit status `Velnor / Verified`, bound by `integration_id`, for PR heads and merge groups. Actions `Required` stays on `push` to `main` only. No bypass actors. `gate.rs` changes first. | likely | Astra, Sol, Opus; Carbon, Fable, Muse Wonder, Argon (transition, canary) |
| Q3 | Not established. Reconcile from REST state every 10 s; webhooks only shorten the delay; no label is applied with `GITHUB_TOKEN`. | needs a lab test | Opus, Fable, Astra, Sol |
| Q4 | Yes: v2.338.2 returns the legacy evaluator's result on every path, and `Expressions2` is that evaluator. Port the new one. | settled | Opus, Fable; Muse Wonder Full |
| Q5 | Cargo refuses a workspace with a missing member, and a sparse sandbox turns a real failure green. Run on the full tree; narrow only the key, by audited reads. | settled | Opus; Neon, Astra, Sol |
| Q6 | Tests stay in the dev profile. Release bytes are built once per key in the merge group, stamped with the group SHA, smoke-tested as shipped and promoted by digest. | likely | Opus; Fable |
| Q7 | Yes. With dependency assertions and overflow checks on, the measured test uses 67% less CPU; the checks cost 5% of instructions. | likely | Carbon, Argon, Neon (the claim); Fable, Opus, Astra, Muse Wonder Full (it was unmeasured) |
| Q8 | Yes: a dispatched run's check lands on the head commit under integration 15368. That makes it a fallback, and it is why an Actions check cannot be the lock. | likely | Fable, Muse Wonder Full, Carbon |
| Q9 | Yes: a verdict belongs to a commit. No extra machinery. | settled | Astra, Sol (raised it) |
| Q10 | ALLGREEN now. Build depth 2, then 4. | settled; likely | Opus, Fable, Astra, Sol, Carbon; Astra, Sol (depth) |
| Q11 | 74–83% at 1–2.5 min and 67% at 5 min, only with auto-enqueue against the queue tip. Today 16%. | likely | Sol, Astra, Opus |
| Q12 | Two executions before merge on different nodes, and one cold audit after landing. | likely | Opus, Fable |
| Q14 | About 5 s to a queued job and 8 s to a running one. Not on the verdict path. | settled | Fable; Opus, Astra, Sol |
| Q15 | v2.338.2. Deadlines come from the API: 2.337.0 stops on 2026-12-25 and 2.338.0 on 2027-01-04. | settled | Astra, Sol; Argon; Opus, Fable, Neon, Muse Wonder, Muse Wonder Full |
| Q16 | Reflink clone is the default. Overlayfs and the transport need a head-to-head. A hard-linked base is not adopted. | needs a lab test | Fable, Muse Wonder Full, Opus; Neon, Carbon |
| Q17 | Off on every build that mints a receipt or ships bytes. | needs a lab test | Fable, Carbon, Muse Wonder, Muse Wonder Full, Astra, Sol |
| Q19 | A private privileged daemon per request on tainted nodes. | needs a lab test | Fable, Muse Wonder, Neon, Argon; Opus (the experiment) |

## Evidence gathered for this document

- **V1. Ruleset today.** `gh api repos/tailrocks/velnor-new/rulesets/24396608` (`protect-main`): required contexts `Required verdict` and `Required`, neither with an `integration_id`; merge queue `SQUASH`, `HEADGREEN`, build 2, merge 1–10, wait 5 min, timeout 90 min; `bypass_actors`: repository role 5 (admin), mode `pull_request`.
- **V2. The bypass is in use.** `GET /repos/tailrocks/velnor-new/activity?ref=refs/heads/main`: since the queue started (2026-11-16 01:12 UTC, 3.0 days) `main` moved 136 times: 129 `merge_queue_merge` and 7 `pr_merge`. PR #1379 has no queue event in its timeline and merged 4 s after `ready_for_review`.
- **V3. Red commits on `main`.** Of 239 `merge_group` runs of `ci.yml` (172 PRs), 45 failed. Three failed group heads are ancestors of `main`: `d2eaecb3a1` (#1276), `41e7beb8c3` (#1364) and `b2413d7709` (#1310). Each has exactly one `Required` check run, with conclusion `failure`, and no push run.
- **V4. The group SHA is the `main` SHA.** `merge_group` run 44395550209 and `push` run 44399226704 have the same head, `90ce2ad4b1`. The commit below it in the same group, `c3cd9a6304`, has no push run.
- **V5. Offer latency.** Ref update on `main` to `ci.yml` push run created, n=656: p50 3 s, p90 4 s, p99 5 s, five outliers of 122–424 s. Run created to first job created: 1–2 s in 24 of 24 `merge_group` runs; 46–1,792 s in 8 of 12 `push` runs, which queue in a concurrency group. Job created to started on the cluster scale set, n=1,124: at most 3 s for 30% (an idle runner), p50 13 s, p90 27 s.
- **V6. Replay of queue entries.** 172 first merge groups in 70.6 h, 2.44 entries per hour; 8% arrive within 10 s of the previous entry; at most 3 arrive within 120 s and 4 within 300 s. Share with no other queue entry, group re-creation or bypass merge in the preceding window: 83% (1 min), 78% (2), 74% (2.5), 67% (5), 56% (10), 37% (20). Share built directly on the `main` tip: 37%. The landing-gap replay that v2 §6.6 quotes gives 97, 90, 87, 80, 68 and 51% on the same days.
- **V7. Runner source.** `gh api repos/actions/runner/releases/latest`: `v2.338.2`, published 2026-10-26, commit `d1eb12d4`. The tag is the 2.338.0 branch point `67f01c2` plus three release commits; `git diff v2.338.0 v2.338.2` is 14 lines of version and SDK pins in 11 files. The panel's clone `7afce9e` is unreleased `main`: 46 commits past the branch point, differing from the tag in 31 non-test files under `src/`, among them #4785 "Stop fallback to old messagelistener on github.com", #4799 (GitHub context in embedded steps) and #4836 (node20 removed).
- **V8. Deprecation dates.** `GET /repos/tailrocks/velnor-new/actions/runners/deprecations/{version}`: `runtime_deprecates_at` is `null` for 2.338.2, `2027-01-04T16:53:28Z` for 2.338.0, `2026-12-25T15:32:22Z` for 2.337.0 and `2026-11-05T08:04:55Z` for 2.336.0.
- **V9. Label events.** The 400 most recent `labeled` and `unlabeled` events: 390 by the owner's user identity, 5 through an App on the owner's behalf, 5 by `renovate[bot]`, none by `github-actions[bot]`.
- **V10. Skipped callers.** Run 44409336518 (`product-release.yml`) skipped its three reusable-workflow callers. Its check suite 120217424263 holds seven check runs. The three callers appear once each, with conclusion `skipped`, and no `caller / inner job` check exists. Run 44430641287, where the callers ran, has them.
- **V11. Dispatched check.** Run 42326119857 (`workflow_dispatch` of `ci.yml` on branch `codex/guarded-crate-release-main-merge`) created check suite 114680945852 (app 15368) on commit `b21046dd33`. The suite contains a check run named `Required`.

## Q1. Does a skipped required job count as success, and does a skipped caller leave the inner check pending?

- **Ruling.** (a) A required check is satisfied by a job skipped by its own `if:` and by a job skipped because a job it `needs` failed. (b) When a reusable-workflow caller is skipped, GitHub creates one check for the caller and none for the called jobs, so a required `caller / Required` shows "Waiting for status to be reported" and blocks. Behaviour (b) is observed and has no documentation behind it. If GitHub changed it, every unlabelled PR would turn green, so the final design does not use it. Q2 names the lock.
- **Evidence.** D1: "A job that is skipped will report its status as "Success". It will not prevent a pull request from merging, even if it is a required check." D2, table "Handling skipped but required checks": "A job is skipped by a conditional" gives "The job reports "Success""; "A job depends on a failed job" gives "The dependent job is skipped and may not block merging"; "A workflow is skipped by path filtering, branch filtering, or a commit message" gives "Associated checks stay in a "Pending" state and block merging". V10 for (b). Today's `Required` already guards the `needs` case with `if: ${{ always() }}` and a failing step (`.github/workflows/ci.yml:614-625`).
- **Confidence.** (a) settled. (b) likely: one run, three callers, and the skip came from a failed `needs`, not from `if:`.
- **Lab test and default.** Lab A, cases A6 and A7. Default: no required context is produced by anything that can be skipped (Q2). It is correct whichever way (b) goes.

## Q2. Which object may be the required check?

- **Ruling.**
  1. The ruleset for `main` requires exactly one context: the commit status `Velnor / Verified`, written by a dedicated GitHub App (working name `velnor-verify`) and bound to that App's id. GitHub keeps one required-check list per branch. It applies the list to the PR head, to merge or to enter the queue, and to the head of each merge group, to land. The same status therefore gates both, on two different SHAs.
  2. No Actions job is in that list. A check from Actions cannot be the lock: integration 15368 is every workflow in the repository, including one the PR adds or edits (Q8), and a skipped job passes (Q1).
  3. `ci.yml` keeps one always-run job named `Required` on `push` to `main`. The release gate reads it. It is outside the ruleset, so it is never on the merge path.
  4. `bypass_actors` is empty. Decision 12 allows a merge only with a passing verdict on the latest commit, and the admin bypass carried 7 of 136 merges in three days (V2). A break-glass, if the owner wants one, is an explicit ruleset edit.
  5. `gate.rs` is changed and released before the ruleset changes.
- **Ruleset, final state** (`PUT /repos/tailrocks/velnor-new/rulesets/24396608`; the `deletion`, `non_fast_forward`, `required_linear_history` and `pull_request` rules stay as they are; `max_entries_to_build` stays 2 and the timeout stays 90 until the condition in Q10 holds):

```jsonc
{
  "enforcement": "active",
  "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}},
  "bypass_actors": [],
  "rules": [
    {"type": "required_status_checks", "parameters": {
      "strict_required_status_checks_policy": false,
      "do_not_enforce_on_create": false,
      "required_status_checks": [
        {"context": "Velnor / Verified", "integration_id": APP_ID}   // integer from `gh api /apps/velnor-verify --jq .id`
      ]}},
    {"type": "merge_queue", "parameters": {
      "merge_method": "SQUASH", "grouping_strategy": "ALLGREEN",
      "max_entries_to_build": 4, "min_entries_to_merge": 1, "max_entries_to_merge": 10,
      "min_entries_to_merge_wait_minutes": 0, "check_response_timeout_minutes": 15}}
  ]
}
```

| Where | Required by the ruleset | Written by | On which SHA |
|---|---|---|---|
| PR: merge button and queue entry | `Velnor / Verified`, `integration_id` = the App | the App, after the ledger verdict | `pull_request.head.sha` |
| `merge_group` | the same entry | the App, on `merge_group.checks_requested` | `merge_group.head_sha` |
| `push` to `main` | nothing | Actions job `Required` (integration 15368), read by the release gate | the pushed tip |

- **The label flow** (decision 12), driven by App webhooks and by the reconciler of Q3. `pull_request.labeled` with `velnor:verify` starts verification of the current head and writes `pending`. `synchronize` on a labelled PR cancels the running request and starts one on the new head. `unlabeled` and `closed` cancel and write nothing. A push to an unlabelled or draft PR, and `ready_for_review`, cause nothing. `merge_group.checks_requested` verifies the group head, reusing the PR's verdict only on an identical tree; `merge_group.destroyed` cancels.
- **Order of change.** Step 0, configuration only, now: add `"integration_id": 15368` to today's two contexts (they accept any source, V1), set `bypass_actors: []` and `ALLGREEN`; `gate.rs:280` already accepts 15368. Step 1: release the changed `velnor-ci`. Step 2: the App runs in shadow while today's per-push Actions CI stays the gate. Step 3, after Lab A and 300 heads with no case of App `success` beside an Actions failure: require all three contexts and start the label flow; from here a label `if:` on the Actions jobs is harmless, because a skipped job no longer unlocks anything. Step 4: drop the two Actions contexts. Before step 3 the label flow must not start on Actions alone with a job-level `if:`. If the owner wants it before the App exists, the only safe Actions shape is Muse Wonder's and Argon's: an always-run job with no `if:` and no `needs:` that fails when the label is absent.
- **Why an unverified head can never be green.** Each step rests on documented behaviour.
  1. Only the App satisfies the context. D5: "If the status is set by any other person or integration, merging won't be allowed." The App's private key exists only in the control plane; no workflow holds it.
  2. A status has one passing state. D10: "Can be one of: `error`, `failure`, `pending`, `success`". A check run has eight conclusions (D11), and two more of them pass: D16 says required checks "must have a `successful`, `skipped`, or `neutral` status". So the gate is a status, and the detail view is a check run with a different name (D2: "If a check and a commit status have the same name, both must pass when that name is required.").
  3. One function writes `success`. Its input is a signed ledger verdict that names the subject SHA, the verified tree and the plan epoch; it posts to `/statuses/{subject SHA}` and nowhere else. The plan comes from the released helper, never from the PR's tree.
  4. A push makes a new SHA, which has no status. D2: "Required checks must pass on the latest commit SHA." "Checks from earlier commits don't satisfy the requirement."
  5. The App never writes the context on the PR's test-merge commit. D2, table "Conflicts between head commit and test merge commit": "Test merge commit has a status" means "The test merge commit" must pass.
  6. Absence blocks. D2: "Waiting for status to be reported." An unlabelled PR, a lost webhook and a crashed controller all leave the head without a status.
  7. Nothing bypasses the rule: `bypass_actors` is empty, and the tokens that agents use carry no repository administration, so only the owner can edit the ruleset. Label changes and merges are made under the owner's identity today (V9, V2), so agents need an identity of their own without that permission.
  - The verdict belongs to the commit and the label only starts work (decision 12). So a head verified while labelled stays green after the label is removed, and Q9 follows.
  - Consequence: a PR has no Actions run. People and agents read the verdict from the status and its detail check run (per-obligation results and log links), not from `gh run`.
  - Detection: Fable's canaries. Lab A cases A1, A4 and A6 stay open as permanent PRs and are probed every 10 min; any `mergeable_state` other than `blocked` pages the owner and stops the App from writing `success`. Writes are serialised per SHA, and the reconciler of Q3 compares the newest status on every labelled head and queue branch with the ledger and corrects a difference, so a late or duplicate write cannot leave a `success` the ledger does not hold.
- **The release gate keeps working.**
  - `.github/workflows/product-release.yml:86` needs no change. It requires the newest `ci.yml` push run on the `main` tip to have succeeded and to contain exactly one job named `Required` with conclusion `success`.
  - `velnor-ci release-gate` refuses the new ruleset as written. `require()` rejects every binding other than 15368 and −1 (`ci-helper/crates/velnor-ci/src/gate.rs:277-288`). The PR-only exemption cannot carry the App status either: it rejects a name that has a status on the commit (`gate.rs:84-94`), and the App status is always on the released commit because the group SHA is the `main` SHA (V4).
  - Change. `--app-check 'Velnor / Verified'=velnor-verify` makes that one binding legal. The gate then requires the newest status with that context on the release SHA, created by `velnor-verify[bot]`, to be `success`, inside an active ruleset that has a `pull_request` rule and explicitly empty `bypass_actors` (the existing `require_enforcement`, `gate/exemptions.rs:136-212`). No such status means no release, so a commit that went around the queue cannot ship. `--push-check Required` keeps the existing push-job test (`judge`, `gate.rs:354-376`) for a name the ruleset no longer lists. Any other non-Actions binding still refuses.
  - Order. Release the helper, pin it in `.github/workflows/release-rust-ci-tools.yml:100` and in the generator (`generator/crates/velnor-render/src/release.rs:118-134`), then change the ruleset. Until step 4 the flags permit the new shape without demanding it.
- **Evidence.** V1, V2, V4. D3 (`merge_group`, `checks_requested`): "When you receive this event, you should perform checks on the head SHA and report status back using check runs or commit statuses." D6: `integration_id` is "The optional integration ID that this status check must originate from." D7: a PR is queued "Once a pull request has passed all required branch protection checks". D2: "Otherwise, status checks will not be triggered when you add a pull request to a merge queue. The merge will fail as the required status check will not be reported."
- **Confidence.** Likely. Every link is documented; the combination has not run against this repository.
- **Lab test and default.** Lab A, cases A1–A5, A9 and A10. Default until it passes: steps 0 to 2 above. Today's gate stays in force, hardened, and the App only observes, so the default is correct even if a status binding or the queue behaved differently from the documentation.

## Q3. Do GitHub App webhooks fire when a label is changed with `GITHUB_TOKEN`?

- **Ruling.** Not established: the documentation does not say, and no panel member measured it. The design must be right either way.
  1. Safety does not depend on delivery. Without the event nothing is verified, no status is written and the PR stays blocked.
  2. Liveness comes from state, not from events. Every 10 s a reconciler reads the open PRs that carry the label, with their head SHAs, and the merge queue's branches. It compares them with the requests it is running, starts what is missing and cancels what is no longer wanted: label removed, head moved, group destroyed. A webhook only cuts the delay from 10 s to about 1 s.
  3. Labels are applied with a user or App token. Nothing in the owner's flow uses `GITHUB_TOKEN` for it (V9), and decision 12 rules out automatic labelling. A workflow that must label mints an App token.
  4. The PR path has no Actions trigger, so the one suppression that is documented cannot stall it.
- **Evidence.** D4: events from `GITHUB_TOKEN` "will not create a new workflow run", and "Other `pull_request` activity types (such as `labeled`, `edited`, or `closed`) do not create workflow runs." The text limits the rule to workflow runs; D3 lists no exception for the `pull_request` webhook. D9: "GitHub does not automatically redeliver failed deliveries." So a design that depends on any single delivery is wrong whatever the answer. Verified today: `GET /repos/tailrocks/velnor-new/pulls?state=open&per_page=100` returns labels, head SHA and draft state (the repository has at least 100 open PRs, so the reconciler lists by label); the same request with `If-None-Match` returned `304`, and D14 says "Making a conditional request does not count against your primary rate limit if a `304` response is returned"; `GET .../git/matching-refs/heads/gh-readonly-queue/main/` lists the queue's branches and SHAs.
- **Confidence.** Needs a lab test.
- **Lab test and default.** Lab A, case A11, read with `GET /app/hook/deliveries`. Default: the four points above.

## Q4. Does the latest official runner still return the legacy evaluator's result, and which evaluator is legacy?

- **Ruling.** Yes. At v2.338.2 the Worker returns the legacy evaluator's result on every path. By the runner's own names the legacy evaluator is `GitHub.DistributedTask.Expressions2` (`src/Sdk/DTExpressions2`) with the `DTPipelines` template evaluator, and decision 3 excludes it. The four designs that copy velnor-legacy's module as the one non-legacy evaluator copy a port of the legacy one: `velnor-legacy/crates/velnor-runner/src/expression/mod.rs:15` cites `src/Sdk/DTExpressions2/Expressions2/`. The final design has one production evaluator, ported from `src/Sdk/Expressions` and `src/Sdk/WorkflowParser`. The copied module stays in the conformance harness as a reference. The oracle runs with `ACTIONS_RUNNER_COMPARE_WORKFLOW_PARSER=true`, so GitHub's own comparer reports every job on which the two official evaluators differ; such a job is not qualified, and Velnor refuses it, until the difference is understood.
- **Evidence** (all at tag v2.338.2). `src/Runner.Worker/ExecutionContext.cs:1543-1565`: without the feature flag `actions_runner_compare_workflow_parser` (`src/Runner.Common/Constants.cs:175`) or that environment variable, the method returns `new PipelineTemplateEvaluator(...)` under the comment `// Legacy` (line 1554). With it, `src/Runner.Worker/PipelineTemplateEvaluatorWrapper.cs:224-319` runs `_legacyEvaluator`, then `_newEvaluator`, records `TemplateEvaluatorMismatch` telemetry, re-throws the legacy exception (313-316) and ends with `return legacyResult;` (318). `src/Runner.Worker/ActionManifestManagerWrapper.cs:375-455` does the same for action manifests (`return legacyEvaluator();` at 386, `return legacyResult;` at 454). The two trees are close: the legacy tree has 57 files (4,983 lines), each with a same-named file among the new tree's 72 (6,947 lines); the largest per-file difference is 123 lines, and the new behaviours default to off (`StrictJsonParsing = false`, `src/Sdk/WorkflowParser/WorkflowFeatures.cs:75`). The port is a file-by-file retarget of the copied module, not a rewrite.
- **Confidence.** Settled.

## Q5. Does cargo refuse `-p` builds when other members' sources are missing, and is a sparse sandbox sound?

- **Ruling.** Cargo refuses, and a sparse sandbox is unsound.
  1. Cargo loads every member's manifest before it builds anything; `-p` does not make other members optional.
  2. A sandbox that holds only a closure can be made to load, with stub sources or a trimmed member list. It then turns a real failure green: a test that lists, or optionally reads, paths outside its declared closure sees fewer files and passes. "An undeclared read fails with ENOENT" is not a safety argument, because absence is an input.
  3. The verdict of one package's tests depends on which packages are selected with it.
  - Final design. Every obligation executes on today's input set, which is the whole tree (today's checkout is `/*` without `/docs/**`, `ci.yml:97`). A closure narrows the key, never the sandbox. A key may be narrower than the tree only for an obligation whose audited reads (opens, stats, directory listings and failed lookups) all fall inside it; one read outside widens that obligation's key to the whole tree. The key includes the exact package selection and features. "The restricted run equals the unrestricted run" is not an enrolment proof: the two agree on every tree in which the unseen files happen to be clean.
- **Evidence** (`adjudicate/q5`, cargo 1.99.1; workspace `a`, `b`, `c`, `dep`; `a`'s tests read sibling members).
  - Member `b` without `src/`: `cargo build -p a --offline --locked` fails with "failed to load manifest for workspace member" and "no targets specified in the manifest". Member `b` absent: "failed to read .../b/Cargo.toml". Member `c` with an explicit `[lib] path` and no sources: the build succeeds.
  - Defect placed in `b`. Full tree: `cargo test -p a` exits 101, `no_member_source_contains_banned_word ... FAILED`. Sandbox with `a` and `dep` and the member list trimmed to them, same lockfile, `--locked`: exit 0, 3 passed. Sandbox with empty stubs for `b` and `c`: that test passes as well.
  - `cargo test -p a`: `dep_feature_x_is_off ... ok`. `cargo test --workspace`: the same test fails with `left: "x-on"`, because `b` enables the feature.
  - In velnor-new, 79 of the 86 `.rs` files that use `CARGO_MANIFEST_DIR` climb out of their package, and 70 test files list directories.
- **Confidence.** Settled.

## Q6. Where and when are release bytes built, and which profile do tests use?

- **Ruling.**
  1. Tests keep the dev profile. Testing the release build is not available: Cargo ignores `panic` for test targets, and the tools ship with `panic = "abort"`, thin LTO and `strip`, as musl binaries. No test build is the shipped code. One `verified` profile for both (Muse Wonder Full) would change what ships and is not adopted.
  2. Release bytes are one obligation, keyed by closure, lockfile, toolchain, profile, target and, while the commit id is compiled in, the commit SHA. The key is first computable at the merge group, whose SHA becomes the `main` SHA, so the bytes are built there, in parallel with verification. For the tools that is the release PR's group, and the build and a smoke test of the shipped bytes are part of that group's verdict. The product is released by dispatch on the `main` tip, so its bytes are built in each group, outside the verdict, in place of today's per-push preview build. With the commit id in the key that is one build per tip, as today, and none at release.
  3. The tag job fetches by digest, re-hashes, attests and publishes. A miss refuses the release; it never builds.
- **Evidence.** V4, and evidence pack §A T7 (120 of 159 green groups have a push run on the same SHA). D12: "Tests, benchmarks, build scripts, and proc macros ignore the `panic` setting." `[profile.release]` in `ci-helper/Cargo.toml` and `generator/Cargo.toml`: `opt-level = "s"`, `lto = "thin"`, `strip = true`, `panic = "abort"`. Musl targets: `generator/crates/velnor-render/src/release_binaries.rs:44-45`. The commit id is a compile input: `option_env!("VELNOR_COMMIT")` at `ci-helper/crates/velnor-ci/src/io/mod.rs:78` and `generator/crates/velnor-gen/src/io/mod.rs:50`, and `VELNOR_RUNNER_BUILD_SOURCE_SHA` in `crates/velnor-runner/crates/velnor-runner-host/build.rs:14-23`. Today's rebuild at the tag: `.github/workflows/release-rust-ci-tools.yml:143-150`.
- **Confidence.** Likely. The facts are settled. The placement is a design choice that cannot produce a false green: bytes built for a group that does not land carry a SHA that never reaches `main`.
- **Open item.** Fable's stamp-after-link would take the SHA out of the key and cut the product build from once per tip to once per closure. It is a lab item (the patched binary must pass the same smoke test and attestation), not a default.

## Q7. Is dependency `opt-level` optimisation valid with assertions on?

- **Ruling.** Yes. With dependencies at `opt-level = 3` and `debug-assertions` and `overflow-checks` both on, the slowest-class test uses 67% less CPU and 74% fewer instructions than under today's dev profile. Turning the two checks off as well saves a further 5% of instructions and no measurable CPU. Carbon, Argon and Neon cite a figure that was measured with assertions off (`panel/draft/proposals/fable-5.2.md:345`, `panel/fable-5.2/notes.md:77`); the citation was wrong and the claim holds. Adopt the override with both checks written out, and raise `turso_core`'s own line too, because a named override beats `"*"`. The override removes no assertion. It changes every compile key once.
- **Evidence** (`adjudicate/q7`; cargo 1.99.1 on an Apple M5 Max under load, so instructions retired is the stable measure). Test `launch::bootstrap::unbound_retirement::tests::shape::an_aborted_initializer_with_no_containers_is_claimed` in the `velnor-runner-host` lib test binary: one case, run alone, two runs per configuration. The repository was not edited: profiles were set with `--config 'profile.dev.package."*".opt-level=3'` and the like, with a scratch target directory, `--locked --offline`.

| Configuration | Instructions retired | User CPU | Build, wall |
|---|---|---|---|
| A. Dev profile as committed: dependencies 0, `turso_core` 1 | 1.727 and 1.728 × 10¹¹ | 23.5 and 23.0 s | 470 s |
| B. Dependencies 3, `turso_core` 3, both checks on | 4.55 and 4.53 × 10¹⁰ | 7.8 and 7.6 s | 778 s |
| C. As B, both checks off | 4.31 and 4.30 × 10¹⁰ | 7.5 and 7.8 s | 733 s |

- **Reading the table.** A reproduces Fable's baseline (142.5 CPU-s for six cases, 23.8 s per case) and C his 47.9 CPU-s (8.0 s per case). The build times are wall time at `-j 8` on a busy machine and only show the direction: the optimised dependency build costs more.
- **Profile rules.** D12: overrides match in the order named package, `"*"`, build override, and "Overrides cannot specify the `panic`, `lto`, or `rpath` settings"; the dev defaults are `debug-assertions = true` and `overflow-checks = true`. The repository already overrides `sha2` to 3 (`Cargo.toml:69-73`) and `turso_core` to 1 (`crates/velnor-runner/Cargo.toml:74-75`).
- **Confidence.** Likely: one test case on one machine class.
- **Lab test and default.** Lab B, part B1, for the whole suite on a worker and for the compile cost of a lockfile change. Default: the override may be adopted now with both checks on, because it cannot turn a failure green. No target counts the gain until B1.

## Q8. Can a dispatched Actions run satisfy the PR's required check?

- **Ruling.** Yes, for a check bound to Actions. A `workflow_dispatch` run of `ci.yml` on the PR's head branch attaches its `Required` check run to the head commit under integration 15368, which is all an Actions-bound required check matches. Two consequences follow. Dispatch works as a trigger fallback for an Actions-bound check. And an Actions-bound check cannot be the lock, because the workflow that produces it is read from the branch under test: a PR that edits `ci.yml`, or adds any workflow with a job named `Required`, mints the check itself. Under Q2 the final design needs no dispatch on the PR path.
- **Evidence.** V11. D6: "The status check context name that must be present on the commit." D13: for `workflow_dispatch`, `GITHUB_SHA` is the "Last commit on the `GITHUB_REF` branch or tag"; for `pull_request` it is the "Last merge commit on the `GITHUB_REF` branch", so the workflow file comes from the PR's own merge commit.
- **Confidence.** Likely: the data model and the documentation agree; the merge box was not observed with a dispatched check.
- **Lab test and default.** Lab A, case A8. Default: nothing in the final design depends on the answer. If it is yes, the `integration_id` binding closes the hole; if it is no, no path uses dispatch.

## Q9. Do two open PRs at the same head SHA share a green?

- **Ruling.** Yes, and nothing needs to be built for it. A status belongs to a commit, so every PR whose head is that commit shows it. This agrees with decision 12: the gate is a passing verdict on the PR's latest commit, and the verdict is true of that commit for every PR into `main`, because the verified tree is `merge-tree(queue tip, head)` in each case. The merge group then verifies the exact merged tree again. Sol's topic refs and ref-writer gateway and Astra's reviewer-rule witness are rejected: each adds a write path for refs or reviews, and decision 8 removes the hostile author they defend against. One rule remains: the ruleset covers `main` only. If another base branch is ever protected, the context must name the base.
- **Evidence.** D6: the context "must be present on the commit". D10: a status is created at `/repos/{owner}/{repo}/statuses/{sha}`; no field names a PR. V11: check runs are listed per commit.
- **Confidence.** Settled. Lab A, case A9, records it.

## Q10. HEADGREEN or ALLGREEN, and how deep?

- **Ruling.** ALLGREEN, now. HEADGREEN lands commits whose own required check failed, and it has done so three times in three days (V3). That contradicts decision 12 ("a passing verdict on the exact merged tree") and decision 11, so this ruling closes v2's owner question 3. Depth is a cost setting under ALLGREEN, not a correctness setting: `max_entries_to_build` stays 2 until a merge-group verdict takes at most 5 min at P90 and paired false failures are at most 3%, then 4. Nothing supports 8.
- **Evidence.** D6: "When set to HEADGREEN, only the commit at the head of the merge group, i.e. the commit containing changes from all of the PRs in the group, must pass its required checks to merge." "When set to ALLGREEN, the merge commit created by merge queue for each PR in the group must pass all required checks to merge." V3: #1276's group run ended in failure at 14:00:16 UTC on 2026-11-18, three minutes after #1037's group above it passed, and both commits are on `main`. V6: 2.44 entries per hour, at most 3 within 120 s and 4 within 300 s. At today's 8.1% false failures per run, depth 4 disturbs 29% of entries and depth 8 disturbs 49% (v2 §6.5 arithmetic, 1−(1−f)^n).
- **Confidence.** ALLGREEN settled; the depth likely.

## Q11. How often will the merge-group tree equal the PR-tested tree?

- **Ruling.** The evidence supports three numbers, not one.
  1. Today: 16%. Seven of 45 sampled groups had the PR head's tree, with hours between verdict and group. Only 37% of first groups are even built directly on the `main` tip, because 20-minute group runs keep the queue occupied.
  2. The 78–89% that six models quote is the share of landings with no other landing in the preceding 1–5 min. It reproduces (80–97%). But a group's base changes when another PR enters the queue, not when it lands, and entries are burstier than landings.
  3. Replayed on entries, the share of groups whose base is unchanged over the window is 83% at 1 min, 78% at 2, 74% at 2.5, 67% at 5, 56% at 10 and 37% at 20.
  - The third set holds only if the PR verdict is computed on `merge-tree(queue tip, head)` and the PR enters the queue inside the window. That needs auto-merge, which 0 of 40 PRs use; at today's median green-to-enqueue delay of 7.5 min the figure is about 60%. Sol is right that the quoted figures do not transfer; the cause is the event that was replayed. Plan on 75% at best, and on 60% without auto-merge.
- **Evidence.** V6; evidence pack §A T7; v2 §6.6.
- **Confidence.** Likely for the replay (three days, 172 entries, one arrival pattern). The hit rate of the real design needs Lab C, part C1.
- **Default.** A merge group always derives its own keys and executes every miss, so no verdict depends on the rate. Latency targets carry the group's delta cost for one merge in four.

## Q12. How many pre-merge executions of a changed test keep today's confidence?

- **Ruling.** Two before the merge and one after. Today a changed test runs once in the PR run and once in the merge-group run before the merge, and once more in the push run after it. v2's three parallel executions before the merge are one more than that. The third buys detection that today's process does not have, and costs false failures the budget does not have.
  - Every obligation whose key is new runs twice in parallel, on two different nodes, before the PR's status goes green. Any failure blocks. There is no retry, as in today's CI profile (`crates/velnor-runner/.config/nextest.toml:14`, `retries = 0`).
  - A merge group runs once more every obligation whose key the PR's receipts do not cover.
  - A cold, reuse-free audit of every landed commit is the third execution. The `main` push `Required` job reports it, so the release gate keeps the execution it has today.
- **Evidence.** Evidence pack §A T7: the PR, merge-group and push runs execute the same 39 jobs, and 120 of 159 green groups have a push run on the same SHA. v2 §1.1: 10 of 124 push runs failed on a SHA whose group had passed, 8.1%. Arithmetic: a regression that fails with p = 0.2 is caught by two runs with 36% and by three with 49%; at 8.1% false failures per run, two runs fail falsely 15.5% of the time and three 22.4%.
- **Confidence.** Likely. The count is today's. What is not measured is whether two simultaneous runs on two nodes are as independent as today's two runs hours apart.
- **Lab test and default.** Lab C, part C2. Default: the twin runs on different nodes with a different test order. If C2 shows that twins pass together more often than (1−p)² predicts, the merge group re-executes the obligations the PR changed even on an identical tree. That is today's shape exactly, and it costs the delta's time on each merge.

## Q14. How long does GitHub take to offer an Actions job, and is the offer on the verdict path?

- **Ruling.** GitHub creates the run 3 s after the event and the first job 1–2 s later, and an idle runner starts it within 3 s. An Actions job is therefore queued about 5 s and running about 8 s after the event. Fable's 9 s is the closest figure; v2's 15 s is not supported. The offer is not on the verdict path: under Q2 the App status judges the PR head and the merge group, and verification starts on the webhook. The offer stays on the release path only, in the `main` push `Required`, where runs also queue behind older pushes.
- **Evidence.** V5. A design that kept an Actions `Required` in the ruleset would pay the larger of the verification time and about 10 s on every verdict, and would inherit two tails: 0.8% of runs appear 2–7 min after the event, and a run that supersedes a running one waits 60 s at P50 and 626 s at P90 today (v2 §1.1). A superseded App request holds no GitHub job, so cancelling it cannot stall the next one, which is what decision 12 asks for.
- **Confidence.** Settled for this repository in the scale-set era.

## Q15. What is the latest official runner version, and what is the update deadline?

- **Ruling.** The latest release is v2.338.2 (2026-10-26, `d1eb12d4`); Astra and Sol are right. The source for the oracle and for every citation is that tag, not the supplied clone, which is unreleased `main` (V7). The rule is 63 days from the day a newer version becomes available, which is not its release date; a critical update stops assignment at once. The binding dates are per version and readable (V8). Velnor reports the newest version it has certified. The conformance run for a new release finishes within 14 days, and an alarm fires 21 days before the `runtime_deprecates_at` of the version in use.
- **Evidence.** V7, V8. D8: "If you disable automatic updates, you must update your runner version within 63 days of a newer version being made available." "If a critical security update is required, the service will not assign jobs until the runner is updated." "The date a runner version becomes available for updates can differ from its release date." D15 names the endpoint. The dates fit the rule: 2027-01-04 less 63 days is 2026-11-02, seven days after v2.338.2 was published. v2 §3.1 cites v2.337.0 for its evaluator; a runner that reports 2.337.0 stops receiving jobs on 2026-12-25.
- **Confidence.** Settled.

## Q16. Reflink clones or overlayfs, and which cross-node transport?

- **Ruling.** Default: a reflink clone of the site builder's generation, at the fixed path, on XFS. It is the only variant measured on the cluster. Reflink and an overlayfs lower layer both isolate writes by construction. A hard-linked `deps/` base (Argon) is not adopted: every sandbox holds the same inode, so its isolation rests on file ownership being right on every path, and a mistake puts one request's bytes into another's verdict. An overlayfs lower layer shares the page cache the same way with a read-only layer the kernel enforces. Two rules apply to every variant. The materialiser sets mtimes itself: unchanged files keep the generation's recorded mtimes, and every file whose blob differs gets an mtime later than the generation's newest output. A planted older-mtime edit runs as a standing canary. No transport is on the verdict path: a node without the generation builds cold, or the request goes to a node that has it.
- **Evidence.** `v2/experiments/mbx-cache-cluster.md`: the clone takes 0.25 s and 2.5 MB (P2); each fully read clone holds its own 14.29 GB of page cache (E1d); overlayfs is "not tested"; `mbx-cache` seeds a cold node in 39.9 s against 92 s (M3) and a lockfile change in 43.4 s against 67.4 s (L1mb), uploads 6.8 GiB for an 11-crate change, and ran with garbage collection off and metadata in memory. Carbon's 13 s layer seed is an estimate. Reproduced here (`adjudicate/q16`, cargo 1.99.1): after a warm build, a source file is changed to a failing value and stamped older than the build; `cargo test` exits 0 with `answer_is_42 ... ok` and compiles nothing. With a current mtime it recompiles and fails. `-Z checksum-freshness` exists, but "the `-Z` flag is only accepted on the nightly channel".
- **Confidence.** Needs a lab test.
- **Lab test and default.** Lab B, parts B2 and B3. Default as ruled, with at most six build sandboxes on a 125 GiB node (6 × 14.29 GB), so the measured worst case costs memory and re-reads and never correctness.

## Q17. Incremental compilation on or off?

- **Ruling.** `CARGO_INCREMENTAL=0` on every build that mints a receipt or ships bytes, as today. Incremental state is allowed only in lanes that mint nothing, such as an agent's own fast feedback. The gain is real and the risk is of the wrong kind: a stale incremental object is a test passing on code that is not in the tree. The audit that catches this for clean builds, digest equality between warm and cold test binaries (v2 C6), is not expected to apply to incremental outputs, whose code-generation units differ.
- **Evidence.** `ci.yml:346` sets `CARGO_INCREMENTAL: "0"`, and the cluster experiment ran with 0. `v2/experiments/blaze.md` B3: 1.5 s against 9.2 s and 2.8 s against 12.2 s per edit. v2 §6.3: 10.9 s against 61 s for `velnor-runner-host`. D12: "Incremental compilation is only used for workspace members and "path" dependencies."
- **Confidence.** Needs a lab test.
- **Lab test and default.** Lab B, part B4. Enrol only at zero differences, and keep the cold audit non-incremental. Until then, take the compile-floor gain from the crate split, which raises no soundness question.

## Q19. Docker-needing tests: privileged class, rootless, or microVM?

- **Ruling.** Default: the Docker-needing test binaries run as their own obligation with a private daemon per request, in a privileged sidecar, on tainted nodes. That is today's isolation, kept for the three binaries that need it. Decision 8 removes the hostile author, so a microVM is not required for safety. Two rules are required: a daemon is never shared between requests, and a Docker test that is skipped or missing fails the obligation. Rootless and microVM daemons are experiments that must first show the same pass, fail and skip sets.
- **Evidence.** v2 §3.8 names the binaries: `local_pair_public`, `local_host_public` and `velnor-local`'s Docker tests. `crates/velnor-runner/.config/nextest.toml:5-24`: a wrapper sets `VELNOR_TEST_DOCKER_HOST` for the first two, and the group `docker-networks` limits them to 4 threads. Evidence pack §B: a privileged dind sidecar in every pod and 57 probe failures per hour. No measurement of a rootless or microVM daemon exists.
- **Confidence.** Needs a lab test.
- **Lab test and default.** Lab D. Default as ruled; it is today's mechanism.

## Lab experiments

- **Lab A. GitHub semantics.** A lab repository with a ruleset, a merge queue and a test App subscribed to `pull_request` and `merge_group`. For each case record `mergeable_state` from `GET /repos/{o}/{r}/pulls/{n}`, the merge-box text and `GET /app/hook/deliveries`. A1 no status. A2 App `pending`. A3 App `success`, then one more push. A4 `success` with the same context from a user token and from `GITHUB_TOKEN` with `statuses: write`. A5 App `success` on the test-merge SHA only. A6 a required Actions job with `if: false`. A7 a caller with `if: false` whose inner job is required. A8 a `workflow_dispatch` run on the head branch under an Actions-bound requirement. A9 two PRs at one SHA, one labelled. A10 queue entry with no report, with `failure`, with `success`. A11 a label added and removed by a workflow with `GITHUB_TOKEN`, with an App token and with a user token. Pass: A1, A2, A4 and A7 blocked; A3 mergeable, then blocked; A6 and A8 mergeable, which documents the traps; A9 both mergeable; A10 lands only on `success`; A5 and A11 are recorded.
- **Lab B. Build matrix.** One idle worker, the runner workspace at a pinned commit, three repetitions. B1: dependency `opt-level` 0 and 3 against `debug-assertions` on and off; test CPU-seconds for the whole `velnor-runner-host` suite, and compile CPU-seconds after a lockfile change. B2: reflink clone against an overlayfs lower layer with 1, 4 and 6 concurrent sandboxes; wall time, page cache, crates compiled (0 on an unchanged tree), the planted older-mtime edit (must rebuild), and test-binary digests against a cold build. B3: seeding a cold node and applying a lockfile change with `mbx-cache`, file-delta sync and compressed layers; seconds and bytes; keep a transport only if it beats a cold build on more than 10% of requests. B4: `CARGO_INCREMENTAL` 0 against 1 over 300 replayed historical diffs and 20 planted failing edits; verdicts and test inventories must be identical.
- **Lab C. Shadow ledger**, beside production, for 300 merges. C1: for each merge group, the whole-tree hit and the per-obligation hits against the PR's receipts. C2: twin executions on two nodes against two executions at least 30 min apart, on 20 tests with a seeded failure probability of 0.05–0.2; compare the joint pass rates with (1−p)².
- **Lab D. Docker class.** The three Docker-needing binaries, 200 runs each, under a privileged daemon per request, a rootless daemon and a microVM with a root daemon. The pass, fail and skip sets must equal the first; record start time and flake rate; a container, network or image left by one request must be invisible to the next.

## Sources

- D1: https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-jobs-with-conditions
- D2: https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/collaborating-on-repositories-with-code-quality-features/troubleshooting-required-status-checks
- D3: https://docs.github.com/en/webhooks/webhook-events-and-payloads#merge_group
- D4: https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow
- D5: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets
- D6: https://docs.github.com/en/rest/repos/rules
- D7: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue
- D8: https://docs.github.com/en/actions/reference/runners/self-hosted-runners
- D9: https://docs.github.com/en/webhooks/using-webhooks/handling-failed-webhook-deliveries
- D10: https://docs.github.com/en/rest/commits/statuses
- D11: https://docs.github.com/en/rest/checks/runs
- D12: https://doc.rust-lang.org/cargo/reference/profiles.html
- D13: https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows
- D14: https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api
- D15: https://docs.github.com/en/rest/actions/self-hosted-runners
- D16: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches

## Changes the final design must make

Each group is one commit, "research: final design, Rust-only fixes from <model>". Section numbers are v2's.

**GPT-6.2-Astra**
1. §6.1, §6.4: the required context on PR heads and merge groups is the App commit status `Velnor / Verified`; no conditional job is ever a gate (Q1, Q2).
2. §6.4: keep one genuine Actions `Required` on the `main` push for the release gate; the gate recognises the App proof explicitly, never as an unchecked exemption (Q2).
3. §3.6: the oracle and every citation move to tag v2.338.2; a critical runner update stops assignment at once, so the drift canary alarms on it (Q15).
4. §6.5: build depth 2, then 4, raised only on a measured gain (Q10).
5. §4.1: observed reads may propose key declarations but never prove a key complete; start from the full tree (Q5).

**Claude Opus 5.5**
6. §3.4, §6.1: no Actions run on PR pushes; the App's context is bound by `integration_id`; handle `merge_group.destroyed` and reconcile missed deliveries (Q2, Q3).
7. §3.1, row "The older `DTExpressions` evaluator": replace `Expressions2` with a port of `src/Sdk/Expressions` and `src/Sdk/WorkflowParser` (Q4).
8. §1.2 C2, §4.1: delete "a sparse checkout of exactly those paths"; full tree plus a syscall read audit that includes failed lookups and directory listings (Q5).
9. §1.2: changed obligations run twice, on two nodes, not three times (Q12).
10. §3.9, §6.4: release bytes are built in the merge group with the group SHA (the release PR's group for the tools); a smoke test runs on the shipped musl bytes; the tag job never compiles (Q6).
11. §3.9: tests keep the dev profile; one profile for tests and shipped bytes is not adopted, because Cargo ignores `panic` for tests and the shipped targets are musl (Q6).
12. §5, §12: the planted older-mtime edit is a standing canary (Q16).

**GPT-6.2-Sol**
13. §6.4: migrate `gate.rs`, the generator's release step and its pins as one compatibility change, helper first (Q2).
14. §6.1: the App's detail view is a check run with a different name from the required status (Q2).
15. §6.6: tree-equality figures taken from landings do not transfer; use the entry replay and Lab C (Q11).
16. §4: the plan comes from the released helper; a candidate cannot weaken its own gate (Q2, Q8).
17. §4.3: a missing, duplicate or skipped expected test fails the obligation, Docker tests included (Q19).
18. §5: the mtime rule alone never authorises a hit; the cold digest comparison stays (Q16).

**Claude Fable 5.2**
19. §3.6: permanent canary PRs that must stay unmergeable, probed every 10 min, and Lab A as the cutover proof (Q1, Q2, Q8).
20. §3.1: `Expressions2` is the runner's legacy evaluator; the copied port lives in the harness only, with a fuzzer against both official evaluators (Q4).
21. §4.4: a cold audit of every landed commit is the third execution, reported by the `main` push `Required` (Q12).
22. §6.6: "GitHub creates it about 15 s after the push" becomes the measured 5 s to queued and 8 s to running (Q14).
23. §5: `CARGO_INCREMENTAL=0` on every receipt-minting build (Q17).
24. §3.9: stamping the commit id after link is a lab item; it is what would make the product build once per closure instead of once per tip (Q6).

**Adjudication (evidence new in this document; no panel member)**
25. Phase 0, configuration only: `bypass_actors: []` (7 merges went around the queue in 3 days), `integration_id: 15368` on today's two contexts (they accept any source) and `grouping_strategy: ALLGREEN` (three commits with a failed `Required` are on `main`) (Q2, Q10).
26. §6.4: `gate.rs` gains `--app-check` and `--push-check`; the PR-only exemption cannot carry the App status, because the group SHA is the `main` SHA (Q2).
27. §6.6: replace 89, 78–83, 70 and 52–53% with 83, 74–78, 67, 56 and 37%, and state the two preconditions: tip-targeted verification and auto-enqueue (Q11).
28. §3.6 and evidence pack §F: `7afce9e` is unreleased `main`; list its three behaviours that no release has; read the conformance deadline from the deprecations endpoint (Q15).
29. §5: the materialiser sets mtimes explicitly; the false green is reproduced, and `-Z checksum-freshness` is nightly-only on 1.99.1 (Q16).
30. §4.1: a receipt key carries the exact package selection and features; reproduced with `-p a` against `--workspace` (Q5).
31. §6.3: dependency `opt-level = 3` holds with assertions on: 67% less CPU on the measured test, and the checks cost 5% of instructions; write `debug-assertions = true` and `overflow-checks = true` out and raise `turso_core`'s line (Q7).
32. §3.4: the reconciler's three REST reads are verified against this repository, including the `304` path; a workflow that must label mints an App token (Q3).
33. §3.8: agents get an identity without repository administration, so the party the ruleset constrains cannot edit it (Q2).
34. §6.1: state that a verdict belongs to a commit, so PRs at one SHA share it; no PR-scoped witness and no ref gateway is built (Q9).

**Gemini 4 Carbon**
35. §6.1: during the transition both contexts are required, the Actions one and the App one; absence blocks (Q2).
36. §5: an overlayfs lower layer and compressed layer streaming enter Lab B as candidates, with no default role; overlayfs replaces the hard-linked `deps/` base, because it shares the page cache with isolation the kernel enforces (Q16).

**Gemini 4 Neon**
37. §4.1: feature unification follows the selected packages, so each obligation fixes its selection (Q5).
38. §7: the page-cache cost of reflink clones sets the per-node sandbox limit until Lab B2 decides on overlayfs (Q16).
39. §11: the four-way `opt-level` by assertions matrix runs on a worker before any target counts the gain (Q7).

**Gemini 4 Argon**
40. §3.6: read `runtime_deprecates_at` for the version in use and alarm 21 days ahead (Q15).
41. Phase 0: the generator hardcodes hosted runners at `generator/crates/velnor-render/src/release.rs:29` and `release_binaries.rs:44-46`; decision 9 needs them gone.

**Muse Wonder Full**
42. §3.6: the oracle runs in compare mode and the corpus keeps every case on which the two official evaluators differ (Q4).
43. §5: the transport head-to-head runs in phase 1 with a kill criterion (Q16).

**Muse Wonder**
44. §3.8: the Docker class on tainted nodes with a private privileged daemon is the default (Q19).
45. §6.1: "not requested" is shown as its own state and is excluded from false-failure accounting (Q2).
46. §6.1: if the label flow must start before the App exists, the only Actions shape is the always-run job that fails without the label (Q2; also Argon).
