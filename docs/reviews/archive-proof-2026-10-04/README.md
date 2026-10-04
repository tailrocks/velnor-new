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

A separate reviewer downloaded all eight published assets into a new empty directory after publication. API asset sizes and SHA-256 digests matched downloaded bytes. It verified all 31 `SHA256SUMS` entries; verified the external tar's 24 expected regular members before extraction; restored the main bundle into a fresh bare clone; verified six exact snapshot refs, commits, trees, reachable history, and strict fsck; restored each of 24 external bundles into fresh bare repositories; verified strict fsck and all 183 linked commit OIDs with complete object walks. The reviewer reports 504 successful verification commands. The proof records exact verifier commands, tool versions, API snapshots, download paths, restore results, and hashes.

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

The original verifier retained its downloads/restores under `/local/cache`; this is an ephemeral machine path. The original proof JSON is retained in this repository as the byte-identical compressed blob, so the temporary directory is not the durable evidence copy.

Archive ref anchors and branch retirement remain separate gates. The six source branches remain present until exact consumer/history closure, protected anchor tag verification, rollback mapping, and the `2026-10-11` owner checkpoint are complete. Three additional MBX refs are outside the six-ref archive and retirement scope.
