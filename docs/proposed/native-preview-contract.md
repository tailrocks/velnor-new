# Native preview release and Homebrew contract

**Status:** reviewed normative configuration and receipt contract; implementation and runtime behavior remain unproven until their acceptance gates pass.

This contract extends the consumer Rust binary release in [binary-release-contract.md](binary-release-contract.md). It incorporates Jackin's native preview design and security review dated 2026-10-18. This document owns Velnor's native configuration and workflow contract.

## 1. Configuration

Preview policy is independently enabled under the existing binary release. Stable `enabled` keeps its current meaning and approval contract. Preview defaults off. `distribution.homebrew_tap` is a separate optional destination and may reference only an enabled native preview channel.

```toml
[stacks.rust.binary_release]
enabled = false
manifest_path = "Cargo.toml"
package = "jackin"
bin = "jackin"

[stacks.rust.binary_release.preview]
enabled = true
versioning = "package-preview-commit-count-sha7"
version_env = "JACKIN_VERSION_OVERRIDE"
environment = "preview-publish"

[stacks.rust.binary_release.preview.producer]
task = "release:preview-package"
source = { mise_config = "mise.toml", working_directory = "." }
runner = "xcode-27-arm64"
inputs = ["Cargo.lock", "Cargo.toml", "mise.toml", "scripts/release-preview.sh"]
tools = ["rust", "cargo-zigbuild", "jq", "tar", "zig"]
system_tools = [{ kind = "xcode", version = "27.1", build = "27A9275", sdk_version = "27.0", developer_dir = "/Applications/Xcode_27.1_Release_Candidate.app/Contents/Developer" }]
timeout_minutes = 5
build_jobs = 2
output_directory = "dist/preview"
manifest = "dist/preview/package-output.json"

[stacks.rust.binary_release.preview.evidence]
profile = "signed-sbom-v1"

[[stacks.rust.binary_release.preview.products]]
id = "jackin"
package = "jackin"
bins = ["jackin", "jackin-role", "jackin-usage-broker"]
targets = ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu"]
archive = "tar.gz"

[[stacks.rust.binary_release.preview.products]]
id = "jackin-capsule"
package = "jackin-capsule"
bins = ["jackin-capsule"]
targets = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
archive = "tar.gz"

[[stacks.rust.binary_release.preview.companions]]
id = "capsule-manifest"
path = "capsule-manifest.json"
asset_name = "capsule-manifest.json"

[distribution.homebrew_tap]
enabled = true
source = "rust.binary_release.preview"
repository = "jackin-project/homebrew-tap"
formula = "Formula/jackin-preview.rb"
updater = { task = "package:update-preview", source = { mise_config = "mise.toml", working_directory = "." }, runner = { label = "ubuntu-26.04", platform = "linux_x64", executor = "hosted" }, inputs = ["mise.toml", "Formula/jackin-preview.rb", "scripts/package-update.sh"], tools = ["jq", "python"], timeout_minutes = 5 }
environment = "tap-update"
app_id_variable = "HOMEBREW_TAP_APP_ID"
app_private_key_secret = "HOMEBREW_TAP_APP_PRIVATE_KEY"
merge = "squash-after-gates"
review_completion = { check_name = "Review completion", app_id_variable = "REVIEW_APP_ID", expected_reviewers = [{ app_id_variable = "SECURITY_REVIEW_APP_ID" }] }
```

The example Xcode version/build tuple illustrates the schema only; it is not runner qualification evidence. Consumer activation must replace it with the exact observed and qualified native runner installation.

This example records Jackin's six executable archives: four `jackin` archives, one per listed target, and two capsule archives. The capsule manifest is the sole producer-owned companion. Velnor owns checksums, SBOMs, signatures, attestations, and the canonical receipt; none may be omitted by configuration. Additional companion paths must be explicitly declared and validated. There is no wildcard inventory authority.

Configuration uses closed enums and `deny_unknown_fields`. No arbitrary workflow events, jobs, permissions, shell, URL, merge conditions, evidence booleans, or signer identities. `version_env` must match `^[A-Z][A-Z0-9_]*_VERSION_OVERRIDE$`; reject names beginning `GITHUB_`, `ACTIONS_`, `RUNNER_`, `VELNOR_`, `MISE_`, `CARGO_`, `RUST_`, `BASH_`, `LD_`, or `DYLD_`. Velnor supplies the computed version as a literal value; configuration cannot supply or evaluate it.

Producer and updater bindings are typed named Mise task projections. Each binds the exact tracked Mise configuration, working directory, tracked input closure, declared tools, runner placement, bounded timeout, and output contract. Resolve tools through the qualified setup path. During task execution disable hooks, ambient Mise config/environment loading, automatic tool installation, inherited credentials, and unsafe caches. The output manifest is untrusted input, never publication authority. No producer or updater task runs in a signer, publisher, App writer, or merge job.

