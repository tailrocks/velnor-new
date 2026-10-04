# Native source bootstrap binding

Status: integration contract and bounded acceptance fixture. The fixture below
has not run. Descriptor binding alone does not qualify restored native bytes.

## Owner APIs

1. Resolve the actual runner with `tool_target_for_runner_label(label)`. Match
   its exact target against `DistributionHost::abi()`; reject an absent match.
2. Compile the native installation with
   `catalog::tool_prepare::helper_for_tools(catalog, domain, host, selectors,
   generator_version)`. Selectors come from `catalog.tool_specs`: Node for
   `NpmBootstrap`, Bun for `BunBootstrap`, OpenTofu for `TofuBootstrap`.
3. Derive the descriptor with
   `tool_producer_steps::descriptor_for_record(record, label, target, domain,
   mise_setup)`. Never synthesize selectors or qualification identities.
4. Bind that descriptor to `SourceProducer.tool_cache`. Build both the source
   report record and report step after binding, using identical metadata.
5. Prepend the exact three `tool_consumer_steps(descriptor, mise_setup)` steps:
   platform, readonly restore, compiled Mise bootstrap. Preserve their IDs,
   environments, conditions and arguments. Follow with exactly one native
   installation/verification record before any native source operation.
6. Inventory the same descriptor for the separate trusted `PureToolProducer`.
   Source consumers never gain tool-cache save authority. Source job scheduling,
   dependencies and outputs derive from its bound source metadata.

`source_records` and `producer_job` must receive the same canonical `MiseSetup`
and runtime version. Runtime versions remain the packaged factory boundary:
foreign versions cannot redefine helper source or qualify emitted records.

## Required compiled installation evidence

The owner record must bind exact native distribution, host, asset format,
archive checksum, source identity, executable/tree checksum, installation root
and selector. Version strings, path fragments and an unrelated qualified Mise
binary do not establish that binding. Before restored native execution, reject
escaping or linked ancestors, unexpected files, altered bytes, and ownership
outside the fixed domain payload. Verification precedes native version commands
and source collection. Recovery may discard invalid owned state and install
fresh qualified bytes; it must never execute invalid restored state.

The inspected generic factory currently binds a qualified Mise distribution,
then invokes selector installation and parses version output. This does not yet
prove the required native distribution/tree. Native factory migration must
preserve existing verification until the replacement evidence exists.

## Bounded independent acceptance fixture

Use one minimal source candidate per domain and each supported literal host.
Inspect the final generated IR and independently reconstruct owner records.

| Case | Required result |
|---|---|
| Canonical cold producer | Descriptor equals recomputed installation descriptor; first three steps equal canonical consumer steps; one native installation; source admission succeeds. |
| Warm tool restore | Native bytes verified before first native execution; unchanged useful state causes no tool save; source computation remains isolated. |
| Corrupt native executable | No native execution of corrupt bytes; discard/reinstall qualified state or fail before source use. |
| Corrupt native support tree | Same fail/repair rule, including Node's npm support files and native configuration/shims. |
| Symlink ancestor or escaping member | Reject before native verification or execution. |
| Version-looking wrong bytes | Reject even when output contains the pinned version. |
| Descriptor selector/hash/root/host mutation | Recomputed descriptor or owner-record admission rejects. |
| Missing/reordered/edited bootstrap step | Source admission rejects the prefix. |
| Duplicate native installation | Reject; native verification may not be bypassed by an earlier install. |
| Old unbound report record | Source owner evidence admission rejects. |
| Foreign runtime version | Factory rejects before source/helper registration. |
| Writer-role substitution | Source job cannot save tool state; tool producer cannot perform source computation. |

For cold/warm/corrupt cases, instrument native execution independently and
record the verification event before the first native event. Retain actual
archive/tree digests and final descriptor equality. Stand-ins can prove event
ordering but cannot prove native artifact qualification. Bun consumer payload
emission remains disabled until public attestation and payload guard proof.
