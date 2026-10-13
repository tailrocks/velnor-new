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

## v0.1.6 candidate atomic-observer failure

PR #122 is a draft version candidate at source head
`7690dea7ddb39130f2a2dde679ce07582300aae4`, based on main
`2edc5cad367f098fd295e5edd09e5f1fd79a9acb`. Its version-owned outputs target
v0.1.6. This section records a local qualification failure and a bounded
diagnostic; it does not qualify the candidate or establish release acceptance.

The exact `./scripts/verify-local.sh` run on that head failed in
`cargo test --locked -p velnor-actions-orchestrator`. The integration binary
reported 728 passed, 1 failed, 0 ignored. The failure was
`impl_generate_p09_atomic::atomic_commit_never_exposes_missing_tree`: observer
1's `symlink_metadata` of
`/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/.tmpungynC/.github` returned
`NotFound` / raw errno 2 at iteration 55,380, while the sampled writer phase
was `rewrite_3_in_progress`. Subsequent parent and target metadata probes
succeeded. They ran after the original error and therefore do not establish
continuous target visibility. The preserved log is
`/private/tmp/velnor-gap-probe/historical-atomic-verify-local-7690-failure-20261009.log`,
SHA-256 `f3501bb1b8c5120e97aa51d2009ce715980e074dbd9c3b4b6adade4ed9b82e03`.

An earlier harness description called its cross-parent child-stage cell
`ProductTempDir`; that label was inaccurate. In the actual integration test,
`make_repo()` provides the repository `TempDir`, root permissions are present,
and `stage_in_place` selects that staging `TempDir` root itself. The existing
`.github` and staged root are sibling entries under the repository root; the
exchange is same-parent. The earlier child-stage cell had different topology,
and its same-parent comparison did not model the production `TempDir` drop
after `remove_dir_all`. Neither earlier standalone cell attributed the
historical lookup error. No source behavior or assertion changed for this
diagnostic.

The bounded replay used a disposable worktree rooted at commit `7690dea7`.
Its temporary writer trace used a 64-entry thread-local buffer for 56
phase/operation records over eight rewrites. The original observer success
loop remained unchanged. Formatting and trace-file output occurred only after
the scoped test threads joined. For the exchange record, target and staged
device/inode metadata was sampled before the exchange, and only when capture
was enabled; no metadata lookup, formatting, or global mutex was added between
exchange and old-tree removal. The `TempDir` drop stayed after the swap result,
matching the production lifecycle. The recorded monotonic timestamps share
the observer's original `Instant`; they are diagnostic timing only and do not
prove uninterrupted pathname visibility.

With Rust/Cargo 1.98.1, `CARGO_HOME=/tmp/velnor-110-cargo-home`,
`CARGO_TARGET_DIR=/private/tmp/velnor-atomic-prod-7690-target`, and Cargo
offline, the one isolated run used:

```sh
cargo test --locked --offline -p velnor-actions-orchestrator --test velnor_orchestrator \
  impl_generate_p09_atomic::atomic_commit_never_exposes_missing_tree -- --exact --test-threads=1
```

It passed 1 test, with 728 filtered out, in 2.49 seconds. The next and only
default-concurrency comparison ran the same integration test target, rebuilt
only for post-join trace formatting, without a test filter or
`--test-threads` override:

```sh
cargo test --locked --offline -p velnor-actions-orchestrator --test velnor_orchestrator
```

It passed 729 tests, 0 failed, in 25.08 seconds. Neither replay recorded an
observer metadata error. The reconstructed source trees for the runs are
`fbbd1b65bf9555eade96aaebfc02fd8aad0d96ca` (isolated) and
`3b62d4cbd12c59ff525b11d85aa8cc75338550af` (concurrent), both based on the
same 7690 commit. Between those runs, only post-join trace rendering changed:
the isolated output used derived `AtomicDiagnosticIdentity` debug formatting;
the concurrent output printed the same fields individually and added Debug
derives to silence warnings. The writer and observer windows were unchanged.
The second test binary SHA-256 was
`d7d4f9f7df97a528695faace3b0a6103d4684771a5e0134a2c1884c85fe70690`. The
first binary hash was not saved before Cargo replaced that target with the
format-only build, so no binary hash is claimed for the isolated run.

Trace and command-output artifacts are retained under
`/private/tmp/velnor-gap-probe/`:

