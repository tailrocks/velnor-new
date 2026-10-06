# Bun and Node compatibility qualification

Checked 2026-10-03 on local macOS ARM64. This is bounded tool upgrade
compatibility evidence. Local paired browser execution is complete, with the
two existing visual golden failures retained. Hosted Ubuntu behavior, cache
migration and three-run performance remain unqualified.

## Proposed pins and official authority

| Tool | Baseline | Proposed | Exact source commit | Official authority |
|---|---|---|---|---|
| Bun | 1.3.14 | 1.4.2 | `744846f844374847c902b5e7fd59b4342a51ef99` | [Exact release](https://github.com/oven-sh/bun/releases/tag/bun-v1.4.2) |
| Node | 24.20.0 | 24.21.0 | `955266bfdd854cd280dffd47548673914484e4c0` | [Exact release](https://github.com/nodejs/node/releases/tag/v24.21.0) |

Bun's official latest release is stable and resolves directly to the recorded
commit. Node's annotated tag was peeled to the recorded commit. The
[official release table](https://nodejs.org/en/about/previous-releases) and
[distribution index](https://nodejs.org/dist/index.json) identify 24.21.0 as
latest within the maintained 24 LTS Krypton cohort. Both tested Node versions
bundle npm 11.19.0; the candidate's immutable `deps/npm/package.json` confirms
that version. The Node cohort intentionally excludes Current 26.

Downloaded baseline and candidate archives match official GitHub asset digests
or Node checksum manifests. Cryptographic release signatures were not verified.

| Candidate artifact | Archive SHA256 | Extracted binary SHA256 |
|---|---|---|
| Bun Darwin ARM64 ZIP | `90987a3a16d7db556d886ac3d551e7b6d3edf0a1cf43acaed622e8676be1d12f` | `35d20dd0263e5c950194434b925454fdfa9ba6e4467da960410fa05b08a7a5b5` |
| Node Darwin ARM64 tar.xz | `6239d4cf92d864487ec8cd3615038f7b67e7f58b77b21cd2f09ea9fbd68065fe` | `e4b5a3af0e05c75de2eae013904145f40fe7fc2a6e6f17510128bf45cca4e79b` |

## Actual owning commands

Each version uses a separate copied consumer source, package installation and
cache directory. Commands invoke the downloaded exact tool; package scripts
resolve it first through PATH. Source locks were retained.

Bun consumer: Parallax commit
`90d901c9d12477e93a56a9e021077ced7c78f9df`, complete immutable source archive,
with commands in `ui`.

| Command | Bun 1.3.14 | Bun 1.4.2 |
|---|---|---|
| `bun install --frozen-lockfile` | passed | passed |
| `bun run build` | passed | passed |
| `bun run test:ci` | 157 files / 692 tests passed | 157 files / 692 tests passed |
| `bun run typecheck` | passed | passed |
| `bun run lint` | passed | passed |
| `bun run check` | failed: nine formatting violations | same nine violations |
| `bun run test:browser:list` | lists 74 tests / 45 files | same inventory |
| Initial UI-only `bun run test:browser:foundation` | fails before tests | fails before tests |

The nine formatting findings exist in both versions. Browser listing alone
does not prove execution. The initial UI-only foundation attempt failed because
its `cargo xtask` server prerequisite was absent from the scoped copy. That
acquisition omission was resolved by the subsequent complete source experiment
below; it is historical evidence, not a current browser prerequisite blocker.

### Subsequent complete browser execution

Both versions used physically separate full source copies, Cargo homes/targets,
Bun caches and browser downloads. Authenticated upstream commit/tree reads bind
Parallax commit `90d901c9d12477e93a56a9e021077ced7c78f9df` to tree
`66d9f81c4940cfcb2edc8bbf0d15ed9d044ed807`; all 1,877 tested Git blobs match
with zero mismatches. Frozen Bun install/build and Chromium/Firefox/WebKit
installation passed for both. Actual owning xtask/server prerequisites were
built offline with exact consumer Rust 1.97.0 and 594 checksum-verified Cargo
registry payloads. Full-stack tests used the actual managed Greptime/Turso
harness. No product source or golden images were changed.

All six owning scripts executed with `--workers=1` for each version:

| Owning script | Bun 1.3.14 | Bun 1.4.2 |
|---|---|---|
| `test:browser:foundation` | 1 passed | 1 passed |
| `test:browser` | 18 passed | 18 passed |
| `test:browser:full` | 27 passed | 27 passed |
| `test:browser:cross` | 18 passed | 18 passed |
| `test:browser:a11y` | 8 passed | 8 passed |
| `test:browser:visual` | 2 failed | same 2 failed |

Thus each version passes 72 functional tests and fails two existing visual
golden comparisons. All 16 paired expected/actual/diff/failure/retry PNGs are
byte-identical across versions; both report the same 7,962 and 21,287 mismatched
pixels, including retries. This proves paired output compatibility for those
captures. The cause of disagreement with the existing goldens remains unproven;
the visual golden gate is still failed. Source locks stayed unchanged and all
owned ports were released.

The project named `visual-chromium-linux` ran on macOS ARM64. Its name does not
establish Linux qualification. Hosted Linux execution and cache/performance
proof remain open. Independent final browser evidence review passed: full
source/archive/browser receipts and all 16 paired PNGs were checked.

Node consumer: audited Java monorepo commit
`5f77c0b09eda3ac4a6fb8c5de0a622a0cb926c45`, scoped
`frontend/wallet-screening` source. All 34 files match their Git blob SHA1 and
retained SHA256. Independent authenticated exact GitHub API reads bind the
commit to root tree `f6da1ab7dbb086595e95a8b3fa9a1cbd8ceca51a`, then `frontend`
tree `f917970525fb4d49203ec48548b29734a520f9d1`, then wallet tree
`a9af43badf4a81f86d07d8e92c930abec7c04d64`. Its complete recursive response
has 42 entries: 34 blobs and eight subtrees, with `truncated:false`. All 34
path/OID pairs match the tested source manifest exactly; no missing or extra
paths remain.

| Command | Node 24.20.0 | Node 24.21.0 |
|---|---|---|
| `npm ci` | passed | passed |
| `npm run typecheck` | passed | passed |
| `npm run build` | passed | passed |

Both installations report five locked dependency vulnerabilities: four moderate
and one high. They also report deprecated `tsconfck` and an `fsevents`
install-script allow-list warning. Compatibility success does not establish
dependency security. Those source findings need their own disposition.

## Native npm owner condition

The actual compiled producer composition and native fixture were executed with
both downloaded Node versions. Baseline 24.20.0 passes. Candidate 24.21.0 is
rejected by the existing hardcoded `v24.20.0` owner check. An isolated candidate
body qualifying `v24.21.0` passes cold/warm/corrupt, actual offline `npm ci`,
new-version, provenance-change, authentication/private and fallback cases.

The product owner expectation now derives Node version, npm version and module
ABI from the central catalog. Node 24.21.0 constants and exact fixture pins are
adopted; npm remains 11.19.0 and ABI remains 137. Focused compilation and native
execution of the adopted product state remain queued.

## Evidence and decision

Raw release/ref/checksum bodies, absolute artifact paths, command argv/results,
logs, source blob manifests and installed binary digests are retained at
`/tmp/velnor-tool-upgrades/bun-node/qualification-report.json`. The reviewed
86-entry manifest SHA256 is
`e80179d57bbaedaf49c36293050b0d647cb051bdb469f27f139bd88e9316e24c`;
report SHA256 is
`e8a6049a58d9144c3d9fd8c190fc2e0918a29a316ca1a1ebe32a7572f321fc8c`.
Private consumer raw sources stay outside this repository.

The later browser report is retained at
`/tmp/velnor-tool-upgrades/bun-browser/qualification-report.json`, SHA256
`c8974c6f4166353e82a1ee643ae66da05d08d1eabcccab935f374614e587c489`.
Its evidence manifest SHA256 is
`ce93b495ab9303ecada9d132d1a16742b4aec95140b8cff78a2f46294b34c61f`.
This supplement preserves the original UI-only failure and closes its missing
full source prerequisite; it does not supersede formatting/security findings.

Independent evidence review confirms the command results, artifact hashes,
source blob hashes and explicit limits. Bun 1.4.2 is supported by this scoped
compatibility proof. The Java commit/tree provenance gap is independently
closed; private raw API responses and hash checks are retained under
`/tmp/velnor-tool-upgrades/bun-node/independent-java-linkage/` with directories
`0700` and files `0600`. Node 24.21.0 has scoped compatibility evidence; the
central authority update is implemented, with its focused product gates pending.
This document claims no hosted/cache performance completion.

A separate exact gzip HTTP installation was independently verified after
adoption: archive SHA256
`bed7eea5325e1108f32ce5228ddd6a5f0f08a499ee42aa7442aea583702f6057`;
extracted and installed Node/npm/npx bytes agree. The explicit HTTP selector
binds this URL/checksum, strip 1 and `bin_path=bin`. Runtime Node 24.21.0,
npm/npx 11.19.0, ABI 137 and N-API 10 pass locally. This qualifies the Mac
installation descriptor separately from the consumer's tested xz archive.
