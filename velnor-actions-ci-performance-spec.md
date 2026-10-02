# Velnor Actions: reusable-cache and affected-work performance specification

**Research snapshot:** 3 October 2026.  
**Generator inspected:** `tailrocks/velnor-new` at `c57c700459bbe1549fe7eedcb7d8689585c38986`.  
**Scope:** the generator plus 46 consumers, listed in Appendix A and `scope.json`.  
**Status:** evidence-backed implementation specification. No repository changes, new CI runs, cache deletion, releases, or deployments were performed by this research.

## 1. Executive finding and evidence boundary

The immediate problem is not simply that generated YAML lacks caching. It already contains several cache layers, but important integration details defeat reuse. Prioritize the following, in order:

1. **Repair Mise restore/save identity and restore the complete isolated Rust installation.** The emitted restore and save use different path strings, and the Rustup home is outside the Mise directory being saved.
2. **Stop the Plan job's immutable MBX snapshot from starving downstream validation snapshots.** The observed Plan and Rust jobs share the same MBX primary key, although they populate different build configurations.
3. **Eliminate duplicated tool installation and expensive work before selection.** Use an already-verified Mise-managed MBX executable; do not allocate a Rust runner, restore a large cache, or download tools for an obligation that is completely covered.
4. **Preserve useful workspace state through MBX's supported transport; do not create a competing raw-target cache.** The observed MBX import already restores Cargo workspace state, not just compiler objects.
5. **Complete and verify sound affected-work selection and qualified reuse.** An object-cache hit is not evidence that tests passed. Selection must be based on the actual candidate, dependency graphs, complete inputs, and valid baseline proofs.

The research located related migration/update PRs and matching generated-cache workflow evidence for **45 of the 46 consumers**, with varying depth. Code-search snippets are not full workflow or PR-diff audits. Four consumer main-run summaries were inspected, plus a historical-only Java-monorepo run. The generator's current run, its jobs, and the full Plan and Rust/contract logs were examined in depth. **A complete job-by-job audit and controlled consecutive-run benchmark for every repository has not been performed.** Appendix A and the CSV deliberately preserve these gaps. The `ChainArgos/jackin-agent-brown` PR-list endpoint returned HTTP 404; this does not establish deletion, visibility, or migration status.

Sources [S01–S16] support the implementation findings. Consumer references and audit depth are in Appendix A. All performance targets below are proposed acceptance criteria, not measured improvements.

## 2. What the observed runs actually demonstrate

### 2.1 Generator Plan: about 66 seconds, with about 2.2 seconds of planning

The user's quoted 20-hit/two-miss build appears in Plan job `110855000716` of run `37012391691`, a default-branch push on 2 October 2026. Approximate intervals below are computed from that job's timestamped log, not claimed as independent benchmarks. [S01, S02]

| Segment | Observed approximate duration | Interpretation |
|---|---:|---|
| Whole Plan job, runner setup through cleanup | 65.6 s | Not just planner execution |
| Pinned tool installation | 14.9 s | Rust toolchain/components installed despite prior runs |
| MBX action setup and restore | 11.3 s | Includes action/tool preparation and workspace import |
| Helper release build | 16.7 s | 20 hits, two misses; substantial reuse already exists |
| Offline Cargo metadata/source check | 1.8 s | No evidence of source re-download in this step |
| Generated-tree verification | 0.8 s | Small compared with setup/build |
| Actual planning operation | 2.2 s | Optimizing graph computation alone will not remove the dominant costs |
| MBX post-job export/save | 4.2 s | Publishes the snapshot later consumed by Rust jobs |

Cargo source restore was an exact hit of approximately **18.2 MB compressed**. MBX restored approximately **86.4 MB compressed**, reported **268.4 MiB** of object payload, and restored **221 Cargo workspace files / 261.1 MiB**. These are different measurements and must not be summed as if all were independent network transfers. [S02]

### 2.2 Rust/contract: tests take 0.345 seconds, but the job takes about 54.5 seconds

Job `110855475688` in the same run shows: [S03]

| Segment | Observed approximate duration / result |
|---|---|
| Whole job | 54.5 s |
| Pinned tool installation | 13.2 s |
| MBX setup/restore | 10.3 s |
| Clippy | 8.9 s; one hit, zero misses, **51 not looked up**, three `cc-missing-output` bypasses |
| Step labelled “Build test executables” | About 6 s; actual command is `mbx build`, not a complete Nextest test-binary build |
| Nextest step | About 4.7 s including test-binary preparation |
| Test execution inside that step | **195 tests passed in 0.345 s** |
| MBX final save | Skipped because the exact cache key already existed |

The Plan job first saved the current-commit MBX key. Rust/contract then restored that **exact** key, produced additional Clippy/dev/test work, and did not save its new state. This is concrete evidence of an immutable-key/writer-domain integration problem. It does not prove every consumer suffers the identical planner collision: ordinary consumers acquire a released helper instead of building the generator's helper. Sibling consumer jobs may still collide with each other and must be tested. [S02, S03, S07–S09]

### 2.3 Interpret compiler output correctly

The presence of Cargo's `Compiling` lines is **not** the failure criterion. MBX can fulfill compiler work after Cargo has announced a compilation. The 20-hit/two-miss summary demonstrates reuse; its “1m 45s saved” figure is an estimate of summed compiler work, not measured end-to-end speedup. Likewise, **zero misses is not equivalent to no recompilation** when there are many “not looked up” or bypassed units. [S02, S03, S07–S09]

Measure these separately: Cargo-fresh units; MBX eligible hits/misses; units not looked up; unsupported/bypassed units and reasons; compiler process time; link/build-script/rustdoc time; test execution; GitHub archive bytes and restore/export time. MBX's zero network-byte counter does not include GitHub cache archive transfer.

### 2.4 Current correctness and consumer qualification

The inspected generator main run concluded **failure**: the orchestrator job failed at Format, Required failed, and baseline publication was skipped. Preserve that failure; never turn it green by relaxing aggregation. A failed run can demonstrate cache behavior but cannot qualify a successful validation baseline. [S01]

The inspected main-push summaries for `jackin`, `jackin-agent-smith`, `tui-snap`, and `tailrocks/github-terraform` concluded success. Those summaries are **not** controlled warm benchmarks or verification of all their CD paths. The Java-monorepo query returned an August 16 run, whereas migration PR #2081 merged October 2: that old green run is not post-migration evidence. See Appendix A and [S17–S21].

## 3. Root causes and implementation requirements

### C01 — Canonical cache path identity: highest priority

**Observed/static evidence.** The generated workflow sets `jdx/mise-action` to restore with `cache: true`, `install: false`, and `cache_save: false`. A separate `actions/cache/save` saves `~/.local/share/mise`. The pinned Mise action constructs an absolute home-directory path for its restore. GitHub's cache toolkit derives a hidden cache version from the path strings and compression settings; the save action passes its input paths into that API. The same visible key therefore does not establish the same cache identity. Logs show the consistent symptom: a Mise restore miss followed by a reservation failure for a cache already saved under that visible key. [S02–S06, S10]

