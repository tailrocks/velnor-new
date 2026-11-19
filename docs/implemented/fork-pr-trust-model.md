# Fork-PR trust model (G1)

`ci.yml` renders `pull_request` / `push` / `merge_group` triggers only
(`orchestrator::workflow::build_workflow`,
`crates/velnor-actions-orchestrator/src/workflow.rs:130-138`; PR types
`opened, synchronize, reopened, ready_for_review` at
`crates/velnor-actions-workflow-renderer/src/render.rs:31`; dispatch and
schedule are hardcoded `None`). Every job checks out the PR tree
(plan: full-history checkout in
`crates/velnor-actions-orchestrator/src/workflow_jobs.rs:49-78`; lint and
validators: pinned checkout in `workflow_jobs.rs:81-116` and
`crates/velnor-actions-workflow-renderer/src/support.rs:224-240`), so
`Required` executes PR-controlled YAML. The helper asset coordinates
(`VELNOR_ASSET_URL` / `VELNOR_ASSET_SHA256`) are PR-tree bytes too,
rendered from the committed release manifest
(`crates/velnor-actions-workflow-renderer/src/closure.rs:75-78`).

## What a malicious-PR regen can do

A PR that edits `.velnor/config.toml` and regenerates consistently stays
green by construction. It can narrow features, add named Mise checks or
`workflow.tasks` that execute repository-owned task bodies in isolated jobs,
pick a permitted runner label, and shrink the `needs` inventory—as long as
the emitted tree is internally consistent. `workflow.task_suites` adds
standalone push/schedule companion workflows that do not join `Required`; they
also execute repository-controlled task code without a task credential. The
freshness gate
(`closure.rs:184-202`: regenerate into scratch, `diff -r` against
committed `.github`) catches hand edits that disagree with generator
output, never a consistent regen. The `needs` exact-inventory cross-check
(`needs_channel.rs:81-83`, `required_evidence.rs:110-145`) catches
runtime divergence between observed conclusions and the committed
expected set, never a committed inventory that was shrunk at generation
time. This self-attestation limit is recorded in code, not discovered
here: "The merge cannot distinguish legit regeneration from tampering;
review is the trust root for the committed `needs` set"
(`crates/velnor-actions-orchestrator/src/needs_channel.rs:10-18`).

## What it cannot do

- Redirect the helper asset: consumer acquisition re-binds the record URL
  to the official release path and rejects attacker manifests
  (`crates/velnor-actions-orchestrator/src/pins.rs:139-153`, test
  `consumer_gate_rejects_attacker_manifests`).
- Forge the asset digest: a wrong `VELNOR_ASSET_SHA256` fails
  `sha256sum -c` in the Acquire step (`pins.rs:192-198`) — red, not green.
- Poison shared caches: a job with no `cache-mode` key gets GitHub's default,
  which is `write` on a trusted trigger, so the renderer never leaves the key
  out. It writes `read` on every generated job that neither declares a mode nor
  saves, and `write` on a generated job that saves to the Actions cache
  service (an `actions/cache/save` step, `StepKind::saves_to_cache_service`,
  `crates/velnor-render/src/cache.rs` `saving_mode`). What the mode controls,
  from GitHub's reference and the velnor-lab probe (velnor-lab#383): `read`
  permits restores and denies saves at the cache service; the denied save logs
  `Failed to save: Unable to reserve cache ... cache write denied: token has
  no writable scopes` as a warning and the step still succeeds. It does not
  gate the REST API: a `read` job with `actions: write` deleted a cache entry,
  so the trim jobs stay `read`. GitHub accepts only the four literals, not an
  expression (a workflow with `cache-mode: ${{ ... }}` is rejected as a
  workflow-file issue, and `jactionlint` 2.6.0 refuses it too), so a mode is
  one value for every event of the job and cannot be `read` on a pull request
  and `write` on the default branch. The event gate therefore lives on the
  save steps: each save of a generated job carries the default-branch gate
  (`github.ref == format('refs/heads/{0}', github.event.repository.default_branch)`
  with a push, schedule or dispatch event) or the same-repository pull request
  gate (`github.event.pull_request.head.repo.full_name == github.repository`,
  the per-PR delta), so a fork pull request never reaches a save. A same-repository
  pull request writes only its own `refs/pull/<n>/merge` scope, which neither
  the default branch nor another pull request restores. Declared writers
  (`cache_mode = "write"` in `.velnor/workflows.toml`, today the four
  `qualification.yml` cache jobs) are a different kind: dispatch-only and gated
  off the default branch so test writes never pollute it. `generate` refuses a
  write mode on a declared job whose `if` lacks
  `github.ref != format('refs/heads/{0}', github.event.repository.default_branch)`
  (`crates/velnor-workflows/src/job_env.rs`), and a declared job that saves
  (`cache`, `cache-save`) without a writing mode
  (`job_limits::saves_need_write`). Velnor's runner does not decide any of this:
  GitHub's cache service enforces the effective mode through scoped tokens. A
  job that calls a reusable workflow has no `cache-mode` of its own, because
  GitHub documents none for a caller job. Its callee jobs, when Velnor
  generates them, carry theirs. A hand-written callee can still request write.
  The test `cache_mode_default::no_generated_job_leaves_the_cache_mode_to_the_default`
  fails when a generated job that saves does not write, when a write has no
  save step or declaration, or when a save step of a write job is reachable
  from a fork pull request.
