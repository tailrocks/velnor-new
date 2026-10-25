//! Parse the Rust components step that populates the tools cache.

use velnor_actions_contract::{Step, StepKind};

use crate::RenderError;

/// Parse the exact pinned components step that populated this tool payload.
/// # Errors
pub(crate) fn rust_components(
    step: &Step,
    version: &str,
    target: &str,
) -> Result<Vec<String>, RenderError> {
    let StepKind::Shell { run, env } = &step.kind else {
        return Err(RenderError::BadCommand(
            "rust_components_not_shell".to_owned(),
        ));
    };
    if env
        .get("RUSTUP_TOOLCHAIN")
        .is_some_and(|value| value != version)
    {
        return Err(RenderError::BadCommand(
            "rust_components_toolchain_env_mismatch".to_owned(),
        ));
    }
    let words = crate::cache_p08_detect::detector_words(run);
    let Some(start) = words
        .windows(3)
        .position(|triple| triple == ["rustup", "component", "add"])
    else {
        return Err(RenderError::BadCommand(
            "rust_components_command_missing".to_owned(),
        ));
    };
    let payload = &words[start + 3..];
    if payload.len() < 3 || payload[0] != "--toolchain" {
        return Err(RenderError::BadCommand(
            "rust_components_toolchain_missing".to_owned(),
        ));
    }
    let expected = format!("{version}-{target}");
    if payload[1] != expected {
        return Err(RenderError::BadCommand(
            "rust_components_toolchain_mismatch".to_owned(),
        ));
    }
    let mut components = payload[2..].to_vec();
    components.sort();
    components.dedup();
    if components != ["clippy", "rustfmt"] {
        return Err(RenderError::BadCommand(
            "rust_components_set_mismatch".to_owned(),
        ));
    }
    Ok(components)
}

#[cfg(test)]
mod tests {
    use super::rust_components;
    use crate::steps::ambient_shell_step;
    use std::collections::BTreeMap;

    #[test]
    fn parses_the_fixed_rust_components_payload() {
        let step = ambient_shell_step(
            "Prepare Rust components",
            vec![
                "mise".to_owned(),
                "exec".to_owned(),
                "rust@1.98.1".to_owned(),
                "--".to_owned(),
                "rustup".to_owned(),
                "component".to_owned(),
                "add".to_owned(),
                "--toolchain".to_owned(),
                "1.98.1-x86_64-unknown-linux-gnu".to_owned(),
                "clippy".to_owned(),
                "rustfmt".to_owned(),
            ],
            BTreeMap::from([("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned())]),
        )
        .expect("fixed step");
        assert_eq!(
            rust_components(&step, "1.98.1", "x86_64-unknown-linux-gnu").expect("components"),
            ["clippy", "rustfmt"]
        );
    }
}
