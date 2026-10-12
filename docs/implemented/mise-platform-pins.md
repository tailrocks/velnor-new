# Mise platform binary pins

Named workflow checks pin the official Mise `v2026.10.8` release and use
the target-specific raw executable SHA-256 as the `jdx/mise-action` input.
The workflow generator selects only the digest for the job's exact target.
The full official [`SHASUMS256.txt`](https://github.com/jdx/mise/releases/download/v2026.10.8/SHASUMS256.txt)
was captured at `2026-10-12T23:08:53Z`; its SHA-256 is
`9237b178f5f3d1625045c18608e4bde6f66a023593822e8ae9029967c19474b0`.
The raw receipt is recorded in
[`freshness-upstream-probe-2026-10-13.json`](freshness-upstream-probe-2026-10-13.json).

| Target | Official raw executable | SHA-256 |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | [`mise-v2026.10.8-linux-x64`](https://github.com/jdx/mise/releases/download/v2026.10.8/mise-v2026.10.8-linux-x64) | `c72477ae6c3a8e7d817a322f43da70e4643e75ee5c21265f1c19c894273b7428` |
| `aarch64-apple-darwin` | [`mise-v2026.10.8-macos-arm64`](https://github.com/jdx/mise/releases/download/v2026.10.8/mise-v2026.10.8-macos-arm64) | `dd77de9a7574e38d8409fb06fa85e1581415896639d075987791d8c4e5a57b73` |
| `x86_64-apple-darwin` | [`mise-v2026.10.8-macos-x64`](https://github.com/jdx/mise/releases/download/v2026.10.8/mise-v2026.10.8-macos-x64) | `a122fd04db5f2da07d24528ed61109bc82142ca44e821d4faafa05650f6ba627` |

The source-build bootstrap also accepts Linux ARM64 and macOS ARM64 archives.
Their archive and extracted executable hashes are checked against the same
official release manifest and captured archive bytes; the targeted
`mise_linux_arm64_archive_uses_official_release_checksum` test guards the
Linux ARM64 archive digest used by the source-build catalog.

## Hosted qualification

Qualification for `2026.10.8` passed in the exact-source [`mise-pin` run
38321521320](https://github.com/tailrocks/velnor-new/actions/runs/38321521320),
dispatched against source SHA
`438bd90d2ef64515373f413b501b7ae01189a8e7`. Both [Linux x64
job 115050479288](https://github.com/tailrocks/velnor-new/actions/runs/38321521320/job/115050479288)
and [macOS x64
job 115050479238](https://github.com/tailrocks/velnor-new/actions/runs/38321521320/job/115050479238)
passed the installed binary version and digest checks. This qualification
covers those two hosted targets only. It does not qualify the generator
release or establish qualification for Ubuntu `24.04`/`22.04` runner labels.
