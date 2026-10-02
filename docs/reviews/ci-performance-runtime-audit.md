# Generator runtime and raw CI audit

Audit date: 2026-10-03. Baseline source: `c57c700459bbe1549fe7eedcb7d8689585c38986`. This records historical observations, not remediation qualification. No hosted experiment was dispatched by this audit.

## Evidence retained

Public raw artifacts are cached once in `/tmp/velnor-ci-runtime-audit`: `regression-run.json`, `regression-jobs.json`, `plan.log`, `contract.log`, `orchestrator.log`, `measurements.json`; pinned action sources and bundles; release metadata/manifest; recent generator PR diffs. Raw logs were fetched using `gh api --allow-escape-sequences repos/tailrocks/velnor-new/actions/jobs/<id>/logs` because GitHub CLI rejects escape sequences by default. Local cache is temporary; archive required before final deliverable.

Plan log SHA-256: `ea725c1e2a2790c3df1c0ddbf24bc196b548ed8a7530bfbb541a44e93621003c`. Contract log: `7a972da72a88383bd5e5c5565f06464cdd46b7d079037d842fa6c746fb0c0376`.

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

## Runtime identity

Live `v0.1.0` release metadata says target commit `95c1d6f0f1056881e42d53846dac8ffccd7aa6a5`; downloadable schema-1 manifest binds source `c57c700459bbe1549fe7eedcb7d8689585c38986`. Linux digest: `aa7e44d6579e9c586106d120ed3658fcf1c9b041027ad9f03473e8efacd3b5d5`; Darwin digest: `b6f514b71e3d1d72978c66cecf23560e7f25ad51727e9f26c88870b77d61695f`. The manifest/artifact binding must be qualified; release label/target metadata alone is inadequate. Both release binaries were downloaded once and independently hashed with `shasum -a 256`; both match the manifest and GitHub asset digests. This verifies bytes, not build provenance or successful current-source validation.

## Remaining qualification

Cold/warm/third fresh-runner experiments, changed-input and negative tests T01–T26, exact compiler process accounting, supported delta snapshots and action/compiler identity agreement remain **unverified**. Generator status remains `INCOMPLETE`. PR-diff review coverage is bounded and recorded separately; inventory references never count as full performance audit.

## Independent bounded PR audit

A separate reviewer read historical complete renderer cache/setup/election files, Mise source/transport cache files, PR #10 changed cache/runtime/selection/tool-set/staging files, all PR #3 diff, and PR #11 product-source changes. Large PR #1/#10 API diffs exceed GitHub's 300-file limit; their exact head objects were fetched read-only and relevant path diffs cached locally.

- PR #1 introduced independent definitions: built-in Mise restore, elected explicit saver of `~/.local/share/mise`, and separately inventoried owned runner-temp Rustup home. Inventory did not govern archive closure (`pr-1-relevant.diff`: 74132, 73991, 74465, 74774, 9793). This is architectural drift across payload ownership definitions.
- PR #10 extracted `cache_steps_tools.rs` but retained the same incomplete payload (`pr-10-relevant.diff`:17760); new provider election is independent (:17440, :17533).
- Selection retains all obligations without qualified evidence. Missing base, lock/root changes, unowned inputs or graph errors broaden. PR #10 adds Rust/OpenTofu input splitting and neutral dependency edges (:9117); this cannot be replaced by filename-only omission.
- Tool trimming already exists for generator suites (:8439, :8472); consumer validators remain conservative. Avoid claiming this existing optimization as new remediation.
- PR #1 historical transport uses assumed 20 MiB/s; local harness queue is `na`, compiler cost is metadata proxy and transfer is zero by harness construction (:3655, :73204). These are not hosted warm measurements.
- PR #3 proves Required includes upstream validators and excludes downstream publisher, with tests only. PR #11 product changes concern generated instructions/symlink tree, not cache/selection behavior.

Limits: this is **not complete whole-PR #1/#10 review**. Broader cache eligibility/trust/task-result internals, many tests/docs, full matrix changes, `select_tofu`, and PR review threads remain outside this bounded review. Large historical workflow output was structurally inspected rather than fully read. This explicit limit must remain in W0 until independently completed.
