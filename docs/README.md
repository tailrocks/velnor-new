# Velnor technical documentation

This documentation separates work by delivery state:

| State | Meaning | Document |
|---|---|---|
| Proposed implementation | Normative Velnor V1 generator specification and build sequence. Nothing here is claimed to exist yet. | [Proposed V1 specification](content/docs/proposed/index.mdx) |
| Deferred implementation | Roadmap clauses that remain after the macOS Scale Set spec. Conflicting clauses are superseded. | [Deferred work](deferred/README.md) |
| Active runner specification | Native macOS Scale Set controller and generator routing. | [macOS Scale Set runner](proposed/macos-scaleset-runner.md) |
| Already implemented | Verified implementation records only, plus V1 Gate 0–8 records pending merge on `docs/velnor-actions-spec`. | [Implemented index](content/docs/implemented/index.mdx) |
| Research references | Primary documentation and tool references used by the specifications. | [References](references.md) |

Words **MUST**, **MUST NOT**, **SHOULD**, and **MAY** in proposed and deferred specifications define requirements. A recommendation marked **SHOULD** may be changed only by recording the reason and impact in the relevant decision record.

The former combined redesign plan has been replaced by these state-specific specifications. Do not use older conversation notes as an implementation source.
