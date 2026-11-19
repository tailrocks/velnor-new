# Audit: the merged plan against main

| Field | Value |
|---|---|
| Model | Claude Opus 5.5 (`claude-opus-5-5`) |
| Client | Claude Code 2.1.325 (general-purpose subagent) |
| Reasoning effort | high |
| Date | 2026-11-20 |
| Repository state | main at `4fc0ede34e`–`a619478fb2` unless the text says otherwise; #1747 (old engine deleted), #1274, #1335 and #1748 merged after it |

Read-only evidence for the velnor-gen rebase round. Written by a subagent from the code, `gh` and git; unedited except for this header and replacing home-directory paths with `~`.

---

# E. Plan audit: the 2026-11 Velnor runner design and delivery plan vs velnor-new main

Read-only audit. Nothing was committed, pushed, or changed on GitHub or the cluster. I ran read-only `gh api` calls for the live ruleset, labels, commit list, and a few issue states.

Audited checkout: `~/.cache/ci-ambition/main-tip` at `a619478fb2` (2026-11-20 00:45 +0700). Plan base: `95de9b704b` (2026-11-18). Reports A-D (`~/.cache/ci-ambition/regen/`) were trusted unless code said otherwise. A-D cite them as A§, B§, C§, D§ below.

Status words: VALID (still true), DONE (implemented on main), CHANGED (premise moved), OBSOLETE (no longer applies), CONFLICT (contradicts goals.md, hard-decisions.md, or as-built behaviour). Actions: KEEP, REWRITE, DROP, MARK-DONE, GATE (needs owner approval first).

## 0. Read this first

1. **The audited checkout is already stale.** `origin/main` is `57b65af0db` (2026-11-19T18:05Z), five commits ahead: #1747 "Delete the old engine (velnor-actions)" (-452k lines, closes #1459), #1335 (lane-guard), #1274 (`[[workspace_groups]]`), #1743, #1746. After #1747: 18 crates remain (no `velnor-actions-*`, `velnor-freshness`, `velnor-test-support`); `.velnor/generator.lock`, `controller-approvals.json`, `freshness-inventory.json`, `qualification-gen.yml` are gone; `qualification.yml` is now velnor-gen-generated; `.velnor/config.toml` survives with only `[stacks.rust]` (read by `velnor-ci plan --mode`). Reports A, B and D still describe the engine as present (A§6, D finding 7). Everything below treats engine items as DONE by #1747 and the tip checkout as a lower bound.
2. **The plan was written for a CI that no longer exists.** It assumes the 39-job engine `ci.yml`, `Required verdict`, Plan/Ledger-CLI jobs, 26 per-crate jobs, state seal, image candidate, source qualification, product release, macOS legs, `velnor-actions 0.1.81/0.1.82` releases, and `.velnor/config.toml` as the config. All are gone. Today: one `rust` job (31 "alone" steps), `policy`, `Required`, `Cache`, `rust-trim`, `ci-run-label` in generated `rust-ci.yml`; four hand-written lint jobs in `ci.yml`; `release-rust-ci-tools.yml`; `rust-cache-trim.yml`; dispatch-only `qualification.yml`.
3. **The cutover made the gate non-gating.** Post-cutover (4.3 h sample) `Required` failed on 226 of 268 PR runs; 157 of 173 merges were direct merges that can only be admin-bypass (inferred by C from the ruleset and failing `Required`), median 0.6 min after the last push, with zero queue use (C§0,§1,§4). Live ruleset 24396608 (read now): `required_status_checks = [{context: "Required"}]` with NO `integration_id`, `bypass_actors = [RepositoryRole 5 admin]`, merge queue HEADGREEN, build 2, min wait 5, timeout 90, `allow_auto_merge=false`. The plan's phase-0 premise ("fix the gate, then speed up") is now step zero: make main green first.
4. **The as-built label flow is the opposite of owner decision 12 on the one point that matters.** An unlabelled ready PR gets a GREEN `Required` (quick set only), so it can merge. See section 1.
5. **Native executor and ARC deletion contradict goals.md and the active runner spec** (section 2). `docs/goals.md:3` forbids changing goals without explicit owner approval; no work item in the plan asks for it.
6. **The design's `executor/` Cargo root contradicts #1290/#1594** (one root workspace, flat `crates/velnor-*`), and its stated reason (delete engine crates without touching new code) is moot after #1747.
7. **Per-root keys (V08, "helper and generator roots", the 47% PR class) no longer exist as a concept.** There is one root workspace; `velnor-ci plan` already selects by package. The latency goals built on the helper/generator class need a re-derivation.
8. **Release on Velnor conflicts with generator policy.** velnor-gen's release verification passes `gh attestation verify --deny-self-hosted-runners` (`crates/velnor-render/src/release_attest.rs:70`) and pins hosted runners (`crates/velnor-render/src/release.rs:31`). Plan items R01/A06/G02/cohort 4 want releases on self-hosted Velnor.
9. **Dead references in the docs themselves:** `rustfs-exp/results.md` (actual `experiments/rustfs.md`), `manager/owner-decisions-late.md` (absent), `backlog/` and `~/.cache/ci-ambition/evidence/` (outside the repo), `milestones.md` (absent). AGENTS.md also requires evergreen docs without dated logs or PR/branch references; the plan docs are full of both.
10. **Counts:** delivery plan 174 items: VALID 86, CHANGED 50, CONFLICT 20, DONE 10, OBSOLETE 8 (section 6.2). Owner questions: 21 listed open D-questions, of which 3 are already answered by events (D1, D9, D14), 5 must be reworded (D2, D3, D8, D11, D16), D13 must be re-asked, and 11 new questions are missing (section 7.3).

## 1. The label flow: owner's requested flow vs main

