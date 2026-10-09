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
| empty-suite/ | Nextest library with `test = false` and `doctest = false` | valid_no_test_targets |

## Parity corpus (`parity/`)

Refactor-behavior corpus consumed by
`crates/velnor-actions-cli/tests/impl_cli_parity_golden.rs`: each
`<case>/input` is copied to a scratch git checkout, then `plan`,
`generate`, and `plan-v1` outputs must byte-match `<case>/expected`
(modulo documented normalization: repo path, head SHA, generator
target/SHA). Regenerate only at a known-good commit with
`VELNOR_UPDATE_GOLDENS=1`.

Consumer-generation positive tests build a current-version manifest at test
runtime and install it only in disposable repositories. Its canonical asset
URL shape satisfies the schema; target digests are computed from deterministic
mock payloads and its source marker is synthetic. It is not publication or
qualification evidence. The checked-in `consumer-release-manifest.json` stays
at v0.1.4 as a schema placeholder for direct schema tests; its values do not
describe a published release. Missing manifests are tested as an error by the
actual CLI.

| Case | Intent |
|---|---|
| minimal-cargo/ | single crate, cargo-only baseline |
| multi-crate/ | workspace + path dep + declared mbx/nextest + 2 shards + lockfile + bin doctest-less crate |
| ignored-stack/ | valid repo with `stacks.ignore = ["rust"]` |
| malformed/ | invalid `Cargo.toml`; `plan` fails with `malformed_manifest:` |
| malformed-ignored/ | invalid `Cargo.toml` plus ignore; still fails (ordering) |

Package names must differ from their directory basenames: cargo
elides `name@` from member IDs when they match, and the elided shape
bypasses checkout-path normalization (absolute paths enter digests;
pre-existing product bug, out of refactor scope).

Rules: valid TOML where expected-valid; hostile-config
parses as TOML but MUST fail schema validation.
No committed symlinks: symlink hazards (escape, loop)
are built dynamically in TempDirs by
`index_refuses_symlink_escape` / `index_refuses_symlink_loop`
(`velnor-actions-rust` tests). A static loop previously broke
every generic tree-walker (cargo-machete scan, mise-action
`**` cache-key glob in run 36753845572), so hazards must
never rest in the tree.

Consumer-generation harnesses use the runtime-synthetic current-version
manifest described above in temporary ConsumerV1 repositories. Direct schema
tests continue to read the unchanged v0.1.4 placeholder fixture. The actual
CLI suite separately verifies that a missing manifest is rejected.
