# jactionlint is the only workflow linter (hard decision 10)

**Decision.** [jactionlint](https://github.com/jdx/jactionlint) replaces
actionlint completely. Every generated lint (the always-on `jactionlint` CI
job, the release gates, candidate qualification and the staged validation
before `generate` writes) runs the pinned jactionlint with the pinned
ShellCheck over the `hk` file set. There is no actionlint job, config, pin,
ignore pattern, label bridge or patched build, and no mode that selects
actionlint or runs both. The only knob is `[workflow.lint] profile`
(`correctness` by default, so the security and policy rule groups are
opt-in; `default`; `pedantic`). Findings are fixed in the generator, never
ignored. Contract: [workflow lint contract](proposed/workflow-lint-contract.md).

**Owner instruction (2026-10-22).** "we need to setup jactionlint for each
project which is using github actions, make sure to add that support
natively to velnor-new".

**Rationale.** actionlint 1.7.12 could not parse what Velnor emits (step
`background`, the body-less `wait-all`, `cache-mode`, `ubuntu-26.04`) and
needed ignore regexes, a label bridge and a patched build. jactionlint 2.3.0
validates all four natively, includes actionlint's checks, and also lints
composite actions and `dependabot.yml`.

**Status.** Adopted on the owner's instruction above; recorded as hard
decision 10 in [hard decisions](hard-decisions.md). Changing it needs the
owner's explicit approval.
