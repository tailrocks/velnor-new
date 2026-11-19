// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const READ_WRITE_NONE: &[&str] = &["read", "write", "none"];
const READ_NONE: &[&str] = &["read", "none"];
const WRITE_NONE: &[&str] = &["write", "none"];

/// The `GITHUB_TOKEN` permission scopes a workflow can declare, with the access
/// levels GitHub documents for each one. Keep it in step with
/// <https://docs.github.com/actions/reference/workflows-and-actions/workflow-syntax#permissions>.
const TOKEN_PERMISSIONS: [(&str, &[&str]); 18] = [
    ("actions", READ_WRITE_NONE),
    ("artifact-metadata", READ_WRITE_NONE),
    ("attestations", READ_WRITE_NONE),
    ("checks", READ_WRITE_NONE),
    ("code-quality", READ_WRITE_NONE),
    ("contents", READ_WRITE_NONE),
    ("copilot-requests", WRITE_NONE),
    ("deployments", READ_WRITE_NONE),
    ("discussions", READ_WRITE_NONE),
    ("id-token", WRITE_NONE),
    ("issues", READ_WRITE_NONE),
    ("models", READ_NONE),
    ("packages", READ_WRITE_NONE),
    ("pages", READ_WRITE_NONE),
    ("pull-requests", READ_WRITE_NONE),
    ("security-events", READ_WRITE_NONE),
    ("statuses", READ_WRITE_NONE),
    ("vulnerability-alerts", READ_NONE),
];

/// The access levels a `GITHUB_TOKEN` scope takes, or `None` for a name that is
/// not a scope.
#[must_use]
pub fn token_permission_accesses(scope: &str) -> Option<&'static [&'static str]> {
    TOKEN_PERMISSIONS
        .iter()
        .find(|(name, _)| *name == scope)
        .map(|(_, accesses)| *accesses)
}

/// The access levels of a scope as prose, such as `read or none`.
#[must_use]
pub fn token_permission_accesses_prose(scope: &str) -> String {
    match token_permission_accesses(scope) {
        Some([first, middle @ .., last]) => {
            let head = std::iter::once(first).chain(middle).copied();
            format!("{} or {last}", head.collect::<Vec<_>>().join(", "))
        }
        Some([only]) => (*only).to_owned(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{TOKEN_PERMISSIONS, token_permission_accesses, token_permission_accesses_prose};

    #[test]
    fn every_scope_takes_none_and_the_list_is_sorted_and_unique() {
        for pair in TOKEN_PERMISSIONS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for (scope, accesses) in TOKEN_PERMISSIONS {
            assert!(accesses.contains(&"none"), "{scope}");
        }
    }

    #[test]
    fn the_new_scopes_take_only_the_access_github_documents() {
        let take = |scope| token_permission_accesses(scope).map(<[&str]>::to_vec);
        assert_eq!(take("models"), Some(vec!["read", "none"]));
        assert_eq!(take("vulnerability-alerts"), Some(vec!["read", "none"]));
        assert_eq!(take("copilot-requests"), Some(vec!["write", "none"]));
        assert_eq!(take("id-token"), Some(vec!["write", "none"]));
        assert_eq!(take("discussions"), Some(vec!["read", "write", "none"]));
        assert_eq!(take("made-up"), None);
        assert_eq!(token_permission_accesses_prose("models"), "read or none");
        assert_eq!(
            token_permission_accesses_prose("contents"),
            "read, write or none"
        );
    }
}
