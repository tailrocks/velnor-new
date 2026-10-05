# Upstream custom ancestry evidence — 2026-10-05

**Scope:** read-only ancestry and tree-diff verification for the 15 Appendix A commits in `velnor-owned-source-implementation-goal-part-2.md`, plus four later MBX snapshot commits found on current `owned-source/*` refs. This is evidence for source closure, not authorization to delete refs or a full consumer audit.

## Finding

All 15 requested commit objects resolve in isolated mirrors of their named official upstream repositories after the official refs and peeled tags were verified. The four newer MBX commits also resolve. The 19 unique custom commits are not reachable from the captured official heads or tags. Their upstream parents, merge parents, full identities, author dates, first-parent deltas, generated `dist/` output, tests, and the MBX vendored Bats closure are recorded below.

The MISE no-config behavior in `dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` has since been accepted upstream as `dfe74a90b41603625ee6aabecb42f14a1f5eb0f6` and is an ancestor of official `v2026.10.2`. The other private feature groups remain separate from their upstream base commits. MBX v1.21.1, action v1.6.0, and actions/cache v6.1.0 are independently available from official refs.

## Method and pre-import proof

Five fresh mirrors were cloned directly from the official GitHub repositories with `--mirror --filter=blob:none`. Before importing any Velnor snapshot objects, every captured `refs/heads/*` and `refs/tags/*` entry was resolved with `rev-parse`, checked with `cat-file`, and, for tags, peeled with `rev-parse <tag>^{commit}` and checked with `cat-file`. Annotated tag object IDs and peeled commit IDs were retained. Only after this proof were the exact Velnor snapshot tips fetched into `refs/custom-snapshots/*` in those disposable mirrors. This audit did not modify shared Velnor refs, the Git index, or product code; this report is the authorized passive workspace addition.

