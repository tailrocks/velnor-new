# Package publication contract

**Status:** Proposed. Implements items RF (release and tag flow, versioning)
and RA (release-asset bundles) of the ChainArgos split design (§4.0, §4.4),
under [hard decision 8](../hard-decisions.md). Image promotion (I.promote)
builds on this flow and is specified when it lands; npm (N) is section 8 and
Maven bundles (M) are section 9.
Every choice marked *adopted per the split brief; owner may revisit* was
taken from the split design without a separate owner review.

## 1. Versioning and tags

Versioning is independent SemVer per artifact. The source of truth is the
artifact's own manifest, never a derived or shared number:

| Source | Rule (generation time and run time agree) |
|---|---|
| `Cargo.toml` | first literal `version = "X.Y.Z"` line of `[package]` or `[workspace.package]`; `version.workspace = true` is not a literal, so point `version_file` at the workspace root |
| `gradle.properties` | first `version=X.Y.Z` line |
| `package.json` | top-level `version` string |
| inline | `version = "X.Y.Z"` in the `[[publish.bundles]]` entry itself |

`X.Y.Z` is strict: three numeric parts, no leading zeros, no pre-release or
build suffix. Consumers pin exact versions; breaking changes bump the major.

A release tag is `<artifact-id>-v<X.Y.Z>`, for example `explorer-schema-v1.4.0`.
An artifact id is lowercase alphanumeric words joined by single hyphens,
starting with a letter, at most 64 bytes. The id and version split at the
last `-v` (`ReleaseTag::parse`), which is unambiguous because a SemVer never
contains `-v`. This matches release-plz's `{{package}}-v{{version}}`, and
generation refuses a bundle id equal to a released Rust package name so one
tag can never mean two artifacts.

A maintainer merges a version bump to `main`; when `Required` is green on
that SHA, an annotated tag is pushed. Org rulesets restrict creation of
`*-v*` tags and block tag updates and deletions (owner action; see §5).

## 2. Generated `publish.yml`

`publish.yml` is emitted only under `consumer-v1` with at least one
`[[publish.bundles]]` entry. It deviates from the design's single
`release.yml` because that name already belongs to the release-plz family.
Trigger: `push` of tags matching `*-v[0-9]*.[0-9]*.[0-9]*`. The glob is coarse
on purpose; the authority job re-parses the tag with the strict grammar.
There is no other event. Concurrency is one never-cancelled queue per ref
(`publish-${{ github.ref }}`), not per artifact: GitHub keeps one pending run
per group and cancels older pending runs, which would drop a release.

### Authority job (`publish-authority`)

Permissions `contents: read`, `checks: read`; no checkout. It runs fixed
scripts, each failing the job, in order:

1. **Parse release tag**: event is `push`, ref type is `tag`, repository is the
   generation-time `origin` slug, version and id match the strict grammar, and
   the id is in the declared set. Outputs `artifact` and `version`.
2. **Verify annotated tag**: the ref is a tag object (not a lightweight tag)
   and its target commit equals the pushed `GITHUB_SHA`.
3. **Verify default branch ancestry**: the compare API reports the commit
   `behind` or `identical` to the default branch.
4. **Verify Required check**: a check run named `Required`, concluded
   `success`, created by the `github-actions` app, exists at the SHA.
5. **Verify version**: the manifest read through the contents API at the SHA
   (or the inline literal) equals the tag version. Generation applies the same
   reader to the work tree, so a stale `version_file` fails `generate`.

GitHub API reads use the pinned `gh` through `mise exec` with the job's
`GITHUB_TOKEN`. jq programs end in `error(...)`, which makes `gh api --jq`
exit non-zero.

Deferred from the design: "the tag points to the same commit as the Cargo.lock
or `deps.lock` SHA recorded at release" depends on item P's `deps.lock`.

## 3. Release-asset bundles (RA)

```toml
[[publish.bundles]]
id = "explorer-schema"
paths = ["backend-rust/blockchain-explorer/schemas/schema.graphql"]
version_file = "backend-rust/blockchain-explorer/Cargo.toml"
```

