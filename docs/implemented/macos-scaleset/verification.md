# macOS Scale Set verification

Vocabulary: `PASS`, `FAIL`, `BLOCKED_EXTERNAL`, `NOT_RUN`. A skipped test is
not a pass.

| Gate | Status | Why |
|---|---|---|
| G0 | PASS for identity, consumer inventory, and supersession only | `evidence.md`. Implementation and live rows are not part of this pass. |
| G1 | PASS for migrate, labels, capacity, and wire tests | Checklist rows cite `40e08e8` and `da68f44`. Registration HTTP is not this pass. |
| G2 | PASS | Pending is visible before the effect returns. Uncertain does not release `Capacity`. Advertise waits for `before_advertise`. Live HTTP and Docker sockets are not this pass. |
| G3 | NOT_RUN | Images are `linux/amd64`. One exited worker's mounts were private volumes only. Spec kill and canary matrix not run. |
| G4 | NOT_RUN | Ordinary echo jobs succeeded, including `https://github.com/tailrocks/velnor-new/actions/runs/37081936404/job/111084145716` on runner `m100000009` and five later echo jobs (`m100000011`, `m100000013`, `m100000015`, `m100000017`, `m100000019`). One-class runs, both lanes: JavaScript run `37089483013` jobs `111106558048` and `111106558263` (`m100000044`, success); services run `37089523814` jobs `111106680712` and `111106680574` (`m100000046`, success); artifacts run `37089574338` jobs `111106828788` and `111106829084` (`m100000048`, success); Buildx run `37089620426` jobs `111106968032` and `111106967890` (`m100000050`, success); expected-negative run `37089657990` jobs `111107078885` and `111107079056` (`m100000052`, `failure`, the required conclusion). Compare stayed skipped (`inputs.mode == 'both'`). The JavaScript class is `node -e`, not a pinned third-party action. Later classes in `evidence.md` passed on both lanes: composite, pinned `actions/github-script`, local Docker action, `container:` with redis health options and service DNS, outputs/env/path, masked echo, OIDC request URL, post-fail (job `failure`, Post step `success`), and cancel during `sleep 180` (`cancelled`). Cache run `37093073005` did not upload; rerun `37093907324` restored `cache-ok` after the tar shim. Later runs, both lanes unless noted: Compose `37095989453` (`m100000076`), bind `37096048488` (`m100000078`), Testcontainers `11.14.0` with `ryuk-seen` `37096079378` (`m100000080`), submodule and LFS `37096132369` (`m100000082`, `git-lfs/3.7.1`), cancel-with-service `37096285742` (runner `sa8ccd085be05`, both logs printed `service-up`, conclusion `cancelled`), same-port `37096417428` (scale-set `m100000086` and `m100000087` on one session with `VELNOR_MAX_JOBS=2`, both printed `port-held`). Still not this pass: a single `features` dispatch, queue pressure behind two busy workers, and a live crash or restart. Mocks must not flip this to PASS. |
| G5 | PASS for in-repo routing and compare | `40e08e8` and `658154c`. Run `37089657990` concluded `failure` on both lanes and its compare job was skipped. That GitHub run is not this pass. |
| G6 | PASS | Help exits 0 and status is `waiting_for_credentials` at `658154c`. LaunchAgent `gui/501` ran absolute `daemon run` with `forks = 0`; a second daemon exited 1; the job was removed. |
| G7 | NOT_RUN | `gh workflow run image-release.yml --ref macos-scaleset` and `gh workflow run macos-binary-release.yml --ref macos-scaleset` each exited 1: HTTP 404, workflow not found on the default branch. Not retried. No image or macOS binary published beyond `v0.1.0`. ChainArgos not updated. |
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
