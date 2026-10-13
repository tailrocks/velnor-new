/goal
Improve `velnor-actions` in https://github.com/tailrocks/velnor-new so its own CI and all 46 already-migrated consumer repositories achieve correct, measurable cross-run reuse of Mise tools, Rust toolchains, Cargo sources and MBX-supported compiler/workspace state, and run only the validation obligations genuinely affected by a change or not covered by valid evidence.

Implement, test, independently review, publish the qualified generator runtime, regenerate the consumers, merge the changes, and verify the actual resulting CI behavior. This is not a request to recommend caching, add cache-looking YAML, or stop at open PRs.

## Read these files first

Read `velnor-actions-ci-performance-spec.md` completely, including C01–C09, the cache ownership/trust model, affected-work rules, T01–T26, source references, and Appendix A. Read `scope.json`, `repository-evidence.csv`, and `repositories.txt` alongside it. These files form the task's implementation reference and initial evidence inventory.

The research baseline is generator commit `c57c700459bbe1549fe7eedcb7d8689585c38986`, not a permanent pin. Re-read the current repository, instructions, contracts, tool/action source, open work, releases and runtime manifests before making changes. Resolve documentation/source discrepancies using evidence and update the specification when a better verified design is needed. Do not blindly repeat stale versions or assume that a `0.1.0` version string identifies one binary.

The research located 45 consumer PR/workflow references but did not complete every PR-diff/job-log audit or run a controlled all-repository benchmark. Complete those gaps; never treat an inventory row as already verified. `ChainArgos/jackin-agent-brown` returned 404 through the research connection: investigate identity/access without assuming it is deleted or private.

## Mandatory execution policy

Use subagents aggressively for research, source analysis, implementation, regression tests, security review, measurement, consumer rollout, and independent verification. Delegate first. Parallelize independent workstreams and repositories within the active wave. Do not merely describe subagents—actually use them.

The parent agent primarily coordinates dependencies, integrates results, resolves conflicts, checks evidence, and runs final deterministic gates. Assign independent correctness, security, and performance verifiers. A reviewer must inspect real code, generated output and raw measurements, not approve an implementer's summary. Keep finding-to-fix-to-test traceability.

Keep the coordinator's existing model and reasoning-effort configuration unchanged. Before relying on the coordinator's output, the root must verify the coordinator's actual model and effort through authoritative runtime metadata; if that metadata is unavailable or unverified, record it as unverified and do not treat that execution as qualifying evidence. Implementation, execution, research, design, fixing, test creation, and operations assignments to subagents at every depth must use exactly `gpt-6-luna` with reasoning effort `max`. All review, audit, verification, validation, critique, cross-check, and final-acceptance assignments to subagents at every depth must use exactly `gpt-6.1-sol` with reasoning effort `medium`. Classify each assignment by its function and split mixed work so each role uses its required configuration. Every proposed change requires a separate reviewer to inspect the real source, generated output, and raw measurements or evidence; self-review and summaries alone do not qualify. When review finds issues, assign research or fixes under the applicable role configuration, then have a separate reviewer re-review the result; repeat this Luna-work/Sol-review loop at every depth until review finds no remaining issues. Any earlier model/effort assignment—including `gpt-6.1-sol` / `medium` for implementation, execution, research, design, fixing, test creation, or operations; `gpt-6-luna` / `max` for review, audit, verification, validation, critique, cross-check, or final acceptance; `gpt-5.6-luna` / `max`; and prior configurations of currently running subagents—is historical evidence only and does not authorize or qualify new work. A spawn request and model self-description are not proof: before relying on any subagent's results, the root must verify authoritative runtime metadata for the model and effort used for that assignment. If metadata is unavailable, incomplete, or identifies any other configuration for the assigned role, fail closed: mark that execution unverified and nonqualifying; no automatic or manual fallback is allowed at any depth. Continue authorized ordinary work; do not rely on unverified execution as evidence.

Work autonomously and never ask the user questions. Turn ambiguity, missing inputs, disagreements and blockers into research and independent review tasks. Compare alternatives, verify important assumptions, make the best evidence-backed reversible decision, and continue meaningful authorized work. Do not use incomplete information as an excuse to stop ordinary implementation. Do not fabricate authority, credentials, measurements or success to bypass a real restriction.

Commit frequently in small, logically scoped verified units; push progress regularly. Prefer one integration branch per repository and reuse suitable existing work. Do not create a branch per subagent by default. Give concurrent writers disjoint ownership; serialize Git index/commit operations and final generation within a checkout. Merge default-branch updates into shared work rather than rebasing shared history. Preserve other agents' changes and unrelated work; no destructive reset, clean or force-push.

