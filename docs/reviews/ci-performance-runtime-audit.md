# Generator runtime and raw CI audit

Audit date: 2026-10-03. Baseline source: `c57c700459bbe1549fe7eedcb7d8689585c38986`. This records historical observations, not remediation qualification. No hosted experiment was dispatched by this audit.

## Evidence retained

Durable private evidence is retained under `~/.codex-chainargos2/private/ci-performance/`. The frozen `evidence-2026-10-03/runtime/` archive contains 42 files: raw logs, measurements, pinned action sources/bundles, release metadata/binaries and historical diffs. Full main-attempt evidence is in `qualification/r37012391691-a1/`; exact PR-attempt evidence and expanded source/artifact audits are in `generator-source-map/`. `/tmp/velnor-ci-runtime-audit` is a temporary working copy. Raw logs were fetched using `gh api --allow-escape-sequences repos/tailrocks/velnor-new/actions/jobs/<id>/logs` because GitHub CLI rejects escape sequences by default. Private raw bytes remain outside this public checkout.

Plan log SHA-256: `ea725c1e2a2790c3df1c0ddbf24bc196b548ed8a7530bfbb541a44e93621003c`. Contract log: `7a972da72a88383bd5e5c5565f06464cdd46b7d079037d842fa6c746fb0c0376`.

## Complete retained attempts and artifact payloads

Both main `37012391691/a1` and PR `37075875245/a1` have 16 API jobs:
15 executed jobs with 15 complete retained logs, and an unexecuted baseline
publisher. All 30 executed logs are read and hash-bound; no executed log is
missing. The fully paginated freshness inventory contains zero runs. Its
schedule/manual workflow is source-audited without an inferred runtime result.

PR head is `6209c06d87c2f7e41ff162b77cc51e0c99989eec`; actual checkout/helper
source is integration `9927a4907c1b3aeb4cc2043df823c62cdcd80935`. Its tree equals
the committed head tree. Floating PR metadata subsequently advanced to
`92fee7465294af8f721319ccaec6d1308832d9df` and is retained separately.
PR Orchestrator Clippy fails at `retrieve_planned_baseline_tests.rs:51`; CLI
tests fail a golden comparison (expected 49,628 bytes, actual 49,720; first
difference 24,419). Required fails and publication is skipped.

Independent artifact review reproduces 22/22 ZIP API hashes/sizes and all
230 ZIP-to-extracted file comparisons. Each attempt has 114 parsed JSON files
and one helper binary: nine identical plan copies, nine identical matrix
copies, 47 matrix reports, 47 task reports, one final report and one helper
manifest. Task/report sets and source/run/attempt/helper hashes reconcile.

| Final payload | Selected | Executed | Failed | Blocked | Result |
|---|---:|---:|---:|---:|---|
| W0 main | 47 | 41 | 1 | 5 | failed |
| PR attempt | 47 | 40 | 2 | 5 | failed |

Baseline payload absence follows the skipped publisher; both plans report
`baseline_unavailable`. Task output arrays are empty; complete raw job logs
remain available. These are absent artifact telemetry, not download failures.
Outcomes and set equality do not establish semantic input validity.

Artifact evidence: private `generator-source-map/artifact-audit/`, including
immutable corrected `artifact-audit-92ee4c320b773985257fe0c3fe997f0e62db1109275a325d7dd300cea1e99460.json`,
`review/independent-review.{md,json}` and `review/correction-review.{md,json}`.
The original artifact report is retained; the correction changes only an
incorrect archive path and records its original digest.

## Real regression result