The producer receives the fixed `VELNOR_PREVIEW_REQUEST_FILE` native task-handoff path in addition to fixed `VELNOR_RELEASE_SOURCE_COMMIT`, `VELNOR_RELEASE_SOURCE_REF`, `VELNOR_RELEASE_VERSION`, `VELNOR_RELEASE_COMMIT_COUNT`, `VELNOR_RELEASE_OUTPUT_DIRECTORY`, `VELNOR_RELEASE_OUTPUT_MANIFEST`, `CARGO_BUILD_JOBS`, and configured application version variables. The two output-path variables are absolute paths for task convenience; the executor resolves them beneath the canonical checkout and requires them to correspond to the repository-relative configured output root and manifest. They do not grant path authority. The producer-side verifier receives the configured manifest path relative to the output root; any safe descendant path is valid. After verification, Velnor copies that manifest to the fixed `package-output.json` name in the uploaded handoff, which the isolated signer independently verifies. CI run identity is supplied independently to signer and publisher by the eligibility job; producer output never chooses that authority.

The static request remains canonical `velnor.package-release-plan.v1` at `$RUNNER_TEMP/velnor/package-release-plan.json`. The private `execute-preview-producer-v1` operation requires a literal `payload` or `assemble` stage; no implicit serial route remains. Every product/target pair has one literal job, followed by a complete-union assembly job. Native producer-side code re-derives source policy, full history and execution identity before running the declared isolated task. Each leaf emits a distinct partial output schema; assembly preserves every admitted archive and SBOM and creates the complete handoff and opaque companions. The source-free signer independently acquires the aggregate by immutable artifact ID and mandatory service ZIP digest, validates native evidence and the complete raw inventory, and then signs. It never loads source configuration, discovers Cargo, checks out source, or runs a repository task. The full request, native proof, artifact admission and task-input contracts are specified in [native-preview-production.md](native-preview-production.md). Capsule companion semantics remain application-level validation owned by Jackin's producer and tap tests; Velnor binds exact companion bytes and digest only.

The closed producer handoff discriminator is `velnor.package-output.v1`. Its
top-level fields are exactly `schema`, `source_commit`, `source_ref`, `version`,
`payloads`, `companions`, and `sboms`. Each payload has `product_id`, `target`,
`format`, `name`, and `sha256`; preview format is `tar-gz`. Each companion has
`id`, `path`, `name`, and `sha256`. Each SBOM has `payload_name`, `name`, and
`sha256`. Payload and SBOM rows are sorted by payload name; companion rows are
sorted by ID. The producer generates the required SBOM bytes without signing
authority, and Velnor independently verifies their standard CycloneDX file
component name and SHA-256 binding. No `archives` or `archive_name` aliases are
accepted. Velnor generates checksum sidecars and signs only admitted bytes.

Every producer tool ID must resolve through Velnor's `qualified_tools` inventory for the producer platform. Updater tools must be qualified from the trusted tap source and must not be satisfied by source-controlled version installation during task execution. Inputs are explicit tracked paths, unique, safe, and include the task's Mise config; output directory and manifest cannot overlap trusted source inputs.

The native tap updater currently requires a catalogued GitHub-hosted runner. An ephemeral self-hosted executor is rejected at configuration validation and rendering because this promotion path has no native provisioning lifecycle for it.

`producer.system_tools` is optional and reuses the strict named-check native host-tool contract. Its closed kinds are `swift` and `xcode`; each declaration pins exact version and build, entries are sorted and unique, and declarations are valid only for a macOS producer runner. Runtime verification must prove the selected developer directory and executable identity against these pins before running the producer. These pins observe existing host-installed tools; they do not install tools or declare arbitrary system dependencies. Jackin's producer runs on a native macOS runner and obtains its macOS SDK from that runner's exact pinned Xcode installation. It builds the macOS targets using that SDK and cross-builds Linux targets with pinned Zig/cargo-zigbuild. The producer must not download or extract an unverified SDK archive.

`homebrew_tap` binds a repository slug, one safe formula path, source channel, updater task, credential references, fixed environment, fixed merge policy, and exact-head review completion check. App credentials are environment variable/secret names, never config values. The App installation must be target-repository scoped and have no ruleset bypass. Tap-main checks and install tests use existing named `[[checks]]` and feed the required check.

The `preview-publish` environment must contain `PREVIEW_IMMUTABILITY_READ_TOKEN`, a source-repository-scoped credential with Administration read permission only. The publisher and recovery operations receive it as `VELNOR_RELEASE_STATUS_TOKEN` for only the fixed immutable-release, main-branch, and effective-branch-rules GET routes. The workflow's `GH_TOKEN` is used only for fixed source-environment and deployment-branch-policy GET routes, source branch-head reads, original workflow-run/job and signed artifact lookup/download during retained-run recovery, and GitHub attestation lookup. These source-only reads require Contents:read, Actions:read, and Attestations:read as applicable; they do not authorize tap writes. Neither credential is exposed to producer/updater tasks; the status credential is never exposed to the signer. Both must differ from the tap App token. Missing credentials, denied reads, disabled immutability, or ambiguous responses block every unfinished publication mutation. The immutable-status response's `enabled` field must be true; owner-wide enforcement metadata is not an authorization condition. GitHub workflow `contents: write` does not grant Administration read.

