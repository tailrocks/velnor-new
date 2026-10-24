# Consumer publication budgets

The native `stacks.rust.binary_release.publication_timeout_minutes` setting
selects the native publication deadline and continues to bound preview source
eligibility. It defaults to five minutes and accepts whole minutes from one
through 333. Configuration validation and both public channel renderer
entrypoints reject invalid values, including values created through
deserialization.

Publisher and recovery job limits add the caps on their actual pre-operation
steps, the configured native deadline, and one minute for completion and
cleanup. Uncapped preparation steps receive the normal five-minute job-step
default. The existing six-minute `Acquire Velnor` cap is preserved. With the
current steps, a 15-minute operation renders a 42-minute publisher and a
32-minute recovery job. The publisher's 27 minutes of preparation and cleanup
leave room for the maximum 333-minute operation within GitHub's 360-minute job
limit. If rendered preparation steps change, generation recomputes their total
and rejects any combined job budget above that limit.

The setting applies to preview eligibility and to publisher and original-evidence
recovery jobs in both channels. Preview eligibility authenticates recovery twice,
before source checkout and during finalization; each phase can download and
verify the complete original signed artifact. Stable eligibility only observes
its original tag intent and CI identity, so its existing budget is unchanged.
Producer, signer, reconciliation and tap jobs keep their separate budgets.

Only publisher and recovery operation steps receive the literal
`VELNOR_RELEASE_PUBLICATION_TIMEOUT_MINUTES` binding. Runtime validates its
canonical decimal value before filesystem, credential or API work. An absent
binding retains five minutes; malformed or out-of-range values fail closed.
One absolute deadline covers verification, the complete publication transaction,
retained signed-directory custody and final readback within that invocation.
It is never recreated per asset. Shorter enclosing deadlines still win, and the
existing cleanup reserve remains inside the budget.

Initial read-only admission retains one five-minute aggregate scope that ends
before the upload loop. Nested archive and cryptographic verification scopes
can only shorten that admission scope, never refresh it between phases.
They do not shorten the returned custody deadline.
Each HTTP request retains its 90-second bound. Source phase deadlines, repeated
verification, privileges and immutable publication rules are unchanged. The
budget is not part of the signed release policy or receipt, so retained original
artifacts remain compatible. The outer job budget includes declared pre-operation
step caps and one minute of completion and cleanup headroom. A preparation step
that reaches its cap can still consume time that otherwise would remain before
the native deadline begins; this is a bounded lifecycle policy, not a guarantee
for runner startup or GitHub post-job processing.

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

The later automatic run
[`40038086236`](https://github.com/jackin-project/jackin/actions/runs/40038086236)
at source `41bad147b5ba9db09be99b07248531ce1bd867be` also completed every payload,
assembly and signer job. Its publisher ran native publication for four minutes
and 55 seconds before the same explicit five-minute job cancellation. It retained
the exact-source annotated tag and draft release `422399645`, with 19 of the
29 signed entries uploaded. The 200,309,470-byte signed artifact remained available.
No public release existed. Source inspection then identified two additional
five-minute ceilings: the absolute publication operation scope and the deadline
stored in the signed directory's custody closure. Raising only the outer job
budget would leave both ceilings in place. Publication now passes the same
configured absolute deadline through both, while preserving the shorter
admission, archive and HTTP limits.

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

## Retained tap verification input custody

Retained tap reconciliation verifies the original signed artifact during initial
loading and again immediately before a branch or merge mutation. These calls
run in the same native process. Recreating a fixed `release-recovery-input`
directory made the second verification fail with
`package_recovery_input_not_fresh`, even when every original identity and byte
remained valid. This was a source-proven path collision, independent of the
publication timeout and inventory size.

Each retained verification now allocates a distinct private directory under the
validated runner root. Its temporary-directory owner transfers into the sealed
byte custody object. Every shared custody reference keeps those exact inputs
alive; the final reference releases only its owned directory. Failed extraction
or verification releases the newly allocated input without deleting an earlier
held directory. Borrowed directory admission and the separate source-selection
and finalization input rules remain unchanged.

This allocation does not cache or skip original artifact acquisition, policy,
CI, identity, signature, attestation, published-byte or pre-mutation freshness
checks. Temporary path names do not become release identity. Tap job and native
deadlines remain separate from the configurable publication deadline; a live
Tap promotion is still required qualification.