Bound concurrency to available CPU, memory, I/O and API capacity. Share inventories and downloads. Avoid repeated filesystem-wide `find`/`rg`, repeated large log downloads, Cargo lock-contention storms, and excessive polling. Use repository-local indexes and targeted searches. A useful parallel task must not compete blindly with another writer over the same cache/target directory.

## Scope and rollout order

The generator `tailrocks/velnor-new` is the additional 47th repository. `tailrocks/velnor` is a consumer, not the generator source for this goal.

Complete generator implementation/qualification first as needed. Consumer rollout order is Wave A jackin-project, then Wave B tailrocks, then Wave C ChainArgos. Start with `jackin-project/jackin` as the first consumer canary. Parallelize independent work inside each wave. Read-only research may cover all waves, but do not push or merge ChainArgos changes before earlier consumer waves are qualified. Revalidate earlier consumers affected by a later generator fix.

The exact 46-consumer scope is listed below and in `scope.json`; do not silently drop inaccessible or already-compliant targets.


### Wave A: jackin-project — first

01. https://github.com/jackin-project/jackin
02. https://github.com/jackin-project/jackin-agent-smith
03. https://github.com/jackin-project/homebrew-tap
04. https://github.com/jackin-project/jackin-role-action
05. https://github.com/jackin-project/jackin-sentinel
06. https://github.com/jackin-project/jackin-dev
07. https://github.com/jackin-project/jackin-github-terraform
08. https://github.com/jackin-project/jackin-the-architect

### Wave B: tailrocks — second

09. https://github.com/tailrocks/github-terraform
10. https://github.com/tailrocks/termpane
11. https://github.com/tailrocks/tui-snap
12. https://github.com/tailrocks/velnor
13. https://github.com/tailrocks/termrock
14. https://github.com/tailrocks/parallax
15. https://github.com/tailrocks/terminal-components-claude
16. https://github.com/tailrocks/tailrocks-repository-skills
17. https://github.com/tailrocks/tailrocks-skills
18. https://github.com/tailrocks/tailrocks-pull-request-skills
19. https://github.com/tailrocks/homebrew-velnor
20. https://github.com/tailrocks/velnor-apt
21. https://github.com/tailrocks/parallax-telemetry-playground
22. https://github.com/tailrocks/velnor-actions-fixture
23. https://github.com/tailrocks/holla
24. https://github.com/tailrocks/tracing-request-level
25. https://github.com/tailrocks/pg-bigdecimal
26. https://github.com/tailrocks/ruxel
27. https://github.com/tailrocks/schemalane
28. https://github.com/tailrocks/holla-apt
29. https://github.com/tailrocks/homebrew-parallax
30. https://github.com/tailrocks/homebrew-ruxel
31. https://github.com/tailrocks/homebrew-tablerock
32. https://github.com/tailrocks/homebrew-holla
33. https://github.com/tailrocks/tablerock
34. https://github.com/tailrocks/cloudflare-tofu
35. https://github.com/tailrocks/tailrocks-typescript-skills
36. https://github.com/tailrocks/tailrocks-skill-authoring-skills
37. https://github.com/tailrocks/tailrocks-rust-skills
38. https://github.com/tailrocks/tailrocks-roadmap-skills
39. https://github.com/tailrocks/tailrocks-open-source-skills
40. https://github.com/tailrocks/tailrocks-macos-skills
41. https://github.com/tailrocks/tailrocks-code-quality-skills

### Wave C: ChainArgos — last

42. https://github.com/ChainArgos/blockchain-nodes
43. https://github.com/ChainArgos/java-monorepo
44. https://github.com/ChainArgos/jackin-agent-brown
45. https://github.com/ChainArgos/cloudflare-tofu
46. https://github.com/ChainArgos/github-terraform

## Phase 0 — Complete the audit and lock down correctness

Create a living performance-remediation ledger with all 47 repositories, current source/runtime identities, relevant PRs, default branches, visibility, selected workflows/events, CI/CD obligations, actual runs/attempts/jobs, cache evidence and qualification gaps.

For each repository, read the relevant recent generated-workflow migration/update PRs and subsequent workflow changes. Inspect the current generated workflow family, typed config, generator runtime manifest, tool pins, cache paths/keys/save conditions, matrix topology, baseline/task selection, release behavior and required gate. Include every affected stack, not just Rust. Inspect recent representative PR and actual default-branch runs; paginate APIs and distinguish run metadata from full step/log evidence.

