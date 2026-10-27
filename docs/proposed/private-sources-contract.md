# Private sources contract (HD-7)

Status: adopted per the split brief; the owner may revisit. Hard decision:
[HD-7](../hard-decisions.md). Origin: private cross-repository dependency
credentials (H1) of the stacks design. Implementation record:
[private sources](../implemented/private-sources.md).

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
`credential.useHttpPath=true`. The helper reads the token from a mode-0600
file in a mode-0700 Velnor-owned directory that is removed before the revoke
step, not from the environment, so no child process environment carries it
(the workspace forbids `unsafe`, which rules out inheriting a pipe file
descriptor into `git`; this is the recorded deviation from the design text).
`cargo fetch` runs no build scripts. A repository `[source]` replacement that
points elsewhere receives no credential. Later steps run
`--locked --offline` against `$CARGO_HOME/git/db`.

## Placement in generated jobs

When `[private_sources]` is configured, the block is inserted immediately before
the `Fetch Cargo sources` step of

- the Plan job (all lockful workspace roots), after moving the helper staging
  step up to just before the block, and
- every Rust crate job, scoped to the roots the job fetches, and
- one hosted assets producer for each Bun member job that consumes locked
  `release-assets` repositories (`bun-<slug>--assets`). Only this producer
  stages the helper, mints the App token, fetches and revokes it, verifies the
  archives, and uploads the same-run artifact. It runs no repository scripts,
  Bun installation, lifecycle hooks, or package checks.
- every consuming Bun member, suite, and report depends on its member's
  producer and imports that producer's verified artifact before workspace
  verification, registry installation, or scripts. Consumers cannot contain
  the App mint, fetch, or revoke block. The renderer rejects a missing,
  malformed, or mismatched artifact handoff. Each import re-checks files and
  lock archive digests in `verified.json`
  ([Bun browser suites](bun-browser-suites.md)).

The assets producer remains on the GitHub-hosted runner under ScaleSet,
Both, local defaults, and consumer overrides. An explicit local producer
override is rejected. Consumer placement and the one logical member check
remain unchanged.

The Plan job gets the block only when the repository has Cargo roots; a
Bun-only repository has no Cargo source fetch to precede and its release
assets belong to the hosted Bun producers that name them.

A job gets the block only when its allowlist is non-empty: Cargo git sources of
its roots under `owner`, plus the locked repositories of the kinds a Rust job
retrieves (`cargo` and `release-assets`; `git-trees` and `maven-bundles` belong
to the jobs that consume them). No table, or a closure with no private source,
leaves the job byte-identical. Generation fails when declared sources have no
lock (`deps_lock_missing`), when the lock drifted from the declarations
(`deps_lock_drift:<id>`), and for the Velnor repository policy, which stages no
helper (`requires_consumer_policy`). Because the allowlist is a literal, any
change to a `Cargo.lock` private source or to `deps.lock` changes the generated
workflow and the freshness gate turns red until it is regenerated.

Not yet wired: the prebuilt-binary image job (`image_prebuilt`), the credential-free
preview preparation, and jobs of other stacks. They must call
`PrivateWiring::steps_for_roots` before any step that resolves private sources.

## Fetch and revoke operations

The fetch step carries, as literal non-secret env, the owner, the sorted
allowlist (`VELNOR_PRIVATE_REPOSITORIES`), the source kinds to retrieve
(`VELNOR_PRIVATE_SOURCES`: `cargo`, `git-trees`, `maven-bundles`,
`release-assets`), the Cargo workspace roots (`VELNOR_PRIVATE_FETCH_ROOTS`, `.`
is the repository root) and the owned toolchain env. `private-fetch-v1`:

1. refuses an empty or malformed token (`private_source_credential_unavailable`);
2. loads config and `.velnor/deps.lock` from the checkout as data, requires the
   lock to match the declarations, recomputes the allowlist from the roots'
   `Cargo.lock` git sources and the locked sources of the requested kinds, and
   stops with `private_allowlist_drift` on any difference, before a single
   credentialed call (a pull request that adds a private source to a lock cannot
   widen what the minted token can reach);
