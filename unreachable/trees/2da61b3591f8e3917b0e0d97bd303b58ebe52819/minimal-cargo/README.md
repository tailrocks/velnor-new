# minimal-cargo fixture
Intent: single crate, cargo-only; no Mise/MBX/Nextest signals.
Files: Cargo.toml, src/lib.rs (1 unit test).
Expected detector outcome: one workspace, profile
compile_driver=cargo, test_runner=cargo_test (+ Nextest
recommendation only).
