# T04 Rust-only golden evidence (Phase A behavior bracket)

Pre-refactor capture at `a12efd7` (behavior-identical to `origin/main`
106bfd7; docs-only delta). Every T06 ownership move must re-run the
brackets below with byte-identical results. Never re-bless blindly:
a mismatch is a behavior change until proven otherwise.

## Bracket 1 (primary): parity corpus suite

`cargo test --locked -p velnor-actions-cli --test velnor_cli
impl_cli_parity_golden` — the corpus now includes a real-CLI negative for a
missing consumer manifest, alongside the artifact and malformed-input checks.

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

`scripts/capture-opentofu-goldens.sh check /path/to/release/velnor-actions`
— **all 5 match** for the 2026-10-05 release-build capture. `capture` mode
regenerates `cases/` (known-good only). Setup per case: fixture copy + minimal
`.velnor/config.toml` + the explicit deterministic manifest fixture at
`fixtures/consumer-release-manifest.json` + fixed-identity/date git commit;
`plan.txt` normalizes the `Repository:` line + head SHA; generated YAML is
byte-exact with no normalization. The manifest carries placeholder provenance
and exists only to exercise structural generation; it is not release or
qualification evidence.

| Case | plan exit | Goldens in `cases/<case>/` |
|---|---|---|
| nested | 0 | plan.txt, generate tree (ci.yml + actionlint.yaml), tree.sha256 |
| mbx-nextest | 0 | same |
| empty-suite | 0 | same |
| minimal-cargo | 0 | same |
| dogfood (this repo) | 0 | plan.txt, generate tree, `dogfood.verdict=identical`, tree.sha256 |

The dogfood case records `identical`: a fresh release-binary `generate
--output-dir` tree matches the checked-in `.github` tree. The release binary
was built with `cargo build --release --locked -p velnor-actions-cli --bin
velnor-actions` under Rust 1.98.1; SHA-256 was
`7073069f26525d4e520b2e8cd8af390feb03c849aca9e745cd7ae44bab431614`.
The required `Check generated files` workflow gate still verifies generated
tree parity on each PR head. This structural generator proof is separate from
consumer release qualification or deployment, which require verified
immutable Velnor assets and their matching manifest.

`MANIFEST.sha256` pins every golden file.

## Historical capture

The original behavior bracket was captured at `a12efd7`, before the explicit
manifest fixture was required by the golden harness. It is retained as
historical context only; current expected files and the current `MANIFEST.sha256`
come from the release-binary capture above.

## Fixture test results (pre-refactor)

- Parity suite: 2 passed, 0 failed (above).
- Full workspace gates run separately per refactor move (see T07/T28);
  this directory pins outputs, the suites pin behavior.
