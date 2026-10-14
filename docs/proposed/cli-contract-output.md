## 7. Output and exit status

`plan` writes its report to stdout. `generate` writes human-readable
recommendations about optional tooling files and detection findings to stderr.
When `generate` replaces the `.github` tree, it MUST also list every
removed path that was not Velnor-generated output on stderr (see
[generated-file §3](generated-file-contract.md)); previews report the
same list without modifying the repository.
Neither `plan` nor `generate` has JSON or format-selection options. The
generated workflow is the machine-readable product; detailed plans and reports
remain internal and are validated by the orchestrator and workflow's final
required gate.

Exit statuses are intentionally small:

| Code | Meaning |
|---:|---|
| 0 | Command completed successfully. A valid no-work plan or generation is success. |
| 1 | Repository discovery, configuration, detection, planning, rendering, validation, or output replacement failed. |
| 2 | Clap usage error or unsupported command/option. |

No command may report success when a required detector, renderer, workflow
validation, or output replacement failed.