Before promotion, runtime checks the source default branch is protected and that effective protections include a pull-request rule (zero required approvals is valid), resolved review threads, nonempty required status checks, force-push prevention, and deletion prevention. It reads GitHub's effective active-rules endpoint for repository/organization rulesets and the classic branch-protection endpoint, combining protections from both. A classic branch-protection 404 counts only as an absent classic rule; ruleset protections must still satisfy any requirements the classic endpoint does not. The fixed `preview-publish` and `tap-update` environments must each have no approval/custom protection rule and must use custom deployment branch policies with the single exact `main` branch pattern; `protected_branches=true` alone is not an exact main-only restriction. The preview publisher checks `preview-publish`; the App writer and merge gate recheck `tap-update` immediately before their write/merge boundary. Stable publication retains its existing human-reviewed `consumer-binary-release` environment contract, including at least one reviewer and self-review prevention.

These checks use GitHub's documented [branch protection](https://docs.github.com/en/rest/branches/branch-protection), [effective branch rules](https://docs.github.com/en/rest/repos/rules), [deployment environments](https://docs.github.com/en/rest/deployments/environments?apiVersion=2026-03-10), [deployment branch policies](https://docs.github.com/en/rest/deployments/branch-policies), and [immutable releases](https://docs.github.com/en/rest/repos/repos?piVersion=2022-11-28) endpoints. The status credential needs source repository Administration:read (also Metadata:read for effective rules); `GH_TOKEN` needs Actions:read for environment settings and retained workflow evidence, Contents:read for source identity, and Attestations:read for artifact verification.

## 2. Selection, version, and immutable identity

The initial automatic preview policy requires the resolved default branch to be
`main`, matching the fixed deployment-environment policies. Generation rejects a
different branch before emitting an unusable publication workflow. Stable manual
publication retains support for its configured default branch.

Native stable or preview publication requires `workflow.name = "CI"`, matching the canonical workflow trigger and authenticated CI authority. Configuration rejects any other name before generation; disabled native publication and generic workflows retain custom names.

Consumer policy validation evaluates stable, preview, and distribution independently. All four stable/preview enablement combinations are valid subject to each enabled policy's own fields. Distribution enabled with missing, disabled, or unsupported source channel is an error. Stable's protected `consumer-binary-release` environment and required human reviewer remain mandatory whenever stable publication is enabled; preview settings cannot weaken stable authorization.

Automatic preview starts only from `workflow_run: completed` for the canonical `CI` workflow on `main`. The dedicated native trigger does not extend the generic workflow event allowlist. Eligibility authenticates the triggering run and attempt through read-only GitHub APIs: canonical CI workflow name, path and numeric ID; source and head repository numeric identities; genuine upstream `push` event; exact default branch; completed success; latest relevant run and attempt; and one successful Required job on that attempt. Payload fields select the run to authenticate and never grant publication authority. No polling waits for CI inside a preview job.

Pinned zizmor flags every `workflow_run` independently of authentication. The dedicated native trigger carries an audited `dangerous-triggers` annotation. In zizmor 1.31.1, that annotation applies to the same rule across the entire `on` block, so its position alone cannot protect against a dangerous sibling event. Before emission, native production admission therefore requires the exact closed event inventory (`workflow_run` and manual recovery `workflow_dispatch`) and canonical CI/completed/main filters. Added `pull_request_target`, `issue_comment`, unknown events, duplicate events, or changed completion filters must fail structural admission even when the annotation is present. Generic restricted-event rejection and every other staged scanner audit remain active.

The authenticated CI head SHA, actual `GITHUB_SHA`, actual `GITHUB_WORKFLOW_SHA`, checked-out SHA, and live main tip must equal exact commit `S`. Reject stale completion events before executing producer code or obtaining signing authority. The original producing and signing event is strictly `workflow_run`, including receipt source event, Fulcio build trigger, provenance, and authenticated original-run recovery; the separately recorded upstream CI event remains `push`. Never rewrite the actual event or accept both events interchangeably. Candidate must still be the current default-branch head immediately before tag creation and publication. Candidate history must be complete and append-only. No PR head, stale run, or rerun against superseded source can publish. A bounded `workflow_dispatch` recovery accepts only an existing candidate SHA and original successful CI/build run identity; it may resume that candidate only while it remains current default-branch head. It cannot introduce a new candidate or bypass eligibility.

Given numeric Cargo base version `P` (exact `X.Y.Z`, no leading `v`, prerelease, or build metadata), exact source commit `S`, and count `N` of commits reachable from the default branch through `S` (one-based), version is `P-preview.N+<S[0:7]>`; SHA is lowercase 40-character hex. Example: `1.4.2-preview.381+0123abc`. Count uses complete Git history and is deterministic for `S`. Any source rewrite changes `S` and therefore identity. Tag is exactly `v<version>`. Velnor passes this value only through the validated application `version_env`. The numeric prerelease count advances SemVer precedence; build metadata carries source disambiguation.

