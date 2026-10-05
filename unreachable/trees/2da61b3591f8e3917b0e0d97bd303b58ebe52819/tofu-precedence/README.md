# tofu-precedence fixture
Intent: same-basename `.tf` vs `.tofu` extension precedence.
`main.tf` declares `which`="tf" plus a `ghost` variable; `main.tofu`
declares `which`="tofu". Both loaded would be a duplicate error.
Expected discovery outcome: load set = {main.tofu, extra.tf};
`main.tf` excluded (not even parsed — its `ghost` var is undeclared).
`dup-control/` is the control: the same duplicate across two normal
files MUST error, proving the root's silence is precedence, not
tolerance. fmt visits all three root files (precedence is load-time
only, not a fmt exclusion).

Probes (OpenTofu v1.12.5 on darwin_arm64; no init/plan/apply):
- `fmt -check -recursive` root: exit 0, silent
- `validate` root: exit 0 `Success! The configuration is valid.`
- `validate` dup-control/: exit 1 `Duplicate variable declaration`
