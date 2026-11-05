# Velnor Actions release contract (consumer-release scope amendment)

**Status:** Required consumer-release specification. The Rust publication path
remains fail-closed until its implementation and security boundaries are qualified.
This document amends the V1 generator boundary with a reviewed consumer-release
scope; it does not authorize Kubernetes,
or general deployment platforms beyond the Runner Scale Set controller (`crates/velnor-runner`).

Velnor Actions generates repository-local release-plz preparation, validation,
protected publishing, and reconciliation workflows. It does not implement a
registry uploader, a second versioning algorithm, a separate workspace
publisher, or a release daemon. release-plz remains responsible for Rust
release coordination and Cargo publication; native Cargo remains responsible
for metadata, packaging, and verification. Single-package and selected
multi-package release are two configurations of one mechanism, not separate
scripts.

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
packages = ["demo-crate"]
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

The qualified CLI catalog selects release-plz `0.4.3` for the current Rust
release implementation. The workflow MUST acquire the exact catalogued archive
and verify both archive and executable digests; it MUST NOT resolve `latest` or
recompile release-plz for each run. The archive and executable pins establish
what bytes were acquired, not independent upstream source provenance. The
recorded release tag is unsigned and the measured asset evidence and limits
are in [release-plz qualification](../implemented/release-plz-0.4.3-qualification-2026-11-04.md).
Protected use additionally requires the approved generator, Rust toolchain,
source, runner and artifact custody to satisfy the publication boundary.

Preparation and publication use explicitly different commands and generated
configurations. Do not rely on Action defaults, the Action's dual-command
default, or a dry-run result as proof of publication.

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
  set lands, the protected coordinator MUST process selected packages serially
  in dependency order, invoking release-plz for one approved package at a
  time. The generator MUST NOT upload workspace crates in an uncontrolled
  parallel matrix.

## 4. Packaging-rule graph

Dependency validation uses the real normalized published-package graph,
preserving normal/build/optional/target-specific constraints, with
development edges handled under actual Cargo packaging rules. The generator
MUST NOT invent publish cycles from the conservative CI graph.

- Reject unresolved local/`git`-only dependencies and missing unselected
  registry prerequisites during admission. A selected predecessor may be
  absent until its own approved upload; before verifying or uploading the
  dependent, establish that the predecessor is published and index-visible.
- Diagnose a missing external prerequisite with the exact requirement chain.
- Validate internal version requirements against the versions that will
  actually be published, in dependency order.

## 5. Version-group semantics

Default to independent crate versions. Native release-plz version groups
compute a group maximum, but release-plz gives inherited workspace versions
precedence. An independently versioned member can be updated even when its
source is unchanged; an inheriting member need not follow the group maximum.
The pinned release-plz implementation determines the resulting versions.

## 6. Effective release-plz configuration invariants

