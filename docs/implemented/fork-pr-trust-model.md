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
- Poison shared caches: the Plan `Compute cache keys` step grants
  sources writes only to protected default-branch pushes and
  same-repository PRs; a same-repository PR writes a PR-scoped sources
  entry visible only to that PR, forks and merge groups never write,
  and the default branch never restores `pr` entries (its restore keys
  name only the `trusted` scope). Tools saves keep their trust gate
  and never re-store an exact hit (`gate-4-tool-compilation-reuse.md`,
  R13).
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
