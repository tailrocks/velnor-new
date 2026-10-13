/goal

# Finish the useful PR #12 work and retire the unnecessary owned-source system

## Mode and outcome

IMPLEMENTATION MODE. This is not another research-only review. Implement, test,
independently review, commit, push, integrate, release, migrate affected consumers,
and clean up obsolete code and refs within the scope below.

This goal supersedes the research-only operating mode in the earlier review pack.
It does not supersede repository protections, model restrictions, or evidence gates.

Deliver the smallest production-ready result that preserves real requirements:

1. Real Rust regression coverage for Velnor's mise configuration-isolation contract,
   followed by adoption of a verified fixed official mise distribution.
2. One working stock-MBX cache lifecycle, fixing the concrete PR #26 round-trip and
   disk-pressure problems where they still exist, without private tool forks.
3. Removal of unnecessary owned-source builders, publishers, qualification code,
   Foundation-only wiring, custom protocols, and their exclusive tests/dependencies.
4. Deterministic generator output, verified consumer adoption, and recoverable
   retirement of the six owned-source snapshot branches.
5. Preservation of useful independent PR #12 fixes, existing functionality, and
   other agents' work. Salvage requirements and correct code, not the whole apparatus.

Do not stop at a plan, a rewritten specification, local tests, or opened PRs. Finish
all independently executable work. An external blocker makes its affected item
PARTIAL; it does not justify abandoning unrelated work or inventing a success.

## Authority and boundaries

Use normal development branches, small commits, pushes, focused PRs, approved
release machinery, CI/qualification runs, and policy-compliant merges to complete
this scope. Retire only the specified obsolete snapshot refs after the archival,
consumer-migration, exact-head verification, and deletion gates pass.

Do not force-push, rewrite shared history, discard uncommitted work, weaken branch
protections, overwrite immutable releases, remove repositories, or delete unrelated
branches, clones, worktrees, credentials, or release assets. Do not merge foreign
upstream-root histories into Velnor main merely to preserve them.

CI migration is not permission to apply Terraform/OpenTofu, change infrastructure,
deploy business workloads to production, or run privileged untrusted code. Use
qualification fixtures and the existing authorized CI/release paths. Missing
permissions are a blocker to record, not authorization to bypass controls.

## Mandatory execution policy

Use subagents aggressively for evidence collection, implementation, testing,
security review, consumer migration, and independent verification. The coordinator
orchestrates, integrates, resolves conflicts, and checks gates; it must not claim
parallelism or reviews that were not actually performed.

For Velnor work, the coordinator and every subagent must use exactly:
  model: gpt-5.6-luna
  reasoning effort: max

Repository-specific exception: all work on jackin-project/jackin must use a separate
coordinator/team with exactly gpt-6.1-sol and reasoning effort medium, including its
analysis, implementation, reviews, and tests. The Velnor coordinator only routes
that handoff and records its result. Do not apply this exception to other repos.

Verify effective model and effort using trustworthy runtime metadata. A prompt or
model self-description is not verification. No fallback, substitution, reduced
effort, or unverified agent is permitted. Fail closed for an unavailable lane; do
not silently perform its work with another model.

Work autonomously. Do not ask the user to resolve ordinary technical ambiguity.
Assign a research/review subagent, inspect evidence, choose the smallest correct
reversible solution, record the decision, and continue. Do not invent credentials,
authority, maintainer approval, or evidence to unblock yourself.

Use distinct implementation and review/verification agents. Require independent
security and deletion-blast-radius review for executable downloads, cache trust,
process/environment boundaries, archive extraction, publication, and ref retirement.

Commit small coherent verified changes frequently and push regularly. Prefer one
integration branch per repository; reuse relevant existing PRs rather than opening
competing work. Merge main rather than rebasing shared history. Serialize Git-index,
commit, integration, and final-generation operations. Parallel agents need disjoint
file ownership or isolated disposable worktrees; they must not share mutable
compiler output/cache directories unsafely. Bound CPU, disk, Git, and network load.

Read current AGENTS.md and repository-specific instructions before acting. Address
all relevant review comments and unresolved threads before merging; record evidence
for accepted fixes and explanations for rejected suggestions.

## Inputs and historical anchors

Primary repository: https://github.com/tailrocks/velnor-new
PR under review: https://github.com/tailrocks/velnor-new/pull/12
Related stock-cache repair: https://github.com/tailrocks/velnor-new/pull/26
Accepted mise change to revalidate: https://github.com/jdx/mise/pull/13926

