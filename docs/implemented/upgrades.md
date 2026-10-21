# Daily upgrade pull request and freshness gate (`[upgrades]`)

- State: implemented (this pull request)
- Status: adopted per the owner rule "every project always uses the latest
  stable Rust, dependency and tool, with no cooldown"; owner may revisit.
- Specification: this page; generator-owned `.github/` ([generated file
  contract](../proposed/generated-file-contract.md)).
- Delivered: `UpgradesConfig` (`velnor-actions-contract`, `config/upgrades.rs`),
  `render_upgrades_workflow` (`velnor-actions-workflow-renderer`,
  `upgrades.rs` plus `upgrades_{prepare,publish,gate}.sh`),
  `scripts/upgrades/bump-gradle-catalog.py`, and emission and plan listing
  in `velnor-actions-orchestrator` (`upgrades_emit.rs`).
- Acceptance evidence: `config::upgrades::tests`, `upgrades::tests` (including
  the byte-exact golden `crates/velnor-actions-workflow-renderer/tests/goldens/upgrades.yml`),
  `impl_upgrades`. The golden passes actionlint 1.7.12 with shellcheck 0.11.0
  and zizmor 1.31.1 with no findings.
- Deviations: the App token also asks for `workflows: write` (see
  [Credentials](#credentials)).
- Follow-up: the owner grants the App permissions below; then enable
  `[upgrades]` in processor-shared-rust first (WP-E5 rollout).

## Configuration

Off by default. Enable it in `.velnor/config.toml` and regenerate:

```toml
[upgrades]
enabled = true
# schedule = "17 5 * * *"                       # daily, UTC (default)
# app_id_variable = "VELNOR_DEPS_APP_ID"        # default
# app_private_key_secret = "VELNOR_DEPS_APP_PRIVATE_KEY"  # default
# stuck_days = 3                                # 1..=30
```

There is no cooldown key: the job always runs with
`MISE_MINIMUM_RELEASE_AGE=0s`. Unknown keys (`cooldown`, `scopes`,
`minimum_release_age`) are rejected.

## What `upgrades.yml` does

Triggers: `schedule` and `workflow_dispatch` only. Every job runs only on the
default branch. One concurrency group, never cancelled.

1. **Prepare upgrade** (`contents: read`, no credential, 60 minutes):
   - **Velnor itself**: reads
     `tailrocks/velnor-new/releases/latest/download/release-manifest.json`,
     validates its schema, repository, version and commit, and replaces
     `.velnor/release-manifest.json` when it is newer. It downloads the
     manifest's Linux asset from the exact versioned release URL and checks
     its SHA-256 before running it.
   - **Mise tools**: `mise upgrade --bump` for every `[tools]` entry except
     Rust, then Rust on its own, then `mise lock --platform
     linux-x64,linux-arm64,macos-arm64` when `mise.lock` is tracked. The
     `channel` of every tracked `rust-toolchain.toml` (and plain
     `rust-toolchain`) follows the new Rust version, because the standard
     layout keeps that file and it must agree with `mise.toml`.
   - **Cargo**: in every directory with a tracked `Cargo.lock`,
     `cargo upgrade --incompatible --pinned` (cargo-edit, pinned
     `CARGO_EDIT_VERSION`), `cargo update`, then `cargo metadata --locked`.
   - **npm through Bun**: `bun update --latest --ignore-scripts` in every
     directory with a tracked `bun.lock`.
   - **Gradle version catalogs**: every `[versions]` entry referenced by a
     library or plugin moves to the newest stable release on Maven Central or
     the Gradle Plugin Portal that every referencing module publishes, on the
     same release line (`33.x-jre` stays `-jre`). Then
     `./gradlew --write-locks --write-verification-metadata sha256 dependencies`.
   - **Regeneration**: `velnor-actions generate` with the selected Velnor
     rewrites `.github/` and every generated file. If the newest Rust is not in
     that Velnor's [channel-manifest catalog](rust-channel-manifest.md), the
     Rust bump alone is rolled back and recorded, so the pull request stays
     green and the gate stays red until a Velnor release adds the digest.
   - Only upgrade-owned paths are kept (`mise.toml`, `mise.lock`,
     `rust-toolchain.toml`/`rust-toolchain`,
     `.velnor/release-manifest.json`, `.github/**`, `Cargo.toml`/`Cargo.lock`,
     `package.json`/`bun.lock`, Gradle catalog, lock and verification files).
     Anything else is left out and reported. The result is uploaded as a
     binary patch plus a Markdown summary.
   - A scope that fails (for example a private git or npm source that needs a
     fetch credential, which this job never holds) is rolled back and listed
     under **Needs migration** in the pull request body.
2. **Open or update the upgrade pull request** (`contents: read`; runs only
   when there is a patch): re-checks every patch path, mints an App
   installation token for this repository only, applies the patch with hooks
   disabled, and commits it as a merge of the previous `velnor/upgrades` head
   and the current default branch. The branch only fast-forwards (no force
   push) and the pull request diff is exactly the upgrade. It opens the one
   pull request or updates its body. Normal pull-request CI then qualifies it;
   App-token pushes trigger workflows.
3. **Freshness gate** (`contents: read`, `pull-requests: read`, runs even when
   prepare failed): fails with `pins_not_latest` when prepare found anything
   to upgrade, `upgrade_prepare_failed` when prepare did not succeed, and
   `upgrade_pull_request_stuck` when the open `velnor/upgrades` pull request
   is older than `stuck_days`. It exists only in `upgrades.yml`, never in
   `ci.yml` or `Required`, so an upstream release never blocks unrelated pull
   requests.

## Credentials

The token never coexists with repository code: the job that runs `mise`,
Cargo, Bun, Gradle and Velnor holds no credential, and the job that holds the
token runs only `git` and the pinned GitHub CLI over a path-filtered patch.

The minted token requests exactly:

| Permission | Why |
|---|---|
| `contents: write` | push `velnor/upgrades` |
| `pull_requests: write` | open or edit the one upgrade pull request |
| `workflows: write` | GitHub rejects an App push that changes `.github/workflows/**` without it, and regenerating `.github/` is part of the job |

`repositories` is `${{ github.event.repository.name }}`, so the token covers
this repository only. The token is revoked when the job ends.

**Owner action required.** On 2026-10-22 the `velnor-deps` App (id 5394413,
owner ChainArgos) had only `contents: read` and `metadata: read`
(`gh api /apps/velnor-deps`). Before enabling `[upgrades]`, grant the App
`Contents: Read and write`, `Pull requests: Read and write` and
`Workflows: Read and write`, accept the new permissions on each installation,
and make sure the installation includes the repository. Granting write to the
App that also serves read-only dependency fetches widens what its private key
can do in every installed repository; a separate upgrade App, named through
`app_id_variable`/`app_private_key_secret`, keeps the two apart.
