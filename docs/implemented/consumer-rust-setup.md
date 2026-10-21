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
  tests), `tool_catalog_resolve::rust_source::tests`.
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
rust = { version = "1.99.1", mr_boxington = true }
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

`rust-toolchain.toml`, whose `channel` must equal the `mise.toml` rust version:

```toml
[toolchain]
channel = "1.99.1"
components = ["clippy", "rustfmt"]
targets = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
```

- `mr_boxington = true` and the `[wrappers.cargo]` entry both route every
  `cargo` through MBX. They need `mr-boxington` in the same active `[tools]`.
- Add other tools the project uses (for example
  `"aqua:nextest-rs/nextest/cargo-nextest"`) to `[tools]` with exact pins.
- `lockfile = true`: run `mise lock` and commit `mise.lock` (packslip entries
  record a sha256 and a sigstore signer for `mr-boxington`).
- `min_version` requires Mise 2026.10.17 or newer on every machine. An older
  global Mise fails every command in the project, including the `cargo`
  command wrapper (`mise self-update`).
- `mbx setup` is only needed for rust-analyzer. Choose "Create the shim
  without activating it"; do not pass `--yes`, `--global` or `--local`.

### Enforce that the versions agree

Hard decision 4 already fails generation when two sources declare different
versions (`conflicting_project_tool_versions`). To make `mise.toml` the named
authority, opt in from `.velnor/config.toml`:

```toml
[stacks.rust]
toolchain_source = "mise"
```

Generation then requires a `rust` entry in a Mise config
(`rust_not_declared_in_mise` otherwise), and fails with
`rust_version_disagrees_with_mise:<file>=<v>,mise.toml=<v>` when any other
declaration differs from it:

- `rust-toolchain.toml` or `rust-toolchain` at the root or in any
  subdirectory;
- a `rust` line in `.tool-versions`;
- `[stacks.rust] toolchain` or `[tools] rust` in `.velnor/config.toml`.

An agreeing `rust-toolchain.toml` is accepted, which is the standard layout.
A toolchain file without a `channel` declares no version and is accepted.

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

velnor-new uses the same layout: `mise.toml` with `min_version`,
`rust = { version, mr_boxington = true }`, `mr-boxington`, `hk`,
`cargo-binstall`, Nextest, the three settings and `[wrappers.cargo]`, plus a
`rust-toolchain.toml` that mirrors the version. Its `.velnor/config.toml`
keeps `[stacks.rust] compile_driver = "mbx"` as a sticky declaration that the
Mise evidence agrees with. `scripts/toml-tool-pin.awk` reads both the plain
and the inline-table form of a `[tools]` entry, so `verify-local.sh` and
`local-cargo.sh` still report the local pin. The hk configuration (`hk.pkl`
and its secret scanners) comes with the hk rollout, not this page.
