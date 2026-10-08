# Release gates: source and external evidence

This record separates checked-in release mechanics from hosted evidence and
infrastructure. The official `v0.1.4` CLI release was published on
2026-10-08 from main commit `d3590d321e51f7b99bfb0a92c11d3d6eb7af61cd`;
its signed release manifest lists the Linux x86_64, macOS ARM64, and macOS
x86_64 assets and their digests. This candidate prepares the next immutable
CLI version, `v0.1.5`; its hosted qualification and publication remain
pending. The renderer emits a source-bound, dispatch-only
`.github/workflows/product-release.yml` coordinator with reusable image,
binary, and generator modules. The generator module includes candidate
qualification, a same-run manifest, source-bound attestations, and protected
publication. Source and local tests do not prove a hosted qualification run,
an immutable release for this candidate, or GitHub environment protection.

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
- BOOT-2.1 (release assets): `v0.1.4` PUBLISHED; `v0.1.5` CANDIDATE NOT YET
  QUALIFIED. The source-bound workflow builds three
  target binaries, admits each candidate TAR through the checkout-owned native
  guard before parsing, and binds qualification, attestations, and the
  canonical versioned manifest to the same measured artifact bytes. The
  existing `v0.1.4` release contains all three CLI assets, a versioned manifest,
  checksums, and provenance. The current `v0.1.5` candidate still needs an
  exact-source hosted qualification, protected publication, and a separate
  review of its published bytes before consumer provenance is updated.
- BOOT-4.7 (protected release job): SOURCE-IMPLEMENTED, INFRASTRUCTURE
  UNVERIFIED. The renderer emits a dispatch-only coordinator and a generator
  publisher with a protected `generator-release` environment, serialized
  publication, source/CI rechecks, immutable-tag preflight, and
  digest/attestation verification. Unblock = verify the repository's actual
  environment rules and branch protections, complete a qualified hosted run,
  and land the bootstrap-lock update in a SEPARATE reviewed change while
  ordinary CI keeps using the previous seed.
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
  expired holds. Unblock = a successful exact-source protected workflow run.
- VER-3.2 / VER-3.3 (update machinery): procedure + Renovate proposals +
  script gates exist. Residual NEEDS-HUMAN: a maintainer MUST assemble each
  update set, record timestamp+delta, and run full qualification. No updater
  binary exists and none is planned for V1 — this is the mechanical maximum.
- VER-3.7 (merge-after-qual): SOURCE CHECKS EXIST; BRANCH PROTECTION
  UNVERIFIED. The release gate checks the exact-source main CI run and its
  `Required` job before building and repeats that check before publication.
  Unblock = verify that repository branch protection requires the intended
  CI qualification checks for merge; generated workflow source alone cannot
  establish that setting.
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
