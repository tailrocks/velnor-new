## What

<!-- One paragraph: the capability or fix this PR lands. -->

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --locked --workspace --all-targets -- -D warnings`
- [ ] `cargo nextest run --locked --workspace`
- [ ] `alint validate-config && alint check --fail-on-warning`
- [ ] Docs record updated in the same PR (`docs/content/docs/`)
