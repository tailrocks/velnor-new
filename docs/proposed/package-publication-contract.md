# Package publication contract

**Status:** Proposed. Implements items RF (release and tag flow, versioning)
and RA (release-asset bundles) of the ChainArgos split design (§4.0, §4.4),
under [HD-8](../hard-decisions.md). Maven bundles (M), npm (N) and image
promotion (I.promote) build on this flow and are specified when they land.
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
| `bundle-publish-<id>` | `contents: write`, environment `release` | No checkout. Re-verifies the checksum, then `gh release create --verify-tag` or `gh release upload` (never `--clobber`), so an existing asset fails the job |

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

## 5. Owner actions and blockers

- Tag rulesets on every producer that block updating and deleting `*-v*`
  tags and restrict creation to maintainers.
- A protected `release` environment in each producer repository.
- `velnor-deps` GitHub App (contents: read) for consumers to fetch bundles
  is item P and does not exist yet.
