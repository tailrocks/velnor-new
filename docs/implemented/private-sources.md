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
| Placement in `cargo_bin` image build jobs (helper staging, mint, fetch, revoke before the Cargo source fetch and the BuildKit build) | orchestrator `image_prebuilt.rs` | `image_prebuilt_tests.rs` |

## Decisions

- Bundles use `<workspace>/.velnor/deps/<id>/` in local runs and CI. Both
  environments use the same `.cargo/config.toml`. Bundles contain no
  credentials. The workflow revokes the token before repository code runs.
  `prepare_deps_root` rejects unsafe parent paths and removes existing
  dependency entries without following symbolic links.
  `private_fetch_attack_tests.rs` checks these cases. The owner adopted this
  location through the split brief and can review it again.
- The generated `actionlint.yaml` declares `private_sources.app_id_variable`.
  `declared_config_variables` supplies this name.
  `tests/impl_private_sources_generate.rs` checks generation with the pinned
  actionlint.

- Cargo Git sources use the managed Cargo home. When the project declares
  Rust, the fetch uses that version. Otherwise, it uses the Velnor pin.
  The fetch requires that toolchain to be installed before starting Cargo.
  In bitcoin-processor-rust run 39591265476, the project declared 1.98.1 and
  the Velnor pin was 1.99.1. Mise warned about the missing pin and ran ambient
  `cargo`, which fetched into `~/.cargo`. The step logged success.
  The next `Fetch Cargo sources` step missed the offline cache.
  It then failed while attempting an unauthenticated clone of the private repository.
  Tests: `cargo_sources_are_fetched_with_the_rust_the_project_declares`,
  `cargo_fetch_runs_exactly_the_rust_it_is_given`, and
  `the_toolchain_check_asks_mise_where_for_the_same_rust_and_carries_no_token`.

- After a successful fetch, `.velnor/deps/verified.json` records each verified
  bundle's tag, commit, asset, and archive SHA-256. Repository code can compare
  those values with the lock without another fetch. The owner may revisit this
  decision, adopted under the split brief. See "Verified marker" in the private
  sources contract. Tests: `private_fetch_verified_tests.rs` and the asset-tampering
  test in `private_fetch_attack_tests.rs`.

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
