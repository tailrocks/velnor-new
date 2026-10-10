# Rust 1.99.0 hosted qualification evidence — 2026-10-10

This record supports the `rust` qualified-tool entry in `.velnor/config.toml`.
It measures the official Rust 1.99.0 component archives and installed
executables on Linux x64 and macOS ARM64. It does not claim that the full
Velnor release, all CI jobs, or local macOS 26/OrbStack behavior is qualified.

## Source and hosted run

Qualification run [38085022656](https://github.com/tailrocks/velnor-new/actions/runs/38085022656)
completed successfully on source and workflow SHA
`090482e536d9967aa13b7a6ec8b27bfee7ea3ee8`, attempt 1. Both required
toolchain jobs passed:

| Host | Job | Artifact | Receipt SHA-256 | Artifact ZIP SHA-256 |
| --- | --- | --- | --- | --- |
| Linux x64 | [114309632989](https://github.com/tailrocks/velnor-new/actions/runs/38085022656/job/114309632989) | [11682645165](https://github.com/tailrocks/velnor-new/actions/runs/38085022656/artifacts/11682645165) | `12aa4d015863f8d265e70e4ade03e189cc427d259daab141086380b52858a856` | `82095a6aa8ded50685df5ac17edc377072da5659a4ae0aee17ff2600f0d8a1ed` |
| macOS ARM64 | [114309633013](https://github.com/tailrocks/velnor-new/actions/runs/38085022656/job/114309633013) | [11681951101](https://github.com/tailrocks/velnor-new/actions/runs/38085022656/artifacts/11681951101) | `66de27acd564f7ae435fe7184e7294ce22f928bca962ca5be64ac578ef06b51b` | `d92b70ae57c028cfbbd6c92f05bace049f2f5a0df252813822e5f67e63d15add` |

The artifact ZIP digests match the GitHub Actions artifact API `digest` fields;
each ZIP contains only `rust-toolchain-qualification.json`. The receipts bind
the run, attempt, repository, dispatched source SHA, and workflow SHA. They
record the installed Rust tree plus executable hashes and version/host probes.

## Pinned Rust manifest and component records

Both receipts identify
[`channel-rust-1.99.0.toml`](https://static.rust-lang.org/dist/channel-rust-1.99.0.toml)
with SHA-256
`ce6dddc886364f8d786514771212cebe9b731ba82d6b859951c6b0ccc516b6a2`. Each
receipt's URL and SHA-256 for every component exactly match the corresponding
target record in those pinned manifest bytes.

| Target | Component | Official manifest artifact | SHA-256 |
| --- | --- | --- | --- |
| Linux x64 | cargo | [`cargo-1.99.0-x86_64-unknown-linux-gnu.tar.xz`](https://static.rust-lang.org/dist/2026-10-01/cargo-1.99.0-x86_64-unknown-linux-gnu.tar.xz) | `d7674918d28093097614cd9728b6ca60db9ea3038f640f0bd1e9a4188c7568ce` |
| Linux x64 | rust-std | [`rust-std-1.99.0-x86_64-unknown-linux-gnu.tar.xz`](https://static.rust-lang.org/dist/2026-10-01/rust-std-1.99.0-x86_64-unknown-linux-gnu.tar.xz) | `3e58dff2d0b72196b5ea4e90536e174d400de88564a52694686b81e091169933` |
| Linux x64 | rustc | [`rustc-1.99.0-x86_64-unknown-linux-gnu.tar.xz`](https://static.rust-lang.org/dist/2026-10-01/rustc-1.99.0-x86_64-unknown-linux-gnu.tar.xz) | `77171ba2a0345fdf2abc4fedda55d6de078dae7a68527c28be8c77dcc9604bd5` |
| macOS ARM64 | cargo | [`cargo-1.99.0-aarch64-apple-darwin.tar.xz`](https://static.rust-lang.org/dist/2026-10-01/cargo-1.99.0-aarch64-apple-darwin.tar.xz) | `76abff0ad79a10a3152dff640ea0d8b3c37637d9300bf55da41b95c3ddc069c1` |
| macOS ARM64 | rust-std | [`rust-std-1.99.0-aarch64-apple-darwin.tar.xz`](https://static.rust-lang.org/dist/2026-10-01/rust-std-1.99.0-aarch64-apple-darwin.tar.xz) | `a7c6de8aa21e7c31163a7656295b321bb85f5dce55ed8ea95ac9ac561202bb5b` |
| macOS ARM64 | rustc | [`rustc-1.99.0-aarch64-apple-darwin.tar.xz`](https://static.rust-lang.org/dist/2026-10-01/rustc-1.99.0-aarch64-apple-darwin.tar.xz) | `334a66714ca316d71bbe5efe44f71762d0b276cea73be2987374693a837f7450` |

## Installed tool measurements

Both jobs installed MBX `1.23.0` through the pinned Mise action. The workflow
checked `mbx --version` and the receipt records the absolute executable path
and SHA-256. The Rust probes reported Cargo `1.99.0 (5f94df478
2026-08-27)` and rustc `1.99.0 (b940084d7 2026-09-28)`, including the
expected host triple.

| Host | MBX executable SHA-256 | Rust install tree SHA-256 | Cargo executable SHA-256 | rustc executable SHA-256 |
| --- | --- | --- | --- | --- |
| Linux x64 | `4b1e52337161a4c38c0a200bc8648e8dfe5e292e03385396efc912e15c838dff` | `0f35f35b8690b4ddd921c1be2b9802d776142070a5c193dbbaebf2afc7feddc3` | `e951141cc55a6cd7b9876d187bd30a3720e6086b99413af95e3b8de1cdd72f14` | `f3834d26669b03f6855fa54bd2381443123e860167bc0c7a8dd137e5cf6e5e4f` |
| macOS ARM64 | `fe8a13b057e04e0e17288dd0d96e5a9672d7bc1a993aa88c6c7abd90eb3379b8` | `5120b02b0a1abc1f6ecbcfd1af0f735ac1ae313b0ed35a689a95276e993e70fd` | `fc7aa5077deca8abc0651f329e8ec1d968f520e996a93b428f9935014362b382` | `dd6f58440b3418aa9157ae3f05d71eecff741ca41bc4eec120e7e8f2d933b9a0` |

The machine-readable cross-check summary is retained at
`/private/tmp/velnor-rust-qualification-38085022656-verified/verified-summary.json`
(SHA-256 `2266d91e0b924812059c048f24723bb80e467acecd171577ffbce4a2fec2bbf2`).
It records the API run/job/artifact metadata, artifact ZIPs, both receipts, and
the pinned channel manifest. The verifier compared exact manifest table URL
and checksum pairs rather than accepting a URL-shaped match.
