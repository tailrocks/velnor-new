# Consumer retirement union — refreshed heads, 2026-10-04

## Authority and cutoff

This read-only retirement scan completed at 2026-10-04 03:11 UTC. It supplements the earlier 02:27 UTC snapshot in [consumer-scope-audit-2026-10-04.md](consumer-scope-audit-2026-10-04.md); the earlier result remains bounded to its original six refs and 15 SHAs.

The refreshed current candidate scope is open PR #12 at head `c694d8029eb880db639fa89b0589090dd2b15364` (base `c57c700459bbe1549fe7eedcb7d8689585c38986`, last updated 01:02:17 UTC). Its `scope.json` blob is `33928dd5d3a617bc0b1341b8df81955a54f7607d`; `repository-evidence.csv` is `b1bbcfa8fb8c8f992d6be62e427860aaf2c8c1fa`. The scope has 47 rows including Velnor and 46 consumers. Consumer names and order match both the companion CSV and the historical Appendix C list in `velnor-owned-source-implementation-goal.md`; there are no duplicates or set/order differences.

GitHub resolved every name. All 46 default branches are `main`; all 46 default heads and all 40 open PR heads were fetched and scanned. Final API metadata at about 03:11 UTC matched all 86 scanned full SHAs. No repository or PR head was inaccessible. The only name resolution is historical `tailrocks/tui-snap` → canonical `tailrocks/tuiscotti`, same repository ID `1358764452` (confirmed through both API names).

The 02:27 CSV is stale for jackin-project/jackin PR #1111: that head advanced from `e426186cc8b7fe3387b8ac8891c7d3b08b0fc0b1` to `60f7d661d14af7ebdfbdf21bd48f5c5eebb28888` during this refresh. The new head was fetched and scanned; its four-file delta is limited to account/schema/workspace editor code and tests. The 03:11 metadata check still matched it. The refreshed per-head CSV records the current SHA for each default and PR snapshot.

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

- All 86 current default/PR trees were accessible and scanned. All nine `owned-source/*` refs, all 18 listed SHAs (including the three excluded MBX ancestry SHAs), and the broad `owned-source/` literal had zero matches in every snapshot. `tailrocks/velnor-new@` also had zero matches. This is a bounded current-head result, not a claim about repositories or refs outside this union.
- Generic `tailrocks/velnor-new` hits are the expected upstream documentation and immutable release-asset paths. Every default CI workflow stages a Velnor asset URL: 45 use release tag `v0.1.0`; ChainArgos/java-monorepo uses `generator-d40868152f7fe0106e3ede858a411f502f00810f`. The matching `.velnor/release-manifest.json` entries are declarative metadata. `.github/AGENTS.md` mentions are documentation. None is an owned-source branch reference.
- Fifteen default CI workflows stage the stock public MBX action `jdx/mr-boxington-action@9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3`: jackin-project/jackin; tailrocks/termpane, parallax, parallax-telemetry-playground, velnor, velnor-actions-fixture, holla, tracing-request-level, pg-bigdecimal, ruxel, schemalane, tablerock, cloudflare-tofu; ChainArgos/java-monorepo and cloudflare-tofu. This is executable workflow evidence for the stock action; it is not evidence of custom owned-snapshot cache behavior. H7 still requires artifact-bound runtime/resource qualification.
- Other `uses:` results are pinned public setup/cache actions or Velnor generator/source/test fixture text. The scan classifies them by file and path in the CSV. Static references do not establish a runtime invocation. This addendum gathered no new workflow-run logs.
- No owned-source migration is indicated by these bounded ref searches. Existing rollout, task-contract, runtime, and MBX qualification holds remain as recorded in the earlier audit; owner checkpoint remains 2026-10-11 UTC.

## Limits and gate

The scan covers the refreshed 46-repository candidate union, each current default tree, and every open PR head at the recorded SHAs. It does not cover closed PRs, other nondefault branches, untracked/binary payload, or unnamed repositories/forks outside the candidate union. `git grep -I` skips binary blobs. The static scan cannot prove actual workflow execution.

Before deletion, verify these default/PR refs still equal the CSV SHAs and route the evidence for owner review. If a head moves, scan that exact new head. No consumer repository was changed, no test/build ran, and no branch deletion or PR occurred.
