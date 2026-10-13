# Product version preparation for v0.1.6

- State: candidate source preparation on `fix/release-0.1.6-qualified-generator`,
  stacked on PR #120 head `4c95ff70c3037da326e193ecce97ddc8e633ce67`. This
  does not qualify or publish a release. No v0.1.6 tag or release exists.
- Version decision: refreshed remote refs and GitHub releases on 2026-10-09
  show tags `v0.1.0` through `v0.1.5`; `v0.1.5` is allocated to tag
  `f1041f322c54cd7edfdef06afd307799eeaa801a` and draft release 407717552.
  The draft has not been published. The next unused version is `0.1.6`.
- Specification: [version policy](../proposed/version-policy.md) §§1–2 and
  [bootstrap and release contract](../proposed/bootstrap-and-release-contract.md)
  §§2–4. The release contract requires immutable version tags and a new patch
  version for a later source revision; it prohibits reusing the occupied
  `v0.1.5` tag.
- Version owners: the root workspace version in `Cargo.toml` is `0.1.6`,
  inherited by its ten workspace packages. Their local exact path-dependency
  requirements and the ten local package records in `Cargo.lock` match. The
  separate excluded `crates/velnor-runner` workspace stays at `0.1.0`; external
  Cargo pins and dependency lock entries are unchanged. The release renderer's
  `schema2_generator_release_assets.rs` owns the `0.1.6` release constant and
  three target asset names; `schema2_generator_release_archive.rs` uses those
  names for archive checks.
- Preserved inputs: `.velnor/version-policy.toml` and
  `.velnor/freshness-inventory.json` contain tool/action pins, not the product
  release version, and remain unchanged. `.velnor/generator.lock` is absent at
  this base and no official release manifest was created. The nested runner
  and the historical synthetic `fixtures/consumer-release-manifest.json` at
  `0.1.4` remain unchanged.
- Generated outputs pending: the checked-in `.github` tree, renderer snapshots,
  and OpenTofu dogfood captures still describe the prior `0.1.5` source. They
  will be reconciled after PR #120 integrates because its publisher action and
  snapshot and closely related publish fixtures are intentionally excluded
  from this checkpoint. Regenerate root output through `cargo run --locked
  -p velnor-actions-cli -- generate`; use
  `scripts/capture-opentofu-goldens.sh capture` followed by `check` for the
  OpenTofu cases. The generator publish action/snapshot and remaining
  version-bound release tests must then be refreshed from that same integrated
  source. Do not hand-edit managed outputs.
- Focused evidence: `cargo update --workspace --offline` updated exactly the
  ten local workspace package records in `Cargo.lock`; external dependencies
  did not change. `cargo fmt --all -- --check` passed. The CLI normalization,
  release asset-version, candidate sidecar, archive-consumer setup, and
  shell-quoting focused tests each passed. Full repository gates and release
  qualification remain pending. Source-only focused Clippy passed; the
  all-targets Clippy run also linted PR #120's protected
  `schema2_generator_release_publish_fixtures.rs` and reported its existing
  `release_json` function at 91/80 lines. That helper remains outside this
  version checkpoint.
- Follow-up: merge PR #120, forward-merge the integrated main into this branch,
  refresh generated outputs using the supported commands above, run the
  version and generated-tree checks, and obtain independent review. No tag,
  draft, release, or publication state was changed during this preparation.
