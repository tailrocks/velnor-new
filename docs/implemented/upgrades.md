# Daily upgrade pull request and freshness gate (`[upgrades]`)

- State: implemented (this pull request)
- Status: adopted per the owner rule "every project always uses the latest
  stable Rust, dependency and tool, with no cooldown"; owner may revisit.
- Specification: this page; generator-owned `.github/` ([generated file
  contract](../proposed/generated-file-contract.md)).
- Delivered: `UpgradesConfig` (`velnor-actions-contract`, `config/upgrades.rs`),
  `render_upgrades_workflow` (`velnor-actions-workflow-renderer`,
  `upgrades.rs` plus `upgrades_{prepare,publish,gate}.sh` and the shared
  `upgrades_patch_filter.sh`),
  `scripts/upgrades/bump-gradle-catalog.py`, and emission and plan listing
  in `velnor-actions-orchestrator` (`upgrades_emit.rs`).
- Acceptance evidence: `config::upgrades::tests`, `upgrades::filter_exec_tests`
  (the rendered scripts executed against crafted attack patches, see
  [Patch filter](#patch-filter)), `upgrades::tests` (including
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
   - Only what the [patch filter](#patch-filter) admits is kept. Anything
     else is unstaged and reported. The result is uploaded as a text patch
     (`git diff --cached --no-renames --full-index`, never `--binary`) plus a
     Markdown summary, after the filter has checked that exact patch.
   - A scope that fails (for example a private git or npm source that needs a
     fetch credential, which this job never holds) is rolled back and listed
     under **Needs migration** in the pull request body.
2. **Open or update the upgrade pull request** (`contents: read`; runs only
   when there is a patch): runs the patch filter again on the downloaded
   patch, in a scratch index, before the worktree sees a byte, mints an App
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

## Patch filter

`upgrades_patch_filter.sh` is shared by prepare and publish. It validates the
patch text and the index the patch produces; it never relies on `git apply`
rejecting anything.

| Rule | Code |
|---|---|
| Every path is upgrade-owned: `mise.toml`, `.mise.toml`, `mise.lock`, `.mise.lock`, `rust-toolchain.toml`/`rust-toolchain`, `.velnor/release-manifest.json`, `.github/**`, `Cargo.toml`/`Cargo.lock`, `package.json`/`bun.lock`, Gradle `libs.versions.toml`, `gradle.lockfile` and `verification-metadata.xml` | `upgrade_patch_path_rejected` |
| Paths are plain and relative: no leading `/`, empty, `.` or `..` segment, no `.git` segment in any case, only `A-Za-z0-9._/@+-` | `upgrade_patch_path_rejected` |
| No rename or copy, whatever its source or destination (`rename`/`copy from`/`to`, `similarity index`); prepare builds the patch with `--no-renames`, so a move is its deletion plus its addition and both paths are checked | `upgrade_patch_rename_rejected` |
| Only generated `.github/**` files may be deleted | `upgrade_patch_delete_rejected` (owned) or `upgrade_patch_path_rejected` (foreign) |
| Every file stays a regular, non-executable file (mode `100644`): no `old mode`/`new mode`, no other `new file mode`, `deleted file mode` or `index` mode, no type change | `upgrade_patch_mode_rejected` |
| No symlink (`120000`), new or replacing a file | `upgrade_patch_symlink_rejected` |
| No binary change (`GIT binary patch`, `Binary files`, a `-` numstat); no path is allowed one | `upgrade_patch_binary_rejected` |
| Every file patch is a `diff --git` section, one per `git apply --numstat -z` record; no CR bytes | `upgrade_patch_malformed` |

Publish checks the text, applies the patch to a scratch index built from
`HEAD` and checks every entry of that index against `HEAD`
(`git diff --cached --raw -z --no-renames`, so modes come from Git's own
index), and only then applies it to the real index and worktree and checks
again. Prepare runs the same index check in drop mode (each violation is
unstaged and listed under **Needs migration**) and then the full check on the
patch it uploads.

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
