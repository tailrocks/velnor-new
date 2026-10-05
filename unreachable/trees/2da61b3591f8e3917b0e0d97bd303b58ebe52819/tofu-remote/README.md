# tofu-remote fixture
Intent: remote module sources — `git::https://…` and a registry
address (`example-ns/widgets/fictional`, all hosts fictional).
OFFLINE FIXTURE: never `init` in tests (requires network); never
`plan`/`apply`.
Expected discovery outcome: root discovered with 2 remote edges
recorded as {kind: git, kind: registry}; adapter must mark both
offline-unresolvable / expected-fail-without-network and must NOT
treat "module not installed" as a config error distinct from the
local-modules case. Uninitialized `validate` fails exactly like
local modules (proves validate cannot distinguish remote edges
pre-init — edge KIND comes from source-string parsing only).

Probes (OpenTofu v1.12.5 on darwin_arm64; no init/plan/apply):
- `fmt -check -recursive`: exit 0, silent
- `validate` uninitialized: exit 1 `Module not installed` (x2)
- `init`: NOT RUN (needs network by construction)
