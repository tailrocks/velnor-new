# Five-minute execution limit

Every concrete generated GitHub Actions job has an explicit literal timeout of
one through five minutes. Five is the default and maximum. Configuration that
requests zero or more than five is rejected, including custom checks, native
builds, verification, preview production, suites, and tap updates. No job family
may substitute GitHub's implicit six-hour default or silently clamp a request.

The contract owns the range and defaults. Typed workflow factories use that
contract, and final workflow admission rejects missing, dynamic, or oversized
limits. Reusable workflow callers cannot declare a job timeout; their exact
canonical local callee inventory is admitted together, and each executable
callee job has its own bounded timeout. External or missing callees are rejected.

Native preview production starts one monotonic deadline before reading its
context, configuration, plan, or preparation. A shorter configured budget uses
the same original start. A synchronous scoped ceiling bounds transitive
preparation subprocesses; explicit deadlines remain ceilings and nested scopes
can only shorten them. The same budget reaches tool qualification, task
execution, artifact admission, staging, and proof publication. Process cleanup
reserves its existing bounded grace inside the admitted operation budget.

Timeouts fail closed. Descendants are killed through the existing process-group
runner. Partial or late proof staging cannot supply a successful handoff, and
an entry-owned guard removes only the proof created by the failed attempt.
Preexisting evidence and foreign replacements are preserved. Signing and
publication still require successful independently validated evidence; timeout
never grants approval or bypasses authenticated recovery.

These are execution budgets, not a five-minute total pipeline promise. Queuing,
environment approval, separate DAG jobs, and separately authorized recovery
attempts are distinct. GitHub starts cancellation at the job timeout and has
additional cancellation grace. Scoped native deadlines do not preempt arbitrary
blocking kernel operations or propagate implicitly to new threads. See the
[pinned runner-source reference](../proposed/github-actions-reference.md).

The canceled Jackin run 39133387860 spent 73 minutes 21 seconds inside an opaque
native operation after 45 seconds of bootstrap. Its captured output did not
identify the internal stage. A prior partially cached six-payload diagnostic
took 37 minutes 37 seconds, with each CLI target already exceeding five minutes.
The new policy does not claim those builds fit: they must fail within the budget
until measured build changes make the required artifacts available in time.

Regression coverage exercises configuration boundaries, all workflow families,
reusable callee admission, cumulative preparation and retry budgets, scoped
restoration, descendant termination, and proof cleanup after outer expiry.
