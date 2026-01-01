//! Trust gates for tool-cache writes in generated consumer workflows.

const SAVE_GATES: [&str; 5] = [
    "success()",
    "github.event_name == 'push'",
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)",
    "github.ref_protected == true",
    "steps.v2.outputs.enabled == 'true'",
];

/// Build the GitHub expression for a successful push to its protected default branch.
pub(super) fn condition() -> String {
    SAVE_GATES.join(" && ")
}
