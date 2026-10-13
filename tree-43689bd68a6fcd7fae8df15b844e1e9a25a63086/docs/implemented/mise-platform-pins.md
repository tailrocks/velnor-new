# Mise platform binary pins

Named checks use the repository's adopted Mise `2026.10.6` pin. The exact
candidate source was qualified on GitHub-hosted Linux x64 and macOS x64 in
Qualification run [37995746931](https://github.com/tailrocks/velnor-new/actions/runs/37995746931)
at source commit `bbf2cb2df8380b173a0629d07cb862f4870588f8`. This evidence is
limited to those two hosted targets; it does not qualify the generator release
or the Velnor runner-image families. For a typed native check, the generator
selects the SHA-256 for that runner target; the `jdx/mise-action` `sha256`
input identifies the installed executable.

On 2026-10-09, each raw executable was downloaded from the official Mise
`v2026.10.6` release and its SHA-256 and byte count were measured locally.
The measured digests match the corresponding entries in the release's
[`SHASUMS256.txt`](https://github.com/jdx/mise/releases/download/v2026.10.6/SHASUMS256.txt).

| Target | Official raw executable | SHA-256 | Bytes |
| --- | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | [`mise-v2026.10.6-linux-x64`](https://github.com/jdx/mise/releases/download/v2026.10.6/mise-v2026.10.6-linux-x64) | `3f44343eebc7e0d6623bcea46e304864f02dff648edd75c82871b53cc697b366` | 161267824 |
| `aarch64-apple-darwin` | [`mise-v2026.10.6-macos-arm64`](https://github.com/jdx/mise/releases/download/v2026.10.6/mise-v2026.10.6-macos-arm64) | `bbcea7b0f844d026424a4c8335357a15a2f5c9e9132c9408de990d9be6f26101` | 126670800 |
| `x86_64-apple-darwin` | [`mise-v2026.10.6-macos-x64`](https://github.com/jdx/mise/releases/download/v2026.10.6/mise-v2026.10.6-macos-x64) | `70e1407e2fdc7a19f94db35745a8e5885b0e4bbdbfb34bfb7e3619d6230a8f70` | 154455968 |

The measured byte counts are the per-target upper bounds used when projecting
the selected executable into an owned check home. Unsupported targets fail
generation; platform-specific digests and byte limits stay paired.

## Hosted qualification on 2026-10-09

The generated `qualification.yml` workflow's `mise-pin` mode checked out the
dispatch SHA and verified the version and executable SHA-256 on each hosted
target. Both required jobs succeeded for the exact source above:

| Target | Runner label | Job | Result |
| --- | --- | --- | --- |
| Linux x64 | `ubuntu-26.04` | [114041252080](https://github.com/tailrocks/velnor-new/actions/runs/37995746931/job/114041252080) | Passed |
| macOS x64 | `macos-15-intel` | [114041252169](https://github.com/tailrocks/velnor-new/actions/runs/37995746931/job/114041252169) | Passed |

The workflow run completed successfully at `2026-10-09T21:50:11Z`. The
inventory records Mise `2026.10.6` as current and qualified on this evidence.
The run checks executable identity on these two targets only; it does not
publish Velnor, qualify the generator's release assets, or establish
qualification for Ubuntu `24.04` or `22.04` runner families. The candidate
version and platform digests are compiled into the generator, so update them
and regenerate the workflow when adopting a later Mise release.
