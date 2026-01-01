# tofu-consumer-mirror fixture
Intent: sanitized shape-mirror of the consumer root (w3a §4):
13 root `.tf` (backend, checks x5 blocks, comment-only
data-repo-mapping, data-sources, imports, locals, main,
organization, outputs incl module + var reads, providers with
`>= 1.7.0` + fictional registry, repositories with the single
`./modules/repository-policy` edge + 3 `moved` blocks,
variables) + 1 local module (main/variables/outputs,
`required_version >= 1.5`, no provider config → inherits
caller). All names/addresses fictional (example.com,
example-org); NO secrets. No `.tofu`/`.tf.json`/`.tfvars`/
override files — mirrors the consumer gap those paths need
the sibling fixtures for.
Expected discovery outcome: root load set = 13 `.tf`; closure
adds 3 module files (16 fmt files, as consumer); 1 local edge
`./modules/repository-policy`; module MUST NOT validate
standalone (inherits caller providers — same as consumer).
Deviations from consumer: 6 import blocks stand in for 99
(count scaled, shape kept); fictional `example.com/acme/widgets`
provider stands in for github/onepassword; backend is `local`.

Probes (OpenTofu v1.12.5 on darwin_arm64; no init/plan/apply):
- `fmt -check -recursive`: exit 0, silent (16 files clean —
  every file parses with real tofu)
- `validate` root uninitialized: exit 1 `Module not installed`
  (needs init; init NOT run — offline)
- `validate` modules/repository-policy standalone: exit 1
  `Missing required provider` (inherits caller — must NOT
  validate standalone, as consumer)