`paths` are sorted, unique, repo-relative files or directories (no globs, no
`..`, no `.git`, no symlinks, checked at generation). Exactly one of
`version` and `version_file` is set. Per bundle, three jobs:

| Job | Authority | Rule |
|---|---|---|
| `bundle-build-<id>` | `contents: read` | `if` the authority's `artifact` output equals the id. The only checkout. Packs `<id>-<version>.tar.gz` plus `.sha256` and uploads them as a run artifact |
| `bundle-attest-<id>` | `contents: read`, `id-token: write`, `attestations: write` | No checkout. Re-verifies the checksum, then runs `actions/attest-build-provenance` on the tarball |
| `bundle-publish-<id>` | `contents: write`, environment `release` | No checkout. Re-verifies the checksum, then `gh release create --verify-tag` or `gh release upload` (never `--clobber`), so an existing asset is never overwritten: one with the same sha256 is skipped (a release finished by hand, or a re-run), a different one fails the job |

The archive is deterministic: GNU tar with `--sort=name --mtime=@0
--owner=0 --group=0 --numeric-owner --format=ustar --mode=u=rwX,go=rX`,
then `gzip -n -9`. A rebuild after touching a file's mtime is byte-identical.
Any symlink or missing path fails the pack.

Consumers lock the bundle with its sha256 in `.velnor/deps.lock` (item P).
Attestation verification at the consumer is part of P's fetch step.

## 4. Verification

- Contract: tag grammar, manifest readers and config validation unit tests.
- Renderer: role matrix, authority reachability, closed triggers, checkout
  rule and output validation (`impl_renderer_publish_spec.rs`).
- Orchestrator: generated shape, determinism, stale-declaration errors and
  the scripts executed for real (`publish_scripts_tests.rs`): tag parsing,
  byte-identical packing (GNU tar hosts), and the jq version programs
  against the Rust reader (hosts with `jq`).
- Live API: the annotated-tag script was run against a real annotated tag
  (match, target mismatch and lightweight-tag failure) and the `Required`
  check-run filter against a real commit.

## 5. Verification of git-tag mode

See §6 for the mode; its tests are `impl_orch_publish_git_tag.rs` (generated
shape, config keys, credential rejection, mixed workflow), the wait script
executed against a fake `gh` that applies the real jq program (success,
re-run supersedes failure, failure, spoofed app, unsettled, absent), the
branch gate with wrong event, ref type, repository and ref, and the
selection and pin-check unit tests in `velnor-actions-rust`.

## 6. `registry = "git-tag"` (RT)

Rust crates consumed as `git` dependencies release by tag. The same
`publish.yml` carries the Rust tag release when
`[stacks.rust.release] registry = "git-tag"`; see
[release config schema](release-config-schema.md#git-tag-mode) for the keys.

- Trigger: `push` to the default branch. With bundles declared the workflow
  also triggers on release tags; each authority job then carries
  `if: github.ref_type == 'tag'` or `'branch'`, so a push runs exactly one.
- `publish-branch-authority` (`contents: read`, `checks: read`, no checkout):
  confirms the event, ref type, repository and `refs/heads/<default>`, then
  polls the newest `Required` check run from GitHub Actions at the pushed
  SHA every 30 seconds for up to 30 minutes (job timeout 35). A settled
  non-success fails at once; running, absent or an exhausted budget fails
  closed. A re-run that succeeds supersedes an earlier failure.
- `rust-tag-release` (`contents: write`, `pull-requests: read`, environment
  `release`, no `id-token`, no registry token; `release-plz release` lists the
  pull requests of the released commit through `commits/<sha>/pulls`, which
  answers 403 without the read scope): checks out the commit, then runs the pinned
  `release-plz release` with `GIT_TOKEN`. It runs no repository code;
  release-plz reads manifests and history.
- Tags that release-plz creates with `GITHUB_TOKEN` do not trigger other
  workflows, so a tag-driven artifact (bundle) is released by a maintainer's
  tag push, never by this job.
- Generation refuses a private git source on the owner's organization that is
  not pinned by `tag`, using `cargo metadata`'s resolved sources (this covers
  dependencies and `[patch]` entries); `Cargo.lock` carries the `#<sha>`.
  Consumer repositories that only depend on producers need the same check
  without a release section: that wiring and the derived repository
  allowlist belong to item P (private sources) and are not part of this
  change.

