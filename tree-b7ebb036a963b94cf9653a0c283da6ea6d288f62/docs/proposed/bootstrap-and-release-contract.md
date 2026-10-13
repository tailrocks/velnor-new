# Velnor Actions bootstrap and release contract

**Status:** Proposed. The source-bound generator-release workflow is
implemented in the renderer; no bootstrap seed, successful hosted
qualification, or published release has been verified.

This contract defines how generated consumer workflows acquire Velnor, how
Velnor pins its tools, and how a candidate becomes a release. Consumer
workflows MUST be usable after `velnor-actions init` and `generate`; they MUST
NOT depend on Velnor-only files.

## 1. Version catalog

The generator release contains a compiled-in catalog of exact versions for
Mise, Rust/components, MBX, Nextest, GitHub CLI, CI tools, GitHub Actions, and
supported runner labels. The orchestrator passes typed catalog records to the
Mise, Actionlint, and workflow adapters. Generated consumer YAML embeds every
selected exact version, immutable action SHA, and runner label. It never
resolves `latest` at workflow runtime.

`.velnor/version-policy.toml` is Velnor-repository-only. It mirrors the
compiled catalog for freshness checks and release qualification. Dogfood CI
MUST fail when the mirror differs from the catalog. Consumer repositories do
not need this file. The generator MUST NOT create it or read it during
consumer generation.

## 2. Consumer workflow bootstrap identity

Every official Velnor release MUST publish:

1. One immutable `velnor-actions` binary asset per supported target.
2. A versioned release manifest listing each target, exact asset URL,
   SHA-256 digest, and the full source commit SHA the release was cut
   from (`commit`, required since F3).

Every consumer repository MUST commit a byte-identical copy of that
published manifest at `.velnor/release-manifest.json`. When any
`velnor-actions` binary generates consumer workflows, it reads the
committed file and validates it — schema, version, repository,
commit, supported targets, immutable URLs — with no network access
during generation; it then embeds the runner-target URL, digest, and
recorded commit in generated workflow steps (`VELNOR_ASSET_URL`,
`VELNOR_ASSET_SHA256`, `VELNOR_RELEASE_COMMIT` on `Acquire Velnor`).
The workflow MUST download that exact asset and verify SHA-256
before invoking it. The generating binary and the checking binary
read the same committed file, so identical output follows by
construction. The release manifest record is selected by exact
version; generation MUST NOT query a floating `latest` endpoint.

The committed file is supply-chain-critical: unlike `config.toml` or
the workflow YAML, it selects executed code, so review MUST verify
its version, repository, commit, artifact URLs, and digests against
the published release, not just its shape. The mechanism guarantees
self-consistency (both sides read one file) and fail-closed behavior
(absent, unreadable, or invalid files fail generation), and the
manifest validation binds every artifact URL to the exact official
release asset for its version and target. This reverses the earlier
debug-only posture, under which release builds never read the file:
a binary cannot embed a manifest containing its own SHA-256, so a
compile-time bake can never cover the seed binary, and seed and
release assets need no bake.

## 2.1. Release process: immutable tags and attestations (F3)

Code enforces the manifest shape (`commit` required, 40 lowercase
hex) and records the pinned commit in generated steps. The
generator-release workflow also builds and qualifies exact target
artifacts, assembles a same-run manifest, verifies source-bound
attestations, and publishes only through its protected dispatch job.
Consumer generation still does not verify release signatures or
attestations; the external release and seed procedure below remains
in force.

The release manager MUST follow this exact process for every
official release:

1. Cut the release from a known-good commit on the default branch;
   record that full SHA as the manifest `commit`.
2. Create the version tag (`vX.Y.Z`) pointing at that commit and push
   it exactly once. Tags are IMMUTABLE: never move, delete, or
   re-publish a version tag. A bad release gets a new patch version,
   never a re-pointed tag. Same-version asset replacement is
   forbidden for the same reason.
3. Upload one binary asset per supported target plus the release
   manifest; verify each asset's SHA-256 matches the manifest before
   publishing.
4. Produce Sigstore/SLSA provenance for every asset (build
   attestation bound to the source commit and the release workflow)
   and attach it to the release. Until generator-side attestation
   verification lands, reviewers MUST manually verify each
   attestation against the manifest `commit` before approving a
   manifest update.
5. Open the manifest-update change with the published manifest bytes;
   a second reviewer MUST confirm the committed file is
   byte-identical to the published release and that `commit` equals
   the release tag's commit.

