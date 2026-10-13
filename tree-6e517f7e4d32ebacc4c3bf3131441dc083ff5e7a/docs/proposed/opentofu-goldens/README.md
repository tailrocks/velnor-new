# T04 Rust-only golden evidence (Phase A behavior bracket)

Pre-refactor capture at `a12efd7` (behavior-identical to `origin/main`
106bfd7; docs-only delta). Every T06 ownership move must re-run the
brackets below with byte-identical results. Never re-bless blindly:
a mismatch is a behavior change until proven otherwise.

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
| nested | 0 | plan.txt, generate tree (ci.yml + actionlint.yaml), tree.sha256 |
| mbx-nextest | 0 | same |
| empty-suite | 0 | same |
| minimal-cargo | 0 | same |
| dogfood (this repo) | 0 | plan.txt, generate tree, `dogfood.verdict=DIFFERS` (preview is not shipped), tree.sha256 |

The current dogfood capture records a source preview, not a release. Its diff
includes this preflight change and existing generator-to-`.github` drift from
main, including qualification workflow changes; it is not attributable only to
this patch and does not establish release parity. Shipping `.github` remains
unchanged. Regenerate it only from a verified release artifact and review the
resulting diff before publishing.

`MANIFEST.sha256` pins every golden file.

## Capture environment

- `cargo 1.98.1`, `rustc 1.98.1` (pinned `mise.toml`), `--locked` builds.
- Binary `target/debug/velnor-actions` sha256 `ca05f870fb4f9784…` (local
  build; digests embedding the host triple are normalized by harness).
- Fixture maintenance in this commit: `fixtures/nested/Cargo.lock`
  regenerated (`cargo generate-lockfile --offline`) — the stale lock made
  CLI `plan` fail `preparation_incomplete` before any analysis; no test
  reads that file directly.

## Fixture test results (pre-refactor)

- Parity suite: 2 passed, 0 failed (above).
- Full workspace gates run separately per refactor move (see T07/T28);
  this directory pins outputs, the suites pin behavior.
