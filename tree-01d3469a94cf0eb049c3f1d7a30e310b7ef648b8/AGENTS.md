# AGENTS.md — velnor-actions (Gate 0 skeleton)

Workspace: virtual Cargo workspace, 7 crates under `crates/`, edition 2024,
resolver 3, MSRV 1.98. `Cargo.lock` committed; always pass `--locked`.

## Commands

- Build: `cargo build --workspace --locked`
- Test (all): `cargo test --workspace --locked`
- Test (one): `cargo test --package <name> --locked`
- Nextest CI: `cargo nextest run --workspace --locked --profile ci --no-tests fail`
- Fmt check: `cargo fmt --all -- --check`
- Clippy (per package): `cargo clippy --package <name> --all-targets --locked -- -D warnings`
- Deny: `cargo deny --locked check`

## Gate rules

- `forbid(unsafe_code)` everywhere; no `unwrap`/`panic`/`todo`/`dbg!`
  (tests may use `expect`/`assert`, never `unwrap`).
- Clippy `too_many_lines` is deny (80, Clippy accounting).
- Size caps: any `.rs` 400 lines; `lib.rs`/`main.rs` 150 lines.
- Every crate keeps at least one registered test; `--no-tests fail`.
- Deps need narrow features + shared versions in `[workspace.dependencies]`.
- Alint (`.alint.yml`) covers file/path + required-file + line-count only.
- Normative specs: `docs/proposed/`; landed records: `docs/implemented/`.
