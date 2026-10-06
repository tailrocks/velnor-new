# Owned-source archive post-publication proof

This record concerns recoverability of the immutable archive release. It does not authorize retirement or establish active Action SHA resolution.

## Published release

- Repository: `tailrocks/velnor-new`; owner: `@donbeave`.
- Release ID: `402793692`; URL: <https://github.com/tailrocks/velnor-new/releases/tag/archive-owned-source-2026-10-04>.
- Tag: `archive-owned-source-2026-10-04`, directly targets main `47815c83b9eeadbaf84b741918fffa7ea550da89`.
- Published: `2026-10-04T02:44:00Z`; API reports `immutable: true`.
- `releases/latest` remains `v0.1.0` (release ID `401790118`).
- Prepublication archive packet fingerprint: `SHA256(SHA256SUMS)=a5772df6ce36f61754cb710c29c29dd77fb2a98d916482816c4d3960903cd391`.

## Independent clean-download result

A separate reviewer downloaded all eight published assets into a new empty directory after publication. API asset sizes and SHA-256 digests matched downloaded bytes. It verified all 31 `SHA256SUMS` entries; verified the external tar's 24 expected regular members before extraction; restored the main bundle into a fresh bare clone; verified six exact snapshot refs, commits, trees, reachable history, and strict fsck; restored each of 24 external bundles into fresh bare repositories; and verified the linked commit closure. The original log records 504 successful commands.

The original JSON declared 183 unique external commit OIDs, but its `external_bundles` array listed only 182 direct identities. One nested `bats-support` commit (`3c8fadc5097c9acfc96d836dced2bb598e48b009`), found in `bats-file` and present in the published tar, was verified by original log entries 502–503 but omitted from that array. A separate hash-bound enumeration addendum records all 182 direct and one nested identities, the exact original log records, and repeat verification. It reports no missing or unverified required commit OIDs. The original JSON bytes remain unchanged.

The original JSON embeds hashes and filenames for its verifier and command log, not those files. The exact verifier source, full command log, and release/latest/tag API snapshots are now retained under `support/`; their source hashes are recorded below and compressed files are checked by `support/FILES.sha256`. Log entries retain argv, return code, stdout, and stderr; long stdout is represented by byte count and SHA-256. The log has no timestamps or command hashes. It does not capture tool-version output, so this record makes no tool-version claim.

The reviewer explicitly did not prove that any historical workflow executes or that branch deletion is safe. No source refs or tags were deleted or force-updated.

## Byte-preserved machine-readable proof

The reviewer-produced JSON had 26,019 bytes and 640 lines, above the repository's 400-line file limit. This repository stores those exact JSON bytes in the gzip blob below; deterministic gzip metadata uses `-n` (no timestamp or original filename). Decompression was byte-compared with the original, and the uncompressed SHA-256 remains the reviewer's exact digest.

- Original JSON SHA-256: `07b853c18389042bb8adde7051b4a188f562f5ba461eb1112e4ac8afeba7c50f`.
- Original JSON bytes/lines: `26019` / `640`.
- Compressed blob bytes: `8662`.
- Compressed blob SHA-256: `57012c3e2f5c1eb5b9e9f9d69fdff38a2f6808ff850a8f0a6352670c848e194e`.
- Blob: `post-publication-proof-sha256-07b853c18389042bb8adde7051b4a188f562f5ba461eb1112e4ac8afeba7c50f.json.gz`.

Reconstruct and verify from this directory:

```sh
gzip -dc post-publication-proof-sha256-07b853c18389042bb8adde7051b4a188f562f5ba461eb1112e4ac8afeba7c50f.json.gz > post-publication-proof.json
shasum -a 256 post-publication-proof.json
```

The expected uncompressed digest is `07b853c18389042bb8adde7051b4a188f562f5ba461eb1112e4ac8afeba7c50f`.

## Ephemeral verifier paths

The original verifier retained its downloads/restores under `/local/cache`; this is an ephemeral machine path. The original proof JSON and enumeration addendum are retained here as byte-preserving compressed blobs. The referenced eight published assets remain available from the immutable release.

Verify compressed support-file digests from this directory with `(cd support && shasum -a 256 -c FILES.sha256)`. To inspect a compressed JSON file, run `gzip -dc support/<content-addressed-name>.json.gz`; compare its output SHA-256 with the source digest below.

| Support file | Original bytes | Original SHA-256 |
|---|---:|---|
| `verification-command-log.json` | 304485 | `995384863a4c494748c5d965589b3a14f94731d76bcda33477e8d1fdcc22dfaa` |
| `verify-published-archive.py` | 8011 | `b470d7e25fec47d671657cc70acf5d55f7b077bab67ccb811647bc72038613c6` |
| `release-api.json` | 14345 | `63a97afc904151a9f1e9552158eb0f991935535662b49a6a897dd7ca25c9a58a` |
| `latest-api.json` | 6389 | `9c9104d94abccd635a15a8929659fec3d487943cdba05f7cdcaaa55e24253173` |
| `tag-api.json` | 427 | `14d46a80df639c5698cb2fb8ae8a37aa35d4441c44ace00ae05c03b50c81475f` |
| `post-publication-proof-enumeration-addendum.json` | 67855 | `68f84758234395a049e83c17070403915d07ada1a92bbe9c4b6785ebb21cada0` |

Archive ref anchors and branch retirement remain separate gates. The six source branches remain present until exact consumer/history closure, protected anchor tag verification, rollback mapping, and the `2026-10-11` owner checkpoint are complete. Three additional MBX refs are outside the six-ref archive and retirement scope.

See [`snapshot-retirement-ledger.md`](snapshot-retirement-ledger.md) for the six flat tag targets, the refreshed nine-ref scan, historical custom-commit ancestry, rollback command, and remaining deletion gates.
