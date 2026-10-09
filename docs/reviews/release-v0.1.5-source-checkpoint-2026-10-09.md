# v0.1.5 source qualification checkpoint — 2026-10-09

**Status:** The candidate-output binding fix is integrated on main. Official
v0.1.5 publication and consumer adoption remain pending. This checkpoint
records source, review, CI, integration, and release-metadata evidence; it is
not a release manifest, publication receipt, artifact digest, or publication
acceptance.

At the `2026-10-09T10:10Z` read-only API recheck, release `407717552` still
reported `draft=true` and `published_at=null`; tag `v0.1.5` still pointed to
`f1041f322c54cd7edfdef06afd307799eeaa801a`, with 20 uploaded asset metadata
records. This publication status is separate from the later PR #121 source
integration recorded below.

## Gate 8 marker false pass

The P1 review on the earlier candidate (`315804ef45`) found that
`check-release` normalized generated asset digests and the source commit to
golden marker values before comparing the output tree. A wrapper that emitted
those hard-coded values could therefore match the oracle while disagreeing
with the staged candidate manifest. The [P1 finding](https://github.com/tailrocks/velnor-new/pull/119#discussion_r4227667532)
records this enabling cause.

The reproduction rewrote emitted SHA-256 values to `a` repeated 64 times and
the source commit to `b` repeated 40 times. At the reviewed code, normalization
preserved those markers and `check-release` passed. PR #119 changed
`capture_release_case` to validate every raw ConsumerV1 URL, target, digest,
and source tuple against the staged manifest before normalization. Binding
errors now stop qualification with exit 2. The [author's fix record](https://github.com/tailrocks/velnor-new/pull/119#discussion_r4227854168)
describes the eight full-path fault cases, including hard-coded values,
mixed, missing, unknown, and wrong-target bindings. The implementation is in
[`qualification-goldens.sh`](../../scripts/generator-release/qualification-goldens.sh)
and its test harness is
[`test-capture-opentofu-goldens-bin.sh`](../../scripts/test-capture-opentofu-goldens-bin.sh).

Independent review by `/root/repository_evidence/qualification_fixture_design_review`
covered `ff150c0bbb23984b6d4106883eba74d1326bff51..11e6f3294b450d979dbcb13945de80cbf6deda2e`
and returned **Ready**. It reran the prior all-marker candidate against the
fix and observed exit 2 with `wrong digest for target x86_64-unknown-linux-gnu`
before normalization.

## Gate and hosted PR evidence

On clean head `11e6f3294b450d979dbcb13945de80cbf6deda2e`, the focused candidate
qualification, eight binding-fault cases, targeted snapshot test, all six
repository gates, and `./scripts/verify-local.sh` passed. Nextest reported
3,645 passed and 2 skipped; Alint reported 50 checks passed and 4
informational findings; Cargo Deny exited successfully with existing
duplicate-crate and unmatched-license warnings. Freshness passed.

[PR #119](https://github.com/tailrocks/velnor-new/pull/119) merged this head.
Hosted CI run [37900535373](https://github.com/tailrocks/velnor-new/actions/runs/37900535373)
completed with 20 of 21 jobs successful, including Required job
`113725335213`; the expected `Publish baseline` job was skipped. The separate
DCO check passed.

Main CI run [37902214173](https://github.com/tailrocks/velnor-new/actions/runs/37902214173)
targets merge SHA `f1041f322c54cd7edfdef06afd307799eeaa801a`. At
`2026-10-09T08:17:27Z`, the run was still `in_progress` with no conclusion;
18 of 19 jobs had succeeded with no failures. The remaining
`Rust / velnor-actions-cli` job was in its `Post Restore MBX objects` step,
started at `08:11:41Z`; its job timeout is 30 minutes. This is a timestamped
pending observation, not a completed main gate or release acceptance. By
`2026-10-09T08:21:33Z`, the API reported the same run `completed` with
conclusion `success` on the same source SHA.

## Draft publication attempt

Hosted generator release run
[37904531694](https://github.com/tailrocks/velnor-new/actions/runs/37904531694)
used source and workflow-authority SHA
`f1041f322c54cd7edfdef06afd307799eeaa801a`. Source eligibility job
`113734693458` and generator `verify-release-source` job `113735177253`
passed. All three target qualification jobs passed: Linux
`113736710445`, macOS ARM `113736710529`, and macOS Intel `113736710450`.
Their three attestations also passed: Linux `113737019260`, macOS ARM
`113737158999`, and macOS Intel `113737964502`; manifest attestation
`113738670435` passed.

The GitHub Actions approvals API records the protected `generator-release`
environment as approved by `donbeave` after the target qualifications and
manifest attestation. The `publish-generator` job `113738982913` then failed
in its `Publish generator release` step at `2026-10-09T08:37:30Z`. Its
`Upload verified immutable release acceptance` step was skipped. The run has
build, manifest, and attestation artifacts, but no
`generator-release-accepted-f1041f322c54cd7edfdef06afd307799eeaa801a`
acceptance artifact.

The GitHub release API record [407717552](https://api.github.com/repos/tailrocks/velnor-new/releases/407717552)
reports tag `v0.1.5`, target `f1041f322c54cd7edfdef06afd307799eeaa801a`,
`draft=true`, and `published_at=null`. The `v0.1.5` Git ref points directly
to that same commit. The draft has 20 unique expected asset names, all with
state `uploaded`; the metadata-only inventory is:

| Asset name | State |
|---|---|
| `release-manifest.json` | `uploaded` |
| `release-manifest.json.intoto.jsonl` | `uploaded` |
| `velnor-actions-0.1.5-aarch64-apple-darwin` | `uploaded` |
| `velnor-actions-0.1.5-aarch64-apple-darwin.intoto.jsonl` | `uploaded` |
| `velnor-actions-0.1.5-aarch64-apple-darwin.provenance.json` | `uploaded` |
| `velnor-actions-0.1.5-aarch64-apple-darwin.provenance.json.intoto.jsonl` | `uploaded` |
| `velnor-actions-0.1.5-aarch64-apple-darwin.sha256` | `uploaded` |
| `velnor-actions-0.1.5-aarch64-apple-darwin.sha256.intoto.jsonl` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-apple-darwin` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-apple-darwin.intoto.jsonl` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-apple-darwin.provenance.json` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-apple-darwin.provenance.json.intoto.jsonl` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-apple-darwin.sha256` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-apple-darwin.sha256.intoto.jsonl` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-unknown-linux-gnu` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-unknown-linux-gnu.intoto.jsonl` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-unknown-linux-gnu.provenance.json` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-unknown-linux-gnu.provenance.json.intoto.jsonl` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-unknown-linux-gnu.sha256` | `uploaded` |
| `velnor-actions-0.1.5-x86_64-unknown-linux-gnu.sha256.intoto.jsonl` | `uploaded` |

The metadata-only check queried release `407717552`, compared its asset names
with `release_asset_names` in
[`generator-release-publish/action.yml`](../../.github/actions/generator-release-publish/action.yml),
checked uniqueness and upload states, and compared the first asset's URL path
with the final-tag path predicate. The metadata-only API projection was:

```sh
gh api repos/tailrocks/velnor-new/releases/407717552 --jq '
  . as $release
  | {
      id: $release.id,
      tag_name: $release.tag_name,
      target_commitish: $release.target_commitish,
      draft: $release.draft,
      published_at: $release.published_at,
      assets: [$release.assets[] | {name, state}],
      names_unique: ([$release.assets[].name] | unique | length) == ($release.assets | length),
      all_states_uploaded: all($release.assets[]; .state == "uploaded"),
      first_asset_uses_untagged_slug: ($release.assets[0].browser_download_url | contains("/releases/download/untagged-")),
      first_asset_final_tag_path_match: ($release.assets[0].browser_download_url | endswith("/releases/download/\($release.tag_name)/\($release.assets[0].name)"))
    }'
```

The result was 20 names matching source, 20 unique names, 20 `uploaded`
states, `first_asset_uses_untagged_slug=true`, and
`first_asset_final_tag_path_match=false`. The first asset,
`release-manifest.json`, had a redacted `untagged-<slug>` path tag and did not
match the expected `/releases/download/v0.1.5/release-manifest.json` path.
Source calls `verify_release_assets` on the draft response before its `PATCH`
that clears `draft`; the metadata-only reproduction fails that URL predicate
before the `PATCH` stage. The projection reports only asset names, states,
and predicate booleans; it does not expose URL values. No asset body, size,
or digest is recorded.

The raw hosted log is at
`/tmp/velnor-37904531694-publish-generator-failure.log`. It records the
publish step's exit 1, but has no shell xtrace and does not prove which command
failed. The pre-PATCH URL-predicate attribution is an inference from the
workflow source order and the current draft metadata, corroborated by the
metadata-only reproduction. A separate runner-images graph failed in
`Build runner images` job `113735185782`; its publish and attest jobs were
skipped. That detached runner-image failure is separate from generator job
`113738982913`.

The tag and draft assets do not establish official publication or consumer
adoption. This run produced no acceptance artifact or publication receipt.

## Integrated commit record

The PR was squash-merged at `2026-10-09T07:59:26Z` as
`f1041f322c54cd7edfdef06afd307799eeaa801a`, tree
`470a4ebc2c0d3e6e93c2942936c55ee0d0d84c75`, parent
`ff150c0bbb23984b6d4106883eba74d1326bff51`. Its body has
`Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>` before
`Co-authored-by: Codex <codex@openai.com>`. That order differs from the
required order. Source commit `11e6f3294b450d979dbcb13945de80cbf6deda2e`
has the required Codex co-author trailer followed by Alexey's sign-off; the
GitHub-generated squash commit reverses that order. The merged commit is
recorded as observed; this checkpoint does not rewrite it.

Official v0.1.5 publication and adoption still require the release gates and
fresh accepted publication evidence in
[`release-gates.md`](../implemented/release-gates.md).

## PR #121 integration and trailer-order observation

[PR #121](https://github.com/tailrocks/velnor-new/pull/121) was squash-merged
at `2026-10-09T10:04:21Z`. The guarded command used was:

```sh
gh pr merge 121 --repo tailrocks/velnor-new --squash \
  --match-head-commit 26fb6a56d8a9acc1e3efc2ceedb76a92112add8d
```

It used `--repo`, `--squash`, and `--match-head-commit`; no custom subject or
body was supplied. The original PR body contains no commit trailers. The
repository reports squash settings `PR_TITLE` for the commit title and
`PR_BODY` for the commit message.

The source commit
[`26fb6a56d8a9acc1e3efc2ceedb76a92112add8d`](https://github.com/tailrocks/velnor-new/commit/26fb6a56d8a9acc1e3efc2ceedb76a92112add8d)
has tree `2628929c0279f6a5357a6527f359ef8f4be0c89e` and trailers ordered as
`Co-authored-by: Codex <codex@openai.com>` followed by
`Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>`. The resulting squash
commit
[`72dd1c002309b88164929b4f7dedfd79f5100ccd`](https://github.com/tailrocks/velnor-new/commit/72dd1c002309b88164929b4f7dedfd79f5100ccd)
has that same tree and sole parent
`2a1d60547cd7c6b7646c655652c9c9ed28db6b69`. GitHub resolves its author
account `donbeave` to Alexey Zhokhov. Its message orders
`Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>` before
`Co-authored-by: Codex <codex@openai.com>`, the reverse of the source
commit's trailers.

This records an observed trailer-order discrepancy. Its cause is unproven;
no rewrite was attempted, and the immutable merge commit remains unchanged.
The merge does not change the separate v0.1.5 publication status above.
