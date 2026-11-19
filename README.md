# Velnor

Velnor provides a GitHub Actions workflow generator (`velnor-gen`, `crates/velnor-gen`) and runner support via the Runner Scale Set controller (`crates/velnor-runner-*`). `velnor-gen` writes the Rust CI workflow of a repository from `.velnor/rust-ci.toml` and every other workflow declared in `.velnor/workflows.toml`; `velnor-ci` is the one program the generated steps call at run time. The old `velnor-actions` engine is deleted (#1459).

See the [documentation index](docs/README.md) for specifications, Runner Scale Set architecture, and implemented status.

For the Rust CI generator, see [how Velnor keeps Rust CI fast](docs/rust-ci.md). The configuration reference is [`generator/CONFIG.md`](generator/CONFIG.md).

## Generate the workflows

```text
velnor-gen generate   # write .github/workflows/rust-ci.yml, the declared workflows and .velnor/rust-metadata.json
velnor-gen check      # exit 1 when a generated file differs from what generate would write
```

Build it from a clean checkout with `mise install` (`mise.toml` pins Rust, MBX and Nextest) and
`cargo run -p velnor-gen -- generate`.

## Consumer installation

Consumers pin a released helper with `velnor-gen pin-helper <version>`, which writes `.velnor/rust-ci.lock`, and run `velnor-gen generate`. The release settings are documented in the [release configuration reference](docs/implemented/generator-release-config.md).

## Direction

- Generate deterministic, repository-local GitHub Actions workflows with focused jobs and steps.
- Use Mise as the authority for Rust, components, MBX, Nextest, and related tools.
- Run independent crate work in parallel; keep formatting, linting, builds, tests, and doctests visible as distinct tasks.
- Explain selected, executed, reused, skipped, and invalidated work.
- Dogfood Velnor by generating and checking its own committed CI workflow.

Runner support: macOS-hosted Docker execution and official GitHub Actions Runner Scale Set supervision live in `crates/velnor-runner-*` and are governed by `docs/proposed/macos-scaleset-runner.md`.
