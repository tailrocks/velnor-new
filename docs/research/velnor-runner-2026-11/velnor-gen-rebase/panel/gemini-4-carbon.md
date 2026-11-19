# velnor-gen rebase by Gemini 4 Carbon

| Field | Value |
|---|---|
| Model | Gemini 4 Carbon (`gemini-4-carbon`) |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high |
| Date | 2026-11-20 |
| Main tip analysed | `0eb2d76a5c` |
| Brief | [velnor-gen rebase brief v1](../briefs/velnor-gen-brief-v1.md) |

Independent answer from the Velnor runner research panel, written without seeing other members' answers. Unedited except for this header and replacing home-directory paths with `~`.

---

# Rebase of the Velnor runner plan onto velnor-gen

Audited against `tailrocks/velnor-new` at `main` tip `0eb2d76a5c` (`2026-11-20`, including `#1274` `61067c17fc`, `#1743` `94da9295e0`, `#1335` `47ef96f8ea`, `#1747` `57b65af0db`, and `#1748` `0eb2d76a5c`). Evidence tags follow `docs/proposed/velnor-runner-architecture.md:20-26`: `[M]` measured, `[M, proxy]` measured on a proxy clock or pre-cutover cohort, `[E]` estimated with arithmetic shown, `[C]` verified in repository source at the cited `file:line`, and `[T]` target.

## 1. Verdict in ten lines

1. Your goal is to make Velnor the fastest GitHub-Actions-compatible verification and execution system for `velnor-new` on top of `velnor-gen`, without ever trading correctness for speed.
2. **Yes, the core runner design still holds on `velnor-gen`:** the two-witness verification ledger, the `Velnor / Verified` App status, `Velnor Native` plus `GitHub Official` behind `Executor`, `Platform` (`Kubernetes` and `Docker`), tiered `BlobStore`, node mirrors, XFS reflink generations, and agent-native `velnor verify` all remain sound.
3. **What disappears is the old plan's workflow, workspace, and migration scaffolding:** `#1523` (`053fefa6a3`) and `#1747` (`57b65af0db`) deleted all 12 `velnor-actions-*` crates (`-450,687` lines `[C]`), moved `100%` of `Rust CI` off GitHub-hosted runners (`0` of `2,660` sampled jobs hosted `[M]`), and collapsed four nested workspaces into one 18-crate root `Cargo.toml:5-24` `[C]`.
4. **`velnor-gen` and `velnor-ci` already ship half of Phase 0 and the data plane's content addressing:** package diff selection (`crates/velnor-rust/src/select.rs:16-40`), deterministic mtime restoration (`crates/velnor-ci/src/io/inputs.rs:137-188`), isolated PR build deltas (`crates/velnor-rust/src/delta_cache.rs:9-101`), node-local tool and MBX stores (`crates/velnor-ci/src/tools_local.rs:15-270`, `mbx_store.rs:32-221`), and the cluster crates mirror (`.velnor/rust-ci.toml:11-14`).
5. **The single most important change to the plan is architectural unification:** `velnor-gen` (`WorkflowPlan` and `VerdictSpec`) becomes the sole compiler of the verification contract, emitting `.github/workflows/rust-ci.yml`, `.velnor/obligations.json`, and local CLI check definitions in one `velnor_rust::plan` pass instead of maintaining a parallel obligation list by hand.
6. **Before any ruleset lock or runner build begins, Step 0 must repair `main`:** `0eb2d76a5c` crashes `100%` of test steps because `velnor-ci inputs build` injects `--no-run` alongside `--no-fail-fast` (`crates/velnor-ci/src/io/audit/build/invocation.rs:79-87` `[C]`, confirmed in push run `44584861409` `[M]`), alongside two Clippy test errors, four `Alint` line-cap breaches, and four deleted manifest paths in `.github/workflows/ci.yml:117` `[C]`.
7. **Because `0` heavy `main` pushes have succeeded since cutover (`4` docs-only passes of `172` push runs `[M]`), zero `velnor-tgt-*` target caches exist on `main`, which also starves `#1107`'s PR delta cache (`rust-ci.yml:180`).**
8. **Meanwhile, merge gating has two active holes:** `167` of `173` post-cutover merges bypassed a failing `Required` (`[M, inferred from ruleset 24396608]`), and `pr_ci = "label"` turns `Required` **green** on drafts and unlabelled ready PRs with zero tests run (`crates/velnor-rust/src/required_job.rs:113-148` `[C]`), inverting Owner Decision 12.
9. **Governance must catch up before executor crates merge:** `docs/goals.md:16` and `docs/proposed/macos-scaleset-runner.md:37-39` still mandate official-runner-only execution, requiring explicit owner approval on the exact `docs/goals.md` diff in Section 7.3.
10. **Sequenced as green `main` -> strict `velnor-gen` demand-label gate -> non-bypassable `ALLGREEN` ruleset -> `velnor-gen`-compiled obligations + reflink generations + `Velnor Native`, the plan sheds `257` agent-hours and reaches real speed without an unverified merge.**

## 2. The design mapped onto velnor-gen

Every new crate belongs under `crates/velnor-*` in the single root workspace (`Cargo.toml:5-24` `[C]`), enforced by `.alint.yml:127-138` (`crates-listed-in-workspace` `[C]`). The merged plan's separate `executor/` Cargo root (`docs/proposed/velnor-runner-architecture.md:240`) existed only to isolate new crates while deleting `velnor-actions-*`; `#1290` (`23bed02e7e`) and `#1747` (`57b65af0db`) made a second Cargo root obsolete and forbidden under `.alint.yml:150-158` (`velnor-no-stray-cargo-lock` `[C]`).

### 2.1 Master placement table

| # | Component | Generator-time (`velnor-{model,rust,render,gen,release}`) | Run-time (`velnor-ci`, `velnor-rust::verdict`) | Runner-side (`crates/velnor-*`) | Already free from `velnor-gen` + `velnor-ci` at `0eb2d76a5c` |
|---|---|---|---|---|---|
| 1 | Verification ledger & verdict keys | `ObligationSpec`, `ObligationKey`, `ReuseClass` in `velnor-model`; `.velnor/obligations.json` from `velnor_gen::generated_with_notes` | `velnor-ci plan` resolves keys; `velnor_rust::verdict::decide` verifies receipts | New `crates/velnor-ledger` (2-witness slots, Ed25519, epochs, A1/A2, joins) | `CacheKey` (`cache.rs:95-113`), `compat` (`compat.rs:23-51`), `lock_key` (`lock_key.rs:12-40`), `VerdictSpec` (`verdict.rs:71-75`), `inputs.tsv`, `.velnor/rust-inputs.lock` |
| 2 | `Velnor / Verified` App status | `[ci] gate_mode` in `.velnor/rust-ci.toml`; `plan_budget::triggers` drops PR/group triggers at Step 4; push `Required` runs `AuditWait` | `velnor-ci audit-wait` polls/claims A1; `velnor-ci release-gate` binds `--pr-only-check` | New `crates/velnor-control` (webhooks, 10 s ETag reconciler, fenced writer) | `required_check` (`repo_config.rs:28`), `job_trust_boundary.rs:11-158`, `gate/exemptions.rs:136-253` (App-bound PR check + empty-bypass proof) |
| 3 | Label flow (`velnor:verify`) | `PullRequestType::Unlabeled`; `pr_label` key; `FailOnDraftRun` + `StepKind::FailWhenUnrequested`; delete `ClearRunLabel` | `velnor-ci gate` runs only on labelled PRs, `merge_group`, `push`, `schedule`, `dispatch` | `velnor-control` armed state (`velnor:verify` + `!draft`), cancel on `unlabeled` | `PrCi` (`repo_config.rs:145-149`), `PrRun` (`plan.rs:119-128`), `CancelOlderPullRequestRunsExceptUnrelatedLabels` (`events.rs:13-28`), base-retarget filter (`gate.rs:253-270`) |
| 4 | Reuse: agent -> PR -> queue -> main -> release | Rename `Step.skip_after_merge_group` to `skip_when_reused`; add `SelectionReason::Reused` | Stage 1: `[ci] skip_push_after_merge_group = true`; Stage 2: `velnor-ci plan` ledger lookup | `velnor-ledger::ensure` promotes T2 records to T3 `main` after cold A1 | `HelperCommand::MergeGroupPassed` (`steps.rs:166`), `merge_group_skip.rs:33-58`, `merge_group.rs:45-86`, `verdict_plan.rs:28-112` |
| 5 | Release promotion by digest | `.velnor/release.toml` `mode = "promote"` + lift `family.rs:77-97` (`tools-v*`, `runner-v*`); archive obligation in `WorkflowPlan` | `velnor-ci release promote` + `velnor-ci release-gate` + post-link ELF note (`R06`) | `velnor-storage` `release` route (`strict` durability) stores attested archives | `velnor-release` (`ruleset.rs:11-178` `RELEASE_APP_ID = 5_762_496`), `velnor-render/src/release*.rs`, `velnor-gen pin-helper` (`io/pin.rs:15-50`) |
| 6 | Agent-native `velnor verify` / `cargo` / `velnord` | `.velnor/obligations.json` + `.velnor/cli-checks.json`; `cargo_wrapper` key retires MBX steps when `velnor cargo` takes over | `velnor-ci` obligation runner shared by `velnor check <name>`, `hk.pkl`, and cluster nodes | New `crates/velnor-{cli,daemon}` (`velnord`: private Git index -> `refs/velnor/wip/*` -> T2 `interactive`) | `.velnor/rust-metadata.json` (`read_recorded_checkout` in `velnor-gen/src/io/mod.rs:310-340`), `mise.toml:47-49` wrapper slot |
| 7 | Velnor Native vs GitHub Official | `[executor]` certified digests in `.velnor/rust-ci.toml`; drop `github-script` from `ALLOWED_ACTIONS` | `velnor-ci obligation exec` (`O03`) reports exits + nextest counts over root channel | New `crates/velnor-{executor,exec,official,native-actions,dispatch,conformance}`; reuse `velnor-runner-github` scale-set types | 15-action SHA-pinned `ALLOWED_ACTIONS` (`rules.rs:12-36`), 1-command `run:` rule (`rules.rs:306-376`), `0` `background: true` `[C]`, `qualification.yml` (`64` jobs) |
| 8 | Platform trait (`K8s` + `Docker`) | `[[test_groups]]` `needs = ["docker"]` (`config_test_groups.rs:23-224`) isolates the 3 Docker test binaries | Unchanged (`velnor-ci` is platform-agnostic) | New `crates/velnor-{platform,platform-k8s,platform-docker}` merging `#1324` `Observation` + `docker_spec::audit_plan` | `velnor-runner-core` (`lifecycle.rs:9-67`, `ownership.rs:31-41`), `docker_client.rs:15-125`, `docker_spec.rs:47-245`, `velnor-resource-probe` |
| 9 | Storage (`NVMe` / `RustFS` / `S3`) | `.velnor/sites.toml` schema validated by `velnor-gen check` | `velnor-ci` reads/writes local paths or presigned URLs from `velnor-node` | New `crates/velnor-storage` (`BlobStore`, local XFS L0, S3/RustFS L1, async L2) | `velnor-resource-probe` mount/tmpfs checks, SHA-256 verification helpers in `velnor-ci` |
| 10 | Native `actions/cache` server | Plan-time cache-key manifest for L0 prefetch; `cache_backend = "generations"` (`C09`) omits YAML cache steps | `velnor-ci cache` commands work unchanged against Twirp v2 | New `crates/velnor-cache` (home-site authoritative index, 1 MiB CDC chunks, `ResultsServiceUrl` proxy) | `velnor-cache-rules` (`lookup.rs:8-41`), `velnor-local` Twirp v2 + Azure blob server (`protocol.rs:10-163`), 6 conformance probes (`local/conformance/*.json`) |
| 11 | Git, crate, tool, image mirrors | `[crates_mirror]` (`rust-ci.toml:11-14`), `StepKind::CratesMirror`, `StepKind::ToolsLocal` | `velnor-ci crates-mirror` + lockfile checksum verification; `velnor-ci tools-local` | `crates/velnor-node` bare Git mirror (`flock`, staged-tree hash), `EROFS` tool mount, OCI mirror | Live `crates-mirror` DaemonSet + `velnor-ci crates-mirror` + `velnor-ci tools-local` (`0 s` hit in run `44584861409` `[M]`) |
| 12 | Warm snapshots (generations) | `CARGO_INCREMENTAL=0`, `SOURCE_DATE_EPOCH=0`, `ci_debuginfo = "line-tables-only"`, fixed `CARGO_TARGET_DIR` (`compat.rs`) | Port `inputs.rs`, `delta_freshness.rs`, and `io/audit/dep_info.rs` into generation manifest `M_G` | `crates/velnor-node` twin cold builders, XFS reflink clone (`0.25 s` `[M]`), read-set invalidation | `velnor-ci inputs` (`inputs.rs:194-243`), `velnor-ci delta` (`io/delta*.rs`), `velnor-ci inputs audit` (`io/audit/dep_info.rs:16-90`) |
| 13 | MBX (`mr-boxington`) | `mbx_shared_store` (`repo_config.rs:169-179`), `StepKind::{MbxStore,MbxSeed,MbxRemote}` | `velnor-ci mbx-store` (`mbx_store.rs:32-221`) and `mbx-seed` (`mbx_seed.rs:42-188`) until `C08` | Benchmark `C08` (`mbx-cache` vs chunked generation shipping for cold nodes); delete loser | Working `VELNOR_MBX_SHARED_DIR` + read-only `VELNOR_MBX_SEED_DIR` (`31 s` warm workspace Clippy in `44584861409` `[M]`) |

### 2.2 Exact generator-time, run-time, and runner-side extensions

#### 2.2.1 Verification ledger and verdict keys
- **Generator-time:** Extend `crates/velnor-model/src/verdict.rs:71-75` (`VerdictSpec`) with `ObligationInventory { schema: u32, obligations: Vec<ObligationSpec> }`, `ObligationKey`, and `ReuseClass` (`TreeExact | ClosureExact | BinaryDigest | AlwaysFresh`). Keep `crates/velnor-render/src/cache.rs:19` (`render_key`) as the sole canonical key serializer. In `crates/velnor-gen/src/lib.rs:43-51` (`generated_with_notes`), emit `.velnor/obligations.json` directly from `velnor_rust::plan::plan_with` (`crates/velnor-rust/src/plan.rs:50-118`).
- **Run-time:** `crates/velnor-ci/src/io/plan.rs:38-93` (`run_plan`) computes concrete `ObligationKey` values over resolved subtree/blob OIDs; `crates/velnor-rust/src/verdict.rs:277-317` (`decide`) verifies witness signatures and `tests_by_package` (`PACKAGE_COUNTS_LIMIT = 1_000_000` at `verdict.rs:57`).
- **Runner-side:** New `crates/velnor-ledger` stores obligation records (`RecordStatus`, two distinct-node witness slots, `ExecutorId`, `ExecutionFingerprint`, policy/sandbox/ledger epochs, cold `A1` and `2%` live `A2` schedulers, and in-flight singleflight joins).
- **Free from `velnor-gen`:** `CacheKey` (`crates/velnor-model/src/cache.rs:95-113`), `compat` (`crates/velnor-rust/src/compat.rs:23-51`), `lock_key` stripping workspace member versions (`crates/velnor-rust/src/lock_key.rs:12-40`), `inputs.tsv` (`crates/velnor-ci/src/inputs.rs:194-243`), and `.velnor/rust-inputs.lock` (`39` audited out-of-crate compile paths `[C]`).

