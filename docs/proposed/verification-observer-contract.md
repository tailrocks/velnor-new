# Verification failure observer

`[workflow.verification] alert = true` opts the main CI workflow into a fixed
nightly failure issue observer. The default is false. Alerts require a configured
schedule or manual dispatch. No satellite dispatch workflow is generated: the
configured cadence executes the same graph and exact `Required` check as CI.

The observer is a separate job depending only on `Required`. It never enters
the Required dependency inventory. Its token has only `issues: write`; the
workflow and verification jobs retain read-only or empty permissions. The job
requires the generated origin repository, protected default branch, and a
`schedule` or `workflow_dispatch` event. Pull requests, forks, pushes, and merge
groups cannot write issues. A protected `verification-alerts` environment is
required at the job boundary.

The observer executes a generator-owned Python helper with exact catalog Python
and Gh pins. It performs no checkout, artifact execution, generator acquisition,
repository task, hook, or repository Mise configuration. Installation and
execution use owned temporary homes, isolated configuration, and disabled hooks.
Rendering revalidates the fixed job role and exact step templates.

The helper opens or updates the fixed title `Nightly CI red` only when Required
fails, is cancelled, or is skipped. The body contains its closed result value,
the GitHub run URL, and the source SHA. Repository identity, source SHA, and
numeric run identity are validated before API access. Logs, arbitrary templates,
commands, credentials, and user-authored issue content are never accepted.
Open issues are searched in at most five pages of 100. Matching pull requests
are excluded; the lowest matching issue number is updated. Exhausting the bound
fails before any write.

When alert and manual dispatch are enabled, the generator adds the Boolean
`simulate_failure` input with a native false default. A fixed failing step sits
immediately before the existing Plan operation and executes only for an explicit
true manual input. Ordinary runs allocate no additional runner. A simulation
fails Plan before a valid plan artifact exists; Required's normal merger emits
the actual `planning_failed` final report. A simulation never publishes a
passing final report or bypasses substantive CI verification.

Restoration proof is local only. Tests mock the API; validating this contract
does not authorize workflow dispatches or issue writes.