**Required fix.** Introduce one typed canonical payload definition consumed by restore and save. Resolve absolute paths once; use the identical ordered/deduplicated path list, archive format, and compression implementation for both operations. Prefer an action-supported read-only/read-write split when it truly preserves identical behavior, or emit a supported explicit pair from a single definition. Do not maintain two separately spelled path lists.

Keep repository configuration/environment/hooks disabled on the isolated tool-install path. Simply enabling Mise action `install: true` to make its post-save run is not an acceptable fix if it also executes project-controlled configuration. Audit the exact pinned action behavior. The existing contract deliberately uses generator-catalog tools, not arbitrary repository tool definitions. [S04, S11]

Give the corrected payload a new schema/generation namespace rather than trying to overwrite old immutable entries. Add regression tests against the bundled/pinned cache implementation, plus real fresh-runner cold/warm runs. A YAML snapshot containing `cache: true` is insufficient proof.

### C02 — Complete tool installation closure, restored before installation

**Observed/static evidence.** Rust installation uses `MISE_RUSTUP_HOME=$RUNNER_TEMP/velnor/rustup` and `MISE_CARGO_HOME=$RUNNER_TEMP/velnor/cargo`; the saved Mise payload is its normal data directory. The Rustup installation is not covered by that directory. The Cargo source cache is restored later and is not a complete Rust toolchain cache. [S02–S04]

Define owned tool roots and ensure Mise data, required Rustup toolchains/components/targets, Rustup proxies, metadata, and any necessary symlink targets are restored **before** the idempotent install/verification step. Do not assume restoring a symlink recreates its destination. Ensure shell steps and action processes observe the same intended homes and toolchain identity.

Separate executable/tool state from Cargo registry/Git sources. The current source payload contains Cargo `bin` and `.crates*` metadata; audit this boundary and migrate tool-owned contents to the tool layer without losing required proxies. Never archive Cargo credentials, ambient global configuration, tokens, or unrelated packages. Do not overlap cache ownership.

Use a minimal Rust installation profile plus the explicitly required components/targets when supported by the selected pinned Mise/Rust integration. Do not blindly remove Clippy, rustfmt, source/target components required by a task, or replace the generator's pinned authority with an ambient hosted-runner toolchain. Derive actual supported options from pinned help/source. [S11, S22]

Keep `mise install` or an equivalent supported verification/fill operation after restore, but a complete compatible hit must result in **no tool payload download or reinstallation**. A missing component must be repaired and reported. A corrupted entry must fall back safely. Archive size and restore time must be measured: caching an unnecessarily huge toolchain can be slower than installing a small required subset.

### C03 — Separate MBX compatibility domains and immutable snapshot writers

**Observed evidence.** The pinned MBX action's default cache key incorporates OS, architecture, generation, toolchain, and revision. It does not encode Velnor's workspace/build-kind/writer-lane semantics. An exact hit causes the action to skip saving. The generator's helper-release build and downstream validation jobs can therefore share an incompatible *coverage domain* even when MBX correctly validates individual objects. [S02, S03, S07–S09]

The preferred initial design is a small number of **explicit build-domain snapshots**: generator-helper build, workspace validation/configuration cohorts, and genuinely different targets/profiles. Do not blindly create a full duplicate archive for every crate or a globally shared mutable target.

Each domain gets a compatibility prefix and an immutable snapshot suffix. Compatibility must cover repository/workspace, platform/ABI, actual compiler/toolchain/components, MBX format/generation, relevant build configuration, workspace/path-layout contract, and the stable writer domain. Snapshot identity must distinguish useful newer exported state. A commit SHA alone is not sufficient. Restore prefixes may relax snapshot identity only, never cross incompatible or untrusted domains. Keep semantic task-result keys stricter than source/object transport keys. [S11]

Use actual pinned MBX action inputs such as `cache-key`, `restore-keys`, and `cache-generation` where they meet the design. These are action inputs, **not** currently promised Velnor TOML keys. Extend typed Velnor configuration/IR and validation where necessary; do not patch generated YAML. If the action cannot safely publish a useful delta after a restore, improve the supported upstream integration or use its supported export/import API with a reviewed writer design. Never parse or merge undocumented MBX cache formats.

An exact hit may suppress export only when it is a complete compatible snapshot for that producer's required domain and there is no useful new state. If new useful state exists, write a new immutable snapshot rather than overwriting an existing key. Avoid unconditional run-ID cache churn: generate a new snapshot only on a useful export delta, with a documented budget and retention policy.

**Mandatory adversarial test:** Plan finishes first and saves a release-domain snapshot; a later Clippy/test job produces additional artifacts; a fresh next run must restore that validation work. Repeat with two sibling jobs finishing in the opposite order and with a retry of the same SHA. Confirm newly produced state is neither silently discarded nor trusted across PR/release boundaries.

### C04 — Mise installs MBX once; transport must not install it again

The pinned MBX action's setup code skips its PATH reuse probe when an explicit MBX version input is supplied. The observed workflow first installs MBX through Mise, then the action performs its own installation. This is avoidable ownership duplication. [S02, S03, S07]

Make the action consume a verified Mise-managed executable, or add a supported upstream transport-only/executable-path interface. Omitting the version input is acceptable only when preflight guarantees the exact expected MBX executable is visible and a failed lookup cannot fall back to an unpinned download. Do not invent a nonexistent `skip-install` input. Verify MBX path, digest/version, Rust compiler identity, owned homes, cache directory, and cache generation before transport and execution.

Check the action's ambient `rustc -vV` probe: it must resolve the same toolchain used by isolated task steps. This is an audit requirement, not a claim that the inspected job used a wrong compiler. Pass an explicitly verified supported toolchain identity or align action environment safely. Do not suppress security-sensitive identity checks to save milliseconds.

### C05 — Preserve MBX workspace/prediction state instead of layering raw `target/` caches

The observed objects-mode import restores both objects and Cargo workspace files. Therefore “add `target/` caching because MBX only caches objects” is the wrong diagnosis for this snapshot. [S02, S03, S08]

Investigate why restored state is not useful for Clippy/test profiles: wrong domain, missing prediction history, cold configuration, workspace/path relocation, fingerprints, feature unification, compiler flags, host/target differences, output availability, and legitimate changed inputs. Preserve supported MBX state across checkouts. Capture detailed miss/not-looked-up reasons with supported diagnostics.

Keep one mutable target/build directory per compatible sequential lane. Never run competing Cargo writers into it. Account for Cargo's actual target and intermediate-build directories, including any configured separate build directory. Do not run broad `cargo clean`, delete fingerprints, or unconditionally discard restored state. Do not falsify source mtimes or compiler fingerprints to suppress rebuilds. [S23, S24]

Do not add Swatinem build-target caching or sccache on top of MBX as a default fix. Compare alternatives only in isolated experiments when MBX has a demonstrated unsupported case; preserve the user's MBX-first requirement and Velnor's single-owner architecture. Unsupported work must execute normally with a visible reason, not silently skip validation.

### C06 — Save only useful new data; avoid repeated source work

The observed source restore hits, but the source save still compresses data and attempts to reserve an already-existing key. Make restore outputs available to the save decision and avoid unchanged exports. Preserve a save opportunity after a compatible-prefix hit that fetched genuinely missing sources. A healthy exact source hit must not recompress/re-upload identical data. [S02, S10]

