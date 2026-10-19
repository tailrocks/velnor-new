# Native Rust capacity phases

This change replaces the combined Rust crate job with fixed lint, tests and
docs execution responsibilities. It does not establish cold capacity or preview
publication. Every concrete job retains the existing five-minute ceiling.

The enabling condition was serial grouping, not an absent timeout. Jackin main
[CI run 39214070421](https://github.com/jackin-project/jackin/actions/runs/39214070421),
attempt 1, canceled its CLI and workflow crate jobs. Clippy took 136 and 98
seconds respectively. CLI test compilation used another 150 seconds before
cancellation. Workflow test compilation took 124 seconds, tests 16 seconds,
doctests one second, and documentation was canceled after 40 seconds. Tool
acquisition took only 12–13 seconds. Same-tree PR CI passed with a materially
different cache state; that result is not cold-run evidence.

The Rust adapter owns the closed task-kind mapping. The orchestrator groups by
package, unchanged configuration and phase. It assigns collision-safe base IDs
once and then appends the fixed phase suffix. Planner entries and emitted
executions use that same assignment. OpenTofu root jobs keep their namespace
and staging; collocated Rust and OpenTofu obligations have separate owners.

Lint contains formatting and Clippy. Tests contains executable building, test
execution and doctests, keeping compiled binaries and their consumers on one
runner. Docs contains documentation building. Empty phases do not emit jobs.
Same-phase gates remain ordered local obligations. Other runnable gate owners
become exact job dependencies. Required depends on every phase and still checks
the exact report inventory, task digests and job conclusions. There is no
generic phase configuration, arbitrary task matrix, compiled-artifact transport
or old unsuffixed Rust execution path.

The [adoption amendment](../reviews/pr-1-adoption.md) explicitly supersedes the
earlier single-job topology under the later five-minute requirement. Logical
obligation and execution manifest schemas remain unchanged; their job-owner
binding reflects the new graph.

The additional literal jobs also exercise the existing 500,000-byte workflow
ceiling. Over-cap workflows may share identical run scalars, environment/input
maps and whole steps through workflow-local YAML anchors. The renderer compares
the complete expanded typed document before and after sharing, preserving field
order, scalar types, expressions and annotations. Expansion has independent
depth, node and byte bounds. Job identities, dependencies, permissions, outputs
and execution scopes remain literal; generated composite actions do not use
this projection. The staged shell validator expands supported aliases before
checking scripts under each use site's effective shell. Neither the size limit
nor the five-minute execution limit is relaxed.

The focused 0.1.20 qualification ran 64 tests successfully, including the
150-member fixture with 453 literal Rust phase jobs, exact task inventory,
unchanged 500,000-byte admission and staged validators. This exposed and fixed
two representation errors before acceptance: compact flow-map action inputs
must remain intact, and whole-step sharing must remove declarations whose last
literal alias disappeared. Quoted-key field maps remain unshared when the
staged normalizer does not admit that spelling. Tests also execute the real
ShellCheck rejection for aliased Bash code used under a POSIX shell. The Rust
adapter's complete 206-test package suite and dependency audit pass. These
results establish generator correctness for those checks, not hosted cold
compilation capacity or completion of the full release qualification.

An independent CLI generation of the same synthetic consumer shape produced
410,267 bytes for those 453 phase jobs in 7.328 seconds. The synthetic manifest
is schema-only fixture data, never installation or release evidence. Regenerated
Velnor repository CI contains 48 literal jobs, all capped at five minutes,
including 39 phase owners for 13 selected packages. Independent inspection
matched all 77 task records to exactly one correct owner and confirmed that
Required and source-bound helper verification retain their complete inventory.

Retry artifacts bind run and attempt. A failed-only rerun cannot obtain a fresh
Plan and must fail closed. Jackin attempt 2 demonstrated that rejection before
Rust execution; it provides no warm-cache or capacity evidence. A complete
workflow rerun creates a fresh Plan and reports, but does not by itself prove
the cold path fits.

Qualification still requires focused and regression tests, executable generated
graph admission, all root repository gates, independent final-source review,
an official source-bound generator release, consumer regeneration, and real cold
execution of every phase below its budget. Preview's six payloads require their
own fixed leaf/aggregate protocol and measured capacity; this CI partition does
not shorten or qualify the current serial preview producer. Scoped credentials
and trusted exact-head review-completion evidence remain separate promotion
prerequisites. Publication and unattended promotion must stay disabled until
those requirements are satisfied.
