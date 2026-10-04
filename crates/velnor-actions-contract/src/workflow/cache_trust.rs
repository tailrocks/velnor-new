//! Fixed GitHub expressions for the trusted default-branch cache writer.

// One predicate owns explicit-save, action-input, and mode spellings.
macro_rules! trusted_push {
    () => {
        "github.event_name == 'push' && github.ref_protected == true && github.ref == format('refs/heads/{0}', github.event.repository.default_branch)"
    };
}

/// Trusted writer predicate; protection is verified by runner context.
pub const CACHE_TRUSTED_PUSH_EXPR: &str = trusted_push!();
/// Action-input spelling of the trusted writer predicate.
pub const CACHE_DEFAULT_BRANCH_WRITE_EXPR: &str = concat!("${{ ", trusted_push!(), " }}");
/// Explicit saves require successful producers and a protected default-branch push.
///
/// PRs, fork PRs, merge groups, and other branch pushes remain read-only.
/// Missing or false protection evidence keeps writes disabled.
pub const CACHE_SAVE_CONDITION: &str = concat!("success() && ", trusted_push!());
/// Cache-mode selector inner expression, shared with the renderer allowlist.
pub const CACHE_MODE_PUSH_WRITE_INNER: &str = concat!(trusted_push!(), " && 'write' || 'read'");
/// Cache-mode value requests writes only on protected default-branch pushes.
///
/// This environment value conveys intent; it does not enforce the pinned
/// action's transport or the GitHub cache service's authorization policy.
pub const CACHE_MODE_PUSH_WRITE_EXPR: &str =
    concat!("${{ ", trusted_push!(), " && 'write' || 'read' }}");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_write_spelling_inherits_protected_default_push() {
        let predicate = "github.event_name == 'push' && github.ref_protected == true && github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";
        assert_eq!(CACHE_TRUSTED_PUSH_EXPR, predicate);
        assert_eq!(CACHE_SAVE_CONDITION, format!("success() && {predicate}"));
        assert_eq!(
            CACHE_DEFAULT_BRANCH_WRITE_EXPR,
            format!("${{{{ {predicate} }}}}")
        );
        assert_eq!(
            CACHE_MODE_PUSH_WRITE_EXPR,
            format!("${{{{ {predicate} && 'write' || 'read' }}}}")
        );
    }
}
