# Velnor Actions release contract (consumer-release scope amendment)

**Status:** Proposed. All source and SDK qualification gates remain mandatory.
Current generation rejects enabled release modes while source-intent and SDK
qualification remain incomplete. Standalone parsing and filesystem tests do
not authorize workflow emission or publication.
This document amends
the V1 generator boundary with a reviewed consumer-release scope; it does not
authorize the deferred runner, Docker, Kubernetes, database, or general
deployment platform.

Velnor Actions generates repository-local preparation, anonymous packaging,
protected publication, and reconciliation workflows. Pinned release-plz
performs anonymous version and changelog updates; native Cargo performs
metadata discovery, packaging, and normal verification. Fixed compiled
helpers upload the verified immutable crate bytes and perform bounded GitHub
PR, tag and Release operations in separate fresh credential jobs. They are
closed generation operators, not a release daemon or runtime task graph.

## 1. Generator boundary and defaults

The public CLI stays minimal: `init`, `plan`, `generate [--output-dir PATH]`.
`plan`/`generate` MUST NOT become publishing commands and MUST NOT perform
account or registry mutations during generation.

Publication MUST be disabled by default. Enabling it MUST NOT auto-publish
every discovered package. Consumer intent lives in `.velnor/config.toml`
under a validated `[stacks.rust.release]` section (illustrative schema, NEW
keys — not claimed to exist yet):

```toml
[stacks.rust.release]
enabled = true
manifest_path = "Cargo.toml"
packages = ["termpane"]
environment = "release"
authentication = "trusted-publishing"
release_pr = true
tag_name = "{{ package }}-v{{ version }}"
```

The generator MUST reject unknown fields, invalid paths, ambiguous manifests,
unsafe names, contradictory authentication modes, and raw shell/YAML/`uses`
overrides. Generic product logic MUST NOT hard-code any consumer package,
organization, account, or version. `packages` accepts one name, an explicit
list, or an explicitly opted-in publishable-workspace mode; all three use the
same selection mechanism (§3). The workspace mode is selected with
`publishable_workspace = true`, which is mutually exclusive with `packages`;
an enabled section requires exactly one of the two.

## 2. Pinned release-plz toolchain selection

Qualified 2026-09-30 from official sources; reverify at execution. Pin the
executable content and the Action identity separately. No runtime `latest`
lookup and no recompilation of the coordinator on every release.

| Component | Selection | Evidence |
|---|---|---|
| release-plz CLI | `0.3.169` | `release-plz-v0.3.169` release exists; tag `63a04978…`, commit `786894b6…` |
| GitHub Action | `release-plz/action@v0.5.139` pinned to full SHA `b8d6b54b02889ff2ae2bb82e8b57c3a8fc1683a5` | Tag verified via `git ls-remote` on `release-plz/action`; peeled commit recorded |
| Binary digest | TBD-integrator | Upstream publishes no checksum asset for `release-plz-v0.3.169`; the integrator MUST measure and record the SHA-256 of the exact consumed artifact out of band before protected use |

Install-chain note: at the pinned Action tag, `action.yml` declares the
`version` input with `default: "0.3.169"` — the Action release is paired with
the CLI release, and the Action downloads the CLI from the official
`release-plz/release-plz` release assets. Generated workflows MUST still set
`command` and `version` explicitly on every invocation (§6); they MUST NOT
rely on Action defaults, and MUST NOT use the Action's dual-command default
or its partial dry-run behavior as a substitute for an explicit phase.

## 3. Selection contract

The release set derives from authoritative Cargo metadata plus declared
publication policy, validated before the first upload: every selected name,
manifest path, version, registry, and local dependency requirement.

- `publish = false` and registry restrictions are respected even when a
  caller selects the package. All-publishable-workspace mode is explicit and
  MUST NOT override them.
- CI affected-work selection is separate from release eligibility. A package
  being affected, unchanged, or merely present in a workspace is not
  permission to publish it.
- Root packages, virtual workspaces, independent versions, supported native
  release-plz version groups, and private/test/helper members are covered;
  explicit private helpers are excluded and the upload set MUST NOT expand
  implicitly.
