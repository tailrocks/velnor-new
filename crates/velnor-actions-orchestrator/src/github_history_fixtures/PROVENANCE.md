# Historical GitHub response fixtures

Captured read-only on 2026-10-27 from public `jackin-project/jackin` pull request 1179 and its immutable head commit. The three original captures are copied byte-for-byte from `/tmp/sdk58-history-independent-evidence-20261027/`; the ref response was captured directly for this test addition. Credentials and request headers are not stored. The repository-wide `REUSE.toml` aggregate annotation covers these files under Apache-2.0.

These fixtures establish API response shapes and cross-API identity agreement. They do not establish an authenticated tap promotion, a successful commit POST, or global removal of REST `merge_commit_sha`.

| File | SHA256 |
| --- | --- |
| `rest-pull-1179.json` | `f20dee25dd30d62d3ee101722608d139b27ad74b7eb55b530452a0293c858c8c` |
| `graphql-pull-1179.json` | `c02008d0ac97a2b7b201e0e0f07385e879e7b95d011cc4b81ca53a9b248f2895` |
| `rest-git-commit.json` | `1c882a4b9f185939b0427e3cd89c3818259eb945246ca2f472f6d903dea115ec` |
| `rest-git-ref.json` | `07576ff56ac045683769cc313c33a5910aa6aa3650087b9d0fef1499b3accd0b` |

Commands, with output redirected to the corresponding file:

```sh
gh api -H 'X-GitHub-Api-Version: 2026-03-10' repos/jackin-project/jackin/pulls/1179
gh api graphql -f 'query=query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){fullDatabaseId nameWithOwner pullRequest(number:$number){number state merged mergedAt headRefName headRefOid baseRefName baseRefOid headRepository{fullDatabaseId nameWithOwner} mergeCommit{oid}}}}' -F owner=jackin-project -F name=jackin -F number=1179
gh api -H 'X-GitHub-Api-Version: 2026-03-10' repos/jackin-project/jackin/git/commits/ea9238fcf2892578ddd23d6f283038ea269bd14a
gh api -H 'X-GitHub-Api-Version: 2026-03-10' repos/jackin-project/jackin/git/ref/pull/1179/head
```

The REST pull is closed and merged at `2026-10-22T22:39:28Z`, omits `merge_commit_sha`, and agrees with GraphQL on repository ID `1197700841`, head/base identity and timestamp. GraphQL supplies landed commit `9b296fc56a61bbc28db76805fe2bac730bc040eb`. Commit tree/parent objects omit `type`; the actual ref object includes `type: commit`.

Official source references: [REST pull requests](https://docs.github.com/en/rest/pulls/pulls#get-a-pull-request), [GraphQL pull requests](https://docs.github.com/en/graphql/reference/pulls), [GraphQL repositories](https://docs.github.com/en/graphql/reference/repos), [Git commit objects](https://docs.github.com/en/rest/git/commits), and [Git references](https://docs.github.com/en/rest/git/refs).