## 7. Owner actions and blockers

- Tag rulesets on every producer that block updating and deleting `*-v*`
  tags and restrict creation to maintainers.
- A protected `release` environment in each producer repository.
- `velnor-deps` GitHub App (contents: read) for consumers to fetch bundles
  is item P; the App exists (id 5394413) and is installed on the producers.

## 8. npm packages (N)

```toml
[[publish.npm]]
id = "ui"                                    # tag ui-v1.4.0
root = "frontend"                            # a [stacks.bun] root
member = "@chainargos/ui"                    # workspace member published at the tag
pack_script = "pack"                         # root script writing the tarballs
tarballs = "frontend/.portable/tarballs"     # directory it writes <scope>-<name>-<version>.tgz into
registry = "github"                          # closed: npm.pkg.github.com
```

The version source is the member's `package.json`; the authority job reads it
through the contents API at the tagged commit like any manifest. Generation
refuses a member that is `private`, whose `publishConfig.registry` is not
`https://npm.pkg.github.com`, whose version is not a literal `X.Y.Z`, that
depends directly on a package of a `[private_sources] npm_scopes` scope
(`npm_private_dependency_unsupported`: the unprivileged build has no registry
credential yet), or whose root lacks the pack script. Ids share the tag
namespace with bundles, promotable images and released Rust packages.

Per entry, three jobs:

| Job | Authority | Rule |
|---|---|---|
| `npm-build-<id>` | `contents: read` | `if` the authority's `artifact` output equals the id. The only checkout. Installs with the pinned Bun, runs the pack script **twice**, requires the `sha256` of every tarball to be identical between the runs, checks the member's tarball (`name`, `version`, not `private`, registry) with `jq`, and uploads it with a sidecar checksum |
| `npm-attest-<id>` | `contents: read`, `id-token: write`, `attestations: write` | No checkout. Re-verifies the checksum, then `actions/attest-build-provenance` on the tarball. npm's own `--provenance` works only on npmjs.com |
| `npm-publish-<id>` | `contents: read`, `packages: write`, environment `release` | No checkout, no package manager and no lifecycle script. Re-verifies the checksum, assembles the registry document with `jq` (manifest, sha1, sha512 integrity, base64 attachment) and `PUT`s it with `curl` and the job token. An existing version fails the job |

After a package's first publish, an org admin grants "Manage Actions access"
read to every consumer repository; GitHub offers no API for it.

Tests: `impl_bun.rs` (generated chain, role permissions, fail-closed
policy), `publish_npm_tests.rs` and the role matrix in the renderer.

## 9. Maven bundles (M)

GitHub Packages Maven is repository-scoped, so no scoped CI credential can
serve a private cross-repository consumer. A Maven library therefore ships as a
release-asset bundle, the same mechanism as section 3 with a zip in place of
the tarball (*adopted per the split brief; owner may revisit*).

```toml
[[publish.maven]]
id = "processor-shared-libs"     # tag processor-shared-libs-v2.3.0, zip name stem
root = "backend"                 # a [stacks.gradle] root
```

Provenance is never optional. Repositories whose plan has no attestation
store declare `[publish] provenance = "sigstore_bundle"` (section 10). The
earlier `attest = false` / `attest_reason` opt-out is removed, and both keys
are now rejected as unknown.

The root's own convention plugin owns the bundle: `./gradlew :mavenBundle`
stages every published project and writes
`<root>/build/maven-bundle/<id>-<version>-maven.zip` (a `repository/` Maven
tree) plus `<zip>.sha256` in `sha256sum` format. The version source is the
root's `gradle.properties` (`version=X.Y.Z`); generation refuses a root that is
not a configured `[stacks.gradle]` root or whose version is not a literal. Ids
share the tag namespace with bundles, npm packages, promotable images and
released Rust packages.

Per entry, three jobs and the same authority, role split, environment, tag and
immutability rules as the other kinds (hard decision 8):

