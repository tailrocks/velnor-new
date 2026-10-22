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

The upgrade workflow is disabled by default. Enable it in `.velnor/config.toml` and regenerate:

```toml
[upgrades]
enabled = true
# schedule = "17 5 * * *"                       # daily, UTC (default)
# app_id_variable = "VELNOR_DEPS_APP_ID"        # default
# app_private_key_secret = "VELNOR_DEPS_APP_PRIVATE_KEY"  # default
# stuck_days = 3                                # 1..=30
```

There is no cooldown key. The job runs with `MISE_MINIMUM_RELEASE_AGE=0s`.
The parser rejects unknown keys such as `cooldown`, `scopes`, and `minimum_release_age`.

## What `upgrades.yml` does

The workflow runs on `schedule` and `workflow_dispatch` events only.
Each job runs only on the default branch. One concurrency group runs jobs one at a time without cancellation.

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
     workflow reverses and records only the Rust update.
     The other proposed changes remain for CI checks. The freshness gate continues to fail until a Velnor release adds the digest.
   - Only upgrade-owned paths are kept (`mise.toml`, `mise.lock`,
     `rust-toolchain.toml`/`rust-toolchain`,
     `.velnor/release-manifest.json`, `.github/**`, `Cargo.toml`/`Cargo.lock`,
     `package.json`/`bun.lock`, Gradle catalog, lock and verification files).
     Anything else is left out and reported. The result is uploaded as a
     binary patch plus a Markdown summary.
   - If an upgrade scope fails, the workflow reverses its supported changes and lists the failure under **Needs migration**.
     For example, a private Git or npm source can require a fetch credential that this job does not hold.
2. **Open or update the upgrade pull request** (`contents: read`; runs only
   when there is a patch): re-checks every patch path, mints an App
   installation token for this repository only, applies the patch with hooks
   disabled, and commits it as a merge of the previous `velnor/upgrades` head
   and the current default branch. The branch only fast-forwards (no force
   push) and the pull request diff is exactly the upgrade. It opens the one
   pull request or updates its body. Normal pull-request CI then qualifies it;
   Pushes with the App token trigger workflows.
3. **Freshness gate** (`contents: read`, `pull-requests: read`, runs even when
   prepare failed): fails with `pins_not_latest` when prepare found anything
   to upgrade, `upgrade_prepare_failed` when prepare did not succeed, and
   `upgrade_pull_request_stuck` when the open `velnor/upgrades` pull request
   is older than `stuck_days`. It exists only in `upgrades.yml`, never in
   `ci.yml` or `Required`, so an upstream release never blocks unrelated pull
   requests.

## Credentials

The job that runs Mise, Cargo, Bun, Gradle, and Velnor does not hold the App token.
The job that holds the token runs fixed Git and GitHub CLI commands over a patch with filtered paths.
It does not run repository build or upgrade commands with the token.

The minted token requests exactly:

| Permission | Why |
|---|---|
| `contents: write` | push `velnor/upgrades` |
| `pull_requests: write` | open or edit the one upgrade pull request |
| `workflows: write` | GitHub rejects an App push that changes `.github/workflows/**` without it, and regenerating `.github/` is part of the job |

The `repositories` input is `${{ github.event.repository.name }}`, which limits the token to this repository.
The token action attempts revocation when the job ends.

**Owner action required.** On 2026-10-22, the `velnor-deps` App had only `contents: read` and `metadata: read`.
Its ID was 5394413, its owner was ChainArgos, and the query was `gh api /apps/velnor-deps`.
Before enabling `[upgrades]`, grant `Contents: Read and write`, `Pull requests: Read and write`, and `Workflows: Read and write`.
Accept the new permissions on each installation and confirm that the installation includes the repository.
These permissions increase the authority of the dependency-fetch App private key in each installed repository.
To separate upgrade and dependency-fetch authority, configure a different App with `app_id_variable` and `app_private_key_secret`.