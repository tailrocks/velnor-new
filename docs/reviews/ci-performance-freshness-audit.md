# CI performance freshness audit

Checked 2026-10-02 19:54–19:55 UTC (2026-10-03 local). This is official source verification, not hosted performance or qualification evidence. Historical runtime pins remain unchanged by this audit. Eight new native workload tools are audited separately by the catalog workstream.

## Findings

- Newly stale without existing hold: Mise `2026.9.18` → `2026.10.0`, MBX `1.21.0` → `1.21.1`, MBX action `v1.5.0` → `v1.6.0`. Latest MBX action commit: `1687e54eb349cadf61fa38b5813a77875489e8e6`. Pin qualification or an explicitly reviewed technical hold remains required.
- Existing Rust, Mise action and Alint holds retain original grant `2026-10-01`, expiry `2026-10-15`, owner `@donbeave`, issue `#6`. No hold was renewed or invented. Holds do not mean latest or performance verified.
- All nine action pinned tags resolve to the inventory full commit SHA; annotated Rust-cache tag was peeled explicitly. Restore/save share one actions/cache repository source lookup.
- Seven other existing tools match latest stable/non-yanked versions. Official runner tables confirm Ubuntu 26.04/24.04/22.04 for x64; default 26.04 is the highest supported versioned family. Image contents remain mutable; actual ImageVersion must still be captured in run evidence.

## Migration evidence