| Job | Authority | Rule |
|---|---|---|
| `maven-build-<id>` | `contents: read` | `if` the authority's `artifact` output equals the id. The only checkout. Acquires the staged helper, mints, fetches and revokes the private-source token for the lock's Maven bundles (`PrivateWiring::steps_for_gradle`; the token is revoked before any repository build logic runs), sets up Mise, then runs `gradle-run-v1` with task `:mavenBundle` (the Gradle wrapper gates, strict dependency verification and the Velnor-owned Gradle home apply as in CI). A fixed script then re-checks the checksum, requires every zip entry under `repository/` and every POM at the tag's version, and moves exactly the zip and its checksum into `$RUNNER_TEMP/bundle` for upload.  Database: when any project of the entry's root declares `services = ["postgres"]` under `[stacks.gradle.projects]`, this job carries the same hermetic local postgres service container and `VELNOR_GRADLE_DATABASE=1` that the root's CI jobs use (jOOQ codegen through Flyway needs it); roots without a declared database are unchanged. Attest and publish never carry services. Its timeout is the Gradle stack's |
| `maven-attest-<id>` | `contents: read`, `id-token: write`, `attestations: write` | No checkout. Re-verifies the checksum, then `actions/attest-build-provenance` on the zip |
| `maven-publish-<id>` | `contents: write`, environment `release` | No checkout. Re-verifies the checksum, then `gh release create --verify-tag` or `gh release upload` (never `--clobber`) of the zip and checksum, so an existing asset is never overwritten: one with the same sha256 is skipped (a release finished by hand, or a re-run), a different one fails the job |

The build job is the only one that runs a Velnor internal operation. The
publication renderer allows exactly `gradle-run-v1`, `private-fetch-v1` and
`private-revoke-v1`, only in the `ArtifactBuild` role, and requires the staged
helper path whenever one is present; every privileged role still rejects any
internal operation. No role or permission is added: Maven reuses
`ArtifactBuild`, `ArtifactAttest` and `ArtifactPublish`.

Consumers lock the zip with its sha256 in `.velnor/deps.lock` (item P) and
pin each jar a second time in `verification-metadata.xml`.

Tests: `impl_orch_publish_maven.rs` (generated chain, roles, database service inheritance, private-source
ordering, determinism, stale declarations), `publish_maven_scripts_tests.rs`
(the collect, verify and upload scripts run for real against fixture zips),
`impl_renderer_publish_spec.rs` (internal operations by role, staged helper)
and `publish_maven_tests.rs` (config grammar). Owner blocker: the same tag
rulesets and protected `release` environment as section 7.

## 10. Provenance store (`[publish] provenance`)

```toml
[publish]
provenance = "sigstore_bundle"   # default: "github_attestation"
```

One repository-level key covers every tag-driven artifact (`[[publish.bundles]]`,
`[[publish.npm]]`, `[[publish.maven]]`). Both modes sign keylessly with the
attest job's GitHub OIDC identity through GitHub's Sigstore instance. They
differ only in where the signed statement is stored and how it is verified.
`[[images]]` attestation is unchanged and still needs the attestation store.

- **`github_attestation` (default).** `actions/attest-build-provenance` writes
  the attestation to `POST /repos/{owner}/{repo}/attestations`. On a private
  repository whose plan lacks artifact attestations (an organization below
  GitHub Enterprise Cloud), this fails with "Failed to persist attestation:
  Feature not available". The attest job fails, nothing is published, and
  nothing downgrades automatically. This is the fail-closed default.
