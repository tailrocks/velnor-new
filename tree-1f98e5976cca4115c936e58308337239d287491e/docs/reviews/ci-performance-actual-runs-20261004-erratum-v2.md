# Hosted CI evidence erratum v2: current main 47815

Status: append-only correction to the private run-audit packet. This erratum preserves the original packet and its files. It makes no performance or runtime qualification claim.

## Frozen evidence boundary

Evidence root: `~/.codex-chainargos2/evidence/velnor-pr12/recovery-20261004/`. Original packet directory: `current-main-47815-ci-audit/`. Its `SHA256SUMS` has 252 entries, all verified; the directory contains 253 regular files including that manifest. Manifest SHA-256: `31a91f2c76889cad4a8ce8588bee5e6c183ba3ced31196bf25cc2a6a51e1b416`.

Independent review receipt: `current-main-47815-ci-audit-independent-review.md` under the evidence root above; SHA-256 `f5a6406a1fbb5d1602b23e7a48bae8d33532021910c9f01d3314da61b09a5f3a`. It is outside the frozen packet manifest. The receipt checked all 252 entries and verified the mappings below from the captured job logs and ZIP.

## Plan-log representation correction

There are two distinct forms of main Plan job `111321789117`:

| Representation | Packet path | Size | SHA-256 |
| --- | --- | ---: | --- |
| Raw API job-log capture; byte-equal to `19_Plan.txt` in the full CI log ZIP | `job-log-111321789117.bin` | 81,328 bytes | `37ffa45140540f4e6a0f7ee2db3ff075394aa0b5626a4e7a2f1b503d87f4e6fa` |
| Enriched step/timestamp rendering | `main-plan-111321789117.log` | 106,569 bytes | `2435a740ccb64947529728c0df5e3fcfd38e2eb0a1ad3905e56ded2fa6f8744e` |

The full main CI ZIP is `logs-37163556069.zip`, SHA-256 `d6d28d04333c48d014c758d7348147c5d4266c6a1ed866d925ee41e21dcb32ae`. In the original packet, `audit-summary.md` reports the raw capture hash `37ffa…`; `cache-and-run-details.md` names the enriched rendering and reports its `2435…` hash. Those are compatible records, but the older summary wording does not identify the raw capture filename or distinguish the two representations. Keep both files and hashes.

Three other names are zero-byte placeholders, not additional log captures: `main-37163556069-plan-111321789117.log`, `main-37163556069-contract-111322024578.log`, and `logs-job-111321789117.zip`. Do not cite them as populated evidence.

## Correct Mise version hash

The pinned toolkit computation for explicit save path `~/.local/share/mise` is `e61abf9f359d5350c7c9daa8fafea5bf582b24379c29361dec0dc476075a131a`. It matches cache-index entry `8465891388` for the logged Mise key. The previous `cache-and-run-details.md` prints a value differing by one character; this erratum gives the corrected value without rewriting that packet file.

The pinned Mise action resolves `/home/runner/.local/share/mise`, whose computed version is `7a8f38effbb692ecc590f129f1a759a935e12173263c55e06b5c8ec7ae304837`. No entry with that version was present for the visible Setup Mise key. The log reports a cache miss and then downloads Mise and installs pinned tools. This is consistent with the path mismatch; the API version is a path/compression compatibility hash, not an archive digest.

Raw cache-index response: `current-main-47815-ci-audit/cache-index-final.json` under the evidence root above; SHA-256 `d91ec1ea8f4400204602183cc69fa981417cafcffe68675123ced47e324cca4d`. It came from `GET /repos/tailrocks/velnor-new/actions/caches?per_page=100`; the response exposes `version` on each cache record.

## Artifact mapping correction

The two `velnor-final` artifacts belong to these CI runs:

| CI run | Source | Artifact |
| --- | --- | --- |
| PR CI `37162836495` | head `37416ae648c9bdb7319ddfc8a21776dcec93eba9` | `11288551302` |
| Main CI `37163556069` | exact SHA `47815c83b9eeadbaf84b741918fffa7ea550da89` | `11288896425` |

The exact-SHA release dispatch `37163597428` instead has `generator-linux-assets` and `generator-macos-assets`; it has no `velnor-final` artifact. Clarify the old phrase “velnor-final artifacts for both CI runs” with the PR/main mapping above. Do not combine the release workflow with the CI artifact pair.

The main final report records 71 selected/executed, zero failed/reused/covered/not-run, and successful required results. Those are execution/report-delivery facts, not a cold/warm comparison or performance qualification. Main task stage fields are zero in the task reports; compiler/link/download/test timings remain unavailable. No result here changes the 47-row completion-ledger status.

## Workflow identity and custody

The raw workflow bytes acquired at exact target commit `47815c83b9eeadbaf84b741918fffa7ea550da89` are `ci-47815.yml` (SHA-256 `cc68fa9248dd3d40007b693ec7e73657387e5e77a1e85b986e8e8c863bd3f06a`) and `generator-release-47815.yml` (SHA-256 `d13645f1aa9667a1ce9fa0886ca5f4930c3a9a0e33c3dfe7df6b8942cad9962a`). Their contents-API Git blob IDs are `aee06e8685ea16bcaf840b4240ebf6e3d1f4f2d6` and `87abf573dd3da43695aaf0b7ccd7da41325c2aa0`, respectively.

This erratum is a separate versioned file. It does not alter the frozen packet, its manifest, repository files, index, or Git refs.