Residual risks, explicitly scoped as future work (not silently
dropped). Same-version seed rollback: URL binding proves an artifact
URL names this version's official asset, but a committed seed
replaced at the same version (or a re-published tag upstream) is
caught only by reviewer comparison against the published release
(process step 5 until automated). Unsigned seeds: no signature or
attestation is verified by code yet (process step 4 until
generator-side Sigstore/SLSA verification lands). The remaining
follow-ups are: Sigstore/SLSA attestation verification for release
assets, a published-vs-committed comparison job proving the
committed manifest is byte-identical to the published release, and
CODEOWNERS ownership of `.velnor/release-manifest.json` so every
seed change gets security review.

Compromise rotation (X5 runbook note): a compromised seed poisons
every cache entry its runs wrote, and entries persist after the seed
is replaced. After confirming a seed compromise, rotate the cache
namespace by bumping the key-version segment (`velnor-v1-` to
`velnor-v2-`, with the same bump in every restore prefix), purge the
old-namespace entries from the GitHub cache, and re-run a trusted
default-branch build to repopulate before PRs restore again. Never
reuse a pre-rotation restore prefix after rotation.

A repository without the committed manifest MUST fail consumer
workflow generation with a diagnostic recommending installation of
an official Velnor release. It MUST NOT emit an unverified download
URL or a placeholder digest. The generated file remains deterministic
for a given generator release, config, and repository state.

## 3. Velnor-only bootstrap lock

Only Velnor's own repository uses `.velnor/generator.lock` to break the
bootstrap cycle and mirror action pins:

```toml
schema = 1

[generator]
binary = "velnor-actions"
version = "<exact-semver>"
commit = "<40-lowercase-hex-source-commit>"

[[generator.binaries]]
target = "x86_64-unknown-linux-gnu"
artifact = "<immutable-release-asset-url>"
sha256 = "<64-lowercase-hex>"

[[generator.binaries]]
target = "aarch64-apple-darwin"
artifact = "<immutable-release-asset-url>"
sha256 = "<64-lowercase-hex>"

[mise-bootstrap]
version = "<exact-semver>"
artifact = "<immutable-release-asset-url>"
sha256 = "<64-lowercase-hex>"

[[actions]]
name = "actions/checkout"
version = "<review-label>"
sha = "<40-lowercase-hex-commit>"
reviewed = "YYYY-MM-DD"
```

There MUST be one binary record per supported target. Targets, immutable URLs,
digests, action names, and pins MUST match the compiled release catalog. The
`[generator]` `commit` is required with the same 40-lowercase-hex strictness as
the release manifest (F3): the lock-backed `Acquire Velnor` records it as
`VELNOR_RELEASE_COMMIT` exactly like the manifest path. CI verifies the lock
and manifest before use. Consumer workflows MUST NOT read, require, or
generate this file.

The Velnor repository's `.mise-version` and lock's `mise-bootstrap` record
MUST pin the same exact Mise release and SHA-256. Velnor Actions never creates
or updates consumer `mise.toml`, `mise.lock`, `rust-toolchain.toml`, or
`.mise-version` files.

## 4. Candidate qualification and promotion

The bootstrap binary alone plans the workflow graph. A candidate binary MUST
NOT decide the graph that builds or promotes itself.

1. The protected lock points to an already published bootstrap release. Seed
   the first release with a manually reviewed binary built through pinned Mise
   and MBX; no Velnor binary is needed for this seed.
2. The bootstrap selects the matching host-target lock record, verifies its
   digest, and produces the plan and matrix. Candidate changes cannot alter
   these obligations.
3. Build the candidate with a fixed Cargo/MBX argument vector through Mise,
   using the compiled version catalog. Dogfood CI first checks the repository
   version-policy mirror. Do not load or modify project tool files.
4. Upload the candidate once with its digest, source commit, target triple,
   and toolchain identity. Qualification downloads this artifact and MUST NOT
   rebuild it in another job.
5. Qualify the exact candidate against generation fixtures, negative policy
   fixtures, and required V1 gates. The previous bootstrap MUST NOT have to
   reproduce newly changed candidate output. The candidate MUST NOT emit a
   workflow matrix; golden fixture files are the oracle for changed output,
   and committed digests MUST be real measured values, never placeholders.
6. A protected release job publishes only a qualified candidate. It publishes
   immutable per-target assets and the versioned release manifest, verifies
   their digests, then updates `.velnor/generator.lock` in a separate reviewed
   change. Ordinary CI uses the previous bootstrap until that update lands.
   Consumers adopt the release by committing a byte-identical copy of the
   published manifest (§2); the release job never bakes the manifest into
   the assets it describes.

The candidate build command is:

```text
mise --no-config exec rust@<exact> mr-boxington@<exact> --
  mbx build --release --locked --package velnor-actions-cli --bin velnor-actions
```

The package and binary names differ. Explicit tool arguments prevent project
Mise configuration from changing the build. CI MUST verify MBX handled the
compile. Pull-request candidates MUST NOT be promoted or replace the protected
bootstrap.
