# velnor-gen rebase by Gemini 4 Neon

| Field | Value |
|---|---|
| Model | Gemini 4 Neon (`gemini-4-neon`) |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | xhigh |
| Date | 2026-11-20 |
| Main tip analysed | `0eb2d76a5c` |
| Brief | [velnor-gen rebase brief v1](../briefs/velnor-gen-brief-v1.md) |

Independent answer from the Velnor runner research panel, written without seeing other members' answers. Unedited except for this header and replacing home-directory paths with `~`.

---

# Rebase of the Velnor Runner Plan onto `velnor-gen`

**Panel member:** Gemini 4 Neon (XHigh) | **Date:** 2026-11-20 | **Audited tip:** `0eb2d76a5c` (`velnor-new`, post-#1747/#1274/#1335/#1748) | **Rule books:** `docs/goals.md`, `docs/hard-decisions.md`, `AGENTS.md`

---

## 1. Verdict in ten lines

1. **The final design still stands—its verification thesis is stronger on `velnor-gen`, but its topology, Phase-0 plan, and workspace layout are obsolete.**
2. Deleting the 450k-line `velnor-actions` engine (`57b65af0db`, #1747) and collapsing four workspaces into one 18-crate root (`23bed02e7e`, #1290/#1594) already eliminated the 39-job fan-out, hosted gate queue (0/2,660 Rust CI jobs hosted post-cutover, C§3.3), `state-seal`, `source-qualification`, and `job-image-candidate`.
3. **The single most important change to the plan:** promote `velnor-gen` (`velnor-model`, `velnor-rust`, `velnor-render`, `velnor-ci`) from a passive YAML emitter to the **single verification compiler** whose typed IR (`RepoFacts` → `WorkflowPlan` → `ObligationInventory`) drives GitHub Actions workflows, the Velnor Native ledger and verdict keys, and local agent `velnor verify`/`check` runs alike.
4. Do **not** create a separate `executor/` Cargo root; place every new runner, ledger, storage, and platform crate directly in the root workspace (`crates/velnor-*`), inheriting the single `Cargo.lock` and `velnor-rust::select` dependency graph.
5. **Step zero is correctness, not runner code:** `main` is red (2/268 PR and 4/172 push `Rust CI` successes post-cutover, C§2), 167/173 post-cutover merges landed with a failing `Required` via admin bypass (C§1), `pr_ci = "label"` turns `Required` green without running tests on unlabelled PRs and drafts (`crates/velnor-rust/src/required_job.rs:106-149`), and `release-rust-ci-tools.yml:86-105` checks ancestry without checking `Required`.
6. Six high-leverage fixes cost hours inside `velnor-gen` before writing a single runner crate: fixing `pr_ci` strict-label mode, migrating `ci.yml` into `.velnor/rust-ci.toml` `[[lint]]` and deleting `ci.yml`, wiring `velnor-ci check-generated` (`GeneratorCheck`), deleting `.velnor/config.toml` (which blinds `Drift::Unmarked`), fixing the `merge_group_skip` × `post_merge` silent-drop bug (`crates/velnor-rust/src/merge_group_skip.rs:9-28`), and lifting `velnor-ci-serial` (`.config/nextest.toml:17,114-116`).
7. **Hard legal gate:** `docs/goals.md:16,20-22,43,45,59,75` and `docs/proposed/macos-scaleset-runner.md:37-39,318-323` explicitly forbid a native executor, Kubernetes runtime, native cache replacing GitHub's 10 GB budget, and dropping fork fences; zero lines of Streams `X`, `ST`, `P04`, or `F01` may land until the owner approves the exact `docs/goals.md` amendment in Section 7.3.
8. Two incompatible runner proposals on `main` (`docs/proposed/kubernetes-runtime.md` #1324 keeping `velnor-host` and the Turso journal vs `velnor-runner-architecture.md` §5.10 deleting both) must be reconciled into one capability-driven `Platform` trait (adopting `#1324`'s tri-state `Observation`/`AbsenceProof` UID proofs and K8s security rules without the Turso journal or hardcoded 2-container pair).
9. Re-derived targets on the 18-crate `velnor-gen` baseline tighten the goal (PR verdict ≤ 60 s P50 / ≤ 3.0 min P90; push-to-merge ≤ 2.5 / ≤ 6.0 min gross; ≤ 20 runner-min) because 450k lines of engine crates and their 274–679 s test loops are gone, while `velnor-runner-host` (16 test binaries, 924.8 s `receipt_integrity`) remains the P90 pole until `F01`.
10. Keep Velnor Native + GitHub Official behind one `Executor` interface, XFS reflink generations + RustFS/L0 storage + node mirrors, two-witness verification records + A1 cold audit, and `Velnor / Verified` App gating—rebuilt cleanly on top of `velnor-gen`'s existing typed contracts.

---

## 2. The design mapped onto `velnor-gen`

### 2.1 Unified compiler pipeline (promoting `velnor-gen` from YAML emitter to verification compiler)

In the pre-cutover design, Velnor treated workflows as opaque YAML to parse back and reverse-engineer into an "obligation inventory" (`G03`). On tip `0eb2d76a5c`, `velnor-gen` already owns the repository model (`RepoFacts`), selection graph (`velnor-rust::select`), workflow IR (`WorkflowPlan`), verdict specification (`VerdictSpec`), and cache-key algebra (`CacheKey`). Extending that pure IR makes all three execution surfaces emit from **one** deterministic Rust compiler pass:

```text
  .velnor/rust-ci.toml + rust-metadata.json + rust-inputs.lock + mise.toml + Cargo.lock
                                          │
                           velnor-gen::io::scan (RepoFacts)
                                          │
                      velnor-rust::plan_with + obligations()   [pure, PATH=/nonexistent]
                                          │
             ┌────────────────────────────┼────────────────────────────┐
             ▼                            ▼                            ▼
    velnor-render (Target A)     velnor-ledger / control (Target B)   velnor CLI (Target C)
    ────────────────────────     ──────────────────────────────────   ─────────────────────
    • .github/workflows/         • Typed ObligationInventory +        • `velnor verify` / `check`
      rust-ci.yml                  canonical ObligationKey vectors    • Shares exact `velnor-rust`
      qualification.yml          • Plan-time lookup before pods start   selection & key algebra
      release.yml                • Two-witness slots + A1/A2 audit    • Local Docker or T2 cluster
    • GitHub-hosted & Actions    • `Velnor / Verified` App status       produces byte-identical
      consumer compatibility     • Digest-promoted releases             keys to PR & merge queue
```

### 2.2 Layer placement summary (all 13 design components)

| Design component | Generator-time (`velnor-model`, `-rust`, `-render`, `-release`, `-gen`, `-workflows`) | Run-time (`velnor-ci`) | Runner-side (`crates/velnor-*`, unified root workspace) |
|---|---|---|---|
| **Verification ledger & verdict keys** | `velnor-model`: `Obligation`, `ObligationId`, `ObligationKey`, `ReuseClass`, `ContractInputs` beside `CacheKey` (`cache.rs:95`) & `VerdictSpec` (`verdict.rs:72`); `velnor-rust`: emit `.velnor/obligations.json` from `RepoFacts` + `rust-inputs.lock` | `velnor-ci plan` (`io/plan.rs:38-93`) computes per-obligation key vector after `select_event` (`select_event.rs:20`); `velnor-ci obligation exec` runs one obligation with digest capture | `crates/velnor-ledger` (library + site service): signed records, 2-witness slots, in-flight joins, epochs, revocation, A1/A2 audit scheduler |
| **`Velnor / Verified` App status** | `velnor-model/src/repo_config.rs:11-46`: `required_check` & `[gate]` mode (`actions` vs `app`); `velnor-rust/src/required_job.rs:106` emits push-only A1 wait job once App gates PR & `merge_group` | `velnor-ci audit wait` (push `Required` wait step); `velnor-ci release-gate` (`gate.rs:57-173,292-303`) adds `--app-check` & `--push-check` | `crates/velnor-control`: GitHub App webhook receiver, 10 s REST reconciler, fenced epoch-lease writer (`Velnor / Verified` status + `Velnor / Details` check run) |
| **Label flow** | `velnor-model/src/plan.rs:41-49,116-128`: add `PullRequestType::Unlabeled`, strict `PrCi::Label` semantics (migrate & delete old quick-set bypass), configurable label (`velnor:verify`), `StepKind::FailOnUnrequestedLabel`; `velnor-rust/{plan_budget,required_job}.rs`; `velnor-render/{events,gate,draft_steps}.rs` | `velnor-ci gate` (`velnor-rust/src/verdict.rs:278`) enforces unlabelled/draft failure during Actions-gated interim phase | `crates/velnor-control` armed state machine (owner decision 12 steps 1–7) once App replaces Actions PR/group gates |
| **Reuse (agent → PR → queue → main → release)** | `velnor-model/src/plan.rs:110` (`skip_after_merge_group` generalized to `skip_when_reused: Option<ReuseSource>`), `selection.rs:65-84` (`SelectionReason::Reused`); `velnor-rust/src/merge_group_skip.rs:9-58` (fix `post_merge` exclusion first) | `velnor-ci merge-group-passed` (`merge_group.rs:45-86`) for Phase-0 push reuse; `velnor-ci plan --explain` (`io/plan.rs:81-89`) prints ledger reuse reasons & record digests | `crates/velnor-ledger` (`ensure(tree, plan)`), `crates/velnor-node` signed T2 receipts & A1 promotion into T3 trusted scope |
| **Release promotion by digest** | `velnor-release/src/lib.rs` + `.velnor/release.toml` for `velnor-new` (`tools-v`); `velnor-render/src/release*.rs` parameterize `HOSTED_RUNNER` & add `promote_by_digest` job (gate → fetch → verify → publish, 0 compiles) | `velnor-ci release-gate` (`gate.rs:57-173`) + `velnor-ci release promote` (verify receipt digest + Sigstore attestation, call existing `release publish`, `cli_release.rs:34`) | `crates/velnor-dispatch` + `crates/velnor-storage` release bucket (FI `strict` durability) storing merge-group-built, smoke-tested, attested artifacts |
| **Agent-native (`velnor verify`/`cargo`/`velnord`)** | `velnor-gen` emits named obligation inventory (`DG07`, derived from `CargoCommand` & `[[lint]]` step names); `mise.toml:47-49` switches `[wrappers.cargo]` from `mbx` to `velnor cargo` after generator drops `mbx` for `velnor-new` | `velnor-ci plan --local` & `velnor-ci obligation exec` call pure `velnor-rust` library functions directly | `crates/velnor-cli` (`velnor` binary: `verify`, `ship`, `check`, `resolve`, `wait`, `repro`, `why-red`, `explain`, `cargo`, `diff`, `run`, `status`) + `crates/velnord` per-user daemon |
| **Velnor Native vs GitHub Official** | `velnor-rust/src/config_keys.rs:4-39` + `velnor-model/src/repo_config.rs:11`: `[executor]` (`default`, per-job-type & per-obligation overrides, certified digests); `velnor-render/src/rules.rs:16-33` reconciles `ALLOWED_ACTIONS` (16) with native closed set | `velnor-ci obligation exec` (sandbox-side runner for Official obligation mode, emitting exits & test IDs over unwritable channel) | `crates/velnor-executor` (`Executor`, `ExecutorId`, `Work`, `Role`), `crates/velnor-exec` (Native), `crates/velnor-native-actions`, `crates/velnor-official` (official runner `2.338.x`), `crates/velnor-dispatch`, `crates/velnor-conformance` |
| **Platform trait** | None on workflow YAML (runner stays label/site target in `.velnor/rust-ci.toml` & `.velnor/sites.toml`) | None (helper stays platform-agnostic) | `crates/velnor-platform` (`Platform`, `Capabilities`, `EnvSpec`, tri-state `Observation<T>`/`AbsenceProof`), `crates/velnor-platform-k8s` (`kube-rs`, Pod-UID proofs), `crates/velnor-platform-docker` (refactored from `velnor-runner-host` + `velnor-local`) |
| **Storage** | Config schema in `.velnor/sites.toml` & `[storage.routes]` (validated by `velnor-gen check`) | None on hot path; `velnor-ci report` adds tier hit/miss counters | `crates/velnor-storage` (`BlobStore`, `StorageCaps`, filesystem L0 + S3/RustFS L1 + replication L2) |
| **`actions/cache` server** | `velnor-model/src/cache.rs:95` (`CacheKey`) & `velnor-render/src/cache.rs:19` (`render_key`) unchanged—plain `actions/cache` YAML runs unmodified | `velnor-ci cache {decide,trim,verify,budget}` retained for GitHub-hosted consumers; dropped from `velnor-new` once native cache + generations graduate | `crates/velnor-cache` (authoritative index + 1 MiB CDC chunks + L0 reflink extract + v2 Twirp/Azure endpoint for Official), directly reusing `crates/velnor-cache-rules` (`lib.rs:10-14`) |
| **Git & crate mirrors** | `.velnor/rust-ci.toml` `[crates_mirror]` (`config_keys.rs:13`) models crate mirror; extend with `[mirrors]` for git/toolchains/images or read from `.velnor/sites.toml` | `velnor-ci crates-mirror` (`io/crates_mirror.rs`) + `velnor-ci tools-local` (`tools_local.rs`) stay active during transition; native `actions/checkout` & `mise-action` supersede them on Velnor Native | `crates/velnor-node` bare git mirrors (`flock` single-writer, sealed tree bases, full staged-tree hash), site crates/toolchain/registry mirrors |
| **Warm snapshots (generations)** | `velnor-model/src/cache.rs` + `velnor-rust/src/compat.rs` compat fingerprint fields reused directly as generation key inputs | `velnor-ci inputs` (`inputs.rs:194-243`) & `io/audit/dep_info.rs:16-49` refactored so diff base is generation manifest `M_G` (fixing Q16 base-drift bug) | `crates/velnor-node` generation builder (XFS reflink clones, per-unit eBPF/LSM read sets, twin cold builds, dep-info blob verification) |
| **`mbx`** | `.velnor/rust-ci.toml:6` (`mbx_shared_store = true`), `velnor-model/src/steps.rs:100-102` (`MbxRemote`, `MbxStore`, `MbxSeed`); remove from `velnor-new` config only after XFS generations beat `mbx` in lab B3 (`C08`) | `velnor-ci mbx-store` (`mbx_store.rs:32` `trusted_event`) & `mbx-seed` kept for non-Velnor self-hosted users; `trusted_event` policy reused for ledger writer scoping | Background node-seeding candidate only (`C08`: kept only if it saves ≥ 20 runner-min/day over XFS generations, else deleted) |

### 2.3 Detailed per-component architecture & exact types/functions to extend

1. **Verification ledger & verdict keys (`crates/velnor-model`, `crates/velnor-rust`, `crates/velnor-ci`, `crates/velnor-ledger`):**
   - Extend `crates/velnor-model/src/verdict.rs:9-75` (`JobCondition`, `VerdictJob`, `VerdictSpec`) with `ObligationId(String)`, `ReuseClass { Content, PerRequest, Never }`, and canonical `ObligationKey` (versioned struct containing `schema_version`, `definition_digest`, `executor_id`, `execution_fingerprint`, `reuse_class`, and canonical sorted `ContractInputs`).
   - Add `pub fn obligations(facts: &RepoFacts, plan: &WorkflowPlan) -> ObligationInventory` in `crates/velnor-rust/src/obligations.rs` called from `crates/velnor-rust/src/plan.rs:40-132` (`plan_with`). Every check, clippy invocation, fmt, doc, `local-mode` lane, `[[lint]]` command, `policy` check, and release prerequisite emitted into `WorkflowPlan` produces a matching typed `Obligation`.
   - Extend `crates/velnor-model/src/selection.rs:65-84` (`SelectionReason` += `Reused(RecordDigest)`) so `velnor-ci plan --explain` (`crates/velnor-ci/src/io/plan.rs:81-89`) and `velnor explain` emit identical reuse diagnostics.
   - `crates/velnor-ledger` stores signed witness records, in-flight deduplication handles, epoch counters, and A1/A2 audit state—consuming `velnor-model::ObligationKey` directly with zero duplicate serialization schema.

2. **`Velnor / Verified` App status & label flow (`crates/velnor-model`, `crates/velnor-rust`, `crates/velnor-render`, `crates/velnor-control`):**
   - **Interim Actions-gated phase (Phase 0):** Fix `PrCi::Label` in place (no parallel mode):
     - `crates/velnor-model/src/plan.rs:41-49`: add `PullRequestType::Unlabeled`; `:116-128`: remove the unlabelled quick-set bypass from `PrRun`.
     - `crates/velnor-model/src/steps.rs:108-112`: reuse existing `StepKind::FailOnDraftRun` (`:111`, already rendered at `crates/velnor-render/src/draft_steps.rs:41-48`) and add `StepKind::FailOnUnrequestedLabel`.
     - `crates/velnor-rust/src/plan_budget.rs:49-76` (`triggers`, `draft_guard`) & `crates/velnor-rust/src/required_job.rs:106-160` (`verdict_job`, `tests_expected`): when `pr_ci = "label"`, emit `FailOnDraftRun` on drafts and `FailOnUnrequestedLabel` ("CI not requested") on unlabelled ready PRs, drop quick-set runs on unlabelled PRs, and cancel in-flight runs when `unlabeled` fires.
     - `crates/velnor-render/src/events.rs:40-58` & `gate.rs:241-270`: emit `unlabeled` in `on.pull_request.types` and fail `Required` whenever the label is absent.
   - **App-gated phase (`W10` → `L05`):** Add `[gate]` (`mode = "actions" | "app"`, `app_context = "Velnor / Verified"`) to `crates/velnor-model/src/repo_config.rs:11-46` and `crates/velnor-rust/src/config_keys.rs:4-39` (`KEYS`, currently 34 entries). In `mode = "app"`, `velnor-rust` drops `pull_request` and `merge_group` triggers from `rust-ci.yml` and renders `Required` on `push` as a single `StepKind::AuditWait` (`velnor-ci audit wait`) step that awaits A1 cold-audit completion on `main`. `crates/velnor-control` owns the owner-decision-12 armed state machine (steps 1–7) and writes `Velnor / Verified` under an epoch-fenced leader lease.

3. **Reuse across agent run → PR → queue → main → release (`velnor-rust`, `velnor-ci`, `velnor-ledger`):**
   - Generalize `Step.skip_after_merge_group: Option<PlanFrom>` (`crates/velnor-model/src/plan.rs:110`) to `skip_when_reused: Option<ReuseSource>`.
   - Fix `crates/velnor-rust/src/merge_group_skip.rs:9-28,54-56` immediately (Section 3, P0-5) so `When::NotCancelledPostMerge` steps (`Doc`, `DocFeatures`, `CheckAlone`) are **never** marked skippable after `merge_group`.
   - Extend `crates/velnor-rust/src/verdict_plan.rs:28-111` (`PlanOutput::resolve`) and `verdict.rs:207,278` (`verdict`, `decide`) to accept signed ledger receipts (generalizing `merge_group_ran` at `verdict_plan.rs:103-105`), always executing `ReuseClass::Never` (C5) obligations fresh.
   - Delete legacy `args.mode`/`args.request` dispatch in `crates/velnor-ci/src/io/plan.rs:38-44` and `crates/velnor-ci/src/plan_request/diff/config.rs:14` so `velnor-ci plan` has one clean path from `select_event` → ledger lookup → `plan_steps`.

4. **Release promotion by digest (`crates/velnor-release`, `crates/velnor-render`, `crates/velnor-ci`):**
   - `crates/velnor-ci/src/gate.rs:57-173,292-303` (`release_gate`, `require`): add `--app-check <context:app_id>` and `--push-check <context>` so `require()` accepts the `velnor-verify` App alongside GitHub Actions `15368`, and asserts active ruleset enforcement with empty `bypass_actors`.
   - `crates/velnor-render/src/release.rs:31,165,180`, `release_binaries.rs:52-60,257,293`, `release_family.rs:242,269`, `release_attest.rs:45,70`: parameterize `HOSTED_RUNNER` (`"ubuntu-latest"`) via `.velnor/release.toml` runner policy and add a `promote_by_digest` job mode (zero checkout, zero repo code, verifies digest + attestation + calls existing `velnor-ci release publish` at `cli_release.rs:34`). Gated on owner ruling `Q-N5` because `release_attest.rs:70` currently passes `--deny-self-hosted-runners`.

5. **Agent-native `velnor verify` / `velnor cargo` / `velnord` (`crates/velnor-cli`, `crates/velnord`):**
   - `velnor verify` calls `velnor_rust::plan_with` + `obligations()` locally in < 50 ms, submits the exact obligation vector to `velnor-control` as a T2 run, streams `velnor check <obligation>` progress, and prints the resulting witness digests.
   - `velnor cargo` (backed by `velnord` on local socket) replaces `mbx` in `mise.toml:47-49` only after `velnor-gen` removes `mbx_shared_store` from `.velnor/rust-ci.toml:6`, preventing conflicting Cargo wrappers.

6. **Velnor Native vs GitHub Official & `ALLOWED_ACTIONS` (`crates/velnor-executor`, `-exec`, `-native-actions`, `-official`, `-render`):**
   - Reconcile `crates/velnor-render/src/rules.rs:16-33` (`ALLOWED_ACTIONS`, 16 entries) with Velnor Native's closed action set: today `ALLOWED_ACTIONS` includes `DOCKER_LOGIN`, `DOCKER_SETUP_BUILDX`, `DOCKER_BUILD_PUSH`, `GITHUB_SCRIPT`, `ATTEST_BUILD_PROVENANCE`, and `MR_BOXINGTON`. Replace all 3 remaining live `actions/github-script` calls (`release-rust-ci-tools.yml:26`, `qualification.yml` ×2) with `velnor-ci` commands (`A08`) and remove `GITHUB_SCRIPT` from `ALLOWED_ACTIONS`.
   - `crates/velnor-official` wraps the official runner binary (bumping `.velnor/runner.lock:19` from `2.338.0` → latest `2.338.x`), running behind the exact same `Executor` trait in both `Production` and isolated `Reference` roles. Reuse `crates/velnor-runner-github` (8.2k LOC pure scale-set client, org/group support `#1330`) instead of re-porting `scaleset/client.rs` from `velnor-legacy`.

7. **Platform trait reconciling `#1324` (`kubernetes-runtime.md`) vs ARCH §5.10 (`crates/velnor-platform`, `-platform-docker`, `-platform-k8s`):**
   - `#1324` (`docs/proposed/kubernetes-runtime.md:11-16,80-139`) assumes keeping `velnor-host`, its Turso journal, and a hardcoded 2-container `WorkerSpec { runner, engine }` pair (`:99-105`), while ARCH §5.10 deletes `velnor-host` and the journal.
   - **Reconciliation (`W07`):** adopt ARCH's capability-declaring `Platform` trait + `EnvSpec` (single container by default; privileged `dind` sidecar ONLY on tainted pool when `needs_docker = true`, no Turso journal), AND import `#1324`'s three strongest inventions: (a) tri-state `Observation<T> { Present(T), AbsentProven(AbsenceProof), Unknown(PlatformError) }` (`kubernetes-runtime.md:113-118`, timeouts never map to `AbsentProven`), (b) `WorkloadRef`/`StorageRef` carrying Pod/PVC UID preconditions for delete/absence proofs, and (c) Kubernetes `HOST_NEEDLES`, Pod Security `baseline` + separate `velnor-dind` namespace, default-deny `NetworkPolicy`, and etcd `secrets-encryption` (`:391-417`).
   - `crates/velnor-platform-docker` lifts the audited `bollard` spec rules (`docker_spec.rs` `ContainerPlan`/`audit_plan`/`HOST_NEEDLES`) and `runner-v1.json` seccomp profile out of `velnor-runner-host`.

8. **Storage, `actions/cache` server, mirrors, warm snapshots, and `mbx` (`crates/velnor-storage`, `-cache`, `-node`):**
   - `crates/velnor-cache` directly imports `crates/velnor-cache-rules` (`lib.rs:10-14`: `access_for`, `Entry`, `Scope`, `lookup`, `reserve`, `evict`, `ENTRY_LIFETIME_SECONDS`, `REPOSITORY_LIMIT_BYTES`) as its authoritative policy engine—zero re-implementation of GitHub cache scoping/eviction semantics.
   - `crates/velnor-node` replaces `velnor-ci crates-mirror`, `velnor-ci tools-local`, and `mbx-seed` file copies on Velnor Native with read-only node mirrors and XFS reflink clones (0.25 s clone + 0.31 s warm no-op build `[M, experiments/mbx-cache-cluster.md]`). Refactor `velnor-ci inputs` (`crates/velnor-ci/src/inputs.rs:194-243`) and `io/audit/dep_info.rs:16-49` so the incremental diff base is the immutable generation manifest `M_G` rather than the previous PR's mutable cache (closing the Q16 base-drift bug structurally).

### 2.4 What `velnor-gen` already gives for free

- **Pure, hermetic, sub-2-second compiler core:** `velnor-model`, `velnor-rust`, `velnor-render`, `velnor-release`, and `velnor-gen` run under `PATH=/nonexistent` with 2 s test timeouts (`.config/nextest.toml:12-13,25-33,118-120`), strict `io/` separation, and 38 golden integration samples (`crates/velnor-gen/tests/it/golden.rs`).
- **Executable YAML security rules (`crates/velnor-render/src/rules.rs:16-45`):** closed 16-action SHA-pinned list, one-command `run:` (forbidding `if/for/while/case/coproc` and `| & && || ; $( \` <( >(`), 512-char cache key cap, 500 KB workflow cap, automated `job_trust_boundary.rs` audit (`docs/implemented/job-trust-boundary.md:19-28`), `HELPER_COMMANDS` registry (`crates/velnor-workflows/src/helper_call.rs:6-45`, 38 commands), version-floor gate (`crates/velnor-gen/src/helper_gate.rs:8-62`), and launcher binary SHA-256 verification on every step (`.github/workflows/rust-ci.yml:67-82`, `.velnor/rust-ci.lock`).
- **Static + dynamic dependency selection:** `velnor_rust::select` (`crates/velnor-rust/src/select.rs:17-89`) + `.velnor/rust-metadata.json` + `.velnor/rust-inputs.lock` (38 audited compile-time input edges) + `velnor-ci check-metadata` (`crates/velnor-gen/src/io/check.rs:18-51`) + `velnor-ci lane-guard` (`#1335`, `rust-ci.yml:291-293`) + overlapped nextest listing (`#1748`, `0eb2d76a5c`, `crates/velnor-ci/src/io/early_list.rs`) + 1 MB package-count budget (`crates/velnor-rust/src/verdict.rs:57`).
- **Typed cache keys & conformance oracle:** single-renderer `CacheKey`/`render_key`, member-version-blind `lock_root` (`velnor-rust/src/lock_key.rs`), `compat` hash (`velnor-rust/src/compat.rs`), `inputs.tsv` blob+mtime manifest (`inputs.rs:9-243`), `dep_info.rs` verifier, PR build deltas (`#1107`), `tools-local`, `crates-mirror`, `mbx_store::trusted_event` (`mbx_store.rs:32`), and `crates/velnor-cache-rules` tested against live GitHub conformance recordings (`local/conformance/*.json`).
- **Built-in measurement & budgeting CLI:** `velnor-ci report` (`ci-metrics.json`), `velnor-ci bench {collect,compare,budget}`, `velnor-ci timings`, `velnor-gen suggest-groups`, and `[budget]` job generation (`crates/velnor-rust/src/plan_budget.rs:17-47`).
- **Existing runner primitives:** `crates/velnor-runner-github` (8.2k LOC pure scale-set client), `crates/velnor-runner-core` (`CleanupProof::covers` fixed `#1275` at `ownership.rs:31-41`; `Acquired` state `#1144`), and `velnor-runner-host`'s audited `docker_spec.rs` rules.

### 2.5 Naming collisions & workspace layout ADR (`N16`)

| Colliding term | Existing meaning on `0eb2d76a5c` | Proposed design meaning | Binding resolution |
|---|---|---|---|
| `audit` | `velnor-ci inputs audit` (compile-time dep-info scan, `rust-ci.yml:300-302`) | Post-merge cold `A1` & clock-shifted `A2` verification audits | Keep `inputs audit`; name ledger audits `cold-audit` / `A1`/`A2` (`velnor-ci audit wait`) |
| `ledger` | `velnor-local` authority dispatch log (`local/README.md:135`) & `runner-host` `request_ledger` | Verification-result store (`crates/velnor-ledger`) | Rename local authority log to `dispatch-log`; reserve `ledger` strictly for verification records |
| `gate` / `verify` | `velnor-ci gate` (Actions `needs` judge) & `velnor-ci release-gate` | `velnor verify` (agent pre-push 2-witness run) & `Velnor / Verified` | Keep `velnor-ci gate` / `release-gate` for workflow steps; reserve `velnor verify`/`check` for user/agent CLI |
| Binaries & root | `velnor-host`, `velnor-local`, `velnor-gen`, `velnor-ci`; single root `Cargo.toml` | `velnor` + `velnord` + separate `executor/` root | **Drop `executor/` root**; keep all crates in `crates/velnor-*`. Keep tiny static `velnor-ci` & pure `velnor-gen`; unify operator/agent/local CLI into `velnor` as `F01`/`P07` retire `velnor-host` and `velnor-local` |

---

## 3. Correctness first

### 3.1 Ranked correctness gaps on `main` (`0eb2d76a5c`)

| Rank | Correctness gap on `0eb2d76a5c` | Verified evidence | Concrete fix & location | Needs owner? |
|---|---|---|---|---|
| **P0-1** | **Red `main`, unbound `Required`, admin-bypass merges, `HEADGREEN` queue** | Post-cutover: 2/268 PR & 4/172 push `Rust CI` passes (C§2); 167/173 merges had failing `Required` on head, 157/173 direct merges via inferred admin bypass (C§1); ruleset `24396608` requires `Required` with no `integration_id`, `bypass_actors: [RepositoryRole 5]`, `HEADGREEN` (C§4). | 1) Fix all current `main` lint/test/cache failures (`#1555`); 2) apply ruleset `24396608` update in §3.2.5 (`integration_id: 15368`, `bypass_actors: []`, `ALLGREEN`, `min_wait: 0`); 3) enable `allow_auto_merge = true`. | **Yes** (ruleset PUT, repo setting, zero-bypass policy) |
| **P0-2** | **`pr_ci = "label"` turns `Required` GREEN without tests on unlabelled ready PRs AND on drafts; `unlabeled` missing** | `.github/workflows/rust-ci.yml:5-12` lacks `unlabeled`; `required` (`:571-612`) emits `DraftNotice` (exit 0) on drafts (`draft_steps.rs:11-18`) and skips `velnor-ci gate` on unlabelled ready PRs (`required_job.rs:112-135`, `gate.rs:253-270`), passing `Required` on quick-set only. Violates owner decision 12 & `goals.md:41`. | Replace `PrCi::Label` semantics (migrate & delete, no parallel mode): add `PullRequestType::Unlabeled` (`velnor-model/src/plan.rs:41-49`, `velnor-render/src/events.rs:40-58`), emit `StepKind::FailOnDraftRun` (`draft_steps.rs:41-48`) + `StepKind::FailOnUnrequestedLabel` ("CI not requested", exit 1), stop quick-set on unlabelled PRs, cancel in-flight runs on `unlabeled`. | **Yes** (`Q-N2`: label name `velnor:verify` vs `ci:auto`, retire `ci:run`) |
| **P0-3** | **Tools release gate checks only branch ancestry, never that `Required` passed** | `.github/workflows/release-rust-ci-tools.yml:86-105` declares `checks: read` (`:93`) unused and only calls `compare/$default_branch...$GITHUB_SHA`. Violates `docs/goals.md:77` rule 13. Meanwhile `runner-v` (`release-plz.toml:45-106`) has no release workflow at all. | Rebase & merge open PR `#1608` (verifies push `Required` on default branch); then extend `crates/velnor-ci/src/gate.rs:292-303` (`require()`) for `--app-check` & `--push-check` before App cutover. | **No** for `#1608`; **Yes** for hosted vs self-hosted release policy (`Q-N5`) & `runner-v` (`Q-N9`) |
| **P0-4** | **Hand-written `ci.yml` is broken (`Cargo Deny` reads 4 deleted manifests, `Cargo Machete` checks 1/18 crates), non-gating, and duplicates `policy`** | `.github/workflows/ci.yml:117` runs `cargo deny` on deleted `ci-helper/`, `crates/velnor-runner/`, `generator/`, `local/Cargo.toml` (`deny.toml:15` ALREADY ignores `RUSTSEC-2026-0543`); `ci.yml:134` checks `crates/velnor-archive-guard` only; `ci.yml:5-10` triggers on unfiltered `edited`; `ci.yml:98-99` pins stale mise `2026.11.1`; `Alint` & `Jactionlint` are NOT in `protect-main` (C§4). | Pin `"github:asamarts/alint" = "0.22.1"` in `mise.toml`; add `[[lint]]` entries (`jactionlint`, `alint` + `probe = "scripts/alint-line-cap-probe.sh"`, `mise-task` `lint-generator-samples`) to `.velnor/rust-ci.toml` (`generator/CONFIG.md:216-258` already wires them into `Required`); regenerate `rust-ci.yml`; **delete `.github/workflows/ci.yml`**. | **No** (implements `goals.md:76` rule 12) |
| **P0-5** | **Silent check-disappearance bug: `skip_push_after_merge_group = true` + `docs/check_alone = "post_merge"` drops `cargo doc` & all 18 `Check <crate> alone` steps on BOTH `merge_group` and `push`** | `crates/velnor-rust/src/docs_post_merge.rs:6-18` sets `When::NotCancelledPostMerge` on `Doc`, `DocFeatures`, `CheckAlone` (skipped on `merge_group`). `crates/velnor-rust/src/merge_group_skip.rs:9-28,54-56` unconditionally puts `Doc`, `DocFeatures`, `CheckAlone` in `is_check` and sets `skip_after_merge_group`! Enabling it on `velnor-new` silently drops docs and alone-checks until nightly cron. | In `crates/velnor-rust/src/merge_group_skip.rs:54-56` (ordered after `docs_post_merge::apply` in `plan.rs:128-131`), never set `skip_after_merge_group` on any step with `step.when == When::NotCancelledPostMerge`. Add golden regression test. | **No** (pure soundness bug) |
| **P1-6** | **Dead `StepKind::GeneratorCheck` + orphaned `.velnor/config.toml` blinding `Drift::Unmarked`** | `StepKind::GeneratorCheck` (`velnor-model/src/steps.rs:108`) is never emitted, and `velnor-render/src/helper_steps.rs:74-80` emits `velnor-gen check` even though `actions/setup-velnor/action.yml:43,100-107` installs ONLY `velnor-ci`! Worse: `.velnor/config.toml` is NEVER read by `rust-ci.yml` (`io/plan.rs:38-44`), yet sets `engine_configured = true` (`velnor-gen/src/io/mod.rs:179`, `drifts.rs:75`), disabling `Drift::Unmarked`! | Expose `velnor-ci check-generated` (calling already-linked `velnor-gen =0.1.95` at `crates/velnor-ci/Cargo.toml:39`); route `generator_check_step()` through it and emit before tools install; delete `.velnor/config.toml`, dead engine commands in `velnor-ci/src/cli.rs:82-89,140,152-167,175-185`, and `engine_configured` in `drifts.rs:75`. | **No** (confirm zero external callers of legacy `velnor-ci` subcommands) |
| **P1-7** | **Merge queue unused (`0` `Rust CI` `merge_group` runs) & `min_wait = 5 min`** | Zero `merge_group` runs post-cutover (C§1); `Rust CI`'s `merge_group` path is untested live; `allow_auto_merge = false` (C§4). | Enable `allow_auto_merge = true`, set `ALLGREEN` & `min_entries_to_merge_wait_minutes = 0`, land green PRs through the queue to exercise `merge_group` end-to-end. | **Yes** (`Q-N4`, `D2'`, `D3'`) |
| **P1-8** | **Same-job trust boundary gap for cache saves & `ACTIONS_RUNTIME_TOKEN`** | `docs/implemented/job-trust-boundary.md:38-59` documents that `actions/cache/save` (`rust-ci.yml:313-348,378-384`), `upload-artifact` (`:395-404`), and post-test `velnor-ci` steps share the `rust` job with repo code, exposing `ACTIONS_RUNTIME_TOKEN` and target dirs. | Document interim acceptance under decision 8 (trusted authors, PRs isolated to `velnor-dlt-pr<N>`) during Phase 0; close structurally via `velnor-node` out-of-sandbox observation & storage (`V02`, `ST03`), removing in-job cache/artifact tokens entirely. | **Yes** (`Q-N6`, reconcile with `goals.md:43`) |
| **P1-9** | **Noisy `Cache` job failures (17/60 sampled runs) & hosted `rust-cache-trim.yml`** | `Cache` (`rust-ci.yml:627-666`) failed 17/60 sampled runs (C§2) while advisory (`cache_check_blocks = false`), conditioning reviewers to ignore cache bugs; `crates/velnor-render/src/delta_cleanup.rs:23` hardcodes `runs_on: "ubuntu-latest"` (`.github/workflows/rust-cache-trim.yml:20`). | Diagnose & clear cache-waste causes on green `main` (verify `inputs save`/`restore` + delta staging on `0.1.95`); thread `facts.config.runner` into `delta_cleanup()` so zero PR-triggered workflows hit hosted queues. | **No** |
| **P2-10** | **`fetch-depth: 2` degrades multi-commit pushes & batched merge groups to select-everything** | `crates/velnor-rust/src/plan.rs:37` (`PLAN_DEPTH = 2`) + `crates/velnor-ci/src/io/git.rs:66-79` (`range_base` runs `git merge-base --is-ancestor base HEAD`): whenever `base` is > 1 commit behind `HEAD` (e.g., batched `merge_group` of 2–10 PRs), `range_base` errors and `io/plan.rs:176-182` falls back to `AllTrigger::BaseUnavailable`. | In `crates/velnor-ci/src/io/git.rs:66-79`, if `base` is absent in a shallow clone, run a bounded blobless fetch (`git fetch --depth=1 --filter=blob:none origin <base>` or deepen to merge-base) before failing closed to `BaseUnavailable`. | **No** |
| **P2-11** | **Unmapped non-Rust files (`UnknownPath`) trigger all 18 crates** | `crates/velnor-rust/src/classify.rs:29-97` & `select.rs:262-276`: editing `deny.toml`, `.alint.yml`, `.github/workflows/{ci,qualification,release-rust-ci-tools}.yml`, `.velnor/workflows.toml`, `release-plz.toml`, `local/**`, or `scripts/*` hits `AllTrigger::UnknownPath` → full 18-crate compile & test. | Add `[[inputs]]` (`local/**` → `velnor-local`, script tests) and explicit `classify.rs` rules (`deny.toml` → policy-only; `.velnor/workflows.toml` & `qualification.yml` → `velnor-gen`/`velnor-workflows`). Keep fail-closed `UnknownPath` for truly unmapped paths. | **No** |
| **P2-12** | **Docs, samples & pin drift (`CONFIG.md`, `velnor-new` sample, `mise.toml:93`, `runner.lock`)** | `generator/CONFIG.md:190-215` vs `:259-283` contradict each other on `paths`/`events` in `[[test_groups]]`; `generator/samples/velnor-new` is stale (A§1.4); `mise.toml:93` still hardcodes `build.jobs=2 --jobs 2`; `.velnor/runner.lock:19` pins `2.338.0` (deprecates 2027-01-04) vs plan's `2.338.2`. | Deduplicate `CONFIG.md`, refresh `generator/samples/velnor-new`, delete `build.jobs=2` at `mise.toml:93`, bump `.velnor/runner.lock` to latest `2.338.x` in `W09`. | **No** |

