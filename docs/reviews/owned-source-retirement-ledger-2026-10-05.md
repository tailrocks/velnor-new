# Owned-source retirement ledger

Status: PR #12 is open and unmerged at `0977a7ec88d8f61900ca0d5d800d4d5c991a365e`.
Its history contains the pushed owned-source/Foundation retirement commit
`4bf20ed69dcb456c311a7470a7407aea6375a0c3`, which removes 40 dedicated scripts:
the 25-path private candidate/performance/Git slice below and the coordinated
15-path source/publication closure. The newer MBX retirement commit
`0977a7ec88d8f61900ca0d5d800d4d5c991a365e` removes 29 dedicated Python suite
scripts, its README/manifest/source-owner contract, and three exclusive registry
fixture files. Across those two slices, 69 dedicated scripts are retired. The
MBX commit changes one generic fixture-exclusion regression to use a nested
Cargo fixture identity; it has 36 changed paths total (35 deletions and that
one test update). The exact diff received independent Sol review and approval
under patch SHA
`8936b2f8d456fea5cf5c9a244bd99ec506d331a313ad86362c667500a40329b8`. The
registered test
`repo_sample_excludes_nested_cargo_test_fixtures_before_admission` passed
(1 passed, 679 filtered); log SHA-256 is
`e0e59e561042e407868f504fb4a1f140aa35f62e8ee56b67a6c491d053dd0ce2`.

The retired-module/import scan for the owned-source slice is clean. Overall
source cleanup, cache/hosted/release qualification, external consumer migration,
and snapshot/branch/tag retirement remain PARTIAL. No branch or tag deletion is
authorized or claimed here. Historical publications, receipts, reports, and
their source identities remain passive evidence.

## Prior tracked-path inventory (historical)

At 2026-10-05T02:41:57Z, the read-only tree scan compared main
`7fb8367d7daa67f13ccaa7c76caae47d55d6262b`, PR #12
`8625f5d191bffc807f7b80a607ad1fc51cbc171b`, and the other 14 open PR heads.
Main and each of the other 14 heads had zero exact path hits for this list; all
25 paths were present on PR #12:

```text
scripts/analyze-ci-performance.py
scripts/collect-ci-performance.py
scripts/test_ci_performance_analysis.py
scripts/bootstrap-owned-tool-builder.py
scripts/build-owned-tool.py
scripts/download-owned-tool-candidate.py
scripts/immutable_publication.py
scripts/owned_mbx_observation.py
scripts/owned_tool_behavior.py
scripts/owned_tool_execution_test_fixtures.py
scripts/owned_tool_git_fixture.py
scripts/owned_tool_publication_test_fixtures.py
scripts/owned_tool_qualification_evidence.py
scripts/owned_tool_source.py
scripts/publish-owned-tool-artifacts.py
scripts/stage-owned-tool-source.py
scripts/test_download_owned_tool_candidate.py
scripts/test_owned_mbx_observation.py
scripts/test_owned_tool_build.py
scripts/test_owned_tool_execution_evidence.py
scripts/test_owned_tool_qualification_evidence.py
scripts/test_publish_owned_tool_artifacts.py
scripts/test_qualify_mbx_observation.py
scripts/test_qualify_owned_tool.py
scripts/test_git_optional_locks.py
```

Other open PR heads in that scan were: #57
`a0489267d1b79ada3b461482cfb02b1b61ee7f3c`; #56
`e41356bd5f24d524d0b0a28ecaca8c6bbf3dc98b`; #55
`2cb1b4ea5ffcb6c6fd54b25e78197f392bbfdae3`; #54
`6deba5a071dbd75682b0799c387a6777fc6075f3`; #53
`3fa5303cdbe6d27c79255bf612a7530892495392`; #48
`e835a9c6d8658cce90b3ca0b21d946140e351260`; #46
`6697928f27cb28f2e604407492e23f185f2c44ff`; #41
`5e91d51e1c88eccbf8ab0ce16a320ec02a5c5d8c`; #38
`5e34e432a4f6cea0d09ac3b55fb680ba722848b4`; #30
`fe0064cbb82d861653439faaf4a088186571dec6`; #29
`4ad1e34a1e20589386ddefdc25eaceb067341f0b`; #28
`e426a0f2f124c5820a193869a36f8d0b69c46f48`; #25
`3fc93a9ff9cb40dc130f0da9439b1885e0145f76`; #20
`f53081d700d18e5f42723d5237679c4bccf72a78`.

