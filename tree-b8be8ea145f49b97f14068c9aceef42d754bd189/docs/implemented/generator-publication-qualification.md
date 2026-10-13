# Generator publication qualification

- Refreshed inventory (read-only GitHub observation at 2026-10-10 02:14 UTC):
  `v0.1.4` remains the latest published generator release: immutable release
  ID 406452151, 20 assets, with manifest digest
  `d6f7788e50e0c6168c36d122d910476ac6772cd4603352ef2319a88b888fc076`; its
  tag resolves to `d3590d321e51f7b99bfb0a92c11d3d6eb7af61cd`. The occupied
  `v0.1.5` draft remains release ID 407717552, with 20 assets and the first
  asset URL under `untagged-6899c9b4aa4e941dadba`; its tag resolves to
  `f1041f322c54cd7edfdef06afd307799eeaa801a`. The `v0.1.6` tag was absent
  from `git ls-remote`, the release list, and a release lookup at this
  observation. This records availability only; it does not reserve `0.1.6`.
  GitHub's `isLatest` flag names a separate immutable `binary-...` release,
  which is not a generator acceptance receipt.
- Earlier snapshot (read-only GitHub observation at 2026-10-09 23:34 UTC): immutable
  `v0.1.4` (release ID 406452151) is the latest published generator release;
  its tag resolves to `d3590d321e51f7b99bfb0a92c11d3d6eb7af61cd` and it has 20
  assets.