#### 2.2.2 The `Velnor / Verified` App status
- **Generator-time:** Add `gate_mode: GateMode` (`Actions | Shadow | App`) under `[ci]` in `crates/velnor-rust/src/config_ci.rs:9-17` and register `"ci.gate_mode"` in `crates/velnor-rust/src/config_keys.rs:4-39`. At Step 4 (`gate_mode = "app"`), `crates/velnor-rust/src/plan_budget.rs:49-66` (`triggers`) omits `pull_request` and `merge_group` from `rust-ci.yml`, keeping `push` (`Required` running new `HelperCommand::AuditWait` in `crates/velnor-model/src/steps.rs:158-167`), `schedule`, and `workflow_dispatch`.
- **Run-time:** New `velnor-ci audit-wait` subcommand (`crates/velnor-ci/src/cli.rs:36-186`) waits for or claims cold `A1` verification on `main` pushes so `goals.md:77`'s default-branch `Required` check stays authoritative.
- **Runner-side:** New `crates/velnor-control` receives GitHub App webhooks, runs a `10 s` ETag reconciler over open labelled PRs and `merge_group` entries, and writes fenced commit status `Velnor / Verified` plus check run `Velnor / Details`.
- **Free from `velnor-gen`:** Single `required_check` (`crates/velnor-model/src/repo_config.rs:28`), untrusted-to-gate artifact fencing (`crates/velnor-gen/tests/it/job_trust_boundary.rs:11-158`), and `crates/velnor-ci/src/gate/exemptions.rs:136-253`, which **already** accepts `--pr-only-check "Velnor / Verified"` bound to a non-Actions GitHub App `integration_id` when enforced by an active ruleset with `bypass_actors == []` alongside push `Required` `[C]`.

#### 2.2.3 Label flow (`velnor:verify`)
- **Generator-time:** Add `PullRequestType::Unlabeled` to `crates/velnor-model/src/plan.rs:40-59` and `crates/velnor-render/src/events.rs:40-81`; add `pr_label: String` (default `"velnor:verify"`) in `crates/velnor-model/src/repo_config.rs:10-46`; delete `StepKind::ClearRunLabel` (`steps.rs:113`) and `ci-run-label` (`crates/velnor-rust/src/report.rs:94-106`); gate `rust`, `policy`, and `lint-*` on `PrRun::Requested` (`plan.rs:330-343`); replace `DraftNotice` with `FailOnDraftRun` (`plan_budget.rs:68-76`); and replace `FailWhenANeedFailed` in `crates/velnor-rust/src/required_job.rs:132-135` with `StepKind::FailWhenUnrequested` (`exit 1` with `::error::CI not requested: add label velnor:verify`).
- **Run-time:** `crates/velnor-ci/src/io/gate.rs:40-96` (`run_gate`) executes only on labelled PRs, `merge_group`, `push`, `schedule`, and `workflow_dispatch`; `Required` never passes without `velnor-ci gate`.
- **Runner-side:** Under `gate_mode = "app"`, `velnor-control` tracks armed state (`has_label("velnor:verify") && !draft`), cancels running witnesses on `unlabeled` or `synchronize` within `5 s`, and leaves any already-verified status on an unchanged SHA intact unless revoked.
- **Free from `velnor-gen`:** `PrCi` (`repo_config.rs:145-149`), `PrRun` (`plan.rs:119-128`), `CancelOlderPullRequestRunsExceptUnrelatedLabels` (`events.rs:13-28`), and `PullRequestOpenedOrSynchronizedOrBaseChanged` (`crates/velnor-render/src/gate.rs:253-270`).

#### 2.2.4 Reuse across agent run -> PR -> queue -> main -> release
- **Generator-time:** Generalise `Step.skip_after_merge_group: Option<PlanFrom>` (`crates/velnor-model/src/plan.rs:109`) to `Step.skip_when_reused`, add `HelperCommand::LedgerLookup` (`steps.rs:158-167`), and add `SelectionReason::Reused` (`crates/velnor-model/src/selection.rs:64-71`).
- **Run-time:** Stage 1 sets `[ci] skip_push_after_merge_group = true` in `.velnor/rust-ci.toml`, activating `crates/velnor-ci/src/merge_group.rs:45-86` (`passed`) immediately once the merge queue resumes. Stage 2 extends `crates/velnor-ci/src/io/plan.rs:50-75` to query `velnor-ledger` per obligation while keeping `ReuseClass::AlwaysFresh` (`cargo-deny` advisories, `release-gate`) uncached across days.
- **Runner-side:** `velnor-ledger::ensure` reuses `T2` witness pairs across `velnor verify` (`refs/velnor/wip/*`), PR heads, and `merge_group` commits whenever `ObligationKey` matches, then runs cold `A1` on `main` to promote records to `T3`.
- **Free from `velnor-gen`:** `HelperCommand::MergeGroupPassed` (`steps.rs:166`), `crates/velnor-rust/src/merge_group_skip.rs:33-58`, `crates/velnor-rust/src/select_event.rs:20-32`, and `crates/velnor-rust/src/verdict_plan.rs:28-112`.

#### 2.2.5 Release promotion by digest
- **Generator-time:** Lift `crates/velnor-release/src/family.rs:77-97` (`unsupported`) so `family_tag` (`tools-v{{ version }}` and `runner-v{{ version }}`) composes with `[[binaries]]`, `attest`, `smoke`, `commit_file`, and `member_digests`; add `mode = "promote"` on `[[binaries]]` in `crates/velnor-release/src/config.rs:18-62`; emit `.github/workflows/release.yml` from `.velnor/release.toml` and delete `.github/workflows/release-rust-ci-tools.yml`.
- **Run-time:** Add `velnor-ci release promote` (fetches attested archive bytes by SHA-256 from `velnor-storage`, stamps `VELNOR_COMMIT` into `.note.velnor.commit` when `R06` is active, runs native smoke, verifies `SHA256SUMS` and `BINARY-SHA256SUMS`, and publishes with zero compilation).
- **Runner-side:** `velnor-storage` `release` route (`strict` durability, `30 d` retention) stores release archives built once in `merge_group`/`push`.
- **Free from `velnor-gen`:** `crates/velnor-release/src/ruleset.rs:11-178` (`RELEASE_APP_ID = 5_762_496`), `crates/velnor-render/src/release_{binaries,publish,attest,gate}.rs`, and `crates/velnor-gen/src/io/pin.rs:15-50` (`pin-helper`).

#### 2.2.6 Agent-native `velnor verify`, `velnor cargo`, and `velnord`
- **Generator-time:** Emit `.velnor/cli-checks.json` alongside `.velnor/obligations.json` in `velnor_gen::generated_with_notes`, and add `cargo_wrapper = "mbx" | "velnor" | "none"` to `RustCiConfig` (`crates/velnor-model/src/repo_config.rs:10-46`) so switching `mise.toml:47-49` from `mbx` to `velnor cargo` stops `velnor-gen` from emitting `MbxStore`/`MbxSeed`.
- **Run-time:** Extract obligation execution in `crates/velnor-ci` into a library entrypoint shared by `velnor check <name>`, `hk.pkl`, and cluster workers.
- **Runner-side:** New `crates/velnor-cli` (`velnor`) and `crates/velnor-daemon` (`velnord`): `velnord` snapshots tracked plus untracked non-ignored files through a private Git index (`GIT_INDEX_FILE`), pushes thin packs to `refs/velnor/wip/<id>` on the home-site node mirror, routes builds by replay threshold (`AN06`), and calls `velnor-ledger::ensure` on two `interactive`-class nodes (`T2` receipts).
- **Free from `velnor-gen`:** `.velnor/rust-metadata.json` (`velnor_gen::io::read_recorded_checkout` in `crates/velnor-gen/src/io/mod.rs:310-340`, `< 5 ms` load without running `cargo metadata` `[C]`) and `mise.toml:47-49`.

#### 2.2.7 `Velnor Native` executor vs `GitHub Official`
- **Generator-time:** Add `[executor]` in `.velnor/rust-ci.toml` recording certified `Velnor Native` and `GitHub Official` digests read from the target branch (`base_sha`), and remove `actions/github-script` plus unused `docker/*` entries from `ALLOWED_ACTIONS` (`crates/velnor-render/src/rules.rs:12-36`) once `A08` replaces the 3 `github-script` calls.
- **Run-time:** Add `velnor-ci obligation exec` (`O03`) so `GitHub Official` can execute single obligations from synthetic job messages and return structured exits and nextest counts over a root-owned socket.
- **Runner-side:** New `crates/velnor-{executor,exec,official,native-actions,dispatch,conformance}`; reuse `crates/velnor-runner-github`'s clean scale-set types (`src/{paths,poll,acquire,jit,client}.rs` and `src/session/*`, `8,176` lines total `[C]`) and add message-session decryption, run-service `acquirejob`/`renewjob`/`completejob`, Results Twirp, OIDC, and `Sdk/Expressions` (`X06b`). Because `0` `background: true` steps exist anywhere in the repo `[C]`, `X13` is dropped.
- **Free from `velnor-gen`:** 15-target SHA-pinned `ALLOWED_ACTIONS` (`rules.rs:12-36`), one-command `run:` enforcement (`rules.rs:306-376`), generated `qualification.yml` (`64` jobs from `.velnor/workflows.toml`), and `.velnor/runner.lock` (`2.338.0`).

#### 2.2.8 Platform trait (`Kubernetes` + `Docker`)
- **Generator-time:** Declare a `docker` group in `.velnor/rust-ci.toml` `[[test_groups]]` (`needs = ["docker"]`, parsed by `crates/velnor-rust/src/config_test_groups.rs:23-224`) covering the 3 Docker test binaries in `.config/nextest.toml:35-44` (`docker-integration`, `velnor-local` `it`, `velnor-resource-probe` `container_cgroup` `[C]`) so build jobs drop `docker:dind`.
- **Run-time:** `velnor-ci` stays platform-agnostic (`NEXTEST_TEST_THREADS` derived from cgroup quota via `velnor-resource-probe`).
- **Runner-side:** New `crates/velnor-{platform,platform-k8s,platform-docker}` combining `docs/proposed/kubernetes-runtime.md:80-154` (`Observation`, Pod-UID `preconditions.uid` deletion) with `crates/velnor-runner-host/src/docker_spec.rs:47-245` (`audit_plan`, `HOST_NEEDLES`) and `crates/velnor-runner-core/src/ownership.rs:31-41` (`CleanupProof::covers`), dropping pair-coupled calls and replacing `velnor-runner-host`'s `65.8k`-line Turso `journal/` (`JOURNAL_VERSION = 59` at `journal/schema.rs:77` `[C]`) at `F01`.
- **Free from `velnor-gen`:** `[[test_groups]]` `needs = ["docker"]`, `velnor-runner-core`, `docker_client::connect_unix` (`docker_client.rs:15-125`), `docker_spec::audit_plan`, and `velnor-resource-probe`.

#### 2.2.9 Storage (`NVMe`, `RustFS`, `S3`)
- **Generator-time:** Validate `.velnor/sites.toml` schema and retention routes inside `velnor-gen check` (`G05`).
- **Run-time:** `velnor-ci` reads/writes local paths or short-lived presigned URLs injected by `velnor-node` without embedding cloud credentials in job pods.
- **Runner-side:** New `crates/velnor-storage` (`BlobStore` trait: `head`, `get_range`, `put_verified`, `delete`; local NVMe XFS L0, per-site `RustFS`/S3 L1, async cross-site L2 replication, and `ephemeral`/`cached`/`evidence`/`release` durability routes).
- **Free from `velnor-gen`:** `velnor-resource-probe` filesystem probes and strict SHA-256 verification discipline across `velnor-ci`.

#### 2.2.10 Native `actions/cache` server
- **Generator-time:** Emit expected cache-key prefixes in `.velnor/obligations.json` for L0 prefetch; add `cache_backend = "github" | "generations"` (`C09`) in `RustCiConfig` to omit YAML `actions/cache` steps once generations graduate.
- **Run-time:** `velnor-ci cache` (`crates/velnor-ci/src/cache_cmd.rs`) speaks unchanged Twirp v2 (`CreateCacheEntry`, `FinalizeCacheEntryUpload`, `GetCacheEntryDownloadURL`).
- **Runner-side:** New `crates/velnor-cache` promoting `crates/velnor-cache-rules/src/lookup.rs:8-41` and `crates/velnor-local/src/protocol.rs:10-163` into a home-site authoritative index with `1 MiB` content-defined chunks on `velnor-storage` plus an Official-runner `ResultsServiceUrl` proxy (`ST05`).
- **Free from `velnor-gen`:** Exact GitHub scope/prefix matching in `velnor-cache-rules`, working Twirp v2 + Azure BlockList handlers in `velnor-local`, and 6 live-GitHub-verified conformance fixtures (`local/conformance/*.json` `[C]`).

#### 2.2.11 Git, crate, tool, and image mirrors
- **Generator-time:** Already wired via `[crates_mirror]` (`.velnor/rust-ci.toml:11-14`), `StepKind::CratesMirror` (`crates/velnor-model/src/steps.rs:88`), and `StepKind::ToolsLocal` (`steps.rs:103`).
- **Run-time:** Extend `crates/velnor-ci/src/io/crates_mirror.rs:16-144` to verify downloaded `.crate` SHA-256 digests against `Cargo.lock` checksums before unpack (`C02`).
- **Runner-side:** `crates/velnor-node` adds a node-local bare Git mirror (`flock`-serialized fetch, daily site bundle seed, full staged-tree hash check `C10`), read-only `EROFS`/bind tool mounts (`#275` defense), and a site-local OCI pull-through mirror (`C11`).
- **Free from `velnor-gen`:** Shipped `crates-mirror` DaemonSet + `velnor-ci crates-mirror` and `velnor-ci tools-local` (`0 s` hit in live push run `44584861409` `[M]`).

