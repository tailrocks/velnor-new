# CI performance scope/access audit

Observed 2026-10-03 through authenticated GitHub REST calls. Inventory/access evidence only; no row is performance-qualified.

## Exact scope and provenance

`scope.json` and `repositories.txt` were absent in the checkout. Targeted searches covered repository/project roots, migrations, prior Velnor evidence/scan/work roots, plus immediate Projects/Downloads/Desktop inventories. Reconstructed both from the 47-row CSV and independently cross-checked every ordered consumer against the goal. No repository added, dropped, or renamed. Independent verifier confirmed counts and equality: generator 1; Wave A 8; Wave B 33; Wave C 5.

All 47 repositories are accessible, nonarchived, and currently use `main`. There are 43 public and four private repositories. The original Brown 404 is resolved: `ChainArgos/jackin-agent-brown` is private and accessible to the authenticated account. Its migration is PR #243, merged 2026-10-02T15:29:29Z, head `9b85ac956fc917df97198dd6df67f844d41d0c84`.

## Evidence boundary

Downloaded live repository metadata, ten most recently updated PRs, referenced migration PR metadata, fully paginated migration file lists, and immutable current default-branch commit identities for all 47 repositories. All 46 consumer migration PRs are merged. API file listings can omit large patches; their presence is not a completed PR-diff review. Current workflow family, runtime digests, all relevant subsequent workflow changes, protection/gates, step logs, cache state, and controlled hosted experiments remain separate tasks.

Raw authenticated API evidence is stored outside this public checkout at `/tmp/velnor-ci-performance-scope-audit/`; private repository source/diff evidence must stay outside public artifacts. Files use `owner__repository.json`, plus `summary.json`. A collector timestamp is retained per repository. Reuse these downloads rather than repeat API requests.

Prior `/Users/donbeave/Projects/CI_MIGRATION_STATUS.md` and `ci_migration_ledger.json` record a narrow private-ChainArgos admin-merge policy. Their old outcomes are stale and do not qualify current changes or performance. Live metadata confirms the four private identities; apply the goal policy only after current review/authority checks. No waiver is claimed granted by this audit.

## Current immutable inventory

Every row remains `INCOMPLETE`: no cold/warm/third-run measurement or resulting rollout is established here. Cache metrics, compiler work, selected/covered obligations, and critical path are unknown rather than zero. The original CSV remains unchanged as historical research input.

