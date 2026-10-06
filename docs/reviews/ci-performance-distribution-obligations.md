# Distribution obligations audit

Observed 2026-10-03. Read-only GitHub inspection and shared immutable migration
evidence. No release, dispatch, publication, deployment, or approval mutation.

## Inventory and evidence

| Consumer | Migration | Prior release | Required lost behavior |
|---|---:|---|---|
| `tailrocks/velnor-apt` | #249 | Enabled, APT | Verify packages, sign staged feed, retain rollback, deploy Pages |
| `tailrocks/holla-apt` | #100 | Enabled, APT | Same family, distinct package and signing identity |
| `tailrocks/homebrew-velnor` | #8 | Disabled | Local tap preparation, strict online Homebrew audit |
| `tailrocks/homebrew-parallax` | #125 | Disabled | Local tap preparation, strict online Homebrew audit |
| `tailrocks/homebrew-ruxel` | #39 | Disabled | Local tap preparation, strict online Homebrew audit |
| `tailrocks/homebrew-tablerock` | #51 | Disabled | Local tap preparation, strict online Homebrew audit |
| `tailrocks/homebrew-holla` | #160 | Disabled | Local tap preparation, strict online Homebrew audit |
| `jackin-project/homebrew-tap` | #505 | Disabled | Repository `mise run check` Homebrew unit |

Authoritative prior declarations: removed `.github/ci/project.toml` in each
migration. APT removed `.github/workflows/release.yml` is complete, 302 lines in
each repository. Homebrew had no declared release workflow: inventing a new
publisher would not restore the audited prior contract.

Raw public evidence, kept outside this checkout:

- `/tmp/velnor-ci-performance-scope-audit/owner__repository.json`: immutable
  current SHA, migration base/head, paginated file patches.
- `/tmp/velnor-ci-performance-wave-b/dist/raw/local/{velnor-apt,holla-apt}/migration_removed_release.yml.patch`:
  complete removed APT workflow.
- `/tmp/velnor-ci-performance-wave-b/dist/raw/local/homebrew-velnor/`:
  decoded current workflow, configuration, scripts and runtime evidence.
- `/tmp/velnor-ci-performance-wave-b/dist/raw/extra/homebrew-{parallax,ruxel,tablerock,holla}/`:
  current files, complete migration patches and actual plan artifacts.

Historical native source is available locally in
`/Users/donbeave/Projects/tailrocks/velnor/crates/velnor-workflow/src/apt.rs`
and its `runtime.rs` command wrappers. Local HEAD is newer; inspect with
`git show eed474c4a1d9b071fd1b5de00c769c8997398e5a:<path>` for exact prior
runtime evidence. That commit is present in the local object database.

Distribution auditor reports actual current plans with empty packages,
obligations and matrix; Required succeeds as `no_work`. That result proves the
generated inventory, not preservation of these deleted obligations. No
retirement authorization or replacement workflow was established.

## APT family to restore

Both prior workflows use daily `17 4 * * *` and manual dispatch with `channel`
choice `stable`/`preview`, optional `version`, optional source `commit`. A stable
repository lock `package-feed-apt-${{ github.repository }}` never cancels.
Workflow permission is `contents: read`; privileged jobs restrict publication
to default branch `main` and those two events.

1. **Verify**, Ubuntu 24.04, 30 minutes, read only. Resolve stable release/tag
   commit or preview manifest/version/commit. Reject explicit preview version
   disagreement. Fetch packages and manifests; require Debian subjects. Verify
   each subject with GitHub attestation against the exact signer workflow,
   source ref and source digest. Verify package manifest/checksums, package and
   binary identity, GPG identity and stable OCI evidence. Transport verified
   inputs through a required artifact, retaining hidden evidence files.
2. **Publish**, depends on verify, 30 minutes, environment `package-feed`,
   `contents: write`. Recover both amd64/arm64 prior packages; derive previous
   pointer using publication record and candidate record digest. Preview must
   retain exactly one rollback version, or explicitly enter first-publication
   bootstrap. GPG private key/passphrase secret references occur only here.
   Sign the staged suite, preserve rollback pair, update channel/source record,
   upload required staged feed artifact.
3. **Deploy**, depends on publish, 20 minutes, environment `github-pages`,
   `contents: read`, `pages: write`, `id-token: write`. Download staged tree;
   compare live channel pointer with staged version and reject rollback. Then
   configure Pages, upload Pages artifact, deploy.
4. **Feed result**, always runs, five minutes. Verify must succeed. On main,
   publication and deployment must succeed; off main they must be skipped.
   Missing, cancelled or unexpected dispositions fail.

These are generator-owned fixed operations. They are neither arbitrary shell
configuration nor Cargo release-plz publication.

## Minimum explicit typed configuration

An APT configuration must declare enablement and source/consumer identities,
package, binary, identity directory, manifest schema, keyring path, expected
full GPG fingerprint, origin, description, HTTPS feed URL, exact signer
workflow identity, publication/deployment environments, secret names and
stable OCI verification policy. Fixed channel and architecture semantics may
remain adapter constants; branch/event predicates and permission sets must be
typed renderer invariants. Validate repository/path/fingerprint/URL identities;
reject unknown fields and incompatible authentication/role combinations.

| Field | Velnor | Holla |
|---|---|---|
| Source repository | `tailrocks/velnor` | `tailrocks/holla` |
| Consumer repository | `tailrocks/velnor-apt` | `tailrocks/holla-apt` |
| Package / binary | `velnor-runner` | `holla` |
| Identity directory | `velnor` | `holla` |
| Keyring | `velnor.gpg` | `holla.gpg` |
| Expected GPG fingerprint | `7E66E3A53F9B3B5CA61D0F53261EDAC957DEB801` | `925B7B4B807283B2391DEF2DE5BC87724E0F3E0A` |
| Origin | `Velnor` | `Holla` |
| Feed URL | `https://velnor-apt.tailrocks.com` | `https://holla-apt.tailrocks.com` |

