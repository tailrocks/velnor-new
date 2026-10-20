# Job timeout limits

Every concrete generated GitHub Actions job has an explicit literal timeout of
one through 360 minutes. Five is the general default. Official product compiler
jobs have an explicit ten-minute budget. 360 is
GitHub's per-job execution limit on hosted runners
([Actions limits](https://docs.github.com/en/actions/reference/limits)).
Configuration that requests zero or more than 360 is rejected for general crate
jobs, custom checks, native builds, verification, and suites. Native preview
production accepts one through twenty minutes and tap-updater configuration accepts
one through five, both defaulting to five independently of the general workflow range. No job family may omit the
field, substitute GitHub's implicit default, or silently clamp a request.

Crate jobs (the one job per Rust crate and each OpenTofu root job) use
`[workflow] crate_timeout_minutes` (default `5`), a
[hard decision](../hard-decisions.md#2-configurable-crate-job-timeout). Hosted
cold builds can exceed five minutes before every check finishes; raising this
one value gives every crate job the same larger literal bound without changing
the one-job-per-crate topology. Parallel check steps are linked to the job's
cancellation, so the bound covers the whole job. Control jobs (plan, required,
validators, candidate, release and publish) keep the five-minute default except
that `[workflow] plan_timeout_minutes` may set the planning job independently
(default `5`, bounded `1..=360`). It does not change other control-job budgets.

The three official generator compiler jobs (Linux, macOS ARM64, and macOS Intel)
and the official `velnor-host` binary compiler job use ten minutes. This bound
covers checkout, tool setup, compilation, artifact upload, and job cleanup.
Their eligibility, qualification, signing, manifest, and publication jobs retain
five minutes; runner-image jobs and configurable consumer budgets are unchanged.
In [run 39471133008](https://github.com/tailrocks/velnor-new/actions/runs/39471133008),
macOS ARM64 compilation finished successfully in 4 minutes 35 seconds and its
artifact uploaded, but the complete job exceeded five minutes during cleanup.
Local MBX objects were not uploaded to a remote cache, so a warm retry was not
established. The compiler budget provides bounded setup and completion margin;
it does not make a cancelled build eligible for publication or change required
checks, evidence, retries, permissions, or protected approval.

```toml
[workflow]
crate_timeout_minutes = 20 # every Rust crate job and OpenTofu root job
plan_timeout_minutes = 15  # planning job only
```

The public Rust `WorkflowConfig` struct now has a `plan_timeout_minutes`
field. Downstream Rust struct literals must provide it; TOML and Serde inputs
that omit it continue to default to five, and default generated workflow bytes
remain unchanged. Explicitly setting five is equivalent to omitting the key.

The contract owns the range and defaults. Typed workflow factories use that
contract, and final workflow admission rejects missing, dynamic, or oversized
limits. Reusable workflow callers cannot declare a job timeout; their exact
canonical local callee inventory is admitted together, and each executable
callee job has its own bounded timeout. External or missing callees are rejected.

Native preview production starts one monotonic twenty-minute maximum deadline
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
The policy does not claim those native builds fit. The later explicit Jackin
CI/CD timeout authorization permits a larger producer budget. The native
producer ceiling is twenty minutes, retaining five as the default and the tap
updater ceiling. Actual builds must still finish
inside their configured producer budget; a larger setting is not capacity proof.
The general workflow limit cannot widen this independent native ceiling.
The later cold Jackin preview run 39346520177 exceeded its explicit ten-minute
producer jobs while legitimate release dependencies were still compiling, after
qualification and xtask bootstrap. Twenty minutes provides a bounded cold-build
window, not qualification that every payload fits. All six real payloads must
still complete and pass the unchanged SDK, artifact, signer and publisher checks.

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
