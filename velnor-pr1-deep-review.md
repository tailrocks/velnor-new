# Velnor Actions PR #1 — deep advisory review

**Reviewed:** September 30, 2026. **Head:** `8ccc60c506a232ff6de524d24afea463fbdb95a4`. **Branch:** `docs/velnor-actions-spec`. **Base:** `main` at `58323453ce6a7eb4841d56f2bf16b897eabc6ced`.

[PR](https://github.com/tailrocks/velnor-new/pull/1) · [Pinned repository](https://github.com/tailrocks/velnor-new/tree/8ccc60c506a232ff6de524d24afea463fbdb95a4) · [Exact-head CI run](https://github.com/tailrocks/velnor-new/actions/runs/36617447350) · [Review proposal](https://github.com/tailrocks/velnor-new/blob/8ccc60c506a232ff6de524d24afea463fbdb95a4/docs/reviews/pr-1.md)

## Conclusion and evidence limits

**Request changes.** The PR is now a substantial implementation: 96 commits, 414 changed files and 72,545 additions, not the documentation-only change described by its title/body. The important blockers are evidence completeness, source-bound identities, end-to-end enforcement of required checks, and the gap between the reviewed job/cache design and generated execution. Strong lint settings already exist, but they cannot compensate for architectural APIs that accept incomplete proof as a successful state.

This is an advisory audit; no repository files, comments, reviews, branch settings or releases were changed. The review used GitHub-connected source reads, review threads, the living review document, current Actions metadata/logs, selected production call paths and tests, official tool documentation, and an isolated local Git reproduction. It is not an every-line certification of all 414 files.

No callable subagent runtime or coding-agent CLI was available after discovery attempts. The work therefore used direct review passes, not actual independent subagents. The container also lacked the Rust/Mise toolchain and could not resolve GitHub for an archive checkout. No Rust suite or local Cargo security audit was run. The findings below distinguish direct source evidence, observed remote execution, local Git reproduction, and additional qualification work.

The audit uses all nine [audit-playbook categories](https://github.com/shadcn/improve/blob/main/skills/improve/references/audit-playbook.md). Findings are ordered by correctness/trust dependencies, not impact divided by effort. No known defect is dismissed on ROI grounds. Fix risk describes what needs regression protection, not a reason to leave the defect unfixed.

## Current CI, not historical green evidence

Run `36617447350` at the reviewed head finished **failure** on September 29, 2026, at 19:33:58 UTC. It had 47 jobs. The failing crate task was Nextest for `velnor-actions-rust`, job `109575245197`; the required aggregate job `109582892261` also failed.

The failed assertion is at `crates/velnor-actions-rust/tests/impl_rust_f2b.rs:104`, in `alint_config_holds_generic_rules_only`: it requires a particular comment phrase in `.alint.yml`. The log reports 62 tests passed, 1 failed and 46 not run after fail-fast. This is a brittle policy test, not a reason to restore comment wording as the repair. The same test's hardcoded list of rule kinds conflicts with adding the newly requested supported TOML edition rule.

The failed job also provides useful runtime evidence:
- A Mise tool cache restored approximately 80.2 MB, but `mise ls` reported a broken Rust symlink; setup subsequently downloaded six Rust components and reinstalled Rust in about 11.8 seconds.
- Cargo updated the registry index and downloaded dependencies before the MBX object-cache action ran.
- The task's generated command selected MBX 1.19.0 through Mise, while the pinned MBX action reported setting up MBX 1.21.0. Action SHA pinning did not by itself pin that action's downloaded executable.
- Nextest ran its **default** profile. The reviewed requirement is to select a configured CI profile when available.
- The task environment displayed a masked `MISE_GITHUB_TOKEN`; no secret value is reproduced here.

These are observations from this one job, not a claim that every job has byte-identical archives or that a warm benchmark was controlled. The run's approximately 22m39s from creation to completion includes scheduling and must not be presented as pure execution time. Its final check correctly failed for a failed crate report; that does not test the missing-global-validator-evidence defect below, because the global validators in this run passed.

## Findings

Paths below are relative to the pinned repository. Symbol anchors are supplied where exact source line numbers were not independently retained.

### COR-01 — Require global validator evidence in the final gate

**Evidence:** `crates/velnor-actions-orchestrator/src/merge_request.rs::assemble_merge_request` omits global job conclusions. `src/merge.rs::MergeRequest` defaults `required_jobs` to an empty collection. `merge_internal`/`build_final` evaluate supplied jobs, without a plan-declared expected set. The generated final job uses `needs` and `if: always()` but does not transmit all required conclusions.

**Impact:** Passing crate reports can allow the Required check to ignore a failed or absent global validator. This is a concrete incomplete-evidence path; it is not a claim the current failed run actually went green.

**Root cause:** The evidence contract accepts an empty set without proving it is the complete required set. Job scheduling dependencies are mistaken for result validation.

**Fix:** Declare required evidence in the immutable plan, pass actual GitHub job conclusions, and validate exact expected-versus-observed sets before calculating status. Missing evidence must not default to success.

**Verification:** Independently fail Alint, Cargo Deny, Cargo Machete, Actionlint and Zizmor with passing crate reports, including a no-Rust fixture. Required must fail. Cover cancelled/skipped/missing cases, not only explicit failure.

**Confidence:** HIGH, source-confirmed. **Change risk:** HIGH, required-check and schema migration. **Plan:** P01.

### COR-02 — Validate task report files, not only summaries

**Evidence:** `merge_request.rs::read_expected_reports` reads aggregate `matrix-report.json`. `merge.rs::check_matrix_entry` validates the aggregate's reported IDs/statuses rather than independently reading the required task files and all declared output evidence.

**Impact:** A structurally acceptable aggregate can claim evidence that is absent or disagrees with its task files. Strong JSON shape checks are insufficient when the underlying artifacts are not joined.

**Root cause:** Summaries and authoritative proof are interchangeable in the current interface.

**Fix:** Derive the complete evidence manifest from the plan and validate actual task files, digests, run/attempt, outputs and exact counts. Construct a validated aggregate from those results; never trust a producer's counts as the authority.

**Verification:** Missing task JSON, altered digest, contradictory summary, duplicate/unexpected ID, wrong run, missing declared output and extra payload all prevent a passing final result.

**Confidence:** HIGH, source-confirmed. **Change risk:** HIGH, report compatibility. **Plan:** P01/P04.

### COR-03 — Preserve the full obligation set before affected selection

**Evidence:** `src/internal.rs::plan_internal` calls `select_groups` before `build_plan`. `src/select.rs` returns selected groups, and an empty change set can become an empty list. `src/cover_baseline.rs` runs after those omissions.

**Impact:** Unselected checks disappear from the obligation universe without matching baseline/cache proof. A baseline miss cannot restore obligations that no longer exist. An empty diff can be confused with no validation required.

**Root cause:** 'Probably unaffected' and 'proved covered' are represented by the same destructive filter.

**Fix:** Enumerate every configured obligation first; classify each with explicit evidence; derive execution from that classified universe. Baseline misses execute every otherwise-unproven obligation.

**Verification:** Empty diff without baseline, unrelated-crate changes without proof, missing baseline, wrong-base proof, renames, removed edges and complete exact coverage. All-covered means passed; no-work means zero obligations.

**Confidence:** HIGH, source-confirmed. **Change risk:** HIGH, correctness and performance change together. **Plan:** P02.

### COR-04 — Bind identities to source contents and effective execution

**Evidence:** `src/internal_plan.rs::task_identity_digest` supplies empty inputs/environment/VCS collections. Extension construction leaves important lock/Nextest/archive/rerun identity fields absent. `default_generator` creates a zero digest. `src/cover_identity.rs` compares resulting task identities to justify coverage. `crates/velnor-actions-rust/src/identity.rs` cannot turn absent input evidence into proof of completeness.

**Impact:** The identity layer cannot reliably distinguish a source-modified task from an unchanged one. It also fails to establish the full effective tool/profile/platform boundary and can over-invalidate due to checkout-specific Cargo IDs or unrelated ordering.

**Root cause:** Digest-shaped fields are considered valid even when their content closure is incomplete.

**Fix:** Build one immutable, content-bound execution snapshot; make incomplete inputs explicit and ineligible for omission. Include all consumed configuration/tool/test/dependency inputs; normalize nonsemantic checkout paths and lane ordering. Keep actual executable provenance separate from version labels.

**Verification:** Source/lock/profile/tool/component/input changes invalidate the right task; relocation alone does not; absent closure cannot produce covered/reused; no zero or copied release digest can authenticate a source build.

**Confidence:** HIGH, source-confirmed. **Change risk:** HIGH, invalidates old evidence and cache schemas. **Plan:** P03.

### SEC-01 — Validate baseline provenance against expected platform facts

**Evidence:** `src/cover_baseline.rs` and `cover_identity.rs` do not bind all expected repository/workflow/compatibility fields and actual downloaded-run metadata into one validated provenance object. The download path can fall back to a run-wide artifact download when an exact name is unavailable.

**Impact:** Syntax-valid manifest fields are weaker than proof that the intended protected workflow at the exact base produced the evidence. Downloading unrelated artifacts also violates the declared narrow evidence boundary.

**Root cause:** Authentication, schema validation and semantic identity comparison are not a single enforced admission step.

**Fix:** Admit only an exact repository/source/ref/event/run/attempt/workflow/artifact/digest combination verified against GitHub metadata, then validate task compatibility. An unavailable or invalid baseline broadens execution.

**Verification:** Wrong repository, source, workflow, ref, event, run, attempt, artifact, generator, compatibility and carried-proof cases. Caller-supplied data cannot grant itself trusted status.

**Confidence:** HIGH for missing validation boundaries; no exploitation claimed. **Change risk:** HIGH, security-sensitive admission. **Plan:** P04.

### COR-05 — Replace fabricated cache and archive evidence

**Evidence:** `src/wire_w2.rs::plan_reuse_outcome` can turn Ready availability into reuse without proving restored outputs. `verify_reused_task` supplies `RestoreEvidence::intact()` with empty verification collections. Archive source identity is derived from a manifest path string. The current caller's hardcoded Missing availability keeps part of this path inactive.

**Impact:** Enabling the existing integration API could authorize reuse without the proof required by the contracts. Keeping availability permanently Missing does not implement the promised reuse capability either.

**Root cause:** Eligibility, presence, restore and verified replay are collapsed into one outcome, and tests bless synthetic proof.

**Fix:** Separate states and require real cache input/output checks before constructing verified reuse. Bind archive identity to actual content/configuration. Remove fabricated success paths, not merely the call that currently reaches them.

**Verification:** Key present/payload absent, wrong output, incomplete descriptor, source changed at same pathname, corrupt cache and normal miss. Every unsafe restore executes normally or fails validation; only complete actual evidence is reused.

**Confidence:** HIGH, source-confirmed; partly dormant in current workflow. **Change risk:** HIGH. **Plan:** P04.

### ARCH-01 — Lower obligations into crate jobs instead of equating the two

**Evidence:** `src/internal.rs` emits one matrix entry per task group; `crates/velnor-actions-workflow-renderer/src/task_steps.rs` renders one task execution per matrix job. The current run has 42 task jobs plus five supports. Review section 1 instead requires seven crate jobs with in-job steps and separate global validators.

**Impact:** Clippy and dependent tests are independently scheduled rather than enforced as a crate-local chain. Setup/compiler/report overhead is multiplied, formatting is duplicated, and human job names expose internal metadata.

**Root cause:** There is no separate job-grouping layer between the fine-grained task graph and GitHub execution.

**Fix:** Add a crate-job representation while retaining complete per-task proof. Render ordered steps, independent crate concurrency, split validators and names requested by the review. Migrate to ci.yml and coordinate Required-check references.

**Verification:** Default graph has exactly seven Rust crate jobs; each obligation occurs once; Clippy failure blocks its own dependents; siblings continue; independent validator failures reach Required; no per-task fan-out or branded names remains.

**Confidence:** HIGH, source and remote execution. **Change risk:** HIGH, workflow/state migration. **Plan:** P05.

### DX-01 — Derive the public plan from the finalized emitted workflow

**Evidence:** `src/plan.rs` produces display facts separately from the finalized rendered tree, labels task-group counts as a Rust crate matrix, uses fixed chain descriptions, reports Rust selected for empty inventory, and advertises cache/pin information not fully derived from emitted steps.

**Impact:** A user cannot rely on plan to describe generate accurately. This is especially damaging for an agent using the plan as a verification oracle.

**Root cause:** Human output is a second partial description of the product rather than a view over the final IR.

**Fix:** Render the human report from the same finalized analysis/IR manifest used for output. Preserve no-write behavior and distinguish template plans from event-time cache/baseline outcomes.

**Verification:** Compare independently parsed YAML facts to plan across no-Rust, ignored-Rust, Cargo/MBX/Nextest, custom names, actions, caches and configuration variants.

**Confidence:** HIGH, source-confirmed. **Change risk:** MEDIUM, user-output/golden changes. **Plan:** P05.

### ARCH-02 — Implement structured detection and reconcile tool authority

**Evidence:** `crates/velnor-actions-rust/src/evidence.rs` explicitly excludes `.config/nextest.toml` from usage evidence. The review now requires that file and its CI profile to select Nextest. Current root `mise.toml` uses a Cargo wrapper; standard structured wrapper detection is not equivalent to finding executable words in line-based evidence. Existing explicit Velnor overrides should be preserved, not mistaken for automatic detection.

**Impact:** Standard project configuration alone does not reliably yield the newly required command profile. Project Mise behavior also conflicts with older advisory-only/no-config contracts.

**Root cause:** Configuration semantics, executable-text hints and tool/version authority are not cleanly separated.

**Fix:** Mise adapter parses Mise configuration; Rust adapter parses Nextest/Rust configuration; orchestrator resolves typed facts and explicit overrides. Qualify the four independent driver/runner combinations, including MBX+Nextest and the CI profile. Revise consumed-input identity rules when project config becomes executable authority.

**Verification:** Wrapper-only, Nextest-config-only, CI-profile, conflicting evidence, override precedence, no global tools, misleading task names/comments and repeated generation fixtures. No arbitrary discovered task execution.

**Confidence:** HIGH for requirement mismatch. **Change risk:** HIGH, intentional behavior migration. **Plan:** P06/P07.

### SEC-02 — Make subprocess isolation an environment policy, not an overlay

**Evidence:** `crates/velnor-actions-mise/src/command.rs::command` creates Command and overlays variables without clearing inherited environment. `with_env` appends values after isolation settings. Generated task shell steps are a separate execution path. The actual failed-job log exposes masked MISE_GITHUB_TOKEN in that step's environment.

**Impact:** Disabling Mise config/env-file loading does not remove inherited action/download credentials from repository code. Extra environment values can also override nominal isolation fields.

**Root cause:** Tool configuration isolation is mislabeled as complete child-process isolation; trust domains share ambient process state.

**Fix:** Typed environments for download/lookup/task/aggregation, reserved-field validation, explicit child environment construction, and one common task-launch boundary for local/generated execution. Preserve legitimate documented platform configuration; do not indiscriminately classify proxy handling as unsafe.

**Verification:** Nonsecret sentinel credentials in parent/action setup are absent in repository task children while required tool/platform values survive; reserved overrides fail; actual generated workflow path is tested, not only local Command construction.

**Confidence:** HIGH, source and runtime environment evidence. **Change risk:** HIGH, credential and tool compatibility. **Plan:** P07.

### PERF-01 — Correct cache paths, ordering and ownership before tuning concurrency

**Evidence:** Generated workflow/task_steps: role-specific caches of Mise's installation directory, Mise action cache/install/env disabled, temporary Rust/Cargo homes, unconditional Cargo fetch before MBX restore, object mode without a source-cache step. Runtime log confirms a cache hit followed by Rust reinstallation and source downloads.

**Impact:** A cache-hit indicator does not imply usable tool/source reuse. Repeated job setup/downloads and duplicated archives dominate work that should be shared or restored correctly.

**Root cause:** Cache identity and payload ownership are not derived from the actual filesystem/tool closure used by execution.

**Fix:** One owner per layer, actual-home source cache, symlink-target-complete Rust toolchain restore, cache-before-consumer ordering, one race-safe shared-source writer, minimal tools per job. Measure target versus objects payloads across seven jobs; do not stack overlapping cache mechanisms.

**Verification:** Cold and genuinely warm sequential runs, restored components present, complete warm sources run offline, changed semantic inputs invalidate, fork/repository PR trust separation, aggregate bytes and timings with quota headroom.

**Confidence:** HIGH, source plus actual logs. **Change risk:** HIGH, cache compatibility/trust. **Plan:** P08.

### DEP-01 — Pin and verify the executable installed by cache setup

**Evidence:** In job 109575245197, generated task tools select mr-boxington@1.19.0, while the full-SHA-pinned MBX action reports setting up 1.21.0.

**Impact:** The action and compilation may operate with different executable/cache-format assumptions; an exact action revision is not an exact downloaded-tool identity.

**Root cause:** Two installation authorities are treated as though a shared version catalog controls both.

**Fix:** Use supported exact-version/preinstalled-tool action behavior or a qualified action update, then verify effective executable and format identities for setup, compilation and export. Do not fabricate unsupported action inputs.

**Verification:** Log actual tool identities at all stages and fail mismatch before accepting restored/exported evidence. Exercise cold and cached tool setup.

**Confidence:** HIGH for observed identity disagreement; downstream format corruption was not demonstrated. **Change risk:** MEDIUM/HIGH. **Plan:** P07/P08.

### COR-06 — Make output replacement truthful and preview validation non-mutating

**Evidence:** `src/generate.rs::prepare_preview_dir` can create a destination before rejecting repository containment. `swap_directories` renames old output aside then staged output into place, discards rollback error, and can surface cleanup failure after successful replacement.

**Impact:** Rejected preview can leave repository directories, the replacement is not an atomic exchange, and caller-visible failure can disagree with committed filesystem state.

**Root cause:** Validation, reservation, commit and cleanup are not separate typed transaction phases.

**Fix:** Validate before any writes; use supported atomic commit semantics or reject unsupported capability before mutation; preserve rollback errors and classify postcommit cleanup accurately. Test concurrent ownership and actual platforms.

**Verification:** Nested internal preview, symlinks/ancestors, concurrent generation, failed validation, failures at every rename/cleanup point, old-tree preservation, no transient missing destination where atomicity is promised.

**Confidence:** HIGH, source-confirmed; platform transaction changes need actual execution tests. **Change risk:** HIGH, destructive output operation. **Plan:** P09.

### COR-07 — Resolve Git worktree configuration and filenames through Git

**Evidence:** `src/prepare.rs::origin_matches` manually reads `<absolute-git-dir>/config`; linked worktrees use a shared configuration outside that directory. `src/select.rs` parses line-oriented `git diff --name-only` output.

**Local reproduction:** With Git 2.47.3 in an isolated scratch repository, linked-worktree `git-dir/config` was absent while `git config --get remote.origin.url` returned the configured origin correctly. A Unicode path was C-quoted with ordinary --name-only and preserved with --name-only -z.

**Impact:** Velnor-only policy can reject a legitimate linked worktree. Path parsing can misclassify names; its precise selection consequence must be tested, rather than assuming every encoding case produces a false skip.

**Root cause:** Custom parsing of Git storage/display formats instead of supported semantic machine interfaces.

**Fix:** Typed Git config query, repository normalization, NUL-delimited filename handling and explicit encoding rules.

**Verification:** Main checkout/worktree/includes, Unicode/whitespace/newline filenames, rename/delete and nested invocation through actual product APIs.

**Confidence:** HIGH, source plus Git reproduction. **Change risk:** MEDIUM. **Plan:** P10.

### TEST-01 — Replace synthetic policy/proof tests with behavioral boundary tests

**Evidence:** `crates/velnor-actions-cli/tests/impl_repo_policy.rs` uses textual heuristics for manifest, test, source and policy claims. `wire_w2.rs` tests accept Ready-to-reuse and synthetic intact evidence. The current red test checks comment wording. Existing mutation configuration is manual-only and does not prove production workflow wiring.

**Impact:** Large test counts and green isolated helpers can coexist with missing production invariants. Text checks can both reject harmless edits and accept semantically broken structures.

**Root cause:** Tests certify representations/helper assumptions, not independently observed behavior at the product boundary.

**Fix:** Retain useful shape snapshots but add semantic TOML/Cargo/compiler checks, registered-test inventory, actual pipeline fixtures and mutation/property tests of proof rejection. Keep test-only proof factories out of production APIs. Do not build an unnecessary general Rust AST linter.

**Verification:** The production defects above fail before their fixes and pass afterward; comments cannot satisfy policy; removed validators/reports/inputs are caught; live generated negative workflows fail Required.

**Confidence:** HIGH. **Change risk:** MEDIUM/HIGH, replacing misleading guarantees. **Plan:** P00/P11/P12/P13.

### DEP-02 — Distinguish pin consistency from complete upstream freshness

**Evidence:** `scripts/check-freshness.sh` explicitly avoids upstream queries, inspects only a subset of dependency declaration forms, and uses a name-keyed lock representation that cannot preserve every version/source identity. Exception and complete-inventory validation require strengthening. `docs/implemented/update-procedure.md` and older version-policy promises are not fully equivalent to these checks.

**Impact:** An incomplete inventory or local equality check can be described as stronger version-policy enforcement than it actually provides. No vulnerability or outdated-version claim follows merely from this gap.

**Root cause:** Different properties—syntax, manifest/lock consistency, effective executable identity, upstream age and advisories—are combined under one freshness label.

**Fix:** Validate complete Cargo declaration/package identities; separate local/effective/upstream/security evidence; impose explicit hard exception rules and current-source timestamps. Every failed/missing inventory row must affect exit status.

**Verification:** Workspace/build/dev/target/alias/string dependency fixtures; multiple versions/sources; missing actions/tools; stale/unreachable sources; malformed/future/expired/overlong exceptions; failing-row exit behavior.

**Confidence:** HIGH for implementation/contract mismatch. **Change risk:** MEDIUM, update workflows. **Plan:** P12.

### PERF-02 — Remove repeated analysis using an immutable indexed snapshot

**Evidence:** `src/inventory.rs::run_inventories` issues metadata work serially per candidate manifest; per-task identity construction rebuilds/looks up graph information. Multiple member manifests can return the same workspace metadata.

**Impact:** Analysis repeats expensive subprocess/graph work as repository size and task count grow. A speedup factor has not been measured in this audit.

**Root cause:** Shared analysis is recomputed from individual task/manifests instead of reused after validated workspace discovery.

**Fix:** Index validated workspace membership and immutable graph/inputs once, reuse results, and bound concurrency without introducing Cargo lock contention. Preserve nested/independent/malformed project semantics.

**Verification:** Instrument metadata invocations and graph builds across 1/10/100-crate fixtures; compare identical inventory/obligations and actual runtime. Never omit malformed candidates to reduce work.

**Confidence:** HIGH for repeated-work pattern; benefit magnitude unmeasured. **Change risk:** MEDIUM/HIGH, discovery correctness. **Plan:** P13.

### PERF-03 — Complete the performance acceptance evidence

**Evidence:** `docs/implemented/performance.md` explicitly leaves hosted two-minute acceptance unpassed. Its cold case retained a warm registry; four lanes were not tested; a dependency/toolchain case touched Cargo.lock's timestamp rather than changing semantic dependencies. Historical tool execution also differed from one recorded Nextest pin.

**Impact:** These records are useful partial measurements, not proof of all seven acceptance scenarios. The document is candid about several limitations; preserve that distinction.

**Root cause:** Measurement scenarios are not all bound to the exact tool/input/cache/obligation definitions used by acceptance.

**Fix:** Reproducible benchmark harness recording actual complete scenarios, process-tree/resource costs, tool identities, source/cache transfer and Required critical path separately from queue time.

**Verification:** True cold, fresh-checkout warm, unchanged validation, leaf, shared API, two/four lanes, real dependency/toolchain update; same obligation set; raw hosted run evidence. Unmet budgets remain unpassed.

**Confidence:** HIGH, document-supported. **Change risk:** LOW/MEDIUM, instrumentation and benchmark reliability. **Plan:** P13.

### DOC-01 — Remove contradictory implementation and execution instructions

**Evidence:** PR title/body still claim documentation-only. `AGENTS.md` leads with runner-protocol material despite V1 generator scope. `docs/reviews/pr-1.md` proposes project-Mise semantics, detection/naming/job changes that conflict with older contracts. Implemented records and green runs are tied to earlier revisions.

**Impact:** A new agent can faithfully follow the wrong authority, reintroduce old behavior or claim historical proof covers current code.

**Root cause:** Proposal, accepted contract, implementation and qualification status have not been consistently advanced together.

**Fix:** Explicit adoption/disposition record; update all affected contracts and accurate local commands; concise V1 AGENTS entrypoint; exact-SHA implementation evidence; honest source-build/release limitations.

**Verification:** Clean-environment documented commands succeed; no contradictory default job/tool authority remains; old evidence retains its SHA; current failure and unpassed bootstrap/performance gates are not relabeled complete.

**Confidence:** HIGH. **Change risk:** MEDIUM, migration authority. **Plan:** P15 and initial contract phase.

### DIR-01 — Complete the trustworthy V1 path before expanding product scope

**Evidence:** Existing V1 contracts already promise local generation, complete required checks, consumer bootstrap, qualified omission/reuse and dogfooding; deferred docs explicitly separate runners and extra environments.

**Direction, not a newly discovered runtime bug:** Finish the existing promise: config -> complete plan -> safe output -> actual generated execution -> verified evidence -> accurate Required status. That is the repository-grounded next product deliverable.

**Do not silently add:** self-hosted/native runners, remote-cache infrastructure, extra stack adapters, dashboards or general workflow execution. Review alternatives involving remote MBX require a deliberate scope decision, not opportunistic implementation.

**Confidence:** HIGH that this follows stated intent; strategy remains with the maintainer. **Plan:** P15/scope invariants.

## Review feedback reconciliation

The companion goal contains a complete R01–R30 ledger covering the living document's acceptance checklist, plus G01–G03 for the original resolved inline review threads. It requires reading later replies/non-checkbox prose as well.

Important decisions that cannot be handled by blind copying:

| Review requirement | Necessary reconciliation |
|---|---|
| Seven crate jobs, separate validators | Keep fine-grained obligations and proof, but group execution by crate; validators remain independent. |
| ci.yml, unbranded IDs and Required name | Update generator, goldens, baseline workflow identity, docs and protected required-check references together. |
| Standard Mise install/cache/env behavior | Separate project execution from trusted generator/download/lookup operations; remove task credentials and update consumed-input identities. |
| Nextest config and CI profile | Change old exclusion/evidence rules and qualify four independent compile-driver/test-runner combinations. |
| Edition-2024 equality | Literal equality cannot simply be applied to inherited edition without a coherent migration or an explicitly equivalent supported rule. |
| Alint bundle list | Deduplicate the repeated GitHub Actions bundle; verify actual pinned capabilities. |
| Cargo-only cache action | A suggested new action needs an allowlist/metadata/full-SHA qualification change. |
| Same-repo PR cache save | Verify the pinned action actually supports it; do not invent an input or broaden trusted cache scope. |
| Remote MBX cache alternative | It conflicts with V1's remote-service exclusion; select a valid in-scope transport or record a separate approved scope change. |
| Local source-built CLI instructions | Preserve real release-provenance restrictions for consumer generation and provide a tested supported path. |

The original three review threads concerned consumer bootstrap identity, consumer tool pins and subprocess ownership. All were marked resolved at the earlier documentation stage. They remain regression requirements, not evidence that the present implementation is fully accepted. No thread was resolved or modified in this audit.

## Strict Rust: the substantive target

The existing workspace already uses strong language/lint foundations. Retain those. More useful strictness comes from enforced semantic boundaries: complete plans, private validated proof constructors, exhaustive outcomes, exact effect/tool policies, no ambiguous empty defaults and independent negative tests.

Use Clippy's broad standard/pedantic checks with denied warnings as specified, then individually qualify additional restrictions. Do not enable the entire restriction group: [Clippy's own guidance](https://doc.rust-lang.org/clippy/usage.html) warns that its restrictions can conflict and are not intended for blanket enabling. A policy requiring widespread suppressions is not stronger proof.

Rust's [Command documentation](https://doc.rust-lang.org/std/process/struct.Command.html) also matters here: environment overlays do not remove inherited variables. GitHub's [workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax) defines scheduling and conditional execution; it does not replace this project's exact evidence aggregation logic. These are external primary-source confirmations, separate from the repository findings.

## Suggested remediation order

1. Record adopted review changes; reproduce the current red assertion and the false-green proof boundaries.
2. Fix required evidence, full obligation enumeration, content identities, baseline/cache verification and trust separation.
3. Correct environment/tool authority, worktree/output safety and effective executable pinning.
4. Implement seven-crate execution, separate validators, names/filename migration and structured project detection.
5. Correct cache ownership and qualify real warm behavior; optimize shared analysis only with invariant-preserving tests.
6. Complete strict policy/behavioral tests, freshness evidence, controlled hosted performance and accurate docs.

Independent implementation and adversarial reviewers should validate every phase. This audit supplies the evidence and concrete handoff; it does not certify those future changes as already implemented or tested.
