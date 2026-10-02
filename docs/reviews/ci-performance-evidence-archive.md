# CI performance evidence archive

Private raw W0, runtime, release-identity, cache-trust and qualification evidence is retained outside the public checkout. Originals remain intact. This archive establishes evidence preservation; it grants no performance qualification.

Archive: `~/.codex-chainargos2/private/ci-performance/evidence-2026-10-03/`. Private `manifest.json` records source directories, relative file paths, SHA256, bytes, collection identity files and all 47 collected source commits. Directories use mode 0700; regular files use 0600. Symlinks and special entries are never followed. Literal symlink targets are recorded privately.

Snapshot status: waves A–C report collection complete; final evidence snapshot captured after all wave audit writers finished. Performance qualification remains incomplete.

| Collection | Regular files | Bytes | Skipped entries |
|---|---:|---:|---:|
| runtime | 38 | 35207521 | 0 |
| scope-audit | 48 | 20364177 | 0 |
| wave-a | 3701 | 92745881 | 11 |
| wave-b | 9107 | 320029336 | 0 |
| wave-c | 1804 | 330469857 | 0 |
| release-identity | 3 | 14831267 | 0 |
| cache-trust | 25 | 4241703 | 0 |
| qualification | 19 | 1258671 | 0 |

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

Private manifest SHA256: `198fe6749c6ba0e5050ca9c911b6a4df7db85aa12291abc0bfedc3acc6ab3bd2`. Independent preservation verification passed: all 14,745 regular files (819,148,413 bytes) match their source and destination SHA256 and sizes; inventories and private permissions match. Eleven skipped source symlinks match their recorded literal targets and modes. All 47 source identities and ordered scope inputs match. This proves preservation, not cache behavior or performance.
