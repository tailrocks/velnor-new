/goal Correct and complete PR #1 in tailrocks/velnor-new using the evidence, decisions, file-specific work packages, and acceptance checklist below. Repair architectural causes, not just the current failing test or visible CI names. Deliver a trustworthy Rust GitHub Actions generator whose required result proves complete validation and whose generated CI follows the reviewed crate-oriented topology.

# 1. Repository, starting evidence, and authorization

Repository: https://github.com/tailrocks/velnor-new
PR: https://github.com/tailrocks/velnor-new/pull/1
Reviewed head: 8ccc60c506a232ff6de524d24afea463fbdb95a4
Reviewed branch: docs/velnor-actions-spec
Reviewed base: main at 58323453ce6a7eb4841d56f2bf16b897eabc6ced
Review date: September 30, 2026.

At the reviewed head the PR is OPEN, has 96 commits and 414 changed files, and is an implementation PR despite its stale documentation-only title/body. Do not restart from the old documentation-only f729a55 snapshot or overwrite subsequent work.

The exact-head Actions run is https://github.com/tailrocks/velnor-new/actions/runs/36617447350 . It finished with FAILURE. Its 47 jobs include a failed Rust-adapter Nextest job, 109575245197, and failed final job, 109582892261. The failing test is `impl_rust_f2b::alint_config_holds_generic_rules_only`, at `crates/velnor-actions-rust/tests/impl_rust_f2b.rs:104`: it requires the comment text `Generic file/path placement` in `.alint.yml`. The log records 62 passed, 1 failed, and 46 not run after fail-fast. This is a starting reproduction, not the full defect list.

The advisory audit used pinned source reads, PR discussions, Actions metadata/logs, official documentation, and an isolated Git reproduction. It did NOT run the Rust test suite locally and had no subagent runtime. Reproduce findings independently before changing code; never represent this advisory audit as completed multi-agent implementation verification.

This document is a handoff to an implementation agent. Receiving this goal authorizes work on this PR, but does not invent missing maintainer approval, authorize changing protected settings without permission, authorize publishing a release, or authorize merging without required reviews. The original review document describes itself as a proposal. Record its adopted decisions explicitly; distinguish an implementation decision within this goal from an approval by a particular human.

Fetch the current PR, all comments/reviews/replies, all review threads including resolved/outdated ones, current base/head, and complete paginated Actions results. Preserve unrelated dirty changes and work by other agents. If the head moved, relocate each cited symbol and re-evaluate the finding against the new head; do not mechanically apply an obsolete patch.

# 2. Mandatory working rules

Use subagents aggressively for all work.

Always delegate work to subagents whenever delegation is possible. Treat subagents as the default execution mechanism, not an optional optimization.

Your execution strategy must:
- decompose the goal into independent or partially independent workstreams;
- spawn subagents for each workstream;
- parallelize all work that can safely run concurrently;
- use additional subagents for research, implementation, review, testing, verification, and cross-checking;
- avoid doing work serially in the parent agent when it can be delegated;
- keep spawning useful subagents as new independent tasks are discovered;
- use independent subagents to verify important conclusions and completed changes;
- coordinate and synthesize subagent results into the final implementation.

Do not merely recommend parallelization—actually execute the goal through subagents.

The parent agent should primarily orchestrate, resolve dependencies/conflicts, integrate results, run final deterministic checks, and ensure the complete goal is finished.

Default rule: delegate first, parallelize aggressively, verify independently, then integrate.

Always commit changes frequently while working. Prefer small, incremental, logically scoped commits instead of keeping a large dirty working tree for a long time and committing everything at the end. As soon as a meaningful unit of work is complete and verified, commit it.

Push progress to the remote repository regularly so work is continuously propagated, recoverable, reviewable, and easy to bisect or revert. Avoid unnecessary branches. Prefer doing as much work as possible on a single working branch. Create another branch only when a concrete technical/workflow requirement makes safe work on the existing branch impractical or impossible. Commit often, push regularly, and minimize branch proliferation.

Never ask the user questions or wait for clarification. Work fully autonomously. Turn ambiguous, uncertain, conflicting, or incomplete matters into independent investigation tasks. Analyze repository context, documentation, history, external primary sources, alternatives, and tradeoffs. Verify assumptions independently; re-verify critical decisions before acting. Make the best supported reversible decision and continue. When uncertainty is significant, use multiple independent reviewers to challenge it.

Your responsibility is to unblock yourself. Do not stop because information is imperfect. Continue until the goal is completed and verified and no meaningful actionable work remains. A genuine unavailable credential, protected human approval, missing platform, or unavailable upstream capability must be demonstrated and reported accurately; it never permits claiming a gate passed. Continue independent work around an external blocker. Do not fabricate delegation, tests, reviews, commits, cache hits, benchmarks, or approvals.

## Decision and bug-fixing principles

Judge every piece of work by whether it SHOULD be done: correctness, consistency, and service to the goal. Never decide by ROI, cost, effort, or whether a known defect is 'worth it'. Do not excuse a known-wrong state as low-value, marginal, an edge case, or a competitor convention. A reference implementation's error does not justify repeating it.

Stop short only on a demonstrated limitation, not an assumption that work is difficult, large, heavy, or expensive. Investigate, try, and measure capability before declaring a blocker. Present choices by correctness and actual capability tradeoffs, such as portability or required trust, rather than impact/effort scores.

Before fixing ANY bug, identify why the architecture allowed it and whether the same design permits a class of related bugs. Prefer eliminating the enabling condition. A guard or special case is insufficient when a correct, feasible structural fix belongs in this change. If a root fix genuinely belongs in a separate change or is demonstrably infeasible, identify that root cause, link the separate work/blocker, and retain explicit unpassed acceptance where applicable. This requires diagnosis first; it does not justify unrelated refactoring.

## Parallel work safety

Use disjoint file ownership for concurrent writers. Serialize changes to Cargo.toml, Cargo.lock, shared schemas, generated output, Git index, commits, and integration. Prefer merging current main over rebasing shared work. Do not force-push or reset another agent's changes. Bound compiler/test processes separately from research concurrency; separate mutable target directories for simultaneous writers. Close/reuse completed subagents rather than creating idle threads indefinitely. Keep scratchpads and agent session artifacts out of commits. If this handoff or its checklist is committed, split it into a concise index and responsibility-specific documents that meet the repository's per-file instruction/document limits; do not add a blanket size exception.

# 3. Sources and precedence

