# Product version preparation for v0.1.6

- State: version-owned source and generated outputs have been reconciled on
  `fix/release-0.1.6-qualified-generator`. Merge commit
  `2ca50a23a6c11b6db4d23a10414c08b853f7948b` integrates accepted `main`
  commit `1b2b5f25e418a883c85f5af31e2b657f0deb42bc`, whose tree
  `43689bd68a6fcd7fae8df15b844e1e9a25a63086` equals reviewed PR #129 head
  `ab8ea904d46b2bf925db73c6ed8a3a80051cdd08`. Supported captures and the
  focused checks below pass. Full branch gates, final independent review,
  official binary qualification, and hosted publication remain pending. This
  is a preparation record, not a readiness or release claim.
- Version decision: a read-only GitHub inventory at 2026-10-10 02:14 UTC found
  immutable generator release `v0.1.4` (ID 406452151, 20 assets; manifest
  digest `d6f7788e50e0c6168c36d122d910476ac6772cd4603352ef2319a88b888fc076`)
  as the latest published generator release. Tag `v0.1.5` resolves to
  `f1041f322c54cd7edfdef06afd307799eeaa801a` and remains occupied by draft
  release ID 407717552 with 20 assets under the temporary
  `untagged-6899c9b4aa4e941dadba` URL. The `v0.1.6` tag and release were absent
  in the inventory; `0.1.6` is a candidate observation, not a reservation.
  GitHub's overall `isLatest` entry is a separate immutable `binary-...`
  release and does not establish generator acceptance.
- Owners: the root workspace and ten packages, exact local path requirements,
  and ten local `Cargo.lock` package entries use `0.1.6`. A locked offline
  metadata audit confirmed 15 local path requirements at `=0.1.6`. The release
  renderer owns the `0.1.6` version and three target asset names. The separate
  nested runner remains at `0.1.0`; tool-policy pins and external dependency
  records are unchanged.
- Preserved inputs: the static synthetic
  `fixtures/consumer-release-manifest.json` remains at `0.1.4`, as do
  `.velnor/version-policy.toml` and `.velnor/freshness-inventory.json`.
  No official release manifest or candidate binary was created.
- The accepted source merge preserves the integrated Mise 2026.10.6,
  `rustix` process support, freshness and UID/process-scope changes, protocol
  updates, cache retirement, and source tests. A durable renderer
  source-regression report at `/private/tmp/v110r-gi0n7aiv/EVIDENCE.md`
  (SHA-256
  `0ccb17863b2d08840d675da18c33b6c64ef146e83b6a7a952b272ef26d2cbc29`)
  is tied to reviewed source `ab8ea904d46b2bf925db73c6ed8a3a80051cdd08`,
  whose tree equals the accepted merge's source tree. It records equal
  canonical and compact renderer captures across three explicitly labeled
  debug fixtures: 46 jobs and 248 run-related entries (227 string-valued
  command scalars plus 21 `defaults.run` mappings), with six YAML anchors and
  215 alias nodes. The report also records a passing external Psych roundtrip
  for 12 edge strings and direct Zizmor results of 88 ignored and 1 suppressed
  with `unpinned-uses.ignore: []`. The CBA fixture removes its release manifest
  for debug rendering; these results do not prove unchanged-consumer adoption
  or official binary/release qualification. The independent artifact audit of
  this report remains pending.
- Tooling and capture environment: commands used the retained local wrapper
  `/private/tmp/velnor-persistent-stage-mise.sh` (SHA-256
  `602fa43a3854e3f2c85d78f2c491d972ecc32b8021173170dee86f4c1395cd33`),
  which reported Mise 2026.10.6, Rust/Cargo 1.98.1, cargo-nextest 0.9.148,
  and MBX 1.21.1. A fresh short `TMPDIR` was verified by Python as caller-owned
  UID 501/GID 20 with mode 0700. MBX reported the short cache path and available
  Unix-domain listeners; the first actual Cargo build bootstrapped the MBX
  cache. The build target and logs were separate from the source-capture
  worker's paths.
- Supported `.github` generation ran the preview and in-place commands:

  ```text
  cargo run --locked --offline -p velnor-actions-cli -- generate --output-dir <fresh-preview>
  cargo run --locked --offline -p velnor-actions-cli -- generate
  ```

  The generated preview and in-place `.github` tree were byte-identical. The
  CLI listed 26 managed paths. No managed `.github` file was hand edited.
- The 13 version-owned snapshots were derived from those exact CLI outputs:
  the 11 `generator-release-*` local action bodies and the
  `product-release.yml` and `product-release-generator.yml` workflow bodies.
  The generated marker is omitted because the snapshot assertion helper adds
  it back.
- OpenTofu goldens were captured and checked with the same freshly built CLI:

  ```text
  scripts/capture-opentofu-goldens.sh capture <CLI_BINARY>
  scripts/capture-opentofu-goldens.sh check <CLI_BINARY>
  ```

  Both commands passed for nested, mbx-nextest, empty-suite, minimal-cargo,
  and dogfood. Dogfood reported `tree identical`; all five case trees and
  their `tree.sha256` entries match the regenerated manifest.
- CLI parity goldens were captured and compared through the documented test:

  ```text
  VELNOR_UPDATE_GOLDENS=1 cargo test --locked --offline -p velnor-actions-cli --test velnor_cli parity_
  cargo nextest run --locked --offline -p velnor-actions-cli --test velnor_cli parity_
  ```

  Capture passed 4 tests; the default Nextest comparison passed 4 tests.
- Focused release and source checks passed through the pinned local wrapper:

  ```text
  cargo nextest run --locked --offline -p velnor-actions-workflow-renderer generator_release
  cargo nextest run --locked --offline -p velnor-actions-workflow-renderer product_release
  cargo nextest run --locked --offline -p velnor-actions-orchestrator --test velnor_orchestrator schema2
  cargo nextest run --locked --offline -p velnor-actions-orchestrator special
  cargo nextest run --locked --offline -p velnor-actions-orchestrator atomic
  ```

  Results: generator renderer 18 passed; product renderer 25 passed;
  orchestrator snapshot/routing 20 passed; special-permission fixtures 2
  passed; atomic generation/replacement 2 passed. The special-permission tests
  ran with the verified caller-owned, group-compatible temporary directory.
  `git diff --check` passed. Full workspace gates, `verify-local.sh`, exact-main
  release eligibility, three-target official binary qualification, and
  generator acceptance receipt remain separate pending requirements.
- No `v0.1.6` tag, release, draft, publication, or qualification was created
  or claimed. The branch remains a preparation checkpoint and does not replace
  the canonical protected product-release workflow.
