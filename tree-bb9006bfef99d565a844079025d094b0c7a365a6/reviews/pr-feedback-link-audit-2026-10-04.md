# PR feedback link audit — 2026-10-04

This note records link-reply chronology only. It does not change any review
finding, resolution state, or merge decision.

| Pull request | Finding thread | Existing fixing-link reply | Later duplicate reply | Fixing commit |
|---|---|---|---|---|
| #26 | `4173921736` | `4176530041` at `2026-10-04T07:08:40Z` | `4176800237` at `2026-10-04T08:42:41Z` | [`8480ddb5ee655dfc3f6c6c04360ad6c073980f0b`](https://github.com/tailrocks/velnor-new/commit/8480ddb5ee655dfc3f6c6c04360ad6c073980f0b) |
| #26 | `4173921740` | `4176530138` at `2026-10-04T07:08:42Z` | `4176800308` at `2026-10-04T08:42:43Z` | [`8480ddb5ee655dfc3f6c6c04360ad6c073980f0b`](https://github.com/tailrocks/velnor-new/commit/8480ddb5ee655dfc3f6c6c04360ad6c073980f0b) |
| #27 | `4175334377` | `4176530194` at `2026-10-04T07:08:43Z` | none | [`d4f4823c66cb39b1e5aac8c1731b065304d48e74`](https://github.com/tailrocks/velnor-new/commit/d4f4823c66cb39b1e5aac8c1731b065304d48e74) |

The PR26 duplicate replies resulted from querying only the intermediate replies
and missing sibling replies under each root thread. They repeat existing commit
links; they are not new fixes or dispositions. They remain in the thread. The
PR27 fixing URL was present before this audit and received no duplicate reply.

For future mutations, enumerate all paginated review comments by root thread,
including sibling replies and resolved or outdated threads, then refetch the
thread before posting.
