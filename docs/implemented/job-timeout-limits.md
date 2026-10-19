# Job timeout limits

Every concrete generated GitHub Actions job has an explicit literal timeout of
one through 360 minutes. Five is the default for every job family. 360 is
GitHub's per-job execution limit on hosted runners
([Actions limits](https://docs.github.com/en/actions/reference/limits)).
Configuration that requests zero or more than 360 is rejected for general crate
jobs, custom checks, native builds, verification, and suites. Native preview
production and tap-updater configuration instead accept only one through five
minutes, independently of the general workflow range. No job family may omit the
field, substitute GitHub's implicit default, or silently clamp a request.

Rust package jobs and OpenTofu root jobs use
`[workflow] crate_timeout_minutes`. The default is five minutes. The Velnor
configuration sets ten minutes for build and cleanup. Control jobs use the
five-minute default.

```toml
[workflow]
crate_timeout_minutes = 10 # timeout for each crate job
```

The contract owns the range and defaults. Typed workflow factories use that
contract, and final workflow admission rejects missing, dynamic, or oversized
limits. Reusable workflow callers cannot declare a job timeout; their exact
canonical local callee inventory is admitted together, and each executable
callee job has its own bounded timeout. External or missing callees are rejected.

Native preview production starts one monotonic five-minute maximum deadline
before reading its context, configuration, plan, or preparation. A shorter
configured native budget uses
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

These are per-job execution budgets, not a total pipeline promise. Queuing,
environment approval, separate DAG jobs, and separately authorized recovery
attempts are distinct. GitHub starts cancellation at the job timeout and has
additional cancellation grace. Scoped native deadlines do not preempt arbitrary
blocking kernel operations or propagate implicitly to new threads. See the
[pinned runner-source reference](../proposed/github-actions-reference.md).

The canceled Jackin run 39133387860 spent 73 minutes 21 seconds inside an opaque
native operation after 45 seconds of bootstrap. Its captured output did not
identify the internal stage. A prior partially cached six-payload diagnostic
took 37 minutes 37 seconds, with each CLI target already exceeding five minutes.
The policy does not claim those native builds fit: they must fail within their
one-through-five-minute budget until measured build changes make the required
artifacts available in time. The general configurable workflow limit does not
permit widening this native release boundary.

Regression coverage exercises configuration boundaries, all workflow families,
reusable callee admission, cumulative preparation and retry budgets, scoped
restoration, descendant termination, and proof cleanup after outer expiry.


Automatic preview begins after the canonical exact-commit CI workflow completes.
Its authenticated completion gate does not wait or poll for unfinished CI.
Manual product/stable release keeps its existing explicit authorization and
fails eligibility when CI is not already complete. This separates CI scheduling
from each job's execution budget without adding dispatch credentials or
misrepresenting the producing event.


Enabled native binary publication requires the exact generated CI name `CI`.
Configuration rejects custom names at the shared release-authority boundary,
before writing workflows. Generic workflows without native publication retain
custom names. This prevents a validated configuration from generating a preview
that can never receive its completion trigger.
