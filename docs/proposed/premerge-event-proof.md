# Premerge event proof

A successful workflow run's REST `head_sha` identifies the PR head. It does not
identify the synthetic merge commit used by default checkout. After merge, the
run and check can expose an empty `pull_requests` array. Current PR associations
and timestamps cannot recover immutable historical base provenance.

## Generated reusable producer

The optional `premerge_event_proof` configuration generates
`.github/workflows/premerge-event-proof.yml`. It accepts only `workflow_call`,
with no inputs or inherited secrets. The sole job runs renderer-owned code on a
fresh hosted Ubuntu 26.04 runner in the configured protected environment. It
never checks out candidate code or exchanges a registry credential.

```toml
[premerge_event_proof]
repository_id = 7
owner_id = 11
environment = "premerge-proof"
reviewer_id = 60
caller_workflow = ".github/workflows/ci.yml"
```

The IDs above illustrate the configuration shape. Operators must use their
repository, owner and reviewer IDs. Repository name and target default branch
come from generation preparation.

The caller must reference the reusable workflow at a reviewed immutable commit
and grant its calling job `id-token: write` and `attestations: write`, with
read access to actions, contents and pull requests. These permissions must not
be added to jobs that execute candidate code. The callee cannot elevate the
caller's permissions. This increment emits the reusable producer; it does not
add a caller to CI or activate repository settings.

## Receipt construction

Trusted callee code reads its own server-created event/context and GitHub API
responses. Caller artifacts and inputs supply no receipt facts. The code checks
repository and owner IDs, an open nondraft same-repository PR, exact head/base
refs and commits, and the synthetic merge commit and tree. The merge commit must
have exactly the ordered parents `[base, head]`. Runtime `job.workflow_*`
properties identify the callee independently of caller `github.workflow_*`
properties and the tested merge `GITHUB_SHA`.

The dedicated environment must disable administrator bypass, require the
configured human reviewer, and have exactly the custom branch policy
`refs/pull/*/merge`. A main-only environment cannot authorize this job. These
server-side controls must exist before execution; runtime checks do not secure
arbitrary modified workflows that can reach an incorrectly configured
environment. The code verifies environment approval and reobserves source and
approval before writing a bounded secret-free receipt.

The job grants only read access to actions, contents and pull requests, plus
`id-token: write` and `attestations: write`. After receipt construction, fixed
catalog-pinned attestation and upload actions sign and retain it. Failed
construction or attestation does not continue to artifact upload. The probe's
15-second socket timeout is not a hard total request deadline; the hosted job's
10-minute timeout bounds execution.

The receipt claims observed context only. It does not assert that parallel
checks passed, reviews were accepted, or a candidate is authorized to merge or
publish. Downstream admission must independently authenticate the subject and
signature, pinned caller/callee revisions and transitive closures, protected
environment/review evidence, and relevant check results. No `Required` result
is a prerequisite to event signing, avoiding a dependency cycle.

## Binding contract and qualification limits

`ReusableAttestationBinding` checks structural bindings in bounded,
duplicate-key-free GitHub CLI output. It returns no authenticated authority.
Its certificate model separates callee signer, caller workflow and source
revisions. Source Git refs require `refs/`; workflow references also accept
canonical SHA pins equal to the expected workflow revision. Existing SDK
release-manifest policy retains its own restrictions.

The genuine reusable-workflow fixture establishes field mapping for a
`workflow_dispatch` artifact with equal revisions. Local tests exercise generated
source and controlled external API responses. Neither proves actual PR event
files, new `job.workflow_*` values, environment behavior or attestation action
execution on a hosted runner. A real protected PR run remains required before
this producer can support release admission. The release gate remains closed.

## Staging

Merge generator support and the reusable workflow through existing normal
checks. Then generate a CI caller pinned to that reviewed callee revision and
configure the explicitly approved environment. After a real signed premerge
event has been independently verified, later release admission can consume that
historical proof. No extra crate version publication is needed.

## References

- [Reusable workflow context and environment isolation](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations)
- [Callee job context properties](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#job-context)
- [Environment deployment-ref rules](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments)
- [OIDC identities for reusable workflows](https://docs.github.com/en/actions/how-tos/secure-your-work/security-harden-deployments/oidc-with-reusable-workflows)
- [Reusable workflow attestation verification](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/increase-security-rating)
- [Fulcio certificate field meanings](https://github.com/sigstore/fulcio/blob/main/docs/oid-info.md)
