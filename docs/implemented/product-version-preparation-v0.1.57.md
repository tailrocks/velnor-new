# Product version preparation for v0.1.57

This source preparation carries the generator release-verification fix and the Bun runtime changes from the preceding source work. The branch started at `f83c1be187e1cc98c835e9d85cbf6fa5f47193c4`.

The workspace version advances from 0.1.56 to 0.1.57. All 25 exact internal path requirements use 0.1.57. The lockfile changes only the workspace package versions and the existing `serde_json` test dependency membership. External dependency versions remain unchanged. The runner workspace remains separately versioned.

The source CLI was built with the locked workspace using Rust 1.99.1 and MBX 1.28.0. Its path was `/tmp/bun-migration-velnor-reconcile-target/release/velnor-actions`; it reports `velnor-actions 0.1.57` and has SHA-256 `b13ab0435045495546c6937cc970631658ca4b16e4d4386810c00035bf735bdc`.

The CLI generated the checked-in `.github` tree. A separate preview matched all generated paths, file hashes, and symlink targets. The final second generation matched all 33 generated files and symlinks. A second generation left the tree unchanged. The documented golden capture and check both used this exact binary. All five cases matched, and dogfood reported `tree identical`.

The guarded offline workspace check and release build passed. Formatting passed. The renderer integration suite passed 7 tests; the orchestrator package passed 19 tests across three integration binaries (18 existing tests and the Bun runtime test). All-target Clippy passed for both packages. Cargo Deny and Alint configuration checks exited successfully. Cargo Deny reported existing duplicate-crate and unmatched-license-allowance warnings. Alint reported informational repository and whitespace findings.

The first local freshness check failed because upstream evidence exceeded the 24-hour interval. Official release checks then found GitHub CLI 2.103.3 and Nextest 0.9.150. Their qualified catalog, policy, inventory, bootstrap and lock identities were updated together. Official archive checks and native probes passed. Nextest 0.9.150 passed the guarded seven-case renderer canary through Mise and MBX. A task-scoped, ignored Mise environment selects the verified Nextest executable before the older ambient Cargo executable; global binaries and Cargo settings are unchanged. The final rebuilt CLI passes local and upstream freshness checks. The existing ARM64 musl lock row resolves to a GNU archive through upstream Mise; this preparation does not qualify that platform. The configured Linux bootstrap remains x86-64 GNU.

This is source preparation only. It does not publish a release, create or move a tag, or prove hosted CI, qualification, attestations, publication, or consumer adoption.
