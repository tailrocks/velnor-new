# tofu-tfvars fixture
Intent: `.tfvars` files never join the config load set, but DO
join the fmt set. `terraform.tfvars` and `extra.auto.tfvars` are
deliberately mis-spaced (fmt-dirty by design); `custom.tfvars`
is canonical; `terraform.tfvars.example` is mis-spaced but must
NOT be flagged (`.example` suffix excluded, as in the consumer).
Expected discovery outcome: validate/init selection = {main.tf}
exactly — adding/removing/renaming tfvars never changes it, and
var VALUES never change file selection. fmt selection = {main.tf,
terraform.tfvars, extra.auto.tfvars, custom.tfvars} (all `*.tfvars`
by extension); `terraform.tfvars.example` excluded.

Probes (OpenTofu v1.12.5 on darwin_arm64; no init/plan/apply):
- `fmt -check -recursive`: exit 3, lists exactly
  extra.auto.tfvars + terraform.tfvars (NOT the .example file)
- `validate`: exit 0 `Success!` (tfvars present, values assigned)
