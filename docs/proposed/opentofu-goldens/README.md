# T04 Rust-only golden evidence (Phase A behavior bracket)

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

The four consumer fixtures receive the checked-in
`fixtures/consumer-release-manifest.json` in their scratch repositories.
Its placeholder commit and target digests only exercise the canonical
three-target schema. They are not a source-bound candidate manifest, native
qualification, or release evidence. The dogfood producer repo stays on its
VelnorRepositoryV1 path and receives no consumer manifest. The actual CLI
parity suite also removes the fixture and verifies that `plan` fails closed.

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

- `mise 2026.10.4`, `cargo 1.98.1`, `rustc 1.98.1` (`--locked` build).
- Binary `target/debug/velnor-actions` sha256 `84733ea0ae8b9f3d73acb5ba658906a7cf4e46deaded85b812596629a5e4438f` (local
  debug build of the captured source; digests embedding the host triple are normalized by harness).
- Fixture maintenance in this commit: `fixtures/nested/Cargo.lock`
  regenerated (`cargo generate-lockfile --offline`) — the stale lock made
  CLI `plan` fail `preparation_incomplete` before any analysis; no test
  reads that file directly.

## Fixture test results (pre-refactor)

- Parity suite: 2 passed, 0 failed (above).
- Full workspace gates run separately per refactor move (see T07/T28);
  this directory pins outputs, the suites pin behavior.
