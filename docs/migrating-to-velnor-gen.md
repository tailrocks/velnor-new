# Migrating from velnor-actions to velnor-gen

This page is for a project that runs Rust CI on the old velnor-actions engine and moves it to `velnor-gen`. The new generator writes one workflow, `.github/workflows/rust-ci.yml`, from `.velnor/rust-ci.toml`. The settings are in [`rust-ci.md`](rust-ci.md) and [`generator/CONFIG.md`](../generator/CONFIG.md).

Most failed migrations failed before the generator ran: on the mise layout, on a file Git does not track, or on a dependency the generator cannot fetch. Check the prerequisites first.

## Prerequisites

### The mise and mbx layout

This is the recommended layout. Its `mise.toml` declares the Rust toolchain with `mr_boxington = true`, pins mbx and nextest, and routes every `cargo` through mbx with no `[wrappers.cargo]`. Generation accepts this native layout. The explicit layout, `[wrappers.cargo]` with `command = "mbx"` and `env = { MBX_CARGO_SHIM_MODE = "1" }` next to the same option and pin, is also accepted. Without the shim env the wrapper breaks `cargo -V` and `cargo --list`, so generation refuses it. velnor-ci [0.1.109](https://github.com/tailrocks/velnor-new/releases/tag/tools-v0.1.109) is released, and generation requires one of these two layouts (#2108).

```toml
min_version = "<mise version>"

[tools]
rust = { version = "<rust version>", profile = "minimal", components = ["clippy", "rustfmt"], targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"], mr_boxington = true }
mr-boxington = { version = "<mbx version>", workflow = "release.yml@refs/heads/main" }
"aqua:nextest-rs/nextest/cargo-nextest" = "<nextest version>"

[settings]
lockfile = true
```

Pin the newest release of each tool. The `mise.toml` and `rust-toolchain.toml` of the velnor-new repository hold the current pins. Generation refuses a project that runs its tests with nextest and does not pin `cargo-nextest` in `mise.toml`.

`rust-toolchain.toml` mirrors the `rust` entry. Generation compares only the channel of the two files, so a different channel fails. Keep `components` and `targets` equal by hand: a mismatch in them is not refused, and local and generated setup then differ.

```toml
[toolchain]
channel = "<rust version>"
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

### Committed lock files

Commit `Cargo.lock`. Generation refuses a checkout where it is not committed (`Cargo.lock is not committed`). Create `mise.lock` with `mise lock` and commit it too; generation refuses a missing one (`mise.lock is required`). Both must be tracked by Git, not only present on disk.

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

3. Handle tracked symlinks. A symlink that should select no package goes in `non_inputs` by its exact path, because a change to the link is reported under the link's own name. A glob there may not overlap an `inputs` glob. If the symlink also has descendants that should select nothing, list both forms:

   ```toml
   non_inputs = ["vendor-link", "vendor-link/**"]
   ```

4. Stage the config and pin the helper. `pin-helper` writes `.velnor/rust-ci.lock` in the `--root` directory and stages it only when that directory is the repository root. Use the newest `velnor-ci` release:

   ```sh
   git add .velnor
   velnor-gen --root . pin-helper <version>
   ```

5. Generate the workflows:

   ```sh
   velnor-gen --root . generate
   ```

   Stage every path that `generate` writes or changes, not only the workflow. Run `git status` and add each generated path. The list includes `.github/workflows/rust-ci.yml`, `.velnor/rust-metadata.json` and `.github/workflows/rust-cache-trim.yml`. The cache-trim workflow appears when the helper enables PR delta caching with default target directories and public dependencies. Release and declared-workflow settings write more files. A clean checkout without these files fails `check`.

6. Check that the output is current. `check` exits 1 when a generated file is out of date or missing:

   ```sh
   velnor-gen --root . check
   ```

7. Remove the old engine in the same pull request. Delete the workflow files that call velnor-actions, and delete `.velnor/config.toml`. Use `git rm` so the removal is staged:

   ```sh
   git rm .github/workflows/<old-workflow>.yml .velnor/config.toml
   ```

   The generated `Required` job is the default `required_check`. If a branch ruleset requires a check with that name, the ruleset keeps matching after the old workflow is gone. Otherwise update the ruleset in the same change.

## Known gaps

Checked against `origin/main`. Four gaps are still open:

| Gap | Tracking | Effect |
|---|---|---|
| App tokens in declared workflows | #2107 | `create-github-app-token` cannot receive its private key from a declared workflow. |
| Test group partitions and failure artifacts | #2121 | A test group runs as one job, with no partitions and no artifacts on failure. |
| Public git dependencies and caching | none yet | A cold build of a git dependency can be slower than a crates.io build. |
| Caching with private dependencies | #2140 | Confirm the cache hits on the first run; see the verification section below. |

Three gaps closed in [0.1.109](https://github.com/tailrocks/velnor-new/releases/tag/tools-v0.1.109): private release-asset fixtures (#2039, fixed by #2059), hand-written image workflows (#2099, fixed by #2111) and per-lane timeouts (#2122, fixed by #2143). From 0.1.109, use `[[release_assets]]`, `external_workflows` and `[root_timeouts]`, as lesson 6 below describes. Before 0.1.109, use the workarounds below.

**Image workflows (before 0.1.109).** Before 0.1.109, a repository whose image build is a hand-written workflow cannot both delete `.velnor/config.toml` (step 7) and pass `velnor-gen check` (step 6). From 0.1.109, list that workflow under `external_workflows` instead.

## Workarounds

Before 0.1.109:

- **Private release-asset fixtures.** Commit a copy of a fixture only when everyone who can read the repository and its history may read it, because a committed copy stays in Git history. Check its sha256 in a `[[test_setup]]` step, with the expected digest in the same file, so a changed copy fails the run. A confidential fixture has no workaround before 0.1.109; from 0.1.109, use `[[release_assets]]`.
- **Attestation for a private repository.** `attest = true` is a `[[binaries]]` key in `.velnor/release.toml`, and it uses GitHub artifact attestations, which a private repository may not have on its plan. For an image, build with BuildKit's own provenance (`provenance: mode=max` in `docker/build-push-action`) in a hand-written workflow instead.
- **Unsigned commits refused by a ruleset.** A commit that GitHub does not mark verified is refused by a signed-commit ruleset. Sign commits locally (`git commit -S`) or commit through GitHub's web interface, which signs them. A commit created through the REST API with a user token is unsigned unless the request supplies a signature. GitHub signs API commits only for some bot identities; see [GitHub's bot signature conditions](https://docs.github.com/en/authentication/managing-commit-signature-verification/about-commit-signature-verification#signature-verification-for-bots). Do not turn off the ruleset to land the change.

## Lessons from the 0.1.107 and 0.1.108 migrations

Each item names the release that ships the fix or the option. An item marked 0.1.109 ships in [0.1.109](https://github.com/tailrocks/velnor-new/releases/tag/tools-v0.1.109); pin the helper to that version only when you need its keys.

1. **Run `cargo fmt --all --check` before you push.** Before 0.1.108, a failed Format step skipped the private-source token and fetch steps, because their implicit `success()` condition was false. The compile steps still ran, offline, and failed with "you are in the offline mode" errors that hide the formatting diff. #2180 runs the phase under `!cancelled()` when a compile step does, so the fetch runs. It is in 0.1.108. Still run the check locally first.

2. **Pin `gh` in `mise.toml` for a hand-written workflow that calls it through mise.** Mise has no version of `gh` to run unless `mise.toml` pins one. The generated attestation job does not need this: it installs `gh@<version>` by name, so the rule covers only hand-written steps.

3. **Rust 1.100 changes the target directory layout.** Helper 0.1.108 reads that layout in the audit, prune and delta steps (#2186). An older helper finds a built target with no units it recognises. Set the helper to 0.1.108 or later before you move the toolchain to 1.100. In 0.1.109, a built target with no unit file in a known layout is refused (#2191) instead of read as empty.

4. **The `inputs build` directory cap is 1,000,000 by default (0.1.108, #2181).** The old cap was 4096 directories, which a full build and test of a multi-package workspace passes easily. `VELNOR_TARGET_MAX_DIRECTORIES` raises it up to 10,000,000. A value outside that range is refused.

5. **mbx is required from 0.1.109 (#2108).** Generation refuses a `mise.toml` without one of two layouts:
   - native: `mr_boxington = true` on the `rust` tool and no `[wrappers.cargo]`;
   - explicit: the same option, plus `[wrappers.cargo]` with `command = "mbx"` and `env = { MBX_CARGO_SHIM_MODE = "1" }`.

   It also refuses:
   - an mbx pin not spelled exactly `mr-boxington`, with an `os` filter, or with more than one version request;
   - a `[wrappers.cargo]` with `command = "mbx"` and no shim env, because it breaks `cargo -V`;
   - mbx settings split between `mise.toml` and `.mise.toml`;
   - a second tool whose key ends in `mr-boxington`.

   To run cargo without mbx, set `[cache] mbx = false` (helper 0.1.109). That setting is refused with `mbx_shared_store = true` or `[cache] mbx_remote`.

6. **From 0.1.109, use the keys that close the known gaps.** Each one replaces a workaround in Known gaps. Before 0.1.109, use the workaround instead.
   - Hand-written workflows (#2111): list each one in `.velnor/rust-ci.toml`. Without the key, `velnor-gen check` reports every workflow it does not generate as unowned.

     ```toml
     external_workflows = [".github/workflows/image.yml"]
     ```

     The listed file is skipped by `check` and `generate`, so review it on its own. The retired `.velnor/config.toml` bypass is deprecated.
   - Private release assets (#2059): a `[[release_assets]]` table names the file, its tag and its sha256. The asset is fetched with a scoped token and checked against that sha256.
   - Per-root and per-lane time limits (#2143): a `[root_timeouts]` table, keyed by root folder (`.` for the repository root), sets one limit for a heavy root without raising every job.

7. **Run a nextest setup script with `bash`.** In a `[[test_setup]]` command, write `bash <script>`. A script committed without the executable bit then still runs. Before 0.1.107, nextest's `SETUP` lines could drop the per-package test counts. 0.1.107 ignores them (#2169).

8. **Use `[[test_setup]] phase = "build"` for a fixture that a build script or clippy reads (0.1.109, #2155).** The default phase runs the step before the tests only, so clippy and `build.rs` do not see the fixture. With `phase = "build"` the step runs before the first compiling cargo step of each job.

9. **Caching with private dependencies is open (#2140), not on main.** Until it ships, confirm the cache hit on the first run, as in the verification section below.

10. **Do not set `mbx_shared_store` or `[cache] mbx_remote` until you have a trusted runner pool.** Both share compiler results between runs. `mbx_shared_store` reads a per-node store on default-branch events. `mbx_remote` saves after a pull request from the same repository. Keep the default private store per job until the runners only accept writes from trusted events.

## Verification

A migration is done when all of the following hold:

- `velnor-gen --root . check` exits 0 on the branch.
- The `Required` check is green on the pull request. A red `Required` means a child job failed.
- Every compile step prints `mbx[cache]: object cache: H hits, M misses` lines. If those lines are missing, the build did not go through mbx; go back to the `type -a cargo` check.
- On the second run of an unchanged branch, the compile steps show hits. If a private-dependency build shows only misses, the cache key is not reused, and that is the gap in the table above.
- No old-engine workflow and no `.velnor/config.toml` remain in the tree.
