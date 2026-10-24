# Immutable preview API qualification

## Draft lookup and source-scoped concurrency (2026-10-22)

The [REST get-release-by-tag endpoint](https://docs.github.com/en/rest/releases/releases#get-a-release-by-tag-name)
documents lookup of a published release; it cannot discover a draft by tag.
GitHub CLI handles this case in its official
[`fetchDraftRelease`](https://github.com/cli/cli/blob/7948e8056e45ac3c729b829e0eafe2d0797a95c6/pkg/cmd/release/shared/fetch.go):
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
with the original run ID as the manual-recovery fallback). CI-completion runs
serialize per source; manual recovery serializes per original run. Separate main
commits do not replace each other in one global pending slot, and running
publication is not cancelled. Exact-source CI and `A <= B <= H` ancestry gates
admit historical releases; only current protected-main A can promote Homebrew.

The source above records the 2026-10-22 Velnor GitHub CLI 2.103.0 baseline at commit
`7948e8056e45ac3c729b829e0eafe2d0797a95c6`. The fetched source has SHA-256
`301e98fdab7932f43ce2ca74edaeef17d888885bf99ce1ce495e431bb4b5b666`.
A live read-only query of Jackin’s existing `preview` release returned
`databaseId = 328385904` and `isDraft = false`; it did not mutate that release.

## GitHub CLI 2.103.0 source qualification (historical 2026-10-22 baseline)

The 2026-10-22 Velnor source baseline pinned GitHub CLI 2.103.0 at commit
`7948e8056e45ac3c729b829e0eafe2d0797a95c6`; this baseline is not yet in a
public SDK release. The fetched tag and commit sources matched.
[`fetch.go` at v2.103.0](https://github.com/cli/cli/blob/7948e8056e45ac3c729b829e0eafe2d0797a95c6/pkg/cmd/release/shared/fetch.go)
has SHA-256
`301e98fdab7932f43ce2ca74edaeef17d888885bf99ce1ce495e431bb4b5b666`.
It is byte-identical to the v2.102.1 copy at commit
`89d7dcec017194af1d6f8853176b2dae9c1da762`. The GraphQL tag lookup yields a
draft's numeric database ID, then REST fetches that exact release by ID.

The [v2.103.0 API command](https://github.com/cli/cli/blob/7948e8056e45ac3c729b829e0eafe2d0797a95c6/pkg/cmd/api/api.go),
[`http.go`](https://github.com/cli/cli/blob/7948e8056e45ac3c729b829e0eafe2d0797a95c6/pkg/cmd/api/http.go),
and [API HTTP client](https://github.com/cli/cli/blob/7948e8056e45ac3c729b829e0eafe2d0797a95c6/api/http_client.go)
were compared with v2.102.1. Repeated `--raw-field`, `--field` and `--header`
arguments retain the same parsing and transport behavior, so explicit JSON
Accept and API-version headers are preserved. The observed v2.103.0 change is
refresh-aware authentication plumbing; it does not change the publisher's
GraphQL request or header semantics. This is source-only compatibility evidence,
not hosted runtime qualification. Evidence files and comparison
notes are retained under
`/Users/donbeave/Projects/release-evidence-20261022/jackin-immutable-preview/gh2103-qualification/`.

## GitHub CLI 2.103.2 bounded source comparison (2026-10-24)

Velnor's current reviewed GitHub CLI pin is 2.103.2 at verified commit
`4f2cfe92e3499f8581bf39db77e0a1431fa7b55c`. The bounded comparison from the
2026-10-22 baseline commit `7948e8056e45ac3c729b829e0eafe2d0797a95c6` covers
11 commits and 32 changed files. It changes no `pkg/cmd/api/*`,
`pkg/cmd/release/*`, or `pkg/cmd/factory/*` path. At both immutable refs,
`pkg/cmd/api/api.go` is byte-identical (SHA-256
`5c416146a1dbd87b2a57d63bd597b0a1191c05c3657132349d99c22b6db05a3c`),
`pkg/cmd/api/http.go` is byte-identical (SHA-256
`9e4256cdd216bb763c519e6eb04453b6558ea8cb3464ce8bfd96a9b6826406b6`), and
`pkg/cmd/release/upload/upload.go` is byte-identical at 4,100 bytes (SHA-256
`8a276af9cba13142c6481b68f82f245b489137fe11a2281108d0849b0d41e15e`). The
release-create path is also identical (SHA-256
`d18e9145ef9ebf06457773f1a3befe30777a30e4645d82b4d741616e6cdea257`). The
full compare and raw-source hashes are retained in the 2026-10-24 freshness
evidence record and its private source addendum.

The inspected `gh api` HTTP implementation therefore retains its absolute-URL
handling. Velnor's release publisher uses `gh api` with a validated absolute
`uploads.github.com` URL and passes the GitHub token through the child
environment; it does not call `gh release upload` or the issue/PR attachment
command. Both CLI tags still pin `github.com/cli/go-gh/v2` v2.17.0, whose
inspected client normalizes the uploads subdomain for GitHub token selection.
This supports the specific API transport and absolute release-asset request
path inspected here. It is not a claim of general CLI semantic equivalence,
a live upload qualification, or hosted runtime qualification.

## Workflow-run source and authority observation

GitHub's [workflow-run event table](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run)
defines `GITHUB_SHA` as the default branch's last commit and `GITHUB_REF` as
that branch. The completed upstream run remains a separate payload identity.
A read-only observation on 2026-10-22 found a distinguishing public example:
[downstream run 39771302142](https://github.com/openai/codex/actions/runs/39771302142)
and its job `119799234869` report authority B
`61f34c35af77777db4d77d0ea2aeb8547a6b4523`, branch `main`, and the base
repository as both repository identities. Its default checkout fetched B.
The log's `CLI_RUN_ID=39760554254` identifies the
[upstream push run](https://github.com/openai/codex/actions/runs/39760554254),
whose source A is `de8e7df8a026d30ca5e7b98fd0be8fef9249a4b0`.
Thus current downstream REST run/job heads bind B even when triggering A differs.

This is provider identity evidence, not a hosted Jackin release qualification.
The observed upstream push was a release tag, not a main-branch push, and
`GITHUB_WORKFLOW_SHA` was not directly logged. The bounded API metadata and
checkout/log excerpts are indexed under
`/Users/donbeave/Projects/release-evidence-20261022/jackin-immutable-preview/workflow-run-authority-audit/`.
The capture index SHA-256 is
`f3ab41274201aaf9794e366ec25989fc88d9109c807881ef0099cfc55293b7ef`.