Keep Cargo registry/Git sources separate from compiler artifacts. Key source archives by the actual source-input closure, registries/source replacements, and compatible source layout, rather than invalidating all downloaded source archives for irrelevant compiler/source-file changes. Use deterministic hashes of the detected roots/lockfiles; avoid a broad lockfile glob that also captures unrelated fixtures or generated scratch workspaces.

Measure archive extraction versus repeated crate unpacking. Whether to include extracted registry source trees is an experiment: compare archive bytes, small-file extraction, reuse, and trust. It is not automatically faster to archive everything in Cargo home.

Keep the existing offline-first source-availability check, but ensure it covers selected packages, features, targets, and nested workspaces. One lightweight metadata check is not proof that all selected target dependencies exist. Fetch only missing selected requirements using supported Cargo arguments. Online work in tests/build scripts must be separately declared; do not turn `--offline` into a blanket claim that no network was used.

### C07 — Select before runner allocation and expensive setup

`covered_tasks.rs` and `matrix_step.rs` already implement baseline-covered obligation step conditions. Preserve and extend this work; do not state that change selection is entirely absent. A step-level skip is not enough when a job still checks out, installs Rust, restores 86 MB, and downloads artifacts. [S12, S13]

Construct a typed execution plan with all obligations and their dispositions, then create only selected executable jobs, plus the minimal required aggregation/proof work. Use validated job-level predicates/dynamic matrices to omit fully covered lanes **before** provisioning and cache setup. Do not fetch all Cargo dependencies or MBX data in a plan-only job that only needs a verified prebuilt helper.

When coverage is missing, malformed, expired, incompatible, or not trustworthy, execute the affected obligations conservatively. A missing plan or unknown change set is not an empty successful plan. Distinguish `NoWork` with complete coverage evidence from “failed to determine work.” Keep required-check reporting reliable for docs-only and no-op changes.

Avoid global workflow `paths-ignore` filters that suppress the required gate or miss transitive inputs. Keep the workflow able to report a valid outcome even when no compiler job is needed. Preserve merge-queue triggers and evaluate actual integration candidates, not merely the feature-branch head.

### C08 — Build/test scheduling: fewer repeated setups, no semantic shortcuts

Compare per-crate jobs against **workspace/configuration cohorts**. Share sequential compatible compiler state within a runner, while keeping distinct formatting, Clippy, test-build, test-run, doctest, documentation, and security outcomes. Preserve fail propagation and per-obligation reports. Do not hide everything in an opaque background shell script.

`mbx build` does not automatically replace building integration/unit test executables. Label commands accurately and remove a separate build step only after proving its obligation is subsumed. Compare direct Nextest build/run with Nextest archive-once and test-only shards for expensive suites. Archive distribution is worthwhile only when avoided builds outweigh upload/download and runner startup costs. Tiny subsecond test suites do not benefit from many new runners. [S03, S25]

Validate the exact version's supported archive/filter/sharding interfaces. Nextest archive consumers need the correct source revision, fixtures, compatible runtime/platform, and captured dynamic-library/build-script outputs. Doctests remain a separate obligation where required; MBX caching does not remove rustdoc work by assertion. Do not equate Clippy/check/dev/test/release fingerprints.

Group only configurations whose combined Cargo invocation preserves the intended feature resolution, target selection, optional/development/build dependencies, and tests. A grouping change can alter feature unification. Add semantic equivalence tests before using it as an optimization.

The generator's older performance records identify expensive integration fixtures and contention-sensitive tests. Re-measure them at the current revision. Share immutable fixture preparation and prebuilt helper artifacts where safe; keep each test's mutable execution isolated. Do not remove security fixtures or slow tests to meet a budget. Separate performance experiments from normal validation only with explicit retained coverage and required execution on relevant changes. [S14]

### C09 — Generator bootstrap, artifact and report critical path

Consumers should acquire one pinned, digest-verified helper; do not build the generator from source in every consumer job. The generator's own dogfood pipeline must still test the source under review. Its preseed helper is currently built once in Plan and distributed as an artifact. Preserve source binding. Never validate new generator code solely with an old released binary. [S15]

Compare a cached current-source helper build, a lighter qualified build profile, and the supported lock-backed path. Choose by measured build/link/runtime trade-offs. Do not publish a binary with old bytes under a new commit identity. Stage and verify a current-run helper once per job; avoid redundant downloads/probes in every obligation.

Audit checkout depth, helper/plan/report artifact calls, polling, duplicate uploads, and report aggregation. Fetch sufficient Git history/base objects to compute the true candidate; replace unconditional full-history fetch only when shallow/missing-base fixtures prove correctness. Batch API reads and artifact downloads with bounded concurrency, immutable IDs, and exact run/attempt/source matching.

Do not re-upload downloaded plan/helper files in every task report unless a documented consumer requires them. Keep reports compact and fresh. An empty or missing report is not evidence of success. A failed task must remain visible even when later tasks/cache exports are skipped.

## 4. Required cache ownership and identity model

The following is a proposed design contract, not a claim that these configuration fields already exist. Reconcile it with Velnor's current typed contracts and version schemas where necessary. [S11]

| Layer | Owner | Payload and compatibility | Save/read rule |
|---|---|---|---|
| Verified bootstrap tools | Mise integration | Exact tool identities, installation options, required metadata and owned Rustup closure; canonical paths | Trusted producer saves only a useful complete delta; compatible consumers restore before install verification |
| Cargo sources | Velnor transport | Credential-free registry/Git data for required source/lock/registry closure | Compatible prefix may fill missing source data; no compiler outputs or arbitrary Cargo-home contents |
| Compiler objects and supported workspace state | Pinned MBX integration | MBX-supported bundle and format; explicit workspace/build-domain identity | Single stable writer domain per snapshot; immutable useful progress; no custom format merging |
| Same-run helper/test artifacts | GitHub run artifacts | Exact source/run/attempt, platform, configuration and digest | Data-dependency transfer, not proof that tests ran successfully |
| Qualified task results | Mise task cache | Complete declared inputs, outputs and deterministic task contract | Disabled until qualification; no duplicate Velnor task-cache engine |
| Trusted validation baseline | Velnor proof contract | Immutable successful protected-run evidence for exact task/input identities | PRs may consume proven coverage; failed/untrusted runs cannot publish it |

### 4.1 Canonical fields

Cache descriptors must carry at least layer, schema/generation, repository identity, relevant workspace/domain identity, OS/host architecture/target/ABI, actual toolchain and components, format identity, canonical payload paths, execution environment compatibility, snapshot identity, restore prefixes, producer, and read/write policy.

Do not apply every field indiscriminately to every layer. Source archives need source compatibility, not full compiler semantics. Tool archives must not churn for ordinary source commits. Object transport may retrieve compatible historical objects, but MBX still validates object inputs. Exact task results require the complete semantic digest. Trust is a permission/namespace boundary rather than a shortcut hash proving provenance.

Preserve the existing contract's runner-image identity unless an independently tested compatibility class replaces it through an explicit schema change. Do not silently cross Linux distribution, libc, Xcode/SDK, compiler, flags, feature, target, profile, or linker boundaries.

