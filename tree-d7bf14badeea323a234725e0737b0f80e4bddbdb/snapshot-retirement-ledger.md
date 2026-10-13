# Snapshot ref anchors and retirement status

**Status: PARTIAL — no source branch has been deleted.** This ledger records six
protected recovery tags and their reviewed targets. It does not prove that
historical GitHub Actions SHA resolution works after branch deletion, finish
consumer closure, or authorize deletion.

## Anchor verification

All six flat tags below were created as lightweight refs and pushed together
without force. At `2026-10-04T03:24:36Z`, `git ls-remote origin` and the GitHub
`GET /repos/tailrocks/velnor-new/git/ref/tags/<tag>` API returned the same
commit target for each tag. Each API object had `type: commit`; its `url` was
the matching `/git/commits/<sha>` endpoint. The same ref query returned the
current source branches listed below. The immediately preceding verification
window was `2026-10-04T03:22:07Z`–`03:22:10Z`.

| Historical source branch | Current tip | Flat protected anchor tag | Anchor target |
|---|---|---|---|
| `owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` | `dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` | `archive-owned-source-2026-10-04-mise-dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` | `dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` |
| `owned-source/mbx/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | `archive-owned-source-2026-10-04-mbx-ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` |
| `owned-source/mbx-action/c3cbe8e56ccb4727624df45022357f49d2953075` | `c3cbe8e56ccb4727624df45022357f49d2953075` | `archive-owned-source-2026-10-04-mbx-action-c3cbe8e56ccb4727624df45022357f49d2953075` | `c3cbe8e56ccb4727624df45022357f49d2953075` |
| `owned-source/mbx-action/62ec0713473dffeab46884b7c03906042794e696` | `62ec0713473dffeab46884b7c03906042794e696` | `archive-owned-source-2026-10-04-mbx-action-62ec0713473dffeab46884b7c03906042794e696` | `62ec0713473dffeab46884b7c03906042794e696` |
| `owned-source/semver-checker/583dddce84706786fc54c41a2c768c28a09c65fd` | `583dddce84706786fc54c41a2c768c28a09c65fd` | `archive-owned-source-2026-10-04-semver-checker-583dddce84706786fc54c41a2c768c28a09c65fd` | `583dddce84706786fc54c41a2c768c28a09c65fd` |
| `owned-source/cache-action/8758d976a1b25eb387f48aa04ea86f57739b84cf` | `8758d976a1b25eb387f48aa04ea86f57739b84cf` | `archive-owned-source-2026-10-04-cache-action-8758d976a1b25eb387f48aa04ea86f57739b84cf` | `8758d976a1b25eb387f48aa04ea86f57739b84cf` |

Ruleset `protect-tags` (ID `24397132`) was queried at `03:24:36Z`. GitHub
reported `enforcement: active`, `target: tag`, rules `deletion` and
`non_fast_forward`, and no bypass actors. This is the currently observed
protection state, not a claim that organization owners cannot change it.

At that same ref scan, these three additional MBX branches remained present
and were excluded from the six-ref retirement scope:

| Excluded branch | Tip at `03:24:36Z` |
|---|---|
| `owned-source/mbx/1cde36b9f519f1fea8bf4ba8d82721906410448c` | `1cde36b9f519f1fea8bf4ba8d82721906410448c` |
| `owned-source/mbx/3e462cf9248dd3d17baf9131af5f7a4aec76b80e` | `3e462cf9248dd3d17baf9131af5f7a4aec76b80e` |
| `owned-source/mbx/7fd10972216ce478478b704781b4a4f2715f46d4` | `7fd10972216ce478478b704781b4a4f2715f46d4` |

## Historical commit reachability

On the integration checkout, `git merge-base --is-ancestor` succeeded for
each of the 15 unique custom commits below against its corresponding anchor
tag. `git rev-list --count` over the six anchor tags returned **12,194 unique
reachable commits**. This local Git graph check complements the independent
fresh-download proof in `README.md`; it does not establish active workflow use.

| Inventory ID | Commit | Anchor tag suffix |
|---|---|---|
| MISE-1 | `dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` | `mise-dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` |
| MBX-1 | `1ca12eb48391061a75e97d32e5b06fedf8a6253d` | `mbx-ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` |
| MBX-2 | `2f324e89af509a1c61d802cf23741a04d7510b57` | `mbx-ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` |
| MBX-3 | `ee250ac37654a4cfbb55b6cd470f2a257204bbe9` | `mbx-ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` |
| MBX-4 | `e07c07cfec773897e5c439043901e661ad3f7b45` | `mbx-ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` |
| MBX-5 | `29865b5a18e3414084ff5db27d1da455772b4c34` | `mbx-ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` |
| MBX-6 | `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | `mbx-ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` |
| ACT-1 | `f053f215866af0ddb6d2f32ecc19d45a5d25edc2` | `mbx-action-c3cbe8e56ccb4727624df45022357f49d2953075` |
| ACT-2 | `198f0d1a538d34a91d7692f8f302259643c0f737` | `mbx-action-c3cbe8e56ccb4727624df45022357f49d2953075` |
| ACT-3 | `06f353d41002af758d27490164f53c82e2165637` | `mbx-action-c3cbe8e56ccb4727624df45022357f49d2953075` |
| ACT-4 | `c3cbe8e56ccb4727624df45022357f49d2953075` | `mbx-action-c3cbe8e56ccb4727624df45022357f49d2953075` |
| ACT-5 | `62ec0713473dffeab46884b7c03906042794e696` | `mbx-action-62ec0713473dffeab46884b7c03906042794e696` |
| SEM-1 | `d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a` | `semver-checker-583dddce84706786fc54c41a2c768c28a09c65fd` |
| SEM-2 | `583dddce84706786fc54c41a2c768c28a09c65fd` | `semver-checker-583dddce84706786fc54c41a2c768c28a09c65fd` |
| CACHE-1 | `8758d976a1b25eb387f48aa04ea86f57739b84cf` | `cache-action-8758d976a1b25eb387f48aa04ea86f57739b84cf` |

