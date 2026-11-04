# Optional state producer failure isolation

The state producer (`state-producer-<root>`) and the state seal job
(`state-seal-<root>`) stay outside Required and carry `continue-on-error: true`.
A failed optional cache build must not turn otherwise successful CI into a failed
workflow. Crate checks, opaque checks, Plan, Required, signing and publication
remain strict.

`continue-on-error` makes `needs.<producer>.result` read `success` for a
producer whose compile failed, so the seal job never reads the result. The
producer sets the job output `produced` to `true` in its last step, after the
export, the pack and the artifact upload all succeeded, and every earlier step
runs under the default `success()`. A failed lane, a refused export or a failed
upload therefore leaves `produced` empty, and the seal job's condition
(`needs.<producer>.outputs.produced == 'true'`) keeps it from attesting or
saving anything.

The typed `ProducerKind::StateProducer` identifies a producer. Workflow
validation rejects mixed roles, parallel steps outside the lane records,
services, protected environments, check runners and effective write grants.
Every producer retains the protected default-branch push condition and the sole
Plan dependency. The producer runs no quality checks or report producer: its
task steps are copies of the crate jobs' build, Clippy and documentation lane
records, and a test requires the dependency-skeleton phase and the real-members
phase to run the same records.

GitHub's [job-level continue-on-error reference](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idcontinue-on-error)
defines the workflow-result behavior. The original runner
[schema at b6d10b59c178914d331bcf03823133d5f9e366bb](https://github.com/actions/runner/blob/b6d10b59c178914d331bcf03823133d5f9e366bb/src/Sdk/DTPipelines/workflow-v1.0.json)
accepts the job property as `boolean-strategy-context` (Git blob
`c51b9f66e995edea6896f7c18b3337e2271d0942`). Tests compose the real factories,
contract validation and the final renderer for two roots, and a small evaluator
of the step conditions shows that a failed lane leaves no `produced` output.
They do not claim a hosted failure injection; the first hosted push is that
proof.