Read in full before implementing:
- `docs/reviews/pr-1.md`, including ALL prose, alternatives, caveats, and every acceptance checkbox—not just the numbered headings.
- `README.md`, `AGENTS.md`, `CLAUDE.md` when present, `CODEOWNERS`, `.alint.yml`, `.velnor/config.toml`, `.velnor/generator.lock`, `.velnor/version-policy.toml`, `mise.toml`, `.mise-version`, `.config/nextest.toml`.
- `docs/proposed/README.md` and every linked architecture, CLI, workflow, generated-file, task-execution, cache/report, parallelism/selection, Rust-quality, agent/performance, tooling-input, bootstrap/release, version-policy, and implementation-plan contract.
- `docs/implemented/README.md`, `deviations.md`, `gate-8-dogfooding.md`, `performance.md`, `update-procedure.md`, and the relevant capability records.
- Actual Cargo manifests, generated workflows, helper entrypoints, production call paths, and tests; documents do not prove that wiring works.
- Audit playbook: https://github.com/shadcn/improve/blob/main/skills/improve/references/audit-playbook.md . Apply its nine categories and evidence discipline, but NOT its ROI/effort ranking or 'not worth doing' rubric; the user's correctness-first rules supersede those parts.

Create a concise review-disposition record with stable IDs, source URLs/sections, current implementation evidence, root cause, decision, affected files, regression tests, commit, and final verification. Do not copy secrets or full sensitive environment values into it. Distinguish confirmed bugs, documented-but-undelivered requirements, accepted design changes, and unverified hypotheses.

New review directions conflict with parts of the older specification. Resolve these in a dedicated, independently reviewed contract change BEFORE changing generated behavior:
1. Crate jobs and separate global validators versus one task obligation per job.
2. `ci.yml` and unbranded job IDs/names versus the old `velnor.yml`/`Velnor / Required` contract.
3. Standard project Mise configuration and Nextest configuration detection versus old advisory-only/no-config execution.
4. The requested edition assertion versus `edition.workspace = true` in member manifests.
5. Shared/cache-owner behavior versus role-specific archives; suggested remote MBX infrastructure versus V1 exclusions.
6. Complete task/result evidence versus stubbed integration paths and permissive empty defaults.

Adopt the requested seven-crate topology, clear validator names, standard-config detection, and complete verification. Correct technically inconsistent examples rather than copying them. Preserve security boundaries and explicitly revise superseded rules across code, schema, tests, docs, and generated files. Do not leave two contradictory authorities or claim all review suggestions were already approved.

# 4. Product boundaries and implementation direction

Keep V1 a stack-generic GitHub Actions generator with only the Rust adapter registered. Preserve the seven existing product crates:
`velnor-actions-contract`, `velnor-actions-rust`, `velnor-actions-mise`, `velnor-actions-actionlint`, `velnor-actions-workflow-renderer`, `velnor-actions-orchestrator`, `velnor-actions-cli`.

Preserve the public command surface: `velnor-actions init`, `velnor-actions plan`, `velnor-actions generate [--output-dir PATH]`, plus built-in help/version. Do not add public task/report/internal commands as a shortcut. Use the repository's explicitly documented internal helper protocol for generated workflow operations; fix and qualify that protocol rather than adding a second planner in shell.

Preserve responsibility boundaries. Contract types are stack-neutral/effect-free. Rust owns Cargo metadata interpretation, Rust inputs and selection. Mise owns tool configuration interpretation, resolved command construction and process effects. Actionlint owns its capability/schema/configuration. The renderer serializes generic typed IR, not Rust/Mise policy. Orchestrator composes adapters and evidence; it must not own a second Rust graph implementation or raw process API. CLI remains thin.

Do not add runners, Docker supervision, protocol clients, Kubernetes, remote-cache servers, databases, UI, TypeScript/Bun adapters, or a general workflow engine. A remote MBX service is a possible later direction, not implicit V1 scope. Do not replace working architecture wholesale or create generic core/utils crates. Complete migrations without duplicate legacy paths or silent compatibility shims.

Separate validation OBLIGATIONS from GitHub JOBS. Many independently identified obligations may belong to one crate job. A smaller job graph must not mean fewer checks or less evidence.

# 5. Parallel workstreams and dependency order

Start independent read/reproduction workstreams immediately:
- W0: full review ledger, contract conflicts, exact-head failing test, and documentation consistency.
- W1: complete obligations, immutable plan, required-result evidence, artifact/report validation.
- W2: source identities, baseline trust, generator provenance, cache-result verification.
- W3: crate-oriented workflow graph, naming migration, detection, command profiles, Mise setup.
- W4: tool/source/compiler cache ownership, effective pins, measurements and bounded scheduling.
- W5: Git/worktree handling, output transactions, no-write guarantees, process/environment boundaries.
- W6: Rust policy, behavioral tests, dependency/freshness checks, local verification entrypoint.
- Independent reviewers: correctness/trust reviewer, Rust/architecture reviewer, and workflow/performance reviewer. Reviewers must not simply approve their own implementation.

Integrate in dependency order: contract decisions and reproductions; typed evidence/identity fixes; execution/environment/filesystem safety; crate-oriented rendering and profile resolution; caches/optimizations; actual hosted qualification; documentation and review closure. Research may overlap; enabling an optimization may not precede its correctness proofs.

# 6. P00 — Repair the real red test and the test design that caused it

Evidence: `crates/velnor-actions-rust/tests/impl_rust_f2b.rs::alint_config_holds_generic_rules_only`, especially line 104 at the reviewed SHA; `.alint.yml`; current failed job 109575245197.

- [ ] Reproduce the failing test under the declared pinned Rust/MBX/Nextest toolchain. Preserve the failing output before editing.
- [ ] Remove dependence on incidental comment prose as proof of repository policy. Do NOT restore a comment merely to turn the test green.
- [ ] Replace the rule-kind whitelist that would reject the newly requested supported TOML edition rule. Parse actual configuration and verify supported semantics, not historical wording.
- [ ] Move repository-wide policy assertions out of the Rust adapter's domain tests where they do not belong; use the existing repository verification layer without creating a new general linter.
- [ ] Add positive and negative fixtures for each enabled rule and bundle behavior. A comment-only edit must not fail semantic policy; removing or weakening an enabled rule must fail.
- [ ] Rerun the focused suite with complete failure collection during qualification, then all relevant product tests. Report not-run tests honestly; do not count them as passes.

# 7. P01 — Make the final required result prove every required check

Evidence:
- `crates/velnor-actions-orchestrator/src/merge_request.rs::assemble_merge_request` omits global job conclusions.
- `.../merge.rs::MergeRequest` defaults `required_jobs` to an empty collection.
- `.../merge.rs::merge_internal`, `build_final`, and `check_matrix_entry` evaluate supplied evidence rather than proving the full expected evidence set exists.
- Generated `.github/workflows/velnor.yml` has `needs` plus `if: always()`, but does not pass all required conclusions into the merge request.

Root cause: required obligations and their proofs are not enforced as one closed, plan-declared contract. An empty or partial collection can represent success.

