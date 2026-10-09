# Velnor

Velnor Actions is proposed as a stack-generic GitHub Actions workflow generator. It scans repositories for every registered stack and plans all detected, supported stacks unless `.velnor/config.toml` excludes them. V1 registers only the Rust/Cargo adapter; TypeScript, Bun, and other stacks can be added later through dedicated adapters. Rust analysis, Mise integration, generic workflow rendering, orchestration, and CLI are separate crates. Gates 0–8 are implemented on branch `docs/velnor-actions-spec` (unmerged; records in `docs/implemented/`, generated tree is 15 jobs in `.github/workflows/ci.yml` plus the scheduled `.github/workflows/freshness.yml` probe).

See the [documentation index](docs/README.md) for the proposed V1 specification, deferred runner roadmap, and implemented status.

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
developer tool selection (Rust 1.98.1, MBX 1.21.1, Nextest 0.9.146), and
`mise install` resolves those versions. Velnor's CI toolchain is owned by
`.velnor/version-policy.toml` and the compiled catalog; it currently uses
Rust 1.98.1 and Nextest 0.9.148 while the workspace MSRV remains Rust 1.98.
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

Current Velnor CI tool pins checked on 2026-10-09: Mise 2026.10.5,
Rust 1.98.1, Nextest 0.9.148, and release-plz 0.3.170. Issue [#6](https://github.com/tailrocks/velnor-new/issues/6)
proposes Rust 1.99.0, but this update keeps the selected toolchain aligned
with the declared Rust 1.98 MSRV while a separate compatibility decision is
pending. `mise.toml` remains a read-only local developer input.
The workflow pins `jdx/mise-action` v5.1.1 to reviewed commit
`2d8d4cafcbd33be2ea37d2b6f5ad595363d1f1ca`; the action's
`persist_github_token` default is false, and Velnor leaves it unset. This
supersedes issue #6's earlier v5.0.1 target.

Notes: `plan`/`generate` require the checkout's origin to be
`tailrocks/velnor-new` (a local-path clone is identity-rejected until its
origin is set); the preview directory must not be a symlink (`/tmp` on
macOS is one — use `/private/tmp` or another real directory). The full
gated local pass is `scripts/verify-local.sh` (fmt, policy, freshness,
per-crate clippy/tests/doctests/docs, fixtures, Nextest `ci` profile).

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

Deferred roadmap: V2 adds macOS-hosted Docker execution; V3 extends that same model to Debian. A genuine native GitHub Actions runner remains a later milestone.
