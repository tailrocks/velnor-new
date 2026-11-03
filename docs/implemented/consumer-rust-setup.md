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
- Follow-up: Rust 1.100 (about 2026-11-12) brings Cargo's new build-dir
  layout, which lets MBX collect unused units one at a time and seed new
  worktrees from existing ones; bump the catalog when it ships.

## 1. `mise.toml` and `rust-toolchain.toml` (owner standard layout)

The owner's standard layout (2026-10-22) for every Rust project. Use the
newest stable version of each tool when applying it.

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

`rust-toolchain.toml`, an exact mirror of the `mise.toml` rust entry (never
edited alone):

```toml
[toolchain]
channel = "1.99.1"
profile = "minimal"
components = ["clippy", "rustfmt"]
targets = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
```

- Why both files: once `mise.toml` pins Rust, Mise ignores the toolchain
  file completely, including its `components` and `targets`. rustup-only
  consumers (editors and rust-analyzer, hooks run outside Mise, GUI git,
  Docker `rust:` images, a plain shell) read only the file and otherwise fall
  back to rustup's default toolchain. So the Mise entry must carry the
  components and targets too (or cross builds under Mise fail with E0463 and
  `mise.lock` records no options), and the file must repeat them.
- Generation enforces the mirror: when both declare Rust, a different
  `channel`/`version`, `components`, `targets` or `profile` fails with
  `rust_toolchain_mirror_mismatch:<key>:<toolchain file>=[...],<mise file>=[...]`.
  Set `profile` in both or in neither; `minimal` skips rust-docs.
  Generated CI always installs the minimal profile plus `clippy` and `rustfmt` and no extra targets.
  - The Mise side is every config Mise reads (`mise.toml`, `.mise.toml`,
    `mise.<env>.toml`, `.config/mise.toml`, `.config/mise/config.toml`,
    `.config/mise/conf.d/*.toml`, `mise/config.toml`, `.mise/config.toml`
    and their `.<env>` forms), each `[tools] rust` entry on its own.
  - The toolchain side is every `rust-toolchain.toml` and legacy
    `rust-toolchain`, at the root or nested (every file the upgrade job
    rewrites). A legacy plain `rust-toolchain` names only a channel, so it
    mirrors only an entry without components, targets or profile; use
    `rust-toolchain.toml` otherwise.
  - `components` and `targets` may be arrays or Mise's comma-separated
    string (`"clippy,rustfmt"`); order and duplicates do not matter.
- `idiomatic_version_file_enable_tools = ["rust"]` is a no-op while
  `mise.toml` pins Rust; it is part of the standard as a fallback.
- `mr_boxington = true` and the `[wrappers.cargo]` entry both route every
  `cargo` through MBX. They need `mr-boxington` in the same active `[tools]`.
- Install hk hooks with `hk install --mise` (or `HK_MISE=1`), so cargo-based
  hook steps run inside the Mise environment even from GUI git clients.
- The `cargo_fmt`, `cargo_check`, and `cargo_clippy` HK steps use
  `scripts/hk-workspace-admission`, which requires an operator-provided
  `rust-build-guard` executable on the hook's `PATH`. The project Mise config
  does not install it; admission exits with status 69 if it is missing or not
  a regular executable.
- Dockerfiles take the version from Mise, not from the toolchain file:
  pass `--build-arg RUST_VERSION="$(mise current rust)"`, and in the base
  layer run `rustup component add clippy rustfmt` and `rustup target add`
  with the file's targets, so builds do not download them after `COPY . .`.
- Add other tools the project uses (for example
  `"aqua:nextest-rs/nextest/cargo-nextest"`) to `[tools]` with exact pins.
- `lockfile = true`: run `mise lock` and commit `mise.lock` (packslip entries
  record a sha256 and a sigstore signer for `mr-boxington`).
