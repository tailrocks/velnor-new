# T04 Rust-only golden evidence (Phase A behavior bracket)

## Toolchain refresh (2026-10-08)

The toolchain update tracked by issue #6 refreshed the five checked-in case
trees from the current Velnor CLI. `scripts/capture-opentofu-goldens.sh capture`
completed with exit 0; the dogfood preview was byte-identical to the generated
root `.github` tree. The capture ran with Mise 2026.10.4 and Rust 1.98.1, and
`MANIFEST.sha256` was regenerated with the case outputs. Mise action v5.1.1 is
used at reviewed commit `2d8d4cafcbd33be2ea37d2b6f5ad595363d1f1ca`; this
current stable release supersedes issue #6's earlier v5.0.1 target.

## Product version prep (2026-10-09)

The product version owners and generated outputs are prepared from 0.1.4 to
0.1.5. Supported CLI generation refreshed the root `.github` tree;
`scripts/capture-opentofu-goldens.sh capture` and the follow-up `check` both
passed with Cargo/Rust 1.98.1 under Mise 2026.10.4. The four scratch cases and
dogfood match their captured bytes, and the dogfood preview is identical to
the root `.github` tree. The CLI parity golden update and check each passed
four tests.

Positive ConsumerV1 scratch cases receive a runtime-synthetic manifest whose
version comes from the tested CLI, whose target digests are computed from
deterministic mock payload files, and whose source marker is synthetic. The
canonical URL shape is present only to satisfy the manifest schema. These
values are not published assets, source-bound candidate evidence, native
qualification, or a release record. The checked-in
`fixtures/consumer-release-manifest.json` stays unchanged at 0.1.4 as a schema
placeholder for direct schema tests. Candidate qualification continues to
stage and verify the exact supplied candidate manifest and binary through its
separate `check-release` path.

The 2026-10-05 checkpoint and capture environment below record the earlier
source state and remain as historical evidence.

## PR41 + PR46 integration checkpoint (2026-10-05)

After syncing main `2d9bca8` (including the PR71 generator correction and the
PR73 seed-authority correction) and applying the source policy fix, the pinned
debug CLI regenerated the checked-in `.github` tree and recaptured all five
fixture trees. A separate
`scripts/capture-opentofu-goldens.sh check` reports **all five match**, and
`dogfood.verdict` is `identical`. Under
`VelnorRepositoryV1`, discovery neither reads nor synthesizes the consumer
manifest; the generated source tree contains no debug-only manifest data. The
regression in `impl_consumer_manifest_file.rs` proves this path still emits no
consumer `Acquire Velnor` step. No authentic same-run three-target candidate
manifest is present; `check-release`, hosted qualification, immutable
publication, and infrastructure protection remain unverified. The seed
authority correction is present in this source tree, but hosted cache and seed
qualification remain unverified.

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
| nested | 0 | plan.txt, complete generated `.github` tree, tree.sha256 |
| mbx-nextest | 0 | same |
| empty-suite | 0 | same |
| minimal-cargo | 0 | same |
| dogfood (this repo) | 0 | plan.txt, complete generated tree, `dogfood.verdict=identical`, tree.sha256 |

These case trees were regenerated from the current source after the V2 tools-cache
migration using the actual `velnor-actions` CLI. Hosted cache previews include the
generated identity-and-seed prelude, tools-cache restore composite, and identity
helper where the workload uses V2; the V1 `mise-v1-*` key and built-in Mise cache
route are absent. The dogfood tree is byte-equal to the checked-in `.github` tree.
This is generated-source evidence only; it does not claim a hosted cache hit or
persistent-cache round trip.

The four consumer fixtures receive a runtime-synthetic current-version
manifest in their scratch repositories. It uses deterministic mock payload
digests and a synthetic source marker solely to exercise positive
ConsumerV1 generation; generated workflow bytes are the actual CLI output for
that test input. These captures are not publication or qualification
evidence. The dogfood producer repo stays on its VelnorRepositoryV1 path and
receives no consumer manifest. The actual CLI parity suite also removes the
manifest and verifies that `plan` fails closed.

Historical pre-V2 producer capture: the checked-in workflow was regenerated from the reviewed source with
the locked release candidate (`mbx build --release --locked --package
velnor-actions-cli --bin velnor-actions`). Its complete `.github` output was
reproduced by a second `generate --output-dir` run; the only difference from
the preceding shipping tree was `.github/workflows/ci.yml`. That historical CI
workflow SHA-256 was `613eeba58b49f4b6f28da06c97fadeb6567d7f2b521dece53149631643e839b9`.
The local golden collector still builds `target/debug/velnor-actions`; its
`identical` dogfood verdict confirms that this source preview matches the
checked-in tree, but the debug binary is not the producer artifact. The
workflow's required `Check generated files` gate remains in place and must pass
on the PR head before merge. This is producer self-dogfooding only; ChainArgos
consumer regeneration and deployment still require a verified immutable
Velnor product and its matching manifest.

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
