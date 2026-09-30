# Velnor consumer fixtures

Minimal consumer repos mirroring detector/validator cases.
Each subdir is self-contained (<=15 small files, no target/).
Detector/validator behavior itself is proven by synthetic
`TempDir`/`ProfileInputs` cases (e.g.
`handwritten_workflow_invoking_mbx_is_strong_evidence`,
`conflicting_runners_rejected` in
`crates/velnor-actions-rust/tests/impl_rust_evidence.rs`),
not by reading these dirs; only `alint-negative/*` is
consumed directly (via `include_str!` in
`crates/velnor-actions-contract/tests/impl_alint_negative.rs`).

| Fixture | Intent | Expected outcome |
|---|---|---|
| minimal-cargo/ | single crate, cargo-only | cargo / cargo_test + Nextest hint |
| mbx-nextest/ | mise mr_boxington + nextest task | mbx / cargo_nextest |
| nested/ | nested workspaces + path dep | each found exactly once |
| handwritten-workflow/ | hand-written ci.yml w/ mbx | mbx pre-generation + warning |
| hostile-config/ | unknown keys, bad types, traversal | REJECT validation |
| conflicting-runners/ | cargo test AND nextest scripts | REJECT ambiguous_test_runner |
| empty-suite/ | crate, no tests | valid_no_test_targets |

Rules: valid TOML where expected-valid; hostile-config
parses as TOML but MUST fail schema validation.
No committed symlinks: symlink hazards (escape, loop)
are built dynamically in TempDirs by
`index_refuses_symlink_escape` / `index_refuses_symlink_loop`
(`velnor-actions-rust` tests). A static loop previously broke
every generic tree-walker (cargo-machete scan, mise-action
`**` cache-key glob in run 36753845572), so hazards must
never rest in the tree.
