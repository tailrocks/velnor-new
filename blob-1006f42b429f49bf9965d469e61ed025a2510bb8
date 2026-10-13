# tofu-multiroot fixture
Intent: multi-root repo (alpha, beta with mixed `.tf`+`.tofu`),
a space-named root (`with space/`), and a dot-dir root
(`.hiddenroot/` with deliberately unformatted main.tf).
Expected discovery outcome: alpha + beta are independent roots
(beta load set = {main.tf, extra.tofu}); tofu itself ACCEPTS the
space-named root (validate 0) — any rejection of spaces is
adapter policy, not tofu behavior, so adapter tests must pin
their own rule against this fixture. `.hiddenroot` is excluded
from the parent load set (nested dirs are separate modules) AND
recursive fmt skips dot-dirs — but explicit-path fmt still flags
it, so skip-logic lives in the walker, not the formatter.
Control-char names (`\n`, `\x7f`, trailing-dot) are NOT committed
(per repo "no committed hazards" rule, cf. fixtures/README.md):
adapter tests must build them dynamically in TempDirs.

Probes (OpenTofu v1.12.5 on darwin_arm64; no init/plan/apply):
- `fmt -check -recursive` repo root: exit 0 (dot-dir skipped
  despite unformatted file inside)
- `fmt -check` explicit `.hiddenroot/main.tf`: exit 3, listed
- `validate` alpha, beta, `with space`, `.hiddenroot` (each as
  its own cwd): exit 0 `Success!` x4
