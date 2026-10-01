# Release gates: NEEDS-HUMAN unblock conditions

Rows that no script, config, or doc edit can close. Each names the precise
condition that unblocks it and what already exists mechanically. The
protected release job's workflow emission belongs to the renderer stream;
this file seeds the procedure side only.

## Seed and first release (BOOT)

- BOOT-4.2 (seed v0): NEEDS-HUMAN. Unblock = a manually reviewed build with
  pinned Mise + MBX (no Velnor binary), 2 distinct admin approvals, and an
  independent reproducible rebuild (second party, pinned catalog, sha256
  match) recorded in the seed PR (Gap E review). Pre-seed is trust-on-review.
- BOOT-2.1 (release assets): NEEDS-HUMAN. Unblock = first release publishes
  per-target immutable binary assets + versioned manifest (target/URL/
  SHA-256). Manifest schema code + round-trip/tamper tests already exist.
- BOOT-4.7 (protected release job): NEEDS-HUMAN + NEEDS-INFRA. Unblock =
  (a) renderer emits the protected release job (publishes qualified
  candidate only, digests verified); (b) branch protection confines it to
  the release lane; (c) lock update lands in a SEPARATE reviewed change
  while ordinary CI keeps using the previous bootstrap.
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
- VER-2.18 (stale default blocks release): NEEDS-HUMAN. Unblock = release
  pipeline exists (BOOT-4.7) AND its gate calls `scripts/check-freshness.sh`
  (which already fails stale/missing/mismatched/expired holds). The script
  half is done; the release gate to call it does not exist yet.
- VER-3.2 / VER-3.3 (update machinery): procedure + Renovate proposals +
  script gates exist. Residual NEEDS-HUMAN: a maintainer MUST assemble each
  update set, record timestamp+delta, and run full qualification. No updater
  binary exists and none is planned for V1 — this is the mechanical maximum.
- VER-3.7 (merge-after-qual): build-flag half proven (`--locked --offline`
  everywhere). Residual NEEDS-HUMAN: branch protection MUST require the
  qualification gate before merge; no protection rules exist yet.
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
