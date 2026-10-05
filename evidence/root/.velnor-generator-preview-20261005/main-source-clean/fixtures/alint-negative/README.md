# Alint negative fixtures

One failing input per enabled rule in `.alint.yml` (mirrored by
`crates/velnor-actions-contract/tests/impl_alint_negative.rs`, which embeds
these files with `include_str!` and asserts the mirrored predicate rejects
each one). Real enforcement is the pinned `alint` binary in its own CI job
(rust-quality-contract §8). This directory is excluded from the walked index
by the `fixtures/**` ignore, so these inputs never trip the real job.

| Fixture | Rule | Violation |
|---|---|---|
| `required-files/repo-file-list.txt` | `required-files` | Simulated repo root missing `clippy.toml` |
| `crates-only/stray.rs` | `crates-only` | `.rs` file at simulated `src/stray.rs`, outside `crates/` |
| `rust-max-lines/oversized.rs` | `rust-max-lines` | 401 physical lines (limit 400) |
| `lib-main-max-lines/lib.rs` | `lib-main-max-lines` | 151 physical lines (limit 150) |

Header comments (`repo-path:`, `expected-lines:`) are read by the tests to
guard against silent fixture drift: editing a fixture without updating its
header fails the test instead of weakening it.
