# macOS Scale Set verification

Vocabulary: `PASS`, `FAIL`, `BLOCKED_EXTERNAL`, `NOT_RUN`. A skipped test is
not a pass.

| Gate | Status | Why |
|---|---|---|
| G0 | PASS for identity, consumer inventory, and supersession only | `evidence.md`. Implementation and live rows are not part of this pass. |
| G1 | NOT_RUN | Wire decode tests exist. Session create/poll/acquire/JIT/ack and schema-2 routing are not landed. Decode tests are not this gate. |
| G2 | NOT_RUN | Local journal module exists. Reopen, fault-injection, and reconcile-before-advertise tests are not landed. |
| G3 | NOT_RUN | Image recipes and ownership decisions exist. No image build, canary, or foreign-object run. |
| G4 | NOT_RUN | No real GitHub job. Mocks must not flip this to PASS. |
| G5 | NOT_RUN | Generator negatives not landed. |
| G6 | NOT_RUN | `velnor-host` builds. Launchctl argv is unit-tested. A live LaunchAgent is not proven. Linux unit tests must not mark launchd PASS. |
| G7 | NOT_RUN | ChainArgos not updated. |
| G8 | NOT_RUN | No promotion. Required checks stay. |

Both workspaces, when they exist, must pass:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo nextest run --locked --workspace
cargo test --locked --workspace --doc
cargo deny check
alint validate-config
alint check --fail-on-warning
bash scripts/check-freshness.sh
cargo fmt --manifest-path crates/velnor-runner/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/velnor-runner/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo nextest run --manifest-path crates/velnor-runner/Cargo.toml --locked --workspace
cargo test --manifest-path crates/velnor-runner/Cargo.toml --locked --workspace --doc
cargo deny --manifest-path crates/velnor-runner/Cargo.toml check
```

Pinned cargo-deny 0.20 rejects `--locked`. The command is `cargo deny check`.
Root workspace success without the nested manifest is a fail.
