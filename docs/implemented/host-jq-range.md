# Host `jq` versions and Mise discovery limits

The owner may revisit this decision. Two defects caused generated verification
jobs to fail on GitHub-hosted `ubuntu-26.04`.
`ChainArgos/infrastructure` found them in its Delorean policy verification task.

## 1. Accept the jq 1.8 release line

Typed verification and build task scripts resolve `jq` with `command -v jq`
and check `jq --version`. The parser reads `mise config ls --json` before
trusted tool installation. The previous check required `jq-1.8.2`.
The `ubuntu-26.04` image supplies `jq-1.8.1`, as recorded in
`actions/runner-images` `Ubuntu2604-Readme.md`. The step therefore exited
with status 1 without a diagnostic.

The check now accepts `jq-1\.8\.[0-9]+`, defined by `JQ_VERSION_PATTERN`
in `task_script.rs`. It rejects other release lines and decorated version strings.
The fixed filter is `-r '.[].path'`. Its result must match the expected path list.
This path comparison does not authenticate the jq executable.

jq runs before the trusted Mise tool installation and before a tool root exists.
Installing jq through `velnor-tool-seed` would require another initial installation
because the current parser checks the Mise configuration first.
The image can change its jq patch version independently of this crate.
The 1.8 release line defines the accepted CLI and version-output format.

If the owner requires parser provenance, add `jq` to the task-tool seed.
Then make the check use that installation.

## 2. Set the Mise limit above the task root

The install step writes `mise.toml` into `$task_root`.
Previously, it set `MISE_CEILING_PATHS="$task_root"` before running `mise config ls`.
A Linux reproduction used Mise 2026.10.15 and an empty temporary directory.
With the ceiling equal to that directory, Mise excluded the directory's own configuration.
It listed only `global.toml` and `system.toml`, so the expected-path comparison failed.
No tools were installed. Setting the ceiling to the parent included `mise.toml`.
The macOS reproduction did not show this behavior.

The contract helper `config_ceiling_directory` specifies the directory immediately above the configuration directory.
Verification and build install steps now use `MISE_CEILING_PATHS="${task_root%/*}"`.
The build run step uses `$GITHUB_WORKSPACE/..` when reading the workspace `mise.toml`.

Tests cover the emitted jq check, accepted versions, Mise limits, and verification-task generation:
`task_script::tests`, `verification_jobs_tests`, and the orchestrator verification-task generation test.
