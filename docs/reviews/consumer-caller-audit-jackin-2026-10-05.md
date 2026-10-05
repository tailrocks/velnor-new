# Jackin consumer caller scan

Probe time: 2026-10-05 03:00 UTC for the GitHub default-head and open-PR refresh. This is a bounded actual-content scan of the eight Jackin repositories in the current PR12 scope snapshot. Current consumer files were fetched by immutable commit SHA; GitHub code-search index results were not used. No API/access errors occurred.

## Scope and target corpus

PR12 was at `8625f5d191bffc807f7b80a607ad1fc51cbc171b` (`perf/cache-selection-qualification`, updated 2026-10-05T02:29:09Z). Scope inputs at that head were `scope.json` blob `33928dd5d3a617bc0b1341b8df81955a54f7607d`, `repositories.txt` blob `4a8fd7a4ff342e941fc08ac3a5dd1f12feeaf6ab`, and `repository-evidence.csv` blob `b1bbcfa8fb8c8f992d6be62e427860aaf2c8c1fa`. These scope inputs are absent (404) on PR12's main head `7fb8367d7daa67f13ccaa7c76caae47d55d6262b`; scope provenance is therefore **UNKNOWN/reconstructed**, as recorded by the coordinator. This eight-repository lane does not establish global consumer closure.

Target terms were taken from current PR12 owned-source material: `docs/reviews/owned-source-review-2026-10-04/owned-source-inventory.json` blob `e9d6f7c5fdacad79347e4010cb0cbdceb5951476`, `owned-source-inventories.md` blob `c2c6002eb8b4826f7d181dc28c43a9dca9c82abb`, `owned-source-review.md` blob `cd36727520c52d92a458177f6a485031e87000ee`, `owned-source-spec.md` blob `6b85d376380b9670df5cd16f5914e063db79aece`, and the current goal/continuation blobs `fc6ed138701dc25249db8694d047bdd6b690d87b` and `d62bd2926364802f211d2cb09a5cce832bfce2bf`. The six full owned-source refs/heads were:
- `owned-source/cache-action/8758d976a1b25eb387f48aa04ea86f57739b84cf`
- `owned-source/mbx/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81`
- `owned-source/mbx-action/c3cbe8e56ccb4727624df45022357f49d2953075`
- `owned-source/mbx-action/62ec0713473dffeab46884b7c03906042794e696`
- `owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96`
- `owned-source/semver-checker/583dddce84706786fc54c41a2c768c28a09c65fd`

The searched vocabulary included Foundation/foundation-qualification; `owned-source`, all refs and pinned SHAs above; `MISE_OWNED_CARGO_WRAPPER`, `owned-cache-transport`, `comparison-state`, `expected-binary-sha256`, `velnor-compare`, `cargo-semver-checks-owned`, supplied-semantics, SDK/quarantine, IPC 12/13, export v2, comparison report versions, snapshot usefulness/retention, root-pair restore, lineage/durable state, compiler-event; and retired build/publication/qualification identifiers and test stems from the PR12 owned-source review (including `owned_mise_qualification`, `analyze-ci-performance`, `collect-ci-performance`, `bootstrap-owned-tool-builder`, `build-owned-tool`, `download-owned-tool-candidate`, `immutable_publication`, `owned_tool_behavior`, `owned_tool_qualification_evidence`, `owned_tool_source`, `publish-owned-tool-artifacts`, `publish_owned_source`, `qualify-owned-tool`, `source_proof_capsule`, `source_publication`, `source_publication_records`, `source_qualification_execution`, `source_release_policy`, `source_semver_capsule`, `stage-owned-tool-source`, plus corresponding `test_*` and fixture stems).

## Default-branch exact-head inventory

The REST tree responses were complete (`truncated:false`); all eight default branch names were `main`. Commit time below is the commit committer timestamp; `pushed_at` is repository metadata, not the probe time.

