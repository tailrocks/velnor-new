# MBX 1.28.0 and Mr. Boxington action v1.7.3

**Checked:** 2026-10-26 14:11 UTC
**Status:** Local update-set qualification passed. Pull-request and protected-main gates remain.

## Upstream records

The GitHub release API for [MBX v1.28.0](https://github.com/jdx/mr-boxington/releases/tag/v1.28.0)
reports source commit `44f50c329a83dedd012e1e49cf9f265f79226a83` and source
tree `ccfdb5978fb74a7bd854a0cd9eeafa635619c50f`. Downloaded release archives
match the API's SHA-256 digests. The extracted `mbx` executable digests were
measured independently:

| Target | Archive SHA-256 | Extracted `mbx` SHA-256 |
|---|---|---|
| Linux x86_64 | `50ce9360221d9274a73d70c9d2d5285dc2d8353a101de0845e3da5bb0083bcfb` | `6d3e1afe2a91f83aeed762a576d0a2b3c990a82f7e3e45926e1cd4b922f0df1b` |
| Linux aarch64 | `9056d34520991f80c9f708d36008fcc732e500d735591a8649029d476cb566bd` | `bc9d537789f16da1e37b9ca64fde107246a902f916b989fe3f84b0a4d4bbd2a5` |
| macOS aarch64 | `ac73ba0637a443ba9c43354008f6bcd5d9e8a84b703e46f23202025c1f849472` | `068cf7b41d58b5dd630e56b4980650a4f57f5e3267abc6d16a614088f499cebf` |

The [Mr. Boxington action v1.7.3 release](https://github.com/jdx/mr-boxington-action/releases/tag/v1.7.3)
resolves to commit `88acf2bb50552c7c508cbac13e5f8c576b95b1de` (tree
`7dbe827d9bb69496e5533750e4aa157e929eac89`). Its release notes report that
`dist/index.js` now bundles `undici` 6.29.1, fixing two high-severity denial
of-service advisories (`GHSA-4qv8-2p9j-2685` and
`GHSA-xjj6-f33q-929c`). They state that cache restore, save, and configuration
behavior did not change. The compared release changes workflow files,
`CHANGELOG.md`, `dist/index.js`, `mise.lock`, `package.json`, and
`package-lock.json`; it adds `rebuild-dist.yml`. It does not change
`action.yml` (blob `3f6670f6dcc3cf709cb2580d1ce0da590169acbe` at both tags).

## Verification

Commands used to verify MBX release artifacts:

```sh
curl -fsSL https://github.com/jdx/mr-boxington/releases/download/v1.28.0/mbx-x86_64-unknown-linux-gnu.tar.gz -o mbx-linux-x86_64.tar.gz
sha256sum mbx-linux-x86_64.tar.gz
tar -xOzf mbx-linux-x86_64.tar.gz mbx | sha256sum
```

The same archive and extraction checks passed for Linux aarch64 and macOS
aarch64. GitHub release and commit API records supplied the source commit,
tree, action SHA, and official archive digests.

Cosign Packslip verification passed for all three archives with the exact
identity
`https://github.com/jdx/mr-boxington/.github/workflows/release.yml@refs/heads/main`,
issuer `https://token.actions.githubusercontent.com`, and predicate type
`https://packslip.dev/release/v1`. GitHub attestation verification also passed
for all three archives with repository `jdx/mr-boxington` and signer workflow
`.github/workflows/release.yml`. An initial Cosign check expected a tag-ref
identity and failed. The signed bundle reported the workflow's `main` identity;
verification then passed with that exact identity. The failed check is retained
here to distinguish the inspected signer from the incorrect assumption.

The successful command form was:

```sh
cosign verify-blob-attestation \
  --bundle packslip.sigstore.json \
  --certificate-identity 'https://github.com/jdx/mr-boxington/.github/workflows/release.yml@refs/heads/main' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --type https://packslip.dev/release/v1 mbx-x86_64-unknown-linux-gnu.tar.gz
gh attestation verify mbx-x86_64-unknown-linux-gnu.tar.gz \
  --repo jdx/mr-boxington \
  --signer-workflow jdx/mr-boxington/.github/workflows/release.yml
```

Both commands passed for Linux x86_64, Linux aarch64, and macOS aarch64.
At 2026-10-26 14:11 UTC, direct authenticated HTTPS requests to the official
GitHub Releases API again returned MBX `v1.28.0` and action `v1.7.3` as the
latest releases.

The installed Mise-selected MBX executable reports `mbx 1.28.0`. Its `--help`
command passed. `mbx doctor` reported zero failures and zero warnings, with the
existing host Cargo and Rust versions and the existing shared cache. This is a
local host check. The installed executable digest equals the extracted macOS
aarch64 release executable digest shown above.

## Local runtime qualification

The local fixture used a dependency-free Rust binary, a private empty MBX cache,
a private target root, private XDG cache and configuration directories, and the
committed Mise lock copied into a temporary Mise config with only the exact
Rust and MBX tools from the repository's `mise.toml`. It used Rust 1.99.1,
`MBX_GC_AUTO=0`, `MBX_SHARE_OUT_DIR=0`, and Cargo offline mode. It did not use
the host's shared MBX cache or modify global settings.

Both builds ran through:

```sh
rust-build-guard -- mise exec -- cargo build --locked --offline -j 2
```

The first build started with an empty MBX cache and passed with zero hits,
zero misses, one action not looked up, and zero bytes transferred. Its binary
ran and printed `mbx128 qualification probe`. A second fresh checkout of the
same source built with one MBX cache hit, zero misses, and zero bytes
transferred. Its binary ran with the same output. `mbx cache verify` passed
with 7 verified objects and 1 action result. Cache statistics reported 2
checkouts and 2 target directories. The temporary fixture is under `/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/mbx128-isolated-qualification-zv6g1l_e`.

Local qualification is complete for MBX 1.28.0 and action v1.7.3. This does
not claim Linux runtime execution or a protected-main hosted cache roundtrip.
The new-action pull-request checks must pass before merge. The protected-main
MBX cache roundtrip remains required before an SDK release.

The Velnor native `--check-upstream` probe ran at 2026-10-26 14:06 UTC. It
returned `lookup_failed (timeout: global)` for every upstream row, including
MBX and the action. Local inventory, policy, and pin validation passed. This
network timeout is recorded as a failed probe and is not used as freshness
evidence. The direct official GitHub API checks at 14:11 UTC confirmed the
latest MBX and action versions.

The previous scheduled freshness run
[`40317958599`](https://github.com/tailrocks/velnor-new/actions/runs/40317958599)
reported MBX `1.27.0` and action `v1.7.2` as stale. The pins and policy mirror
now name the current releases. The inventory records MBX 1.28.0 and action
v1.7.3 as locally qualified and current, based on the evidence above. This does
not claim hosted acceptance. The new-action pull-request checks and the
protected-main MBX cache roundtrip must still pass before SDK publication.
A catalog change alone does not qualify either pin.