A proposed generated cache key family is conceptually:

```text
velnor-<schema>-<layer>-<trust-domain>-<compatibility-digest>-<snapshot-digest>
```

Use the actual current contract's canonical encoding and size constraints; this notation is not ready-to-paste YAML. Full keys and a human-readable explanation belong in reports. Broad fallback prefixes must never cross compatibility or trust boundaries.

### 4.2 Cache completion is not just a key match

A source hit can still lack a newly selected dependency. A tool hit can lack a Rustup symlink target. An MBX exact hit can cover a release build but not the later test workload. A task-result hit can be missing required outputs. Validate each layer using its owning tool and typed contract.

Do not create a second manifest that pretends to validate MBX internals. Velnor can bind domain/format/source/producer metadata and ask MBX to validate its bundle. Mise remains responsible for tool/task semantics. Restoration failure is an explained cold fallback, not a forged success or automatic correctness failure.

### 4.3 Non-Rust and container workloads: apply the same ownership rules

These are workload-specific implementation requirements, not claims that the current pipelines in every consumer have been inspected. Inventory the actual build system and apply only the relevant cache layers. Never install Rust or restore MBX solely because a repository uses the Velnor generator.

**Java/Gradle/Maven.** Keep tool installation, dependency downloads, and task-output reuse distinct. For Gradle, use its native task/build-cache semantics with complete inputs and outputs, and qualify relocation across fresh checkouts; absolute paths and undeclared inputs can prevent reuse or make it unsound. Give the native build tool ownership of its outputs rather than archiving the same mutable state through multiple independent systems. Measure configuration/setup versus compilation and test execution separately. Detect Maven instead of assuming Gradle from the repository name, then research and qualify its actual supported dependency/output reuse. Exclude credential-bearing settings and private authentication data from every archive. [S29]

**TypeScript/JavaScript.** Cache the actual selected package manager's supported download/content store, keyed by its relevant lockfiles and compatibility. A populated store does not prove the installation or build is complete: retain the required frozen/locked install verification and project tests. Preserve workspace dependency selection and native-addon/platform constraints. Mise can remain the tool installer; a second Node setup action is not required merely to copy a lockfile-aware caching pattern. Compare store restoration with more extensive installation-state reuse only through measured, reproducible experiments. [S30]

**OpenTofu.** Restore a checksum/lockfile-qualified provider cache before initialization; preserve separate per-root mutable data directories, backend-disabled validation and the existing module closure. Do not archive infrastructure state, credentials, or backend configuration as a performance shortcut. Provider payload reuse does not guarantee zero registry metadata requests: OpenTofu may still query available providers during initialization. Bound cache growth. Verify concurrency behavior against the actual OpenTofu version and filesystem; current documentation describes best-effort locking, so neither guaranteed safety nor a blanket claim that concurrent use is unsupported is sufficient. Never benchmark through apply/destroy/state mutation. [S31]

**Containers.** Treat Docker/BuildKit layer cache, build cache mounts, host Cargo sources and MBX transport as distinct owners and payloads. A host MBX hit does not prove that a compiler inside a container has the same cache. GitHub-backed BuildKit layer export does not, by itself, persist cache-mount contents. Qualify the selected transport and build-context/platform scope; compare inline/minimal versus richer registry/GitHub exports using restore bytes, build time and storage cost. Preserve secret mounts and release provenance, and do not add duplicate publishers or unreviewed privileged actions. [S32]

For skills, action, tap and APT repositories, prioritize the minimal verified tool set and their real validation/package contracts. No-op planning must not replace substantive checks. Record each non-Rust layer in the same cache-owner, hit/miss, bytes, useful-delta and affected-input reports used for Rust.

## 5. Trust and security constraints

Caches contain executable state and may be readable by pull requests; do not put secrets or private-source artifacts where unauthorized readers can access them. Cache keys alone are not authentication. Trust decisions must use event, repository/ref, token permissions, protected workflow provenance, and artifact identity. [S16]

Keep the existing default: PR/merge-group consumption is read-only for trusted namespaces; only qualified protected default-branch producers publish trusted state. Do not remove the write restriction to make sequential PR runs appear warmer. A PR-scoped cache is a separate policy proposal, not implicit authorization to promote PR binaries into main or release caches. Compare (a) the current read-only policy and (b) server-isolated, unprivileged PR-only writes for repeat runs of that same PR. Option (b) requires an explicit versioned contract decision, independent security review, exact scope enforcement and proof that trusted/main/release jobs cannot consume the result as trusted state. Do not rely on a key prefix alone for isolation. Fork handling, credentials and permitted cache readers need their own tests. Preserve the safe default until this qualification succeeds. Explain the performance consequence when a PR's new dependency cannot be saved under current policy; never promise universal sequential warmth under that restriction.

Current GitHub documentation describes workflow/job `cache-mode` controls and event-scoped cache permissions. Verify support against the pinned runner/action/validator versions before generating them. Setting `ACTIONS_CACHE_MODE` in a shell or action environment is not a substitute for a server-enforced trust policy. Never enable low-trust writes to default-branch scope merely for performance. [S16]

Release jobs must retain the current stricter policy: do not adopt PR-built executable/MBX/task-result caches for publication. Preserve clean or explicitly approved source-only release paths, signing, approvals, and immutable artifact provenance. Validate release packaging with safe dry-runs or existing authorized flows; do not create arbitrary tags, publish versions, or deploy production solely to benchmark.

Save only after the actual producer has succeeded and emitted valid reports. No trusted save after failure/cancellation, no detached asynchronous cache writes, and no bypass of required gates. Do not give repository build scripts cache/write/OIDC credentials unnecessarily. Keep secrets stripped from untrusted execution without breaking the separately scoped transport operation.

Do not globally delete caches, raise paid storage limits, purchase remote-cache services, rotate credentials, or alter organization-wide protection. Use an isolated benchmark cache generation to obtain controlled cold runs. Never delete other teams' warm data.

## 6. Sound affected-work selection

### 6.1 Algorithm and proof obligations

Start from the actual tested commit and exact event base: pull-request integration candidate, merge-group candidate, or push `before`/`after`. Fetch missing objects safely. Build the relevant base **and** candidate inventories, then union the effects of added/removed/renamed packages and dependency edges. Use Cargo package IDs plus manifests, not package names alone. [S26]

Include normal, build, development, optional, feature-selected and target-specific edges. Propagate production/build changes through reverse dependents; narrow test-only propagation only after fixtures demonstrate it is safe. Include build scripts, proc macros, native code, schemas, generated sources, test fixtures, included Markdown, submodules and environment inputs. Cycles must terminate deterministically. A graph failure or unclassified path broadens work.

For every required obligation, compute its full semantic identity and decide: execute; reuse a qualified task result; or cover using validated baseline proof. Record a reason and dependency path. A cache hit at one layer cannot suppress another obligation. Do not skip a test because its executable was cached.

