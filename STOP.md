# Immediate-stop preservation — 2026-10-05

The original PR finalization goal was stopped at the user's explicit instruction. No new agents, implementation, tests, reviews, fixes, or merges were started during delivery. WIP checkpoints bypassed hooks and contain unchanged partial work, including unresolved conflict markers. They are not completion or validation claims.

## Returned subagent output

- archive25: stopped merge/CI watching; PR41 bfc947f2 ready-event CI passed, not merged. PR25 f17e2e18 and PR94 1557aabb were still running; cancellation requested. PR56 validator tools were unavailable. Existing PR bodies and CI logs copied into evidence.
- cache_impl: PR38 b08bc122 clean; ad109 parent had 3163 passed/1 skipped. b08 only focused restore, build and goldens had been rerun. Unresolved parent-symlink cleanup finding and seed-import/restore-miss ordering concern remain untouched. Discarded local duplicate d038384 retained remotely via reflog ref.
- docs18: PR56 7f4b24af plus seven unfinished combined-election/semantic Step equality edits. No tests started for this segment; edits checkpointed as-is. Pending conflict-map descendant interrupted before initialization.
- integration_coordinator: six baseline ZIP/listing modules returned in PR28. Focused 25/25 passed before stop; parser-only independent review passed. Callsite integration was unfinished. No active processes.
- verification55_impl: PR28 38a6cf7a plus unfinished merge, six conflicted golden files and integration/test edits. Existing full run had 1259 passes/20 failures; none baseline retrieval/ZIP. One scale expectation change had a focused pass before stop. Original index, merge metadata, staged/unstaged patches and both PR28 stashes preserved. Conflict markers deliberately remain in WIP checkpoint.
- qualification12_delivery: PR94 1557aabb clean and pushed; focused 11 archive tests passed before stop, full gates previously on 6c2d058. Logs and scanner source retained. No processes or descendants active.
- runner25: PR25 f17e2e18 clean and pushed; aggregate and detached postmerge worktrees returned. Two local image builds and raw 23-input/50-artifact receipts retained. Four probe containers and volume were already removed. No local process active.
- release_impl (previously completed): PR41 bfc947f2 clean/pushed; native/protected publication not performed.
- release_review_final (previously completed): parser-slice P2 closure approved; parent integration/staging-root authority explicitly outside approval.
- tofu_restore_impl (previously completed): existing 23-input and 50-artifact proof hashes matched; no source writes.
- pr64_sync (previously completed): conflict guidance only, no files/index/stash changes.
- Five pending-init agents had no produced work and were interrupted; completed agents were not restarted.

## Delivery records

`preserved-refs.tsv` lists every remote preservation branch, exact SHA, and source ref/path. `checkpoints.tsv` lists WIP commits. `worktrees.before`, `refs.before`, `stashes.before`, `clones.tsv`, metadata and original patches capture the initial state. Evidence copies and SHA256 records preserve existing logs, findings, commands, receipts and source snapshots. Cleanup records list only verified removals.

All new commits use Alexey Zhokhov <alexey@zhokhov.com> and ordered Codex co-author / Alexey signoff trailers. Existing historical commits are preserved without rewriting their original metadata.

Additional repo worktrees contributed 7 checkpoints (27 total at preparation), including old PR12 source/probes/retirement, native candidate and /var/tmp integration. Additional local refs/reflogs were pushed under additional/. Public/downloaded dependency caches and compiled outputs are generated state; authored source and existing receipts are preserved in split archives.
PR59 clone initially failed commit API verification (404), so its local heads/reflogs were explicitly pushed and verified under pr59-clone/. MBX action source commit was verified on its upstream; the earlier exception attribution was corrected.
