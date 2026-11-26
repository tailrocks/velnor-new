# velnor-gen rebase by Gemini 4 Argon

| Field | Value |
|---|---|
| Model | Gemini 4 Argon (`gemini-4-argon`) |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high |
| Date | 2026-11-20 |
| Main tip analysed | `0eb2d76a5c` |
| Brief | [velnor-gen rebase brief v1](../briefs/velnor-gen-brief-v1.md) |

Independent answer from the Velnor runner research panel, written without seeing other members' answers. Unedited except for this header and replacing home-directory paths with `~`.

---

# Rebase of the Velnor Runner Plan on `velnor-gen` (Main Tip `0eb2d76a5c`)

## 1. Verdict in ten lines

1. **The architecture stands, and `velnor-gen` makes it dramatically simpler.** The cutover (`053fefa6a3`, #1523), workspace collapse (`23bed02e7e`, #1290/#1594), and engine deletion (`57b65af0db`, #1747, −452k lines) eliminated the 39-job `velnor-actions` graph, the 8 hosted gate jobs, and 79% hosted-queue wait (`C§3.3`).
2. **The single most important change to the plan is that `velnor-gen` + `velnor-ci` is now the control plane and obligation compiler.** Instead of scraping a 39-job engine YAML into a parallel `executor/` workspace (`docs/proposed/velnor-runner-architecture.md:240-256`), every obligation, verdict key, cache key, and gate spec is emitted directly from `velnor-model`, planned by `velnor-rust`, rendered by `velnor-render`, and enforced at runtime by `velnor-ci`.
3. **Phase 0 is no longer "move gate jobs off hosted runners"—that is done (`C§3.3`). Phase 0 is "stop bypassing a red gate and fix `pr_ci = label`."** Post-cutover, 226 of 268 PR runs failed `Required` and 157–167 of 173 merges bypassed the gate (`C§1,§2`); meanwhile `pr_ci = "label"` turns `Required` **green without running tests** on unlabelled ready PRs and drafts (`.github/workflows/rust-ci.yml:582-612`), inverting owner decision 12 (`docs/research/velnor-runner-2026-11/owner-decisions.md:20`).
4. **The workspace is now 18 crates in one root (`Cargo.toml:5-24`), not 4 nested workspaces or 31 crates.** Per-root keys (V08) and the "47% `generator/` or `ci-helper/` PR class" (`docs/proposed/velnor-runner-architecture.md:150`) are obsolete; `velnor_rust::select::select` (`crates/velnor-rust/src/select.rs:17-40`) already computes package-closure selection, while hard decision 11 (`docs/hard-decisions.md:486-513`) compiles `--workspace` and filters nextest with `-E`.
5. **The long pole has shifted from hosted-runner queueing to a single `rust` job (`rust-ci.yml:34-404`).** Because `.velnor/rust-ci-timings.json` is absent, `decide_shape` (`crates/velnor-rust/src/shape.rs:18-33`) emits `Shape::OneJob` (P50 2.9 min / P90 6.2 min on heavy PRs, P50 7.5 min / P90 11.3 min on push, `C§2`), serialising clippy twice, test compilation, nextest, 18 post-merge `Check <crate> alone` steps, docs, and `policy`.
6. **Three immediate code bugs keep `main` red at `0eb2d76a5c` and can be fixed in one hour.** `.github/workflows/ci.yml:117` runs `cargo deny` on four deleted nested `Cargo.toml` paths; four Rust files exceed `.alint.yml:208-228` line caps (`delta_cache.rs` 423, `prepare_inputs.rs` 417, `velnor-render/src/lib.rs` 154, `velnor-rust/src/lib.rs` 152); and `ci.yml`'s lint jobs sit outside `Required`.
7. **Reuse plumbing already exists in embryonic form inside `velnor-gen` / `velnor-ci`.** `CacheKey` (`crates/velnor-model/src/cache.rs:95-113`), `inputs save/restore` (`crates/velnor-ci/src/inputs.rs:140-243`), PR build deltas (`crates/velnor-ci/src/delta_cli.rs`, `7f16e1fe15` #1107), `mbx-store`/`mbx-seed` (`crates/velnor-ci/src/mbx_store.rs:10-50`), and `merge-group-passed` (`crates/velnor-ci/src/merge_group.rs:45-86`) are live building blocks for the verification ledger.
8. **All new runner/ledger/executor crates belong in flat `crates/velnor-*` under the single root `Cargo.toml`.** The separate `executor/` Cargo root (`docs/proposed/velnor-runner-architecture.md:240`) contradicts `#1290`/`#1594` (`23bed02e7e`) and `.alint.yml:150-159` (`velnor-no-stray-cargo-lock`), and its rationale (isolating old-engine deletions) vanished with `#1747` (`57b65af0db`).
9. **Five hard governance conflicts (`docs/goals.md:16,20-22,43,59,75`, `release_attest.rs:70`, `hard-decisions.md:223-229`) must be explicitly approved by the owner before executor/cache/release cutovers begin.** Under `docs/goals.md:3` and `AGENTS.md`, no agent may silently edit `docs/goals.md` or bypass those constraints.
10. **With `velnor-gen` as foundation, total remaining delivery drops from 1,292 to ~890 agent-hours.** Phase 0 + generator-driven shape/lint consolidation cuts PR verdict P50 to ~90–120 s on the existing cluster before a single line of Velnor Native executor code runs.

---

## 2. The design mapped onto `velnor-gen`

### 2.1 Architectural rule: one pipeline, flat `crates/velnor-*`

Every feature in `velnor-gen` flows through five pure stages plus one runtime binary (`crates/velnor-gen/src/lib.rs:76-104`):
1. **Config & Facts (`velnor-model`, `velnor-rust`):** `RepoView` (`crates/velnor-model/src/repo.rs:11-17`) → `velnor_rust::detect` (`crates/velnor-rust/src/detect.rs:30-70`) + `config::parse` (`crates/velnor-rust/src/config.rs:72-140`, whitelist in `crates/velnor-rust/src/config_keys.rs:4-39`) → `RepoFacts` (`crates/velnor-model/src/repo.rs:92-108`).
2. **Plan (`velnor-rust`):** `velnor_rust::plan` (`crates/velnor-rust/src/plan.rs:41-118`) → `WorkflowPlan` (`crates/velnor-model/src/plan.rs:22-29`) containing `Vec<Job>` (`plan.rs:85-100`) and `Vec<Step>` (`plan.rs:103-114`) with `StepKind` (`crates/velnor-model/src/steps.rs:29-132`) and `VerdictSpec` (`crates/velnor-model/src/verdict.rs:71-75`).
3. **Render & Rule Check (`velnor-render`):** `rendered_job` (`crates/velnor-render/src/steps.rs:53-95`) → `check_rules` (`crates/velnor-render/src/rules.rs:233-267`, enforcing the 16 SHA-pinned `ALLOWED_ACTIONS`, one-command `run:` with no shell operators, 512-char cache keys, 100 KB env, 500 KB workflow cap) → `seal` (`crates/velnor-render/src/header.rs:23-34`, `# velnor-gen-sha256:`).
4. **Helper Gate (`velnor-gen`):** `check_helper` (`crates/velnor-gen/src/helper_gate.rs:8-62`) enforces the minimum pinned `velnor-ci` version in `.velnor/rust-ci.lock` for every config key used.
5. **Runtime Execution (`velnor-ci`):** SHA-verified launcher (`.github/workflows/rust-ci.yml:77-82`) invokes `velnor-ci` subcommands (`crates/velnor-ci/src/cli.rs:36-186`, mirrored in `crates/velnor-workflows/src/helper_call.rs:6-45`).

All new runner, ledger, storage, and executor crates live in `crates/velnor-*` in the single root workspace (`Cargo.toml:4-25`), never in a separate `executor/` root (which would violate `.alint.yml:150-159`).

### 2.2 Component-by-component mapping

| # | Design component | Where it lives (Generator-time vs Runtime vs New service) | Primary crates & files |
|---|---|---|---|
| **1** | Verification ledger & verdict keys (`ARCH §6.1–6.2`; `DG01`, `V01–V04`) | Generator model (`velnor-model`, `velnor-rust`) + runtime CLI (`velnor-ci`) + new service (`crates/velnor-ledger`) | `crates/velnor-model/src/verdict.rs`, `crates/velnor-rust/src/{compat,lock_key,verdict_plan}.rs`, `crates/velnor-ci/src/cli.rs`, `crates/velnor-ledger` |
| **2** | `Velnor / Verified` App status & `Velnor / Details` check (`ARCH §8.2`; `L01–L08`) | Generator config (`velnor-model`, `velnor-rust`) + runtime gate (`velnor-ci`) + new service (`crates/velnor-control`) | `crates/velnor-model/src/repo_config.rs`, `crates/velnor-ci/src/gate.rs:15,292-303`, `crates/velnor-control` |
| **3** | Label flow (`pr_ci`) (`ARCH §8.2`, OD 12; `B07`, `N02`) | Generator-time (`velnor-model`, `velnor-rust`, `velnor-render`, `velnor-gen`) | `crates/velnor-model/src/{plan,steps,repo_config}.rs`, `crates/velnor-rust/src/{plan,plan_budget,required_job}.rs`, `crates/velnor-render/src/{gate,draft_steps}.rs` |
| **4** | Reuse across agent run → PR → queue → main → release (`ARCH §6.3`, OD 10) | Generator-time (`velnor-model`, `velnor-rust`, `velnor-render`) + runtime (`velnor-ci`, `velnor-ledger`) | `crates/velnor-rust/src/{merge_group_skip,verdict_plan}.rs`, `crates/velnor-ci/src/{merge_group,io/plan}.rs`, `crates/velnor-model/src/selection.rs` |
| **5** | Release promotion by digest (`ARCH §8.4`, OD 10; `R01–R06`) | Generator release planner (`velnor-release`, `velnor-render`) + runtime (`velnor-ci`) | `.velnor/release.toml`, `crates/velnor-release/src/{plan,family,velnor_config}.rs`, `crates/velnor-render/src/release*.rs`, `crates/velnor-ci/src/gate.rs:57-139` |
| **6** | Agent-native `velnor verify` / `velnor cargo` / `velnord` (`ARCH §8.6`, `UX §5`; `AN01–AN14`) | Planner library (`velnor-model`, `velnor-rust`) linked into new CLI/daemon (`crates/velnor-cli`, `crates/velnor-daemon`) | `crates/velnor-rust/src/{detect,select,plan}.rs`, `crates/velnor-ci/src/io/plan.rs:42-50`, `crates/velnor-local/src/cli.rs`, `crates/velnor-cli` |
| **7** | Velnor Native executor vs GitHub Official (`ARCH §5.3`, OD 1, 19, 20; `X01–X17`, `O01–O04`) | Generator config (`velnor-model`, `velnor-rust`, `velnor-render`) + runner crates (`crates/velnor-executor`, `velnor-exec`, `velnor-official`, `velnor-native-actions`, `velnor-dispatch`) | `crates/velnor-render/src/rules.rs:16-45`, `crates/velnor-runner-github/src/*`, `crates/velnor-runner-core/src/*`, `crates/velnor-{executor,exec,official,native-actions,dispatch}` |
| **8** | Platform trait (Kubernetes + Docker) (`ARCH §5.4`, OD 13, 21; `P01–P08`) | Runner crates (`crates/velnor-platform`, `velnor-k8s`, `velnor-docker`) | `crates/velnor-runner-core/src/{ownership,lifecycle}.rs`, `crates/velnor-runner-host/src/docker_*`, `crates/velnor-{platform,k8s,docker}` |
| **9** | Storage & Native `actions/cache` server (`ARCH §7.1–7.2`, OD 22, 23; `ST01–ST07`) | Cache rules (`velnor-cache-rules`) + local cache server (`velnor-local`) + new crates (`crates/velnor-storage`, `velnor-cache`) | `crates/velnor-cache-rules/src/lookup.rs:8-41`, `crates/velnor-local/src/{cli,protocol,io/mod}.rs`, `crates/velnor-{storage,cache}` |
| **10** | Git, crate, and tool mirrors (`ARCH §7.4`, OD 24; `C01–C03`, `C10–C11`) | Generator/helper (`velnor-model`, `velnor-rust`, `velnor-render`, `velnor-ci`) + node agent (`crates/velnor-node`) | `crates/velnor-model/src/steps.rs:88,103,140-144`, `crates/velnor-ci/src/cli.rs:94-97,115-117`, `crates/velnor-node` |
| **11** | Warm snapshots (generations) & MBX (`ARCH §7.3`; `C05–C09`, `C12–C13`) | Generator/helper (`velnor-model`, `velnor-rust`, `velnor-render`, `velnor-ci`) + node agent (`crates/velnor-node`) | `crates/velnor-ci/src/{inputs,delta_cli,mbx_store}.rs`, `crates/velnor-ci/src/io/audit/dep_info.rs`, `crates/velnor-node` |

#### 2.2.1 Verification ledger and verdict keys (`ARCH §6.1–6.2`; `DG01`, `V01–V04`)
- **Layer:** Model (`velnor-model`), Planner (`velnor-rust`), Runtime CLI (`velnor-ci`), Service (`crates/velnor-ledger`, new).
- **Exact location, types, and functions to extend:**
  - `crates/velnor-model/src/verdict.rs:64-75`: add `ObligationId`, `ReuseClass { Content, PerRequest, Never }`, `ObligationKey` (canonical struct: command argv, feature set, profile, nextest profile, helper SHA, `compat` digest, `lock_root` digest, `ExecutorId`, execution fingerprint, and tree/package-closure input digest), and `ObligationSpec` alongside `VerdictSpec`.
  - `crates/velnor-rust/src/compat.rs` & `crates/velnor-rust/src/lock_key.rs`: reuse `compat` (16-hex SHA-256 of rustc, target, profile-env, repo-profile, image, rustflags, workspace, driver) and member-version-stripped `lock_root` directly as the definition and dependency fields of `ObligationKey`.
  - `crates/velnor-rust/src/verdict_plan.rs:20-112`: extend `PlanOutput::resolve` to accept signed ledger witness receipts (`witness_1`, `witness_2`) in addition to `MERGE_GROUP_SKIP_OUTPUT` (`verdict_plan.rs:103-105`).
  - `crates/velnor-ci/src/cli.rs:36`: add `Command::Ledger(LedgerCommand)` (`resolve`, `record`, `explain`, `audit-wait`) and register `"ledger"` in `crates/velnor-workflows/src/helper_call.rs:6-45`.
  - `crates/velnor-ledger` (new crate in root `Cargo.toml:5-24`): witness slot state machine, Ed25519 receipt verification, epoch revocation, A1/A2 audit streams.
- **What `velnor-gen` / `velnor-ci` already gives for free:**
  - `compat` (`docs/rust-ci-cache-keys.md:65-71`) already canonicalises compiler, flags, profile, and OS image.
  - `lock_root` (`docs/rust-ci-cache-keys.md:16-22`) already hashes `Cargo.lock` with workspace member versions blanked so version-bump PRs do not bust caches.
  - `inputs.tsv` (`crates/velnor-ci/src/inputs.rs:13-38`) already hashes every tracked file (`path`, `blob`, `mode`) and symlink.
  - `VerdictSpec` (`crates/velnor-model/src/verdict.rs:64-75`) and `verdict::decide` (`crates/velnor-rust/src/verdict.rs:278-317`) already enforce zero-test failure (`listed > 0 && run == 0` fails, `:264-270`).

#### 2.2.2 `Velnor / Verified` App status and `Velnor / Details` check (`ARCH §8.2`; `L01–L08`)
- **Layer:** Config/Model (`velnor-model`, `velnor-rust`), Runtime (`velnor-ci`), Service (`crates/velnor-control`, new).
- **Exact location, types, and functions to extend:**
  - `crates/velnor-rust/src/config_keys.rs:4-39` & `crates/velnor-model/src/repo_config.rs:11-46`: keep `required_check = "Required"` during Phase 0–Step 3; add optional `[gate] app_status = "Velnor / Verified"` and `app_id = <u64>` when transitioning to the App gate.
  - `crates/velnor-ci/src/gate.rs:15,292-303`: update `require()` (which currently rejects any `integration_id` other than `ACTIONS_APP_ID = 15368` or `-1` at `gate.rs:295-299`) to accept the configured `VELNOR_VERIFY_APP_ID` when `--app-check` is passed, while still requiring the push-run check (`gate.rs:141-173`).
  - `crates/velnor-control` (new crate): webhook receiver (`pull_request`, `merge_group`, `push`, `check_run`), 10 s REST reconciler with ETag, epoch-fenced status writer for `Velnor / Verified` (`statuses: write`) and summary writer for `Velnor / Details` (`checks: write`).
- **What `velnor-gen` / `velnor-ci` already gives for free:**
  - `velnor-ci gate` (`crates/velnor-rust/src/verdict.rs:138-161`) already formats the Markdown verdict table for `$GITHUB_STEP_SUMMARY`.
  - `velnor-ci report` (`crates/velnor-ci/src/cli.rs:42-45`) already produces `ci-metrics.json` with cache hits, compiled crate counts, and timings to feed `Velnor / Details`.

#### 2.2.3 Label flow (`pr_ci`) (`ARCH §8.2`, OD 12; `B07`, `N02`)
- **Layer:** Generator-time (`velnor-model`, `velnor-rust`, `velnor-render`, `velnor-gen`).
- **Exact location, types, and functions to extend:**
  - `crates/velnor-model/src/plan.rs:40-49`: add `PullRequestType::Unlabeled` to `PullRequestType`.
  - `crates/velnor-model/src/plan.rs:116-128`: add `pub const VELNOR_VERIFY_LABEL: &str = "velnor:verify";` (or configurable label); update `PrCi::Label` (`crates/velnor-model/src/repo_config.rs:146-149`) so `PrRun` distinguishes `Armed` (non-draft same-repo PR carrying the active label) from `Unarmed` (draft or unlabelled PR).
  - `crates/velnor-model/src/steps.rs:111-113`: replace `StepKind::DraftNotice` (which exits 0 on drafts, `rust-ci.yml:582-584`) and `StepKind::FailWhenANeedFailed` (`rust-ci.yml:610-612`, which exits 0 when `rust` and `policy` pass the quick set without tests) with `StepKind::FailOnUnrequestedPr` (`exit 1` with `::error::CI not requested: add label`).
  - `crates/velnor-rust/src/plan.rs:330-343`: in strict label mode, `quick()` becomes `PrRun::Armed` (nothing runs on ready unlabelled PRs, matching OD 12 steps 1–2).
  - `crates/velnor-rust/src/plan_budget.rs:49-75`: include `PullRequestType::Labeled`, `PullRequestType::Unlabeled`, and `PullRequestType::ConvertedToDraft`; emit `FailOnDraftRun` instead of `DraftNotice`.
  - `crates/velnor-render/src/gate.rs:20,245-270`: update `RELATED_EVENT` and `requested_here()` so `unlabeled` cancels any in-flight run via the PR concurrency group (`rust-ci.yml:22-24`) and runs `Required`, which fails closed with `"CI not requested"`.
- **What `velnor-gen` / `velnor-ci` already gives for free:**
  - Concurrency group rendering (`crates/velnor-model/src/plan.rs:62-65`, `rust-ci.yml:22-24`) already routes unrelated label events to `-ignored-${{ github.run_id }}` so unrelated labels never cancel a running verification.
  - `edited` filter (`crates/velnor-render/src/gate.rs:20`, `rust-ci.yml:23,35`) already ignores title/body edits unless `github.event.changes.base != null`.

#### 2.2.4 Reuse across agent run → PR → queue → main → release (`ARCH §6.3`, OD 10)
- **Layer:** Generator-time (`velnor-model`, `velnor-rust`, `velnor-render`) and Runtime (`velnor-ci`, `velnor-ledger`).
- **Exact location, types, and functions to extend:**
  - **Step 1 (Queue → Main, built today):** Enable `[ci] skip_push_after_merge_group = true` in `.velnor/rust-ci.toml` (`crates/velnor-model/src/repo_config.rs:38,130-143`, `crates/velnor-rust/src/merge_group_skip.rs:33-58`, `crates/velnor-ci/src/merge_group.rs:45-86`).
  - **Step 2 (Agent / PR → Queue → Main via Ledger):** Generalise `Step.skip_after_merge_group: Option<PlanFrom>` (`crates/velnor-model/src/plan.rs:110`) to `Step.skip_when_verified: Option<PlanFrom>`.
  - `crates/velnor-ci/src/io/plan.rs:38-93`: after `select_event` (`plan.rs:162`), query `velnor-ledger` for the subject tree's `ObligationKey` vector. Obligations with 2 passing T2/T3 witnesses and no C5 requirement mark their package/step output as `verified` (`SelectionReason::Reused(RecordId)` in `crates/velnor-model/src/selection.rs:65-71`), while C5 obligations (`ReuseClass::PerRequest`) and cache-warming builds on `main` still execute.
  - `crates/velnor-rust/src/verdict_plan.rs:103-106`: extend the `merge_group_ran` check in `PlanOutput::resolve` so `velnor-ci gate` accepts a ledger-verified obligation only when `velnor-ci plan` outputs a signed ledger receipt digest for that root/obligation.
- **What `velnor-gen` / `velnor-ci` already gives for free:**
  - `merge_group_skip::apply` (`crates/velnor-rust/src/merge_group_skip.rs:9-58`) already partitions `StepKind` into `is_check` (fmt, clippy, doc, check-alone, test-setup, nextest, lane-guard, doctest) vs cache-warming builds and saves, skipping duplicate checks while keeping `main` cache saves warm.
  - `velnor-ci plan --explain` (`crates/velnor-ci/src/plan.rs`) already prints per-package selection reasons and skipped steps to `$GITHUB_STEP_SUMMARY`.

#### 2.2.5 Release promotion by digest (`ARCH §8.4`, OD 10; `R01–R06`)
- **Layer:** Config/Model (`velnor-release`, `velnor-model`), Renderer (`velnor-render`), Runtime (`velnor-ci`).
- **Exact location, types, and functions to extend:**
  - Migrate `.github/workflows/release-rust-ci-tools.yml` (currently hand-written, 253 lines) and the missing `runner-v*` release workflow onto `.velnor/release.toml` (`crates/velnor-release/src/plan.rs`, `crates/velnor-render/src/release.rs:42-97`).
  - Lift the generator restriction in `crates/velnor-release/src/family.rs` (`generator/CONFIG.md:575,610`) that currently forbids combining `family_tag = "tools-v{{ version }}"` with `bins`, `attest = true`, `smoke = true`, `commit_file = true`, `member_digests = "sha256"`.
  - Add `promote_from_ci = true` to `[[binaries]]` in `crates/velnor-release/src/velnor_config.rs`: the `merge_group` / `push` run builds `release-tools` and `runner` binaries once, attests them, and stores them in the site store by SHA-256; `crates/velnor-render/src/release_binaries.rs` emits a promotion job (`velnor-ci release promote`) that fetches by digest, verifies `SHA256SUMS` + `BINARY-SHA256SUMS` + Sigstore attestation, and publishes in ~15 s with zero `cargo build` steps at tag time.
  - Update `crates/velnor-render/src/release.rs:31` (`HOSTED_RUNNER`) and `crates/velnor-render/src/release_attest.rs:70` (`--deny-self-hosted-runners`) per owner decision on Q-N5.
- **What `velnor-gen` / `velnor-ci` already gives for free:**
  - `velnor-release` + `velnor-render/src/release*.rs` already model `family_tag`, `[[binaries]]`, `profile = "release-tools"`, `member_digests = "sha256"`, `commit_file = true`, `smoke = true`, `attest = true`, and `velnor-ci release-gate` (`crates/velnor-ci/src/gate.rs:57-139`).
  - `velnor-gen release ruleset` and `ruleset-drift` (`crates/velnor-release/src/ruleset.rs:46-48`) already validate `protect-tags` and `release-tags` rulesets.

#### 2.2.6 Agent-native `velnor verify` / `velnor cargo` / `velnord` (`ARCH §8.6`, `UX §5`; `AN01–AN14`)
- **Layer:** Planner library (`velnor-rust`, `velnor-model`), New CLI/daemon (`crates/velnor-cli`, `crates/velnor-daemon`).
- **Exact location, types, and functions to extend:**
  - Link `velnor-model` and `velnor-rust` directly into the `velnor` / `velnord` binary (`crates/velnor-cli`): `velnor resolve` calls `velnor_gen::io::read_recorded_checkout` + `velnor_rust::detect` + `velnor_rust::select` (`crates/velnor-ci/src/io/plan.rs:42-50`) locally in <10 ms without touching cargo.
  - `velnord` snapshots the worktree via a private Git index (`git write-tree` over tracked + untracked non-ignored files), pushes thin packs to the node mirror under `refs/velnor/wip/<id>`, computes `merge-tree(queue_tip, wip_tree)`, and submits missing `ObligationSpec`s to `velnor-dispatch` in the `interactive` class (2 witnesses on 2 cluster nodes).
  - `velnor cargo` wraps `mbx` (or replaces `mbx` in `mise.toml:47-49` once `velnor-gen` adds `cargo_wrapper = "velnor"` in `.velnor/rust-ci.toml` so `check_tools` in `crates/velnor-rust/src/detect.rs` does not fail on `mr_boxington`).
- **What `velnor-gen` / `velnor-ci` already gives for free:**
  - `velnor_rust::select` reads `.velnor/rust-metadata.json` (`crates/velnor-ci/src/io/plan.rs:42`) with zero `cargo metadata` startup cost.
  - `velnor-local` (`crates/velnor-local/src/cli.rs:29-74`) already provides the local Docker execution driver and scenario runner on macOS.

#### 2.2.7 Velnor Native executor vs GitHub Official (`ARCH §5.3`, OD 1, 19, 20; `X01–X17`, `O01–O04`)
- **Layer:** Generator config (`velnor-model`, `velnor-rust`, `velnor-render`), Runner crates (`crates/velnor-executor`, `velnor-exec`, `velnor-official`, `velnor-native-actions`, `velnor-dispatch`).
- **Exact location, types, and functions to extend:**
  - **Generator side:** Add `executor` routing in `.velnor/rust-ci.toml` (`crates/velnor-rust/src/config_keys.rs:4-39`) and `.velnor/workflows.toml` (`crates/velnor-workflows`): maps job/obligation kinds to runner labels or `ExecutorId { kind: Native | Official, version, image_digest }`. Add `allow_forks = false` (`crates/velnor-render/src/steps.rs:71`, `rust-ci.yml:57`) once OD 8 / Q-N6 is approved.
  - **Runner side (in root `crates/velnor-*`):**
    - `crates/velnor-executor`: `Executor` trait (`identity()`, `run(Work, Env, Role, JobIo)`).
    - `crates/velnor-official`: refactor `crates/velnor-runner-host/src/launch/*` and `crates/velnor-resource-probe/src/job_hook.rs` to drive official runner `2.338.0`/`2.338.2` (`.velnor/runner.lock:17`) in `Role::Production` and `Role::Reference`, with `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` mounted from the node store.
    - `crates/velnor-exec`: native Rust step runner (expressions, contexts, env files, workflow commands, Aho-Corasick masker, post steps, cancellation, run-service acquire/renew/complete, Twirp Results). `BackgroundStepCoordinator` (`X13`) is **dropped** because `velnor-gen` emits **0** `background: true` steps (`crates/velnor-render/src/rules.rs`).
    - `crates/velnor-native-actions`: native implementations of `ALLOWED_ACTIONS` (`crates/velnor-render/src/rules.rs:16-34`).
    - `crates/velnor-dispatch`: cluster-wide scale-set listener + placement service, reusing `crates/velnor-runner-github` (`paths.rs:4-25`, `poll.rs`, `acquire.rs`) and `crates/velnor-runner-core` (`lifecycle.rs:9-67`, `capacity.rs:21-126`, `ownership.rs:23-41`).
- **What `velnor-gen` / `velnor-ci` already gives for free:**
  - `crates/velnor-render/src/rules.rs:16-45` **already enforces the closed action set (`ALLOWED_ACTIONS`, 16 actions, full 40-hex SHA pins in `pinned_actions.rs`) and single-command `run:` steps with no shell syntax**, eliminating 90% of arbitrary YAML/shell edge cases for `velnor-exec`.
  - `crates/velnor-runner-github` (8.2k LOC) already implements scale-set CRUD, sessions, long-poll, `acquirejobs`, and `generatejitconfig` against `actions/scaleset` `e6daac70`.

#### 2.2.8 Platform trait (Kubernetes + Docker) (`ARCH §5.4`, OD 13, 21; `P01–P08`)
- **Layer:** Runner crates (`crates/velnor-platform`, `velnor-k8s`, `velnor-docker`).
- **Exact location, types, and functions to extend:**
  - Reconcile `docs/proposed/kubernetes-runtime.md:127-153` (`trait Runtime`, pair-shaped) with `docs/proposed/velnor-runner-architecture.md:313-334` (`trait Platform`, environment-shaped with `Capabilities`) into `crates/velnor-platform`: keep `#1324`'s `Observation<T> { Present, AbsentProven(AbsenceProof), Unknown }` (`kubernetes-runtime.md:113-122`) and `CleanupProof` (`crates/velnor-runner-core/src/ownership.rs:23-37`), while using ARCH's generic `EnvSpec`/`DirSpec`/`ProcSpec`/`Capabilities` rather than hardcoding a 2-container `runner`+`dind` pair on every job (only 3 test binaries need Docker).
  - `crates/velnor-docker`: extract the ~50 `bollard` call sites and `docker_spec::audit_plan` (`HOST_NEEDLES`) from `crates/velnor-runner-host/src/docker_*`.
  - `crates/velnor-k8s`: `kube-rs` pod provisioning, warm pod pool, `preconditions.uid` deletion proof, private privileged dind sidecar only when `EnvSpec` requests `Docker::PrivateDaemonPrivileged`.
- **What `velnor-gen` / `velnor-ci` already gives for free:**
  - `velnor-runner-core`'s tri-state ownership/cleanup proof (`crates/velnor-runner-core/src/ownership.rs:31-37`, `lifecycle.rs:304-315`) and `docker_spec.rs` security audit are already built and tested.
  - Linux `file:` credential support (`#1329`, `586faa21b8`) and org-level scale sets (`#1330`, `7be003b42a`) are already merged in `velnor-runner-host`.

#### 2.2.9 Storage and Native `actions/cache` server (`ARCH §7.1–7.2`, OD 22, 23; `ST01–ST07`)
- **Layer:** Cache rules (`velnor-cache-rules`), Local cache server (`velnor-local`), New crates (`crates/velnor-storage`, `velnor-cache`).
- **Exact location, types, and functions to extend:**
  - `crates/velnor-storage`: `BlobStore` trait (`get`, `put_if_absent`, `copy`, `list`, `delete`, `find_missing`, `StorageCaps`) with local XFS/APFS filesystem and S3/RustFS backends.
  - `crates/velnor-cache`: promote `crates/velnor-local/src/io/mod.rs:144` + `protocol.rs:22` (which already serves Twirp `CacheService` v2 `CreateCacheEntry`/`GetCacheEntryDownloadURL`/`FinalizeCacheEntryUpload` + Azure Blob PUT/GET backed by `crates/velnor-cache-rules/src/lookup.rs:8-41`) into a production site service backed by `velnor-storage` (L0 unpacked XFS trees for reflink, L1 1 MiB content-defined chunks in RustFS, L2 cross-site replication) and an authoritative home-site index.
  - Point `velnor-native-actions` (`A03`) and `velnor-official` (`ST05`, via rewritten `ResultsServiceUrl` in the job message) at `velnor-cache`.
- **What `velnor-gen` / `velnor-ci` already gives for free:**
  - `velnor-cache-rules` (`crates/velnor-cache-rules/src/lookup.rs:8-41`) and `velnor-local cache-server` (`crates/velnor-local/src/cli.rs:30-32`) **already implement GitHub's exact cache v2 Twirp protocol, branch scoping, immutable keys, and prefix restore order**, verified against real GitHub conformance probes (`local/conformance/cache-*.json`).

#### 2.2.10 Git, crate, and tool mirrors (`ARCH §7.4`, OD 24; `C01–C03`, `C10–C11`)
- **Layer:** Generator/Helper (`velnor-model`, `velnor-rust`, `velnor-render`, `velnor-ci`), Node agent (`crates/velnor-node`).
- **Exact location, types, and functions to extend:**
  - **Crates mirror (DONE in generator/helper):** `.velnor/rust-ci.toml:11-14` (`[crates_mirror]`), `StepKind::CratesMirror` (`crates/velnor-model/src/steps.rs:88`), `velnor-ci crates-mirror` (`crates/velnor-ci/src/cli.rs:94-97`). Extend in `C02` with a read-only pre-unpacked crate store per lockfile.
  - **Tool store (PARTIAL in generator/helper):** `StepKind::ToolsLocal(Restore|Save)` (`crates/velnor-model/src/steps.rs:103,140-144`) and `velnor-ci tools-local` (`crates/velnor-ci/src/cli.rs:115-117`, `rust-ci.yml:94-99,124-128`) already copy `~/.local/share/mise` and `~/.rustup` from `VELNOR_MBX_SHARED_DIR`/`VELNOR_MBX_SEED_DIR`. Extend in `C01` to mount a read-only (`EROFS`) node tool store directly so 0 bytes are copied per job.
  - **Git mirror (NEW in `velnor-node` + `velnor-ci`):** `velnor-node` maintains bare repo mirrors (`/var/lib/velnor/mirrors/<owner>/<repo>.git`) updated on webhooks under `flock` + sealed pre-materialised `main` trees. `velnor-native-actions` `checkout` (or `velnor-ci checkout` under Official) reflink-clones the sealed base, rewrites diff paths, and verifies the full staged tree hash (`C10`).
- **What `velnor-gen` / `velnor-ci` already gives for free:**
  - `velnor-ci crates-mirror` (`generator/CONFIG.md:45-68`) already installs a per-job Cargo wrapper that survives `CARGO_HOME` and working-directory changes in child build scripts and tests.
  - `velnor-ci tools-local` already skips network tool cache downloads whenever the node store hits (`rust-ci.yml:102`).

#### 2.2.11 Warm snapshots (generations) and MBX (`ARCH §7.3`; `C05–C09`, `C12–C13`)
- **Layer:** Generator/Helper (`velnor-model`, `velnor-rust`, `velnor-render`, `velnor-ci`), Node agent (`crates/velnor-node`).
- **Exact location, types, and functions to extend:**
  - Build `C05` (Verified Generations) directly on top of `velnor-ci inputs` (`crates/velnor-ci/src/inputs.rs:9-243`), `velnor-ci inputs audit` (`crates/velnor-ci/src/io/audit/dep_info.rs`), and `velnor-ci delta` (`crates/velnor-ci/src/delta_cli.rs`, `#1107`):
    - Today `inputs save` writes `target/.velnor/inputs.tsv` (`path`, `blob`, `mtime_ns`, `mode`), `delta prepare/apply/stage` overlays per-PR changes against a base target inventory and invalidates Cargo fingerprints when the source snapshot differs (`docs/rust-ci-cache-keys.md:55-58`), and `inputs audit` checks Cargo `dep-info` (`crates/velnor-ci/src/io/audit.rs`).
    - Fix the Q16 bug in `crates/velnor-ci/src/inputs.rs:194-242` (`plan_restore`): diff the request tree against the **generation's own saved `inputs.tsv` (`M_G`)**, stamp every changed/added path in `Δ` strictly newer than `G`'s newest output, and remove Cargo fingerprints for any unit whose traced read set meets `Δ`.
    - Replace `actions/cache/restore` of `target` + `target.velnor-delta` (`rust-ci.yml:162-190`) with an XFS reflink clone (`0.25 s`) of the sealed node generation `G` mounted by `velnor-node` (`VELNOR_GENERATION_DIR`).
  - **MBX:** Today `.velnor/rust-ci.toml:6` sets `mbx_shared_store = true`, emitting `velnor-ci mbx-store` (trusted events write `VELNOR_MBX_SHARED_DIR`) and `velnor-ci mbx-seed` (PR/merge_group copy missing CAS files from `VELNOR_MBX_SEED_DIR` in parallel, `rust-ci.yml:83-87`). Keep `mbx-store`/`mbx-seed` for cold/lockfile-change builds until `C08` benchmarks `mbx` vs plain cargo over XFS reflink generations (`C05`), then retire the loser.
- **What `velnor-gen` / `velnor-ci` already gives for free:**
  - `velnor-ci inputs save/restore/build/audit` and `velnor-ci delta prepare/apply/stage` already implement ~70% of the content-addressed input manifest and dep-info verification machinery.
  - `velnor-ci mbx-store` and `mbx-seed` (`crates/velnor-ci/src/mbx_store.rs:10-50`) already enforce strict trust-scope separation (only `push`/`schedule` on `main` write the shared node store; PRs copy from a read-only seed).

---

## 3. Correctness first

Ranked by severity (probability × impact of landing unverified or broken code on `main`). Every gap is verified against `main` tip `0eb2d76a5c`.

| Rank | Severity | Correctness gap on `main` today (`0eb2d76a5c`) | Primary files (`file:line`) | Needs owner? |
|---|---|---|---|---|
| **1** | **P0** | `pr_ci = "label"` turns `Required` **GREEN without running tests** on every unlabelled ready PR and every draft PR | `.github/workflows/rust-ci.yml:582-612`, `crates/velnor-rust/src/{plan,plan_budget,required_job}.rs` | **Yes (Q-N2)** |
| **2** | **P0** | `main` is red (98% fail rate; 157–167 of 173 merges used admin bypass) due to stale 5-manifest `Cargo Deny` in `ci.yml:117` and 4 files breaching `.alint.yml:208-228` line caps | `.github/workflows/ci.yml:117`, `delta_cache.rs:423`, `prepare_inputs.rs:417`, `velnor-render/src/lib.rs:154`, `velnor-rust/src/lib.rs:152` | **No** |
| **3** | **P0** | Ruleset `protect-main` (`24396608`) has `Required` unbound (`integration_id: null`), admin bypass active, `HEADGREEN`, `allow_auto_merge: false`, and 0 queue runs | GitHub ruleset `24396608`, repo setting `allow_auto_merge` | **Yes (D3'/Q-N3, D2'/Q-N4)** |
| **4** | **P0** | `release-rust-ci-tools.yml:86-105` checks only default-branch ancestry, not that `Required` passed (violating `goals.md:77`), and no `runner-v*` release workflow exists | `.github/workflows/release-rust-ci-tools.yml:86-105`, `.velnor/release.toml`, `crates/velnor-release/src/family.rs` | **No** (PR #1608) |
| **5** | **P1** | `StepKind::GeneratorCheck` (`velnor-gen check`) is dead code and `.velnor/config.toml` sets `engine_configured = true`, ignoring unmarked workflows (`Owner::Nobody`) | `crates/velnor-model/src/steps.rs:108`, `crates/velnor-rust/src/plan.rs:274-280`, `crates/velnor-gen/src/drifts.rs:55,75`, `.velnor/config.toml` | **No** |
| **6** | **P1** | Filtered `[[test_lane]]` runs `Lint (features: local-mode)` and `Documentation (features: local-mode)` across `--workspace` even when `velnor-runner-host` is not selected | `.github/workflows/rust-ci.yml:294-299`, `crates/velnor-rust/src/config_test_lane.rs` | **No** |
| **7** | **P1** | Same-job trust boundary (`#1220`) and advisory `Cache` failure (`17/60` runs) marking `workflow_run.conclusion == 'failure'`, which blocks `release.yml` (`release.rs:34`) | `crates/velnor-render/src/release.rs:34`, `crates/velnor-ci/src/io/report.rs` | **No** |
| **8** | **P2** | `fetch-depth: 2` (`PLAN_DEPTH = 2`) silently degrades multi-commit `push` and multi-PR `merge_group` runs to `All(AllTrigger::BaseUnavailable)` | `crates/velnor-rust/src/plan.rs:37`, `crates/velnor-ci/src/io/git.rs:66-79`, `crates/velnor-ci/src/io/plan.rs:176-211` | **No** |
| **9** | **P2** | Non-Rust workflow/config edits (`ci.yml`, `qualification.yml`, `.alint.yml`, `scripts/**`) hit `Class::Unknown` → `AllTrigger::UnknownPath` (selects entire workspace) | `crates/velnor-rust/src/classify.rs:22-97`, `.velnor/rust-ci.toml:1-32` | **No** |
| **10** | **P2** | Open PRs `#1200` (mise platform locks), `#1320` (per-root inputs audit), and `#1026` (sibling/custom target restore) fix real correctness bugs but sit unmerged | `crates/velnor-rust/src/{mise_lock,os_pins,audit_job}.rs`, `crates/velnor-ci/src/{inputs,io/audit}.rs` | **No** |

### 3.1 Rank 1 (P0): `pr_ci = "label"` turns `Required` GREEN without tests on unlabelled ready PRs and draft PRs
- **Gap on `main` today (`0eb2d76a5c`):**
  - On a draft PR, `rust` and `policy` skip (`.github/workflows/rust-ci.yml:35,409`), and `Required` runs only `DraftNotice` (`echo "::notice title=Draft::draft: CI not requested"`, `rust-ci.yml:582-584`) and exits **0 (green)**.
  - On a ready PR without `ci:run`/`ci:auto`, `rust` runs only Format and Lint (skipping `Test`, `Test (features: local-mode)`, and `Test lane guard`, `rust-ci.yml:278-293`), `velnor-ci gate` is **skipped** (`rust-ci.yml:604-609`), and `Required` only runs `FailWhenANeedFailed` (`rust-ci.yml:610-612`), exiting **0 (green)** whenever Format+Lint+Policy pass.
  - Removing a label does not trigger a run (`unlabeled` is absent from `rust-ci.yml:5-12`).
  - Directly violates Owner Decision 12 (`docs/research/velnor-runner-2026-11/owner-decisions.md:20`) and `docs/goals.md:41` ("The required check passes only when everything that was selected ran and passed").
- **Exact fix:**
  1. Add `PullRequestType::Unlabeled` and `ConvertedToDraft` to `triggers()` when `pr_ci = "label"`.
  2. In `velnor-rust`, replace `StepKind::DraftNotice` and `StepKind::FailWhenANeedFailed` on `PrRun::Unrequested` with `StepKind::FailOnUnrequestedPr` (`echo "::error title=CI not requested::Add label velnor:verify to run CI" && exit 1`).
  3. In strict label mode (OD 12), gate `rust`, `policy`, and `lint-*` on `PrRun::Requested` so unlabelled pushes run nothing except `Required` failing fast in ~2 s.
  4. Retire one-shot `ci:run` (`ci-run-label` job, `rust-ci.yml:613-626`) in favour of persistent `velnor:verify` (or `ci:auto`) per Q-N2.
- **Where it goes:** `crates/velnor-model/src/plan.rs:40-49,116-128`, `crates/velnor-model/src/steps.rs:110-114`, `crates/velnor-rust/src/plan.rs:330-343`, `crates/velnor-rust/src/plan_budget.rs:49-75`, `crates/velnor-rust/src/required_job.rs:106-149`, `crates/velnor-render/src/gate.rs:20,245-270`, `crates/velnor-render/src/draft_steps.rs`.
- **Needs owner approval?** **Yes (Q-N2):** confirm retiring the unlabelled quick-set green and one-shot `ci:run`, plus the label name (`velnor:verify` vs `ci:auto`).

### 3.2 Rank 2 (P0): `main` is red (98% fail rate) and 157–167 of 173 merges bypassed `Required`
- **Gap on `main` today (`0eb2d76a5c`):** Direct code inspection at `0eb2d76a5c` uncovered three concrete bugs keeping `main` red (`C§0,§1,§2`):
  1. **Stale multi-workspace `Cargo Deny` in `ci.yml`:** `.github/workflows/ci.yml:117` runs `cargo deny` on `$GITHUB_WORKSPACE/ci-helper/Cargo.toml`, `crates/velnor-runner/Cargo.toml`, `generator/Cargo.toml`, and `local/Cargo.toml`—all 4 were deleted in `#1290`/`#1594` (`23bed02e7e`). (Root `deny.toml:15` already ignores `RUSTSEC-2026-0543`.)
  2. **Four Rust files on `main` breach `.alint.yml` physical line caps:**
     - `crates/velnor-gen/tests/it/delta_cache.rs`: **423 lines** (cap 400, `.alint.yml:208-219`).
     - `crates/velnor-ci/tests/it/prepare_inputs.rs`: **417 lines** (cap 400, `.alint.yml:208-219`).
     - `crates/velnor-render/src/lib.rs`: **154 lines** (cap 150, `.alint.yml:220-228`).
     - `crates/velnor-rust/src/lib.rs`: **152 lines** (cap 150, `.alint.yml:220-228`).
  3. **`velnor-ci` pin lag during rapid merges:** PRs changing generator/helper contracts merged before `tools-v0.1.95` was released and pinned (`#1737`→`#1742`).
- **Exact fix:**
  1. Split `delta_cache.rs` (423→two files ≤250 lines) and `prepare_inputs.rs` (417→two files ≤250 lines); trim 5 lines in `crates/velnor-render/src/lib.rs` (154→≤145) and `crates/velnor-rust/src/lib.rs` (152→≤145).
  2. Move `alint` (`probe = "scripts/alint-line-cap-probe.sh"`), `jactionlint` (`config = ".github/jactionlint.yaml"`), and `mise-task` `lint-generator-samples` into `.velnor/rust-ci.toml` as `[[lint]]` entries (`generator/CONFIG.md:216-258`), and **delete hand-maintained `.github/workflows/ci.yml`** (which also removes the broken 5-manifest `cargo-deny` and partial `cargo-machete` at `ci.yml:117,134`, already covered by `rust-ci.yml`'s `policy` job at `rust-ci.yml:480-485`).
  3. Replace the sole `actions/github-script` step in `.github/workflows/release-rust-ci-tools.yml:26` with `velnor-gen release ruleset-drift` so `jactionlint` does not require pinning `node` in `mise.toml` (`crates/velnor-rust/src/lint_tools.rs`).
- **Where it goes:** `crates/velnor-gen/tests/it/delta_cache.rs:1-423`, `crates/velnor-ci/tests/it/prepare_inputs.rs:1-417`, `crates/velnor-render/src/lib.rs:1-154`, `crates/velnor-rust/src/lib.rs:1-152`, `.velnor/rust-ci.toml:1-32`, `.github/workflows/ci.yml` (delete), `.github/workflows/rust-ci.yml` (regenerate).
- **Needs owner approval?** **No.**

### 3.3 Rank 3 (P0): Ruleset `protect-main` (`24396608`) has unbound `Required`, admin bypass, `HEADGREEN`, and no merge queue
- **Gap on `main` today (`0eb2d76a5c`):** In ruleset `24396608` (`C§1,§4`), `Required` has `integration_id: null` (any token with `statuses: write` can spoof it), `bypass_actors` grants `RepositoryRole 5` (admin) `always` bypass, `grouping_strategy` is `HEADGREEN` (intermediate red commits in a group land when the head commit passes), `min_entries_to_merge_wait_minutes` is `5` (adds 5 min idle delay to every single-PR merge), `allow_auto_merge` is `false`, and 0 `merge_group` runs have executed post-cutover.
- **Exact fix:** Once Rank 2 makes `main` green and folds `Alint` + `Jactionlint` into `Required`, update ruleset `24396608` (`integration_id: 15368`, `bypass_actors: []`, `grouping_strategy: "ALLGREEN"`, `max_entries_to_build: 2`, `min_entries_to_merge_wait_minutes: 0`, `check_response_timeout_minutes: 90`) and set `allow_auto_merge: true` on `tailrocks/velnor-new`.
- **Where it goes:** `PUT /repos/tailrocks/velnor-new/rulesets/24396608` and `PATCH /repos/tailrocks/velnor-new`.
- **Needs owner approval?** **Yes (D3' / Q-N3, D2' / Q-N4):** owner applies ruleset + repo setting once `main` is green.

### 3.4 Rank 4 (P0): Tools release gate checks only ancestry, not `Required`, and no `runner-v*` release workflow exists
- **Gap on `main` today (`0eb2d76a5c`):** `.github/workflows/release-rust-ci-tools.yml:86-105` calls `repos/$GITHUB_REPOSITORY/compare/$default_branch...$GITHUB_SHA` and accepts `behind|identical` even if `Required` failed on that commit (which happened on 166 of 172 push runs post-cutover, `C§2`), violating `docs/goals.md:77` (Rule 13). Meanwhile `release-plz.toml:45-106` defines 6 runner crates with `git_tag_name = "runner-v{{ version }}"`, which no release workflow builds or publishes.
- **Exact fix:**
  1. **Immediate (PR #1608):** Merge open PR `#1608` (`fast/1555-tools-gate`, 0 conflicts locally, `D§1`), wiring the strict `Required`-from-push check into `release-rust-ci-tools.yml`.
  2. **Consolidation (`#733` / `N07`):** Add `.velnor/release.toml` covering both `tools-v{{ version }}` and `runner-v{{ version }}`, using `velnor-ci release-gate` (`crates/velnor-ci/src/gate.rs:57-139`), and delete hand-written `release-rust-ci-tools.yml`.
- **Where it goes:** `.github/workflows/release-rust-ci-tools.yml:86-105` (PR #1608), `.velnor/release.toml` (new), `crates/velnor-release/src/family.rs`, `crates/velnor-render/src/release.rs:125-170`.
- **Needs owner approval?** **No** for merging PR #1608; **Yes (Q-N5)** only when moving release builds to self-hosted runners.

### 3.5 Rank 5 (P1): `StepKind::GeneratorCheck` (`velnor-gen check`) is dead code and `.velnor/config.toml` disables unmarked-workflow drift checks
- **Gap on `main` today (`0eb2d76a5c`):** `StepKind::GeneratorCheck` is defined (`crates/velnor-model/src/steps.rs:108`) and rendered as `velnor-gen check` (`crates/velnor-render/src/steps.rs:279`, `helper_steps.rs:74-80`), but **never constructed in `velnor-rust`**. Furthermore, because `.velnor/config.toml` still exists on disk (`crates/velnor-gen/src/io/workflows.rs:12`), `engine_configured` is `true` (`crates/velnor-gen/src/drifts.rs:55,75`), causing `velnor-gen check` to silently ignore `Owner::Nobody` unmarked workflow files.
- **Exact fix:** Unpack `velnor-gen` in `actions/setup-velnor/action.yml` (using `[velnor-gen]` SHA-256 digests already present in `.velnor/rust-ci.lock:12-14`), emit `StepKind::GeneratorCheck` in `crates/velnor-rust/src/plan.rs:274-280`, and delete `.velnor/config.toml` + legacy `--mode ci|qualification` shims (`crates/velnor-ci/src/io/plan.rs:39-41`).
- **Where it goes:** `actions/setup-velnor/action.yml`, `crates/velnor-rust/src/plan.rs:274-280`, `crates/velnor-gen/src/drifts.rs:52-80`, `crates/velnor-gen/src/io/workflows.rs:12`, `.velnor/config.toml`.
- **Needs owner approval?** **No.**

### 3.6 Rank 6 (P1): Filtered `[[test_lane]]` runs `Lint (features: local-mode)` and `Documentation (features: local-mode)` even when `velnor-runner-host` is not selected
- **Gap on `main` today (`0eb2d76a5c`):** In `.github/workflows/rust-ci.yml:283-299`, `Test (features: local-mode)` and `Test lane guard (features: local-mode)` check `contains(..., '-p velnor-runner-host') || contains(..., '--workspace')`, but `Lint (features: local-mode)` (`:294-296`) and `Documentation (features: local-mode)` (`:297-299`) are gated only on `rust_root_mode != 'none'`, forcing every leaf PR to run a second workspace-wide clippy build with `--features local-mode`.
- **Exact fix:** In `crates/velnor-rust/src/config_test_lane.rs`, attach `GateNeed::PackageArgs` with `lane.packages` to `CargoCommand::ClippyFeatures` and `CargoCommand::DocFeatures`.
- **Where it goes:** `crates/velnor-rust/src/config_test_lane.rs`, `crates/velnor-render/src/cargo.rs`.
- **Needs owner approval?** **No.**

### 3.7 Rank 7 (P1): Same-job trust boundary (`#1220`) and advisory `Cache` failure blocking `release.yml` via `workflow_run.conclusion`
- **Gap on `main` today (`0eb2d76a5c`):**
  1. In `one-job` shape (`rust-ci.yml:34-404`), `actions/cache/save` (`:314-320,333-342,379-384`), `inputs save`, and `delta stage` run in the same `rust` job container after untrusted test binaries and build scripts have executed (`docs/implemented/job-trust-boundary.md`, `#1220`).
  2. Advisory `Cache` (`velnor-ci report --check`, `rust-ci.yml:628-666`) failed on 17 of 60 sampled failed runs (`C§2`). Even though `Required` ignores `Cache` (`cache_check_blocks = false`), a red `Cache` job makes the overall `Rust CI` workflow conclusion `failure`, which **prevents `release.yml` from running** because `crates/velnor-render/src/release.rs:34` checks `github.event.workflow_run.conclusion == 'success'`.
- **Exact fix:** Fix the `release.yml` trigger condition in `crates/velnor-render/src/release.rs:34` (gate on `velnor-ci release-gate` verifying the push run's `Required` check-run conclusion rather than `workflow_run.conclusion == 'success'`), fix the `report --check` false positives in `crates/velnor-ci/src/io/report.rs`, and move cache/generation sealing outside the job sandbox into `velnor-node` (`V02`, `C05`).
- **Where it goes:** `crates/velnor-render/src/release.rs:34`, `crates/velnor-ci/src/io/report.rs`, `crates/velnor-node`.
- **Needs owner approval?** **No.**

### 3.8 Rank 8 (P2): `fetch-depth: 2` (`PLAN_DEPTH = 2`) silently degrades multi-commit `push` and multi-PR `merge_group` runs to whole-workspace selection
- **Gap on `main` today (`0eb2d76a5c`):** `velnor-ci plan` on `push` or `merge_group` calls `git::range_base` (`crates/velnor-ci/src/io/plan.rs:208-211`), which executes `git merge-base --is-ancestor <base> HEAD` (`crates/velnor-ci/src/io/git.rs:73-77`). With `fetch-depth: 2` (`crates/velnor-rust/src/plan.rs:37`, `rust-ci.yml:65`), any push of ≥2 commits or merge group batching ≥2 PRs has `<base>` deeper than `HEAD^1`, so `merge-base --is-ancestor` fails and `changes()` (`io/plan.rs:176-181`) falls back to `All(AllTrigger::BaseUnavailable)`.
- **Exact fix:** In `crates/velnor-ci/src/io/git.rs:66-79`, if `merge-base --is-ancestor` fails in a shallow clone (`.git/shallow` exists), deepen via `git fetch --no-tags --depth=64 origin <base> <head>` (and raise `PLAN_DEPTH` in `crates/velnor-rust/src/plan.rs:37` until `C03` node git mirrors provide full history locally).
- **Where it goes:** `crates/velnor-rust/src/plan.rs:37`, `crates/velnor-ci/src/io/git.rs:66-79`.
- **Needs owner approval?** **No.**

### 3.9 Rank 9 (P2): Non-Rust workflow/config edits hit `Class::Unknown` (`AllTrigger::UnknownPath`) and select the entire workspace
- **Gap on `main` today (`0eb2d76a5c`):** After `#1747` (`57b65af0db`), `.velnor/rust-ci.toml:1-32` has zero `[[inputs]]` and zero `non_inputs` entries. Edits to `.velnor/workflows.toml`, `.github/workflows/qualification.yml`, `.alint.yml`, or `scripts/**` hit `Class::Unknown` (`crates/velnor-rust/src/classify.rs:57`) → `AllTrigger::UnknownPath` (`crates/velnor-rust/src/select.rs:272-275`).
- **Exact fix:** Populate `[[inputs]]` (`generator/samples/**` → `["velnor-gen"]`, `local/**` → `["velnor-local"]`, `images/**` → `["velnor-runner-image-manifest", "velnor-runner-host"]`, `.velnor/workflows.toml` → `["velnor-workflows", "velnor-gen"]`) and `non_inputs` (`.alint.yml`, `.github/jactionlint.yaml`) in `.velnor/rust-ci.toml`.
- **Where it goes:** `.velnor/rust-ci.toml:1-32`, `crates/velnor-rust/src/classify.rs:24-97`.
- **Needs owner approval?** **No.**

### 3.10 Rank 10 (P2): Unmerged correctness PRs `#1200`, `#1320`, and `#1026`
- **Gap on `main` today (`0eb2d76a5c`):** `#1200` (platform-specific mise tool locks, `#1104`), `#1320` (per-root inputs audit without double workspace build, `#1073`), and `#1026` (sibling package and custom target-dir input restore, `#1011`/`#1018`) fix real `velnor-gen`/`velnor-ci` bugs (`D§1`).
- **Exact fix:** Land `#1200` in Item 3 (0 conflicts), rewrite `#1320` onto `crates/velnor-*` in Item 5, and split `#1026` into two `<400`-line PRs ahead of `C05`.
- **Where it goes:** `crates/velnor-rust/src/{mise_lock,os_pins,audit_job}.rs`, `crates/velnor-ci/src/{inputs,io/audit,io/cold}.rs`.
- **Needs owner approval?** **No.**

---

## 4. Speed on the new baseline

### 4.1 Where the time goes now (measured post-cutover baseline, `C§1–§5` & `rust-ci.yml`)

The cutover (`053fefa6a3`, #1523) moved 100% of `Rust CI` jobs onto `ubuntu-26.04-scale-set` (0 hosted jobs out of 2,660 sampled jobs, `C§3.3`). The old 26.8 min P50 / 73.2 min P90 hosted-queue-dominated PR run is gone (`C§2`). On the new baseline (`0eb2d76a5c`), a run executes the generated `one-job` graph (`rust` → `policy` → `Required`, plus parallel `ci.yml` and post-job `Cache` / `rust-trim`):

| Stage in `.github/workflows/rust-ci.yml` | PR (no-op / docs) | PR (heavy lane, `n=141`) | Push to `main` (heavy, `n=74`) | Evidence & Bottleneck Analysis (`C§2,§3` and `rust-ci.yml` line citations) |
|---|---|---|---|---|
| **1. Scale-set queue / spin-up** | P50 2–13 s | P50 2–15 s / P90 26 s | P50 6–13 s / P90 57 s (max 73 s) | ARC listener + pod start + `docker:dind` init sidecar (`C§3.1–3.3`, `B§1.1 P2`). Share of job time that is queue dropped from 96–97% (hosted) to 13–15% (`C§3.3`). |
| **2. Setup, helper, tools, cache & delta restore** (`rust-ci.yml:62-204`) | 16 s (plan only) | ~20–35 s | ~20–35 s | Checkout `fetch-depth: 2` (`:62-66`), `setup-velnor` 0.1.95 + SHA check (`:67-82`), `mbx-store`/`mbx-seed` (`:83-87`), `velnor-ci plan` (`:88-90`), `tools-local` + `mise-action` (`:94-123`), `prepare` + `check-metadata` (`:138-144`), `actions/cache/restore` of `sources`, `target`, and PR `delta` (`:145-190`), `inputs restore` + `restored` (`:191-200`). |
| **3. Format & workspace Clippy #1** (`rust-ci.yml:205-210`) | skipped | ~15–45 s | ~15–45 s | `cargo-fmt --all --check` (`:207`), then `velnor-ci inputs build -- cargo-clippy clippy --workspace --all-targets --locked` (`:210`). Per HD 11 (`docs/hard-decisions.md:488`), clippy always checks `--workspace`. |
| **4. Post-merge Docs & 18 `Check <crate> alone` steps** (`rust-ci.yml:211-267`) | skipped | skipped (`post_merge`) | ~90–160 s | On `push`/`schedule`/`dispatch` only (`docs = "post_merge"`, `check_alone = "post_merge"` in `.velnor/rust-ci.toml:7-8`): `cargo doc --workspace --no-deps` (`:213`) plus **18 sequential `cargo check -p <crate>` steps** (`:214-267`; down from 31 after `#1747` deleted 13 crates!). Each `-p` uses a distinct feature unification (`docs/hard-decisions.md:494-498`), causing cache misses. |
| **5. Test compile, Nextest, Feature lane & Second Clippy** (`rust-ci.yml:268-299`) | skipped | ** skipped without label!**<br>With label: ~60–320 s | ~180–360 s | • On `push`, `Build the tests` (`nextest run --workspace --no-run`, `:268-270`) prebuilds all test binaries.<br>• `Test` (`nextest run --workspace -E "$TEST_FILTER"`, `:278-282`; `#1748` overlaps test listing with execution).<br>• `Test (features: local-mode)` + `Test lane guard` (`:283-293`) when `velnor-runner-host` is selected.<br>• **`Lint (features: local-mode)` (`:294-296`) runs a SECOND `--workspace --all-targets` clippy build on every PR and push**, and `Documentation (features: local-mode)` (`:297-299`) runs a second workspace rustdoc on push! |
| **6. Saves (`target`, `sources`, `delta`) & metrics** (`rust-ci.yml:300-404`) | ~5 s | ~15–35 s (delta stage + save) | ~30–60 s (inputs save + target save) | PRs stage and upload `target.velnor-delta` (`:350-384`); `push` saves `inputs.tsv` and `target` (`:303-326`), plus `ci-metrics-rust` artifact (`:385-404`). |
| **7. Serial `policy` job** (`rust-ci.yml:406-505`) | skipped | **~45–78 s (serial after `rust`!)** | **~45–78 s (serial after `rust`!)** | Because `policy` has `needs: [rust]` (`rust-ci.yml:407-408`), it waits for `rust` to finish, then queues for a pod (14 s), checks out again, installs helper + tools again, and runs `cargo-deny` and `cargo machete` (`C§3.1`: 14 s queue + 78 s run = **92 s added straight to the critical path!**). |
| **8. `Required` gate + `Cache` / `rust-trim`** (`rust-ci.yml:506-666`) | 13 s (`Required`), 25 s (`Cache`) | 12–48 s (`Required` queue+run) | 11–31 s (`Required`), 20–47 s (`rust-trim`) | `Required` (`:571-612`) waits for both `rust` and `policy`, queues for a pod (2–36 s, `C§3.1`), installs helper, and runs `velnor-ci gate`. |
| **Total wall time (`C§2,§3`)** | **P50 0.8 min / P90 1.2 min** | **P50 2.9 min / P90 6.2 min** (up to 11.9 min on full failures) | **P50 7.5 min / P90 11.3 min** (`rust` job P50 371 s / P90 560 s) | Verified twice per merge (`PR` + `push`), summing **6.4+ min on failing runs and ~10.4–17.5 min on green heavy runs** (`C§5`). |

### 4.2 The next five speed levers in order of saved minutes per merge

Every lever below is grounded in `velnor-gen` / `velnor-ci` and ordered by **saved wall-clock minutes per merged PR** on the post-`#1747` 18-crate workspace.

#### Lever 1: Enable merge queue + `[ci] skip_push_after_merge_group = true` → grows into exact-tree ledger reuse (V04)
- **Why it saves time:** Today every merged PR runs the full pipeline on `pull_request` (or `merge_group`) and then repeats the exact same checks on `push` to `main` (`P50 7.5 min / P90 11.3 min`, `C§2,§5`). `velnor-gen` **already implements** `[ci] skip_push_after_merge_group = true` (`crates/velnor-rust/src/merge_group_skip.rs:33-58`, `crates/velnor-ci/src/merge_group.rs:45-86`): when `merge_group` passed on `$GITHUB_SHA`, the `push` run skips Format, Lint, Docs, all 18 `Check <crate> alone` steps, test setup, Nextest, and Doctests, running only cache-warming builds and saves. Extending that hook with `velnor-ledger` (V04) also reuses the PR verdict in `merge_group` whenever the group tree equals the PR-verified tree (60–75% of groups with auto-merge, `ARCH §8.3`).
- **Saved minutes per merge:** **5.5–9.0 min at P50 / 8.5–14.0 min at P90** (eliminates check execution on `push` and 60–75% of `merge_group` runs).
- **How to measure:** Run `velnor-ci bench collect` (`crates/velnor-ci/src/cli.rs:47`) over 30 `push` and `merge_group` runs before and after enabling `skip_push_after_merge_group = true`, and compare with `velnor-ci bench compare`: measure `push` check-step wall time (target: 0 s for check steps) and `merge_group` cache/ledger hit rate.

#### Lever 2: Record `.velnor/rust-ci-timings.json` (`velnor-ci timings`), split `rust` into parallel `lint-test` + `[[test_groups]]`, and decouple `policy` from `rust` compilation
- **Why it saves time:**
  1. Because `.velnor/rust-ci-timings.json` is missing (`rust-ci.yml:32`), `decide_shape` (`crates/velnor-rust/src/shape.rs:28-33`) puts everything into one serial `rust` job. Simply running `velnor-ci timings --repo tailrocks/velnor-new` (`crates/velnor-ci/src/cli.rs:169-171`) and `velnor-gen suggest-groups` (`crates/velnor-gen/src/lib.rs:67-70`) switches the workflow to `lint-test` or `groups`, overlapping `rust-lint` (fmt + clippy + docs + check-alone) in parallel with `rust-test` and splitting `velnor-runner-host` (which contains the 61–387 s `receipt_integrity` and `local-mode` suites, `B§4.2`, `C§3.3`) into its own parallel test group!
  2. In `one-job` shape, `policy` (`cargo-deny` + `cargo machete`) has `needs: [rust]` (`rust-ci.yml:407-408`) only to read `needs.rust.outputs.rust_deny` and `rust_machete` from the inline `plan` step—adding **78–92 s in series after `rust`** (`C§3.1`)! With a separate `rust-plan` job (or running `policy` in parallel with `rust`), `policy` overlaps `rust` completely and adds **0 s** to the critical path.
  3. Gate `Lint (features: local-mode)` (`rust-ci.yml:294-296`) on `velnor-runner-host` selection (Correctness Gap 6) so non-runner PRs stop running workspace clippy twice.
- **Saved minutes per merge:** **2.5–4.5 min at P50 / 5.0–8.0 min at P90** (78–92 s from taking `policy` off the critical path + 30–60 s from skipping duplicate `local-mode` clippy on non-runner PRs + 60–180 s from overlapping lint with tests and isolating `velnor-runner-host` test groups).
- **How to measure:** Commit `.velnor/rust-ci-timings.json`, regenerate `rust-ci.yml`, and run `velnor-ci bench compare` on 20 PR runs and 20 push runs (`ci-metrics.json` job wall times and critical-path duration from run creation to `Required` completion).

#### Lever 3: Node-local XFS reflink generations (`C04` + `C05`) + read-only mounted tool/crate stores (`C01` + `C02` + `C03`) replacing network `actions/cache` tarballs
- **Why it saves time:** Even with `tools-local` and `mbx-seed` (`rust-ci.yml:85-99`), every job currently pays network checkout (`fetch-depth: 2`, ~4–6 s), `actions/cache/restore` of `sources`, `target` (1.92 GB archive, 44–61 s on cache restore + extraction, `ARCH §3`), and PR `delta` restore + stage + save (~15–35 s). Replacing network `target`/`delta` cache extraction with an XFS reflink clone of a node-local verified generation (`0.25 s` clone + `0.31 s` 0-crate warm check, `ARCH §7.3`), mounting `~/.local/share/mise` and `~/.rustup` read-only (`EROFS`, 0 s copy), and checking out from the node bare git mirror (`≤ 1 s`) cuts pre-cargo setup + cache restore + post-cargo save by 50–80 s per job.
- **Saved minutes per merge:** **1.5–2.5 min at P50 / 3.0–5.0 min at P90** across the PR and merge-group legs.
- **How to measure:** `first_cargo_seconds` and cache restore/save durations in `ci-metrics.json` (`crates/velnor-ci/src/io/report.rs`, enforced by `[budget] setup_seconds = 10` in `.velnor/rust-ci.toml`).

#### Lever 4: Agent pre-push cluster verification (`velnor verify` / `velnor ship`, `AN01–AN04`) + strict label flow (`N02` / `L02`)
- **Why it saves time:** Agents push intermediate commits (108 of 268 post-cutover PR runs were cancelled by a newer push, `C§2`), and when a PR is ready they wait for the full PR run after labelling. With strict label mode (`N02`, unlabelled pushes run nothing) and `velnor verify` (`AN04`, snapshotting the dirty/committed worktree to `refs/velnor/wip/` and filling both T2 witness slots on warm cluster nodes against `merge-tree(queue_tip, head)` before pushing), adding `velnor:verify` becomes a ledger lookup of already-satisfied `ObligationKey`s.
- **Saved minutes per merge:** **1.5–2.5 min at P50 / 3.5–5.5 min at P90** on the PR verdict clock (label-to-green drops from ~120 s to ~3–10 s), while cutting wasted cancelled-push runner-minutes by ~40%.
- **How to measure:** Measure `label_to_green_seconds` and `reused_from_pre_push_count` in `Velnor / Details` and `velnor-ci bench` across 50 agent-shipped PRs (`AN04` gate).

#### Lever 5: Eliminate the `docker:dind` sidecar from non-Docker jobs (`P05`) + split the `velnor-runner-host` test floor (`T02`/`T03`)
- **Why it saves time:** Today every ARC runner pod starts a privileged `docker:dind` init-sidecar (3–4 s per pod + 57 probe failures/hour, `ARCH §4 row 20`), even though only 3 test binaries (`local_pair_public`, `local_host_public`, and `velnor-local` Docker tests) touch Docker. Meanwhile, `velnor-runner-host` still has 28 test binaries and `receipt_integrity` (`61–387 s` after `#1474`, `B§4.2`). Removing `dind` from non-Docker sandboxes (`P05`, warm pool `≤ 1 s` start) and sharding/Consolidating `velnor-runner-host`'s remaining multi-binary tests (`T02`/`T06`) cuts the P90 tail on whole-tree and runner PRs.
- **Saved minutes per merge:** **0.5–1.0 min at P50 / 2.5–4.5 min at P90** (primarily on the P90 tail and runner-touching PRs).
- **How to measure:** `velnor-ci timings` `test_binary_seconds` in `.velnor/rust-ci-timings.json` (`crates/velnor-model/src/repo.rs:141`) and `pod_to_first_step_seconds` in `velnor-dispatch` OTel traces (`DG11`).

### 4.3 Honestly re-derived targets from the new 18-crate single-workspace baseline

Because the repository is now **one root workspace of 18 crates** (`Cargo.toml:5-24`) after `#1290`/`#1594` and `#1747` (−452k lines of `velnor-actions-*` deleted), the old plan's "47% `generator/` or `ci-helper/` root class" (`ARCH §3`) is replaced by three real selection classes produced by `velnor_rust::select::select` (`crates/velnor-rust/src/select.rs:17-40`):
1. **Class L (Leaf / Tools-only, ~45% of PRs):** touches `velnor-model`, `velnor-rust`, `velnor-render`, `velnor-gen`, `velnor-release`, `velnor-workflows`, `velnor-ci`, `velnor-cache-rules`, or `velnor-local`. Compiles `--workspace` over a warm generation (only the changed crate closure recompiles; 0-crate check is `0.31 s`, 1–5 crate incremental over warm target is `8–22 s`), runs `-E 'package(...)'` nextest (`2–12 s`, `generator/TESTING.md:15-17`).
2. **Class R (Runner, ~35% of PRs):** touches `velnor-runner-*` or `velnor-resource-probe`. Recompiles runner crates (`15–35 s` warm) and runs `velnor-runner-host` + `local-mode` tests (`60–140 s` today; `≤ 45 s` after `[[test_groups]]` split and `T02`).
3. **Class W (Whole-workspace / global trigger, ~20% of PRs):** touches `Cargo.toml`, `Cargo.lock`, `mise.toml`, `.velnor/rust-ci.toml`, or `.github/workflows/rust-ci.yml` (`AllTrigger`, `crates/velnor-model/src/selection.rs:74-84`). Runs all 18 packages.

| Milestone | PR Verdict P50 / P90 (All PRs) | PR Verdict P50 (Class L / Class R) | Push-to-Merge Gross P50 / P90 (incl. ~45 s GitHub queue) | Push-to-Merge Net P50 / P90 (Velnor share) | Runner-min / merged PR |
|---|---|---|---|---|---|
| **Pre-cutover baseline (`95de9b704b`, 39-job old engine)** | 26.7 / 92.3 min [M] | — | 85.0 / 457.0 min [M] (121.8 / 700.8 in `C§1`) | ~77 / ~440 min [M] | 285–347 [E] |
| **Post-cutover raw (`0eb2d76a5c`, 98% red, bypass merges)** | 2.9 / 6.2 min [M, heavy] (1.1 / 5.8 all) | ~2.2 min / ~4.8 min [E] | 0.6 / 3.8 min [M, **invalid: admin bypass!**] | unmeasured (queue unused) | ~12–18 (failing early) |
| **Phase 0 + Generator tuning (Day 1: green `main`, ALLGREEN queue, `skip_push_after_merge_group`, `lint-test` + `test_groups`, `policy` parallel)** | **1.8 / 4.2 min** [E] | **1.4 min / 2.8 min** [E] | **4.0 / 9.5 min** [E] (PR + queue, `push` checks skipped) | **3.2 / 8.0 min** [E] | **~18–26** [E] |
| **Warm XFS Generations + Mirrors + Ledger Exact-Tree Reuse (Day 5)** | **55 s / 2.5 min** [E] | **35 s / 90 s** [E] | **1.8 / 5.0 min** [E] (60–75% queue tree-match lookup) | **60 s / 3.8 min** [E] | **~22–32** [E] (2 witnesses) |
| **Goal (Day 10: Per-Package Contracts Enrolled + `velnor verify` + Test Floor ≤ 45 s)** | **≤ 45 s / ≤ 2.5 min** [T] (`~10 s` after `velnor verify`) | **≤ 25 s / ≤ 75 s** [T] | **≤ 2.0 / ≤ 5.5 min** [T] | **≤ 60 s / ≤ 4.0 min** [T] | **≤ 25** [T] |
| **Stretch (Day 16: Binary-digest test keys `DG03` + ELF post-link commit stamp `R06` + Floor ≤ 15 s)** | **≤ 15 s / ≤ 60 s** [T] | **≤ 10 s / ≤ 35 s** [T] | **≤ 75 s / ≤ 3.0 min** [T] | **≤ 25 s / ≤ 90 s** [T] | **≤ 12** [T] |

*Why the re-derived targets are faster and cheaper in runner-minutes than the old plan's 90 s / 3 min / 50 runner-min goal:* Deleting the 452k-line old engine (`#1747`) shrank the workspace from 31 crates to 18 crates (`Cargo.toml:5-24`) and removed the slowest old-engine test suites (`velnor-actions-cli` 434 s, `non_inputs_differential` 679 s, `maintenance-helpers` 596 s, `C§3.3`), cutting cold build and full-suite test time by more than half before runner optimizations even begin.

---

## 5. What to keep, change, drop

### 5.1 Audit of Report E's classification (agreements, disagreements, and what E missed)

Report E (`E§6.2–6.3`) classified the 174 delivery items into **9 MARK-DONE, 7 DROP, 42 REWRITE, 22 GATE, 1 DEFER, 93 KEEP** and added **N01–N24**. We agree with 92% of E's item classifications, **disagree on 6 items where `0eb2d76a5c` or deeper code inspection changes the verdict**, and **add 6 items (`N25–N30`) that E missed**:

#### Where we disagree with Report E:
1. **`T07` (Split monolithic crates): E says `MARK-DONE` for generator and `REWRITE` for `velnor-ci` → We say `DROP` both.**
   - *Why:* `velnor-gen` is already split into 6 crates (`velnor-model`, `velnor-rust`, `velnor-render`, `velnor-release`, `velnor-workflows`, `velnor-gen`). And in `velnor-ci`, `.alint.yml:208-219` already enforces a strict 400-line limit per `.rs` file (`crates/velnor-ci/src/` is modularised into small files). Splitting `velnor-ci` into multiple Cargo packages would *increase* link passes and metadata overhead inside the single workspace while HD 11 (`docs/hard-decisions.md:488`) compiles `--workspace` anyway.
2. **`A07` (`create-github-app-token` native action): E says `DEFER until a consumer exists` → We say `KEEP` (required by `.velnor/release.toml` dogfooding).**
   - *Why:* As soon as velnor-new migrates `release-rust-ci-tools.yml` and `runner-v*` onto `.velnor/release.toml` (`#733` / `N07`), `velnor-render/src/release.rs:138,193,235` emits `actions/create-github-app-token` in `gate`, `release-pr`, and `tag`. Deferring `A07` would block Cohort 4 (`R01`) from running generated `release.yml` on Velnor Native.
3. **`P07` (Local runs on the Docker backend): E says `GATE` (conflicts with `goals.md:75` rule 11) → We say `REWRITE` to keep both `--runner native` and `--runner official` behind the `Executor` trait.**
   - *Why:* Owner decision 20 (`owner-decisions.md:43-54`) explicitly keeps `GitHub Official` as a first-class selectable executor alongside `Velnor Native` on both Docker and Kubernetes! Therefore `velnor-local` does *not* drop the ability to run the official runner locally on Docker; it drives *both* `velnor-exec` (Native) and `velnor-official` (Official) through `velnor-docker`, satisfying both `goals.md:75` and OD 20.
4. **`V08` & `V13` (Per-root keys vs Per-package keys): E keeps both as `REWRITE` → We say `MERGE V08 into V13` and `DROP V08`.**
   - *Why:* With one root workspace (`Cargo.toml:5-24`), "per-root" *is* whole-tree! Maintaining a separate `V08` (per-root) milestone before `V13` (per-package closure) is dead ceremony. `velnor_rust::select` (`crates/velnor-rust/src/select.rs:17-40`) and `.velnor/rust-inputs.lock` (`crates/velnor-ci/src/io/audit.rs`) already operate at **per-package** granularity.
5. **`AN01` (`velnor cargo` replacing `mbx` in `mise.toml:47-49`): E says `REWRITE; order after generator change` → We say `REWRITE` so `velnor cargo` wraps or delegates to `mbx` locally instead of deleting `mr_boxington = true` from `mise.toml`.**
   - *Why:* Hard decision 4 (`docs/hard-decisions.md:232-308`) and `mise.toml:7-19` require `rust-toolchain.toml` and `mise.toml` (`mr_boxington = true`) to stay in lockstep, and `rust-ci.yml:83-87` uses `mbx-store`/`mbx-seed`. Having `velnor cargo` intercept counting CI subcommands and chain to `mbx` (or plain `cargo` once `C05` retires `mbx`) avoids breaking `velnor-gen`'s `check_tools` validation.
6. **`N14` (Retire `.velnor/config.toml` and ported engine ops in `velnor-ci`): E lists it as cleanup → We elevate it to `P1 Correctness` (part of `N04`).**
   - *Why:* As we proved in `crates/velnor-gen/src/drifts.rs:75` and `crates/velnor-gen/src/io/workflows.rs:12`, the mere presence of `.velnor/config.toml` sets `engine_configured = true`, which causes `velnor-gen check` to **ignore unmarked workflow files** (`Owner::Nobody`). Deleting `.velnor/config.toml` (after moving `ci.yml` lints to `.velnor/rust-ci.toml` and `release-rust-ci-tools.yml` to `.velnor/release.toml`) is required to make `velnor-gen` the sole owner of `.github/workflows/`.

#### What Report E missed (New items `N25–N30`):
- **`N25` (Done on `0eb2d76a5c`): Overlap `cargo nextest list` with test execution (`#1161` / `#1748`).** Landed at `0eb2d76a5c` (`crates/velnor-ci/src/io/early_list.rs:1-81`), hiding 0.3–4 s of test listing behind test execution, and confirming `PACKAGE_COUNTS_LIMIT = 1_000_000` (`crates/velnor-rust/src/verdict.rs:57`).
- **`N26`: Fix the four `.alint.yml` line-cap violations on `main` (`delta_cache.rs:423`, `prepare_inputs.rs:417`, `velnor-render/src/lib.rs:154`, `velnor-rust/src/lib.rs:152`) and the broken 5-manifest `cargo-deny` command in `.github/workflows/ci.yml:117`.** Report E (`N01`) knew `Alint` and `Cargo Deny` failed in `C§2`, but attributed `Cargo Deny` to `#1218` (`RUSTSEC-2026-0543`, already ignored in `deny.toml:15`) instead of the 4 deleted manifest paths in `ci.yml:117`, and did not identify the 4 files breaching `.alint.yml:208-228`.
- **`N27`: Migrate `ci.yml`'s `Alint` and `Jactionlint` into `.velnor/rust-ci.toml` `[[lint]]` entries and delete `.github/workflows/ci.yml`.** Eliminates duplicate `cargo-deny`/`cargo-machete` runs, deletes 157 lines of legacy engine-style YAML, and brings `Alint` and `Jactionlint` inside the `Required` gate (`crates/velnor-rust/src/lint_jobs.rs:9-84`).
- **`N28`: Gate `[[test_lane]]` `Lint (features: ...)` and `Documentation (features: ...)` on the lane's `packages` in `crates/velnor-rust/src/config_test_lane.rs`.** Today `rust-ci.yml:294-296` runs `--workspace --features local-mode` clippy on every PR that selects *any* crate, even when `velnor-runner-host` is untouched.
- **`N29`: Decouple `policy` (`cargo-deny` + `cargo machete`) from `needs: [rust]` (`crates/velnor-rust/src/lane.rs:200`, `rust-ci.yml:406-410`) and commit `.velnor/rust-ci-timings.json` (`velnor-ci timings`) + `[[test_groups]]`.** Takes `policy` (78–92 s) off the critical path and splits `rust` into parallel `rust-lint` and `rust-test` / `rust-test-runner-host` jobs.
- **`N30`: Fix the `workflow_run.conclusion == 'success'` trap (`crates/velnor-render/src/release.rs:34`) where an advisory `Cache` (`report --check`) or `rust-budget` failure marks the `Rust CI` workflow run `failure` and blocks `release.yml` from triggering even when `Required` is green.**

### 5.2 Which delivery-plan streams survive and how their first items change

All 14 streams from `delivery-plan.md:629-647` survive, but their scope shrinks from **174 items (1,292 build + 120 review agent-hours)** to **156 active items (~890 build + ~95 review agent-hours)** because `#1523`, `#1550`, `#1290`/`#1594`, `#1274`, `#1335`, `#1747`, and `#1748` already completed the generator cutover, workspace consolidation, and old-engine deletion:

| Stream | Survives? | Old → New Agent-h | How the stream and its first item change on `velnor-gen` |
|---|---|---|---|
| **B (Phase 0 / Bootstrap)** | **Yes (rewritten)** | 30 h → **18 h** | `B02`, `B03`, `B08`, `B10`, `B11`, `B12`, `B13`, `B14` are **DONE/DROPPED**. Stream B now starts with **fixing red `main` and deleting `ci.yml` (`N01`/`N26`/`N27`)**, merging `#1608` (`N06`), wiring `velnor-gen check` (`N04`), fixing `pr_ci` label mode (`B07`/`N02`), recording timings/test-groups (`N29`), and applying the ruleset (`B05`/`N03`). |
| **G (Generator & Inventory)** | **Yes (centralised)** | 40 h → **24 h** | `G04` (cutover to `velnor-gen`) is **DONE** (`#1523`/`#1747`). **First item is now `G03` (Obligation Inventory v1):** instead of scraping 39 old-engine jobs, `velnor-gen` emits `.velnor/obligations.json` directly from `WorkflowPlan` (`crates/velnor-model/src/plan.rs:22-29`) and `.velnor/rust-metadata.json`! `G02`/`G05` add executor/storage/mirror keys to `.velnor/rust-ci.toml` (`crates/velnor-rust/src/config_keys.rs:4-39`). |
| **RV (Reviews before reuse)** | **Yes** | 21 h → **16 h** | **First item `RV01` + `RV02` (merged into one 6 h review):** reconcile `#1324` (`docs/proposed/kubernetes-runtime.md`) and `ARCH §5.4` against `velnor-runner-host` at journal v58. `RV03` shrinks to 2 h because `schema2_release_eligibility.rs` is already deleted (`#1747`) and `velnor-gen`/`velnor-ci` are live. |
| **P (Platform trait & backends)** | **Yes (gated on Q-N1)** | 58 h → **48 h** | **First item `P01` + `P02` (`crates/velnor-platform`):** lives in `crates/velnor-platform` (not `executor/`), merging `#1324`'s `Observation<T>` and Pod-UID proofs with ARCH's capability model (`Capabilities`). `P03` refactors existing `bollard` code in `velnor-runner-host`; `P07` keeps both Official and Native executors in `velnor-local`. |
| **O (GitHub Official executor)** | **Yes** | 33 h → **26 h** | **First item `O01` (`crates/velnor-official`):** wraps the existing official runner image (`images/runner`, `.velnor/runner.lock:17`, bump 2.338.0 → 2.338.2) behind `trait Executor` with `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` from the node store. |
| **X (Velnor Native executor)** | **Yes (gated on Q-N1)** | 140 h → **118 h** | **First item `X01`:** creates `crates/velnor-executor` and `crates/velnor-exec` in the root `Cargo.toml` (no separate `executor/` workspace). **`X13` (Background steps, 8 h) is DROPPED** because `velnor-gen` emits 0 `background: true` steps (`crates/velnor-render/src/rules.rs`). `X02` reuses `crates/velnor-runner-github` for the scale-set side and adds only broker/run-service/Results. |
| **A (Native actions)** | **Yes (gated on Q-N1)** | 49 h → **38 h** | **`A09` (Closed-set lint) is DONE** (`crates/velnor-render/src/rules.rs:16-34`). **First item `A08` (4 h):** replace the 5 remaining `actions/github-script` calls (`release-rust-ci-tools.yml:26`, `qualification.yml`) with `velnor-ci` subcommands and remove `GITHUB_SCRIPT` from `ALLOWED_ACTIONS`. Reconcile `ALLOWED_ACTIONS` (`docker/*` actions) with the native set (`N20`). |
| **H (Conformance harness)** | **Yes** | 58 h → **50 h** | **First item `H01`:** seeds `velnor-conformance` from `.velnor/workflows.toml` (`qualification.yml`, now `velnor-gen`-generated with 64 `-hosted` and `-scale-set` jobs, `#1747`), `local/conformance/*.json`, and `local/scenarios/velnor-new.json` (`#1718`). |
| **V (Ledger, witnesses, audits)** | **Yes (gated on Q-N8)** | 127 h → **96 h** | **First item `V01` (`crates/velnor-ledger`):** anchors `ObligationKey` in `crates/velnor-model/src/verdict.rs` next to `CacheKey` and `VerdictSpec`. **`V08` (per-root keys) is merged into `V13` (per-package keys)** because the repo is now a single workspace; `V14` (no-`.git` census) targets the 18 remaining crates (`velnor-ci/tests/it/plan_*.rs`). |
| **L (App & label flow)** | **Yes** | 47 h → **40 h** | **First item `L01` (Lab A) + `L02` (`crates/velnor-control`):** transitions cleanly from `velnor-gen`'s strict `pr_ci` label mode (`N02`) and single `Required` check (not two contexts). |
| **C (Generations, checkout, mirrors)** | **Yes** | 69 h → **52 h** | **First item `C01` + `C02`:** extends existing `velnor-ci tools-local` (`rust-ci.yml:94-99`) and `velnor-ci crates-mirror` (`rust-ci.yml:91-93`) to read-only mounts; `C05` builds verified XFS generations directly on `velnor-ci inputs` (`crates/velnor-ci/src/inputs.rs`) and `delta` (`#1107`). |
| **ST (Storage & native cache)** | **Yes (gated on Q-N1)** | 60 h → **54 h** | **First item `ST01` (`crates/velnor-storage`):** `BlobStore` trait; `ST03` (`crates/velnor-cache`) promotes `crates/velnor-cache-rules` + `velnor-local cache-server` (`crates/velnor-local/src/cli.rs:30-32`) onto `velnor-storage`. |
| **T & S (Test floor & cluster)** | **Yes (shrunk)** | 40 h → **20 h** | **`T04`, `T05`, `T06`, `T07`, `S04` are DONE or DROPPED** (engine tests and `scripts/ci-fixtures.sh` deleted by `#1747`; single `tests/it` already done for `velnor-ci`, `velnor-gen`, `velnor-local`, `velnor-release`, `velnor-cache-rules`). **First item `T03` + `T08`:** parallelise remaining `velnor-ci` tests and inject clock in `crates/velnor-ci/src/io/mod.rs:289` and `io/budget.rs:241`. |
| **R & F (Release & final deletion)** | **Yes** | 49 h → **42 h** | **First item `R02`:** starts by merging `#1608` (`N06`), then generates `release.yml` for both `tools-v*` and `runner-v*` (`N07`), and adds `--app-check` to `crates/velnor-ci/src/gate.rs:292-303`. `F01` updates `release-plz.toml:45-106` and `.velnor/rust-ci.toml:26-32` (`[[test_lane]]`) when retiring `velnor-runner-host`. |
| **AN & DG & BM (Agent-native, Dagger, Benchmarks)** | **Yes** | 290 h → **252 h** | **First items `DG01` (in `velnor-model`), `DG08` (`StepKind::GeneratorCheck` in `velnor-rust`), `BM03`/`B15` (built on existing `velnor-ci bench collect/compare/budget` and `timings`, `crates/velnor-ci/src/cli.rs:47,171`).** |

### 5.3 Complete item-ID disposition matrix at `0eb2d76a5c` (all 174 items + `N01–N30`)

- **`MARK-DONE` (11 items — already completed on `main` at `0eb2d76a5c`):**
  - `B02`, `B03`, `B08` (gate jobs moved to `ubuntu-26.04-scale-set` in `#1523`, `053fefa6a3`; `ci-gate-runner.yml` deleted in `#1550`, `2caae2a773`).
  - `B10`, `B11`, `B14` (old `0.1.81` release tag `#1345` superseded by `velnor-gen` cutover `#1523`).
  - `G04` (`velnor-gen` cutover `#1523` + old-engine deletion `#1747`, `57b65af0db`).
  - `A09` (closed-action-set rule enforced by `crates/velnor-render/src/rules.rs:16-34`).
  - `T04`, `T05` (old-engine test consolidation superseded by `#1747` deleting `velnor-actions-*`; single `tests/it` already in place for `velnor-ci`, `velnor-gen`, `velnor-local`, `velnor-release`, `velnor-cache-rules`).
  - `N25` (`#1161` / `#1748` overlapping `cargo nextest list` with test execution landed at `0eb2d76a5c`, `crates/velnor-ci/src/io/early_list.rs:1-81`).
- **`DROP` (9 items — obsolete after `#1290`/`#1594`, `#1523`, and `#1747`):**
  - `B12`, `B13` (old-engine `Required` + `Required verdict` two-context migration; `velnor-gen` has only one `Required` job).
  - `V08` (per-root keys; merged into `V13` per-package keys because the repository is now a single Cargo workspace at `Cargo.toml:5-24`).
  - `X13` (background step coordinator; `velnor-gen` emits 0 `background: true` steps, `crates/velnor-render/src/rules.rs`).
  - `T01`, `T06`, `T07` (old-engine crate test floor, `scripts/ci-fixtures.sh` deleted in `#1747`, and crate splitting dropped because `.alint.yml:208-219` already caps files at 400 lines and `velnor-gen` is already 6 crates).
  - `S04` (pod cache mount for deleted `velnor-actions` binary).
  - `F03` (delete `velnor-actions-*` crates; **already done** by `#1747`, `57b65af0db`, −452k lines).
- **`GATE` on owner approval of `Q-N1` / `Q-N5` / `Q-N8` (21 items):**
  - `P04`, `X01–X12`, `X14–X17`, `ST03`, `ST05`, `R01`, `F01` (require `Q-N1` `docs/goals.md` amendment; `R01` also requires `Q-N5`; `V01–V04` require `Q-N8`).
- **`REWRITE` onto `velnor-gen` / single-workspace `crates/velnor-*` (41 items):**
  - **Stream B:** `B01`, `B04`, `B05`, `B06`, `B07`, `B09`, `B15`.
  - **Stream G:** `G01`, `G02`, `G03`, `G05`.
  - **Stream RV & P & O:** `RV01`, `RV02`, `RV03`, `RV05`, `P01`, `P02`, `P03`, `P07`, `O01`, `O02`.
  - **Stream A & H:** `A03`, `A08`, `H01`, `H04`.
  - **Stream V & L:** `V04`, `V09`, `V13`, `V14`, `L03`, `L07`, `L08`.
  - **Stream C & T & R & F & AN & DG & BM:** `C01`, `C02`, `C05`, `C08`, `T03`, `T08`, `R02`, `F02`, `AN01`, `DG01`, `DG07`, `DG08`, `BM03`.
- **`KEEP` as designed in flat `crates/velnor-*` (92 items):**
  - `RV04`, `P05–P06`, `P08–P11`, `O03–O04`, `A01–A02`, `A04–A07`, `H02–H03`, `H05–H07`, `V01–V03`, `V05–V07`, `V10–V12`, `L01–L02`, `L04–L06`, `C03–C04`, `C06–C07`, `C09–C13`, `ST01–ST02`, `ST04`, `ST06–ST07`, `T02`, `S01–S03`, `S05–S08`, `R03–R06`, `AN02–AN14`, `DG02–DG06`, `DG09–DG14`, `BM01–BM02`, `BM04–BM15`.
- **`ADD` (`N01–N30`, folded into Phase 0 and Streams B/G/C/R/T):**
  - `N01–N08` and `N26–N30` form Phase 0 (Items 1–6 in Section 6.2); `N09–N24` are folded into their target items (`N09–N11` → Item 3/5/Section 6.1; `N12` → `O01`; `N13` → `T02`; `N14` → Item 2; `N15` → `RV01`/`P01`; `N16` → `V01`/`P01`; `N17–N19` → Item 4/`C02`/`C01`; `N20` → `A08`; `N21` → `F01`; `N22` → `C08`; `N23` → `H01`; `N24` → `T08`).

---

## 6. Phase 0 and the first ten work items, rewritten

### 6.1 Disposition of all open PRs as of `0eb2d76a5c`

Report D (`D§1`) listed 7 open PRs at `a619478fb2`. Since then, **`#1335` (`47ef96f8ea`) and `#1274` (`61067c17fc`) have MERGED**, and **`#1747` (`57b65af0db`) and `#1748` (`0eb2d76a5c`) also MERGED**. That leaves **5 open PRs**:

| Open PR | Title & Branch | State vs `0eb2d76a5c` | Exact Action in Phase 0 |
|---|---|---|---|
| **#1608** | `release gate: require Required from the push run on the default branch (#1555)` (`fast/1555-tools-gate`) | +35/−13, 4 files; 0 merge conflicts locally (`D§1`). Failed only because `main` was red. | **Land in Item 1** (or fold into the green-main PR): closes Correctness Gap 4 on `release-rust-ci-tools.yml:86-105`. |
| **#1200** | `fix(mise): preserve platform-specific tool locks` (`peer/1104-mise-platform-pins`, closes `#1104`) | +1639/−272, 33 files; already on the flat `crates/velnor-*` layout; 0 merge conflicts (`D§1`). | **Land in Item 3:** rebase onto green `main` (`0eb2d76a5c`), verify `cargo nextest`, merge through queue. |
| **#1320** | `(#1073) per-root inputs audit; no double workspace build` (`fast/1073-per-root-audit`) | +1317/−239, 30 files; written against old `generator/crates/*` & `ci-helper/crates/*` paths; conflicts with `#1274` (`audit_job.rs`, `io/audit.rs`). | **Rewrite and land in Item 5:** remap paths to `crates/velnor-*`, drop pre-0.1.81 compat branches (deleted by `#1713`), reconcile with `#1274`'s `crates/velnor-ci/src/io/audit/groups.rs`. |
| **#1026** | `fix(inputs): restore sibling packages and custom target directories` (`peer/1018-1011-inputs`, closes `#1011`, `#1018`) | +5792/−505, 66 files, 96 commits on old layout (`D§1`). | **Split and rewrite after Item 7** into two `<400`-line PRs on `crates/velnor-ci` (`inputs.rs`, `io/cold.rs`) and `crates/velnor-rust`, feeding directly into `C05` (Verified Generations). |
| **#1297** | `fix(deps): update cargo minor/patch` (`renovate/cargo-minorpatch`) | Conflicts on deleted `ci-helper/Cargo.lock` and old per-box lockfiles (`D§1`). | **Close immediately:** let Renovate open a fresh single-workspace PR against root `Cargo.lock`. |

### 6.2 The first ten work items (Phase 0 → Foundation)

Every item is sized in agent-hours, respects the 400-line per-file cap (`.alint.yml:208-219`), and names its implementation and independent review model + effort (`docs/roadmap.md:20-34`).

#### Item 1 (`N01` + `N26` + `N27` + `#1608`): Make `main` green, fold `ci.yml` lints into `rust-ci.toml`, delete `ci.yml`, and enforce `Required` in the tools release gate
- **Goal:** Fix every current failure on `main` (`0eb2d76a5c`), eliminate the unrequired/broken hand-maintained `.github/workflows/ci.yml`, put `Alint`, `Jactionlint`, and `lint-generator-samples` inside `rust-ci.yml`'s `Required` gate, and land `#1608` so `release-rust-ci-tools.yml` checks that `Required` passed on the push run.
- **Files:**
  - `crates/velnor-gen/tests/it/delta_cache.rs:1-423` (split into `delta_cache.rs` and `delta_cache_stage.rs`, each <250 lines)
  - `crates/velnor-ci/tests/it/prepare_inputs.rs:1-417` (split into `prepare_inputs.rs` and `prepare_inputs_overlap.rs`, each <250 lines)
  - `crates/velnor-render/src/lib.rs:1-154` & `crates/velnor-rust/src/lib.rs:1-152` (trim to ≤145 lines to satisfy `.alint.yml:220-228`)
  - `.github/workflows/release-rust-ci-tools.yml:16-105` (apply `#1608` push-run `Required` check in `gate`; replace `actions/github-script` in `tag-ruleset-drift` with `velnor-gen release ruleset-drift` so no workflow uses `github-script`)
  - `.velnor/rust-ci.toml:1-32` (add `[[lint]]` entries for `alint` with `config = ".alint.yml"` and `probe = "scripts/alint-line-cap-probe.sh"`, `jactionlint` with `config = ".github/jactionlint.yaml"`, and `mise-task` `lint-generator-samples`)
  - `.github/workflows/ci.yml` (**delete**; its broken 5-manifest `cargo-deny` at `:117` and partial `cargo-machete` at `:134` are already superseded by `rust-ci.yml`'s `policy` job at `:406-505`)
  - `.github/workflows/rust-ci.yml` & `.velnor/rust-metadata.json` (regenerated by `velnor-gen generate`)
- **Done when:** `cargo nextest run --workspace` passes; `alint check --fail-on-warning` and `scripts/alint-line-cap-probe.sh` exit 0; `jactionlint` exits 0; `velnor-gen check` exits 0; a push to `main` produces a green `Required` check covering `rust`, `policy`, `lint-alint`, `lint-jactionlint`, and `lint-generator-samples`; `.github/workflows/ci.yml` is gone; `#1608` is merged.
- **Size:** 3.0 agent-hours.
- **Implementation:** `claude-sonnet-5-5` @ `high`. **Review:** `claude-opus-5-5` @ `high`.

#### Item 2 (`N04` + `N14` + `N28`): Wire `StepKind::GeneratorCheck` (`velnor-gen check`), delete `.velnor/config.toml` + legacy plan shims, and fix `[[test_lane]]` feature-lint scoping
- **Goal:** Make `velnor-gen` self-verifying in CI (`velnor-gen check`), remove the `.velnor/config.toml` loophole (`engine_configured = true`) that tolerated unmarked workflows, delete dead old-engine port subcommands in `velnor-ci`, and gate `Lint (features: local-mode)` / `Documentation (features: local-mode)` on `velnor-runner-host` selection.
- **Files:**
  - `actions/setup-velnor/action.yml` & `crates/velnor-render/src/helper_integrity.rs` (unpack and SHA-verify `velnor-gen` alongside `velnor-ci` using `[velnor-gen]` digests already in `.velnor/rust-ci.lock:12-14`)
  - `crates/velnor-rust/src/plan.rs:274-280` (emit `StepKind::GeneratorCheck` after `StepKind::Helper(HelperCommand::CheckMetadata)`)
  - `crates/velnor-rust/src/config_test_lane.rs` (gate `CargoCommand::ClippyFeatures` and `DocFeatures` on `GateNeed::PackageArgs` with `lane.packages`)
  - `.velnor/rust-ci.toml:1-32` (migrate path mappings from `.velnor/config.toml` into `[[inputs]]` and `non_inputs`)
  - `.velnor/config.toml` (**delete**), `crates/velnor-gen/src/io/workflows.rs:12`, `crates/velnor-gen/src/drifts.rs:52-80` (remove `engine_configured` flag so `Owner::Nobody` workflows always fail `velnor-gen check` unless explicitly listed in a small `HAND_MAINTAINED` constant until `release-rust-ci-tools.yml` is generated in Item 6)
  - `crates/velnor-ci/src/plan_request*.rs`, `crates/velnor-ci/src/io/plan.rs:39-41` (delete legacy `--mode ci|qualification --request` port of deleted `velnor-actions`)
  - `generator/samples/*/expected/.github/workflows/rust-ci.yml` (update goldens)
- **Done when:** `rust-ci.yml` contains `Check the generated workflow` (`velnor-gen check`); `Lint (features: local-mode)` in `rust-ci.yml` has `contains(..., '-p velnor-runner-host') || contains(..., '--workspace')`; `.velnor/config.toml` is deleted; `cargo nextest run --workspace` passes.
- **Size:** 4.0 agent-hours.
- **Implementation:** `claude-sonnet-5-5` @ `high`. **Review:** `claude-opus-5-5` @ `high`.

#### Item 3 (`#1200` + `N02` / `B07` + `B06`): Land `#1200` and implement strict on-demand `pr_ci` label mode in `velnor-gen` (Owner Decision 12)
- **Goal:** Land `#1200` (mise platform lock matching), and update `velnor-gen` so unlabelled ready PRs and draft PRs run no build/test jobs and **fail `Required` closed ("CI not requested")** instead of passing green without tests; add `unlabeled` and `converted_to_draft` triggers to cancel in-flight runs and block merge when the label is removed; retire one-shot `ci:run` / `ci-run-label` in favour of persistent `velnor:verify` (or `ci:auto` per Q-N2).
- **Files:**
  - Rebase and merge `#1200` (`crates/velnor-rust/src/mise_lock.rs`, `os_pins.rs`, `config_fork.rs`)
  - `crates/velnor-model/src/plan.rs:40-65,116-128` (`PullRequestType::Unlabeled`, `VELNOR_VERIFY_LABEL`, `PrRun`)
  - `crates/velnor-model/src/steps.rs:110-114` (`StepKind::FailOnUnrequestedPr`, remove `DraftNotice` on `Required`)
  - `crates/velnor-rust/src/plan.rs:330-343`, `crates/velnor-rust/src/plan_budget.rs:49-75`, `crates/velnor-rust/src/required_job.rs:106-149`
  - `crates/velnor-render/src/gate.rs:20,245-270`, `crates/velnor-render/src/draft_steps.rs`
  - `docs/rust-ci.md:78-91`, `generator/CONFIG.md:537-556`, `.github/workflows/rust-ci.yml`, `generator/samples/pr-ci-label/*`
- **Done when:** On a draft PR or an unlabelled ready PR, `rust`, `policy`, and `lint-*` skip and `Required` concludes **`failure`** (`"CI not requested: add label velnor:verify"`) in <15 s; adding the label triggers the full suite and `velnor-ci gate`; pushing while labelled cancels the prior run and starts a new full run; removing the label cancels any in-flight run and marks `Required` red; golden tests pass.
- **Size:** 4.5 agent-hours (requires Tools release + `velnor-gen pin-helper 0.1.96` to activate in `.github/workflows/rust-ci.yml`).
- **Implementation:** `claude-opus-5-5` @ `high`. **Review:** `claude-opus-5-5` @ `xhigh`.

#### Item 4 (`N29` + `N18` + `B15`): Record `.velnor/rust-ci-timings.json`, split `rust` into parallel `lint-test` + `[[test_groups]]`, decouple `policy`, enable `skip_push_after_merge_group`, and set `[budget]`
- **Goal:** Harvest the immediate generator speed wins on the 18-crate workspace: (1) run `velnor-ci timings` and `velnor-gen suggest-groups` to split `rust` into parallel `rust-plan`, `rust-lint`, `rust-test-default`, and `rust-test-runner-host` jobs; (2) make `policy` depend on `rust-plan` instead of `rust` so `cargo-deny` and `cargo machete` run in parallel with compilation (saving 78–92 s on the critical path); (3) turn on `[ci] skip_push_after_merge_group = true`; (4) fix shallow `fetch-depth: 2` ancestor fallback (`crates/velnor-ci/src/io/git.rs:66-79`); and (5) configure `[budget]` in `.velnor/rust-ci.toml`.
- **Files:**
  - `.velnor/rust-ci-timings.json` (new, generated by `velnor-ci timings --repo tailrocks/velnor-new`)
  - `.velnor/rust-ci.toml` (add `[ci] skip_push_after_merge_group = true`, `[[test_groups]]` for `velnor-runner-host`, and `[budget]` table)
  - `crates/velnor-rust/src/lane.rs:200` (in multi-job shape, `policy` needs `PLAN_JOB` rather than `RUST_JOB`)
  - `crates/velnor-rust/src/plan.rs:37` & `crates/velnor-ci/src/io/git.rs:66-79` (deepen shallow range check on multi-commit `push`/`merge_group` so diff selection does not degrade to `AllTrigger::BaseUnavailable`)
  - `.github/workflows/rust-ci.yml` (regenerated)
- **Done when:** `rust-ci.yml` runs `rust-lint`, `rust-test-*`, and `policy` in parallel after `rust-plan`; `push` runs after a passing `merge_group` execute `velnor-ci merge-group-passed` and skip all check steps; `velnor-ci bench compare` shows ≥2.5 min critical-path reduction on heavy runs.
- **Size:** 3.5 agent-hours.
- **Implementation:** `claude-sonnet-5-5` @ `high`. **Review:** `claude-opus-5-5` @ `high`.

#### Item 5 (`B01` + `B05` / `N03` + `B04` + `#1320`): Apply the hardened ruleset, enable auto-merge & merge queue, and land rewritten `#1320` through the queue
- **Goal:** Once Items 1–4 and `tools-v0.1.96` are live on `main`, lock the gate: owner applies the ruleset update on `24396608` (`Required` bound to `integration_id: 15368`, `bypass_actors: []`, `grouping_strategy: "ALLGREEN"`, `max_entries_to_build: 2`, `min_entries_to_merge_wait_minutes: 0`, `check_response_timeout_minutes: 90`) and enables `allow_auto_merge: true`. Prove the full label → auto-merge → `merge_group` → `skip_push_after_merge_group` flow by landing rewritten PR `#1320` (per-root inputs audit, no double workspace build) as the first zero-bypass queue merge.
- **Files:**
  - Ruleset `24396608` JSON (`PUT /repos/tailrocks/velnor-new/rulesets/24396608`, owner-applied) & repo `allow_auto_merge: true`
  - `crates/velnor-ci/src/io/audit.rs`, `crates/velnor-rust/src/audit_job.rs`, `crates/velnor-model/src/steps.rs`, `crates/velnor-render/src/steps.rs` (rewrite of `#1320` onto `crates/velnor-*`)
- **Done when:** `gh api repos/tailrocks/velnor-new/rulesets/24396608` matches the expected JSON field-by-field (`bypass_actors: []`, `integration_id: 15368`, `ALLGREEN`); an unlabelled canary PR is `BLOCKED`; PR `#1320` is labelled, auto-enqueued, passes `merge_group`, merges via queue, and the resulting `push` run on `main` skips duplicate checks via `merge-group-passed`.
- **Size:** 3.5 agent-hours.
- **Implementation:** `claude-sonnet-5-5` @ `high`. **Review:** `claude-opus-5-5` @ `high`.

#### Item 6 (`N08` + `N07` / `#733` + `R02`): Land owner-approved `docs/goals.md` amendment and generate `release.yml` for both `tools-v*` and `runner-v*`
- **Goal:** Apply the owner-approved wording update to `docs/goals.md` and `docs/proposed/macos-scaleset-runner.md` (Section 7.3 below), unlocking the Native executor, platform trait, ledger reuse, and native cache streams. In the same wave, add `.velnor/release.toml` so `velnor-gen` generates `.github/workflows/release.yml` for both `tools-v{{ version }}` and `runner-v{{ version }}` (from `release-plz.toml:30-106`), guarded by `velnor-ci release-gate` (`crates/velnor-ci/src/gate.rs:57-139`), and delete hand-written `.github/workflows/release-rust-ci-tools.yml`.
- **Files:**
  - `docs/goals.md:11-24,43,59,75` & `docs/proposed/macos-scaleset-runner.md:37-39,318-323` (owner-approved text)
  - `crates/velnor-release/src/family.rs`, `crates/velnor-render/src/release_family.rs` (allow `family_tag` with `bins`, `member_digests = "sha256"`, `commit_file = true`, `smoke = true`, `attest = true`)
  - `crates/velnor-ci/src/gate.rs:292-303` (add `--app-check` support for the future `Velnor / Verified` App ID)
  - `.velnor/release.toml` (new)
  - `.github/workflows/release.yml` (generated by `velnor-gen generate`)
  - `.github/workflows/release-rust-ci-tools.yml` (**delete**)
- **Done when:** Every workflow in `.github/workflows/` (`rust-ci.yml`, `rust-cache-trim.yml`, `qualification.yml`, `release.yml`) carries `# Generated by velnor-gen.` and a valid `# velnor-gen-sha256:` header; `velnor-gen check` exits 0 with zero exemptions.
- **Size:** 5.0 agent-hours.
- **Implementation:** `claude-opus-5-5` @ `high`. **Review:** `claude-opus-5-5` @ `xhigh`.

#### Item 7 (`G03` + `DG01` + `V01`): Emit the Obligation Inventory from `velnor-gen` and build `velnor-ledger` core types on `velnor-model`
- **Goal:** Instead of hand-maintaining an obligation list, have `velnor-gen` derive the versioned obligation inventory from `RepoFacts` and `WorkflowPlan` (every format, clippy, feature-clippy, doc, check-alone, policy, lint, and per-package test binary obligation, with its `ReuseClass`), and implement `ObligationKey` (`DG01`) + witness record state machine (`V01`) in `crates/velnor-model` and `crates/velnor-ledger`.
- **Files:**
  - `crates/velnor-model/src/verdict.rs` (add `ObligationId`, `ReuseClass`, `ObligationKey`, `ObligationInventory`)
  - `crates/velnor-rust/src/obligations.rs` (new: derive `ObligationInventory` from `RepoFacts` and `WorkflowPlan`)
  - `crates/velnor-gen/src/lib.rs:42-51` (emit `.velnor/obligations.json` or embed in `.velnor/rust-metadata.json`, verified by `velnor-ci check-metadata`)
  - `crates/velnor-ledger/Cargo.toml` & `crates/velnor-ledger/src/{lib,key,record,witness,epoch}.rs` (new crate in root `Cargo.toml`)
- **Done when:** `velnor-gen generate` writes the complete obligation inventory for all 18 workspace crates and 3 lint jobs; `velnor-ci plan --explain` prints each obligation's canonical `ObligationKey` and `ReuseClass`; unit tests in `crates/velnor-ledger/tests/it/main.rs` prove two-witness slot filling, distinct-node/seed enforcement, priority inheritance on in-flight joins, and epoch revocation.
- **Size:** 6.0 agent-hours.
- **Implementation:** `claude-opus-5-5` @ `xhigh`. **Review:** `claude-opus-5-5` @ `xhigh`.

#### Item 8 (`RV01` + `RV02` + `RV05` → `P01` + `P02`): Reconcile `#1324` and `ARCH §5.4` into `crates/velnor-platform`
- **Goal:** Complete the review of `velnor-runner-host` (journal v58, `docker_client.rs`, `docker_spec.rs`, `launch/*`) and `#1324` (`docs/proposed/kubernetes-runtime.md`), and land `crates/velnor-platform` defining `trait Platform`, `Capabilities`, `EnvSpec`, `DirSpec`, `ProcSpec`, and `#1324`'s `Observation<T> { Present, AbsentProven(AbsenceProof), Unknown }` + `CleanupProof` (`crates/velnor-runner-core/src/ownership.rs:23-37`), plus the backend conformance test kit.
- **Files:**
  - `crates/velnor-platform/Cargo.toml` & `crates/velnor-platform/src/{lib,capabilities,spec,observation,kit}.rs` (new crate in root `Cargo.toml`, depending on `velnor-runner-core`)
  - `docs/proposed/kubernetes-runtime.md` (add superseded-by-`velnor-platform` header recording what was kept: `Observation`, `AbsenceProof`, Pod-UID preconditions, node cache PVCs)
- **Done when:** `cargo nextest run -p velnor-platform` passes the backend kit against an in-memory mock platform and tests every capability fallback (`reflink` → copy, no `PrivateDaemon` → refuse Docker class, no `read_audit` → whole-tree keys, `Unknown` observation never releases capacity).
- **Size:** 6.0 agent-hours.
- **Implementation:** `claude-opus-5-5` @ `xhigh`. **Review:** `claude-opus-5-5` @ `xhigh`.

#### Item 9 (`P03` + `O01` + `H01`): Land `crates/velnor-docker`, `crates/velnor-executor`, and `crates/velnor-official`, and run the first conformance pairs locally
- **Goal:** Extract the Docker backend (`crates/velnor-docker`, wrapping `bollard` and `docker_spec::audit_plan` from `velnor-runner-host`), define `trait Executor` in `crates/velnor-executor`, wrap the official runner (`2.338.0`/`2.338.2` from `.velnor/runner.lock`) in `crates/velnor-official` (both `Role::Production` and `Role::Reference`, with `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`), and wire `velnor-conformance` (`H01`) to run a job through `velnor-official` on `velnor-docker`.
- **Files:**
  - `crates/velnor-docker/Cargo.toml` & `crates/velnor-docker/src/{lib,client,audit,pool,proof}.rs`
  - `crates/velnor-executor/Cargo.toml` & `crates/velnor-executor/src/lib.rs`
  - `crates/velnor-official/Cargo.toml` & `crates/velnor-official/src/{lib,worker,obligation,archive_cache}.rs`
  - `crates/velnor-conformance/Cargo.toml` & `crates/velnor-conformance/src/{lib,replay,compare}.rs`
- **Done when:** `velnor-docker` passes the `velnor-platform` kit; `velnor-official` executes both an acquired job and a synthetic obligation job (`velnor-ci obligation exec`) on `velnor-docker` with zero secret/mount audit violations and 0 diffs in `velnor-conformance`.
- **Size:** 8.0 agent-hours (split into two `<400`-line PRs: `velnor-docker` first, then `velnor-executor` + `velnor-official` + `velnor-conformance`).
- **Implementation:** `claude-opus-5-5` @ `high`. **Review:** `claude-opus-5-5` @ `xhigh`.

#### Item 10 (`ST01` + `ST03` + `C05-local`): Land `crates/velnor-storage` and `crates/velnor-cache` (promoting `velnor-cache-rules`), plus content-verified generation restore in `velnor-ci`
- **Goal:** Build `crates/velnor-storage` (`BlobStore` trait with local XFS/APFS and S3/RustFS backends), promote `velnor-cache-rules` + `velnor-local`'s Twirp cache-v2 server into `crates/velnor-cache` (`ST03`), and upgrade `velnor-ci inputs restore` (`crates/velnor-ci/src/inputs.rs:194-242`) with the generation coverage and `dep-info` blob-verification rule (`C05`), fixing Q16 (older-mtime edit when `main` moved past the generation base).
- **Files:**
  - `crates/velnor-storage/Cargo.toml` & `crates/velnor-storage/src/{lib,local,s3,chunks}.rs`
  - `crates/velnor-cache/Cargo.toml` & `crates/velnor-cache/src/{lib,index,twirp,blobs}.rs` (reusing `crates/velnor-cache-rules/src/lookup.rs:8-41`)
  - `crates/velnor-ci/src/inputs.rs:194-242` & `crates/velnor-ci/src/io/audit/dep_info.rs` (diff against generation manifest `M_G`, invalidate fingerprints on read-set overlap, verify surviving unit dep-info blobs)
- **Done when:** `velnor-cache` passes all 6 GitHub cache conformance probes (`crates/velnor-cache-rules/tests/it/conformance.rs`) plus concurrent-save, stale-L0-prefix-vs-exact-L1, and PR-to-main isolation tests; `velnor-ci inputs restore` passes all 4 Q16 standing canaries (`ARCH §7.3`).
- **Size:** 7.5 agent-hours (split into two PRs: `velnor-storage` + `velnor-cache`, then `velnor-ci` generation verification).
- **Implementation:** `claude-opus-5-5` @ `high`. **Review:** `claude-opus-5-5` @ `xhigh`.

---

## 7. Owner questions, rewritten

### 7.1 Triage of D1–D26 after the `velnor-gen` cutover and `#1747`

- **OBSOLETE / ANSWERED BY EVENTS (remove from sheet: 4):**
  - **D1** (Release `velnor-actions 0.1.81`): **Done & obsolete.** `#1345` merged, `#1523` cut over to `velnor-gen`, and `#1747` (`57b65af0db`) deleted `velnor-actions`.
  - **D9** (Pre-approve CI-only engine releases): **Obsolete.** The old engine is gone; `velnor-ci`/`velnor-gen` releases use `tools-v*` (`release-plz.toml:30-44`).
  - **D14** (Delete the engine crates after G04): **Done.** `#1747` (`57b65af0db`) deleted all 12 `velnor-actions-*` crates (−452k lines).
  - **D26** (Native arm64 lane because `aarch64` musl only gets a qemu smoke test): **Premise changed, keep deferred.** `.github/workflows/release-rust-ci-tools.yml:121-124` already builds and smoke-tests `aarch64-unknown-linux-musl` natively on `ubuntu-24.04-arm`, not under qemu! Keep a self-hosted `linux/arm64` cluster worker deferred ("Not now").
- **ALREADY SETTLED (keep in settled section: 4):** **D5** (merged into D3/D7), **D6** (reference executor = Official, OD 19), **D10** (two witnesses + ALLGREEN), **D12** (release bytes built once and promoted by digest).
- **MUST BE REWORDED / RE-ASKED (6):** **D2** (merge queue + `allow_auto_merge` is currently `false` and queue unused), **D3** (ruleset `24396608` now has only `Required`, no `Required verdict`, and requires a green `main` first), **D8** (deploy identity + node XFS/RustFS changes + where cluster manifests live since no `deploy/` directory exists in `velnor-new`), **D11** (cohort cutover counts re-mapped to `velnor-gen`'s job types), **D13** (was "park ~35 open runner-host PRs"; those merged and runner-host grew to journal v58—re-ask freeze policy before `P03`/`F01`), **D16** (folded into the unified `pr_ci` label-flow question `Q-N2`).
- **STILL VALID AS WRITTEN (12):** **D4** (Expression evaluator A vs B), **D7** (`velnor-verify` App + non-admin agent identity), **D15** (bad-release recovery: redeploy digest + break-glass verifier), **D17** (owner-signed obligation/test reduction), **D18** (Velnor merge train after BM14 measurement), **D19** (no pre-label speculative compile), **D20** (rent high-clock node for BM15), **D21** (keep two independent compiles for witnesses), **D22** (rent comparator arms for BM03), **D23** (no auto-verify of unpushed commits yet), **D24** (`interactive` agent class above PR work within 30 CPU-min/h budget), **D25** (two-agent pilot for cluster sandboxes after day 6.5).

### 7.2 Consolidated decision sheet for the owner (Reworded D-questions + New Q-N1..Q-N11)

| ID | Question for the owner | Why it is needed now (with `0eb2d76a5c` evidence) | Recommendation & Default |
|---|---|---|---|
| **Q-N1** *(blocks X*, ST*, P04, P07, R01, F01)* | **Approve the exact amendments to `docs/goals.md` (Section 7.3 below) and `docs/proposed/macos-scaleset-runner.md` (§3, §8)?** | `docs/goals.md:3` and `AGENTS.md` forbid changing `goals.md` without explicit owner approval. Today `goals.md:16` ("Jobs are always executed by GitHub's official runner"), `:20-22` (runner as standalone product), `:43` (fork PRs), `:59` (GitHub's 10 GB cache), and `:75` (local mode with official runner) conflict with Owner Decisions 1, 8, 18, 20, 22, 23 (`E§2`). | **Recommend: Approve the exact diff in Section 7.3.**<br>*Default:* Blocked; no PR touching `goals.md` or cutting over to Native/RustFS lands until approved. |
| **Q-N2 (replaces D16)** *(blocks Item 3 / N02)* | **Label flow in `velnor-gen` (`pr_ci`): (a) confirm unlabelled ready PRs and drafts must run nothing and fail `Required` ("CI not requested") instead of today's unlabelled quick-set green; (b) retire one-shot `ci:run` and rename `ci:auto` to `velnor:verify`; (c) does a label applied during draft arm the PR when marked ready (`D16`)?** | Today (`.github/workflows/rust-ci.yml:582-612`), an unlabelled ready PR runs Format+Lint+Policy, skips tests and `velnor-ci gate`, and gets a **green `Required`**, while drafts get a green `Required` notice—violating Owner Decision 12 (`owner-decisions.md:20`). | **Recommend: Yes to all three:** (a) unlabelled/draft = no heavy work, red `Required` until the App gate replaces it with no status; (b) one persistent label `velnor:verify` (retiring `ci:run` and `ci-run-label`); (c) label on draft arms at `ready_for_review`. |
| **D3' + Q-N3** *(blocks Item 5 / B05)* | **Once Item 1 makes `main` green, apply the Phase-0 ruleset update to `protect-main` (`24396608`): bind `Required` (the sole required check now that `Required verdict` is gone) to GitHub Actions (`integration_id: 15368`), set `bypass_actors: []`, set `grouping_strategy: "ALLGREEN"`, `max_entries_to_build: 2`, `min_entries_to_merge_wait_minutes: 0`, `check_response_timeout_minutes: 90`?** | Post-cutover (`C§1,§4`), `Required` is unbound (`integration_id: null`), `grouping_strategy` is `HEADGREEN`, `min_entries_to_merge_wait_minutes` is `5`, and 157–167 of 173 PRs merged via admin bypass with a failing `Required`. | **Recommend: Yes, apply immediately after Item 1 turns `main` green.** (Applying before Item 1 would deadlock `main`.) |
| **D2' + Q-N4** *(blocks Item 4 & 5 / B04)* | **Flip repository setting `allow_auto_merge = true`, require all merges to go through GitHub's merge queue (no direct/admin merges, no catch-up merges of `main` into PR branches), and enable `[ci] skip_push_after_merge_group = true`?** | `allow_auto_merge` is currently `false` (`C§4`), 0 `merge_group` runs have executed since the cutover (`C§1`), and `push` runs repeat 100% of PR work (`C§5`). Enabling the queue + `skip_push_after_merge_group` saves 5.5–9.0 min per merge immediately. | **Recommend: Yes to all three.** |
| **Q-N5** *(blocks Item 6, R01, A06)* | **Release builds & provenance (`release.yml`): (a) migrate `release-rust-ci-tools.yml` and the missing `runner-v*` release workflow to `velnor-gen`'s `.velnor/release.toml` now; (b) when Cohort 4 moves releases to self-hosted Velnor (`R01`), drop `--deny-self-hosted-runners` from `crates/velnor-render/src/release_attest.rs:70` and verify against Velnor's Sigstore Fulcio/Rekor root (`velnor-runner-attestation`) instead?** | `crates/velnor-render/src/release.rs:31` hardcodes `HOSTED_RUNNER = "ubuntu-latest"` and `release_attest.rs:70` passes `gh attestation verify ... --deny-self-hosted-runners`, which rejects any attestation minted on a self-hosted Velnor runner (`E§2 C10`). | **Recommend: (a) Generate `release.yml` on hosted runners in Phase 0 (Item 6); (b) in Cohort 4 (`R01`), replace `--deny-self-hosted-runners` with Velnor's pinned `--custom-trusted-root` / workflow-signer digest check.** |
| **Q-N6** *(blocks fork-fence removal)* | **Confirm dropping the generated fork-runner fence (`fork_runner = "ubuntu-latest"`, `rust-ci.yml:57`) for `velnor-new` under Owner Decision 8 ("No pull requests from forks"), or keep the fence in `velnor-gen` for external consumer repos while disabling it for `velnor-new`?** | `docs/goals.md:43` and `generator/CONFIG.md:5-16` state that repositories with a self-hosted `runner` have no opt-out from the `fork_runner` fence, whereas OD 8 says "Drop all fork handling" (`E§2 C4`). | **Recommend: Add `allow_forks = false` (default `true` for consumers, set `false` in `velnor-new`'s `.velnor/rust-ci.toml`) so `velnor-new` drops fork expressions while `velnor-gen` stays safe for public consumer repos.** |
| **Q-N7** *(blocks shape design)* | **Keep `velnor-gen`'s `Shape` model (`one-job` / `lint-test` / `groups` sharing one warm target generation per workspace, `docs/goals.md:68-69` Rules 4–5 and HD 11) rather than resurrecting the deleted old engine's separate target directories per check kind (`target/velnor-clippy`, `target/velnor-doc`, `ARCH §8.1`)?** | `ARCH §8.1` cited `docs/hard-decisions.md:56-62` (old engine HD 1) for per-check-kind target directories, which contradicts `docs/goals.md:68` Rule 4 ("Compile at most twice") and breaks `velnor-gen`'s single-target `CacheKey::Target` and `Delta` design (`E§2 C6`). | **Recommend: Keep `velnor-gen`'s `lint-test` / `groups` shape (`goals.md` Rules 4–5).** With XFS reflink clones (`0.25 s`), parallel `rust-lint` and `rust-test` sandboxes each get an instant reflink clone of the same warm `target` generation with zero lock contention! |
| **Q-N8** *(blocks V01–V04)* | **Confirm that the verification ledger (`velnor-ledger`) and `[ci] skip_push_after_merge_group = true` are approved under `docs/goals.md:40` ("skip only when the change provably cannot affect it"), overriding old-engine HD 3 (`docs/hard-decisions.md:223-229`)?** | Old-engine HD 3 banned "baseline-gated skipping or any other second skip mechanism without owner approval" after Gate 5 was removed (`A§5.4`). Although `hard-decisions.md:14-19` binds only the deleted engine, explicit sign-off avoids any ambiguity. | **Recommend: Approve.** Ledger reuse requires two independent T2 witnesses, full-tree or read-audited keys, and cold A1 post-merge verification. |
| **D13' + Q-N9** *(blocks runner-host work)* | **Freeze `crates/velnor-runner-host` feature/journal work at v58 now that `#1263`, `#1346`, `#1731`, and `#1743` have landed, and direct all new runner work into `crates/velnor-platform`, `velnor-docker`, `velnor-k8s`, and `velnor-official`?** | ~55 runner-host commits landed between 2026-11-18 and `0eb2d76a5c` (`B§5.2`), growing `velnor-runner-host` to 181k lines that `ARCH §5.10` marks for replacement at `F01`. | **Recommend: Yes, freeze `velnor-runner-host` at journal v58.** Only port its reviewed pieces (`RV01`/`RV05`) into `velnor-docker` and `velnor-official`. |
| **D8' + Q-N10 + Q-N11** *(blocks P04, X15, backlog)* | **(a) Approve `velnor-system` deploy identity and node XFS/RustFS LV changes (`D8`), and put cluster manifests under `deploy/` in `velnor-new` (or keep in `~/Projects/tailrocks/velnor-runners`)? (b) Move dated research logs out of evergreen `docs/proposed/` (`AGENTS.md` rule)? (c) Keep `docs/roadmap.md` `exec:haiku/sonnet/opus` labels or add multi-vendor labels?** | (a) No `deploy/` dir exists in `velnor-new` (`B§0.2`); (b) `AGENTS.md` requires evergreen docs without dated logs or PR refs (`E§2 C12`); (c) `backlog-assignments.tsv` names non-Claude models (`E§2 C13`). | **Recommend:** (a) Approve `D8` and commit manifests under `deploy/` in `velnor-new` so `velnor-gen` / `alint` guard them; (b) keep dated logs in `docs/research/velnor-runner-2026-11/` only and make `docs/proposed/*.md` evergreen; (c) keep `exec:haiku/sonnet/opus` labels in GitHub issues. |
| **D11' (reworded)** *(blocks G01 cohorts)* | **Accept the re-mapped 4-cohort migration order for `velnor-gen`'s actual workflows?**<br>• **Cohort 1 (Gates & Lints, 150 pairs):** `Required`, `rust-plan`, `policy`, `lint-alint`, `lint-jactionlint`, `lint-generator-samples`, `rust-trim`, `Cache`, `ci-run-label`.<br>• **Cohort 2 (Rust build & non-Docker tests, 300 pairs):** `rust-lint`, `rust-test-default` (16 non-Docker crates).<br>• **Cohort 3 (Docker-class test groups, 150 pairs):** `velnor-runner-host` (`local_pair_public`, `local_host_public`) and `velnor-local` on private per-request Docker daemons (`P05`).<br>• **Cohort 4 (Releases & Qualification, 30 pairs + 3 dry runs):** `release.yml` (`tools-v*`, `runner-v*`) and `qualification.yml`. | The old cohort table (`ARCH §10.1`, `delivery-plan.md:728-742`) listed 26 per-crate jobs, `Required verdict`, `Ledger CLI`, `state-seal`, `job-image-candidate`, and `source-qualification`, all of which were deleted by `#1523`, `#1550`, and `#1747`. | **Recommend: Approve the 4-cohort `velnor-gen` table as written.** |
| **D4, D7, D15, D17–D25** | **Confirm recommendations on the remaining unchanged questions:** **D4** = Evaluator B (`Sdk/Expressions` port, default A until ready); **D7** = create `velnor-verify` App + non-admin agent identity; **D15** = redeploy image digest + break-glass cold 2-witness verifier; **D17** = owner-signed inventory reductions only; **D18** = keep GitHub merge queue until `BM14` measures shadow train; **D19** = no pre-label compile; **D20** = rent 1 high-clock node for `BM15`; **D21** = keep 2 independent compiles; **D22** = rent 1 vendor arm + GitHub larger runners for `BM03`; **D23** = auto-verify unpushed commits off; **D24** = `interactive` above PR work (≤30 CPU-min/h); **D25** = 2-agent cluster sandbox pilot after day 6.5. | Unchanged by `velnor-gen` cutover (`E§7.2`). | **Recommend: Approve defaults/recommendations in one batch.** |

### 7.3 Exact proposed wording change to `docs/goals.md` for Owner Approval (Q-N1)

Below is the exact line-by-line diff against `docs/goals.md:9-24,43,59,75` at `0eb2d76a5c` for the owner to approve or reject:

```diff
--- a/docs/goals.md
+++ b/docs/goals.md
@@ -9,20 +9,23 @@
 | Code | Rule book |
 |---|---|
-| New work: the Rust generator in `generator/`, the CI helper in `ci-helper/`, the runner in `crates/velnor-runner`, the local environment in `local/`, and the workflows the generator writes | This file |
-| Everything else: the existing engine in `crates/` (except `crates/velnor-runner`), its configuration in `.velnor/`, the workflows it generates, and the scripts, images and fixtures that support it. That includes this repository's own CI, Rust included, until the generator takes over the Rust part. The engine is maintained, not developed | [`hard-decisions.md`](hard-decisions.md), with the specifications under `proposed/` and the procedures under `implemented/` |
+| All active code in `crates/velnor-*` (the generator `velnor-gen`, the CI helper `velnor-ci`, the model/planner/renderer crates, the runner, executor, ledger, cache, storage and platform crates, and `velnor-local`) and the workflows the generator writes | This file |
+| Retained legacy support (`crates/velnor-archive-guard`, `qualification/`, and `scripts/`) | [`hard-decisions.md`](hard-decisions.md), with the specifications under `proposed/` and the procedures under `implemented/` |
 
 The root `AGENTS.md` applies to all code. Hard decisions bind the existing engine only. Where new work needs the same choice, this file states it.
 
-New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. Jobs are always executed by GitHub's official runner: on GitHub's machines, or on the project's own machines inside the containers that `velnor-runner` starts.
+New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. On Velnor self-hosted runners, jobs and verification obligations are executed behind one `Executor` interface either by Velnor's native Rust executor (`velnor-exec`, the default for repositories that select Velnor Native) or by GitHub's official runner (`velnor-official`, the permanent reference oracle and a selectable production executor). Every result key binds the executor identity, so results are never mixed across executors.
 
 ## Vision
 
-Velnor has two parts. The generator (`velnor-gen`, with its helper `velnor-ci`) writes the GitHub Actions workflows a project needs; they run on GitHub-hosted runners or on self-hosted runners. The runner (`velnor-runner`) runs jobs on the project's own machines. `velnor-local` runs a checkout's workflows on a developer's machine without pushing, imitating GitHub, and starts each job through the runner.
+Velnor has two parts. The generator (`velnor-gen`, with its helper `velnor-ci`) writes the GitHub Actions workflows and verification obligations a project needs; they run on GitHub-hosted runners or on Velnor's self-hosted runners. The runner (`velnor-dispatch`, `velnor-node`, `velnor-exec`, `velnor-official`, `velnor-ledger`, `velnor-cache`, and `velnor-storage`) verifies changes and runs jobs on the project's own Kubernetes cluster or Docker host. `velnor-local` and `velnor verify` run a checkout's workflows and obligations on a developer's machine or warm cluster sandboxes before pushing.
 
 The runner is developed as a product in its own right, because local mode and self-hosted runs depend on it.
@@ -43,4 +46,4 @@
-4. **Secrets never reach the build.** Private dependencies are fetched with a short-lived token that can read only the repositories the project lists. The token is revoked before any code of the project or of its dependencies runs, and cargo then runs offline. Checkout stores no credentials, and the job's `GITHUB_TOKEN` has read-only permissions while that code runs. A pull request from this repository writes a cache only into its own scope. A pull request from a fork gets no token and only reads caches. Where a fork can read the caches, as in a public repository, nothing fetched from a private dependency or built from it is cached.
+4. **Secrets never reach the build.** Private dependencies are fetched with a short-lived token that can read only the repositories the project lists. The token is revoked before any code of the project or of its dependencies runs, and cargo then runs offline. Checkout stores no credentials, and the job's `GITHUB_TOKEN` has read-only permissions while that code runs. A pull request writes a cache or candidate record only into its own untrusted scope and never into the default branch's trusted scope without post-merge verification. Where fork pull requests are enabled, a fork gets no token, runs on the configured `fork_runner`, and only reads public caches; a project where only trusted members and agents open pull requests may set `allow_forks = false`.
 5. **Every speed claim has a measurement.** A change states its effect with numbers from real runs, before and after.
-6. **Inside GitHub's free limits.** The design fits in GitHub's 10 GB of cache and on standard hosted runners. It does not depend on paid storage or larger machines.
+6. **Inside GitHub's free limits on hosted runners, and configurable storage on self-hosted runners.** A project on GitHub-hosted runners fits in GitHub's 10 GB of cache (`cache_budget_bytes` at most 8 GB) and standard hosted machines. A project running Velnor's self-hosted runner may route `actions/cache`, artifacts and build generations through Velnor's local NVMe and site S3/RustFS tiers under the same GitHub branch-scoping and immutability rules.
@@ -75,1 +78,1 @@
-11. **Local first.** A workflow is verified on the developer's machine with GitHub's official runner, started by the Velnor runner, a local stand-in for GitHub and a local cache that enforces GitHub's rules. Real GitHub confirms it afterwards.
+11. **Local and pre-push first.** A workflow or obligation is verified before pushing—on the developer's machine through the Docker backend (with `velnor-official` or `velnor-exec`, a local stand-in for GitHub and a cache that enforces GitHub's rules) or on warm cluster sandboxes through `velnor verify`. Real GitHub and the merge gate confirm it afterwards.
```

---

## 8. Be ambitious: what a generator-first foundation makes possible

The old plan treated the workflow generator and the runner as two separate worlds: a YAML generator that wrote opaque GitHub Actions files, and a runner/control plane that had to parse YAML back out, guess what checks existed, maintain a hand-written obligation inventory (`G03`), and scrape logs to know whether tests ran.

Because **`velnor-gen` (`velnor-model`, `velnor-rust`, `velnor-render`) and `velnor-ci` are pure, deterministic Rust libraries in the same workspace as the runner**, we can solve the five hardest problems in CI/CD by making the generator and the runner two ends of one typed compiler:

### 8.1 Zero-YAML-Parse Native Execution: `velnor-gen` compiles a sealed `ExecutionPlan` IR alongside `.github/workflows/rust-ci.yml`
- **The hard problem:** Writing a full GitHub Actions YAML parser, expression evaluator (`X06b`, 28 agent-hours), step-condition resolver, and action-input validator that never drifts from GitHub's quirks is the highest-risk surface in `velnor-exec` (`delivery-plan.md:836-839`).
- **What `velnor-gen` makes possible:** `velnor-rust::plan` (`crates/velnor-rust/src/plan.rs:41-118`) *already* constructs the strongly typed intermediate representation `WorkflowPlan` (`crates/velnor-model/src/plan.rs:22-29`), where every job condition is an enum (`When`, `PrRun`, `Gate`, `GateNeed`), every cargo invocation is a typed `CargoCommand` (`plan.rs:262-320`), every cache key is a typed `CacheKey` (`cache.rs:95-113`), and every action comes from `ALLOWED_ACTIONS` (`crates/velnor-render/src/rules.rs:16-34`).
- **The mechanism:** `velnor-gen generate` serialises the canonical IR (or derives it deterministically at runtime from `.velnor/rust-ci.toml` + `.velnor/rust-metadata.json`, which `velnor-ci check-metadata` already proves byte-exact!) and seals it with `# velnor-gen-sha256:` (`crates/velnor-render/src/header.rs:23-45`). When `velnor-exec` (or `velnor-node` running obligations) executes a `velnor-gen` workflow:
  1. It verifies the workflow's `# velnor-gen-sha256:` header against the base-commit `velnor-gen` library.
  2. It executes the typed `WorkflowPlan` / `ObligationSpec` IR **directly** without parsing YAML or evaluating untrusted string expressions!
  3. The YAML expression evaluator (`X06a`/`X06b`) is only needed for declared workflows (`.velnor/workflows.toml`) and conformance cases—and for `rust-ci.yml`, `velnor-conformance` proves once per generator change that `render(WorkflowPlan)` under `velnor-official` is 100% equivalent to direct IR execution under `velnor-exec`.

### 8.2 Plan-time Merkle Verdict Keys and Speculative Pre-Job Prefetch
- **The hard problem:** In conventional runners (and even in Dagger/Bazel without a monorepo rewrite), a runner does not know which caches, crates, or test binaries a job needs until the job pod starts, checks out the repo, and runs a build tool.
- **What `velnor-gen` makes possible:** `velnor_rust::detect` + `velnor_rust::select` (`crates/velnor-rust/src/select.rs:17-40`) + `crates/velnor-rust/src/lock_key.rs` + `.velnor/rust-inputs.lock` (`crates/velnor-rust/src/reads.rs`) operate purely on `RepoView` (Git tree blobs + committed `.velnor/rust-metadata.json`), **without running `cargo` or starting a sandbox**.
- **The mechanism:** When a webhook (or `velnor verify` thin pack) arrives at `velnor-control` / `velnor-dispatch`:
  1. The resident planner (`P09`) reads the tree blobs directly from the node's bare git mirror in **< 5 ms** (`read_view` over git object database).
  2. It computes `select_event` (`crates/velnor-rust/src/select_event.rs:20-32`), `lock_root` (`lock_key.rs`), and the exact `ObligationKey` vector (`DG01`) **before any sandbox is claimed**.
  3. If every `ObligationKey` hits in `velnor-ledger` and no C5 obligation is selected, `velnor-control` posts `Velnor / Verified: success` in **< 500 ms with zero pods/containers touched**.
  4.For misses, `velnor-dispatch` knows the exact `CacheKey`s, `compat`, `lock_root`, and selected package closure *before* placement, so it routes the obligation to the node whose L0 already holds that exact generation (`VELNOR_GENERATION_DIR`) and prefetches any missing L1 RustFS chunks while the sandbox is being claimed.

### 8.3 Compile-Input Enrolment Built into `velnor-gen`: Unifying `.velnor/rust-inputs.lock` (`velnor-ci inputs audit`) with the Kernel eBPF/LSM Read Audit (`V07`)
- **The hard problem:** `ARCH §6.1` notes that a runtime file-access trace alone cannot license a narrow key (`X01`: untaken branches are invisible), while static analysis alone (`docs/hard-decisions.md:199-204`) misses dynamic reads. So `ARCH §6.1` required a manual "reviewed contract in the trusted inventory" for every obligation (`V08`/`V13`).
- **What `velnor-gen` makes possible:** `velnor-gen` and `velnor-ci` *already have* an automated compile-time contract file: `.velnor/rust-inputs.lock` (`crates/velnor-rust/src/reads.rs:INPUTS_LOCK`), populated and verified by `velnor-ci inputs build` and `velnor-ci inputs audit` (`crates/velnor-ci/src/cli.rs:353-389`, `crates/velnor-ci/src/io/audit.rs`) from Cargo's `dep-info` and build-script `rerun-if-changed` outputs, and consumed at plan time by `velnor_rust::select` (`crates/velnor-rust/src/select.rs:204-216`)!
- **The mechanism:** Turn `.velnor/rust-inputs.lock` into the single machine-verified contract for both compile-time **and** test-runtime reads:
  1. Extend `.velnor/rust-inputs.lock` from `path<TAB>package` (`cli.rs:386`) to `phase(compile|test)<TAB>path_or_dir<TAB>package`.
  2. At compile time, `velnor-ci inputs audit` verifies the `compile` rows against Cargo `dep-info` + `V07`'s eBPF/LSM cgroup trace of `rustc`, build scripts, and proc macros.
  3. At test time, `V07`'s eBPF/LSM cgroup trace (plus seccomp blocking untraced path syscalls, `ARCH §6.4`) checks every file/directory opened or `stat`ed by each test binary against the `test` rows of `.velnor/rust-inputs.lock` and declared `[[inputs]]` in `.velnor/rust-ci.toml`.
  4. Any undeclared read immediately fails `velnor-ci inputs audit` and tells the developer the exact line to add via `velnor-ci inputs audit --write` (`cli.rs:382-389`)! Because `.velnor/rust-inputs.lock` is a tracked file in git, every contract expansion is diffed and reviewed in the PR that introduces the read, giving **automatic per-package key enrolment (`V13`) with zero hand-written contract boilerplate**.

### 8.4 Solving the Hard Decision 11 (`--workspace` vs `-p`) Feature-Unification Dilemma at the Generator Level
- **The hard problem:** Hard Decision 11 (`docs/hard-decisions.md:486-513`) forced `velnor-gen` to compile `--workspace` on every PR (even when only 1 leaf crate changed) because `cargo -p <leaf>` unifies features only over `<leaf>`'s subgraph, rebuilding shared dependencies with narrow features and thrashing the cache. That bends `docs/goals.md:34` Promise 1 ("A run checks only what it selects": a lint warning or broken doctest in an unselected crate fails an unrelated PR, and cold/evicted runs must compile all 18 crates).
- **What `velnor-gen` makes possible:** Because `velnor-gen` owns `.velnor/rust-metadata.json` (`crates/velnor-model/src/repo.rs:38-90`) and `velnor-rust` analyses the full Cargo dependency and feature graph at generation time:
  1. `velnor-gen` can compute the **workspace-unified feature set** of every internal and external dependency from `cargo metadata` and emit a generated workspace feature-anchor crate (or pass explicit `--features` / synthesized Cargo config) so that compiling a selected package closure `-p <pkg>` uses the exact same feature unification on third-party dependencies as `--workspace`!
  2. Alternatively, on top of a warm XFS reflink generation (`C05`) where all 18 workspace crates' `rlib`/`rmeta` artifacts are already present in `target/`, `velnor-ci` can pass `-p <selected_pkg>` whenever `velnor-gen` proves from `.velnor/rust-metadata.json` that `<selected_pkg>`'s transitive feature map over third-party dependencies equals the workspace union (which holds for **14 of the 18 crates** in `velnor-new`!). For those 14 crates, `-p` reuses 100% of the `--workspace` generation cache with zero feature-unification drift and compiles *only* the selected crate even on a cold workspace leaf!

### 8.5 One Plan, Three Surfaces (`velnor verify`, `velnor-local`, and Cluster CI) + Generator-Owned Self-Benchmarking
- **The hard problem:** In almost every project, local pre-commit hooks (`hk.pkl`), local workflow runners (`velnor-local`), and production CI (`.github/workflows/rust-ci.yml`) drift apart: hooks run raw `cargo` flags that miss feature lanes (`[[test_lane]]`), `velnor-local` runs static scenarios (`local/scenarios/*.json`), and benchmarks (`BM01–BM15`) are fragile external scripts.
- **What `velnor-gen` makes possible:**
  1. **Unified execution across all three surfaces:** `velnor check <name>` (`DG07`), `velnor verify` (`AN04`), `velnor-local`, and `velnor-dispatch` all call `velnor_rust::plan(&facts)` (`crates/velnor-rust/src/plan.rs:41`). Whether an obligation runs on a developer's Mac in `velnor-local` (Tier T0/T1) or on two cluster nodes in `velnor verify` / `velnor-control` (Tier T2/T3), it executes the **exact same `CargoCommand` argv, `CI` build profile (`crates/velnor-ci/src/prepare.rs`), `LaneGuard`, and `VerdictSpec`** generated by `velnor-rust`.
  2. **Generator-owned S1–S6 benchmark & planted-fault gate (`BM01`, `BM03`):** `velnor-ci` *already* owns `velnor-ci bench collect`, `velnor-ci bench compare`, `velnor-ci bench budget` (`crates/velnor-ci/src/cli.rs:47`, `generator/CONFIG.md:168-185`), and `velnor-ci timings` (`cli.rs:169-171`). Extending `velnor-ci bench` with `velnor-ci bench s1-faults` (applying the 100 planted failing edits of `BM01` to reflink clones of the workspace and asserting 100% red verdicts) and `velnor-ci bench replay` (replaying historical PR SHAs through `velnor_rust::select_event` and comparing arms under the pre-registered bootstrap rule of `BM09`) turns the entire `BM01–BM15` research suite into a version-locked, repeatable `velnor-ci` subcommand enforced by the generated `rust-budget` job (`crates/velnor-rust/src/plan_budget.rs:17-47`) every night.
