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

## GitHub CLI 2.103.0 comparison evidence

The current Velnor pin remains GitHub CLI 2.102.1. A read-only source
qualification compared v2.103.0 commit
`7948e8056e45ac3c729b829e0eafe2d0797a95c6` with v2.102.1 commit
`89d7dcec017194af1d6f8853176b2dae9c1da762`; fetched tag and commit sources
matched. [`fetch.go` at v2.103.0](https://github.com/cli/cli/blob/7948e8056e45ac3c729b829e0eafe2d0797a95c6/pkg/cmd/release/shared/fetch.go)
is byte-identical to the v2.102.1 copy, with SHA-256
`301e98fdab7932f43ce2ca74edaeef17d888885bf99ce1ce495e431bb4b5b666`.
Therefore the draft lookup behavior described above remains supported in
v2.103.0: the GraphQL tag lookup yields a draft's numeric database ID, then
REST fetches that exact release by ID.

The [v2.103.0 API command](https://github.com/cli/cli/blob/7948e8056e45ac3c729b829e0eafe2d0797a95c6/pkg/cmd/api/api.go),
[`http.go`](https://github.com/cli/cli/blob/7948e8056e45ac3c729b829e0eafe2d0797a95c6/pkg/cmd/api/http.go),
and [API HTTP client](https://github.com/cli/cli/blob/7948e8056e45ac3c729b829e0eafe2d0797a95c6/api/http_client.go)
were compared with v2.102.1. Repeated `--raw-field`, `--field` and `--header`
arguments retain the same parsing and transport behavior, so explicit JSON
Accept and API-version headers are preserved. The observed v2.103.0 change is
refresh-aware authentication plumbing; it does not change the publisher's
GraphQL request or header semantics. This is source-only compatibility evidence,
not a pin update or hosted runtime qualification. Evidence files and comparison
notes are retained under
`/Users/donbeave/Projects/release-evidence-20261022/jackin-immutable-preview/gh2103-qualification/`.