Use the observed generator run `37012391691`, Plan job `110855000716`, and Rust/contract job `110855475688` as regression examples. Reproduce their defects before claiming a fix. The inspected generator main run failed at orchestrator Format and Required; restore actual correctness without hiding failure or deleting checks. Do not use an old successful unrelated run as present validation.

Measure queue/provision/setup, tool download/install, cache transfer/import/export, helper compilation, Cargo/MBX work, tests, reports and total critical path separately. Capture actual tool identities and runner image/resources. A “Compiling” line is not proof of a cache miss; zero MBX misses is not proof of zero compilation when units were not looked up or bypassed. Record all those categories.

## Phase 1 — Repair tool and source caches

Implement C01, C02 and C06 upstream in the generator's existing typed cache/tool integration.

Make restore and save use the same canonical resolved payload paths, ordering, archive/compression/version identity and key. Eliminate the current absolute-path versus tilde-path mismatch. Use a new schema/generation for the corrected payload; do not overwrite or globally delete old immutable caches.

Restore the complete isolated required Rustup/tool installation before verification/install, including necessary proxies, metadata and symlink targets. Keep tool state separate from registry/Git sources and exclude credentials. Verify shell steps and action processes use the intended homes and exact toolchain. Prefer the minimum required Rust profile/components/targets after testing supported pinned options.

Keep Mise's idempotent verify/fill behavior, but a complete compatible warm hit must not download or reinstall tools. Preserve isolation from repository config, env files and hooks. Do not solve the save issue by accidentally allowing untrusted project tool hooks to execute.

Use selected dependency/source closure for Cargo fetch and cache identity. Reuse compatible archives, fetch only missing selected requirements, and do not recompress or re-upload unchanged source/tool data. No broad Cargo-home archives containing credentials or unrelated mutable state.

## Phase 2 — Make MBX reuse persistent across real jobs and runs

Implement C03–C05. Route supported Rust compilation workloads through the selected pinned MBX integration. Do not silently fall back to an uncached Cargo-only path; explain legitimate unsupported/bypass work and deliberately uncached verification probes without suppressing execution. Do not impose irrelevant Rust jobs on non-Rust repositories. Apply Section 4.3 to the actual Java, Node, OpenTofu, container and distribution workloads: native cache ownership, complete inputs, immutable compatible snapshots, and no credential/state leakage. Do not assume a host cache automatically persists container cache mounts or that a dependency-store hit replaces install verification.

Make Mise the single installer of the exact MBX executable and use a verified supported action integration that consumes it. Eliminate the redundant install; never replace an explicit pin with a possible floating fallback. Verify the action's compiler identity probe agrees with task execution.

Separate the generator helper-release domain from validation domains. Define compatible workspace/configuration/writer cohorts, immutable useful-progress snapshots and constrained restore prefixes. Fix exact-key starvation so Clippy/dev/test work produced after Plan is saved and reusable. Verify sibling writers and retry order. Do not create one gigantic shared mutable target or a wasteful full archive per tiny crate.

Use the pinned MBX action's supported object/workspace-state transport and actual inputs/APIs. It already restores Cargo workspace state in the observed run; investigate wrong/missing profile/prediction/fingerprint state rather than stacking an arbitrary `target/` cache on top. No custom parsing/merging of MBX internal formats, sccache layer, second cache engine or cache server.

Compare the existing trusted-read-only PR policy with a separately reviewed, server-isolated PR-only cache policy where useful. Do not claim that new PR-only inputs will remain warm across fresh runs when writes are intentionally disabled and no trusted snapshot exists. Never let PR state enter trusted/default-branch/release caches, expose credentials, or relax production protection for speed. Preserve the safe default unless the explicit policy change passes the specification's security and provenance tests.

## Phase 3 — Select early and shorten the critical path

Implement C07–C09 and Section 6 of the specification. Extend existing selection and baseline code instead of replacing it with filename heuristics.

Compute effects from the actual integration candidate and its exact base, using both graphs for deletions/renames/topology changes. Include reverse normal/build/dev/optional/target dependencies, proc macros, build scripts, generated inputs, native code, fixtures, included documentation, relevant config/environment and source-consuming tools. Unknown/incomplete inputs broaden work. Refine global invalidation only with complete evidence.

Enumerate every required obligation. It must execute successfully, reuse a qualified complete deterministic result, or be covered by valid exact baseline proof. Cached compiler output is not a passed test. Failed/missing/old/untrusted proof must not make a no-op success.

