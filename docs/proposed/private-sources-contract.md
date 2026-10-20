# Private sources contract (HD-7)

Status: adopted per the split brief; the owner may revisit. Hard decision:
[HD-7](../hard-decisions.md). Origin: private cross-repository dependency
credentials (H1) of the stacks design.

A generated job that consumes dependencies from private sibling repositories
gets a read-only credential for exactly the repositories its locks name,
uses it inside Velnor-owned steps, and revokes it **before any
repository-controlled step runs**. No credential reaches Gradle, Cargo build
scripts, Bun lifecycle scripts, Mise tasks or any other repository code.

## Credential choice

| Source | Credential | Why |
|---|---|---|
| Cargo git dependencies, release assets, Maven bundles, git trees | Installation token of a dedicated GitHub App `velnor-deps` (`contents: read`, `metadata: read`, installed on producer repositories only) | Scoped to an exact repository list, lives at most one hour, is revoked at fetch end |
| npm and GHCR | `GITHUB_TOKEN` with `packages: read` plus a per-package Actions access grant, in the single registry-phase step | App tokens are not accepted by these registries |

GitHub Packages Maven is repository-scoped and cannot serve private
cross-repository consumers with a scoped credential, so Maven artifacts ship
as release-asset bundles fetched with the same App token. Machine-user classic
PATs and deploy keys are rejected: they are not repository-scopable or they
multiply secrets per producer and consumer.

## Configuration

`.velnor/config.toml`:

```toml
[private_sources]
owner = "ChainArgos"                                   # every producer repository lives here
app_id_variable = "VELNOR_DEPS_APP_ID"                 # organization variable (App client id)
app_private_key_secret = "VELNOR_DEPS_APP_PRIVATE_KEY" # organization secret (App private key)
npm_scopes = ["@chainargos"]                           # registry fixed to npm.pkg.github.com

[[private_sources.maven_bundles]]                      # fixed location, no extract_to
id = "processor-shared-libs"
repository = "ChainArgos/processor-shared"
tag = "processor-shared-libs-v2.3.0"
asset = "processor-shared-libs-2.3.0-maven.tar.gz"

[[private_sources.release_assets]]
id = "explorer-schema"
repository = "ChainArgos/blockchain-explorer"
tag = "explorer-schema-v1.4.0"
asset = "explorer-schema-1.4.0.tar.gz"
extract_to = "explorer-schema"                         # under the Velnor-owned dependency root

[[private_sources.git_trees]]
id = "platform-history-2025-11"
repository = "ChainArgos/java-monorepo"
tag = "platform-history/2025-11"
```

The tables declare what to fetch. They never carry commits or digests.
`deny_unknown_fields` applies everywhere; there are no shell, argv, YAML,
registry-URL or `uses:` fields. Validation rejects, with the key path:

- a repository outside `owner`, a malformed owner, repository, tag or asset name;
- `extract_to` that is absolute, empty, contains `.`/`..` segments, or is missing for a release asset (Maven bundles have a fixed location);
- duplicate ids across all three tables;
- variable and secret names that are not upper-case identifiers or start with `GITHUB_`.

This document extends the design table with `owner` and the declaration
tables so that the generator can render the `owner:` input literally and
`deps lock` knows what to pin.

## Lock

`.velnor/deps.lock` is written only by `velnor-actions deps lock` (it never
writes config; it mirrors `cargo update`) and read by plan, generate and
fetch.

```toml
schema = 1

[[maven_bundles]]
id = "processor-shared-libs"
repository = "ChainArgos/processor-shared"
tag = "processor-shared-libs-v2.3.0"
commit = "<40 lowercase hex>"
asset = "processor-shared-libs-2.3.0-maven.tar.gz"
sha256 = "<64 lowercase hex>"
```

`release_assets` entries add `extract_to`; `git_trees` entries have no
`asset` or `sha256` (the commit is the pin). Freshness requires the lock to
match the config declarations exactly: a declaration without an entry, an
entry with different repository, tag, asset or `extract_to`, or an entry
without a declaration is drift (`deps_lock_drift:<id>`,
`deps_lock_extra:<id>`). A moved tag fails the commit check and the SHA-256
check, never silently.

## Token allowlist

The allowlist is the union of

