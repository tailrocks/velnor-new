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

## Bounded qualification checkpoint: 2026-10-02 23:33 UTC

This checkpoint preserves previous observations as history. The root freshness receipt `/tmp/velnor-tool-upgrades/freshness-profile-adoption-final.log` still **fails**, with three stale entries: Community Java `25.0.4.1` against `25.0.4.1.1`, Boltffi `0.30.1` against `0.31.0`, and desktop Rust `1.97.1` against `1.99.0`. Receipt status is distinct from pending experiments or authorized future adoption.

| Current adopted tool | Pin | Source |
|---|---|---|
| python | 3.14.8 | [official source](https://www.python.org/downloads/) |
| uv | 0.12.22 | [official source](https://api.github.com/repos/astral-sh/uv/releases/latest) |
| reuse | 6.2.0 | [official source](https://api.github.com/repos/fsfe/reuse-tool/releases/latest) |
| bun | 1.4.2 | [official source](https://api.github.com/repos/oven-sh/bun/releases/latest) |
| node | 24.21.0 | [official source](https://nodejs.org/dist/index.json) |
| java | 25.0.4.1 | [official source](https://api.github.com/repos/graalvm/graalvm-ce-builds/releases?per_page=10) |
| gradle | 9.8.0 | [official source](https://api.github.com/repos/gradle/gradle/releases/latest) |

Minimal Python/Uv/REUSE integration was committed as `8c383dc`; the workstream records 67 tests and independent review. Local macOS ARM64 compatibility evidence in `/tmp/velnor-tool-upgrades/python-uv/qualification-report.json` verifies exact isolated Python, Uv and REUSE runtime selection plus actual immutable consumer `reuse lint`, including negative checks. Linux and fresh-hosted-runner reuse remain unqualified.

Bun `1.4.2` and Node `24.21.0` have source freshness receipts checked at 23:10 UTC and bounded actual consumer qualification on macOS ARM64. Bun retains nine unchanged format violations. The later complete browser supplement `/tmp/velnor-tool-upgrades/bun-browser/qualification-report.json` (SHA256 `c8974c6f4166353e82a1ee643ae66da05d08d1eabcccab935f374614e587c489`) closes the earlier UI-only browser execution gap: each version passed 72 functional browser tests and failed the same two visual tests; 16 compared PNGs match byte-for-byte. This proves paired behavior, not a green visual suite. Node locked dependencies retain five reported vulnerabilities (four moderate, one high). No dependency-security or hosted performance success is claimed.

Java is now an explicitly typed **GraalVM Community** role; it no longer uses the historical Oracle provider. The catalog workstream reports that the bounded official Community release parser (three pages of ten records) passed independent review. Current Community `25.0.4.1` installation/source identity does not qualify latest `25.0.4.1.1`; freshness stays stale. Managed Gradle `9.8.0` is source-current with actual local HTTP installation/health qualification, while the audited consumer wrapper engine `9.5.1` and bootstrap `9.4.1` remain a separate authority. Profile alias/configuration and negative rebind guards are under independent review at this checkpoint.

Desktop Rust `1.99.0` has a local normal shipping archive and frozen Swift/native evidence: `/tmp/velnor-tool-upgrades/desktop/final-qualification.json` records archive SHA256 `1fda7040db08cda50278171c4b449a71db33fc227941e79dbf0473180543c771`, source integrity and 80 native tests on installed Xcode 27 / SDK 27. Independent review/adoption remains pending; parent authorization is conditional on review. The required shipping Xcode 26.6 / SDK 26.5 hosted gate remains open. Catalog `1.97.1` is still stale in the cited receipt.

Cargo-semver-checks `0.50.0` is a researched addition, not yet a catalog slot. [Canonical macOS ARM64 archive](https://github.com/obi1kenobi/cargo-semver-checks/releases/download/v0.50.0/cargo-semver-checks-aarch64-apple-darwin.tar.gz) SHA256 `f99f928d67501c29e8410026f27a54cf799fb13611c3b4a6a58f2772cf7e3799` installs executable SHA256 `9ce8bc0cd0a5f0f1aa8b5728d9745104adf774fb84970847cc9901b55f61a3a1` through the fixed HTTP recipe. `/tmp/nativeauthority/cargo-semver-checks/http-canonical-receipt.json` records complete installed/archive byte comparison and CLI version/publisher syntax; the workstream reports independent source/syntax review passed. Actual API/semver analysis is pending, as are Linux runtime and hosted performance. No owned published Velnor identity is inferred from these tool receipts.

## Adoption checkpoint: 29 tools, two stale entries

The current mirror has **29 tool rows: 28 `PinnedTool` entries plus Mise**. Only Community Java `25.0.4.1` against `25.0.4.1.1` and Boltffi `0.30.1` against `0.31.0` remain stale. Desktop Rust `1.99.0` was adopted after independent local normal-shipping archive, frozen Swift and 80-test proof. Its required Xcode 26.6 / SDK 26.5 hosted gate remains open; source/current status does not mean hosted or cache performance qualification.

The catalog workstream records completed independent review of profile guards: actual configuration/import alias and rebind bypass reproductions now reject, seven extracted regression groups pass, and canonical Java/Gradle roles resolve. Cargo fixture gates remain queued; this bounded static review supersedes the earlier pending-review status.

Cargo-semver-checks `0.50.0` now has a catalog slot, source aliases and mirrors. The source/installation identity remains the canonical fixed HTTP recipe recorded above. `/tmp/nativeauthority/cargo-semver-checks/versioned-api-independent-review.json` independently verifies actual versioned public API comparison: genuine [published is_terminal_polyfill 1.70.2](https://crates.io/api/v1/crates/is-terminal-polyfill/1.70.2) against diagnostic `1.71.0` candidates. Compatible addition exits 0 (196 checks pass, 58 skipped); removal of public `IsTerminal` exits 100 (195 pass, one failure, 58 skipped).

The API fixture docs were generated with Rust `1.98.0`, `--locked --offline`, and unchanged locks; diagnostic rustdoc JSON uses `RUSTC_BOOTSTRAP=1` and unstable JSON flags. The checker consumed supplied files with `/usr/bin:/bin` PATH and no Cargo executable. This qualifies file-mode API behavior, not the default checker generation path, manifest/dependency-specific obligations or an approved generated-workflow use of diagnostic flags. Actual anonymous release-plz/helper execution, Linux and hosted performance remain unqualified. Raw plan/results: `/tmp/velnor-anonymous-semver-versioned-proof/locked-api-proof-summary.json`.

Latest Community Java `25.0.4.1.1` native wrapper `9.5.1` proof is reported complete, with independent review ongoing; latest has not been adopted. The workstream reports paired Boltffi `0.31.0` passed 612 Rust and 80 native tests with review; global adoption remains blocked until the reviewed Wave A consumer source migration. These pending adoption gates remain separate from source metadata and local test outcomes.
