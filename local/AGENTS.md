# local

Read [`docs/goals.md`](../docs/goals.md) for the design rules and [`README.md`](README.md) for how to run it. The generated CI it reproduces is described in [`docs/rust-ci.md`](../docs/rust-ci.md).

Rules for every crate of this group (under `crates/`):

- No file, process, network or environment access outside the modules named `io`. Everything else takes data and returns data.
- No Python and no shell scripts. No `include_str!` of a file outside the crate.
- One integration-test executable per crate (`tests/it/main.rs`).
- No test starts `git`, `cargo`, a shell or a network call, except in the helper's `io` tests.
- Internal crates set `publish = false` and `version = "0.0.0"`. Only the two binary crates carry the release version.

Build and test from the repository root (one workspace): `cargo nextest run --locked -p velnor-local -p velnor-cache-rules` (never `cargo test`), `cargo clippy --locked --all-targets -p velnor-local -p velnor-cache-rules -- -D warnings`, `cargo fmt --check`.
