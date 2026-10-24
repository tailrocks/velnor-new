use super::ShellDialect;

pub(super) fn hosted_runner_default(runs_on: Option<&str>) -> Option<ShellDialect> {
    let label = runs_on?;
    let is_known_linux = velnor_actions_contract::config::is_hosted_catalog(label);
    let is_known_macos = matches!(
        velnor_actions_contract::ReleaseTarget::for_runner_label(label),
        Some(
            velnor_actions_contract::ReleaseTarget::MacosArm64
                | velnor_actions_contract::ReleaseTarget::MacosX86_64
        )
    );
    if (is_known_linux && label.starts_with("ubuntu-")) || is_known_macos {
        // GitHub-hosted Ubuntu and macOS runners default `run` steps to Bash.
        Some(ShellDialect::Bash)
    } else {
        None
    }
}

pub(super) fn mapping_entry(content: &str) -> Option<(&str, &str)> {
    let content = content.strip_prefix("- ").unwrap_or(content);
    let (key, value) = content.split_once(':')?;
    if key.is_empty() || key.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return None;
    }
    Some((key, value.trim()))
}

pub(super) fn content_is_step_item(key: &str, value: &str) -> bool {
    key == "name" && !value.is_empty()
}
