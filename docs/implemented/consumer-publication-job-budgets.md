# Consumer publication job budgets

The native `stacks.rust.binary_release.publication_timeout_minutes` setting
selects a bounded outer job budget. It defaults to five minutes and accepts
whole minutes from one through 360 through the existing `JobTimeout` contract.
Configuration validation and both public channel renderer entrypoints reject
invalid values, including values created through deserialization.

The setting applies to preview eligibility and to publisher and original-evidence
recovery jobs in both channels. Preview eligibility authenticates recovery twice,
before source checkout and during finalization; each phase can download and
verify the complete original signed artifact. Stable eligibility only observes
its original tag intent and CI identity, so its existing budget is unchanged.
Producer, signer, reconciliation and tap jobs keep their separate budgets.
Native API deadlines, source phase deadlines, repeated verification, privileges,
and immutable publication rules are unchanged.

## Hosted failure that requires the setting

Jackin automatic preview run
[`40027030223`](https://github.com/jackin-project/jackin/actions/runs/40027030223)
used SDK 0.1.48 at source
`7d4b8acea25d28fd5286c958e3890148fc2bf40e`. All six product/target producers,
assembly and signing succeeded. The publisher's signed artifact was 200,218,355
bytes. Its download took four minutes and eight seconds, followed by 44 seconds
of native publication before cancellation. GitHub check run `120677269779`
reported `The job has exceeded the maximum execution time of 5m0s`.
No immutable source tag or release existed after this interruption.

Jackin can explicitly select 15 minutes without changing built-in defaults.
This provides time for artifact transfer, independent verification, uploads and
readback, but is not a worst-case performance guarantee. Successful hosted
publication and original-evidence recovery remain required qualification.

## Recovery identity during source finalization

The supported manual recovery attempt
[`40035078656`](https://github.com/jackin-project/jackin/actions/runs/40035078656)
retained the successful signed candidate from run `40027030223`, attempt one.
Its first source-admission phase succeeded after approximately three minutes
and 31 seconds. Exact-source checkout then succeeded, but finalization failed
with `package_recovery_context_missing` before any publisher or recovery writer
ran. The generated selector received the original run ID and attempt; the
finalizer omitted both required environment bindings. No source tag or release
was created, and the historical rolling preview remained unchanged.

The existing closed native operation binding owner now supplies
`VELNOR_RELEASE_ORIGINAL_RUN_ID` and `VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT` to both
source phases through the same preview context. The selector's separate literal
binding block is removed. This preserves canonical ID validation and independent
receipt/provenance verification after checkout, without rebuilding, signing,
relaxing source custody, or borrowing the current recovery caller's identity.
