# Release gates: source and external evidence

This record separates checked-in release mechanics from hosted evidence and
infrastructure. The renderer emits a source-bound, dispatch-only
`.github/workflows/product-release.yml` coordinator with reusable image,
binary, and generator modules. The generator module includes candidate
qualification, a same-run manifest, source-bound attestations, and protected
publication. Source and local tests alone do not prove the hosted path. As of
2026-10-09, `v0.1.4` is published, but its source workflow run did not produce
the generator publisher's acceptance receipt.

This release integration checkpoint does not constitute whole-tree source
acceptance. The merged catalog keeps production MBX at 1.21.1 and retains the
hold on the unqualified 1.22.0 promotion. The merged PR73 seed-authority
correction is present in this source tree, but no hosted cache or seed
qualification is established by the release-source checks below.

## Seed and first release (BOOT)

- BOOT-4.2 (seed v0): NEEDS-HUMAN. Unblock = a manually reviewed build with
  pinned Mise + MBX (no Velnor binary), 2 distinct admin approvals, and an
  independent reproducible rebuild (second party, pinned catalog, sha256
  match) recorded in the seed PR (Gap E review). Pre-seed is trust-on-review.
- BOOT-2.1 (release assets): SOURCE-IMPLEMENTED, THREE-TARGET HOSTED
  QUALIFICATION AND ATTESTATION JOBS PASSED; PUBLISHER JOB FAILED.
  Run [`37730166493`](https://github.com/tailrocks/velnor-new/actions/runs/37730166493)
  used source `d3590d321e51f7b99bfb0a92c11d3d6eb7af61cd`. The Linux,
  macOS arm64, and macOS x86_64 qualification and attestation jobs succeeded.
  The `publish-generator` job verified candidate checksums, emitted an
  untagged-release URL, and exited 1; its final logged acceptance-artifact
  step was skipped. The historical log does not establish which predicate
  caused the job to exit or show that final acceptance was reached. The same
  coordinator run also failed the runner-image build, a separate branch outside
  the generator DAG; record that image result and the overall coordinator
  conclusion separately from generator acceptance. The public
  [`v0.1.4` release](https://github.com/tailrocks/velnor-new/releases/tag/v0.1.4)
  is now present with all three target assets and a manifest whose GitHub
  asset digest is `sha256:d6f7788e50e0c6168c36d122d910476ac6772cd4603352ef2319a88b888fc076`;
  that inventory does not turn the failed run into publisher acceptance
  evidence. A sanitized replay of the publisher command sequence
  isolates the draft-metadata `html_url` check and no subsequent PATCH, but
  that replay is not historical-run evidence. Unblock = for the renderer-size
  fix's exact current-main SHA, pass release eligibility and the protected
  generator build, qualification, attestation, and publication gates, then
  produce the generator acceptance receipt. Separately review the published
  manifest bytes for consumer adoption; report the image-job result and overall
  coordinator conclusion alongside the generator result.
- BOOT-4.7 (protected release job): SOURCE-IMPLEMENTED, REPOSITORY
  PROTECTION VERIFIED, GENERATOR PUBLISH ACCEPTANCE INCOMPLETE. The live
  `generator-release` environment requires a reviewer and permits deployments
  only from protected branches. The active `protect-main` ruleset requires
  `Required`; the hosted run above still failed before producing the required
  publisher acceptance artifact. Unblock = produce the acceptance receipt from
  the protected generator path for the qualified fix, then land any
  bootstrap-lock update in a SEPARATE reviewed change while ordinary CI keeps
  using its previous bootstrap. The overall coordinator conclusion is reported
  separately from generator acceptance.
- BOOT-3.4 (mise-bootstrap equality): half done (`.mise-version` ==
  `MISE_VERSION` const). Unblock = seed creates `.velnor/generator.lock`
  with the same exact Mise release + SHA-256; the equality check then
  becomes mechanical.

## Freshness process gates (VER)

- VER-1.5 (incompatible newest = migration): procedure defined in
  [update-procedure.md](update-procedure.md) (one coherent set, migration
  reported, never silent). Residual NEEDS-HUMAN: a maintainer MUST run the
  update set and write the migration; no automation proposes migrations.
- VER-1.7 (expedited security path): procedure defined (same-day set,
  minimal scope, full gate before merge). Residual NEEDS-HUMAN: a human
  MUST declare the security exception and drive it.
- VER-2.18 (stale default blocks release): SOURCE-IMPLEMENTED, HOSTED RUN
  UNVERIFIED. The publisher's source gate calls
  `scripts/check-freshness.sh`, which rejects stale, missing, mismatched, or
  expired holds. Unblock = a successful exact-source protected generator
  publisher run after exact-main `Required` CI eligibility.
- VER-3.2 / VER-3.3 (update machinery): procedure + Renovate proposals +
  script gates exist. Residual NEEDS-HUMAN: a maintainer MUST assemble each
  update set, record timestamp+delta, and run full qualification. No updater
  binary exists and none is planned for V1 — this is the mechanical maximum.
- VER-3.7 (merge-after-qual): SOURCE CHECKS AND BRANCH PROTECTION VERIFIED.
  The live `protect-main` ruleset requires the strict `Required` status check.
  Main merge SHA `993050b379ed1b190315d0c2aaa272fd76080c9b` passed all 21
  jobs, including `Required` job 113580987515, in
  [run `37854363776`](https://github.com/tailrocks/velnor-new/actions/runs/37854363776)
  after PR #113 merged. This verifies the publication-source integration's
  main CI, not hosted release qualification or publication. The release gate
  checks the exact-source main CI run and its `Required` job before building
  and repeats that check before publication. Refresh this evidence if main
  advances before a new release.
- VER-4.4 (refresh Velnor-owned locks only): procedure defined; refresh of
  `Cargo.lock`/policy/generator/runner locks is a maintainer action, tool
  files get recommendations only. Residual NEEDS-HUMAN: same as VER-3.3 —
  human execution per update set.

## Deferred (not a V1 gate failure)

- VER-4.6 (`turso` exact + journal/txn/lock/durability/crash qual, Cloud
  sync disabled): sanctioned V2 deferral. No `turso` in manifests by
  design. Unblock = runner implementation starts (post-V1 gates) and
  `.velnor/runner.lock` binds the full runner identity. See
  [deferred work](../deferred/README.md).