Publication tag ownership is exclusive to the native channel publisher. Preview tag is `v<preview-version>`; stable preserves `<single selected Cargo package>-v<base-version>`. The publisher creates an annotated tag at exact source `S` without force; its message is the bounded canonical recovery envelope containing the exact receipt and detached signature bundle. It creates the tag object first, then rechecks eligibility before creating the non-forcing tag ref. An unreferenced object after interruption is harmless. An existing lightweight tag or an annotated tag with another envelope is never adopted, even when it points to `S`. The publisher never moves, deletes, or reuses a version tag for different bytes. Preview release is an immutable prerelease; stable retains its existing release classification and mandatory reviewer environment. Rolling aliases/tags are not release identities and cannot be used by consumers.

Generated workflow paths are fixed by channel: preview is `.github/workflows/binary-preview.yml`; stable is `.github/workflows/binary-release.yml`. The canonical receipt's workflow path and signer workflow identity must equal the selected channel's fixed path.

## 3. Product and evidence inventory

The declared set is the complete expected set, derived before task execution from Cargo metadata and the typed product/target declarations. For Jackin: three CLI binaries across four targets, one capsule binary across two Linux targets, and `capsule-manifest.json`. Each archive must have exactly its declared executables and required layout. Reject unknown package/bin/target, missing or duplicate identity, feature-gated bins without an explicit feature contract, extra/missing outputs, path escape, symlink, non-regular file, invalid archive member, architecture mismatch, version mismatch, or untracked producer input.

The closed `signed-sbom-v1` profile freezes CycloneDX JSON SBOM per archive. Exact Jackin release asset inventory contains these 29 names:

```text
jackin-aarch64-apple-darwin.tar.gz
jackin-aarch64-apple-darwin.tar.gz.sha256
jackin-aarch64-apple-darwin.tar.gz.bundle
jackin-aarch64-apple-darwin.tar.gz.sbom.json
jackin-x86_64-apple-darwin.tar.gz
jackin-x86_64-apple-darwin.tar.gz.sha256
jackin-x86_64-apple-darwin.tar.gz.bundle
jackin-x86_64-apple-darwin.tar.gz.sbom.json
jackin-aarch64-unknown-linux-gnu.tar.gz
jackin-aarch64-unknown-linux-gnu.tar.gz.sha256
jackin-aarch64-unknown-linux-gnu.tar.gz.bundle
jackin-aarch64-unknown-linux-gnu.tar.gz.sbom.json
jackin-x86_64-unknown-linux-gnu.tar.gz
jackin-x86_64-unknown-linux-gnu.tar.gz.sha256
jackin-x86_64-unknown-linux-gnu.tar.gz.bundle
jackin-x86_64-unknown-linux-gnu.tar.gz.sbom.json
jackin-capsule-aarch64-unknown-linux-gnu.tar.gz
jackin-capsule-aarch64-unknown-linux-gnu.tar.gz.sha256
jackin-capsule-aarch64-unknown-linux-gnu.tar.gz.bundle
jackin-capsule-aarch64-unknown-linux-gnu.tar.gz.sbom.json
jackin-capsule-x86_64-unknown-linux-gnu.tar.gz
jackin-capsule-x86_64-unknown-linux-gnu.tar.gz.sha256
jackin-capsule-x86_64-unknown-linux-gnu.tar.gz.bundle
jackin-capsule-x86_64-unknown-linux-gnu.tar.gz.sbom.json
SHA256SUMS
capsule-manifest.json
capsule-manifest.json.bundle
release.json
release.json.bundle
```

Each Jackin archive contains `jackin`, `jackin-role`, and `jackin-usage-broker`; each capsule archive contains `jackin-capsule`. `SHA256SUMS` contains exactly six archive digests. Each `.bundle` signs its same-name archive or manifest; `release.json.bundle` signs exact canonical receipt bytes. GitHub artifact attestations bind all six archives, capsule manifest, and receipt to source and exact authorized workflow identity. SBOMs bind to their archive digest and name. Receipt asset digests cover every listed asset except `release.json` and `release.json.bundle`; that exclusion prevents recursive self-digests. The detached bundle authenticates receipt digest. `SHA256SUMS` excludes sidecars, itself, manifest, and receipt. No other release asset is allowed. Unknown profile or attempt to disable/substitute any proof is invalid. Signer never executes repository code and accepts only validated immutable bytes from the build artifact.

## 4. Canonical receipt v2

One Velnor-owned canonical JSON document, `release.json`, identifies the entire immutable artifact set. Schema discriminator: `velnor.package-release.v2`. Encode UTF-8 JSON with deterministic key ordering and no duplicate keys; signatures bind the exact canonical bytes. No fallback parser or dual v1/v2 publication. Stable binary release migrates to this same typed artifact-set receipt while retaining its existing human-approval gate.