- For a selected dependent requiring an unpublished local dependency outside
  the authorized set, generation or preflight MUST fail with the exact
  missing dependency and the needed config change. Once a reviewed expanded
  set lands, the fixed registry helper publishes in the verified dependency
  order. The
  generator MUST NOT upload workspace crates in an uncontrolled parallel
  matrix.

## 4. Packaging-rule graph

Dependency validation uses the real normalized published-package graph,
preserving normal/build/optional/target-specific constraints, with
development edges handled under actual Cargo packaging rules. The generator
MUST NOT invent publish cycles from the conservative CI graph.

- Reject unresolved local/`git`-only dependencies and missing registry
  versions before granting publisher authority.
- Diagnose a missing external prerequisite with the exact requirement chain.
- Validate internal version requirements against the versions that will
  actually be published, in dependency order.

## 5. Version-group semantics

Default to independent crate versions. Native release-plz version groups are
supported only with documented semantics: they do NOT necessarily force every
unchanged member to release. Strict lockstep MUST NOT be advertised without
an implemented and tested explicit contract.

## 6. Effective release-plz configuration invariants

The normal effective config MUST preserve all of the following:

- `release_always = false`;
- `semver_check = true` where applicable;
- `publish_no_verify = false`;
- `publish_allow_dirty = false`;
- registry publication enabled, never `git_only` mode;
- an explicit package allowlist (workspace `release = false` plus selected
  package `release = true` entries);
- a stable package-qualified tag convention (`git_tag_name =
  "{{ package }}-v{{ version }}"`), configured explicitly so tags do not
  shift when workspace membership changes.

Illustrative normal effective config (not a complete workflow or credential
setup; validate against the selected release-plz schema):

```toml
[workspace]
release = false
release_always = false
semver_check = true
publish_no_verify = false
publish_allow_dirty = false
git_tag_name = "{{ package }}-v{{ version }}"

[[package]]
name = "termpane"
release = true
publish = true
git_only = false
publish_features = ["pty"]
```

`publish_features` requests verification with the feature; consumers still
choose their own features. Preparation runs the pinned anonymous `update`
command and emits an immutable proposal. A fresh fixed GitHub coordinator
creates the release PR from that proposal. Registry and forge publishers are
separate fixed source helpers; neither invokes release-plz, Cargo, Git, or
repository programs. Do not treat coordinator output or dry-run success as
proof of publication.

## 7. Workflow boundaries and per-job permissions

| Boundary | Trigger / authority | Result |
|---|---|---|
| Release PR | Trusted branch schedule/push under the configured preparation policy; narrow GitHub PR authority, no registry authority | Reviewed versions and notes |
| Source snapshot | Fixed read-only GitHub commit/tree/blob API; no repository execution | Immutable source bytes with independently checked Git object hashes |
| Package preparation | Fresh job without publication or GitHub tokens; qualified Cargo preparation without repository execution | Original immutable crate bytes and Cargo metadata |
| Package verification | Separate fresh job without publication or GitHub tokens; normal locked Cargo verification | Success bound to the original source and package artifacts |
| Preflight | Selected immutable source; read-only GitHub/registry queries, no publication credential | Package set, source, and validation receipt |
| Bootstrap publisher | Protected exact-source dispatch; short-lived first-publication token | Only the authorized first crate version(s) |
| Routine publisher | Qualified merged release PR; native per-crate Trusted Publishing | Authorized missing registry versions, tags, releases |
| Reconciliation | Same recorded intent; separately bounded API access | Independently checked registry, archive, metadata, completion receipt |

Rules:

- Record both the workflow-authority SHA and the package-source SHA; they may
  differ legitimately but MUST never differ accidentally through checkout
  fallback or a mutable-branch input. Source reconstruction, package
  preparation, and executable verification occupy separate fresh jobs.
  Publishers consume the original prepared bytes after normal verification;
  verifier-generated metadata and archives never supply publication authority.
- Publish only from repository-local generated jobs on fresh GitHub-hosted
  runners, an explicitly preconfigured protected environment, and trusted
  eligible branch/dispatch events. Never publish from PRs,
  `pull_request_target`, or `workflow_run`. Enforce exact repository identity
  and reviewed source ancestry. Dispatch inputs reference an approved exact
  package/version/source plan, never arbitrary shell arguments, external
  repositories, or user-supplied executable content.
