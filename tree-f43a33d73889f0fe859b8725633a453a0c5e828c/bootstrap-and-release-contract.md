# Velnor Actions bootstrap and release contract

**Status:** Proposed. No release or bootstrap behavior is implemented.

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
2. A versioned release manifest listing each target, exact asset URL, and
   SHA-256 digest.

When a release-installed `velnor-actions` binary generates consumer workflows,
its compiled-in manifest record for its own exact version MUST be validated —
schema, version, repository, supported targets, immutable URLs — with no
network access during generation; it then embeds the runner-target URL and
digest in generated workflow steps. The workflow MUST download that exact
asset and verify SHA-256 before invoking it. The release manifest record is
selected by exact version; generation MUST NOT query a floating `latest` endpoint.

A source build or binary without an official release version MUST fail
consumer workflow generation with a diagnostic recommending installation of
an official Velnor release. It MUST NOT emit an unverified download URL or a
placeholder digest. The generated file remains deterministic for a given
generator release, config, and repository state.

## 3. Velnor-only bootstrap lock

Only Velnor's own repository uses `.velnor/generator.lock` to break the
bootstrap cycle and mirror action pins:

```toml
schema = 1

[generator]
binary = "velnor-actions"
version = "<exact-semver>"

[[generator.binaries]]
target = "x86_64-unknown-linux-gnu"
artifact = "<immutable-release-asset-url>"
sha256 = "<64-lowercase-hex>"

[[generator.binaries]]
target = "aarch64-apple-darwin"
artifact = "<immutable-release-asset-url>"
sha256 = "<64-lowercase-hex>"

[[generator.binaries]]
target = "x86_64-apple-darwin"
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
digests, action names, and pins MUST match the compiled release catalog. CI
verifies the lock and manifest before use. Consumer workflows MUST NOT read,
require, or generate this file.

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

The candidate build command is:

```text
mise exec --no-config rust@<exact> mr-boxington@<exact> --
  mbx build --release --locked --package velnor-actions-cli --bin velnor-actions
```

The package and binary names differ. Explicit tool arguments prevent project
Mise configuration from changing the build. CI MUST verify MBX handled the
compile. Pull-request candidates MUST NOT be promoted or replace the protected
bootstrap.
