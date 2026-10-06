# Consumer retirement union — 07:42 UTC snapshot, 2026-10-04

## Current candidate and refreshed snapshot

The current integration candidate is PR #28 at head
`52297bc15ce19e39c84deffe10be76d0ba026d15`; this evidence worktree is based
on that exact commit. Its generator version is `0.1.1`. This is readiness
evidence only; it does not approve migration or adoption.

At 2026-10-04 07:42 UTC, a GraphQL snapshot rechecked the same 86 refs: 46
default heads and 40 open PR heads across the complete 46-repository union.
All 86 repositories/refs were accessible and each API SHA matched its scanned
SHA. The 46-repository order and all default heads stayed unchanged. Only two
open PR heads moved since the prior packet:

- `jackin-project/jackin` PR #1111, branch
  `codex/credential-routing-recovery-20260930`: `60f7d661d14af7ebdfbdf21bd48f5c5eebb28888`
  → `3a28c199f17da335ecd9abd8dd67ebf1aecc0421`; API `updatedAt`
  `2026-10-04T04:14:24Z`.
- `ChainArgos/java-monorepo` PR #2085, branch `n1-qualification`:
  `baa78037a99c89cde1b9c2201be68e80c9755f67` →
  `7cbe1db11ccabb463053a93a8a08d33bda418b29`; API `updatedAt`
  `2026-10-04T05:01:14Z`.

The companion CSV updates those two PR rows and their API `updatedAt` values;
the other 84 ref rows and every repository identity/order remain unchanged.
Generic reference targets, generic-evidence JSON, `uses:` counts, and positive
path classifications on the two moved PR rows remain from their prior scanned
heads. The 07:53 refresh below rechecks exact needles only, so those generic
fields are not current-head classifications for the two moved PRs.

## Readiness packet still required

All 46 consumer repositories still await the exact immutable artifact packet
for version `0.1.1`, sourced from full commit
`52297bc15ce19e39c84deffe10be76d0ba026d15`. It must contain an immutable
`v0.1.1` release with these three exact binary assets, each with its own
64-character lowercase-hex SHA-256:

- `velnor-actions-0.1.1-x86_64-unknown-linux-gnu`
- `velnor-actions-0.1.1-aarch64-apple-darwin`
- `velnor-actions-0.1.1-x86_64-apple-darwin`

The release must also publish `velnor-actions-release-manifest.json`. Its
schema-1 JSON must bind version `0.1.1`, repository `tailrocks/velnor-new`,
the full source commit above, and exactly one record per supported target
with the official immutable asset URL and 64-character lowercase-hex
SHA-256. The
asset URLs must use
`https://github.com/tailrocks/velnor-new/releases/download/v0.1.1/<asset-filename>`;
the manifest asset uses that same release path and the canonical filename
above. The
release-process packet must include Sigstore/SLSA provenance for each asset,
bound to the source commit and release workflow. Each consumer must then
commit a byte-identical published manifest at
`.velnor/release-manifest.json`, acquire the exact target asset, verify its
SHA-256 before invocation, regenerate with that exact binary, and prove
deterministic parity. The current 0.1.0 consumer pins below are historical
consumer state, not evidence that this 0.1.1 packet exists.

Three recorded owner holds remain: Velnor-actions-fixture disposition and
retired hosted triggers; ChainArgos/java-monorepo PR #2085 qualification; and
ChainArgos/jackin-agent-brown PR #241 rollout/runner/cache qualification.
Jackin PR #1108's validator checkpoint also remains as separately recorded.
No consumer repository or remote GitHub API/data was mutated.

## Prior 03:11 UTC scan and authority

This read-only retirement scan completed at 2026-10-04 03:11 UTC. It supplements the earlier 02:27 UTC snapshot in [consumer-scope-audit-2026-10-04.md](consumer-scope-audit-2026-10-04.md); the earlier result remains bounded to its original six refs and 15 SHAs.

The 46-consumer scope source remains open PR #12 at head `c694d8029eb880db639fa89b0589090dd2b15364` (base `c57c700459bbe1549fe7eedcb7d8689585c38986`, last updated 01:02:17 UTC). Its `scope.json` blob is `33928dd5d3a617bc0b1341b8df81955a54f7607d`; `repository-evidence.csv` is `b1bbcfa8fb8c8f992d6be62e427860aaf2c8c1fa`. The scope has 47 rows including Velnor and 46 consumers. Consumer names and order match both the companion CSV and the historical Appendix C list in `velnor-owned-source-implementation-goal.md`; there are no duplicates or set/order differences. PR #12 remains the scope source; PR #28 at `52297bc15ce19e39c84deffe10be76d0ba026d15` is the current integration candidate.

