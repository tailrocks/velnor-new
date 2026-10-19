//! Trust gate for tool-cache writes in generated workflows.
//!
//! Tools keep one protected-push writer: only a protected push to the
//! default branch saves, and only after the restore missed the exact key,
//! so a byte-identical entry is never written again. Pull requests never
//! write tools entries.

/// Exact-key miss of the job's V2 tools restore.
pub(crate) const TOOLS_RESTORE_MISS: &str = "steps.tools-restore.outputs.cache-hit != 'true'";

const SAVE_GATES: [&str; 6] = [
    "success()",
    "github.event_name == 'push'",
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)",
    "github.ref_protected == true",
    "steps.v2.outputs.enabled == 'true'",
    TOOLS_RESTORE_MISS,
];

/// Build the GitHub expression for a V2 tools cache save.
pub(super) fn condition() -> String {
    SAVE_GATES.join(" && ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_tools_save_gate_is_protected_push_only_and_requires_an_exact_restore_miss() {
        let condition = super::condition();
        assert!(condition.ends_with(super::TOOLS_RESTORE_MISS));
        assert!(condition.contains("github.ref_protected == true"));
        assert!(!condition.contains("pull_request"));
    }
}
