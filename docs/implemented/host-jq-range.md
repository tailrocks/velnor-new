# Host `jq` range and install-step Mise ceiling

Decision, adopted per the split brief; owner may revisit. Two defects made every
generated verification job fail on GitHub-hosted `ubuntu-26.04` (found by
`ChainArgos/infrastructure` running its Delorean policy test as a verification
task).

## 1. Host `jq`: accept the 1.8 line, not one patch

Every typed task script (verification and build tasks) resolves `jq` with
`command -v jq` and checks `jq --version`, because it parses
`mise config ls --json` before any tool exists. The guard required exactly
`jq-1.8.2`; the `ubuntu-26.04` image ships `jq-1.8.1`
(actions/runner-images `Ubuntu2604-Readme.md`), so the step exited 1 silently.

Chosen: the guard accepts `jq-1\.8\.[0-9]+` (`JQ_VERSION_PATTERN` in
`task_script.rs`). Why not install a pinned, checksummed jq:

- `jq` is not a task tool. It is the parser that runs before the trusted Mise
  install and before any tool root exists, so installing it would need a second
  bootstrap with its own provenance chain (`velnor-tool-seed` seeds Mise
  tools, which cannot be used before the config chain is verified by this very
  parse).
- The guard's job is to reject an unknown parser dialect, not to prove
  provenance: input is Mise's own output, the filter is the fixed
  `-r '.[].path'`, and the result is compared against an exact expected path
  list, so a wrong jq can only fail the job, never admit extra configs.
- Image patch releases move independently of this crate; an exact patch pin
  re-breaks on every image refresh. The minor line is the contract (CLI and
  `--version` format), so it stays pinned. Any other line (1.7, 1.9, 2.x) or a
  decorated version string is rejected.

Owner review point: if the owner wants provenance for the parser too, add `jq`
to the task-tool seed and make the guard compare against that install.

## 2. Install-step ceiling is the parent of `$task_root`

The install step wrote `mise.toml` into `$task_root`, then set
`MISE_CEILING_PATHS="$task_root"` and verified `mise config ls`. Reproduced
with mise 2026.10.15 on Linux (bare temp directory, `MISE_CEILING_PATHS` equal
to the directory): the ceiling directory itself is excluded, so only
`global.toml` and `system.toml` were listed, the exact path comparison failed,
and no tools were installed. The same call with the ceiling at the parent lists
`mise.toml`. (macOS mise does not show the quirk, which is why it went
unnoticed.) The contract helper `config_ceiling_directory` already documents
"directory immediately above the config directory"; the install steps now do
the same: `MISE_CEILING_PATHS="${task_root%/*}"` for verification and build
install steps, and `$GITHUB_WORKSPACE/..` for the build run step that reads the
workspace `mise.toml`.

Tests: `task_script::tests` (real guard against fake jq 1.7/1.8.x/1.9),
`verification_jobs_tests` (ceiling and jq pattern in the emitted install
script), and the orchestrator verification-task generation test.
