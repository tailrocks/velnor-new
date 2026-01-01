# Required-check migration: `Velnor / Required` to `Required`

P05-9 procedure. The generator renamed the main workflow from
`.github/workflows/velnor.yml` (display `Velnor`, gate `Velnor / Required`)
to `.github/workflows/ci.yml` (display `CI`, gate `Required`).
Branch protection still waiting on the old name blocks every merge;
dropping the old requirement before the new check exists unprotects
the branch. Follow the order below exactly.

## Sources of truth

- Old/new paths and check names:
  `RequiredCheckMigration::velnor_to_ci`
  (`crates/velnor-actions-contract/src/workflow/jobs.rs`).
- `velnor-actions plan` prints the same migration under
  `Required-check migration`; the last step carries
  `[EXTERNAL: repository admin]`.
- The committed branch tree holds `ci.yml` only; no `velnor.yml` remains
  (the scheduled `freshness.yml` probe is a separate workflow).
  `main` itself has no `.github` tree yet — the branch is unmerged.

## Procedure

1. Merge the generator change so `.github/workflows/ci.yml`
   replaces `.github/workflows/velnor.yml` in one commit.
   `generate` replaces the complete `.github` tree, so the stale
   file disappears with the merge; never hand-delete one side.
2. Let one `ci.yml` run complete on the default branch so the
   `Required` check appears. Confirm with:
   `gh run list --workflow ci.yml --branch <default> --limit 1`
   then `gh run view <id> --json conclusion`.
3. NEEDS-HUMAN (repository admin): in branch protection, require
   `Required` and remove `Velnor / Required`; never remove the old
   check before the new one exists. Verify with:
   `gh api repos/<owner>/<repo>/branches/<default>/protection
   --jq .required_status_checks.checks`.
   An automation agent cannot perform this step: it needs a
   human admin credential outside every agent trust boundary.

## Rollback

If the new check never reports, keep requiring the old check and
revert the generator commit; do not leave the branch with neither
check required.
