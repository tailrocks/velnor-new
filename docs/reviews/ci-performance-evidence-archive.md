# CI performance evidence archive

Private raw W0, runtime, release-identity, cache-trust and qualification evidence is retained outside the public checkout. Originals remain intact. This archive establishes evidence preservation; it grants no performance qualification.

Archive: `~/.codex-chainargos2/private/ci-performance/evidence-2026-10-03/`. Private `manifest.json` records source directories, relative file paths, SHA256, bytes, collection identity files and all 47 collected source commits. Directories use mode 0700; regular files use 0600. Symlinks and special entries are never followed. Literal symlink targets are recorded privately.

Snapshot status: W0 supplements for all waves captured after the audit writers finished. Previous manifests remain in private `manifest-history/`; differing metadata revisions are retained by digest. Renewed independent preservation verification passed. Performance qualification remains incomplete.

| Collection | Regular files | Bytes | Skipped entries |
|---|---:|---:|---:|
| runtime | 42 | 41283736 | 0 |
| scope-audit | 48 | 20364177 | 0 |
| wave-a | 4148 | 114580588 | 18 |
| wave-b | 10833 | 431250805 | 0 |
| wave-c | 1863 | 331542562 | 0 |
| release-identity | 3 | 14831267 | 0 |
| cache-trust | 25 | 4241703 | 0 |
| qualification | 21 | 1442046 | 0 |
| rust-profile-local | 7 | 5548 | 0 |
| desktop-rust-profile-local | 24 | 13686 | 0 |
| cargo-release-identity | 6 | 1748834 | 0 |

The qualification collection preserves the observed failing generator run, attempt 1, including raw jobs API, logs and artifact API identities. Artifact API identity is distinct from downloading and inspecting artifact payloads. Missing compiler CPU, exact downloads, queue measurements, cache persistence and controlled benchmarks remain unknown.

The 47-repository JSON, text and CSV inventories match exactly, in order, including the generator and 46 consumers. All collected scope source commits are retained privately. Historical collection SHAs are not asserted to be live heads after later updates.

| Repository | Wave | W0 gap record | Controlled cold/warm/third-run proof |
|---|---|---|---|
| tailrocks/velnor-new | G | [Audit limitations](ci-performance-runtime-audit.md) | NOT_PERFORMED |
| jackin-project/jackin | A | [Audit limitations](ci-performance-wave-a-audit.md) | NOT_PERFORMED |
| jackin-project/jackin-agent-smith | A | [Audit limitations](ci-performance-wave-a-audit.md) | NOT_PERFORMED |
| jackin-project/homebrew-tap | A | [Audit limitations](ci-performance-wave-a-audit.md) | NOT_PERFORMED |
| jackin-project/jackin-role-action | A | [Audit limitations](ci-performance-wave-a-audit.md) | NOT_PERFORMED |
| jackin-project/jackin-sentinel | A | [Audit limitations](ci-performance-wave-a-audit.md) | NOT_PERFORMED |
| jackin-project/jackin-dev | A | [Audit limitations](ci-performance-wave-a-audit.md) | NOT_PERFORMED |
| jackin-project/jackin-github-terraform | A | [Audit limitations](ci-performance-wave-a-audit.md) | NOT_PERFORMED |
| jackin-project/jackin-the-architect | A | [Audit limitations](ci-performance-wave-a-audit.md) | NOT_PERFORMED |
| tailrocks/github-terraform | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/termpane | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tui-snap | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/velnor | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/termrock | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/parallax | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/terminal-components-claude | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tailrocks-repository-skills | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tailrocks-skills | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tailrocks-pull-request-skills | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/homebrew-velnor | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/velnor-apt | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/parallax-telemetry-playground | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/velnor-actions-fixture | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/holla | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tracing-request-level | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/pg-bigdecimal | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/ruxel | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/schemalane | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/holla-apt | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/homebrew-parallax | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/homebrew-ruxel | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/homebrew-tablerock | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/homebrew-holla | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tablerock | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/cloudflare-tofu | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tailrocks-typescript-skills | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tailrocks-skill-authoring-skills | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tailrocks-rust-skills | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tailrocks-roadmap-skills | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tailrocks-open-source-skills | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tailrocks-macos-skills | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| tailrocks/tailrocks-code-quality-skills | B | [Audit limitations](ci-performance-wave-b-audit.md) | NOT_PERFORMED |
| ChainArgos/blockchain-nodes | C | [Audit limitations](ci-performance-wave-c-audit.md) | NOT_PERFORMED |
| ChainArgos/java-monorepo | C | [Audit limitations](ci-performance-wave-c-audit.md) | NOT_PERFORMED |
| ChainArgos/jackin-agent-brown | C | [Audit limitations](ci-performance-wave-c-audit.md) | NOT_PERFORMED |
| ChainArgos/cloudflare-tofu | C | [Audit limitations](ci-performance-wave-c-audit.md) | NOT_PERFORMED |
| ChainArgos/github-terraform | C | [Audit limitations](ci-performance-wave-c-audit.md) | NOT_PERFORMED |

All 47 performance statuses remain `INCOMPLETE`. Green historical CI, retained source, a workflow index and waived CI are separate evidence states. These rows do not mark indexed workflow references as completed performance audits. Raw private source, diffs, commands, logs and operational configuration are not published here.

Private manifest SHA256: `593aa6741a2b0169cf0ff1a8c2a3a893e128f51a130f48703b79e974c6a2ec53`. Independent final preservation verification passed: 17,020 regular files (961,304,952 bytes) match selected source and destination SHA256, sizes and inventories; private modes match. Eighteen skipped symlink targets and modes match. Ten excluded local tool-state trees were not traversed. Four prior manifest digests and all 47 ordered scope identities were verified. Preservation proves retained bytes, not hosted cache behavior or performance.

The two Rust profile collections preserve selected local proof logs, JSON records, summaries and metadata fixture files. Installed tool-state directories are explicitly excluded. These local sequences do not prove hosted fresh-runner restoration or performance.

The Cargo release identity collection retains official source metadata and its independent crosscheck. A local Mac runtime proof does not establish Linux runtime identity or hosted restoration.