GitHub resolved every name. All 46 default branches are `main`; all 46 default heads and all 40 open PR heads were fetched and scanned. Final API metadata at about 03:11 UTC matched all 86 scanned full SHAs. No repository or PR head was inaccessible. The only name resolution is historical `tailrocks/tui-snap` → canonical `tailrocks/tuiscotti`, same repository ID `1358764452` (confirmed through both API names).

The original 02:27 CSV recorded jackin-project/jackin PR #1111 at `e426186cc8b7fe3387b8ac8891c7d3b08b0fc0b1`. During the 03:11 refresh, that head advanced to `60f7d661d14af7ebdfbdf21bd48f5c5eebb28888`; that head was fetched and scanned, and the 03:11 metadata check matched it. The later 07:42 snapshot advanced Jackin #1111 to `3a28c199f17da335ecd9abd8dd67ebf1aecc0421` and ChainArgos/java-monorepo #2085 to `7cbe1db11ccabb463053a93a8a08d33bda418b29`. Both refreshed CSVs record those current heads; all other existing scope-audit fields remain from their stated earlier snapshots.

## Search set

Every exact snapshot used `git grep -n -I -F` over the full tracked text tree. The 31 exact literals were nine ref names, 18 unique source/tip/ancestry SHAs, and four generic path literals. The nine refs were:

- `owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96`
- `owned-source/mbx/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81`
- `owned-source/mbx-action/c3cbe8e56ccb4727624df45022357f49d2953075`
- `owned-source/mbx-action/62ec0713473dffeab46884b7c03906042794e696`
- `owned-source/semver-checker/583dddce84706786fc54c41a2c768c28a09c65fd`
- `owned-source/cache-action/8758d976a1b25eb387f48aa04ea86f57739b84cf`
- `owned-source/mbx/1cde36b9f519f1fea8bf4ba8d82721906410448c`
- `owned-source/mbx/7fd10972216ce478478b704781b4a4f2715f46d4`
- `owned-source/mbx/3e462cf9248dd3d17baf9131af5f7a4aec76b80e`

The 15 in-scope SHAs were: MISE `dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96`; MBX `1ca12eb48391061a75e97d32e5b06fedf8a6253d`, `2f324e89af509a1c61d802cf23741a04d7510b57`, `ee250ac37654a4cfbb55b6cd470f2a257204bbe9`, `e07c07cfec773897e5c439043901e661ad3f7b45`, `29865b5a18e3414084ff5db27d1da455772b4c34`, `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81`; MBX action `f053f215866af0ddb6d2f32ecc19d45a5d25edc2`, `198f0d1a538d34a91d7692f8f302259643c0f737`, `06f353d41002af758d27490164f53c82e2165637`, `c3cbe8e56ccb4727624df45022357f49d2953075`, `62ec0713473dffeab46884b7c03906042794e696`; Semver `d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a`, `583dddce84706786fc54c41a2c768c28a09c65fd`; Cache `8758d976a1b25eb387f48aa04ea86f57739b84cf`.

The 02:30:31 UTC post-cutoff discovery adds the excluded MBX ancestry `3e462cf9248dd3d17baf9131af5f7a4aec76b80e` → `7fd10972216ce478478b704781b4a4f2715f46d4` → `1cde36b9f519f1fea8bf4ba8d82721906410448c` → in-scope `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81`. GitHub commit metadata shows the new tip changes only `crates/mbx/src/session_tests.rs`; its parent is `7fd1097…`. The earlier 02:27 scan did not include any of these three excluded refs or tips.

Generic checks also searched literal `owned-source/`, `tailrocks/velnor-new@`, `tailrocks/velnor-new/`, and `github.com/tailrocks/velnor-new`; all `uses:` lines; and repository/action path forms using `uses:`, HTTPS/SSH GitHub URLs, `repository:`, or `repo:` fields for `velnor-new`, `mbx-action`, `mr-boxington`, `mbx`, `mise`, `semver-checker`, and `cache-action`. Exact patterns and per-snapshot outcomes are in the companion CSV.

