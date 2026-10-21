# Product version preparation for v0.1.34

This prepares the workspace and generated release graph for 0.1.34, which
carries five fixes found while generating CI for the ChainArgos repositories
on top of 0.1.33:

- D2 (#268): the git-tag `rust-tag-release` publication job holds
  `contents: write` and `pull-requests: read`, the exact scopes
  `release-plz release` needs (`commits/<sha>/pulls`).
- V1 (#269): `gradle-run-v1` reads Maven bundles from
  `<workspace>/.velnor/deps/maven`, the directory `private-fetch-v1` writes,
  through one shared layout module.
- V2 (#270): `private-revoke-v1` retries its post-revoke probe with a bounded
  backoff (29.5 s) and still fails closed when the token authenticates at the
  end.
- V3 (#272): a dbt project whose `profiles.yml` uses the Postgres adapter gets
  `DBT_ALLOW_EXPERIMENTAL_ADAPTERS=true` from the generator on every dbt step;
  the `[stacks.dbt].env` allowlist stays closed.
- V4 (#273): the default branch resolves from the config key, `origin/HEAD`,
  then the CI event payload, so `[workflow] default_branch` is optional in CI.
  Velnor’s own generated workflows are unchanged by V3 and V4.

It does not change the release protocol or weaken source, attempt, role,
signature or protected-environment admission.

All workspace packages advance together: the workspace version, the exact
internal path requirements and the local package records in the root
`Cargo.lock`. The independently versioned runner, external dependencies,
historical release manifests, consumer pins and project tool configuration
remain unchanged.

The source-built 0.1.34 CLI regenerated the root `.github` tree in place. The
OpenTofu goldens were captured with `scripts/capture-opentofu-goldens.sh` and
the CLI parity goldens with `VELNOR_UPDATE_GOLDENS=1` on the `parity_` tests.
No managed `.github` file was hand edited.

Publication still requires reviewed default-branch source, exact-source full CI,
all three native compiler and qualification paths, attestations and the existing
protected `generator-release` approval.