[Mise 2026.10.0 release](https://github.com/jdx/mise/releases/tag/v2026.10.0) fixes a `.tool-versions` trust gap (GHSA-wcqh-j26q-g44x), keyless signer verification and attestation workflow matching. Version policy requires expedited security updates; an older pin cannot be described as current. Any temporary retention requires verified isolation rationale and review.

[MBX 1.21.1 release](https://github.com/jdx/mr-boxington/releases/tag/v1.21.1) fixes pruning test binaries leased by running commands and extends outside-workspace path dependency caching. Requalification must include Nextest execution and T21 relocation; upstream describes intentional cache-key changes.

[MBX action 1.6.0 release](https://github.com/jdx/mr-boxington-action/releases/tag/v1.6.0) adds opt-in same-repository PR cache writes (disabled by default), save-eligibility outputs, cache-mode handling and remote backend validation. Preserve trusted read-only PR policy until separate server-boundary/security qualification; the new release does not itself authorize PR writes.

## Tool sources

| Tool | Pin | Latest stable | Latest published UTC | Status | Official source SHA256 |
|---|---|---|---|---|---|
| [mise](https://api.github.com/repos/jdx/mise/releases/latest) | 2026.9.18 | v2026.10.0 | 2026-10-02T01:42:32Z | outdated; no existing hold | `dca7cd710ac3f662bd681133125ac3d2dc7b9e90d035fce2e2dbde6715e170c2` |
| [mr-boxington](https://api.github.com/repos/jdx/mr-boxington/releases/latest) | 1.21.0 | v1.21.1 | 2026-10-02T12:50:15Z | outdated; no existing hold | `67741cadeddac8d06aba1ed97578ca2e39c0165fe55614c9c5f24b6f2219e8dd` |
| [gh](https://api.github.com/repos/cli/cli/releases/latest) | 2.102.0 | v2.102.0 | 2026-09-30T02:40:02Z | current | `33732ee5b9204d11b332be4cb5a92445a1c2bdc5373d5f711ca8fb917c259e35` |
| [actionlint](https://api.github.com/repos/rhysd/actionlint/releases/latest) | 1.7.12 | v1.7.12 | 2026-03-30T17:49:21Z | current | `38c071bd4c1d710911a03902c6e6b3aef3fba86762d0f9237f15769f1a92907b` |
| [shellcheck](https://api.github.com/repos/koalaman/shellcheck/releases/latest) | 0.11.0 | v0.11.0 | 2025-08-04T00:27:19Z | current | `3258167c932f0f82538987cb17cfcee545b6fcf109d0182e6ac254a2868130eb` |
| [zizmor](https://api.github.com/repos/zizmorcore/zizmor/releases/latest) | 1.30.1 | v1.30.1 | 2026-09-09T05:34:10Z | current | `f22793694e6c4e80f695dff994de677003f0d281040da504d48f8fde6a32f900` |
| [nextest](https://crates.io/api/v1/crates/cargo-nextest) | 0.9.146 | 0.9.146 | 2026-09-21T18:39:40.614185Z | current | `2f174af016b6f0ad1e77eaa3c6e9d60ae78c76cf24f7ba7461adc480b85a4674` |
| [opentofu](https://api.github.com/repos/opentofu/opentofu/releases/latest) | 1.13.1 | v1.13.1 | 2026-10-01T17:15:15Z | current | `2d33b4f188d7660efc201428804c75d5133ed85f938f5b5532d3a66c0a528c82` |
| [release-plz](https://crates.io/api/v1/crates/release-plz) | 0.3.169 | 0.3.169 | 2026-09-19T10:32:25.588592Z | current | `3312bcaa2c77eb414dc8cd1c82d26367c9ccae8c7b02b8677479c2cf70873a5d` |
| [rust](https://static.rust-lang.org/dist/channel-rust-stable.toml) | 1.98.1 | 1.99.0 | 2026-10-01 | held through 2026-10-15 | `ce6dddc886364f8d786514771212cebe9b731ba82d6b859951c6b0ccc516b6a2` |

## Action sources

| Action | Pin | Latest stable | Latest published UTC | Status | Official source SHA256 |
|---|---|---|---|---|---|
| [jdx/mise-action](https://api.github.com/repos/jdx/mise-action/releases/latest) | v5.0.0 | v5.0.1 | 2026-09-30T22:37:15Z | held | `7056f6f66b3cc372b507492e33fd516e890d6a9dae5d3576155e4dc63b3477c7` |
| [actions/checkout](https://api.github.com/repos/actions/checkout/releases/latest) | v7.0.1 | v7.0.1 | 2026-07-20T15:10:05Z | current | `15acdede088b17023cdbbcea4ede7e6ed93c6f3be85202506e545a7f4da1d834` |
| [actions/download-artifact](https://api.github.com/repos/actions/download-artifact/releases/latest) | v8.0.1 | v8.0.1 | 2026-03-11T15:44:25Z | current | `5160878f01ca82c77d2b4fd652d73ef150024accbf58fc1f165af165bbae6875` |
| [actions/upload-artifact](https://api.github.com/repos/actions/upload-artifact/releases/latest) | v7.0.1 | v7.0.1 | 2026-04-10T17:31:14Z | current | `86c5f1ac21ba0a4e7648ebd4ab82b1fe11f14c70aef5d7531e235d152088b03f` |
| [actions/cache/restore](https://api.github.com/repos/actions/cache/releases/latest) | v6.1.0 | v6.1.0 | 2026-06-26T19:17:06Z | current | `fab1cd173714779bcd8cf67c96c3a29f04323b5a5389ad404afcd17abfb36e53` |
| [actions/cache/save](https://api.github.com/repos/actions/cache/releases/latest) | v6.1.0 | v6.1.0 | 2026-06-26T19:17:06Z | current | `fab1cd173714779bcd8cf67c96c3a29f04323b5a5389ad404afcd17abfb36e53` |
| [jdx/mr-boxington-action](https://api.github.com/repos/jdx/mr-boxington-action/releases?per_page=100) | v1.5.0 | v1.6.0 | 2026-10-02T10:02:20Z | outdated | `9a09fbbd5d85ecbe99406d1eba4693ee6f9fa8fff620b4271702da287a978a53` |
| [asamarts/alint](https://api.github.com/repos/asamarts/alint/releases/latest) | v0.16.1 | v0.17.0 | 2026-10-01T05:27:59Z | held | `6d4ffcc35f1aea87a3858670e40b4a67eebd4d6efc9178fb0bac4651b16f939f` |
| [Swatinem/rust-cache](https://api.github.com/repos/Swatinem/rust-cache/tags?per_page=100) | v2.9.2 | v2.9.2 | 2026-08-06T06:26:27Z | current | `be2d551501d2370cd0c1467a21838df81937d7fb6cd274a5f23b653b755d9799` |

## Reproduction and evidence retention

Bounded official API requests used authenticated `gh api` for exact Git tag/ref/release metadata. Each source response is saved with retrieval timestamp and SHA256. Crates.io versions were filtered to stable and non-yanked. Rust uses both stable and exact pinned distribution manifests. Runner official HTML was independently read alongside the cached Firecrawl document (cache timestamp `2026-10-02T16:45:39.283Z`). No access errors occurred.

Local raw evidence: `/tmp/velnor-freshness-tools`, `/tmp/velnor-freshness-actions`, `/tmp/velnor-freshness-platform`; each has `results.json`. Latest sources are mutable: future refetches need not reproduce these hashes. These source hashes identify fetched evidence, not installed tool artifact digests.

| Platform source | Evidence SHA256 | Detail |
|---|---|---|
| [rust](https://static.rust-lang.org/dist/channel-rust-stable.toml) | `ce6dddc886364f8d786514771212cebe9b731ba82d6b859951c6b0ccc516b6a2` | checked 2026-10-02T19:55:25Z |
| [runner](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) | `1d71bbaf34024a309c7b4ac9fea29e28c79bcf6f8616ff54bb2ee62e085b38be` | checked 2026-10-02T19:55:25Z |

Exact Rust pin manifest: [1.98.1](https://static.rust-lang.org/dist/channel-rust-1.98.1.toml), published `2026-09-03`, SHA256 `a7c8774a5fd8441c997d94c029776cbc5eb111e9d72ab5d256fa69866644347e`.

The inventory owner must integrate fresh observations without changing historical pins silently. A `current` row requires genuine pin/latest equality and freshness within 24 hours; newly stale rows must fail until migration qualification or an honestly attributed temporary hold. Hosted CI being green does not discharge this requirement.

## Integration checkpoint: 2026-10-02 21:07 UTC onward

The tables above preserve the initial audit snapshot. Subsequent authorized source updates synchronized Mise `2026.10.0`, MBX `1.21.1` and MBX action `v1.6.0` / `1687e54eb349cadf61fa38b5813a77875489e8e6`. Their initial stale observations remain historical evidence. This checkpoint records source identity and installed executable bytes; hosted workload/cache performance remains unqualified.

The current inventory contains **26 Mise/catalog tools** (the earlier 25-tool checkpoint gained Node), plus two separate Homebrew native authority records and three delivery-tool records. Nine tool rows remain explicitly stale; no additional hold was invented. Existing three holds remain separate.

| Added tool | Pin | Latest observed | Source status |
|---|---|---|---|
| [bun](https://api.github.com/repos/oven-sh/bun/releases/latest) | 1.3.14 | 1.4.2 | stale; hosted performance unqualified |
| [swift](https://api.github.com/repos/swiftlang/swift/releases/latest) | 6.4.0 | 6.4.0 | current; hosted performance unqualified |
| [ruby](https://api.github.com/repos/ruby/ruby/releases/latest) | 4.0.7 | 4.0.7 | current; hosted performance unqualified |
| [reuse](https://api.github.com/repos/fsfe/reuse-tool/releases/latest) | 6.2.0 | 6.2.0 | current; hosted performance unqualified |
| [java](https://www.oracle.com/a/tech/docs/graalvm-downloads.json) | 25.0.3 | 25.0.4.1.1 | stale; hosted performance unqualified |
| [gradle](https://api.github.com/repos/gradle/gradle/releases/latest) | 9.7.0 | 9.8.0 | stale; hosted performance unqualified |
| [python](https://www.python.org/downloads/) | 3.14.7 | 3.14.8 | stale; hosted performance unqualified |
| [uv](https://api.github.com/repos/astral-sh/uv/releases/latest) | 0.11.29 | 0.12.22 | stale; hosted performance unqualified |
| [cargo-audit](https://crates.io/api/v1/crates/cargo-audit) | 0.22.2 | 0.22.2 | current; hosted performance unqualified |
| [cargo-deny](https://crates.io/api/v1/crates/cargo-deny) | 0.20.2 | 0.20.2 | current; hosted performance unqualified |
| [alint](https://api.github.com/repos/asamarts/alint/releases/latest) | 0.16.1 | 0.17.0 | stale; hosted performance unqualified |
| [node](https://nodejs.org/dist/index.json) | 24.20.0 | 24.21.0 | stale; hosted performance unqualified |
| [boltffi](https://api.github.com/repos/boltffi/boltffi/releases/latest) | 0.30.1 | 0.31.0 | stale; hosted performance unqualified |
| [xcodegen](https://api.github.com/repos/yonaskolb/XcodeGen/releases/latest) | 2.46.0 | 2.46.0 | current; hosted performance unqualified |
| [jq](https://api.github.com/repos/jqlang/jq/releases/latest) | 1.8.2 | 1.8.2 | current; hosted performance unqualified |
| [rust-desktop](https://static.rust-lang.org/dist/channel-rust-stable.toml) | 1.97.1 | 1.99.0 | stale; hosted performance unqualified |

Java freshness now uses Oracle GraalVM 25 vendor/channel selection, excluding the separate innovation channel: [official downloads metadata](https://www.oracle.com/a/tech/docs/graalvm-downloads.json) and [25.0.4.1.1 metadata](https://www.oracle.com/a/tech/docs/graalvm-25.0.4.1.1.json). Historical `25.0.3` is stale. A former lookup failure has been resolved by source research; that is not installation or performance proof.

Native source details are retained in `/tmp/velnor-native-authorities/summary.json`; eleven initial workload/tool additions are in `/tmp/velnor-tool-pin-evidence/inventory-ready.json`. XcodeGen `2.46.0` and jq `1.8.2` match latest source metadata. Boltffi `0.30.1` remains stale against `0.31.0`; desktop Rust `1.97.1` remains stale against `1.99.0`, without a new hold. Exact commit and published asset digest records prove source identity only.

### Mise executable byte qualification

Release source commit: `bc11f90c74eba23bf0d7350efb540e62fb7d9ffd`. `catalog_mise_binary.rs` pins installed executable SHA256 independently from compressed archive SHA256. Platform downloads matched official standalone/checksum metadata; ARM64 binaries were extracted from `mise/bin/mise`. Raw results: `/tmp/velnor-mise-platforms/results.json`. Cryptographic release signatures were downloaded but **not verified**.

| Platform | Installed executable SHA256 |
|---|---|
| linux-arm64-mise | `4b8cacffac83e8493fc5d1eef25f6365edba73ccbed5a1f3987b7cb3f5079656` |
| macos-arm64-mise | `8d2007efdae0c2b64e3955257533e6ec17197bc2fdcbc5dd8f6847f92881deea` |
| linux-x64-mise | `57ced973f968b8fbab07aa8e32bd7077d4a357e200a22356d98963c723c6de0a` |

### Homebrew native authority boundary

Homebrew source is pinned to [`8e858db5584704dcd469b8e826228c0d5a5a94f6`](https://github.com/Homebrew/brew/tree/8e858db5584704dcd469b8e826228c0d5a5a94f6) (release `7.0.7`). This is an immutable source authority, not a Mise installation entry or a claim of qualified Homebrew runtime performance. Portable Ruby `4.0.7` uses artifact blob digests read from that exact source commit:

| Platform | Portable Ruby artifact SHA256 | Verified source |
|---|---|---|
| x86_64-linux | `bf2a9bf102694d40084ed436b06a1566dded60a519f4d1879c90c81046e11081` | [commit-bound vendor record](https://raw.githubusercontent.com/Homebrew/brew/8e858db5584704dcd469b8e826228c0d5a5a94f6/Library/Homebrew/vendor/portable-ruby-x86_64-linux) |
| arm64-linux | `c9b75dd6bd9578921f3ce739dacae8866698c3399c0f83574a5d5fadda2aab2d` | [commit-bound vendor record](https://raw.githubusercontent.com/Homebrew/brew/8e858db5584704dcd469b8e826228c0d5a5a94f6/Library/Homebrew/vendor/portable-ruby-arm64-linux) |

The catalog workstream reports its 46-fixture probe passed and root-local checks passed; Cargo qualification remains queued at this checkpoint. Those reports are not independent hosted execution evidence. The nine observed stale tool rows remain freshness failures until reviewed qualification/migration; green local checks do not convert them to current or performance verified.

## Integration delta: 2026-10-02 21:25 UTC onward

This later checkpoint supersedes counts and statuses in the preceding historical snapshots. Inventory now has **28 tool rows: 27 `PinnedTool` entries plus Mise**, and **four native source authority records covering 13 exact pins**. Eight stale tools remain: Bun, Java, Gradle, Python, Uv, Node, Boltffi and desktop Rust. Alint's previous hold was removed after the paired compatibility update; Rust and Mise action holds remain. No new hold or hosted performance qualification is claimed.

Alint tool/action now both pin approved `0.17.0`; action/source commit `d93c0283b19dd78afcd8a4b303f1556a7759ba81`. Paired actual configuration and action-flag compatibility evidence is `/tmp/velnor-tool-pin-evidence/alint-qualification/qualification-report.json`: both versions validate the 53-rule config, and actual check/action outputs have identical five errors/four informational findings. This verifies upgrade compatibility, not a green repository check; the reported policy failures remain separate work.

| Added source-current tool | Version | Peeled source commit | Evidence SHA256 |
|---|---|---|---|
| [swiftlint](https://api.github.com/repos/realm/SwiftLint/releases/latest) | 0.65.1 | `6aba03e3d8302b33f106e0f922210f35ca4b52cf` | `d03a40908ebd3301389d45f8354e9e489a051a1ab34deb95c4c5eb55a96078b0` |
| [periphery](https://api.github.com/repos/peripheryapp/periphery/releases/latest) | 3.8.0 | `a2db299196ae774cd644c79fa6b1f67556d78de8` | `cc0eafdc5cc4c14965b172ac271006e04232982baa35056bdaf1effc8da9e22e` |

These Swift tool observations retain published asset metadata and source qualification in `/tmp/nativeauthority/summary.json`; isolated acquisition probes are separately reported. Actual hosted workload/cache performance remains unqualified.

The Gradle wrapper authority records execution engine `9.5.1` separately from bootstrap `9.4.1` source commit `2d6327017519d23b96af35865dc997fcb544fb40`. Distribution SHA256 is `bafc141b619ad6350fd975fc903156dd5c151998cc8b058e8c1044ab5f7b031f`; actual project script SHA256 is `aed171fb114f82e6eaea4970a245a200e0582a7dcc8ec0891ca41b6e4a62b754`; JAR SHA256 is `55243ef57851f12b070ad14f7f5bb8302daceeebc5bce5ece5fa6edb23e1145c`. The JAR matches official 9.4.1 bytes. The project-generated script has a template-origin/VM-options substitution; it must not be described as byte-identical to the official root script. Source comparison is retained in `/tmp/velnor-gradle-authority/README.md`.

PostgreSQL fixture authority is `postgres:18.6-trixie@sha256:4ef4dbc939d61acea57712655ddb4b4ab27419c913f94cca0cd57cb3ea3c2280`. Its exact registry manifest identifies the source image; database startup, tests, trust and hosted cache behavior are not qualified by this metadata.

Mise descriptors for three hosts are migrating to the independently identified official artifacts recorded above. This is official tool-artifact qualification; it does not establish a newly published Velnor runtime identity, consumer rollout or hosted performance completion.

## Committed source and pending qualification checkpoint

At commit `577694a`, the authoritative inventory has **13 tool rows: 12 `PinnedTool` entries plus Mise**. Python `3.14.8`, Uv `0.12.22` and REUSE `6.2.0` are committed; Mise remains `2026.9.18`, MBX `1.21.0`, Alint action `v0.16.1` and MBX action `v1.5.0`. The preceding 26/28-row integration snapshots describe working qualification evidence, not the catalog committed at that SHA.

The inspected working inventory has 29 rows (28 `PinnedTool` entries plus Mise), with Community Java `25.0.4.1` and Boltffi `0.30.1` still stale. Bun `1.4.2`, Node `24.21.0`, desktop Rust `1.99.0` and cargo-semver-checks `0.50.0` source/qualification changes remain prospective relative to `577694a`; do not treat them as committed adoption before the separately verified pin unit lands. Local proof retains nine format violations, two unchanged visual failures and five Node dependency vulnerabilities; Rust's Xcode 26.6 / SDK 26.5 hosted gate remains open.

Latest GraalVM Community `25.0.4.1.1` bounded local wrapper qualification passed independent review and parent approval; exact registry/pin adoption is pending. [Official Community release source](https://api.github.com/repos/graalvm/graalvm-ce-builds/releases?per_page=10) remains distinct from the historical Oracle provider. Gradle `push=false` is a read-only cache configuration, not proof of filesystem immutability; observed raw log outcomes retain a structured argv/exit receipt gap. Latest metadata, local compatibility, committed source and hosted performance are separate qualification states.