| # | Wave | Repository | Visibility | Default SHA | Migration PR | Relevant diff gaps | Status |
|---:|---|---|---|---|---|---|---|
| 0 | G | `tailrocks/velnor-new` | public | `c57c700459bbe1549fe7eedcb7d8689585c38986` | — | generator PR audit pending | INCOMPLETE |
| 1 | A | `jackin-project/jackin` | public | `6c389d38eadab93d6d6a4005e01dbdd8c4160221` | [1110](https://github.com/jackin-project/jackin/pull/1110) | `.github/workflows/ci-main.yml`; `.github/workflows/ci-pr.yml`; `.github/workflows/ci.yml` | INCOMPLETE |
| 2 | A | `jackin-project/jackin-agent-smith` | public | `2e7119b9c668ca7a9c55218b20049885299d198f` | [213](https://github.com/jackin-project/jackin-agent-smith/pull/213) | patches downloaded; review pending | INCOMPLETE |
| 3 | A | `jackin-project/homebrew-tap` | public | `cd05a0ea2cf68fd6c2753ee938247b2dcd4c7551` | [505](https://github.com/jackin-project/homebrew-tap/pull/505) | patches downloaded; review pending | INCOMPLETE |
| 4 | A | `jackin-project/jackin-role-action` | public | `59e538704b8c119f3b6668cea0154909a6158ff6` | [189](https://github.com/jackin-project/jackin-role-action/pull/189) | patches downloaded; review pending | INCOMPLETE |
| 5 | A | `jackin-project/jackin-sentinel` | public | `587a0d1a8eef96108c9d9d530bdaa13df91edd2b` | [154](https://github.com/jackin-project/jackin-sentinel/pull/154) | patches downloaded; review pending | INCOMPLETE |
| 6 | A | `jackin-project/jackin-dev` | public | `a01b342162bc56cdf1e8bbaab793e73d31c1d621` | [48](https://github.com/jackin-project/jackin-dev/pull/48) | patches downloaded; review pending | INCOMPLETE |
| 7 | A | `jackin-project/jackin-github-terraform` | public | `b43a2314c6b58906d52828115d3b3973026f6a2a` | [48](https://github.com/jackin-project/jackin-github-terraform/pull/48) | patches downloaded; review pending | INCOMPLETE |
| 8 | A | `jackin-project/jackin-the-architect` | public | `2cf461e2fed1b95d9fd1e7ba74c10d4d8b1c685d` | [478](https://github.com/jackin-project/jackin-the-architect/pull/478) | patches downloaded; review pending | INCOMPLETE |
| 9 | B | `tailrocks/github-terraform` | public | `288dc40dec53500dfeb8bed148613b99cfea92c2` | [38](https://github.com/tailrocks/github-terraform/pull/38) | patches downloaded; review pending | INCOMPLETE |
| 10 | B | `tailrocks/termpane` | public | `7cf2f9981ef9a3f7c8fc295a7501a261fba955a8` | [31](https://github.com/tailrocks/termpane/pull/31) | patches downloaded; review pending | INCOMPLETE |
| 11 | B | `tailrocks/tui-snap` | public | `a47c9aaefb34e4c00026f99d8a8dd7ee5916b274` | [10](https://github.com/tailrocks/tui-snap/pull/10) | `.github/workflows/ci.yml` | INCOMPLETE |
| 12 | B | `tailrocks/velnor` | public | `3f6633252963efef0d71244aadae36516a11601e` | [1136](https://github.com/tailrocks/velnor/pull/1136) | `.github/workflows/ci.yml`; `.github/workflows/preview.yml`; `.github/workflows/release.yml` | INCOMPLETE |
| 13 | B | `tailrocks/termrock` | public | `e2515bac765f440b11843a25d36ed8f720ae6435` | [72](https://github.com/tailrocks/termrock/pull/72) | `.github/workflows/ci.yml` | INCOMPLETE |
| 14 | B | `tailrocks/parallax` | public | `90d901c9d12477e93a56a9e021077ced7c78f9df` | [125](https://github.com/tailrocks/parallax/pull/125) | `.github/workflows/ci-main.yml`; `.github/workflows/ci.yml` | INCOMPLETE |
| 15 | B | `tailrocks/terminal-components-claude` | public | `84482d066c5f0bc531f875f7f9d7716929c1b9c6` | [13](https://github.com/tailrocks/terminal-components-claude/pull/13) | `.github/workflows/ci.yml` | INCOMPLETE |
| 16 | B | `tailrocks/tailrocks-repository-skills` | public | `036063edc58d88cd88f57e4e3a9721ebadf3ad62` | [13](https://github.com/tailrocks/tailrocks-repository-skills/pull/13) | patches downloaded; review pending | INCOMPLETE |
| 17 | B | `tailrocks/tailrocks-skills` | public | `1e9a23a63e0a44abf6a5ef17b69711011316cd9f` | [119](https://github.com/tailrocks/tailrocks-skills/pull/119) | patches downloaded; review pending | INCOMPLETE |
| 18 | B | `tailrocks/tailrocks-pull-request-skills` | public | `1b260bab1e356b1123fbbbdcdb17bb7bbf8ae83a` | [4](https://github.com/tailrocks/tailrocks-pull-request-skills/pull/4) | patches downloaded; review pending | INCOMPLETE |
| 19 | B | `tailrocks/homebrew-velnor` | public | `a0db8c185b76e1bfab3508124504d0f6361e83b6` | [8](https://github.com/tailrocks/homebrew-velnor/pull/8) | patches downloaded; review pending | INCOMPLETE |
| 20 | B | `tailrocks/velnor-apt` | public | `115b5c42d7ad5659c8496600fabf6cd8061b64b1` | [249](https://github.com/tailrocks/velnor-apt/pull/249) | patches downloaded; review pending | INCOMPLETE |
| 21 | B | `tailrocks/parallax-telemetry-playground` | public | `763518791d60d4197d36119a47311e008ae3f5bf` | [54](https://github.com/tailrocks/parallax-telemetry-playground/pull/54) | `.github/workflows/ci.yml` | INCOMPLETE |
| 22 | B | `tailrocks/velnor-actions-fixture` | public | `1c076c5b5828fb6ba04887885c567da26fe01a69` | [172](https://github.com/tailrocks/velnor-actions-fixture/pull/172) | patches downloaded; review pending | INCOMPLETE |
| 23 | B | `tailrocks/holla` | public | `c756189538c776eaa563ff83a5488e1ac96600c9` | [226](https://github.com/tailrocks/holla/pull/226) | patches downloaded; review pending | INCOMPLETE |
| 24 | B | `tailrocks/tracing-request-level` | public | `2675c867fa2f8af78c2bf2543482add847eec713` | [36](https://github.com/tailrocks/tracing-request-level/pull/36) | patches downloaded; review pending | INCOMPLETE |
| 25 | B | `tailrocks/pg-bigdecimal` | public | `0a06226df6ec5f4e1a70a853d14368ebf2e69a80` | [34](https://github.com/tailrocks/pg-bigdecimal/pull/34) | patches downloaded; review pending | INCOMPLETE |
| 26 | B | `tailrocks/ruxel` | public | `304da9f21271be81964459f5c7e6f51f41e5fdcf` | [52](https://github.com/tailrocks/ruxel/pull/52) | patches downloaded; review pending | INCOMPLETE |
| 27 | B | `tailrocks/schemalane` | public | `94a58dad0fe6714312373fd5dac69f2ced75d194` | [42](https://github.com/tailrocks/schemalane/pull/42) | `.github/workflows/ci.yml` | INCOMPLETE |
| 28 | B | `tailrocks/holla-apt` | public | `405239a2ffec34bca5eb14d95915fb0cdaae0453` | [100](https://github.com/tailrocks/holla-apt/pull/100) | patches downloaded; review pending | INCOMPLETE |
| 29 | B | `tailrocks/homebrew-parallax` | public | `70af3b38e051abfa5a14c08ed5cd5388219ffc40` | [125](https://github.com/tailrocks/homebrew-parallax/pull/125) | patches downloaded; review pending | INCOMPLETE |
| 30 | B | `tailrocks/homebrew-ruxel` | public | `1db9876988e9912bafb6ea9333eea116853dc31f` | [39](https://github.com/tailrocks/homebrew-ruxel/pull/39) | patches downloaded; review pending | INCOMPLETE |
| 31 | B | `tailrocks/homebrew-tablerock` | public | `20428a6d85086aa8454e3dd693e575541e714df2` | [51](https://github.com/tailrocks/homebrew-tablerock/pull/51) | patches downloaded; review pending | INCOMPLETE |
| 32 | B | `tailrocks/homebrew-holla` | public | `5b7c0f27b570de6ef07f17f593cf96737b0de3ce` | [160](https://github.com/tailrocks/homebrew-holla/pull/160) | patches downloaded; review pending | INCOMPLETE |
| 33 | B | `tailrocks/tablerock` | public | `a9771cab32271b1b3fca40f536d9112970989623` | [84](https://github.com/tailrocks/tablerock/pull/84) | `.github/workflows/ci.yml` | INCOMPLETE |
| 34 | B | `tailrocks/cloudflare-tofu` | public | `03fb253cc0f352c3f57ff7dff9d35e4ea95eec4e` | [23](https://github.com/tailrocks/cloudflare-tofu/pull/23) | patches downloaded; review pending | INCOMPLETE |
| 35 | B | `tailrocks/tailrocks-typescript-skills` | public | `0652a50fce67c4a11f01ebb960a50fd5e283e0a9` | [2](https://github.com/tailrocks/tailrocks-typescript-skills/pull/2) | patches downloaded; review pending | INCOMPLETE |
| 36 | B | `tailrocks/tailrocks-skill-authoring-skills` | public | `93fa4d609f348c04869a94d0dc7a10751e31bf1d` | [2](https://github.com/tailrocks/tailrocks-skill-authoring-skills/pull/2) | patches downloaded; review pending | INCOMPLETE |
| 37 | B | `tailrocks/tailrocks-rust-skills` | public | `0317f100714dc66285c01c24f7967134375b5ac5` | [2](https://github.com/tailrocks/tailrocks-rust-skills/pull/2) | patches downloaded; review pending | INCOMPLETE |
| 38 | B | `tailrocks/tailrocks-roadmap-skills` | public | `98d23280cd562c9298ce13ae40bd0eb73a3e376a` | [2](https://github.com/tailrocks/tailrocks-roadmap-skills/pull/2) | patches downloaded; review pending | INCOMPLETE |
| 39 | B | `tailrocks/tailrocks-open-source-skills` | public | `3e51bc5c91949f361ed926d8f760bcb16edef111` | [2](https://github.com/tailrocks/tailrocks-open-source-skills/pull/2) | patches downloaded; review pending | INCOMPLETE |
| 40 | B | `tailrocks/tailrocks-macos-skills` | public | `eb0be5522fe0c1c9c74d41ee354446a011b10c74` | [2](https://github.com/tailrocks/tailrocks-macos-skills/pull/2) | patches downloaded; review pending | INCOMPLETE |
| 41 | B | `tailrocks/tailrocks-code-quality-skills` | public | `e63a82f28b688a7fada418e615aa9ec0eeec4c04` | [2](https://github.com/tailrocks/tailrocks-code-quality-skills/pull/2) | patches downloaded; review pending | INCOMPLETE |
| 42 | C | `ChainArgos/blockchain-nodes` | public | `0881638712a837bdcb90b3cd811a8a9be3aa8d83` | [729](https://github.com/ChainArgos/blockchain-nodes/pull/729) | `.github/workflows/ci-main.yml`; `.github/workflows/ci-pr.yml`; `.github/workflows/release.yml` | INCOMPLETE |
| 43 | C | `ChainArgos/java-monorepo` | private | `5f77c0b09eda3ac4a6fb8c5de0a622a0cb926c45` | [2081](https://github.com/ChainArgos/java-monorepo/pull/2081) | `.github/ci/project.toml`; `.github/workflows/ci-main.yml`; `.github/workflows/ci-pr.yml`; `.github/workflows/ci.yml` | INCOMPLETE |
| 44 | C | `ChainArgos/jackin-agent-brown` | private | `6b2ac2277103a22b9ae155bc92415ca5ddf05f95` | [243](https://github.com/ChainArgos/jackin-agent-brown/pull/243) | patches downloaded; review pending | INCOMPLETE |
| 45 | C | `ChainArgos/cloudflare-tofu` | private | `cc0e2b687dc6c1a9943b0ae9f5911f1e782c1a20` | [6](https://github.com/ChainArgos/cloudflare-tofu/pull/6) | patches downloaded; review pending | INCOMPLETE |
| 46 | C | `ChainArgos/github-terraform` | private | `c8d47ea87a611251f965555dafbd8015f29f303b` | [14](https://github.com/ChainArgos/github-terraform/pull/14) | patches downloaded; review pending | INCOMPLETE |

## Remaining audit work

1. Read full relevant migration diffs; obtain full immutable before/after files where API patches are omitted. Inspect subsequent workflow commits and runtime/configuration at each recorded default SHA.
2. Audit required checks, release/CD obligations, representative PR integration revisions and current default runs. Download full attempt/job logs once; distinguish unavailable logs from metadata.
3. Record exact tool/cache identities, payload paths, trust/write decisions and timing categories. Existing indexed snippets and successful run summaries remain unqualified.
4. Qualify generator and canary before Wave A rollout, then Wave B, then Wave C. Revalidate affected earlier consumers after generator changes.
5. Keep waived hosted CI, static qualification, inaccessible evidence and measured performance distinct in the final 47-row remediation ledger.
