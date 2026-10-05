# tofu-modules fixture
Intent: local module edges — `./` child, `./nested` grandchild,
`../shared` parent-relative, and a diamond (a→shared, b→shared).
All modules provider-free so the tree is fully offline-parseable.
Expected discovery outcome: root load set = {main.tf}; module
closure = {modules/a, modules/a/nested, modules/shared (x2 refs,
one dir), modules/b}; diamond resolves to ONE shared dir, not
two copies. fmt-recursive covers all 5 files. Uninitialized
`validate` MUST fail (child modules need `tofu init` even when
local) — adapter tests assert this exact failure, not success.

Probes (OpenTofu v1.12.5 on darwin_arm64; no init/plan/apply):
- `fmt -check -recursive`: exit 0, silent (5 files clean)
- `validate` uninitialized: exit 1 `Module not installed` (x2,
  for module.a and module.b)
