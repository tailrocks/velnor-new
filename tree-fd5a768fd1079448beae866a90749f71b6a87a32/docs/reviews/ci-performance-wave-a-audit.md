# CI performance Wave A baseline audit

Observed 2026-10-03. All eight exact Wave A repositories remain `INCOMPLETE`; none is performance-qualified. This audit reads complete migration diffs, immutable current source/configuration/runtime/workflow families, active ruleset details, representative PR/default runs, every selected attempt and paginated jobs, raw logs and available plan/final/baseline artifacts. It establishes current defects, not a controlled cold/warm/third-run result.

## Source-bound inventory and measured default runs

All eight default branches are `main`, public, and contain only `.github/workflows/ci.yml`. Their committed runtime manifests declare generator source `c57c700459bbe1549fe7eedcb7d8689585c38986`, version `0.1.0`, Linux binary SHA256 `aa7e44d6579e9c586106d120ed3658fcf1c9b041027ad9f03473e8efacd3b5d5`, and macOS SHA256 `b6f514b71e3d1d72978c66cecf23560e7f25ad51727e9f26c88870b77d61695f`. The [runtime audit](ci-performance-runtime-audit.md) downloaded both binaries and checked their digests; the Wave A second reader rehashed those cached bytes. This proves artifact integrity, not build provenance or generator qualification.

Every run below is attempt 1 and concluded success. Span is earliest job start through last job completion, including dependency/scheduling gaps and baseline publication, excluding initial queue. It is not an independently measured pure execution critical path. Step durations use API second precision; rounded zero is not evidence of zero work.

