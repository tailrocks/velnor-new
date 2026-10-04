# Owned Actionlint cache-mode qualification

Run on each actual native host: Linux AMD64, Mac ARM64, and Intel Mac AMD64.
Rosetta execution fails admission. The source build receipt must bind the exact
recipe and approved Go compiler asset before any behavioral cases run.

```sh
python3 scripts/qualification/actionlint-cache-mode/run.py \
  --actionlint /absolute/path/to/actionlint \
  --expected-version 1.7.12-velnor-cache-mode.1+patch.7c81196d7996 \
  --build-receipt /absolute/path/to/build-receipt.json \
  --output /absolute/path/to/new-native-report.json
```

The executable path must be an absolute regular executable. The report path must
be absent. Exit 0 means all 47 actual CLI cases passed and binary bytes stayed
unchanged; exit 1 preserves a failure report. Reports retain commands, durations,
exit codes, raw diagnostics, version output, host hardware evidence, build receipt
SHA-256, executable SHA-256, runner SHA-256, manifest SHA-256, and fixture SHA-256.

Cases cover exact `read`, `write`, `write-only`, and `none` values at workflow and
job scope; literal aliases; reusable calls; invalid values and expressions;
wrong YAML types and explicit tags; duplicate keys; and ordinary needs/action
errors. Negative cases require Actionlint's diagnostic exit 1 and named native
rules. No fields are stripped and no diagnostics are waived. Standard
`-shellcheck=` and `-pyflakes=` flags disable optional external language analyzers
in isolated temporary fixture directories.

The manifest and exact fixture corpus are pinned inside the runner. Any approved
fixture change must update both pins, receive review, and run on all native hosts.
This directory contains qualification inputs; it makes no claim that any host
execution or artifact publication has occurred.