The exact schema has these required keys and value shapes; `additionalProperties` is false at every object level, arrays are sorted by identity, and unknown or duplicate JSON keys fail validation. Preview requires numeric `commit_count` and `source_sha7`; stable uses JSON `null` for both because the existing stable workflow is shallow-checkout `workflow_dispatch` and does not derive preview identity. `source.event` is `push` for preview and `workflow_dispatch` for stable. Stable `tag` is `<product.package>-v<version.base>`; preview `tag` is `v<version.value>`. Producer and signer are separate jobs in the same generated workflow run, so `producer_run_id` and `producer_run_attempt` equal `run_id` and `run_attempt`; validation rejects any different values.

```json
{
  "schema": "velnor.package-release.v2",
  "channel": "preview",
  "repository": {"slug": "jackin-project/jackin", "id": 123456789},
  "source": {"commit_sha": "<40 lowercase hex>", "ref": "refs/heads/main", "default_branch": "main", "event": "workflow_run", "ci_run_id": 1, "ci_run_attempt": 1},
  "workflow": {"path": ".github/workflows/binary-preview.yml", "ref": "refs/heads/main", "authority_sha": "<40 lowercase hex>", "run_id": 2, "run_attempt": 1, "producer_run_id": 2, "producer_run_attempt": 1, "signer_job_id": "preview-signer", "signer_workflow": ".github/workflows/binary-preview.yml"},
  "version": {"base": "1.4.2", "value": "1.4.2-preview.381+0123abc", "commit_count": 381, "source_sha7": "0123abc"},
  "tag": "v1.4.2-preview.381+0123abc",
  "products": [
    {"id": "jackin", "package": "jackin", "package_version": "1.4.2", "bins": ["jackin", "jackin-role", "jackin-usage-broker"], "payloads": [{"format": "tar-gz", "target": "aarch64-apple-darwin", "name": "jackin-aarch64-apple-darwin.tar.gz", "members": ["jackin", "jackin-role", "jackin-usage-broker"]}]}
  ],
  "companions": [{"id": "capsule-manifest", "name": "capsule-manifest.json"}],
  "assets": [{"name": "jackin-aarch64-apple-darwin.tar.gz", "kind": "payload", "product_id": "jackin", "target": "aarch64-apple-darwin", "subject_name": "jackin-aarch64-apple-darwin.tar.gz", "size": 1234, "sha256": "<64 lowercase hex>"}],
  "evidence": {"profile": "signed-sbom-v1", "sbom_format": "CycloneDX-JSON", "signer_issuer": "https://token.actions.githubusercontent.com", "signer_workflow_ref": "jackin-project/jackin/.github/workflows/binary-preview.yml@refs/heads/main", "attestation_predicate": "https://slsa.dev/provenance/v1", "attested_subject_names": ["capsule-manifest.json", "jackin-aarch64-apple-darwin.tar.gz", "jackin-x86_64-apple-darwin.tar.gz", "jackin-aarch64-unknown-linux-gnu.tar.gz", "jackin-x86_64-unknown-linux-gnu.tar.gz", "jackin-capsule-aarch64-unknown-linux-gnu.tar.gz", "jackin-capsule-x86_64-unknown-linux-gnu.tar.gz", "release.json"]}
}
```

`products[*].payloads` must enumerate every declared target; the abbreviated example shows one. Preview payloads use `format: "tar-gz"`; stable's shared receipt uses `format: "raw-binary"` and the existing `<bin>-<target>` asset filename. `assets` must enumerate exactly the 27 preview payload/supporting assets other than `release.json` and its own signature bundle: six archives, 18 archive sidecars, `SHA256SUMS`, and the capsule manifest plus its bundle. Every asset record has exactly `name`, `kind`, `product_id`, `target`, `subject_name`, `size`, and `sha256`; `product_id` and `target` are JSON `null` when not applicable; payload and same-name sidecars set `subject_name` to the payload, while companion and companion bundle set it to `capsule-manifest.json`, and `SHA256SUMS` uses JSON `null`. `kind` is a closed enum: `payload`, `sha256`, `signature_bundle`, `sbom`, `checksum_list`, `companion`, `companion_signature_bundle`. Payload and sidecar records carry their owning `product_id` and target; `subject_name` identifies the signed or SBOM-bound file. Inventory association is fixed by the schema, never producer-selected.

The separately published `release.json.bundle` and GitHub attestations are excluded from `assets` to avoid self-reference. The detached bundle authenticates `release.json` digest; verifier checks its exact preview signer workflow, ref, source authority and OIDC issuer. GitHub attestations are queried and verified by subject digest/name against GitHub's authenticated attestation service. The receipt fields bind repository slug and numeric ID; full source SHA/ref/branch/event and CI run attempt; workflow path/ref/authority SHA and run identities; exact package/version/bin/target/archive/member inventories; immutable version/tag; all 27 asset names/sizes/SHA-256 digests; evidence profile, signer workflow/ref/issuer, and attestation predicate/subject names. Stable uses the same schema with `channel: stable`, its own exact signer identity, and its existing human approval prerequisite. No receipt timestamp or release ID participates in identity. No fallback parser or dual v1/v2 publication.

