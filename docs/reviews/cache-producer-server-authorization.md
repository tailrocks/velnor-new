# Cache producer server authorization qualification

Status: source-qualified on 2026-10-03; hosted negative-save proof and repository-wide writer audit remain required.

## Supported boundary

GitHub documents workflow/job `cache-mode` as a scoped-token capability, enforced by its cache service. The four literal modes are `read`, `write`, `write-only`, and `none`. A job override replaces the workflow default. Reusable-workflow callers impose an explicit capability ceiling.

Emit workflow `cache-mode: read`; admit literal job `write` only for a closed pure producer. Gate the entire producer job on the existing qualified protected default-branch push policy. Expression-valued modes are not qualified. Changing `ACTIONS_CACHE_MODE` inside a process does not change the server token capability.

The pinned cache action bundles backend refusal handling: `CacheWriteDeniedError` identifies receiver-prefixed `cache write denied:` errors caused by a read-only JWT. Runner 2.337.0 consumes the server `actions_cache_mode` variable and exposes its effective value to JavaScript actions.

Primary evidence:

- [GitHub cache authorization contract](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#controlling-cache-access-with-cache-mode).
- [GitHub workflow/job syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#cache-mode).
- [Pinned cache action bundle](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/dist/restore-only/index.js), `CacheWriteDeniedError` and reservation/finalization handling.
- [Runner 2.337.0](https://github.com/actions/runner/blob/v2.337.0/src/Runner.Worker/Handlers/NodeScriptActionHandler.cs), server variable propagation.

## Scope of the guarantee

Cache tokens constrain jobs, not cache keys or payload namespaces. A default-scope workload job with write access can forge a source/tool producer key and matching payload digest. Pure producer roles alone do not remove that attack while any repository-executing job retains default-scope write access.

A pull-request workflow can request write access to its own merge-ref cache scope. An attacker can seed the same key there; later PR restores can prefer that shadow snapshot over the default-branch snapshot. A default-workflow read token does not prevent this independent PR-scope writer. The consumer must enforce authenticated default-producer provenance or a qualified service/API default-only lookup before admitting bytes. Cache-mode alone does not close SEC19.

Every writer in the repository must therefore be audited. MBX validation jobs need server read access and supported export through a pure fixed-namespace writer. Release and historical workflows require the same scope audit. A new generation separates old snapshots, but does not authenticate their creator. Pure writers must use fixed admitted keys/paths and never execute imported workload payloads.

Branch protection qualification remains separate. An unprotected default branch is not a qualified protected producer. Do not turn missing protection evidence into write authority.

## Validator qualification

Pinned actionlint 1.7.12 rejects both workflow and job `cache-mode` keys. Latest official release remains 1.7.12; main commit `011a6d15e749bb3f2d771eed9c7aa0e7e3e10ee7` also lacks those parser cases.

`scripts/actionlint-cache-mode.patch` extends upstream actionlint source at exact release commit `914e7df21a07ef503a81201c76d2b11c789d3fca`. It adds AST fields and literal enum parsing at the existing workflow/job grammar layers. It retains upstream alias resolution, duplicate-key validation, diagnostics and all other rules. Unknown values, wrong node types, empty values and expressions fail closed. Tests cover all four modes, source positions, aliases, duplicates, reusable calls and normal linter output. Independent review found and closed YAML scalar-tag coercion (`!!int write`); both workflow and job tests reject tagged nonstrings.

After updating upstream diagnostic goldens, `go test . ./cmd/actionlint` passes. The initial full `go test ./...` reached unrelated remote-source maintenance tests and failed on network access; that run also used earlier diagnostic goldens. No complete all-package pass is claimed. The qualified patch SHA-256 is `7c81196d799636344ea309336af5a11a4d67f2bd4ece3cee733765670dc00142`.

This source patch is not a distributed validator. Publication must bind the patched executable digest to upstream source plus patch digest, and workflows must consume that exact qualified executable. Do not ignore syntax errors or filter `cache-mode` out before invoking actionlint.

## Receipt alternative

If any repository-executing default-scope writer must remain, authenticate each pure producer payload separately. Ordinary artifact metadata binds a run, not its creator job. Standard Sigstore provenance binds workflow/run/attempt, not the current job.

GitHub now provides the server-signed OIDC `check_run_id` claim. A receipt can bind its canonical payload hash through a domain-specific audience, verify RS256 against fixed GitHub issuer JWKS, and join signed run/attempt/check-run identity to the attempt-specific jobs API. Require the exact successful admitted producer, immutable workflow/source identities, repository identity, and payload digest. Never accept receipt-supplied public keys. A missing rotated issuer key must cause cold fallback. Historical issuance validation and bounded parsing require their own implementation and adversarial tests; this note does not claim a shipped verifier. Current OIDC claims do not prove historical branch protection, so source-policy and protected-producer qualification remain independent requirements.

- [OIDC current-job claim](https://docs.github.com/en/actions/reference/security/oidc#custom-claims-provided-by-github).
- [Issuer discovery](https://token.actions.githubusercontent.com/.well-known/openid-configuration).
- [Attempt-specific jobs API](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run-attempt).
- [Artifact metadata](https://docs.github.com/en/rest/actions/artifacts).
