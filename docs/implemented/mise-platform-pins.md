# Mise platform binary pins

Named checks use the repository's adopted Mise `2026.10.6` pin. Hosted
qualification is pending; the freshness inventory records that state. For a
typed native check, the generator selects the SHA-256 for that runner target;
the `jdx/mise-action` `sha256` input identifies the installed executable.

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

## Pre-merge hosted qualification

The generated `qualification.yml` workflow has a `mise-pin` mode for checking
a candidate Mise release before changing the repository's adopted runtime pin.
The mode checks out the selected ref at the dispatch event's exact `github.sha`,
then verifies the Mise version and executable SHA-256 on GitHub-hosted Linux
x64 (`ubuntu-26.04`) and macOS x64 (`macos-15-intel`). It is read-only and does
not publish Velnor or generator artifacts.

Dispatch the candidate ref after it contains the generated qualification mode:

```sh
gh workflow run qualification.yml \
  --repo tailrocks/velnor-new \
  --ref <candidate-ref> \
  --field mode=mise-pin
```

Confirm that both hosted jobs pass for the dispatch SHA before merging this
pin update. A successful run qualifies the candidate executable on those two
hosted targets. The inventory keeps `qualified` at the prior release and uses
`pending-qualification` until that evidence exists; the freshness gate will
remain red during this candidate state. The candidate version and platform
digests are compiled into the generator, so update them and regenerate the
workflow when adopting a later Mise release.