3. runs `cargo fetch --locked` per root from the runner-owned isolation
   directory (no repository `.cargo/config.toml`), under the private-fetch policy,
   with the Rust the project declares (hard decision 4, resolved from the
   checkout exactly as generation does, so it is the version the generated
   resolver exports as `VELNOR_TOOL_RUST`), after `mise where` proves that
   toolchain installed (`private_rust_toolchain_missing:<spec>` otherwise).
   `mise exec` with a missing version only warns and runs the ambient `cargo`
   with the ambient `CARGO_HOME`; the git database would then miss the owned
   `$RUNNER_TEMP/velnor/cargo` that the later offline steps and the Cargo
   sources cache read;
4. for each locked asset: checks the tag still resolves to the locked commit
   (`private_tag_moved`), the published digest equals the locked SHA-256, the
   downloaded bytes hash to it (`private_asset_sha256_mismatch`), then extracts
   through the bounded archive reader (no absolute or `..` paths, no links) into
   `<workspace>/.velnor/deps/<extract_to>`, `.../maven/<id>` or `.../git/<id>`
   (see "Bundle location");
5. downloads git trees by locked commit.

`private-revoke-v1` runs `always()` after a successful mint: it removes any
leftover credential directory, sends `DELETE /installation/token` (204, or 401
for an already dead token), then calls the API with the same token and requires
401. GitHub invalidates a deleted token eventually, so the probe is retried
with a bounded backoff (0.5, 1, 2, 4, 8, 8 and 6 seconds, 29.5 s in total); the
step fails closed with `private_token_still_valid:<status>` only when the token
still authenticates at the end. Requests use `curl` under the
private-fetch policy with an `https`-only, API-origin-only configuration file;
curl drops the `Authorization` header when a redirect leaves the API host.

## Bundle location

Bundles land under `<workspace>/.velnor/deps/`, the path a consumer's local
fetch script (for example `mise run fetch-deps`) already uses, so one
`.cargo/config.toml` (`[env]` entries with `relative = true` that point
`build.rs` at `.velnor/deps/<id>/...`) works locally and in CI. Consumers
gitignore `.velnor/deps/`. A bundle holds no credential: the token is revoked
before any repository step runs, so a repository build script reading the
bundle gains nothing it could not read from the locked, digest-verified
download. The workspace is repository-controlled, so before extracting, the
fetch resolves the workspace path, requires `.velnor` to be a real directory
and removes whatever occupies `.velnor/deps` (planted files, stale content or a
symlink) without following links. The archive reader then refuses symlinked
ancestors, so extraction cannot leave the workspace. Downloads, the credential
directory and the revoke state stay under `$RUNNER_TEMP/velnor/`.

## Verified marker

After every requested bundle and tree is verified and extracted, the fetch
writes `<workspace>/.velnor/deps/verified.json`. It is a stable contract for
repository code (a `build.rs`, a test, a local script) that wants to confirm
the extracted trees match `.velnor/deps.lock` without fetching again:

```json
{
  "schema": 1,
  "sources": [
    {
      "kind": "release-assets",
      "id": "bitcoin-protos",
      "path": "bitcoin-protos",
      "repository": "ChainArgos/bitcoin-processor-java",
      "tag": "bitcoin-protos-v1.0.0",
      "commit": "<40 lowercase hex>",
      "asset": "bitcoin-protos-1.0.0.tar.gz",
      "sha256": "<64 lowercase hex>"
    }
  ]
}
```

- `schema` is `1`; any incompatible change bumps it. New optional fields may
  be added within schema 1, so readers ignore unknown fields.
- `sources` lists exactly what this job verified, sorted by `kind` then `id`.
  `kind` is `git-trees`, `maven-bundles` or `release-assets`; `path` is the
  extracted directory relative to `.velnor/deps` (`<extract_to>`,
  `maven/<id>` or `git/<id>`); `commit` is the commit the tag resolved to,
  equal to the lock; `asset` and `sha256` (the digest of the downloaded
  archive bytes, equal to the lock) are absent for git trees, which the commit
  pins.
- It is written last and atomically, so its presence means the whole fetch
  succeeded. A failed fetch leaves none. The bundle root is reset at the start
  of every fetch, so a marker planted in the checkout never survives.