### 3.2 Root-cause & fix mechanics for the five sharpest code-level traps

1. **Why `pr_ci = "label"` passes `Required` without tests (P0-2):**
   - `crates/velnor-rust/src/plan_budget.rs:49-76` wires `draft_guard` to `StepKind::DraftNotice` (`crates/velnor-render/src/draft_steps.rs:11-18`, prints a notice and exits **0**), even though `StepKind::FailOnDraftRun` (`velnor-model/src/steps.rs:111`, `draft_steps.rs:41-48`, exits **1**) already exists in the codebase!
   - On unlabelled ready PRs, `crates/velnor-rust/src/required_job.rs:106-149` sets `tests_expected` to false when the label is absent, and `crates/velnor-render/src/gate.rs:241-270` skips `velnor-ci gate` whenever tests were not requested—leaving `Required` green after quick-set lint/policy alone.
   - `crates/velnor-render/src/events.rs:40-58` omits `unlabeled`, so removing the label never cancels an in-flight run. Fixing all four sites together (and removing quick-set execution on unlabelled PRs) aligns `velnor-gen` with owner decision 12 before a single line of `velnor-control` exists.
2. **Why enabling `skip_push_after_merge_group` today breaks coverage (P0-5):**
   - In `crates/velnor-rust/src/plan.rs:128-131`, `docs_post_merge::apply` marks `Doc`, `DocFeatures`, and `CheckAlone` with `When::NotCancelledPostMerge` (`docs_post_merge.rs:6-18`) so they skip PRs and `merge_group` and run on `push`.
   - `crates/velnor-rust/src/merge_group_skip.rs:9-28,54-56` classifies those exact three `StepKind`s inside `is_check` and attaches `skip_after_merge_group` unconditionally. On a repo with `docs = "post_merge"` and `check_alone = "post_merge"` (which `velnor-new` sets at `.velnor/rust-ci.toml:12-13`!), a `merge_group` run skips them (not post-merge) and the subsequent `push` skips them (`merge_group_passed == true`)—so neither run executes them. Filtering `!matches!(step.when, When::NotCancelledPostMerge)` in `merge_group_skip.rs:54-56` closes the bug permanently.
