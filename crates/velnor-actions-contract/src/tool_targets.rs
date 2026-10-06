//! Tool distribution targets; independent of generator release artifact support.

/// Exact executable target for a supported hosted tool-cache runner label.
/// Unknown labels have no compiled distribution authority and fail closed.
#[must_use]
pub fn tool_target_for_runner_label(label: &str) -> Option<&'static str> {
    match label {
        "ubuntu-22.04" | "ubuntu-24.04" | "ubuntu-26.04" => Some("x86_64-unknown-linux-gnu"),
        "ubuntu-24.04-arm" | "ubuntu-26.04-arm" => Some("aarch64-unknown-linux-gnu"),
        "macos-15" | "macos-26" => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::tool_target_for_runner_label;

    #[test]
    fn executable_targets_do_not_extend_generator_release_support() {
        assert_eq!(
            tool_target_for_runner_label("macos-26"),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(
            tool_target_for_runner_label("ubuntu-26.04-arm"),
            Some("aarch64-unknown-linux-gnu")
        );
        assert_eq!(crate::target_for_runner_label("macos-26"), None);
        assert_eq!(crate::target_for_runner_label("ubuntu-26.04-arm"), None);
        for label in [
            "ubuntu-latest",
            "macos-latest",
            "macos-15-intel",
            "unknown",
            "${{ matrix.os }}",
        ] {
            assert_eq!(tool_target_for_runner_label(label), None);
        }
    }
}
