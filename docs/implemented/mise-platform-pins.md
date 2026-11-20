# Mise platform binary pins

Generated workflows pin the official Mise release with the raw executable
SHA-256 as the `sha256` input of every `jdx/mise-action` step (issue #2158).
The renderer holds the digests (`MISE_BINARIES` in
`crates/velnor-render/src/mise_digest.rs`, next to `MISE_VERSION`). A job whose
runner labels name one platform gets that platform's digest; any other job
selects it at run time from `runner.os` and `runner.arch`, and a platform
without a row selects a text that matches no binary, so the action fails
instead of installing an unchecked executable. The design
and the bump procedure are in [docs/rust-ci.md](../rust-ci.md#the-mise-binary-is-pinned-by-sha-256).

The digests come from the official
[`SHASUMS256.txt`](https://github.com/jdx/mise/releases/download/v2026.11.10/SHASUMS256.txt)
of the release and agree with the `digest` field of each release asset in the
GitHub releases API. The raw executable equals the binary inside the release
archive that the action downloads and extracts (checked for `linux-x64`: the
extracted `mise/bin/mise` has the digest below).
`scripts/mise-binary-digests.sh --check` repeats the comparison.

| `runner.os`-`runner.arch` | Official raw executable | SHA-256 |
| --- | --- | --- |
| `Linux-X64` | `mise-v2026.11.10-linux-x64` | `b603a4f48563fc379a794d9ea2cf9f709ed276c9141068604cb0c59fa3e674a9` |
| `Linux-ARM64` | `mise-v2026.11.10-linux-arm64` | `86ef275d541c5243cd20b1c0fcd16810e5f3601113d8db09278d0608617a3833` |
| `macOS-ARM64` | `mise-v2026.11.10-macos-arm64` | `3f4ba9d21c6f2170048d0870fd98ab287e04730b76a6fc8e83964d59c80eb704` |
| `macOS-X64` | `mise-v2026.11.10-macos-x64` | `7eab8054d301b75dcbd93440ff184bee6864d95799cfdc4a00ce858313b68783` |

## History

The old engine (velnor-actions 0.1.78) pinned the same digests per target. The
rewrite dropped them, so the generated steps passed only `version` until #2158.
The previous table (`v2026.10.8`, captured `2026-10-12T23:08:53Z`, receipt in
[`freshness-upstream-probe-2026-10-13.json`](freshness-upstream-probe-2026-10-13.json))
qualified Linux x64 and macOS x64 in the exact-source
[`mise-pin` run 38321521320](https://github.com/tailrocks/velnor-new/actions/runs/38321521320).
