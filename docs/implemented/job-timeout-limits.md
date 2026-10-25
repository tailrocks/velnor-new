# Job timeout limits

Every concrete generated GitHub Actions job has an explicit literal timeout of
one through 360 minutes. Five is the general default. Official product compiler
jobs have an explicit ten-minute budget. The runner-image build job has its own
thirty-minute budget. 360 is
GitHub's per-job execution limit on hosted runners
([Actions limits](https://docs.github.com/en/actions/reference/limits)).
Configuration that requests zero or more than 360 is rejected for general crate
jobs, custom checks, native builds, verification, and suites. Native preview
native-operation production accepts one through thirty minutes and tap-updater configuration accepts
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

Dependency-seed jobs use `[workflow] dependency_seed_timeout_minutes` (default
`30`, bounded `1..=360`) independently of crate and plan budgets. Each MBX
workspace has one optional non-crate seed job that serially compiles test,
Clippy and documentation lanes for the whole workspace. A crate's budget is
not a budget for those three workspace-wide builds. The Cargo driver emits no
seed job. The public `WorkflowConfig` field has the same name; default values
are omitted from serialized configuration. Rust struct literals must supply
the new field.

Jackin main runs [39809903680](https://github.com/jackin-project/jackin/actions/runs/39809903680),
[39801216166](https://github.com/jackin-project/jackin/actions/runs/39801216166)
and [39797125851](https://github.com/jackin-project/jackin/actions/runs/39797125851)
reached their ten-minute seed envelope while the final documentation lane was
still compiling. The test and Clippy seed lanes succeeded; GitHub then cancelled
the documentation processes without a Rust diagnostic. The repeated timing is
consistent with the configured job limit; the API does not identify a canceller
or an explicit reason. Their required checks passed, but the cancelled overall
CI runs could not authorize native Preview publication. Thirty minutes provides
bounded headroom, not measured completion proof. The seed remains optional,
protected-push-only and outside Required, and cancelled or partial producers
still cannot save. Native publication still requires a successful exact-source
CI run.

The three official generator compiler jobs (Linux, macOS ARM64, and macOS Intel)
and the official `velnor-host` binary compiler job use ten minutes. The runner-image
`build-images` job uses its dedicated thirty-minute bound for tool setup,
two Docker builds, the source-bound resource-probe build and tests, artifact upload,
and job cleanup. Eligibility, ordinary candidate qualification, signing, manifest,
and publication jobs retain five minutes. Historical Homebrew A/D qualification
retains its separate thirty-minute bound; configurable consumer budgets are
unchanged.
In [run 39471133008](https://github.com/tailrocks/velnor-new/actions/runs/39471133008),
macOS ARM64 compilation finished successfully in 4 minutes 35 seconds and its
artifact uploaded, but the complete job exceeded five minutes during cleanup.
Local MBX objects were not uploaded to a remote cache, so a warm retry was not
established. The compiler budget provides bounded setup and completion margin;
it does not make a cancelled build eligible for publication or change required
checks, evidence, retries, permissions, or protected approval.

In [run 40159263908](https://github.com/tailrocks/velnor-new/actions/runs/40159263908),
both runner-image Docker builds and the resource-probe tests succeeded, but the
locked resource-probe producer build reached the five-minute job limit. Check
run `121068068300` explicitly reports that maximum execution time was exceeded.
The next [run 40171356786](https://github.com/tailrocks/velnor-new/actions/runs/40171356786)
used the ten-minute compiler budget. Its `build-images` job spent 3 minutes 29
seconds on both Docker builds, then installed and qualified the native producer.
The producer build started 6 minutes 20 seconds into the job and was still running
when check `121103166333` reported that the ten-minute execution limit was exceeded.
The dedicated thirty-minute image allowance responds to both measured timeouts;
thirty-minute hosted completion remains unmeasured. Signing and publication
descendants retain their existing budgets and authority checks, and the complete
producer run must still succeed before its SDK release can be adopted.

```toml
[workflow]
crate_timeout_minutes = 20 # every Rust crate job and OpenTofu root job
plan_timeout_minutes = 15  # planning job only
dependency_seed_timeout_minutes = 30 # each MBX workspace dependency seed only
```

Bun jobs resolve their bound most specific first:
`[stacks.bun.members."<name>"] timeout_minutes` for that member's job, then
`[stacks.bun] timeout_minutes` for every member job, then
`[workflow] crate_timeout_minutes`. The member value is validated by the same
contract check (`bad_timeout:<n>:expected_1..=360`, never clamped); the stack
value keeps its `must_be_between_1_and_360` code. Both are optional and omitted
from serialization, so a repository that sets neither generates the same bytes.
The public Rust `BunMemberConfig` struct gained a `timeout_minutes` field; it
derives `Default`, so downstream literals add `..Default::default()`.

```toml
[stacks.bun]
timeout_minutes = 30                          # every Bun member job

[stacks.bun.members."@chainargos/platform"]
timeout_minutes = 60                          # this member's job only
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

Native preview and stable production start one monotonic thirty-minute maximum deadline
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

Consumer preview eligibility and preview/stable publisher and recovery jobs use
`[stacks.rust.binary_release] publication_timeout_minutes` (default `5`, bounded
`1..=333`). The general `JobTimeout` contract remains `1..=360`; this stricter
limit leaves room for the publisher's declared preparation-step caps and one
minute of cleanup headroom. Publisher and recovery job timeouts sum their actual
pre-operation step caps, the configured native deadline, and that headroom.
Publisher and recovery native operations also receive the configured literal
budget. One absolute clock covers the full transaction and retained signed
custody; neither upload iterations nor readback refresh it. Separate admission
and archive subscopes stay at five minutes, individual HTTP requests stay at 90
seconds, and a shorter enclosing deadline still wins. The value does not change
signed release policy, producer or signer budgets, or publication authority.
A preparation step can consume time that otherwise remained before the native
deadline starts; see
[consumer publication budgets](consumer-publication-job-budgets.md) for the
hosted failures and recovery-identity qualification.

The canceled Jackin run 39133387860 spent 73 minutes 21 seconds inside an opaque
native operation after 45 seconds of bootstrap. Its captured output did not
identify the internal stage. A prior partially cached six-payload diagnostic
took 37 minutes 37 seconds, with each CLI target already exceeding five minutes.
The policy does not claim those native builds fit. The later explicit Jackin
CI/CD timeout authorization permits a larger producer budget. The native
producer native-operation ceiling is thirty minutes, retaining five as the default and the tap
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

SDK37 separates the native operation budget from the producer job envelope. Each
payload and assembly job has five additional minutes for setup, artifact retention
and job cleanup (maximum thirty-five minutes); the default native operation remains
five minutes and its job is ten. The lifecycle allowance does not reset or extend the configured native deadline. The
existing process-group cleanup grace remains inside the native budget.

Jackin run 39583938014 completed five payloads, but its Linux ARM job was canceled
by the outer twenty-minute limit while compilation and linking remained active.
Setup consumed eighty-eight seconds before native execution began, so the outer
limit preempted the native deadline. The preceding same-target success took
eighteen minutes thirty seconds; macOS ARM in the failed run succeeded in
seventeen minutes forty-nine seconds. These measurements justify bounded cold-build
headroom, not a claim of an MBX regression or a guarantee of completion. Signer,
publisher, control and Tap limits remain unchanged; crate jobs are independent.
