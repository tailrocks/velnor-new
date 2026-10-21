# Consumer Rust setup (Mise, MBX, profiles)

- State: implemented (this pull request)
- Specification: [task execution contract](../proposed/task-execution-contract.md)
  (durable `mr_boxington` evidence), [cache contract](../proposed/cache-contract.md)
  (MBX bundle family), [hard decision 4](../hard-decisions.md#4-tool-versions-come-from-the-projects-mise-config)
- Delivered: the recommended consumer layout below; a test that proves the
  Mise option alone selects MBX in CI; the opt-in
  `[stacks.rust] toolchain_source = "mise"` check; velnor-new's own
  `mise.toml` on `mr_boxington = true`.
- Acceptance evidence: `impl_consumer_mbx_setup` (orchestrator integration
  tests), `tool_catalog_rust_source::tests`, `config_stacks::tests::rust_toolchain_source_loads_only_the_mise_value`.
- Deviations: none
- Follow-up: Rust 1.100 (about 2026-11-12) brings Cargo's new build-dir
  layout, which lets MBX collect unused units one at a time and seed new
  worktrees from existing ones; bump the catalog when it ships.

## 1. `mise.toml`

`mise.toml` is the only source of the Rust version. Put the version, profile,
components and any cross targets in the `rust` entry, and pin MBX exactly:

```toml
[settings]
# Owner rule: always the latest stable, no cooldown. Exact pins are exempt from
# Mise's default 24 h minimum_release_age anyway; this also covers `latest`
# resolution in `mise upgrade --bump`.
minimum_release_age = "0s"

[tools]
rust = { version = "1.99.1", profile = "minimal", components = ["clippy", "rustfmt"], mr_boxington = true }
mr-boxington = "1.26.0"   # exact pin; mise.lock records it (packslip, sha256 + sigstore signer)
"aqua:nextest-rs/nextest/cargo-nextest" = "0.9.149"
```

- `mr_boxington = true` makes Mise (2026.9.2 or newer) run every `cargo`
  through MBX. It needs `mr-boxington` in the same active `[tools]`. Do not
  add `[wrappers.cargo]`: an explicit wrapper takes precedence over the option.
- Add `targets = ["aarch64-unknown-linux-gnu", ...]` to the `rust` entry for
  cross targets instead of a `rust-toolchain.toml`.
- Delete `rust-toolchain.toml`/`rust-toolchain` and
  `[settings] idiomatic_version_file_enable_tools = ["rust"]`. Inside Mise,
  `RUSTUP_TOOLCHAIN` already outranks the file, so it adds nothing but a
  second place to edit. Repoint anything that read it (Dockerfiles, docs) to
  `mise current rust`.
- Run `mise lock --platform linux-x64,linux-arm64,macos-arm64` and commit
  `mise.lock`.
- `mbx setup` is only needed for rust-analyzer. Choose "Create the shim
  without activating it"; do not pass `--yes`, `--global` or `--local`.

### Enforce the single source

Opt in from `.velnor/config.toml`:

```toml
[stacks.rust]
toolchain_source = "mise"
```

Generation then fails with `rust_version_source_not_mise`, naming both
files, when any of these exists next to the Mise `rust` entry, even if the
versions agree:

- `rust-toolchain.toml` or `rust-toolchain` at the root or in any
  subdirectory;
- a `rust` line in `.tool-versions`;
- `[stacks.rust] toolchain` or `[tools] rust` in `.velnor/config.toml`.

It fails with `rust_not_declared_in_mise` when no Mise config declares Rust.
Without the option, hard decision 4 is unchanged: every source is accepted
as long as they agree on one exact version.

## 2. What CI does with it

Nothing needs to be declared in `.velnor/config.toml`. `mr_boxington = true`
in `mise.toml` is durable evidence for `compile_driver = mbx`, so generation
selects:

- the MBX compile driver in every crate job and in the dependency-seed job;
- the seed-only bundle writer: only the `Dependency seed` job runs
  `Record MBX producer success`, `Export MBX bundle` and `Save MBX bundle`,
  on trusted pushes; crate jobs restore and import read-only;
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
- Resolve dependencies once (`cargo fetch --locked`), then build with
  `--locked --offline`, so concurrent builds hold the `$CARGO_HOME` package
  lock only briefly.
- `mbx adopt --recursive <dir>` moves existing unmanaged `target/`
  directories under management; `mbx clean --under <dir>` drops records of
  deleted checkouts.
- Keep at most the current and previous stable toolchain:
  `mise uninstall rust@<old>`.
- Rely on Cargo's stabilized cache auto-GC for `~/.cargo`; never set
  `cache.auto-clean-frequency = "never"` on a developer machine (Velnor CI
  sets it inside jobs on purpose).

## 6. velnor-new itself

velnor-new uses the same option: its `mise.toml` declares
`rust = { version = "...", mr_boxington = true }` and `mr-boxington`, with no
`[wrappers.cargo]`. Its `.velnor/config.toml` keeps
`[stacks.rust] compile_driver = "mbx"` as a sticky declaration that the Mise
evidence now agrees with. `scripts/toml-tool-pin.awk` reads both the plain
and the inline-table form of a `[tools]` entry, so `verify-local.sh` and
`local-cargo.sh` still report the local pin.