[Run 37012391691](https://github.com/tailrocks/velnor-new/actions/runs/37012391691), attempt 1, default-branch push, failed. Latest main run returned by live API remains this run; preceding success `37009657819` belongs to different source `d369a83d619a4c2e89b4c66d0488b5380d633bbb` and cannot qualify current source.

[Orchestrator job 110855475750](https://github.com/tailrocks/velnor-new/actions/runs/37012391691/job/110855475750) fails Format: `internal_request.rs:305` requires multiline formatting of `matches!(verdict.status, FinalStatus::Passed | FinalStatus::NoWork)`. Raw log lines 520–532 show diff and exit 1. Required failed; baseline publication skipped. Other successful jobs do not establish a complete passed baseline.

## Measured historical intervals

| Segment | Plan | Rust/contract |
|---|---:|---:|
| API started/completed job interval | 75 s | 59 s |
| First/last raw log interval | 65.634 s | 54.484 s |
| Pinned tool installation, log | 14.9 s | 13.2 s |
| MBX setup/restore, log | 11.3 s | 10.3 s |
| Cargo source archive transferred | 18,244,511 B | 18,244,511 B |
| MBX archive transferred | 86,442,519 B | 86,449,650 B |
| Helper release build | 16.47 s | N/A |
| Clippy | N/A | 8.77 s |
| Labelled test-executable build (`mbx build`) | N/A | 5.96 s |
| Nextest build preparation | N/A | 4.15 s |
| Nextest test execution | N/A | 0.345 s, 195 passed |
| Offline source check | about 1.8 s | about 1.6 s |
| MBX post export/save | about 4.2 s | skipped after exact hit |

Run started `13:20:57Z`; Plan API job start `13:21:03Z` is a 6 s scheduling/provision interval, not a separately instrumented queue measurement. Rust/contract API starts `13:22:20Z`, 2 s after Plan API completes. Required completes `13:24:31Z`, about 214 s after run start. API/log boundary differences remain explicit. Exact queue/provision separation, CPU count/load, compiler process CPU/time, linking/build-script time, tool payload bytes, Cargo-fresh unit counts and source-fetch bytes are **unknown**, not zero. No percentile claim is supported.

Runner raw log: runner `2.337.0`, Ubuntu `26.04.1`, image `ubuntu-26.04` version `20260927.149.1`, Azure westus. MBX `1.21.0`; installed Rust `1.98.1 (48a229cea 2026-09-01)`.

## Proven enabling conditions

1. Mise restore miss appears at Plan log 195 despite visible key matching save at 708. Pinned Mise action `9149ea85001c7435d5a66bb127d6a1b6227cb0a5` restores literal absolute `/home/runner/.local/share/mise`; generated save uses literal `~/.local/share/mise`. Both pinned bundled cache implementations hash unnormalized path strings, compression and salt `1.0`. `cache-version-reproduce.cjs` extracts and executes each bundled `getCacheVersion` function; both reproduce the mismatch. For zstd on Linux, versions are `979fe6608f470059673ecf40aa5ad3c22db7e3209cbf7a71e1e9107df8a847f2` versus `6111fbff76edd2452f371f8c5f3f5c8666f6f96cbeec8a92239a9004ad488450`. Visible key equality is insufficient. Save recompresses then fails reservation (Plan log 721).
2. Isolated Rustup/Cargo tool closure is outside saved Mise root. Both jobs install Rust again. Source-cache exact hit cannot supply missing tool installation. Warm tool-payload download elimination is unverified.
3. Plan restores prior MBX revision, imports 22 actions/312 objects (268.4 MiB), and restores 221 Cargo workspace files (261.1 MiB). These logical payload sizes overlap and are not additional transfer bytes. Plan exports 21 actions/309 objects to current revision key. Contract restores that same exact key, executes validation, then refuses export (contract log 1018). Later useful validation state is lost by immutable snapshot policy.
4. Clippy reports one hit, zero misses, **51 not looked up**, three `cc-missing-output` bypasses. Dev build reports 25 hits, zero misses, 13 not looked up. Nextest reports two not looked up; doctest/documentation each report a rustdoc bypass. Neither zero misses nor Cargo `Compiling` text identifies real compiler work.
5. Mise installs MBX; pinned action explicitly requests version `1.21.0`, skips its PATH probe, installs a second copy under `/opt/hostedtoolcache/mbx-.../1.21.0/x64/mbx`. Action compiler probe is ambient `rustc -vV`; task compiler is isolated Mise Rust. Matching identities were not directly recorded; the key's `rust-39d372ba6fa2` alone does not prove equivalence.
6. Source archive exact hit still reaches compression/save and reservation failure (Plan log 595). Fetch uses whole workspace offline metadata, which is not proof every selected target dependency exists.

## Supported MBX transport boundary

Pinned action source `9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3` exposes `cache-key`, `restore-keys`, `cache-generation`, `toolchain`, `working-directory`, `github-cache-mode`, version and dispatch-save controls. It imports with `mbx cache import <bundle>`; objects export uses `mbx cache export --group <producer UUID> --format directory <bundle>`. This already carries workspace state. No `skip-install`, executable-path, useful-delta/save-key input exists. Omitted version reuses PATH but can fall back to latest. Exact-hit post handler returns before exporting, irrespective of newly learned state.

Live upstream release `v1.6.0`, commit `1687e54eb349cadf61fa38b5813a77875489e8e6`, retains these ownership/exact-hit limitations. A pin upgrade alone does not solve C03–C05. Supported binary transport or a qualified upstream interface is required; no custom internal-format merge or second target cache is justified by this audit.

## Exact action and runtime protocol closure

The expanded private `generator-source-map/action-audit/` retains seven
immutable action trees, 86 Git-blob/hash-bound source files and bundles, and
all eight workflow `uses` references. Cache restore/save subpaths select
different pinned YAML/entry bundles. Alint uses its composite action and
pinned installer, rather than the retained Dockerfile. Runner and tool
versions, paths, gates, key families and per-job regions are read from all
30 complete logs.

Independent action review checks every retained file's Git blob/SHA-256 and
all 142 action-pin log markers, and reruns the exact bundled cache-version
function. Alint logs archive checksum `OK`; cosign is absent, so no signature
verification is observed.

Standalone `actions/cache/save` executes `save-only` with `NullStateProvider`;
it cannot inherit another restore step's matched-key state. Combined-action
exact-hit suppression must not be attributed to that standalone saver.
Main has eight executed standalone save groups: seven Mise, one Cargo source;
all eight reach reservation failure, with no successful-save line. Nine is
the static YAML reference count. PR's push gate permits zero save invocations.
MBX separately owns exact-hit state: nine main jobs use write mode, nine PR
jobs read mode. Main Plan exports current-key state; later exact-hit jobs
do not export. These counts are runtime observations, not qualification.

Mise, MBX and Alint installer/release digests remain expected identities;
hosted extracted executable hashes were not emitted or archived. Rust's
version is recorded without an executable hash. Action compiler probe and
isolated task compiler equality remains unproven. Missing measurements do
not become zero.

## Runtime identity

Archived `v0.1.0` release metadata says target commit `95c1d6f0f1056881e42d53846dac8ffccd7aa6a5`; downloadable schema-1 manifest binds source `c57c700459bbe1549fe7eedcb7d8689585c38986`. Linux digest: `aa7e44d6579e9c586106d120ed3658fcf1c9b041027ad9f03473e8efacd3b5d5`; Darwin digest: `b6f514b71e3d1d72978c66cecf23560e7f25ad51727e9f26c88870b77d61695f`. The manifest/artifact binding must be qualified; release label/target metadata alone is inadequate. Both release binaries were downloaded once and independently hashed with `shasum -a 256`; both match the manifest and GitHub asset digests. This verifies bytes, not build provenance or successful current-source validation.

Retained release metadata reports `immutable=false`. Retained read-only GitHub
attestation lookups for both binary digests returned HTTP 404 under the audit
credentials, recorded in private `action-audit/release-provenance-endpoint-probe.json`.
No signed source-build receipt was returned or retained. This is an explicit
provenance limitation; installer checks and byte hashes do not close it.

Separately reviewed current event (`9a164fb`): an authorized `make_latest:
"true"` PATCH restored `v0.1.0` as latest. The server also changed its
immutability flag from false to true and its update timestamp; the backend
mechanism is unknown. All 13 asset identities/digests across the three
releases remained unchanged. Historical W0 `immutable=false` bytes remain
untouched. This metadata event grants no source-build provenance or runtime
qualification. Private `release-metadata-event-9a164fb/` retains the complete
before/after metadata and independent effect receipt `8c3484f3…`; committed
facts are in [owned source publication](ci-performance-owned-source-publication.json).

## Remaining qualification

Cold/warm/third fresh-runner experiments, changed-input and negative tests T01–T26, exact compiler process accounting, supported delta snapshots and action/compiler identity agreement remain **unverified**. Generator status remains `INCOMPLETE`. PR-diff review coverage is bounded and recorded separately; inventory references never count as full performance audit.

## Independent relevant source and PR audit

Independent source review verifies complete immutable binary diffs for PRs
#1–#5 and #7–#12, all 12 before/after source archives and all 1,721 archived
file identities against Git blobs and trees. Numbered item #6 is an ordinary
issue, positively verified by the retained primary API response. Large PR
#1/#10 API limits are bypassed through exact local Git objects, without
substituting floating heads. PR #12 uses the explicit W0-to-6209 boundary.

The semantic supplement reads 101 exact complete files: 47 product source,
40 tests/fixtures and 14 documents. Cache eligibility/trust, task-result,
Rust/Tofu selection, baseline, Required, provenance and policy reading gaps
are covered. Its original category wording is corrected append-only by
`pr-history-audit/semantic-review/coverage-correction.{md,json}`.

The workflow supplement reads all 33 tracked YAML blobs across five source
points: 1,779,695 bytes and 29,146 lines, including ten root runtime workflows
and fixture/golden workflows. Every file parses; all 278 historical external
rows match their complete source execution fields with zero mismatches.
Events, dependencies, conditions, environments, commands and matrix/control
wiring are inspected completely. No tracked release workflow exists.

- PR #1 introduced independent definitions: built-in Mise restore, elected explicit saver of `~/.local/share/mise`, and separately inventoried owned runner-temp Rustup home. Inventory did not govern archive closure (`pr-1-relevant.diff`: 74132, 73991, 74465, 74774, 9793). This is architectural drift across payload ownership definitions.
- PR #10 extracted `cache_steps_tools.rs` but retained the same incomplete payload (`pr-10-relevant.diff`:17760); new provider election is independent (:17440, :17533).
- Selection retains all obligations without qualified evidence. Missing base, lock/root changes, unowned inputs or graph errors broaden. PR #10 adds Rust/OpenTofu input splitting and neutral dependency edges (:9117); this cannot be replaced by filename-only omission.
- Tool trimming already exists for generator suites (:8439, :8472); consumer validators remain conservative. Avoid claiming this existing optimization as new remediation.
- PR #1 historical transport uses assumed 20 MiB/s; local harness queue is `na`, compiler cost is metadata proxy and transfer is zero by harness construction (:3655, :73204). These are not hosted warm measurements.
- PR #3 proves Required includes upstream validators and excludes downstream publisher, with tests only. PR #11 product changes concern generated instructions/symlink tree, not cache/selection behavior.

The earlier relevant source-reading and full-workflow limits are now closed
by these exact manifests and independent supplements. Identity verification
of 1,721 rows does not claim semantic reading of every unrelated file or an
arbitrary repository-wide security review. Parent-owned PR comments, replies,
threads and final-head merge dispositions remain separate.

Static task-cache source renders an unsupported Mise `--file` shape; the
unqualified availability guard forces real execution, Required rejects
unproven reuse and publication refuses it. Supported task-definition delivery,
backend/output proof and replay qualification remain open. G14 actual base
inventory and G17 include/native/generated-input cases remain unqualified;
source edge union and `.rs` discovery do not establish those inventories.

## Finite generator W0 evidence closure

**Closed with explicit unavailable observations** for the frozen generator
source/runtime audit. All requested relevant source, executed logs and
published artifact payloads are retained and independently reviewed. No
downloadable executed log or published artifact remains unavailable.
Skipped baseline publication, zero freshness runs, empty task-output payloads,
unemitted executable/CPU/queue telemetry and attestation HTTP 404 responses
have explicit dispositions. Unknown measurements are not zero.

Immutable private closure: `generator-source-map/whole-w0-closure-0947e2e94a4ac1e54595c32d5a7f2a2308d1a0c5ee43ba29088b8ed12a1dd64f.json`.
Evidence manifest: `whole-w0-evidence-manifest-01dc3760ac94e32ebe469db8d5439e9bd7b4589775e3fc4f1711e2e770cce361.json`,
binding 716 files and 166,890,887 bytes. Original archive manifest checks
confirm all 42 frozen runtime and 21 qualification files remain unchanged.
Original native map `967dccf2…` and corrected immutable `24d2d538…` remain
resolvable; the corrected map is authoritative.

This closes the generator's finite W0 evidence row. Aggregate 47-repository
W0 closure, execution/rollout, semantic cache/task/Tofu qualification, later
`92fee746…` ninth-Rust-member/eight-lane reconciliation, source-build provenance
and controlled T01–T26 performance remain **OPEN/INCOMPLETE**. Both retained
Required attempts remain failed.
