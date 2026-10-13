# Product version preparation for v0.1.5

- State: candidate evidence only for open draft PR #116 at current head
  `ade9b0736fa227ec166a2606d75765c32919e40a`. This is not an implemented
  record until the PR merges with required checks passing. No v0.1.5 tag,
  dispatch, publication, or consumer adoption has been completed.
- Specification: [version policy](../proposed/version-policy.md) §§1–2;
  [bootstrap and release contract](../proposed/bootstrap-and-release-contract.md)
  §2.1.
- Candidate PR: [#116](https://github.com/tailrocks/velnor-new/pull/116),
  draft at the exact current head above.
- Delivered candidate scope: root workspace version `0.1.5` in `Cargo.toml`,
  inherited by its ten workspace packages; the nested `crates/velnor-runner`
  workspace stays excluded and at `0.1.0`. The generator release template
  separately owns `VERSION = "0.1.5"` and the three Linux x86-64, macOS ARM64,
  and macOS x86-64 asset identities. This record covers version preparation
  only; it does not qualify release assets or consumer use.
- Generated-output provenance: there is no dedicated version-preparation CLI
  subcommand. Supported `velnor-actions generate` writes the root `.github`
  tree; `velnor-actions generate --output-dir PATH` writes a preview under
  `PATH/.github`. The [T04 golden record](../proposed/opentofu-goldens/README.md#product-version-prep-2026-10-09)
  documents the capture/check workflow and parity-golden evidence for this
  preparation. Those are generation-specific records, not a passing full
  repository gate or release acceptance.
- Acceptance evidence: hosted Plan run
  [37868872201](https://github.com/tailrocks/velnor-new/actions/runs/37868872201),
  observed at `2026-10-09 01:20:11 UTC` for current head `ade9b07`, passed Plan
  job `113622031736`; CLI, orchestrator, runner CLI, and runner host jobs were
  still in progress, and Required had not been emitted. A prior PR run at the
  earlier `c330be5` head,
  [37868090646](https://github.com/tailrocks/velnor-new/actions/runs/37868090646),
  had failing CLI and orchestrator Clippy steps; it is not a passing result
  for the current head. Full local repository verification remains pending,
  and no current candidate required-check result is claimed as passing. Main CI run
  [37866554995](https://github.com/tailrocks/velnor-new/actions/runs/37866554995)
  succeeded at merge SHA `82a3b5931d431b85579435a99cf981bc6f248a65`; that
  result belongs to the base main commit and does not qualify PR #116.
- Manifest distinction: the checked-in fixture has dummy values and is not
  evidence of any published release. The official v0.1.4 manifest overlay
  used for renderer measurements is a separate byte-exact input staged only
  in disposable worktrees; it does not qualify an unchanged consumer input
  or establish release adoption. See the
  [renderer qualification](../reviews/pr-110-renderer-qualification.md) and
  [renderer record](workflow-run-scalar-sharing.md).
- Deviations: none approved.
- Follow-up: resolve the candidate's failing required checks and merge PR #116
  only after the required checks pass. Then separately verify the protected
  v0.1.5 publication and adoption with the released binary and official
  manifest against unmodified consumer inputs. Until then, no v0.1.5 tag,
  publication, or consumer adoption is claimed; the
  [generator publication record](generator-publication-qualification.md)
  keeps dispatch and hosted publication pending.