- [ ] Introduce an explicit plan-declared required-evidence inventory: crate obligations, every global validator, planning/generated-file checks, and candidate qualification when enabled.
- [ ] Remove permissive defaults for missing REQUIRED evidence. Parse into untrusted input types, then construct validated types only after exact-set checks. A legitimately empty Rust inventory still has global required checks.
- [ ] Pass actual GitHub `needs` conclusions to the final helper through a validated data channel, including failure/cancelled/skipped states. Do not rely on job dependency scheduling to infer success.
- [ ] Derive required validator IDs from the finalized workflow plan; do not maintain an unrelated hardcoded list in the final helper.
- [ ] Read and validate each required task report file, not just `matrix-report.json`'s summary of those files. Verify schema, run/attempt, task/plan identity, digest, status, exit, output evidence and exact counts. Reject missing files, mismatched summaries, duplicate/unexpected IDs and contradictory statuses.
- [ ] Bind downloaded artifact service IDs and names to the expected run and producer. Validate no traversal/symlink/oversize/extra payload can change the evidence set. Preserve the no-wildcard-download boundary.
- [ ] Keep `failed`, `cancelled`, `blocked`, `not_run`, `planning_failed`, `passed`, and `no_work` distinct. Require the documented precedence and useful diagnostics.
- [ ] Emit a final diagnostic report even when evidence download or parsing fails, to the extent runner cancellation permits. Do not make missing evidence a success to ensure a report exists.
- [ ] Separate PR candidate QUALIFICATION from release PROMOTION. Fix `check_candidate_proof` so a required untrusted PR candidate can be safely qualified without being eligible for promotion; the final gate must not reject qualification merely because the event is a PR, nor grant it trusted release status.

Verification:
- [ ] For each validator independently: passing crate reports + failed/missing/skipped/cancelled validator => nonzero Required.
- [ ] No-Rust, ignored-Rust, all-covered, one-failed-crate, malformed-plan, missing-task-file, altered aggregate, duplicate artifact, wrong run/attempt, missing candidate, and failed candidate cases have explicit expected results.
- [ ] An actual generated GitHub fixture intentionally fails one global validator while all crate checks pass; Required fails for that validator. Repeat for no-work.
- [ ] The existing exact-head failure correctly propagates; document that this alone does not validate the independent-validator case.

# 8. P02 — Enumerate obligations before selecting executable work

Evidence: `.../orchestrator/src/internal.rs::plan_internal` calls `select_groups` before `build_plan`; `.../select.rs` returns only changed/selected groups; `.../cover_baseline.rs` cannot recover obligations already removed.

Root cause: a changed-work filter is being treated as proof that omitted validation succeeded.

- [ ] Build the full configured obligation universe from the validated inventory first. Attach identities before applying changed-work hints or baseline/cache evidence.
- [ ] Keep every obligation in the plan with exactly one disposition: execute, verified task-result reuse, verified baseline coverage, or explicit failure/blocked state. Construct the execution matrix only after classification.
- [ ] A baseline/cache miss must execute ALL otherwise-unproven obligations, including apparently unaffected crates. Empty diff is not no-work when validation obligations exist.
- [ ] Compare PR merge candidate and exact base; merge-group candidate/base; push before/head. Verify actual checkout and intended candidate match. Unknown or unavailable comparison broadens work.
- [ ] Preserve both base and head dependency information for additions, deletion, renames and removed edges. Unknown paths/dynamic build inputs cannot silently remove work.
- [ ] Keep reverse dependency closure correct across normal, build, dev, optional, target-specific and local path edges. Use package identity plus manifest, not ambiguous display names.
- [ ] Do not treat Markdown as automatically irrelevant: included rustdoc, test fixtures, schemas and build inputs are semantic inputs.

Verification: empty diff/no baseline executes all; leaf edit/no baseline still verifies unproven peers; exact matching baseline covers only matching identities; missing/expired/malformed/wrong-base baseline broadens; deletion/rename/diamond/build-script/unknown-file cases match an independently built expected obligation set. All-covered is passed, not no_work.

# 9. P03 — Replace placeholder identities with complete content-bound snapshots

Evidence: `.../orchestrator/src/internal_plan.rs::task_identity_digest` supplies empty input/environment/VCS collections; extension construction leaves important lock/Nextest/archive/rerun digests absent; `default_generator` supplies a zero digest. `.../rust/src/identity.rs` cannot prove completeness merely from those shapes. `.../cover_identity.rs` matches these identities to justify omission.

Root cause: syntactically valid digest fields are confused with evidence that every semantic input was captured.

- [ ] Create one immutable execution snapshot per analysis with normalized repository identity, canonical package/workspace graph, content digests, resolved tool profile, command arguments, environment contract, outputs, generator identity and execution platform.
- [ ] Include each task's complete first-party input closure: source, manifests, lockfile, applicable Cargo/Nextest config, features/target/profile, local dependencies, build scripts, declared extra files, included docs/fixtures, and declared behavior-affecting environment/VCS data.
- [ ] Unknown effects are an explicit incomplete-input state that forbids result reuse and baseline coverage. Do not equate `None` or an empty list with proven absence of dependencies.
- [ ] Include exact compiler/components, selected compile driver, selected test runner/profile, relevant executable/cache formats, and concrete runner image evidence when semantically relevant.
- [ ] With revised project-Mise execution, tool files actually consumed by execution become semantic inputs. Pure advisory findings remain outside task identity. Update the old exclusions consistently rather than retaining the old assumption after enabling project config.
- [ ] Normalize checkout paths in semantic identity. Keep raw Cargo IDs as diagnostic metadata where necessary, not an accidental absolute-path cache discriminator. Preserve case/Unicode and reject unsupported path encodings explicitly rather than lossy rewriting.
- [ ] Make lane identity stable by responsibility/configuration, not collection ordinal; unrelated task insertion must not invalidate all lanes. Never let concurrent writers share a mutable target directory.
- [ ] Remove zero digests and name/version-only substitutions from trusted generator identity. Verify the actual executable/release manifest. A source build is not the released binary merely because versions match.
- [ ] Centralize canonical serialization and duplicate-key rejection. Schema-changing canonicalization requires an explicit version migration; do not accept old evidence under a new interpretation.

Verification: each semantic input changes the relevant digest; an unchanged checkout at another path retains it; irrelevant changes do not invalidate unrelated tasks after complete dependency proof; source edit cannot be baseline-covered; changing Nextest/compiler/components/image/profile invalidates; missing evidence disables omission; fake generator checksum/version pairs fail.

# 10. P04 — Authenticate baseline evidence and implement real result reuse

Evidence: `.../cover_baseline.rs`, `.../cover_identity.rs`, `.../merge.rs::check_reuse`, and `.../wire_w2.rs::{plan_reuse_outcome,verify_reused_task,check_archive_identity}`. The latter manufactures `RestoreEvidence::intact()` and uses incomplete descriptors; archive source identity is derived from a path string. The current plan's hardcoded Missing availability keeps part of this unqualified reuse path inactive; do not confuse inactivity with correctness.

