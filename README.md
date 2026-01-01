# Velnor

Velnor Actions is a stack-generic GitHub Actions workflow generator. It scans
repositories for every registered stack and renders deterministic,
repository-local workflows (`velnor-actions init`, `plan`, `generate`).
V1 registers only the Rust/Cargo adapter.

Canonical documentation (MDX site sources):

- [Documentation home](docs/content/docs/index.mdx)
- [Proposed V1 specification](docs/content/docs/proposed/index.mdx)
- [Implemented work](docs/content/docs/implemented/index.mdx)
- [Bootstrap and release contract](docs/content/docs/proposed/bootstrap-and-release-contract.mdx)
- [Strict Rust policy rollout record](docs/content/development/rust-policy-rollout.mdx)

Build the docs site from `docs/` (`bun install --frozen-lockfile`,
`bun run build`). The CLI lives at `crates/apps/velnor-actions-cli`;
run `cargo run -p velnor-actions-cli -- --help` for flags.