This is a tracked-path presence scan, not proof that no runtime can invoke a
script by another name or through an external repository. It records PR #12 at
`8625f5d…`; it is not a current main or all-open-PR inventory. The inventory
owner reported no Python invocation strings in its workflow scan, but the
reconstructed `.velnor/scope.json` has unknown provenance and the complete
direct/indirect caller scan across all 47 consumers is not certified. Treat
consumer closure as PARTIAL. The underlying scope hashes and limits are recorded
in `docs/reviews/ci-performance-scope-audit.md`; this ledger does not claim an
authoritative original scope provider or current all-ref closure.

## Foundation workflow retirement evidence

The test seed is the exact PR #12 candidate
`.github/workflows/foundation-qualification.yml` at `8625f5d…`, Git blob
`a29b61ff997ddb403d57a0457f83b932247c080c`, SHA-256
`c777fb6c4837da2fd38fe4ac68b5235b759fb628de68a22c6adc14153ba086a3`. The real
root generator no longer emits the Foundation managed workflow; only that
`.github` path was removed. Official OpenTofu golden `capture` then `check`
passed for all five cases.

The retired lifecycle regression compares generated output bytes over two runs
for both Velnor policy (`AGENTS`, `CLAUDE`, `actionlint`, `ci`, freshness) and
consumer policy (`AGENTS`, `CLAUDE`, `actionlint`, `ci`), checks plan/output
pins and path absence, and preserves
`.github/hand-maintained/controls.md` byte-for-byte. Focused Rust validation
passed: renderer 293 tests, CLI args 9, orchestrator `validation_failure` 10,
and `impl_orch_f2a_prepare` 1. `cargo fmt --all -- --check` and
`git diff --check` passed. The Python freshness wrapper was inspected but not
run because it invokes the interpreter.

## Internal callers and coordinated deletion boundaries

The retired performance analyzer dynamically loaded the collector; its tests
loaded the analyzer and stubbed GitHub API access. No current workflow, task, or
Rust call site was found. The adopted implementation goal marks the one-off tools
for deletion after exact import closure. Remove their obsolete manual commands
and dependency-install instructions, but keep the recorded failed-run timeline,
hashes, and T01–T26 pending status as passive evidence. Tool retirement does not
qualify any performance experiment.

The owned candidate pipeline is private and unactivated. The builder/bootstrap,
candidate downloader, observer, receipt, and publisher modules are linked by
dynamic imports and owned-only tests. The owned qualifier and source-proof,
publication, release-policy, and semver helpers/tests are in the coordinated
`source_closure_audit` deletion slice. In particular, downloader/evidence and
fixture modules import `source_qualification_execution`; the qualifier loads
the builder; the source-release transaction test reuses the source-publication
test fixture. These paths were removed together in the pushed import-closed
retirement commit; a targeted search across scripts, crates, workflows, and tasks found
no remaining retired-module/import references. Historical source-only releases,
receipts, asset IDs, hashes, and review records stay unchanged and passive.

The source-closure owner’s exact 15-path slice is `source_proof_capsule.py`,
`source_publication.py`, `source_publication_records.py`,
`source_qualification_execution.py`, `source_release_policy.py`,
`source_semver_capsule.py`, `qualify-owned-tool.py`,
`owned_mise_qualification.py`, `publish_owned_source.py`,
`test_source_proof_capsule.py`, `test_source_publication.py`,
`test_source_publication_records.py`, `test_source_qualification_execution.py`,
`test_source_release_transaction.py`, and `test_source_semver_capsule.py`, all
under `scripts/`. No branch or tag cleanup is part of this candidate.

`test_qualify_mbx_observation.py` reused the private qualifier test fixture and
owned source/receipt modules. It was removed with `test_qualify_owned_tool.py`
and the private observer closure. It is a root-level owned-candidate test, not
part of the separate 29-file MBX synchronous suite audited by the MBX owner; do
not cite that suite audit as its caller proof. The earlier exact-candidate scan
at `8625f5d…` found no workflow, task, or Rust invocation of this root test,
while the broader 47-consumer caller audit remains partial. Preserve ordinary
stock MBX task and runner/workload behavior independently of this private
candidate observer.