- [ ] Require a validated baseline provenance object tying together actual repository, protected workflow/ref/event, exact base source commit, successful run/attempt, expected artifact name/service ID, manifest digest, schema, generator and task compatibility.
- [ ] Compare expected values, not just field syntax. A digest-shaped repository ID is not proof it is this repository. Do not accept caller-provided manifest metadata as platform authentication.
- [ ] Download only the exact bounded evidence artifact. Do not download every artifact in a run as a fallback when compatibility/name is unknown.
- [ ] Preserve direct execution provenance when carrying baseline proof forward. PR/fork/merge-group evidence must not become protected default-branch evidence.
- [ ] Separate cache Eligibility, Presence, Restored, and Verified states. Only real validated outputs and exact input identity can construct the reused state.
- [ ] Replace `RestoreEvidence::intact()` production shortcuts and empty descriptor checks with actual restore results, output existence/content verification, trust checks and complete Mise cache inputs. Delete or rewrite tests that currently bless fabricated evidence.
- [ ] Build/archive identities bind actual source/configuration/tool/runtime content, never merely the manifest pathname. Consumers verify the exact archive and do not rebuild it silently.
- [ ] Keep cache misses/unavailability/corruption nonfatal optimizations: discard unsafe evidence, execute normally and report why. Do not make an always-Missing stub the claimed completion of Gate 6.
- [ ] Keep reports fresh per run, separate from cached task outputs. No cache restore can replay a successful report from another run as current execution.
- [ ] Qualify the actual pinned Mise task-cache feature and opaque transport before enabling it. Never introduce a second cache-format implementation or remote cache service.

Verification: wrong repo/ref/event/base/run/attempt/artifact/generator; missing, altered or incomplete output; cache key hit without payload; mutable/untracked external input; zero-byte valid declared output; eligible-but-unavailable result; same path with changed source; carried proof with mismatched identity. Every unsafe case executes or fails validation, never passes by omission.

# 11. P05 — Build the requested human-readable crate job graph

Primary changes: `.../orchestrator/src/internal.rs`, `internal_plan.rs`, and the workflow/job-construction modules reached by those callers; `.../workflow-renderer/src/task_steps.rs`, `render` modules; `.../orchestrator/src/plan.rs`; generated workflow and graph/CLI fixtures.

- [ ] Introduce a crate-job representation containing ordered, individually reported obligations. Do not rename 42 task jobs and claim consolidation.
- [ ] For this repository's current single default configuration emit exactly seven Rust jobs: one for each discovered product crate, named `Rust / <Cargo package name>`. Folder fallback is only for a missing display label; stable IDs must remain collision-safe.
- [ ] Put format, Clippy, selected runner tests, and applicable docs/doctests/build preparation inside the crate job as distinct meaningful steps. Do not emit separate visible jobs for those tasks.
- [ ] Within a crate, Clippy failure blocks dependent tests. Unrelated crate jobs continue and report; matrix fail-fast is false. Additional feature configurations must be represented without silently dropping checks or multiplying default task jobs.
- [ ] Give formatting one owner per intended scope. Remove the Plan job's duplicated first-crate formatting and overlapping whole-workspace formatting unless a separately demonstrated formatting scope requires a distinct obligation.
- [ ] Emit independent validator jobs named Alint, Cargo Deny, Cargo Machete, Actionlint, and Zizmor where required. No Policy or Workflow Lint umbrella. Do not install unrelated Rust compilation caches into these jobs.
- [ ] Keep Plan and Required as clearly named orchestration/status jobs. Remove Velnor prefixes from ALL job IDs and visible names; do not expose task IDs, cache keys, paths or matrix JSON in display names.
- [ ] Change the generated main workflow to `.github/workflows/ci.yml`, display name CI. Remove stale generated `velnor.yml` through the generator. Update path constants, golden fixtures, artifact/baseline workflow references and documentation together.
- [ ] Prepare an explicit required-check migration so branch protection neither waits forever for the old name nor temporarily stops requiring validation. Apply protected-setting changes only through authorized access; otherwise report the exact remaining external step, not a completed migration.
- [ ] Do not copy raw shell into repository configuration. Render commands and statuses from validated typed data; keep task reporting/aggregation logic in Rust rather than hand-built printf JSON.
- [ ] Replace hardcoded zero durations and fabricated cache states with actual measurements or explicitly unavailable telemetry. Do not label absent telemetry as a measured zero.
- [ ] Public plan output must derive from the SAME finalized IR and execution plan that generate writes. Fix empty-repository 'Rust selected', per-task count labeled as crate count, phantom Cargo-cache claims, and incomplete action-pin display.

Verification: parsed generated YAML has seven crate entries/default jobs plus the separately required validators and orchestration; each expected obligation appears exactly once; no per-task job fan-out; no vendor-prefixed IDs/names; ci.yml only; plan/YAML parity; one-crate lint failure blocks only its dependent steps; global failure blocks Required; configuration variants and no-Rust cases remain correct. Repeated generation is byte-identical.

# 12. P06 — Resolve tool behavior from structured project evidence

Primary changes: `.../rust/src/{evidence,profile}.rs`, relevant `.../mise` TOML inspection modules, adapter contracts, orchestrator discovery/preparation, config schema/sample, command emission and profile fixtures.

- [ ] Parse repository Mise TOML in the Mise adapter. Recognize the current repository's `[wrappers]`/inline `wrappers.cargo.command = "mbx"` and `MBX_CARGO_SHIM_MODE` configuration structurally, not by substring coincidence. Return typed evidence to the orchestrator; do not move Mise parsing into Rust.
- [ ] Treat `.config/nextest.toml` as Nextest evidence per the new review direction. Parse whether `[profile.ci]` exists and select it for CI; otherwise use documented Nextest defaults. Correct the current blanket exclusion of this file from evidence.
- [ ] Keep compile driver and test runner as independent dimensions with explicit override precedence. Do not require users to duplicate detected standard configuration in `.velnor/config.toml`.
- [ ] Qualify all four combinations: Cargo+Cargo test, MBX+Cargo test, Cargo+Nextest, MBX+Nextest. The combined case must use the selected MBX route AND Nextest, with the resolved CI profile; doctests remain separate.
- [ ] Absence of repository-local MBX evidence defaults to Cargo. Do not infer machine-global wrappers. An explicit Velnor override remains available and its provenance is reported.
- [ ] Do not auto-run a discovered Mise task just because its name resembles lint/test/build. Any custom task execution requires explicit supported opt-in with declared effects/inputs and safe trust boundaries. Do not turn existing evidence scanning into arbitrary task execution.
- [ ] Ensure removal/replacement of old generated workflows cannot change the next generation's profile. Generated output is not circular discovery evidence; use durable project configuration or explicit overrides.
- [ ] Preserve strict parsing, diagnostics for malformed relevant config and explicit ambiguity handling. Do not silently ignore a conflicting setting.