- A job whose fetch extracts nothing (Cargo sources only) writes no marker and
  creates no `.velnor/deps`.
- A release asset whose `extract_to` is `verified.json` fails the fetch with
  `private_verified_marker_collision`.

The marker records verification at fetch time. It is not a signature: code
running later in the same job can edit the tree and the marker alike, so treat
it as a consistency check against the lock, not a trust anchor.

Every `vars.*` the generator emits for private sources is declared:
`config-variables` in the generated `jactionlint.yaml` lists
`private_sources.app_id_variable` next to the review and tap variables.

## Credential boundary

`EnvPolicy::PrivateFetch` (`crates/velnor-actions-mise/src/command_env.rs`)
spawns children from a cleared environment plus proxy passthrough and allows
exactly `VELNOR_PRIVATE_FETCH_TOKEN` and `VELNOR_NPM_READ_TOKEN`
(`CREDENTIAL_ALLOWLIST_PRIVATE_FETCH`). It is reachable only through
`IsolatedCommand::private_fetch`, which bakes `GIT_CONFIG_GLOBAL`,
`GIT_CONFIG_NOSYSTEM=1`, `GIT_TERMINAL_PROMPT=0`, `GIT_ALLOW_PROTOCOL=https` and
`CARGO_NET_GIT_FETCH_WITH_CLI=true`, and accepts at most one credential of a
closed kind. Its callers are the orchestrator's `private_fetch` and
`bun_registry_install` builders; a source scan pins that. Every other policy
strips both tokens as `*_TOKEN` credentials, and `with_env` and repository task
declarations reject them as reserved keys.

`GIT_ASKPASS`, `SSH_ASKPASS`, `GIT_CONFIG`, `GIT_CONFIG_*`, `NPM_CONFIG_*`,
`npm_config_*` and `BUN_CONFIG_REGISTRY` are never inherited by any policy
(`FETCH_REDIRECT_ENV_KEYS` and `is_fetch_redirect_key`), and rendered task env
may not declare them.

The renderer mirrors the boundary with four step roles
(`StepRole::PrivateTokenMint`, `PrivateFetch`, `PrivateRevoke` and
`BunRegistryInstall`, the [Bun registry phase](bun-contract.md)):

- `${{ steps.deps-token.outputs.token }}` may appear only as the exact value of
  `VELNOR_PRIVATE_FETCH_TOKEN` in a fetch or revoke step, and `${{ github.token }}`
  only as `VELNOR_NPM_READ_TOKEN` in the Bun registry-phase step. Anywhere else (any env key,
  action input, argv) fails with `credential_binding_outside_private_fetch`.
- The mint step is the only place a secret may be named, only as `private-key`,
  and its inputs are closed (`client-id`, `private-key`, `owner`, `repositories`,
  `permission-contents: read`, `skip-token-revoke: false`);
  anything else fails with `private_mint_shape`.

Parity tests pin the renderer and Mise lists and predicates equal.

## Threat model

| Threat | Outcome |
|---|---|
| Fork PR | Secrets are withheld, so the mint step fails (the App-token action rejects the empty key), the job fails and `Required` fails closed. The fetch operation independently refuses an empty token (`private_source_credential_unavailable`). `pull_request_target` is never used. |
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

```text
velnor-actions deps lock
```

`deps lock` is the only writer of `.velnor/deps.lock` and never writes config.
It requires a `[private_sources]` table, resolves every declared maven bundle,
release asset and git tree to the commit its tag points at (annotated tags
peeled) and, for assets, to the SHA-256 of the published asset, and replaces the
lock atomically with the deterministic document it prints. Resolution runs
through the pinned `gh` with the operator's own `GH_TOKEN` or `GITHUB_TOKEN`;
generated jobs never run it. A draft release, an asset that is missing,
duplicated or not `uploaded`, or an unreadable tag fails the whole command and
leaves the existing lock untouched. Exit statuses follow the
[CLI contract](cli-contract.md) section 7.

The fetch is the internal operation `private-fetch-v1` (the design's `deps
fetch`) and the revocation is `private-revoke-v1`, both selected through
`VELNOR_INTERNAL_OP` with no request file; they are deliberately not public
subcommands.