Owner decision 12 (binding): (1) draft pushes run nothing; (2) ready-for-review runs nothing; (3) adding the label starts CI on the current head; (4) while labelled, every push starts a new pipeline and cancels the previous one; (5) removing the label stops further runs; (6) merge/enqueue only if the LATEST commit has a green required check; an unlabelled PR never has one; (7) queue verifies merged tree, reusing the PR verdict only for an identical tree. The design names the label `velnor:verify`; main has `pr_ci = "label"` (`.velnor/rust-ci.toml:10`, PR #1476, helper >= 0.1.86). Labels in the repo: `ci:run` ("One-shot full PR CI run") and `ci:auto` ("Full PR CI on every push"); `velnor:verify` does not exist (`gh label list`).

| Owner step | As built (rust-ci.yml, docs/rust-ci.md:76-90) | Status | Evidence | Fix |
|---|---|---|---|---|
| 1 Draft push runs nothing | `rust`, `policy`, `cache` skipped; `Required` runs and passes with notice "draft: CI not requested" | CONFLICT (green on draft) | R:640-645; docs/rust-ci.md:84 | Draft `Required` must not be success; use a failing or absent context |
| 2 Ready runs nothing | Runs the quick set on every push: plan, tools, caches, delta, Format, Lint, policy | CHANGED (cheap checks run) | A§2.5 | Owner call: quick set allowed or not |
| 3 Label starts CI on head | `ci:run` or `ci:auto` triggers full suite via `labeled` | DONE | R:4-19 | Rename or alias to the owner's label |
| 4 Label present: each push runs and cancels previous | `ci:auto` yes (concurrency `cancel-in-progress` for PRs, R:22-24); `ci:run` NO (one run, label removed by `ci-run-label`, then pushes get the quick set) | CHANGED | R:673-686 | Retire `ci:run` or make it non-owner-flow |
| 5 Label removed stops runs | `unlabeled` is not in the trigger list (opened, synchronize, reopened, ready_for_review, edited, labeled); an in-flight run is not cancelled; later pushes still run the quick set | CONFLICT | R:5-12 | Add `unlabeled` + cancel; no quick set without label |
| 6 Merge only on green LATEST commit; unlabelled never green | Unlabelled ready PR: `Required` = quick set result, gate step skipped (condition requires the label), so it is GREEN and mergeable. After label removal the new head is green without tests. Plus admin bypass (157/173 merges) and unbound `Required` | CONFLICT | R:636-668; C§1,§4 | New mode: non-draft PR without label fails `Required` "CI not requested"; bind to integration 15368; remove bypass |
| 7 Queue as additional check | `merge_group` runs the full suite in rust-ci.yml, but the queue is unused since 12:12Z 11-19 and `Rust CI` has never produced a merge_group run | CHANGED | C§1 | Revive queue; enable auto-merge |
| 8 No auto-labelling | Only `ci-run-label` removes `ci:run` | DONE | R:673-686 | — |
| Fork PRs | Quick set only, full suite in queue | CONFLICT with decision 8 ("drop fork handling") and with goals.md:43 | docs/rust-ci.md:90 | Owner decision, see section 7 |

Technical constraints for the fix (all inside velnor-gen, `PrRun` in `crates/velnor-model/src/plan.rs:115-127`, `plan_budget.rs:49-70`, `required_job.rs`): an Actions job can only be success/failure/skipped, and a skipped required job passes, so "unlabelled = blocked" needs a `Required` job that fails with "CI not requested", re-run on `labeled` and `unlabeled`; a green verdict then belongs to its commit and stays after label removal (matches design §8.2), but in-flight runs must be cancelled. `GITHUB_TOKEN` label changes do not trigger workflows, so agents must label with an App or user token (design says the same). Docs and tests to update: `docs/rust-ci.md`, `generator/CONFIG.md` `pr_ci` table, golden samples.

## 2. Conflicts with docs/goals.md, docs/hard-decisions.md and active specs

| # | Plan claim | Constraint | Status | Evidence |
|---|---|---|---|---|
| C1 | Owner decision 1/20: Velnor Native Rust executor is default; official runner optional | "Jobs are always executed by GitHub's official runner" | CONFLICT | docs/goals.md:16; `docs/proposed/macos-scaleset-runner.md:37-39` ("does not implement an Actions interpreter") and §8 lines 318-323 (out of scope: "a second workflow interpreter", Kubernetes, ARC, "a new distributed cache"); `docs/deferred/self-hosted-runner.md` ("MUST NOT be simulated") |
| C2 | Decision 18/delivery F01: delete ARC and `crates/velnor-runner-*` | goals.md:20-22 "the runner is developed as a product in its own right"; release-plz `runner-v` group publishes 8 runner crates; #1740 just added a CI lane for velnor-runner-host `local-mode` | CONFLICT | goals.md:20-22; release-plz.toml:49+; rust-ci.toml `[[test_lane]]` |
| C3 | Decision 13: Kubernetes + Docker platform trait replacing the `Runtime` trait | `docs/proposed/kubernetes-runtime.md` (#1324) proposes a different trait that KEEPS the journal; ARCH §5.10 deletes the journal | CONFLICT (two proposals) | B§5.4, B§6.1 item 2 |
| C4 | Decision 8: drop fork handling; "no PR comes from a fork" | goals.md:43 (fork PRs read only, no secrets), `docs/implemented/fork-pr-trust-model.md`, generated fork fence (`fork_runner`, #1328); repo is public, nothing prevents forks | CONFLICT | goals.md:43; R:57 |
| C5 | Decisions 22-23: native `actions/cache`, RustFS tiers, GitHub cache off the path | goals.md:59 "at most 8 GB of GitHub's 10 GB", "inside GitHub's free limits" (never-compromise 6), cache-keys design in `docs/rust-ci-cache-keys.md` | CONFLICT | goals.md:59; A§3 |
| C6 | Design §8.1: format/lint/docs/check-alone/tests as parallel lanes with one target dir per check kind, citing `hard-decisions.md:56-62` | goals rules 4-5 (compile at most twice per configuration; one job per workspace by default) and HD11 (`hard-decisions.md:486-512`); HD1 lanes are engine-only and engine is deleted | CONFLICT | goals.md rules 4,5; A§2.4,§8 |
| C7 | Ledger/exact-tree reuse across PR, group, push | HD3 (`hard-decisions.md:223-229`) bans baseline-gated skipping or any second skip mechanism without owner approval; binds engine only (hard-decisions.md:14-19, goals.md:14) but Gate 5 history says "ask" | CONFLICT (owner approval needed) | A§5.4,§7.3 |
| C8 | PR and group verdicts without an Actions job; `Velnor / Verified` App status as the gate | goals.md:41 "required check passes only when everything selected ran"; rule 12 and 13 (plan and required check from a released version; release only from default-branch commit whose required check passed); `velnor-ci release-gate` ties required checks to push-run jobs and refuses non-Actions App checks (gate.rs:295-298) | CONFLICT until gate/goals amended | gate.rs; goals.md:76-77 |
| C9 | P07 deletes velnor-local `--runner pair/plain` and runner.server | goals rule 11 (goals.md:75): local-first with the official runner started by the Velnor runner | CONFLICT | local/README.md; B§2 |
| C10 | Releases on Velnor (R01, cohort 4) | `--deny-self-hosted-runners` in generated verify step; `HOSTED_RUNNER` pins | CONFLICT | release_attest.rs:70; release.rs:31 |
| C11 | `velnor deploy --site` (UX §1.1) | `docs/host-deployment-decision.md` (Ansible deploys stay operator-run) | CHANGED, reconcile wording | B§8 |
| C12 | Plan docs carry dated measurements and PR numbers | AGENTS.md review rule: documentation stays evergreen, no dated logs or PR/branch references | CONFLICT | AGENTS.md "Review guidelines" |
| C13 | Backlog assigns gpt/gemini/muse models (backlog-assignments.tsv) | docs/roadmap.md runs Claude Code executors only; labels exist only for `exec:haiku/sonnet/opus` | CONFLICT | roadmap.md; `gh label list` |

## 3. Where the verification ledger and reuse plug into velnor-gen

From A§7.3 plus checks in code. Order by increasing risk.

1. **Prove the plumbing with what exists.** `velnor-ci merge-group-passed` + `[ci] skip_push_after_merge_group` (`crates/velnor-ci/src/merge_group.rs:46-86`, helper gate 0.1.82) already implements "reuse a prior run's verdict only on proof, else run everything". It is OFF here and useless while the queue is unused. Turn the queue back on, then enable it; this is the first measured reuse.
2. **Key types.** Extend `velnor-model` (`cache.rs` CacheKey, `verdict.rs` VerdictSpec, `selection.rs` RustSelection/AllTrigger) rather than a new `velnor-ledger` root; the plan's DG01/V01 canonical key struct belongs next to them. `CacheKey` text is rendered only by `velnor-render/src/cache.rs:20-61` `render_key`; keep that single-renderer rule.
3. **Decision point.** `velnor-ci plan` (`crates/velnor-ci/src/io/plan.rs:37-90`) after `select_event` (`velnor-rust/src/select_event.rs`, takes the diff as a closure). A hit turns `Some/All` into `None`-for-reused only with digest equality; add `SelectionReason`/`AllTrigger` variants so `--explain` names it.
4. **Per-step skip.** `Step.skip_after_merge_group: Option<PlanFrom>` (`velnor-model/src/plan.rs:109`, `merge_group_skip.rs`, output `skip_push`) generalises to a second `PlanFrom`; steps get an `if:` from a plan output.
5. **Gate.** `velnor-rust/src/verdict.rs` `decide` must accept "reused" only with evidence; today the sole reused case is `push && skip_push == 'true'`. `tests_expected` (`required_job.rs:149`) and the `tests_run/listed/ignored/by_package` outputs must still match.
6. **Trust.** Writers: trusted events only, as `mbx_store::trusted_event` (`crates/velnor-ci/src/mbx_store.rs:32`); PR writes stay isolated (goals.md:43, rule 8). Matches the design's candidate/trusted scopes.
7. **New command + config.** `HelperCommand` (`velnor-model/src/steps.rs:158-167`), `velnor-ci` subcommand, `helper_gate.rs` minimum, `[ci]` table in `config_ci.rs`, key in `config_keys.rs:4`, `generator/CONFIG.md`, golden sample under `generator/samples/`. Declared workflows can call helpers only if listed in `velnor-workflows/src/helper_call.rs` `HELPER_COMMANDS`.
8. **Where the verdict data already exist.** Delta layer source identity (`velnor-ci delta`, #1107), `inputs save/restore` blob ids, MBX CAS (node store), helper SHA digests (A§5.2). Reuse these digests before inventing new ones.
9. **Naming collisions to resolve in an ADR:** `velnor-ci inputs audit` (existing) vs design "audit A1/A2"; velnor-local authority "ledger" (dispatch event ledger) vs verification ledger; `velnor-ci report` vs `velnor trace`; `velnor-ci gate`/`release-gate` vs `velnor check/verify`; binary `velnor-host` and `velnor-local` vs the proposed single `velnor`.

## 4. docs/proposed/velnor-runner-architecture.md

| Section | Claim | Status | Evidence | Proposed fix |
|---|---|---|---|---|
| Status | "Proposal (v3); replaces nothing yet" | VALID | B§0.4 (zero lines implemented) | Add "written against 95de9b704b; repo facts superseded" banner |
| §1.1 table | Today: push-to-merge 85/457 min, PR run 26.7/92.3 min, 52.9 runner-min/run, full runs 3 per merge | CHANGED | C§0,§1,§5: pre-cutover P50 122/P90 701; post-cutover verified twice (PR + push), PR run P50 1.1 min but 98% red | Replace "Today" with C numbers; relabel old ones as pre-cutover |
| §1.1 | "8.1% false failures (10 of 124 push runs)" | CHANGED | C§2: post-cutover push runs 4 of 172 succeed | Re-measure after main is green |
| §1.2 C1 | "Under HEADGREEN three commits whose own Required failed sit on main" | CHANGED (worse) | C§1: 167 of 173 post-cutover merges had a failing `Required`; ruleset still HEADGREEN | Update evidence; stronger case for no-bypass |
| §1.2 C3-C4 | Plan/inventory "from a released helper and the base commit"; digests of every binary | VALID | goals rule 12 already holds: `.velnor/rust-ci.lock` + SHA-pinned setup action (A§1.5) | Cite as existing mechanism |
| §1.3 | `ci` nextest profile avoids the 2 s kill | VALID | rust-ci.yml uses `--profile ci`; `.config/nextest.toml` | — |
| §3 table | stage costs (hosted queue 5.5 min, Plan 12 s, Ledger CLI, `Required verdict` 49 s) | OBSOLETE | C§3.3: 0 hosted Rust CI jobs; Plan = `velnor-ci plan` step 16 s; Ledger CLI and `Required verdict` deleted | Re-derive stage table from `ci-metrics.json` of rust-ci |
| §3 stage 4 | Checkout 6 s x 31-39 jobs | CHANGED | one `rust` job + `policy`; checkout `fetch-depth: 2` (R:62-66) | Update job count |
| §4 rows 1-3 | Unrequested pushes cancelled 45%; edited re-runs 18.3% | CHANGED | `pr_ci=label` + `edited` filtered (R:35); runs still created | Mark half done |
| §4 row 7 | Plan job, Ledger CLI build, `Required verdict` aggregation | DONE | #1523; plan is a step; Ledger CLI gone | Delete row |
| §4 rows 9-10 | crates compiled 4.4x per run; clippy/doc/check-alone per job | CHANGED | one job, 31 `Check <crate> alone` steps post-merge only; HD11 `--workspace` compile (A§2.3) | Re-measure |
| §4 rows 12-14 | PR, merge group, push all run full | CHANGED | merge_group unused; push repeats PR work; `skip_push_after_merge_group` exists but off (A§5.3) | Cite the existing mechanism |
| §4 rows 15-18 | State seal, cache prune, source qualification, image candidate, source preview, product release, macOS legs | DONE | #1550 deleted workflows, #1554 image candidate, #1420 engine code | Delete rows; keep only tools release rebuild |
| §4 row 20 | A Docker daemon in every pod | VALID (unverifiable here) | B P2: ARC pods have privileged dind sidecar; cluster is out-of-repo | Keep, tag [M, 11-18] |
| §5.2 | New code in a new Cargo root `executor/` with its own lockfile | CONFLICT | `Cargo.toml` single workspace; #1290/#1594; engine deleted #1747 | Use `crates/velnor-*`; drop the lockfile rationale |
| §5.2 | Crate names `velnor-control/-dispatch/-executor/-exec/-official/-native-actions/-platform/-k8s/-docker/-storage/-cache/-ledger/-node/-conformance` | VALID | no collisions in `crates/`; close to `velnor-cache-rules`, `velnor-ci/src/platform.rs` | ADR on names |
| §5.3 | Executors: Native default, Official reference; APIs per v2.338.2 | CONFLICT (goals.md:16) / CHANGED pin | `.velnor/runner.lock` pins 2.338.0 (lines 17-21) | Owner approval first; bump pin |
| §5.3 | `velnor-ci obligation exec` | VALID (new command) | not in cli.rs | needs helper_gate minimum |
| §5.4 | Platform trait + capabilities | VALID, conflicts with #1324 trait | B§3.1 | Merge the two proposals |
| §5.6 | Closed action set from "every `uses:`" with counts 61, 77/60, 2/17/5, 48, 12, 8/1, 6, 134 composite calls, 105 background | CHANGED | tip counts: checkout 49, cache/restore 14, cache/save 9, cache 3, upload 10, download 6, mise-action 15, mr-boxington 4, setup-velnor 6, github-script 5, alint 1; 0 `background: true`; no attest/app-token use in this repo | Regenerate counts from `rules.rs` ALLOWED_ACTIONS (16) and workflows |
| §5.6 | Native set lacks docker/login, setup-buildx, build-push, github-script (JS) which velnor-gen allows | CONFLICT inside design | rules.rs:17-34 | Add to set or remove from generator list |
| §5.7 | `qualification.yml` is the oracle seed | CHANGED | engine file deleted, `qualification.yml` now velnor-gen-generated from `.velnor/workflows.toml` (#1747); 64 jobs, hosted vs scale-set pairs | Re-point |
| §5.8 | No fork PRs | CONFLICT | see C4 | Owner decision |
| §5.9 | "Journal discipline" ported from Turso journal | VALID | journal v58 (B§4.2) | — |
| §5.10 | `velnor-runner-core` open defects (short id covers, live slot replaced) become tests | CHANGED | #1142/#1143 closed; fixed (ownership.rs:31-36; #1144 `580b4e1a3b`, #1275) | Update row |
| §5.10 | velnor-host journal "Delete at the final milestone, no new investment" | CHANGED | ~55 hardening commits since base; runner-host 171k to 181k lines; CI lane #1740 | Re-decide (D13 re-ask) |
| §5.10 | hosted release runners at `release.rs:29`, `release_binaries.rs:44-46` "go in phase 0" | CHANGED | now `crates/velnor-render/src/release.rs:31`, `release_binaries.rs:54-57,257,293`; verify uses `--deny-self-hosted-runners` | Not phase 0; owner decision |
| §5.10 | engine 39-job `ci.yml` and product release gate: Redesign | DONE | #1523, #1550, #1747 | Delete row |
| §6.1 | whole repo checkout `/*` without `/docs/**` (`ci.yml:97`) | CHANGED | rust-ci.yml full checkout depth 2; ci.yml sparse only for lint jobs | Update |
| §6.1 | `now_seconds()` at `io/mod.rs:210` | CHANGED | `crates/velnor-ci/src/io/mod.rs:289`, `io/budget.rs:241` | Fix refs |
| §6.2 | nextest 0.9.151 pinned (`mise.toml:24`) | CHANGED | `mise.toml:24` = 0.9.152 | Fix |
| §6.5 | A1 audit and `Required` push check | CHANGED | name collision with `inputs audit`; `release-rust-ci-tools` gate only checks ancestry (A§4.1); #1608 open to fix | Rename; land #1608 first |
| §7.3 | `inputs restore` port `inputs.rs:102-151`; `io/audit/dep_info.rs` | VALID | `crates/velnor-ci/src/io/audit/dep_info.rs` exists; `inputs.rs` exists (lines not re-verified) | Re-cite after reading |
| §7.3 | mbx on a second node, `mbx` missed 10/10 locally | CHANGED | `mbx_shared_store` shipped #1411 and used in CI (rust-ci.toml:7) | Re-measure with shared store |
| §7.4 | node mirrors of repo and upstreams | VALID | git mirrors absent; crates mirror shipped (A§3.4, B§3.4) | Cite crates mirror as precedent |
| §8.1 | Parallel lanes, one target dir per check kind (`hard-decisions.md:56-62`) | CONFLICT | C6 | Owner decision |
| §8.1 | Push `Required` runs `velnor-ci audit wait`; "push runs not cancelled, ci.yml:57-59" | CHANGED | `Required` is generated in rust-ci.yml (R:631-672) via `velnor-ci gate`; ci.yml concurrency at lines 37-39 cancels only PRs | Re-cite |
| §8.1 | Obligation inventory maps today's blocking checks (Plan, lints, every Rust variant...) | CHANGED | blocking checks now: `Required` only (ruleset); `ci.yml` jobs not required (C§4) | Rebase inventory on velnor-gen plan |
| §8.2 | "Step 0, now: two contexts bound to 15368, `bypass_actors: []`, ALLGREEN, depth 2..." | CHANGED | This is the PROPOSAL; live state is one unbound context, admin bypass, HEADGREEN, wait 5 | Rewrite "today" row |
| §8.2 | Transition step 1 "both release entry points migrated" | CHANGED | one release entry point left (`release-rust-ci-tools.yml`); product release deleted | Simplify |
| §8.2 | label `velnor:verify`; label gate job "fails without the label" | CONFLICT | section 1 | Align generator mode |
| §8.3 | ALLGREEN, auto-merge, queue depth | CHANGED | queue unused; `allow_auto_merge=false` | Revive |
| §8.4 | "Today the product workflow embeds its shell gate (`schema2_release_eligibility.rs:223-247`)" | OBSOLETE | file deleted by #1747 | Delete |
| §8.4 | `velnor-ci release-gate` gains `--app-check`, `--push-check` (`gate.rs:84-94,277-288,354-376`) | CHANGED | flags absent (cli_release.rs:12-23 has `--pr-only-check` only); push-run evidence already required (gate.rs:141-172); non-Actions App checks refused (gate.rs:295-298) | Describe delta vs existing |
| §8.4 | tools release 190-434 s; aarch64 cross-compiled and smoke-run under qemu | CHANGED | run P50 4.1 min post-cutover; arm built natively on `ubuntu-24.04-arm` (release-rust-ci-tools.yml:106-186) | Update |
| §8.5 | 924.8 s `receipt_integrity`, 679 s `non_inputs_differential`, actions-cli 132 s/258-274 s | CHANGED | receipt_integrity still in runner-host; other suites deleted with engine | Drop deleted suites |
| §8.5 | 356 integration binaries to one per package | DONE (mostly) | velnor-ci/-gen/-local/-release/-cache-rules have one `tests/it`; engine tests gone | Mark done; runner-host residual |
| §8.6 | `velnor cargo` takes `mise.toml`'s wrapper slot from `mbx` | CHANGED | `mise.toml:44-46`; CI also depends on `mbx-store`/`mbx-seed` | Order after generator change |
| §9 | cluster facts (5 workers, ARC, 92-95% CPU) | VALID (unverified) | B§0.2 snapshot 11-18; no in-repo source | Tag as snapshot |
| §10.1 cohorts | job types move per cohort | CHANGED | job inventory replaced (G01) | Rebuild cohorts |
| §10.2 day 0 deletions | hosted release runners, `edited`, fork twin, catch-up, `-j2`, Darwin, per-push triggers | DONE except hosted release runners/`edited` | #1523, #1550, #1747 | Update |
| §10.3 lab A | GitHub semantics lab | VALID | no App exists | Keep; also test label flow in rust-ci.yml |
| §11 | benchmark baseline "today's CI" | CHANGED | pre-cutover CI gone | Define baseline |
| §13 D1, D9, D14 | engine release and delete questions | OBSOLETE | #1345, #1747 | Remove |
| §13 D3 | JSON with `Required verdict` | CHANGED | section 7 | Reword |

## 5. docs/proposed/velnor-user-experience.md

| Section | Claim | Status | Evidence | Proposed fix |
|---|---|---|---|---|
| §1.1 | App `velnor-verify`, agent identity, deploy identity | VALID | none exist (B§6 row 9) | Keep |
| §1.1 | `velnor deploy --site de` | CHANGED | no deploy dir; cluster manifests in external `velnor-runners`; host-deployment-decision.md | Say where manifests live |
| §1.1 | "Official runner image kept" | VALID | images/runner, `.velnor/runner.lock` 2.338.0 (doc says 2.338.2) | Fix version |
| §1.2 | `velnor local up`, install via `cargo install` | CHANGED | existing binaries `velnor-local up/run/down` and `velnor-host`; local mode is macOS Apple-Silicon only (B§2.4) | Align names |
| §2 | Config in `.velnor/config.toml` `[velnor.*]` | CONFLICT | #1747 kept config.toml only for `[stacks.rust]`; generator config is `.velnor/rust-ci.toml` with unknown keys rejected (config.rs:75-77) | New file or new keys through config_keys.rs |
| §2 | `[velnor.executor.job_types]` "release-rust-ci-tools", "product-release", "job-image-candidate" | OBSOLETE (two of three) | product-release and image candidate deleted | Drop |
| §2 | `runner_lock = ".velnor/runner.lock"` | VALID | file exists | — |
| §3 | velnor-gen "replaces the three github-script steps" | CHANGED | 5 uses in 3 workflows (release-rust-ci-tools.yml:26; qualification.yml) | Fix count |
| §3 | After day 5.2 PRs and groups have no Actions run | CONFLICT | goals rules 9, 12, 13; C8 | Owner decision |
| §3 | Closed action table | CHANGED | see arch §5.6 | Regenerate |
| §4 | "Open a draft... No status... blocks" | CONFLICT | as built draft is green | section 1 |
| §4 | "Until App gate: Actions job fails 'CI not requested'" | CONFLICT | as built unlabelled is green | section 1 |
| §4 | label `velnor:verify`, remove label unchanged verdict | CHANGED | `ci:run`/`ci:auto` | Rename |
| §4 | queue reuse "60-75% of groups" | VALID (estimate) | no queue use now | Keep as [E] |
| §5 | `velnor verify/ship/wait/resolve/why-red/check/explain/cache/mirror/status/env/mcp/lock/trace/run/diff` | VALID (all new) | none exist; velnor-local `run` is nearest | Name ADR |
| §5 | `velnor run ci.yml:check-generator-samples` | OBSOLETE | job name gone (engine) | Use a rust-ci step or `qualification` job |
| §5 | `velnor explain ... clippy:generator`, `test:velnor-gen` | CHANGED | one workspace; names should follow velnor-gen step/package names | Rewrite examples |
| §6 | `Velnor / Details` check, metrics list | VALID | `velnor-ci report`, `Cache` check exist (A§3.3) | Reuse |
| §7 | Recovery D15, A1 lag alarms, queue 90 min | CHANGED | ruleset timeout 90, wait 5 (not 0) | Fix numbers |
| §9 | agents label with App identity, auto-merge at creation | CHANGED | `allow_auto_merge=false`; admin merges | Prereq B04 |
| §9 | "Push freely. Drafts and unlabelled PRs run nothing" | CONFLICT | ready PRs run quick set (A§2.5) | Align with owner decision on quick set |

## 5a. docs/research/velnor-runner-2026-11/explainer.md

| Claim | Status | Evidence | Fix |
|---|---|---|---|
| §2 "typical PR touches only generator/ or ci-helper/ (47%)" | OBSOLETE | those directories hold only docs/samples now (D§0.6) | Re-measure class mix with `velnor-ci plan` package output |
| §2 stage table (Plan waits 5.5 min hosted; Required waits 24.3 min) | OBSOLETE | C§3.3 | Replace with C numbers |
| §3.1 "Today 45% of PR runs cancelled, 4.77 runs per merged PR; edited 18.3%" | CHANGED | post-cutover 108 of 268 cancelled; edited filtered | Refresh |
| §3.1 "Today the same commit is tested three times" | CHANGED | twice (PR + push), queue unused (C§5) | Update |
| §3.2 "ARC goes away" | CONFLICT | goals.md:16, macos spec §8 | Gate |
| §3.5 "No Node" | CHANGED | generator allows JS actions (github-script, cache, upload, mise-action) and docker/* | Reconcile with ALLOWED_ACTIONS |
| §4 `velnor cargo` replaces mbx | CHANGED | mbx in CI via shared store | Order |
| §5 "bypass list is empty; admin bypass carried 7 of 136 merges" | CHANGED | bypass is now the normal path: 157 of 173 (C§1) | Strengthens the case; update numbers |
| §6.2 floors for helper and generator class | CHANGED | single workspace | Re-derive |
| §7 comparison table | VALID | no re-check needed; vendor claims | Keep |
| §10 day table | CHANGED | see delivery plan | Re-date after owner answers |
| §11 open list D1, D9, D14 | OBSOLETE | answered by events | Remove |
| Link `owner-questions.md`, `delivery-plan.md`, `decisions.md` | VALID | files exist | — |

## 5b. docs/research/velnor-runner-2026-11/evidence.md

Frozen snapshot of 2026-11-18 at `95de9b704b`. It reports facts only, so most rows are historical and correct for their date. Rows that now mislead:

| Claim | Status | Evidence | Fix |
|---|---|---|---|
| Headline "Workflows: engine `velnor-actions` 0.1.80 generates them; velnor-gen and velnor-ci not wired in yet" | OBSOLETE | #1523 cutover 13:31Z 11-19 | Add "superseded" banner |
| "Each run has 39 jobs", T3/T5/T10 per-job tables | OBSOLETE | rust-ci.yml has 1 `rust` job + 5 | Keep as pre-cutover history |
| "8 gate jobs stay on GitHub-hosted ubuntu-26.04" | OBSOLETE | C§3.3 | — |
| "velnor-new runner is a separate 8-crate workspace of ~185k lines" | CHANGED | one workspace; host 181k lines (B§5.1) | Update |
| "crates mirror DaemonSet currently unused" | CHANGED | enabled in rust-ci.toml `[crates_mirror]` | Update |
| "MBX_REMOTE_URL empty in 68 of 68 logs; mbx local cache 40-74%" | CHANGED | `mbx_shared_store=true` (#1411) | Re-measure |
| ARC, cluster, node facts (§B) | VALID (not re-verifiable here) | B§0.2 | Mark snapshot |
| Protocol facts (§F), runner v2.338.0 | VALID | `.velnor/runner.lock` | — |
| Runner v2.338.2 cited elsewhere | CHANGED | repo pin 2.338.0 | Reconcile |
| Source file paths `ci-timings.md` etc. | CHANGED | raw data in `~/.cache/ci-ambition/evidence/` outside repo | Note |

## 5c. planning/backlog-issue-format.md and backlog-assignments.tsv

| Item | Status | Evidence | Fix |
|---|---|---|---|
| `milestone: "D1"` defined in `milestones.md` | CHANGED | no `milestones.md`; repo milestones are FAST-0..FAST-6, FAST-L, FAST-R (D§3) | Map to FAST milestones or create |
| `impl.model` ids (claude-opus-5-5, gpt-6.2-sol, ...) | CONFLICT | repo labels are `exec:haiku/sonnet/opus`, `effort:low/medium/high` and roadmap.md runs Claude Code executors | Restrict to claude ids or extend roadmap.md |
| TSV uses `opus-5.5`, `muse-wonder-full-1.0` but format lists `claude-opus-5-5`, `muse-wonder-1.0` | CHANGED | model ids not in the allowed list | Normalise |
| TSV groups G1-G9 cover streams V; L RV; X; H O; P F; A; ST; C T S; B G R | CHANGED | streams AN (14 items), DG (11), BM (15) have no group | Assign |
| `parent: VLN` root epic, L1 epics per stream (19) | CHANGED | existing root is #574 `[FAST]` with 91 sub-issues, epics #578/#579/#581/#626/#627, `epic`, `fast-ci` labels | Attach under #574 or a new root, same labels |
| `blocked_by: [keys]` | CHANGED | repo uses native "blocked by" relationships and the ready-issue query over label `fast-ci` (roadmap.md) | Create relationships, add `fast-ci` |
| "`deletes: crates/velnor-runner/...`" | CHANGED | path no longer exists; crates are flat `crates/velnor-runner-*` | Fix examples |
| "`## What exists today` exact paths read at the frozen commit" | CHANGED | frozen commit `d6be276a6c` predates restructure; most `generator/`, `ci-helper/` paths wrong | Regenerate against 57b65af0db |
| `owner_decision: D4` field | VALID | repo has `needs-owner`, `owner-action`, `owner-decision` labels | Add label mapping |
| `## If blocked: add label blocked` | CHANGED | roadmap.md: retry once with stronger model before `blocked` | Align |
| Script creates issues after owner confirms | VALID | not run; 174 items plus sub-issues is comparable to or larger than the current 215 open issues | Needs owner call |

## 6. docs/research/velnor-runner-2026-11/delivery-plan.md

### 6.1 Non-item claims (preamble, sections 1-9)

| Section | Claim | Status | Evidence | Proposed fix |
|---|---|---|---|---|
| Header | "Code was re-read at velnor-new `d6be276a6c`" | CHANGED | tip `a619478fb2`, origin `57b65af0db` | Re-read at new tip |
| §1 | Phase 0 first: ALLGREEN, empty `bypass_actors`, both contexts bound to 15368 | CHANGED | live ruleset: one unbound context, admin bypass, HEADGREEN (gh api, C§4) | Rewrite with `Required` only |
| §1 | "Every CI job on Velnor, or retired: day 4.8" | CHANGED | Rust CI is already 100% on the scale set; remaining hosted: fork PRs, release-rust-ci-tools, qualification pairs | Redefine as "no ARC" |
| §2.1 | Port legacy `scaleset/client.rs`, `session.rs`, `listener.rs` (6 h port / 20 h fresh) | CHANGED | Rust scale-set client already in `crates/velnor-runner-github` (8.2k lines) and `velnor-runner-host` (sessions, long-poll, acquire, JIT, org-level set, B§1.3) | Only broker/run-service/Results are new; do not port twice |
| §2.1 | velnor-legacy paths under `refs/velnor-legacy` | VALID (outside repo, not re-read) | evidence.md §E | — |
| §2.2 | `velnor-runner-github` 6,878 LOC; `velnor-runner-core` 719 LOC | CHANGED | 8.2k and 925 lines (incl. tests) | Update |
| §2.2 | Engine orchestrator 186.8k and renderer 88.7k lines; "Redesign: replaced by G04" | DONE | engine deleted #1747 (-452k lines) | Drop row |
| §2.2 | velnor-gen has "one `runner` for all jobs (`velnor-rust/src/plan.rs:344`)"; hosted runners at `release.rs:157,172`, `release_binaries.rs:188,222` | CHANGED | per-job overrides exist (`test_groups.runner`, `crates_mirror.runner`, `[[workflow]] runner`); hosted at `crates/velnor-render/src/release.rs:31`, `release_binaries.rs:54-57,257,293` | Re-cite |
| §2.2 | "Freeze the old runner (D13): ~35 open velnor-runner-host PRs (#1249-#1359)" | OBSOLETE | none open; 7 open PRs on origin were CI/generator (D§1); ~55 runner commits merged | Delete; re-ask policy |
| §2.2 | `velnor-local` has a mitmproxy recorder | CHANGED | not found; recorder-like pieces are `local/conformance` and the Rust cache server/api-stub (B§2) | Fix |
| §2.3 | Platform trait, Docker first, Kubernetes production | VALID | B§3.1 | Reconcile with #1324 |
| §2.4 | Evaluator A/B | VALID | no repo dependency | — |
| §3.1 | Location: `executor/`; "engine crates, `ci-helper/` and `generator/` keep their roots" | CONFLICT | `generator/`, `ci-helper/` now hold only docs/samples; one root workspace | New code in `crates/` |
| §3.3 | Owner decisions D1.. | see section 7 | | |
| §3.4 | Phase-0 JSON, `PUT /repos/tailrocks/velnor-new/rulesets/24396608` | CHANGED | ruleset id still 24396608 (VALID), JSON obsolete | Regenerate |
| §3.4 | "Phase-0 PRs merge through the queue; no admin merge" | VALID | roadmap.md never-do list: no `--admin`; but 157/173 recent merges were admin | Needs enforcement mechanism |
| §3.4 | Agent PR cycle 94 min to 34 min | CHANGED | post-cutover Rust CI P50 1.1 min but `Required` red; merge in 0.6 min via bypass (C§1) | Re-baseline |
| §4.1 | Streams and agent-hours | CHANGED | stream B, G shrink; X, A, V depend on owner gate | Recompute after item table |
| §4.4 | "Engine releases about 104 min with macOS, 27 without" | OBSOLETE | engine deleted | Drop |
| §4.4 | One workflow-lane agent: `.velnor/config.toml` and `ci.yml` single-writer | CHANGED | writers now: `.velnor/rust-ci.toml`, `.velnor/workflows.toml`, `.velnor/rust-ci.lock`, `rust-metadata.json` (generated), `rust-ci.yml` | Update single-writer list |
| §5 | Cohort table with engine job names | CHANGED | see G01 | Rebuild |
| §6 | Milestones by day | CHANGED | days shift: day 0 deletions done, day 0 requires green main first | Re-date |
| §7.3 | `claimed:<agent>` in the roadmap project | CHANGED | project fields Status/Stage/Parent/Sub-issues; label `in-progress`; 0 of 215 issues have assignees (D§3) | Use labels |
| §7.1 | "one PR under 400 refactored lines" | VALID | consistent with alint 400-line Rust cap (ci.yml `Alint` fails on Rust file > 400 lines, C§2) | Keep |
| §8 | Risk "agents collide on `.velnor/config.toml`" | CHANGED | file is now small | Update |
| §9 | First ten items B01, B05, B02... | CHANGED | B02 done; B03 done; add N01 | Replace |
| Whole doc | `(X05) 61 tests`, `(X08) 765 legacy tests` etc. refer to legacy repo | VALID | not in tree | — |

### 6.2 All 174 work items

Status counts: VALID 86, CHANGED 50, CONFLICT 20, DONE 10, OBSOLETE 8.

Each row: status, why (evidence), action. Item scope text is in the plan; only changes are recorded.

| ID | Title | Status | Why / evidence | Action |
|---|---|---|---|---|
| B01 | Owner decision pack | CHANGED | D1/D9/D14 answered by events (#1345 merged, engine deleted #1747); D3 premise changed (see B05); label flow question missing | REWRITE: new pack = D3', label flow (owner vs ci:run/ci:auto), goals.md rewrite approval, bypass retirement |
| B05 | First ruleset update (owner applies) | CHANGED | Live ruleset 24396608: only `Required` (no integration_id), bypass_actors=[RepositoryRole 5 admin], HEADGREEN, build 2, min wait 5, timeout 90 (C§4; gh api read). `Required verdict` was dropped 2026-11-19 12:30Z (v58581881), so the plan JSON (§3.4) would require a check nothing emits | REWRITE: JSON = `Required` bound to integration 15368, bypass [], ALLGREEN; precondition new item N01 (green main) or PRs deadlock (226 of 268 PR runs fail `Required`, C§4) |
| B02 | Release velnor-actions 0.1.81 | DONE | Engine 0.1.81 released (#1345 merged; generator.lock pinned 0.1.81 at tip); engine deleted by #1747 (57b65af0db) | DROP |
| B03 | Gate jobs off hosted runners | DONE | #1523 053fefa6a3: every Rust CI job on `ubuntu-26.04-scale-set`, 0 hosted jobs in 2660 sampled jobs (C§3.3); plan/ledger-cli/required-gate jobs deleted | MARK-DONE (hosted remains only for fork PRs, release-rust-ci-tools, qualification pairs) |
| B04 | Merge procedure | VALID | repo `allow_auto_merge=false`, squash only (C§4); queue not used since 2026-11-19 12:12Z, 157/173 post-cutover merges are admin-bypass direct merges (C§1) | KEEP, add: re-enable queue use and retire admin bypass (needs green `Required`) |
| B06 | No `edited` | CHANGED | rust-ci.yml still lists `edited`; jobs skip unless base changed (R:23,35) but a run object is still created; hand-written ci.yml still runs 4 jobs on `edited` (ci.yml:5-10,50) | REWRITE: drop `edited` from ci.yml; generator option to emit no `edited` |
| B07 | Interim label step | CONFLICT | Plan: unlabelled head is RED ("CI not requested"). As built (`pr_ci="label"`): unlabelled ready PR gets GREEN `Required` after quick set, gate step skipped (rust-ci.yml:636-668; docs/rust-ci.md:80-90); drafts green too. Violates owner decision 12 | REWRITE as velnor-gen mode (new `pr_ci` value) + `unlabeled` trigger; see label-flow table |
| B08 | Cancel stall | DONE | All gate jobs on scale set; PR concurrency cancels older runs (R:22-24); 83 cancelled-by-newer-push runs observed (C§2); p90 supersede wait not measured | MARK-DONE; measure |
| B10 | macOS paused | DONE | product-release*, Darwin legs deleted #1550; velnor-ci has no macOS build since 2026-11-19 (actions/setup-velnor/action.yml); release-rust-ci-tools Linux musl only | MARK-DONE |
| B11 | Narrow per-push workflows | DONE | job-image-candidate (#1554), source-qualification, source-preview deleted (#1550); qualification is dispatch-only | MARK-DONE |
| B12 | No `-j2` caps | DONE | rust-ci.yml has no `-j2`; remaining `build.jobs=2` were engine mise tasks (mise.toml:94-181 at tip) removed by #1747 | MARK-DONE after grep |
| B13 | Per-job profiles | OBSOLETE | Per-job engine overrides and `velnor-native` profile label belonged to the engine; velnor-gen has global `runner`, `fork_runner`, `test_groups.runner`, `crates_mirror.runner` (A§1.3) | DROP; fold needed label routing into G02 |
| B14 | Release 0.1.82 and regenerate | OBSOLETE | Engine release 0.1.82 batch moot | DROP |
| B15 | Daily report | CHANGED | Metrics exist: `velnor-ci report`, `bench collect/compare/budget`, `timings`, `ci-metrics.json`, `[budget]` (A§1.1,§8); no push-to-merge report; baseline "85 min proxy" is pre-cutover | REWRITE: build on velnor-ci bench/timings; re-baseline from C data; add push-to-merge from GitHub API |
| RV01 | velnor-host for the Docker backend | CHANGED | velnor-runner-host now 181k lines, journal v58, org-level scale set, linux credential file (B§5); #1142/#1143 defects fixed (#1144,#1275) | KEEP with new inputs; decide before more hardening |
| RV02 | The #1324 `Runtime` trait | VALID | #1324 merged as docs only (`docs/proposed/kubernetes-runtime.md`), 0 of 12 PRs started (B§5.4) | KEEP; must reconcile K8S doc (keeps journal) vs ARCH §5.10 (deletes journal) |
| RV03 | velnor-ci, velnor-gen and the product release gate | CHANGED | gate.rs at crates/velnor-ci/src/gate.rs; planner now crates/velnor-rust/src/plan.rs; schema2_release_eligibility.rs/.sh deleted by #1747; hosted release labels at crates/velnor-render/src/release.rs:31, release_binaries.rs:54-57,257,293 | REWRITE scope to new paths |
| RV04 | velnor-local | CHANGED | velnor-local is crates/velnor-local + local/{scenarios,conformance,docker}; no mitmproxy recorder found; runner.server stand-in is patched in local/docker/runner-server/Dockerfile; `velnor-new` scenario added #1718 (B§2) | REWRITE scope |
| RV05 | The official-runner wrapper for `velnor-official` | VALID | crates/velnor-runner-host/src/launch/*, job-start hook in crates/velnor-resource-probe, images/runner (B§1.1 P6,P7) | KEEP |
| P01 | Capability model | VALID | No trait exists (`git grep "trait Runtime"` empty, B§3.1); K8S doc proposes a different `Runtime` | KEEP; merge with #1324 design |
| P02 | `velnor-platform` trait | VALID | ~50 files import bollard; no kube dep (B§6 row 3) | KEEP |
| P03 | Docker backend | CHANGED | Docker backend already exists in velnor-runner-host (bollard 0.21.1, docker_client.rs, docker_spec.rs audit) | REWRITE as refactor of existing code per RV01 |
| P04 | Kubernetes backend | VALID | ARC in production is outside the repo and not Velnor code; no deploy/ dir (B§0.2) | KEEP; decide repo home for manifests (new item N11) |
| P05 | Private Docker daemon per request | VALID | images/dind exists; ARC pods use privileged dind sidecar (B P2) | KEEP |
| P06 | Per-backend conformance | VALID | only per-backend corpus is local/conformance probes (B§2.3) | KEEP |
| P07 | Local runs on the Docker backend | CONFLICT | Deletes velnor-local `--runner pair` and `plain` and runner.server; goals rule 11 (goals.md:75) names the official runner started by the Velnor runner and a local stand-in; local/README.md | GATE: owner approves goals rule 11 change |
| O01 | `velnor-official` for acquired jobs | CHANGED | Official runner in pair containers is today's only executor (velnor-host, images/runner, .velnor/runner.lock pin 2.338.0, plan assumes 2.338.2) | REWRITE: extend existing wrapper, bump pin |
| O02 | Reference role isolation | VALID | none of reference role exists (B§6 row 2c) | KEEP |
| O03 | Official obligation mode | VALID | synthetic job message: nothing exists | KEEP |
| O04 | Paired speed per job type | VALID | nothing exists | KEEP |
| X01 | Root and port checker | CONFLICT | `executor/` separate root with own lockfile; repo just collapsed 4 nested workspaces into one (#1290/#1594) and the reason (delete engine crates untouched) is moot after #1747; goals.md:16 and macos spec §8 forbid a second interpreter | GATE; place crates under crates/velnor-* |
| X02 | Broker and JIT | CONFLICT | Broker/JIT client in Rust: only scale-set side exists (crates/velnor-runner-github); broker/run-service/Results absent (B§1.3) | GATE |
| X03 | Acquire, renew, complete | CONFLICT | run-service acquire/renew/complete absent | GATE |
| X04 | Results | CONFLICT | Results service absent | GATE |
| X05 | Job model | CONFLICT | job model absent; velnor-legacy not in tree | GATE |
| X06a | Expressions, option A | CONFLICT | expression engine absent | GATE |
| X06b | Expressions, option B | CONFLICT | as above | GATE |
| X07 | Templates and contexts | CONFLICT | absent | GATE |
| X08 | Job state machine and steps | CONFLICT | absent | GATE |
| X09 | Commands | CONFLICT | absent | GATE |
| X10 | Masker | CONFLICT | absent | GATE |
| X11 | Post steps and cancel | CONFLICT | absent | GATE |
| X12 | Composites and admission | CHANGED | "134 composite calls" gone: remaining composites are qualification/actions/{composite,docker,post-fail}, actions/setup-velnor, .github/actions/velnor-verify-jactionlint | GATE; recount |
| X13 | Background steps | OBSOLETE | "105 `background: true` and 29 wait-all" came from the engine ci.yml; 0 in any workflow at tip (grep) | DROP unless a consumer appears |
| X14 | Journal and faults | CONFLICT | absent; discipline already in velnor-host journal (v58) | GATE |
| X15 | Executor image and deploy | CONFLICT | deploy has no repo home (no deploy/; manifests in outside `velnor-runners`) | GATE; N11 |
| X16 | velnor-dispatch | CONFLICT | dispatcher replaces ARC; today host controller is single-engine, macOS/launchd-shaped (B§6 row 1) | GATE |
| X17 | Legacy-incident suite | VALID | legacy incidents as tests (#389 etc.) are about velnor-legacy | KEEP after X-gate |
| A01 | checkout | CHANGED | checkout uses: 49 across workflows at tip (not 61); `sparse-checkout` only in ci.yml lint jobs; rust-ci uses fetch-depth 2; velnor-gen `sparse_checkout` default off | REWRITE scope |
| A02 | Artifacts v4 | CHANGED | upload-artifact 10, download-artifact 6 uses | KEEP; recount |
| A03 | `actions/cache` | CHANGED | cache/restore 14, cache/save 9, cache 3 uses; rust-ci has 5 families of restore+save in one job (A§3) | KEEP; key families are tools/sources/target/delta |
| A04 | mise-action | CHANGED | jdx/mise-action 15 uses (not 48); `cache: false` in rust-ci.yml | KEEP |
| A05 | mr-boxington, alint, setup-velnor | CHANGED | mr-boxington 4 uses, setup-velnor 6 (composite shell), alint 1 (only hand-written ci.yml) | KEEP |
| A06 | Attest | CHANGED | no workflow here uses actions/attest; only velnor-gen release.yml for consumers; `gh attestation verify --deny-self-hosted-runners` (crates/velnor-render/src/release_attest.rs:70) | GATE with R01 |
| A07 | App token | CHANGED | create-github-app-token not used in this repo's workflows; only in generated release.yml | DEFER until a consumer exists |
| A08 | Replace github-script | CHANGED | github-script: 5 uses in 3 workflows (release-rust-ci-tools.yml:26, qualification*.yml), and it is in ALLOWED_ACTIONS | KEEP; recount, needs velnor-ci commands + generator change |
| A09 | Closed-set lint | DONE | crates/velnor-render/src/rules.rs:17-34 ALLOWED_ACTIONS (16, SHA-pinned in pinned_actions.rs) enforced at render; set differs from plan (adds docker/login, setup-buildx, build-push, github-script, actions/cache; no alint, no generic attest) | MARK-DONE; reconcile native set with the 16 |
| H01 | Lab | VALID | velnor-lab exists (goals rule 12) | KEEP |
| H02 | Transform and replay | VALID | local/scenarios + `velnor-new` scenario #1718 usable as replay seed | KEEP |
| H03 | Comparator | VALID | `velnor-ci qualification compare` and `velnor-local conformance` exist as partial comparators | KEEP; reuse |
| H04 | Corpus v1 | CHANGED | seed is now .velnor/workflows.toml qualification (64 jobs, hosted vs scale-set pairs; velnor-gen-generated after #1747) + local/conformance probes, not "ported tests" | REWRITE seed |
| H05 | Mutants | VALID | nothing exists | KEEP |
| H06 | Release watcher | VALID | runner.lock pins 2.338.0, comment about 2.337.0 download API (.velnor/runner.lock:17-28) | KEEP |
| H07 | Drift sampler | VALID | nothing exists | KEEP |
| H08 | Flake-aware pairing | VALID | nothing exists | KEEP |
| H09 | Canary PRs | VALID | nothing exists; ruleset has no App-bound check today | KEEP |
| L01 | Lab A | VALID | no App exists (B§6 row 9) | KEEP |
| L02 | `velnor-control` | CHANGED | required check is `Required` from rust-ci.yml, produced by `velnor-ci gate` (R:631-672); no webhook receiver | KEEP; name `Velnor / Verified` vs `Required` must be decided (release-gate reads push-run `Required`) |
| L03 | Fenced status writer | VALID | absent | KEEP |
| L04 | Reconciler | VALID | absent | KEEP |
| L05 | Policy changes and revocation | VALID | absent | KEEP |
| L06 | Shadow and parity | CHANGED | obligation inventory must be derived from velnor-gen plan (job `rust` + policy + ci.yml lints), not 39 jobs | REWRITE |
| L07 | Ruleset step 3 (owner applies) | CHANGED | requires "all three contexts"; only one context exists | REWRITE |
| L08 | Ruleset step 4 | CHANGED | "drop two Actions contexts; delete PR and group Actions gates; B07 interim step": none of those exist in that form | REWRITE |
| G01 | Cohort moves | CHANGED | cohorts name engine jobs (Plan, Ledger CLI, 26 crate jobs, runner-host jobs, state seal, image candidate, source qualification) that no longer exist; today: rust, policy, Required, Cache, rust-trim, ci-run-label, 4 ci.yml lints, qualification, release-rust-ci-tools, rust-cache-trim | REWRITE cohorts |
| G02 | Routing in velnor-gen | CHANGED | hosted labels in crates/velnor-render/src/release.rs:31 (HOSTED_RUNNER) and release_binaries.rs; per-job `runner` overrides partly exist; `--deny-self-hosted-runners` verify rule | REWRITE; N05 |
| G03 | Obligation inventory v1 | CHANGED | "today's 39 jobs" gone; inventory should come from WorkflowPlan/VerdictSpec/`velnor-ci plan` outputs (velnor-model plan.rs, velnor-rust verdict.rs) | REWRITE |
| G05 | `.velnor/` schema | CHANGED | `.velnor/config.toml` is no longer the engine config: after #1747 it holds only [stacks.rust] for velnor-ci plan; generator config is .velnor/rust-ci.toml, unknown keys rejected (velnor-rust config.rs:75-77, config_keys.rs) | REWRITE: new keys via config_keys.rs + CONFIG.md + helper_gate.rs |
| G04 | velnor-gen cutover (#1290) | DONE | #1523 cutover: rust-ci.yml from velnor-gen, `Required` via `velnor-ci gate`, engine ci.yml trimmed; #1747 deleted the engine; design's variant (`Required` = `audit wait`) not built | MARK-DONE for cutover; REWRITE remainder as "Required reports A1" |
| V01 | Ledger core | VALID | no ledger code; velnor-model CacheKey/VerdictSpec and merge-group-passed are the templates (A§7.3) | KEEP; anchor in velnor-model |
| V02 | Observation and signing | VALID | absent | KEEP |
| V03 | Witness scheduler | VALID | absent | KEEP |
| V04 | Exact-tree records | VALID | absent; HD3 (hard-decisions.md:223-229) forbids a second skip mechanism without owner approval (binds engine only, goals.md:14) | GATE: owner approval for reuse |
| V05 | Audit A1 | CHANGED | name collision with `velnor-ci inputs audit`; "Deletes the full push run" must not drop push `Required` (goals rule 13) | REWRITE naming |
| V06 | Audit A2 | VALID | absent | KEEP |
| V07 | Read audit | VALID | absent (eBPF/LSM in node agent) | KEEP |
| V08 | Per-root keys | CHANGED | per-root keys: repo is ONE root workspace now (#1290/#1594); "helper and generator roots" no longer exist; use per-package closure (velnor-ci plan already selects packages) | REWRITE as per-package |
| V09 | Flake sampler | VALID | absent | KEEP |
| V10 | Queue-tip speculation | VALID | merge queue unused today (C§1) | KEEP after queue revived |
| AN01 | `velnord` and `velnor cargo`, count-only | CHANGED | mise.toml:44-46 `[wrappers.cargo] command="mbx"` still there, but velnor-gen emits mbx-store/mbx-seed/mr-boxington around it (#1411); deleting the wrapper conflicts until generator stops emitting | REWRITE; order after generator change |
| AN02 | Tree snapshots and wip refs | VALID | absent | KEEP |
| AN03 | Remote counting lane | VALID | absent | KEEP |
| AN04 | `velnor verify`, `ship`, `resolve`, `wait` | VALID | absent; velnor-local `run` is the nearest (macOS only) | KEEP |
| AN05 | Trust tiers | VALID | absent | KEEP |
| AN06 | Routing by replay | VALID | absent | KEEP |
| AN07 | Flake soak in verify | VALID | absent | KEEP |
| AN08 | Dev runs feed graduation | VALID | absent | KEEP |
| AN10 | Hooks through check names | CHANGED | hk.pkl exists with raw cargo lines | KEEP |
| AN11 | Ejection prediction (lab) | VALID | absent | KEEP |
| AN12 | Failure lookaside and queue-tip stream | VALID | absent | KEEP |
| AN13 | Receipt-coverage parity | VALID | absent | KEEP |
| AN14 | Shared local generation | VALID | absent | KEEP |
| AN09 | Per-test-case receipts (stretch) | VALID | stretch | KEEP |
| V12 | C5 classifier | CHANGED | now_seconds at crates/velnor-ci/src/io/mod.rs:289 and io/budget.rs:241 (design cites :210); count of clock-reading tests unverified | REWRITE refs |
| V14 | No-`.git` census | OBSOLETE | example `root.rs:22` is velnor-actions-orchestrator (deleted #1747); velnor-ci tests call `git rev-parse` in tests/it/plan_diff.rs, plan_main.rs | REWRITE census scope |
| V13 | Per-package keys (stretch) | CHANGED | stretch becomes the default per-package path (see V08) | REWRITE |
| C01 | Tool store | CHANGED | `velnor-ci tools-local` already restores ~/.local/share/mise and ~/.rustup from a node store (R:94-99,124-128) | REWRITE as extension |
| C02 | Crates store | CHANGED | crates-mirror DaemonSet + `velnor-ci crates-mirror` shipped, enabled in rust-ci.toml [crates_mirror] (not "currently unused" as decision 24 says) | REWRITE: per-lockfile registry on top |
| C03 | Git mirror and bundle | VALID | git mirrors absent (B§3.4) | KEEP |
| C04 | XFS volumes | VALID | cluster work, out-of-repo; emptyDir today (B§3.2) | KEEP |
| C05 | Verified generations | CHANGED | PR delta layer (#1107, velnor-ci delta/inputs, `target.velnor-delta`) and `inputs restore` mtime rule already implement part; `mbx_shared_store` | REWRITE: build on delta+inputs |
| C06 | Reproducibility audit | VALID | absent | KEEP |
| C07 | Compile-reuse shadow | VALID | absent | KEEP |
| C08 | mbx-cache head-to-head | CHANGED | baseline changes: `mbx-store` shared node store shipped #1411 | REWRITE baseline |
| C09 | Retire GitHub-cache caching | CHANGED | state seal, cache-prune, seal code gone (#1550/#1420/#1747); what remains is the 4 GitHub cache families in rust-ci.yml and rust-cache-trim.yml | REWRITE |
| C10 | Checkout verification | VALID | absent | KEEP |
| C11 | Upstream mirrors | CHANGED | crates mirror exists; toolchains/tools via tools-local; registry mirror absent | REWRITE |
| ST01 | `velnor-storage` | VALID | absent | KEEP |
| ST02 | Tiers and routes | VALID | absent | KEEP |
| ST03 | Cache index | CONFLICT | goals.md:59 "at most 8 GB of GitHub's 10 GB" and goals "inside GitHub's free limits"; macos spec §8 lists "a new distributed cache" out of scope | GATE |
| S03 | FI volume rebuild | VALID | cluster work, out-of-repo | KEEP |
| ST04 | RustFS per site | VALID | absent | KEEP |
| ST06 | RustFS phase-1 cluster test | VALID | absent | KEEP |
| ST07 | Site replication | VALID | absent | KEEP |
| ST05 | Cache endpoint for Official | VALID | absent | KEEP |
| T02 | Loop splits | OBSOLETE | actions-cli 258-274 s looped tests deleted with engine (#1747); remaining slow: runner-host `receipt_integrity` (crates/velnor-runner-host/.../bootstrap/receipt_integrity.rs) until F01 | REWRITE scope or DROP |
| T03 | Parallel helper tests | CHANGED | ci-helper dir now crates/velnor-ci; check .config/nextest.toml for serial overrides | REWRITE after check |
| T04 | Fixture and profile | OBSOLETE | 89-s fixture = scripts/ci-fixtures.sh, removed by #1747 (rust-ci.toml no longer has the fixture [[test_setup]]) | DROP |
| T05 | Git differential speed | OBSOLETE | non_inputs_differential.rs lived in velnor-actions-orchestrator tests (deleted #1747) | DROP |
| T07 | Split monolithic crates | DONE | generator already split: velnor-gen 2.2k src, velnor-model 2.5k, -rust 13.1k, -render 6.6k, -release, -workflows; velnor-ci still 33.7k src + 42.5k tests | MARK-DONE for gen; REWRITE velnor-ci part with real numbers |
| P08 | Namespace slots (lab) | VALID | lab item | KEEP |
| T08 | Clock injection and fenced fixtures | CHANGED | velnor-ci now_seconds at io/mod.rs:289, io/budget.rs:241 | REWRITE refs |
| T06 | One `tests/it` per package | DONE | velnor-ci, -gen, -local, -release, -cache-rules each have a single tests/it; engine hundreds of test files deleted; remaining multi-binary: runner-host (19+9), others small | MARK-DONE; residual runner-host only |
| S01 | Cluster policy | VALID | cluster policy out-of-repo; ARC pods request 2 CPU burst, no reservations (B§3.2) | KEEP |
| S02 | Control plane | VALID | single k3s server | KEEP |
| R01 | Release jobs on Velnor | CONFLICT | release on self-hosted vs `gh attestation verify --deny-self-hosted-runners` (crates/velnor-render/src/release_attest.rs:70, tests/it/release_yml_attest.rs:124); HOSTED_RUNNER constant; goals rule 13 | GATE: owner decides hosted-attested vs self-hosted releases |
| R02 | One release authority | CHANGED | `velnor-ci release-gate` exists (gate.rs, cli_release.rs) with `--pr-only-check` only; no --app-check/--push-check flags but push-run job evidence is already required (gate.rs:141-172); it refuses non-Actions App checks (gate.rs:295-298); engine eligibility shell deleted; #1608 (open) adds Required-from-push to release-rust-ci-tools gate | REWRITE; merge #1608 first |
| R03 | Smoke shipped bytes | VALID | release-rust-ci-tools only runs --version and check-tag; native arm64 hosted runner, not qemu | KEEP |
| R04 | Build once, promote | CHANGED | tools release rebuilds on tag (A§4.1.5); product release + source preview deleted; runner-v release has no workflow | REWRITE |
| R05 | Release timing | VALID | run P50 4.1 min post-cutover vs 11.2 pre (C§2) | KEEP |
| R06 | Stamp after link (lab) | VALID | lab | KEEP |
| F01 | Final deletion | CONFLICT | deletes crates in release-plz `runner-v` group (release-plz.toml:49+, 8 crates), the new CI lane `local-mode` for velnor-runner-host (#1740, rust-ci.toml [[test_lane]]), `velnor-local` pair mode; goals.md:20-22 "runner developed as a product" | GATE; also edit release-plz.toml, tag rulesets |
| BM01 | S1 planted failures | VALID | baseline paths changed | KEEP |
| BM02 | S2 control race | VALID | ARC baseline out-of-repo | KEEP |
| BM03 | S3 head-to-head harness | CHANGED | "today's CI" and the 85-min proxy are pre-cutover; post-cutover Rust CI P50 1.1 min but red 98% of PR runs (C§0) | REWRITE baseline definition |
| BM10 | RBE arm (optional) | VALID | optional | KEEP |
| BM07 | Strong conventional baseline arm | VALID | baseline | KEEP |
| BM04 | S4 loaded replay | VALID | trace of 2026-11-18 is pre-cutover | KEEP; refresh trace |
| BM05 | S5 census | VALID | absent | KEEP |
| BM06 | S6 warm compile | CHANGED | roots changed (single workspace) | REWRITE |
| BM08 | Floor inventory | CHANGED | engine suites gone; `receipt_integrity` 924.8 s remains until F01 | REWRITE |
| BM09 | Contract freeze and publication | VALID | absent | KEEP |
| BM11 | Toolchain matrix (lab B4+) | VALID | absent | KEEP |
| BM12 | Page-cache contention (lab B2+) | VALID | absent | KEEP |
| BM13 | Per-execution overhead | VALID | absent | KEEP |
| BM14 | GitHub's share and a shadow train | CHANGED | queue not used since 11-19 12:12Z, auto-merge disabled (C§1,§4); needs revival first | REWRITE precondition |
| BM15 | Hardware floor (K11) | VALID | absent | KEEP |
| P09 | Resident planner, pre-cloned sandboxes | VALID | planner today is `velnor-ci plan` per job (rust-plan 16 s, C§3.1) | KEEP; target velnor-ci |
| C13 | Lab B5: narrowed step 3 | VALID | absent | KEEP |
| V16 | Flake scoring | VALID | absent | KEEP |
| S04 | Image-key census | OBSOLETE | image jobs and `Job image candidate` deleted (#1554/#1550) | DROP |
| DG01 | Key struct (Dagger D01) | VALID | CacheKey in velnor-model/src/cache.rs is the nearest typed key | KEEP; reuse |
| DG02 | Content-key telemetry (D02) | VALID | absent | KEEP |
| DG03 | Run step keyed by binary digests (D03) | VALID | absent | KEEP |
| DG04 | Reuse class per obligation (D04) | VALID | absent | KEEP |
| DG05 | Targets (D05) | VALID | absent | KEEP |
| DG06 | Node-local request socket (D06) | VALID | absent | KEEP |
| DG07 | Checks by name (D07) | CHANGED | names must come from velnor-gen step names (`Format`, `Lint`, `Test`, `Check <crate> alone`) | REWRITE |
| DG08 | Generator freshness (D08) | CHANGED | `velnor-gen check` is not a CI step; StepKind::GeneratorCheck exists but no planner builds it (A§1.4); `velnor-ci check-metadata` covers only rust-metadata.json | REWRITE: wire GeneratorCheck first |
| DG09 | Agent environments (D09) | VALID | absent; velnor-local scenarios nearest | KEEP |
| DG10 | Frozen resolution (D10) | VALID | crates mirror/tools-local exist | KEEP |
| DG11 | OTel traces (D11) | CHANGED | `velnor-ci report`/`ci-metrics.json` already exist | REWRITE: extend |
| V15 | Fast-red lane | VALID | absent | KEEP |
| C12 | Twin generations | VALID | absent | KEEP |

### 6.3 Items by action

**MARK-DONE (already on main): 9**

B03, B08, B10, B11, B12, A09, G04, T07, T06

**DROP (obsolete): 7**

B02, B13, B14, X13, T04, T05, S04

**REWRITE (target files, crates or premise moved): 42**

B01, B05, B06, B07, B15, RV03, RV04, P03, O01, A01, H04, L06, L07, L08, G01, G02, G03, G05, V05, V08, AN01, V12, V14, V13, C01, C02, C05, C08, C09, C11, T02, T03, T08, R02, R04, BM03, BM06, BM08, BM14, DG07, DG08, DG11

**GATE (blocked on owner approval of a goals.md change or a policy decision): 22**

P07, X01, X02, X03, X04, X05, X06a, X06b, X07, X08, X09, X10, X11, X12, X14, X15, X16, A06, V04, ST03, R01, F01

**DEFER: 1**

A07

**KEEP (unchanged or with small edits): 93**

B04, RV01, RV02, RV05, P01, P02, P04, P05, P06, O02, O03, O04, X17, A02, A03, A04, A05, A08, H01, H02, H03, H05, H06, H07, H08, H09, L01, L02, L03, L04, L05, V01, V02, V03, V06, V07, V09, V10, AN02, AN03, AN04, AN05, AN06, AN07, AN08, AN10, AN11, AN12, AN13, AN14, AN09, C03, C04, C06, C07, C10, ST01, ST02, S03, ST04, ST06, ST07, ST05, P08, S01, S02, R03, R05, R06, BM01, BM02, BM10, BM07, BM04, BM05, BM09, BM11, BM12, BM13, BM15, P09, C13, V16, DG01, DG02, DG03, DG04, DG05, DG06, DG09, DG10, V15, C12



## 7. Owner decisions and questions

### 7.1 Owner decisions 1-24 (owner-decisions.md)

| # | Decision | Status | Evidence | Note |
|---|---|---|---|---|
| 1 | Rust-only executor, official runner never used | CONFLICT | goals.md:16; macos spec §3, §8 | Needs goals approval (Q-N1); amended by 20 |
| 2 | Official runner only as oracle | CONFLICT | same; superseded by 19/20 | Same |
| 3 | Latest APIs only | VALID | none | — |
| 4 | No Node; closed action set | CHANGED | ALLOWED_ACTIONS has JS actions and docker/* (rules.rs:17-34) | Reconcile set |
| 5 | velnor-legacy stays archived | VALID | not in tree | — |
| 6 | Experimental deps fine | VALID | mbx, turso in use | — |
| 7 | GitHub stays the platform | VALID | — | — |
| 8 | No fork PRs, drop fork handling | CONFLICT | goals.md:43; fork fence #1328; public repo | Q-N6 |
| 9 | macOS release builds paused | DONE | #1550; velnor-ci no macOS build (setup-velnor action.yml) | Runner/local on macOS stay (macos spec is the active runner spec) |
| 10 | Build once, verify once, promote | VALID | HD8 agrees for images; ledger needs HD3 approval | Q-N8 |
| 11 | Correctness first, zero false greens | VALID | aligns with goals "never compromise 1,2" | — |
| 12 | Label flow (steps 1-5, merge only on green latest commit) | CONFLICT | section 1 | Q-N2 |
| 13 | Kubernetes + Docker platform trait | CONFLICT | macos spec §8 K8s out of scope; #1324 | Q-N1 |
| 14 | velnor-legacy is very old; rewrite | VALID | — | — |
| 15 | Agents do all work; size by real constraint | CHANGED | roadmap.md unattended Claude Code loop already exists | Align |
| 16 | Per-job-type migration, no moving back | CHANGED | job types replaced (G01) | Re-derive cohorts |
| 17 | Always review; reuse only if well designed | VALID | — | — |
| 18 | Migrate and delete; no fallbacks | DONE (engine part) | #1747 | Matches AGENTS.md "No legacy code" |
| 19 | Official runner kept as reference executor | VALID | consistent with goals.md:16 | Strongest-agreement item |
| 20 | Two executors, Native default | CONFLICT | decision 1 | Q-N1 |
| 21 | Extensible platform abstraction | VALID | design only | — |
| 22 | Native `actions/cache`, L0/L1/L2, RustFS | CONFLICT | goals.md:59; macos spec §8 | Q-N1 |
| 23 | Configurable storage | CONFLICT | same | Q-N1 |
| 24 | Node-local repo caches and mirrors | CHANGED | crates mirror is already used by this repo (rust-ci.toml `[crates_mirror]`); decision text says "currently unused" | Update text |

### 7.2 Owner questions D1-D26 (owner-questions.md; 21 listed open)

| Id | Status | Evidence | Action |
|---|---|---|---|
| D1 release engine 0.1.81 | OBSOLETE | #1345 merged; engine deleted #1747 | Remove |
| D9 pre-approve CI-only engine releases | OBSOLETE | engine gone; tools release is release-plz `tools-v` | Remove |
| D14 delete engine crates | DONE | #1747 deleted 12 crates; `.velnor/config.toml` `[stacks.rust]` kept for `velnor-ci plan --mode` | Remove; open N14 |
| D3 first ruleset update | CHANGED | live ruleset facts in section 0 item 3; JSON lists `Required verdict` | REWORD: `Required` only, bind 15368, bypass [], ALLGREEN, plus precondition "main green" |
| D2 auto-merge and no catch-up merges | CHANGED | `allow_auto_merge=false` (a repo setting the owner must flip); queue unused; 157/173 admin merges | REWORD: ask auto-merge on, queue on, bypass off |
| D8 deploy identity and node changes | CHANGED | no deploy dir; ARC manifests live in an outside checkout `~/Projects/tailrocks/velnor-runners` (evidence §B) | REWORD: also decide where cluster manifests live |
| D11 cutover counts per type | CHANGED | cohort job names gone | REWORD after G01 |
| D16 label carried from draft arms the PR | CHANGED | superseded by the full label-flow question (section 1); today `ci:run`/`ci:auto` | REWORD into Q-N2 |
| D7 `velnor-verify` App + agent identity | VALID | none exist | Keep; add: does `Required` stay the gate until the App ships? |
| D15 bad-release recovery | VALID | no break-glass verifier exists; admin merge is today's de facto recovery | Keep |
| D4 evaluator A or B | VALID | — | Keep; only after Q-N1 |
| D17 who approves a reduction | VALID | — | Keep |
| D19, D20, D21, D22, D23, D24, D25, D26 | VALID | no repo dependency | Keep |
| D18 merge train | CHANGED | needs GitHub's queue to be in use to measure (BM14) | Keep, add precondition |
| D5, D6, D10, D12 "settled" | VALID | D10: ALLGREEN settled but ruleset is HEADGREEN and queue unused | Note |
| D13 "park old runner PRs" settled by decision 16 | CHANGED | those PRs merged; runner-host hardening continued (#1263, journal v58, #1740) | RE-ASK |

### 7.3 Questions the plan does not ask but must

| New | Question | Why now | Evidence |
|---|---|---|---|
| Q-N1 | Approve rewriting goals.md (lines 16, 20-22, 43, 59, 75) and the macOS runner spec (§3, §8) to allow a native executor, Kubernetes, ARC deletion, a native cache and no-fork rule? | `docs/goals.md:3`, AGENTS.md: never silently change; every X, ST, P04, F01 item depends on it | section 2 |
| Q-N2 | The label flow: name (`velnor:verify` or keep `ci:auto`), retire one-shot `ci:run`, is a quick set on unlabelled PRs allowed, must an unlabelled ready PR show a red `Required`? | main's `Required` is green on unlabelled PRs | section 1 |
| Q-N3 | Retire admin bypass and bind `Required` to integration 15368 now, accepting a blocked main until it is green? | 157/173 merges bypassed | C§1,§4 |
| Q-N4 | Is the merge queue part of the flow (it is configured but unused, `Rust CI` has never produced a merge_group run)? | owner step 7; BM14, V10, D18 depend on it | C§1 |
| Q-N5 | Releases on self-hosted Velnor, hosted-attested, or both? Accept dropping `--deny-self-hosted-runners`? | R01, cohort 4 | release_attest.rs:70 |
| Q-N6 | Fork PRs: drop handling (decision 8) or keep the fence (goals.md:43)? Repo is public | decision 8 vs goals | R:57 |
| Q-N7 | Lanes as parallel jobs with separate targets (design §8.1) or one job per workspace (goals rules 4-5)? | cache key design and delta layer assume one target | A§2.4 |
| Q-N8 | Allow a verification-result ledger given HD3's ban on a second skip mechanism? | V04, V10, DG03 | hard-decisions.md:223-229 |
| Q-N9 | Runner-host policy until F01: continue hardening (current practice) or freeze? | D13 premise is false | B§5.2 |
| Q-N10 | Evidence home: keep the 40k-word plan under docs/ despite the evergreen rule, or move to issues? | AGENTS.md | C12 |
| Q-N11 | Backlog executors: Claude-only (matches `exec:*` labels) or add other vendors to roadmap.md? | C13 | roadmap.md |

## 8. Items the plan is missing (given velnor-gen, velnor-ci and the cutover)

| New | Title | Why / evidence | Where in the code |
|---|---|---|---|
| N01 | Restore a green `main` and a trustworthy `Required` before any ruleset step | 226/268 PR runs red; causes in a 60-run sample: `velnor-ci plan` step failures (33, e.g. helper-install SHA-256 mismatch), `velnor-ci gate` (36), lint (about 58 across lanes), `Cache: Check cache waste` (17), Alint 400-line cap, cargo-deny RUSTSEC-2026-0543 (#1218) (C§2); #1555 open | issues #1555, #1218; `.alint.yml` |
| N02 | Label-required PR mode in velnor-gen | section 1 | `PrRun`, `plan_budget.rs`, `required_job.rs`, `generator/CONFIG.md`, goldens |
| N03 | Bind `Required` to integration 15368, empty bypass, revive queue, enable auto-merge | live ruleset; settings | `gh api` ruleset 24396608; repo settings |
| N04 | Wire `velnor-gen check` into CI (GeneratorCheck step is dead code) | A§1.4,§9.2: freshness relies on out-of-repo scripts (`regen-main.sh`) | `StepKind::GeneratorCheck` (`velnor-model/src/steps.rs:108`), planner |
| N05 | Release policy item: hosted vs self-hosted, attest verification rule | C10 | `release.rs:31`, `release_attest.rs:70` |
| N06 | Merge #1608 (release gate requires `Required` from the push run) and extend to the queue | goals rule 13 not met (A§4.1,§8); D§1 recommends rebase and finish | PR #1608 |
| N07 | A workflow for the `runner-v` release group | A§9.1: release-plz.toml has the group but no workflow builds or publishes it; F01 would delete its crates | `release-plz.toml:49+`, `release-rust-ci-tools.yml` |
| N08 | Goals/spec amendment PR (owner-approved) | section 2 | `docs/goals.md`, `docs/proposed/macos-scaleset-runner.md`, `docs/deferred/*` |
| N09 | Re-baseline: one measurement run on a green main (PR verdict, push-to-merge net of admin bypass) | C§6 caveats: post-cutover sample 4.3 h and red | `velnor-ci bench`, `timings`, `[budget]` |
| N10 | Reuse the existing measurement tooling in the BM harness | `velnor-ci bench collect/compare/budget`, `timings --repo`, `report`, `ci-metrics.json` exist | A§1.1,§9.11 |
| N11 | Decide where cluster and ARC manifests live in the repo | B§0.2 (no `deploy/`), X15, P04, S01-S03 | new dir or a second repo |
| N12 | Reconcile `docs/proposed/kubernetes-runtime.md` with the architecture (journal kept vs deleted) | B§6.1 | #1324 |
| N13 | Close the Cache job gap: `Cache` is advisory so a red `Cache` does not block, yet fails runs (17 sampled) | goals rule 3 says visible, does not block | R:687-726 |
| N14 | Retire `.velnor/config.toml [stacks.rust]` and the ported engine ops in velnor-ci (`plan --mode`, `write-request`, `resolve-qualification`, `image`, `admit-preview`, `run-authority`, `qualification compare`) | #1747 kept the file only for `plan_request/diff/config.rs`; ports exist for deleted workflows (A§1.1) | `crates/velnor-ci/src/plan_request*` |
| N15 | Per-package keys as the default (single workspace) and the `[[workspace_groups]]` interaction (#1274 merged) | V08 premise gone | `velnor-rust/src/groups.rs` (post-tip) |
| N16 | Naming ADR for crates, binaries and subcommands (audit, ledger, verify, check, run) | section 3 item 9 | — |
| N17 | Duplicate policy lanes: `ci.yml` Cargo Deny/Machete and rust-ci `policy`, serialised behind `rust` | A§5.1 | `ci.yml:68-134`, R:466-565 |
| N18 | `fetch-depth: 2` degrades multi-commit pushes and multi-PR groups to "select everything" (inference) | A§2.3 step 4, §9.9 | `velnor-ci/src/io/git.rs` |
| N19 | Make the PR delta layer measurable (no timing exists) before native cache work | A§3.2 "No timing measured" | #1107, #1742 |
| N20 | A-set reconciliation: native action list vs `ALLOWED_ACTIONS` | section 4 §5.6 | `rules.rs:17-34` |
| N21 | Evidence hygiene: fix dead links, move dated numbers out of docs/proposed or label them snapshot | section 0 item 9, C12 | docs |
| N22 | velnor-local scenario `velnor-new` has no recorded run; record one and use it as conformance seed | B§2.4 | local/scenarios |
| N23 | Tag/ruleset impact of the plan: `protect-tags`, `release-tags` rulesets, tag-ruleset-drift job, App 5762496 for any new release identity | A§1.5 | `release-rust-ci-tools.yml:16-85` |
| N24 | Decide whether to keep goals' 500 000-byte workflow cap and the delta layer relief valve when per-obligation checks arrive | A§1.2 | `velnor-render/src/rules.rs` |

## 9. Suggested order of work

1. N01 (green main), #1608, N04 (generator check in CI), N03 (bind `Required`, auto-merge, queue). These are repo/ruleset work with no owner design dependency except Q-N3/Q-N4.
2. N02 label mode (Q-N2) in velnor-gen; update docs and goldens.
3. N09/N10: a clean baseline with existing velnor-ci tooling.
4. Q-N1 and N08: one owner decision pack on goals.md. Nothing in X, ST, P, F, O, H (executor parts) can start before this.
5. Only then: RV01-RV05 reviews, platform trait, ledger on velnor-model types, enable `skip_push_after_merge_group` as first reuse, verification ledger behind shadow mode.
6. F01 last, with `release-plz.toml` runner group, rulesets and the `local-mode` CI lane updated in the same change.

## 10. What I did not verify

- Cluster, ARC and node facts (evidence.md §B, architecture §9) are out of repo; they are carried as snapshot facts from 2026-11-18.
- velnor-legacy facts (line counts, tests, incident numbers) are outside the tree and were not re-read.
- Line numbers cited by the plan for `inputs.rs:102-151`, `gate.rs:84-94,277-288,354-376`, and the count of clock-reading test files were not re-derived; I confirmed only that the files exist and where `now_seconds` lives.
- Post-tip state (`57b65af0db`) was read through `gh api` only: crate and file lists, `.velnor/rust-ci.toml`, `.velnor/config.toml` head, `ci.yml` job names, and the #1747 commit message. The audited checkout was not advanced.
- Whether `ci:run`/`ci:auto` are applied by humans or agents, and whether `Alint`, `Cargo Deny`, `Cargo Machete`, `Jactionlint` are required, are not knowable from the repo (the live ruleset lists only `Required`).
- Admin-bypass attribution (157 of 173) is C's inference; GitHub does not expose the bypass flag through these endpoints.
