# Hosted runner freshness evidence — 2026-10-05

This evidence refreshes only the hosted-runner identity observation in
`.velnor/freshness-inventory.json`. It does not change a tool/action pin,
qualify a cache route, or refresh the upstream release-tag rows.

The [GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
was rechecked on 2026-10-05. Its public and private standard-runner tables
still list `ubuntu-26.04`, `ubuntu-24.04`, and `ubuntu-22.04`, matching the
inventory's supported labels and unchanged `ubuntu-26.04` default.

Protected-main [CI run 37300188152](https://github.com/tailrocks/velnor-new/actions/runs/37300188152)
completed successfully at head `2d9bca8a37b0440e29a3520aa03e77752079f400`.
Its successful [Rust / velnor-actions-cli job](https://github.com/tailrocks/velnor-new/actions/runs/37300188152/job/111731481419)
started at `2026-10-05T11:02:45Z`. The runner log records Ubuntu `26.04.1`,
image `ubuntu-26.04`, image version `20260927.149.1`, runner version `2.337.0`,
and [image release `ubuntu26/20260927.149`](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20260927.149).
This is a runtime image observation only; it is not a formal Qualification run.
