# Freshness and toolchain qualification evidence — 2026-10-08

This refresh completes the approved tool updates tracked by issue #6. Velnor's
generated workflows use Mise `2026.10.4`, Rust `1.98.1`, Nextest `0.9.148`,
and release-plz `0.3.170`, with `jdx/mise-action` v5.1.1 and
`asamarts/alint` v0.17.0. Issue [#6](https://github.com/tailrocks/velnor-new/issues/6)
proposes Rust `1.99.0` while the workspace MSRV remains Rust `1.98`;
`Cargo.toml` keeps `rust-version = "1.98"`. This candidate retains the
selected Rust toolchain at `1.98.1` and records Rust `1.99.0` as a temporary
hold until separate compatibility evidence and approval support moving the
compiler pin above the declared MSRV. The local Rust tool in `mise.toml` also
stays at `1.98.1` under VER-3.4/VER-4.4. Local Rust and Nextest selections
remain read-only developer inputs; `scripts/verify-local.sh` uses the exact
CI toolchain from `.velnor/version-policy.toml` and reports local differences.
The alint pin was already current at the base revision. The action target is
v5.1.1 because it is the current stable release; it supersedes the issue's
earlier v5.0.1 target.

## Mise `2026.10.4`

The official [`v2026.10.4` release checksum list](https://github.com/jdx/mise/releases/download/v2026.10.4/SHASUMS256.txt)
was compared with the raw release binaries. All local SHA-256 measurements
and byte counts matched:

| Target | SHA-256 | Bytes |
| --- | --- | ---: |
| Linux x86-64 | `2b8ce21f550872807bcaabf45b6bc5c64bfbd6dc3bf49dd4e67de700ef3ceb75` | 158829568 |
| macOS ARM64 | `5c530143fc750e8a98c9a36be8d361e5dd953fa0b004d58f7577783f7cf2ac24` | 125641616 |
| macOS x86-64 | `9f58d924a4d7b47aeb1610cd981beca2125907b2591d805237efbca8aae4308e` | 153265264 |

The checked host executable reported `2026.10.4 macos-arm64 (2026-10-07)`.
The Linux x86-64 executable reported `2026.10.4 linux-x64 (2026-10-07)`.

## Rust `1.99.0` upstream release (held)

The official release was checked as the latest stable Rust version, but this
candidate does not select it. The freshness inventory records a temporary
hold for the existing `1.98.1` pin while the separate compatibility decision
is pending.

The official [`channel-rust-1.99.0.toml`](https://static.rust-lang.org/dist/channel-rust-1.99.0.toml)
lists release commit `b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`, dated
2026-09-28. For `x86_64-unknown-linux-gnu`, the manifest's xz package
artifacts are:

| Component | Official artifact | SHA-256 |
| --- | --- | --- |
| cargo | [`cargo-1.99.0-x86_64-unknown-linux-gnu.tar.xz`](https://static.rust-lang.org/dist/2026-10-01/cargo-1.99.0-x86_64-unknown-linux-gnu.tar.xz) | `d7674918d28093097614cd9728b6ca60db9ea3038f640f0bd1e9a4188c7568ce` |
| rust-std | [`rust-std-1.99.0-x86_64-unknown-linux-gnu.tar.xz`](https://static.rust-lang.org/dist/2026-10-01/rust-std-1.99.0-x86_64-unknown-linux-gnu.tar.xz) | `3e58dff2d0b72196b5ea4e90536e174d400de88564a52694686b81e091169933` |
| rustc | [`rustc-1.99.0-x86_64-unknown-linux-gnu.tar.xz`](https://static.rust-lang.org/dist/2026-10-01/rustc-1.99.0-x86_64-unknown-linux-gnu.tar.xz) | `77171ba2a0345fdf2abc4fedda55d6de078dae7a68527c28be8c77dcc9604bd5` |

The pre-reconciliation isolated artifact probe reported
`cargo 1.99.0 (5f94df478 2026-08-27)` and
`rustc 1.99.0 (b940084d7 2026-09-28)`, host
`x86_64-unknown-linux-gnu`, LLVM 23.1.1. This confirms artifact identity; it
does not qualify Rust `1.99.0` as the selected Velnor toolchain.

## Nextest `0.9.148` and release-plz `0.3.170`

The official [Nextest `0.9.148` release checksum file](https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.148/cargo-nextest-0.9.148-x86_64-unknown-linux-gnu.sha256)
and the downloaded Linux x86-64 archive both report SHA-256
`223ba936714cc861a9cdedad5a8eb664dfdd69d6fbdf6cea6aa8184f523290dc`.
The archive is 12,127,365 bytes. Its extracted `cargo-nextest` executable is
mode `0755` with SHA-256
`53470cfc9b5cbcd96780db2d348700d76af962467503791f1f8a8824d8967e5c`; the
single-file install tree digest is
`3d65a55f9602c19326ae050d373175a8b778e21cdbc4f82adfe0bd1be30e8c11`.
An isolated Mise install reported `cargo-nextest 0.9.148 (cd1d6d546
2026-10-07)`.

The official [crates.io release-plz `0.3.170` metadata](https://crates.io/api/v1/crates/release-plz/0.3.170)
reports publication at `2026-10-07T17:23:32Z`, and the official crate archive
download matched its API checksum:
`7a6feb33b1dec2ee457e1e418ef419bd67c5cd5fd507e495cb832643e4846b72`
(`release-plz-0.3.170.crate`, 116,381 bytes). The crate records upstream source
commit `aea6c32fdf4a70378ea8c7136348ba574f6fd199`. An isolated Mise install
reported `release-plz 0.3.170`; its local executable SHA-256 was
`2fb23eab1da4624551e244af8c02bc9311000794aefdb29846321365ba43b5ef`.
The release-plz `0.3.170` source still requires the `--registry` name to exist
in Cargo config and reads its forge credential from `GIT_TOKEN`; the generated
default-registry invocation and token binding remain valid.

The stable Rust manifest is 943,486 bytes and advertises byte ranges. A request
for bytes `0-65535` returned HTTP `206` with
`Content-Range: bytes 0-65535/943486`; the `[pkg.rust]` table begins at byte
80,035. The bounded freshness probe requests the first 128 KiB, requires an
exact range response, and still parses Rust `1.99.0` without raising the
generic 512 KiB response cap.

## GitHub Actions pins and token scope

The current [mise-action v5.1.1 release](https://github.com/jdx/mise-action/releases/tag/v5.1.1)
resolves to commit `2d8d4cafcbd33be2ea37d2b6f5ad595363d1f1ca`. Its official
[`action.yml`](https://raw.githubusercontent.com/jdx/mise-action/v5.1.1/action.yml)
declares `persist_github_token` with default `false`. Velnor omits this input,
so token persistence stays disabled. Existing step-level credential scrubbing
remains in place.

Using Mise `2026.10.4` with a fresh isolated `MISE_DATA_DIR`, `gh@2.102.0`
installed with `GITHUB_TOKEN`, `MISE_GITHUB_TOKEN`, and `GH_TOKEN` unset;
`gh --version` reported `2.102.0`. In a separate fresh `MISE_DATA_DIR`,
`cargo-deny@0.20.2` installed with those variables and
`CARGO_REGISTRIES_CRATES_IO_TOKEN` unset; `cargo-deny --version` reported
`cargo-deny 0.20.2`.

The pinned `gh` and `cargo-deny` installs measured above did not need an
install token; that result applies to those two tool installs.

## Retired owned-source candidate and retained bootstrap metadata

The former manually dispatchable owned-tool workflow used the official Mise
`v2026.10.4` release as its verified build bootstrap. Its GitHub release was
published at
`2026-10-07T16:21:40Z`; the tag resolves to commit
`96cca90d3e55519a47cffa0cb99baa4c3d3ecca3`, tree
`92dda3fb668211ebaa2cf4edd832a184526ee918`, committed at
`2026-10-07T13:08:18Z`. The [official release checksum file](https://github.com/jdx/mise/releases/download/v2026.10.4/SHASUMS256.txt)
matched the measured downloaded archive bytes:

| Bootstrap target | Archive SHA-256 | Extracted binary SHA-256 |
| --- | --- | --- |
| Linux x86-64 standalone | `2b8ce21f550872807bcaabf45b6bc5c64bfbd6dc3bf49dd4e67de700ef3ceb75` | `2b8ce21f550872807bcaabf45b6bc5c64bfbd6dc3bf49dd4e67de700ef3ceb75` |
| Linux ARM64 tar.gz | `8760841cdbf964ecf9902a50c94716c77185a99af7f8eb55c9c51ec73ecd8880` | `9013ce1d7d9bbbf65254cda178562f5450c474a705907c18b77e6b678bb10041` |
| macOS ARM64 tar.gz | `744ae45f9b7c2a443adfa61df48397930e88b13c541834b7bd22ca31d4dfcfcd` | `5c530143fc750e8a98c9a36be8d361e5dd953fa0b004d58f7577783f7cf2ac24` |

The obsolete custom Mise source descriptor `.velnor/owned-tool-sources.json` and
the manually dispatchable `.github/workflows/owned-tools.yml` have been removed.
Manual owned-tool qualification is therefore disabled until a new source is
approved and added. This retirement is intentional: Phase B adopts the fixed
official Mise release and the repository policy forbids carrying the legacy
custom fork publication path. The retained bootstrap metadata identifies
official upstream source and does not constitute an owned source candidate.

## Hosted runner image observation

The successful [CI run 37803236060](https://github.com/tailrocks/velnor-new/actions/runs/37803236060),
job `Rust / velnor-actions-workflow-renderer` (job 113402549990), completed at
`2026-10-08T15:49:37Z`. Its hosted log at `2026-10-08T15:48:32Z` records
Ubuntu `26.04.1` LTS and runner image `ubuntu-26.04`, version
`20260927.149.1`, release
[`ubuntu26/20260927.149`](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20260927.149).
This successful CI log refreshes the runner image observation only; it does
not claim the formal Qualification workflow ran again.