- **`sigstore_bundle` (declared fallback).** The action cannot skip the write:
  `@actions/attest` is called without `skipWrite`, the action has no input for
  it, and the bundle file is written only after the write succeeds. So the
  attest job (same id, `<kind>-attest-<id>`) gets the `ArtifactSign` role
  instead: `contents: read`, `id-token: write`, no `attestations`, no checkout.
  It sets up the pinned `gh`, re-verifies the checksum, and then:
  1. fetches the catalog's Cosign (`v3.1.4` Linux amd64, the native signer's
     pin) over HTTPS and admits it only by its exact sha256;
  2. selects GitHub's trusted root from `gh attestation trusted-root`;
  3. runs `cosign attest-blob --type slsaprovenance1` with a fixed signing
     config that names only GitHub's Sigstore instance (Fulcio
     `fulcio.githubapp.com`, TSA `timestamp.githubapp.com`) and no Rekor log,
     so a private repository's identity never reaches a public transparency
     log. The SLSA v1 predicate is built from the runner's GitHub context;
     the binding identity is the Fulcio certificate;
  4. requires a `application/vnd.dev.sigstore.bundle.v0.3+json` bundle and
     verifies it at once with `gh attestation verify <subject> --bundle
     <subject>.sigstore.json --repo $GITHUB_REPOSITORY --source-digest
     $GITHUB_SHA --source-ref $GITHUB_REF --signer-workflow
     $GITHUB_REPOSITORY/.github/workflows/publish.yml --signer-digest
     $GITHUB_WORKFLOW_SHA --predicate-type https://slsa.dev/provenance/v1`;
  5. writes the receipt `<subject>.provenance.json`: mode, reason, Sigstore
     instance, subject and bundle sha256, source repository, ref and commit,
     workflow ref and sha, run id and attempt, and the exact verify command.
     It uploads the bundle and receipt as the run artifact
     `<kind>-provenance-<id>`.

  Every publishing job downloads that artifact and requires exactly the two
  files. It re-runs the same `gh attestation verify`, and checks that the
  receipt names this subject's sha256, the bundle's sha256, and this run's
  source ref and commit. Then it records `<subject>.sigstore.json` and
  `<subject>.provenance.json` as immutable assets of the tag's release, with
  the same never-overwrite, same-sha256-skip upload as the artifact itself.

| Kind | Where the bundle and receipt are recorded |
|---|---|
| `bundles`, `maven` | `<kind>-publish-<id>` uploads them beside the tarball/zip and its `.sha256` |
| `npm` | A new `npm-provenance-<id>` job (`ArtifactPublish`: `contents: write`, environment `release`, no checkout) uploads the tarball, its `.sha256`, the bundle and the receipt to the tag's release. GitHub Packages holds no extra files. `npm-publish-<id>` then needs it and checks the receipt against the pushed bytes, so a package is never pushed without recorded provenance |

Consumer verification needs no attestation-store access:

```sh
gh release download <id>-v<X.Y.Z> --repo <owner>/<repo> --pattern '<subject>*'
gh attestation verify <subject> --bundle <subject>.sigstore.json --repo <owner>/<repo> \
  --source-ref refs/tags/<id>-v<X.Y.Z> --signer-workflow <owner>/<repo>/.github/workflows/publish.yml
```

The receipt's `verify` field holds the full command, including the source and
signer digests.

Qualification (2026-10-23): a throwaway repository, since deleted, ran this
exact cosign, signing-config and `gh attestation verify --bundle` sequence
on a hosted runner.
- The result was a v0.3 bundle with a certificate and RFC 3161 timestamp and
  no tlog entry.
- `gh attestation verify --bundle` passed, both in the run and offline on a
  workstation.
- A wrong `--source-digest` and a tampered subject both failed.
- `cosign verify-blob-attestation` cannot verify these bundles: GitHub's
  Fulcio issues no SCTs. `gh` is the verifier.

ChainArgos private-repository runs already showed GitHub's Fulcio and TSA
signing on a plan without the store: the error comes after signing, at the
persist call.

Tests: `publish_provenance_scripts_tests.rs` runs the sign, verify, receipt
and stage scripts for real against fake `curl`, `gh` and Cosign. It covers:
the pin admission, the GitHub-only signing config, the strict verify argv, the
receipt content, and refusal of an unpinned Cosign, a failed verification, an
old bundle format, a tampered subject, another commit and an extra file.
`impl_orch_publish_maven.rs` and `impl_bun.rs` cover the generated chains in
both modes and the rejected removed keys. `impl_renderer_publish_spec.rs`
covers the `ArtifactSign` matrix and the npm record ordering. `publish_tests.rs`
covers the closed, default-off key.
