# tofu-override fixture
Intent: override-file loading, lexicographic order, `.tofu`-wins.
`a_override.tf` sets level="a", `b_override.tf` level="b":
doc-sourced expectation is level="b" (later in lexicographic
order wins; value itself needs plan to observe — adapter tests
assert application order = sorted filenames). `c_override.tf` is
deliberate garbage shadowed by valid `c_override.tofu`;
`override.tf` is deliberate garbage shadowed by valid
`override.tofu` — both prove shadowed files are not parsed AND
that literal `override.*` names load with the same precedence.
`load-proof/` (unknown block in `z_override.tf`) proves override
files participate in the load set at all.
Expected discovery outcome: load set = {main.tf, a_override.tf,
b_override.tf, c_override.tofu, override.tofu}; `c_override.tf`
and `override.tf` excluded. fmt still visits excluded files
(fmt inclusion is by extension, not precedence).

Probes (OpenTofu v1.12.5 on darwin_arm64; no init/plan/apply):
- `fmt -check -recursive` root: exit 2, errors on c_override.tf
  AND override.tf (both visited despite shadowing)
- `validate` root: exit 0 `Success!` — garbage files ignored
- `validate` load-proof/: exit 1 `Unsupported block type`
  (on z_override.tf — override files load)
