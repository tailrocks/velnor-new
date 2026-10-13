# Mise platform binary pins

Named checks use the repository's qualified Mise `2026.10.4` pin. For a
typed native check, the generator selects the SHA-256 for that runner target;
the `jdx/mise-action` `sha256` input identifies the installed executable.

On 2026-10-08, each raw executable was downloaded from the official Mise
`v2026.10.4` release and its SHA-256 and byte count were measured locally.
The measured digests match the corresponding entries in the release's
[`SHASUMS256.txt`](https://github.com/jdx/mise/releases/download/v2026.10.4/SHASUMS256.txt).

| Target | Official raw executable | SHA-256 | Bytes |
| --- | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | [`mise-v2026.10.4-linux-x64`](https://github.com/jdx/mise/releases/download/v2026.10.4/mise-v2026.10.4-linux-x64) | `2b8ce21f550872807bcaabf45b6bc5c64bfbd6dc3bf49dd4e67de700ef3ceb75` | 158829568 |
| `aarch64-apple-darwin` | [`mise-v2026.10.4-macos-arm64`](https://github.com/jdx/mise/releases/download/v2026.10.4/mise-v2026.10.4-macos-arm64) | `5c530143fc750e8a98c9a36be8d361e5dd953fa0b004d58f7577783f7cf2ac24` | 125641616 |
| `x86_64-apple-darwin` | [`mise-v2026.10.4-macos-x64`](https://github.com/jdx/mise/releases/download/v2026.10.4/mise-v2026.10.4-macos-x64) | `9f58d924a4d7b47aeb1610cd981beca2125907b2591d805237efbca8aae4308e` | 153265264 |

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

Confirm that both hosted jobs pass for the dispatch SHA. A successful run
qualifies that candidate executable on those two hosted targets; it does not
change `.velnor/version-policy.toml` or the named-check pins above. Keep the
current pin record unchanged until the separate pin update is reviewed and
merged. The candidate version and platform digests are compiled into the
generator, so update them and regenerate the workflow when qualifying a later
Mise release.