- Hosted `v0.1.5` outcome: product-release run
  [37904531694](https://github.com/tailrocks/velnor-new/actions/runs/37904531694)
  was dispatched on `main` at exact source
  `f1041f322c54cd7edfdef06afd307799eeaa801a`. Exact-source CI run
  [37902214173](https://github.com/tailrocks/velnor-new/actions/runs/37902214173)
  and its `Required` job succeeded. The Linux, macOS arm64, and macOS x86_64 builds and qualifications, plus
  their attestations and the manifest attestation, passed. `publish-generator`
  then failed at job
  [113738982913](https://github.com/tailrocks/velnor-new/actions/runs/37904531694/job/113738982913)
  while checking draft asset download URLs. The publisher did not upload its
  acceptance artifact. GitHub still reports release ID 407717552 as a draft
  for tag `v0.1.5`, targeting the exact source above, with 20 assets whose
  download URLs use the temporary `untagged-6899c9b4aa4e941dadba` path.
- The same coordinator also had an independent runner-image build failure at
  job
  [113735185782](https://github.com/tailrocks/velnor-new/actions/runs/37904531694/job/113735185782),
  so its overall conclusion is failure. Report that branch separately from
  generator acceptance: the generator publisher itself failed, and no
  acceptance receipt was produced.
- Publisher fix [PR #120](https://github.com/tailrocks/velnor-new/pull/120)
  merged at `2edc5cad367f098fd295e5edd09e5f1fd79a9acb` on 2026-10-09
  10:18:46 UTC, after the failed run. It permits temporary draft asset URLs
  while retaining the draft's release identity, exact source, and complete
  asset inventory checks; after publication it still requires canonical asset
  URLs and immutable metadata. No successful official publication after this
  fix has been observed.
- Historical PR #122 snapshot at 2026-10-09 23:34 UTC: the `v0.1.5` tag and
  draft were occupied and both `v0.1.6` tag/release lookups returned 404. PR
  #122 was then at head `87f7390abf7272d076f43b7fb75a9218228d14d0` against
  `main` at `3139334cb79c0b494eb530b1de88ff258af10f21`. That snapshot was
  version preparation only; it did not establish release readiness or hosted
  acceptance. The refreshed inventory above confirms that `v0.1.5` remains
  occupied and `v0.1.6` remains unused.
- Canonical route and gates: dispatch only
  [`.github/workflows/product-release.yml`](../../.github/workflows/product-release.yml#L4)
  on `main`, with `workflow_dispatch: {}` and no inputs. Release eligibility
  requires the exact current `main` tip and successful same-source CI with the
  `Required` job. Generator publication runs through the protected
  `generator-release` environment; its current reviewer gate is recorded below.
- Specification: [bootstrap and release contract](../proposed/bootstrap-and-release-contract.md),
  §2.1; [release gates](release-gates.md), BOOT-2.1 and BOOT-4.7.
- Earlier source change: [PR #113](https://github.com/tailrocks/velnor-new/pull/113)
  merged as `993050b379ed1b190315d0c2aaa272fd76080c9b` on 2026-10-08
  22:34:50 UTC. Its focused replay and rerun cases qualify source behavior;
  they are not hosted publication evidence and did not cover the draft asset
  URL failure observed in run 37904531694.
- Follow-up: resolve PR #122's review and main-CI gates, then use a new version
  through the canonical protected workflow. Consumer adoption remains a
  separate reviewed change after a successful immutable release.

## Earlier source reproduction and failure evidence

The earlier v0.1.4 product-release dispatch in
[run 37730166493](https://github.com/tailrocks/velnor-new/actions/runs/37730166493)
used source `d3590d321e51f7b99bfb0a92c11d3d6eb7af61cd` and also failed in both
the runner-image and generator-publish branches. Its available log shows the
rendered publisher script, checksum output, and a draft-creation URL of
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

PR #113's source fix exempts the release-page `html_url` comparison while
the release is a draft. It keeps stable ID, tag, API URL, draft, prerelease,
exact source-tag, and complete asset-inventory checks. That fixed the earlier
fixture's release-page predicate but did not cover the separate asset
`browser_download_url` predicate exposed by the v0.1.5 hosted run above. PR #120
skips that canonical asset-URL comparison only while a draft; once published,
the publisher still requires canonical asset URLs and `immutable == true`, and
the acceptance receipt remains downstream of those checks. Negative fixtures
continue to reject noncanonical published URLs.

The same source inspection found that generator draft creation omitted
`--target`, unlike the generic family publisher. At the current snapshot, the
published `v0.1.4` API object still reports `target_commitish=main`, while its
tag resolves to `d3590d321e51f7b99bfb0a92c11d3d6eb7af61cd`. The generator's
existing-release revalidation requires `target_commitish` to equal the exact
source SHA and separately resolves the tag to that SHA, so revalidating this
release against the source-bound metadata predicate would fail on
`target_commitish`. This is separate from both observed draft-URL failures and
does not explain either run. New draft creation passes `--target
"$GITHUB_SHA"`, matching the generic publisher and preserving both checks.

## Focused verification

Before changing publisher behavior, the sanitized replay was added and run
against the baseline source:

```text
cargo test --locked -p velnor-actions-workflow-renderer complete_publish_script_uses_parent_tag_and_checks_all_assets -- --nocapture
1 passed; the UntaggedDraftHtmlUrl case expects publisher failure and asserts
that the canonical draft html_url predicate alone is false, exact asset
verification passes, the source tag matches, GET-by-ID occurs, and PATCH does not.
```

After PR #113 changed the release-page predicate, the same test passes
with the untagged draft case successfully publishing through the mocked PATCH
and receipt checks. The
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

The supported entry point is the `main`-only `.github/workflows/product-release.yml`
coordinator with `workflow_dispatch: {}` and no inputs; it invokes the reusable
`.github/workflows/product-release-generator.yml` module. Release eligibility
requires the dispatch SHA to be the exact current `main` tip and its
exact-source main CI run and `Required` job to succeed; see
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

Read-only GitHub rules inspection on 2026-10-08 confirmed that the active
`protect-main` ruleset requires the strict `Required` check, pull requests with
resolved review threads, squash merges, and linear history. A read-only check
of the `generator-release` environment at 2026-10-09 23:34 UTC confirmed its
required reviewer `donbeave` for protected branches. These settings establish
the configured approval gate, not hosted generator acceptance; the actual
hosted outcome is recorded above.

The read-only release snapshot at 2026-10-09 23:34 UTC found published,
immutable `v0.1.4` (20 assets, ID 406452151); its API `target_commitish` is
`main`, while the tag resolves to `d3590d321e51f7b99bfb0a92c11d3d6eb7af61cd`.
Tag `v0.1.5` resolves to `f1041f322c54cd7edfdef06afd307799eeaa801a` and is held
by draft release ID 407717552 (20 assets, `immutable=false`). Its page and
asset download URLs use the temporary
[`untagged-6899c9b4aa4e941dadba` draft](https://github.com/tailrocks/velnor-new/releases/tag/untagged-6899c9b4aa4e941dadba)
path. The `v0.1.6` tag and release lookups returned 404; PR #122 is an open
version-preparation change, not an official release or qualification result.
