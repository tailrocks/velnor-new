# Product version preparation for v0.1.6

- State: version-owned source and generated outputs are prepared on
  `fix/release-0.1.6-qualified-generator`, forward-merged with integrated
  `origin/main` at `2edc5cad367f098fd295e5edd09e5f1fd79a9acb`. Supported
  generation, OpenTofu capture/check, parity capture/check, and focused release
  tests pass against that merged source. This record does not qualify or
  publish a release.
- Version decision: refreshed tags and GitHub release metadata on 2026-10-09
  show tags `v0.1.0` through `v0.1.5`; `v0.1.5` remains allocated to its
  existing tag and draft release. The next unused product version is `0.1.6`.
- Owners: the root workspace and its ten packages, exact local path
  requirements, and ten local `Cargo.lock` package entries use `0.1.6`. The
  release renderer owns the `0.1.6` version and three target asset names. The
  separate nested runner remains at `0.1.0`; tool-policy pins and external
  dependency records are unchanged.
- Preserved inputs: the static synthetic
  `fixtures/consumer-release-manifest.json` remains at `0.1.4`, as do
  `.velnor/version-policy.toml` and `.velnor/freshness-inventory.json`.
  No official release manifest was created.
- Supported `.github` generation was repeated after the main merge:

  ```text
  cargo run --locked --offline -p velnor-actions-cli -- generate --output-dir /private/tmp/velnor-version-0.1.6-post-main-20261009
  cargo run --locked --offline -p velnor-actions-cli -- generate
  ```

  Both commands ran with Mise 2026.10.4 and Rust/Cargo 1.98.1. The generated
  preview and in-place `.github` tree were byte-identical. The CLI listed 26
  generated paths; replay after the merge introduced no further `.github`
  differences from the prior prepared checkpoint.
- The 13 version-owned snapshots were captured from those exact CLI outputs:
  the 11 `generator-release-*` local action bodies and the
  `product-release.yml` and `product-release-generator.yml` workflow bodies.
  Their generated marker is omitted because the existing snapshot assertion
  helper adds it back. No generated YAML semantics were edited after capture.
- OpenTofu goldens were regenerated and checked with the same CLI binary:

  ```text
  scripts/capture-opentofu-goldens.sh capture /private/tmp/velnor-version-0.1.6-target/debug/velnor-actions
  scripts/capture-opentofu-goldens.sh check /private/tmp/velnor-version-0.1.6-target/debug/velnor-actions
  ```

  Both commands passed for nested, mbx-nextest, empty-suite, minimal-cargo,
  and dogfood. Dogfood reports `tree identical`; the capture refreshed the
  five case trees and their measured `tree.sha256` and `MANIFEST.sha256` data.
  The commands used `MISE_DATA_DIR=/tmp/velnor-110-mise-data-run2`,
  `CARGO_HOME=/tmp/velnor-110-cargo-home`, and the pinned Rust tool PATH.
  Logs: `/private/tmp/velnor-version-0.1.6-post-main-opentofu-capture.log`
  and `/private/tmp/velnor-version-0.1.6-post-main-opentofu-check.log`.
- The documented CLI parity golden capture and compare each passed four tests:

  ```text
  VELNOR_UPDATE_GOLDENS=1 cargo test --locked --offline -p velnor-actions-cli --test velnor_cli parity_
  cargo test --locked --offline -p velnor-actions-cli --test velnor_cli parity_
  ```

  Focused release renderer tests passed 204 with one ignored; orchestrator
  `schema2` snapshot/routing tests passed 20 with 709 filtered out; the merged
  atomic-generation regression passed 1 with 728 filtered out. Full workspace
  gates and `verify-local.sh` remain pending.
- No v0.1.6 tag, release, draft, publication, or qualification was created or
  claimed. The current task branch remains a preparation checkpoint and does
  not replace the required final qualification from the accepted integrated
  source.