- Fake validator conclusions at runtime: `needs` conclusions are
  server-side; any observed-vs-expected divergence fails closed
  (`needs_channel.rs:81-83`).
- Forge plan event/trust: the merge re-checks the plan stamp against the
  merge job's own `GITHUB_EVENT_NAME` ground truth
  (`merge_checks.rs:62-85`).

## Trust root

Reviewed PRs: generated workflow files change only through review (repo
`CODEOWNERS`, root file, currently `* @donbeave`) plus branch
protection on the stable `Required` check
(`needs_channel.rs:13-16`). Reviewers treat inventory, condition, asset
env, and trigger edits as security-sensitive.

## Project tool versions

A PR may change `mise.toml`, `mise.lock`, or a Rust toolchain file; its CI
then runs that PR's declared tool versions
([hard decision 5](../hard-decisions.md)).
The generated resolver reads the declarations with `mise --no-config --no-env
--no-hooks config get`, which loads no config and expands no template, so the
PR gains no code execution beyond the build it already runs. It writes only
validated exact `X.Y.Z` values to `$GITHUB_ENV`. The tools cache identity
hashes those files, so a changed declaration restores and saves under a
different key; saves keep the existing writer policy (fork PRs never save).

## Uncovered: base-pinned validator (future work)

No base-pinned validator exists. Specified, not dropped:

- A job that checks out the base ref (never the PR tree), regenerates,
  and diffs security-sensitive fields (triggers, `needs` inventory,
  asset env, conditions) between base-regen and the PR-tree workflow.
- Acceptance criteria: (1) runs on base bytes only; (2) fails closed on
  regen errors; (3) intentional deltas pass via a reviewed allowlist,
  not silence; (4) wired as a required check alongside `Required`.
- Until then, consistent-malicious regen is caught by human review only.

## Private sources (HD-7)

Generated jobs that consume private sibling repositories mint a GitHub App
installation token (`contents: read`, exact repository allowlist) and revoke
it before any repository-controlled step. Fork pull requests receive no
secrets, so the mint step fails and `Required` fails closed;
`pull_request_target` is never used to work around that. A same-repository
writer who edits the generated workflow can mint the same read-only token for
the installed repositories: this is the self-attestation limit above, with
review and CODEOWNERS on `.github/**` and `.velnor/**` as the trust root.
The credential boundary and the threat model live in the
[private sources contract](../proposed/private-sources-contract.md).
Bun suites and their Playwright legs hold no secret: one assets job per member
mints, fetches and revokes, and the suites receive the verified tree as an
artifact of the same run, never across runs, so a fork cannot plant one. On a
fork pull request that job's mint fails, the suites are skipped and `Required`
fails closed. Fork runs restore the Bun family caches read-only and never
save; only leg 1 of a trusted push saves. A consistent malicious regen could
drop a suite from `needs`: that is the self-attestation limit above.

## Image publication

Pull requests build image layouts with `contents: read` and upload them as
one-day artifacts; they hold no `packages`, `attestations` or `id-token`
scope, and `ci.yml` has no publish job. Registry writes happen only in
`publish.yml`, whose trigger set is `push` to the default branch and release
tags, in jobs bound to the `images` environment (the repository owner must
restrict it to the default branch and release tags; environment protection is
repository settings, not generated). The publish job downloads layouts only
from a successful `ci.yml` run of the pushed commit with the `push` event and
the default-branch head, never from a pull-request run, so fork and
same-repository PR artifacts cannot reach the registry. It checks out and
executes no repository code. See the
[image publication contract](../proposed/image-publication-contract.md).