3. **Why `.velnor/config.toml` + `GeneratorCheck` + `ci.yml` form one cleanup unit (P0-4, P1-6):**
   - `#1747` left `.velnor/config.toml` believing `rust-ci.yml` reads `[stacks.rust]` for selection; verified false (`rust-ci.yml:90` invokes `velnor-ci plan --explain` without `--mode`/`--request`, so `io/plan.rs:38-44` bypasses `plan_request::run`). Meanwhile `crates/velnor-gen/src/io/workflows.rs:12` & `io/mod.rs:179` check `.velnor/config.toml` existence to set `engine_configured = true`, silencing `Drift::Unmarked` (`drifts.rs:75`) so hand-written workflows like `ci.yml` pass `velnor-gen check` undetected!
   - And `crates/velnor-ci/Cargo.toml:39` already links `velnor-gen = "=0.1.95"` as a library, so exposing `velnor-ci check-generated` gives zero-overhead `velnor-gen check` inside every generated workflow without adding a second binary to `actions/setup-velnor/action.yml:43,100-107`.
4. **Why `velnor-ci` still carries ~9 dead engine subcommands (`crates/velnor-ci/src/cli.rs`):**
   - `WriteRequest` (`:85`), `ResolveQualification` (`:89`), `Image` (`:140`), `VerifyRuntime` (`:155`), `VerifyMacProof` (`:159`), `RunAuthority` (`:163`), `AdmitPreview` (`:167`), `ValidateArtifactId` (`:175`), `StageBundle` (`:183`), and legacy `plan --mode --request` all served the deleted engine and source-preview pipelines. Deleting them in `W02` shrinks the 33.7k-LOC `velnor-ci` compile unit and removes obsolete attack surface before `velnor-ci` splits in `T07`.
5. **Exact target Phase-0 JSON for ruleset `24396608` (`protect-main`, applied in `W05` after `W02` makes `main` green):**
   ```json
   {
     "name": "protect-main",
     "enforcement": "active",
     "bypass_actors": [],
     "rules": [
       {
         "type": "required_status_checks",
         "parameters": {
           "strict_required_status_checks_policy": false,
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
           "max_entries_to_merge": 5,
           "min_entries_to_merge": 1,
           "min_entries_to_merge_wait_minutes": 0,
           "check_response_timeout_minutes": 90
         }
       }
     ]
   }
   ```

---

## 4. Speed on the new baseline

### 4.1 Where the time goes now (honest post-cutover anatomy)

Report C's post-cutover window (4.3 h, 2026-11-19 13:31Z–17:50Z) proves one real win and exposes two measurement traps:
- **Proven win:** The 79% hosted-runner queue tax is **eliminated** for `Rust CI`. All 2,660 sampled post-cutover `Rust CI` jobs ran on `ubuntu-26.04-scale-set` with queue P50 **2–13 s**, P90 **13–57 s** `[M, C§3.3]`. Heavy PR runs completed in **2.9 min P50 / 6.2 min P90** (n=141), heavy push runs in **7.5 min P50 / 11.3 min P90** (n=74), and plan-only runs in **0.8 min P50 / 1.2 min P90** `[M, C§2]`.
- **Trap 1 (invalid push-to-merge):** Post-cutover push-to-merge (**0.6 min P50 / 3.8 min P90**, C§1) is an artifact of 167/173 admin-bypass merges on a red `main` (only 2 PR and 4 push runs succeeded—all docs/regen runs taking 56–180 s, C§6). Real green PR and merge-queue latencies are currently **unmeasured**.
- **Trap 2 (stale job shape):** C's per-job table (`rust-root`, `rust-crates_velnor_runner`, `rust-ci_helper`) straddled the `#1290/#1594` single-workspace collapse and predated `#1747` (which deleted 12 engine crates and 450k lines, shrinking `rust-ci.yml` from 31 to 18 crates).

