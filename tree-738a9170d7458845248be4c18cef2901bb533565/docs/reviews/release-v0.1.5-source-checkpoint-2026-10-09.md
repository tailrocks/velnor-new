# v0.1.5 source qualification checkpoint — 2026-10-09

**Status:** The candidate-output binding fix is integrated on main. Official
v0.1.5 publication and consumer adoption remain pending. This checkpoint
records source, review, and CI evidence; it is not a release manifest,
publication receipt, artifact digest, or publication acceptance.

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
pending observation, not a completed main gate or release acceptance.

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