## 5. Job boundaries and credential projection

Generated preview workflow is separate from CI and stable release. Its fixed DAG is eligibility; unprivileged producer; independent output/evidence validation; signer; immutable publisher; unprivileged tap preparation; isolated App PR writer; exact-head gated merge. Build has no OIDC, release write, or tap credentials. Signer does not execute repository code. Publisher has release contents permission only and no build/updater execution. The publisher and recovery verifier use an explicitly passed source-repository read token for GitHub attestation lookup; the pinned `gh` child receives only that token after clearing ambient credentials. Reject identical source-read and tap App tokens. Updater has no write credentials. App token exists only in isolated writer/merge boundary with target-scoped `contents:write` and `pull_requests:write`; no `actions:write`, admin, or bypass. Use clean ephemeral jobs, no persisted checkout token, hooks, filters, executable config, producer cache, or environment-file credential handoff.

Manual stable/product release remains explicitly dispatched; eligibility fails immediately if exact-commit CI is not already complete. This scheduling rule grants no new stable release authorization.

Publisher rechecks CI, exact head, environment restrictions, and immutable-release support before mutation. Formula updater receives verified receipt identity and immutable asset URLs, not arbitrary hosts. It checks current formula version and source identity and refuses downgrade or unrelated-source replacement.

## 6. Formula patch, PR, and merge gates

Preparation captures tap `main` commit `B` and checks it out read-only, without write credentials. Updater proposes a patch. Before credentials are available, a separate validator parses the formula without evaluating Ruby and independently derives canonical expected bytes from trusted base `B` by an exact allowlist of literal metadata replacements. Only configured formula's version, immutable release URLs, resources, SHA-256 values, and source identity literals may change. Every other byte, including all executable install/test logic, must remain identical. Reject ambiguous syntax, added expressions/statements, extra hunks, any other path, rename/copy, mode change, symlink/submodule, Git metadata, malformed expected identity, or a patch that differs from canonical output.

Writer revalidates captured base and exact formula blob/tree after acquiring a short-lived target-scoped App token, applies only validated canonical bytes, signs off commit, then opens or reconciles one PR. If main advances, recapture and regenerate from the new trusted base; never replay stale patch. Identical source/version/digest reconciles idempotently. Older versions cannot downgrade formula. Failed CI, invalid artifact/evidence, or unaddressed feedback leaves PR unmerged.

Merge policy is the closed `squash-after-gates`. Required tap checks must pass on the exact candidate head, including updater and actual formula/capsule installation checks. The configured trusted completion App ID and check-run name must bind repository, PR number, exact head SHA, the complete feedback snapshot, and every nonempty, unique expected reviewer App ID. It must prove every finding/comment has disposition; a quiet period or empty review list is not completion. Separately, before merge the runtime must authenticate the PR target/base, automation author and branch, canonical formula blob/tree, and source receipt against the captured candidate; those facts need not be duplicated in the review check payload. Re-fetch the completion signal, checks, reviews/comments/replies, unresolved/outdated threads, disposition evidence, repository/base/branch/author, canonical formula blob, source receipt, and head immediately before merge. The final authenticated admission observation must reject missing/mismatched bindings; a changed head invalidates every prior decision. Merge API request includes expected head SHA. Do not enable asynchronous auto-merge that can merge a replacement head.

## 7. Publication retry state machine

Retry identity is the full candidate tuple: repository ID, source SHA, workflow authority SHA/ref, original candidate workflow/run attempt, version, canonical receipt digest, and every output/evidence digest. A matching source/version alone cannot authorize changed bytes. A recovery invocation's new `workflow_dispatch` run ID is execution metadata only; receipt retains original candidate, producer, and signer identities so a retry reconstructs identical canonical bytes.

| State observed | Allowed action |
| --- | --- |
| No tag and no release | Recheck eligibility/evidence; create annotated tag object with the exact recovery envelope, recheck, create its ref at candidate SHA, then draft. |
| Tag only | Create draft only if the annotated tag points to candidate SHA and its exact envelope binds the authenticated receipt and bundle. Lightweight, malformed, or different-envelope tags fail closed. Never move tag. |
| Matching draft | Verify exact annotated tag envelope, original authenticated receipt/run identity, draft author, and every existing asset against retained original digests. Upload only missing declared assets whose original bytes/evidence remain available. Never overwrite. |
| Matching immutable published release | Verify immutable status, receipt, complete inventory, hashes, and provenance; resume tap PR reconciliation only. |
| Any mismatch, unknown draft origin, absent original bytes/evidence, stale unfinished source, or inconsistent partial state | Fail closed. Do not delete/move tag, replace asset, adopt draft, or publish. Require inspected explicit recovery. |

