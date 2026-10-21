# Optional dependency-seed failure isolation

SDK38 keeps the cache-only dependency seed outside Required and gives each root
or nested MBX workspace seed job `continue-on-error: true`. A failed optional
cache build must not turn otherwise successful CI into a failed workflow and
block the native preview's existing successful-CI requirement. Crate checks,
opaque checks, Plan, Required, signing and publication remain strict.

The existing typed `MbxSeedBuild` role identifies a seed. Workflow validation
rejects mixed seed/quality/report/control roles, untyped or parallel steps,
services, protected environments, check runners and effective write grants,
including inherited grants. Only the closed setup/cache/fetch/build role family
is admitted. Private dependency mint/fetch/revoke steps remain supported, with
their existing order and credential validation. This is typed IR validation,
not a claim to recognize arbitrary shell command behavior.

Every seed retains the protected default-branch push condition and sole Plan
dependency. It runs no quality checks or report producer. All three compile
lanes must exist before its existing producer-success marker. Failed compilation
still prevents a bundle save; cancellation, digest checks, cache writer election,
size ceilings and all real Required task/report coverage are unchanged.

GitHub's [job-level continue-on-error reference](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idcontinue-on-error)
defines this workflow-result behavior. The original runner
[schema at b6d10b59c178914d331bcf03823133d5f9e366bb](https://github.com/actions/runner/blob/b6d10b59c178914d331bcf03823133d5f9e366bb/src/Sdk/DTPipelines/workflow-v1.0.json)
accepts the job property as `boolean-strategy-context` (Git blob
`c51b9f66e995edea6896f7c18b3337e2271d0942`). Tests compose the actual seed factory,
contract validation and final renderer for root and nested workspaces, and reject
mixed roles and inherited privilege. This does not claim a hosted failure
injection or a particular API conclusion for a shielded failed seed. An operator
may retain a stricter all-jobs-success qualification rule.