- `min_version` requires Mise 2026.10.17 or newer on every machine. An older
  global Mise fails every command in the project, including the `cargo`
  command wrapper (`mise self-update`).
- `mbx setup` is only needed for rust-analyzer. Choose "Create the shim
  without activating it"; do not pass `--yes`, `--global` or `--local`.

### How `mr_boxington = true` and MBX 1.29.1 behave

Checked 2026-11-02 against mise 2026.10.25 and MBX 1.29.1. velnor-new pins
MBX 1.29.1, so the 1.29.1 behavior below applies to it.

- At mise 2026.10.25, `mr_boxington = true` on the `rust` tool adds a
  `[wrappers.cargo]` entry with `command = "mbx"` and the environment
  variable `MBX_CARGO_SHIM_MODE=1`. It needs `mr-boxington` in `[tools]` and
  mise 2026.9.2 or newer. Sources: `add_rust_wrapper` in
  [`src/config/command_wrapper.rs`](https://github.com/jdx/mise/blob/v2026.10.25/src/config/command_wrapper.rs)
  and [`docs/lang/rust.md`](https://github.com/jdx/mise/blob/v2026.10.25/docs/lang/rust.md)
  in jdx/mise v2026.10.25. The 2026.9.2 requirement comes from
  [`docs/setup.md`](https://github.com/jdx/mr-boxington/blob/v1.29.1/docs/setup.md)
  in jdx/mr-boxington v1.29.1 (lines 7, 13 and 26).
- An explicit `[wrappers.cargo]` entry takes precedence over the option, so
  the standard layout above does not wrap `cargo` twice. Sources: the same
  `command_wrapper.rs` and
  [`docs/setup.md`](https://github.com/jdx/mr-boxington/blob/v1.29.1/docs/setup.md)
  in jdx/mr-boxington v1.29.1.
- `MBX_CARGO_SHIM_MODE` selects what `mbx` does. `1` makes it act as the
  cargo shim, `0` runs the normal `mbx` commands, and unset decides from the
  stem of `argv[0]`. The shim removes the variable before it launches cargo.
  The shim comment states that mise removes its wrapper directory before it
  launches `mbx`, so `mbx` has no shim path to exclude. Source:
  [`crates/mbx/src/cli/shim.rs`](https://github.com/jdx/mr-boxington/blob/v1.29.1/crates/mbx/src/cli/shim.rs)
  in v1.29.1.
- MBX 1.29.1 contains
  [PR #873](https://github.com/jdx/mr-boxington/pull/873). After
  `mbx cache import` restores workspace state, a grouped export keeps the
  actions that Cargo reports as fresh. MBX releases before 1.29.1, including
  1.28.0 (the former velnor-new pin), can export "0 actions and 0 objects"
  in that case, as the before output in PR #873 shows. A grouped export needs
  the group at both phases. Set `MBX_CACHE_EXPORT_GROUP=G` while the build
  runs, so MBX records the actions under `G`. Then run
  `mbx cache export --group G` at export time to select them. Without the
  build-time variable, the export has nothing to select. In a direct MBX-action
  integration, `--format directory` avoids archiving twice when `actions/cache`
  also compresses the result. Velnor's generated flow sets
  `MBX_CACHE_EXPORT_GROUP=velnor` for the build, exports with
  `--group "$MBX_CACHE_EXPORT_GROUP" --format directory`, then the
  `Publish state snapshot` step repacks that directory with GNU
  `tar | zstd -3 --long=30` as `payload.tar.zst`. `Save state snapshot` passes
  that file to `actions/cache`, so Velnor keeps its own inner zstd envelope
  under the cache archive.
  Sources: PR #873,
  [`crates/mbx/CHANGELOG.md`](https://github.com/jdx/mr-boxington/blob/v1.29.1/crates/mbx/CHANGELOG.md),
  [`docs/cli/cache/export.md`](https://github.com/jdx/mr-boxington/blob/v1.29.1/docs/cli/cache/export.md)
  and [`docs/github-action.md`](https://github.com/jdx/mr-boxington/blob/v1.29.1/docs/github-action.md).
- MBX sets `CARGO_INCREMENTAL=0` by default. `MBX_INCREMENTAL=1` opts in to
  incremental compilation. Source:
  [`docs/limits.md`](https://github.com/jdx/mr-boxington/blob/v1.29.1/docs/limits.md).
- `MBX_STATS_REPORT` writes a JSON report (version 5) with lookups, hits,
  misses, bypasses, compiler outcomes and downloaded bytes. It does not split
  results into registry and workspace units, so the report alone cannot show
  a registry hit rate. Source:
  [`crates/mbx/src/session/stats.rs`](https://github.com/jdx/mr-boxington/blob/v1.29.1/crates/mbx/src/session/stats.rs).

### Optional: name `mise.toml` the authority

The mirror check above is always on. `[stacks.rust] toolchain_source = "mise"`
in `.velnor/config.toml` additionally requires a `rust` entry in a Mise config
(`rust_not_declared_in_mise`) and names it as the authority for every other
declaration: a nested toolchain file, `.tool-versions` or a
`.velnor/config.toml` pin that differs fails with
`rust_version_disagrees_with_mise:<file>=<v>,mise.toml=<v>`. It is not part
of the standard layout and is not enabled in any repository.

## 2. What CI does with it

Nothing needs to be declared in `.velnor/config.toml`. `mr_boxington = true`
in `mise.toml` is durable evidence for `compile_driver = mbx`, so generation
selects:

- the MBX compile driver in every crate job, and in the dependency-seed job
  when `[stacks.rust] dependency_seed` is not `"off"`;
- the seed-only bundle writer: when the seed job is generated, only that job
  runs `Record MBX producer success`, `Export MBX bundle` and
  `Save MBX bundle`, on trusted pushes; crate jobs restore and import
  read-only. velnor-new sets `dependency_seed = "off"`
  (`.velnor/config.toml:40`, PR #466), so no job writes compiled state there
  and every crate-job restore misses; see
  [compiled-state persistence](../proposed/compiled-state-persistence.md);
- the opaque bundle transport: `mbx cache export --group velnor --format
  directory` and `mbx cache import`, never a Cargo `target/` cache (the MBX
  action is install-only, no `github-cache-mode`).

`impl_consumer_mbx_setup` generates the layout above and asserts all three.
Add `[stacks.rust] compile_driver = "mbx"` only if generation reports
`transient_evidence_requires_declaration` (MBX seen only in a transient
place such as a hand-written workflow).

Every job that installs Rust also verifies it against the catalog's
channel-manifest digest ([verified Rust installs](rust-channel-manifest.md)),
so pin a Rust release the Velnor catalog knows.

Velnor never injects `RUSTFLAGS` or `CARGO_PROFILE_*`: CI builds with the
repository's own profile, so developer and CI cache keys match.

## 3. Cargo profiles

```toml
[profile.dev]
debug = "line-tables-only"   # backtraces keep file:line
[profile.dev.package."*"]
debug = false                # no debuginfo for dependencies
# No [profile.test] override: test inherits dev, so build, test and clippy
# share every dependency unit.
```

Measured on three ChainArgos workspaces (build, then tests, then clippy, from
an empty target): 26-43% smaller than Cargo's defaults with a `debug = 0`
test profile, and 49-60% smaller than what the old mise tasks produced. A
`[profile.test]` that differs from `dev` keeps two copies of every
dependency. Use `cargo build --profile debug` (Cargo 1.99+) for debugger
sessions. Leave `split-debuginfo` at the platform default and do not set
`incremental`: MBX forces `CARGO_INCREMENTAL=0` and keeps its own learned
incremental state, and Cargo turns incremental off when `CI` is set.

## 4. Tasks

No inline flags in Mise tasks:

```toml
[tasks.test]
run = "cargo nextest run --locked --workspace"   # not: CARGO_INCREMENTAL=0 RUSTFLAGS='-C debuginfo=0' cargo ...
```

`RUSTFLAGS` changes every artifact hash. On processor-monitor the old
`RUSTFLAGS='-C debuginfo=0' CARGO_INCREMENTAL=0` tasks added a third artifact
set next to plain `cargo` builds (+1.33 GiB, +48%) and separate MBX cache
entries. Keep one flag set per repository: the profile above.

Never set `CARGO_TARGET_DIR`, `build.target-dir`, `build.build-dir` or
`rustc-wrapper` in a repository. MBX manages `target/` only when none of them
is set; with any of them it leaves the directory alone and never collects it.

## 5. Developer machines: MBX garbage collection

MBX shares compiled outputs across every checkout through its action store
and collects managed targets automatically. Give it a budget it can meet:

```sh
mbx settings unset gc.min_free_size            # default: 10% of the disk, 5-50 GiB
mbx settings set gc.max_total_size 60GiB       # one logical budget for store, targets, incremental
mbx settings set target.max_age 14d
mbx settings set target.evict_first .claude/worktrees,/private/tmp
mbx settings set scheduler.reserve_cpus 2      # keep the editor and browser responsive
mbx gc --dry-run && mbx doctor
```

- A `gc.min_free_size` larger than the free space the disk can reach keeps
  MBX in low-disk mode: it collects every 5 minutes and evicts every managed
  target and shared object, so every build is cold. Check
  `mbx gc --dry-run` after changing it.
- Use one git worktree per concurrent agent or terminal, and never start a
  second `build`/`test` in a checkout that already runs one: Cargo's own
  build-directory lock serializes them. `check` and `clippy` use MBX's
  separate check lane and do not wait.
- Agents and background builds: `MBX_SCHEDULER_PRIORITY=low`.
- Resolve dependencies once, then build with `--locked --offline`, so
  concurrent builds hold the `$CARGO_HOME` package lock only briefly. Use the
  form that `velnor-actions generate` prints for `cold_registry`:
  `cd '<workspace root>' && cargo fetch --locked --manifest-path '<absolute path to Cargo.toml>'`.
  If a `.cargo/config` or `.cargo/config.toml` file, or a `rust-toolchain`
  file with a `path` key, exists from the workspace root up to the repo root,
  `generate` prints no command. It names those files instead. Review them,
  because Cargo and rustup load them by working directory. Then run
  `cargo fetch --locked` for that manifest. If you export `MISE_CARGO_HOME`, run it with that variable
  set, because it warms that home and not `~/.cargo`.
- `mbx adopt --recursive <dir>` moves existing unmanaged `target/`
  directories under management; `mbx clean --under <dir>` drops records of
  deleted checkouts.
- Keep at most the current and previous stable toolchain:
  `mise uninstall rust@<old>`.
- Rely on Cargo's stabilized cache auto-GC for `~/.cargo`; never set
  `cache.auto-clean-frequency = "never"` on a developer machine (Velnor CI
  sets it inside jobs on purpose).

## 6. velnor-new itself

velnor-new uses the same layout: `mise.toml` with `min_version`,
`rust = { version, mr_boxington = true }`, `mr-boxington`, `hk`,
`cargo-binstall`, Nextest, the three settings and `[wrappers.cargo]`, plus a
`rust-toolchain.toml` that mirrors the rust entry (channel, components,
targets). Its `.velnor/config.toml`
keeps `[stacks.rust] compile_driver = "mbx"` as a sticky declaration that the
Mise evidence agrees with. `scripts/toml-tool-pin.awk` reads both the plain
and the inline-table form of a `[tools]` entry, so `verify-local.sh` and
`local-cargo.sh` still report the local pin. The hk configuration (`hk.pkl`
and its secret scanners) comes with the hk rollout, not this page.