An already complete immutable release may be inspected after source advances, but cannot authorize an obsolete formula downgrade. Exercise interruption after tag creation, draft creation, each asset upload, release publication, PR creation, and merge request.

Tap reconciliation has separate typed preparation and promotion outcomes. The
`velnor.homebrew-candidate.v2` proof distinguishes a changed formula from an
already-current formula. The latter requires the exact independently derived
canonical bytes for this signed receipt; formula equality alone does not prove
that the native PR pipeline completed. The `velnor.homebrew-tap-pr.v2` receipt
binds the current preparation proof and either an open PR candidate or an
authenticated historical merged PR candidate. The historical candidate also
binds its original reconstructed proof, merge commit, and merge time. No old
handoff schema or compatibility parser remains.

An already-promoted candidate authorizes no branch, commit, PR, or merge write.
The independently authenticated merge boundary must prove the actual merged
PR, original formula commit and base, expected bot and branch, canonical formula
tree, signed-off commit, merge ancestry to captured tap main, exact current
formula, required exact-head checks including DCO, and trusted completed review.
A matching formula without that historical proof is blocked. A newer formula is
not an already-current outcome for an older receipt and must never be downgraded.

Changing the tap base does not discard earlier feedback. Merge admission must
independently enumerate every same-receipt PR through a bounded complete listing,
including closed PRs and deleted branches, and validate each PR's authenticated
identity and complete current feedback. Each historical PR requires its own
exact-head completion over its current snapshot; unresolved outdated threads,
general comments without disposition, missing completion, changed head, or
truncated history block promotion. Re-enumerate and revalidate this history in
the final admission snapshot. The writer does not gain check-read permissions
and cannot replace this merge-boundary proof with a self-reported history list.

Draft recovery uses an exact bounded envelope containing the original canonical
receipt and detached signature bundle, and requires the GitHub Actions bot author
identity. The publisher independently reauthenticates that original receipt,
signer run, artifact attestations, tag target, and current source eligibility
before any unfinished mutation. This binds reconciliation to the original signed
candidate; it does not claim that the release API proves which workflow run
created the draft. Replaying the identical authenticated candidate is safe;
different receipt or bundle bytes are a conflict and cannot be adopted.

## 8. Acceptance tests and release gate

Configuration acceptance: all stable/preview enablement combinations; preview default off; distribution source missing/disabled; unknown keys/enums; invalid product/package/bin/target/companion; malformed environment names; forbidden control names; unqualified task, runner, input, tool, path, timeout, credential reference; attempt to weaken evidence; inventory bounds must ensure every admitted config can fit receipt limits; stable reviewer requirement unchanged.

Generation acceptance: one separate preview workflow for enabled policy; no preview workflow when disabled; preview-only generation works while stable disabled; correct typed product inventory, version binding, job permissions and isolation; stable rendering/approval parity; no handwritten workflow ownership.

Executable acceptance: exact commit CI/run-attempt checks; deterministic version across reruns; stale source rejection at every mutation boundary; invalid/extra/missing archive outputs; unsafe members; exact signer workflow/ref/source rejection; complete evidence and receipt verification; immutable tag/release collisions; each retry state and interruption point; no overwrite; exact six Jackin archives and capsule manifest; v2 stable receipt retains `workflow_dispatch`, null preview-only commit count/SHA fields, package-prefixed tag, and required reviewer gate.

Distribution acceptance: updater executes without write credential; malicious formula code injection rejected even when metadata matches; unexpected path/mode/Git metadata rejected; canonical byte preservation; no downgrade; tap-main advancement regenerates from fresh base; idempotent PR reconciliation; App token absent outside writer; DCO signoff; wrong App/check identity rejected; failing required checks, unresolved feedback, or changed head block merge; merge request pins expected head. Tap installation verifies both CLI and capsule assets and signature identity.

These are specification acceptance requirements, not claims that the generator, publisher, App boundary, repository settings, or live upgrade pipeline already pass them. Official Velnor release and two real consecutive Jackin upgrades remain separate completion gates.

### Exact-head merge admission

The native merge gate consumes authenticated required-check and feedback
observations. The configured completion App must be distinct from the writer
App; every expected reviewer App must be distinct from the writer. Runtime
compares numeric identities, not merely different variable names. Check metadata
must come from GitHub's check-run API, never from a self-declared App ID in output.

