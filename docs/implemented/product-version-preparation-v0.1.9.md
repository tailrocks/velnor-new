# Product version preparation for v0.1.9

- State: source-only release preparation from `origin/main` at
  `e34cfb836761d5b9e209802c056b83736afd816c`, including follow-up fixes for
  repository `rust-toolchain.toml` channel validation, `[tools]` alias
  validation, and archived suite extraction (`--extract-overwrite`). The source
  candidate is v0.1.9; no tag or release was present at preparation time. No
  release has been created.
- Version owners: the root workspace version metadata and its ten workspace
  crates use 0.1.9. All exact local path requirements and the ten workspace lock
  entries were refreshed. The separately excluded `crates/velnor-runner` remains
  independently versioned. The generator release renderer now targets tag
  `v0.1.9` and its Linux x86_64, macOS arm64, and macOS x86_64 binary,
  checksum, and provenance asset names. The matching source fixtures and
  generated snapshots were updated.
- Preserved inputs: `.velnor/config.toml`, tool policy and upstream pins,
  external dependency versions, and the historical static
  `fixtures/consumer-release-manifest.json` were not changed.
- Generated output: `cargo run --locked -p velnor-actions-cli -- generate`
  regenerated `.github/` cleanly, and a separate `--output-dir` preview verified
  zero drift against the committed `.github` tree. No generated workflow was
  hand-edited.
- OpenTofu capture: `scripts/capture-opentofu-goldens.sh capture
  target/debug/velnor-actions` refreshed the five cases. The dogfood preview
  reported `tree identical`. The subsequent `check` passed nested,
  mbx-nextest, empty-suite, minimal-cargo, and dogfood. All entries in
  `docs/proposed/opentofu-goldens/MANIFEST.sha256` verified.
- CLI parity: `VELNOR_UPDATE_GOLDENS=1 cargo nextest run --locked -p
  velnor-actions-cli --test velnor_cli parity_` captured four cases; the
  default comparison passed all four.
- Verification suite: `cargo fmt --all -- --check`, `cargo clippy --locked
  --workspace --all-targets -- -D warnings`, `cargo nextest run --locked
  --workspace`, `cargo doc --workspace --no-deps`, `alint validate-config &&
  alint check --fail-on-warning`, `cargo deny check`, and
  `bash scripts/check-freshness.sh` all passed.
- This preparation does not establish exact-main release eligibility, protected
  three-target hosted qualification, attestations, publication, or consumer
  adoption. Those require the canonical product-release workflow after this
  source change is integrated.
