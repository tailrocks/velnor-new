# PR #12 feature and file disposition

Snapshot: 2026-10-04. This is a passive review record against integration base
`f7d38268ce45c23216d9bbdb20184177fc9888ba`.

## Evidence boundary

- PR #12 is open at exact head [`c694d8029eb880db639fa89b0589090dd2b15364`](https://github.com/tailrocks/velnor-new/commit/c694d8029eb880db639fa89b0589090dd2b15364). Its recorded base is `c57c700459bbe1549fe7eedcb7d8689585c38986`; live `main` is `47815c83b9eeadbaf84b741918fffa7ea550da89`. Integration base `f7d38268...` descends from that live main.
- Refresh on 2026-10-04 returned the same PR head and base, with 252 changed paths across the paginated files endpoint; there is no new diff since this snapshot. GitHub reports only the DCO check complete/successful and no submitted reviews. No hosted qualification or consumer result is inferred. See [PR #12 Files changed](https://github.com/tailrocks/velnor-new/pull/12/files).
- `LANDED` means the named source fix is an ancestor of `f7d38268`; it does not mean runtime or rollout qualification. All missing or unresolved work stays `PARTIAL` or `TEMPORARY-HOLD`.
- Direct tree checks at `f7d38268` find the private Python tools, Foundation qualification, `velnor-actions-native`, `root_identity.rs`, and typed `matrix_suite.rs` absent. Keep current freshness, release, OCI, workflow, and cache code intact; this document changes none of it.

## Feature dispositions

| Feature | Status at `f7d38268` | File-level disposition / evidence |
|---|---|---|
| Self-gated crate obligations | LANDED | `crate_job.rs` and its tests; fix [`87bc5c8041fb066f407017daa664a29dd8bb194e`](https://github.com/tailrocks/velnor-new/commit/87bc5c8041fb066f407017daa664a29dd8bb194e). |
| Canonical release asset URL validation | LANDED | `crates/velnor-actions-contract/src/tooling.rs`; fix [`a97797e738df4e90b50142d7e44ae2b048ba659f`](https://github.com/tailrocks/velnor-new/commit/a97797e738df4e90b50142d7e44ae2b048ba659f). |
| Optional Git lock isolation | LANDED | `crates/velnor-actions-mise/src/command_git.rs`, command and Git tests; fix [`d32139e95d2a88795f065e7cf634ba49e8a9a39d`](https://github.com/tailrocks/velnor-new/commit/d32139e95d2a88795f065e7cf634ba49e8a9a39d). |
| Tools-only no-Rust bootstrap | LANDED | Generator/bootstrap workflow paths; fix [`dd995b5df61b1573cd75386ae558bfdb7062acb9`](https://github.com/tailrocks/velnor-new/commit/dd995b5df61b1573cd75386ae558bfdb7062acb9). |
| Attempt-bound artifact retrieval | LANDED | `retrieve_baseline.rs`, planned retrieval, retry fixtures/tests; base fix [`d20226ebc052af03199c4ed2f37a17ae48043520`](https://github.com/tailrocks/velnor-new/commit/d20226ebc052af03199c4ed2f37a17ae48043520). This does not close the separate provenance-carry hold below. |
| Full Git-ref validation | LANDED | `crates/velnor-actions-contract/src/vcs.rs`; fix [`f7d38268ce45c23216d9bbdb20184177fc9888ba`](https://github.com/tailrocks/velnor-new/commit/f7d38268ce45c23216d9bbdb20184177fc9888ba). |
| Branch shorthand grammar | PARTIAL | PR change [`6209c06d87c2f7e41ff162b77cc51e0c99989eec`](https://github.com/tailrocks/velnor-new/commit/6209c06d87c2f7e41ff162b77cc51e0c99989eec) updates `branch.rs`, `cover_baseline.rs`, and `internal_request.rs`; it is not in `f7d38268`. Full-ref validation above does not establish shorthand parity. |
| Provenance carry and closed source proof | PARTIAL | Retrieval is landed, but PR-only `provenance_lineage.rs`, `provenance_lineage_tests.rs`, provenance callers, and source proof capsule work are absent from the base. PR commits [`5280a5a1a7858e41842d77632e426b63ec9655b2`](https://github.com/tailrocks/velnor-new/commit/5280a5a1a7858e41842d77632e426b63ec9655b2) and [`1dcd0791e90abb0e5b758556f0746d0f061c994e`](https://github.com/tailrocks/velnor-new/commit/1dcd0791e90abb0e5b758556f0746d0f061c994e) remain candidates; no caller evidence is supplied here. |
| Tofu root identity and caller migration | TEMPORARY-HOLD | PR additions `root_identity.rs` and root-key caller/test updates from [`49441b888679ba08f6cbb3d0c33809958fd20f31`](https://github.com/tailrocks/velnor-new/commit/49441b888679ba08f6cbb3d0c33809958fd20f31) and [`5b4f9be958e97acb1ec8034ada252fc54747e9ac`](https://github.com/tailrocks/velnor-new/commit/5b4f9be958e97acb1ec8034ada252fc54747e9ac) are absent at the base. No caller migration is inferred. |
| Typed repository suite ownership | TEMPORARY-HOLD | PR `matrix_suite.rs`, tests, and crate-job/tool wiring from [`6c110a9050b7f8af0fef72b2be85ed24e762146c`](https://github.com/tailrocks/velnor-new/commit/6c110a9050b7f8af0fef72b2be85ed24e762146c) are absent at the base. |
| Nextest binary preparation | TEMPORARY-HOLD | PR Rust argv/workflow-tool tests from [`8a75f54847d636fe278d302310abce060e236120`](https://github.com/tailrocks/velnor-new/commit/8a75f54847d636fe278d302310abce060e236120) and regenerated `.github/workflows/ci.yml` from [`ce3c157a444e00db3148ca1885f533f91e700d1b`](https://github.com/tailrocks/velnor-new/commit/ce3c157a444e00db3148ca1885f533f91e700d1b) are not in the base. Keep the current workflow until matching source is integrated and regenerated. |
| `.github` output preservation | TEMPORARY-HOLD | PR `generate_preserve.rs` and its tests from [`f3c937acc2df90fada059f3f427f7157ce08461c`](https://github.com/tailrocks/velnor-new/commit/f3c937acc2df90fada059f3f427f7157ce08461c) are absent. Preserve the integration tree until this behavior and its complete ownership boundary are integrated. |
| Foundation executable closure | TEMPORARY-HOLD | PR CLI dispatch, orchestrator source/tests, renderer-owned workflow source/tests, and `.github/workflows/foundation-qualification.yml` from [`57dc370d8e84e6d1edc34a273a5ccc313cb03dd0`](https://github.com/tailrocks/velnor-new/commit/57dc370d8e84e6d1edc34a273a5ccc313cb03dd0) / [`d94fe9cefa6a0b97f00d235191c2d3142d1af6ff`](https://github.com/tailrocks/velnor-new/commit/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff) are absent. Source files and fixtures alone do not establish executable closure. |
| Owned-tool source, qualification, and publication scripts | TEMPORARY-HOLD | The 19 principal scripts, `owned_mise_qualification.py`, two support fixtures, and 14 script tests plus pinned requirements are listed below. They are absent from the base; source inspection or test fixtures do not qualify a caller or published tool. |
| Independent OCI / Ruby / shell / REUSE owners | TEMPORARY-HOLD | `crates/velnor-actions-native/**`, including OCI codecs/tests and Ruby, shell, and REUSE facades; native ownership commit [`92fee7465294af8f721319ccaec6d1308832d9df`](https://github.com/tailrocks/velnor-new/commit/92fee7465294af8f721319ccaec6d1308832d9df), OCI addition [`8cba870d41f00be145ff6ddb3420926078d85451`](https://github.com/tailrocks/velnor-new/commit/8cba870d41f00be145ff6ddb3420926078d85451). The crate is absent at the base; no native recipe adoption is inferred. |
| MBX external-bundle lifecycle | LANDED | The base already saves hosted MBX as one external bundle and restores it before the build in `mbx_bundle.rs` and `cache_steps.rs`; fixes [`9a249e9ab99a019ab7ba0e6f835dee406827db20`](https://github.com/tailrocks/velnor-new/commit/9a249e9ab99a019ab7ba0e6f835dee406827db20) and [`47815c83b9eeadbaf84b741918fffa7ea550da89`](https://github.com/tailrocks/velnor-new/commit/47815c83b9eeadbaf84b741918fffa7ea550da89) are in the base ancestry. Preserve this current lifecycle. |
| MBX parity / fixture evidence | TEMPORARY-HOLD | Separate candidate [`1ecd84d1f6320e9a0209968c2057952b9363c8a4`](https://github.com/tailrocks/velnor-new/commit/1ecd84d1f6320e9a0209968c2057952b9363c8a4) is not in PR #12 or `f7d38268`. Hold only reconciliation of its renderer and fixture parity evidence with the already-landed lifecycle; it is not the source of that current behavior. |

## Python source and test file dispositions

All paths in this section are absent at `f7d38268` and remain `TEMPORARY-HOLD`.

19 principal private source scripts:

```text
scripts/analyze-ci-performance.py
scripts/bootstrap-owned-tool-builder.py
scripts/build-owned-tool.py
scripts/collect-ci-performance.py
scripts/download-owned-tool-candidate.py
scripts/immutable_publication.py
scripts/owned_tool_behavior.py
scripts/owned_tool_qualification_evidence.py
scripts/owned_tool_source.py
scripts/publish-owned-tool-artifacts.py
scripts/publish_owned_source.py
scripts/qualify-owned-tool.py
scripts/source_proof_capsule.py
scripts/source_publication.py
scripts/source_publication_records.py
scripts/source_qualification_execution.py
scripts/source_release_policy.py
scripts/source_semver_capsule.py
scripts/stage-owned-tool-source.py
```

Separate qualification module and two support fixtures:

```text
scripts/owned_mise_qualification.py
scripts/owned_tool_execution_test_fixtures.py
scripts/owned_tool_publication_test_fixtures.py
```

14 script tests and pinned requirements:

```text
scripts/test_ci_performance_analysis.py
scripts/test_download_owned_tool_candidate.py
scripts/test_git_optional_locks.py
scripts/test_owned_tool_build.py
scripts/test_owned_tool_execution_evidence.py
scripts/test_owned_tool_qualification_evidence.py
scripts/test_publish_owned_tool_artifacts.py
scripts/test_qualify_owned_tool.py
scripts/test_source_proof_capsule.py
scripts/test_source_publication.py
scripts/test_source_publication_records.py
scripts/test_source_qualification_execution.py
scripts/test_source_release_transaction.py
scripts/test_source_semver_capsule.py
scripts/ci-performance-analysis-requirements.txt
```

## Remaining changed-path families

| Changed paths in PR #12 | Status / disposition |
|---|---|
| `.github/workflows/ci.yml`, `.github/workflows/foundation-qualification.yml`, `Cargo.toml`, `Cargo.lock` | `PARTIAL` / `TEMPORARY-HOLD`: workflow and dependency changes travel with the exact generator/feature that owns them; retain the base workflow and cache behavior meanwhile. |
| `.velnor/freshness-inventory.json`, `.velnor/version-policy.toml`, `scripts/check-freshness.sh` | `PARTIAL`: preserve the existing freshness/pin gates; PR edits are not evidence those gates pass on the integration source. |
| `crates/velnor-actions-mise/src/catalog.rs`, `catalog_versions.rs`, and catalog tests | `PARTIAL`: keep catalog/version changes with their pin and freshness evidence; no new tool version is adopted by this ledger. |
| `crates/velnor-actions-orchestrator/src/release_admission.py`, `release_source_snapshot.py`, `release_source_tree.py`; `crates/velnor-actions-rust/src/release_source_intent_guard.py` and tests | `TEMPORARY-HOLD`: PR-only embedded release guards and source readers; absent at the base and unqualified as callers. Keep with source-bound publication work. |
| `crates/test_support/**`, `crates/velnor-actions-cli/tests/fixtures/p12_*`, `crates/velnor-actions-*/tests/**`, `fixtures/parity/**` | `PARTIAL`: test helpers, P12 fixtures, and regenerated expected outputs remain attached to their held source changes; a golden update is not implementation proof. |
| `docs/proposed/{architecture.md,rust-quality-contract.md,workflow-contract.md,workflow-matrix-contract.md}` | `PARTIAL`: contract proposals only; they do not close implementation, caller, or acceptance obligations. |
| `docs/reviews/ci-performance-*`, `docs/reviews/ci-performance-nextest-evidence/**`, `docs/reviews/python-uv-upgrade-evidence.json`, `repositories.txt`, `repository-evidence.csv`, `scope.json`, `velnor-actions-ci-performance-{goal,spec}.md` | `PARTIAL`: retain as audit/evidence records. PR source records all 47 performance rows `INCOMPLETE`; no runtime, performance, archive, or consumer rollout is granted. |
| Remaining edits under `crates/velnor-actions-{cli,contract,mise,orchestrator,rust,tofu,workflow-renderer}/**` | Follow the feature rows above: landed base fixes stay; PR-only source, tests, and callers remain `PARTIAL` or `TEMPORARY-HOLD`. Exact paths are in the linked PR file list. |

## Separate archive and consumer holds

**Archive status: TEMPORARY-HOLD.** The local candidate at
`/private/tmp/velnor-owned-source-archive-candidate-20261004` is unpublished and
not durable. A live `ls-remote` on 2026-10-04 returned all eight owned-source
refs, including these six in-scope tips; none was deleted.

Each row is a Phase G hold owned by `@donbeave` through checkpoint
**2026-10-11** (seven calendar days from the 2026-10-04 start). For every row,
next event is: publish a fresh immutable archive tag, independently download,
checksum, and restore it in a clean repository, then recheck the current ref
and consumers and record the retained action-SHA plan. This depends on final
caller migration and release CI. Keep the archive `TEMPORARY-HOLD` until the
published bytes pass that proof; keep every ref until then.

| Ref | Tip SHA | Owner | Checkpoint | Status |
|---|---|---|---|---|
| `refs/heads/owned-source/cache-action/8758d976a1b25eb387f48aa04ea86f57739b84cf` | `8758d976a1b25eb387f48aa04ea86f57739b84cf` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |
| `refs/heads/owned-source/mbx-action/62ec0713473dffeab46884b7c03906042794e696` | `62ec0713473dffeab46884b7c03906042794e696` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |
| `refs/heads/owned-source/mbx-action/c3cbe8e56ccb4727624df45022357f49d2953075` | `c3cbe8e56ccb4727624df45022357f49d2953075` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |
| `refs/heads/owned-source/mbx/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |
| `refs/heads/owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` | `dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |
| `refs/heads/owned-source/semver-checker/583dddce84706786fc54c41a2c768c28a09c65fd` | `583dddce84706786fc54c41a2c768c28a09c65fd` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |

**Consumer status: PARTIAL.** The PR's source inventory and local changes do not
prove final caller migration, current consumer pins, release CI, or adoption.
Recheck current consumers and action SHAs after the migration and release CI;
do not infer adoption from the archive candidate or from PR source/test files.
