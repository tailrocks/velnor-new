# Generator publication qualification

- State: source fix merged to `main` by PR #113 at
  `993050b379ed1b190315d0c2aaa272fd76080c9b`. The exact PR head
  `537f90e297cae61eb9df65b70d2039cdace3dc5e` received independent Ready
  review and a passing required check. The protected v0.1.5 release and
  consumer adoption have not been run.
- Dispatch state: v0.1.5 dispatch and its actual hosted outcome remain
  pending; no v0.1.5 release dispatch, publication, or consumer adoption has
  been performed.
- Specification: [bootstrap and release contract](../proposed/bootstrap-and-release-contract.md),
  §2.1; [release gates](release-gates.md), BOOT-2.1 and BOOT-4.7.
- Landed by: [PR #113](https://github.com/tailrocks/velnor-new/pull/113),
  squash merge `993050b379ed1b190315d0c2aaa272fd76080c9b`.
- Merge date: 2026-10-08 (22:34:50 UTC).
- Delivered: generator publisher draft handling accepts GitHub's temporary
  untagged draft URL while preserving the canonical URL check for published
  releases; draft creation explicitly targets the exact source SHA. The
  generator prepare gate still checks both `target_commitish == source SHA`
  and the tag's resolved commit independently.
- Acceptance evidence: the focused replay and rerun cases below qualify the
  source fix only. No v0.1.5 dispatch, tag creation, or publication was
  performed; historical run 37730166493 is described below.
- Deviations: none.
- Follow-up: protected hosted v0.1.5 qualification/publication and a separately
  reviewed consumer adoption remain outstanding.

## Failure evidence and scope

The available log excerpt for product-release run
[37730166493](https://github.com/tailrocks/velnor-new/actions/runs/37730166493)
shows the rendered publisher script, checksum output, a draft-creation URL of
`https://github.com/tailrocks/velnor-new/releases/tag/untagged-c155089fcae36e2c5c68`,
and an exit status of 1. It has no shell xtrace. Since the script is printed
before execution, the excerpt does not establish which later commands ran; in
particular, it does not prove the draft metadata check, asset check, final CI
or tag check, draft-to-published PATCH, or receipt check. Historical API JSON
for that attempt is unavailable, so this record does not attribute that run's
exit to a specific predicate.

A sanitized fixture reproduces the observed draft URL transition against the
unmodified publisher source. The mocked `gh release create` returns the
`untagged-c155089fcae36e2c5c68` URL. The subsequent GET by the stable release ID
returns matching release ID 123, tag `v0.1.4`, API URL, draft state, and
non-prerelease state. Its tag ref resolves to the exact fixture source SHA;
all 20 asset names, download URLs, sizes, and SHA-256 digests match the local
inventory. Individual checks of the draft metadata fields show that only the
comparison `.html_url == canonical /releases/tag/v0.1.4` is false. The full
publisher consequently exits at draft metadata before PATCH. This is a
reproduced source predicate defect, not proof of the exact failing command in
run 37730166493.

The source fix exempts the draft `html_url` comparison while the release is a
draft. It keeps stable ID, tag, API URL, draft, prerelease, exact source-tag,
and complete asset-inventory checks. Once published, the publisher still
requires the canonical `/releases/tag/v0.1.4` URL and `immutable == true`; the
acceptance receipt remains downstream of those checks. A separate negative
fixture confirms a noncanonical published URL still fails.

The same source inspection found that generator draft creation omitted
`--target`, unlike the generic family publisher. The immutable `v0.1.4` API
object currently reports `target_commitish=main`, while its tag resolves to
`d3590d321e51f7b99bfb0a92c11d3d6eb7af61cd`. The generator `prepare` gate
requires `target_commitish` to equal the exact source SHA and separately
resolves the tag to that SHA. Therefore the current release fails prepare
revalidation on the mutable target field even though its tag is source-bound.
This is a later rerun/revalidation issue; the available evidence does not
show that it caused run 37730166493 to fail. New draft creation now passes
`--target "$GITHUB_SHA"`, matching the generic publisher and preserving both
prepare checks.

## Focused verification

Before changing publisher behavior, the sanitized replay was added and run
against the baseline source:

```text
cargo test --locked -p velnor-actions-workflow-renderer complete_publish_script_uses_parent_tag_and_checks_all_assets -- --nocapture
1 passed; the UntaggedDraftHtmlUrl case expects publisher failure and asserts
that the canonical draft html_url predicate alone is false, exact asset
verification passes, the source tag matches, GET-by-ID occurs, and PATCH does not.
```

After the source change, the same test passes with the untagged draft case
successfully publishing through the mocked PATCH and receipt checks. The
`WrongPublishedUrl` case still fails after publication. The mock CLI rejects
draft creation unless `--target` names the exact fixture source SHA.

The prepare rerun test extracts the metadata predicate and tag-target function
from the generated `prepare-generator` script. A release fixture with
`target_commitish` equal to the source SHA passes metadata validation; an
otherwise identical fixture with `target_commitish=main` fails. In both cases
the separate tag-target function confirms the tag resolves to the exact source
SHA. Focused commands and results:

```text
cargo test --locked -p velnor-actions-workflow-renderer generator_prepare_rerun_requires_source_target_and_exact_source_tag -- --nocapture
1 passed; 511 filtered out.

cargo test --locked -p velnor-actions-workflow-renderer complete_publish_script_uses_parent_tag_and_checks_all_assets -- --nocapture
1 passed; 511 filtered out.
```

The focused test source and realistic sanitized API/CLI fixtures are
`crates/velnor-actions-workflow-renderer/src/schema2_generator_release_tests.rs`,
`schema2_generator_release_publish_fixtures.rs`,
`schema2_generator_release_fake_commands.rs`,
`schema2_generator_release_cli_tests.rs`, and
`schema2_product_release_tests.rs`. The source behavior lives in
`schema2_generator_release_manifest_publish.rs`. The fixture tests do not
change release versions.

## Pull-request run 37849214858 and correction

The initial CI run for pull request 113 used head
`bfe3b6bf2f5e647e29744249b6ac5710767a28bd` and finished with the `Required`
job failing at **Merge reports**; baseline publication was skipped. Job
113558432958 (`Rust / velnor-actions-workflow-renderer`) failed only in
Clippy: the new `all_requested_products_share_one_dispatch_only_source_bound_workflow`
test was 82/80 lines, and its two calls to `prepare_release_json` added
needless references to `expected`. The test now puts publisher-string checks
in a small helper and passes `expected` directly.

Job 113558433065 (`Rust / velnor-actions-cli`) failed in
`impl_cli_release_manifest::check_release_rejects_invalid_candidate_manifests`,
which runs the golden collector from a spaced relative path. The dogfood
preview still contained the old generated
`.github/actions/generator-release-publish/action.yml` (SHA-256
`02d765643735dba3e9b8b52c482afe3977601d58d0282ce15faa57d9095fc465`), while
the current CLI emitted the publication change (SHA-256
`04429fd25a82bb031d32beb5b7512f844a5a4e94b52b7a454b7dec1121f7e1ef`). The
full-tree check therefore rejected the stale dogfood preview and its tree
hash.

The dogfood files were refreshed with the documented CLI capture command,
`scripts/capture-opentofu-goldens.sh capture`, using the pinned Rust 1.98.1
toolchain. It captured all five cases, reported the dogfood tree as
`identical`, and changed only the dogfood generated publisher action and
`cases/dogfood/tree.sha256`; the collector also regenerated
`MANIFEST.sha256` to index those bytes. The checked-in root `.github` output
was not hand edited.

On macOS 27 arm64 with Rust 1.98.1, the exact hosted renderer Clippy command
passes. The complete renderer package run reported 172 library tests and 340
renderer integration tests passed, with zero ignored in either suite; doc
tests ran zero tests. The formerly failing spaced-relative CLI test also
passes (1 passed). A full `cargo test --locked -p velnor-actions-cli` run
reported 287 passed and 2 failed, with zero ignored: the existing P12 tests
`p12_manifest::stranded_lock_package_fails` and
`p12_policy_b::non_object_entries_fail` did not find their expected fixture
diagnostics. The full-suite failures remain unexplained and have not been
waived or attributed to this change. Each filtered P12 test passed
independently on both PR head
`537f90e297cae61eb9df65b70d2039cdace3dc5e` and the untouched main baseline;
the full-suite failure remains unexplained, is not waived, and is not
attributed to this change. The corrected PR head passed its hosted Required
check in run [37853197157](https://github.com/tailrocks/velnor-new/actions/runs/37853197157).

The generated publisher output was refreshed through the supported CLI, with
no hand edits:

```text
cargo run --locked -p velnor-actions-cli -- generate --output-dir /private/tmp/velnor-publication-generated
```

The first preview attempt exposed uncached registry entries used by offline
metadata inspection. `cargo fetch --locked` populated the root lockfile's
cache, and the nested runner lockfile's cache was populated by
`cargo fetch --manifest-path crates/velnor-runner/Cargo.toml --locked`. The
same preview command then succeeded. Comparing its `.github` tree with the checked-in tree
showed one changed path: `.github/actions/generator-release-publish/action.yml`.
That exact generated file was copied from the successful preview, and its
unmarked companion snapshot was refreshed from the same bytes at
`crates/velnor-actions-orchestrator/tests/snapshots/generator-release-publish.yml`.
No other `.github` workflow, action, version reference, or lockfile changed.

## Route and external gates

The supported entry point is the dispatch-only `.github/workflows/product-release.yml`
coordinator, which invokes the reusable `.github/workflows/product-release-generator.yml`
module. Release eligibility requires the dispatch SHA to be the exact current
`main` tip and its exact-source main CI run and `Required` job to succeed; see
[`release-eligibility`](../../.github/workflows/product-release.yml#L11) and its
main/`Required` checks at
[`product-release.yml`](../../.github/workflows/product-release.yml#L51).
The image and generator branches both depend on that eligibility job, while
`release-generator` depends on eligibility and `prepare-generator`; it does not
depend on `release-images` or its outcome. A runner-image job failure is
therefore outside the generator DAG and must be reported separately.

Generator publication acceptance requires the existing gates as a whole:
build and qualify Linux x86_64, macOS arm64, and macOS x86_64 assets; assemble
the same-run manifest; verify source- and workflow-bound attestations for all
three assets and the manifest; recheck source and CI immediately before
publication; and publish through the serialized protected
`generator-release` environment. The `publish-generator` job is conditioned on
successful attestations and uploads
`generator-release-accepted-${{ github.sha }}` with the verified manifest and
acceptance receipt only after the publisher completes; the job has
`actions: write` and `contents: write`. The receipt binds the source commit,
workflow-authority SHA, run and attempt, published tag and release ID, and the
verified manifest identity. Acceptance is recorded by that generator receipt,
not by the overall coordinator conclusion. Report the image-job outcome and
overall coordinator conclusion separately; a failed image branch does not
negate a valid generator receipt.

The `prepare-generator` consumer path checks a previously published release's
exact tag and source, immutable metadata, expected asset inventory and GitHub
asset metadata, then verifies the downloaded manifest and asset attestations
against the exact bytes, source digest, `refs/heads/main`, generator workflow
path, and signer digest. It binds the caller's source and workflow authority
to the same exact `main` SHA and performs these checks directly; it does not
require the overall coordinator conclusion.
See the existing-release checks in
[`prepare-generator`](../../.github/workflows/product-release.yml#L161) and the
protected generator publication job at
[`product-release-generator.yml`](../../.github/workflows/product-release-generator.yml#L455).

Read-only GitHub settings inspection on 2026-10-08 confirmed that the active
`protect-main` ruleset requires the strict `Required` check, pull requests with
resolved review threads, squash merges, and linear history; the protected
`generator-release` environment requires reviewer `donbeave` for protected
branches. These settings do not establish hosted generator acceptance. No
live dispatch or publication was used for this qualification.

Read-only release inventory checked at 2026-10-08 22:39 UTC found immutable
`v0.1.4` (20 assets, ID 406452151), `v0.1.2`, `v0.1.1`, and `v0.1.0`.
Tag `v0.1.3` exists but has no matching release; its commit SHA is
`33d79403fef47164944d83a5e42313f463679b39`. The `v0.1.5` tag lookup returned
HTTP 404 and no `v0.1.5` release appeared in the inventory. The next unused
official version is therefore `v0.1.5`; no version reference was changed in
this branch.
