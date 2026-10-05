# Scoped cargo-deny evidence: native trailer helper dependency

Status: **PASS for the `velnor-actions-cli` package graph only.** This is not a whole-workspace cargo-deny result and does not replace the steward-owned locked workspace gate.

Collected 2026-10-05 UTC under the independently attested `/root/consumer_delta_execution` rollout. The latest verified owned context was Luna/max at ordinal 7094, timestamp `2026-10-05T16:51:00.403Z`; rollout snapshot SHA-256 `c873027f97a7eb780675cc1fff5d8e44c42ad865fc3cb121e40e366ab05326e7`.

## Source and pins

The candidate is a private derivative of current main `2217ad1b53209fc19895912ffc0dde7c4fcf7915`, tree `216a1dcd0cc2665bba13584ea5fb577a472f5cef`, under `/root/.cache/velnor-task-targets/20261005/trailer-maintenance-2217ad1-candidate/source`. Its approved trailer-retirement delta is patch SHA-256 `919dde7f8cbeb01360f8913cb22e23a53c88925676c6e738f3e860cda3fc882f`; retirement review SHA-256 `3984b4ab5b00e6839698c0eeab56ad5a1e4f69ffbbc12adc94f8f2f2db86cf20`.

The candidate regular-file manifest check passed 1533/1533 entries (`post-deny-candidate-manifest-check.log`, SHA-256 `4cab4644cef0e40b944409059f901ad199db4bbe4a16f5dbe028cda6015a605f`). The package manifest containing `unicode-general-category = "=1.1.0"` has SHA-256 `ea8a49418d823b73e27bef6be87be0c41951f1e8ea428bcd2ce377136cb79a70`. `Cargo.lock` stayed unchanged at SHA-256 `5b54f72eb8a4fa1bde6aa7232481769e4f33defeb39d3a9857ad93cfd3c12229`; the new lock entry resolves from crates.io with checksum `0b993bddc193ae5bd0d623b49ec06ac3e9312875fdae725a975c51db1cc1677f`.

Tool pins are present in repository policy: `.github/workflows/ci.yml` (SHA-256 `6950a77b522f432ac481a4d0e552852eec2e0abc0ea0a22e51e54452bb3b3b5c`) and `scripts/verify-local.sh` (SHA-256 `d1aec591a1c1519da01f58a7d8e1bfe411cd3f82d56feaa8b9ccdff0d844a19b`) pin cargo-deny 0.20.2; `mise.toml` (SHA-256 `8bdb98902de6341bfaa670178cb1d06337c2564571ef2e8f3938032653b8d74f`) pins mr-boxington 1.21.1. The root `deny.toml` SHA-256 is `862f83c22d65f2a18124f3ccda83865f531623c5d98fcb1dd3a773a2d3fca20f`.

## Tool provenance

Installed only into private task-scoped Mise directories using the repository-pinned versions; no global tool installation was made. The cargo-deny release API asset was `cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz`, advertised SHA-256 `9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f`. Mise logged API-digest checksum verification; a separate fetch of that official asset matched the same digest, and the extracted binary matched the installed executable SHA-256 below. The raw release JSON SHA-256 is `3f42a746e7a87d88572b3ef8f39ca06d82e86d78406b2768828a7b715aa36664`; response headers SHA-256 is `cc3deaf182779f8fdc7961ea2ba9dd90fec83255ee3feb9271b005fbc00f5a61`.

- `cargo-deny 0.20.2`: `/root/.cache/velnor-task-targets/20261005/trailer-deny-mise-data/installs/cargo-deny/0.20.2/cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny`, executable SHA-256 `b329e25933d01c36dd7c47d84ea5716694f9b7caf53a5003d45674703a8ed54a`.
- `mr-boxington 1.21.1`: `/root/.cache/velnor-task-targets/20261005/trailer-deny-mise-data/installs/mr-boxington/1.21.1/mbx`, executable SHA-256 `97984b8c92953cefc027014d24c8abf8773f2d12ded0385156d2050da8d5fb8c`. Mise verified the Packslip Sigstore signature for the pinned mr-boxington release workflow and checked the archive checksum.
- Installation log: `mise-install.log`, SHA-256 `82e659aec1fbccc2e3a7998add2ad47b5a282e682f25202086542ec13bcb5ca2`.
- Rust toolchain used: pinned rustup `1.98.1-x86_64-unknown-linux-gnu`; cargo and rustc both reported 1.98.1.

## Check and result

The check used private `CARGO_HOME`, `CARGO_TARGET_DIR`, Mise data/cache/download directories, and `MBX_CACHE_DIR`; `CARGO_BUILD_JOBS=1`. GitHub token variables were unset. The executed command was:

```sh
mise --no-config --no-env --no-hooks --jobs 1 exec cargo-deny@0.20.2 mr-boxington@1.21.1 -- \
  cargo deny --locked \
    --manifest-path /root/.cache/velnor-task-targets/20261005/trailer-maintenance-2217ad1-candidate/source/crates/velnor-actions-cli/Cargo.toml \
    --config /root/.cache/velnor-task-targets/20261005/trailer-maintenance-2217ad1-candidate/source/deny.toml \
    check
```

Exit status was 0. The check reported `advisories ok, bans ok, licenses ok, sources ok`. Its non-fatal warnings are: configured BSD-2-Clause, BSD-3-Clause, and ISC allowances were not encountered in this package graph; and the graph contains both winnow 0.7.15 and 1.0.4 while policy sets duplicate versions to warn. `cargo deny list` for the same manifest/config lists `unicode-general-category@1.1.0` under the allowed `Apache-2.0` license.

Raw result logs:

- `cargo-deny-native-cli-check.log`, SHA-256 `0d4e6b19d247610da35598796723ba1af5c411a478911a87566bd49cbde549ad`.
- `cargo-deny-cli-list.log`, SHA-256 `a2e20a4ef890bb57a5990fac19942c1e5355d56c92382b3848b4d6b528a00ca2`.
- `post-deny-source-delta.txt`, SHA-256 `1a31bac3197217c0243b47224ca9a4452a690f6387fc894756e743c6cc760918`.

No Python was executed or created, and no shared repository, Git index, branch, or ref was changed during this gate. The remaining cargo-deny gate is the actual locked whole-workspace run by the steward on the integrated candidate and their current Mise toolchain.