| Repository | Head SHA | Commit time (UTC) | pushed_at (UTC) | Tree entries |
|---|---|---|---|---:|
| jackin-project/jackin | `0aa821a088e1bacf3d4d85a4c9faaa67faa85132` | 2026-10-03T12:25:30Z | 2026-10-05T02:36:44Z | 3464 |
| jackin-project/jackin-agent-smith | `2e7119b9c668ca7a9c55218b20049885299d198f` | 2026-10-02T13:50:53Z | 2026-10-02T13:50:56Z | 32 |
| jackin-project/homebrew-tap | `cd05a0ea2cf68fd6c2753ee938247b2dcd4c7551` | 2026-10-02T13:51:06Z | 2026-10-02T13:51:08Z | 32 |
| jackin-project/jackin-role-action | `59e538704b8c119f3b6668cea0154909a6158ff6` | 2026-10-02T13:51:18Z | 2026-10-02T15:15:44Z | 28 |
| jackin-project/jackin-sentinel | `587a0d1a8eef96108c9d9d530bdaa13df91edd2b` | 2026-10-02T13:51:34Z | 2026-10-02T15:17:15Z | 36 |
| jackin-project/jackin-dev | `a01b342162bc56cdf1e8bbaab793e73d31c1d621` | 2026-10-02T13:51:48Z | 2026-10-02T15:15:27Z | 77 |
| jackin-project/jackin-github-terraform | `b43a2314c6b58906d52828115d3b3973026f6a2a` | 2026-10-02T13:51:59Z | 2026-10-02T15:16:03Z | 37 |
| jackin-project/jackin-the-architect | `2cf461e2fed1b95d9fd1e7ba74c10d4d8b1c685d` | 2026-10-02T13:52:14Z | 2026-10-05T00:35:45Z | 44 |

## Direct content coverage and findings

At each listed main SHA I read `.github/workflows/ci.yml`, `.velnor/config.toml`, and `.velnor/release-manifest.json`. No `.github/actions/**` files or additional local reusable workflows were present in these trees. The workflow contents had no `workflow_call`, `uses: ./...`, `.github/actions/`, or shell-script invocation edges, except the explicit action script noted below. The following tracked scripts were read at the corresponding immutable heads:
- `jackin`: `scripts/desktop-install-proof.sh`, `scripts/hooks-install`, `scripts/phase0-apple-container.sh`, `scripts/pre-commit-snapshot`, `scripts/test-pre-commit-snapshot`.
- `jackin-agent-smith`: `scripts/marketplace-audit.sh`.
- `homebrew-tap`: `scripts/package-update.sh`, `scripts/test-package-update.sh`.
- `jackin-role-action`: root `action.yml` and `scripts/download-jackin-role.sh`; action.yml line 39 invokes the latter through `${{ github.action_path }}`.
- `jackin-sentinel`: `scripts/marketplace-audit.sh`.
- `jackin-dev`, `jackin-github-terraform`: no tracked `scripts/` paths.
- `jackin-the-architect`: `scripts/marketplace-audit.sh`, `scripts/update-jackin-toolchain.rs`.

Direct actual-content results at those heads: no target hits for Foundation/foundation-qualification, any owned-source branch/ref/full SHA, retired source-build/publication/qualification identifiers, or the custom protocol/state markers listed above. No owned-source action or branch pin was found. Do not read this as a scan of every arbitrary documentation or source file in these repositories.

