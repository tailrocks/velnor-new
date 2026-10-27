# Hosted-runner evidence refresh, 2026-10-27

The [official hosted-runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) was retrieved at `2026-10-27T21:52:54Z`. Its public and private Linux x64 tables support `ubuntu-26.04`, `ubuntu-24.04`, and `ubuntu-22.04`. The bounded response returned HTTP 200 with 20,459 bytes of Markdown containing HTML tables; its SHA-256 is `8c3412686bda8f8ce79afa435d11a6414e0d3a58b799e32672856fcdd201a9ba`.

[CI job 122483329271](https://github.com/tailrocks/velnor-new/actions/runs/40587663856/job/122483329271) succeeded at merged commit `2f181244cb94ac56f15d8691a19c0bfe71e55c24`. Its setup log records Ubuntu `26.04.1 LTS`, runner image `ubuntu-26.04`, and image version `20261012.163.1`, with [this image release](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20261012.163). This is a normal CI image observation. The formal Qualification matrix was not repeated.

Only `runner.checked_at` advances in the freshness inventory. Runner labels, tool pins, status, and the 24-hour evidence interval stay unchanged. The old observation exceeded that interval and correctly failed the freshness gate. [The structured receipt](evidence/freshness-2026-10-27-runner/receipt.json) records the source and successful job bindings.
