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
