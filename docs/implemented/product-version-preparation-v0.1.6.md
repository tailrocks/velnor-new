# Product version preparation for v0.1.6

- State: version-owned source and generated outputs are prepared on
  `fix/release-0.1.6-qualified-generator`. The task branch is stacked on merge
  commit `f21e00dd619c0637ae19e3e3c703ae105d17ca8d`, whose parents include the
  version checkpoint and PR #120 source head `2a1d60547cd7c6b7646c655652c9c9ed28db6b69`.
  An actual integrated-main forward merge remains pending. This record does
  not qualify or publish a release.
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
- Supported `.github` generation used:

  ```text
  cargo run --locked --offline -p velnor-actions-cli -- generate --output-dir /private/tmp/velnor-version-0.1.6-preview-20261009
  cargo run --locked --offline -p velnor-actions-cli -- generate
  ```

  The generated preview and the in-place `.github` tree were byte-identical.
  The CLI listed 26 generated paths; 25 changed: `.github/AGENTS.md`,
  `.github/actionlint.yaml`, 15 action YAMLs, the tool-cache identity script,
  and seven workflow YAMLs. `.github/CLAUDE.md` was generated and remained
  byte-identical.
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
  Logs: `/private/tmp/velnor-version-0.1.6-opentofu-capture.log` and
  `/private/tmp/velnor-version-0.1.6-opentofu-check.log`.
- The documented CLI parity golden capture and compare each passed four tests:

  ```text
  VELNOR_UPDATE_GOLDENS=1 cargo test --locked --offline -p velnor-actions-cli --test velnor_cli parity_
  cargo test --locked --offline -p velnor-actions-cli --test velnor_cli parity_
  ```

  Focused release renderer tests passed 204 with one ignored; the orchestrator
  `schema2` snapshot/routing tests passed 20 with 709 filtered out. Formatting,
  the full workspace gates, and `verify-local.sh` have not been rerun on this
  stacked base.
- No v0.1.6 tag, release, draft, publication, or qualification was created or
  claimed. After PR #120 integrates into main, forward-merge that exact main
  head and repeat the required qualification from integrated source.
