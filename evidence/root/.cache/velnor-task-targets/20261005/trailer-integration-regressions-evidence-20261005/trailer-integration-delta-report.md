# Trailer integration regression delta

Collected 2026-10-05 in an isolated validation copy. No shared checkout, index, branch, or remote ref was modified, and no Python was created or run.

## Source and patches

Validation source: `/root/.cache/velnor-task-targets/20261005/trailer-integration-regressions-20261005`, private branch `codex/trailer-integration-check`, HEAD `c7ee94a48d1ac9504aecc5ae09a4aaa15941cd38`; private target directory is `target/`. The patch preimages are the steward's existing WIP at `/tmp/velnor-release-intent-guard-af82-candidate-20261005` (based on e608f820), not the e608 commit itself. Each patch was checked with `patch --dry-run -p1` against that WIP.

- `dependency-audit-and-lock.patch`, SHA-256 `08820e5868ec0a2f206906697789870334bb4957e766f90bb7ac3cbada92eed4`: moves exact `unicode-general-category = 1.1.0` to CLI dev-dependencies; removes only the proven-unused CLI `proc-macro2` and `syn` dev edges; retains all other crates' edges; updates the lock; dependency policy scanning now includes examples and maps the crate name `unicode_general_category`.
- `git-identity-guard.patch`, SHA-256 `269d29cc293c34d93251b7760b3917110efc6566d9a365952250e54997141333`: keeps the broad direct-Git spawn guard and admits only the one example's fixed, read-only `git var GIT_{AUTHOR,COMMITTER}_IDENT` calls after asserting its exact argv, cwd, and optional-lock setting. It also carries rustfmt's removal of one extra blank line in the existing trailer fixture support file.
- `raw-index-fixture.patch`, SHA-256 `06a3b99cb69c6a931ae993bd645e151b67aa2c88ac2546d6e4162f500542516c`: migrates two direct Git commands in the non-UTF-8 tracked-index regression to existing `git_fixture::command(root)` while preserving `hash-object -w` and raw `update-index --cacheinfo` behavior.

Current postimage SHA-256 values:

- `Cargo.lock`: `0752e0e2a9a72f47508140689e1278aa86545d20af36b2eda3e84729813d2fcc`
- `crates/velnor-actions-cli/Cargo.toml`: `2746eb87acdeac873bd77c955e8d3f4fc6d2d9cb9948dbf14fda65dc8b5f4957`
- `crates/velnor-actions-cli/tests/impl_repo_policy.rs`: `b3ed0d61854b9ea678d0dc4419fb6e78bb865559f281c4138fe5c6bbc90d9f6d`
- `crates/velnor-actions-cli/tests/impl_repo_deps.rs`: `b93a29d61134fd6da4798b625e92b30c1b34a51b8b4b1472ebabc47c186a6bb8`
- `crates/velnor-actions-cli/tests/impl_cli_git_isolation.rs`: `c020b7a6309412598fdc7b5b13de2442f9f1014cd7b3bc651e11d794bd7e9a0b`
- `crates/velnor-actions-cli/tests/support/validate_commit_trailers.rs`: `dac3faa180748caf4640ac591ea4ea2e33999dd544e96cd426ed2f390f8f8dde`
- `crates/velnor-actions-orchestrator/tests/impl_discovery_cache.rs`: `6a9dc3c084c7da4602bf64c55cf30426a2bd8809d347b8127b6f01ec79256ca2`

## Verification

Commands ran with Rust 1.98.1, `--locked --offline`, one Cargo job, private `HOME`, private target, and isolated Git config:

- Focused trailer example: 24 passed, 0 failed. Log SHA-256 `29e56e0ce322b80766ff90613468ec8873526a4d018b0ae190098d864425b84b`.
- Full CLI all-target tests: 242 passed, 1 failed. The sole failure is the existing independent layout guard because `crates/velnor-actions-orchestrator/src/lib.rs` is 151 lines. This is the freshness worker's separate cap repair. Log SHA-256 `0fc0bc949aea7a662510a09f7a9eaa45f49c6848a9c8a00c291f7814e489e3c8`.
- CLI all-target strict Clippy (`-D warnings`): passed. Log SHA-256 `d0f3f8a039f440f1ae4b90a3e43288b7b90097f06d5dde5a1458a3ec0fc835d3`.
- Workspace `cargo fmt --all -- --check`: passed (empty output). SHA-256 of empty log `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
- Raw non-UTF-8 index regression: 1 passed, 0 failed. Log SHA-256 `7b35046fdc91476e45b84b80503b9a663e61162c2019591428a8b30fe64a63c6`.
- The dependency audit and direct-Git isolation guard both pass in the full CLI run. Earlier focused logs are retained as `dependency-audit-rerun.log` (SHA-256 `3270886f97ff5629ba9902a4fa33db6a8fe16bb744c08d3e9201d301934b246c`) and `git-isolation-guard.log` (SHA-256 `15de3c5e3cca63ddf001df0e47eed4b996d2f0846a48d94b0f8e041beff251cf`).
- Final focused reruns also passed: dependency-audit test 1/1, log `dependency-audit-final.log` SHA-256 `874cb8c22c350980a61ac808a1557e6f6dc49e188818a37d02572035edb73eeb`; direct-Git isolation test 1/1, log `git-isolation-final.log` SHA-256 `15de3c5e3cca63ddf001df0e47eed4b996d2f0846a48d94b0f8e041beff251cf`.
- All three exported patches apply cleanly in dry-run mode to the exact steward WIP preimages.

The environment's actual `mise --version` was 2026.10.1, while `.mise-version` declares 2026.9.18. Tests used the installed PATH executable. This is an environment caveat, not a claim that the pinned Mise version was qualified.
