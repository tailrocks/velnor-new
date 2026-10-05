The runner freshness row was more than 24 hours old, causing the repository-policy stage test and `check-freshness.sh` to fail. This update records a fresh supported-label check and the actual hosted image observed in successful protected-main CI run 37300188152 at head `2d9bca8a37b0440e29a3520aa03e77752079f400`.

The Rust CLI job log records Ubuntu 26.04.1, runner image `ubuntu-26.04`, image version `20260927.149.1`, runner version `2.337.0`, and the matching image release. The current GitHub-hosted runner reference still lists the three inventory labels: `ubuntu-26.04`, `ubuntu-24.04`, and `ubuntu-22.04`.

This refresh changes only the runner observation timestamp and adds its dated source record. It does not change tool/action pins, cache qualification, or claim a formal platform Qualification run.

Validation: `check-freshness.sh` passes; the isolated `verify_local_repo_policy_stage_executes` test passes; Alint 0.16.1 config/check pass; `git diff --check` is clean. The full workspace run had a single failure on that time-expired runner observation before this update.