## Rollback and remaining gates

The immutable release and six protected tags give two recovery routes. For a
branch-name rollback, first confirm the exact tag target and that the old branch
name is absent, then recreate only that one branch from its flat tag; never use
force. Example for the mise snapshot:

```sh
git fetch origin refs/tags/archive-owned-source-2026-10-04-mise-dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96:refs/tags/archive-owned-source-2026-10-04-mise-dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96
git push origin refs/tags/archive-owned-source-2026-10-04-mise-dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96:refs/heads/owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96
```

For bundle recovery, download the eight immutable release assets and follow the
released [`archive-plan.md`](https://github.com/tailrocks/velnor-new/releases/download/archive-owned-source-2026-10-04/archive-plan.md).
It contains the checksum-before-tar commands. From the parent directory with
the verified assets in `./archive`, restore the main bundle in a fresh bare
repository like this:

```sh
git init --bare restored-main.git
git -C restored-main.git bundle verify ../archive/six-owned-source-refs.bundle
git -C restored-main.git fetch ../archive/six-owned-source-refs.bundle '+refs/*:refs/*'
git -C restored-main.git fsck --full --strict --no-reflogs
git -C restored-main.git for-each-ref --format='%(objectname) %(refname)'
```

Repeat in a separate fresh bare repository for each of the 24 extracted
external bundles, then compare refs, commits, trees, and linked OIDs with the
release inventories. The immutable release and protected tags are separate
recovery routes. Neither route proves that Actions accepts every historical
full-SHA pin after branch retirement.

## Active temporary hold

**TEMPORARY-HOLD — retain all six source branch refs.** The archive is
recoverable, but final consumer/history closure and source cleanup have not
finished. The 86-head consumer scan (`e4c03ed1fa48f6397b03996eb6972bd3585bb783`)
found no exact snapshot refs or SHAs in the scanned default/open-PR trees; that
does not close known owner decisions or replace a final scan after rollout.
Deep audits still record the fixture's live scheduled/manual workflow removal
under PR #172, stale workflow state in `tailrocks/velnor#1133`, and unfinished
ChainArgos #2085 MBX cache qualification. Forty-five consumer default CI
workflows still stage Velnor Actions `0.1.0`; `ChainArgos/java-monorepo` stages
generator `d40868152f7fe0106e3ede858a411f502f00810f`. Ordered migration to the
replacement artifact remains pending.

Velnor PR #12 is also an unresolved current ref, outside the six deletion set:
at `2026-10-04T03:34Z` the API reported open draft head
`c694d8029eb880db639fa89b0589090dd2b15364`, base
`c57c700459bbe1549fe7eedcb7d8689585c38986`, and zero reviews/comments. The
integrated PR #12 disposition binds this exact c694 head/base and all 252
changed paths, with no new diff at its recorded refresh. That disposition is
based on integration tree `f7d38268...`; its Foundation/custom-source closure
remains `PARTIAL`/`TEMPORARY-HOLD`. The current diff includes executable
Foundation workflow/CLI/orchestrator/renderer paths, custom-source release
readers, and Python publisher/build scripts. Refresh every path, caller, and
feedback disposition at the final integration SHA before closing or
superseding the PR.

| Gate | Owner | Next probe | Resolving event |
|---|---|---|---|
| Final ref and consumer closure | Velnor goal coordinator `@donbeave`, with each consumer owner | After source/release candidate is final, re-fetch all nine Velnor source refs and current default/open-PR union; rescan all ref names, 15 custom SHAs, newly found refs, and indirect workflow/action references. Close the fixture, Velnor #1133, and ChainArgos #2085 owner decisions against exact heads. | Every scoped repo has a recorded merged migration with exact post-merge CI, or a reviewed exact-head `NO-CHANGE`; no unresolved branch-name/full-SHA runtime caller remains. |
| PR #12 source closure | Velnor goal coordinator `@donbeave` and PR #12 disposition owner | Refresh the 252-path c694 disposition against the final integration SHA; classify executable Foundation/custom-source callers, preserved PR #12 fixes, and current feedback. | Final-SHA file/caller disposition is integrated and PR #12 is closed or explicitly superseded after final feedback refresh. |
| Six-ref deletion safety | Velnor goal coordinator `@donbeave`; independent deletion reviewer | At final integration SHA, recheck all six tag targets/protection, immutable archive retrieval, six branch tips, and required action/history ancestry. Test exact `uses@SHA` resolution if any current caller is found. | Independent review approves deletion of each exact branch after the caller and history gates above close. |

**Checkpoint:** reassess by `2026-10-11`; this is not automatic deletion. Keep
the three excluded MBX refs untouched. If a gate remains open, update this hold
with its exact dependency, owner, next probe, and resolving PR/SHA or release
event. Do not delete any branch while the hold remains.