1. Cargo.lock sources of the exact shape `git+https://github.com/<owner>/<repo>[.git][?...][#sha]`, and
2. every `deps.lock` repository under `owner`.

It is computed by the generator, sorted, de-duplicated and rendered
literally into the mint step's `repositories:` input. A source on another
host, with userinfo or a port, under another owner, over SSH, or a registry
source never qualifies and so never receives a credential. Changing a lock
selects every job that consumes it.

## Generated steps

For a job whose lock closure is non-empty, after tools are prepared and before
any repository-controlled step:

1. **Mint**: the pinned `actions/create-github-app-token` with `owner`,
   the literal `repositories` allowlist, `permission-contents: read`, and
   `skip-token-revoke: false`.
2. **Fetch**: the Velnor-owned fetch step (credential policy `PrivateFetch`),
   SHA-256 verified.
3. **Revoke**: `DELETE /installation/token`, `if: always()`.

Cargo fetch uses `CARGO_NET_GIT_FETCH_WITH_CLI=true`, a Velnor-owned
`GIT_CONFIG_GLOBAL` with `GIT_CONFIG_NOSYSTEM=1`, and a credential helper
keyed exactly on `https://github.com/<owner>/<repo>` with
`credential.useHttpPath=true`. The helper reads the token from an inherited
pipe, not from the environment, so git's children never see it.
`cargo fetch` runs no build scripts. A repository `[source]` replacement that
points elsewhere receives no credential. Later steps run
`--locked --offline` against `$CARGO_HOME/git/db`.

## Credential boundary

`CredentialPolicy::PrivateFetch` (`EnvPolicy::PrivateFetch` in
`command_env.rs`) allows exactly `VELNOR_PRIVATE_FETCH_TOKEN` and
`VELNOR_NPM_READ_TOKEN`. It is constructible only by the orchestrator's
`private_fetch` and `bun_registry_install` builders. The renderer mirrors it
with `StepRole::PrivateFetch` and rejects
`${{ steps.deps-token.outputs.token }}` or `${{ github.token }}` in any other
step as `credential_binding_outside_private_fetch`. A parity test pins the
renderer and Mise lists equal. Repository task, Gradle, Bun-script, dbt and
Ansible policies keep stripping every `*_TOKEN`. `GIT_ASKPASS`,
`GIT_CONFIG_*` (except Velnor's own), `NPM_CONFIG_*` and
`BUN_CONFIG_REGISTRY` are never-inherited keys.

## Threat model

| Threat | Outcome |
|---|---|
| Fork PR | Secrets are withheld. The mint fails with `private_source_credential_unavailable:fork`, the job fails, `Required` fails closed. `pull_request_target` is never used. |
| Same-repository PR by a writer | Can edit the generated workflow and mint `contents: read` tokens for App-installed repositories. This is the recorded self-attestation limit (review and CODEOWNERS are the trust root, see [fork-PR trust model](../implemented/fork-pr-trust-model.md)). Mitigations: App installed on producers only, no write permission, secret visible only to consumer repositories, CODEOWNERS on `.github/**` and `.velnor/**`, rotation runbook, audit-log alerts. |
| Compromised dependency or build script | The token is revoked before the first repository-controlled step; a crate build script never sees it. |
| Exfiltration by redirect | The helper sends the token only to allowlisted GitHub URLs. Velnor writes the bunfig; repository registry overrides fail generation. |
| Tag tampering | Rulesets block tag updates; Cargo.lock and deps.lock pin commit and SHA-256. |
| Cache poisoning | Private objects live in repository-scoped caches and bundles are re-verified by SHA-256 on every restore. |

## Owner blockers

The `velnor-deps` GitHub App does not exist yet. Until the owner creates it
(`contents: read`, `metadata: read`, installed on producers only), sets the
organization variable and secret named in `[private_sources]`, and adds tag
rulesets blocking update and deletion of release tags in every producer,
nothing here can be exercised against live repositories. Implementation and
qualification use fixtures.

## CLI

`velnor-actions deps lock` resolves every declaration to a commit and
SHA-256 and writes `.velnor/deps.lock`; `velnor-actions deps fetch` is the
fetch operation the generated step runs. See the
[CLI contract](cli-contract.md).