Inspecting the live 666-line `.github/workflows/rust-ci.yml` at `0eb2d76a5c` reveals five exact critical-path bottlenecks on a real green PR and merge:
1. **Forced serial chain (`rust` → `policy` → `Required`):** Because `.velnor/rust-ci-timings.json` does not exist, `decide_shape` (`crates/velnor-rust/src/shape.rs:28-33`) emits `one-job` (`rust-ci.yml:32`). Because `plan` is embedded inside `rust`, `Lane::policy` (`crates/velnor-rust/src/lane.rs:34-41`) sets `needs: [rust]` (`rust-ci.yml:406-409`). Thus `policy` (checkout + `setup-velnor` + tools restore + `mise` + `cargo-deny` + `cargo-machete`) waits for the **entire** compile + clippy + test + delta-save cycle, adding ~45–90 s directly to wall time.
2. **Self-inflicted test serialization:** `.config/nextest.toml:17,110-116` puts **every test in `package(=velnor-ci)`** (42.5k test lines) in `velnor-ci-serial = { max-threads = 1 }` for both `default` and `ci` profiles! And `:80-83` gives `crates_mirror` tests `threads-required = "num-test-threads"`, pausing every other concurrent nextest worker across the workspace while they execute.
3. **HD11 `--workspace` compile + double feature-lane clippy + 3× `nextest list` + 16 runner-host binaries:** Every PR touching any crate runs `cargo clippy --workspace --all-targets` (`rust-ci.yml:210`), `cargo nextest run --workspace` (`:282`), a **second** `cargo clippy --workspace --all-targets --features local-mode` (`:296`), and—when `velnor-runner-host` is selected—`Test (features: local-mode)` (`:287`), `Test lane guard` (`:293`, calling `cargo-nextest list` 3 times), and 16 separate integration test binaries in `crates/velnor-runner-host/Cargo.toml` (including the 924.8 s `receipt_integrity` test).
4. **Network cache & per-job setup overhead:** Each `rust` job pays network `actions/checkout`, `setup-velnor`, `mbx-seed` file copies into ARC `emptyDir`, `tools-local`/`actions/cache` tools restore, `mise-action`, `sources` restore, multi-GB `target` restore over GitHub cache, `delta` restore/apply, `inputs restore`, and `delta` stage+save (~45–120 s per job; plus multi-GB `target` + `sources` uploads on `main`).
5. **Wasted & duplicate runs:** Unlabelled ready PR pushes still run the quick set (setup + target/delta restore + `--workspace` clippy + `policy`, driving 108/268 cancelled PR runs, C§2); post-merge `push` repeats full tests (because `skip_push_after_merge_group` is off and the queue is unused) plus 2× `cargo doc --workspace`, 18 serial `Check <crate> alone` steps (`:214-267`), and `Build the tests` prebuild (`:211-270`).

### 4.2 The next five levers in order of saved minutes per merged PR

1. **Lever 1 — Un-serialize `velnor-ci` tests + activate `shape = lint-test` / `test_groups` + parallelize `policy` (`W06`)**
   - *Changes:* Isolate tempdirs/env in `crates/velnor-ci/tests/it/**` so `velnor-ci-serial` (`max-threads = 1`, `.config/nextest.toml:17,114-116`) and `crates_mirror` `num-test-threads` (`:80-83`) are deleted; run `velnor-ci timings` to commit `.velnor/rust-ci-timings.json` so `decide_shape` splits `rust-plan` + parallel `rust-lint` / `rust-test` and automatically lets `policy` (and the migrated `[[lint]]` jobs) run in parallel with `rust-lint`/`rust-test`; split `velnor-runner-host` Docker/local-mode tests into `[[test_groups]]`.
   - *Estimated savings `[E]`:* **3–6 min wall, 3–5 runner-min per merged PR** (`velnor-ci` tests drop from ~60–90 s → ≤ 12 s; ~60 s serial `policy` tail disappears; clippy overlaps nextest).
   - *How to measure (and falsify):* Run `velnor-ci bench collect` + `velnor-ci bench compare` over ≥ 10 green PRs before and after; compare `ci-metrics.json` `Test` duration, `policy` start offset relative to `plan` end, and total PR verdict P50/P90. Revert the `lint-test` split if `rust-lint` + `rust-test` spin-up exceeds saved wall time (`docs/goals.md:69` rule 5).

2. **Lever 2 — Strict label flow + revive `ALLGREEN` merge queue (`min_wait = 0`, `auto_merge`) + fixed `skip_push_after_merge_group` (`W04`, `W05`, `W06`)**
   - *Changes:* Stop running quick-set on unlabelled ready PR pushes (eliminating ~40–45% superseded PR churn); flip `allow_auto_merge = true` (removing ~7.5 min manual enqueue delay) and `min_entries_to_merge_wait_minutes = 0` (removing the 5 min artificial queue floor); fix `merge_group_skip.rs:54-56` to keep `NotCancelledPostMerge` steps and enable `[ci] skip_push_after_merge_group = true` so `main` push skips tests already proven in `merge_group`.
   - *Estimated savings `[E]`:* **5–10 min push-to-merge wall, 6–12 runner-min per merged PR** (eliminates duplicate push test run + ~0.5–1.5 wasted PR runs per merge + 5 min queue wait).
   - *How to measure:* Run `c-data/an_prs.py` and `an_dup.py` across ≥ 50 green queue-merged PRs: track `last_push → green`, `green → enqueue`, `enqueue → merge`, cancelled runs per PR, and `merge-group-passed` `skip=true` rate (must show `Documentation` & `Check <crate> alone` still executing on `push`).

3. **Lever 3 — XFS reflink warm generations + node git/tool/crate mirrors on Velnor K8s backend (`P04`, `C01–C05`, `ST03`)**
   - *Changes:* Replace network `actions/checkout`, `mbx-seed` file copies into ARC `emptyDir`, and 44–61 s GitHub `actions/cache` restores with node-local XFS reflink clones (0.25 s clone + 0.31 s warm no-op build `[M, experiments/mbx-cache-cluster.md]`), read-only tool/crate stores (≈ 0 s), node bare git mirror (< 1 s verified checkout), and RustFS L1 / NVMe L0 cache server (`velnor-cache`).
   - *Estimated savings `[E]`:* **2–4 min verdict wall, 8–15 runner-min per merged PR** (setup + warm compile drops from ~90–180 s → ~15–25 s; eliminates multi-GB GitHub cache uploads on `main`).
   - *How to measure:* Track `ci-metrics.json` `first_cargo_seconds` (target: `goals.md:57` ≤ 30 s → ≤ 2 s on Velnor Native), `S6` warm-compile benchmark (300 replayed real diffs vs cold control at idle and 80% cluster load), and GitHub WAN egress bytes per merge.

4. **Lever 4 — Content-addressed verification ledger + `Velnor / Verified` App gate + `velnor verify`/`ship` pre-push reuse + digest release promotion (`L02–L05`, `V01–V10`, `AN01–AN04`, `R04`)**
   - *Changes:* Move PR and `merge_group` verdicts off GitHub Actions jobs (removing 13–27 s acquire + job spin-up); resolve exact-tree `merge_group` verdicts from PR ledger entries (60–75% hit rate with auto-merge + queue-tip `merge-tree`); count agent `velnor verify` T2 cluster runs toward the two witness slots; promote merge-group-built release binaries by digest (4.1 min tools rebuild → ~15 s).
   - *Estimated savings `[E]`:* **3–5 min push-to-merge wall, 15–25 runner-min per merged PR** (label → green falls to 3–15 s after `velnor verify`; 60–75% of merge groups skip tests; 0 release compiles).
   - *How to measure:* `velnor-control` OTel spans (`label → Velnor / Verified success`), ledger hit-rate per obligation class, `S3` head-to-head replay against live Actions runs, and `R05` end-to-end tag-to-release duration.

5. **Lever 5 — Test-floor cleanup + `UnknownPath` input mapping + `velnor-ci` crate split (`W06`, `T02`, `T06`, `T07`, `F01`)**
   - *Changes:* Consolidate `velnor-runner-host`'s 16 integration test binaries to 1 `tests/it` (and retire the 924.8 s `receipt_integrity` test at `F01`, or split beforehand if `velnor-host` stays); map `local/**`, `deny.toml`, `.velnor/workflows.toml`, and `release-plz.toml` in `classify.rs`/`.velnor/rust-ci.toml` to eliminate spurious `AllTrigger::UnknownPath` full-workspace runs; split 33.7k-LOC `velnor-ci` into ≤ 10k-LOC crates (`T07`) so helper edits rebuild less.
   - *Estimated savings `[E]`:* **2–5 min P90 wall (up to 15 min on `velnor-runner-host` PRs), 4–8 runner-min per merged PR**.
   - *How to measure:* `velnor-ci timings` per-binary wall inventory (`BM08`), `AllTrigger::UnknownPath` frequency across 100 PRs via `velnor-ci plan --explain`, and 30-diff edit-to-test compile timing on `velnor-ci`.

### 4.3 Re-derived targets (honest numbers on the 18-crate `velnor-gen` baseline)

Every post-cutover green row below is `[E]` or `[T]` until **W06** (`N09`) collects ≥ 20–50 green queue-merged PRs on the repaired baseline. Because `#1747` deleted 12 engine crates (450k lines, including `non_inputs_differential` 679 s and `actions-cli` 274 s loops), the old "47% helper/generator PR class" disappears and is replaced by four single-workspace selection classes from `velnor-rust::select`: **docs/meta-only** (0 crates, ~15%), **leaf crate** (`velnor-render`, `velnor-cache-rules`, `velnor-release`, `velnor-local`, ~25%), **tools/model core** (`velnor-model`, `velnor-rust`, `velnor-gen`, `velnor-ci`, ~35%), and **runner-host / whole-workspace** (`velnor-runner-host` 16 bins + `local-mode` lane, or `Cargo.toml`/`Cargo.lock` edits, ~25%).

| Milestone / state | PR verdict P50 / P90 | Push-to-merge P50 / P90 (gross, incl. ~45 s GH queue) | Push-to-merge P50 / P90 (net of GH queue) | Runner-min / merged PR | Basis |
|---|---|---|---|---|---|
| Pre-cutover engine (`CI`, Nov 16–19) | 26.7 / 92.3 min | 121.8 / 700.8 min (Nov 18 baseline: 85 / 457) | — | 285–347 (CI only) | `[M, proxy]` C§1, ARCH §1.1 |
| Post-cutover raw (Nov 19, 4.3 h, **red/bypassed**) | 2.9 / 6.2 min (heavy PRs, mostly failing early) | *0.6 / 3.8 min (invalid: 167/173 bypass)* | — | 6.4 (`Rust CI`) + 5.6 (`ci.yml`) | `[M]` C§1, C§2, C§5 |
| **Projected green `velnor-gen` baseline (after W01–W05, before Lever 1)** | ≈ 4.0 / 10.0 min | ≈ 9.0 / 22.0 min | ≈ 8.2 / 21.0 min | ≈ 22–35 | `[E]` 18 crates, 1 job + serial `policy`, `ALLGREEN` queue, no reuse; **measure in W06 (`N09`)** |
| **Phase 0+1 complete (Levers 1–2 on ARC, zero new runner)** | **≤ 2.5 / ≤ 6.0 min** | **≤ 5.0 / ≤ 12.0 min** | **≤ 4.2 / ≤ 11.0 min** | **≤ 18** | `[T]` parallel `lint`/`test`/`policy`, parallel `velnor-ci` tests, strict labels, `skip_push`, `min_wait=0` |
| **App gate + Native K8s + whole-tree keys + cold 2nd witness** | ≈ 3.5 / 9.0 min | ≈ 5.5 / 14.0 min | ≈ 4.7 / 13.0 min | ≈ 45 (2 witnesses + A1, before compile reuse) | `[E]` cold 2nd witness is ~2× smaller post-#1747 |
| **Goal (Levers 1–5 + compile reuse + per-package keys + F01)** | **≤ 60 s / ≤ 3.0 min** | **≤ 2.5 / ≤ 6.0 min** | **≤ 1.7 / ≤ 5.0 min** | **≤ 20** | `[T]` tighter than old 90 s / 4 min & 50 runner-min because 450k engine lines are gone |
| **Stretch (`velnor verify` T2 + per-package keys + post-link stamp)** | **≤ 15 s / ≤ 60 s** | **≤ 75 s / ≤ 3.0 min** | **≤ 30 s / ≤ 2.0 min** | **≤ 10** | `[T]` gross floor dominated by ~45 s GitHub merge queue (`D18'`) |

---

## 5. What to keep, change, drop

### 5.1 Audit of Report E's 174-item classification (agreements, disagreements, and misses)

Report E audited all 174 delivery-plan items against `a619478fb2` (peeking at `#1747`) and proposed: **9 MARK-DONE, 7 DROP, 42 REWRITE, 22 GATE, 1 DEFER, 93 KEEP**, plus missing items `N01–N24`. Verified directly against tip `0eb2d76a5c`:

