# Upstream freshness capture, 2026-11-06

`capture-1` and `capture-2` were produced by the existing `capture.py` workflow copied from `freshness-2026-11-05`. Each queried the inventory's primary upstream release endpoints using authenticated `gh api` for GitHub and HTTPS for Rust and crates.io. `capture-agreement.json` records identical observations across the two captures.

The captures agree that four reviewed pins now trail current upstream releases: mise `2026.11.2`, jactionlint `2.5.0`, OpenTofu `1.13.9`, and `asamarts/alint` `v0.21.1`. Their pinned and qualified values remain unchanged and their inventory status is `behind`; this evidence refresh does not qualify or introduce any tool update. Bun's upstream tag is `bun-v1.4.4`, equivalent to pinned `1.4.4`; its inventory remains `current`.

The GitHub-hosted runner evidence was refreshed from the current [runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) and a current CI log. `runner/runner-observation.json` records the supported Ubuntu labels, default label, run and job IDs, source SHA, and hashes for the captured document and log.

`bash scripts/check-freshness.sh` passes every row except the four expected `upstream-freshness` mismatches listed above. `bash scripts/check-freshness.sh --check-upstream` was also attempted; its unauthenticated bounded probe receives HTTP 403 from GitHub API release endpoints in this environment. It does not alter the inventory or weaken the policy. The authenticated capture workflow succeeded.
