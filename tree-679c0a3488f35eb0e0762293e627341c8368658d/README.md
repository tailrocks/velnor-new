# Velnor

Velnor is restarting as a Rust-first GitHub Actions workflow generator for Rust repositories. Version 1 discovers crates, plans affected work, generates readable workflows, runs focused CI tasks, and coordinates Mise and MBX for cache reuse. It does not implement a self-hosted runner.

The source conversation and its complete design notes are recorded in [docs/velnor-redesign-plan.md](docs/velnor-redesign-plan.md).

## V1 direction

- Discover Cargo workspaces, crates, dependency edges, and relevant changes.
- Generate deterministic, repository-local GitHub Actions workflows with focused jobs and steps.
- Use Mise as the authority for Rust, components, MBX, Nextest, and related tools.
- Use MBX for compilation reuse and Mise task caching only for qualified deterministic task results.
- Run independent crate work in parallel; keep formatting, linting, builds, tests, and doctests visible as distinct tasks.
- Explain selected, executed, reused, skipped, and invalidated work.
- Dogfood Velnor by generating and checking its own committed CI workflow.

V2 adds macOS-hosted Docker execution; V3 extends that same model to Debian. The earlier native GitHub Actions runner proposal remains a later milestone.
