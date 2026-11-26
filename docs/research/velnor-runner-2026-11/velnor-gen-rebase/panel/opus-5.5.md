# velnor-gen rebase by Claude Opus 5.5

| Field | Value |
|---|---|
| Model | Claude Opus 5.5 (`claude-opus-5-5`) |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | high |
| Date | 2026-11-20 |
| Main tip analysed | `0eb2d76a5c` |
| Brief | [velnor-gen rebase brief v1](../briefs/velnor-gen-brief-v1.md) |

Independent answer from the Velnor runner research panel, written without seeing other members' answers. Unedited except for this header and replacing home-directory paths with `~`.

---

# Velnor plan rebased on velnor-gen (panel member opus-5.5, 2026-11-20)

Checkout: `~/.cache/ci-ambition/main-tip` at `0eb2d76a5c` (#1748). `R` = `.github/workflows/rust-ci.yml` (666 lines at the tip, not the 726 lines A cites). `C/` = `crates/`. Live GitHub facts come from read-only `gh` calls made on 2026-11-20 (runs `44584861409`, `44584072510`, `44583204159`, `44577659465`; `gh pr list`). "[M]" = measured, "[C]" = read in code, "[E]" = estimate or inference, "[L]" = needs a lab proof before anyone relies on it.

---

## 1. Verdict in ten lines

1. The final design still stands, and velnor-gen makes its correctness half easier and cheaper. The plan's run-time half (Velnor Native, ARC deletion, RustFS, native cache) is still blocked by `docs/goals.md:16,20,45,59,75`. It is off the critical path.
2. The generator owns the obligation list (`WorkflowPlan` → `VerdictSpec`, `C/velnor-model/src/verdict.rs:64-75`), the selection (`velnor_rust::select`), the gate (`velnor_rust::decide`, `C/velnor-rust/src/verdict.rs:278`) and the one proof-based reuse that exists (`merge-group-passed`, `C/velnor-ci/src/merge_group.rs:46-86`). The ledger, label flow and reuse belong there. They do not belong in a new `executor/` root or in the App.
3. Main is red **by construction, not by flake**. The released helper pair cannot run tests. `velnor-ci inputs build` inserts `--no-run` into the generated nextest argv (`C/velnor-ci/src/io/audit/build/invocation.rs:77-86`). That argv carries `--no-fail-fast` (`C/velnor-render/src/cargo.rs:295`, R:282), and nextest refuses the pair: "the argument '--no-fail-fast' cannot be used with '--no-run'" (run 44584861409, main tip) [M]. So every labelled PR run, queue run and push run that selects tests is red. That fully explains the admin-bypass culture.
4. `Required` also goes green without verification on four paths: an unlabelled ready PR, a draft, the next push after a consumed `ci:run`, and (likely, [L]) any unrelated label or title edit. In the last case the `Required` job is skipped on the head SHA (`generator/CONFIG.md:553`), and GitHub counts a skipped check as passing.
5. **The single most important change:** phase 0 changes from "apply the ruleset first" to a strict order. First make `Required` mean "this exact commit was verified": fix the preflight bug, then add a `pr_ci = "label"` mode in which `Required` is never green or skipped without a verdict for the head SHA. Only then bind the ruleset, empty the bypass list, and turn on the queue and auto-merge.
6. The verification ledger starts as **verdict records proved through the GitHub API**, which is the `merge-group-passed` pattern generalised and keyed by git tree id. It does not start as a signed-receipt service. Tree-equal reuse from PR to queue to push needs no App, no new storage and no goals change.
7. The `Velnor / Verified` App moves to phase 2. Its real value is not label handling. It lets the gate come from the base branch's rules rather than the PR's own workflow: today a PR can edit `rust-ci.yml` and weaken its own gate.
8. C's speed numbers describe a job graph that no longer exists. The one-job shape landed at `23bed02e7e` (17:17Z), 33 minutes before C's window closed. A green, test-running baseline of today's workflow has never existed. Targets are re-derived as estimates and must be re-measured in W9.
9. On the new baseline the biggest levers are, in order: a warm cache, which only a green main can save; queue reuse; removing the serial post-merge tail (18 alone-checks ≈ 4.5 min, docs ≈ 1.5 min, lane guard ≈ 1.5 min [M]); mapping unplaced paths, since about 40% of recent commits select everything [M]; and test shape.
10. Be ambitious where the generator is unique. It can emit the verification contract, compute verdict keys at plan time, drive local and cluster runs from one plan, and shrink "GitHub-Actions-compatible" to "compatible with the dialect velnor-render can emit". That shrink cuts the Native executor's scope by most of X06–X12.

---

## 2. The design mapped onto velnor-gen

Pipeline anchors: `RepoView` → `detect()` → `RepoFacts` → `plan()` (`C/velnor-rust/src/plan.rs:40-115`) → `WorkflowPlan{jobs: Vec<Job{steps: Vec<Step{kind: StepKind}>}>}` (`C/velnor-model/src/plan.rs:22-113`) → `velnor-render` → sealed YAML. At run time, every `run:` is one `velnor-ci` command (R:84-342). Generator-time extension points are `config_keys.rs` + `RawConfig`, `StepKind`/`HelperCommand` (`C/velnor-model/src/steps.rs:29-167`), `PrRun` (`plan.rs:119-128`), `GateNeed`/`PlanFrom` (`plan.rs:178-231`), `VerdictSpec`, the renderer match (`C/velnor-render/src/steps.rs:279`) and the helper version gate (`C/velnor-gen/src/helper_gate.rs:8-40`).

| Design component | Where it lives now | Types and functions to extend | What velnor-gen already gives |
|---|---|---|---|
| **Verdict keys** (arch §6.1) | Generator-time: the *definition* half (obligations, commands, profiles, helper digest, toolchain pins) is fixed once `rust-ci.yml` is sealed. Its digest is already the header line 2 (R:2; `C/velnor-render/src/header.rs:23-45`). Run-time: the *input* half (tree id, selection, runner image) is computed in `velnor-ci plan` | New `VerdictKey{definition: WorkflowDigest, helper: HelperDigest, image: ImageDigest, executor: ExecutorId, inputs: Inputs::{Tree(TreeId), Packages(BTreeMap<PackageName, TreeId>)}}` in `velnor-model` next to `CacheKey` (`cache.rs:78-97`), with one renderer as for `render_key`. `velnor-ci plan` writes `verdict_key` as an output | Typed keys rendered in one place; sealed workflow digest; pinned helper SHA-256 (`.velnor/rust-ci.lock`); `lock_root`/`compat` hashes; per-package selection with reasons (`selection.rs:64-84`) |
| **Verification ledger** (arch §6.2) | Phase 1: run-time only. Records are GitHub check runs plus the `ci-metrics-*` artifact of a run whose `Required` passed, read through the Actions API like `merge-group-passed`. Phase 2: an append-only store, once a service exists | `velnor-ci verdict lookup --key … --sha …` and `verdict record` (new `HelperCommand::VerdictLookup`), `helper_gate.rs` minimum 0.1.96. Generalise `Step.skip_after_merge_group: Option<PlanFrom>` (`plan.rs:110`) to `Step.reuse: Option<ReusePoint>` with `ReusePoint::{SameSha, SameTree, QueuePassed}` | `merge_group.rs:46-86` already holds "write `skip=true` only on proof, every error is `skip=false`"; `velnor-rust/src/merge_group_skip.rs:32-58` already threads a plan output into step `if:`s |
| **Gate / verdict** | Run-time `velnor-ci gate` → `velnor_rust::decide` (`verdict.rs:278-319`); generator-time `verdict_job` (`C/velnor-rust/src/required_job.rs:105-150`) | `decide` gains `Outcome::Reused{record, key}`, accepted only when the key equals the plan's key. A `FailWhenANeedFailed` replacement emits "not verified" (section 3 #3) | "Green means verified" already enforced: a skipped scheduled job fails; `tests_run`/`listed`/`by_package` must match (`verdict.rs:207-275`) |
| **`Velnor / Verified` App status** | Not now. The Actions `Required` (from `required_check`, `.velnor/rust-ci.toml:5`) stays the gate. `velnor-ci release-gate` already refuses a non-Actions App check (`C/velnor-ci/src/gate.rs:292-298`) | Phase 2: `velnor-control` computes the verdict from the **base** branch's plan and writes the status. `release-gate` learns the App id then | — |
| **Label flow** (owner decision 12) | Generator-time: `PrCi`/`PrRun`, `quick()`/`full()` (`C/velnor-rust/src/plan.rs:330-343`), `pr_parts` (`C/velnor-render/src/gate.rs:254-270`), triggers (`plan_budget.rs`), concurrency `CancelOlderPullRequestRunsExceptUnrelatedLabels`. Run-time: `gate` + `verdict lookup` | Rewrite `PrCi::Label` (migrate and delete; no third value). Add `unlabeled`, drop `edited`, delete `ci:run`/`ClearRunLabel`/`ci-run-label` (R:613-625). Make `Required` `When::Always` with no skip path | Label conditions, the per-label concurrency suffix, the draft guard and `[[lint]]` gating all exist and are golden-tested |
| **Reuse: agent → PR → queue → main → release** | Run-time decision in `velnor-ci plan` after `select_event` (`C/velnor-rust/src/select_event.rs:20-32`); per-step skip from a plan output | `ReusePoint` above; new `AllTrigger`/`SelectionReason::Reused(key)` so `--explain` names the record; delta layer source identity (#1107) as the agent-to-PR bridge | `merge-group-passed`, delta layer, `inputs save/restore` blob ids, MBX CAS, `skip_push` handling inside `decide` |
| **Release promotion by digest** | Generator-time: `velnor-render/src/release*.rs` (generated `release.yml` with `velnor-ci release-gate`, `release.rs:126`). It is unused here; the tools release is hand-written (`release-rust-ci-tools.yml`) and rebuilds on hosted runners | `.velnor/release.toml` for this repo (#733); a `StepKind::ReleaseBuild` on the default-branch push keyed by `lock_root`+`compat`+release profile; the tag job promotes the stored artifact after `release-gate` | Release model, digest files, `release-gate` with push-run evidence (`gate.rs:57-172`), tag rulesets and drift job |
| **`velnor verify` / `velnor cargo` / `velnord`** | Runner-side plus a new render target. The command graph is the `WorkflowPlan`; `velnor-local` (`C/velnor-local`) already runs generated workflows on macOS | `velnor-gen plan --json` (export the plan); `velnor-render::local`; results are T0 advisory and never fill a verdict record | One plan, one selection library shared by generator and helper (`velnor-ci/src/io/plan.rs:37-90`) |
| **Velnor Native vs GitHub Official** | Runner-side, gated by goals.md:16 (Q-N1). Generator-side it is only `runner`/`fork_runner` labels (`velnor-model/src/ids.rs:99-101`) | `ExecutorId` inside `VerdictKey`; a `[runner] executor = "native"\|"official"` key only after Q-N1 | A closed, SHA-pinned action list of 16 (`velnor-render/src/rules.rs:17-34`), one-command `run:` (`rules.rs:44-45`), a finite expression vocabulary in `gate.rs`/`events.rs`. See section 8.4 |
| **Platform trait** | Runner-side (`velnor-runner-host`, bollard, ~50 files, B§3.1). Generator does not see it | Unchanged by velnor-gen; reconcile `docs/proposed/kubernetes-runtime.md` vs arch §5.10 (N12) | Node capabilities reach jobs as env (`VELNOR_MBX_SHARED_DIR`, `VELNOR_MBX_SEED_DIR`), checked by the helper (`C/velnor-ci/src/mbx_store.rs:12-37`) |
| **Storage (NVMe/RustFS)** | Runner-side; gated by goals.md:45,59 (Q-N1) | — | 8 GB budget manager (`velnor-ci cache budget`, R:~560), four typed families |
| **actions/cache server** | Runner-side; `velnor-cache-rules` + `velnor-local cache-server` are the spec | A server only has to serve the 4 families velnor-gen emits for velnor-new (`CacheKey::{Tools,Sources,Target,Delta}`) | Single key renderer; conformance probes (`local/conformance/*.json`) |
| **Git and crate mirrors** | Crates: **done** (`[crates_mirror]`, `StepKind::CratesMirror`, helper ≥ 0.1.93, R:91-93). Git: absent | Git mirror follows the crates-mirror pattern: `Checkout{depth, sparse}` (`steps.rs:30-33`) gains `mirror`, rendered as one `velnor-ci checkout` command that verifies `HEAD^{tree}` | The pattern: config table, helper gate, self-hosted-only condition, loud failure |
| **Warm snapshots** | Today: `target` family restore + `inputs restore` mtimes + PR delta; node store only for tools (`tools-local`) and MBX | A `velnor-ci target-local` beside `tools-local`, keyed by the same target key | Mtime restoration (`docs/rust-ci-cache-keys.md:38,57`), `restored` expiry, the `Cache` waste check |
| **mbx** | `mise.toml:47-48` cargo wrapper; generator emits `mbx-store`/`mbx-seed` (R:83-87) and optional `mbx_remote` | Keep. AN01 (`velnor cargo`) must not take the wrapper slot until the generator stops emitting mbx steps | Trusted-event-only shared store (`mbx_store.rs:28-37`) |

### 2.1 Type sketch: the minimum that makes the ledger real (phase 1)

```rust
// velnor-model/src/verdict_key.rs (new), beside cache.rs and verdict.rs
pub struct WorkflowDigest(Sha256);      // header line 2 of the sealed workflow (header.rs:23-34)
pub struct HelperDigest(Sha256);        // [velnor-ci-binary] in .velnor/rust-ci.lock
pub struct ImageDigest(String);         // runner image id the job ran in; "hosted:<ImageOS>/<ImageVersion>" on GitHub
pub enum ExecutorId { GithubOfficial { version: String }, VelnorNative { version: String } }
pub enum VerdictInputs { Tree(GitTreeId), Packages(BTreeMap<PackageName, GitTreeId>) }
pub struct VerdictKey { pub definition: WorkflowDigest, pub helper: HelperDigest,
                        pub image: ImageDigest, pub executor: ExecutorId, pub inputs: VerdictInputs }
pub enum ReusePoint { SameSha, SameTree, QueuePassed }      // replaces Step.skip_after_merge_group
pub enum Outcome { Ran(JobResult), Reused { run_id: u64, key: VerdictKey } }
```

- `velnor-ci plan` computes `VerdictKey` and writes `verdict_key=<hex>` to `$GITHUB_OUTPUT`, beside `rust_root_mode` (`C/velnor-ci/src/plan.rs:19-63`).
- `velnor-ci verdict lookup` searches the runs API for completed runs of the same workflow path. Accepted are a `pull_request` run with the same `head_sha` (SameSha), a `pull_request` run whose recorded key equals ours (SameTree), or a `merge_group` run with `head_sha == GITHUB_SHA` (QueuePassed).
- It reads that run's `ci-metrics-rust` artifact for the recorded key, and its `Required` check run for conclusion `success`. It writes `reused=<point>` only on proof. Every error writes `reused=none`, exactly as `merge_group.rs:46-47` does.
- `decide` (`C/velnor-rust/src/verdict.rs:278`) accepts `Outcome::Reused` for a scheduled job only when `key == plan.key`. `tests_expected` is then satisfied by the *recorded* `tests_by_package`, never by an empty count.

**Soundness case for each reuse point** (owner decision 11's four parts):

| Point | (1) Key complete because | (2) Failure if not | (3) Detected by | (4) Automatic fallback |
|---|---|---|---|---|
| SameSha (label churn, unlabel, re-run) | Same commit, same workflow digest (in the commit), same helper (pinned in the commit), same image recorded | Runner image changed between runs; C5 test (clock, network) | `image` in key; C5 obligations marked `never` reuse | Lookup misses, so the run executes |
| SameTree (rebase-only push; queue group = PR merge tree) | The tree holds every repository input, including workflow, lock files and toolchain file; helper and image in key | A non-tree input: the runner kernel, an unpinned network fetch (cargo runs `--locked` through a checksum-verified mirror) | The push run on main re-executes every tree that was reused in the queue (one reuse hop per landing); a mismatch is an incident | Any audit mismatch writes `reuse = off` via a repo variable that the lookup reads first; owner re-enables |
| QueuePassed (push after queue) | `merge_group` head SHA = pushed SHA (squash queue, fast-forward) | A queue run that skipped a check the push needs (gap #7) | Golden rule from W8: skip only steps the queue ran | `skip_push=false`; full push |

---

## 3. Correctness first: the gaps on main, ranked

Ranked by how directly each one produces a false green or blocks any green. "Owner" means it needs the owner.

| # | Gap (evidence) | Effect | Fix and where it goes | Owner? |
|---|---|---|---|---|
| 1 | **Tests cannot run.** `inputs build` builds a preflight by inserting `--no-run` before `--` into the run argv (`C/velnor-ci/src/io/audit/build/invocation.rs:77-86`). The generated Test argv has `--no-fail-fast --status-level pass -E … --no-tests=pass` (`C/velnor-render/src/cargo.rs:295`; R:282, also the local-mode lane after R:283). nextest 0.9.152 (`mise.toml:24`) refuses the pair (run 44584861409 "Test" and "Test (features: local-mode)") [M] | No run that selects tests can pass: labelled PRs, queue, push, nightly. The 4 green push runs and 2 green PR runs in C's sample were regen/docs-only. This, not flakiness, is why `Required` is red and merges bypass | Build the preflight positively. Parse the nextest argv into cargo build options (`--workspace`, `-p`, `--features`, `--locked`, `--cargo-profile`, `--target`) and emit `cargo nextest run --no-run <those>`; never copy run-only flags. Add a contract test: for every golden sample, every `inputs build -- <argv>` yields a preflight that nextest's clap accepts. Release 0.1.96, pin, regenerate. **W1** | No |
| 2 | **Clippy red at the tip:** unused import `OsRelease`, dead variant `Platform` in `velnor-ci` test `it` (run 44584861409 Lint), fallout of the parity tests #1747 deleted | Lint red on every event | Delete the leftovers. **W1** | No |
| 3 | **`Required` green without verification.** (a) Unlabelled ready PR: the gate step is skipped and only `FailWhenANeedFailed` runs, so the quick set passing makes `Required` green (R:599-612; `required_job.rs:132-136`). (b) Draft: the job runs a notice and passes (R:582-584). (c) `ci:run` is removed at run start (R:613-625), so the next push gets the quick set and is green without tests. (d) An unrelated label, or an `edited` without a base change, skips `Required` on the head SHA (R:576, `generator/CONFIG.md:553`). GitHub treats a skipped job as success, and the newest check run of a name is what the rule reads, so a red PR can turn mergeable by adding a label [L: prove on velnor-lab, 1 agent-hour]. (e) Fork PR: quick set only, green | Violates owner decision 12 step 6 and goals.md:41 | Rewrite `pr_ci = "label"`: (1) triggers `opened, synchronize, reopened, ready_for_review, labeled, unlabeled`, no `edited`; (2) one label (owner names it; `ci:run` and `ci-run-label` deleted); (3) `Required` always runs on PR events and passes only if this run executed the full gate, or if `velnor-ci verdict lookup` proves a passing full verdict for the **same head SHA and same workflow digest**; otherwise it fails with "not verified: add <label>"; (4) drafts fail the same way; (5) `unlabeled` joins the PR concurrency group, so it cancels the in-flight run, and its `Required` fails unless the SHA already has a verdict. Lives in `PrRun`, `verdict_job`, `pr_parts`, `plan_budget::triggers`, a new `HelperCommand::VerdictLookup`, `generator/CONFIG.md:539-555`, `docs/rust-ci.md:76-90`. **W3** | Label name; whether the quick set runs on unlabelled PRs (Q-N2) |
| 4 | **Admin bypass and an unbound check.** 157–167 of 173 post-cutover merges had a failing `Required` (C§1). `protect-main` requires `Required` with no `integration_id` and bypass = RepositoryRole 5 (C§4) | Unverified code lands; any token with `statuses: write` or `checks: write` can post `Required` | Owner edits the ruleset: `integration_id: 15368`, `bypass_actors: []`. Do this only after W1 and W3 merge (otherwise every PR deadlocks). The two PRs that fix the gate are the last admin merges, recorded in their descriptions | **Yes** (Q-N3) |
| 5 | **The queue is unused and `allow_auto_merge = false`** (C§1, §4) | Nothing verifies the merged tree; the HEADGREEN queue rule is dead weight | Owner sets auto-merge on, keeps the queue, sets ALLGREEN, `min_entries_to_merge_wait_minutes` 5 → 0 (C§4 shows 5) | **Yes** (Q-N4, Q-N14) |
| 6 | **A PR can weaken its own gate.** `pull_request` and `merge_group` runs execute the workflow from the PR's tree, and the header digest is unkeyed (`header.rs:20-22`). An agent that "fixes" red CI by editing `rust-ci.yml` or `rust-ci.toml` passes its own gate. Config weakenings appear only as `--explain` warnings (`C/velnor-ci/src/io/plan.rs:60-89`) | False green by an honest but buggy agent: the realistic threat under owner decision 8 | Phase 1: `velnor-ci gate` reads the base's sealed `rust-ci.yml` (`git show HEAD^1:…`), parses its `GATE_SPEC`, and fails when the head's obligation set or test coverage is smaller, unless the PR carries an owner-signed reduction (D17). Phase 2: the gate's rules come from the base branch (ruleset "required workflows" pinned to a ref [L], or the App) | D17 (who approves a reduction); Q-N12 |
| 7 | **Post-merge-only checks.** `docs = "post_merge"` and `check_alone = "post_merge"` (`.velnor/rust-ci.toml:7-8`; `C/velnor-rust/src/docs_post_merge.rs:7-16`) run `cargo doc` and the 18 alone-checks only on push and schedule. A break lands first and is found after. Worse, enabling `skip_push_after_merge_group` would skip them for good, because `merge_group_skip::is_check` includes `Doc`, `DocFeatures` and `CheckAlone` (`merge_group_skip.rs:9-27`) although the queue never ran them | Doc and feature-unification breaks land on main; a dormant flag would remove them entirely | Change the meaning of `post_merge` to "in the merge queue, and on a push without a passing queue run". In `docs_post_merge.rs`, `When::NotCancelledPostMerge` adds `merge_group`. In `merge_group_skip.rs`, skip only steps the queue run actually ran (compute the set from `When`, not a fixed list). Golden test: no step is skipped on push unless its `When` includes `merge_group`. **W8** | No |
| 8 | **Release gate checks ancestry only** (`release-rust-ci-tools.yml:86-105`) vs goals.md:77. #1608 fixes it with 13 lines of shell in YAML (breaks rule 9, goals.md:73) and edits paths #1747 deleted (`fixtures/opentofu-goldens/*`, `generator/crates/velnor-gen/tests/it/release_workflow.rs`) | A tag on a red commit releases | Replace the gate step with one command, `velnor-ci release-gate --repo … --sha "$GITHUB_SHA"`, installed by `actions/setup-velnor` at the pinned release. It already requires push-run evidence (`gate.rs:141-172`). Close #1608 as superseded. Later: generate this workflow (#733) | No |
| 9 | **`ci.yml` has rotted.** Cargo Deny runs `cargo deny` on `ci-helper/Cargo.toml`, `crates/velnor-runner/Cargo.toml`, `generator/Cargo.toml` and `local/Cargo.toml`, which were all deleted by #1594 (ci.yml "Run cargo-deny" step). Machete covers only `velnor-archive-guard`. Alint is red. None of it is required. It duplicates `policy` (R:480-485) | Permanent red noise teaches everyone to ignore red | Delete `ci.yml`. Alint becomes `[[lint]] kind = "alint"`, Jactionlint becomes `[[lint]] kind = "jactionlint"` (`C/velnor-model/src/lint.rs:16-39`); deny and machete stay in `policy`. `[[lint]]` jobs must follow the label flow (today they "run on every ready pull request, also when `pr_ci` is `label`", `CONFIG.md:218`). **W4** | No |
| 10 | **RUSTSEC-2026-0543 (chrono via turso, #1218)** fails cargo-deny | `policy` turns red once #1–#3 are fixed | Either an advisory ignore with an expiry and a reason in `deny.toml` (a policy weakening, so the owner decides) or drop the path to chrono | **Yes** (Q-N13) |
| 11 | **Dead `StepKind::GeneratorCheck`** (`C/velnor-model/src/steps.rs:108`, rendered `C/velnor-render/src/helper_steps.rs:74-80`, never planned) | A hand-edited `rust-ci.yml` that still parses is never caught in CI (freshness relies on out-of-repo `regen-main.sh`) | Plan a `generated` gate step in the `rust` job (always, cheap) running the **pinned** `velnor-gen check`. The lock already pins velnor-gen digests (`.velnor/rust-ci.lock`). Add `GateNeed::Generated` so the verdict requires it. **W5** | No |
| 12 | **Cache saves need a green run** (`success()` in R:304-315). Restructure #1594 changed `compat`, so the target cache is cold, and a red main never saves a new one. The tip push compiled third-party crates from scratch (run 44584861409 Lint log) [M] | Slow, not wrong (goals never-compromise 3 holds); every run is cold until main is green | No code change: fixed by #1–#2. W9 records it. Optional: allow `inputs save` + target save after a red Test but green build (separate `When::BuildSucceeded`) | No |
| 13 | **`fetch-depth: 2`** (R:62-66, `PLAN_DEPTH`, `C/velnor-rust/src/plan.rs:37`): a multi-commit push or a multi-PR group fails `range_base` (`C/velnor-ci/src/io/git.rs:66-79`) and selects everything | Safe but slow | Leave it for squash queue entries (base = `HEAD^1`). For multi-entry groups add a `velnor-ci fetch-base --sha <base>` step (one command, `git fetch --depth=1`) before `plan`, and log `all:base-unavailable` counts in W9 | No |
| 14 | **Unknown paths select everything** (`C/velnor-rust/src/classify.rs:56`, `select.rs:274`): 160 of the last 396 commits (40%) touched a path the planner cannot place, mainly `generator/samples/**`, `.github/workflows/**` and `generator/CONFIG.md`. 72 (18%) touched a global trigger [M, git log + classify rules replayed in a script] | Correct, slow | Section 4 lever 4. The mapping must be proven: `velnor-gen` tests read `.github/workflows/qualification.yml` (render equality, #1747), so `.github/workflows/**` maps to `velnor-gen` and its dependents, not to "nothing" | No |
| 15 | **Dead second selector.** `.velnor/config.toml [stacks.rust]` (93 lines) is read only by `plan --mode … --request` (`C/velnor-ci/src/cli_args.rs:81-95`, `io/plan.rs:40`, `plan_request/diff/config.rs:14`), and no workflow calls that. #1747's message says deleting it "would turn every PR into a full run". That is wrong for `rust-ci.yml`, whose selection reads `.velnor/rust-ci.toml` (`classify.rs`). Also dead: `write-request`, `resolve-qualification`, `admit-preview`, `image`, `run-authority` | Two selection engines with different rules: a correctness trap for reviewers | Delete the file and the commands. **W10** | No |
| 16 | Stale rule-book text: `goals.md:11-12,16`, `hard-decisions.md:115` (names a deleted engine test, per #1747) | Rules that cannot be followed | Owner-approved edit, bundled with section 7.3 | **Yes** |

### 3.1 Owner decision 12, step by step: as built vs after W3

| Owner step | As built today (R, `CONFIG.md:539-555`) | After W3 | Mechanism |
|---|---|---|---|
| 1. Draft pushes run nothing | Heavy jobs skipped; `Required` **passes** with a notice (R:582-584) | Heavy jobs skipped; `Required` runs, the lookup finds no verdict, so it **fails** "draft: not verified" | `PrRun` draft parts + `VerdictLookup` |
| 2. Ready runs nothing | Quick set runs (plan, tools, caches, Format, Lint, policy); `Required` **green** | Only `Required` runs (≈ 15 s, one helper call), so it is **red** "not verified" (or the quick set too, if the owner wants it, Q-N2: still red) | New `PrRun` for quick steps under the label mode |
| 3. Label starts CI on head | `ci:run` (once) or `ci:auto` (sticky) via `labeled` | One label; `labeled` with that name starts the full run | Unchanged condition, one label |
| 4. While labelled, each push reruns and cancels the old run | `ci:auto` yes; `ci:run` no (label removed at start) | Yes; `ci:run` and `ci-run-label` deleted | Concurrency group per PR (R:22-24) |
| 5. Unlabel stops runs | `unlabeled` not a trigger; the in-flight run continues; later pushes run the quick set and go green | `unlabeled` triggers a run in the PR's group: it cancels the in-flight run; `Required` re-emits only an existing verdict for that SHA, otherwise red | `unlabeled` in `plan_budget::triggers` |
| 6. Merge only with green latest commit | Violated by steps 1, 2 and 5 and by skipped `Required` on unrelated label or `edited` events; plus admin bypass | `Required` is never skipped on a PR event; it is green only with a verdict for this SHA; ruleset bound, no bypass (W7) | `When::Always`, no `Related` skip |
| 7. Queue verifies the merged tree; reuse only for an identical tree | Queue full run configured, never used since cutover | Queue run; W11 adds `SameTree` reuse with the push audit | `ReusePoint::SameTree` |
| 8. No auto-labelling | Holds | Holds | — |

Agents must apply the label with a user or App token. A `GITHUB_TOKEN` label change starts no workflow, which is why W3 deletes `ci-run-label` instead of relying on it.

Not a gap: rule 12 ("plan and required check from a released version") holds for the helper binary (`.velnor/rust-ci.lock` + SHA-pinned `setup-velnor`, R:71-82). But the *workflow* that calls it comes from the PR, so #6 is the part rule 12 does not yet cover.

---

## 4. Speed on the new baseline

### 4.1 Where the time goes now

C measured the post-cutover **multi-root** graph: `rust-plan` plus 7 lane jobs (C§3.1). #1594 replaced it with one `rust` job 33 minutes before C's window closed. The numbers that describe today's workflow are these:

| What | Value | Source |
|---|---|---|
| Queue to job start on the scale set | P50 2–13 s, P90 13–57 s; up to 73 s under burst | C§3.3 [M] |
| Job start to first cargo command (Format) | 21 s (target ≤ 30 s, goals.md:57: met) | run 44584072510 step offsets [M] |
| Unlabelled PR run (plan, quick set) | P50 0.8 / P90 1.2 min | C§2 [M] |
| One-job push, everything selected, cold (post-#1594) | 836–1052 s, n = 4, all red | runs 44577659465, 44578162215, 44576523025, 44577762649 [M] |
| One-job push at `57b65af0db` (cold, red) | about 10 min. Format 3 s, Lint 18 s (failed), Documentation 42 s, **18 alone-checks 9–31 s each, about 270 s in sequence**, Test 15 s (failed by #1), local-mode Test 14 s (failed), **lane guard 96 s**, local-mode Lint 28 s, local-mode Documentation 46 s | run 44584072510 [M] |
| A green PR verdict with tests | **never observed** (#1 makes it impossible) | — |
| Tools release | P50 4.1 min, rebuild on hosted | C§2 [M] |

Step profile of the post-#1747 push at `57b65af0db` (run 44584072510, job `rust`, seconds from job start; steps ≥ 1 s) [M]:

| Offset | Step | Duration | Note |
|---|---|---|---|
| 0 | Set up job | 5 | scale-set pod already up |
| 6 | Check out (depth 2) | 4 | network checkout |
| 10–15 | helper install, `mbx-store`, `plan`, crates mirror, tools from node store, `mise-action`, `prepare`, `check-metadata` | ≈ 5 total | node stores hit |
| 15 | Restore the sources cache | 5 | GitHub cache, hit |
| 20 | Restore the target cache | 1 | effectively a miss (cold after #1594 changed `compat`) |
| 21 | Format | 3 | first cargo command at 21 s |
| 24 | Lint | 18 | failed (gap #2) |
| 42 | Documentation | 42 | post-merge only |
| 84–360 | 18 × `Check <crate> alone` | 270 total (9–31 each) | post-merge only, in sequence |
| ≈ 360 | Test | 15 | failed (gap #1) |
| ≈ 375 | Test (features: local-mode) | 14 | failed (gap #1) |
| ≈ 390 | Test lane guard | 96 | three `nextest list` calls |
| ≈ 486 | Lint (features: local-mode) | 28 | failed (gap #2) |
| ≈ 514 | Documentation (features: local-mode) | 46 | post-merge only |
| ≈ 600 | report and upload; Required 12 s, Cache 13 s after | — | run wall ≈ 10 min |

Of about 600 s of job time, about 360 s (60%) is the post-merge tail (docs ×2, alone-checks, lane guard), and none of it is on the queue path today. Once main is green the profile will be dominated by the test build and runner-host tests, which have never been measured on this shape.

Two costs the plan never saw:

- The lane guard runs `nextest list` three times under two feature sets (`C/velnor-ci/src/lane_guard.rs:27-35`). That builds the test binaries for a second configuration on top of Test, which goes against goals.md rule 4 ("compile at most twice").
- `cargo check -p X` alone-checks re-resolve features per crate, in sequence, on every push.

### 4.2 The next five levers, ordered by minutes saved per merge

A merge costs: labelled PR verdict (on the path) + queue run (on the path) + push run (off the path, but it holds runner-minutes and main's health). Estimates are [E] until W9.

| # | Lever | Saves per merge [E] | Where | How to measure |
|---|---|---|---|---|
| 1 | **A warm cache.** A green main saves `target`; PRs restore it and add the delta. Today every run is cold (#12) | Cold whole-workspace build ≈ 10–15 min of compile → warm incremental ≈ 1–3 min, on the PR and on the queue: **≈ 15–20 runner-min, ≈ 8–12 wall-min** | W1 + W2; nothing new | `velnor-ci report` per run: `first_cargo_seconds`, crates compiled vs needed, `hit exact\|fallback\|miss` (`ci-helper/docs/rust-ci-report.md`); the `Cache` check stays green |
| 2 | **Reuse across queue and push.** (a) Push after a passed queue run on the same SHA skips its checks (`skip_push_after_merge_group`, once #7 is fixed). (b) A queue run whose `HEAD^{tree}` equals the tree of the PR run's merge commit re-emits that verdict (`ReusePoint::SameTree`) | (a) the whole push re-verification, ≈ 7.5 runner-min (C§2 heavy push P50); (b) ≈ 3–5 wall-min on 50–75% of single-PR groups (UX doc estimate) | W8 (a); W11 (b) | Count `skip=true` and `reused=tree` outcomes per day; the push run stays the cold audit for every tree the queue reused (one reuse hop at most per landing) |
| 3 | **Remove the serial post-merge tail.** Move alone-checks and docs into a **parallel** queue job (shape `lint-test` keeps `docs`/`check_alone` off the test critical path). Restrict alone-checks to the 10 crates that release (`release-plz.toml` tools and runner groups); others only nightly. Make the lane guard read Test's listing (`tests_by_package`) instead of three `nextest list` calls | ≈ 4.5 min (alone) + 1.5 min (docs) + 1.5 min (lane guard) per push, ≈ 7.5 runner-min; ≈ 0 wall on the queue path if parallel | `C/velnor-rust/src/docs_post_merge.rs`, `root_jobs.rs` shape; `lane_guard.rs`; config key `check_alone_packages` | Step timings from the jobs API; the shape reason written into R:32 |
| 4 | **Map unplaced paths.** `[[inputs]]` for `generator/samples/**` and `.github/workflows/**` → `velnor-gen` (plus reverse deps `velnor-ci`), `local/**` outside crates → `velnor-local`; `.velnor/runner.lock` and `images/**` → `velnor-runner-host`; `generator/*.md` → nothing only if no test reads it | On the 40% of commits that now select all 18 crates: skip `velnor-runner-host` tests (181k lines; the plan's evidence cites `receipt_integrity` at 924.8 s): **≈ 2–15 wall-min on those PRs** | `.velnor/rust-ci.toml`; proof by `velnor-ci inputs audit` (compile reads) plus a test-read trace | Replay the last 400 commits through `velnor-ci plan --changed` offline and count `all:unknown-path` before and after |
| 5 | **Test shape from timings.** Commit `.velnor/rust-ci-timings.json` from `velnor-ci timings --repo` on green runs, so `shape = auto` can pick `lint-test` or `groups` (`C/velnor-rust/src/shape.rs:18-59`). Put slow runner-host suites in a `[[test_groups]]` job | Wall = max(lint, test) instead of the sum, ≈ 1–3 wall-min on PRs that select runner-host | `shape.rs`, `config_test_groups.rs` | Shape reason line in R; PR verdict P50/P90 from W9 |

Later levers, not in the top five because they need owner decisions or a runner change: queue wait 5 → 0 min (Q-N14, up to 5 wall-min per merge by itself, cheapest of all once the owner agrees); node-local target store (`target-local` like `tools-local`); warm pods; the Native executor.

### 4.3 Targets, re-derived honestly

The old plan's goal was a PR verdict ≤ 90 s / ≤ 4 min and push-to-merge ≤ 3 / ≤ 8 min, set against a baseline of 85/457 min. That baseline assumed the Native executor, ledger and per-root keys. Today the honest baseline is "no green verdict possible". The re-derived targets, all [E] until W9 runs on a green main:

| Stage | After phase 0 (W1–W10) | After levers 1–5 | Stretch (needs Q-N1 runner work) |
|---|---|---|---|
| Label → `Required` on a typical PR (P50 / P90) | 5 / 12 min | 3 / 7 min | 90 s / 4 min |
| Queue run | 5 / 12 min + 5 min wait | 0 (tree reuse) to 3 / 7 min | ≤ 2 min |
| Push-to-merge with the label already on and auto-merge | 15 / 30 min | 6 / 12 min | 3 / 8 min |
| Runner-min per merged PR | ≈ 35 | ≈ 15 | ≤ 10 |
| Unlabelled push | 0 heavy jobs; `Required` fails in ≈ 15 s | same | same |

Pass rule (goals.md:44, 49): medians of at least 3 ordinary runs on a green main, with the raw data kept. Any target that gets worse needs the owner (goals.md:60).

---

## 5. What to keep, change, drop

E's classification of the 174 items: VALID 86, CHANGED 50, CONFLICT 20, DONE 10, OBSOLETE 8. I agree with E's per-item evidence. I disagree where noted.

| Group (E §6.2) | E's action | Mine | Why |
|---|---|---|---|
| B (phase 0) | B03, B08, B10–B12 done; B02, B13, B14 drop; B01, B05–B07, B15 rewrite | **Agree**, but phase 0 is replaced wholesale by W1–W10 (section 6) | E's N01 "green main" is a wave of fixes; in fact it is 3 root causes (#1, #2, #9), about 6 agent-hours. B07 (interim label step) becomes W3 |
| RV (reviews) | keep | **Narrow:** RV03 (velnor-ci/gen/gate/release-gate) now; RV01, RV02, RV04, RV05 wait for Q-N1 | Runner reviews buy nothing until the executor question is answered |
| P, O, X, A (platform, official, native, native actions) | gate on owner | **Agree on the gate; rescope X and A** to "the dialect velnor-render emits" (section 8.4) before any start; X01's `executor/` root is dropped (one workspace, #1594) | The generated dialect is finite and golden-tested |
| H (conformance) | keep | **Keep H01 (lab) now:** it must prove the GitHub semantics W3 relies on (skipped check, newest check run, `unlabeled`, ruleset-required workflows on `merge_group`). H02–H09 wait for Q-N1 | Lab A is now a phase-0 dependency |
| L (App and gate) | L01–L05 keep, L06–L08 rewrite | **Change:** L01 (lab A) only, now. L02–L05 move to phase 2 with a new purpose, a gate computed from the base branch (#6). L06–L08 dropped and redone after phase 2 | The owner flow is reachable with Actions plus `verdict lookup` |
| V (ledger, reuse) | V01–V03 keep; V04 gate; V05, V08, V13 rewrite | **Rebase:** V01 → `VerdictKey` in `velnor-model`; V04 → `ReusePoint::SameTree` (still owner-gated by HD3's spirit, Q-N8); V05 (audit A1) = the push run, renamed to avoid `inputs audit`; V08/V13 → per-package keys only after V07 (read audit); V02, V03 (signing, witnesses) wait until a non-Actions verdict exists | Records live in GitHub until a service is needed |
| AN (agent-native) | keep | **Keep, rebase** on `velnor-gen plan --json` + `velnor-local`; AN01 after the generator stops depending on the `mbx` wrapper | One plan drives every surface |
| C (generations, mirrors) | C01, C02, C05, C08, C09, C11 rewrite | **Agree.** C02 is effectively done (crates mirror live); C03 (git mirror) keeps the crates-mirror pattern | — |
| ST (storage) | ST03 gate, rest keep | **Gate all of ST** behind Q-N1 (goals.md:45, 59) | No speed claim depends on it before the executor |
| T (test floor) | T04, T05 drop; T02, T03, T08 rewrite | **Agree;** T02 targets `receipt_integrity` in runner-host | — |
| R (release) | R01 gate, R02, R04 rewrite | **Agree;** R02 = W6; R04 needs a release build on main (section 2) | — |
| F01 (final deletion) | gate | **Agree** | — |
| BM (benchmarks) | BM03, BM06, BM08, BM14 rewrite | **Agree,** plus BM00 = W9 (first green baseline) and the generator-owned benchmark (section 8.3) | — |
| DG (Dagger-derived) | DG07, DG08, DG11 rewrite | **Agree;** DG08 = W5; DG01 = `VerdictKey` | — |

**What E missed** (each verified in this report):

1. The preflight `--no-run`/`--no-fail-fast` bug, the root cause of red main (#1).
2. The skipped `Required` on unrelated label and `edited` events (#3d).
3. `merge_group_skip` would drop post-merge-only checks (#7).
4. `ci.yml` cargo-deny targets deleted manifests (#9).
5. Cache saves require a green run, so a red main stays cold (#12).
6. The lane guard compiles a second configuration (96 s).
7. The 18 alone-checks run in sequence (≈ 270 s).
8. C's per-job numbers are of the replaced multi-root graph.
9. `.velnor/config.toml` and `plan --mode` are dead code; #1747's claim about them is wrong (#15).
10. #1608 breaks rule 9 and edits deleted paths.
11. #1320 is closed (17:57Z), so #1073 has no PR.
12. The queue's 5-minute minimum wait.
13. `[[lint]]` jobs ignore the label flow (`CONFIG.md:218`).
14. A PR can weaken its own gate (#6).
15. 40% of commits select everything because of unplaced paths.

**Streams that survive:**

1. **Correctness reset** (new; W1–W8): replaces "Phase 0 and workflows".
2. **Verdict and reuse** (V + L01): first item W11 (tree-equal queue reuse).
3. **Measurement** (BM + B15): first item W9.
4. **Release** (R): first item W6.
5. **Selection and shape** (new, from FAST-3): first item W10.
6. **Agent-native** (AN): first item `velnor-gen plan --json`.

Parked behind Q-N1: Platform, Official, Native, Native actions, Storage, Final deletion, Generations (except C03).

---

## 6. Phase 0 and the first ten work items, rewritten

**State of open work (gh, 2026-11-20).**

| PR | State | Disposition |
|---|---|---|
| #1274 (`[[workspace_groups]]`) | Merged `61067c17fc` | No action |
| #1335 (lane guard) | Merged `47ef96f8ea` | No action |
| #1747 (engine deleted) | Merged `57b65af0db` | No action |
| #1320 (per-root audit) | **Closed unmerged** 17:57Z | #1073 stays open with no PR; re-scope after W10 (one root workspace now) |
| #1608 (release gate) | Open | **Close as superseded by W6.** Its shell breaks rule 9, and 3 of its 4 files are deleted or moved paths |
| #1200 (mise platform pins, closes #1104) | Open, 2 behind | **Rebase and finish after W1/W2**; red only because main is red |
| #1026 (inputs `--target-dir`, siblings; 100 commits, conflicting) | Open | **Split into two PRs after phase 0** (helper, then generator); not needed by this repo's own CI, so off the critical path. 10–14 agent-hours |
| #1297 (Renovate) | Open | **Close**; Renovate recreates it on the single workspace after W1 and Q-N13 |
| #1751 (draft research PR for this round) | Open | Not read, per the brief |

Issue hygiene (an orchestrator task, ≈ 1 agent-hour, Haiku-class model, no code):

- Close as done: #604 (switch Rust CI, #1523), #608 (delete the engine's Rust path, #1420/#1747; box 2 is the owner's waiver), #1459 (#1747), #1161 (#1748), #644 (#1274).
- Rescope: #1290 "P1: velnor-new runs entirely on velnor-gen". The remaining boxes are W4 (`ci.yml`) and W6/#733 (release workflow).
- Rescope: #1073. One root workspace now, and its PR #1320 is closed. Check whether the inline `inputs audit` (R:300-302) still double-builds after W1.
- Rescope: #1555 "main fallout". Its root causes are gaps #1, #2 and #9; link W1 and W4.
- New issues: one per W-item, under #574, labels `fast-ci` plus `exec:*`/`effort:*` per section 6 (roadmap.md's ready-issue query picks them up).
- Close the complete milestones FAST-0, FAST-1, FAST-2 and FAST-R (0 open issues each, D§3).

**Phase 0 ordering rule.** W1 and W2 merge first; they are the last admin merges, recorded. Then W3, W4, W5 and W6 merge with a real green `Required`. Then the owner applies W7. Every later PR goes through the queue.

| # | Item | Goal | Files | Done when | Size (agent-h) | Implement | Review |
|---|---|---|---|---|---|---|---|
| W1 | **Tests can run; main green** | Fix #1 and #2; release velnor-ci 0.1.96; pin | `C/velnor-ci/src/io/audit/build/invocation.rs`, new `C/velnor-ci/tests/it/preflight_argv.rs`, velnor-ci test `it` leftovers, `.velnor/rust-ci.lock`, regenerated R | A push run on main runs `Test` with `tests_run > 0` and `Required` is green; a contract test fails on today's code (red) and passes after (green), confirmed by an independent subagent | 5 | Sonnet 5.5, high | Opus 5.5, high |
| W2 | **Cache warm after green** | Record that main saves `target` and a PR restores it with `hit exact\|fallback` | none (verification); optional `When::BuildSucceeded` | Two consecutive main pushes: the second compiles only changed crates (`Cache` check green, `report` numbers kept) | 1 | Sonnet 5.5, medium | Sonnet 5.5, medium |
| W3 | **Label flow with no unverified green** | Rewrite `pr_ci = "label"` as in section 3 #3; add `velnor-ci verdict lookup --sha` (same SHA, same workflow digest, full gate passed); delete `ci:run`, `ClearRunLabel` and `ci-run-label`; gate `[[lint]]` jobs by the label | `C/velnor-model/src/plan.rs` (`PrRun`, labels), `C/velnor-rust/src/{plan.rs,plan_budget.rs,required_job.rs,lint_jobs.rs}`, `C/velnor-render/src/{gate.rs,events.rs,draft_steps.rs}`, new `C/velnor-ci/src/verdict_lookup.rs`, `helper_gate.rs` (≥ 0.1.96), `generator/CONFIG.md:539-555`, `docs/rust-ci.md:76-90`, goldens | velnor-lab cases (L01) prove all of these: draft red; ready unlabelled red; label → full run; push while labelled cancels and reruns; unlabel cancels and the new head is red; unrelated label keeps the SHA's verdict; title edit runs nothing | 12 | Opus 5.5, high | Opus 5.5, high + lab proof |
| W4 | **Delete `ci.yml`** | Alint and Jactionlint as `[[lint]]`; deny and machete only in `policy`; delete `.github/actions/velnor-verify-jactionlint` if unused | `.velnor/rust-ci.toml`, `.github/workflows/ci.yml` (deleted), regenerated R | `ci.yml` is gone; `Required` needs the two lint jobs; `policy` covers the whole workspace | 3 | Sonnet 5.5, medium | Sonnet 5.5, high |
| W5 | **Generator freshness in the gate** | Plan `StepKind::GeneratorCheck` with the pinned `velnor-gen`; add `GateNeed::Generated` | `C/velnor-rust/src/plan.rs`, `C/velnor-render/src/helper_steps.rs:74-80` (install from lock), `required_job.rs`, goldens | A PR that hand-edits R fails `Required` with the `velnor-gen check` drift list | 4 | Sonnet 5.5, high | Opus 5.5, medium |
| W6 | **Release gate = `velnor-ci release-gate`** | Replace the ancestry-only gate; close #1608 | `.github/workflows/release-rust-ci-tools.yml:86-105` | A tag on a commit whose push `Required` is red stops at Gate (dry run on a fork tag, or a lab repo); goals.md:77 holds | 2 | Sonnet 5.5, medium | Opus 5.5, medium |
| W7 | **Ruleset and settings** (owner applies; agent prepares and reads back) | `Required` bound to 15368; `bypass_actors: []`; queue ALLGREEN, min wait 0; `allow_auto_merge: true` | ruleset 24396608, repo settings | Read-back JSON matches field by field; the next 10 merges all have a green `Required` on head and on the group | 1 (+ owner) | Sonnet 5.5, low | Opus 5.5, medium |
| W8 | **Post-merge checks run in the queue; safe push skip** | Fix #7, then enable `[ci] skip_push_after_merge_group = true` | `C/velnor-rust/src/{docs_post_merge.rs,merge_group_skip.rs}`, `.velnor/rust-ci.toml`, `CONFIG.md:96-101` | Golden: no check is skipped on push unless the queue ran it; 20 merges: every push after a passed group has `skip=true` and only builds and saves | 5 | Opus 5.5, medium | Opus 5.5, high |
| W9 | **First green baseline** | Measure PR verdict, queue, push and push-to-merge on green main (≥ 3 runs each class); commit `.velnor/rust-ci-timings.json` | `velnor-ci bench collect\|compare`, `velnor-ci timings --repo`; issue comment with raw data (evergreen docs rule) | A table in the issue replaces section 4.3's [E]; `shape = auto` decides from timings | 3 + wall | Sonnet 5.5, medium | Opus 5.5, medium (`benchmark-checklist`) |
| W10 | **Selection hygiene** | `[[inputs]]` mapping for the unplaced paths (lever 4), proven by audit; delete `.velnor/config.toml` and the dead `plan --mode`/`write-request`/`resolve-qualification`/`admit-preview`/`image`/`run-authority` commands (#15) | `.velnor/rust-ci.toml`, `C/velnor-ci/src/{cli.rs,cli_args.rs,plan_request*,write_request,resolve_qualification,image_candidate,preview}`, `.velnor/config.toml` | Offline replay of 400 commits: `all:unknown-path` falls from 40% to < 5%; no test-read trace finds a read outside the mapping | 8 | Opus 5.5, high | Opus 5.5, high |

### 6.1 Test plans for the phase-0 items (AGENTS.md: every kept test seen red on broken code, green on fixed)

| Item | Behaviour the test proves | Positive case | Negative case | Why existing tests miss it |
|---|---|---|---|---|
| W1 | Every generated test argv yields a preflight nextest accepts | Each `Test`/`Test (features: …)` argv in all 38 goldens → preflight parsed by `cargo-nextest`'s argument rules accepts `--no-run` | Today's insertion keeps `--no-fail-fast` and is rejected | Goldens compare text; helper tests use a hand-built argv (`C/velnor-ci/src/tests_run/list_argv.rs:22,50`) without the generator's flags |
| W3 | `Required` is never success without a verdict for the head SHA | Labelled run passes; unlabelled push red; unrelated label re-emits the existing verdict | Draft → red; unlabel mid-run → cancelled + red; `edited` title → no run | Goldens assert YAML conditions, not GitHub's skipped-check semantics: the lab case (velnor-lab, real GitHub) is the test |
| W5 | A hand edit of R fails the gate | Generated R passes `velnor-gen check` | One byte changed in R's body → `Forged` drift → red | `GeneratorCheck` is never planned (`steps.rs:108`) |
| W6 | Release stops on a red push `Required` | Tag on a green main commit → Gate passes | Tag on a commit whose push `Required` failed → Gate fails | Current gate reads only `compare` status |
| W8 | No check is skipped on push unless the queue ran it | With `docs = post_merge`, `Documentation` runs in `merge_group` and is skipped on push after `skip=true` | Same config, step absent from queue → must still run on push | `merge_group_skip::is_check` is a fixed list (`merge_group_skip.rs:9-27`) |
| W10 | Mapped paths select exactly the packages that read them | `generator/samples/x` → `velnor-gen`, `velnor-ci` (reverse dep) | `crates/velnor-runner-host/src/x.rs` still selects runner-host and dependents; an unmapped new top-level dir still selects everything | No test replays real history through `select` |

### 6.2 Merge order and who merges

```
W1 (helper fix, 0.1.96 release, pin)  ── admin merge #1 (recorded) ─┐
W2 (verification only)                 ───────────────────────────────┤ main green, cache warm
W3 (label mode, 0.1.96 helper)          ── first PR with a real green `Required`
W4, W5, W6 (parallel)                   ── green `Required`, still direct merge
W7 (owner: ruleset + auto-merge)        ── from here: queue only, no bypass
W8, W9, W10                             ── through the queue
```

If W1's release is cut from the W1 branch commit, rule 13 ("released only from a default-branch commit whose required check passed") cannot hold for 0.1.96 at all: no commit has a green `Required` yet. The owner accepts this one exception explicitly (part of Q-N3), and W6's gate applies from 0.1.97 on.

Phase 0 total ≈ 44 agent-hours plus owner time for W7, Q-N2, Q-N13 and Q-N14. With 3–4 builders, wall time is set by the queue and lab proof: about 1.5–2 days [E].

Next after the first ten:

- **W11**: tree-equal queue reuse (`ReusePoint::SameTree`, push run = cold audit), gated on Q-N8. 10 h, Opus high / Opus high.
- **W12**: gate refuses obligation reductions versus base (#6). 8 h, Opus high / Opus high.
- **W13**: lane guard reads Test's listing. 3 h, Sonnet high / Opus medium.
- **W14**: alone-checks scoped and parallel. 4 h, Sonnet high / Opus medium.
- **W15**: #1026 split. 12 h, Sonnet high / Opus high.
- **W16**: `velnor-gen plan --json` and a local render target. 10 h, Opus high / Opus medium.

---

## 7. Owner questions, rewritten

### 7.1 D1–D26

| Id | Status now | Rewritten question (if any) | Recommendation |
|---|---|---|---|
| D1 release engine 0.1.81 | **Obsolete** (engine deleted, #1747) | — | Remove |
| D9 pre-approve engine releases | **Obsolete** | Replace with: may agents release `tools-v` patch versions (velnor-ci/velnor-gen) without a per-release approval, if `release-gate` passes? | Yes |
| D14 delete engine crates | **Done** (#1747) | — | Remove |
| D3 first ruleset update | **Reworded** | After W1–W3 merge: bind `Required` to 15368, empty the bypass list, ALLGREEN, min wait 0, auto-merge on? | Yes, in that order. Accept that W1 and W2 are the last admin merges |
| D2 auto-merge, no catch-up merges | **Reworded** (folded into D3) | Agents enable auto-merge at PR creation and never merge `main` into their branches | Yes |
| D7 `velnor-verify` App + agent identity | **Reworded** | Create an agent identity now (it labels PRs; it cannot edit rulesets). Defer the App to phase 2, whose job is a gate computed from the base branch | Identity now; App later |
| D8 deploy identity, node changes | Valid, **reworded** | Also: where do the cluster manifests live (N11)? Needed only for runner work after Q-N1 | Defer until Q-N1 |
| D11 cutover counts per job type | **Obsolete until Q-N1** | — | Park |
| D15 recovery from a bad Velnor release | **Reworded** | A PR's workflow comes from its own tree, so a bad `velnor-ci` release is fixed by a re-pin PR that verifies itself with the previous helper. Approve "re-pin PR, no admin merge" as the recovery path? | Yes; no break-glass verifier needed while the gate is Actions |
| D16 label on a draft arms at ready | Valid, reworded for W3 | Should `ready_for_review` with the label already present start the full run? | Yes (the event carries labels; zero cost) |
| D4 evaluator A/B | **Obsolete until Q-N1**; rescope by section 8.4 | — | Park |
| D17 who approves a reduction | **Valid, now implementable** (W12) | Owner-signed reduction entry for any PR that shrinks the base's obligation set or test coverage? | Owner only |
| D19 compile before the label | Valid | — | No |
| D20, D22, D24, D25 | Valid, no repo dependency | — | Park until W9 shows the floor |
| D21 one compile for both runs | Valid | — | No |
| D18 merge train | Valid | — | Not now; W11 removes most of the reason |
| D23 auto-verify unpushed commits | Valid | — | Not yet |
| D26 native arm64 lane | **Premise changed:** release binaries are built on native `ubuntu-24.04-arm` (`release-rust-ci-tools.yml` build matrix), not qemu | Add an advisory arm64 *test* lane? | Not now |
| D13 park old runner PRs | **Re-ask** (premise false; hardening continued) | See Q-N9 | — |
| D5, D6, D10, D12 | Settled. D10's ALLGREEN is still not applied (W7) | — | — |

### 7.2 New questions (E's Q-N1..Q-N11, plus four)

| Id | Question | Recommendation |
|---|---|---|
| Q-N1 | Approve the goals.md amendment in 7.3 (Rust-only executor), and separately decide goals.md:45, 59 (GitHub's 10 GB cache) and :43 (fork rules)? | Approve 7.3 now. Keep :45/:59 until W9 shows cache is the limiter. Amend :43 only with Q-N6 |
| Q-N2 | Label flow: name; one label (retire `ci:run`); quick set on unlabelled ready PRs or nothing; unlabelled `Required` red | One label `ci:auto` (no rename churn); retire `ci:run`; **no quick set** (an unlabelled push runs only `Required`, ≈ 15 s, red "not verified") |
| Q-N3 | Remove the admin bypass and bind `Required` now (= D3) | Yes, after W1–W3 |
| Q-N4 | Is the merge queue part of the flow? | Yes; it is the only place the merged tree is verified |
| Q-N5 | Releases: hosted-attested (`--deny-self-hosted-runners`, `C/velnor-render/src/release_attest.rs:70`) or on Velnor? | Hosted-attested for now; promotion by digest (R04) works either way |
| Q-N6 | Fork PRs: drop handling (decision 8) or keep the fence (goals.md:43)? | Drop the self-hosted path for forks, keep "forks never get a green": with W3 a fork's `Required` is red unless a maintainer labels it, and labels are refused for forks |
| Q-N7 | Parallel lanes with separate targets vs one job per workspace | One target; split only by measured shape (lever 5) |
| Q-N8 | Allow verdict reuse (tree-equal queue reuse, W11) despite HD3's ban on a "second skip mechanism"? | Yes, with its soundness case (key = workflow digest + helper digest + image digest + tree id; the push run audits every reused tree cold; any audit mismatch turns reuse off) |
| Q-N9 | Runner-host: keep hardening or freeze until Q-N1? | Freeze new features; keep fixes for the `local-mode` lane, which is in CI |
| Q-N10 | Evidence home (evergreen-docs rule) | Move the dated research to issues or a `research/` branch; keep only contracts under `docs/` |
| Q-N11 | Executors for the backlog: Claude-only? | Claude-only (matches `exec:*` labels and `roadmap.md`) |
| Q-N12 (new) | Should the gate's rules come from the base branch (ruleset "required workflow" pinned to a ref, or the phase-2 App) so a PR cannot weaken its own gate? | Yes. W12 now; pick the mechanism after a lab test |
| Q-N13 (new) | RUSTSEC-2026-0543 (#1218): time-boxed ignore in `deny.toml` or remove the chrono path? | Remove the path if ≤ 4 agent-hours; else an ignore that expires in 30 days |
| Q-N14 (new) | Queue minimum wait 5 → 0 min and ALLGREEN | Yes |
| Q-N15 (new) | Alone-checks only for the 10 crates that release, the rest nightly? | Yes (lever 3) |

### 7.3 Proposed wording change to `docs/goals.md` (for approval or rejection)

**Line 16, current:**

> New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. Jobs are always executed by GitHub's official runner: on GitHub's machines, or on the project's own machines inside the containers that `velnor-runner` starts.

**Line 16, proposed:**

> New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. On GitHub's machines, GitHub's official runner executes jobs. On the project's own machines, `velnor-runner` executes them with one of two executors behind one interface: Velnor Native, a Rust executor for the workflows `velnor-gen` writes, which is the default; or GitHub's official runner, which the project chooses in its configuration. Velnor never switches executors on its own, never reuses a result across executors, and keeps the official runner as the reference that proves Velnor Native gives the same results.

**Line 20, current sentence:** "The runner (`velnor-runner`) runs jobs on the project's own machines."

**Proposed:** "The runner (`velnor-runner`) runs jobs on the project's own machines, with the executor the project configures."

**Line 75 (rule 11), current:** "A workflow is verified on the developer's machine with GitHub's official runner, started by the Velnor runner, a local stand-in for GitHub and a local cache that enforces GitHub's rules."

**Proposed:** "A workflow is verified on the developer's machine with the executor the project uses on its own machines, started by the Velnor runner, a local stand-in for GitHub and a local cache that enforces GitHub's rules."

**Lines 11–12, factual layout fix** (no change of meaning; still needs approval under line 3): replace "the Rust generator in `generator/`, the CI helper in `ci-helper/`, the runner in `crates/velnor-runner`, the local environment in `local/`" with "the generator and helper crates `crates/velnor-{gen,ci,model,rust,render,release,workflows}`, the runner crates `crates/velnor-runner-*` and `crates/velnor-resource-probe`, and the local environment `crates/velnor-local`, `crates/velnor-cache-rules` and `local/`". Then delete the line-12 row's engine clause ("the existing engine in `crates/`… until the generator takes over the Rust part"), because #1747 deleted the engine.

Not proposed now: lines 43, 45 and 59 (forks and the GitHub cache limits). They are separate decisions (Q-N1, Q-N6) and nothing in phases 0–1 depends on them.

---

## 8. Be ambitious: what a generator-first foundation makes possible

### 8.1 The generator emits the verification contract

Today the obligation list is implicit in YAML `if:`s and one `GATE_SPEC` string (R:609). velnor-gen can emit `.velnor/rust-obligations.json` next to `rust-metadata.json`, built from `WorkflowPlan` + `VerdictSpec`. For each obligation it records:

- an id (`lint`, `test:root`, `alone:velnor-ci`, `doc`, `lint:alint`);
- the definition digest of its exact command line and profile;
- the events that require it;
- its selection rule;
- the reuse class (`content | per-request | never`, arch §6.1).

Three consequences:

- The gate checks the head's contract against the **base's** contract. A reduction needs an owner signature (D17). This closes #6 without an App.
- `velnor explain` and `--explain` speak in obligation ids, not step names.
- A reviewer reads one diff of the contract instead of 666 lines of generated YAML.

The contract is also the native executor's acceptance corpus.

### 8.2 Verdict keys at plan time, and one reuse rule for every surface

The definition half of the key is known when `velnor-gen generate` seals the workflow. The input half costs nothing at run time: `git rev-parse HEAD^{tree}`, plus per-package subtree ids (`git rev-parse HEAD:crates/velnor-ci`) once the read audit enrols a package. One rule, "a verdict is re-emitted only for an equal key, and every landed tree is executed at least once by a run on that exact tree", then covers all of these:

- rebase-only pushes (same tree, new SHA);
- `edited` and label churn (same SHA);
- the queue (PR merge tree = group tree);
- push after queue (same SHA);
- release (tag SHA = pushed SHA, promotion).

That is the old design's ledger with no new service, no signing and no App for phase 1. Signing becomes necessary only when a verdict is produced off GitHub (agent sandboxes, Native executor).

### 8.3 A generator-owned benchmark and speed budget

The generator knows every step it emitted, so it can also emit a declared `rust-ci-bench.yml` (workflow_dispatch plus nightly, via `.velnor/workflows.toml`). That workflow replays the last N landed commits through the *same* plan on the current helper and on the candidate helper, with `velnor-ci bench collect|compare|budget`. A helper release is pinned (#1742-style PR) only when the comparison passes: goals.md rule 13 ("passed on the lab project first") becomes "passed on this repo's own history". `[budget]` (`C/velnor-rust/src/config_budget.rs`, helper ≥ 0.1.92) turns the targets of section 4.3 into an advisory check, and then a blocking one. This is the only way "fastest" stays true after the panel disbands.

### 8.4 Make "Actions-compatible" mean "compatible with what velnor-render emits"

The old plan sized a general Actions interpreter (X05–X12, ≈ 100 agent-hours, expressions A/B, composites, post steps). velnor-new's workflows are all generated (#1747), and velnor-render guarantees:

- 16 SHA-pinned actions (`rules.rs:17-34`);
- every `run:` is one command with no shell control flow (`rules.rs:44-45`);
- a bounded set of expression templates (`velnor-render/src/{gate.rs,events.rs,cache.rs}`);
- a sealed header digest.

A Native executor that admits only sealed velnor-gen workflows needs:

- an expression evaluator for that template grammar, which can be enumerated from the renderer's tests;
- native implementations of the 16 actions, of which 6 (`setup-velnor`, `cache/restore`, `cache/save`, `checkout`, `mise-action`, `upload-artifact`) cover the Rust CI path;
- one-command step execution.

The 38 golden samples (`generator/samples/*`) are the conformance corpus, and the official runner is the oracle. The renderer and the executor then evolve together: a new template is added to both in one PR, or the renderer refuses it. This is the strongest argument for owner decisions 1/20 that velnor-gen creates, and it is why Q-N1 should be answered in favour of the amendment in 7.3.

### 8.5 One plan drives the agent, the cluster and the laptop

`velnor-gen plan --json` exports the `WorkflowPlan` and the selection. Three runners consume it:

- **`velnor-local`** (Mac, official runner), already able to run generated workflows;
- **`velnor verify`** (agent sandbox on a cluster node), which runs only the obligations the selection picks and writes T0 records that predict the queue outcome but never fill a verdict;
- **CI.**

Because the key is the same struct everywhere, an agent knows *before pushing* whether its push will be a cache hit, a reuse or a full run. "Agent-native" stops meaning a second CI and becomes the same CI moved earlier.

### 8.6 Plan-aware scheduling without new jobs

`SECOND_JOB_COST_SECONDS = 5` (`C/velnor-rust/src/shape.rs:10`) prices a job at 5 s. The scale set measured 2–73 s of queue alone (C§3.3), so the constant is wrong for self-hosted runners, and the shape decision should read a measured start cost from `rust-ci-timings.json`. The plan also knows which steps share nothing: Format, the policy tools and the lint jobs read no `target/`. On the official runner that knowledge can only become separate jobs. On a Velnor executor it can become concurrent steps inside one sandbox, with the plan as the DAG. Express the dependency edges in `WorkflowPlan` now (`Step.after: Vec<StepId>`), even though the Actions renderer serialises them. The same plan is then ready for an executor that does not.

### 8.7 Release as a by-product of main, not a rebuild

The tools release rebuilds `velnor-ci` and `velnor-gen` from source on hosted VMs (`release-rust-ci-tools.yml:106-186`), with no cache and a different profile. With the generator owning both workflows:

- The push run on main builds `--profile release-tools` for the two binaries, in a parallel job, only when `velnor-ci` or `velnor-gen` is selected. It keys the output by `VerdictKey` + target + profile and stores it as an artifact (or in a node store later).
- The tag workflow runs `release-gate`, downloads the artifact whose key matches the tag's SHA, verifies its SHA-256, and publishes.

Owner decision 10 ("release promotes, never rebuilds") is then met with GitHub-native pieces. The hosted-attestation question (Q-N5) only decides *where* that parallel job runs.

### 8.8 The hard problems (where to spend the strongest agents)

1. **Key completeness for per-package verdicts.** Compile-time reads are known (`.velnor/rust-inputs.lock`, `velnor-ci inputs audit`). Test-time reads are not: 376 files use `CARGO_MANIFEST_DIR`; velnor-gen tests read `.github/workflows/*` and `generator/samples/*`. Without a kernel-level read audit (arch §6.4, V07), every verdict stays whole-tree. Spend V07 effort here first. Sub-minute verdicts for small changes depend on nothing else.
2. **Clock, network and randomness (C5).** A whole-tree reuse of a test that reads `SystemTime` is unsound in principle. The generator can mark obligations C5 from the audit and never reuse them. The cost is that those tests always rerun.
3. **The gate's own trust root.** As long as the PR's workflow runs its own gate, every guarantee above rests on reviewers noticing workflow edits. The contract diff (8.1) makes that review cheap. The App or a ruleset-pinned workflow (Q-N12) makes it unnecessary.
4. **Flakes vs zero false greens.** With verdict reuse, a flaky pass is copied forward. Rule: a reused verdict is never older than the newest red on the same key. The push audit's red revokes all records with that key.
5. **Selection-aware queue grouping.** The planner knows each PR's package closure, so two PRs with disjoint closures and no global-trigger files can be verified as one group and their union tested once. GitHub's queue cannot do this. A Velnor merge train (D18) could. Measure in BM14 before building.

---

## 9. Risks, and what would change this verdict

| Risk or unknown | Effect on the plan | How it is settled | Owner of the test |
|---|---|---|---|
| GitHub may not let a later *skipped* `Required` override an earlier red one (gap #3d) | #3d becomes a smaller issue; the rest of W3 is unchanged | velnor-lab: red `Required`, then add an unrelated label, then read the PR's mergeability | L01, 1 agent-hour |
| Ruleset "required workflows" may not run on `merge_group`, or may not pin a ref for the same repo | Q-N12 mechanism becomes the App (phase 2) | velnor-lab ruleset with a `workflows` rule; open a PR and enqueue it | L01, 2 agent-hours |
| The test build plus runner-host tests may dominate even warm (never measured on one job) | Lever 5 (shape) moves to first place; targets in 4.3 move | W9 | Measurement stream |
| `SameTree` reuse may rarely fire (main moves between PR run and enqueue) | W11 saves less; the merge train (D18) gains weight | Count PR-merge-tree = group-tree matches over 50 merges after W7 | BM14 |
| Fixing W1 may expose real test failures hidden behind the argv error since #1192/#1229 | Phase 0 grows by fix-forward work, of unknown size | W1's first green push; triage per failing package | W1 owner |
| Owner rejects the goals.md amendment (7.3) | Native executor, storage and ARC deletion stay out of scope; phases 0–1 (this plan) are unaffected | Q-N1 | Owner |
| `velnor-local` still cannot run the `velnor-new` scenario end to end (no recorded run, B§2.4) | Agent-native work (8.5) waits; W1-style generator/helper incompatibilities are caught only on GitHub | Record one run after W1 | AN stream |

What would make me change the verdict: if W9 shows a warm one-job PR verdict still above 10 minutes at P50 with tests selected, then the Actions-only phase cannot meet even the intermediate targets. In that case the runner work (Q-N1, warm pods, node-local target store) moves ahead of W11–W16.

---

## Appendix: claims verified for this report (beyond A–E)

| Claim | Evidence |
|---|---|
| 18 alone-check steps at the tip (not 31) | `grep -c "name: Check .* alone" R` = 18 |
| Tests fail by argv conflict on the tip push | run 44584861409, steps "Test" and "Test (features: local-mode)": `error: the argument '--no-fail-fast' cannot be used with '--no-run'` |
| Clippy errors at the tip | same run, "Lint": unused import `OsRelease`; variant `Platform` never constructed; `velnor-ci` (test "it") |
| ci.yml cargo-deny targets deleted manifests | `ci.yml` "Run cargo-deny" step: `ci-helper/Cargo.toml`, `crates/velnor-runner/Cargo.toml`, `generator/Cargo.toml`, `local/Cargo.toml`; `ls crates` shows the flat layout |
| `.velnor/config.toml` has no workflow reader | `grep` for `plan --mode`/`--request` in `.github/`, `actions/`, `scripts/`: no hits; only `plan_request/diff/config.rs:14` reads it |
| #1320 closed, #1608 touches deleted paths | `gh pr view 1320` (CLOSED 2026-11-19T17:57Z); `gh pr view 1608 --json files` |
| One-job push timings | jobs API on runs 44584072510 (step offsets), 44577659465, 44578162215, 44576523025, 44577762649 |
| 40% unknown-path commits | `git log --name-only -400` replayed through `classify.rs` rules (old `generator/crates`, `ci-helper/crates`, `local/crates` mapped to packages; deleted fixture paths excluded) |