Use Velnor's current authoritative execution model: compiled tool catalog and isolated Mise invocation. Repository `mise.toml`, `mise.lock`, or `rust-toolchain.toml` changes are not automatically compiler inputs when execution ignores them; they become inputs wherever a real task consumes them. Never ignore an actually consumed configuration file. [S11, S26]

Keep global workflow/generator/security-policy checks when their inputs change. A documentation-only change can skip compiler jobs only when it is not an included source, doctest, generated artifact, package metadata, or fixture. Lockfile changes may be narrowed to changed package closures only with verified resolution/feature behavior; otherwise broaden.

### 6.2 Baseline acquisition

Read immutable baseline evidence for the correct base/source/workflow/run/attempt and validate its provenance and complete task/input identity. Do not pick the latest successful unrelated run as the base. Retain original proof identity when carrying coverage forward. Missing or failed baseline publication must cause honest execution, not bypass. [S26]

The inspected generator run failed and did not publish a successful baseline; this can legitimately broaden later work. First diagnose missing proof before calling every executed crate a selector bug. Also avoid a permanent bootstrap cycle where ordinary successful main runs never create reusable evidence.

Qualify reuse of deterministic local tests through the existing Mise task-result mechanism only after declared-input/output and replay tests pass. Clock-dependent, random, live-network/service, release, deployment and notification tasks remain non-reusable unless the relevant nondeterminism is actually controlled and represented. No cached console output masquerading as a fresh execution report.

### 6.3 Job graph and gate

Omit fully covered executable jobs, not merely their shell commands. Produce a minimal current plan/gate report referencing proofs. When jobs are omitted, the final gate must distinguish deliberate valid coverage from accidentally missing jobs. Dynamic matrices must be bound to the validated plan and cannot discover/filter packages independently.

A missing shard, canceled selected job, failed prerequisite, malformed report, changed workflow identity or invalid proof must not yield green. Handle empty matrices, base-branch updates, rebases/merges, reruns, fork PRs and merge queues explicitly. Keep one stable required gate without cyclical dependencies on downstream baseline publication.

## 7. Measurement protocol and acceptance

### 7.1 Baseline collection for all 47 repositories

Before implementation, complete the missing evidence in Appendix A. Discover the actual default branch and current workflow family; do not hard-code main as proof of default-branch identity. Read each related migration/update PR's complete relevant diff, generated workflow files, config/runtime manifest, required checks, recent representative PR runs and default-branch runs. Include release/CD workflows and preserved legacy obligations, not only `ci.yml`.

For a bounded recent window, collect all pages of runs/jobs/attempts. Record missing/expired logs honestly. Resolve synthetic PR merge SHAs versus PR head and merge/default-branch SHAs. Collect job/step timestamps, statuses, cache keys/versions, paths, hit/miss reasons, bytes, executable identities and dependency selection. Avoid repeatedly fetching unchanged large logs. Keep private evidence outside public repositories.

Do not use workflow `updated_at - created_at` as exact execution time. Derive observed wall intervals from job timestamps/logs; separate queue, inter-job scheduling, provision, execute, cache transfer and cleanup. Capture runner image/CPU resources and simultaneous load. Run metadata alone cannot establish warm cache behavior.

### 7.2 Controlled experiments

Use fresh hosted runners and the same immutable code/configuration for each unchanged comparison. Re-running inside one process or on one warm filesystem is not enough. Do not use meaningless file touches or empty commits as the only changed-input test. Do not dispatch publishing/deployment workflows for experiments.

| ID | Experiment | Required observation |
|---|---|---|
| T01 | Cold run in an isolated new cache namespace | All required work succeeds; cold downloads/compiles measured; no global deletion |
| T02 | Fresh-runner unchanged run after T01 completes | Complete tool closure restored; zero tool-payload re-download; reusable eligible dependencies avoid real compilation |
| T03 | Third fresh-runner unchanged run | Work produced late in T01/T02 remains reusable; no exact-key starvation or redundant unchanged export |
| T04 | Plan-release producer followed by validation producer | Separate useful snapshots; validation artifacts/prediction state survive |
| T05 | Sibling writers finish in reversed orders | No lost useful state, namespace collision, or shared mutable-target race |
| T06 | Real leaf source edit | Only leaf and justified reverse-dependent obligations run; unaffected eligible dependencies remain reusable |
| T07 | Shared crate/proc-macro/build-script edit | Correct reverse closure selected, even when files outside `src/` change |
| T08 | Lockfile dependency version/content change | New dependency fetched/built; compatible unchanged dependencies reused; no stale semantic result |
| T09 | Features, target, profile, flags, linker/SDK or compiler change | Correct compatibility invalidation; no false hit |
| T10 | Added/deleted/renamed package or dependency edge | Both base/head graphs considered; no dropped obligations |
| T11 | Truly irrelevant documentation edit | No Rust/compiler cache setup; valid minimal gate and declared doc checks |
| T12 | Included Markdown/schema/fixture/native input edit | Dependent work executes; no blanket docs exclusion |
| T13 | Missing/corrupt cache or unavailable cache service | Safe real work; clear reason; cache save failure does not falsify task outcome |
| T14 | Failed/canceled producer and missing report/shard | No trusted save/baseline; final gate fails appropriately |
| T15 | Fork/PR/merge-group trust boundary | Allowed trusted reads only; no promotion or namespace escape |
| T16 | Base advances / candidate changes / PR rerun | Exact candidate and proof identity revalidated |
| T17 | Existing exact key plus new useful domain state | New immutable valid snapshot or explicit supported strategy; never silently discard the work |
| T18 | Missing Rustup target/proxy or damaged symlink | Detected/repaired; tool-cache hit alone is not accepted |
| T19 | Generator/tool/action/cache-schema update | Necessary regeneration/invalidation; unchanged source archives not needlessly churned |
| T20 | Cargo source hit with additional selected target dependency | Missing dependency fetched safely; offline failure not hidden |
| T21 | Same source via different workspace paths / Cargo build-dir | Correct supported relocation/fingerprint behavior; no mtime spoofing |
| T22 | Qualified task-result replay | Complete outputs/proof; new report says reused, not executed |
| T23 | Nextest artifact execution, where beneficial | Exact source/runtime/fixtures; tests really execute and fail on injected failure |
| T24 | Full-validation comparison on candidate | Required obligation coverage matches conservative plan; zero false-negative selection |
| T25 | Non-Rust / tools-only consumer | No irrelevant Rust, MBX, Cargo or compiler archives installed/restored |
| T26 | Release qualification | Approved trust boundary intact; no accidental publish/deploy or PR executable reuse |

A compiler miss is allowed for genuinely changed or incompatible work. The strict objective is **zero avoidable eligible third-party compilation** in a complete compatible warm configuration, not zero appearances of the word `Compiling`.

### 7.3 Statistics and performance budgets

Three runs establish cold/warm/persistence behavior, not a stable p95. Use paired baseline/candidate samples on the same runner class; collect a meaningful larger sample (initial target at least 20 comparable observations for percentile reporting), disclose sample size, outliers, failures, queue effects and confidence limitations. Preserve raw evidence so a verifier can recompute the numbers.

Proposed starting budgets, subject to explicit measured workload floors:

