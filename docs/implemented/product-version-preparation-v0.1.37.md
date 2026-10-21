# Product version preparation for v0.1.37

This prepares a native producer timeout correction. Jackin preview run
39583938014 completed five payloads, including macOS ARM in seventeen minutes
forty-nine seconds. Its Linux ARM job was canceled by the twenty-minute outer
job limit while release compilation and linking remained active. Setup consumed
eighty-eight seconds before the native operation began. A preceding successful
Linux ARM payload took eighteen minutes thirty seconds. These observations do
not establish an MBX regression, an exhausted resource, or the remaining build
time when cancellation occurred.

Previously the outer job and native operation shared the same numeric budget
but started at different times. The earlier outer clock could cancel execution
before the operation's deadline and cleanup reserve. The shared native producer
contract now accepts operation budgets of one through thirty minutes, retaining
five as the default. Each payload and assembly job receives five additional
minutes for setup, artifact retention and job cleanup, for a maximum of
thirty-five. This finite allowance is not a guarantee that every setup or cold
build fits. Runtime still uses its original monotonic start, and process cleanup
remains reserved inside the configured operation budget.

Preview and stable share this single producer factory and range. Signer,
publisher, eligibility, recovery and Tap bounds are unchanged, as are crate-job
policy, source authorization, artifact checks, signatures and protected approval.
The configured operation deadline is not extended by the outer allowance.

Regression coverage checks actual rendered payload and assembly envelopes at
operation budgets one, five and thirty for both channels, leaving all other
native jobs at five. Contract and runtime tests reject zero and thirty-one;
runtime coverage preserves elapsed time, nested ceilings and cleanup reserve.
Restoring only the old producer factory makes the renderer test reject a
one-minute payload job where its required job envelope is six minutes.

The first full verification run exposed an independent cancellation-test harness
readiness timeout and leaked descendant. The harness now owns its process group
through all error paths, separates bounded startup from termination, and tests
missing-readiness cleanup explicitly. Production cache behavior is unchanged.

The fourteen workspace package versions and twenty-four exact local dependency
requirements advance to 0.1.37. Cargo controls the lock update; all 128 registry
package records retain their previous versions, checksums and dependency graphs.
The independently versioned runner, tool catalog and project tool configuration
remain unchanged. Historical manifests are preserved.

This is preparation, not official publication or a successful Jackin retry.
Full root and nested-runner checks, independent review, hosted exact-source CI,
protected release publication, public artifact verification, and consumer
adoption remain separate requirements.