| Official source | Mirror `main` at clone | Verified heads/tags | SHA-256 of complete pre-import ref proof |
|---|---|---:|---|
| [`jdx/mise`](https://github.com/jdx/mise) | `e19e8cbefdbc588852314e0bbd465ca9e381fb61` | 996 | `929355b6a98f7f2cd85665c3f94c4ae76bb7ef3af47870ea52bb1f840c85ba7e` |
| [`jdx/mr-boxington`](https://github.com/jdx/mr-boxington) | `27bc6a265560dfce39dea7fe235a235d1efdf889` | 317 | `391426fdd98c04b74c25514c296af2ba19d70c0bbb68d6caf8098c5b7b8ba4f4` |
| [`jdx/mr-boxington-action`](https://github.com/jdx/mr-boxington-action) | `e26ba57049f8656bcb3b1abcb721ede14fafbc03` | 16 | `eb7ccb7e8398f05eb960d2754187bee7dbc7dc30e7c52c0505193c5e24bfbee3` |
| [`obi1kenobi/cargo-semver-checks`](https://github.com/obi1kenobi/cargo-semver-checks) | `260fad71517e8b320c5c1ad1f367900ab37362a1` | 93 | `ca001594cf4868017a7b644f8fe3a8a33602f4905d0ff594ae92982931373f1d` |
| [`actions/cache`](https://github.com/actions/cache) | `3edfce9056124e459a23f683a21433670d47daca` | 198 | `08b1d05c0dde162e0f9bbf8d6e70336057440c0dd30034253d5aa5129e21baa1` |

The complete proof transcripts were `mise-official-ref-proof.txt`, `mbx-official-ref-proof.txt`, `action-official-ref-proof.txt`, `semver-official-ref-proof.txt`, and `cache-official-ref-proof.txt` under `/tmp/velnor-official-ancestry-20261005/`. Their SHA-256 values above bind each full ref listing and object/peel result. The five official mirror names in that directory are `jdx-mise.git`, `jdx-mr-boxington.git`, `jdx-mr-boxington-action.git`, `obi1kenobi-cargo-semver-checks.git`, and `actions-cache.git`.

Selected official tag evidence (annotated tag object → peeled commit where applicable):

| Source tag | Verified object and peeled commit |
|---|---|
| mise `v2026.10.0` | `f2a9db076328ed4c6f93fef665c14bf733b6b7f5` → `bc11f90c74eba23bf0d7350efb540e62fb7d9ffd` |
| mise `v2026.10.1` | `b752bdc18b1b5961e4f9ba01a63bc3c11a0a1f79` → `050ce5a20287a0aafd872b1191699a5fdafff5ac` |
| mise `v2026.10.2` | `f0ff293c4b40ac6bd74b99af9b329c2c62762e6f` → `44ea2537166efbe21b19d808355d9914e830941a` |
| MBX `v1.21.0` | `5d49803a7e200df12b188329aa796842472bfaa3` → `201b9df3d18e8e96831bee631035f6b7c7ae20e0` |
| MBX `v1.21.1` | `fb284e2c05308e32b3a35347ebf14321229f7766` → `a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313` |
| MBX action `v1.5.0`, `v1.6.0` | Lightweight tags at `9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3`, `1687e54eb349cadf61fa38b5813a77875489e8e6` |
| cargo-semver-checks `v0.50.0` | Lightweight tag at `4297e8b5f6306531375ba2ba332171e5792b4c38` |
| actions/cache `v6.1.0` and `v6` | Both lightweight tags at `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` |

The Velnor source-ref snapshot was captured at `2026-10-05T03:15:11.524696793Z`. Its `git ls-remote` transcript is `velnor-owned-source-ls-remote.txt` in the same scratch directory, SHA-256 `d4c77681219519585f5f903662e68c1a76fafafc35bf896d761e991fc83f5e1c`. Every scratch ref target below was checked against that transcript.

| Velnor source ref | Tip |
|---|---|
| `owned-source/cache-action/8758d976a1b25eb387f48aa04ea86f57739b84cf` | `8758d976a1b25eb387f48aa04ea86f57739b84cf` |
| `owned-source/mbx-action/c3cbe8e56ccb4727624df45022357f49d2953075` | `c3cbe8e56ccb4727624df45022357f49d2953075` |
| `owned-source/mbx-action/62ec0713473dffeab46884b7c03906042794e696` | `62ec0713473dffeab46884b7c03906042794e696` |
| `owned-source/mbx/1cde36b9f519f1fea8bf4ba8d82721906410448c` | `1cde36b9f519f1fea8bf4ba8d82721906410448c` |
| `owned-source/mbx/3e462cf9248dd3d17baf9131af5f7a4aec76b80e` | `3e462cf9248dd3d17baf9131af5f7a4aec76b80e` |
| `owned-source/mbx/7fd10972216ce478478b704781b4a4f2715f46d4` | `7fd10972216ce478478b704781b4a4f2715f46d4` |
| `owned-source/mbx/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` |
| `owned-source/mbx/c0996a090ae992de83a5131889e28342a04f4d9b` | `c0996a090ae992de83a5131889e28342a04f4d9b` |
| `owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` | `dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` |
| `owned-source/semver-checker/583dddce84706786fc54c41a2c768c28a09c65fd` | `583dddce84706786fc54c41a2c768c28a09c65fd` |

## Commit ancestry and complete tree deltas

Deltas are exact no-renames parent-to-commit tree diffs using `git diff --no-renames --binary --full-index`; line totals are additions/deletions. First-parent counts are used for merges and therefore include upstream merge changes. The separate second-parent rows below isolate each merge's private overlay. Files are grouped by path: source/config, tests, generated `dist/`, or MBX5's vendored Bats closure. Counts do not say that any test was run.

| ID and full commit | Parent(s), full SHA | Author = committer; author date UTC | Subject; delta; disposition |
|---|---|---|---|
| MISE-1 `dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` | `bc11f90c74eba23bf0d7350efb540e62fb7d9ffd` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-02 22:13:05Z` | `fix: isolate owned Cargo wrapper dispatch`; +403/−1, 9 files (source +347/−1, 8; tests +56, 1). `ADOPT-UPSTREAM` no-config behavior; `DELETE after G0` owned Cargo wrapper/shim/banner. |
| MBX-1 `1ca12eb48391061a75e97d32e5b06fedf8a6253d` | `201b9df3d18e8e96831bee631035f6b7c7ae20e0` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-02 20:29:58Z` | `feat(cache): expose useful snapshot comparison and retention`; +2,247/−57, 15 files (source +1,668/−57, 13; tests +579, 2). `DELETE after G0`: no demonstrated Velnor production caller for this custom comparison/retention protocol. |
| MBX-2 `2f324e89af509a1c61d802cf23741a04d7510b57` | `1ca12eb48391061a75e97d32e5b06fedf8a6253d` + `a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-02 20:32:56Z` | `chore: merge official MBX 1.21.1 for cache qualification`; first parent +3,534/−403, 57 files includes upstream merge. Second parent `v1.21.1`: +2,247/−57, 15 files. `ADOPT-UPSTREAM` official v1.21.1; `DELETE` the private merge as a delivery mechanism. |
| MBX-3 `ee250ac37654a4cfbb55b6cd470f2a257204bbe9` | `2f324e89af509a1c61d802cf23741a04d7510b57` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-02 22:12:37Z` | `fix: restore immutable Cargo root pairs`; +3,942/−1,365, 36 files (source +2,361/−1,338, 25; tests +1,581/−27, 11). `DELETE after G0`: no demonstrated owned root-pair caller. |
| MBX-4 `e07c07cfec773897e5c439043901e661ad3f7b45` | `ee250ac37654a4cfbb55b6cd470f2a257204bbe9` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-02 23:20:47Z` | `feat: verify native cache ownership and measure compiler events`; +2,036/−129, 21 files (source +1,163/−63, 14; tests +873/−66, 7). `DELETE after G0`: no demonstrated stock-consumer requirement for this fork protocol. |
| MBX-5 `29865b5a18e3414084ff5db27d1da455772b4c34` | `e07c07cfec773897e5c439043901e661ad3f7b45` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-03 00:59:08Z` | `chore: close pinned behavioral test sources`; +38,438/−29, 530 files (source +5,336/−17, 5; tests +5/−8, 1; vendored Bats/test-helper +33,097/−4, 524). `DELETE after G0` vendored dependency; recorded as source closure, not test-pass evidence. |
| MBX-6 `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | `29865b5a18e3414084ff5db27d1da455772b4c34` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-03 16:12:30Z` | `feat: preserve MBX lineage and durable execution state`; +22,519/−843, 169 files (source +13,726/−750, 113; tests +8,793/−93, 56). `DELETE after G0`: report-v2/lineage protocol has no approved compatible consumer in the reviewed pairing. |
| MBX-extra `1cde36b9f519f1fea8bf4ba8d82721906410448c` | `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-03 23:38:04Z` | `test(mbx): create private completed-report fixtures`; +9/0, 1 test file. `HOLD` branch: outside the six-ref retirement scope. |
| MBX-extra `7fd10972216ce478478b704781b4a4f2715f46d4` | `1cde36b9f519f1fea8bf4ba8d82721906410448c` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-04 00:24:28Z` | `test(mbx): preserve typed coverage uncertainty`; +45/−5, 2 files (source +2/−2, 1; tests +43/−3, 1). `HOLD` branch: outside scope; preserve for caller review. |
| MBX-extra `3e462cf9248dd3d17baf9131af5f7a4aec76b80e` | `7fd10972216ce478478b704781b4a4f2715f46d4` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-04 02:27:17Z` | `test(mbx): fix CMake shim containment assertion`; +5/−2, 1 test file. `HOLD` branch: outside the six-ref retirement scope. |
| MBX-extra `c0996a090ae992de83a5131889e28342a04f4d9b` | `3e462cf9248dd3d17baf9131af5f7a4aec76b80e` | OpenAI Codex `<codex@openai.com>`; `2026-10-04 03:09:30Z` | `fix(mbx): accept Cargo JSON diagnostic modifiers`; +134/−15, 1 source file. `HOLD` branch pending MBX caller audit; product-source change is outside authorized retirement scope. |
| ACT-1 `f053f215866af0ddb6d2f32ecc19d45a5d25edc2` | `9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-02 19:58:31Z` | `feat: support verified executable and useful cache snapshots`; +740/−113, 11 files (source +264/−40, 5; dist +73/−73, 1; tests +403, 5). `DELETE after G0`: no approved owned comparison pairing. |
| ACT-2 `198f0d1a538d34a91d7692f8f302259643c0f737` | `f053f215866af0ddb6d2f32ecc19d45a5d25edc2` + `1687e54eb349cadf61fa38b5813a77875489e8e6` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-02 20:01:14Z` | `merge: retain reviewed v1.6 cache trust policy`; first parent +1,091/−217, 20 files includes upstream merge. Second parent `v1.6.0`: +758/−115, 11 files (source +273/−42, 5; dist +73/−73, 1; tests +412, 5). `ADOPT-UPSTREAM` official v1.6.0; `DELETE` private merge mechanism. |
| ACT-3 `06f353d41002af758d27490164f53c82e2165637` | `198f0d1a538d34a91d7692f8f302259643c0f737` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-02 20:44:57Z` | `security: verify caller-bound MBX binary digest before execution`; +158/−102, 9 files (source +30/−15, 4; dist +73/−73, 1; tests +55/−14, 4). `KEEP-IN-VELNOR` the process-boundary verification requirement if retained; `DELETE after G0` the foreign action implementation. |
| ACT-4 `c3cbe8e56ccb4727624df45022357f49d2953075` | `06f353d41002af758d27490164f53c82e2165637` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-02 23:14:37Z` | `fix: require protected default branch for cache saves`; +21/−12, 4 files (source +5/−5, 1; dist +1/−1, 1; tests +15/−6, 2). `KEEP-IN-VELNOR` explicit protected-default/event save policy; `DELETE` foreign action fork after G0. |
| ACT-5 `62ec0713473dffeab46884b7c03906042794e696` | `c3cbe8e56ccb4727624df45022357f49d2953075` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-03 02:44:56Z` | `feat(action): bind late export baseline and group`; +142/−77, 6 files (source +23/−3, 3; dist +71/−71, 1; tests +48/−3, 2). `DELETE after G0`: no activated consumer for the late export group. |
| SEM-1 `d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a` | `4297e8b5f6306531375ba2ba332171e5792b4c38` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-03 01:05:22Z` | `feat: add native supplied-document metadata checks`; +867/−3, 10 files (source +589/−3, 9; tests +278, 1). `DELETE after G0`; retain the test driver only as passive evidence; no wrapper-equivalence claim. |
| SEM-2 `583dddce84706786fc54c41a2c768c28a09c65fd` | `d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-03 01:43:53Z` | `ci: restrict workflow lint push to main`; +2/0, 1 workflow. `DELETE after G0`: fork CI policy, not upstream runtime behavior. |
| CACHE-1 `8758d976a1b25eb387f48aa04ea86f57739b84cf` | `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` | Alexey Zhokhov `<alexey@zhokhov.com>`; `2026-10-03 04:04:13Z` | `feat: own cache archives and foundation qualification`; +120,449/−93,453, 117 files (source/config +11,365/−2,098, 105; tests +211/−14, 4; generated `dist/` +108,873/−91,341, 8). `ADOPT-UPSTREAM` official actions/cache v6.1.0; `DELETE after G0` archive/SDK framework; `HOLD` Foundation pin until compiled/generated source closure is detached. |

The first-parent merge patch manifests are in the scratch ledgers. Independently, the complete second-parent diff hashes are MBX-2 `15c6cb5c492e21a2ff4d906912d54a221ae7e182c09e665a34ea1b528fe1bba1` and ACT-2 `addd6fe81f288bedb11d1417f24495dee16c8999d5ff4af1dcb9dd5c5a197f2d`, computed with `git diff --no-renames --binary --full-index <official-parent> <merge>` and SHA-256. They prove the overlay range against the official version without counting the upstream merge as custom work.

Each commit row is backed by `git show -s --format='%H %P %an <%ae> %aI %cn <%ce> %cI %s'` and a full parent-to-commit diff. The reusable source-side manifests are `mise-diff-manifest.tsv`, `mbx-diff-manifest.tsv`, `action-diff-manifest.tsv`, `semver-diff-manifest.tsv`, `cache-diff-manifest.tsv`, `mbx-diff-classification.tsv`, and `action-diff-classification.tsv` under `/tmp/velnor-official-ancestry-20261005/`. In each mirror, custom-only reachability was checked with `git rev-list <refs/custom-snapshots/*> --not --branches --tags`; all 19 listed identities fell outside the captured official heads/tags.

The MISE source diff separates the accepted behavior from the private additions: the `src/config/miserc.rs` no-config guards correspond to upstream `dfe74a90b41603625ee6aabecb42f14a1f5eb0f6`; the custom `src/config/mod.rs` hunk conditionally dispatches the private owned Cargo wrapper. `git merge-base --is-ancestor dfe74a90b41603625ee6aabecb42f14a1f5eb0f6 44ea2537166efbe21b19d808355d9914e830941a` succeeds for `v2026.10.2` and fails for `v2026.10.1` (`050ce5a20287a0aafd872b1191699a5fdafff5ac`). The accepted upstream commit's parent is `3f0b40f0697f35374db98d5a2f298657ce9e66a0`; its author is Alexey Zhokhov and its subject is `fix(config): skip miserc discovery with --no-config (#13926)`.

## MBX test-source closure

MBX commits 1–4 each carry the same four mode-160000 gitlinks. Their paths, object IDs, and `.gitmodules` URLs are:

| Path | Gitlink commit | URL recorded in `.gitmodules` |
|---|---|---|
| `test/bats` | `ae4b94d7cc35f62468297791aa4ab8c3af7377ba` | `https://github.com/bats-core/bats-core.git` |
| `test/test_helper/bats-assert` | `697471b7a89d3ab38571f38c6c7c4b460d1f5e35` | `https://github.com/bats-core/bats-assert.git` |
| `test/test_helper/bats-file` | `6bee58bec7c2f4aed1a7425ccd4bdc42b4a84599` | `https://github.com/bats-core/bats-file.git` |
| `test/test_helper/bats-support` | `0954abb9925cad550424cebca2b99255d4eabe96` | `https://github.com/bats-core/bats-support.git` |

MBX-5 removes `.gitmodules` and those gitlinks, then vendors 524 files in the Bats/test-helper closure. The submodule commits were recorded from the parent trees and URLs; this audit did not separately clone those four repositories or verify their objects. Vendoring the harness is not evidence that MBX behavioral tests were executed or passed.

## Disposition and scope boundary

The labels above are proposed feature dispositions from the current owned-source review, not completed cleanup actions. `DELETE after G0` means preserve the immutable evidence and complete consumer/source closure before any separately authorized ref retirement. Official MBX v1.21.1 and action v1.6.0 are independently adopted versions; their private merge commits are not required as delivery mechanisms. The protected-branch cache-save rule and any required executable digest check remain Velnor-owned policy/process obligations, not a reason to depend on the foreign action fork.

The six exact retirement-scope refs are `owned-source/cache-action/8758d976a1b25eb387f48aa04ea86f57739b84cf`, `owned-source/mbx-action/c3cbe8e56ccb4727624df45022357f49d2953075`, `owned-source/mbx-action/62ec0713473dffeab46884b7c03906042794e696`, `owned-source/mbx/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81`, `owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96`, and `owned-source/semver-checker/583dddce84706786fc54c41a2c768c28a09c65fd`. The four extra MBX tip refs are outside that scope. Keep them and their snapshots intact pending a caller audit: three commits add completed-report, typed-coverage, and CMake containment test changes; `c0996a0…` changes the Cargo JSON artifact parser. In particular, `c0996a090ae992de83a5131889e28342a04f4d9b` contains product source and needs a specific consumer/adoption decision before disposition. No extra ref deletion is proposed here.

`G0` above refers to the evidence-closure gate in the owned-source implementation goal; the listed deletion dispositions remain conditional on that gate and the separately authorized cleanup.

The earlier API-derived [`owned-source-inventories.md`](owned-source-review-2026-10-04/owned-source-inventories.md) reports different aggregate totals for several MBX commits (for example MBX-1 +2,477/−105 versus the exact tree diff +2,247/−57; MBX-5 +27,533/−20 versus +38,438/−29). That file labels itself partial and API-derived. This report supersedes those totals with the captured commit-object tree diffs. The mechanism behind the earlier discrepancy was not established here; it is not attributed solely to merge accounting.

This audit did not scan every Velnor branch, every consumer, or every historical ref; did not run product tests; did not publish refs or change the shared Git index; and did not delete any branch or commit. It proves ancestry and exact tree deltas for the listed upstreams and captured source refs only. The `/tmp/velnor-official-ancestry-20261005/` transcripts are scratch evidence; the official OIDs, source tips, proof-log digests, diff method, key merge comparisons, and disposition limits are retained in this passive report.