- **Group-by-group AGREEMENTS with Report E:**
  - **DONE (remove from remaining backlog, 7 of E's 9):** `B08` (cancel stall eliminated on scale set), `B10` (macOS release paused), `B11` (narrow per-push workflows deleted in `#1550/#1554`), `A09` (`ALLOWED_ACTIONS` enforced in `velnor-render/src/rules.rs:16-33`), `G04` (cutover complete via `#1523/#1747`; keep only the `audit wait` push-job rewrite), `T06` (almost done—only `velnor-runner-host` 16 bins, `velnor-archive-guard` 5, `velnor-runner-core` 4 remain), `T07` (generator already split into 6 crates; keep only `velnor-ci` 33.7k-LOC split).
  - **DROP (obsolete, all 7 of E's):** `B02` (engine `0.1.81`), `B13` (engine per-job profiles), `B14` (engine `0.1.82`), `X13` (`background: true` has **0** uses across all 5 workflows at `0eb2d76a5c`), `T04` (89 s `ci-fixtures.sh` deleted by `#1747`), `T05` (`non_inputs_differential.rs` deleted by `#1747`), `S04` (`job-image-candidate.yml` deleted by `#1554`).
  - **GATE on explicit owner approval of `docs/goals.md` / specs (`Q-N1`, `Q-N5`, `Q-N8`, `Q-N9`, all 22):** `P07`, `X01–X12`, `X14–X16`, `A06`, `V04` (HD3/goals clarity), `ST03`, `R01`, `F01`.
  - **REWRITE core premise (most of E's 42):** `B01`, `B05–B07`, `B15`, `RV03–RV04`, `G01–G03`, `G05`, `L06–L08`, `V05`, `V08`/`V13` (per-package replaces per-root in 1-root workspace), `C01`/`C02`/`C05`/`C08`/`C09`, `R02`, `R04`, `DG07`, `DG08`, `DG11`.
  - **KEEP (unchanged core, ~89 of E's 93):** `P01–P06`, `O01–O04`, `A01–A05`, `H01–H09`, `L01–L05`, `V01–V03`/`V06–V07`/`V09–V12`/`V14–V16`, `AN01–AN06`, `C03–C04`/`C06–C07`, `ST01–ST02`/`ST04–ST06`, `S01–S03`, `BM01–BM15` (re-baselined on `velnor-ci bench`).

- **Ten verified DISAGREEMENTS / corrections to Report E at `0eb2d76a5c`:**
  1. **E§3 item 1 ("enable `skip_push_after_merge_group` immediately") — REJECT until fixed (`N25`):** As proven in §3.2.2, `crates/velnor-rust/src/merge_group_skip.rs:9-28,54-56` skips `Doc`, `DocFeatures`, and `CheckAlone` even when `docs_post_merge.rs:6-18` already skipped them in `merge_group`. Enabling it today silently drops post-merge docs and all 18 alone-checks!
  2. **`B12` ("No `-j2` caps") — DISAGREE with MARK-DONE:** `mise.toml:93` (`test-maintenance-helpers`) still runs `cargo --config build.jobs=2 nextest run ... --jobs 2`. Keep as a 0.2 h fix inside `W02`.
  3. **`B03` ("Gate jobs off hosted runners") — PARTIAL DISAGREE:** `rust-ci.yml` is 100% self-hosted, but `.github/workflows/rust-cache-trim.yml:20` (`crates/velnor-render/src/delta_cleanup.rs:23`) still hardcodes hosted `ubuntu-latest` (`N29`), and `release-rust-ci-tools.yml` runs on hosted `ubuntu-24.04`/`-arm`.
  4. **`N01` root cause (`Cargo Deny` failure) — CORRECT E:** `deny.toml:15` ALREADY ignores `RUSTSEC-2026-0543` (`chrono` via `turso 0.8.3`). `ci.yml`'s `Cargo Deny` fails because `.github/workflows/ci.yml:117` passes 4 deleted nested manifests (`ci-helper/`, `crates/velnor-runner/`, `generator/`, `local/Cargo.toml`). The right fix is migrating `alint`/`jactionlint`/`lint-generator-samples` to `.velnor/rust-ci.toml` `[[lint]]` and deleting `ci.yml` (`N26`, subsuming `N17`).
  5. **`N04` (`GeneratorCheck`) — SHARPEN FIX:** Simply emitting `StepKind::GeneratorCheck` (`velnor-render/src/helper_steps.rs:74-80` → `run: velnor-gen check`) will **fail in CI** because `actions/setup-velnor/action.yml:43,100-107` installs ONLY `velnor-ci`! Expose `velnor-ci check-generated` calling the already-linked `velnor-gen = "=0.1.95"` library (`crates/velnor-ci/Cargo.toml:39`).
  6. **`N14` (`.velnor/config.toml`) — ELEVATE TO P1 CORRECTNESS:** `#1747` kept `.velnor/config.toml` under a false premise (`rust-ci.yml` never reads it, `io/plan.rs:38-44`), AND its presence sets `engine_configured = true` (`velnor-gen/src/io/mod.rs:179`, `drifts.rs:75`), silently blinding `Drift::Unmarked`! Delete it in `W02`.
  7. **`T03` (parallel `velnor-ci` tests) — ELEVATE TO PHASE 0 (`N27`):** E deferred checking `.config/nextest.toml`. Verified on `0eb2d76a5c`: `.config/nextest.toml:17,110-116` forces ALL `velnor-ci` tests single-threaded (`max-threads = 1`) and `:80-83` locks all runner threads for `crates_mirror`. Lifting this is Lever 1.
  8. **`X01` (`executor/` Cargo root) — DROP separate workspace permanently:** Keep all new crates under `crates/velnor-*` in the root `Cargo.toml`, eliminating lockfile drift and duplicate dependency compiles; `X01` shrinks to a workspace provenance lint + crate-size gate.
  9. **Live action counts (`A01–A08`, `X12`) — UPDATE TO POST-#1747 `0eb2d76a5c`:** Across all 5 workflows on `main`: `actions/checkout` 29, `cache/restore` 10, `jdx/mise-action` 9, `upload-artifact` 7, `cache/save` 7, `setup-velnor` 6, `download-artifact` 5, `actions/github-script` **3** (not 5), `mr-boxington-action` 2, `qualification/actions/*` 6, `asamarts/alint` 1 (gone when `ci.yml` deleted), `actions/cache` 1, `velnor-verify-jactionlint` 1 (gone when `ci.yml` deleted), `background: true` **0**.
  10. **`T02` ("Loop splits") — RE-SCOPE:** Both `actions-cli` looped tests and `non_inputs_differential` are deleted (`#1747`). Only `velnor-runner-host`'s `receipt_integrity` (924.8 s) remains; skip splitting it if `F01` deletes `velnor-host` (`Q-N9`), and split it only if `velnor-host` stays a shipped Mac product.

- **Six new items Report E missed (`N25–N30`):**
  - **`N25`:** Fix `merge_group_skip.rs:9-28,54-56` vs `docs_post_merge.rs:6-18` silent check-drop bug before enabling `skip_push_after_merge_group`.
  - **`N26`:** Migrate `Alint` (+ probe), `Jactionlint`, and `lint-generator-samples` into `.velnor/rust-ci.toml` `[[lint]]` and delete `.github/workflows/ci.yml` (fixes broken `ci.yml:117`, single-crate `ci.yml:134`, unfiltered `edited`, and moves lints into `Required`).
  - **`N27`:** Lift `velnor-ci-serial` (`max-threads = 1`, `.config/nextest.toml:17,114-116`) & `crates_mirror` `num-test-threads` lock, and populate `.velnor/rust-ci-timings.json` (`shape = lint-test`) to decouple `policy` from `needs: [rust]`.
  - **`N28`:** Map unowned repo paths (`local/**`, `deny.toml`, `.velnor/workflows.toml`, `release-plz.toml`) in `.velnor/rust-ci.toml` / `classify.rs` to eliminate spurious `AllTrigger::UnknownPath` full-workspace triggers.
  - **`N29`:** Move `rust-cache-trim.yml` off hosted `ubuntu-latest` (`crates/velnor-render/src/delta_cleanup.rs:23`) so zero PR-triggered workflows hit hosted queues.
  - **`N30`:** Clean `mise.toml:93` `build.jobs=2`, duplicate `generator/CONFIG.md:190-215` section, stale `generator/samples/velnor-new`, and reconcile `docs/implemented/job-trust-boundary.md:38-59` with decision 8.

### 5.2 Delivery-plan streams: survival and rewritten entry points

| Stream | Status | How it reshapes on `velnor-gen` | Rewritten first item |
|---|---|---|---|
| **B (Phase 0)** | **REWRITE (shrinks ~40%)** | Engine releases `B02/B13/B14` and `B03/B08/B10/B11` are gone/done. Replaced by green `main`, `ci.yml` & `.velnor/config.toml` deletion, strict `pr_ci` label mode, `#1608`, `check-generated`, ruleset `24396608`, queue revival, and clean `velnor-ci bench` baseline. | **W01 / W02** (Owner pack + green `main` + delete `ci.yml` & `.velnor/config.toml`) |
| **RV (Reviews)** | **KEEP + EXPAND** | Add `#1324` vs ARCH journal reconciliation (`N12`), naming ADR (`N16`), and deploy-repo home (`N11`). Drop deleted engine paths from `RV03`. | **W07** (`RV01–RV05` + `N11/N12/N16` at tip `0eb2d76a5c`) |
| **G (Workflows & Inventory)** | **PROMOTE TO CORE** | `G03` is no longer a hand-coded list of 39 engine jobs—it is a compiler pass in `velnor-rust` emitting obligations from `RepoFacts`/`WorkflowPlan`. `G01` cohorts shrink to 4 real job types (`rust-plan`/`lint`/`policy`/`Required`, `rust-lint`/`rust-test`, `velnor-runner-host`/`velnor-local` Docker group, `release` + `qualification`). | **W08** (`velnor-gen`-emitted obligation inventory + `DG01` canonical key struct) |
| **P (Platform)** | **KEEP (after `Q-N1`)** | Unified `crates/velnor-platform`, `-platform-docker`, `-platform-k8s`. Reconciled with `#1324` (`Observation<T>`, Pod-UID proofs, K8s hardening) minus Turso journal & fixed pair APIs. `P07` gated on `goals.md:75`. | **W09** (`P01 + P02 + P03` on Docker for day-1 conformance) |
| **O (GitHub Official)** | **KEEP** | Wraps official runner (bump `.velnor/runner.lock:19` from `2.338.0` → latest `2.338.x`) behind `crates/velnor-executor`; runs on Docker first for immediate conformance oracle. | **W09** (`O01` on Docker alongside `P03`) |
| **X (Velnor Native)** | **GATE ON `Q-N1`, DROP `X13` & `executor/`** | Lives in `crates/velnor-{executor,exec,dispatch}`. Drop `X13` (0 `background: true` uses). Reuse `velnor-runner-github` scale-set client instead of re-porting `scaleset/client.rs` from `velnor-legacy`; port only broker, run-service, Results, and expressions (`D4`). | `X01'` (in-workspace crate skeletons + provenance lint) after `Q-N1` |
| **A (Native Actions)** | **KEEP (SHRINKS)** | `A09` done; `A07` deferred (no consumer); `A06` gated on `Q-N5`; `A08` shrinks to 3 `github-script` uses; `A01–A05` align directly with `velnor-render`'s `ALLOWED_ACTIONS`. | `A08` (replace 3 `github-script` uses with `velnor-ci`) + `A01–A04` |
| **H (Conformance)** | **KEEP (RE-SEED)** | Seed corpus from `velnor-gen`'s `qualification.yml` (64 jobs, `#1747`), `local/conformance/*.json`, and `local/scenarios/velnor-new.json` (`#1718`). | `H01` (lab on Docker with `O01` + `P03`) |
| **L (App & Gate)** | **KEEP (SIMPLIFIES)** | Ruleset has **1** context (`Required`), not 2, so Step 3/4 transitions are cleaner. Shadow parity compares against `velnor-gen`'s `ObligationInventory`, not 39 engine jobs. | **W10** (`L01` Lab A + `L02` shadow `velnor-control`) |
| **V (Ledger & Audits)** | **KEEP (ANCHOR IN `velnor-model`)** | `V01` builds on `velnor-model` keys; `V08`/`V13` collapse directly to per-package keys (1 root workspace, `[[workspace_groups]]` aware); `V04` gated on `Q-N8`. | **W08** (`DG01/V12/V14`) → `V01` (`crates/velnor-ledger`) |
| **AN (Agent-Native)** | **KEEP (ORDER AFTER `G03`/MBX)** | `AN01` (`velnor cargo` replacing `mbx`) ordered cleanly after generator stops emitting `mbx-store`/`mbx-seed` for `velnor-new`; `velnor check <name>` reads `velnor-gen` step/obligation names. | `AN01` count-only daemon after `W08` |
| **C (Generations & Mirrors)** | **KEEP (BUILD ON EXISTING)** | Extend existing `tools-local` (`C01`), `crates-mirror` (`C02`), `inputs.tsv` + `delta` + `dep_info.rs` (`C05`), and measure `mbx_shared_store` baseline (`C08`). | `C03` (node git mirror) + `C04` (XFS volumes) after `D8'` |
| **ST (Storage & Cache)** | **GATE ON `Q-N1`, REUSE `velnor-cache-rules`** | `ST03` imports `crates/velnor-cache-rules` directly as its executable spec; `ST05` unifies Official cache traffic; all routes stay `local` until `ST06` passes on FI workers. | `ST01` (`crates/velnor-storage`) after `Q-N1` |
| **T & S (Floor & Cluster)** | **SHRINK (`T04/T05/T06` DONE/DROP)** | `T03` moves to Phase 0 (**W06**); `T07` narrows to `velnor-ci` only; `T08` injects `now_seconds()` (`io/mod.rs:289`, `io/budget.rs:241`); `S01–S03` unchanged (awaiting `D8'` & `N11`). | **W06** (`T03` now) + `S01/S03` after `D8'` |
| **R (Release)** | **REWRITE + GATE ON `Q-N5`** | Product release & source preview deleted; only `tools-v` (and `runner-v` if kept) remain. Land `#1608` first, unify `release-gate` (`R02`), then settle hosted-attested vs self-hosted (`Q-N5`) before `R01`/`R04`. | **W03** (`#1608`) → **W10** (`R02`) |
| **F (Final Deletion)** | **GATE ON `Q-N1` & `Q-N9`** | Must also update `release-plz.toml:45-106` (`runner-v` group), tag rulesets, `.velnor/rust-ci.toml:26-32` (`local-mode` lane), and `docs/goals.md:20-22` if `velnor-runner-host` is deleted. | Last item, after cohort 4 + `P07` |
| **BM & DG (Benchmarks & Dagger)** | **KEEP (RE-BASELINE ON `velnor-ci bench`)** | Build `BM03`/`BM08` directly on `velnor-ci bench`/`timings`/`ci-metrics.json` (`N10`), re-baselined on green `velnor-gen` (`N09`). `DG01/DG04/DG07/DG08` fold straight into `velnor-gen` (**W04/W08**). | **W06** (`N09/N10`) + **W08** (`DG01/DG04`) |

---

## 6. Phase 0 and the first ten work items, rewritten

### 6.1 Recent merges and disposition of the 5 remaining open PRs

- **Already merged on tip `0eb2d76a5c` (no longer open):** `#1263` (`a619478fb2`, unlinked `daemon.lock`), `#1274` (`61067c17fc`, `[[workspace_groups]]`), `#1335` (`47ef96f8ea`, `Test lane guard`), `#1747` (`57b65af0db`, deleted old engine & moved `qualification.yml` to `velnor-gen`), `#1748` (`0eb2d76a5c`, overlapped nextest listing, closes `#1161`).
- **Remaining 5 open PRs from Report D§1 — exact disposition:**

| Open PR | Scope | State vs `0eb2d76a5c` | Disposition & slot |
|---|---|---|---|
| **#1608** | Release gate requires `Required` from push run on default branch (`#1555`) | +35/-13, 4 files, clean locally (minor path check if golden test moved) | **Rebase & merge immediately in W03** (closes P0-3 `goals.md:77` violation) |
| **#1200** | Shared mise platform matching, fail-closed on ambiguous locks (`#1104`, 13 tests) | Already on `crates/` layout, minor rebase only | **Rebase & merge in W03** (fixes real `prepare` platform mismatches on green `main`) |
| **#1320** | Per-root `inputs audit --root DIR`, eliminates double workspace build (`#1073`) | 9 conflicts against deleted `generator/`/`ci-helper/` paths | **Rewrite onto `crates/` after W06** (drop pre-0.1.81 compat branches already deleted by `#1713`; speeds up scheduled/expired-base audits) |
| **#1026** | `inputs save/restore --target-dir` & sibling package snapshots (`#1018`, `#1011`) | +5.8k lines, 96 commits, 44 conflicts, open review follow-ups `#1730` | **Split & rewrite in 2 small PRs after W06** (PR 1: `velnor-ci` input scope; PR 2: `velnor-rust`/`-render` custom target-dir delta support, fixing `#1730` inline) |
| **#1297** | Renovate `cargo-minorpatch` across deleted per-box lockfiles | 11 conflicts (`ci-helper/Cargo.lock` deleted) | **Close as superseded in W03**; let Renovate open a fresh PR against the single root `Cargo.lock` |

### 6.2 Phase 0 / Phase 1 dependency graph & critical path

```text
  W01 (Owner Pack & docs/goals.md) ─────────────────────────────────┬───────────────────┐
          │ (Q-N2, Q-N3, Q-N4)                                      │ (Q-N1)            │ (D7')
          ▼                                                         ▼                   ▼
  W02 (Green main + del ci.yml + del config.toml + fix skip) ──► W07 (Reviews/ADRs)  W10 (R02 + Lab A +
          │                                                         │                    shadow control)
          ├──► W03 (Merge #1608 & #1200, close #1297)               ├──► W08 (Obligation    ▲
          │                                                         │     Inventory/Keys)───┘
          └──► W04 (Strict label + check-generated + 0.1.96)        │
                      │                                             └──► W09 (Platform trait +
                      ▼                                                   Docker + Official)
               W05 (Ruleset 24396608 + ALLGREEN queue + canaries)
                      │
                      ▼
               W06 (Un-serialize velnor-ci + timings + baseline)
```

Total size for `W01–W10`: **117.5 agent-h build + 32.5 agent-h review = 150 agent-h** (Phase 0 `W01–W06`: 43.5 agent-h build + 13.0 agent-h review). With 2–3 parallel agents, Phase 0 finishes in ~2 working days (wall clock bounded only by owner approval on `W01` and `W05` ruleset PUT).

### 6.3 The first ten work items, rewritten (`W01–W10`)

Model notation follows `docs/roadmap.md` executor classes (`opus` = strong judgement/architecture, `sonnet` = mid implementation/plumbing, `haiku` = mechanical); review is always an independent session.

1. **`W01` — Owner Decision Pack v2 & `docs/goals.md` Amendment PR** *(replaces B01, N08)*
   - **Goal:** Present the rewritten decision sheet (Section 7: `Q-N1..Q-N12` + remaining open `D`-questions) in one sitting, capture binding answers, and land the owner-approved amendments to `docs/goals.md` and `docs/proposed/macos-scaleset-runner.md`.
   - **Files:** `docs/goals.md`, `docs/proposed/macos-scaleset-runner.md`, `docs/research/velnor-runner-2026-11/{owner-decisions,owner-questions}.md`.
   - **Migrates / deletes:** Replaces obsolete engine-era questions (`D1`, `D9`, `D14`) and removes stale `generator/`/`ci-helper/`/`velnor-actions` wording from `docs/goals.md:11-12` and `macos-scaleset-runner.md:308-309`.
   - **Done-when:** Owner approves or modifies each item in writing; amendment PR merges with explicit approval comment citing every changed line of `docs/goals.md`; `Q-N1` gate cleared (or scoped) for `W07–W10`.
   - **Size:** 2.0 agent-h build + 1.0 agent-h review. | **Impl:** Opus (`high`). | **Review:** Opus (`medium`) + **Owner explicit sign-off**.

2. **`W02` — Restore Green `main`, Migrate `ci.yml` into `[[lint]]`, Delete `.velnor/config.toml` & Dead Helper Ports, Fix `merge_group_skip` Bug** *(replaces N01, N14, N17, N25, N26, B12-rem)*
   - **Goal:** Clear every clippy/alint/test failure on `main` (`#1555`); pin `"github:asamarts/alint" = "0.22.1"` in `mise.toml`; add `[[lint]]` (`jactionlint`, `alint` + `probe = "scripts/alint-line-cap-probe.sh"`, `mise-task` `lint-generator-samples`) to `.velnor/rust-ci.toml`; remove `build.jobs=2 --jobs 2` at `mise.toml:93`; fix `merge_group_skip.rs:54-56` never to set `skip_after_merge_group` on `When::NotCancelledPostMerge` steps.
   - **Files:** `.velnor/rust-ci.toml`, `mise.toml`, `mise.lock`, `.github/workflows/{rust-ci.yml,ci.yml(del)}`, `.velnor/config.toml(del)`, `crates/velnor-rust/src/{merge_group_skip.rs,plan.rs}`, `crates/velnor-gen/src/{drifts.rs,io/mod.rs,io/workflows.rs}`, `crates/velnor-ci/src/{cli.rs,io/plan.rs,plan_request/**(del)}`, `generator/samples/**`.
   - **Migrates / deletes:** Deletes `.github/workflows/ci.yml`, `.velnor/config.toml`, `engine_configured` in `drifts.rs:75`, and dead engine commands in `velnor-ci/src/cli.rs:82-89,140,152-167,175-185`.
   - **Done-when:** Three consecutive `push` runs of `Rust CI` on `main` conclude `Required = success` (covering `lint-alint`, `lint-jactionlint`, `lint-generator-samples`, `policy`, `rust`) with zero admin bypass; `velnor-gen check` passes with `Drift::Unmarked` active; regression golden test proves `merge_group_skip` + `docs = "post_merge"` keeps `Documentation` and `Check <crate> alone` running on `push`.
   - **Size:** 10.0 agent-h build (2–3 PRs) + 3.0 agent-h review. | **Impl:** Sonnet (`high`). | **Review:** Opus (`high`).

3. **`W03` — Land Open PRs `#1608` (Release Gate) & `#1200` (Mise Pins); Close `#1297`** *(replaces N06, D§1 triage)*
   - **Goal:** Rebase and merge `#1608` so `release-rust-ci-tools.yml` enforces push `Required` (`goals.md:77`); rebase and merge `#1200` (fail-closed mise platform matching, closes `#1104`); close superseded Renovate `#1297` and log post-`W06` split-rewrite issues for `#1320` and `#1026`.
   - **Files:** `.github/workflows/release-rust-ci-tools.yml`, `crates/velnor-gen/tests/it/release_workflow.rs`, `crates/velnor-{ci,gen,model,rust}/src/**` (`#1200` scope).
   - **Done-when:** Both `#1608` and `#1200` merge green; a negative test proves `release-rust-ci-tools.yml` rejects a tag on a default-branch commit whose push `Required` failed or is missing; `#1297` closed.
   - **Size:** 4.0 agent-h build + 1.5 agent-h review. | **Impl:** Sonnet (`medium`). | **Review:** Opus (`medium`).

4. **`W04` — Strict Label Flow (`pr_ci = "label"` Fixed) + Wire `velnor-ci check-generated` (`GeneratorCheck`) + Self-Hosted `rust-cache-trim` + Release `tools-v0.1.96`** *(replaces B06, B07, N02, N04, DG08, N29)*
   - **Goal:** Replace `PrCi::Label` semantics with strict owner-decision-12 behavior: drafts run nothing and fail `Required` (`StepKind::FailOnDraftRun`); unlabelled ready PRs run nothing and fail `Required` (`StepKind::FailOnUnrequestedLabel`: "CI not requested"); adding the configured label (`velnor:verify`, retiring `ci:run`) runs full CI and every push cancels previous runs; `PullRequestType::Unlabeled` cancels in-flight runs and stops future ones; drop non-base `edited` triggers. Expose `velnor-ci check-generated` (calling linked `velnor-gen`), emit `StepKind::GeneratorCheck` before tools install, pass `facts.config.runner` to `delta_cleanup()` (`delta_cleanup.rs:23`), release `tools-v0.1.96`, and pin `.velnor/rust-ci.lock`.
   - **Files:** `crates/velnor-model/src/{plan.rs,steps.rs,repo_config.rs}`, `crates/velnor-rust/src/{plan.rs,plan_budget.rs,required_job.rs,config_pr_ci.rs}`, `crates/velnor-render/src/{events.rs,gate.rs,draft_steps.rs,helper_steps.rs,delta_cleanup.rs,steps.rs}`, `crates/velnor-ci/src/{cli.rs,io/mod.rs}`, `crates/velnor-gen/src/helper_gate.rs`, `crates/velnor-workflows/src/helper_call.rs`, `docs/rust-ci.md`, `generator/CONFIG.md`, `.velnor/{rust-ci.toml,rust-ci.lock}`, `.github/workflows/{rust-ci.yml,rust-cache-trim.yml}`.
   - **Migrates / deletes:** Deletes the old unlabelled quick-set bypass, `StepKind::DraftNotice`, one-shot `ci:run` label-removal job (`ci-run-label`), and hosted `ubuntu-latest` in `rust-cache-trim.yml`.
   - **Done-when:** `velnor-lab` proves all 6 label-flow states (draft red, unlabelled ready red, labelled runs full & turns green, push while labelled cancels previous & re-runs, `unlabeled` cancels & stops, stale generated workflow fails `check-generated` in < 10 s); `tools-v0.1.96` released & pinned; 0 hosted runs in `Rust CI` and `Rust cache cleanup`.
   - **Size:** 12.0 agent-h build (2 PRs: code + release/pin) + 3.5 agent-h review. | **Impl:** Opus (`high`). | **Review:** Opus (`high`).

5. **`W05` — Harden Ruleset `24396608`, Revive `ALLGREEN` Merge Queue, Enable Auto-Merge, Deploy Canary PRs** *(replaces B04, B05, N03, H09-lite)*
   - **Goal:** Apply the Section 3.2.5 target JSON to ruleset `24396608` (`Required` bound to `15368`, `bypass_actors: []`, `ALLGREEN`, `max_entries_to_build: 2`, `min_entries_to_merge_wait_minutes: 0`, `check_response_timeout_minutes: 90`), flip `allow_auto_merge = true`, update agent procedures to auto-merge without catch-up `main` merges, and open 3 permanent canary PRs (draft, unlabelled ready, foreign-context status) that must remain `blocked`.
   - **Files:** Ruleset `24396608` (owner applies via `gh api PUT`), repo settings (`allow_auto_merge`), `docs/roadmap.md` (auto-merge & zero-bypass procedure).
   - **Done-when:** Read-back of `GET /repos/tailrocks/velnor-new/rulesets/24396608` matches target JSON field-by-field; first real `merge_group` run of `Rust CI` completes green and lands a PR; all 3 canary PRs report `mergeable_state == "blocked"`.
   - **Size:** 1.5 agent-h build + 0.5 agent-h review (owner applies ruleset). | **Impl:** Sonnet (`low`). | **Review:** Opus (`medium`).

6. **`W06` — `velnor-gen` Speed Quick Wins & Clean Green Baseline** *(replaces B15, N09, N10, N18, N19, T03, N27, N28, N30)*
   - **Goal:** Isolate `crates/velnor-ci/tests/it/**` tempdirs/env to delete `velnor-ci-serial` (`max-threads = 1`) and `crates_mirror` `num-test-threads` lock from `.config/nextest.toml:17,80-83,110-116`; add bounded blobless shallow-fetch fallback in `crates/velnor-ci/src/io/git.rs:66-79`; map `local/**`, `deny.toml`, `.velnor/workflows.toml`, and `release-plz.toml` (`classify.rs` / `.velnor/rust-ci.toml`); record `.velnor/rust-ci-timings.json` via `velnor-ci timings` so `shape = auto` activates `lint-test` (decoupling `policy` from `rust`); enable `[ci] skip_push_after_merge_group = true` and `[budget]`; fix `Cache` waste failures; clean duplicate `generator/CONFIG.md:190-215` & stale `generator/samples/velnor-new`; collect & commit clean green baseline metrics (including PR delta timing, `N19`).
   - **Files:** `.config/nextest.toml`, `crates/velnor-ci/tests/it/**`, `crates/velnor-ci/src/io/git.rs`, `crates/velnor-rust/src/classify.rs`, `.velnor/{rust-ci.toml,rust-ci-timings.json}`, `.github/workflows/rust-ci.yml`, `generator/CONFIG.md`, `generator/samples/velnor-new/**`.
   - **Done-when:** `velnor-ci` test suite finishes in ≤ 15 s parallel; `policy` starts concurrently with `rust-lint`/`rust-test`; `Cache` green on ≥ 10 consecutive runs; clean baseline report committed from ≥ 20 green queue-merged PRs (`velnor-ci bench collect` + `an_prs.py`) with PR verdict, push-to-merge, and delta-layer delta `[M]`.
   - **Size:** 14.0 agent-h build (3 PRs) + 3.5 agent-h review. | **Impl:** Sonnet (`high`). | **Review:** Opus (`medium`).

7. **`W07` — Architecture Reconciliation, Component Reviews (`RV01–RV05`) & Naming/Deploy ADRs** *(replaces RV01–RV05, N11, N12, N16, N21)*
   - **Goal:** Review `velnor-runner-{core,github,host}`, `velnor-local`, and `velnor-ci`/`-gen` at `0eb2d76a5c` under decision 17; reconcile `#1324` (`docs/proposed/kubernetes-runtime.md`) vs ARCH §5.10 into one capability-driven `Platform` spec; publish ADRs for naming (`N16`), unified `crates/velnor-*` layout (dropping `executor/`), and cluster-manifest home (`N11`: `velnor-runners` vs `deploy/`); move dated panel narratives out of `docs/proposed/*` (`Q-N10`) and fix dead links (`N21`).
   - **Files:** `docs/proposed/{velnor-runner-architecture.md,kubernetes-runtime.md,velnor-user-experience.md}`, new ADRs under `docs/proposed/`.
   - **Done-when:** Per-component port/refactor/redesign/delete verdicts approved; zero unresolved conflicts between `#1324` and ARCH; exact crate map under `crates/velnor-*` frozen.
   - **Size:** 14.0 agent-h (parallelizable across 3 reviewers) + 4.0 agent-h review. | **Impl:** Opus (`high`). | **Review:** Opus (`high`).

8. **`W08` — Generator-Emitted Obligation Inventory v1, Canonical Key Struct & C5/`.git` Census** *(replaces G03, DG01, DG04, V12, V14, T08)*
   - **Goal:** Add `Obligation`, `ObligationId`, `ObligationKey`, and `ReuseClass` to `crates/velnor-model`; implement `obligations()` in `crates/velnor-rust` deriving the versioned obligation inventory directly from `RepoFacts` + `WorkflowPlan` + `rust-inputs.lock`; implement C5 classifier + injectable clock replacing `now_seconds()` (`crates/velnor-ci/src/io/mod.rs:289`, `io/budget.rs:241`); complete no-`.git` census over all 18 crates.
   - **Files:** `crates/velnor-model/src/{verdict.rs,obligations.rs(new)}`, `crates/velnor-rust/src/{obligations.rs(new),plan.rs}`, `crates/velnor-ci/src/{io/mod.rs,io/budget.rs,time.rs}`, `crates/velnor-gen/tests/it/**`.
   - **Done-when:** Every blocking check of `rust-ci.yml` maps 1:1 to a typed obligation with canonical key and `ReuseClass`; dropping any test or obligation fails golden tests; C5 & `.git` census committed for all 18 crates; `A2`-style clock shift fails a planted date-bomb test.
   - **Size:** 18.0 agent-h build (3 PRs) + 5.0 agent-h review. | **Impl:** Opus (`high`). | **Review:** Opus (`high`).

9. **`W09` — `velnor-platform` Trait, Docker Backend & `velnor-official` Reference Executor** *(replaces P01, P02, P03, O01, O02; gated on W01/`Q-N1` & W07)*
   - **Goal:** Implement `crates/velnor-executor` (`Executor`, `Work`, `Role`, `ExecutorId`), `crates/velnor-platform` (`Platform`, `Capabilities`, capability fallbacks, tri-state `Observation<T>`/`AbsenceProof`, conformance kit), `crates/velnor-platform-docker` (extracting `velnor-runner-host`'s audited `bollard` client, `HOST_NEEDLES`, and `runner-v1.json` seccomp), and `crates/velnor-official` (wrapping official runner bumped in `.velnor/runner.lock:19` to latest `2.338.x`, with `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` and isolated `Reference` namespace).
   - **Files:** Root `Cargo.toml`, `crates/velnor-{executor,platform,platform-docker,official}/**`, `.velnor/runner.lock`.
   - **Done-when:** Platform conformance kit passes on Linux & macOS Docker with capability-off fallback tests; `velnor-official` executes a real `velnor-lab` job on Docker in both `Production` and isolated `Reference` roles (0 writes to production namespace) without double-reporting under lease fault injection.
   - **Size:** 28.0 agent-h build (4 PRs) + 7.0 agent-h review. | **Impl:** Opus (`high`) for traits/`velnor-official`; Sonnet (`high`) for Docker backend. | **Review:** Opus (`high`).

10. **`W10` — Multi-Authority `velnor-ci release-gate` (`R02`) + GitHub App `velnor-verify` Lab A (`L01`) & Shadow `velnor-control` (`L02`)** *(replaces R02, L01, L02; gated on `D7'` & W08)*
    - **Goal:** Upgrade `crates/velnor-ci/src/gate.rs:57-173,292-303` with `--app-check` and `--push-check` (accepting a non-`15368` App context plus push `Required` A1 verification, plus active ruleset & empty-bypass checks); execute Lab A (`velnor-runner-architecture.md` §10.3) against `velnor-lab` with `velnor-verify`; stand up shadow `crates/velnor-control` consuming webhooks + 10 s REST reconciler + `W08`'s obligation inventory and logging parity against live `Rust CI`.
    - **Files:** `crates/velnor-ci/src/{gate.rs,cli_release.rs}`, `crates/velnor-control/**` (new), `crates/velnor-gen/tests/it/**`.
    - **Done-when:** `release-gate` rejects missing/stale App status, wrong integration ID, non-empty `bypass_actors`, and missing push A1 proof; all Lab A cases pass with recorded run IDs; shadow `velnor-control` tracks every `velnor-new` PR head & `merge_group` with webhook P50 ≤ 2 s and 0 false status writes.
    - **Size:** 24.0 agent-h build (3 PRs) + 6.0 agent-h review. | **Impl:** Opus (`high`). | **Review:** Opus (`high`).

---

## 7. Owner questions, rewritten

### 7.1 Complete disposition of existing questions `D1–D26`

| ID | Original question | Status on `0eb2d76a5c` | Recommendation (and default if unanswered) | Blocks |
|---|---|---|---|---|
| **D1** | Release engine `velnor-actions` 0.1.81 (`#1345`) | **OBSOLETE** (`#1345` merged; engine deleted `#1747`) | Drop | — |
| **D2'** *(reworded)* | Flip `allow_auto_merge = true` on `tailrocks/velnor-new`, revive merge queue usage, stop admin direct merges & catch-up `main` merges? | **CHANGED** (`allow_auto_merge = false`; 0 post-cutover queue runs; 157/173 bypass merges) | **Yes** (no default: owner flips repo setting) | **W05**, Lever 2 |
| **D3'** *(reworded)* | Apply ruleset `24396608`: bind `Required` ONLY (drop deleted `Required verdict`) to integration `15368`, `bypass_actors: []`, `ALLGREEN`, depth 2, `min_wait: 0`, timeout 90—**immediately after W02 makes `main` green**? | **CHANGED** (live ruleset has unbound `Required`, admin bypass, `HEADGREEN`, `min_wait: 5`) | **Yes** (no default: owner applies in **W05**) | **W05**, all trustworthy merges |
| **D4** | Expression evaluator: Option A (`velnor-legacy` `Expressions2`, 3 h) vs Option B (`Sdk/Expressions` + `WorkflowParser`, 28 h)? | **VALID** (verify compare-mode on `2.338.x`, since `.velnor/runner.lock:19` is `2.338.0`) | **Recommend B** (complies with decision 3); default A until answered | `X06a`/`X06b` |
| **D5** | Reference executor choice | **ANSWERED** (decision 19: latest official runner) | Keep settled | — |
| **D6** | `ALLGREEN` + 2-witness / shadow graduation policy | **ANSWERED** (decision 20 & ruling Q6) | Keep settled; apply `ALLGREEN` in `D3'` | — |
| **D7'** *(reworded)* | Create `velnor-verify` GitHub App (`statuses:write`, `checks:write`, `pull_requests:read`, `contents:read`, `metadata:read`) + non-admin agent identity; keep `Required` as sole gate until Lab A + shadow parity pass? | **VALID** (clarifies single existing context `Required`) | **Yes, both** (no default: owner creates) | **W10** (`L01`, `L02`), `velnor ship` |
| **D8'** *(reworded)* | Create `velnor-system` deploy identity, approve node changes (XFS work LVs, 3 CPU/6 GiB reservations, 2 GiB eviction, Docker-test taints, FI VG rebuild for RustFS LV), and choose cluster-manifest home (`velnor-runners` vs `deploy/` in `velnor-new`)? | **CHANGED** (`velnor-new` has no `deploy/`; ARC manifests live in `velnor-runners`) | **Yes; keep manifests in `velnor-runners` (or `deploy/` mapped as `non_inputs` so they never trigger Rust CI)** | `P04`, `X15`, `C01–C04`, `ST04`, `S01–S03` |
| **D9** | Pre-approve CI-only engine releases | **OBSOLETE** (engine deleted; tools use `release-plz` `tools-v`) | Drop (use `tools-v` cadence if desired) | — |
| **D10** | Merge queue grouping strategy (`ALLGREEN` vs `HEADGREEN`) | **ANSWERED** (ruling Q10: `ALLGREEN`, still unapplied on live ruleset) | Apply in `D3'` (**W05**) | **W05** |
| **D11'** *(reworded)* | Approve per-job-type cutover pair counts for the new 4-cohort `velnor-gen` inventory (150 gate/lint/policy, 300 `rust-lint`/`rust-test`, 150 Docker group, 30 release/qualification + 3 dry runs)? | **CHANGED** (old 26-crate cohort names deleted) | **Yes, as written** (default: as written) | `G01` cutovers |
| **D12** | Release build & promotion architecture | **ANSWERED** (ruling Q12 / decision 10; re-scoped by `Q-N5` due to `--deny-self-hosted-runners` at `release_attest.rs:70`) | Settle hosted vs opt-in self-hosted in `Q-N5` | `R01`, `R04`, `A06` |
| **D13'** *(re-ask)* | Policy for `crates/velnor-runner-*` (`velnor-host`) until `F01`: freeze new hardening PRs now, or keep developing it as a standalone Mac product (`docs/goals.md:20-22`, `release-plz.toml:45-106`)? | **CHANGED** (old ~35 PRs merged; ~55 hardening commits landed; `local-mode` CI lane added `#1740`) | **Freeze non-critical hardening now** if `F01` deletes `velnor-host`; else remove `velnor-host` from `F01` (`Q-N9`) | **W07**, `T02`, `F01` |
| **D14** | Delete the engine crates | **DONE** (`#1747` deleted all 12 `velnor-actions-*` crates) | Drop; finish deleting `.velnor/config.toml` & dead `velnor-ci` commands in **W02** | — |
| **D15** | Bad-release recovery without admin bypass: redeploy previous image digest + pre-approved 2-node cold break-glass verifier? | **VALID** (urgent once admin bypass is retired in `W05`) | **Yes, option (b)** (default: redeploy only) | Safe `G01` cutover |
| **D16'** *(reworded)* | Folded into **Q-N2**: if `velnor:verify` is attached while a PR is a draft, does marking it ready start verification automatically? | **CHANGED** (part of strict label flow) | **Yes** (default: No, literal decision 12 step 2) | **W04**, **W10** |
| **D17** | Who may approve an obligation/test reduction (or quarantine exception)? | **VALID** | **Owner only, via signed inventory entry** (default: none accepted) | **W08** (`G03`), `V16` |
| **D18'** *(reworded)* | Velnor merge train vs GitHub merge queue (decision 7)? | **CHANGED** (precondition: revive GitHub queue in **W05** before `BM14` can measure it) | **Not now; revive GitHub queue first, measure 50–200 auto-merged PRs (`BM14`), then decide** | `BM14` |
| **D19** | Idle pre-label compile/test of ready PRs? | **VALID** | **No** (keep decision 12; use explicit `velnor verify`) | Nothing |
| **D20** | Rent high-clock worker(s) (Ryzen 9950X / EPYC 4564P) for `BM15`? | **VALID** | **1 node for `BM15` ratio; 2/site only if ≥ 1.15×** (default: none) | `BM15` |
| **D21** | One compile feeding both test witnesses after graduation? | **VALID** | **No** (keep independent witness compiles) | Nothing |
| **D22** | Rent comparator runners (Namespace/Blacksmith/Depot + GH larger) for `BM03`? | **VALID** | **1 vendor + GH larger for 1 week** (default: none, narrower claim) | `BM03`, `BM10` |
| **D23** | Automatic batch verification of unpushed commits in `velnord`? | **VALID** | **Off for now**; revisit after `velnor verify` telemetry | Nothing |
| **D24** | Agent `interactive` class outranking PR & other orgs' CI (30 CPU-min/h cap)? | **VALID** | **Yes, within budget, never above merge/gate/A1** (default: equal to PR) | `S01`, `AN03` |
| **D25** | Agents working inside cluster sandboxes (sticky non-minting shell)? | **VALID** | **2-agent pilot after `velnor verify` ships** (default: no) | `DG06`, `DG09` |
| **D26** | Native Linux `arm64` test lane (beyond hosted `ubuntu-24.04-arm` build)? | **VALID** (`release-rust-ci-tools.yml:121-124` already builds natively on hosted `ubuntu-24.04-arm`, not qemu) | **Not now; advisory lane when arm64 releases matter** | Nothing |

### 7.2 New questions required by the `velnor-gen` ground truth (`Q-N1..Q-N12`)

| ID | Question | Recommendation | Default / blocks |
|---|---|---|---|
| **Q-N1** | Approve the exact `docs/goals.md` amendment in §7.3 (and matching updates to `docs/proposed/macos-scaleset-runner.md` §0, §1, §6, §8) allowing Velnor Native executor, K8s/Docker platform trait, ARC deletion, configurable self-hosted storage/caches, and optional no-fork mode? | **Yes—approve §7.3 wording.** Preserves every hosted guarantee for external `velnor-gen` consumers while legally unblocking Velnor Native. | **No default (hard gate in `goals.md:3`).** Blocks `W09`, `X*`, `ST*`, `P04`, `P07`, `F01`. |
| **Q-N2** | Strict label flow in `velnor-gen`: rename label to `velnor:verify` (or keep `ci:auto`), retire one-shot `ci:run`, stop running quick-set on unlabelled ready PRs, and fail `Required` ("CI not requested") on drafts and unlabelled PRs until App gate ships? | **Yes: `velnor:verify`, retire `ci:run`, zero unlabelled quick-set, fail `Required` on drafts & unlabelled PRs, and arm carried draft labels on ready (`D16' = yes`).** | Blocks **W04**. |
| **Q-N3** | Retire admin bypass (`bypass_actors: []`) and bind `Required` to `15368` immediately after **W02** makes `main` green, accepting zero bypass merges thereafter? | **Yes** (with `D15` break-glass recovery ready before `G01`). | Blocks **W05**. |
| **Q-N4** | Confirm GitHub's merge queue (`ALLGREEN`, `min_wait = 0`) is part of the live flow for `velnor-new`? | **Yes** (matches decision 7 & 12 step 7). | Blocks **W05**, `BM14`, `V10`. |
| **Q-N5** | Release runner policy: build/attest releases on GitHub-hosted runners (keeping `--deny-self-hosted-runners` at `velnor-render/src/release_attest.rs:70`), OR on self-hosted Velnor with promotion by digest (dropping `--deny-self-hosted-runners` and verifying Velnor signer + Fulcio OIDC)? | **Hosted-attested default for public `velnor-gen` consumers; opt-in `release.self_hosted_promote = true` for `velnor-new` once `R04`/`A06` pass dry runs** (honors decision 10 without weakening external repos). | Blocks `R01`, `R04`, `A06`, `G02`. |
| **Q-N6** | Fork PRs on public `velnor-new`: drop fork-PR handling in `velnor-new`'s own config (`fork_prs = "none"`, relying on repo setting "Require approval for all outside collaborators" + restricted runner group, decision 8), while keeping the fork fence in `velnor-gen` for external consumers (`goals.md:43`)? | **Yes—split product capability (keep fork fence in `velnor-gen` for consumers) from `velnor-new`'s repo policy (no fork PRs, decision 8).** | Blocks `W04`, `W07`, `goals.md:43`. |
| **Q-N7** | Parallel check lanes with separate target dirs (ARCH §8.1, citing engine-only HD1) vs `velnor-gen`'s shared-target `lint-test` / `groups` shape (`goals.md:68-69`, HD11)? | **Follow `goals.md` rules 4–5 (`lint-test`/`groups` on GitHub cache now; XFS reflink per-obligation clones on Velnor Native later—giving parallel lanes with ZERO lock contention AND zero copies).** | Blocks **W06**, **W08**. |
| **Q-N8** | Explicitly authorize the content-addressed verification ledger & exact-tree reuse for `velnor-gen`/Velnor Native (confirming HD3's ban on second skip mechanisms applied only to the deleted engine's unsound baseline coverage)? | **Yes**, subject to the 4-part soundness contract (ARCH §1.2, §6.4: 2 witnesses, 3,000 shadow comparisons, 2% sampling, A1 cold audit, epoch revocation). | Blocks `V04`, `V10`, `DG03`. |
| **Q-N9** | Fate of `crates/velnor-runner-*` (`velnor-host`, `runner-v` release group in `release-plz.toml:45-106`, `local-mode` CI lane `#1740`): delete at `F01` (removing `runner-v` from `release-plz.toml`), or keep `velnor-host` as a shipped macOS product? | **Delete at `F01` once `velnor` + `velnor-platform-docker` + `velnor-official` replace every capability on macOS and Linux (decision 18), removing `runner-v` from `release-plz.toml` in the same PR.** | Blocks **W07**, `N07`, `F01`. |
| **Q-N10** | Documentation hygiene (`AGENTS.md` evergreen rule): move dated research narratives/logs out of `docs/proposed/*` into `docs/research/velnor-runner-2026-11/` (tagged historical snapshot) and keep `docs/proposed/*` evergreen? | **Yes** (satisfies `AGENTS.md` without discarding the panel archive). | Blocks **W07** (`N21`). |
| **Q-N11** | Backlog executor vendors (`docs/roadmap.md` Claude-only `exec:haiku/sonnet/opus` vs panel's multi-vendor `backlog-assignments.tsv`)? | **Keep `roadmap.md`'s abstract tiers (`strong`/`mid`/`cheap`, mapped to `exec:opus/sonnet/haiku` on Claude or host-equivalent models) and drop vendor-locked TSV IDs.** | Blocks backlog issue creation. |
| **Q-N12** | Should `velnor-new` generate `.github/workflows/release.yml` via `.velnor/release.toml` (retiring hand-written `release-rust-ci-tools.yml` and closing `#733`)? | **Yes, right after `W03` (`#1608`) and `Q-N5` are settled**, eliminating the last hand-written workflow in `.github/workflows/`! | Blocks `#733`, `R01`, `R04`. |

### 7.3 Exact proposed wording changes for owner approval

#### A. Exact unified diff for `docs/goals.md` (against tip `0eb2d76a5c`)

Line 16 is the core Rust-only / dual-executor decision; lines 11–12, 20–24, 43, 45, 59, 75, and 77 align the rest of `docs/goals.md` so no clause contradicts it while keeping every promise intact for external GitHub-hosted consumers:

```diff
--- a/docs/goals.md
+++ b/docs/goals.md
@@ -7,18 +7,18 @@
 ## What this file governs
 
 | Code | Rule book |
 |---|---|
-| New work: the Rust generator in `generator/`, the CI helper in `ci-helper/`, the runner in `crates/velnor-runner`, the local environment in `local/`, and the workflows the generator writes | This file |
-| Everything else: the existing engine in `crates/` (except `crates/velnor-runner`), its configuration in `.velnor/`, the workflows it generates, and the scripts, images and fixtures that support it. That includes this repository's own CI, Rust included, until the generator takes over the Rust part. The engine is maintained, not developed | [`hard-decisions.md`](hard-decisions.md), with the specifications under `proposed/` and the procedures under `implemented/` |
+| All active crates in the root workspace (`crates/velnor-*`) and the workflows `velnor-gen` writes | This file |
+| Legacy maintenance leftovers (`crates/velnor-archive-guard`, `qualification/`, `scripts/`) until retired | [`hard-decisions.md`](hard-decisions.md), with the specifications under `proposed/` and the procedures under `implemented/` |
 
 The root `AGENTS.md` applies to all code. Hard decisions bind the existing engine only. Where new work needs the same choice, this file states it.
 
-New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. Jobs are always executed by GitHub's official runner: on GitHub's machines, or on the project's own machines inside the containers that `velnor-runner` starts.
+New work serves Rust projects on GitHub, on GitHub-hosted runners or on self-hosted Velnor infrastructure (Kubernetes or Docker behind a capability-declaring platform interface). On GitHub-hosted runners, jobs are executed by GitHub's official runner. On self-hosted Velnor infrastructure, jobs and verification obligations are executed by **Velnor Native** (a Rust executor over a closed, SHA-pinned action set) by default, or by **GitHub Official** (GitHub's latest unmodified official runner driven by Velnor's dispatcher behind the same executor interface) when a repository explicitly configures it. The official runner is permanently retained as Velnor's reference conformance oracle; Velnor never falls back across executors silently, and the executor identity is part of every verification result key.
 
 ## Vision
 
-Velnor has two parts. The generator (`velnor-gen`, with its helper `velnor-ci`) writes the GitHub Actions workflows a project needs; they run on GitHub-hosted runners or on self-hosted runners. The runner (`velnor-runner`) runs jobs on the project's own machines. `velnor-local` runs a checkout's workflows on a developer's machine without pushing, imitating GitHub, and starts each job through the runner.
+Velnor has one unified foundation with two faces. The generator (`velnor-gen`, with its helper `velnor-ci`) compiles a Rust project's structure into both GitHub Actions workflows and a versioned verification contract (obligations, selection rules, and verdict keys). The runner and control plane (`velnor`) execute that contract on the project's own Kubernetes clusters or local Docker hosts—providing warm sandboxes, node-local mirrors, configurable tiered storage, a native `actions/cache` server, and a signed verification ledger—so local agent runs, pull requests, merge groups, default-branch audits, and releases share one sound verification model.
 
-The runner is developed as a product in its own right, because local mode and self-hosted runs depend on it.
+The runner is developed as a product in its own right, serving both cluster CI and local verification through one platform abstraction.
 
 Velnor reads a Rust project and writes the GitHub Actions workflows that check it. It gives the shortest wait for a trustworthy result that GitHub-hosted runners allow, and the project's owner does not have to tune anything by hand. Velnor analyses the project and decides. Where the project makes a choice in its configuration, Velnor follows it.
@@ -40,6 +40,6 @@
 1. **Correct before fast.** A check is skipped only when the change provably cannot affect it. When Velnor cannot tell what a file affects, it runs the checks.
 2. **Green means verified.** The required check passes only when everything that was selected ran and passed. A test step that should have run tests and ran none is not a pass.
 3. **A broken cache is slow, never wrong, and never silent.** A missing, rejected or stale cache takes a correct cold path. A cache that fails at its job is reported by a red check, as design rule 3 defines.
-4. **Secrets never reach the build.** Private dependencies are fetched with a short-lived token that can read only the repositories the project lists. The token is revoked before any code of the project or of its dependencies runs, and cargo then runs offline. Checkout stores no credentials, and the job's `GITHUB_TOKEN` has read-only permissions while that code runs. A pull request from this repository writes a cache only into its own scope. A pull request from a fork gets no token and only reads caches. Where a fork can read the caches, as in a public repository, nothing fetched from a private dependency or built from it is cached.
+4. **Secrets never reach the build.** Private dependencies are fetched with a short-lived token that can read only the repositories the project lists. The token is revoked before any code of the project or of its dependencies runs, and cargo then runs offline. Checkout stores no credentials, and the job's `GITHUB_TOKEN` has read-only permissions while that code runs. A pull request writes caches and candidate verification records only into its own untrusted scope and never promotes them into default-branch trusted caches or receipts without independent verification. For repositories that accept fork pull requests, `velnor-gen` retains the fork fence (hosted runner, no token, read-only cache, nothing from private deps cached); repositories that disallow untrusted fork execution enforce that boundary in GitHub repository settings and runner-group ACLs.
 5. **Every speed claim has a measurement.** A change states its effect with numbers from real runs, before and after.
-6. **Inside GitHub's free limits.** The design fits in GitHub's 10 GB of cache and on standard hosted runners. It does not depend on paid storage or larger machines.
+6. **Inside GitHub's free limits for hosted workflows; configurable storage for self-hosted Velnor.** Generated workflows targeting GitHub-hosted runners fit within GitHub's 10 GB cache and standard hosted machines without paid storage. Self-hosted Velnor deployments may additionally use operator-configured local NVMe and S3-compatible object storage (such as RustFS) behind standard `actions/cache` semantics.
@@ -57,5 +57,5 @@
 | Time from job start to the first cargo command, cache restore included | At most 30 seconds |
 | Number of jobs | Does not grow with the number of crates in a workspace |
-| Cache storage | At most 8 GB of GitHub's 10 GB, pull request layers included |
+| Cache storage (GitHub-hosted workflows) | At most 8 GB of GitHub's 10 GB, pull request layers included |
 | An ordinary pull request, compared with the previous Velnor version | Neither the wait for the required check nor the runner-minutes gets worse. The one exception is a check that correctness needs, with its cost measured and accepted by the owner |
 | The generator's own tests | Under 1 minute |
@@ -73,6 +73,6 @@
 9. **No logic in YAML.** Every generated `run:` step is one command. A generated workflow uses only actions from a fixed list, each pinned to a full commit SHA. Decisions and loops live in one small helper program, written in Rust and shipped prebuilt.
 10. **Configuration matches reality.** Velnor checks its configuration against the repository, such as the name of the default branch. A mismatch fails the run.
-11. **Local first.** A workflow is verified on the developer's machine with GitHub's official runner, started by the Velnor runner, a local stand-in for GitHub and a local cache that enforces GitHub's rules. Real GitHub confirms it afterwards.
+11. **Local first.** Workflows and obligations are verified locally on the developer's machine on the Docker backend—with Velnor Native and, for conformance, GitHub Official—against Velnor's cache-rules implementation before pushing; real GitHub confirms afterwards.
 12. **Proven on real projects.** A change is tried on the small lab project `tailrocks/velnor-lab` first. This repository runs on its own generator, and that is the final proof. The plan and the required check of a run always come from a released version, never from the change under test.
-13. **Projects stay current.** A new Velnor version reaches a project only after it has passed on the lab project, and it is merged only with the project's own checks green. Velnor's own programs are released only from a tag on a commit of the default branch whose required check passed. The job that publishes checks out nothing and runs no code of the repository. It publishes files that a job without write permission built, after checking their digests.
+13. **Projects stay current.** A new Velnor version reaches a project only after it has passed on the lab project, and it is merged only with the project's own checks green. Velnor's own programs are released only from a tag on a commit of the default branch whose required check (and, where enabled, cold audit) passed. The job that publishes checks out nothing and runs no code of the repository. It promotes or publishes verified files by digest after checking checksums and attestations.
 14. **Decided by experiment.** A choice between tools or designs is settled by an experiment on the lab project. The question, the measurements and the rule that picks the winner are written down before the first run. Whoever runs the experiment applies the rule and commits the result with its raw data. A choice that would change this file or a hard decision goes to the owner.
```

#### B. Companion scope note for `docs/proposed/macos-scaleset-runner.md`

Mark lines `:3-10`, `:37-39`, `:308-309`, and `:318-323` explicitly scoped to the legacy `velnor-host` macOS Docker runner (removing the stale `velnor-actions` reference at `:309`) and noting that cluster execution, Velnor Native, and distributed caching are governed by `docs/goals.md` and `docs/proposed/velnor-runner-architecture.md` until `F01` retires `velnor-host` (`Q-N9`).

---

## 8. Be ambitious: what a generator-first foundation makes possible

The original runner design treated `velnor-gen` as an external YAML generator whose output Velnor had to parse back, interpret, and reverse-engineer into an "obligation inventory." With `velnor-gen` (`velnor-model`, `velnor-rust`, `velnor-render`, `velnor-ci`) now owning 100% of `velnor-new`'s workflows and living inside the same 18-crate root workspace, the hardest problems in CI speed and soundness collapse into **compiler problems**:

### 1. One Verification Compiler, Three Execution Backends (Zero Inventory Drift)
Every existing CI system suffers from split-brain: YAML says one thing, local `cargo` commands do another, and a hand-maintained "obligation inventory" tries to reconcile them. Because `velnor-rust::plan_with` (`crates/velnor-rust/src/plan.rs:40`) ALREADY produces a pure, typed `WorkflowPlan` + `VerdictSpec` from `RepoFacts` with zero I/O, make that IR the **single source of truth compiled to three targets**:
- **Target A (`velnor-render` → GitHub Actions YAML):** for GitHub-hosted consumers and the Actions workflows that remain (`release`, `qualification`, scheduled).
- **Target B (`velnor-ledger` / `velnor-node` → Direct Obligation Graph):** `velnor-control` and `velnor-node` execute the compiled `ObligationGraph` directly without ever round-tripping through YAML, GitHub's expression evaluator, or Actions step wrappers on the PR/merge-group hot path. Parity between "what Actions would run" and "what Velnor verifies" becomes a **compile-time invariant** of one Rust function rather than a 300-PR manual reconciliation exercise.
- **Target C (`velnor check` / `velnor verify` / `hk` → Developer & Agent CLI):** an agent invoking `velnor verify` executes the exact same compiled plan graph and `VerdictSpec` as the merge gate—so an agent's pre-push T2 cluster run produces keys guaranteed byte-identical to what the PR label and merge queue will ask for.

### 2. Plan-Time Verdict Key Algebra & Millisecond Speculative Merge-Queue Resolution
Because `velnor-rust::select` (`crates/velnor-rust/src/select.rs:17`) already runs in milliseconds purely from Git tree blobs + `.velnor/rust-metadata.json` + `.velnor/rust-inputs.lock` (never invoking `cargo`), extend it to emit **complete canonical `ObligationKey` vectors at plan time**—before any container starts:
- On every PR push, label, or queue-tip move, `velnor-control` runs `git merge-tree` in memory against the node mirror and evaluates `velnor_rust::plan_keys(base_facts, head_tree)` in **< 50 ms**.
- **Instant zero-container verdicts:** rebase-only pushes, stacked PRs whose base already passed, duplicate PRs, and the 60–75% of merge groups matching the PR's tree resolve directly from the ledger with **zero sandbox claims** (running only C5 fresh obligations).
- **Split build/run obligations keyed by binary digest (`DG03` + `R06`):** Because the generator owns both the compile invocation and the nextest invocation—and `velnor-ci` already records `.velnor/inputs.tsv` blob manifests and `dep_info`—`velnor-gen` can emit explicit `(BuildObligation → ArtifactDigest)` and `(RunObligation(binary_digest, runtime_inputs) → Verdict)` edges. A comment edit, doc change, or refactor that produces a bit-identical test binary skips re-running that test binary entirely (once twin-build determinism graduates), while any change to runtime inputs still invalidates the run key.
- **Zero-trust failure behavior:** Any unrecognized file, missing metadata, or unverifiable digest fails closed to full whole-tree execution—never a silent cache hit.

### 3. Self-Enforcing Capability Contracts (Solving the Hardest Problem: Sound Narrow Keys)
The hardest unsolved problem in the panel's design is narrowing verification keys from whole-tree to per-package *without ever admitting a false green*. The old plan required humans to hand-author and review per-root input contracts (`V08`). `velnor-gen` ALREADY automates half of this via `velnor-ci inputs audit` → `.velnor/rust-inputs.lock` (38 compile-time input edges already tracked) and `velnor-ci lane-guard` (`#1335`). Pair those with the node agent's eBPF/LSM read audit (`V07`):
- `velnor-gen` compiles each package's `.velnor/rust-metadata.json` + `.velnor/rust-inputs.lock` + `[[inputs]]` into a **closed, machine-checkable read capability contract** per obligation.
- Every cluster execution runs on the intact repository tree with the LSM audit enforcing that contract in real time. Any out-of-contract `openat`/`statx`/`getdents64` (including `ENOENT` probes for optional config files!) **immediately kills the witness**, revokes narrowing for that obligation back to whole-tree, and emits the exact `.velnor/rust-inputs.lock` patch.
- **Payoff:** Per-package keys graduate automatically crate-by-crate—no hand-written contract rot, no trace-based false greens, and immediate detection of hidden proc-macro reads or `CARGO_MANIFEST_DIR` parent climbs.

### 4. Breaking Hard Decision 11 (`--workspace` Feature Unification) at the Generator + Node Layer
Hard Decision 11 (`docs/hard-decisions.md:486-513`) forces `--workspace` compilation for clippy, test builds, and docs because `cargo -p <crate>` alters feature unification and busts the Cargo cache. Editing one leaf crate therefore pays workspace-wide clippy/check overhead on cold or semi-warm builds. Because `velnor-rust` already parses the full Cargo resolve graph (`sibling_features.rs`) and `velnor-ci` already has `check-configurations` (`cli.rs:125`), a generator-first runner breaks HD11's tax in two stages:
1. **Stage 1 (stable Cargo today):** On Velnor Native's XFS reflink generations, the node pre-builds one unified workspace dependency generation (`deps` layer) with the exact workspace feature unification set, and spawns parallel per-obligation sandboxes (`fmt`, `clippy`, `doc`, `nextest`, `local-mode`) over zero-copy reflink clones of that generation—delivering ARCH §8.1's parallel lane speed **without** HD1's duplicate target directories or HD11's serial bottlenecks.
2. **Stage 2 (when `-Zfeature-unification` stabilizes, `hard-decisions.md:500-513`):** `velnor-gen` flips a single flag in `velnor-rust` to emit `--config resolver.feature-unification="workspace"` and restore true `-p <selected>` everywhere across clippy, doctests, and test builds—instantly shrinking both cold and warm compile graphs to the exact affected closure.

### 5. Generator-Owned Closed-Loop Performance & Correctness Benchmarks
Today `shape = auto` is blind because `.velnor/rust-ci-timings.json` is absent (`rust-ci.yml:32`), `velnor-gen suggest-groups` is manual, and speed regressions creep in silently. With `velnor-gen`, `velnor-ci bench`/`timings`, and `velnor-ledger` unified:
- **Closed-loop self-tuning:** Signed receipts from `main` feed a continuous timings & flake ledger; `velnor-gen` automatically computes optimal nextest shard splits and `[[test_groups]]` boundaries (longest-first bin-packing respecting Docker and thread constraints) and opens an automated tuning PR whenever observed P90 deviates > 15% from the generated shape.
- **Generator-owned adversarial correctness suite (`velnor-gen bench-correctness`):** Treat `S1` planted failures (older-mtime edit, hidden `build.rs` read, new sibling `.rs` file, clock shift, dropped test, unlabelled PR, forged status, stale generated workflow, `merge_group_skip` × `post_merge` trap) as a first-class compiler test suite run on `velnor-lab` against every PR touching `velnor-rust`, `velnor-ci`, `velnor-ledger`, or `velnor-exec`—encoding "fast and incorrect is wrong" as an un-bypassable automated gate.

### 6. Drop-In Fast CI for Every Rust Repository (Productizing Beyond `velnor-new`)
Because `velnor-gen` already enforces the closed 16-action set, one-command `run:`, deterministic cache keys, and trust boundaries by construction, **every repository that adopts `velnor-gen` automatically becomes a first-class Velnor Native tenant with zero YAML migration**. A team adopts `velnor-gen` on GitHub-hosted runners today (`velnor-gen generate`, staying inside GitHub's 10 GB free limit), flips `runner = "velnor-k8s"` when it brings its own cluster, and immediately inherits XFS warm generations, node mirrors, ledger reuse, `velnor verify`, and digest-promoted releases—turning `velnor-gen` + Velnor Native from a single-repo speedup into the fastest provably-correct CI/CD platform for Rust.