Positive strings and their classification:
- `jackin-project/jackin/.github/workflows/ci.yml` at main `0aa821a...`, line 181: a `velnor-plan` upload-artifact name in the `Publish plan` step; the file has 30 such generated artifact-name hits. This is ordinary workflow artifact naming, not the `cargo-semver-checks-owned` target.
- The other seven `main:.github/workflows/ci.yml` files contain generated `velnor-plan` artifact names (3 hits each for Agent Smith, Homebrew Tap, role action, Sentinel, dev, and Architect; 4 for GitHub Terraform). These are workflow output names, not owned-source implementation or pins.
- Jackin main `.velnor/config.toml` sets `compile_driver = "mbx"` and `test_runner = "cargo_nextest"`. Its current `.github/workflows/ci.yml` lines 423–427 install Mr Boxington 1.21.0 and call `jdx/mr-boxington-action@9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3` with `github-cache-mode: objects` and `version: 1.21.0`. This is a current workflow pin to the stock third-party action, not a pin to an owned-source branch.
- The pinned external action was also read at immutable SHA `9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3` (commit date 2026-09-24T23:12:38Z; tree 50 entries, complete). `action.yml` routes to `dist/index.js`; `src/index.ts`, `src/lib.ts`, `src/target-cache.ts`, and `dist/index.js` were inspected. It uses ordinary MBX `cache import` (source line 412) and `cache export --group` (lines 493–510) through GitHub Actions cache. No owned-source branch/protocol markers were present. This confirms an active stock MBX object-cache path in Jackin only; the other seven workflows had no `jdx/mr-boxington-action`, `mr-boxington@`, or `mbx` command hits.
- Jackin's `crates/jackin-xtask/src/main.rs:94` has a generic JUnit flake-quarantine comment; this is an unrelated false positive for the broad word quarantine. `jackin-dev` and `jackin-the-architect` .velnor configs contain commented optional action-pin examples only, not active workflow calls.

## Open pull requests at refresh

Three open PRs were returned across these eight repositories.

| PR | Head SHA / time | Changed-path and caller closure | Result / limit |
|---|---|---|---|
| jackin-project/jackin#1108 | `2990df17e25f30afca84804d9c402abc1ce00231`; commit 2026-09-30T10:15:12Z; PR updated 10:21:33Z | Head tree had 3,480 entries, `truncated:false`. Relevant changed paths: `.github-gen/velnor-workflow.toml`, `.github/ci/project.toml`, `.github/ci/.github-actions-generator-state`, `.github/workflows/ci-evidence.yml`, `.github/workflows/ci-push-head-ledger.yml`, `mise.toml`, `crates/jackin-xtask/src/main.rs`, `cmd.rs`, `ci_evidence.rs` and listed submodule files. Both changed workflows invoke `mise run ci-evidence` / `mise run ci-push-head-ledger`; the mise tasks map to `cargo xtask ci-evidence run|record-push`; the changed Rust caller modules were read. | No custom owned-source target markers. `.github/ci/project.toml` has stock `mbx deny check` / `mbx audit`, with Mr Boxington 1.12.0; `config/fleet/velnor-host.env` has ordinary `MBX_GC_MAX_TOTAL_SIZE=50GiB` and `VELNOR_MBX_GENERATION_BOUND=6`. `main.rs:94` has only the generic quarantine comment noted above. The final `.github-gen` / `.github/ci` probe completed against this PR head and complete tree. Unmodified sibling workflows on this PR head were not inspected. |
| jackin-project/jackin-the-architect#479 | `0592d0deeaeaa5b785fa67a43d23d3b627552720`; commit 2026-10-05T00:35:23Z (author 00:32:22Z); PR updated 00:36:05Z | Complete 44-entry head tree; sole changed path `jackin.role.toml`. Read that file plus PR-head `.github/workflows/ci.yml`, `.velnor/**`, and scripts. | No custom target markers or additional caller edges found in this changed-path/caller closure. |
| jackin-project/jackin#1111 | `3a28c199f17da335ecd9abd8dd67ebf1aecc0421`; commit 2026-10-04T04:02:49Z; PR updated 04:14:24Z | PR head contents not inspected. GitHub changed-file inventory was path-filtered: `.codex/config.toml`, application/Rust code and tests, and docs; no changed workflow, action, `.velnor/`, script, `mise.toml`, or identified caller path. | Explicit limitation: no claim about unrelated/unmodified paths or target markers in the entire #1111 head. |

No open PRs were reported for the other five repositories at the 03:00Z refresh.

## Limits

This report covers only the eight reconstructed Jackin scope rows, their exact refreshed main heads, the three open PR inventories, the explicitly followed relevant changed paths/callers, and the external pinned action commit described above. PR #1111 is path-filtered/uninspected; PR #1108's unmodified sibling workflows are uninspected. Arbitrary repository docs/source, historical branches/tags, closed PRs, and every other consumer in the reconstructed union were not exhaustively scanned. No Tailrocks or ChainArgos repository is included, and this report does not assert global consumer rollout or consumer-scope completeness.
