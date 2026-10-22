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

## zizmor is retired

**Decision.** The separate `zizmor` CI job, its pin, catalog entry,
project-tool key and `.zizmor.yml` are removed. jactionlint 2.x carries
zizmor's audits (41 of its 130+ rules), so a second workflow security
scanner adds a pin and a job but no finding. `zizmor` in
`[workflow.verify] jobs` is now a configuration error
(`zizmor_retired`) that says to set `[workflow.lint] profile = "default"`.
jactionlint honours `# zizmor: ignore[<audit>]`, so the narrow annotations
Velnor already emits (`github-env` on the validated tool-version write,
`self-repository` on local action references) keep working unchanged.

**Coverage evidence (velnor-new's own tree, 10 workflows and 16 composite
actions).**

| Run | Result |
|---|---|
| zizmor 1.31.1 as CI ran it | 0 findings (211 ignored by annotations, 55 suppressed by its persona) |
| zizmor 1.31.1 `--no-ignores` | `self-repository` 178, `github-env` 33 |
| jactionlint 2.3.0 `default` | 0 findings |
| jactionlint 2.3.0 `default` `--no-ignores` | 0 findings |
| jactionlint 2.3.0 `pedantic` `--no-ignores` | `self-repository` 137, `undocumented-permissions` 82, `anonymous-definition` 13, `secrets-outside-env` 2, `concurrency-limits` 1 |

- Every finding class zizmor enforced in CI was empty, and jactionlint at
  `default` is empty too.
- `github-env` is a `default` rule. An unsafe `$GITHUB_ENV` write on a
  `pull_request_target` workflow is reported by both tools, and is suppressed
  by the same `# zizmor: ignore[github-env]` annotation
  (`impl_schema2_binary_preview_lint.rs` runs the pinned binary). jactionlint
  reports fewer raw instances than zizmor because it accepts writes it can
  prove carry no attacker-controlled value.
- `self-repository` is a `pedantic` rule in jactionlint. zizmor never
  enforced it here: every instance carried an ignore annotation.
- The zizmor auditor-persona classes (`undocumented-permissions`,
  `anonymous-definition`, `secrets-outside-env`, `concurrency-limits`) are
  `pedantic` rules in jactionlint. Velnor never ran that persona.

**Status.** Adopted on the owner's instruction above; recorded as hard
decision 10 in [hard decisions](hard-decisions.md). Changing it needs the
owner's explicit approval.