Read these earlier research artifacts when available:
  owned-source-review.md
  owned-source-spec.md
  owned-source-inventories.md
  owned-source-inventory.json

Their absence is not a reason to stop: the scope and execution requirements below
are self-contained. Their findings are historical evidence, not current truth or
authority to delete. The review lacked fresh-clone ancestry proofs, complete
all-ref/consumer searches, packaged-action execution, and hosted qualification.

Historical reviewed identities; refresh and validate every one before relying on it:
  velnor-new/main       5a946c33cf005777feab2bc91fa4aa8e01dd58f4
  PR #12 head           d94fe9cefa6a0b97f00d235191c2d3142d1af6ff
  PR #12 branch         perf/cache-selection-qualification
  PR #26 head           1ecd84d1f6320e9a0209968c2057952b9363c8a4
  tar-absolute          5b309009599ce11173830045e237fef6900d6b42
  accepted mise merge   dfe74a90b41603625ee6aabecb42f14a1f5eb0f6

True upstreams to verify:
  jdx/mise
  jdx/mr-boxington
  jdx/mr-boxington-action
  obi1kenobi/cargo-semver-checks
  actions/cache

Do not reset to these anchors, treat them as current heads, or duplicate work that
has since landed. Fetch main, refs/pull/12/head, PR #26, all Velnor branches/tags,
and relevant open PR heads. Record timestamps and full SHAs. Recheck moving refs
before integration, rollout, and deletion. Reconcile only the changed evidence
when a head moves; do not restart an endless whole-project research loop.

## Phase A — close actionable evidence gaps and choose the integration path

Run parallel evidence lanes while safe additive Phase 0 tests are developed. No
deletion passes until its caller/dependency coverage is complete; no snapshot
retirement passes until the full scoped consumer audit is complete.

Inventory PR #12 by feature and file, not just by commit. Separate useful generic
correctness changes from owned-source scaffolding. Check main, other branches,
open PRs, and existing releases for equivalent implementations before editing.
If PR #12 is still the appropriate integration branch, narrow it in place without
rewriting history. Otherwise use one focused branch from current main, selectively
carry over useful changes, and close the superseded PR only after every item has a
recorded disposition. Preserve and coordinate PR #26 and independent runner fixes.

In fresh upstream scratch clones, before importing foreign snapshot objects, prove
upstream objects and peeled tags with git cat-file and git rev-parse. Then inspect
parents, original fork points, later upstream merges, and full custom diffs. Record
authored changes separately from vendored tests and generated bundles. Validate the
15 historical custom commits in Appendix A and account for newly discovered ones.

Load current scope.json and compare it with the reviewed 46-consumer inventory.
Audit their union plus newly discovered consumers. Resolve renamed/transferred
repositories through actual metadata; an old name, missing access, failed request,
or empty indexed search is not a zero-consumer result.

Search every fetched Velnor ref, including generated outputs and tests. For each
consumer inspect current default-branch workflows/configuration, reusable/composite
actions, invoked scripts, and relevant open-PR/branch heads; follow all positive
indirect references. Search full snapshot SHAs, owned-source branch/ref names,
script names/import stems, Foundation, MISE_OWNED_CARGO_WRAPPER,
owned-cache-transport, comparison-state, expected-binary-sha256, velnor-plan,
velnor-compare, and SDK/quarantine identifiers. Use bounded git grep/rg, not repeated
unbounded filesystem scans. Search actual files, not only GitHub's code index.

Classify each reference as passive documentation, mock, real-code test definition,
staged executable workflow, or observed runtime invocation. For observed execution
record run ID, attempt, repository/head/merge SHA, job, command, and relevant logs.
A live scheduled/manual invocation is still a consumer even without retained logs;
do not delete a credible declared runtime contract merely because logs expired.

Verify consumer-v1 versus VelnorRepositoryV1 using the actual generator and effective
configuration. Do not infer omitted policy values from another repository/version.

For every custom feature choose one evidence-backed disposition:
  KEEP-IN-VELNOR: necessary behavior owned by an existing Velnor crate/module.
  ADOPT-UPSTREAM: necessary behavior supplied by verified official upstream code.
  MINIMAL-UPSTREAM-PR: indispensable missing behavior with no adequate Velnor seam.
  DELETE: unnecessary feature whose callers are removed/replaced or absent.
  TEMPORARY-HOLD: a concrete unresolved dependency, named owner, next probe, and
                  expiry event; not an indefinite staging destination.

