# T04 Rust-only golden evidence (Phase A behavior bracket)

## Current release integration checkpoint (2026-10-05)

After syncing main `2d9bca8` and scoping consumer-manifest discovery to
`ConsumerV1`, the pinned debug CLI regenerated the shipping `.github` tree
and all five fixture trees. The golden check reports all five match, and the
dogfood verdict is `identical`; the producer repository no longer reads or
synthesizes a consumer release manifest. No authentic same-run three-target
candidate manifest is present, so `check-release`, hosted qualification,
immutable publication, and infrastructure protection remain unverified.

The capture at `a12efd7` (behavior-identical to `origin/main` 106bfd7;
docs-only delta) is the historical pre-refactor baseline. The V2 tools-cache
migration intentionally changes generated cache steps and files, so its output
is not expected to match that older capture. The checked-in `cases/` below are
the current producer baseline; every later ownership move must rerun these
brackets against the current CLI. Never re-bless blindly: a mismatch is a
behavior change until proven otherwise.

## Bracket 1 (primary): parity corpus suite

`cargo test --locked -p velnor-actions-cli --test velnor_cli
impl_cli_parity_golden` — **2 passed** at capture time.

Cases under `fixtures/parity/<case>/expected/` (byte-compared, documented
normalization only):

| Case | Artifacts pinned |
|---|---|
| minimal-cargo (4 tasks) | plan.txt, ci.yml, actionlint.yaml, plan-v1.json, expected-set.json, task-report.json |
| multi-crate (11 tasks, nextest shards) | same 6 |
| ignored-stack (0 tasks) | plan.txt, ci.yml, actionlint.yaml, plan-v1.json, expected-set.json |
| malformed, malformed-ignored | plan.exit + plan.stderr.txt (`malformed_manifest:` token) |

Identity fields pinned inside `plan-v1.json`: per-obligation
`input_digest`/`task_digest`/`closure_digest`; per-matrix-entry
`input_digest`/`task_digest`/`cache_ids{workspace_id, lane_id, …}`;
`task_ids`, `edges`, `warnings`, `report_id`/`artifact_id` derivations.

## Bracket 2 (supplementary): CLI plan/generate bytes

`scripts/capture-opentofu-goldens.sh check` (default; never writes) — **all
5 match** at capture time. `capture` mode regenerates `cases/` (known-good
only). Setup per case: fixture copy + minimal `.velnor/config.toml` +
deterministic git commit (fixed identity/dates); `plan.txt` normalizes the
`Repository:` line + head SHA; generated YAML is byte-exact with no
normalization.

| Case | plan exit | Goldens in `cases/<case>/` |
|---|---|---|
| nested | 0 | plan.txt, complete generated `.github` tree, tree.sha256 |
| mbx-nextest | 0 | same |
| empty-suite | 0 | same |
| minimal-cargo | 0 | same |
| dogfood (this repo) | 0 | plan.txt, complete generated tree, `dogfood.verdict=identical`, tree.sha256 |

The four consumer fixtures receive the checked-in
`fixtures/consumer-release-manifest.json` in their scratch repositories.
Its placeholder commit and target digests only exercise the canonical
three-target schema. They are not a source-bound candidate manifest, native
qualification, or release evidence. The dogfood producer repo stays on its
VelnorRepositoryV1 path and receives no consumer manifest. The actual CLI
parity suite also removes the fixture and verifies that `plan` fails closed.

The current case trees were regenerated after the V2 tools-cache migration
using the actual `velnor-actions` CLI. Each preview now includes the generated
V2 tools-cache restore composite and identity helper where the workload uses
the cache; the old `mise-v1-*` key and built-in Mise cache route are absent.
The dogfood tree compares byte-for-byte with the checked-in `.github` tree.
`MANIFEST.sha256` was regenerated with the case trees. This is generated-source
evidence only, not a claim of a hosted cache hit or persistent-cache round trip.

Historical pre-V2 producer note: the checked-in producer workflow was then
regenerated from the reviewed source with the locked release candidate
(`mbx build --release --locked --package velnor-actions-cli --bin
velnor-actions`). Its complete `.github` output was reproduced by a second
`generate --output-dir` run; the only difference from that shipping tree was
`.github/workflows/ci.yml`. The historical CI workflow SHA-256 was
`613eeba58b49f4b6f28da06c97fadeb6567d7f2b521dece53149631643e839b9`. At that
capture the local golden collector's `identical` verdict matched the checked-in
tree; the debug binary was not the producer artifact. The workflow's required
`Check generated files` gate remains in place. This producer self-dogfood does
not qualify hosted cache persistence; ChainArgos consumer regeneration and
deployment still require a verified immutable Velnor product and its matching
manifest.

`MANIFEST.sha256` pins every golden file.

## Capture environment

- `cargo 1.98.1`, `rustc 1.98.1` (pinned `mise.toml`), `--locked` builds.
- Binary `target/debug/velnor-actions` sha256 `90d43fabd3d78f3d64a2ff35cb71992c700f4c01e6e7866a286473190341c619` (local
  build; digests embedding the host triple are normalized by harness).
- Fixture maintenance in this commit: `fixtures/nested/Cargo.lock`
  regenerated (`cargo generate-lockfile --offline`) — the stale lock made
  CLI `plan` fail `preparation_incomplete` before any analysis; no test
  reads that file directly.

## Fixture test results (pre-refactor)

- Parity suite: 2 passed, 0 failed (above).
- Full workspace gates run separately per refactor move (see T07/T28);
  this directory pins outputs, the suites pin behavior.
