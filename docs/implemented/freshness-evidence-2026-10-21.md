# Freshness evidence for 2026-10-21

The first SDK 0.1.36 release attempt, workflow run 39559267466 at source
`4179226bf8ec87af5c636fb048a3df984f29ad2f`, failed its source-eligibility check.
The reviewed upstream evidence crossed its 24-hour validity limit between
successful main CI and release eligibility. No candidate artifacts, version
tag, or release were published by that attempt.

Two fresh official metadata captures agreed on all 26 tool/action inventory
rows and all 13 action tag commits. They found five tool updates:

| Tool | Previous pin | Observed release |
| --- | --- | --- |
| Mise | 2026.10.15 | [2026.10.17](https://github.com/jdx/mise/releases/tag/v2026.10.17) |
| MR Boxington | 1.25.0 | [1.26.0](https://github.com/jdx/mr-boxington/releases/tag/v1.26.0) |
| GitHub CLI | 2.102.0 | [2.102.1](https://github.com/cli/cli/releases/tag/v2.102.1) |
| Nextest | 0.9.148 | [0.9.149](https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.149) |
| Cosign | 3.1.3 | [3.1.4](https://github.com/sigstore/cosign/releases/tag/v3.1.4) |

The remaining tool versions and all action tag commits were unchanged. The
Bun release tag `bun-v1.4.3` normalizes to the existing `1.4.3` pin. Raw release
metadata, action tag resolutions, observation timestamps, response hashes, and
HTTP headers were retained. Rust's stable-channel observation required HTTP
206, the exact requested Content-Range, and the complete 128 KiB prefix;
responses were bounded to 512 KiB.

Both canonical `--check-upstream` attempts on this host returned lookup
timeouts for all 26 network rows. Those failures were retained, not reported
as passing probes. The two agreeing official captures used bounded CLI
requests and separate authenticated GitHub metadata/tag observations; their
raw bytes and timestamps support the reviewed ledger update.

The evidence agreement has SHA-256
`2562f8c324c5f903fe8d5733e1049697ea16813ba6d5454a737a01bd74367182`.
The raw-evidence assessment has SHA-256
`c48bea3752a3defdf89d2692d4c48d51ac9de672e7310db7d41a71aa8fe6d319`.

The successful main CI run 39557820072, attempt 1, Plan job 119081652574
reported Ubuntu 26.04.1 and hosted image `20261018.168.1`. This refreshes a
normal CI image observation alongside the official hosted-runner reference;
it does not claim a new formal platform Qualification run.

The 24-hour freshness policy remains enforced. Old pins are not relabeled
current through timestamp edits. The five updates require coherent catalog,
bootstrap, checksum, project tool, generated-output, and compatibility
qualification before the next official release attempt. SDK 0.1.36 remains
unpublished until its new exact-source CI and protected publication complete.
