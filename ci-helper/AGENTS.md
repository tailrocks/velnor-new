# ci-helper

Read [`docs/rust-ci.md`](../docs/rust-ci.md) for what the generated workflow does and which setting changes it and [`docs/goals.md`](../docs/goals.md) for the design rules. The helper is the `velnor-ci` binary that every generated `run:` step calls.

Rules for every crate here:

- No file, process, network or environment access outside the modules named `io`. Everything else takes data and returns data.
- No Python and no shell scripts. No `include_str!` of a file outside the crate.
- One integration-test executable per crate (`tests/it/main.rs`).
- No test starts `git`, `cargo`, a shell or a network call, except in the helper's `io` tests.
- Internal crates set `publish = false` and `version = "0.0.0"`. Only the two binary crates carry the release version.

Build and test from `ci-helper/`: `cargo nextest run --locked` (never `cargo test`), `cargo clippy --locked --all-targets -- -D warnings`, `cargo fmt --check`.
