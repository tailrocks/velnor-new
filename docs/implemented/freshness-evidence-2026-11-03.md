# Freshness evidence — 2026-11-03

This set refreshes all `checked_at` values in `.velnor/freshness-inventory.json`. The 2026-11-02 evidence aged past the 24 h interval at `2026-11-03T11:28:49Z`. Two pins move to the newest stable releases: `nextest` `0.9.150` to `0.9.151` and `jdx/mise-action` `v5.2.5` to `v5.2.6`. Neither bump is blocked, so the set carries no temporary hold.

## Capture time

- `capture-1` ran from `11:37:13Z` to `11:37:59Z`. `capture-2` ran from `11:37:59Z` to `11:38:44Z`.
- The runner documentation was fetched at `11:38:54Z`.
- The oldest `checked_at` in the inventory is `2026-11-03T11:38:00.345Z`. The 24 h freshness window ends at `2026-11-04T11:38:00Z`.
- Release dispatch must start before the window ends. If it cannot, capture again in a small PR that merges before dispatch.

## Capture conditions

- The GitHub requests of both captures ran through `gh api` as the GitHub account `donbeave`, with the OAuth token from the local keyring. This is the authenticated rate limit of 5,000 requests per hour. A capture sends 58 requests to `api.github.com`, one to `crates.io` for `cargo-nextest`, one to `crates.io` for `release-plz` and one to `static.rust-lang.org`. The `crates.io` and `static.rust-lang.org` requests are unauthenticated `curl` calls.
- The native `scripts/check-freshness.sh --check-upstream` probe is always unauthenticated. Its transport sets only `User-Agent`, `Accept` and the encoding headers (`crates/velnor-actions-freshness/src/probe/transport.rs:58-65`) and reads no token. It shares the unauthenticated GitHub limit of 60 requests per hour and per IP address.
- A probe run from an address whose hourly budget is spent gets HTTP 403 on the GitHub lookups. A reviewer rerun hit 403 on six lookups (the cause is inferred from the status code, not from the reviewer's rate-limit reading). At `12:01:29Z` the `rate_limit` endpoint reported `remaining: 0` of 60 for the machine that ran this update. The window reset at `12:33:48Z`. A probe run that began at `12:34:06Z` with `remaining: 60` used 22 requests (`remaining: 38` at `12:34:23Z`) and got no 403. One full probe run therefore fits in the hourly budget, and a second run or the earlier captures from the same address can exhaust it.
- The statement that exactly two pins differed from upstream rests on the authenticated captures, not on a probe run. It is reproducible from `comparison.json`, `receipts.json` and `capture-agreement.json` (see Rows that differed from the pin). A probe run reproduces it only when the address still has budget for every lookup.

## Captures

Two independent authenticated captures are in [the evidence directory](evidence/freshness-2026-11-03/): `capture-1/` and `capture-2/`.

- Each capture queries all 25 tool and action subjects (12 tools, 13 actions).
- GitHub requests use `gh api` with the session credentials. The crates.io requests use a plain `curl` with a user agent. No token is stored in any file.
- Each capture holds 61 receipts, `comparison.json`, `observed-at.json` and `receipts.json`. `receipts.json` binds the 61 response bodies listed in it by SHA-256. `rust.headers.json` (status `206` and the content range of the Rust manifest read) is not bound by `receipts.json`, `capture-agreement.json` or any hash. It records the range response for reading only, and the manifest body itself is bound.
- Each capture reads the Rust manifest with the byte range `0-131071`. The response is `206` with `bytes 0-131071/943486`.
- Each capture records, for every action, one release receipt and two tag receipts (`pinned-*` and `latest-*`). Every pinned action tag resolves to the SHA in the inventory. `mr-boxington` also has both tag receipts, and the commit `b0b67184…` equals its `latest_sha`.
- Annotated tags are resolved to the commit. The raw ref and tag-object responses are kept as `*-ref-raw.body` and `*-tagobj-raw.body`.
- `capture-agreement.json` binds both captures. The `comparison.json` hash is `3f3432e0…` in both. The `observed` fields of all 61 receipts are identical. The result is `agree: true`.

## Rows that differed from the pin

Two rows have `match: false` in both captures. Both were behind upstream when the captures ran, and this set bumps both.

| Subject | Pinned before | Upstream latest | Upstream published |
|---|---|---|---|
| `nextest` | `0.9.150` | `0.9.151` | `2026-11-03T00:02:40Z` on crates.io, `2026-11-03T00:00:41Z` as the GitHub release |
| `jdx/mise-action` | `v5.2.5` | `v5.2.6` | `2026-11-02T19:54:50Z` |

The other 23 rows match their pins. This includes five of the six pins bumped by #467 (`mise`, `mr-boxington`, `jactionlint`, `release-plz` and `asamarts/alint`). #467 also bumped `jdx/mise-action` to `v5.2.5`, which was already behind `v5.2.6` at the time of the captures. The `nextest` pin `0.9.150` was set by 7d8d076872 (#404), not by #467. After this set all 25 rows match their pins.

## Why neither bump is blocked

A pin bump was blocked while the published controller compared the version policy with its compiled catalog (decision X32, PR #464, `lock_mismatch:tool:release-plz:0.4.3:0.4.2` under controller 0.1.66). That check no longer applies to the controller leg.

- `.velnor/generator.lock` and `.velnor/controller-approvals.json` pin controller 0.1.72 at commit `ba363080a7707684fe8f2094851fa513687de73b`. They hold no `nextest` or `jdx/mise-action` value. Their controller-visible pins are the generator binaries, `gh`, the Mise bootstrap and the Mise runtime.
- In that commit, `verify_velnor_repository_files` runs `verify_policy_matches_catalog` only for `AdmissionLeg::Candidate` and does nothing for `AdmissionLeg::Controller` (`crates/velnor-actions-orchestrator/src/validate.rs:56-57`). The controller entry points `prepare_for_controller` and `prepare_for_controller_with_phase_timings` select the controller leg (`crates/velnor-actions-orchestrator/src/prepare.rs:58-61` and `82-87`). These files do not differ between `ba363080a7` and this branch.
- `verify_version_policy` compares only the `[tools]` table (`crates/velnor-actions-mise/src/lock.rs:73-100`). It never reads `[[actions]]`, so no controller code compares a `jdx/mise-action` pin.
- The pin-mirror action (`VELNOR_REPO_POLICY_ACTION=pin-mirror`, required task `test-maintenance-helpers`) enforces the mirror on the pull request head instead. Its fixtures cover tool and action drift (`crates/velnor-actions-cli/tests/pin_mirror_public.rs`) and pass on this branch.
- #467 bumped `mise`, `mr-boxington`, `jactionlint`, `release-plz`, `jdx/mise-action` and `asamarts/alint` in `.velnor/version-policy.toml` and in the compiled catalog under controller 0.1.72. Its `Plan` and `Required` checks passed, and it merged at `2026-11-02T16:53:32Z`.

This set does not change the controller-visible pins, so no velnor-actions release is needed first. The Plan job of this pull request is the final proof. It has not run yet.

## Bumps

`jdx/mise-action` `v5.2.6`:

- Tag `v5.2.6` resolves to commit `f5be966e4a917d54244728e6a5fdcd1d93a35e6c` (`bump/mise-action-v5.2.6-ref.json`). The release is stable, not a draft, and was published at `2026-11-02T19:54:50Z` (`bump/mise-action-v5.2.6-release.json`). The tag `v5.2.5` still resolves to `6612f0ef07ed2a736cd1e00a94a065522dd41195`.
- The comparison `v5.2.5...v5.2.6` has three commits and no change to `action.yml` (`bump/mise-action-5.2.5-to-5.2.6-compare.json`). Runtime changes: the action now calls `process.exit()` after the main and post steps, so a failed cache download cannot hold the step open, and the bundled `tuf-js` moves from `6.0.0` to `6.0.1`. Velnor keeps the action cache disabled and `persist_github_token` unset. The review found no input, default or permission change.
- Regenerated by the generator (`velnor-actions generate`, built from this source): the root `.github` tree and the five OpenTofu golden cases. The root tree equals the generator output. The generator also re-derived the tools-cache identities and the `tools-producer-*` job names, because `.velnor/config.toml` changed for `nextest`.

`nextest` `0.9.151`:

- Release tag `cargo-nextest-0.9.151` points to tag object `cb0e61d1fc7f255076c783b2532f3452b750c86a` and commit `6b3f994577a98874a2e0ea5bef77fbbab3434044`. The release is stable and not a draft. The tag signature is reported as `bad_cert`, as it was for `0.9.150`. No tag signature or artifact attestation is claimed.
- The Linux x86-64 archive (`1d4ce302…`) and the universal Apple archive (`d1d7d025…`) match both the release API digests and the published `.sha256` sidecars.
- The Linux x86-64 executable has SHA-256 `f746a548e01a5ec9f50d209ff3188fbf934a4969bddb1cd84a17af087912219b`. The install-tree digest is `95db46f8610586abf450013b4f4afd869e86f4bf4fcf713fc3e85eabeab1bedc`. The tree computation follows `check_tool_tree.rs` and reproduces the recorded `0.9.150` digest from its executable digest.
- Both binaries report `cargo-nextest 0.9.151 (6b3f99457 2026-11-02)`. The Linux binary ran under `linux/amd64` emulation in the local `ubuntu:26.04` image with a read-only root, no network and no capabilities. The Apple binary ran on `aarch64-apple-darwin`.
- Changed pins: `mise.toml`, `mise.lock`, the compiled catalog, `.velnor/version-policy.toml`, the qualified tool in `.velnor/config.toml`, the `source-qualification.sh` expected value, two unit tests and the README. The `mise.lock` section was derived from the release API by a script that reproduces the `0.9.150` section byte for byte. `mise lock` itself was not run.
- The hosted native check of this pull request downloads the archive and verifies its digest, the install tree, the executable digest and the version probe on the CI runner.

The evidence files are in `evidence/freshness-2026-11-03/bump/`: the release, tag and compare responses, the two sidecars and `nextest-0.9.151-qualification.json`.

The `qualified` values of both rows now equal the new pins, on the evidence above. The repository Mise mirrors other than the files above, the other compiled catalog values and the `qualified` values of the other rows do not change.

## Runner

The official hosted-runner reference still lists `ubuntu-latest`, `ubuntu-24.04`, `ubuntu-22.04` and `ubuntu-26.04` for the standard Linux runner. The inventory keeps `ubuntu-26.04` as default.

Successful main CI run `41626261741` (job `125884265888` `Plan`, source `2a53f0d28311105b5322924eeb10fe7121da4149`) records:

- image `ubuntu-26.04`, version `20261025.179.1`
- runner `2.338.2`

This is a normal CI runtime observation, not formal qualification. The files are in `evidence/freshness-2026-11-03/runner/`: `runner-doc.html`, `ci-job.log` (the `Set up job` step only) and `runner-observation.json`.

## Gates

- `scripts/check-freshness.sh` exits 0: `check-freshness: PASS`, 314 pass rows, 0 fail rows, 5 info rows. All 25 `upstream-freshness` rows pass as `current`, and `exception-expiry` reports no temporary holds.
- `scripts/check-freshness.sh --check-upstream` exits 0, run from `12:34:06Z` with an unauthenticated GitHub budget of 60 and no 403. It reports 339 pass rows, 0 fail rows and no stale pin. The earlier probe run at `11:39:17Z` (before the bumps, with two stale pins and no recorded budget) is superseded.
- Both runs used `MISE_LOCKED=1`, `MISE_ENV=` and `VELNOR_ACTIONS_BIN` unset.
- `alint validate-config` and `alint check --fail-on-warning` exit 0 with the two existing info notices. The local `alint` is 0.19.0, not the pinned 0.21.0.
- `cargo fmt --all -- --check` exits 0 under Rust 1.99.1.
- `cargo nextest run --workspace` (nextest 0.9.151, `--no-fail-fast`) passes 547 of 547 tests, including `pin_mirror_public`, `rust_spec_kind` and `gc1_t53`.
- `scripts/capture-opentofu-goldens.sh ... check` failed before the golden capture, as expected, and `capture` then regenerated the five cases. The dogfood case reports `tree identical` to the root `.github`.
