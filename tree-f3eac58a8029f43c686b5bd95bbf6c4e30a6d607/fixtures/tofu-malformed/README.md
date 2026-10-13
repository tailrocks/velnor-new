# tofu-malformed fixture
Intent: must-error cases, one per subdir (each subdir is its own
root so failures stay isolated): `hcl-syntax/` (unclosed brace),
`json-syntax/` (invalid JSON in `.tf.json`), `hcl-semantic/`
(valid HCL, unknown block type), `dup-var/` (duplicate variable
across two files). `control/` (empty .tf + valid .tf) proves
empty files are legal — malformed verdicts are per-case, and an
empty tree is valid.
Expected discovery outcome: adapter MUST surface all four as
errors (never silent-skip); error KIND differs by layer —
`hcl-syntax` fails at parse (fmt exit 2), `json-syntax` PASSES
fmt (JSON skipped) but fails validate, `hcl-semantic` and
`dup-var` pass fmt but fail validate. Any detector that relies
on fmt-cleanliness alone to declare "valid" is wrong on 3 of 4.

Probes (OpenTofu v1.12.5 on darwin_arm64; no init/plan/apply):
- hcl-syntax: fmt exit 2 `Unclosed configuration block`;
  validate exit 1 same
- json-syntax: fmt exit 0 (JSON skipped!); validate exit 1
  `Root value must be object` + `Invalid JSON keyword`
- hcl-semantic: fmt exit 0 (valid HCL); validate exit 1
  `Unsupported block type`
- dup-var: fmt exit 0; validate exit 1
  `Duplicate variable declaration`
- control: fmt exit 0; validate exit 0 `Success!`