#### 2.2.12 Warm snapshots (verified build generations)
- **Generator-time:** `crates/velnor-rust/src/compat.rs:11-51` already normalizes `CARGO_INCREMENTAL=0`, `SOURCE_DATE_EPOCH=0`, `ci_debuginfo = "line-tables-only"`, `RUSTFLAGS`, and fixed `CARGO_TARGET_DIR` (`$RUNNER_TEMP/velnor-target`). Emit separate generation profiles per feature configuration (`default` vs `local-mode`).
- **Run-time:** Port `crates/velnor-ci/src/inputs.rs:194-243`, `crates/velnor-ci/src/io/delta_freshness.rs:11-40`, and `crates/velnor-ci/src/io/audit/dep_info.rs:16-90` into generation manifest `M_G` verification (computing `mtime` resets against `M_G`'s recorded file digests rather than `HEAD^1`, eliminating Q16 stale-fingerprint reuse by construction).
- **Runner-side:** `crates/velnor-node` builds new generations cold on two independent nodes (`C12` twin equality check on `.rlib`/`.rmeta`/proc-macro `.so`), seals them read-only on XFS (`ftype=1,reflink=1`), and clones per-obligation sandboxes via `FICLONE` (`0.25 s` `[M]`).
- **Free from `velnor-gen`:** Deterministic `inputs.tsv` mtime restoration (`1966080000` / `1966080001`), `#1107` PR delta staging (`io/delta_save.rs`, `io/delta_restore.rs`), and `.d` dep-info auditing (`io/audit/dep_info.rs`).

#### 2.2.13 MBX (`mr-boxington`)
- **Generator-time:** `mbx_shared_store = true` (`crates/velnor-model/src/repo_config.rs:169-179`) currently emits `StepKind::MbxStore` and `StepKind::MbxSeed` (`steps.rs:100-101`).
- **Run-time:** `crates/velnor-ci/src/mbx_store.rs:32-221` and `mbx_seed.rs:42-188` keep ARC builds fast (`31 s` warm workspace Clippy in `44584861409` `[M]`) until XFS generations (`C05`) land.
- **Runner-side:** Run head-to-head experiment `C08` (`mbx-cache` vs chunked generation shipping on cold nodes). Remove MBX from the hot reflink path and from `mise.toml:47-49` when `velnor cargo` (`AN01`) and `C05` take over; delete `mbx_store.rs`/`mbx_seed.rs` if `C08` loses (`principle-subtract-before-you-add`).
- **Free from `velnor-gen`:** Shipped node-local trusted `VELNOR_MBX_SHARED_DIR` and untrusted read-only `VELNOR_MBX_SEED_DIR` separation (`#1411`).

## 3. Correctness first

Ranked from active false-green merge holes down to fail-safe degradations on `main` at `0eb2d76a5c`:

| Rank | Gap on `main` (`0eb2d76a5c`) | Severity | Where the fix goes | Needs owner? |
|---:|---|---|---|---|
| 1 | `167/173` bypass merges, unbound `Required`, `HEADGREEN`, unused merge queue | Active unverified merges | Ruleset `24396608` + repo `allow_auto_merge` (after Rank 3 turns `main` green) | **Yes** (`Q-N3`, `Q-N4`) |
| 2 | `pr_ci = "label"` gives FALSE GREEN `Required` on drafts & unlabelled PRs; ignores `unlabeled` | Active false-green gate | `velnor-model/src/plan.rs`, `velnor-rust/src/{plan_budget,required_job,report}.rs`, `velnor-render` | **Yes** (`Q-N2` label/UX); **No** on blocking |
| 3 | `main` fails `100%` of test runs (`--no-fail-fast` + `--no-run` crash), Clippy, `Alint`, and `ci.yml` `Cargo Deny` | Zero tests run on `main`; 0% target cache | `velnor-ci/src/io/audit/build/invocation.rs:79-87`, 2 test files, 4 `.rs` splits, delete `ci.yml` | **No** |
| 4 | Tools release gate checks ancestry only, never `Required`; `runner-v*` has no workflow | Unverified release bytes (`goals.md:77` breach) | Merge `#1608`; migrate to `.velnor/release.toml` + `velnor-ci release-gate` | **No** (`#1608`); **Yes** (`Q-N5`, `Q-N9`) |
| 5 | `Alint`, `Jactionlint`, and `lint-generator-samples` are outside `Required` | Unenforced quality gates | `.velnor/rust-ci.toml` `[[lint]]` (`velnor-rust/src/lint_jobs.rs`) + delete `ci.yml` | **No** |
| 6 | Dead `StepKind::GeneratorCheck`, stale sample (`2,051` diff lines), `.velnor/config.toml` masking unmarked workflows | Silent workflow & sample drift | `actions/setup-velnor`, `velnor-rust/src/plan.rs`, `velnor-gen/src/drifts.rs`, delete `.velnor/config.toml` + 8 dead CLI commands | **No** |
| 7 | `docs/goals.md:16`, `macos-scaleset-runner.md`, `kubernetes-runtime.md`, and `Cargo.toml` lint/assertion conflicts | Governance & contract contradictions | `docs/goals.md` (Section 7.3), `macos-scaleset-runner.md`, `kubernetes-runtime.md`, `Cargo.toml:83,101-107` | **Yes** (`Q-N1`) for docs; **No** for `Cargo.toml` |
| 8 | Broken `qualification.yml` (`stable-sources`, `submodule`), `tag-ruleset-drift.test.ts`, `hk.pkl`, Mise pin split | Broken qualification & local hooks | `qualification/stable-sources.sh`, `.gitmodules`, `scripts/tag-ruleset-drift.test.ts`, `hk.pkl`, `workflows.toml` | **No** |
| 9 | `rust-cache-trim.yml` crashes on merged/closed PR runs; `Cache` job fails workflow conclusion (`N13`) | Leaked PR caches; noisy red run conclusions | `crates/velnor-ci/src/delta_trim.rs:58-80`; `Cache` check-run/notice reporting | **No** |
| 10 | `fetch-depth: 2` fallback, unmapped repo paths, unconditional `local-mode` lint | Fail-safe over-selection (`AllTrigger`) | `crates/velnor-ci/src/io/git.rs:66-79`, `.velnor/rust-ci.toml` `[[inputs]]`, `velnor-rust/src/root_jobs.rs` | **No** |

### 3.1 Rank 1: Bypass merges, unbound `Required`, `HEADGREEN`, and unused merge queue
- **Evidence:** Across `173` post-cutover merges (`C§1, §4` `[M]`), `167` merged with `Required` failing (`157` admin-bypass direct merges inferred), `0` `Rust CI` `merge_group` runs occurred, and `allow_auto_merge` is `false`. Live ruleset `24396608` (`protect-main`) requires only `Required` with `integration_id: null` (any token can spoof the context name), `bypass_actors: [RepositoryRole 5]`, `grouping_strategy: "HEADGREEN"`, and `min_entries_to_merge_wait_minutes: 5` `[M]`.
- **Fix and location:** Immediately after Rank 3 turns `main` green (`P0-03`), apply the `PUT /repos/tailrocks/velnor-new/rulesets/24396608` payload in Section 6.2 (`integration_id: 15368`, `bypass_actors: []`, `ALLGREEN`, `min_entries_to_merge_wait_minutes: 0`) and set repository `allow_auto_merge: true`. **Needs owner:** **Yes** (`Q-N3`, `Q-N4`).

### 3.2 Rank 2: `pr_ci = "label"` turns `Required` green without tests on drafts and unlabelled PRs
- **Evidence:** `.velnor/rust-ci.toml:9` sets `pr_ci = "label"`. On drafts, `crates/velnor-rust/src/plan_budget.rs:68-76` emits `StepKind::DraftNotice`, so `Required` exits `0` (green) with zero jobs run (`.github/workflows/rust-ci.yml:584-589` `[C]`). On ready PRs without `ci:run` or `ci:auto`, `rust` skips `Test`, `Test (features: local-mode)`, `Test lane guard`, and `inputs audit` (`PrRun::Requested`), and `Required` skips `velnor-ci gate` (`rust-ci.yml:606-611`, `crates/velnor-rust/src/required_job.rs:113-148` `[C]`), turning `Required` **green** whenever `Format` and `Lint` pass (`0` tests run!). Also `PullRequestType` (`crates/velnor-model/src/plan.rs:40-59`) omits `Unlabeled`, and `ci-run-label` (`rust-ci.yml:628-643`) auto-deletes `ci:run`, breaking push-driven re-runs.
- **Fix and location:** In `crates/velnor-model/src/{plan,steps,repo_config}.rs`, `crates/velnor-rust/src/{plan,plan_budget,required_job,report}.rs`, and `crates/velnor-render/src/{events,gate,steps}.rs`: add `PullRequestType::Unlabeled`, gate `rust`/`policy`/`lint-*` on `PrRun::Requested`, emit `FailOnDraftRun` on drafts and `StepKind::FailWhenUnrequested` (`exit 1`) in `Required` when unlabelled, delete `ci-run-label`, and adopt `velnor:verify`. **Needs owner:** **Yes** on label name and whether any non-gating job runs (`Q-N2`); **No** on refusing unverified green status.

### 3.3 Rank 3: `main` is 100% broken on test execution, Clippy, `Alint`, and `ci.yml` `Cargo Deny`
- **Evidence (verified on `0eb2d76a5c` push runs `44584861409` and `44584861503` `[M]` + code `[C]`):**
  1. **100% test crash:** `crates/velnor-ci/src/io/audit/build/invocation.rs:79-87` builds a `--no-run` preflight from `cargo-nextest nextest run ... --no-fail-fast` (`crates/velnor-render/src/cargo.rs:295`, `rust-ci.yml:282,287`) without calling `drop_run_only` (`crates/velnor-ci/src/tests_run.rs:25-49,139-160`). `cargo-nextest 0.9.152` aborts with exit `2`: `the argument '--no-fail-fast' cannot be used with '--no-run'`. Zero tests run on `main` or labelled PRs!
  2. **Clippy `-D warnings` failures:** `crates/velnor-ci/tests/it/prepare_inputs.rs:9:28` (`unused import: OsRelease`) and `crates/velnor-ci/tests/it/prepare.rs:69:5` (`variant Platform is never constructed`, orphaned by `#1747`).
  3. **`Alint` line-cap breaches (`.alint.yml:208-228`):** `crates/velnor-ci/tests/it/prepare_inputs.rs` (`417 > 400`), `crates/velnor-gen/tests/it/delta_cache.rs` (`423 > 400`), `crates/velnor-render/src/lib.rs` (`154 > 150`), and `crates/velnor-rust/src/lib.rs` (`152 > 150`).
  4. **`ci.yml` `Cargo Deny` crash:** `deny.toml:15` already ignores `RUSTSEC-2026-0543` (`advisories ok` in `44584861503`), and then `.github/workflows/ci.yml:117` crashes because it passes deleted `ci-helper/Cargo.toml`, `crates/velnor-runner/Cargo.toml`, `generator/Cargo.toml`, and `local/Cargo.toml`.
- **Fix and location:** Strip `RUN_FLAGS`/`RUN_OPTIONS` in `invocation.rs:79-87`, delete the dead test import/variant, split the 4 `.rs` files under cap, fold `ci.yml` into `rust-ci.yml`, and cut `tools-v0.1.96` (`P0-01` to `P0-03`). **Needs owner:** **No**.

### 3.4 Rank 4: Tools release gate ignores `Required`, and `runner-v*` has no workflow
- **Evidence:** `.github/workflows/release-rust-ci-tools.yml:86-105` checks `compare/${default_branch}...${GITHUB_SHA}` (`behind|identical`) and never checks whether `Required` passed on that commit (`:93` grants `checks: read` unused), violating `docs/goals.md:77`. Meanwhile `release-plz.toml:46-106` defines `runner-v{{ version }}` across 8 crates with no workflow at all `[C]`.
- **Fix and location:** Rebase and merge open PR `#1608` (`c887434791`, dropping 2 deleted `fixtures/opentofu-goldens/*` files) in `P0-03` before cutting `tools-v0.1.96`; then replace `release-rust-ci-tools.yml` with `.velnor/release.toml` + `velnor-ci release-gate` (`crates/velnor-ci/src/gate.rs:57-139`). **Needs owner:** **No** for `#1608`; **Yes** (`Q-N5`, `Q-N9`) for self-hosted attestation and `runner-v` freeze.

### 3.5 Rank 5: Repository lints (`Alint`, `Jactionlint`, generator samples) sit outside `Required`
- **Evidence:** `protect-main` requires only `Required` (`rust-ci.yml:571-612`), which depends only on `[rust, policy]`. `ci.yml`'s `Alint` and `Jactionlint` jobs never gate merges, `ci.yml`'s `Cargo Deny` and `Cargo Machete` duplicate `rust-ci.yml`'s `policy` job (`:405-558`), and `lint-generator-samples` (`mise.toml:51-78`) runs in no workflow `[C]`.
- **Fix and location:** Declare `[[lint]]` entries (`alint` with `probe = "scripts/alint-line-cap-probe.sh"`, `jactionlint`, and `mise-task` `lint-generator-samples`) in `.velnor/rust-ci.toml` (`crates/velnor-rust/src/lint_jobs.rs:9-84`); `crates/velnor-rust/src/required_job.rs:144` automatically adds every `lint-*` job to `Required.needs`. Delete `ci.yml`. **Needs owner:** **No**.

### 3.6 Rank 6: Dead `StepKind::GeneratorCheck`, stale sample, and `.velnor/config.toml` masking unmarked drift
- **Evidence:** `StepKind::GeneratorCheck` (`crates/velnor-model/src/steps.rs:108`, rendered as `velnor-gen check` at `crates/velnor-render/src/helper_steps.rs:74-80`) has `0` callers in `velnor-rust` `[C]`, and `actions/setup-velnor/action.yml` unpacks only `velnor-ci`, not `velnor-gen` `[C]`. `generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml` trails live `rust-ci.yml` by `2,051` diff lines `[C]`. Worse, `#1747` retained `.velnor/config.toml:1-93` assuming live CI read `[stacks.rust]`, when in fact `crates/velnor-ci/src/io/plan.rs:38-41` reads `.velnor/config.toml` **only** under dead `--mode <ci|qualification>`, while its presence (`ENGINE_CONFIG`, `crates/velnor-gen/src/io/workflows.rs:12`, `drifts.rs:75`) disables `Drift::Unmarked` detection on unmarked workflows!
- **Fix and location:** Unpack `velnor-gen` in `actions/setup-velnor/action.yml`, emit `StepKind::GeneratorCheck` in `crates/velnor-rust/src/plan.rs`, refresh `generator/samples/velnor-new`, move live `[[inputs]]`/`non_inputs` into `.velnor/rust-ci.toml`, and delete `.velnor/config.toml` plus the 8 orphaned engine-port subcommands in `crates/velnor-ci/src/cli.rs:77-185` (`P0-02`). **Needs owner:** **No**.

### 3.7 Rank 7: Governance and specification contradictions, plus `Cargo.toml` lint/assertion regressions
- **Evidence:** `docs/goals.md:16,45,59,75` and `docs/proposed/macos-scaleset-runner.md:37-39,318-323` forbid a native Rust executor, Kubernetes, and a non-GitHub distributed cache; `docs/proposed/kubernetes-runtime.md:11-16` keeps the Turso journal while `docs/proposed/velnor-runner-architecture.md:478` deletes it; `Cargo.toml:83` leaves `let_underscore_must_use = "allow"`, and `Cargo.toml:101-107` disables `debug-assertions` and `overflow-checks` on `turso_core`/`turso_parser` (violating Ruling I064) `[C]`.
- **Fix and location:** Present the exact `docs/goals.md` diff (Section 7.3) and `macos-scaleset-runner.md` edits to the owner (`Q-N1`), reconcile `kubernetes-runtime.md` in `W-07`, and restore `let_underscore_must_use = "deny"` and `turso_*` assertions in `Cargo.toml` (`P0-05`). **Needs owner:** **Yes** (`Q-N1`) for `goals.md` and `macos-scaleset-runner.md`; **No** for `Cargo.toml`.

### 3.8 Rank 8: Broken `qualification.yml` modes, `tag-ruleset-drift.test.ts`, `hk.pkl`, and Mise pin split
- **Evidence:** `qualification/stable-sources.sh:84-87,142-145` calls deleted `scripts/generator-release/source-qualification.sh`; `qualification.yml:476-518` (`submodule`) has `.gitmodules:1-4` with no gitlink in `HEAD`; `scripts/tag-ruleset-drift.test.ts:168-180` asserts deleted `config.checks` in `.velnor/config.toml`; `hk.pkl:20` invokes `scripts/hk-workspace-admission` needing missing `rust-build-guard`; `.velnor/workflows.toml` pins Mise `2026.11.1` vs `2026.11.10` in `crates/velnor-render/src/steps.rs:42` `[C]`.
- **Fix and location:** Clean all five residuals in `P0-05`. **Needs owner:** **No**.

### 3.9 Rank 9: `rust-cache-trim.yml` crash on merged/closed PRs and `Cache` job run-failure semantics (`N13`)
- **Evidence:** `Rust cache cleanup` run `44585119533` failed (`exit 1`) at `crates/velnor-ci/src/delta_trim.rs:67` (`run.pull_requests.len() != 1`) because GitHub returns `pull_requests: []` once a PR merges/closes `[M, C]`, leaking `velnor-delta-pr<N>-*` entries. Separately, `Cache` (`rust-ci.yml:645-666`) is excluded from `Required.needs` per `goals.md:67`, yet when `velnor-ci report --check` exits `1` (`17/60` sampled runs `[M]`) it marks the overall workflow run `failure`.
- **Fix and location:** Resolve the PR number from `head_branch`/commits API in `delta_trim.rs:58-80` and prune closed-PR deltas cleanly (`P0-01`); once `Velnor / Details` lands, surface `Cache` waste as its own check run without poisoning workflow conclusions. **Needs owner:** **No**.

### 3.10 Rank 10: Fail-safe full-workspace degradations (`fetch-depth: 2`, unmapped paths, unconditional `local-mode` lint)
- **Evidence:** `PLAN_DEPTH = 2` (`crates/velnor-rust/src/plan.rs:36`) causes `crates/velnor-ci/src/io/git.rs:66-79` to fall back to `AllTrigger::BaseUnavailable` on multi-commit pushes and multi-PR merge groups; unmapped root/script files trigger `AllTrigger::UnknownPath` (`crates/velnor-rust/src/select.rs:272-275`); and `Lint (features: local-mode)` (`rust-ci.yml:295`) runs `--workspace` on **every** non-empty PR even when `velnor-runner-host` is unselected `[C]`.
- **Fix and location:** Fetch missing `base_sha` on demand in `io/git.rs:66-79` (`P0-05`), map non-Rust repo files via `[[inputs]]`/`non_inputs` in `.velnor/rust-ci.toml` (`P0-02`), and gate `ClippyFeatures`/`DocFeatures` in `crates/velnor-rust/src/root_jobs.rs` on lane `packages` (`P0-02`). **Needs owner:** **No**.

## 4. Speed on the new baseline

### 4.1 Where the time goes now (C's post-cutover sample and live `0eb2d76a5c` step timings)

Cutover `#1523` (`053fefa6a3`) and deletion `#1747` (`57b65af0db`) **already removed the #1 pre-cutover bottleneck**: `0` of `2,660` sampled `Rust CI` jobs ran on GitHub-hosted runners (`C§3.3` `[M]`), cutting hosted queue share from `93 to 97%` (`57 min` worst-case gate wait) to `0%` (`13 to 15%` self-hosted scale-set spin-up: `2 to 13 s` P50, `13 to 57 s` P90, `73 s` burst max `[M]`).

Two facts in C (`C§1, §2, §6`) must be decoded before setting targets (`principle-explain-the-number`):
1. **C's PR `1.1 min` overall P50 (`2.9 min` heavy P50, `rust-root` `67 s` P50) is NOT a green test verdict.** Only `4` of `173` merged PRs carried `ci:run`/`ci:auto` (`C§1`), so `97%` skipped tests under `pr_ci = "label"`, `226` of `268` failed early, and earlier runs in the `4.3 h` window still ran the pre-`#1290` 7-root layout.
2. **C's push `7.5 min` heavy P50 (`11.3 min` P90) ran BEFORE `#1747` deleted `450k` lines (`13` crates) AND ran with a `0%` `Target` cache hit rate.** Because `0` heavy `main` pushes succeeded (`C§2`), `Save the target cache` (`rust-ci.yml:315`, gated on `success()`) never executed, leaving both `Restore the target cache` (`rust-ci.yml:165`) and `Restore the delta cache` (`rust-ci.yml:180`, gated on `steps.target_root.outputs.cache-matched-key != ''`) `100%` cold.

Exact step-by-step breakdown of live `main` push run `44584861409` on `0eb2d76a5c` (`18` crates, `4` selected, `rust` job `305 s` `[M]`):

| Stage / step in `rust-ci.yml` | Measured on `0eb2d76a5c` (`44584861409`) | Full 18-crate / PR impact | Limiter and code cause |
|---|---:|---:|---|
| Scale-set pod queue (`ubuntu-26.04-scale-set`) | `6 to 13 s` P50 (`57 s` P90) `[M]` | Every job (`rust`, `policy`, `Required`) | Ephemeral ARC pod + `docker:dind` sidecar startup (`C§3.2`) |
| Job start -> `Format` (`checkout`, `setup-velnor`, `plan`, `tools-local`, `mise`, `sources`, `target` miss) | **`18 s`** (`tools-local` = `0 s`, `sources` = `5 s`) `[M]` | Same on PR and push | Already beats `goals.md:57` (`<= 30 s`); `sources` tar unpack is `5 s` |
| `Format` + default `Lint` (`clippy --workspace --all-targets`) | `3 s` + **`31 s`** = `34 s` `[M]` | Every non-empty PR & push | Warm `VELNOR_MBX_SHARED_DIR` hit, cold `target/`; compiles all 18 crates (HD11) |
| `Documentation` (`cargo doc --workspace --no-deps`, post-merge only) | **`31 s`** `[M]` | `merge_group` & `push` only | Cold `target/doc` (`target` cache empty on `main`) |
| `Check <crate> alone` (`18` steps at `rust-ci.yml:214-267`, post-merge) | **`52 s`** (`4` selected crates, `11-15 s` each) `[M]` | `~180 to 230 s` when all `18` selected `[E]` | Per-crate feature graph differs from `--workspace`, missing shared `.rmeta` |
| `Test` + `Test (features: local-mode)` | Crashed at `12 s` + `12 s` `[M]` | `~35 to 210 s` once fixed `[E]` | `invocation.rs:85` `--no-fail-fast`/`--no-run` bug; double `--no-run` pass; `velnor-ci-serial` `max-threads = 1` (`.config/nextest.toml:115`); `receipt_integrity` (`924.8 s` CPU) |
| **Feature-lane tax:** `Test lane guard` + `Lint (local-mode)` + `Documentation (local-mode)` | **`62 s` + `26 s` + `46 s` = `134 s`** `[M]` | `26 s` on **every** PR; `134 s` post-merge | `lane-guard` runs `cargo-nextest list` twice (`rust-ci.yml:291-293`); `Lint (local-mode)` is NOT gated on `velnor-runner-host` selection; mutates shared `target/` |
| Serial tail: `policy` (`needs: [rust]`) -> `Required` (`needs: [rust, policy]`) | `25 to 78 s` + `9 to 48 s` `[M, C§3.1]` | Adds **`34 to 126 s`** after `rust` finishes | `crates/velnor-rust/src/lane.rs:200` serializes `policy` behind `rust` |

### 4.2 Next five speed levers in order of saved minutes per merge

1. **Verdict reuse across `velnor verify` -> PR -> `merge_group` -> `main` (`saves ~6.0 to 11.0 min` push-to-merge wall and `~8 to 14` runner-min per merge `[E]`).**
   - *Mechanism:* Enabling `[ci] skip_push_after_merge_group = true` (`crates/velnor-ci/src/merge_group.rs:45-86`) immediately eliminates redundant `main`-push compile/lint/doc/alone/test execution after a green `merge_group` (`saves ~4.5` runner-min per merge for 1 line of TOML). Landing `velnor-ledger` (`V01` to `V05`), `Velnor / Verified` (`L02`), and pre-push `velnor verify` (`AN04`) converts `60 to 75%` of `merge_group` runs and pre-verified PR heads into `< 10 s` ledger hits (`saves ~3.5 to 4.5 min` queue wall + `~2.5 to 6.0 min` PR verdict wall `[E]`).
   - *How to measure:* `velnor-ci bench collect` + `velnor-ci bench compare` + the GraphQL PR/queue timeline query (`C§7`) across `50` consecutive queue-merged PRs, reporting P50/P90 push-to-merge (gross and net of GitHub's queue step) and total runner-minutes per merge.

2. **Zero merge-queue wait floor (`min_wait: 0`), `allow_auto_merge: true`, and demand-only `velnor:verify` (`saves ~5.5 to 12.5 min` push-to-merge wall and `~2 to 4` wasted runner-min per merge `[E]`).**
   - *Mechanism:* Ruleset `24396608` currently sleeps `min_entries_to_merge_wait_minutes: 5` (`C§4` `[M]`) on every singleton merge, while `allow_auto_merge: false` added `7.5 min` P50 human/agent enqueue lag pre-cutover (`evidence.md` `[M]`). Setting `min_wait: 0` + `allow_auto_merge: true` removes `5.0 to 12.5 min` of pure idle wait, while skipping `rust`/`policy`/`lint-*` on unlabelled pushes eliminates the `40%` (`108/268` `[M, C§2]`) superseded PR run load.
   - *How to measure:* GraphQL intervals (`status green -> AddedToMergeQueueEvent -> merge_group start -> MergedEvent`) and cancelled-run runner-minutes per merged PR.

3. **Warm `main` base cache + XFS reflink generations (`C04`/`C05`) + feature-lane isolation (`saves ~2.5 to 4.5 min` per active run, `~4.0 to 7.0 min` per merge `[E]`).**
   - *Mechanism:* Repairing `main` (`P0-01` to `P0-03`) populates `velnor-tgt-rust.root` and activates `#1107` PR deltas. Gating `ClippyFeatures`/`DocFeatures` on `velnor-runner-host` selection (`P0-02`) saves `26 s` on every non-runner PR and `72 s` post-merge `[M]`; feeding prebuilt metadata to `lane-guard` cuts `62 s` to `~3 s` `[E]`; skipping `inputs build`'s redundant `--no-run` preflight saves `12 to 25 s` `[M]`; and XFS reflink clones (`0.25 s` `[M]`) of separate `G_default` and `G_local_mode` generations eliminate tarball unpacks and feature-toggle recompilation.
   - *How to measure:* `ci-metrics.json` `invocations[].elapsed_seconds`, `velnor-ci report --check` `compiled` counts, and benchmark `S6` (`300` replayed diffs at idle and `80%` worker load vs cold control).

4. **Parallelise `policy` + `[[lint]]`, remove `velnor-ci-serial` (`max-threads = 1`), split `[[test_groups]]`, and record `rust-ci-timings.json` (`saves ~1.5 to 4.0 min` per active run, `~2.5 to 5.5 min` per merge `[E]`).**
   - *Mechanism:* Removing `needs: [rust]` from `policy` (`crates/velnor-rust/src/lane.rs:200`) takes `25 to 78 s` directly off the critical path `[M]`, while folding `ci.yml` into `[[lint]]` saves `~2` runner-min of duplicate `cargo-deny`/`machete`. Isolating `velnor-ci` tests (`42,264` lines `[C]`) and deleting `velnor-ci-serial = { max-threads = 1 }` (`.config/nextest.toml:17,115` `[C]`) cuts `velnor-ci` test wall from `~62 s` on 1 thread to `< 10 s` across 16 cores `[E]`. Splitting the 3 Docker binaries into `[[test_groups]]` and committing `.velnor/rust-ci-timings.json` lets `shape = "auto"` parallelize build/lint and long test groups.
   - *How to measure:* `velnor-ci timings --repo tailrocks/velnor-new`, `ci-metrics.json` `slowest_tests`, and per-binary `BM08` wall/CPU across `30` runs.

5. **Warm sandbox pools (`velnor-dispatch` + `velnor-node`) + bare Git mirror (`C03`) + digest-promoted releases (`R04`) (`saves ~0.5 to 1.5 min` per merge and `~3.8 min` per tools release `[E]`).**
   - *Mechanism:* Replaces ARC scale-set pod queue (`6 to 13 s` P50, `57 s` P90 `[M]`), default `docker:dind` startup (`3 to 4 s`), `actions/checkout` (`2 to 6 s`), and `sources` tar unpack (`5 s` `[M]`) with `<= 1 s` warm sandbox claim and `< 1 s` reflink checkout, and shrinks `tools-v*` releases from `4.1 min` P50 (`C§2` `[M]`) to `~15 s` `[E]`.
   - *How to measure:* OpenTelemetry stage spans (`DG11`) and benchmark `S2` (`>= 30` interleaved pairs per job kind at `15%` and `80%` worker CPU).

### 4.3 Honest re-derived targets from the single-workspace `0eb2d76a5c` baseline

With one 18-crate workspace (`Cargo.toml:5-24`), `#1365`'s multi-root split ("47% helper/generator roots") and its `285 to 347` runner-min baseline no longer exist. PRs stratify into **Class L** (leaf/generator/helper closures, `1 to 8` crates), **Class R** (`velnor-runner-*`/`velnor-local` closures, carrying `181.6k`-line `velnor-runner-host` until `F01`), and **Class W** (whole-tree triggers: `Cargo.lock`, `.velnor/rust-ci.toml`, `mise.lock`, `~20%` of commits). Because a single full `velnor-gen` run already takes `~6 to 10` runner-min rather than `53 to 65`, the runner-minute target tightens from `<= 50` to **`<= 25` (stretch `<= 15`)**, while wall targets reflect `--workspace` compilation until Section 8.3 lands:

| Milestone stage | PR verdict P50 / P90 (Class L; all PRs) | Single push-to-merge P50 / P90 (gross; net of ~45 s GitHub queue) | 2-verdict agent PR cycle P50 (was `94 -> 34 -> 4.5 min`) | Runner-min / merged PR |
|---|---|---|---|---:|
| **Today (`0eb2d76a5c`)** | `1.1 / 5.8 min` `[M]` *(unlabelled false green / red)* | `0.6 / 3.8 min` `[M]` *(admin bypass on red, invalid)* | Invalid (`0` verified merges `[M]`) | `~12` *(failing)* |
| **Stage A: Green `main` + Phase 0 on `velnor-gen`** (`ALLGREEN`, `min_wait=0`, auto-merge, `skip_push_after_merge_group`, parallel `policy`/`[[lint]]`, `T03'`) | `2.2 min` (`Class L`), `3.2 min` (all P50) / `7.0 min` (P90) `[E]` | `6.5 min` (`5.7 min` net) / `16.0 min` `[E]` | **`10.5 min`** (`2 x 3.2 + 0.1 + 3.2 + 0.8` `[E]`) | `<= 18` `[E]` |
| **Stage B: `Velnor / Verified` + `velnor-ledger` + XFS reflink generations + 2 witnesses + cold A1** | `<= 45 s` (`Class L`), `<= 75 s` (all P50) / `<= 3.5 min` `[T]` | `<= 2.5 min` (`~65 s` net, `60-75%` queue tree hit) / `<= 7.0 min` `[T]` | **`<= 4.0 min`** (`2 x 1.25 + 0.1 + 0.6 + 0.8` `[T]`) | `<= 25` *(2 witnesses + A1)* `[T]` |
| **Stage C: `velnor verify`/`ship` pre-push + `F01` (`velnor-runner-host` deleted) + per-package compile/binary-digest keys** | `<= 15 s` (ledger hit + C5) / `<= 90 s` `[T]` | `<= 75 s` (`~30 s` net, GitHub-queue-bounded) / `<= 3.5 min` `[T]` | **`<= 2.0 min`** (`2 x 0.25 + 0.1 + 0.5 + 0.8` `[T]`) | `<= 15` `[T]` |

## 5. What to keep, change, drop

### 5.1 Group-by-group audit of E's 174-item classification (plus post-`a619478fb2` corrections)

Report E (`E§6.2, §6.3, §8`) split the `174` delivery items into `9` MARK-DONE, `7` DROP, `42` REWRITE, `22` GATE, `1` DEFER, and `93` KEEP, plus `N01` to `N24`. Auditing against `0eb2d76a5c` (6 commits newer than E's `a619478fb2`), **we agree with 158 of E's 174 dispositions** and revise **16 items across 8 concrete corrections**:

- **`MARK-DONE` (E listed 9: `B03, B08, B10, B11, B12, A09, G04, T07, T06`):**
  - **Agree (8):** `B03` (`0` hosted Rust CI jobs), `B08` (`CancelOlderPullRequestRunsExceptUnrelatedLabels`), `B10` (Darwin legs deleted in `#1550`), `B11` (`qualification.yml` dispatch-only, `job-image-candidate` deleted), `A09` (`ALLOWED_ACTIONS` render check at `rules.rs:12-36`, minus reconciling its 15 unique entries in `N20`), `G04` (`#1523`/`#1747` cutover complete; A1 push-gate piece moves to `V05`), `T07` (generator already split into 6 small crates; `velnor-ci` test speed handled in `T03'`), `T06` (`tests/it` consolidated except `velnor-runner-host`, which `F01` deletes).
  - **Disagree on `B12` -> move to `REWRITE` (`0.5 h`):** `0eb2d76a5c` still hardcodes `--config build.jobs=2` and `--jobs 2` in `mise.toml:93`, `jobs = 2` in `hk.pkl:8`, and `CARGO_BUILD_JOBS=2` in `scripts/local-cargo.sh:308` `[C]`.
- **`DROP` (E listed 7: `B02, B13, B14, X13, T04, T05, S04`):**
  - **Agree on all 7, and ADD 2 more (`T02` and `V08`), making 9 DROPs:** `B02`/`B13`/`B14` (engine gone), `X13` (`0` `background: true` steps across all workflows and actions `[C]`, saving `8 h`), `T04` (`scripts/ci-fixtures.sh` deleted by `#1747`), `T05` (`non_inputs_differential.rs` deleted by `#1747`), `S04` (`Job image candidate` deleted by `#1554`). **Add `T02` to DROP** (`velnor-actions-cli` looped tests deleted by `#1747`; `velnor-runner-host`'s `receipt_integrity` dies with the Turso journal at `F01`). **Add `V08` to DROP** (merged into `V13`: with one root `Cargo.toml:5-24`, "per-root keys" equal whole-tree keys, so per-package closure keys in `V13` are the single narrowing step, saving `12 h`).
- **`GATE` (E listed 22: `P07, X01..X12, X14..X16, A06, V04, ST03, R01, F01`):**
  - **Agree on 21** (`Q-N1` gates `P07`, `X02..X12`, `X14..X16`, `ST03`, `F01`; `Q-N5` gates `A06`, `R01`; `Q-N8` gates `V04`).
  - **Disagree on `X01` -> SPLIT:** **DROP** the separate `executor/` Cargo root (`0 h`, violates `.alint.yml:150-158`), and **KEEP** the function-hash port-provenance and 400-line/crate-budget checker inside `crates/` (`2 h`, needs no owner gate).
- **`DEFER` (E listed 1: `A07`):** **Agree** (`actions/create-github-app-token` is unused by `velnor-new`'s own workflows).
- **`REWRITE` (E listed 42) and `KEEP` (E listed 93):**
  - **Agree, with 4 sharpened scopes at `0eb2d76a5c`:**
    1. **`R02` (`4 h`, down from `12 h`):** E (`E§6.2:387`) claimed `velnor-ci release-gate` refuses all non-Actions App checks; in fact `crates/velnor-ci/src/gate.rs:256-260` and `crates/velnor-ci/src/gate/exemptions.rs:136-253` **already** support `--pr-only-check` bound to a non-Actions GitHub App `integration_id` when enforced by an active ruleset with empty `bypass_actors` alongside push `Required` `[C]`.
    2. **`A08` (`2 h`):** `#1747` deleted `qualification-gen.yml`, leaving **3** `actions/github-script` calls (`qualification.yml:131,142` and `release-rust-ci-tools.yml:26` `[C]`), not 5.
    3. **`V13` + `T08` (`-6 h`):** `#1747` cut `CARGO_MANIFEST_DIR` parent climbs across `crates/` from `79` files to **`30` `.rs` files** (`2` `build.rs` + `28` test files `[C]`), `git rev-parse` to **`24` `.rs` files** `[C]`, and `SystemTime::now()` in `tests/` to **`6` files** (`now_seconds` duplicated at `crates/velnor-ci/src/io/mod.rs:289-295` and `io/budget.rs:241-247` `[C]`).
    4. **`X02`/`X03` (`-10 h`):** Reuse `crates/velnor-runner-github`'s pure scale-set types (`src/{paths,poll,acquire,jit}.rs`, `src/session/*`) directly inside `velnor-dispatch`/`velnor-executor`.
- **Missing items (`N01` to `N31`):** Adopt all `24` of E's `N01` to `N24` (elevating `N14` `.velnor/config.toml` + 8 dead CLI commands to Phase 0 because `ENGINE_CONFIG` masks `Drift::Unmarked` at `crates/velnor-gen/src/drifts.rs:75` `[C]`), and add **`N25` to `N31`**:
  - `N25`: Fix `velnor-ci inputs build` `--no-fail-fast`/`--no-run` crash (`crates/velnor-ci/src/io/audit/build/invocation.rs:79-87`) and skip redundant `--no-run` preflight when binaries are fresh.
  - `N26`: Fold `ci.yml` (`Alint`, `Jactionlint`, `lint-generator-samples`) into `.velnor/rust-ci.toml` `[[lint]]` and delete `ci.yml` + `.github/actions/velnor-verify-jactionlint`.
  - `N27`: Gate `ClippyFeatures`/`DocFeatures` (`crates/velnor-rust/src/root_jobs.rs`) on `[[test_lane]]` `packages`, and feed prebuilt state to `velnor-ci lane-guard` (`62 s -> ~3 s`).
  - `N28`: Handle empty `run.pull_requests` on merged/closed PR runs in `crates/velnor-ci/src/delta_trim.rs:67`.
  - `N29`: Fix broken `qualification.yml` (`stable-sources`, `submodule`), `scripts/tag-ruleset-drift.test.ts:168-180`, `hk.pkl:20`, and Mise `2026.11.1` vs `2026.11.10` drift.
  - `N30`: Restore `Cargo.toml:83` `let_underscore_must_use = "deny"` and `Cargo.toml:101-107` `turso_*` assertions `true`.
  - `N31`: Drop `needs: [rust]` from `policy` (`crates/velnor-rust/src/lane.rs:200`) and commit `.velnor/rust-ci-timings.json`.

### 5.2 Surviving delivery streams, rewritten cohorts, and new first items

Total implementation effort drops from `1,292` agent-hours (`delivery-plan.md:458`) to **`1,035` agent-hours** (`-257 h`):

| Stream | Status | Agent-h (was -> now) | What survives on `velnor-gen` | Rewritten first item in stream |
|---|---|---:|---|---|
| **B + G** (Phase 0 & generator contract) | REWRITE | `70 -> 46` | Green `main` (`N25`, `N26`), demand-label `pr_ci` (`B07'`), ruleset lock (`B05'`), `.velnor/obligations.json` (`G03'`), rewritten `G01'` cohorts | `P0-01`: Fix `invocation.rs:85` `--no-run` crash, Clippy, `delta_trim`, and `Alint` caps |
| **RV** (Reviews before reuse) | REWRITE | `21 -> 12` | Consolidated `RV01`+`RV02`+`RV05` (`#1324` vs `architecture.md`, `docker_spec`/`runner-core`/`launch`) and `RV03`+`RV04` (`gate.rs` + `velnor-local`) | `W-07`: Unified `RV01`/`RV02`/`RV05` verdict, `#1324` reconciliation, and naming ADR |
| **P** (Platform trait & backends) | KEEP (gated `Q-N1`) | `58 -> 50` | `crates/velnor-{platform,platform-docker,platform-k8s}`, private `dind` (`P05` for the 3 Docker binaries), backend kit (`P06`), `velnor-local` on Docker (`P07`), `P08`/`P09` | `W-09`: `velnor-platform` capability model + `velnor-platform-docker` refactoring `docker_{client,spec}` |
| **O** (GitHub Official executor) | KEEP | `33 -> 29` | `crates/velnor-official` (`O01` to `O04`, wrapping `.velnor/runner.lock` `2.338.0`+ with `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`) | `W-10`: `velnor-official` on Docker + `velnor-conformance` differential runner (`H01`) |
| **X** (Velnor Native executor) | REWRITE (gated `Q-N1`) | `140 -> 114` | `crates/velnor-{executor,exec,dispatch}` in root workspace; reuse `velnor-runner-github` scale-set types (`-10 h`), drop `X13` (`-8 h`), ship `X06b` (`Sdk/Expressions`) | `X02'`: Message-session decrypt, run-service `acquire/renew/complete` (`X03`), and Results Twirp (`X04`) |
| **A** (Native actions) | REWRITE (gated `Q-N1`) | `49 -> 38` | `crates/velnor-native-actions` covering the 13 active actions in `ALLOWED_ACTIONS`; `A09` done, `A07` deferred | `A08'`: Replace the 3 `actions/github-script` uses (`qualification.yml:131,142`, `release-rust-ci-tools.yml:26`) with `velnor-ci` |
| **H** (Conformance) | KEEP | `58 -> 54` | `crates/velnor-conformance` seeded by generated `qualification.yml` (`64` jobs) + `local/conformance/*.json` + `local/scenarios/velnor-new.json` | `H01` (shipped with `O01` in `W-10`) |
| **L** (App & label gate) | REWRITE | `47 -> 44` | `crates/velnor-control` (`Velnor / Verified` + `Velnor / Details`, fenced writer, 10 s reconciler); Step 3 has 2 contexts (`Required` + `Velnor / Verified`), Step 4 drops PR/group YAML | `L01`: Lab A GitHub ruleset/queue semantics & partition proof (once `D7` creates App) |
| **V + DG** (Ledger, audits, Dagger keys) | REWRITE (gated `Q-N8`) | `198 -> 174` | `crates/velnor-ledger` on `velnor-model` `ObligationKey` (`DG01`); Stage 1 `skip_push_after_merge_group`; `V08` merged into per-package `V13`; `DG08` starts from `GeneratorCheck` | `V01`: Ledger core (`ObligationKey`, 2-witness slots, epochs, singleflight joins) |
| **AN** (Agent-native CLI/daemon) | KEEP | `184 -> 178` | `crates/velnor-{cli,daemon}` (`velnord`, `velnor cargo`, `velnor verify`/`ship`/`check`/`resolve`/`wait`), ordered after `cargo_wrapper` config key | `AN01'`: `velnord` + `velnor cargo` count-only journal + `cargo_wrapper` key in `velnor-gen` |
| **C** (Generations, mirrors, checkout) | REWRITE | `69 -> 54` | Extend `tools-local` (`C01'`) and `crates-mirror` (`C02'`); add bare Git mirror (`C03`/`C10`) and XFS reflink generations (`C04`/`C05`/`C12`) porting `inputs.rs` + `dep_info.rs` | `C04`: Provision XFS work volumes (`ftype=1,reflink=1`, project quotas) on workers |
| **ST** (Storage & native cache) | KEEP (gated `Q-N1`) | `60 -> 54` | `crates/velnor-storage` + `crates/velnor-cache` (promoting `velnor-cache-rules` + `velnor-local/src/protocol.rs` over NVMe L0 + `RustFS` L1) | `ST01`: `BlobStore` trait + local NVMe XFS and S3/RustFS backends |
| **T + S** (Test floor & cluster) | REWRITE | `54 -> 32` | `T02, T04, T05, T06, T07 (gen), S04` dropped/done; keep `T03'` (`velnor-ci-serial` removal), `T08'` (clock injection + 28 test climbs), `S01` to `S03` | `T03'` (in `P0-05`): Isolate `velnor-ci` tests & delete `velnor-ci-serial` in `.config/nextest.toml:17,115` |
| **R + F** (Release & final deletion) | REWRITE (gated `Q-N5`, `Q-N1`) | `49 -> 38` | `#1608` -> `.velnor/release.toml` -> `R04` digest promotion -> `R06` post-link ELF note -> `F01` (deletes ARC, `velnor-runner-host` Turso journal, `runner-v` release-plz group, `local-mode` lane) | `P0-03` (`#1608`), then `R02'`: Migrate tools release to `.velnor/release.toml` + `velnor-ci release-gate` |
| **BM** (Speed benchmarks) | REWRITE | `124 -> 118` | Build on `velnor-ci bench` + `ci-metrics.json` (`N10`), re-baseline on Stage A green `main` (`N09`), stratify by Class L / R / W | `N09` / `B15'`: Clean 3-merge Stage-A baseline + daily report on `velnor-ci bench` |

Rewritten `G01'` executor cutover cohorts (replacing `#1365` §5's deleted 39-job engine table):
- **Cohort 1 (`150` shadow pairs each):** `lint-alint`, `lint-jactionlint`, `lint-generator-samples`, `policy`, `Required`, `Cache`, `Cache maintenance`, and `rust-cache-trim`.
- **Cohort 2 (`300` shadow pairs):** `rust` build/lint/doc/alone/non-Docker test groups + `qualification.yml`'s non-Docker jobs.
- **Cohort 3 (`150` shadow pairs):** `rust-docker` test group (the 3 Docker binaries on `P05` private `dind`) + `qualification.yml` Docker jobs.
- **Cohort 4 (`3` dry runs + `30` shadow gates):** `.github/workflows/release.yml` (`tools-v*` and `runner-v*` digest promotion).

## 6. Phase 0 and the first ten work items, rewritten

### 6.1 Open and merged PR accounting at `0eb2d76a5c`

- **Merged in baseline:** `#1263` (`a619478fb2`), `#1274` (`61067c17fc`, `[[workspace_groups]]`), `#1743` (`94da9295e0`, deleted scale-set generation), `#1335` (`47ef96f8ea`, `CargoCommand::LaneGuard`), `#1747` (`57b65af0db`, engine deletion), and `#1748` (`0eb2d76a5c`, overlapped `nextest list`).
- **Closed:** `#1320` (closed `2026-11-19T17:57Z` after conflicting with `#1274` in `crates/velnor-ci/src/io/audit.rs` and `crates/velnor-rust/src/audit_job.rs`; tracked in `#1745`, non-blocking because single-root `velnor-new` already runs `inputs audit` inline at `rust-ci.yml:300-302`).
- **Open PRs (`gh pr list` + `git merge-tree --write-tree` against `0eb2d76a5c`):**
  - **`#1608` (`fast/1555-tools-gate`):** Clean on `.github/workflows/release-rust-ci-tools.yml` and `crates/velnor-gen/tests/it/release_workflow.rs`; conflicts only on 2 `fixtures/opentofu-goldens/*` files deleted by `#1747` `[C]`. **Land in Item 3 (`P0-03`)** before cutting `tools-v0.1.96`.
  - **`#1200` (`peer/1104-mise-platform-pins`):** Already on `crates/` paths; conflicts only in `.velnor/rust-metadata.json` and `crates/velnor-model/src/lib.rs` `[C]`. **Land in Item 5 (`P0-05`)**.
  - **`#1026` (`peer/1018-1011-inputs`, head `1968b15f96`):** Rebased onto `crates/` (`+6,061/-837`, `91` files), currently failing `Alint` and `Jactionlint` (`run 44587728466` `[M]`). **Split into two `<= 400`-line-compliant PRs after Phase 0** (helper input-scope fix first, generator gate second) ahead of `C05`; never block Phase 0 on a 6k-line PR.
  - **`#1297` (`renovate/cargo-minorpatch`):** Targets deleted `ci-helper/Cargo.lock` and deleted `crates/velnor-actions-*` `[C]`. **Close in Item 5 (`P0-05`)** with reason so Renovate opens a fresh root-workspace PR.
  - *(`#1751` is the draft PR for this panel round; untouched.)*

### 6.2 Rewritten Phase-0 ruleset JSON and why code repair precedes `B05'`

`delivery-plan.md:510-527` applied `B05` at hour 0 and required both `"Required verdict"` and `"Required"`. On `0eb2d76a5c`, `"Required verdict"` no longer exists (`ci.yml` dropped it at `v58581881`), and `velnor-ci 0.1.95` crashes every test step (`invocation.rs:85`), so locking `bypass_actors: []` at hour 0 would deadlock the fix PRs and block releasing `tools-v0.1.96`. Items 1 to 3 repair `main` and ship `tools-v0.1.96` first; Item 4 fixes `pr_ci` false greens; Item 6 applies the corrected `protect-main` (`24396608`) payload (`deletion`, `non_fast_forward`, `required_linear_history`, and `pull_request` rules preserved unchanged):

```json
{
  "enforcement": "active",
  "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } },
  "bypass_actors": [],
  "rules": [
    {
      "type": "required_status_checks",
      "parameters": {
        "strict_required_status_checks_policy": false,
        "do_not_enforce_on_create": false,
        "required_status_checks": [
          { "context": "Required", "integration_id": 15368 }
        ]
      }
    },
    {
      "type": "merge_queue",
      "parameters": {
        "merge_method": "SQUASH",
        "grouping_strategy": "ALLGREEN",
        "max_entries_to_build": 2,
        "min_entries_to_merge": 1,
        "max_entries_to_merge": 10,
        "min_entries_to_merge_wait_minutes": 0,
        "check_response_timeout_minutes": 90
      }
    }
  ]
}
```

At Step 3 (`L07'`), append `{ "context": "Velnor / Verified", "integration_id": <VELNOR_VERIFY_APP_ID> }` (2 contexts total, not 3). At Step 4 (`L08'`), remove `{ "context": "Required", "integration_id": 15368 }` from `protect-main` while keeping `Required` (`velnor-ci audit-wait`) on `main` `push` runs for `velnor-ci release-gate`.

### 6.3 The first ten work items, rewritten

#### Item 1 (`P0-01`): Repair `velnor-ci` test crash, Clippy errors, `delta_trim`, and `Alint` caps on `main`
- **Goal:** Fix every root cause breaking `Rust CI`, `rust-cache-trim`, and `Alint` at `0eb2d76a5c`.
- **Files:** `crates/velnor-ci/src/io/audit/build/invocation.rs` (`:79-87`, strip `RUN_FLAGS`/`RUN_OPTIONS` from `--no-run` preflight and skip preflight when `--no-run` prebuild already converged), `crates/velnor-ci/src/tests_run.rs` (`:139-160`, export `drop_run_only`), `crates/velnor-ci/src/delta_trim.rs` (`:58-80`, handle empty `pull_requests` on merged/closed PR runs), `crates/velnor-ci/tests/it/prepare_inputs.rs` (drop unused `OsRelease` at `:9`, split `417L <= 400L`), `crates/velnor-ci/tests/it/prepare.rs` (drop dead `Platform` at `:69`), `crates/velnor-gen/tests/it/delta_cache.rs` (split `423L <= 400L`), `crates/velnor-render/src/lib.rs` (`154L <= 150L`), `crates/velnor-rust/src/lib.rs` (`152L <= 150L`).
- **Done-when:** Both workspace Clippy commands (`default` and `--features local-mode`), `cargo nextest run --workspace --locked --profile ci`, `alint --config .alint.yml --fail-on-warning check .`, and `bash scripts/alint-line-cap-probe.sh` exit `0`; a regression test in `crates/velnor-ci/tests/it/` fails red on `--no-fail-fast` + `--no-run` before the fix and passes green after.
- **Size:** `4` agent-hours.
- **Implementation:** `claude-sonnet-5-5 @high`.
- **Review:** `claude-opus-5-5 @high`.

#### Item 2 (`P0-02`): Fold `ci.yml` into `velnor-gen`, wire `GeneratorCheck`, and delete `.velnor/config.toml` + dead engine ports
- **Goal:** Bring `Alint`, `Jactionlint`, `lint-generator-samples`, and `velnor-gen check` inside `Required`, parallelize `policy`, gate `local-mode` lints on `packages`, and delete `ci.yml`, `.velnor/config.toml`, and the 8 dead engine-port subcommands in `velnor-ci`.
- **Files:** `.velnor/rust-ci.toml` (add `[[lint]]` for `alint`, `jactionlint`, `lint-generator-samples`; migrate live `[[inputs]]`/`non_inputs` from `.velnor/config.toml`), `actions/setup-velnor/action.yml` (unpack `velnor-gen` beside `velnor-ci`), `crates/velnor-rust/src/{plan,lane,root_jobs}.rs` (emit `StepKind::GeneratorCheck`, drop `needs: [rust]` on `policy`, gate `ClippyFeatures`/`DocFeatures` on lane `packages`), `crates/velnor-gen/src/{drifts,io/workflows,io/mod}.rs` (delete `ENGINE_CONFIG` exemption), `crates/velnor-ci/src/{cli,cli_args,io/plan,plan_request,image_candidate}*.rs` (delete the 8 dead engine-port subcommands), **delete** `.github/workflows/ci.yml`, `.github/actions/velnor-verify-jactionlint/action.yml`, and `.velnor/config.toml`.
- **Done-when:** `velnor-gen check` exits `0` with zero unmarked workflows tolerated; `Required.needs` in `rust-ci.yml` contains `[lint-alint, lint-jactionlint, lint-generator-samples, policy, rust]`; `Lint (features: local-mode)` skips when `velnor-runner-host` is unselected; `rg "plan_request|config\.toml"` in `crates/velnor-{ci,gen}/src` returns `0` matches.
- **Size:** `6` agent-hours.
- **Implementation:** `claude-opus-5-5 @high`.
- **Review:** `claude-opus-5-5 @high`.

#### Item 3 (`P0-03`): Merge `#1608`, cut `tools-v0.1.96`, pin helper, refresh samples, and prove `main` green with warm `Target` save
- **Goal:** Enforce push-`Required` gating on tools releases (`#1608`), release `tools-v0.1.96` carrying Items 1 and 2, pin `.velnor/rust-ci.lock` to `0.1.96`, refresh `generator/samples/velnor-new`, and verify a green `main` push run that writes the first `velnor-tgt-rust.root` cache.
- **Files:** `.github/workflows/release-rust-ci-tools.yml` (`:92-118` from `#1608`), `crates/velnor-gen/tests/it/release_workflow.rs`, `crates/velnor-{ci,gen}/Cargo.toml`, `Cargo.lock`, `.velnor/rust-ci.lock`, `.velnor/rust-metadata.json`, `.github/workflows/{rust-ci,rust-cache-trim}.yml`, `generator/samples/velnor-new/**`.
- **Done-when:** `tools-v0.1.96` passes `gate`, `build`, `checksums`, and `release`; `velnor-gen check` and `cargo nextest run -p velnor-gen` pass (`samples/velnor-new` matches live `rust-ci.yml`); `Rust CI` push run on `main` succeeds on every job and `gh api repos/tailrocks/velnor-new/actions/caches` shows a saved `velnor-tgt-rust.root.*` entry.
- **Size:** `3` agent-hours.
- **Implementation:** `claude-sonnet-5-5 @medium`.
- **Review:** `claude-opus-5-5 @medium`.

#### Item 4 (`P0-04`): Enforce Owner Decision 12 demand-only label gate in `velnor-gen` and enable `skip_push_after_merge_group`
- **Goal:** Eliminate false-green `Required` on drafts and unlabelled ready PRs, cancel in-flight runs on `unlabeled`, ignore PR body/title `edited` runs, retire `ci:run`, and turn on `[ci] skip_push_after_merge_group = true`.
- **Files:** `crates/velnor-model/src/{plan,steps,repo_config}.rs` (add `PullRequestType::Unlabeled`, `StepKind::FailWhenUnrequested`, `pr_label`, delete `ClearRunLabel`), `crates/velnor-rust/src/{config,plan,plan_budget,required_job,report}.rs`, `crates/velnor-render/src/{events,gate,steps}.rs`, `.velnor/rust-ci.toml` (`pr_label = "velnor:verify"`, `[ci] skip_push_after_merge_group = true`), `docs/rust-ci.md`, `generator/CONFIG.md`, `generator/samples/pr-ci-label/**`, plus `tools-v0.1.97` release + repin.
- **Done-when:** Golden tests plus live/lab PR runs prove: (1) draft PR fails `Required` (`FailOnDraftRun`) with `rust` skipped; (2) unlabelled ready PR fails `Required` (`CI not requested`) with `rust` skipped; (3) adding `velnor:verify` runs the full selected suite and `velnor-ci gate`; (4) removing `velnor:verify` cancels any running PR workflow and fails `Required`; (5) `rust-ci.yml` emits `velnor-ci merge-group-passed`.
- **Size:** `5` agent-hours.
- **Implementation:** `claude-opus-5-5 @high`.
- **Review:** `claude-opus-5-5 @high`.

#### Item 5 (`P0-05`): Merge `#1200`, parallelize `velnor-ci` tests (`T03'`), and clear residual hygiene debts (`B12'`, `N18`, `N29`, `N30`)
- **Goal:** Land `#1200` (platform mise locks), lift `velnor-ci-serial` (`max-threads = 1`), remove remaining `jobs=2` caps, deepen missing shallow `base_sha`, fix `qualification.yml`/`hk.pkl`/`tag-ruleset-drift.test.ts`, close `#1297`, and restore `Cargo.toml` lint/assertion flags.
- **Files:** `#1200` files (`crates/velnor-{model,rust,gen,ci}/**`), `.config/nextest.toml` (`:17,110-117`, delete `velnor-ci-serial` after isolating any env-mutating test in `crates/velnor-ci/tests/it/`), `crates/velnor-ci/src/io/git.rs` (`:66-79`), `mise.toml` (`:93`), `hk.pkl` (`:8,20`), `scripts/local-cargo.sh` (`:308`), `.velnor/workflows.toml` + `qualification/stable-sources.sh` + `.gitmodules` + `crates/velnor-render/src/steps.rs:42`, `scripts/tag-ruleset-drift.test.ts` (`:168-180`), `Cargo.toml` (`:83,99-108`).
- **Done-when:** `cargo nextest run -p velnor-ci --profile ci` runs across all cores in `<= 12 s` `[T]`; `mise run test-maintenance-helpers` and `mise run lint-generator-samples` exit `0`; `qualification.yml` modes `submodule` and `stable-sources` have valid targets; `#1200` merged and `#1297` closed.
- **Size:** `6` agent-hours.
- **Implementation:** `claude-sonnet-5-5 @high`.
- **Review:** `claude-opus-5-5 @high`.

#### Item 6 (`P0-06`): Owner decision pack, non-bypassable `ALLGREEN` ruleset lock, and Stage-A baseline (`B01'` + `B05'` + `N09`)
- **Goal:** Record owner decisions (`Q-N1` to `Q-N11`), land approved `docs/goals.md` edits, apply Section 6.2's `protect-main` (`24396608`) JSON and `allow_auto_merge: true`, verify 3 consecutive queue merges, and commit `.velnor/rust-ci-timings.json` + `[budget]`.
- **Files:** `docs/goals.md` and `docs/proposed/macos-scaleset-runner.md` (exact owner-approved edits from Section 7.3), `.velnor/rust-ci-timings.json` (`velnor-ci timings --repo tailrocks/velnor-new`), `.velnor/rust-ci.toml` (`[budget]`), `.github/workflows/rust-ci.yml`. Owner applies ruleset `24396608` and `allow_auto_merge: true`.
- **Done-when:** `gh api repos/tailrocks/velnor-new/rulesets/24396608` matches Section 6.2 field-for-field; an unlabelled canary PR stays `mergeable_state == "blocked"`; 3 consecutive PRs merge via squash auto-merge through `merge_group` with `skip_push == "true"` on the resulting `main` pushes; `rust-budget` passes.
- **Size:** `2.5` agent-hours (plus owner action).
- **Implementation:** `claude-opus-5-5 @medium`.
- **Review:** `claude-opus-5-5 @high`.

#### Item 7 (`W-07`): Runner review verdicts, `#1324` reconciliation, and naming ADR (`RV01` + `RV02` + `RV05` + `N12` + `N16`)
- **Goal:** Reconcile `docs/proposed/kubernetes-runtime.md` (`#1324`) with `docs/proposed/velnor-runner-architecture.md`, freeze `velnor-runner-host` journal work at `v59`, classify every `velnor-runner-{core,github,host}` module as `port` or `delete-at-F01`, strip non-evergreen dated run logs from `docs/proposed/*` (`Q-N10`), and lock crate/CLI/subcommand names (`audit-wait` vs `inputs audit`, `velnor-ledger` vs `velnor-local` `ledger`, `velnor` vs `velnor-host`).
- **Files:** `docs/proposed/velnor-runner-architecture.md`, `docs/proposed/velnor-user-experience.md`, `docs/proposed/kubernetes-runtime.md`.
- **Done-when:** Zero contradictions remain across `goals.md`, `macos-scaleset-runner.md`, `kubernetes-runtime.md`, and `velnor-runner-architecture.md`; every `velnor-runner-*` module has an explicit disposition; `alint check .` passes.
- **Size:** `4` agent-hours.
- **Implementation:** `claude-opus-5-5 @high`.
- **Review:** `claude-opus-5-5 @high`.

#### Item 8 (`W-08`): Generator-emitted `.velnor/obligations.json` and port-provenance checker (`G03'` + `X01'`)
- **Goal:** Make `velnor-gen` emit `.velnor/obligations.json` and `.velnor/cli-checks.json` directly from `WorkflowPlan` and `VerdictSpec` (`1-to-1` bijection with `Required`-gated obligations), enforce freshness in `velnor-ci check-metadata`, and add the crate line-budget and function-hash provenance gate.
- **Files:** `crates/velnor-model/src/{verdict,obligation}.rs`, `crates/velnor-rust/src/{plan,obligation_plan}.rs`, `crates/velnor-gen/src/lib.rs` (`:43-51`), `crates/velnor-ci/src/io/check_metadata.rs`, `.velnor/obligations.json`, `generator/samples/*/expected/.velnor/obligations.json`, `crates/velnor-gen/tests/it/obligation_parity.rs`.
- **Done-when:** `velnor-gen generate` writes `.velnor/obligations.json` covering every `Format`, `Lint`, `Documentation`, `Check <crate> alone`, `Test`, `Test (features: local-mode)`, `Test lane guard`, `policy`, and `lint-*` obligation with exact closures, features, and `ReuseClass`; mutating or dropping any obligation fails `obligation_parity.rs` and `velnor-ci check-metadata`.
- **Size:** `6` agent-hours.
- **Implementation:** `claude-opus-5-5 @high`.
- **Review:** `claude-opus-5-5 @high`.

#### Item 9 (`W-09`): `velnor-platform` capability trait and `velnor-platform-docker` backend (`P01` + `P02` + `P03`)
- **Goal:** Build the capability-driven `Platform` trait (`provision`, `attach`, `spawn`, `signal`, `wait`, `teardown`, `observe`, `recover`) and `velnor-platform-docker` (porting `docker_client::connect_unix`, `docker_spec::audit_plan`, `HOST_NEEDLES`, and `CleanupProof::covers`), with warm container pooling and capability-off fallback tests.
- **Files:** `Cargo.toml`, `crates/velnor-platform/src/**`, `crates/velnor-platform-docker/src/**` (porting reviewed modules from `crates/velnor-runner-host/src/docker_{client,spec}.rs` and `crates/velnor-runner-core/src/{lifecycle,ownership}.rs`).
- **Done-when:** Backend conformance kit passes on Docker (warm claim, stdout/stderr framing, `SIGINT -> SIGTERM -> SIGKILL` cancellation, tri-state crash recovery `Present | AbsentProven | Unknown`, `HOST_NEEDLES` audit, and capability-off fallbacks for reflink->copy and private-dind refusal); all files `<= 400` lines (`lib.rs <= 150`).
- **Size:** `10` agent-hours (2 stacked PRs: `velnor-platform` `4 h`, `velnor-platform-docker` `6 h`).
- **Implementation:** `claude-opus-5-5 @high`.
- **Review:** `claude-opus-5-5 @high`.

#### Item 10 (`W-10`): `velnor-official` executor and `velnor-conformance` differential runner on Docker (`O01` + `O02` + `H01`)
- **Goal:** Build `crates/velnor-executor`, `crates/velnor-official` (driving the `.velnor/runner.lock`-pinned official runner inside `velnor-platform` sandboxes with `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` and strict `Reference`-role namespace isolation), and `crates/velnor-conformance` seeded by `qualification.yml` (`64` jobs) and `local/scenarios/velnor-new.json`.
- **Files:** `Cargo.toml`, `crates/velnor-executor/src/**`, `crates/velnor-official/src/**`, `crates/velnor-conformance/src/**`.
- **Done-when:** `velnor-official` executes acquired and replayed jobs on `velnor-platform-docker` in both `Production` and `Reference` roles; a `Reference` run is proven unable to write verdicts, production caches, or releases; `velnor-conformance` runs `10` self-paired baseline cases (`0` diffs) and catches a planted exit/env/masking difference.
- **Size:** `11` agent-hours (2 stacked PRs: `velnor-executor` + `velnor-official` `7 h`, `velnor-conformance` `4 h`).
- **Implementation:** `claude-opus-5-5 @high`.
- **Review:** `claude-opus-5-5 @high`.

## 7. Owner questions, rewritten

### 7.1 Status of original questions `D1` through `D26`

- **Obsolete / answered by events (remove from decision sheet): `D1`, `D9`, `D14`.** `#1345` released `0.1.81`; `#1523` moved every CI job off hosted runners; `#1747` (`57b65af0db`) deleted all 12 `velnor-actions-*` crates. (`D5` was never assigned in `owner-questions.md`.)
- **Settled by prior rulings (keep recorded): `D6`** (reference executor = Decision 19), **`D10`** (two witnesses + `ALLGREEN`; live ruleset still has `HEADGREEN` until `Q-N3` applies), **`D12`** (release bytes built once in `merge_group`/`push` and promoted by digest).
- **Re-opened because premise changed: `D13` -> replaced by `Q-N9`** (`~55` `velnor-runner-host` commits landed after `95de9b704b`, advancing `journal/schema.rs:77` to `v59`).
- **Reworded for `velnor-gen`: `D2` -> `Q-N4`; `D3` -> `Q-N3`; `D16` -> `Q-N2`; `D8` -> `D8'`; `D11` -> `D11'`; `D18` -> `D18'`.**
- **Valid unchanged (keep existing recommendations): `D4`** (`Option B`, `Sdk/Expressions`), **`D7`** (`velnor-verify` App + non-admin agent identity), **`D15`** (bad-release recovery via redeploy + 2-node cold break-glass verifier), **`D17`** (owner-signed obligation reductions only), **`D19`** (`No` pre-label speculation), **`D20`** (`1` high-clock node for `BM15` only), **`D21`** (`No` single-compile feeding two witnesses), **`D22`** (`1` rented vendor arm + GitHub larger runners for `BM03`), **`D23`** (`Off` by default for auto-verify unpushed trees), **`D24`** (`Yes`, `interactive` above PR runs within `30 CPU-min/h` cap), **`D25`** (`2`-agent cluster shell pilot after `AN06`), **`D26`** (`Not now` on native `arm64` workers).

### 7.2 Active owner decision table (`Q-N1` through `Q-N11`, plus `D7`, `D8'`, `D11'`, `D18'`)

| ID | Question for the owner | Recommendation | Safe default if unanswered | Needed by |
|---|---|---|---|---|
| **Q-N1** | Approve the exact diff to `docs/goals.md` (Section 7.3) and matching §3/§8 edits to `docs/proposed/macos-scaleset-runner.md` authorizing `Velnor Native` + `GitHub Official` behind `Executor`, `Kubernetes` + `Docker` `Platform`, tiered self-hosted `BlobStore`/native cache, `Velnor / Verified` + cold push audit, and optional `fork_prs = "refuse"`? | **Approve Section 7.3 verbatim** (keeps every hosted free-tier guarantee intact for external `velnor-gen` repos while unblocking Velnor's self-hosted stack) | **Hard gate:** `X*`, `P04`, `P07`, `ST03`, `F01` cannot merge without approval (`docs/goals.md:3`) | Item 6 (Day 0) |
| **Q-N2** *(replaces `B07`, `D16`)* | Confirm `velnor-gen` PR label flow: (a) single label `velnor:verify` (retiring `ci:run` and `ci:auto`); (b) drafts and unlabelled ready PRs run **zero** `rust`/`policy`/`lint-*` jobs and fail `Required` (`CI not requested`) until `Velnor / Verified` ships; (c) `velnor:verify` added on a draft arms automatically on `ready_for_review` (`D16`)? | **Yes to (a), (b), and (c).** Quick-set runs on unlabelled pushes wasted cluster slots and caused the false-green bug | Keep `ci:auto` spelling only if desired; **never** allow unlabelled green `Required` | Item 4 (Day 0) |
| **Q-N3** *(rewords `D3`)* | Immediately after Item 3 proves `main` green, apply Section 6.2's `protect-main` (`24396608`) JSON (`Required` bound to `15368`, `bypass_actors: []`, `ALLGREEN`, `max_entries_to_build: 2`, `min_wait: 0`, `timeout: 90`)? | **Yes, right after Item 3's green `main` push** (applying before Item 3 would deadlock `tools-v0.1.96`) | Waits on owner API call | Item 6 (Day 0) |
| **Q-N4** *(rewords `D2`)* | Revive the merge queue and flip repository `allow_auto_merge: true`, instructing agents to enable squash auto-merge at PR creation and stop catch-up merges? | **Yes.** `min_wait: 0` + auto-merge + `skip_push_after_merge_group` cuts `5 to 12.5 min` of dead wait and stops duplicate `main` builds | Waits on owner repo setting | Item 6 (Day 0) |
| **Q-N5** | Release runner & attestation policy (`R01` vs `crates/velnor-render/src/release_attest.rs:70`): move `velnor-new` releases onto self-hosted Velnor with digest promotion (`R04`) and Sigstore Fulcio/Rekor signer-workflow pinning (`allow_self_hosted_attest = true` opt-in), keeping `--deny-self-hosted-runners` as default for hosted consumer repos? | **Yes: self-hosted digest promotion for `velnor-new`**, hosted `--deny-self-hosted-runners` default preserved for external users | Keep `release-rust-ci-tools.yml` on hosted `ubuntu-24.04` + `#1608` gate | Day 3 (`R01`) |
| **Q-N6** | Fork PR policy on public `velnor-new` (Decision 8 vs `goals.md:43`): add `fork_prs = "refuse"` opt-in in `.velnor/rust-ci.toml` for `velnor-new` (forks run zero cluster jobs and fail `Required`), keeping `fork_runner = "ubuntu-latest"` as default for external `velnor-gen` users? | **Yes:** honors Decision 8 for `velnor-new` without breaking OSS consumers of `velnor-gen` | Keep `fork_runner` hosted fence (`rust-ci.yml:57`) | Day 1 |
| **Q-N7** | Workspace compile layout (Architecture §8.1 separate target dirs vs `goals.md` rules 4-5 & HD11): keep one shared generation `target/` per configuration (`G_default`, `G_local_mode`) and parallelize obligations across instant XFS reflink clones (`0.25 s` `[M]`) rather than maintaining `target/velnor-clippy` and `target/velnor-doc`? | **Yes: one generation `target/` per configuration, cloned per obligation via XFS reflinks** (zero lock contention AND zero duplicate dependency compilation) | Single `target/` per run | Day 2 (`C05`) |
| **Q-N8** | Explicitly authorize verdict-level ledger reuse (`V04`, `AN04`, `DG03`) under `goals.md` (confirming HD3's ban in `docs/hard-decisions.md:223-229` bound only the deleted engine, per `goals.md:14`)? | **Approve**, gated on the `C1` to `C6` soundness contract (`3,000` shadow checks, 2 witnesses, `2%` live A2, cold A1, epochs) | Only `skip_push_after_merge_group` active until approved | Day 2 (`V04`) |
| **Q-N9** *(re-asks `D13`)* | Policy for `crates/velnor-runner-host` (`181.6k` lines, journal `v59`) and `release-plz.toml`'s `runner-v` group until `F01`: freeze `velnor-runner-host` to P0 CI-breakage fixes only and skip building an interim `runner-v` workflow before `F01` replaces `velnor-host` with unified `velnor`? | **Freeze `velnor-runner-host` now**; build `runner-v*` in `.velnor/release.toml` directly for the new unified `velnor` runner binaries | Freeze `velnor-runner-host` feature PRs | Item 7 (Day 0) |
| **Q-N10** | Evergreen documentation rule (`AGENTS.md`) vs `docs/research/velnor-runner-2026-11/`: treat `docs/research/*` as frozen dated research snapshots while stripping dated run logs/PR numbers out of `docs/proposed/*`? | **Yes** (enforce in `W-07`) | Leave `docs/research/` untouched | Item 7 (Day 1) |
| **Q-N11** | Roadmap issue labels (`docs/roadmap.md:43` `exec:haiku/sonnet/opus` vs multi-vendor TSV): keep `exec:haiku/sonnet/opus` on GitHub issues for the unattended Claude Code loop and run multi-model adversarial reviews at milestone gates? | **Yes: keep `exec:haiku/sonnet/opus` on issues**, multi-vendor review at design/cutover gates | `exec:haiku/sonnet/opus` | Day 0 |
| **D7** | Provision GitHub App `velnor-verify` (`statuses: write`, `checks: write`, `pull_requests: read`, `contents: read`, `metadata: read`) + non-admin agent identity (confirming `Required` remains sole gate until Step 4)? | **Yes** | Blocks `L01`, `L02`, `AN04` | Hour 12 |
| **D8'** | Provision `velnor-system` deploy identity, approve worker XFS/reservation/taint/FI-LV changes, and place Kubernetes manifests under `deploy/` inside `velnor-new` (rather than external `velnor-runners`)? | **Approve all, with `deploy/` in `velnor-new`** so code, images, and manifests version atomically | Docker-only conformance until approved | Hour 12 |
| **D11'** | Approve `G01'` cutover pair counts (`150` Cohort 1, `300` Cohort 2 `rust`, `150` Cohort 3 `docker`, `3` dry runs + `30` gates Cohort 4) and `D18'` precondition (revive GitHub merge queue and measure `BM14` before any Velnor merge train)? | **Yes to both** | As reworded | Day 1 |

### 7.3 Exact proposed diff to `docs/goals.md` for owner approval (`Q-N1`)

Changing `docs/goals.md` requires explicit owner approval (`docs/goals.md:3`). The unified diff below against `0eb2d76a5c` (`docs/goals.md:9-20, 40-45, 59, 75`) preserves every hosted-runner free-tier promise for external `velnor-gen` users while authorizing Velnor's self-hosted executor, platform, cache, and ledger contracts (paired with updating `docs/proposed/macos-scaleset-runner.md` §3 and §8 to scope that file to legacy macOS scale-set operation until `F01`):

```diff
--- a/docs/goals.md
+++ b/docs/goals.md
@@ -8,15 +8,15 @@
 
 | Code | Rule book |
 |---|---|
-| New work: the Rust generator in `generator/`, the CI helper in `ci-helper/`, the runner in `crates/velnor-runner`, the local environment in `local/`, and the workflows the generator writes | This file |
-| Everything else: the existing engine in `crates/` (except `crates/velnor-runner`), its configuration in `.velnor/`, the workflows it generates, and the scripts, images and fixtures that support it. That includes this repository's own CI, Rust included, until the generator takes over the Rust part. The engine is maintained, not developed | [`hard-decisions.md`](hard-decisions.md), with the specifications under `proposed/` and the procedures under `implemented/` |
+| Active code: `velnor-gen`, `velnor-ci`, `velnor-model`, `velnor-rust`, `velnor-render`, `velnor-release`, `velnor-workflows`, `velnor-cache-rules`, `velnor-local`, the Velnor runner and ledger crates under `crates/velnor-*`, and every generated workflow and obligation inventory | This file |
+| Maintained residuals (`crates/velnor-archive-guard`, `qualification/`, remaining `scripts/` and `.velnor/version-policy.toml`) | [`hard-decisions.md`](hard-decisions.md), with the specifications under `proposed/` and the procedures under `implemented/` |
 
 The root `AGENTS.md` applies to all code. Hard decisions bind the existing engine only. Where new work needs the same choice, this file states it.
 
-New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. Jobs are always executed by GitHub's official runner: on GitHub's machines, or on the project's own machines inside the containers that `velnor-runner` starts.
+New work serves Rust projects on GitHub Actions, on GitHub-hosted runners or on self-hosted Velnor runners (Kubernetes and Docker). On GitHub-hosted runners, jobs execute on GitHub's official runner. On Velnor runners, jobs and verification obligations execute behind one `Executor` interface: **Velnor Native** (written in Rust, the default) or **GitHub Official** (the unmodified latest official runner, selectable per job or obligation kind and maintained permanently as the conformance oracle). Executor identity is part of every verification and cache key.
 
 ## Vision
 
-Velnor has two parts. The generator (`velnor-gen`, with its helper `velnor-ci`) writes the GitHub Actions workflows a project needs; they run on GitHub-hosted runners or on self-hosted runners. The runner (`velnor-runner`) runs jobs on the project's own machines. `velnor-local` runs a checkout's workflows on a developer's machine without pushing, imitating GitHub, and starts each job through the runner.
+Velnor has two parts. The generator (`velnor-gen`, with its helper `velnor-ci`) reads a Rust project and emits its GitHub Actions workflows and its versioned obligation inventory. The runner executes those workflows and obligations on the project's own machines (Kubernetes clusters or a local Docker host), backed by content-addressed caches, node-local mirrors, warm build generations, and a witnessed verification ledger reported through `Velnor / Verified` or the workflow's `Required` check. Local mode runs the same workflows and obligations on a developer's machine on the Docker backend and checks conformance against GitHub's official runner.
 
 The runner is developed as a product in its own right, because local mode and self-hosted runs depend on it.
@@ -38,10 +38,10 @@
 ## What we never compromise
 
-1. **Correct before fast.** A check is skipped only when the change provably cannot affect it. When Velnor cannot tell what a file affects, it runs the checks.
-2. **Green means verified.** The required check passes only when everything that was selected ran and passed. A test step that should have run tests and ran none is not a pass.
+1. **Correct before fast.** A check is skipped or satisfied from the verification ledger only when every input that can affect it is proven identical and witnessed under the active epoch. When Velnor cannot tell what a file affects, it runs the checks.
+2. **Green means verified.** The required check or `Velnor / Verified` commit status passes only when every selected obligation for that exact tree ran (or matched a witnessed, unrevoked ledger record with identical full-input key, with clock/network-sensitive checks run fresh) and passed, and every commit landed on the default branch is audited cold. A test step that should have run tests and ran none, or a pull request where CI was not requested, is never green.
 3. **A broken cache is slow, never wrong, and never silent.** A missing, rejected or stale cache takes a correct cold path. A cache that fails at its job is reported by a red check, as design rule 3 defines.
-4. **Secrets never reach the build.** Private dependencies are fetched with a short-lived token that can read only the repositories the project lists. The token is revoked before any code of the project or of its dependencies runs, and cargo then runs offline. Checkout stores no credentials, and the job's `GITHUB_TOKEN` has read-only permissions while that code runs. A pull request from this repository writes a cache only into its own scope. A pull request from a fork gets no token and only reads caches. Where a fork can read the caches, as in a public repository, nothing fetched from a private dependency or built from it is cached.
+4. **Secrets never reach the build.** Private dependencies are fetched with a short-lived token that can read only the repositories the project lists. The token is revoked before any code of the project or of its dependencies runs, and cargo then runs offline. Checkout stores no credentials, and the job's `GITHUB_TOKEN` has read-only permissions while that code runs. A pull request writes caches or candidate records only into an isolated scope that never enters `main`'s trusted scope without cold verification. A project may either fence fork pull requests onto read-only hosted runners without tokens (`fork_runner`, the default) or refuse fork pull requests outright (`fork_prs = "refuse"`). Where a fork can read caches, nothing fetched from or built with a private dependency is cached.
 5. **Every speed claim has a measurement.** A change states its effect with numbers from real runs, before and after.
-6. **Inside GitHub's free limits.** The design fits in GitHub's 10 GB of cache and on standard hosted runners. It does not depend on paid storage or larger machines.
+6. **Inside GitHub's free limits on hosted runners; configurable tiers on self-hosted runners.** On GitHub-hosted runners, generated workflows fit inside GitHub's 10 GB cache (at most 8 GB used) and standard hosted machines. On self-hosted Velnor runners, Velnor may serve `actions/cache`, artifacts, and build generations from configurable local NVMe and S3-compatible (`RustFS`) storage tiers with identical GitHub scoping rules.
 
 ## Targets
@@ -57,5 +57,5 @@
 | Time from job start to the first cargo command, cache restore included | At most 30 seconds |
 | Number of jobs | Does not grow with the number of crates in a workspace |
-| Cache storage | At most 8 GB of GitHub's 10 GB, pull request layers included |
+| Cache storage | On GitHub's cache service: at most 8 GB of 10 GB, pull request layers included; on self-hosted Velnor storage: bounded by each site's configured volume budget |
 | An ordinary pull request, compared with the previous Velnor version | Neither the wait for the required check nor the runner-minutes gets worse. The one exception is a check that correctness needs, with its cost measured and accepted by the owner |
 | The generator's own tests | Under 1 minute |
@@ -73,5 +73,5 @@
 9. **No logic in YAML.** Every generated `run:` step is one command. A generated workflow uses only actions from a fixed list, each pinned to a full commit SHA. Decisions and loops live in one small helper program, written in Rust and shipped prebuilt.
 10. **Configuration matches reality.** Velnor checks its configuration against the repository, such as the name of the default branch. A mismatch fails the run.
-11. **Local first.** A workflow is verified on the developer's machine with GitHub's official runner, started by the Velnor runner, a local stand-in for GitHub and a local cache that enforces GitHub's rules. Real GitHub confirms it afterwards.
+11. **Local first.** Workflows and obligations run locally on the Docker backend, and executor changes pass differential conformance against GitHub's official runner and a local cache enforcing GitHub's rules before reaching production. Real GitHub confirms afterwards.
 12. **Proven on real projects.** A change is tried on the small lab project `tailrocks/velnor-lab` first. This repository runs on its own generator, and that is the final proof. The plan and the required check of a run always come from a released version, never from the change under test.
 13. **Projects stay current.** A new Velnor version reaches a project only after it has passed on the lab project, and it is merged only with the project's own checks green. Velnor's own programs are released only from a tag on a commit of the default branch whose required check passed. The job that publishes checks out nothing and runs no code of the repository. It publishes files that a job without write permission built, after checking their digests.
```

## 8. Be ambitious

Treating `velnor-gen` as the foundation rather than an external YAML writer makes five capabilities possible on the hardest problems in CI/CD that neither generic Actions runners nor the `#1365` runner-first plan could build, because generic runners see opaque YAML shell strings while `velnor-gen` owns the typed program graph from Cargo source analysis to final gate decision.

### 8.1 Isomorphic three-target compilation from one `WorkflowPlan` (zero parity gap by construction)

In `#1365`, `.github/workflows/ci.yml` and the obligation inventory (`G03`) were separate artifacts, forcing `L06` to spend days running `300` PR heads and `100` merge groups just to discover whether someone missed an edge between YAML and the ledger. In `velnor-gen`, `WorkflowPlan` (`crates/velnor-model/src/plan.rs:21-29`) is already a closed algebraic IR: `StepKind` has `49` typed variants (`steps.rs:28-132`), `CargoCommand` has `18` (`plan.rs:261-341`), `VerdictSpec` (`verdict.rs:71-75`) defines exact gate completeness, and `rules.rs:306-376` forbids arbitrary shell control flow.

**Design:** Compile one `WorkflowPlan` inside `velnor_gen::generated_with_notes` (`crates/velnor-gen/src/lib.rs:43-51`) into **three** byte-deterministic projections in one pass:
1. `.github/workflows/rust-ci.yml` (standard GitHub Actions YAML for hosted consumers, `velnor-official` reference execution, and break-glass cold runs);
2. `.velnor/obligations.json` (the typed DAG of `ObligationSpec` nodes executed directly by `velnor-control` and `velnor-node` once Step 4 drops PR/group YAML runs);
3. `.velnor/cli-checks.json` (the exact check table driving `velnor check <name>`, `velnor cargo`, and `hk` hooks).

A proptest suite in `velnor-gen` proves across all `39` sample fixtures and generated `RepoView` graphs that `velnor_rust::verdict::decide` (`verdict.rs:277-317`) and `velnor-ledger::ensure` accept the **exact same set of execution traces**. Drift between local agent runs, cluster obligations, and Actions YAML becomes unrepresentable.

### 8.2 Sub-20-ms checkout-free verdict resolution from Git Merkle trees and committed metadata

Content-addressed build systems normally materialize a worktree and run `cargo metadata` or syscall tracing before computing a cache key. `velnor-gen` already commits `.velnor/rust-metadata.json` (`55.6 KiB`, verified by `check-metadata`) and `.velnor/rust-inputs.lock` (`39` dep-info-audited compile paths `[C]`).

**Design:** Compute every `ObligationKey` inside `velnor-control` **purely in memory over the bare Git object database**: run in-process `merge-tree(queue_tip, pr_head)` -> read the resulting root tree OID -> walk subtree OIDs for `crates/<pkg>` plus blob OIDs in `rust-inputs.lock` plus `lock_key` (with workspace member versions stripped per `crates/velnor-rust/src/lock_key.rs:12-40`). When a developer pushes a branch already verified via `velnor verify`, rebases onto an unrelated `main` commit, or bumps workspace versions, `velnor-control` computes all `~25` obligation keys and posts `Velnor / Verified: success` in **`< 20 ms` from webhook arrival** (`0` pods claimed, `0` XFS clones, `0` processes spawned). On every *executed* witness, `velnor-node`'s LSM/eBPF file-open audit (`V07`) verifies runtime test reads against per-package read contracts on the intact checkout, revoking and widening any package that opens an undeclared path.

### 8.3 Defeating Cargo's `--workspace` feature-unification tax (HD11) and two-phase binary-digest keys

Hard Decision 11 (`docs/hard-decisions.md:486-513`) is the single largest compile bottleneck left in `velnor-gen`: Cargo `-p <pkg>` resolves features over `<pkg>` alone rather than the workspace union, invalidating `target/debug/deps` fingerprints. As a result, `velnor-gen` compiles `--workspace` (`all 18 crates`) even when a PR edits a 6-file leaf crate (`velnor-cache-rules`), and `[[test_lane]]` (`--features local-mode`) recompiles `--workspace` a second time in the same `target/`.

**Design (two compiler-level breakthroughs owned by `velnor-gen`):**
1. **Generator-verified workspace-unified `-p` compilation:** `.velnor/rust-metadata.json` already holds the complete workspace feature union. `velnor-rust` can emit either a tiny synthetic workspace-feature-anchor target or Cargo `--unit-graph` unit-hash verification proving when `-p <selected>` preserves **byte-identical Cargo unit hashes** to `--workspace`, policed on every run by `velnor-ci report --check` (`ci-helper/docs/rust-ci-report.md:44-54` `[C]`). That restores `O(selected closure)` compilation on cold/lockfile-miss runs without waiting for upstream Cargo `-Zfeature-unification` stabilization, while separate XFS reflink generations (`G_default` and `G_local_mode`) eliminate `[[test_lane]]` feature thrashing completely.
2. **Two-phase `BuildTests -> RunByBinaryDigest` obligations with post-link ELF stamping:** Split `NextestRun` in `obligations.json` into `BuildTests(closure_key) -> Vec<(BinaryId, Sha256)>` and `RunTests(binary_sha256, runtime_read_contract_oid, filter, seed)` (with `VELNOR_COMMIT` written into `.note.velnor.commit` *after* linking for release binaries, `R06`). Whenever a change touches docstrings, formatting, dead items, or an internal detail that leaves downstream test binary machine code unchanged (confirmed across twin builds), `RunTests` hits the ledger by binary SHA-256 and skips executing `velnor-runner-host`'s multi-minute integration binaries with cryptographic certainty.

### 8.4 Disjointness-aware speculative merge-train lattice

Under `ALLGREEN`, a conventional merge queue is linear: if `PR1` in `[PR1, PR2, PR3, PR4]` fails, work on `PR2..PR4` is thrown away or re-run from scratch. Because `velnor_rust::select` (`crates/velnor-rust/src/select.rs:16-40`) and `.velnor/obligations.json` are pure functions of `(base_tree, head_tree)`, `velnor-control` can construct both `[PR1, PR2]` and the ejection fallback `[PR2]`-without-`PR1` in milliseconds via `git merge-tree` and partition obligations into **disjoint** vs **overlapping** sets:
- Whenever `select(PR1)` and `select(PR2)` are disjoint (for example `crates/velnor-render` vs `crates/velnor-runner-core`), per-package `ObligationKey` values for `PR2`'s closure are **identical** in `[PR1, PR2]` and `[PR2]`, so a single two-witness run certifies **both** branches of the ejection tree (`0` wasted CPU).
- Only obligations in `select(PR1) ∩ select(PR2)` schedule a batch-priority fallback witness on `[PR2]`, turning queue ejections into `< 1 s` status promotions.

### 8.5 Profile-guided automatic test sharding and monotonic speed budgets

`velnor-gen` already prints its shape decision in the workflow header (`# one job: .velnor/rust-ci-timings.json has no build times`, `rust-ci.yml:32` `[C]`) and ships `velnor-ci timings` plus `velnor-ci bench budget`, yet sharding is hardcoded off (`SHARD_THRESHOLD_SECONDS = None` at `crates/velnor-rust/src/shape.rs:11` `[C]`).

**Design:** Close the compiler feedback loop over signed `A1` production receipts:
1. **Count-verified automatic sharding:** When `.velnor/rust-ci-timings.json` shows a test group above `30 s` P50, `crates/velnor-rust/src/shape.rs` emits `k` parallel `--partition hash:i/k` execution obligations over pre-built test binaries, and `velnor_rust::verdict::decide` (`crates/velnor-rust/src/verdict.rs:206-275`) enforces `sum_i(ran_i) == total_listed` per package so no test can ever disappear between shards (`goals.md:41`).
2. **Generator-enforced monotonic speed budget:** Weekly scheduled runs execute `velnor-ci timings` and `velnor-ci bench collect` over `A1` receipts; whenever P50 and P90 improve by `>= 15%` over `>= 30` runs, `velnor-gen` opens a PR tightening `[budget]` in `.velnor/rust-ci.toml` and updating shard counts, while `rust-budget` (`crates/velnor-rust/src/plan_budget.rs:17-47`) blocks any generator or runner change that regresses setup, compile, or verdict latency.
