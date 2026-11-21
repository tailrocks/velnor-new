# Mise platform binary pins

Generated workflows pin the official Mise release with the raw executable
SHA-256 as the `sha256` input of every `jdx/mise-action` step (issue #2158).
The renderer holds the digests (`MISE_BINARIES` in
`crates/velnor-render/src/mise_digest.rs`, next to `MISE_VERSION`). A job on a
runner declared `hosted = true` that names one platform gets that platform's digest
(a Linux label only when it starts with `ubuntu-`); any other job selects it at run time from
`runner.os` and `runner.arch`. Windows and Linux
are not pinned by that selection: it is an empty `sha256`, so the action keeps
its signed-archive check, which covers `mise-shim.exe` on Windows and the
binary that suits the runner's libc on Linux (a musl runner downloads a
different binary than the glibc digest names). Any other platform without a row
selects a text that matches no binary, so the action fails instead of
installing an unchecked executable.

The literal for an `ubuntu-*` label assumes a glibc runner, and it applies only
to a job on a runner declared GitHub-hosted (`hosted = true`, or the default
`ubuntu-latest`). A self-hosted pool is fenced (`hosted = false`) and gets the
run-time selection, which has no Linux row, so a musl pool behind an `ubuntu-*`
label keeps the action's signed-archive check and downloads its own binary. A
self-hosted pool whose label is spelled like a GitHub-hosted one (`ubuntu-*`)
therefore has to say `hosted = false`; that key is required only for such a
spelling. Declaring it `hosted = true` claims a GitHub-hosted glibc runner. A
`trusted_runner` pool is always self-hosted and never gets the literal. The design
and the bump procedure are in [docs/rust-ci.md](../rust-ci.md#the-mise-binary-is-pinned-by-sha-256).

| `runner.os`-`runner.arch` | Official raw executable | SHA-256 |
| --- | --- | --- |
| `Linux-X64` | `mise-v2026.11.10-linux-x64` | `b603a4f48563fc379a794d9ea2cf9f709ed276c9141068604cb0c59fa3e674a9` |
| `Linux-ARM64` | `mise-v2026.11.10-linux-arm64` | `86ef275d541c5243cd20b1c0fcd16810e5f3601113d8db09278d0608617a3833` |
| `macOS-ARM64` | `mise-v2026.11.10-macos-arm64` | `3f4ba9d21c6f2170048d0870fd98ab287e04730b76a6fc8e83964d59c80eb704` |
| `macOS-X64` | `mise-v2026.11.10-macos-x64` | `7eab8054d301b75dcbd93440ff184bee6864d95799cfdc4a00ce858313b68783` |
