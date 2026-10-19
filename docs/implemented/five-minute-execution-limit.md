# Bounded job timeouts

Every generated job has a fixed timeout from one to ten minutes. The default
is five minutes. Rust crate jobs use ten minutes. The configuration rejects
zero and values above ten. It does not change task lists or skip checks.

The [job log for run 39228409103](https://github.com/tailrocks/velnor-new/actions/runs/39228409103/job/117981938699)
on commit `b5784d13283e8c44871d0b200709653210a82f9d` shows the limit. Setup
took 36 seconds. The build took 44 seconds. Tests took 205 seconds. Cleanup
took 15 seconds. These stages used 300 seconds. GitHub stopped the job at its
five-minute limit. Ten minutes provides five more minutes for work and delay.

The contract defines the range and defaults. Workflow factories use that
contract. Workflow admission rejects missing, dynamic, or oversized limits.
Reusable workflow callers cannot declare a job timeout. Their local callees
must match the canonical inventory. Each executable callee job has a fixed
timeout. The generator rejects external or missing callees.

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

These timeouts limit each job. They do not limit the full workflow. Queue time,
environment approval, other jobs, and approved recovery attempts have separate
lifetimes. GitHub starts cancellation at the job timeout and has cancellation
grace. Scoped native deadlines do not stop every blocked system call or pass
automatically to new threads. See the
[pinned runner-source reference](../proposed/github-actions-reference.md).

The canceled Jackin run 39133387860 spent 73 minutes 21 seconds inside an opaque
native operation after 45 seconds of bootstrap. Its captured output did not
identify the internal stage. A prior partially cached six-payload diagnostic
took 37 minutes 37 seconds, with each CLI target already exceeding five minutes.
Those results do not prove that the builds fit the ten-minute limit. Required
artifacts remain unqualified until a measured run completes within that limit.

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
