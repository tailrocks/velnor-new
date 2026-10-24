# Private sources (HD-7)

Specification: [private sources contract](../proposed/private-sources-contract.md).
Hard decision: [HD-7](../hard-decisions.md), adopted per the split brief; the
owner may revisit. The `velnor-deps` GitHub App does not exist yet: creating it
(`contents: read` and `metadata: read`, installed on producer repositories
only), the organization variable `VELNOR_DEPS_APP_ID`, the organization secret
`VELNOR_DEPS_APP_PRIVATE_KEY`, and tag rulesets blocking update and deletion of
release tags in every producer are owner actions. Everything below is verified
with fixtures and qualification tests, not against live private repositories.

## What landed

| Part | Where | Evidence |
|---|---|---|
| `[private_sources]` config, `.velnor/deps.lock` schema, drift checks, token allowlist | `velnor-actions-contract` (`config/private_sources.rs`, `deps_lock.rs`) | `tests/impl_contract_private_sources.rs` |
| `velnor-actions deps lock` | orchestrator `deps_lock.rs`, CLI `dispatch_deps.rs` | `deps_lock_tests.rs`, `impl_cli_private.rs` |
| `EnvPolicy::PrivateFetch`, never-inherited fetch-redirect keys | Mise adapter `command_env.rs`, `command_private_fetch.rs` | `tests/impl_mise_private_fetch.rs` |
| Step roles, mint shape and credential-binding gates, block constructor | contract `step_identity.rs`, renderer `private_source_steps.rs`, `support_private_tokens.rs` | `tests/impl_renderer_private_sources.rs`, `impl_private_sources_parity.rs` |
| `private-fetch-v1`, `private-revoke-v1` | orchestrator `private_fetch*.rs`, `private_revoke.rs`, CLI gates | `private_fetch_*tests.rs`, `private_revoke_tests.rs`, `impl_cli_private.rs` |
| Generated placement in the Plan job and Rust crate jobs | orchestrator `private_wiring.rs` | `private_wiring_tests.rs`, `private_wiring_workflow_tests.rs` |
| Placement in MBX `Dependency seed` jobs (Velnor helper staged right after checkout, before the mint, fetch and revoke; without it v0.1.37 failed with `internal_without_acquire:dependency-seed`) | orchestrator `dependency_seed.rs` | `impl_private_sources_generate.rs` (`mbx_seed_job_with_private_sources_stages_the_helper_before_the_fetch`, golden step order) |
| Placement in `cargo_bin` image build jobs (helper staging, mint, fetch, revoke before the Cargo source fetch and the BuildKit build) | orchestrator `image_prebuilt.rs` | `image_prebuilt_tests.rs` |

## Decisions

- Bundle location (adopted per the split brief; owner may revisit). Bundles are
  extracted to `<workspace>/.velnor/deps/<id>/`, not `$RUNNER_TEMP`, so the local
  fetch layout and the CI layout match and one `.cargo/config.toml` serves both.
  No security reason requires keeping bundles outside the workspace: they carry
  no credential and the token is already revoked when repository code runs. The
  hostile-checkout cases (a planted `.velnor/deps` symlink or content) are
  neutralized by `prepare_deps_root`: it rejects unsafe parent paths and removes
  existing entries without following symlinks. These cases are tested in
  `private_fetch_attack_tests.rs`.

- The generated `actionlint.yaml` declares `private_sources.app_id_variable`
  (`declared_config_variables`); `tests/impl_private_sources_generate.rs`
  generates a `[private_sources]` repository through the pinned actionlint.

- Cargo git sources land in the owned Cargo home. The fetch runs the
  project-declared Rust, not the Velnor pin, and refuses to start when that
  toolchain is not installed. Before this, a project whose Rust differed from
  the Velnor pin (bitcoin-processor-rust run 39591265476: project 1.98.1, pin
  1.99.1) fetched into `~/.cargo`: `mise exec` warned about the missing pin
  and ran the ambient `cargo`. The step logged success, the next
  `Fetch Cargo sources` step missed offline and failed an unauthenticated
  clone of the private repository. Tests:
  `cargo_sources_are_fetched_with_the_rust_the_project_declares`,
  `cargo_fetch_runs_exactly_the_rust_it_is_given`,
  `the_toolchain_check_asks_mise_where_for_the_same_rust_and_carries_no_token`.

- Verified marker (`private_fetch_verified.rs`, adopted per the split brief;
  owner may revisit): after a successful fetch,
  `.velnor/deps/verified.json` records each verified bundle's tag, commit,
  asset and archive SHA-256 so repository code can check the extracted trees
  against the lock offline. Contract: "Verified marker" in the private sources
  contract. Tests: `private_fetch_verified_tests.rs` and the tampered-asset case
  in `private_fetch_attack_tests.rs`.

## Threat-model checks and where they are tested

- Token unavailable (fork pull request) fails closed: `impl_cli_private.rs`, `Token` tests.
- The credential helper answers only for the exact allowlisted URLs: real `git credential fill` in `private_fetch_session_tests.rs`.
- A pull request that adds a private source to a lock cannot widen the token: `private_allowlist_drift` tests; lookalike hosts earn no token.
- Moved tag, tampered or republished asset, hostile archives: `private_fetch_attack_tests.rs`.
- No child environment or argv carries the token; credential directory modes and removal: `private_fetch_session_tests.rs`.
- The revoked token is dead (API 401) or the job fails: `private_revoke_tests.rs`.
- Binding the token anywhere but the fetch and revoke steps fails generation: renderer tests.
- Allowlist drift turns the freshness gate red: the allowlist is a literal in the generated workflow (`private_wiring_workflow_tests.rs`).

## Remaining

- Wire `PrivateWiring::steps_for_roots` into the prebuilt-binary image job (`image_prebuilt`), the credential-free preview preparation, and the Java and Bun jobs (with `maven-bundles` and the npm registry phase through `bun_registry_install`).
- Live qualification against `tailrocks/velnor-fixture-private-producer` and `-consumer` once the App exists.
- Per-source consumers in `deps.lock` so that jobs fetch only the sources they use.
