# Velnor

Velnor provides a stack-generic GitHub Actions workflow generator (`crates/velnor-actions-*`) and runner support via the Runner Scale Set controller (`crates/velnor-runner`). It scans repositories for registered stacks (Rust and OpenTofu in V1) and plans all detected, supported stacks unless `.velnor/config.toml` excludes them. TypeScript, Bun, and other stacks can be added later through dedicated adapters. Rust analysis, OpenTofu analysis, Mise integration, generic workflow rendering, orchestration, and CLI are separate crates, while macOS Runner Scale Set host supervision lives in `crates/velnor-runner`. Gates 0–8 are implemented and dogfooded.

See the [documentation index](docs/README.md) for specifications, Runner Scale Set architecture, and implemented status.

The CLI is implemented (`crates/velnor-actions-cli`):

```text
velnor-actions init
velnor-actions plan
velnor-actions generate [--output-dir PATH]
```

Run `cargo run -p velnor-actions-cli -- --help` for flags.
`generate --output-dir PATH` previews into `PATH/.github` without touching
the repository.

`plan` reports detected stacks, Rust workspaces/crates, and the jobs and steps
Velnor would generate. It uses the same analysis and renderer as generation,
prints concise text instead of YAML, and writes no repository files.

## Local build from a clean checkout

No ambient Cargo, Rust, or MBX is required: `mise.toml` pins the local
developer tool selection (Rust 1.99.1, MBX 1.24.1, Nextest 0.9.148), and
`mise install` resolves those versions. Velnor's CI toolchain is owned by
`.velnor/version-policy.toml` and the compiled catalog; it currently uses
Rust 1.99.1 and Nextest 0.9.148 while the workspace MSRV is Rust 1.99.
Historical clean-checkout proof, 2026-10-01 at `34550e8` in a fresh clone
with `cargo`/`mbx` absent from `PATH`:

```sh
git clone https://github.com/tailrocks/velnor-new.git
cd velnor-new && git checkout 34550e8
mise install
mise exec -- cargo --version   # cargo 1.98.1
mise exec -- mbx --version      # mbx 1.21.0
mise exec -- cargo build --locked -p velnor-actions-cli
./target/debug/velnor-actions --help
./target/debug/velnor-actions plan
./target/debug/velnor-actions generate --output-dir /private/tmp/velnor-preview
diff -r .github /private/tmp/velnor-preview/.github  # no output: preview matches
```

Historical freshness evidence checked on 2026-10-17 records Mise 2026.10.13,
Rust 1.98.1, Nextest 0.9.148, and release-plz 0.3.172. The workflow
pins in that evidence were `jdx/mise-action` v5.2.0 and `asamarts/alint` v0.18.4. See
[`freshness-evidence-2026-10-17.md`](docs/implemented/freshness-evidence-2026-10-17.md).
The current catalog selects Mise 2026.10.18, ALint 0.20.0, Rust 1.99.1,
MBX 1.27.0, Nextest 0.9.149, and release-plz 0.4.0.
The selected stable compiler and workspace MSRV are both Rust 1.99.1 and
1.99, respectively. `mise.toml` remains a read-only local developer input.
The workflow pins `jdx/mise-action` v5.2.0 to reviewed commit
`9b200bcecbfd6303e69a4a261b2e09e3757440a7`; the action's
`persist_github_token` default is false, and Velnor leaves it unset. This
supersedes issue #6's earlier v5.0.1 target.

Notes: `plan`/`generate` require the checkout's origin to be
`tailrocks/velnor-new` (a local-path clone is identity-rejected until its
origin is set); the preview directory must not be a symlink (`/tmp` on
macOS is one — use `/private/tmp` or another real directory). The full
gated local pass is `scripts/verify-local.sh` (fmt, policy, freshness,
per-crate clippy/tests/doctests/docs, fixtures, Nextest `ci` profile). Its
generated-tree check executes the binary Cargo just built, so an external or
relative `CARGO_TARGET_DIR` cannot select a stale repository binary.

## Consumer installation

Consumers install an official `velnor-actions` release, then run
`velnor-actions init` and `velnor-actions generate`. Per the
[bootstrap contract](docs/proposed/bootstrap-and-release-contract.md) §2,
every official release publishes one immutable `velnor-actions` binary
asset per supported target plus a manifest with each target's exact asset
URL and SHA-256 digest; generated consumer workflows download that exact
asset and verify its digest before invoking it.

The [`v0.1.4` release](https://github.com/tailrocks/velnor-new/releases/tag/v0.1.4)
is published with binary assets for Linux x86_64, macOS arm64, and macOS
x86_64. Its source run passed all three target qualification and attestation
jobs, but the publisher job exited 1 after checksum verification and an
untagged-release URL was emitted; the historical log does not establish which
final predicate failed. See
[`release-gates.md`](docs/implemented/release-gates.md) before treating the
published release as proof of the complete protected release path. Consumer
repositories must commit a byte-identical copy of the selected published
manifest at `.velnor/release-manifest.json` after reviewing its version,
source commit, asset URLs, and digests. A source build still fails
consumer-policy generation by design (`consumer_requires_release_install`);
it never emits an unverified download URL or a placeholder digest. Velnor's
own repository instead uses the reserved `velnor-repository-v1` policy;
`.velnor/generator.lock` does not exist yet (BOOT-3.4 NEEDS-HUMAN —
seed creates it), so the lock half of the bootstrap cycle is future work,
not a present claim.

## Proposed V1 direction

- Detect every stack with a registered adapter; V1 registers Rust and discovers its Cargo workspaces, crates, dependency edges, and relevant changes.
- Configure stack exclusions in `.velnor/config.toml`; do not add stack selection or exclusion flags to the CLI.
- Keep stack-specific behavior in named adapters; keep Mise integration in `velnor-actions-mise` and compose adapters through `velnor-actions-orchestrator`.
- Generate deterministic, repository-local GitHub Actions workflows with focused jobs and steps.
- Use Mise as the authority for Rust, components, MBX, Nextest, and related tools.
- Select Cargo vs MBX and Cargo test vs Nextest independently from repository evidence; never impose MBX or Nextest on consumers.
- Use Mise task caching only for qualified deterministic task results.
- Run independent crate work in parallel; keep formatting, linting, builds, tests, and doctests visible as distinct tasks.
- Explain selected, executed, reused, skipped, and invalidated work.
- Dogfood Velnor by generating and checking its own committed CI workflow.

Runner support: macOS-hosted Docker execution and official GitHub Actions Runner Scale Set supervision live in `crates/velnor-runner` and are governed by `docs/proposed/macos-scaleset-runner.md`.