- Per-job permissions: the preparation coordinator needs `contents: write`
  and `pull-requests: write`; the separate forge publisher needs
  `contents: write`. Read-only artifact observers receive `actions: read`.
  `id-token: write` belongs only to the Trusted Publishing registry job.
  No `packages: write` for crates.io, no
  write-all, no `secrets: inherit`. Bootstrap-token and OIDC publication are
  distinct modes; failure of one MUST NOT silently try the other.
- No `CARGO_REGISTRY_TOKEN` in normal OIDC jobs; the fixed registry helper
  performs the qualified crates.io exchange. Validation and release-PR jobs
  receive no publish credential; the bootstrap job receives only the required
  bootstrap secret.
- Keep normal Cargo verification mandatory. A qualified preparatory
  `--no-verify` invocation may create the original artifact only before any
  repository execution; it never replaces verification. Reconstruct source
  without Git metadata, reject Cargo configuration and source escapes, and
  use sealed absolute SDK tools with an explicit environment allowlist.
  Do not execute untrusted build products, project hooks,
  ad hoc shell fragments, or PR cache contents in a privileged job; note
  `id-token` permission covers the whole job, so authentication in the last
  step is not isolation. Anonymous `cargo package --locked` performs normal
  package verification in its separate job. Fresh consumers authenticate
  all source/preparation/verification identities and original artifact
  digests, then compare the source snapshot with fresh approved API source.
  Producer identity alone never proves semantics after repository execution.
  The fixed publisher uses Cargo's qualified registry wire protocol to
  upload those exact verified bytes.
- Serialize overlapping publication sets with a stable
  registry/repository/workspace lock, never a run-unique or version-unique
  key. Never cancel an active publisher; recheck eligibility after obtaining
  the lock. Keep test/build parallelism separate.
- Do not cache publication success as a skip condition and do not cache
  credentials/`$CARGO_HOME` wholesale. Publisher caching is disabled or
  narrowly reviewed; caches never authorize publication.

Generated YAML alone does not establish a protected environment: configure
real GitHub settings and bind Trusted Publishing to the exact caller
repository/workflow/environment identity.

## 8. Bootstrap first publication

The first upload of a nonexistent crate needs a crates.io API token; Trusted
Publishing cannot create the initial trust record before the crate exists.
The bootstrap publisher is a generic, bounded exact-source dispatch
authorized by repository configuration holding exact package names, versions,
registry, and immutable source. A dispatch MUST NOT widen this record.

- Prefer one-shot bootstrap retirement after successful initial publication
  over a permanent permissive mode. Revoke the bootstrap token and remove its
  environment secret/reference once the OIDC handover is verified.
- Bootstrap uploads use only the frozen exact package/version/source plan
  after identity, source, absence and provisioned-principal gates. They do
  not enable release-plz always-release mode or alter routine preparation.
- Use the fixed registry upload helper for the exact-version upload.
  Prevent concurrent release-PR preparation from changing the
  bootstrap intent. With no prior registry baseline, historical SemVer
  comparison is inapplicable for the first version; do not fabricate a
  baseline or disable behavioral/package verification.
- Keep the named recovery owner; verify actual ownership after upload. Owner
  and trust administration use a separate authorized session, never the
  minimally scoped publish token. For future multi-crate workspaces, register
  each existing crate and detect newly added names needing bootstrap
  explicitly; never grant a permanent global token to conceal mixed
  new/existing-crate requirements.

## 9. Tag collision and registry absence

The inspected release-plz 0.3.169 code returns early from the per-package
release path when its expected tag already exists. Therefore:

- Tag-exists-but-crate-absent is a collision/incomplete release report, never
  success. A zero exit, an existing tag, or an empty release-plz result is not
  proof of publication.
- New publications use the explicit `{{ package }}-v{{ version }}` pattern.
  Verify the expected tag is absent or already points to the independently
  verified correct source. On conflict, fail closed through a reviewed naming
  policy; never silently move a tag or change the requested Cargo version.
- Historical tags and Releases that identify other sources are preserved: do
  not force-move, delete, or relabel them, and do not publish old source to
  satisfy a version check. Create tags only after independently verified
  registry publication; tag presence cannot suppress registry verification.