A feature's own tests do not by themselves create a product requirement. Conversely,
a real test must not be mislabeled a mock. Investigate any missed consumer first;
suspend only the affected deletion while resolving its requirement.

Try concrete config/process/renderer/composite/stock-API alternatives before changing
upstream. Name the exact seam and prove behavior. Do not claim ordinary cache saving
reproduces semantic usefulness/lineage, or that metadata wrapping reproduces private
semver lint behavior. Do not preserve unused sophistication merely to avoid deletion.

## Phase 0 — port retained regression coverage to Rust before removing Python

Extract only necessary stock-mise no-config/miserc tests from:
  scripts/owned_mise_qualification.py

Proposed target, adjusted to the current real harness:
  crates/velnor-actions-mise/tests/impl_miserc_isolation.rs

Reuse the existing velnor-actions-mise process/environment boundary, integration
harness, fixture conventions, and pinned toolchain. Do not port the receipt engine,
owned wrapper/shim/banner, fake MBX, source capsules, or candidate orchestrator.

Execute real official mise binaries and prove:
  - flag-only and environment-only no-config selection;
  - --version and exec operations;
  - repository-root and nested working directories;
  - malformed project, global, and system miserc discovery layers;
  - a control without no-config that actually reaches the malformed configuration;
  - an affected official release exhibiting the regression;
  - a selected fixed official binary passing the retained contract;
  - Velnor's production --no-config --no-env --no-hooks boundary remains enforced.

Keep flag-only and environment-only tests genuinely distinct: the latter must not
pass merely because the helper silently adds the flag. Use test-only supported
entrypoints where needed, without weakening the production process API. Isolate all
fixtures; never modify the developer's actual home/system configuration. Use existing
safe overrides or disposable containers for system-level discovery tests.

Record release/source identity and actual executable digest. Known-broken controls
must be asserted expected failures, not a permanently red normal suite. Missing
binaries, skipped platforms, fake executable output, and tests not selected by CI
are not qualification passes. Cover every applicable supported platform or explicitly
leave that qualification incomplete.

Audit test_git_optional_locks.py and all other proposed test deletions for retained
generic requirements. Port any necessary regression into its existing owning Rust
crate in this same Phase 0. Add only the smallest reusable helper needed; do not
create a new qualification framework. Keep this slice independently reviewable.

## Phase B — adopt fixed official mise, not the owned fork

Re-query official releases. Prove the selected source contains the accepted
jdx/mise#13926 fix or a verified equivalent, then run Phase 0 against its distributed
binary. The historical review found a merged fix but an inspected release without
it; neither a PR merge nor a version string is deployment evidence.

Update the real version catalog, bootstrap/action selection, checksums, freshness
authorities, and generated fixtures together. Inspect current equivalents of:
  crates/velnor-actions-mise/src/catalog.rs
  crates/velnor-actions-mise/src/command.rs

Do not submit the same upstream fix again. Do not retain the broader owned mise
wrapper/shim/banner simply because one small part was useful.

If no fixed official binary exists, continue independent work. Consider a minimal
Velnor-side workaround at IsolatedCommand::mise_exec, mise_install, with_cwd, and
repo_task only after proving it against the same tests. Neutral CWD alone is not
proof: validate project/global/system discovery, nested Cargo, environment policy,
and tool identity. Do not start a private distribution pipeline as a workaround.

Distinguish source cleanup, a qualified temporary workaround, and official release
adoption. Pending official adoption remains explicitly PARTIAL with a pinned
upstream dependency and the exact release event that resolves it.

## Phase C — finish the stock-MBX cache lifecycle using existing work

Refresh PR #26 and inspect its current implementation, tests, reviews, and CI. Reuse
or correct that work; do not open a competing implementation or reintroduce a bug
already fixed after the research head.

Historical seam to inspect:
  crates/velnor-actions-workflow-renderer/src/mbx_bundle.rs

The research identified a potential mismatch between stock-action restore/import
inside the MBX store and Velnor's separate save of an external bundle. Verify current
packaged behavior before concluding that defect remains.

Choose ONE coherent supported contract:
  A. Preserve the stock action's complete compatible restore/import/export/save path.
  B. Have Velnor render both stock actions/cache/restore and actions/cache/save around
     explicit official MBX import/export of the same external bundle path.

