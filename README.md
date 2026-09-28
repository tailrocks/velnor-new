# Velnor

Velnor Actions is proposed as a stack-generic GitHub Actions workflow generator. It scans repositories for every registered stack and plans all detected, supported stacks unless `.velnor/config.toml` excludes them. V1 registers only the Rust/Cargo adapter; TypeScript, Bun, and other stacks can be added later through dedicated adapters. Rust analysis, Mise integration, generic workflow rendering, orchestration, and CLI are separate crates. No implementation is recorded yet.

See the [documentation index](docs/README.md) for the proposed V1 specification, deferred runner roadmap, and implemented status.

The proposed CLI is:

```text
velnor-actions init
velnor-actions plan
velnor-actions generate [--output-dir PATH]
```

`plan` reports detected stacks, Rust workspaces/crates, and the jobs and steps
Velnor would generate. It uses the same analysis and renderer as generation,
prints concise text instead of YAML, and writes no repository files.

## Proposed V1 direction

- Detect every stack with a registered adapter; V1 registers Rust and discovers its Cargo workspaces, crates, dependency edges, and relevant changes.
- Configure stack exclusions in `.velnor/config.toml`; do not add stack selection or exclusion flags to the CLI.
- Keep stack-specific behavior in named adapters; keep Mise integration in `velnor-actions-mise` and compose adapters through `velnor-actions-orchestrator`.
- Generate deterministic, repository-local GitHub Actions workflows with focused jobs and steps.
- Use Mise as the authority for Rust, components, MBX, Nextest, and related tools.
- Use MBX for compilation reuse and Mise task caching only for qualified deterministic task results.
- Run independent crate work in parallel; keep formatting, linting, builds, tests, and doctests visible as distinct tasks.
- Explain selected, executed, reused, skipped, and invalidated work.
- Dogfood Velnor by generating and checking its own committed CI workflow.

Deferred roadmap: V2 adds macOS-hosted Docker execution; V3 extends that same model to Debian. A genuine native GitHub Actions runner remains a later milestone.