- Registry absence MUST be revalidated as a real 404: a failed registry
  request (network, DNS, auth, rate-limit, service failure) is not evidence
  that a crate is unpublished.

## 10. Reconciliation and recovery matrix

After publishing, perform fresh API/index/download checks with bounded
backoff: version available and not yanked, expected ownership, registry
checksum, package feature metadata, and normalized source content against
the validated plan. The actual archive checksum MUST agree with the registry;
reconcile packaging nondeterminism explicitly and never infer byte identity
solely from `.cargo_vcs_info.json` or a dry-run archive.

Recovery is per operation and per package:

| Situation | Required behavior |
|---|---|
| Upload timeout | Inspect exact registry state before any retry |
| Partial workspace upload | Retain success receipts; publish only still-missing authorized versions in dependency order |
| Crate exists, tag/Release absent | Verify source, then repair only the missing metadata |
| Tag exists, crate absent | Report collision/incomplete release, never success |
| Version exists with wrong ownership or contents | Stop; no overwrite, delete, yank, rebrand, or version switch without an explicit separately authorized remedy |
| Cancellation/crash | Retain a durable incomplete receipt and revalidate before resuming |

No broad ignore-errors, no success on an empty result, no republishing every
member after a partial failure. The fixed registry publisher authenticates
each existing version before skipping it. The separate forge publisher
repairs only missing tags and Releases after verified registry completion.

A nonsecret release receipt records schema version, release intent ID,
repository, registry, workflow source SHA, actual library source SHA,
selected package/version map, normalized file inventory, validation results,
generator/release-plz/toolchain identities, archive and registry checksums,
tag targets, release URLs, workflow run, current owners, authentication
mode, and per-operation result. It MUST NOT contain tokens, authorization
headers, credential-provider output, or private key material.

## 11. File ownership

- `release.yml`: emitted for `consumer-v1` only when
  `[stacks.rust.release].enabled = true`, rendered from typed workflow IR
  with an explicit per-job `permissions:` block. (Under
  `velnor-repository-v1` the same filename keeps its existing Velnor-internal
  meaning.)
- Anonymous preparation embeds the generated release-plz config in its
  approved source-helper environment and materializes it in an isolated
  temporary directory. No user-owned release config is read or created.
  The former `.github/release-plz.toml` and
  `.github/release-plz-bootstrap.toml` paths are retired generated content;
  bootstrap uploads do not require release-plz configuration.
- Source checkouts exist only in anonymous preparation and packaging jobs;
  their credentials are disabled. Fixed preparation config comes from the
  compiled workflow approval, never executable source configuration.
- Pinned release-plz 0.3.169 `update` accepts an explicit repository URL
  and forge without a Git token. Its output is human text; the anonymous
  helper derives a bounded proposal from changed manifest, lockfile, and
  changelog bytes. Credentialed jobs execute only frozen source helpers.
  Registry OIDC and bootstrap token modes are exclusive; the forge writer
  receives no registry credential. Stock `release`, `release-pr`, and
  `--dry-run` commands must not run with publication authority.
- The generator MUST NOT rewrite consumer Cargo manifests, Mise files,
  lockfiles, or toolchains. An explicit reviewed repository setup change may
  edit those inputs; generation itself cannot.

## 12. Verification hooks

Unit/integration tests plus deterministic workflow/config snapshots cover:
disabled release; root/virtual/subset workspaces; independent versions and a
supported version group; explicit subsets; `publish = false` helpers;
unsupported registries; unknown/duplicate selection; path escaping; missing
requirements; unpublished dependencies outside the allowlist; mixed
existing/new versions; source-vs-workflow SHA and release-PR-head behavior;
fork/prohibited-event rejection with no release privilege on PR
validation/preparation; bootstrap mismatch and token/OIDC failure modes with
no silent fallback; tag-collision-with-registry-absence; accepted-upload
timeout; index delay; mid-workspace failure; repeated invocation; queued
overlap; cancellation with partial results; safe argv/env construction;
deterministic complete `.github` generation with preview and stale-file
cleanup; pinned release-plz command/config compatibility. Mocks and a
disposable registry cover failure cases; no throwaway public names are
reserved. Generated output passes actionlint and the existing
workflow-security checks.