Verification: real fixture repositories with only the current wrapper, only Nextest config, CI/default profiles, explicit overrides, no evidence, conflicting evidence, misleading comments/README/task names, nested workspace-specific configs, repeated generation, and all four command combinations. Assert actual argv, tool versions and consumed config, not only a display label.

# 13. P07 — Separate trusted generator tooling from project execution and fix isolation

Primary changes: `.../mise/src/command.rs::{IsolatedCommand,with_env,command,run}`, Mise action configuration, tool/bootstrap catalog, workflow tool setup and task-launch construction.

Evidence: the current process wrapper overlays Mise variables on inherited environment; `with_env` can override that overlay. Generated task shell steps inherit the job environment. The failed job log displays masked `MISE_GITHUB_TOKEN` in the task step environment. No secret value should be copied into tests/docs.

- [ ] Define separate typed environment policies for bootstrap/tool download, baseline lookup, project task execution and final evidence validation. Do not reuse a token-bearing environment across these purposes.
- [ ] For repository task children, construct an explicit environment from permitted platform/tool variables and declared project inputs. Remove action/runtime/download credentials including MISE_GITHUB_TOKEN and aliases before repository code executes. Preserve legitimate documented proxy/network configuration where needed without treating platform conventions themselves as vulnerabilities.
- [ ] Prevent arbitrary `with_env` overrides of reserved security/tool-policy fields. Validate allowed overrides at construction; do not rely on append order.
- [ ] Make generated tasks use the same validated environment contract as local helper requests. Fix both launch paths; sanitizing only IsolatedCommand leaves generated shell execution unchanged.
- [ ] Bound subprocess output, execution time and cancellation/cleanup for effectful requests. Preserve real exit/signal and diagnostics; do not report a cancelled/hung child as a normal cache miss.
- [ ] Follow the reviewed use of project Mise setup in jobs that need it: pinned action after checkout, appropriate normal install/cache/env/path behavior. Resolve its version explicitly from the selected project/tool policy and verify its platform-specific binary checksum.
- [ ] Keep trusted generator/validator/bootstrap operations isolated from arbitrary repository hooks/config. Enabling project Mise semantics is an explicit capability, not permission to execute project hooks while holding privileged credentials.
- [ ] Adopt a concrete repository-owned lock/pin policy. The generator never silently creates/edits consumer `mise.toml`, `mise.lock`, `rust-toolchain.toml`, or `.mise-version`. Developer-authored changes to this repository are separate reviewed changes, not generator side effects.
- [ ] Reconcile the current root Cargo wrapper requiring MBX with the absence of a local MBX tool pin. Prove a clean developer environment works using the chosen tool authority; do not assume a globally installed MBX.

Effective version defect to qualify: in job 109575245197 the Mise invocation pins MBX 1.19.0, while the pinned Mr. Boxington action reports setting up MBX 1.21.0. Pinning an action SHA did not pin its installed executable. Provide the supported exact version/tool-path input or use the action's supported preinstalled-tool mode; otherwise qualify an explicitly chosen action/version update. Fail on mismatched effective compiler/cache tool identities. Do not mix cache transport and compilation formats based on a nominal catalog string.

Verification: sentinel credentials present in parent/action environment are absent in repository child; required tool paths remain; reserved overrides rejected; cold project setup succeeds; actual Mise/Rust/components/MBX/Nextest/Actionlint identities match the selected catalog/profile; action setup and compiler use compatible proven MBX identities; cancellation and bounded-output fixtures cannot hang verification.

# 14. P08 — Give every cache layer one owner and prove warm reuse

Primary changes: workflow preparation/task/action steps, Mise cache integration, source transport and cache identity modules, Rust lane planning, generated workflow and cache integration fixtures.

Observed evidence: task logs restore a Mise archive, report a broken Rust symlink, then download six Rust components. Cargo fetch updates the index and downloads sources before MBX object restore. The action then reports no MBX cache. The review document also records role-suffixed Mise snapshots; do not claim their archives were byte-identical without inspection.

- [ ] Inventory exact runtime paths for Mise installations, rustup toolchains, Cargo sources/binaries, Cargo target and MBX objects. Cached symlinks without their targets do not count as a warm toolchain.
- [ ] Use the Mise action's qualified built-in cache for compatible resolved tool inputs; remove duplicate role-specific manual caches over the same directories. Distinct actual tool sets may need distinct identities; job-role names alone must not create redundant copies.
- [ ] Cache only the necessary Cargo source subset, with credentials excluded, at the Cargo home actually used. Design one race-safe trusted writer for a shared immutable source snapshot and readers in crate jobs. Never have seven jobs race to update one immutable key.
- [ ] Restore appropriate caches and configure MBX BEFORE Cargo fetch/build/test. When all locked sources are available, skip online fetch and execute offline. A legitimate cold/incomplete source cache fetches through an explicit preparation path and records the miss.
- [ ] For MBX choose and document a QUALIFIED transport: per-crate target archives OR objects archives plus shared source cache. Benchmark stored bytes, cross-job duplicate bytes, aggregate transfer, restore/save, compatibility and churn. Keep target directories isolated where writers are concurrent.
- [ ] Do not archive the same paths via MBX, another Cargo action, and custom archives. If target mode includes registry data, redesign ownership rather than stacking a second registry archive blindly.
- [ ] For Cargo-only projects provide an appropriate Cargo cache. A new action such as Swatinem/rust-cache needs an explicit allowlist/catalog/schema change, full-SHA pin and negative-input tests; it is not already permitted by the old allowlist.
- [ ] Global validators cache only their own necessary tools/dependencies. Eliminate installing Actionlint/ShellCheck/Zizmor in each Rust task when those checks run elsewhere.
- [ ] Qualify same-repository PR cache saving only when the selected pinned action supports a PR-scoped policy. Fork PRs stay read-only. PR outputs never become trusted/release evidence. Include commit/run ancestry and exact compatibility where required by the backend.
- [ ] Save only producer-successful useful deltas in allowed trust scope, after writers finish. Cache-service errors must not turn successful verification into failed source correctness, nor permit skipped work.
- [ ] Measure quota/headroom from current service data, not a hardcoded assumption. Distinguish stored duplicate archives from unavoidable separate-runner download traffic.
- [ ] Do not add remote MBX infrastructure under this goal merely because the review lists it as an alternative; V1 remains free of a remote-cache service unless scope is explicitly changed outside this goal.