Completion binds the full normalized feedback snapshot: all paginated review,
comment (including commit comments), reply, current and outdated thread, and
justified-disposition records. Each authenticated tap PR contains exactly one
commit above its validated base; capture therefore paginates comments on that
exact head, including for historical same-receipt PRs. The REST endpoint is
[`GET /repos/{owner}/{repo}/commits/{commit_sha}/comments`](https://docs.github.com/en/rest/commits/comments#list-commit-comments),
checked against GitHub's documentation on 2026-10-19. The snapshot includes the exact PR head and a canonical digest of those records.
A new comment or disposition change invalidates earlier completion. Unresolved
threads, findings without an independently verified accepted/rejected disposition,
and outstanding change requests block merge even if a trusted check says success.
Every configured reviewer must have terminal completion for this exact head;
missing comments or elapsed time do not establish completion.

The final admission re-fetches tap main and PR head and requires both to equal
the independently validated base and formula commit. Every active required check,
including DCO, must complete successfully on that exact commit with any required
App binding. The squash request must carry the validated head SHA as its API
precondition. A changed base requires preparation again. This contract and its
negative acceptance tests do not themselves deploy a trusted completion App or
prove any existing review provider's terminal-completion semantics; unattended
merge remains disabled until that provider is selected and verified on real PRs.

The completion check's `output.text` is exact canonical UTF-8 JSON using
`velnor.review-completion.v1`, without a Markdown wrapper. It contains
`schema`, `repository`, `pull_request_number`, `head_sha`, `verdict` (only
`complete`), `feedback_digest`, `has_unaddressed_findings` (false), and a sorted
unique `reviewers` array. Each reviewer has `app_id`, `head_sha`,
`evidence_digest`, `complete` (true), and `has_unaddressed_findings` (false).
Digests use the canonical `b3-<64 lowercase hex>` form. Unknown schemas, states,
fields, duplicate keys, noncanonical text, missing/duplicate reviewers, and a
failed actual check reject. Numeric App identity, check name, actual head and
actual result are fetched independently from GitHub; payload fields cannot
replace those facts. Snapshot and payload bind both repository and PR number so
completion cannot be replayed between PRs sharing a commit.

This payload authenticates review completion for one exact PR head. It is not
the complete merge authorization by itself: the runtime separately re-fetches
and validates target/base, PR author and branch, formula blob/tree, and source
receipt, then combines those authenticated facts with this completion signal,
the exact-head checks, and current feedback before calling the pure merge
admission contract.

The [merge REST API](https://docs.github.com/en/rest/pulls/pulls#merge-a-pull-request)
provides a head-SHA precondition, not a base-SHA precondition. Therefore tap main
must require pull requests, resolved review threads, and strict up-to-date status
checks, with no writer App bypass. GitHub documents that required status checks
alone still allow commits to be pushed directly to the protected branch; strictness
only requires branches to be current before merging. A PR-only promotion contract
must therefore authenticate the server's PR rule separately. Force-pushes and
deletion must also be blocked so tap history cannot be rewritten or removed to
evade monotonic version checks. Otherwise tap main could advance between the final
read and merge request, or be rewritten afterward, allowing an older formula to
replace a newer preview. Native merge admission fails if any of these controls is
absent or bypassable. Re-reading the base immediately before merge is necessary
but does not replace server enforcement.
If classic protection exempts administrators, native admission requires active
effective rulesets to supply PR and resolved-thread protection, strict checks,
force-push and deletion protection, DCO and completion bindings, and every
required classic check binding, with no writer bypass.
Strictness evidence requires a nonempty validated check list from that same
source; an empty strict rule cannot qualify checks supplied only by a loose
source. Absent classic protection does not invent an administrator exemption. This keeps
complementary protection sources usable without treating an exempt classic check
as server-enforced authority.
Required-check contexts must also bind an explicit numeric integration/App ID in
repository protection. An unbound context cannot establish trusted CI merely by
matching a display name, so native merge admission rejects it. Duplicate matching
checks are ambiguous and block promotion.

## Implementation qualification status

The native publisher, shared stable/preview payload model, isolated tap preparation,
App writer, and exact-head merge gate are implemented on the feature branch. This
is not an official released generator or evidence of an operational Jackin pipeline.
Focused tests cover the canonical receipt-to-promotion digest chain, raw stable
handoff, preview privilege separation, retained-run recovery admission, required
check identity, and stale source/base/head rejection. Full workspace gates and a
settled-tree independent review remain mandatory for each release candidate.
Scripted GitHub protocol tests cover interrupted PR creation and merge responses,
authenticated already-promoted outcomes, complete historical PR feedback, and
rejection of altered identity, formula, checks, or completion evidence without
duplicate writes. These tests are local qualification, not live deployment proof.

The following acceptance work remains before the feature is complete:

- Measure ruleset bypass-list visibility with the actual read-only merge App
  credential. Missing visibility blocks merge; Administration:write is not an
  acceptable workaround for the writer.
- Install a dedicated completion App, separate from the writer, reviewer Apps,
  and generic GitHub Actions identity. Its credential belongs only to a trusted
  main-only completion workflow that never executes PR code. Require its exact
  name and numeric App ID in server-side branch checks. Observe positive exact-head
  reviewer completion and test forged or edited machine-state rejection before
  enabling unattended merge. A quiet PR or failed/quota-limited reviewer is not
  completion.
- Release and verify the official generator, migrate Jackin and the tap, configure
  repository prerequisites, and prove two consecutive real prebuilt Homebrew
  upgrades, signature/capsule loading, immutable first-release availability, and
  the required negative scenarios.
