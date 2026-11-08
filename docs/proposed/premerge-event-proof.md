# Premerge event proof

A successful workflow run's REST `head_sha` identifies the PR head. It does not
identify the synthetic merge commit used by default checkout. After merge, the
run and check can expose an empty `pull_requests` array. Current PR associations
and timestamps cannot recover immutable historical base provenance.

The first implemented contract is `ReusableAttestationBinding`. It checks
structural bindings in bounded, duplicate-key-free GitHub CLI output. It returns
no authenticated authority. The genuine reusable-workflow fixture establishes
field mapping; native signature verification and producer execution remain
separate work.

## Producer graph

The planned generator-owned reusable workflow has no candidate-content inputs.
The CI workflow calls it at an independently reviewed immutable revision. GitHub
preserves the caller workflow's context inside a reusable workflow, while OIDC
identifies the callee through `job_workflow_ref` and `job_workflow_sha`.

The collection job has read-only GitHub permissions, no OIDC and no checkout.
Renderer-owned code reads its server-created event/context and queries GitHub.
It records repository and owner IDs, PR number, head/base refs and commits,
synthetic merge commit and tree, caller workflow identity, and run/attempt.
The merge commit must have exactly the ordered parents `[base, head]`.

The attestation job runs independently reviewed callee code in a dedicated
protected environment on a fresh hosted runner. It reads its own server-created
event/context and authenticates the same identities through GitHub. It does not
accept a collector artifact as truth: it either compares exact sanitized bytes
under artifact/job custody or omits that artifact. It has no candidate checkout,
secrets inheritance, registry exchange, or publication command.

The signer requires a nonbypassable human reviewer and explicitly configured
premerge-ref admission. A main-only environment cannot authorize this job. Its
approval binds the callee revision, caller run/attempt and observed event. No
normal `Required` result is a prerequisite to signing the event snapshot; checks
are verified separately, avoiding a dependency cycle.

## Consumer bindings

Signature verification must authenticate the subject digest, GitHub issuer,
hosted runner, independently pinned callee URI/revision, caller URI/revision,
source URI/ref/revision, numeric repository/owner IDs and run/attempt. The
certificate's three revision fields are independent. The existing SDK
release-manifest policy must retain its own restrictions.

The consumer must additionally authenticate protected-environment settings and
approval, canonical callee code and its transitive closure, the event receipt's
PR/head/base/merge bindings, and relevant job results. The receipt claims only
observed context. It does not assert that parallel tests passed.

The initial fixture is a real reusable `workflow_dispatch` signature with equal
revisions and no demonstrated premerge environment. A real protected PR run is
required before this design can support admission.

## Staging

First merge generator support and the reusable workflow through existing normal
checks, without enabling an OIDC exchange producer. Then configure a CI caller
that pins the reviewed callee revision and the explicitly approved environment.
Only after a real signed premerge event has been verified can a later producer
consume that historical proof. No extra crate version publication is needed.

## References

- [Reusable workflow context and environment isolation](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations)
- [OIDC identities for reusable workflows](https://docs.github.com/en/actions/how-tos/secure-your-work/security-harden-deployments/oidc-with-reusable-workflows)
- [Reusable workflow attestation verification](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/increase-security-rating)
- [Fulcio certificate field meanings](https://github.com/sigstore/fulcio/blob/main/docs/oid-info.md)
