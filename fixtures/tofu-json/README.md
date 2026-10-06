# tofu-json fixture
Intent: JSON config variants (`.tf.json`/`.tofu.json`) load into
the same module as HCL; same-basename `.tofu.json`-wins.
`valid/` holds minified-on-purpose JSON (proves fmt skips JSON
even when non-canonical) plus a `jpair.tf.json`/`jpair.tofu.json`
same-basename pair declaring the same variable (must NOT dup).
`clash/` declares one variable in HCL and the same in
`clash.tf.json`: MUST error, proving JSON participates in the
load set (a silent pass would mean JSON is ignored).
Expected discovery outcome: load set includes `*.tf.json` and
`*.tofu.json`; `jpair.tf.json` excluded by precedence; fmt set
excludes all JSON.

Probes (OpenTofu v1.12.5 on darwin_arm64; no init/plan/apply):
- `fmt -check -recursive` valid/: exit 0 (minified JSON untouched)
- `validate` valid/: exit 0 `Success!`
- `validate` clash/: exit 1 `Duplicate variable declaration`
  (citing clash.tf.json:1 — JSON loads)
