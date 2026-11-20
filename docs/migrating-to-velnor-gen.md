# Migrating from velnor-actions to velnor-gen

This page is for a project that runs Rust CI on the old velnor-actions engine and moves it to `velnor-gen`. The new generator writes one workflow, `.github/workflows/rust-ci.yml`, from `.velnor/rust-ci.toml`. The settings are in [`rust-ci.md`](rust-ci.md) and [`generator/CONFIG.md`](../generator/CONFIG.md).

Most failed migrations failed before the generator ran: on the mise layout, on a file Git does not track, or on a dependency the generator cannot fetch. Check the prerequisites first.

## Prerequisites

### The mise and mbx layout

`velnor-gen` requires the standard layout. Its `mise.toml` declares the Rust toolchain with `mr_boxington = true`, pins mbx, and routes every `cargo` through mbx:

```toml
min_version = "2026.11.10"

[tools]
rust = { version = "1.100.0", profile = "minimal", components = ["clippy", "rustfmt"], targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"], mr_boxington = true }
mr-boxington = { version = "v1.30.0", workflow = "release.yml@refs/heads/main" }

[settings]
lockfile = true

[wrappers.cargo]
command = "mbx"
env = { MBX_CARGO_SHIM_MODE = "1" }
```

The versions above were the newest releases when this page was written. Check the newest mise, mbx and stable Rust releases before you pin, and update `mise.toml` and `rust-toolchain.toml` together.

`rust-toolchain.toml` mirrors the `rust` entry exactly (channel, components and targets). Mise ignores the file's options, and generation fails on any drift between the two:

```toml
[toolchain]
channel = "1.100.0"
profile = "minimal"
components = ["clippy", "rustfmt"]
targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]
```

Build through mise and mbx from the checkout. Do not call `rustup` or set `RUSTC_WRAPPER` to skip mbx:

```sh
export MISE_TRUSTED_CONFIG_PATHS=$PWD
mise install
type -a cargo | head -1   # must show a path under mise/command-wrappers/bin/
mise exec -- cargo build
```

### Tracked files

The generator reads `.velnor/rust-ci.toml`, `.velnor/release.toml` and `.velnor/workflows.toml` only through Git's tracked view. A file that exists on disk but is untracked or ignored is refused, not replaced by defaults. Run `git add <path>` on each file (`git add -f` for an ignored one) before you run `generate`.

## Steps

1. Write `.velnor/rust-ci.toml`. A minimal file for a GitHub-hosted runner:

   ```toml
   runner = "ubuntu-24.04"
   hosted = true
   ```

   A label spelled like a GitHub-hosted runner must declare `hosted`. A custom self-hosted label is fenced by default. The full key list is the Settings table in [`rust-ci.md`](rust-ci.md).

2. Move the old exclusions. The old `[discovery] exclude` in `.velnor/config.toml` becomes a top-level `exclude` in `.velnor/rust-ci.toml`:

   ```toml
   # old: [discovery] / exclude = ["samples/*"]
   exclude = ["samples/*"]
   ```

   Each glob must match a tracked `Cargo.toml`, or the config is refused. A trailing `/` is removed.

3. Handle tracked symlinks. A symlink that should select no package goes in `non_inputs`. A glob there may not overlap an `inputs` glob.

   ```toml
   non_inputs = ["vendor-link/**"]
   ```

4. Stage the config and pin the helper. `pin-helper` writes `.velnor/rust-ci.lock` in the `--root` directory and stages it only when that directory is the repository root. Use the newest `velnor-ci` release:

   ```sh
   git add .velnor
   velnor-gen --root . pin-helper <version>
   ```

5. Generate the workflow:

   ```sh
   velnor-gen --root . generate
   ```

   This writes `.github/workflows/rust-ci.yml` and `.velnor/rust-metadata.json`. Stage both.

6. Check that the output is current. `check` exits 1 when either file is out of date:

   ```sh
   velnor-gen --root . check
   ```

7. Remove the old engine in the same pull request. Delete the workflow files that call velnor-actions, and delete `.velnor/config.toml`. Use `git rm` so the removal is staged:

   ```sh
   git rm .github/workflows/<old-workflow>.yml .velnor/config.toml
   ```

   The generated `Required` job is the default `required_check`. If a branch ruleset requires a check with that name, the ruleset keeps matching after the old workflow is gone. Otherwise update the ruleset in the same change.

## Known gaps

Each gap below has a tracking issue in `velnor-new`, except the last one.

| Gap | Tracking | Effect |
|---|---|---|
| Private release-asset fixtures (`[[release_assets]]`, scoped token, sha256 check, digest cache) | #2039 | A test cannot download a private release asset from a declared config. |
| Hand-written image workflows, and the `external_workflows` escape for `velnor-gen check` | #2099 | `check` has no documented way to accept a hand-written workflow beside the generated one. |
| App tokens in declared workflows | #2107 | `create-github-app-token` cannot receive its private key from a declared workflow. |
| Test group partitions and failure artifacts | #2121 | A test group runs as one job, with no partitions and no artifacts on failure. |
| Per-lane timeouts | #2122 | `timeout_minutes` is one value per job, not per lane. |
| Public git dependencies and caching | none yet | A cold build of a git dependency can be slower than a crates.io build. |
| Caching with private dependencies | none yet | Confirm the cache hits on the first run; see the verification section below. |

## Workarounds

- **Fixtures from private release assets (#2039).** Commit a copy of each fixture into the repository and check its sha256 in a `[[test_setup]]` step. Pin the expected digest in the same file, so a changed copy fails the run.
- **Image workflows (#2099, #2107).** Keep every workflow that builds or publishes an image hand-written under `.github/workflows/`. Do not declare it in `workflows.toml`.
- **Attestation for a private repository.** `attest = true` uses GitHub artifact attestations, which a private repository may not have on its plan. For an image, build with BuildKit's own provenance (`provenance: mode=max` in `docker/build-push-action`) in the hand-written image workflow instead.
- **Rejected unsigned commits.** If a branch ruleset refuses commits that GitHub does not mark verified, push commits that GitHub verifies (for example, commits created through the GitHub API or signed with a key GitHub knows). Do not turn off the ruleset to land the change.

## Verification

A migration is done when all of the following hold:

- `velnor-gen --root . check` exits 0 on the branch.
- The `Required` check is green on the pull request. A red `Required` means a child job failed.
- Every compile step prints `mbx[cache]: object cache: H hits, M misses` lines. If those lines are missing, the build did not go through mbx; go back to the `type -a cargo` check.
- On the second run of an unchanged branch, the compile steps show hits. If a private-dependency build shows only misses, the cache key is not reused, and that is the gap in the table above.
- No old-engine workflow and no `.velnor/config.toml` remain in the tree.