| Repository | Current source SHA | Migration | Default run | Span s | Plan s | Tool install s | Status |
|---|---|---|---|---:|---:|---:|---|
| `jackin-project/jackin` | `6c389d38eadab93d6d6a4005e01dbdd8c4160221` | [1110](https://github.com/jackin-project/jackin/pull/1110) | [37015723857](https://github.com/jackin-project/jackin/actions/runs/37015723857) | 858 | 62 | 14 | INCOMPLETE |
| `jackin-project/jackin-agent-smith` | `2e7119b9c668ca7a9c55218b20049885299d198f` | [213](https://github.com/jackin-project/jackin-agent-smith/pull/213) | [37015752951](https://github.com/jackin-project/jackin-agent-smith/actions/runs/37015752951) | 49 | 26 | 13 | INCOMPLETE |
| `jackin-project/homebrew-tap` | `cd05a0ea2cf68fd6c2753ee938247b2dcd4c7551` | [505](https://github.com/jackin-project/homebrew-tap/pull/505) | [37015775295](https://github.com/jackin-project/homebrew-tap/actions/runs/37015775295) | 187 | 29 | 13 | INCOMPLETE |
| `jackin-project/jackin-role-action` | `59e538704b8c119f3b6668cea0154909a6158ff6` | [189](https://github.com/jackin-project/jackin-role-action/pull/189) | [37015799099](https://github.com/jackin-project/jackin-role-action/actions/runs/37015799099) | 135 | 24 | 13 | INCOMPLETE |
| `jackin-project/jackin-sentinel` | `587a0d1a8eef96108c9d9d530bdaa13df91edd2b` | [154](https://github.com/jackin-project/jackin-sentinel/pull/154) | [37015830848](https://github.com/jackin-project/jackin-sentinel/actions/runs/37015830848) | 169 | 25 | 14 | INCOMPLETE |
| `jackin-project/jackin-dev` | `a01b342162bc56cdf1e8bbaab793e73d31c1d621` | [48](https://github.com/jackin-project/jackin-dev/pull/48) | [37015858746](https://github.com/jackin-project/jackin-dev/actions/runs/37015858746) | 62 | 23 | 12 | INCOMPLETE |
| `jackin-project/jackin-github-terraform` | `b43a2314c6b58906d52828115d3b3973026f6a2a` | [48](https://github.com/jackin-project/jackin-github-terraform/pull/48) | [37015877749](https://github.com/jackin-project/jackin-github-terraform/actions/runs/37015877749) | 73 | 13 | 2 | INCOMPLETE |
| `jackin-project/jackin-the-architect` | `2cf461e2fed1b95d9fd1e7ba74c10d4d8b1c685d` | [478](https://github.com/jackin-project/jackin-the-architect/pull/478) | [37015906620](https://github.com/jackin-project/jackin-the-architect/actions/runs/37015906620) | 68 | 25 | 11 | INCOMPLETE |

## Correctness blockers before rollout

The generator can produce an empty obligation set for real unsupported workloads. Six non-Rust consumers now run only Actionlint, Plan, Required and baseline publication. Available plan/final artifacts prove empty package/obligation/matrix sets and `no_work`, with zero selected/executed/covered/reused tasks. Their old substantive checks are absent, rather than covered by valid baseline proofs. A green replacement workflow cannot qualify preserved correctness.

| Repository | Missing prior obligations |
|---|---|
| `jackin` | Fifteen prior validation units, including Bun, Docker, Swift and excluded Rust crates; desktop cadence, release/signing/attestation and Renovate workflows. Current configuration excludes native/docs/docker/vendor trees. |
| `jackin-agent-smith` | Explicit Dockerfile build validation. |
| `homebrew-tap` | `mise run check`: formula/cask Ruby syntax, shellcheck, updater fixtures and REUSE. |
| `jackin-role-action` | `mise run ci`: REUSE validation. |
| `jackin-sentinel` | Explicit Dockerfile build validation. |
| `jackin-dev` | `mise run ci`: REUSE validation. |
| `jackin-the-architect` | Explicit Dockerfile build validation. |
| `jackin-github-terraform` | Format/backend-disabled init/validate remain; audited-head generator/ruleset policy, nightly full validation/failure signaling and maintenance cleanup/cache-retention checks are absent. |

Migration diffs modify `.github/AGENTS.md` and `.github/actionlint.yaml`; they do not delete those files. They remove old `.github/ci/project.toml` and generator-state files. Preserve required policy content through an explicit before/after review; do not confuse modified policy files with deleted files.

The full deleted policy/nightly/maintenance bodies were subsequently read for all eight repositories. Prior nightly runs at 03:17 UTC dispatched full `ci-main` validation; their issue signal covered the explicit simulation-failure branch rather than monitoring the dispatched run's result. Prior maintenance at 03:31 UTC cleaned PR merge refs and applied budgeted cache retention with an active-producer guard; its sweep checked `CLOSED` and omitted `MERGED` PRs. Prior policy checked audited-head generator pins, ruleset-required contexts and Actionlint. Current workflows have no replacement for those schedules, cleanup/signaling or separate policy enforcement. These are known missing obligations and historical limitations; reviewed restoration must preserve intended safety and correct defective handling. They are implementation gates rather than unavailable audit evidence.

## Cache, compiler and selection observations

1. **Tools:** all eight default Plan logs explicitly report `mise cache not found`. Workflows restore through pinned Mise action `9149ea85001c7435d5a66bb127d6a1b6227cb0a5` and save separately with `~/.local/share/mise`. Isolated Rustup/Cargo tool roots remain outside that payload. Six tools-only consumers download six Rust components in Plan despite acquiring a prebuilt generator and selecting no compiler work. These are observed defects; canonical hidden cache-version diagnosis is bound to the separate pinned-source generator audit.
2. **Sources:** Jackin source payload includes Cargo executable metadata alongside registry/Git data. Its root-lock/Rust-version identity and whole-root fetch do not encode selected dependency closure. [Plan job 110865975545](https://github.com/jackin-project/jackin/actions/runs/37015723857/job/110865975545) takes 19 s fetching sources, 5 s saving sources and 2 s planning.
3. **MBX:** all 26 Jackin crate jobs use the same default SHA key. Collector raw logs show one writer saved cache ID `8419008888`, 18 siblings failed reservation and printed `Saved ... ID -1`, and seven later exact-hit jobs suppressed export. Independent source review confirms absent Velnor writer/domain identity. Supported useful-delta persistence still requires T03–T05/T17 experiments; collision counts are baseline observations, not proof that a fix works.
4. **Compilation versus tests:** [Rust/jackin job 110866395555](https://github.com/jackin-project/jackin/actions/runs/37015723857/job/110866395555) takes 746 s: MBX restore/setup 16 s, Nextest step 177 s, MBX post 158 s. Its 643 tests execute in 6.064 s. Clippy reports 0 hits/0 misses, 1,187 not looked up and 16 bypasses; build reports 383 hits/180 misses/485 not looked up/13 bypasses; Nextest reports 14/0/326/2. Zero misses does not mean zero compiler work. The MBX export's 13.1 GiB object payload is not compressed network transfer bytes.
5. **Scheduling:** Jackin has fixed crate jobs allocated before step-level coverage conditions. Actual main plan reports baseline unavailable and 154 executed obligations. “Build test executables” runs `mbx build`; Nextest performs its own preparation. The default run consumes 5,706 summed job seconds. Tools-only spans include large inter-job gaps; these are not all tool execution. Queue versus provisioning remains unknown.
6. **OpenTofu:** [Tofu job 110867265066](https://github.com/jackin-project/jackin-github-terraform/actions/runs/37015877749/job/110867265066) takes 15 s. It explicitly misses the provider cache, installs `integrations/github` 6.13.0 through backend-disabled init, validates, then saves the same canonical provider path/key. Task reports measure format/init/validate at 46/655/176 ms; the provider archive is 8,763,745 bytes and compression through save takes 1.695337 s. This is a cold observation, not warm provider reuse proof. Mutable state/credentials are excluded by the current source contract.
7. **Candidate identity:** tools-consumer PR checkout logs identify synthetic merge revisions while downloaded plan `head` identifies feature heads. Example tap checkout `95d6fec2740fee6e5226f5f536bf516d15e968cf` versus plan `9577d588b19fed0dd4ebf87999fe27c070954831`. Execution/proof identity must bind the actual integration candidate before selected-work qualification.

IaC collection also measures complete compressed Mise exports: sentinel and architect have approximately 54.46/62.97/63.47 MB across their three main jobs; Terraform has approximately 54.46/95.04/63.47 MB. These are separately saved tool payloads, not Rustup closure or warm avoided-download evidence. Exact raw byte counts and line references remain in per-repository JSON. Terraform artifacts show three selected/executed obligations and zero covered/reused; sentinel/architect have zero obligations and empty published baselines.

## Trust and evidence boundary

All eight active main rulesets require `DCO` and `Required`. Individual rule details were read; classic protection 404 is distinct from absent protection. Workflows restrict trusted saves to successful main pushes and PRs consume read-only state; these conditions alone do not qualify server cache provenance, failed-producer/report behavior or release isolation. All corresponding negative experiments remain required.

External check outcomes are separate from those Actions run conclusions. All three IaC-repository PRs have failing SonarCloud Security C quality gates; Terraform and Architect default-source gates fail while Sentinel is neutral. Their retained annotations flag workflow-global `actions: read` and request job scope. Those app checks are not required by the observed rulesets. No waiver, successful app outcome or proven exploit is inferred; generator permission scope needs independent security disposition.

Jackin's prior macOS release names environment `release-macos`; its live environment response has no protection rules or deployment-branch policy and permits admin bypass. Do not infer reviewer approval protection from the environment name. [Pinned signing source](https://github.com/jackin-project/jackin/blob/6c389d38eadab93d6d6a4005e01dbdd8c4160221/crates/jackin-xtask/src/desktop/sign_notarize.rs) also has existing verification gaps: missing/empty expected certificate/team variables bypass checks, and a failed entitlement extraction becomes an accepted empty result. Those are known source defects requiring hardening; they are neither release waivers nor permission to execute publication. This audit executes no signing, release or deployment.

Raw evidence is outside the public checkout at `/tmp/velnor-wave-a-audit/<repository-name>/`: `audit.json`, `notes.md`, complete migration diffs, immutable source archives, representative run/attempt/job JSON, raw logs and available artifacts. Independent review is `/tmp/velnor-wave-a-audit/independent-review.md`; the existing metadata inventory is `/tmp/velnor-ci-performance-scope-audit/`. Reuse these files instead of downloading unchanged logs again.

The bounded run windows are Sep30 onward for tools, Oct1 onward for IaC and Oct2 noon onward for canaries. All run-list pages in those windows and selected-job pages were collected. Canary totals are 17 and seven, both below one page. Logs are representative samples, not every historical run. Legacy same-day runs are labeled historical and used to establish removed obligations. Local CLI escape-sequence log guards were recovered safely; they were not access waivers.

## W0 closure ledger

The follow-up closes source/diff/log audit gaps at the frozen collection SHAs above. Fresh live-head comparisons equal each migration merge, so no later default-branch workflow revision exists through that snapshot. Full relevant predecessor/update/proposal diffs are read, including closed-unmerged proposals and Jackin advisory open work; proposal source is distinguished from deployed source. Before/after source includes native scripts, locks, policy configuration and literal symlink identities. Complete current representative PR/default attempts, every paginated job and full substantive logs are read, alongside historical workload/release/cadence samples and available plan/final/baseline payloads.

| Repository | Full relevant diffs | Selected full-log runs | Before/after/config/cache/trust/obligation audit | Unavailable existing telemetry | Overall |
|---|---:|---:|---|---|---|
| `jackin` | 7 | 8 | COMPLETE | Explicit JSON nulls | INCOMPLETE |
| `jackin-agent-smith` | 4 | 6 | COMPLETE | Explicit JSON nulls | INCOMPLETE |
| `homebrew-tap` | 9 | 6 | COMPLETE | Explicit JSON nulls | INCOMPLETE |
| `jackin-role-action` | 10 | 6 | COMPLETE | Explicit JSON nulls | INCOMPLETE |
| `jackin-sentinel` | 4 | 6 | COMPLETE | Explicit JSON nulls | INCOMPLETE |
| `jackin-dev` | 9 | 6 | COMPLETE | Explicit JSON nulls | INCOMPLETE |
| `jackin-github-terraform` | 4 | 6 | COMPLETE | Explicit JSON nulls | INCOMPLETE |
| `jackin-the-architect` | 4 | 6 | COMPLETE | Explicit JSON nulls | INCOMPLETE |

Per-repository `w0-closure.json` and `w0-closure-notes.md` bind each completed field to source/run/job identities and raw path/SHA256/size. Skipped jobs legitimately have no log; external app checks have check output/annotations rather than Actions logs. No substantive selected job log is unavailable. Historical Jackin release dispatch success covers an ad hoc build, with signing skipped; a canceled desktop sample is retained as canceled. Those observations cannot qualify signed release or desktop performance.

Independent closure review rehashes all 3,701 original durable Wave A files and 661 evidence-reference occurrences with no mismatch. The append-only private archive candidate `/tmp/velnor-wave-a-audit/w0-archive-delta.json` has SHA256 `5b93d16726ede7b2efe30bec10ff321ee3b1f77755e1ee6cbb2cee6fc407669e`: 446 new regular files / 21,730,761 bytes, zero changed or missing originals. Combined evidence is 4,147 regular files / 114,476,642 bytes; 18 literal symlinks are recorded without following them. Original archive/manifest remain intact. Durable delta insertion is separate bookkeeping; preservation does not qualify performance.

**Unavailable from retained historical telemetry:** process CPU/fresh/link/build-script/rustdoc accounting, complete tool payload bytes, queue-versus-provision split and resource/load measurements. Retrieving an additional run summary cannot recover unrecorded telemetry. **Actionable after generator qualification:** retained obligation restoration/hardening, source-bound approved runtime, fresh controlled T01–T26, complete conservative-versus-selected proof and reviewed wave rollout. These implementation/measurement tasks are distinct from completed bounded W0 reading; there is no proven access waiver for Wave A.

Compiler process/link/build-script/rustdoc time, Cargo-fresh counts, exact download bytes, complete cache transfer accounting, queue/provision split, CPU/load and useful export deltas remain unknown wherever telemetry is absent. Cold/warm/third unchanged runs, real input changes, security negatives, conservative obligation comparison, release qualification, regenerated reviewed heads and resulting default-branch runs remain outstanding. No missing measurement is zero, no prior green run is performance qualification, and no waiver applies to these eight public repositories.
