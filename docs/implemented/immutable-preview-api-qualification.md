# Immutable preview API qualification

## Draft lookup and source-scoped concurrency (2026-10-22)

The [REST get-release-by-tag endpoint](https://docs.github.com/en/rest/releases/releases#get-a-release-by-tag-name)
documents lookup of a published release; it cannot discover a draft by tag.
GitHub CLI handles this case in its official
[`fetchDraftRelease`](https://github.com/cli/cli/blob/89d7dcec017194af1d6f8853176b2dae9c1da762/pkg/cmd/release/shared/fetch.go):
it queries GraphQL `repository.release(tagName:)` for `databaseId` and `isDraft`,
then fetches the release by numeric ID through REST and verifies its state. The
[GraphQL Release fields](https://docs.github.com/en/graphql/reference/objects#release)
expose `databaseId`, `isDraft`, `isPrerelease`, `immutable`, and `tagName`. Native
publication uses this protocol to distinguish an exact existing draft from a
missing release; a failed published-only REST lookup does not authorize creation
or replacement.

GitHub Actions [concurrency groups](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#concurrency)
support `github` and `inputs` contexts in their expressions. By default, only
one run may wait in a group and a newer pending run replaces the older pending
run ([concurrency behavior](https://docs.github.com/en/actions/concepts/workflows-and-actions/concurrency)).
Preview workflows therefore scope the group to the source SHA (`workflow_run.head_sha`,
with the original run ID as the manual-recovery fallback). Runs for the same
source serialize, while separate main commits do not replace one another in one
global pending slot. Running publication is not cancelled, and the exact-source
CI and latest-main checks still decide whether an older run may proceed.

The source above matches pinned GitHub CLI 2.102.1 at commit
`89d7dcec017194af1d6f8853176b2dae9c1da762`. The fetched source has SHA-256
`301e98fdab7932f43ce2ca74edaeef17d888885bf99ce1ce495e431bb4b5b666`.
A live read-only query of Jackin’s existing `preview` release returned
`databaseId = 328385904` and `isDraft = false`; it did not mutate that release.