The generic repository/action path expression was `(uses:[[:space:]]*|https?://github[.]com/|git@github[.]com:|repository:[[:space:]]*|repo:[[:space:]]*)[^[:space:]#]*(tailrocks/velnor-new|velnor-new|mbx-action|mr-boxington|mbx|mise|semver-checker|cache-action)[^[:space:]#]*`; the separate workflow-use enumeration was `uses:[[:space:]]*[^[:space:]#]+`. CSV timestamp fields distinguish the default commit timestamp from each PR record's API `updatedAt` time.

## Results and classification

- All 86 refs in the 07:42 GraphQL snapshot were accessible and match their scanned SHAs: 84 unchanged trees retain the prior scans, and the two moved PR trees were rescanned at 07:53. All nine `owned-source/*` refs, all 18 listed SHAs (including the three excluded MBX ancestry SHAs), and the broad `owned-source/` literal had zero matches in the exact-pattern scans. `tailrocks/velnor-new@` also had zero matches. This is a bounded current-head result, not a claim about repositories or refs outside this union.
- Generic `tailrocks/velnor-new` hits are the expected upstream documentation and immutable release-asset paths. Every default CI workflow stages a Velnor asset URL: 45 use release tag `v0.1.0`; ChainArgos/java-monorepo uses `generator-d40868152f7fe0106e3ede858a411f502f00810f`. The matching `.velnor/release-manifest.json` entries are declarative metadata. `.github/AGENTS.md` mentions are documentation. None is an owned-source branch reference.
- Fifteen default CI workflows stage the stock public MBX action `jdx/mr-boxington-action@9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3`: jackin-project/jackin; tailrocks/termpane, parallax, parallax-telemetry-playground, velnor, velnor-actions-fixture, holla, tracing-request-level, pg-bigdecimal, ruxel, schemalane, tablerock, cloudflare-tofu; ChainArgos/java-monorepo and cloudflare-tofu. This is executable workflow evidence for the stock action; it is not evidence of custom owned-snapshot cache behavior. H7 still requires artifact-bound runtime/resource qualification.
- Other `uses:` results are pinned public setup/cache actions or Velnor generator/source/test fixture text. The scan classifies them by file and path in the CSV. Static references do not establish a runtime invocation. This addendum gathered no new workflow-run logs.
- These bounded ref searches found no positive retired-ref dependency; that result alone does not establish migration readiness or adoption. Existing rollout, task-contract, runtime, and MBX qualification holds remain as recorded in the earlier audit; owner checkpoint remains 2026-10-11 UTC.

## Limits and gate

The scan covers the 46-repository union, each current default tree, and every open PR head at the recorded SHAs. The 07:53 full-tree inventory counted text and binary files in the two moved PR trees; exact-pattern search does not establish semantic coverage of binary contents. The generic-target/`uses:` classifications for those two moved PR rows remain tied to their prior SHAs and were not refreshed. Closed PRs, other nondefault branches, untracked payload, and unnamed repositories/forks outside the union are not covered. The static scan cannot prove actual workflow execution or artifact acquisition, regeneration, or parity.

Before any deletion, verify these default/PR refs still equal the CSV SHAs and route the evidence for owner review. If a head moves, scan that exact new head. No consumer repository, release/tag/asset, remote API/data, or branch/ref was changed; no test/build ran, and no deletion or PR occurred.

## 07:53 UTC exact-pattern full-tree scan

The two moved PR trees were scanned at the exact 07:42 snapshot SHAs using
the 31-needle R1 set listed above. Both scans reported zero matches for the
29 retirement needles: nine owned-source ref names, 18 reviewed or excluded
SHAs, `owned-source/`, and `tailrocks/velnor-new@`. The other two generic URL
needles (`tailrocks/velnor-new/` and `github.com/tailrocks/velnor-new`) match
expected documentation, workflow, and manifest files. Their generic-path
counts and classifications in the CSV retain prior-snapshot provenance for
these moved PRs; they are not current-head counts. Those results are not zero.

| Repository and ref | Snapshot SHA | Text files | Binary files | Retirement-needle matches (29) |
|---|---|---:|---:|---:|
| `jackin-project/jackin` PR #1111 | `3a28c199f17da335ecd9abd8dd67ebf1aecc0421` | 2,519 | 116 | 0 |
| `ChainArgos/java-monorepo` PR #2085 | `7cbe1db11ccabb463053a93a8a08d33bda418b29` | 7,007 | 5,939 | 0 |

The file counts describe the full-tree inventory for these two snapshots.
The zero result is bounded to the 29 retirement needles. Generic URL/path and
workflow-use classifications remain separate in the CSV and retain their
prior-snapshot provenance for these two moved PRs. No runtime claim is made.
