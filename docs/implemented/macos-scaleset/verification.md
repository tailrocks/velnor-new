# macOS Scale Set verification

Vocabulary: `PASS`, `FAIL`, `BLOCKED_EXTERNAL`, `NOT_RUN`. A skipped test is
not a pass.

| Gate | Status | Why |
|---|---|---|
| G0 | PASS for identity, consumer inventory, and supersession only | `evidence.md`. Implementation and live rows are not part of this pass. |
| G1 | PASS for migrate, labels, capacity, and wire tests | Checklist rows cite `40e08e8` and `da68f44`. Registration HTTP is not this pass. |
| G2 | NOT_RUN | Reopen and a missing-row finish are tested. Fault injection around HTTP and Docker, and reconcile-before-advertise, are not. |
| G3 | NOT_RUN | Image recipes and ownership decisions exist. No image build or live inspect. |
| G4 | NOT_RUN | `GET .../actions/runner-scale-sets` returned 404. No shipped-client session and no GitHub job. Mocks must not flip this to PASS. |
| G5 | PASS for in-repo routing and compare | `40e08e8` and `658154c`. A GitHub run of an expected-negative workflow is not this pass. |
| G6 | PASS | Help exits 0 and status is `waiting_for_credentials` at `658154c`. LaunchAgent `gui/501` ran absolute `daemon run` with `forks = 0`; a second daemon exited 1; the job was removed. |
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