Specify path sets, resolved cache version, key/prefix schema, compression, archive
format, platform/toolchain compatibility, import/export ordering, and write trust
as one contract. Matching keys alone are insufficient. Avoid duplicate implicit
post-action saves when Velnor owns the save step.

Use verified immutable official tool/action pins. Keep logical cache paths stable
between writer and reader; use keys rather than random cache paths for qualification
nonces. Also preserve genuine job isolation: never share a live mutable store solely
to force path equality. Verify actual cross-job path/version behavior on the supported
runner families instead of assuming matching YAML expressions prove compatibility.

Preserve the approved production write policy. PR/fork jobs must not populate trusted
production caches. Manual qualification, when needed, must use an explicitly isolated
qualification namespace and a trusted fixture; it must not loosen production gates.

Handle misses with safe uncached builds. Reject corrupt/incompatible cache input and
recover without consuming unsafe data. Do not save failed/empty exports. Test prefix
restores, cancellation, parallel jobs, old-cache isolation, and disk/inode exhaustion.

Avoid recursive deletion. Any necessary store removal requires canonical containment,
exclusive job ownership, stopped consumers, and symlink/race review. A name containing
'mbx' is not deletion authorization. Never remove a shared or unrelated directory.

Qualification must include actual generated YAML and two fresh hosted jobs using the
real cache service: writer saves; independent reader restores, imports, and shows
object/compiler reuse. A cache-hit output, mocked cache, same-process round trip,
or old-head green badge is insufficient. Use the repository's existing qualification
mechanism rather than inventing another publication/receipt service.

Measure the affected ChainArgos workload separately from a tiny probe. Record cold
and warm setup/build/import/export/upload durations and peak bytes/inodes. Eliminate
avoidable overhead introduced by this change; flag scoped pipelines over the user's
120-second target without hiding work or skipping checks. Do not expand this task
into unrelated CI architecture rewrites.

Do not activate either owned action snapshot with MBX ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81
in comparison mode. The historical source contracts were report v1 versus v2. If no
necessary comparison consumer survives, delete the feature rather than repair an
unused protocol. If one survives, choose one explicitly versioned semantic contract,
inspect/rebuild the actual committed dist bundle, and test real producer-to-packaged-
action execution. Do not blindly accept both versions or rename fields as a fix.

## Phase D — remove unnecessary Python and Foundation feature closure

The target is deletion of unnecessary jobs, not a Rust translation of the whole
pipeline and not a repository-wide ban on Python.

After Phase 0 and exact caller/import closure, delete these principal scripts unless
new evidence proves a retained requirement:
  scripts/analyze-ci-performance.py
  scripts/collect-ci-performance.py
  scripts/bootstrap-owned-tool-builder.py
  scripts/build-owned-tool.py
  scripts/download-owned-tool-candidate.py
  scripts/immutable_publication.py
  scripts/owned_tool_behavior.py
  scripts/owned_tool_qualification_evidence.py
  scripts/owned_tool_source.py
  scripts/publish-owned-tool-artifacts.py
  scripts/publish_owned_source.py
  scripts/qualify-owned-tool.py
  scripts/source_proof_capsule.py
  scripts/source_publication.py
  scripts/source_publication_records.py
  scripts/source_qualification_execution.py
  scripts/source_release_policy.py
  scripts/source_semver_capsule.py
  scripts/stage-owned-tool-source.py

Delete owned_mise_qualification.py after its selected retained tests have moved and
passed. Audit and remove exclusive fixtures/tests listed in Appendix B. For any
newly retained necessary script, port its required behavior to the existing owning
Rust module BEFORE deletion. KEEP-PYTHON requires a concrete technical environment
where Rust cannot run, or genuinely unavoidable short-lived external-tool glue with
a named deletion event. Convenience and sunk implementation cost are not defenses.

Preserve unrelated freshness, OCI, release, Git/process, runner, and workload support.
In particular, do not remove Python needed by existing check-freshness.sh or the
independent tar-absolute/ChainArgos runner-image requirement as collateral cleanup.

Default Foundation outcome: remove it if the audit establishes no retained runtime
requirement. Trace and remove its whole dedicated feature closure, including current
equivalents of:
  .github/workflows/foundation-qualification.yml
  crates/velnor-actions-orchestrator/src/foundation_qualification.rs
  crates/velnor-actions-orchestrator/src/foundation_qualification_source.rs
  crates/velnor-actions-workflow-renderer/src/foundation_qualification.rs
  its template, dedicated CLI/registration/configuration, tests, and source tuple