Verification: trusted seed then unchanged sequential run; fresh checkout with same inputs; source edit; lock/tool/target/profile changes; cache eviction/unavailability/corruption; simultaneous writer attempts; same-repo PR and fork behavior; cold/warm rustfmt and Clippy components. Report effective tools, hits/misses, actual origin downloads, bytes and durations across ALL seven crate jobs, not one cache-hit label.

# 15. P09 — Make filesystem operations meet their advertised guarantees

Primary changes: `.../orchestrator/src/generate.rs::{prepare_preview_dir,swap_directories}`, output path validation and tool snapshots; tests around init/plan/preview/generate.

Evidence: the current preview path can be created before repository containment is rejected. Replacement renames the existing tree aside before moving the staged tree into place; this is not an atomic exchange. Rollback failure is discarded, while cleanup failure can be returned after new output is already installed.

- [ ] Validate destination containment, ancestor relations, symlink resolution and freshness BEFORE creating any directory/file. Reject a preview path inside the repository without leaving created parent directories.
- [ ] Reserve a destination race-safely and reject unrelated nonempty contents. Avoid check-then-act path substitutions between validation and write.
- [ ] Implement a real supported atomic directory exchange/commit primitive through a safe reviewed dependency where needed; keep product unsafe-code prohibition. Document filesystem/platform capabilities. Do not label a two-rename visibility gap atomic.
- [ ] Keep staging and destination on the required filesystem. Unsupported commit capability fails before touching old output; do not silently fall back to delete/copy.
- [ ] Preserve both the primary operation error and rollback failure. Separate committed-success-with-cleanup-warning from not-committed failure so callers can understand actual state.
- [ ] Protect the repository against simultaneous generate operations and partial writes with a tested ownership/lifecycle rule; do not add an unnecessary daemon/database.
- [ ] Snapshot/check relevant user files before the first potentially effectful preparation operation. Do not collapse unreadable and missing files with `read().ok()` when the no-write proof depends on their contents.
- [ ] Qualify real Cargo metadata behavior for lockless and locked workspaces and offline preparation. If an upstream operation can mutate the repository, use a safe scratch/read-only analysis path or fail before mutation; post-hoc detection alone is not a no-write guarantee.

Verification: normal/linked worktree/nested invocation; nonexistent nested preview inside repo; symlink and ancestor cases; concurrent writers; failed validation; injected failure before/during/after commit; rollback failure; cleanup failure; old-tree byte identity on noncommit; no transient missing output where atomicity is promised; missing/unreadable tool files; lockless metadata. Run supported platform tests, not only mocked rename return codes.

# 16. P10 — Use Git as the authority for worktrees and path serialization

Primary changes: `.../orchestrator/src/prepare.rs::origin_matches`, `.../select.rs::changed_files`, Mise-owned typed Git requests and repository discovery tests.

- [ ] Replace manual `<absolute-git-dir>/config` parsing with Git's supported configuration query in the correct working tree. Linked worktrees use a shared config location; includes and worktree configuration are Git semantics, not a custom parser exercise.
- [ ] Normalize the returned repository identity before enforcing Velnor-only policy. Missing/mismatched origin fails appropriately; no network fetch or guessed main branch.
- [ ] Use NUL-delimited Git output for filenames. Preserve path bytes through selection and diagnostics or reject unsupported encoding explicitly. Do not trim away meaningful filename whitespace or parse C-quoted display output as a path.
- [ ] Preserve staged/unstaged/untracked/delete/rename semantics as required by each public versus CI context. Do not introduce a base/head CLI flag or silently use a feature branch as default.

Verification: normal repo + linked worktree share the same remote identity; include/worktree config cases; Unicode, spaces and newline-containing filenames; changed deleted/renamed paths; nested invocation. The advisory audit already reproduced the shared-config location and quoted `--name-only` output with Git 2.47.3; reproduce through the product's actual typed request boundary.

# 17. P11 — Strengthen Rust guarantees without lint theater

Primary changes: root/member Cargo manifests, `clippy.toml`, `rustfmt.toml`, `.alint.yml`, `AGENTS.md`, repository-policy tests, trust/plan/report types and affected module boundaries.

- [ ] Preserve Rust 2024, resolver 3, meaningful declared MSRV, committed locked resolution, inherited lints, unsafe prohibition, denied must-use failures, Clippy all/pedantic, narrow reasoned expectations, documentation warnings and strict formatting.
- [ ] Do NOT enable all of Clippy restriction/nursery indiscriminately. Qualify individual additional rules against this codebase and negative fixtures. Mutually contradictory lints or widespread suppressions reduce useful strictness.
- [ ] Represent validated identifiers, digests, normalized paths, finite positive resource limits and authenticated evidence with private constructors/newtypes. Replace flag combinations permitting 'successful but missing evidence' with exhaustive enums and validated transitions.
- [ ] Keep parsing of untrusted JSON/TOML distinct from validated domain values; reject unknown schemas/keys and duplicate critical keys, impose size bounds, and preserve contextual errors. Do not silently default security-critical fields.
- [ ] Avoid public mutable fields that let callers bypass proof construction. Keep fallible I/O/process errors as typed results, and distinguish missing, malformed, inaccessible, cancelled and unsupported states.
- [ ] Enforce the existing hard file/function limits through the actual supported tools. Split modules by responsibility, not arbitrary wrappers or renamed generated exceptions. Keep handwritten tests in separate files and maintain meaningful registered tests for every product crate.
- [ ] Apply Alint's requested unique OSS/GitHub Actions/Rust/lockfile/tracked-artifact bundles after verifying support in the selected version. Deduplicate the repeated GitHub Actions bundle from the review example.
- [ ] Enforce effective edition 2024 in every crate. Adopt the requested literal field or document and test an equivalent supported inheritance-aware rule; do not add a literal equality rule that necessarily rejects `edition.workspace=true` and then suppress it.
- [ ] Replace substring 'architecture proof' with focused Cargo metadata/TOML/compiler assertions and independent review. Keep Alint limited to its actual supported semantics. Do not create the excluded general Rust AST/test-layout linter.
- [ ] Update AGENTS.md to lead with V1 generator boundaries, concrete verification commands, source-of-truth links and proof invariants. Preserve the user's autonomy/commit/review-integrity rules. Move irrelevant runner-protocol instructions to deferred runner documentation.

Verification: negative fixtures reject unsafe, ignored Result, prohibited production panic/unwrap, oversized files/functions, weakened lint inheritance, wrong edition and unsupported dependencies; valid tests do not require broad allows. Demonstrate rejected incomplete/forged proof construction and exhaustive failure behavior. All seven crate checks pass with denied warnings and real test registration.

# 18. P12 — Replace shallow policy/freshness checks with complete validated inventories

Primary changes: `scripts/check-freshness.sh`, `.velnor/version-policy.toml`, catalog/lock loading and update validation, `crates/velnor-actions-cli/tests/impl_repo_policy.rs`, `.cargo/mutants.toml`, `docs/implemented/update-procedure.md`.

