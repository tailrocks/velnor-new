# Cache GitHub-only qualification harness (T58)

Status: harness defined. No live qualification is claimed. No hosted run was dispatched.

## Harness

- Support module: `crates/velnor-actions-orchestrator/tests/gc1_qualification_support.rs`.
  It holds the bounded qualification types: nonempty workload requirements, complete
  observation requirements, approval-gated hosted command records, known case-ID
  validation for C01 through C22, and qualification-namespace scoping for
  destructive fixture operations.
- Case module: `crates/velnor-actions-orchestrator/tests/gc1_qualification_cases.rs`.
  It exercises the harness under the `gc1_aux` prefix. Both modules are registered
  in `crates/velnor-actions-orchestrator/tests/velnor_orchestrator.rs`.
- The harness defaults to local fixtures. Hosted mode requires the
  `VELNOR_GC1_QUALIFICATION_HOSTED=1` opt-in, and the suite asserts the default
  stays local. Validation never executes a hosted command; dispatch stays a
  blocked contract until a campaign approval exists.

## Run-command manifest

`docs/proposed/cache-github-only/run-commands.json` records the exact workflow file
(`.github/workflows/qualification.yml`), the `workflow_dispatch` input set, the
plan-frozen source pin, the C01-C22 case binding, runner classes, run budgets,
zero fault allowance, collection commands, and the unapproved approval state.
Unknown pins stay null with reasons; they are never zero.

## Verification

```bash
cargo nextest list --locked -p velnor-actions-orchestrator -E 'test(gc1_aux)' --message-format json
cargo nextest run --locked -p velnor-actions-orchestrator -E 'test(gc1_aux)' --no-tests fail
```

Required tests: `gc1_aux::rejects_empty_workload`,
`gc1_aux::rejects_unapproved_fault`. Both must execute with a nonzero count.
