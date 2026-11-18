# 1290 gap A: raw binary asset format

Opt-in key `format = "raw"` on a `[[binaries]]` entry. Default (`"archive"`) output is byte-identical: every existing golden passes untouched, and a test renders the default and `format = "archive"` and compares the text.

Raw means gnu targets (`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`), no musl toolchain, and per platform three assets: `<bin>-<platform>`, `<bin>-<platform>.sha256` (`sha256sum` / `shasum -a 256` line, `<hex>  <name>`), `<bin>-<platform>.provenance.json`. `SHA256SUMS` still covers all of them because `velnor-ci release publish` requires it.

## Proof

Generated velnor-new's binary release workflow with `[[binaries]] package = "velnor-ci", format = "raw"` (the release-bin-raw sample with the package renamed) and compared with the old workflows in this tree.

Asset names versus `.github/workflows/release-rust-ci-tools.yml` (the `velnor-ci-<platform>` consumer names):

```
old: velnor-ci-linux-arm64 velnor-ci-linux-x64 velnor-ci-macos-arm64 
new: velnor-ci-linux-arm64 velnor-ci-linux-x64 velnor-ci-macos-arm64 
diff: none (identical)
```

Full new asset list:

```
velnor-ci-linux-arm64
velnor-ci-linux-arm64.provenance.json
velnor-ci-linux-arm64.sha256
velnor-ci-linux-x64
velnor-ci-linux-x64.provenance.json
velnor-ci-linux-x64.sha256
velnor-ci-macos-arm64
velnor-ci-macos-arm64.provenance.json
velnor-ci-macos-arm64.sha256
```

Checksum file versus the old `product-release-generator.yml` sidecar (`sha256sum <asset> > <asset>.sha256`, one line `<64 hex>  <asset>`): identical format; new step is `sha256sum velnor-ci-linux-x64 > velnor-ci-linux-x64.sha256` (`shasum -a 256` on macOS, as the old macOS jobs do). The same `awk` sidecar check from the old workflow accepts the file.

## Remaining differences from the old engine

- Old `product-release-generator.yml` names assets `velnor-actions-<version>-<triple>`; the `velnor-ci-<platform>` consumer names come from `release-rust-ci-tools.yml`, which has no `.sha256` or `provenance.json`. The raw format combines the consumer names with the generator's sidecars; no old workflow had both.
- `provenance.json` carries `schema`, `tag`, `repository`, `commit`, `target`, `asset`, `sha256`. Old also has `version` and `toolchain.rust`/`toolchain.mr-boxington` (velnor-specific toolchain pins); not reproduced.
- Release also holds `SHA256SUMS`; the old generator workflow publishes none (`release-rust-ci-tools` does).
- Old release also ships `COMMIT` and `velnor-gen-*` in the same release (gap B, multi-binary).
- Not covered: the macOS Intel target of the old generator workflow.