Evidence: the script explicitly avoids upstream lookup and validates only part of the manifest/lock policy; it can miss workspace, string-valued, build, target and renamed declarations. A map keyed only by package name loses multiple versions/source identity. Policy comments/substring searches are not semantic validation. Read the current exact script before modifying because later fixes may already exist.

- [ ] Parse all Cargo dependency forms and scopes, including workspace inheritance, build/dev/target dependencies and aliases. Compare Cargo package identities by name+version+source and retain multiple versions rather than overwriting them in a map.
- [ ] Make every failed inventory row contribute to a nonzero check. Assert the expected complete set of tools/actions/dependencies, not only provided rows.
- [ ] Separate local pin consistency, effective runtime identity, upstream freshness and security-advisory validation. Do not call local equality an upstream freshness proof.
- [ ] Implement the stated scheduled/read-only upstream freshness evidence through bounded supported tooling, with source and check time. Normal builds consume exact reviewed pins and never resolve latest independently. Operational lookup failure is explicitly unknown/failed according to policy, never 'current'.
- [ ] Validate exceptions with hard policy maxima, owner, blocking issue, technical reason, held version, chronology and expiry. Reject arbitrary standing exceptions and future/inverted dates. Keep only explicitly approved special records, such as the separately reviewed Alint action-tag exception.
- [ ] Reconcile the update procedure with the actual mechanism. Do not silently weaken normative freshness or create an unnecessary general update product; use a focused repository verification path.
- [ ] Run Cargo dependency/security validation with the selected tools. Report reachable high/critical advisories only with evidence; do not invent vulnerability claims from version numbers or the absence of a local audit tool.
- [ ] Add a pinned, risk-triggered property/mutation/fuzz path for untrusted parsers, obligation selection, identities and aggregation. Update the current manual-only mutation scope to include the real production wiring under review. Pin each activated tool first.

Verification: fixtures with every dependency syntax, renamed/multiple-source packages, missing action rows, deliberately stale pins, unreachable upstream, malformed/expired/future/overlong exceptions and intentionally failing rows. The check must fail for actual policy violations and pass for a supported complete inventory; comments cannot satisfy it.

# 19. P13 — Test the real pipeline and measure architecture-level performance

Primary changes: production-boundary integration suites, `.../inventory.rs::run_inventories`, repeated identity/graph construction, generated CI, benchmark harness and `docs/implemented/performance.md`.

- [ ] Provide one documented local verification entrypoint using existing Mise/tool conventions, not a new public velnor-actions command. It must check formatting, policy, generated-tree freshness, per-crate Clippy/tests/doctests/docs and relevant integration fixtures, preserving failures.
- [ ] Retain useful unit/golden tests but add integration tests traversing config -> discovery -> complete plan -> IR -> generated workflow/helper -> report artifacts -> Required. Independent expected outcomes must not be generated by the same defective helper.
- [ ] Add explicit regression cases for P00–P12, including generated-workflow negative runs. Do not approve an in-memory scheduler merely because an unconnected unit test passes.
- [ ] Profile subprocess counts and graph construction. `run_inventories` currently executes metadata serially for each candidate; reuse known workspace metadata safely after validating membership and preserving nested/independent/malformed detection. Build the graph/path index once per immutable snapshot.
- [ ] Bound metadata/compiler/test concurrency. Do not trade serial duplication for concurrent Cargo lock contention. Measure operation counts on 1/10/100-crate fixtures and report actual complexity behavior without inventing speedups.
- [ ] Qualify the same required obligation/test set before and after optimization. Build-once Nextest archives and sharding remain conditional and must not recreate forbidden default per-task job fan-out; default seven-crate graph is mandatory.
- [ ] For any enabled partitions, validate full inventory union, pairwise disjointness, archive identity, missing/extra/duplicate tests and explicit empty partition semantics. An unqualified optimization stays off with honest unpassed capability status.

Required benchmark cases, all with exact versions, named hardware/image, same obligation set and raw evidence:
- [ ] Truly empty tool/source/compiler/task caches, with installation and origin downloads included.
- [ ] Fresh checkout restoring compatible warm sources/compiler data, including transfer/extraction.
- [ ] Unchanged repeated validation, not merely an empty build invocation.
- [ ] One source edit in a leaf crate, including required downstream verification.
- [ ] Shared public-API edit and reverse-dependent closure.
- [ ] Two AND four concurrent validation lanes where the named hardware can support them; demonstrate actual capability limits rather than assuming them.
- [ ] Real dependency resolution and toolchain changes. Touching Cargo.lock's timestamp is not a dependency/toolchain-update benchmark.

Record queue time separately from execution, end-to-end Required critical path, total runner work, setup, cache transfer/restore/save, origin downloads, compiler invocations/hits/misses/bypass, tests, resource contention and process-tree memory. Current reports with duration_ms=0 are not telemetry. Current historical 'cold' measurements used a warm registry; preserve them as historical partial cases, not qualified acceptance.

Meet the stated provisioned structural-preflight, warm-leaf and two-minute warm fixture/dogfood budgets on named runners. Report failure honestly when a budget is unmet; do not delete checks, reset baselines, hide setup/transfer, add overlapping durations, or relabel a partial measurement. A current failed run's wall time is observational evidence, not a controlled speedup experiment.

# 20. P14 — Reconcile all review feedback explicitly

Maintain the following ledger alongside original comment URLs and the full review document. Each item needs accepted/modified-with-reason/rejected-with-evidence/proven-external-blocker disposition, code/test evidence, and fixing commit. A checkbox is complete only after behavior is verified.