The checked-in release-PR preparation config MUST preserve all of the following:

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
name = "demo-crate"
release = true
publish = true
git_only = false
publish_features = ["pty"]
```

`publish_features` requests verification with the feature; consumers still
choose their own features. Release eligibility and source selection are decided
by Velnor's reviewed admission, independently of authentication and before an
upload is intended. A normal publication requires a reviewed, merged
release-plz PR; an exact-source recovery requires its separately reviewed,
bounded recovery intent. The existing protected-environment reviewer must
review the exact package-source SHA and release policy before approving, and
attest to both in the run-bound approval comment. This attestation records the
reviewer’s statement; it cannot independently prove the review’s substance.
Neither condition is inferred from release-plz's return code.

For each approved upload the coordinator renders a temporary config outside
the source checkout. It selects exactly that package and its reviewed
features, keeps Cargo verification and clean-source checks enabled, disables
release-plz's tag and GitHub Release creation, and sets `release_always =
true` plus `pr_branch_prefix = ":"`. In the recorded macOS 0.4.3 dry-run
fixtures, these settings retained the selected checkout even with an associated
release PR; those fixtures did not prove the Linux hosted upload or OIDC path.
Before enabling publication, verify that the exact upload invocation packages
the admitted checkout in the protected runner. This execution mechanism serves
both admitted normal releases and admitted recovery; it does not authorize
arbitrary commits or packages. Changing the CLI or settings requires
requalification.

The coordinator journals each upload intent before invoking `release-plz
release`, observes the exact registry version and archive afterward, and
records an independently verified result. A zero exit or skipped package is
not success. Tags and GitHub Releases are separate, create-only journaled
operations after the crate is independently verified. Verify an existing
matching object; stop on conflict, and reobserve an ambiguous result. Do not
invent repeated package flags or treat JSON output as a side-effect-free plan.

## 7. Workflow boundaries and per-job permissions

| Boundary | Trigger / authority | Result |
|---|---|---|
| Release PR | Trusted branch schedule/push under the configured preparation policy; narrow GitHub PR authority, no registry authority | Reviewed versions and notes |
| Preflight | Selected immutable source; read-only GitHub/registry queries, no publication credential | Package set, source, and validation receipt |
| Bootstrap publisher | Protected exact-source dispatch; short-lived first-publication token | Only the authorized first crate version(s) |
| Routine publisher | Qualified merged release PR; native per-crate Trusted Publishing | Authorized missing registry versions, tags, releases |
| Reconciliation | Same recorded intent; separately bounded API access | Independently checked registry, archive, metadata, completion receipt |

Rules:

- Record both the workflow-authority SHA and the package-source SHA; they may
  differ legitimately but MUST never differ accidentally through checkout
  fallback or a mutable-branch input. Preflight resolves the exact source
  release-plz will publish, not merely the event SHA. The qualified temporary
  upload configuration MUST prevent release-plz from substituting an
  associated PR head for the admitted checkout; qualify the actual upload
  path and revalidate source custody.
- Publish only from repository-local generated jobs on fresh GitHub-hosted
  runners, an explicitly preconfigured protected environment, and trusted
  eligible branch/dispatch events. Never publish from PRs,
  `pull_request_target`, or `workflow_run`. Enforce exact repository identity
  and reviewed source ancestry. Dispatch inputs reference an approved exact
  package/version/source plan, never arbitrary shell arguments, external
  repositories, or user-supplied executable content.
- Per-job permissions: the coordinator may need `contents: write` and
  `pull-requests: read` for its GitHub operations; `id-token: write` belongs
  only to the Trusted Publishing job. No `packages: write` for crates.io, no
  write-all, no `secrets: inherit`. Bootstrap-token and OIDC publication are
  distinct modes; failure of one MUST NOT silently try the other.
- No `CARGO_REGISTRY_TOKEN` in normal OIDC jobs; use release-plz's native
  exchange with no redundant auth action. Validation and release-PR jobs
  receive no publish credential; the bootstrap job receives only the required
  bootstrap secret.
- Keep Cargo verification enabled (`--no-verify`/`allow-dirty` are never a
  speed workaround). Do not execute untrusted build products, project hooks,
  ad hoc shell fragments, or PR cache contents in a privileged job; note
  `id-token` permission covers the whole job, so authentication in the last
  step is not isolation. Normal Cargo publication repackages source; do not
  invent an upload-prebuilt-crate flag.
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
  A crate owner must configure a crates.io trusted publisher for each crate,
  naming the caller repository, `release.yml`, and protected release
  environment. Remove the one-shot `new_crates` intent, switch to
  trusted-publishing authentication, and regenerate and review the workflow to
  restore normal push releases. These repository changes do not themselves
  alter crates.io settings or revoke credentials.
- The preparation config remains `release_always = false`. After the
  exact-source, identity and registry-absence gates, the bootstrap upload
  uses the same one-package temporary upload config described in §6. Its
  `release_always = true` does not bypass admission; normal publication uses
  it only after its own reviewed-release-PR admission.
- Run `release-plz release`, not `release-pr`/`update`, for the exact-version
  upload. Prevent concurrent release-PR preparation from changing the
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

The qualified release-plz 0.4.3 behavior returns early from the per-package
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
  satisfy a version check. Do not pre-create the new expected tag before
  publication if doing so makes the coordinator skip.
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
member after a partial failure. Use release-plz for publication with thin
verification/reconciliation around it; do not assume a rerun repairs all
metadata.

A later run may name a reviewed predecessor by run ID, attempt and workflow
SHA independently of the package-source recovery selection. Before any new
mutation, authenticate that predecessor's repository, source, exact run and
historical authorization; validate the complete immutable journal chain and
bind the predecessor identity into the new receipt. Reobserve every unresolved
prior operation against the registry or GitHub state. An absent or ambiguous
result MUST stop rather than trigger a blind retry. A journal entry, its
absence from an incomplete listing, or a prior success alone does not grant
new publication authority. Revalidate the current admission under the lock.

The current implementation accepts only attempt 1 for both the current run
and each predecessor. GitHub's run-level approval history does not establish
which attempt received an approval. To resume an interrupted attempt 1, make
a reviewed recovery configuration naming that exact run ID, attempt `1`, and
workflow SHA, then start a fresh approved dispatch. A rerun of the old workflow
is not a substitute. If a later attempt performed an operation, this mechanism
cannot authenticate that attempt; stop and reconcile it separately before
any further publication.

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
- `.github/release-plz.toml`: generated preparation configuration used by
  release-plz release-PR creation or validation. No second user-owned root
  config is silently created.
- `.github/velnor-release-intent.toml`: generated reviewed selection,
  authentication and tool expectations, including an optional explicit
  predecessor identity for cross-run recovery. It MUST NOT contain secrets.
- No checked-in publish-only or bootstrap-only release-plz config is emitted.
  The coordinator renders the scoped per-package upload config into the runner
  temporary directory after admission, outside the approved source checkout.
  The generator removes obsolete files it previously owned.
- Release-plz receives the GitHub credential through its isolated `GIT_TOKEN`
  binding. The trusted path uses GitHub OIDC and no stored Cargo token; the
  separately configured bootstrap path uses only its required Cargo secret.
  `--registry` is omitted for Cargo's implicit default. The approved source
  checkout and all acquired tools remain subject to the custody controls in
  §7. The preparation checkout retains full history and its branch context;
  it has only the authority needed to open a PR. Release source checkouts
  retain full history and use the exact admitted SHA.
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
reserved. Generated output passes jactionlint and the existing
workflow-security checks.

## 13. Protected rehearsal runner

A repository can enable a protected runner independently of publication:

```toml
[stacks.rust.release.rehearsal]
script = "qualification/rehearse.sh"
environment = "qualification"
reviewer_id = 12345
```

The script is a tracked, repository-relative Bash script. The environment must
already exist, require the configured human reviewer, restrict deployment to
protected branches, and disallow administrator bypass. The runner accepts only a
first-attempt manual dispatch from the protected default branch. It checks the
environment before creating a deployment and checks the policy and approval
again after the environment gate. The read-only acquisition job checks out the
exact workflow commit without persisting credentials. A separate job with empty
GitHub token permissions runs the script with a cleared environment.

The script receives `PATH`, `HOME`, `TMPDIR`, `RUNNER_TEMP`, `CANDIDATE_PR`,
`CANDIDATE_SHA`, and `SOURCE_SHA`. `SOURCE_SHA` identifies the workflow source
commit; the candidate values are unverified inputs. The script cannot use the
repository token or OIDC credentials. A private repository can use the
read-only acquisition job, but the script cannot fetch other private
repositories unless their inputs are made available without credentials.
Clearing environment variables is not an operating-system sandbox.

Set `output_bundle = true` to provide the script with
`VELNOR_REHEARSAL_OUTPUT`, the path for its evidence tar file. The file
must be a regular file no larger than 100 MiB. The executor uploads it as
untrusted data; the witness job downloads and re-uploads it under its own
artifact ID and records that ID and digest. A verifier must authenticate the
witness job and compare the bundle with independently established facts. The
bundle and any observations inside it do not authorize publication.

Set `[stacks.rust.release.rehearsal.metadata]` with `manifest_path`,
`intent_config_path`, and a qualified `rust_version` to enable an additional
metadata job. The intent configuration path is relative to the reviewed
workflow source. Dispatch also
requires `source_sha`. The protected gate captures that commit's tracked source;
the metadata job verifies the pinned Rust toolchain before and after running
`cargo metadata --locked --no-deps`. Cargo runs with empty homes from a fresh
working directory, in a non-root container with a read-only source and
toolchain. The witness records the source commit, tree, source archive digest,
metadata artifact identity, and intent configuration path. A verifier must bind the commit and tree to
independently reviewed source and validate the metadata before using it.

The metadata configuration also requires a `qualification` policy naming the
candidate repository, repository ID, default branch and required workflow, a
review-proof policy, and the historical manifest version, source SHA and
SHA-256. Set `[stacks.rust.release.rehearsal.metadata.qualification.review_proof]`
to `mode = "direct-codex"` for authenticated Codex review inspection, or to
`mode = "completion-check"` with `completion_app_id`, `completion_check_name`,
and `reviewer_app_ids` for a digest-bound completion check. The two modes never
fall back to each other. The canonical workflow binds the reviewed choice.
Qualification must compare it with fresh GitHub evidence and the independently
attested historical manifest. A candidate's provisional manifest is a separate
input and cannot establish a published release.

Cargo may contact hosts named by dependencies or registry configuration in the
lockfile. The job has no GitHub or publishing credentials, but does not enforce
a public-host network allowlist. The GitHub runner and container host remain
trusted; the read-only mounts do not protect against privileged host processes.

The source transfer verifies tracked file and link blobs against the checked-out
Git tree and preserves internal symlinks. It rejects escaping or cyclic links,
submodules, unsafe paths, more than 100,000 entries, an index larger than 16 MiB,
and a source archive larger than 100 MiB. The executor verifies the archive
digest and validates its entries before extraction. A later job records a
runner observation after the script exits successfully. A final trusted step emits
the upload action’s artifact ID and digest as a structured marker in that job’s
log. A verifier must authenticate the exact job log and bind the marker to the
artifact; artifact name or run membership alone is insufficient.

A successful runner observation establishes only the script's exit status and
the recorded workflow source and archive identities. It does **not** establish
that a candidate PR, release source, required checks, publication authority, or
crates.io OIDC exchange was verified. A separate trusted release qualification
verifier must establish those facts before any publication guard is removed.