The separate MBX audit’s boundary was 29 Python suite sources plus its README,
manifest, and source-owner contract under
`scripts/qualification/mbx-synchronous/`, and three exclusive registry fixture
files under
`crates/velnor-actions-mise/tests/fixtures/mbx-synchronous/registry-fixture/`.
Commit `0977a7e` retired those paths while preserving the generic nested-Cargo
fixture-exclusion behavior in the registered regression test. It does not cover
the root-level test above. Stock MBX cache, setup, and workload behavior remain
preserved. The retained historical artifact-closure evidence does not qualify
MBX/T06 or hosted performance.

`test_git_optional_locks.py` imports the retired staging script, but its generic
Git policy remains covered by `crates/velnor-actions-mise/src/command_git.rs`
and `crates/velnor-actions-mise/tests/impl_mise_git_optional_locks.rs`. The
Phase 0 evidence records two focused Rust tests passing: hostile parent
`GIT_OPTIONAL_LOCKS=1` is overridden for status without refreshing the index,
while explicit add, commit, and clone writes still work. Keep that Rust behavior
and regression coverage.

The separate selected stock-Mise no-config regression is recorded in
`docs/reviews/mise-miserc-phase0-evidence-20261005.md`. Linux real-binary
qualification passed; actual macOS execution remains pending. Do not retain or
port owned candidate promotion, fake MBX, receipt, or source-capsule behavior as
part of that stock-Mise test.

## Evidence preservation and limits

The operational notes for retired tools now remove executable commands and
package-install recipes. The adopted read-only `gh`/GitHub API evidence export
remains available; no reusable collector is required. Keep source-bound
historical measurements, published source release records, receipts, review
inventories, hashes, and failed experiment outcomes unchanged. T01–T26 remain
pending until each row's evidence requirements are met; no archived report is
promoted by this cleanup.

The previously recorded consumer packet includes a default-branch indexed
search and a workflow scan, not a certified all-ref or complete
literal-pin/call-path audit.
The 47-consumer inventory was reconstructed and its provenance is UNKNOWN.
Source snapshot, consumer, branch, tag, and upstream-retention closure therefore
remain PARTIAL. This ledger makes no claim that external branches, tags,
snapshots, or published assets were deleted.

The only recorded full guard scan ran in a detached retirement candidate based
on PR #12 head `8625f5d191bffc807f7b80a607ad1fc51cbc171b`. It reported 42
findings and zero scan errors; its output was not saved as a separate log
artifact. That scan is not evidence from the current remote head. The 42
findings then comprised 29 MBX `.py` suite files, 11 freshness `.py` files, and
two shell launch sites; this is 42 findings, not 42 Python files. The MBX files
were retired in `0977a7e`. The 11-file freshness port closure is:

```text
scripts/check_freshness.py
scripts/freshness_context.py
scripts/freshness_dependencies.py
scripts/freshness_evidence.py
scripts/freshness_inventory.py
scripts/freshness_probe.py
scripts/freshness_probe_check.py
scripts/freshness_validation.py
scripts/test_freshness_context.py
scripts/test_freshness_probe.py
scripts/test_freshness_probe_check.py
```

At exact PR #12 head `0977a7ec88d8f61900ca0d5d800d4d5c991a365e`, the 11
freshness Python paths and the two shell launch sites in `check-freshness.sh`
and `verify-local.sh` remain. The freshness workflow calls `check-freshness.sh`;
`verify-local.sh` contains four inline interpreter helpers. Both shell
entrypoints remain pending their native port. A full guard scan was not run at
exact head `0977`; no passing result or mandatory wiring is claimed.

The original 100 tracked Python sources are accounted for as 40 owned-source
scripts deleted, 29 MBX suite scripts deleted, 11 freshness files pending a
native Rust port, and 20 unrelated passive Python sources preserved outside
these active paths. The exact `0977` tracked tree contains 31 `.py` paths: the
11 freshness paths listed above and 20 passive sources (13 OCI modules/tests
and 7 release-source/admission/intent modules/tests). This count describes
tracked paths only; the lexical scanner does not prove absence of arbitrary
dynamic launchers or close external consumer scope.