The reviewed source graph could invoke real native/Python processes. Do not call it
prose-only, replace its subaction with ordinary actions/cache, or delete just the YAML
while the Rust generator can recreate it. Preserve shared primitives used elsewhere.

If a real Foundation requirement survives, implement only that requirement in the
appropriate existing Velnor Rust crate, with real tests and a caller migration. Do
not retain an actions/cache fork to host Velnor-specific behavior. A standalone repo
requires a demonstrated architectural boundary, not merely a place to move a fork.

Remove exclusive Cargo dependencies/features, candidate routes, dead manifests,
SDK admission surfaces, obsolete release tasks, and documentation instructions with
the feature. Confirm usage before deleting anything shared. Never use broad globs
such as rm scripts/*.py or delete unrelated generated workflows.

Preserve useful historical outputs as clearly passive evidence with source identity
and limitations. Historical JSON does not justify retaining an executable publisher.
Do not alter immutable release assets or mislabel source-only publication as tested
binary provenance. Add focused absence/parity tests against reintroducing these
retired dependencies; do not prohibit unrelated legitimate future native features.

## Phase E — upstream only newly proven necessities

For an indispensable requirement not met by official tools or a proven Velnor seam,
inspect the upstream's current CONTRIBUTING, tests, platforms/MSRV, changelog rules,
comparable accepted PRs, and relevant scope rejections. Propose and implement only
the smallest generic core, using upstream conventions and real regression tests.
Split independent concerns into independently mergeable PRs. Keep Velnor identity,
policy, wrappers, source receipts, and publication mechanics out of the generic fix.

Open an upstream PR only for that demonstrated missing behavior; do not duplicate
accepted work or claim future acceptance as fact. A temporary contribution branch
is staging with an upstream release/adoption expiry event, never a permanent private
fork. If upstream will not accept the core, use a justified Velnor-owned solution or
remove the optional behavior; do not retain an unmaintainable fork by default.

## Phase F — integrate, release, regenerate, and verify consumers

Make narrow reviewable slices: Phase 0 tests; official mise adoption; stock-cache
correction coordinated with PR #26; Foundation detachment; pipeline/dependency
retirement; consumer rollout; final ref retirement. Use logical commits on the
integration branch and only the separate PRs needed for independent ownership or
mergeability. Do not block safe source retirement on an unrelated large redesign.

Read current repository commands and run the pinned toolchain/harness equivalents
of formatting, Clippy, targeted and full Rust/Nextest tests, dependency/security
checks, freshness, and generator parity. Do not invent a test invocation or treat
an unregistered integration-test file as executed coverage. Preserve substantive
checks and policy semantics while removing obsolete feature-specific checks.

Generate repository-policy and consumer-policy outputs from the same candidate
twice; require byte-identical second output and no second-run diff. Modify generator
source/typed configuration, never hand-maintain generated YAML. Ensure retired
Foundation output is actually removed by the generator's managed-file lifecycle.

Publish a new immutable generator artifact through existing approved machinery;
record exact source SHA and artifact digest. Never overwrite a used release/tag or
change only version comments. Verify packaged behavior, not just source tests.

Audit all scoped consumers and migrate every affected consumer from that exact
artifact. Begin with jackin-project/jackin using its required separate agent team;
then the remaining jackin-project cohort, tailrocks, and ChainArgos last. Include
deep verification of:
  jackin-project/jackin
  tailrocks/velnor
  tailrocks/parallax
  tailrocks/github-terraform
  tailrocks/velnor-actions-fixture
  ChainArgos/java-monorepo

Use current scope.json plus the historical scope in Appendix C; do not silently
omit a renamed or inaccessible repository. A genuinely unaffected repository needs
an evidence-backed NO-CHANGE result, not a fabricated PR or a needless commit.

For each consumer record canonical repository, reviewed SHA, effective policy,
generator digest, before/after pins, deterministic regeneration result, changed-file
scope, applicable checks, and exact PR and post-merge main CI run/attempt/SHAs.
Preserve existing Rust, Java, Tofu, desktop, runner-selection, release, and security
behavior. No consumer-specific hacks in the generic generator.

Fix failures caused by this work. Investigate apparently unrelated failures enough
to establish causality; do not silently claim a red or unverified main is green.
Follow applicable merge policy. Any explicitly required waiver is a separate status,
never a successful CI observation. Recheck the actual merged result.

## Phase G — archive and retire snapshot branches safely

Retirement scope is these exact historical branches under tailrocks/velnor-new:
  owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96
  owned-source/mbx/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81
  owned-source/mbx-action/c3cbe8e56ccb4727624df45022357f49d2953075
  owned-source/mbx-action/62ec0713473dffeab46884b7c03906042794e696
  owned-source/semver-checker/583dddce84706786fc54c41a2c768c28a09c65fd
  owned-source/cache-action/8758d976a1b25eb387f48aa04ea86f57739b84cf

For EACH branch, before deletion:
  1. Re-fetch its actual tip and verify no newly added work or consumer was missed.
  2. Finish caller migration/removal and record the actual resolving merged SHA.
  3. Prove no retained active code/workflow/release-build path requires its ref or
     snapshot-only behavior. Identify historical reproducibility dependencies too.
  4. Preserve exact Git history in a verified bundle, account for required submodule/
     external source objects, restore independently, and verify expected commits/trees.
  5. Store the archive durably through existing owner-controlled storage, record its
     digest/retrieval instructions, and preserve existing immutable source releases.
  6. Obtain independent deletion review and record owner, evidence, and rollback.

A scratch-only bundle is not durable archival. A tarball is not Git-history proof.
Do not infer foreign commit reachability from release target_commitish; peel tags.
Branch deletion is not instant object deletion, but undocumented object retention
is not a recovery strategy. If historic SHA-based action resolution still depends
on the repository retaining those objects, provide a durable retention plan before
removing the last retaining ref.

Once all gates pass, delete the named obsolete branch and record the result. Never
delete main or unrelated refs. Normal removal of this goal's merged working branch
is permitted only after confirming it contains no unintegrated work.

A blocked retirement gets a concrete TEMPORARY-HOLD: exact dependency, owner, next
probe, resolving event/ref, and a decision checkpoint no later than seven calendar
days from execution start. The date requires reassessment, not automatic deletion.
Do not leave unowned indefinite staging or fabricate completion to meet a deadline.

## Completion and final evidence

Keep the decision ledger and evidence artifacts small and passive; reuse the
existing documentation/test-result conventions rather than creating a new framework.

The independent verifier must check the final tree/artifact, not an earlier reviewed
head, and confirm:
  - every custom change and Python file has a justified final disposition;
  - retained mise regression coverage actually executes with real binaries;
  - official fixed mise is deployed, or its adoption is explicitly incomplete;
  - the retained stock-cache contract passes real writer/reader and affected-workload
    qualification, including trust, path/version, failure, and disk-safety cases;
  - no required coverage or unrelated correctness fix was lost in deletion;
  - generation is deterministic for both relevant policies;
  - active runtime/generated/publication paths no longer reference retired snapshots;
  - all scoped consumers are migrated or proven unaffected, with exact-head results;
  - each snapshot is retired recoverably or has an explicit unresolved hold;
  - no private fork or unnecessary owned-source runtime remains as the end state.

Report separately: SOURCE-CLEANUP, OFFICIAL-TOOL-ADOPTION, CACHE-QUALIFICATION,
CONSUMER-ROLLOUT, and SNAPSHOT-RETIREMENT, each COMPLETE or PARTIAL.

Overall COMPLETE requires all mandatory gates, applicable green PR/post-merge CI,
actual release adoption, and resolved snapshot end states. Missing access, a pending
release/upstream merge, an unrun mandatory test/platform, an unverified consumer,
or an unresolved retention dependency means the relevant item and overall result
remain PARTIAL. Continue all independent work before stopping.

Final report: implemented/removed features; per-change/Python decisions; commit and
PR links; final source/artifact identities; exact test/run evidence; consumer matrix;
branch archive/retirement ledger; rollback procedures. For every remaining blocker,
state what is unknown, evidence that settles it, owner, and cheapest next action.

Do not claim a command, model setting, subagent review, test, release, deployment,
upstream acceptance, or deletion occurred without evidence. The objective is a
smaller working system, not a green-looking report or a renamed version of the
same unnecessary infrastructure.

## Appendix A — historical custom-commit inventory to disposition

Re-derive ancestry and full deltas; these are starting identities, not verdicts.

MISE:
  dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96  no-config core plus owned wrapper/shim/banner
MBX:
  1ca12eb48391061a75e97d32e5b06fedf8a6253d  usefulness/comparison/retention
  2f324e89af509a1c61d802cf23741a04d7510b57  merge of official upstream update
  ee250ac37654a4cfbb55b6cd470f2a257204bbe9  Cargo root-pair restore
  e07c07cfec773897e5c439043901e661ad3f7b45  native ownership/compiler events
  29865b5a18e3414084ff5db27d1da455772b4c34  vendored behavioral-test closure
  ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81  lineage/durable state/report v2
MBX action:
  f053f215866af0ddb6d2f32ecc19d45a5d25edc2  executable selection/comparison
  198f0d1a538d34a91d7692f8f302259643c0f737  merge of official upstream update
  06f353d41002af758d27490164f53c82e2165637  caller-bound binary digest
  c3cbe8e56ccb4727624df45022357f49d2953075  protected-default cache-save policy
  62ec0713473dffeab46884b7c03906042794e696  late baseline/export-group binding
Semver:
  d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a  supplied-document metadata/lint feature
  583dddce84706786fc54c41a2c768c28a09c65fd  fork CI push restriction
Cache:
  8758d976a1b25eb387f48aa04ea86f57739b84cf  archive/SDK changes plus Foundation

Expected direction: adopt the accepted mise core from upstream; keep necessary
policy/process/cache fixes in Velnor; remove unused foreign additions. No custom
commit is a mandate to cherry-pick its entire diff.

## Appendix B — support fixtures/tests to audit with their owning feature

  scripts/owned_tool_execution_test_fixtures.py
  scripts/owned_tool_publication_test_fixtures.py
  scripts/test_ci_performance_analysis.py
  scripts/test_download_owned_tool_candidate.py
  scripts/test_git_optional_locks.py
  scripts/test_owned_tool_build.py
  scripts/test_owned_tool_execution_evidence.py
  scripts/test_owned_tool_qualification_evidence.py
  scripts/test_publish_owned_tool_artifacts.py
  scripts/test_qualify_owned_tool.py
  scripts/test_source_proof_capsule.py
  scripts/test_source_publication.py
  scripts/test_source_publication_records.py
  scripts/test_source_qualification_execution.py
  scripts/test_source_release_transaction.py
  scripts/test_source_semver_capsule.py

Deletion remains conditional on preserving any generic surviving regression.

## Appendix C — historical 46-consumer scope, to reconcile with current scope.json

  jackin-project/jackin
  jackin-project/jackin-agent-smith
  jackin-project/homebrew-tap
  jackin-project/jackin-role-action
  jackin-project/jackin-sentinel
  jackin-project/jackin-dev
  jackin-project/jackin-github-terraform
  jackin-project/jackin-the-architect
  tailrocks/github-terraform
  tailrocks/termpane
  tailrocks/tui-snap
  tailrocks/velnor
  tailrocks/termrock
  tailrocks/parallax
  tailrocks/terminal-components-claude
  tailrocks/tailrocks-repository-skills
  tailrocks/tailrocks-skills
  tailrocks/tailrocks-pull-request-skills
  tailrocks/homebrew-velnor
  tailrocks/velnor-apt
  tailrocks/parallax-telemetry-playground
  tailrocks/velnor-actions-fixture
  tailrocks/holla
  tailrocks/tracing-request-level
  tailrocks/pg-bigdecimal
  tailrocks/ruxel
  tailrocks/schemalane
  tailrocks/holla-apt
  tailrocks/homebrew-parallax
  tailrocks/homebrew-ruxel
  tailrocks/homebrew-tablerock
  tailrocks/homebrew-holla
  tailrocks/tablerock
  tailrocks/cloudflare-tofu
  tailrocks/tailrocks-typescript-skills
  tailrocks/tailrocks-skill-authoring-skills
  tailrocks/tailrocks-rust-skills
  tailrocks/tailrocks-roadmap-skills
  tailrocks/tailrocks-open-source-skills
  tailrocks/tailrocks-macos-skills
  tailrocks/tailrocks-code-quality-skills
  ChainArgos/blockchain-nodes
  ChainArgos/java-monorepo
  ChainArgos/jackin-agent-brown
  ChainArgos/cloudflare-tofu
  ChainArgos/github-terraform
