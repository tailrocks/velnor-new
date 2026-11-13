# GitHub's cache rules

The rules that `velnor-cache-rules` encodes, with the source of each. Documentation means [Dependency caching reference](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching), under the heading in the "Section" column. Probe means a recorded observation of real GitHub from `local/conformance/` (#633). Modelled means the documentation does not state the rule, so the module follows the nearest stated rule, and the row says which.

No probe file exists yet, so no rule below rests on a probe. When `local/conformance/` holds a file that contradicts a row, the row changes to Probe and the test that covers it changes with it.

Each test below is in `tests/it/` and is named after its rule.

| Rule | What the module does | Source | Section | Tests |
|---|---|---|---|---|
| 1a | A run reads its own scope first. A `pull_request` run then reads the base branch. Every run then reads the default branch. A scope that repeats is read once | Documentation | Restrictions for accessing a cache | `read_scopes` |
| 1b | A run cannot read the scope of a sibling branch, a child branch, another tag, or another pull request | Documentation | Restrictions for accessing a cache | `read_scopes`, `special_events` |
| 1d | The head branch of a pull request is not readable by that pull request. The restrictions section supports this (a pull request run reads its merge ref, its base and the default branch). The Cache key matching example lists the `feature` branch as the first lookup for a pull request from `feature`, which contradicts it | Modelled. Probe case for #633 | Restrictions for accessing a cache, against the example in Cache key matching | `read_scopes` |
| 1c | The scopes are searched in order, and each is searched completely (exact key, key as prefix, restore keys) before the next | Documentation | Cache key matching, the `npm-feature-` example | `read_scopes`, `matching` |
| 2a | A `pull_request` run writes to `refs/pull/<n>/merge` | Documentation | Restrictions for accessing a cache | `write_scope` |
| 2b | A push writes to `refs/heads/<branch>` | Documentation | Restrictions for accessing a cache ("current branch") | `write_scope` |
| 3a | The exact `key` with an equal `version` is tried first | Documentation | Cache key matching | `matching` |
| 3b | Then keys that start with `key` | Documentation | Cache key matching | `matching` |
| 3c | Then each restore key in order. An entry whose key equals the restore key wins. Otherwise the newest entry that starts with it | Documentation ("When a key doesn't match directly, the action searches for keys prefixed with the restore key") | Cache key matching | `matching` |
| 3d | Among several matches of one prefix, the entry with the latest `created` wins | Documentation for restore keys ("the most recently created cache"). Modelled for the key as prefix | Cache key matching | `matching` |
| 3e | `version` must be equal. A different version never matches | Documentation | Cache key matching ("Cache version") | `matching` |
| 4 | An entry with the same key and version in the same scope cannot be created again. The same key and version in another scope can | Documentation | `cache` action usage ("You cannot change the contents of an existing cache") | `immutability` |
| 5a | An entry whose last access is more than 7 days before `now` is evicted. Exactly 7 days is kept | Documentation | Usage limits and eviction policy | `limits` |
| 5b | When the total size reaches the limit, entries are evicted, least recently accessed first, until the total is less than the limit. A total equal to the limit evicts. Entries of every scope share one pool | Documentation: "will begin evicting caches until the total size is less than the repository limit" | Usage limits and eviction policy | `limits` |
| 5d | Among entries with the same last access, the one created earlier goes first, then the one with the smaller scope, key and version. The documentation names only the last access date | Modelled | Usage limits and eviction policy | `limits` |
| 5c | `REPOSITORY_LIMIT_BYTES` is 10 000 000 000. The documentation says "10 GB" and gives no unit, so the smaller reading is used. A cache that evicts early is slow, never wrong | Modelled | Usage limits and eviction policy | `limits` |
| 6a | A tag run reads its own tag scope, then the default branch, and writes `refs/tags/<name>`. It cannot read another tag | Documentation | Restrictions for accessing a cache ("different tag names") | `special_events` |
| 6b | A `merge_group` run uses its queue branch as a branch: it reads that branch, then the default branch, and writes that branch. The documentation does not mention merge queues. The queue branch is a `refs/heads/gh-readonly-queue/...` branch, so the rules for branches apply. The section "Cache access for low-trust workflow triggers" lists the only triggers that can create or overwrite caches in the default branch's scope (`push`, `workflow_dispatch`, `repository_dispatch`, `delete`, `registry_package`, `page_build`, `schedule`) and gives read-only access to other triggers that resolve to the default branch. It does not apply: `merge_group` resolves to its queue branch, not to the default branch, and it never writes the default branch scope here | Modelled from rules 1 and 2 | Restrictions for accessing a cache; Cache access for low-trust workflow triggers | `special_events` |
| 6c | A pull request from a fork is the same event as any pull request: it writes `refs/pull/<n>/merge` and reads that scope, the base branch and the default branch. The documentation names forks only for reading the base branch. It states no write restriction. Velnor's own policy that a fork only reads is not a rule of GitHub and is not in this module | Documentation for reading. Modelled for writing | Restrictions for accessing a cache, and the note under Input parameters | none, because the event has no fork field |

## Notes

- Rule 3b: the exact key and the restore keys are not the only steps. The documentation puts the keys that start with `key` between them.
- The documentation example for a pull request names the `feature` branch as the first scope. The sentence after it says a pull request creates its caches for the merge ref and that only re-runs of that pull request restore them. The module follows the restrictions section: the merge ref is the pull request's own scope, and the head branch is not readable (rule 1d).

## Probe cases for #633

- Rule 1d: a pull request run from `feature` looks up an entry saved by a push to `feature`.
- Rule 3b: among several entries that start with `key`, which one is restored.
- Rule 5c: whether the limit of "10 GB" is decimal or binary.
- Rule 5d: which entry goes first among equal last access times.
- Rule 6b: a `merge_group` run reads the queue branch and the default branch, and nothing else.
