# velnor-gen rebase by Claude Fable 5.2

| Field | Value |
|---|---|
| Model | Claude Fable 5.2 (`claude-fable-5-2`) |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | high |
| Date | 2026-11-20 |
| Main tip analysed | `0eb2d76a5c` |
| Brief | [velnor-gen rebase brief v1](../briefs/velnor-gen-brief-v1.md) |

Independent answer from the Velnor runner research panel, written without seeing other members' answers. Unedited except for this header and replacing home-directory paths with `~`.

---

# Rebasing the Velnor plan on velnor-gen (fable-5.2, 2026-11-20)

Checkout read: `~/.cache/ci-ambition/main-tip` at `0eb2d76a5c` (#1748). Reports A–E read in full.
Paths are relative to the checkout root. Short names: `R` = `.github/workflows/rust-ci.yml` (666 lines
at this tip; A's line numbers are for an older 726-line file), `M/` = `crates/velnor-model/src/`,
`Ru/` = `crates/velnor-rust/src/`, `Re/` = `crates/velnor-render/src/`, `Ci/` = `crates/velnor-ci/src/`,
`G/` = `crates/velnor-gen/src/`. Tags: [C] read in code at the tip; [M] measured by me today with
read-only `gh api` calls (run ids given); [R] taken from a report A–E; [E] my estimate or inference;
[T] target. I changed nothing in the repository, on GitHub or on the cluster.

Where the brief or a report is out of date at this tip (all verified):

| Brief / report says | At `0eb2d76a5c` | Evidence |
|---|---|---|
| 31 "Check crate alone" steps | 18 (the 13 engine crates are gone) | R:214-267 [C] |
| Label flow is owner decisions 15–17 | It is decision 12 in the merged file | `docs/research/velnor-runner-2026-11/owner-decisions.md:20` [C] |
| `rust-root` P50 67 s PR / 371 s push | Those are jobs of the old multi-root shape. Today's single `rust` job: 564 s on a push that selects everything, 305 s on a four-package push, 90 s for a PR quick set | runs 44584072510, 44584861409, 44583969919 [M] |
| Open PRs #1608, #1320, #1200, #1026, #1297 | #1320 was closed unmerged at 2026-11-19T17:57Z. Open: #1608, #1297, #1200, #1026, and draft #1751 (this research round; I did not open it) | `gh pr list` [M] |
| `mbx-store` trusts push, schedule and merge_group (B§3.3) | Only `push` and `schedule` on the default branch | `Ci/mbx_store.rs:32-39` [C] |
| `.velnor/rust-ci.toml` has three `[[inputs]]` (A§1.3) | None. The file has 32 lines and no `[[inputs]]`; the old path rules live only in `.velnor/config.toml [stacks.rust]`, which the default `velnor-ci plan` does not read | `.velnor/rust-ci.toml`, `.velnor/config.toml:1-10` [C] |

Two findings no report has (details in section 3):

- **No test has run in Rust CI in the four `main` push runs I opened.** The `Test` step dies after
  12–24 s with `error: the argument '--no-fail-fast' cannot be used with '--no-run'` (runs
  44576523025, 44578162215, 44584072510, 44584861409 [M]). The helper builds a preflight by inserting
  `--no-run` into the test command (`Ci/io/audit/build/invocation.rs:79-87` [C]), and the generator
  renders that command with `--no-fail-fast` (`Re/cargo.rs:295` [C]).
- **A skipped `Required` job counts as a pass.** The `required` job has an `if:` that is false for an
  unrelated label and for a title or body edit (R:576 [C]), so the job is skipped. GitHub reports a
  job skipped by a condition as success for a required check. Run 44577473873 concluded `skipped`
  eight seconds after run 44577457255 failed on the same head `e0a6357a` [M].

---

## 1. Verdict in ten lines

1. The final design stands in its goals and in its core idea: one verdict per tree, proven once,
   reused everywhere, with correctness as the hard limit. Its build order does not stand.
2. **The single most important change: the centre of the plan moves from the runner to the generator.**
   The ledger, the verdict keys, the label flow and the reuse chain are built first, inside
   `velnor-model`, `velnor-rust` and `velnor-ci`, on today's official runner. The native executor
   comes after and becomes much smaller.
3. Reason: `velnor-gen` already owns every fact the ledger needs. It knows every check step
   (`is_check`, `Ru/merge_group_skip.rs:9-29`), wraps every check command in `velnor-ci run`
   (R:207-299), plans from a released helper before any repository code runs (R:88-90), and has a
   typed key renderer (`Re/cache.rs:19`).
4. The runner is no longer the bottleneck. Queue wait is P50 2–13 s on the scale set [R: C§3.3]. The
   time is inside the one `rust` job and in doing the same work on PR, queue and push.
5. A native executor for velnor-new does not need the Actions protocol. All velnor-new workflows
   except `ci.yml` and `release-rust-ci-tools.yml` are generated, so Velnor Native can execute the
   plan itself. Broker, run service, Results, expressions and JavaScript actions leave the critical path.
6. Correctness comes first and it is worse than the plan assumed. Main is red, tests do not run, the
   gate passes without tests in four ways, and 157–167 of 173 merges bypassed it [R: C§1].
7. Phase 0 is therefore "make the gate real": green main, a `Required` that can never be green
   without a verdict, a bound ruleset with no bypass, the queue with ALLGREEN.
8. The speed targets keep their end values. The start is new: the cutover already beat the old
   interim row, so the goal is about 3x away from a green baseline, not 30x.
9. Dropped: everything about the engine, per-root keys, `executor/` as a separate Cargo root, native
   emulation of actions, and deleting the runner crates. Gated on the owner: the `docs/goals.md:16` change.
10. New and ambitious: the generator emits the verification contract; keys are computed at plan time;
    selection becomes "what is not yet proven"; one plan drives YAML, native and local runs.

---

## 2. The design mapped onto velnor-gen

### 2.1 The pipeline that exists, and the four places a component can live

`RepoView` → `detect()` → `RepoFacts` → `plan()` (`Ru/plan.rs:41-118`) → `WorkflowPlan` (`M/plan.rs:22`)
→ `rendered_job` (`Re/steps.rs:53`) → rules → sealed YAML (`Re/header.rs:22-25`). At run time the
released helper `velnor-ci` re-derives the same facts and decides (`Ci/io/plan.rs:38`), and the
`Required` job calls `decide` (`Ru/verdict.rs:278`).

| Place | What it is | Examples today |
|---|---|---|
| Generator time | config key (`Ru/config_keys.rs`, 34 keys), `velnor-model` type, `StepKind` (`M/steps.rs:29`), planner in `velnor-rust`, renderer | `pr_ci`, `PrRun` (`M/plan.rs:120`), `VerdictSpec` (`M/verdict.rs`) |
| Run time | a `velnor-ci` subcommand (`Ci/cli.rs`), the gate | `plan`, `gate`, `merge-group-passed`, `run`, `report`, `release-gate` |
| Runner side | `crates/velnor-runner-*`, node services | scale-set client, pair launcher, crates mirror, MBX node store |
| GitHub settings | ruleset, labels, App | `protect-main` 24396608, `ci:run`, `ci:auto` |

### 2.2 Component map

| Component | Where it lives now | Types and functions to extend | What velnor-gen gives for free |
|---|---|---|---|
| **Verdict keys** | Generator time (recipe) and run time (value) | New `M/obligation.rs`: `Obligation { id, kind, scope, reuse, due }` and `VerdictKey`. `Step` gets `obligation: Option<ObligationId>` and loses `skip_after_merge_group` (`M/plan.rs:110`). The planner tags the steps `is_check` already lists (`Ru/merge_group_skip.rs:9-29`). `velnor-ci plan` writes the key vector as outputs (`Ci/plan.rs:19`) | `compat` (`Ru/compat.rs:24`), `lock_key` (`Ru/lock_key.rs:13`), helper binary digest (R:81), sealed workflow digest (R:2), recorded metadata `.velnor/rust-metadata.json`, compile-time read map `.velnor/rust-inputs.lock` (39 rows), delta source identity (`Ci/io/delta.rs`, `Ci/io/input_snapshot.rs`) |
| **Verification ledger** | Run time first, a service later | `Ci`: new `ledger lookup` and `ledger put`; `velnor-ci run --obligation <id>` writes one record per check (`Ci/io/run.rs:77-87`); `velnor-ci report` already receives every step outcome through `VELNOR_REPORT_STEPS` (`Ci/io/report.rs:77`, R:391). Store trait with one backend at a time (2.3) | The trust rule (`trusted_event`, `Ci/mbx_store.rs:32-39`), the "prove or run everything" lookup pattern (`Ci/merge_group.rs:45-86`), GitHub's cache scopes as an executable rule set (`crates/velnor-cache-rules/src/lookup.rs`), the nightly full run as an audit (R:17-18) |
| **Gate (`decide`)** | Run time | `Ru/verdict.rs:278` judges per job today (`judge`, :172). It must judge per obligation: pass in this run, or a trusted record with the same key. `VerdictSpec` (`M/verdict.rs`) gains the obligation list; `tests_expected` (`Ru/required_job.rs:152`) and the per-package test counts (`Ru/verdict.rs:207-276`) stay | "Green means verified" logic, test-count checks, the released-helper rule (goals rule 12) |
| **`Velnor / Verified` App status** | GitHub settings plus a new service crate `crates/velnor-control` | `required_check` config key names the context. `Ci/gate.rs:292-298` refuses a required check bound to an App other than Actions; `require()` must learn the Velnor App id. Verifier logic is `decide`, linked as a library | The verdict logic and its tests. A no-checkout, no-repository-code job pattern (`rust-cache-trim.yml:1-33`, `docs/implemented/job-trust-boundary.md`) for a first trusted verifier (item W10) |
| **Label flow** | Generator time | `PullRequestType` gains `Unlabeled` (`M/plan.rs:41-49`); `triggers()` (`Ru/plan_budget.rs:49-66`); `PrRun` (`M/plan.rs:120-127`) and `pr_parts` (`Re/gate.rs:253-271`); `verdict_job` (`Ru/required_job.rs:106-150`); `draft_guard` (`Ru/plan_budget.rs:68-76`). Delete `ci:run`, `ClearRunLabel`, `PrRun::Quick`, `PrRun::Unrequested`, job `ci-run-label` (R:613-625) | `labeled` trigger, per-PR cancel group (R:23-24), `FailOnDraftRun` (`M/steps.rs:111`) as the model for a "CI not requested" failure |
| **Reuse: agent → PR → queue → main → release** | Run time (lookup) and generator time (which steps may skip) | Generalise `Ru/merge_group_skip.rs:33-58` from "one question per run" to "one lookup per obligation". Then delete `Ci/merge_group.rs`, `HelperCommand::MergeGroupPassed` (`M/steps.rs:166`) and `[ci] skip_push_after_merge_group`: they become one case of the ledger | The only shipped prior-run lookup, already wired into step conditions and into the gate |
| **Release promotion by digest** | Generator time and run time | `CargoCommand::BuildBinary` (`M/plan.rs:270`) as a `due = release` obligation on trusted runs; `Re/release_binaries.rs:257,293` build jobs become a promote job; `VERIFIED_COMMIT` and `FROM_A_PASSED_PUSH` (`Re/release.rs:34-38`) already tie a release to a passed push; `velnor-ci release-gate` (`Ci/gate.rs:57-135`) | The generated `release.yml` already publishes from a job that checks out nothing (goals rule 13). This repository does not use it yet: no `.velnor/release.toml`, and `release-rust-ci-tools.yml:95-105` checks ancestry only |
| **`velnor verify`** | New binary; shares the planner | `velnor-ci plan --changed <paths>` exists (`Ci/cli_args.rs:70`). Needed: a second renderer, plan → execution manifest (2.4), and a runner for it | `select()` is a pure function (`Ru/select.rs:17`), `select_event` takes the diff as a closure (`Ru/select_event.rs:21-34`), plans never run cargo |
| **`velnor cargo`, `velnord`** | New; runner side for the daemon | Reuse `velnor-ci prepare` (CI profile, R:138-141), `mbx-seed` (R:85-87), `crates-mirror` (R:91-93) to give a developer machine the same environment as a job | Same profile env as CI: `CARGO_INCREMENTAL=0`, `SOURCE_DATE_EPOCH=0`, `line-tables-only` (`docs/rust-ci-cache-keys.md`) |
| **Velnor Native vs GitHub Official** | Runner side; fed by the generator | 2.4. Config: one key `executor = "native" | "official"` per job kind, parsed in `Ru/config.rs`, and part of every `VerdictKey` | The plan is already an executor-neutral description: 49 `StepKind` variants, `When` (24 variants, `M/plan.rs:233-259`), `PrRun`, `Gate` |
| **Platform trait** | Runner side only | `crates/velnor-runner-host` (bollard direct, no trait). Adopt the `Runtime` trait of `docs/proposed/kubernetes-runtime.md`; see 5.2 for the conflict with the merged architecture | Nothing. The generator only supplies a runner label (`runner`, `.velnor/rust-ci.toml:4`) and reads node offers from env |
| **Storage** | Runner side; later | New crate when the ledger service and the node stores need a shared blob store. Not on the first ten items | The 8 GB budget manager (`Ru/cache_budget.rs`, `velnor-ci cache budget`, R:566-569) stays for hosted consumers |
| **`actions/cache` server** | Not needed for generated workflows | The generator owns every cache step (`StepKind::CacheRestore/CacheSave`, `M/steps.rs:39-40`). On a Velnor node it renders a helper step against the node store instead of `actions/cache`. The precedent is `tools-local` (R:94-99, 124-128): node store first, network cache on a miss. A protocol-compatible server matters only for hand-written workflows of other repositories | `velnor-cache-rules` and the velnor-local cache server as the executable spec if the server is ever built |
| **Git and crate mirrors** | Runner side plus one helper step each | Crates: shipped (`[crates_mirror]`, `.velnor/rust-ci.toml:11-14`, R:91-93). Git: copy the same pattern: config table, `StepKind`, helper command, helper-version gate (`G/helper_gate.rs`); the generator renders `Checkout` (`M/steps.rs:30-33`) as a mirror-backed fetch on nodes that offer one | The whole `crates_mirror` path as a template |
| **Warm snapshots** | Runner side (clone), generator time (name) | The snapshot name is the Target key the generator already renders: `velnor-tgt-…-<compat>-<lock_root>` (R:168). New `velnor-ci target-local restore/save` beside `tools-local`. "Build the tests" (R:268-270) already exists only to warm the base | Key, freshness rule (`inputs restore`, R:191-193), expiry (`VELNOR_BASE_MAX_AGE_DAYS`, R:198), delta layer (R:172-190) |
| **mbx** | Run time, shipped | `mbx_shared_store = true` (`.velnor/rust-ci.toml:6`), `mbx-store` and `mbx-seed` (R:83-87). `mbx-seed` copies files and costs 8 s per PR job [M]; make it a link or an overlay | Trusted-writer rule, seed directory, private shims |

### 2.3 The ledger, concretely

**Obligation.** One check the verdict depends on. Kinds are the existing check steps: Format, Lint,
Lint with features, Documentation, Check-alone per package, Tests, Tests with features, Lane guard,
Doctests, Deny, Machete, each `[[lint]]` job, the generator check, and a release build. The planner
already holds all of them as `StepKind::Cargo(CargoCommand::…)` (`M/plan.rs:262-320`) and `StepKind::Lint`.

**Key.** A struct, digested over a canonical encoding, with named fields so a miss can name the field:

| Field | Source today |
|---|---|
| key version, obligation kind, argv, features, profile | the rendered command (`Re/cargo.rs`) |
| toolchain, target, profile env, image, flags | `compat` (`Ru/compat.rs:24`) |
| dependency lock | `lock_root` (`Ru/lock_key.rs:13`) |
| helper and generator | helper binary SHA-256 (R:81), workflow digest (R:2) |
| nextest configuration | digest of `.config/nextest.toml` (profile `ci`, wrappers, groups) |
| executor identity | `native` or `official` plus version (`.velnor/runner.lock:17`) |
| inputs | scope-dependent, next paragraph |

**Scope.** `WholeTree` is `git rev-parse HEAD^{tree}`: one Git call, no history, so no base commit and
no `fetch-depth` problem. `Packages(closure)` is the Git tree ids of the package directories in the
reverse-dependency closure `select()` already computes (`Ru/select.rs:73-86`), plus every path the
classifier cannot place (`Class::Unknown`, `Ru/classify.rs:57`), plus the mapped rows of
`rust-inputs.lock`, plus the feature-resolution digest of that closure from `rust-metadata.json`.
The last item answers hard decision 11: the build stays `--workspace`, and the key records the
unified features the package was built with (`docs/hard-decisions.md:486`).

**Which scope may an obligation use.** Compile-class obligations (Lint, Check-alone, Documentation)
read exactly what rustc's dep-info lists, and `velnor-ci inputs audit` checks that map on every
scheduled run (R:300-302). They may use the package scope once the shadow comparison is clean.
Test obligations read files at run time that nobody declares, so they stay `WholeTree` until a
read audit enrols them. That rule is unchanged from the merged design (architecture §6.1).

**Records and trust.** A record is `{key, outcome, counts, log digest, run id, event, scope}`. Writers:
a `push` or `schedule` run on the default branch writes trusted records; a pull request writes
candidate records that only that pull request reads. This is the rule `mbx-store` already applies.

**Backends, one at a time.**

1. *Shadow (W08).* Records go into `ci-metrics.json` and its artifact (R:395-404). Nothing reads them
   for a verdict. A weekly report lists every case where a key matched and the outcomes differed.
2. *Ledger v0 (W09).* Records are GitHub cache entries with an immutable key `velnor-vrd-<key>`,
   looked up with `actions/cache/restore` and `lookup-only: true`. GitHub's own cache scopes then
   enforce the trust rule: a pull request can read the default branch's entries and its own, and can
   write only its own (`crates/velnor-cache-rules/src/lookup.rs`, `local/conformance/cache-*.json`).
   An evicted record is a miss, and a miss runs the check. The gate job repeats the lookup itself,
   so the evidence does not come from the job that ran repository code. The queue-to-push case keeps
   the API lookup by head SHA, because a queue ref's cache scope is not readable from `main`.
3. *Ledger service.* `velnor-control` holds the records and writes the status. When it lands,
   backend 2 is deleted in the same change (owner decision 18).

### 2.4 Two executors from one plan

The merged design asked for a Rust executor that speaks the Actions protocol and re-implements a
closed set of actions. On a generator-first foundation there is a shorter and safer route.

- **GitHub Official** executes the rendered YAML. It stays the reference executor, and it is what
  every GitHub-hosted consumer uses.
- **Velnor Native** executes the plan directly. A new renderer turns `WorkflowPlan` into an execution
  manifest: an ordered list of `{obligation, argv, env, working directory, condition}` per job.
  Native steps are native implementations of `StepKind` (check out, restore the node target, install
  tools from the node store, run, record), not emulations of `actions/checkout` or `actions/cache`.
- **Conditions are checked two ways.** The same `When`, `PrRun` and `Gate` values are rendered to
  `if:` text for YAML (`Re/gate.rs:253-275`) and evaluated natively for the manifest. A property
  test evaluates the rendered expression with a small evaluator over generated event payloads and
  compares it with the native result. This replaces the evaluator question D4 for velnor-new.
- **Conformance is generator-owned.** For one tree and one plan, the records from the YAML run and
  from the manifest run must be equal, obligation by obligation. `qualification.yml` is already
  generated from `.velnor/workflows.toml` with 24 cases that pair a hosted and a scale-set job; a
  third leg, `native`, joins each pair.
- **What still needs the Actions protocol.** Hand-written workflows and other repositories on the
  cluster (the ChainArgos and jackin scale sets, B§0.2). GitHub Official serves them. A
  protocol-speaking native executor becomes an optional later stream, decided by the owner.

---

## 3. Correctness first

Ranked by how directly the gap lets a broken tree reach `main` or a release.

| # | Gap | Evidence | Fix | Where | Owner needed |
|---|---|---|---|---|---|
| 1 | Merges bypass the gate. 157–167 of 173 merges had a failing `Required`. The ruleset has an admin bypass and no `integration_id` | [R: C§1, C§4; E§0.3] | Empty `bypass_actors`; bind `Required` to integration 15368. Only after gap 2 is fixed, or every PR deadlocks | Ruleset 24396608 | **Yes, applies it** |
| 2 | Main is red and tests do not run. `Test` fails in 12–24 s on the `--no-fail-fast` / `--no-run` conflict. `velnor-ci`'s test binary `it` does not compile (`crates/velnor-ci/tests/it/prepare_inputs.rs:9`, `prepare.rs:69`), so Lint fails too. `Required` then says "the plan job rust did not succeed" | Runs 44576523025, 44578162215, 44584072510, 44584861409 [M]; `Ci/io/audit/build/invocation.rs:79-87`, `Re/cargo.rs:295` [C] | Drop `--no-fail-fast` from the preflight argv (or stop inserting `--no-run` into a run command and use the `Build the tests` form, R:270). Fix the two compile errors. Release the helper and pin it. Cause of the flag conflict [E]: the nextest bump to 0.9.152 (E§4 row §6.2); confirm before fixing | `Ci/io/audit/build/invocation.rs`, `crates/velnor-ci/tests/it/`, `mise.toml` | No |
| 3 | `Required` is green without tests. (a) A ready PR without a label: the gate step needs the label (R:605) and the fallback fails only when a needed job failed (R:610-612). (b) After `ci:run` is used, the label is removed (R:613-625) and the next push is green on the quick set. (c) A draft is green with a notice (R:582-584) | [C]; `docs/rust-ci.md:78-90`; E§1 | The owner's flow in the generator: no label means `Required` fails with "CI not requested". One label, no one-shot label, an `unlabeled` trigger | `M/plan.rs:41,120`, `Ru/plan_budget.rs:49-76`, `Ru/required_job.rs:106`, `Re/gate.rs:253` | **Yes, confirms the flow (Q-N2)** |
| 4 | A skipped `Required` passes. The job is skipped for an unrelated label and for an edit that does not change the base (R:576). A later skipped check on the same commit can hide an earlier failure | R:576 [C]; run 44577473873 [M]; GitHub's rule that a job skipped by `if:` reports success [E, to confirm in the lab] | `Required` is never skipped. On an ignored event it runs and repeats the last real verdict for that commit, or fails when there is none. A trigger cannot filter on the label name or on what an edit changed, so the job has to run | `Ru/required_job.rs:138-143` (`PrRun::Related`), new `velnor-ci gate --carry` | No |
| 5 | HEADGREEN with diff selection is unsound. Queue entry B is selected by `base_sha..head_sha` (`Ci/io/git.rs:66-79`, `on_main = affected`), where the base is entry A. If A fails and B passes, HEADGREEN merges both | Ruleset: HEADGREEN [R: C§4]; `Ru/select_event.rs:11-19` [C] | ALLGREEN. Until then, `on_main = "everything"` for `merge_group` | Ruleset; `.velnor/rust-ci.toml` | **Yes, applies it** |
| 6 | A pull request controls its own gate. `pull_request` and `merge_group` runs use the workflow file of the change. The seal is an unkeyed SHA-256 (`Re/header.rs:22-45`). No workflow runs `velnor-gen check`: `GeneratorCheck` is rendered (`Re/steps.rs:279`) but no planner builds it | [C] grep: only `M/steps.rs:108`, `Re/steps.rs:279`, one test | Wire `GeneratorCheck` as an obligation (cheap, catches drift). Then verify from trusted code: generation is pure and reads only files, so the released `velnor-gen check` can run on a PR tree in a job defined on the default branch (W10) | `Ru/lint_jobs.rs:9`, new planner call; W10 | No |
| 7 | Green on a diff is not a green tree. A push run tests only `before..after`. After a red push, the next push can be green while the tree is still broken. Only the nightly run checks the whole tree | `Ru/select_event.rs:11-19`, R:17-18 [C] | Interim: a push whose predecessor's push run did not pass selects everything (one API lookup, same shape as `Ci/merge_group.rs`). Final: key-based verdicts (2.3), where an unproven obligation always runs | `Ci/io/plan.rs:38-90` | No |
| 8 | The release gate checks ancestry only, not that `Required` passed (goals rule 13) | `.github/workflows/release-rust-ci-tools.yml:95-105` [C] | Merge #1608. Then generate this repository's `release.yml` (#733), whose gate is `velnor-ci release-gate` (`Ci/gate.rs:57`) | #1608, `.velnor/release.toml` | **Yes for #733: ruleset #732** |
| 9 | The four lint jobs block nothing. `ci.yml` jobs (Alint with the 400-line cap, Jactionlint, Cargo Deny, Cargo Machete) are not in the ruleset | `ci.yml:41-157` [C]; [R: C§4] | Move Alint and Jactionlint to `[[lint]]` jobs (`M/lint.rs:13-27`, `Ru/lint_jobs.rs:9`), which join `Required` automatically. Delete the duplicate Deny and Machete (the `policy` job has them, R:480-485). Delete `ci.yml` | `.velnor/rust-ci.toml`, `ci.yml` | No |
| 10 | Deferred checks have no consequence. `docs = "post_merge"` and `check_alone = "post_merge"` (`.velnor/rust-ci.toml:7-8`) move Documentation and 18 alone checks to the push run. A PR can break them and merge green | R:211-267 conditions [C] | Make "deferred" explicit: `due = after_merge`. A failed deferred obligation closes the queue until it passes. With package keys they are cheap enough to return to the queue | `M/obligation.rs`, `Ru/docs_post_merge.rs:6` | No |
| 11 | A tool bump can break the helper unseen. `mise.toml` and `mise.lock` select everything (`Ru/classify.rs:23,70`), but with `pr_ci = "label"` and bypass merges nobody ran the tests | Gap 2 | The lab's nightly run of the generator against pinned tools (#623). A helper-versus-tool contract test in `velnor-ci` | #623 | **Yes: #623 is `needs-owner`** |
| 12 | The queue is configured and unused; `allow_auto_merge` is false | [R: C§1, C§4] | Turn auto-merge on; merge only through the queue | Repository settings | **Yes** |
| 13 | A red main saves no cache and produces no timings. Every save step needs `success()` (R:304, 308, 315). `velnor-ci timings` needs three passing runs on the default branch. So the shape stays `one-job` (R:32) and PRs start colder | [C] | Follows from gap 2 | — | No |
| 14 | `fetch-depth: 2` (`Ru/plan.rs:37`) with the ancestor check (`Ci/io/git.rs:73`) turns a multi-commit push or a multi-PR group into "select everything". Correct, but slow | [C]; [R: A§2.3] | Diff two trees fetched by SHA; no ancestry needed. Disappears with whole-tree and package keys | `Ci/io/git.rs` | No |
| 15 | Unknown paths select everything: `ci.yml`, `.velnor/workflows.toml`, `generator/**`, `local/**`, `scripts/**`, `images/**`, `qualification/**`, `release-plz.toml`. Correct, but slow, and `.velnor/rust-ci.toml` maps none of them | `Ru/classify.rs:24-27, 57` [C] | Move the path rules from `.velnor/config.toml [stacks.rust]` into `[[inputs]]` and `non_inputs` of `rust-ci.toml`, then delete `config.toml` and the `plan --mode` port (E's N14) | `.velnor/rust-ci.toml` | No |
| 16 | The `Cache` check is advisory and often red (17 of 60 sampled failures). A check that is red and ignored trains people to ignore red | [R: C§2] | Fix the waste it reports or fix the check; then set `cache_check_blocks = true` | `Ci/io/report.rs` | No |
| 17 | `docs/goals.md:11-12` still describes `generator/`, `ci-helper/` and the engine | `docs/goals.md:11-12` [C] | One owner-approved edit, together with the executor wording (section 7.3) | `docs/goals.md` | **Yes** |

Order of work: 2, then 3 and 4 in one generator change, then 1, 5 and 12 in one ruleset step by the
owner, then 9 and 6, then 8. Gaps 7 and 10 close with the ledger. Gaps 14 and 15 are speed items.

---

## 4. Speed on the new baseline

### 4.1 Where the time goes now

All jobs ran on `ubuntu-26.04-scale-set`; queue 1–12 s [M]. Steps under 3 s are summed as "other".

| Step group | Push, everything selected (run 44584072510) | Push, 4 packages (run 44584861409) | PR quick set (run 44583969919) |
|---|---|---|---|
| Set up, check out, helper, plan, restores | ≈ 30 s | ≈ 30 s | ≈ 35 s (mbx-seed 8, set up 6, sources 6, check out 3, plan 3) |
| Format | 3 | 3 | 3 |
| Lint | 18 (failed early) | 31 (failed) | 19 (failed) |
| Lint, features `local-mode` | 28 (failed) | 26 (failed) | 28 (failed) |
| Documentation, both variants | 42 + 46 = 88 | 31 + 46 = 77 | not run (post-merge) |
| Check `<crate>` alone | 18 steps, **272** | 4 steps, 52 | not run (post-merge) |
| Test, both variants | 15 + 14 (died before any test) | 12 + 12 (died) | not requested |
| Test lane guard | **96** | 62 | not requested |
| `rust` job total | **564 s** | 305 s | 90 s |
| `Required` / `Cache` / `Cache maintenance` | 12 / 11 / 10 | 12 / 13 / 11 | 9 / 12 / — |
| Run wall | 10.1 min | 5.6 min | 1.8 min |

Readings:

- On a full push, 48% of the job is the 18 alone checks, 17% is the lane guard and 16% is
  documentation. None of the three gates a merge; all three delay the cache save at the end of the job.
- Deleting the engine already halved the push run: runs before #1747 took 14.5–19.1 min wall and
  944–1,052 s in `rust` (runs 44576523025, 44578162215) against 5.6–10.1 min now [M]. The runs are
  not like for like: their selections differ.
- **There is no green sample of a labelled PR run on the new shape.** In C's window only 4 PRs
  carried a label and only 2 of 268 PR runs passed, both without Rust work [R: C§1, C§2]. The test time on this
  shape is unknown. Every number below that includes tests is an estimate until main is green.
- C's medians still describe the period: PR run P50 1.1 / P90 5.8 min, heavy PR runs 2.9 / 6.2,
  heavy push runs 7.5 / 11.3 [R: C§2]. Most of those runs stopped early on a failure.

### 4.2 The next five levers, by minutes saved per merge

A merge under the owner's flow costs three runs: label run L, queue run Q, push run P. L and Q block
the merge; P does not, but it delays the next warm base. Estimates [E]: L ≈ 3–6 min green, Q ≈ L,
P ≈ L plus 6–7 min of deferred checks.

| # | Lever | Saves per merge [E] | Why | How to measure |
|---|---|---|---|---|
| 1 | A green main | 1–6 blocking min; enables levers 2–4 | The Target cache is saved again (R:303-326), so L and Q restore a warm base. Three passing runs produce `.velnor/rust-ci-timings.json` | `ci-metrics.json`: crates compiled and cache `hit exact/fallback/miss` per run, before and after; `velnor-ci bench compare` |
| 2 | Verdict reuse along label → queue → push | Q: 3–6 blocking min times the hit rate. P: 8–10 runner-min on every merge | Queue → push is the same commit, so `skip_push_after_merge_group` already proves it; it is one config line today (`generator/CONFIG.md:26-40`). Label → queue hits only when the group tree equals the tree the PR run tested. With whole-tree keys and a busy main that is rare; with package keys it is the common case | Shadow records: share of obligations whose key in Q equals a passing key from L; count of key-equal pairs with different outcomes (must be 0) |
| 3 | A parallel shape from measured timings | 2–5 blocking min | Today every check runs in sequence in one job. `decide_shape` splits lint and test when the shorter build exceeds 5 s (`Ru/shape.rs:10, 47-55`). The deferred checks move to their own job, so the cache save no longer waits 6 min for them | Jobs API step times (the query I used) on ten runs before and after; `[budget] pr_wall_seconds` |
| 4 | Package keys and path mapping | 4–6 runner-min per full push; fewer full selections | 18 alone checks become "only those whose closure changed". Mapping the unknown paths (gap 15) removes most "select everything" runs. The lane guard is keyed on `velnor-runner-host`'s closure | Share of runs with `rust_root_mode = all` and the trigger named by `plan --explain`; seconds in alone checks per push |
| 5 | The fixed floor and the serial tail | 1–2 blocking min | Job start to first cargo is about 35 s against a 30 s target (`docs/goals.md:57`); `mbx-seed` alone is 8 s. `policy` waits for `rust` and installs tools again (R:406-479). `Required` adds a job (9–12 s) after the last check | `first_cargo_seconds` in `ci-metrics.json`; `Required` created-to-completed from the jobs API |

Not a lever any more: hosted-runner queueing (gone since #1523), and replacing ARC for queue time
(P50 2–13 s). Not yet a lever: anything measured on tests, until they run.

### 4.3 Targets, re-derived

| Stage | Label to verdict P50 / P90 | Push to merge P50 / P90 | Runner-min per merged PR | Basis |
|---|---|---|---|---|
| Measured today | no sample (tests do not run); quick set 1.8 min | 0.6 / 3.8 min, by bypass, so not a CI number | 6.4 + 5.6 run-wall min, mostly failed runs | [M], [R: C§1, C§5] |
| A. Green main, owner's flow, one job, no reuse | ≈ 4 / 8 min | ≈ 9 / 18 min | ≈ 25 | [E]: L + Q + queue overhead |
| B. Reuse queue → push, parallel shape, mapped paths | ≈ 2.5 / 5 min | ≈ 5 / 10 min | ≤ 15 | [E] |
| Goal: package keys, node target clone, verdict without a job | ≤ 90 s / ≤ 4 min | ≤ 3 / ≤ 8 min | ≤ 8 | [T], same end values as the merged plan |
| Stretch | 30 s / 100 s | 100 s / 4 min | ≤ 5 | [T] |

The merged plan's "Today 26.7 / 92.3 min" row and its interim row (≈ 6 / 18 min) describe a CI that
no longer exists. The old goal of 50 runner-minutes per merged PR is already met. Stage A must be
measured before B is promised: re-baseline first (W07).

---

## 5. What to keep, change, drop

### 5.1 E's classification

I agree with E's counts as a starting point (VALID 86, CHANGED 50, CONFLICT 20, DONE 10, OBSOLETE 8)
and with its 9 MARK-DONE and 7 DROP items. I disagree on five groups:

| Group | E says | I say | Reason |
|---|---|---|---|
| X02–X04, X06–X12, X14 (protocol, expressions, templates, commands, composites) | GATE on the owner | **Drop from velnor-new's path.** Replace with a plan-manifest executor (2.4). Keep as an optional stream for hand-written workflows | Every blocking velnor-new check is generated. The protocol work bought nothing the manifest does not |
| A01–A05, A08 (native actions) | KEEP, recount | **Change** to native `StepKind` implementations and node-store renderings | The generator owns the steps; emulating action inputs is the long way round |
| ST03, ST05 (cache index, cache endpoint for Official) | CONFLICT with goals | **Defer.** No conflict remains: hosted consumers keep GitHub's cache, generated workflows on Velnor nodes use node stores through helper steps | 2.2, `actions/cache` row |
| V04 (exact-tree records) | GATE on hard decision 3 | **Keep; start in shadow now.** Ask the owner to confirm before the first enforced reuse | Hard decisions bind the engine only (`docs/hard-decisions.md:14`), the engine is deleted, and `skip_push_after_merge_group` already ships a proven skip under goals. Shadow mode changes no verdict |
| F01, P07 (delete runner crates, delete velnor-local pair mode) | GATE | **Drop the deletion.** `velnor-runner-*` becomes the host of the GitHub Official executor; velnor-local keeps pair mode as the reference harness | The Rust scale-set client exists (B§1.3); 55 hardening commits landed since the plan; goals rule 11 needs it |

### 5.2 Streams

| Stream | Survives? | First item now |
|---|---|---|
| B (phase 0) | Yes, rewritten | W01 green main, then W03 label flow, then W04 ruleset |
| RV (reviews) | Partly. RV03 and RV04 are done by reports A and B. RV01, RV02, RV05 stay | One decision: adopt the `Runtime` trait of `kubernetes-runtime.md` and keep the journal, or the architecture's §5.10 deletion. I recommend the trait and the journal |
| G (generator) | Yes, becomes the main stream | W08 obligation model |
| V (ledger) | Yes, moved into `velnor-model` / `velnor-rust` / `velnor-ci` | W08 shadow keys, W09 ledger v0. V08 "per-root keys" is replaced by package keys |
| L (label flow, App) | Yes | W03 in the generator; W10 trusted verifier; then the App (D7) |
| R (release) | Yes | #1608, then #733, then promotion by digest. R01 waits for Q-N5 |
| DG (Dagger-derived) | Yes; DG01 and DG08 move into phase 1 | DG08 = wire `GeneratorCheck` (W05); DG01 = `VerdictKey` (W08) |
| BM (benchmark) | Yes, re-based | W07: baseline with `velnor-ci bench`, `timings`, `[budget]` |
| AN (agent-native) | Yes, later and smaller | Manifest renderer, then `velnor verify` on it |
| C (caches, mirrors) | Yes; C01, C02, C05 are extensions of shipped code | Node target clone next to `tools-local`; `mbx-seed` without a copy |
| H (conformance) | Yes, re-seeded | A `native` leg in `.velnor/workflows.toml` cases |
| O (Official executor) | Yes | It is today's production path. No new work until the platform decision |
| P (platform) | Yes, off the critical path | The trait decision above; Kubernetes backend only when ARC must go |
| X (protocol executor) | No, for velnor-new | — |
| A (native actions) | Changed, see 5.1 | — |
| ST (storage) | Deferred | With the ledger service |
| T (tests) | Mostly done or obsolete (engine suites gone) | Measure the test floor on a green main first |
| S (cluster) | Unchanged, out of repository | Decide where manifests live (E's N11) |
| F (final deletion) | Dropped as written | Replaced by per-item deletions: `ci.yml`, `.velnor/config.toml`, `ci:run`, `merge_group.rs` |

### 5.3 What E missed

1. The `Test` step is broken at the tip (gap 2). E's N01 lists causes from C's sample, not this one.
2. A skipped `Required` passes (gap 4).
3. HEADGREEN is unsound with `on_main = affected` (gap 5). The merged plan chose ALLGREEN for another reason.
4. A pull request controls its own workflow file; a pure generator makes a trusted re-check possible (gap 6).
5. Green on a diff is not a green tree (gap 7).
6. `rust-ci.toml` has no `[[inputs]]`; the path rules are stranded in `config.toml` (gap 15).
7. A red main also means no cache save and no timings, so the shape cannot change (gap 13).
8. ARC's queue is no longer a bottleneck, so the dispatcher leaves the critical path (4.2).
9. #1320 is closed, not open. `mbx-store` does not trust `merge_group`. There are 18 alone steps.
10. Owner's "nothing runs without the label" is reachable exactly only with an App status. With an
    Actions job as the gate, each push still needs one short job that fails `Required` (section 6, W03).

---

## 6. Phase 0 and the first ten work items

Models use the repository's own labels (`exec:opus`, `exec:sonnet`; `effort:low|medium|high`). Sizes
are agent-hours of implementation, without CI wait. Every item ends with the old path deleted.

**Open pull requests.** #1608: rebase and merge inside W06. #1200: rebase and finish first; it is
two commits behind, already on the `crates/` layout, and its failures are main's own (gap 2). #1026:
replay as two pull requests on the new layout (helper input handling, then generator rendering);
close the original. #1297: close; let Renovate recreate it on the single workspace after W01, and
run it with the label because tool and dependency bumps are what broke gap 2. #1751: this round's
write-up, a draft; merge only after the owner answers section 7. #1320: closed; its issue #1073
(the scheduled inputs audit builds every workspace again) needs a fresh, small change now that
there is one root. #1274 and #1335 are merged: `[[workspace_groups]]` and the lane guard are part
of the baseline, and the lane guard is now 17% of a full push job (4.1).

**Phase 0 = W01 to W06.** Nothing in phase 0 needs a design decision except Q-N2 and the ruleset step.

| # | Item | Goal | Files | Done when | Size | Implement | Review |
|---|---|---|---|---|---|---|---|
| W01 | Green main, tests running | `Required` passes on `main` three times in a row and tests execute | `Ci/io/audit/build/invocation.rs:79-87` (preflight argv), `crates/velnor-ci/tests/it/prepare_inputs.rs`, `prepare.rs`, `deny.toml` (#1218), `.alint.yml` findings, then tools release and `velnor-gen pin-helper` | Three consecutive push runs green with `tests_run > 0`; a Target cache entry saved; `velnor-ci timings` writes `.velnor/rust-ci-timings.json`; a regression test proves the preflight argv is accepted by the pinned nextest | 8 h | opus / high | opus / high |
| W02 | Owner pack v2 | The owner can answer section 7 in one sitting | An issue with the questions, defaults and the exact `goals.md` text | Each question has a recorded answer or a recorded default | 2 h | opus / medium | sonnet / medium |
| W03 | The owner's label flow in the generator | Draft, ready and unlabelled pushes run no checks and `Required` fails with "CI not requested". One label starts CI, every push while labelled runs and cancels the previous run, removing the label cancels and blocks. `Required` is never skipped (gaps 3 and 4) | `M/plan.rs:41-49,116-127`; `Ru/plan_budget.rs:49-76`; `Ru/required_job.rs:106-150`; `Ru/plan.rs:330-343`; `Re/gate.rs:241-271`; `M/steps.rs:111-113`; `generator/CONFIG.md:537-556`; `docs/rust-ci.md:78-90`; goldens under `generator/samples/`; helper gate in `G/helper_gate.rs` | Lab scenario in `tailrocks/velnor-lab` and a velnor-local scenario show: unlabelled head red; labelled head runs; unrelated label and title edit keep the last verdict; label removal cancels the run and turns the head red; `ci:run`, `ci-run-label`, `PrRun::Quick`, `PrRun::Unrequested`, `ClearRunLabel` are gone | 12 h | opus / high | opus / high |
| W04 | Ruleset as generated data, applied by the owner | `Required` bound to integration 15368, no bypass actor, ALLGREEN, auto-merge on; drift is detected | New `velnor-gen ruleset branch` beside `release ruleset` and `ruleset-drift` (`G/cli.rs:65-69`); a drift step like `release-rust-ci-tools.yml:16-85` | The owner applies the printed JSON after W01 and W03 are pinned; a nightly check turns red when a bypass actor or an unbound context appears; one PR merges through the queue with no admin action | 6 h | sonnet / high | opus / medium |
| W05 | One verdict for everything | No blocking check lives outside `Required`; the generator checks itself; one config file | `.velnor/rust-ci.toml` (`[[lint]]` for Alint and Jactionlint, `[[inputs]]` and `non_inputs` from `.velnor/config.toml`); planner call for `GeneratorCheck` (`M/steps.rs:108`, `Re/helper_steps.rs:74`); delete `.github/workflows/ci.yml`, `.velnor/config.toml`, `Ci/plan_request*` and `--mode`; `G/drifts.rs:55-75` loses `engine_configured` | `Required` fails when Alint, Jactionlint or `velnor-gen check` fails; a docs-only change under `generator/` selects nothing; `ci.yml` and `config.toml` do not exist | 12 h | sonnet / high | opus / medium |
| W06 | Release gate | A tools release needs a passed `Required` on the tagged commit; then releases promote | #1608 rebased; `.velnor/release.toml` and generated `release.yml` (#733, after the owner does #732); delete `release-rust-ci-tools.yml` | A tag on a commit whose push run failed publishes nothing; the publish job checks out nothing | 6 h + 12 h for #733 | sonnet / medium | opus / medium |
| W07 | Baseline and shape | Honest numbers on a green main, and the shape the timings choose | `.velnor/rust-ci-timings.json`; `[budget]` in `rust-ci.toml`; `velnor-ci bench collect/compare`; `skip_push_after_merge_group = true` as the first measured reuse | A committed table: label-to-verdict, push-to-merge, runner-min per merge, first-cargo seconds, over at least 30 merges; the generated workflow states its shape and reason (R:32) | 6 h | sonnet / medium | sonnet / medium |
| W08 | Obligations and keys, in shadow | Every check has an id, a key recipe and a record; no verdict changes | New `M/obligation.rs`; tagging in `Ru/lane.rs`, `Ru/root_jobs.rs`; key outputs in `Ci/plan.rs:19`; `velnor-ci run --obligation` (`Ci/io/run.rs:77`); records in `Ci/io/report.rs`; `plan --explain` names the key fields | For 200 runs, zero pairs with equal keys and different outcomes that are not explained as a flake; the nightly full run executes every obligation and agrees with the records | 18 h | opus / high | opus / high |
| W09 | Ledger v0 and per-obligation gate | A proven obligation is not run again; the gate accepts reuse only with evidence | `Ci`: `ledger lookup/put`; `Ru/verdict.rs:172-318` judges obligations; `Ru/merge_group_skip.rs` generalised; delete `Ci/merge_group.rs`, `HelperCommand::MergeGroupPassed`, `skip_after_merge_group`, `[ci] skip_push_after_merge_group`; scope tests in `crates/velnor-cache-rules/tests/it/` and a velnor-local `story` scenario | Planted-failure test: a broken obligation is never green through a record; a PR cannot write a record another ref reads; an evicted record causes a run; push after queue runs zero checks | 16 h | opus / high | opus / high |
| W10 | Trusted verifier and the status | The verdict for a commit is computed by code the change cannot edit, and written once per commit | A generated `workflow_run` workflow like `rust-cache-trim.yml`: no checkout of the change as code, released `velnor-gen check` on the tree as data, `decide` over the run's jobs from the API, a commit status `Velnor / Verified` in shadow next to `Required`. Then the App (D7) takes over the writer and `velnor-control` reuses the same library | Shadow agrees with `Required` on 500 commits; a PR that edits `rust-ci.yml` by hand is red; lab cases for skipped, cancelled and re-run checks pass | 14 h | opus / high | opus / high |

Next after the ten: the manifest renderer and `velnor verify` (≈ 20 h), the node target clone and
`mbx-seed` without a copy (≈ 10 h), package keys enrolled for compile-class obligations (≈ 12 h),
release promotion by digest (≈ 12 h), the `native` conformance leg (≈ 16 h).

Critical path: W01 → W03 → W04 (owner) → W07 → W08 → W09. W05, W06 and W10 run beside it. Two
agents can work at once without touching the same generator files if W03 and W05 are ordered.

---

## 7. Owner questions, rewritten

### 7.1 D1–D26

There is no D5 (`owner-questions.md:15`). D6, D10, D12 and D13 were listed as settled.

| Id | Was | Now | Recommendation |
|---|---|---|---|
| D1 | Release engine 0.1.81 | Obsolete (engine deleted, #1747) | Remove |
| D2 | Auto-merge, no catch-up merges | Reword: turn `allow_auto_merge` on and merge only through the queue | Yes, with W04 |
| D3 | First ruleset update | Reword: `Required` only, bound to 15368, empty bypass, ALLGREEN. Precondition: W01 and W03 pinned | Yes |
| D4 | Expression evaluator A or B | Obsolete for velnor-new; open only if the protocol executor is ever built | Remove from this plan |
| D6 | Lab reference executor | Settled (decision 19) | Keep |
| D7 | `velnor-verify` App and agent identity | Open, and now needed for two reasons: agents must add the label with a non-`GITHUB_TOKEN` identity, and only an App status gives "nothing runs" | Yes |
| D8 | Deploy identity and node changes | Reword: also decide where cluster manifests live (no `deploy/` in this repository) | Yes; manifests in this repository |
| D9 | Pre-approve CI-only engine releases | Obsolete. Successor: pre-approve tools releases that only fix CI (W01 needs one) | Yes |
| D10 | Two witnesses, ALLGREEN | Settled, but not applied: the live ruleset is HEADGREEN. Gap 5 gives a second reason | Apply in W04 |
| D11 | Cutover counts per job type | Reword: counts per obligation kind for enforced reuse (200 clean key-equal pairs) and for the native leg | As written in W08 |
| D12 | Where release bytes are built | Settled in principle (once, promoted). Now depends on Q-N5 | See Q-N5 |
| D13 | Park old runner PRs | Premise gone; becomes Q-N9 | — |
| D14 | Delete the engine crates | Done (#1747) | Remove |
| D15 | Recovery from a bad Velnor release | Open. Today's recovery is the admin bypass, which W04 removes | A documented break-glass: a signed, time-limited ruleset change by the owner, logged in an issue |
| D16 | Label added while a draft | Folded into Q-N2 | The label arms the PR when it becomes ready |
| D17 | Who may remove a check or a test | Open, and the code exists: `introduced()` reports every "check does less" change (`Ru/weaken.rs:57`, `Ci/plan.rs:163`) | Owner only; the gate fails a weakening without a recorded approval |
| D18 | A Velnor merge train | Open; needs the queue in use first | Not now |
| D19 | Compile before the label | Open | No. The owner's flow says nothing runs |
| D20, D22 | Rent nodes, rent comparators | Open, unchanged | D20 no for now; D22 one comparator when the benchmark runs |
| D21 | One compile for both runs | Open | No |
| D23 | Auto-verify unpushed commits | Open | Not yet; after `velnor verify` exists |
| D24 | Agent work above PR work | Open | Equal, until measured |
| D25 | Agents in cluster sandboxes | Open | A two-agent pilot after the manifest executor |
| D26 | Native arm64 lane | Open | Not now |

### 7.2 New questions

| Id | Question | Recommendation |
|---|---|---|
| Q-N1 | Approve the `docs/goals.md` change for two executors (text in 7.3)? | Yes, the narrow text below. It does not need Kubernetes, ARC or cache changes to goals |
| Q-N2 | The label flow: one label named `velnor:verify`; `ci:run` and `ci:auto` deleted; no quick set without the label; an unlabelled head shows a red `Required`. Accept one 5-second job per push until the App status exists? | Yes to all. The job is the only way an Actions check can be red instead of skipped |
| Q-N3 | Remove the admin bypass and bind `Required` once main has been green three times? | Yes. Before that it would block every merge |
| Q-N4 | Is the merge queue part of the flow? | Yes, ALLGREEN. It is the only place the merged tree is checked before it lands |
| Q-N5 | Release bytes: built on Velnor nodes and promoted, or built on GitHub-hosted runners? The generated verify step passes `--deny-self-hosted-runners` (`Re/release_attest.rs:70`) | Promote bytes built on a trusted Velnor run; make the flag a per-project choice with a pinned signer identity. Until decided, keep hosted builds |
| Q-N6 | Forks: decision 8 drops fork handling; goals line 43 and the fork fence (R:57) keep it. The repository is public | Keep the fence. It costs one expression and protects the nodes. Drop only the fork quick set when W03 lands |
| Q-N7 | Parallel jobs per check kind (architecture §8.1) or the goals' shape rule (rules 4 and 5)? | The goals' rule. Timings decide; no separate target directory per check kind |
| Q-N8 | Confirm that a result may be reused when its key is equal, given hard decision 3's history? | Yes. Shadow first; enforced reuse per obligation kind after 200 clean pairs; the nightly run stays a full audit |
| Q-N9 | Runner-host: keep hardening, or freeze? | Keep it as the GitHub Official host. Adopt the `Runtime` trait proposal; stop Mac-only features |
| Q-N10 | Where does the research live, given the evergreen-docs rule in `AGENTS.md`? | Design in `docs/proposed/` without dates or PR numbers; evidence and plans in issues |
| Q-N11 | Executors for the backlog: Claude Code only, or other vendors? | Keep the repository's `exec:*` labels; one vendor, as `docs/roadmap.md` says |
| Q-N12 | May Velnor Native execute the generator's plan directly, without the Actions protocol (2.4)? This replaces "a Rust runner that speaks the runner protocol" for velnor-new | Yes. Keep the protocol executor as a later option |
| Q-N13 | May deferred checks (`docs`, `check_alone` post-merge) close the queue when they fail on `main`? | Yes, until package keys bring them back before the merge |
| Q-N14 | May the first ledger use GitHub's cache as its store (2.3, backend 2)? | Yes, as a step that is deleted when the service lands |

### 7.3 Proposed wording for `docs/goals.md` (for the owner to approve or reject)

Current text, `docs/goals.md:16`:

> New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. Jobs are
> always executed by GitHub's official runner: on GitHub's machines, or on the project's own machines
> inside the containers that `velnor-runner` starts.

Proposed text:

> New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. On
> GitHub's machines, jobs are executed by GitHub's official runner. On the project's own machines
> Velnor has two executors, and the project chooses one for each kind of job. **GitHub Official** is
> GitHub's unmodified runner inside the containers that `velnor-runner` starts. **Velnor Native** is
> a Rust executor that runs the plan the generator wrote, without GitHub's runner. GitHub Official
> is the reference: every check must give the same result under both, and a difference is a defect
> in Velnor Native. Neither executor is a fallback for the other, and the executor is part of the
> identity of every result.

Three smaller edits the same approval should cover:

- Lines 11–12 (the table of what the file governs): name the crates under `crates/velnor-*` and
  remove the engine row; the engine is deleted.
- Rule 11, line 75: "with GitHub's official runner, started by the Velnor runner" becomes "with the
  executor the project uses, started by the Velnor runner".
- A new sentence under "Correct before fast": "A result is reused only when everything that could
  change it is part of its key and is equal. A result that cannot be keyed is produced again."

No change is needed to the 8 GB cache target or to "Inside GitHub's free limits": they describe
generated workflows on GitHub-hosted runners, and nothing above makes a hosted run depend on a node store.

---

## 8. What a generator-first foundation makes possible

**1. The generator emits the verification contract.** Today the contract is implicit in YAML: step
names and `if:` strings. The plan already has it as data. Emitting a sealed `.velnor/verify.lock`
beside the workflow (same header and digest as `Re/header.rs:22`) gives one list of obligations with
their key recipes, reuse class and due time. The workflow, the gate, `velnor verify`, the status
writer and the release gate all read that one list. A change to it is visible in review as a diff,
and `introduced()` (`Ru/weaken.rs:57`) already says which diffs make a check do less. That turns
"who may remove a check" from a policy into a failing gate.

**2. Selection becomes "what is not yet proven".** `select()` answers "what did this diff touch". The
ledger answers "which obligations of this tree have no passing record". The second question needs no
base commit, is the same for a PR, a queue group, a push and a nightly run, and is right on a red
base: unproven things run. `select()` does not go away; its closure becomes the scope of the package
key. The settings `on_main = affected | everything` and `full_run` lose their meaning as selection
modes and remain only as audit frequency.

**3. One plan, three ways to run it.** The YAML for GitHub Official, the manifest for Velnor Native,
and the same manifest on a developer's machine for `velnor verify`. An agent's local run then
produces records with the same keys as CI. Candidate records from an agent's sandbox do not gate a
merge, but they tell the agent in seconds whether the label run will pass, and they pre-warm the
node stores for it.

**4. The gate moves outside the change.** Generation is pure: `velnor-gen` reads files and never runs
cargo or repository code (`generator/AGENTS.md`). So trusted code can regenerate a change's
workflows from its configuration and compare. Together with `decide` over the jobs API this gives a
verdict the change cannot edit. No other design in the merged plan gets this without the App and a
service; here the first version is one generated workflow (W10).

**5. The ruleset is generated and watched.** `velnor-gen release ruleset` and `ruleset-drift` do
this for tags. The same for the branch ruleset turns "admin bypass was re-added" into a red check
the morning after, instead of a finding in a research report three days later.

**6. The benchmark is generated.** `velnor-ci bench`, `timings` and `[budget]` exist. The velnor-local
scenarios (`leaf`, `hub`, `lock`, `docs`, `warm`, `cold`, `story`) are the workload. A declared
workflow can run them on a schedule on both executors and commit the table. The shape rule already
chooses from measured timings; the same loop can refuse a change that moves a budget.

**7. Queue-tip verification by key.** An agent, or the label run itself, can verify the tree the
queue will build: the PR rebased on the current queue tip. If the queue then builds that tree, every
key matches and the group passes on records alone. With package keys the match survives unrelated
merges. This is the merged plan's V10, and it becomes a lookup instead of a scheduler.

**8. Test binaries as the unit of reuse.** The model already has `NextestArchive` and
`NextestRunArchive` (`M/plan.rs:298-306`). A test binary's digest covers every compile-time input
by construction. A test obligation keyed by `{binary digest, run-time input scope, nextest
configuration, environment contract}` is sound for the compile half without any path reasoning.

### The hard problems, named

| Problem | Why it is hard | Proposed handling |
|---|---|---|
| Run-time reads of tests | Tests climb out of their package and list directories; no static analysis is complete. A trimmed checkout is unsound: a test can pass because a file is absent | Whole-tree scope for tests by default. Enrol a narrower scope only from an enforced read audit on the intact tree, with the nightly run as the standing audit. The trace proposes and revokes; it never grants |
| Reproducible test binaries | `compat` includes the workspace path and the image (`docs/rust-ci-cache-keys.md`); two nodes can produce different bytes for equal sources | Fixed workspace path on nodes; path remapping; a shadow comparison of binary digests across two nodes before item 8 is used for keys |
| Flakes and reuse | A lucky pass, once recorded, is reused; a flaky failure blocks | Retries stay at 0 (`.config/nextest.toml` profile `ci`). Records keep per-test results; a test that passes and fails under one key is quarantined by name and its obligation becomes `reuse = never` until fixed |
| Trust in a record | The step that runs repository code also reports the outcome | Outcomes are read from GitHub's jobs API by step name, not from job outputs; the gate job repeats every lookup; only default-branch runs write trusted records. Authors are trusted (decision 8), so the model guards against accidents, not attackers |
| GitHub's check semantics | Skipped jobs pass; several check runs can share a name on one commit; a re-run replaces a result | A status written once per commit by one identity. Until then, `Required` never skips (W03) and lab cases pin each behaviour |
| Key completeness | A missing field is a false green that nothing reports | Shadow mode before every enforcement; the nightly run executes everything and compares; every key carries a version, and a new field is a miss |
| Two truths during migration | Old and new gate side by side is forbidden (decision 18), but a gate cannot be swapped blind | Shadow means the new verdict is computed and compared, never consulted. The switch and the deletion are one change |

What I would not do: build the protocol executor first, emulate `actions/cache` for our own
workflows, split checks into jobs by kind against goals rule 5, narrow test keys from a file-access
trace, or enable any reuse on a red main.

---

## 9. What I verified, and what I did not

Verified at the tip, in code or in a run: every row of the table at the top; the `Required` job's
conditions (R:571-612); the trigger list (R:5-12); the save conditions (R:303-326); the 18 alone
steps; the dead `GeneratorCheck`; the path classifier; the diff-base logic; `trusted_event`;
`decide`; `release-gate` and the release workflow's own gate; the step times of five runs.

Not verified, and marked [E] or [R] where used:

- That GitHub lets a later skipped `Required` hide an earlier failed one on the same commit. The
  precondition is observed and the fix does not depend on the answer. It is a lab case in W03.
- That the nextest bump caused the flag conflict. The conflict itself is in the logs of four runs.
- Any timing of a passing labelled PR run. None exists since the cutover.
- The hit rate of label-to-queue reuse. It depends on how fast `main` moves and on the key scope.
- Cluster, ARC and node facts. They are B's snapshot of 2026-11-18 and are outside this repository.
- The live ruleset. I read C's and E's readings and did not query it again.
- The content of draft #1751. I did not open it, because it belongs to this round.
