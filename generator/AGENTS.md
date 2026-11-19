# generator

Read [`docs/rust-ci.md`](../docs/rust-ci.md) for how the generated Rust CI behaves and [`docs/goals.md`](../docs/goals.md) for the design rules. Generator rules for tests, samples and goldens: [`TESTING.md`](TESTING.md). Config reference: [`CONFIG.md`](CONFIG.md).

Rules for every crate of this group (under `crates/`):

- No file, process, network or environment access outside the modules named `io`. Everything else takes data and returns data.
- No Python and no shell scripts. No `include_str!` of a file outside the crate.
- One integration-test executable per crate (`tests/it/main.rs`).
- No test starts `git`, `cargo`, a shell or a network call, except in the helper's `io` tests.
- Internal crates set `publish = false` and `version = "0.0.0"`. Only the two binary crates carry the release version.

Build and test from the repository root (one workspace): `cargo nextest run --locked -p velnor-gen -p velnor-model -p velnor-release -p velnor-render -p velnor-rust -p velnor-workflows` (never `cargo test`), `cargo clippy --locked --all-targets -p velnor-gen -p velnor-model -p velnor-release -p velnor-render -p velnor-rust -p velnor-workflows -- -D warnings`, `cargo fmt --check`.