| ID | Required review outcome | Main package |
|---|---|---|
| R01 | Seven default Rust crate jobs; separate global validators | P05 |
| R02 | Validators inspect global state in distinct jobs; no orchestration per-task fan-out | P01, P05 |
| R03 | No combined Policy/Workflow Lint job hiding tools | P05 |
| R04 | Unique requested Alint bundles and valid edition-2024 enforcement | P00, P11 |
| R05 | Appropriate caches per tool job; MBX before consuming Cargo operations | P07, P08 |
| R06 | Compatible Mise cache identity without role duplicates/double ownership | P08 |
| R07 | Explicit measured Rust-cache payload choice across all seven jobs | P08, P13 |
| R08 | No overlapping cache owners for Mise/Cargo/MBX/target paths | P08 |
| R09 | Race-safe shared immutable-cache writer policy | P08 |
| R10 | Actual Cargo-home paths; complete warm restore offline; controlled cold fetch | P08 |
| R11 | Whole-workflow cache sizes/transfer/save/eviction and quota headroom | P08, P13 |
| R12 | Trusted seed then unchanged run avoids redundant dependency downloads/builds | P08, P13 |
| R13 | Supported same-repo PR-scoped saving; fork read-only; no trusted promotion | P04, P08 |
| R14 | ci.yml, display CI, responsibility-based filenames | P05 |
| R15 | Rust grouping and package-name display with collision-safe identity | P05 |
| R16 | Format/Clippy/test steps inside crate jobs, no per-obligation jobs | P05 |
| R17 | Nextest config and CI profile detection with explicit override precedence | P06 |
| R18 | Project Mise detection selects setup in every job that needs it | P06, P07 |
| R19 | Pinned action + selected Mise CLI + platform checksum after checkout | P07 |
| R20 | Standard qualified Mise setup; remove unjustified duplicate custom setup | P07, P08 |
| R21 | Reproducible cold installation and explicit lockfile policy | P07, P12 |
| R22 | Cold/warm Rustfmt and Clippy components actually present | P07, P08 |
| R23 | Repository Cargo wrapper selects MBX; no local evidence defaults Cargo | P06 |
| R24 | MBX action/commands only for MBX; Cargo mode remains genuinely Cargo | P06, P07 |
| R25 | Explicit combined MBX+Nextest command/profile rule | P06 |
| R26 | No automatic execution of tasks inferred from names | P06 |
| R27 | No Velnor prefix in job names/IDs, no internal metadata in labels | P05 |
| R28 | Formatting executes once per intended scope | P05 |
| R29 | Accurate local build/help/plan/preview and consumer installation docs | P15 |
| R30 | Record proposal-to-contract adoption before implementation changes | Section 3 contract phase |
| G01 | Original comment 4119057282: consumer bootstrap without Velnor-only lock | P03, P15 |
| G02 | Original comment 4119057286: self-contained consumer tool catalog | P07, P15 |
| G03 | Original comment 4119057292: Mise owns subprocess effects | P07, P11 |

Do not ignore non-checkbox prose, later replies or new feedback. Re-fetch all feedback at the final head. The original three inline threads were resolved at an earlier docs revision; maintain their regression protections without pretending resolution is current-code approval. For accepted feedback, respond with verified fixing commit evidence before resolving. For rejected suggestions, explain concrete contrary evidence before resolving. Never delete feedback or resolve it merely because code was pushed.

# 21. P15 — Finish documentation, bootstrap qualification and the handoff

- [ ] Correct PR title/body and README: this is implemented-but-under-qualification code, not a docs-only PR. State exactly which capabilities are verified, incomplete or deferred.
- [ ] Document local build/help/plan/preview commands that actually use the selected Mise/MBX route. Prove them from a clean environment without a global MBX dependency. Keep the public CLI small and warn that default generate replaces the complete .github tree.
- [ ] Explain source-built versus officially released consumer generation accurately. Do not promise source-built consumer output when verified release provenance is required; do not add an unchecked bypass to make an example work.
- [ ] Preserve self-contained consumer workflows: no consumer generator.lock/version-policy requirement. Verify real release URLs, per-target digests and immutable manifests where the bootstrap gate requires them.
- [ ] Keep candidate qualification and trusted bootstrap planning non-circular. Existing documented pre-seed mode is temporary qualification state, not a completed protected release. Do not call a source binary trusted by filling in a lock digest or accepting its self-report.
- [ ] Update implemented records with exact code SHAs, current passing runs, deviations and unpassed gates. Historical evidence remains tied to its historical SHA. Do not fabricate a merged date, published seed, approval or two-minute result.
- [ ] Keep the direction V1-first: trustworthy local generator, real consumer flow, correct required status, qualified caches and actual dogfood. No speculative runner/UI/extra-stack roadmap expansion under this remediation.

# 22. Verification commands and final acceptance

Use exact qualified tool identifiers from the committed catalog/config. Discover the existing entrypoint rather than inventing one. When a command below is a shape, resolve placeholders to real supported versions before execution and record the actual command:

- Mise-managed Cargo metadata for workspace/package enumeration and locked graph verification.
- Mise-managed `cargo fmt --all -- --check` for the local aggregate audit; generated crate formatting remains correctly scoped.
- For every one of the seven product packages: selected MBX/Cargo `clippy --package <package> --all-targets --locked -- -D warnings`.
- Selected pinned Nextest with the resolved CI profile, or Cargo test, per package; during qualification gather all failures, and count registered/actually run tests. Keep doctests and rustdoc warnings checks separate.
- Pinned Cargo Deny, Cargo Machete, Alint, Actionlint plus ShellCheck, and Zizmor as applicable.
- Generator preview into a fresh external temporary directory; compare the complete .github tree to committed output; run twice for deterministic output and compare public plan facts.
- Negative integration fixtures for all proof, process, cache, path and workflow-state boundaries.
- Actual GitHub-hosted generated workflow at final head, plus controlled negative fixture runs and cold/warm cache experiments.

Do not run a bare PATH binary and call it the pinned tool. Do not quietly substitute a model/tool/version or bypass mandatory exact-model rules in the execution environment. If the repository/runtime enforces an exact agent model and reasoning level, verify it for coordinator and subagents before delegation and fail closed on an unverified substitution.

Final checklist:
- [ ] Every P00–P15 finding is reproduced or explicitly reclassified with contrary evidence, then fixed/tested where confirmed.
- [ ] Every R01–R30 and G01–G03 feedback item has an evidence-backed disposition; later feedback is included.
- [ ] Complete obligations survive selection; no missing proof, validator or report can yield green.
- [ ] Real source/tool/platform identity controls baseline/cache reuse; no fabricated intact evidence or zero trust digests.
- [ ] Exact default seven-crate graph, separate validators, clear IDs/names and ci.yml migration are complete.
- [ ] Project-config/override semantics and effective tool versions are qualified without credential inheritance.
- [ ] Output replacement and preview/no-write guarantees hold on supported platforms with failure injection.
- [ ] Cache ownership/path/order/trust and actual cold/warm behavior are measured across the whole workflow.
- [ ] Strict Rust policy and meaningful behavioral tests pass; no weakened diagnostics, tests or exclusions.
- [ ] Performance cases and required budgets are verified, or accurately remain explicit unpassed external gates—never claimed done.
- [ ] Documentation, local commands, implementation status and PR body match the final source and evidence.
- [ ] Independent reviewers challenge proof soundness, trust, architecture, workflow behavior and performance; their findings are resolved and checks rerun.
- [ ] Final clean-checkout verification and remote checks refer to the same pushed SHA; no unrelated files or temporary artifacts are included.
- [ ] Required approvals/settings/release permissions are respected. No automated claim of human approval, publication or merge.

Return a final completion report with branch/PR/final SHA, root-cause fixes, P/R/G checklist status, actual verification commands/results/run links, tool identities, performance/cache measurements, review dispositions and any precisely demonstrated remaining external blocker. Keep working on actionable independent items until complete. Do not stop at another plan, scaffolding, a green unit suite, or a historical green run.
