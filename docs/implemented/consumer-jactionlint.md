# Consumer workflow linting with jactionlint

Every repository whose CI Velnor generates is linted by
[jactionlint](https://github.com/jdx/jactionlint), Velnor's only workflow
linter ([hard decision 10](../jactionlint-decision.md); specification:
[workflow lint contract](../proposed/workflow-lint-contract.md)). This page
is the rollout procedure for one repository. The owner's instruction of
2026-10-22 applies to every repository that uses GitHub Actions.

## What a generator upgrade changes by itself

Regenerating with a Velnor release that contains the swap, and changing
nothing else:

- replaces the `actionlint` CI job with `jactionlint` (display
  `Jactionlint`), which `Required` needs;
- writes `.github/jactionlint.yaml` with `profile: correctness` and retires a
  generated `.github/actionlint.yaml`;
- lints every workflow, composite action and `dependabot.yml`, during
  `generate` (staged, before any write) and in CI.

`correctness` is actionlint's checks plus jactionlint's bug detectors, so a
tree that passed actionlint can still get new `correctness` findings
(`local-action-checkout`, ShellCheck findings in composite actions, which
actionlint never linted). Fix them in the workflow source, or in the
generator if Velnor wrote them; there is no ignore.

## Per-repository steps

1. **Pin the tools in `mise.toml`** (always the latest stable releases; check
   live when applying):

   ```toml
   [tools]
   jactionlint = "2.3.0"
   shellcheck = "0.11.0"
   ```

   jactionlint ships every few days and Mise hides releases younger than its
   `minimum_release_age` (24 hours by default), so check the latest release
   with `MISE_MINIMUM_RELEASE_AGE=0s mise latest jactionlint` or the GitHub
   releases API rather than a plain `mise latest`.

   CI reads both versions from this file at run time (hard decision 4), so
   CI and local runs use one binary. Lock them for every CI platform:
   `mise lock jactionlint shellcheck --platform linux-x64,linux-arm64,macos-arm64`.

2. **Select the profile in `.velnor/config.toml`.** Rollout PRs select
   `default` (the security and policy rules):

   ```toml
   [workflow.lint]
   profile = "default"
   ```

   `pedantic` is accepted but noisy; nothing else is configurable.

3. **Add the check-only `hk` step** next to the other builtins in `hk.pkl`.
   `Builtins.jactionlint` also ships `fix` and `check_diff`, which write
   `.github/`; generated files are never hand-edited, so both are removed:

   ```pkl
   ["jactionlint"] = (Builtins.jactionlint) {
     fix = null
     check_diff = null
     check = "jactionlint --format gcc --strict-exit --no-online --shellcheck=shellcheck --pyflakes= {{files}}"
   }
   ```

   The builtin's glob is exactly the CI file set
   (`.github/workflows/*.y*ml`, `.github/actions/**/action.y*ml`,
   `.github/dependabot.y*ml`), and jactionlint reads the generated
   `.github/jactionlint.yaml` from the repository root, so local and CI runs
   report the same findings.

4. **Regenerate** with `velnor-actions generate`. Generation lints the staged
   tree with the profile you selected and refuses to write when anything is
   reported. Fix every finding at its source: the workflow you own, or the
   Velnor generator (report it upstream) for generated files.

5. **Open one PR** with `mise.toml`, `mise.lock`, `.velnor/config.toml`,
   `hk.pkl` and the regenerated `.github/`. CI must be green with
   `--strict-exit`.

## Order

Roll out from the least to the most critical repository, velnor-new first
(it self-hosts at `profile = "default"`).

## What not to do

- Do not hand-write `.github/jactionlint.yaml`; edit `.velnor/config.toml`.
- Do not add `# jactionlint ignore=` comments, `ignores:`, `paths:` or a
  baseline to make a finding go away.
- Do not run `jactionlint --fix` or `hk fix` on `.github/`.
- Do not set `JACTIONLINT_NO_HINTS`; it hides the "shellcheck was not found"
  note.
