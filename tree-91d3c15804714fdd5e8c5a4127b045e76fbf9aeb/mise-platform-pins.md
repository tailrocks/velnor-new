# Mise platform binary pins

Named checks use the repository's qualified Mise `2026.10.5` pin. For a
typed native check, the generator selects the SHA-256 for that runner target;
the `jdx/mise-action` `sha256` input identifies the installed executable.

On 2026-10-09, the raw macOS ARM64 executable was measured locally. Its SHA-256
and byte count match the official `v2026.10.5` release's
[`SHASUMS256.txt`](https://github.com/jdx/mise/releases/download/v2026.10.5/SHASUMS256.txt).
The Linux x64 and macOS x64 digests and byte counts match the same official
checksum list and release asset metadata.

| Target | Official raw executable | SHA-256 | Bytes |
| --- | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | [`mise-v2026.10.5-linux-x64`](https://github.com/jdx/mise/releases/download/v2026.10.5/mise-v2026.10.5-linux-x64) | `8a223b5f8ca71100220a3e5bef259614c348e7b1d80e6b15c2a9c9aa3affe5e4` | 158935888 |
| `aarch64-apple-darwin` | [`mise-v2026.10.5-macos-arm64`](https://github.com/jdx/mise/releases/download/v2026.10.5/mise-v2026.10.5-macos-arm64) | `41c4028257d30f5f5742c99247c461f417143d6c7301f167a0c185247c8f206e` | 126420400 |
| `x86_64-apple-darwin` | [`mise-v2026.10.5-macos-x64`](https://github.com/jdx/mise/releases/download/v2026.10.5/mise-v2026.10.5-macos-x64) | `204c7d64e8b0b62c0a95847ab6442bf23bf88247d52967e99c5cad53f56eaa0c` | 154143904 |

The measured byte counts are the per-target upper bounds used when projecting
the selected executable into an owned check home. Unsupported targets fail
generation; platform-specific digests and byte limits stay paired.

The locally installed executable reports
`2026.10.5 macos-arm64 (2026-10-08)` and its measured digest and size match the
macOS ARM64 row above. Native execution and version-banner qualification were
available here only for macOS ARM64. Linux x64 and macOS x64 native checks were
not run locally; their binary identities are backed by the official checksum
list and release asset metadata and still require the matching hosted runner
qualification.

The official [v2026.10.5 release notes](https://github.com/jdx/mise/releases/tag/v2026.10.5)
were reviewed for compatibility. The Java shorthand default change and the
new `experimental = true` requirements do not affect this repository: its
runtime configs select no Java shorthand and enable none of the affected
experimental features. The repository uses the root `mise.toml` and pinned
`mise exec` / `mise install` invocations; no configuration migration was
needed for the qualified runtime paths.
