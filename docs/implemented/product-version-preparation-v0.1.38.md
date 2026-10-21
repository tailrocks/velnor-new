# Product version preparation for v0.1.38

This prepares the dependency-seed failure-isolation correction accepted during
Jackin PR1164 review. Excluding a cache-only job from Required did not prevent its
failure from failing the entire CI workflow. The existing native preview trigger
correctly requires successful CI, so an optional cache failure could block
publication even when all real quality gates passed.

SDK38 emits job-level `continue-on-error: true` only for validated typed
cache-only dependency seeds, including root and nested workspaces. Mixed
execution, report, control, untyped or privileged jobs fail validation instead of
receiving a failure mask. Real crate/check jobs, Plan, Required, source/CI
eligibility, signing, publication and protected approvals remain strict. Producer
success and cancellation still control cache writes. See
[optional seed isolation](optional-dependency-seed.md) for the implementation,
original GitHub sources and qualification limits.

The normal branch starts at upstream main
`46e354a40e4f3d8f4c7c24a0b410206c7ee236c1`, preserving its cache-pruning and
Playwright mirror changes. Fourteen workspace package versions and twenty-four
exact internal path requirements advance to 0.1.38. Cargo controls the lock
update; all 128 external package records retain their versions, checksums and
resolved dependency graphs. The independently versioned runner, tool pins,
project tool configuration and historical manifests are unchanged.

The composed regression builds actual root and nested seed jobs through the
orchestrator and renderer. The old emission produces no optional jobs and fails
against the two expected seeds. The corrected emission isolates exactly those
seeds while all real jobs remain strict. Separate mutations exercise mixed
roles, inherited write grants, untyped steps and misleading display names.

This is version preparation, not publication or a successful consumer retry.
Full root and nested checks, independent review, hosted exact-source CI,
protected release publication, public verification and consumer adoption remain
separate requirements.
