# Native review completion adapter

`workflow.review_completion` optionally emits `.github/workflows/review-completion.yml`.
Presence enables the closed `codex-v1` provider adapter; it adds no task, schedule,
environment, role, or arbitrary command configuration. The generator derives the
repository and default branch from repository identity. The configuration contains
`provider`, `check_name`, `app_id_variable`, `app_private_key_secret`, and
`reviewer_app_id_variable`. The completion App and reviewer variable references
must differ; runtime also authenticates their actual identities.

The companion has one `review-completion` job, a literal five-minute timeout,
and the fixed protected environment `review-completion`. Its only triggers are
`issue_comment` created/edited/deleted and `pull_request_target`
opened/synchronize/reopened/ready_for_review on the generated default branch.
Issue comments on ordinary issues are ineligible. It never checks out PR source,
executes repository tasks, or accepts a caller-supplied PR selector.

The pinned official helper captures authenticated provider and paginated feedback
under the read-only workflow token. A canonical literal policy and digest-bound
proposal remain in the same runner's private temporary directory. All eligible
PR states proceed to emission, including pending and blocked states, so an old
success at the same head is not preserved when current evidence ceases to qualify.
The completion key is supplied only to the pinned App-token mint. Its selected
repository token requests Checks write, Pull requests read, and Contents read,
and is supplied only to the emission step. Automatic revocation remains enabled.
The emitter independently reauthenticates the current head, actual provider,
terminal role evidence and complete feedback before updating the unique check.
Duplicate or ambiguous checks and unknown provider evidence fail closed.

This workflow does not make a provider observation sufficient by itself. Actual
provider role schemas, terminal timestamps, identity and policy must be qualified
before enabling a downstream required completion check. A missing, pending,
blocked or failed observation is never successful review completion. The
completion App never merges; the independently credentialed merge gate remains
responsible for protection, checks, feedback and final-head revalidation.

Authenticated `codex-v1` completion is owned by the closed summary's two exact-head
roles: both must be completed with both finding flags explicitly false, and each
terminal clock must follow known finding edits and review restarts. A genuine
clean review may emit no formal GitHub review at all. PR177's retained public
summary (comment 6265103425 at head 2bc7dc02b508d99dcf97b2afccef833a5d54d7af)
exhibits that protocol. The adapter authenticates every formal receipt that is
present, including historical receipts and their finding custody, but does not
require a new formal code receipt after clean completion. Each present receipt's
submission and authenticated edit time must be covered by its matching Code or
Security terminal clock; later activity remains pending until the summary catches up. Reactions, prose,
missing fields, silence and threshold-only signals never replace the summary.
The retained wire fixture and parser tests establish this historical shape; they
do not qualify automatic review of live writer-App PRs or activate unattended merge.
