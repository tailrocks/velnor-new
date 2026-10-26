# Mise 2026.10.19 qualification

The catalog update uses the authenticated `jdx/mise` release `v2026.10.19`.
Its annotated tag resolves to commit
`860cdc0969db81b1a076b1eb4574bd4b49bdf4e0`, tree
`2fa25d5f04874ce37e374e25681fb35f4e9e5169`. The release checksum file has
SHA-256 `c5c1d5e7729f26a38aae11d1e375857952d6607d243d6c676d817c4f0eb68c28`;
its minisign signature verified with the previously qualified immutable
Mise source key.

All five release assets below were downloaded and matched both the release
API digest and signed checksum. For archives, the extracted executable also
matched the listed raw executable digest.

| Host | Asset | Bytes | SHA-256 | Executable bytes | Executable SHA-256 |
| --- | --- | ---: | --- | ---: | --- |
| Linux x86-64 | `mise-v2026.10.19-linux-x64` | 173449632 | `3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d` | 173449632 | `3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d` |
| Linux ARM64 | `mise-v2026.10.19-linux-arm64.tar.gz` | 60287151 | `f6dbd376928095d3bc8fde72eb1b27a8450d35c1795aa80c05aacf1ce42584aa` | 151874424 | `4a4ca53b28e5433c527f4f73fac1f98f992a17cf32ca136a58e2e375f790f627` |
| macOS ARM64 | `mise-v2026.10.19-macos-arm64.tar.gz` | 47764515 | `a8a581e115f4ebdbe2931c237949dbac7624bd8b4024bf3eb9da7981bde490c9` | 138238816 | `7dc61dfa71abbcda1c40d18440bc556b5d58b80e6042bc4f95fd106fa1bb3c66` |
| macOS x86-64 | `mise-v2026.10.19-macos-x64` | 169788224 | `8d5e1591811143f15334f690373cefb58c13bd6cbd0292008748fd777f6c99d8` | 169788224 | `8d5e1591811143f15334f690373cefb58c13bd6cbd0292008748fd777f6c99d8` |
| macOS x86-64 | `mise-v2026.10.19-macos-x64.tar.gz` | 63677701 | `6949650ed321f6c36455bd755e06487cd1cc232140c7025d602a62db41891f43` | 169788224 | `8d5e1591811143f15334f690373cefb58c13bd6cbd0292008748fd777f6c99d8` |

The raw Linux x86-64, macOS ARM64, and macOS x86-64 executable hashes are
the platform identities used by the renderer and hosted qualification. The
Linux ARM64 executable is recorded for source-build bootstrap only; it is
not added as a hosted check platform.

On 2026-10-26, the qualified macOS ARM64 binary passed the local `mise
--version` and `mise config get` checks. Hosted qualification
[40235179783](https://github.com/tailrocks/velnor-new/actions/runs/40235179783)
passed at source `51647beaa022513bf41976f6b068b1ec4a033126`. Linux x64 job
`121295108359` and macOS x64 job `121295108257` each checked out that source,
ran Mise 2026.10.19, and verified the executable against the platform digest
above. This proves the `mise-pin` mode; other qualification modes were skipped.

## Integrated repository verification

The integrated PR391 tree based on owner revision `99232ee` passed local
verification on 2026-10-26. Root and nested workspaces passed strict Clippy.
Their native `test-members` selections both returned no tests and exit status
0 through the guarded Cargo path; this records the empty selection result and
does not claim a per-child process guard. Freshness, `Alint`, shell syntax,
ShellCheck, and exact native generation reproduction passed. The generation
idempotence log at `/tmp/pr391-integrated-generate-idempotence.log` is empty.

The native HK formatter fixture also passed its real-tool red/green check. With
the prior wrapper configuration at `69e60c6`, real rustfmt changed files and
the write guard rejected the fix (`/tmp/pr391-native-hk-red-real-wrapper.log`).
With the native HK configuration, HK 2.8.0, Mise 2026.10.19, and rustfmt
1.99.1 formatted root and nested workspace sources, passed the real check,
rejected invalid nested Rust, and preserved the staged tree
(`/tmp/pr391-native-hk-green-final.log`). The fixture uses process-local trust
for its temporary copy of the repository Mise configuration and offline
controls; it does not use fake Cargo or guard tools.

Comparison with the concurrent Jackin hook work identified a separate admission
regression: native HK discovers manifests with an existence check, while the
former adapter rejected symlink manifests and physical paths outside the
repository before running tools. The Cargo builtins now share a small admission
prefix that preserves this boundary without dispatching Cargo operations or
rejecting successful formatter writes. The same real-HK fixture accepts a
symlink manifest without this prefix
(`/tmp/pr391-native-hk-admission-red-physical.log`) and rejects it explicitly
with the prefix (`/tmp/pr391-native-hk-admission-green-physical.log`). Both
workspace sources and the staged tree remain unchanged during the rejection.
This is manifest admission, not a sandbox for trusted executable HK config.

Hosted Linux and Intel macOS runtime qualification passed for the source
identified above. Later source revisions require their own qualification run.