| Work | Proposed engineering target |
|---|---|
| Planner computation on provisioned input | Around 2 s or less for representative fixtures; report large-repo scaling separately |
| Fully covered/no-compiler path | Minimal runner work, initially target <=20 s excluding queue; no Rust/toolchain/MBX setup |
| Warm ordinary PR | Target <=120 s end-to-end excluding external queue; investigate every exceedance |
| Warm tiny leaf validation | Aim for tens of seconds including setup, not dozens of jobs for subsecond tests |
| Tool payload reuse | Zero redundant payload downloads on a complete compatible hit |
| Third-party compiler reuse | Zero avoidable eligible actual compiles; explicit counts for bypass/not-looked-up/changed work |
| Cache export | Zero unchanged snapshot upload attempts; useful state survives subsequent runs |
| Correctness | No dropped obligations, false-negative selection, stale-result success or trust regression |

“Almost instant” cannot mean eliminating required test execution, legitimate rebuilds, hosted-runner queueing, or cache eviction. Measure the actual floor and eliminate avoidable overhead. Do not enforce the target with a two-minute timeout, hidden retries, disabled checks, reduced features or omitted platforms.

For each layer compare `restore + validate/import + save/export` against work avoided. If transport costs more than recomputation for a tiny domain, consolidate or redesign it; do not blindly maximize cache size or runner count. Optimize end-to-end critical path and billed runner time together, with peak memory/disk/network and cache footprint as secondary constraints.

## 8. Reporting contract

Extend existing Velnor reports, rather than add another telemetry/control-plane project. Report at run/job/obligation/layer granularity:

- source/candidate/base SHAs, generator source and binary digest, workflow/run/attempt and runner identity;
- selected/covered/reused/executed/failed disposition with dependency path or proof reference;
- cache owner, compatibility/domain/snapshot, visible key, actual restored key, payload path/version inputs and read/write decision;
- tool availability and install/download duration; source-fetch bytes/time;
- MBX hits/misses/not-looked-up/bypasses, Cargo fresh units, real compiler/link/build-script/rustdoc and test durations;
- archive bytes, compression/import/export durations, useful delta and reason for skipping save;
- queue/critical path/total runner time and correctness status.

Do not print credentials, token-bearing URLs, private registry credentials, or source contents into public reports. Keep private repository evidence in the appropriate private workspace. Missing telemetry is unknown, not zero. Report an MBX cache hit separately from an entire task result hit and a baseline-covered obligation.

## 9. Implementation workstreams and source map

Paths below were inspected or discovered at the research baseline. Re-locate functions if the repository has moved; do not patch blindly against stale line numbers. [S04, S11–S15, S26]

| Workstream | Primary areas | Required output |
|---|---|---|
| W0: evidence and correctness baseline | Existing reports/performance records; consumer PRs/runs; `scope.json` | Complete audit ledger, current failing checks understood, reproducible measurements |
| W1: canonical tool/source caches | `velnor-actions-workflow-renderer/src/cache_p08.rs`, `cache_steps_tools.rs`, cache election; `velnor-actions-mise/src/cache_sources.rs` and tool homes | Same restore/save payload identity, complete tool closure, no unchanged saves |
| W2: MBX domain and transport integration | `velnor-actions-orchestrator/src/attach.rs`, Mise MBX catalog; pinned `jdx/mr-boxington-action` integration | Scoped compatible snapshots, no duplicate install, supported workspace-state reuse, late-writer persistence |
| W3: early affected selection | `covered_tasks.rs`, `matrix_step.rs`, task graph/baseline logic and workflow matrix rendering | Job-level omission, complete reasons and proofs, sound base/head reverse closure |
| W4: scheduling and fixture performance | Rust obligations, matrix tools, Nextest integration, costly integration fixtures | Accurate test-build contract, measured cohorts/build-once where beneficial, no lost checks |
| W5: helper/artifact/report path | `attach.rs`, staged-helper verification, report collection/aggregation, CLI internals | Reduced critical path and API calls, exact provenance preserved |
| W6: regression/security/rollout | Contract types, cache/renderer/golden/integration tests; all 46 consumers | Independently verified generated output and fresh-runner warm evidence |

Existing tests that merely assert “Save Mise tools” appears or that a setup input exists must be supplemented by behavioral tests. Keep golden generation deterministic, but do not confuse golden agreement with a working external cache protocol.

Implement reusable fixes upstream in `velnor-new`, not bespoke consumer YAML. No raw-shell/YAML escape hatch, second cache server, second task engine, arbitrary template passthrough or old Velnor runner deployment. Preserve typed, stack-generic separation and existing repository ownership rules for `.github`.

## 10. Rollout, acceptance, and permissions

First prove fixes in generator fixtures and its own hosted CI, then use `jackin-project/jackin` as the first consumer canary. Continue **jackin-project → tailrocks → ChainArgos**. Within each wave, parallelize independent repositories without concurrent writers to the same checkout/index. Retain the original 46-repository scope; generator is the additional 47th target.

Publish a verified consumable runtime through the supported official release/bootstrap mechanism, bound to exact source and binary digests. A source patch alone does not fix consumers downloading an older binary. Version `0.1.0` alone is insufficient identity: inspected migration records reference different source commits under that version. Regenerate consumers using the qualified artifact, preserving non-workflow `.github` content and all CI/CD obligations.

When another generator fix changes the baseline, regenerate and requalify affected earlier consumers. Freeze the tested final baseline for closure rather than chasing unrelated upstream commits indefinitely. Review the latest PR head and verify the resulting default-branch source separately.

The earlier migration's admin-merge waiver is **not performance evidence**. For live-confirmed ChainArgos-private repositories, preserve any explicitly applicable merge waiver without broadening authority, disabling protection, or claiming their CI is green. Inspect existing logs when available and run safe local/static qualification. Do not force cloud execution, production deployment or permission changes merely to manufacture performance proof. Mark unmeasured hosted performance as such; it remains a visible qualification limit.

Use separate final statuses: `PERF_VERIFIED`, `STATIC_ONLY`, `CI_WAIVED_PERF_UNVERIFIED`, `INACCESSIBLE`, and `INCOMPLETE`. A merged PR, unchanged YAML, or a successful unrelated historical run is not `PERF_VERIFIED`. Rust-specific criteria may be genuinely N/A for non-Rust repositories, but tool/selection/gate behavior still needs validation.

Complete the goal only with all applicable acceptance criteria satisfied, or an honest exact inventory of irreducible authorization/access limits after useful authorized work is exhausted. Do not invent measurements, claim asynchronous follow-up, or conceal gaps behind “all green.”

## Appendix A. Repository evidence inventory

**Legend:** `indexed_matching_step` means code-search evidence of the generated save step at the referenced snapshot, not full-file review. `header_directly_read` means the relevant cache header/setup section was fetched, not every workflow. Related PRs are navigation starting points discovered in this research; complete diff review is still a W0 requirement. `not_read` means no main-run assertion is made. No repository has a controlled warm benchmark from this research.