| Run | Artifact | SHA-256 |
|---|---|---|
| Isolated | `atomic-minimal-isolated-20261009.log` | `08937e87e979275cf8d0016706fa58417b4d8d827d82a62817cc624aa123f243` |
| Isolated | `atomic-minimal-isolated-20261009.stdout` | `c9ab6ec32aa23a4733479625be88f06aaf0ddb4baed233e11fa1c82a362751f9` |
| Concurrent | `atomic-minimal-integration-20261009.log` | `4d6e34891391d9638515fecfde2f821d4a0f914d05daa562e2555158ac0b82c5` |
| Concurrent | `atomic-minimal-integration-20261009.stdout` | `32fa04ddbe65c3043808469349132a9192e9e083731ea21d1a689cce31076e5c` |

An earlier replay with global locking, extra metadata probes, and per-event
formatting passed the integration binary but was more intrusive and is
excluded from this comparison. The earlier four-cell native harness is also
not treated as a production-flow reproduction. No additional replay was run.

The only syscall-tracing attempt in this session was
`/usr/sbin/dtrace -ln 'syscall::lstat*:entry'`. It returned
`dtrace: DTrace requires additional privileges` for the current unprivileged
UID 501 session. No syscall or VFS trace was captured. This is a limitation of
this session's tracing capability, not evidence that macOS/APFS cannot be
traced or that the kernel caused the error.

## Bounded userspace controls

To test the remaining cleanup-lifecycle hypotheses without privileged tracing,
two single-test cells ran on disposable source `7690dea7`. The diagnostic-only
source diff SHA-256 was
`f37fb01215414f67802993ecce7e4c7ff74c0b68cde2abd3adde07397a5d1fab`; it did
not alter the repository worktree or production branch. The original absolute
`symlink_metadata` observer loop and strict zero-error assertion were
unchanged. An FD-relative, no-follow `.github` lookup was added only after an
original observer error; neither cell had such an error, so that post-error
snapshot did not execute.

Both cells opened and held a descriptor to each retired root after exchange,
recorded its metadata before cleanup, and checked the descriptor again after
observer threads joined and cleanup had completed. The second check was not
sampled immediately after each removal. Keeping these descriptors pins the retired directory and is an
intervention, so clean results cannot rule out a failure that requires the
un-pinned production lifetime. The immediate cell kept production
`remove_dir_all` and `TempDir` drop ordering. The deferred cell instead kept
the actual eight `TempDir` owners alive until all four observer threads joined,
then dropped them and collected the post-cleanup descriptor metadata. It kept
the same-parent staging topology, eight rewrites, and four observers. This is
a diagnostic control, not a product behavior change or a release gate.

Each invocation used Rust/Cargo 1.98.1, `CARGO_HOME=/tmp/velnor-110-cargo-home`,
`CARGO_TARGET_DIR=/private/tmp/velnor-atomic-prod-7690-target`, Cargo offline,
and the exact filtered test command:

```sh
cargo test --locked --offline -p velnor-actions-orchestrator --test velnor_orchestrator \
  impl_generate_p09_atomic::atomic_commit_never_exposes_missing_tree -- --exact --nocapture
```

The immediate-cleanup cell set `VELNOR_ATOMIC_DEFER_CLEANUP=0`, passed 1 test
with 0 failures, 0 ignored, and 728 filtered out in 2.61 seconds. Its trace is
`/private/tmp/velnor-gap-probe/userspace-immediate-7690.log`, SHA-256
`00b6f750fa11ab8e8ad078b13d801a02c002c35e2348ff43e28017c18c690c07`. The
deferred-cleanup cell set `VELNOR_ATOMIC_DEFER_CLEANUP=1`, passed 1 test with 0
failures, 0 ignored, and 728 filtered out in 2.82 seconds. Its trace is
`/private/tmp/velnor-gap-probe/userspace-deferred-7690.log`, SHA-256
`f579851d635e7b2ab84144d49ceaa532b239473993ecb645323df047635b778e`.

Both traces record eight successful exchanges and eight retired-root
descriptors. All recorded pre- and post-cleanup descriptor metadata calls
succeeded with errno 0 and the same device/inode per descriptor. The deferred
trace records all eight TempDir owners being dropped after the observers
joined. Cargo reported the same test-binary path for both invocations, but its
binary digest was not saved before the diagnostic target disappeared; no
binary hash is claimed. These two non-reproductions show that held descriptors
remain queryable after cleanup in these runs. They do not prove pathname
continuity, explain the historical `NotFound`, or clear the failed assertion.
No additional clean replay was run.

The historical `NotFound` remains a real failed assertion and its production
cause is unproven. The two minimized non-reproductions neither clear that
failure nor establish a namespace gap, a rename error, or an old-tree cleanup
race. PR #122's local qualification therefore remains blocked; the v0.1.5
tag and draft remain occupied, and no release acceptance, publication, or
consumer adoption is claimed.
