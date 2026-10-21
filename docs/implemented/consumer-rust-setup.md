# Consumer Rust setup (Mise, MBX, profiles)

- State: implemented (this pull request)
- Specification: [task execution contract](../proposed/task-execution-contract.md)
  (durable `mr_boxington` evidence), [cache contract](../proposed/cache-contract.md)
  (MBX bundle family), [hard decision 4](../hard-decisions.md#4-tool-versions-come-from-the-projects-mise-config)
- Delivered: the owner's standard consumer layout below; a test that proves
  it selects MBX in CI (also with private sources); the opt-in
  `[stacks.rust] toolchain_source = "mise"` agreement check; velnor-new on
  the same layout.
- Acceptance evidence: `impl_consumer_mbx_setup` (orchestrator integration
  tests), `tool_catalog_resolve::rust_mirror::tests`,
  `tool_catalog_resolve::rust_source::tests`.
- Deviations: none
- Follow-up: Rust 1.100 is expected about 2026-11-12 with a new Cargo build directory layout.
  This layout lets MBX collect unused units separately and prepare new worktrees from existing ones.
  Update the catalog after the release.

## 1. `mise.toml` and `rust-toolchain.toml` (owner standard layout)

Use the owner's standard layout of 2026-10-22 for each Rust project.
Select the latest stable tool versions.

`mise.toml`:

```toml
min_version = "2026.10.17"

[tools]
rust = { version = "1.99.1", profile = "minimal", components = ["clippy", "rustfmt"], targets = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"], mr_boxington = true }
mr-boxington = "1.26.0"
hk = "2.8.0"
cargo-binstall = "1.26.0"

[settings]
cargo.binstall = true
idiomatic_version_file_enable_tools = ["rust"]
lockfile = true

[wrappers.cargo]
command = "mbx"
env = { MBX_CARGO_SHIM_MODE = "1" }
```

Keep `rust-toolchain.toml` values equal to the Mise Rust entry.
Update both files together:

```toml
[toolchain]
channel = "1.99.1"
profile = "minimal"
components = ["clippy", "rustfmt"]
targets = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
```

- Keep both files. Mise uses its Rust entry when `mise.toml` declares Rust.
  Tools outside Mise can use `rust-toolchain.toml` instead.
  Include components and targets in both declarations.
  Missing targets can cause cross builds under Mise to fail with `E0463`.
- Generation checks the toolchain mirror when both files declare Rust.
  The version, profile, component set, and target set must agree.
  A difference reports
  `rust_toolchain_mirror_mismatch:<key>:rust-toolchain.toml=[...],mise.toml=[...]`.
  Set `profile` in both files or neither file. The `minimal` profile omits rust-docs.
- `idiomatic_version_file_enable_tools = ["rust"]` has no effect while
  `mise.toml` declares Rust. The standard includes it as a fallback.
- `mr_boxington = true` and `[wrappers.cargo]` both route `cargo` through MBX.
  Include `mr-boxington` in the same active `[tools]` configuration.
- Install hk hooks with `hk install --mise` or `HK_MISE=1`.
  Cargo hook steps then use the Mise environment, including from graphical Git clients.
- Dockerfiles get the version from Mise.
  Pass `--build-arg RUST_VERSION="$(mise current rust)"`.
  In the base layer, run `rustup component add clippy rustfmt`.
  Run `rustup target add` with the toolchain file's targets.
  This avoids downloads after `COPY . .`.
- Add other project tools to `[tools]` with exact versions.
  For example, add `"aqua:nextest-rs/nextest/cargo-nextest"`.
- With `lockfile = true`, run `mise lock` and commit `mise.lock`.
  Packslip entries record a SHA-256 digest and a Sigstore signer for `mr-boxington`.
- `min_version` requires Mise 2026.10.17 or later on every computer.
  An older global Mise rejects project commands, including the `cargo` wrapper.
  Use `mise self-update` to update Mise.
- Only rust-analyzer needs `mbx setup`.
  Select "Create the shim without activating it".
  Do not pass `--yes`, `--global`, or `--local`.

### Optional: name `mise.toml` the authority

The toolchain mirror check always runs during generation.
The standard layout does not enable `[stacks.rust] toolchain_source = "mise"`.
If you enable this option in `.velnor/config.toml`, a Mise config must declare Rust.
A missing declaration reports `rust_not_declared_in_mise`.
Other Rust version declarations must agree with the Mise version.
This includes nested toolchain files, `.tool-versions`, and `.velnor/config.toml` pins.
A difference reports `rust_version_disagrees_with_mise:<file>=<v>,mise.toml=<v>`.

## 2. What CI does with it

The Mise option selects MBX without an additional `compile_driver` declaration in `.velnor/config.toml`.
The `mr_boxington = true` setting supplies durable evidence for `compile_driver = mbx`. Generation selects:

- the MBX compile driver in every crate job and in the dependency-seed job;
- the seed-only bundle writer: only the `Dependency seed` job runs
  `Record MBX producer success`, `Export MBX bundle` and `Save MBX bundle`,
  on trusted pushes; crate jobs restore and import read-only;
- MBX bundle transport with `mbx cache export --group velnor --format directory` and `mbx cache import`.
  It does not cache Cargo `target/` files. The MBX action only installs MBX and has no `github-cache-mode`.

`impl_consumer_mbx_setup` generates the layout above and asserts all three.
Add `[stacks.rust] compile_driver = "mbx"` only if generation reports
`transient_evidence_requires_declaration` (MBX seen only in a transient
place such as a hand-written workflow).

Generated Plan, Rust crate, and dependency seed jobs check installed Rust manifest metadata against the catalog channel manifest digest.
See [verified Rust installs](rust-channel-manifest.md). Select a Rust release that the Velnor catalog contains.

Velnor never injects `RUSTFLAGS` or `CARGO_PROFILE_*`: CI builds with the
repository's own profile, so developer and CI cache keys match.

## 3. Cargo profiles

```toml
[profile.dev]
debug = "line-tables-only"   # backtraces keep file:line
[profile.dev.package."*"]
debug = false                # no debuginfo for dependencies
# Do not override [profile.test]. It inherits dev.
# Thus, build, test, and clippy can share dependency units.
```

Measurements on three ChainArgos workspaces started with an empty target and ran build, tests, and Clippy in that order.
The profile used 26-43% less space than Cargo defaults with a `debug = 0` test profile.
It used 49-60% less space than the old Mise tasks.
A `[profile.test]` that differs from `dev` keeps separate dependency outputs.
Use `cargo build --profile debug` with Cargo 1.99 or later for debugger sessions.
Keep the platform default for `split-debuginfo` and do not set `incremental`.
MBX forces `CARGO_INCREMENTAL=0` and keeps its own learned incremental state. Cargo disables incremental compilation when `CI` is set.

## 4. Tasks

Do not set compiler flags in Mise tasks:

```toml
[tasks.test]
run = "cargo nextest run --locked --workspace"   # not: CARGO_INCREMENTAL=0 RUSTFLAGS='-C debuginfo=0' cargo ...
```

`RUSTFLAGS` changes artifact hashes.
On processor-monitor, the old `RUSTFLAGS='-C debuginfo=0' CARGO_INCREMENTAL=0` tasks added a third artifact set beside plain `cargo` builds.
This added 1.33 GiB (+48%) and separate MBX cache entries.
Use one flag set per repository with the profile above.

Do not set `CARGO_TARGET_DIR`, `build.target-dir`, `build.build-dir`, or `rustc-wrapper` in a repository.
MBX manages `target/` only when all four are absent.
If any is set, MBX leaves the directory outside its management and does not collect it.

## 5. Developer machines: MBX garbage collection

MBX shares compiled outputs across checkouts through its action store and automatically collects managed targets.
Set a storage budget that the machine can meet:

```sh
mbx settings unset gc.min_free_size            # default: 10% of the disk, 5-50 GiB
mbx settings set gc.max_total_size 60GiB       # one logical budget for store, targets, incremental
mbx settings set target.max_age 14d
mbx settings set target.evict_first .claude/worktrees,/private/tmp
mbx settings set scheduler.reserve_cpus 2      # keep the editor and browser responsive
mbx gc --dry-run && mbx doctor
```

- If `gc.min_free_size` exceeds the disk free space available after cleanup, MBX stays in low-disk mode.
  It collects every 5 minutes and can remove each managed target and shared object.
  Later builds can then require recompilation. Check `mbx gc --dry-run` after changing the setting.
- Use one Git worktree per concurrent agent or terminal.
  Do not start a second `build` or `test` in a checkout that already runs one.
  The Cargo build directory lock runs them one at a time.
  With MBX, `check` and `clippy` use a separate check lane and do not wait for that lock.
- Set `MBX_SCHEDULER_PRIORITY=low` for agents and background builds.
- Resolve dependencies once with `cargo fetch --locked`.
  Then build with `--locked --offline` to reduce time spent holding the `$CARGO_HOME` package lock.
- `mbx adopt --recursive <dir>` moves existing unmanaged `target/`
  directories under management; `mbx clean --under <dir>` drops records of
  deleted checkouts.
- Keep at most the current and previous stable toolchain:
  `mise uninstall rust@<old>`.
- Rely on Cargo's stabilized cache auto-GC for `~/.cargo`; never set
  `cache.auto-clean-frequency = "never"` on a developer machine (Velnor CI
  sets it inside jobs on purpose).

## 6. velnor-new itself

velnor-new uses the same layout. Its `mise.toml` includes `min_version`,
`rust = { version, mr_boxington = true }`, `mr-boxington`, `hk`,
`cargo-binstall`, Nextest, the three settings, and `[wrappers.cargo]`.
Its `rust-toolchain.toml` repeats the Rust version, components, and targets.
Its `.velnor/config.toml` keeps `[stacks.rust] compile_driver = "mbx"`.
The Mise evidence agrees with this declaration.
`scripts/toml-tool-pin.awk` reads plain and inline-table `[tools]` entries.
Thus, `verify-local.sh` and `local-cargo.sh` can report the local version.
The hk rollout supplies `hk.pkl` and its secret scanners. This page does not supply them.