Manifest schema is `velnor.package-release.v1`. Signer workflow is the source
repository's `.github/workflows/ci-release-package-signer.yml`. Existing secret
names are `APT_GPG_PRIVATE_KEY` and `APT_GPG_PASSPHRASE`. Both architectures and
channel source identities must remain bound through artifact transitions.

## Tool pins and environment protection

Prior runtime was `tailrocks/velnor` setup action at
`eed474c4a1d9b071fd1b5de00c769c8997398e5a`. Restore semantics in the new native
adapter; retaining this old runtime would leave the migration incomplete.
Prior external action pins provide exact historical evidence:

| Action | Prior full SHA |
|---|---|
| Checkout | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| Upload artifact | `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` |
| Download artifact | `3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c` |
| Configure Pages | `45bfe0192ca1faeb007ade9deae92b16b8254a0d` |
| Upload Pages artifact | `fc324d3547104276b827a68afc52ff2a11cc49c9` |
| Deploy Pages | `368f82528645a54fb793d4d04e342629a3f51346` |

Live GET `repos/{repository}/environments` confirms `package-feed` exists in
both APT repositories with no protection rules and no branch policy.
`github-pages` has branch policy only; neither environment has a required
reviewer or wait rule. GET `environments/github-pages/deployment-branch-policies`
allows `main` and `gh-pages` in Velnor, `main` in Holla. Administrators may bypass.
Environment binding is established; existing human approval is **not**.

### OCI source provenance prerequisite

Exact historical Velnor producer at `eed474c4a1d9b071fd1b5de00c769c8997398e5a`
declares image repository `ghcr.io/tailrocks/velnor-job-ubuntu` in
`.github/workflows/release.yml:2480`. Its GitHub provenance action attests
tarballs (`:2442`), while Debian subjects use the package signer (`:3474`).
The package signer workflow's `:59` action has only `subject-path`; it does
not attest OCI images. Platform image builds set BuildKit `provenance: true`
(`release.yml:2650`), but the assembled index job has only contents/packages
permissions (`:2702`). No GitHub OCI subject-name/digest provenance step was
found. BuildKit claims do not establish a GitHub authenticated source binding.

Historical `apt.rs:2323` accepts a record-provided image reference ending in
the record's digest; `:2371` extracts that reference's repository and inspects
it. An unsigned record plus matching sidecar and copied image labels cannot
prove source ownership. Native restoration must bind an explicit allowlisted
image repository and verify authenticated OCI provenance against the exact
source repository/ref/SHA and a producer workflow proven to emit it. Do not
guess that the Debian signer also signs OCI.

Holla source immediately before its migration, commit
`1a0dec6851f61930f9582d59fcbce16d37a6af28`, contains neither `release.yml` nor
`ci-release-package-signer.yml`. Its earlier clean-room regeneration
`efa3b7d` already declares release disabled. The APT workflow's historical
signer reference therefore does not prove an active Holla producer. Holla
OCI repository and producer attestation require explicit source-side evidence
and qualification before a consumer publication can succeed.

## Homebrew restoration

Minimum native closed workload: prepare local tap from explicit repository
checkout, then execute `brew audit --strict --online`. Required must include its
obligation independent of Cargo packages. Prior hosted runner was Ubuntu
24.04; it discovered existing `brew` or `/home/linuxbrew/.linuxbrew/bin/brew`
and failed if unavailable. Existing tap preparation scripts manipulate tap
trust and local links: inspect their semantics before implementing the fixed
operation. Preserve explicit formula/cask inputs and dependency ordering.

Current Mise tasks also declare formula/cask Ruby syntax checks, Shellcheck,
and, in several taps, package updater functional tests; Parallax additionally
declares shfmt. `RubySyntax` restores only syntax. Script updater tests and
Homebrew auditing require their own bounded typed execution semantics.
Current checked source `config/workloads.rs` lacks a Homebrew operation.
No audited prior package-update workflow authorizes introducing a publisher.

## Concrete implementation and proof tasks

1. Add closed APT contract/adapter and Homebrew audit workload. Current crates
   contain none of `apt-resolve-commit`, `apt-fetch`, `apt-verify`,
   `apt-previous-pointer`, `apt-publish`, `apt-channel-update`,
   `apt-deploy-guard`, or equivalent `InRelease` logic. Renderer-only calls to
   those absent commands cannot restore behavior.
2. Implement discovery, exact artifact/source verification, package identity,
   signing, retained rollback/pointers and deploy guards using the historical
   contract and source evidence. Keep mutation commands in their scoped jobs;
   never permit PR executables or caches to become release authority.
3. Render the typed verify/publish/deploy/result family, exact external pins,
   noncancelling lock, declared secret scopes and explicit source evidence.
   Restore Homebrew validation in the expected obligation universe.
4. Prove deterministic emission, unknown-field rejection, bad identities,
   source/version/checksum/attestation/GPG mismatch, missing architecture,
   rollback retention and stale deployment rejection, event/branch restrictions,
   least permissions, secret isolation and fail-closed results with controlled
   fixtures. Do not publish while obtaining fixture proof.
5. Regenerate consumers only after implementation and proof. Run repository
   gates; collect current hosted validation evidence separately from release
   fixture qualification. These consumers remain incomplete until their
   obligations execute or receive an explicit authorized retirement.

Blocker is missing native implementation and qualification. No unsupported
implementation waiver or successful release claim is made.
