# Velnor

Velnor Actions is proposed as a stack-generic GitHub Actions workflow generator. It scans repositories for every registered stack and plans all detected, supported stacks unless `.velnor/config.toml` excludes them. V1 registers only the Rust/Cargo adapter; TypeScript, Bun, and other stacks can be added later through dedicated adapters. Rust analysis, Mise integration, generic workflow rendering, orchestration, and CLI are separate crates. Gates 0–8 are implemented on branch `docs/velnor-actions-spec` (unmerged; records in `docs/implemented/`, generated tree is 14 jobs in `.github/workflows/ci.yml` only).

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

## Consumer installation

Consumers install an official `velnor-actions` release, then run
`velnor-actions init` and `velnor-actions generate`. Per the
[bootstrap contract](docs/proposed/bootstrap-and-release-contract.md) §2,
every official release publishes one immutable `velnor-actions` binary
asset per supported target plus a manifest with each target's exact asset
URL and SHA-256 digest; generated consumer workflows download that exact
asset and verify its digest before invoking it.

No official release exists yet (only the `seed/velnor-actions-0.1.0`
bootstrap seed, which is not an official release). Until one does, build
from source on this branch. Version-bound seed manifests remain accepted
as review-gated bootstrap inputs on the consumer path until the first
official release supersedes them. A source build fails consumer-policy
generation by design (`consumer_requires_release_install`); it never emits
an unverified download URL or a placeholder digest. Velnor's own
repository instead uses the reserved `velnor-repository-v1` policy with
`.velnor/generator.lock` to break the bootstrap cycle.

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