Omit fully covered compiler jobs before runner allocation and setup; do not merely skip task shell commands after downloading Rust and MBX archives. Keep a correct minimal current plan and stable Required gate. Test empty matrices, missing reports/shards, cancellation, merge queues, base updates and true docs-only changes. Do not use global path filters that leave required checks absent.

Reduce duplicate setup through measured workspace/configuration cohorts. Preserve Cargo feature semantics and separate visible checks. Correct misleading test-build labels; use actual test-binary preparation. Compare Nextest build-once archives/test-only shards only where transfer/startup costs are justified; preserve fixtures, source identity, compatible runtime and doctests.

Keep consumers on verified prebuilt generator artifacts. The generator's own dogfood path must still exercise the candidate source. Optimize helper build/profile, cache reuse and artifact distribution without using stale code to validate new code. Reduce repeated checkout/history work, API calls and redundant report transfers only with correctness/provenance tests.

Use the existing Mise task-result cache only for qualified deterministic tasks with complete inputs and outputs. Do not cache live-service, release, deployment or undeclared nondeterministic outcomes. Never replay old test logs as fresh test execution.

## Phase 4 — Prove performance and correctness

Execute applicable T01–T26 using isolated cache namespaces and fresh hosted runners. At minimum perform a complete cold run, unchanged warm run, and third unchanged run proving late-produced state persists. Use the same immutable source/configuration for unchanged cases and real semantic edits for changed cases. Do not rely on two commands in one warm checkout or meaningless file touches.

For complete compatible caches, prove zero redundant tool payload downloads, zero avoidable eligible third-party compiler work, correct MBX not-looked-up/bypass accounting, no unchanged exports, and safe cold fallback. Changed source/toolchain/features/targets may legitimately require work; explain each invalidation.

Compare conservative full obligation coverage against the selected plan and inject failures to prove nothing required is lost. Test cache corruption/unavailability, missing Rustup contents, reversed writer order, exact-hit plus useful delta, stale baselines, fork trust boundaries and release isolation.

Measure end-to-end improvement and resource costs, not only estimated compiler time saved. Three runs establish behavior, not a reliable p95; collect a meaningful larger paired sample for percentile claims. Investigate routine warm CI above two minutes, but never meet the target by hiding checks, reducing platforms/features, setting arbitrary two-minute timeouts or moving required work out of sight.

Continue the loop: measure → identify the dominant remaining bottleneck → research alternatives → compare → implement → independently verify → remeasure. Do not chase tiny planner optimizations while installation, transfer, linking, scheduling or test fixtures dominate.

## Phase 5 — Distribute and verify every consumer

Publish a source-bound, digest-verified generator runtime through the supported release/bootstrap process. Regenerate consumers with that artifact; do not assume fixing source or reusing a version label changes their downloaded binary. Preserve all non-workflow `.github` content and existing validation/publishing/deployment obligations. Never hand-edit generated YAML as the final fix.

Review, commit, push and merge consumer changes in the required waves. For normal repositories verify the actual reviewed PR/integration revision and resulting default branch, including applicable CI/CD paths. Re-check after concurrent changes or generator upgrades. A successful workflow that skipped all substantive work is not sufficient.

Retain the earlier narrowly applicable ChainArgos-private merge policy only where live metadata and existing authority qualify it. Admin merge means the supported exact-reviewed-head administrative PR merge, never force-pushing or disabling protection. A CI waiver does not waive static/security review and never counts as hosted performance evidence. Do not force-enable or repeatedly dispatch private/production workflows solely to generate a benchmark. Mark unavailable hosted verification explicitly and exhaust safe authorized alternatives.

## Completion gate and final deliverables

Maintain and satisfy the specification's checklist. Deliver upstream commits/tests and generator artifact identity; complete per-repository workflow/PR evidence; cold/warm/changed-input benchmark artifacts; cache/selection/security regression results; and regenerated consumer PR/default-branch outcomes.

The final table must contain exactly 47 unique repositories and distinguish `PERF_VERIFIED`, `STATIC_ONLY`, `CI_WAIVED_PERF_UNVERIFIED`, `INACCESSIBLE`, and `INCOMPLETE`. Include actual SHAs, run/attempt/job URLs, cache-domain evidence, tool-download and compiler-reuse metrics, selected versus covered work, critical-path measurements and limitations. Never convert unknown telemetry to zero or a waiver to green.

Continue until all applicable requirements are verified and no meaningful authorized actionable work remains. Do not stop at recommendations, cached-looking YAML, a warm local directory, an unmerged PR, or an unsupported claim that CI is now instant.
