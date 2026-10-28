# Native Homebrew merge contract

This document defines the exact-head merge admission boundary of the
[native release channels and Homebrew contract](native-preview-contract.md).
Release production, receipt, retry, and credential-custody requirements remain
in that parent contract; this boundary does not authorize publication or merge
by itself.

## Exact-head merge admission

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

## Channel and committed-generation binding

The signed receipt channel selects one of the two closed configured formula
paths and a channel-distinct deterministic branch. The shared W/O/C/R identities
and exact-head feedback/protection requirements above apply independently to both
channels. A preview receipt cannot authorize a stable formula, stable tag or
stable asset URL; a stable receipt cannot replace preview metadata. Formula paths
must differ and both must belong to the captured updater input closure.

Preview uses only the latest authenticated committed rolling generation. Before
candidate admission and immediately before the head-matched merge, reauthenticate
its fixed release ID, actual tag target, original signed receipt, complete asset
inventory and generation digest. Reobserve after merge and reconcile a concurrent
supersession. GitHub provides no atomic merge precondition covering a generation
in a different repository: this is fail-closed observation and repair, not an
instantaneous cross-repository latestness guarantee. A mutable URL changing after
formula validation produces a checksum failure, never permission to accept new
bytes. Stable uses permanent version-tagged assets and cannot be downgraded.

The generic protected Homebrew seed contract uses the closed version-three
template, whose format floor remains SDK 0.1.60. Each compiled transition sets
its own `minimum_sdk_version`, which determines the SDK required to author that
transition. The current Jackin transition binds the first complete 19-deletion,
27-path inventory and requires SDK 0.1.66. Its eight authored outputs include
`.velnor/release-manifest.json`, rendered from the exact authenticated SDK manifest
bytes used to qualify the seed, and `.github/workflows/review-completion.yml`,
rendered through the ordinary review-completion renderer and authenticated SDK
acquisition path. This keeps the Tap's committed SDK identity aligned with the
SDK that produced its workflow and tool files. The transition rejects older
template inventories, missing review-completion workflows, and candidate evidence
that omits or changes the manifest. The seed caller also requests the
`pull_request` `edited` activity so base retargets rerun the required plan;
it retains `opened`, `synchronize`, `reopened`, and `ready_for_review`.
Immutable SDK 0.1.57, 0.1.58, and 0.1.59 retain their original version-two
implementation.

The seed is a one-time transition from an exact protected base, admitted only
through an authenticated pull request. Its three checkout boundaries use the
verified pull-request head. It does not support merge queues; adding a
`merge_group` trigger alone cannot authenticate a queue commit. GitHub requires
that separate event [when using merge queues](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue).
Before merging
the seed, verify that the Tap does not require a merge queue. Structural cutover
replaces this bootstrap workflow with the ordinary generated Homebrew workflow,
which supports `merge_group`.

The seed emits regular `CLAUDE.md` containing exactly `@AGENTS.md` followed by
one newline. Claude resolves this [relative import](https://code.claude.com/docs/en/memory#import-additional-files) from the repository root, so
`AGENTS.md` remains the single instruction source. Protected source reads and
blob checks remain required; candidate validation also requires its unchanged
mode and blob identity. Candidate transport still rejects symlinks.

For Velnor's current workflow, `.velnor/generator.lock`, the Plan bootstrap,
and the Required controller use authenticated SDK 0.1.66 from source commit
`ecee0521d8631315790662107231dabf2ba6b1ec`. Controller approvals and the lock
bind its published manifest and platform hashes. The Setup Mise step keeps
the default 2026.10.21 pin.

The candidate workflow establishes no pre-merge authority. The protected
default-branch review-completion workflow remains authoritative until the seed
lands through the normal protected merge.
