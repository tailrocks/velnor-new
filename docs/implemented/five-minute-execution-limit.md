# Job timeout limits

Every generated job has a constant timeout from one to ten minutes. The default
is five minutes. Rust crate jobs use ten minutes. The configuration rejects
zero and values above ten. The change does not remove tasks or checks.

The [job log for run 39228409103](https://github.com/tailrocks/velnor-new/actions/runs/39228409103/job/117981938699)
on commit `b5784d13283e8c44871d0b200709653210a82f9d` shows the limit. Setup
time was 36 seconds. Build time was 44 seconds. Test time was 205 seconds.
Cleanup time was 15 seconds. The time for these four stages was 300 seconds.
GitHub stopped the job at its five-minute limit. A ten-minute limit gives five
more minutes for job execution.

The contract gives the range and defaults. Workflow factories use that
contract. Workflow admission rejects missing timeout values, expressions, and
values outside the permitted range. Reusable workflow callers cannot have a
job timeout. Their local callees must agree with the canonical inventory.
Each executable callee job has a constant timeout. The generator rejects
external or missing callees.

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

These timeouts limit each job. They do not limit the full workflow. Job
timeouts do not include queue time, environment approval, other jobs, or
approved recovery attempts. GitHub starts cancellation at the job timeout.
Cancellation can continue after that timeout. Scoped native deadlines cannot
stop every blocked system call. A new thread starts without the scoped native
deadline of its parent thread. See the
[pinned runner-source reference](../proposed/github-actions-reference.md).

The canceled Jackin run 39133387860 spent 73 minutes 21 seconds inside an opaque
native operation after 45 seconds of bootstrap. Its captured output did not
identify the internal stage. A prior partially cached six-payload diagnostic
took 37 minutes 37 seconds, with each CLI target already exceeding five minutes.
Those results do not show that the builds can complete within ten minutes.
A measured run that completes within that limit is necessary for artifact qualification.

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
