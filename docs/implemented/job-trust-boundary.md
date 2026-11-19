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

`generator/crates/velnor-gen/tests/it/job_trust_boundary.rs` reads every
generated workflow in `generator/samples/*/expected/` and fails when a step that
follows a repository-code step in the same job carries a token
(`GITHUB_TOKEN`, `GH_TOKEN`, `github.token`, `secrets.`) or produces a verdict
(`velnor-ci verify`, `velnor-ci cache verify`). A step counts as repository code
when it runs `cargo`, `velnor-ci run --`, `mise run`, or a local action.

## Where the generated workflows put the boundary

| Concern | Job |
|---|---|
| Required verdict | `required`, which runs no repository code |
| Upload verification, cache waste check (token) | `rust-trim` and `cache`, which run no repository code |
| Release tokens (GitHub App token, `contents: write`) | release jobs that run no repository code, fed by artifacts |

## Remaining same-job gaps

These steps follow repository code in the same job. None carries a token or
produces a verdict, so the test passes, but the rule is not met in full.

- Cache publication in the root jobs: `actions/cache/save` for the target and
  sources caches runs after the tests. A build script can tamper with the
  directory before the save. The trusted base cache is written by the `cache`
  job (#1035); the per-job saves are the gap.
- Helper calls after repository code: `velnor-ci inputs save`, `cache decide`
  and `report` run from the launcher copied before the tests. The launcher can
  be replaced between steps (#1109). These steps hold no token. A wrong
  `cache decide` can only make a save more or less likely, and the save above is
  the gap that matters.
