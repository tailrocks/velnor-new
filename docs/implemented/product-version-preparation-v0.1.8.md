# Product version preparation for v0.1.8

- State: source-only release preparation from `origin/main` at
  `4c9a54ac71d3425440864cebb2496e6ed0bc4640`, including the Lychee pinned
  release-asset checksum correction. The source candidate is v0.1.8; no tag or
  release was present at preparation time. No release has been created.
- Version owners: the root workspace version metadata and its ten workspace crates use
  0.1.8. All exact local path requirements and the ten workspace lock entries
  were refreshed. The separately excluded `crates/velnor-runner` remains
  independently versioned. The generator release renderer now targets tag
  `v0.1.8` and its Linux x86_64, macOS arm64, and macOS x86_64 binary,
  checksum, and provenance asset names. The matching source fixtures and
  generated snapshots were updated.
- Preserved inputs: `.velnor/config.toml`, tool policy and upstream pins,
  external dependency versions, and the historical static
  `fixtures/consumer-release-manifest.json` were not changed. The Lychee
  release checksum fix from the base commit remains unchanged.
- Generated output: the supported CLI rendered to a separate
  `--output-dir` preview. Its `.github` path/type inventory and symlink targets
  matched the checked-in tree. Only the 25 byte-different generated files were
  copied from the preview. `diff -qr .github ../tailrocks-v018-preview/.github`
  passed afterward. No generated workflow was hand-edited.
- OpenTofu capture: `scripts/capture-opentofu-goldens.sh capture
  target/debug/velnor-actions` refreshed the five cases. The dogfood preview
  reported `tree identical`. The subsequent `check` passed nested,
  mbx-nextest, empty-suite, minimal-cargo, and dogfood. All entries in
  `docs/proposed/opentofu-goldens/MANIFEST.sha256` verified.
- CLI parity: `VELNOR_UPDATE_GOLDENS=1 cargo test --locked --offline -p
  velnor-actions-cli --test velnor_cli parity_` captured four cases; the
  default comparison passed all four.
- Focused checks passed: renderer `generator_release` (18 tests), renderer
  `product_release` (25), orchestrator `schema2` snapshots (20), `cargo check
  --workspace --offline`, `cargo fmt --all -- --check`, Actionlint,
  `alint validate-config`, `alint check --fail-on-warning` (51 rules pass;
  three informational notices), `scripts/check-freshness.sh`, and
  `git diff --check`.
- Clippy was not run: the installed Homebrew Cargo/Rust is 1.99.0, while this
  repository pins Rust 1.98.1 and the pinned toolchain is unavailable in the
  local Mise setup. Hosted PR CI remains the authoritative pinned-toolchain
  check. Full workspace tests and `verify-local.sh` were not run for this
  version-only release-preparation change.
- This preparation does not establish exact-main release eligibility, protected
  three-target hosted qualification, attestations, publication, or consumer
  adoption. Those require the canonical product-release workflow after this
  source change is integrated.