Generator: [`tailrocks/velnor-new`](https://github.com/tailrocks/velnor-new), source `c57c700459bbe1549fe7eedcb7d8689585c38986`; code/contracts/actions and selected full job logs examined. Run `37012391691` failed; performance remediation not implemented.

| # | Repository | Related migration/update PR | Workflow evidence | Run evidence |
|---:|---|---|---|---|
| 1 | `jackin-project/jackin` | [1110](https://github.com/jackin-project/jackin/pull/1110) | header_directly_read | [success](https://github.com/jackin-project/jackin/actions/runs/37015723857) |
| 2 | `jackin-project/jackin-agent-smith` | [213](https://github.com/jackin-project/jackin-agent-smith/pull/213) | indexed_matching_step | [success](https://github.com/jackin-project/jackin-agent-smith/actions/runs/37015752951) |
| 3 | `jackin-project/homebrew-tap` | [505](https://github.com/jackin-project/homebrew-tap/pull/505) | indexed_matching_step | Not read |
| 4 | `jackin-project/jackin-role-action` | [189](https://github.com/jackin-project/jackin-role-action/pull/189) | indexed_matching_step | Not read |
| 5 | `jackin-project/jackin-sentinel` | [154](https://github.com/jackin-project/jackin-sentinel/pull/154) | header_directly_read | Not read |
| 6 | `jackin-project/jackin-dev` | [48](https://github.com/jackin-project/jackin-dev/pull/48) | indexed_matching_step | Not read |
| 7 | `jackin-project/jackin-github-terraform` | [48](https://github.com/jackin-project/jackin-github-terraform/pull/48) | header_directly_read | Not read |
| 8 | `jackin-project/jackin-the-architect` | [478](https://github.com/jackin-project/jackin-the-architect/pull/478) | indexed_matching_step | Not read |
| 9 | `tailrocks/github-terraform` | [38](https://github.com/tailrocks/github-terraform/pull/38) | indexed_matching_step | [success](https://github.com/tailrocks/github-terraform/actions/runs/37016486887) |
| 10 | `tailrocks/termpane` | [31](https://github.com/tailrocks/termpane/pull/31) | indexed_matching_step | Not read |
| 11 | `tailrocks/tui-snap` | [10](https://github.com/tailrocks/tui-snap/pull/10) | indexed_matching_step | [success](https://github.com/tailrocks/tui-snap/actions/runs/37017332474) |
| 12 | `tailrocks/velnor` | [1136](https://github.com/tailrocks/velnor/pull/1136) | indexed_matching_step | Not read |
| 13 | `tailrocks/termrock` | [72](https://github.com/tailrocks/termrock/pull/72) | indexed_matching_step | Not read |
| 14 | `tailrocks/parallax` | [125](https://github.com/tailrocks/parallax/pull/125) | indexed_matching_step | Not read |
| 15 | `tailrocks/terminal-components-claude` | [13](https://github.com/tailrocks/terminal-components-claude/pull/13) | indexed_matching_step | Not read |
| 16 | `tailrocks/tailrocks-repository-skills` | [13](https://github.com/tailrocks/tailrocks-repository-skills/pull/13) | indexed_matching_step | Not read |
| 17 | `tailrocks/tailrocks-skills` | [119](https://github.com/tailrocks/tailrocks-skills/pull/119) | indexed_matching_step | Not read |
| 18 | `tailrocks/tailrocks-pull-request-skills` | [4](https://github.com/tailrocks/tailrocks-pull-request-skills/pull/4) | indexed_matching_step | Not read |
| 19 | `tailrocks/homebrew-velnor` | [8](https://github.com/tailrocks/homebrew-velnor/pull/8) | indexed_matching_step | Not read |
| 20 | `tailrocks/velnor-apt` | [249](https://github.com/tailrocks/velnor-apt/pull/249) | indexed_matching_step | Not read |
| 21 | `tailrocks/parallax-telemetry-playground` | [54](https://github.com/tailrocks/parallax-telemetry-playground/pull/54) | indexed_matching_step | Not read |
| 22 | `tailrocks/velnor-actions-fixture` | [172](https://github.com/tailrocks/velnor-actions-fixture/pull/172) | indexed_matching_step | Not read |
| 23 | `tailrocks/holla` | [226](https://github.com/tailrocks/holla/pull/226) | indexed_matching_step | Not read |
| 24 | `tailrocks/tracing-request-level` | [36](https://github.com/tailrocks/tracing-request-level/pull/36) | indexed_matching_step | Not read |
| 25 | `tailrocks/pg-bigdecimal` | [34](https://github.com/tailrocks/pg-bigdecimal/pull/34) | indexed_matching_step | Not read |
| 26 | `tailrocks/ruxel` | [52](https://github.com/tailrocks/ruxel/pull/52) | indexed_matching_step | Not read |
| 27 | `tailrocks/schemalane` | [42](https://github.com/tailrocks/schemalane/pull/42) | indexed_matching_step | Not read |
| 28 | `tailrocks/holla-apt` | [100](https://github.com/tailrocks/holla-apt/pull/100) | indexed_matching_step | Not read |
| 29 | `tailrocks/homebrew-parallax` | [125](https://github.com/tailrocks/homebrew-parallax/pull/125) | indexed_matching_step | Not read |
| 30 | `tailrocks/homebrew-ruxel` | [39](https://github.com/tailrocks/homebrew-ruxel/pull/39) | indexed_matching_step | Not read |
| 31 | `tailrocks/homebrew-tablerock` | [51](https://github.com/tailrocks/homebrew-tablerock/pull/51) | indexed_matching_step | Not read |
| 32 | `tailrocks/homebrew-holla` | [160](https://github.com/tailrocks/homebrew-holla/pull/160) | indexed_matching_step | Not read |
| 33 | `tailrocks/tablerock` | [84](https://github.com/tailrocks/tablerock/pull/84) | indexed_matching_step | Not read |
| 34 | `tailrocks/cloudflare-tofu` | [23](https://github.com/tailrocks/cloudflare-tofu/pull/23) | indexed_matching_step | Not read |
| 35 | `tailrocks/tailrocks-typescript-skills` | [2](https://github.com/tailrocks/tailrocks-typescript-skills/pull/2) | indexed_matching_step | Not read |
| 36 | `tailrocks/tailrocks-skill-authoring-skills` | [2](https://github.com/tailrocks/tailrocks-skill-authoring-skills/pull/2) | indexed_matching_step | Not read |
| 37 | `tailrocks/tailrocks-rust-skills` | [2](https://github.com/tailrocks/tailrocks-rust-skills/pull/2) | indexed_matching_step | Not read |
| 38 | `tailrocks/tailrocks-roadmap-skills` | [2](https://github.com/tailrocks/tailrocks-roadmap-skills/pull/2) | indexed_matching_step | Not read |
| 39 | `tailrocks/tailrocks-open-source-skills` | [2](https://github.com/tailrocks/tailrocks-open-source-skills/pull/2) | indexed_matching_step | Not read |
| 40 | `tailrocks/tailrocks-macos-skills` | [2](https://github.com/tailrocks/tailrocks-macos-skills/pull/2) | indexed_matching_step | Not read |
| 41 | `tailrocks/tailrocks-code-quality-skills` | [2](https://github.com/tailrocks/tailrocks-code-quality-skills/pull/2) | indexed_matching_step | Not read |
| 42 | `ChainArgos/blockchain-nodes` | [729](https://github.com/ChainArgos/blockchain-nodes/pull/729) | indexed_matching_step | Not read |
| 43 | `ChainArgos/java-monorepo` | [2081](https://github.com/ChainArgos/java-monorepo/pull/2081) | header_directly_read | [success_historical_only](https://github.com/ChainArgos/java-monorepo/actions/runs/31958623199) |
| 44 | `ChainArgos/jackin-agent-brown` | Unresolved: 404 | not_observed | Not read |
| 45 | `ChainArgos/cloudflare-tofu` | [6](https://github.com/ChainArgos/cloudflare-tofu/pull/6) | indexed_matching_step | Not read |
| 46 | `ChainArgos/github-terraform` | [14](https://github.com/ChainArgos/github-terraform/pull/14) | indexed_matching_step | Not read |

## Appendix B. Primary sources and reproducible references

URLs below are evidence links, not floating runtime installation instructions. Source-code references are pinned where inspected. Public documentation may advance; verify the pinned tool's actual capabilities before implementation.

- **S01 — Inspected generator run and outcome:** https://github.com/tailrocks/velnor-new/actions/runs/37012391691
- **S02 — Full Plan log, source of setup/cache/build timings:** https://github.com/tailrocks/velnor-new/actions/runs/37012391691/job/110855000716
- **S03 — Full Rust/contract log, exact-hit starvation and test timings:** https://github.com/tailrocks/velnor-new/actions/runs/37012391691/job/110855475688
- **S04 — Emitted generator workflow:** https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/.github/workflows/ci.yml
- **S05 — Pinned Mise action; absolute-path restore and setup/save behavior:** https://github.com/jdx/mise-action/blob/9149ea85001c7435d5a66bb127d6a1b6227cb0a5/src/index.ts
- **S06 — GitHub toolkit cache-version implementation (`getCacheVersion`; current source inspected):** https://github.com/actions/toolkit/blob/main/packages/cache/src/internal/cacheUtils.ts . Inspected file blob: `e19e320f88478732415be3f7ac14bdb86333bed1`. Also verify the exact toolkit bundled in the pinned actions during regression reproduction; do not rely solely on future `main`.
- **S07 — Pinned MBX action setup/restore/post logic:** https://github.com/jdx/mr-boxington-action/blob/9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3/src/index.ts
- **S08 — Pinned MBX action key construction:** https://github.com/jdx/mr-boxington-action/blob/9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3/src/lib.ts
- **S09 — Pinned MBX action supported inputs:** https://github.com/jdx/mr-boxington-action/blob/9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3/action.yml
- **S10 — Pinned GitHub save action input handling:** https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/src/saveImpl.ts
- **S11 — Existing Velnor cache/report contract (proposed contract, not proof of implementation):** https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/docs/proposed/cache-contract.md
- **S12 — Existing covered-task skip logic:** https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/crates/velnor-actions-orchestrator/src/covered_tasks.rs
- **S13 — Obligation environment/report/step conditions:** https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/crates/velnor-actions-orchestrator/src/matrix_step.rs
- **S14 — Historical performance records with qualifications; not a new benchmark:** https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/docs/implemented/performance.md
- **S15 — Helper staging/bootstrap/MBX attachment:** https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/crates/velnor-actions-orchestrator/src/attach.rs
- **S16 — GitHub cache identity, scope, immutability and cache permissions:** https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching
- **S17 — Jackin migration main run:** https://github.com/jackin-project/jackin/actions/runs/37015723857
- **S18 — Agent Smith migration main run:** https://github.com/jackin-project/jackin-agent-smith/actions/runs/37015752951
- **S19 — tui-snap migration main run:** https://github.com/tailrocks/tui-snap/actions/runs/37017332474
- **S20 — Tailrocks IaC runtime-update main run:** https://github.com/tailrocks/github-terraform/actions/runs/37016486887
- **S21 — Java-monorepo migration and stale historical run:** https://github.com/ChainArgos/java-monorepo/pull/2081 ; https://github.com/ChainArgos/java-monorepo/actions/runs/31958623199
- **S22 — Mise Rust backend and CI install/cache guidance:** https://mise.jdx.dev/lang/rust.html ; https://mise.jdx.dev/continuous-integration.html . Generic project-config examples do not override Velnor's isolated catalog policy.
- **S23 — Cargo build-cache layout/configuration:** https://doc.rust-lang.org/cargo/reference/build-cache.html
- **S24 — Cargo environment and wrapper/build-directory controls:** https://doc.rust-lang.org/cargo/reference/environment-variables.html
- **S25 — Nextest build archives and partitioning:** https://www.nexte.st/docs/ci-features/archiving/ ; https://www.nexte.st/docs/ci-features/partitioning/
- **S26 — Existing affected-work/baseline contract (proposed):** https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/docs/proposed/parallelism-and-selection-contract.md
- **S27 — Current cache construction/shape checks:** https://github.com/tailrocks/velnor-new/blob/c57c700459bbe1549fe7eedcb7d8689585c38986/crates/velnor-actions-workflow-renderer/src/cache_p08.rs
- **S28 — Inspected Jackin configuration and generated cache header:** https://github.com/jackin-project/jackin/blob/6c389d38eadab93d6d6a4005e01dbdd8c4160221/.velnor/config.toml ; https://github.com/jackin-project/jackin/blob/6c389d38eadab93d6d6a4005e01dbdd8c4160221/.github/workflows/ci.yml

- **S29 — Gradle native task/build cache and complete input/output requirements:** https://docs.gradle.org/current/userguide/build_cache.html
- **S30 — Node package-manager cache integration and lockfile-aware monorepo example (reference pattern, not a required installer):** https://github.com/actions/setup-node/blob/main/README.md
- **S31 — OpenTofu provider cache behavior, metadata checks and version-dependent concurrency:** https://opentofu.org/docs/cli/config/config-file/
- **S32 — Docker/BuildKit CI cache exporters and cache-mount persistence limits:** https://docs.docker.com/build/ci/github-actions/cache/

## Appendix C. Deliverable checklist

- [ ] W0: all 47 repositories classified with full relevant PR/workflow/run evidence or explicit access limitation.
- [ ] C01–C02: real consecutive-run Mise/Rust restore verified, including canonical hidden-version inputs and complete tool closure.
- [ ] C03–C05: MBX domain/snapshot/prediction reuse verified; later job work persists; no duplicate tool owner or raw-target cache.
- [ ] C06: useful-only source/tool exports and selected offline/fetch behavior verified.
- [ ] C07 and Section 6: job-level affected selection, complete baseline proofs and final-gate correctness verified.
- [ ] C08–C09: measured scheduling/helper/report improvements; no required task or release obligation removed.
- [ ] T01–T26: applicable experiments and negative tests passed with raw evidence.
- [ ] Security review independently confirms permissions, artifact/cache provenance, failure handling and release isolation.
- [ ] Generator's actual default branch green; exact source-bound runtime distributed.
- [ ] All applicable consumers regenerated, reviewed, merged and validated in wave order.
- [ ] Final 47-row report distinguishes verified performance, static-only work, waived CI and inaccessible/unresolved targets.
