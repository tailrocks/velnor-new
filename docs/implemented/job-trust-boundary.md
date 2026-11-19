# The job is the trust boundary

Repository code (a Cargo build script, a test, a task body) runs as the same
user as every later step of its job. It can write `GITHUB_ENV` and
`GITHUB_PATH` (for example `LD_PRELOAD`, `PERL5OPT`, `PERL5LIB`), replace a
file it owns between a checksum and an `exec`, and leave processes running.
Per-step hardening (`env -i`, copy-verify-exec) narrows this and cannot close
it. Only a job boundary does.

## Rule

A step whose result or credentials matter runs in a job where no repository
code has executed. That covers the verdict of a required check, any token, and
any cache publication. A job that runs repository code hands data to such a job
only as artifacts, which the receiving job validates.

## What velnor-gen enforces

`crates/velnor-gen/tests/it/job_trust_boundary.rs` reads every
generated workflow in `generator/samples/*/expected/` and fails when a step
that runs repository code, or follows a repository-code step in the same job,
carries a token or produces a verdict. A token is `GITHUB_TOKEN`, `GH_TOKEN`,
`github.token`, `ACTIONS_RUNTIME_TOKEN` or `secrets.`, in the step or in the
workflow or job `env` the step inherits. A verdict is `velnor-ci gate` or
`velnor-ci cache check` (as `"$VELNOR_EXEC" gate` and
`"$VELNOR_EXEC" cache check`), or `velnor-ci verify`. A step counts as
repository code when it runs `cargo`, `"$VELNOR_EXEC" run `, `mise run`, or a
local action.

## Where the generated workflows put the boundary

| Concern | Job |
|---|---|
| Required verdict | `required`, which runs no repository code |
| Upload verification, cache waste check (token) | `rust-trim` and `cache`, which run no repository code |
| Release tokens (GitHub App token, `contents: write`) | release jobs that run no repository code, fed by artifacts |

## Remaining same-job gaps

These steps follow repository code in the same job. The test does not flag
them, because the generated YAML carries no token marker for them, but the rule
is not met in full.

- Cache publication in the root jobs: `actions/cache/save` for the target and
  sources caches runs after the tests. A build script can tamper with the
  directory before the save. The trusted base cache is written by the `cache`
  job; the per-job saves are the gap.
- Runner-injected credentials: `actions/upload-artifact`, `actions/cache/save`
  and `actions/cache/restore` run as JavaScript actions that receive
  `ACTIONS_RUNTIME_TOKEN`. The generated lane jobs run them after Cargo, so a
  process left running by repository code can read that token from the action's
  environment. The token only reaches the run's artifact and cache services.
  Closing this needs the uploads to move into a job that runs no repository
  code.
- Helper calls after repository code: `velnor-ci inputs save`, `cache decide`
  and `report` run from the launcher copied before the tests. The launcher can
  be replaced between steps. These steps hold no token. A wrong `cache decide`
  can only make a save more or less likely, and the cache save above is the gap
  that matters.
