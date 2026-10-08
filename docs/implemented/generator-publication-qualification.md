# Generator publication qualification

- State: source fix candidate on `fix/qualify-generator-publication`; focused
  fixture tests pass, independent review and full branch gates are pending.
- Specification: [bootstrap and release contract](../proposed/bootstrap-and-release-contract.md),
  §2.1; [release gates](release-gates.md), BOOT-2.1 and BOOT-4.7.
- Landed by: pending pull request.
- Merge date: pending.
- Delivered: generator publisher draft handling accepts GitHub's temporary
  untagged draft URL while preserving the canonical URL check for published
  releases; draft creation explicitly targets the exact source SHA. The
  generator prepare gate still checks both `target_commitish == source SHA`
  and the tag's resolved commit independently.
- Acceptance evidence: see the focused replay and rerun cases below. No live
  release dispatch, tag creation, or publication was performed.
- Deviations: none.
- Follow-up: independent review, required branch checks, and protected hosted
  qualification remain outstanding.

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
module. The generator path must pass the exact-source default-branch `Required`
CI gate, build and qualify Linux x86_64, macOS arm64, and macOS x86_64 assets,
assemble the same-run manifest, verify source/workflow-bound attestations,
recheck source and CI immediately before publication, and publish in the
serialized protected `generator-release` environment. The publisher job has
`actions: write` and `contents: write`; actual environment rules and default
branch protection must still be verified in repository settings. No live
dispatch or publication was used for this qualification.

Read-only release inventory checked at 2026-10-08 21:38 UTC found immutable
`v0.1.4` (20 assets, ID 406452151), `v0.1.2`, `v0.1.1`, and `v0.1.0`.
Tag `v0.1.3` exists but has no matching release; its commit SHA is
`33d79403fef47164944d83a5e42313f463679b39`. The `v0.1.5` tag lookup returned
HTTP 404 and no `v0.1.5` release appeared in the inventory. The next unused
official version is therefore `v0.1.5`; no version reference was changed in
this branch.
