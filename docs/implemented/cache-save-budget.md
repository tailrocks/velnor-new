# Optional cache-save job budget

A native Cargo-source save blocked PR 205 CI run `39396624970`, attempt 1.
Plan started at 08:53:09 UTC on 2026-10-20. The helper finished its release
build at 08:56:16; the source save began at 08:56:18 and reported all
57,574,199 bytes sent at 08:56:19.543. It did not finish before cancellation
at 08:58:10.208, and the job annotation explicitly reported its five-minute
limit. Request creation and planning never ran. The 110-second gap after
100% sent identifies a cache finalization or action-exit wait, not a measured
upload-throughput or compiler failure. The exact remote cause is unknown.
Attempt 2 later passed; that does not remove this failure mechanism.

The Plan job now produces and publishes its plan before attempting its source
save. Typed native Cargo-source, tools and OpenTofu-provider save actions alone receive a literal
one-minute timeout and `continue-on-error`. Their existing eligibility guards
remain intact. Source saves retain their fetch-producer guard after unrelated
quality failures; the tools/provider success and trust policies are unchanged. No quality command, task,
report upload or verdict gets this exception.

A renderer-owned clock runs before the job's first original step. Before each
eligible save, admission checks the same original deadline, allowing the write
only with sixty seconds plus a thirty-second startup/cleanup reserve remaining.
A slow first write therefore cannot give a second write a fresh budget. Invalid
or unavailable clocks and outputs refuse the save without failing quality.
Failed or timed-out save outcomes emit a fixed `cache_unavailable` warning.

This is a first-user-step clock, not an observation of the runner's actual job
start. Runner setup precedes it, wall clocks can move, and action cancellation
cleanup may take time. The reserve is conservative; this is no unconditional
whole-job fit or hosted performance claim. Job timeout settings remain unchanged.

Keys, family ownership, paths, trust scopes, producer gates and cache ceilings
are unchanged. Same-repository PR source writes remain eligible when their
existing policy and the remaining budget permit them. The native caches remain
best effort as required by [the cache contract](../proposed/cache-contract.md).
