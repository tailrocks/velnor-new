//! Generated composite for the fixed V2 tools-cache restore payload.

use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{RenderError, marker, steps, tree::RenderedFile, yaml::Yaml};

/// Validate the only call shape accepted for the fixed restore composite.
/// # Errors
pub(crate) fn validate_call(step: &Step) -> Result<&str, RenderError> {
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "tools_cache_restore_shape".to_owned(),
        ));
    };
    let key = with.get("key").ok_or_else(|| {
        RenderError::InvalidWorkflow("tools_cache_restore_key_missing".to_owned())
    })?;
    let seed_admitted = with.get(super::TOOLS_SEED_ADMITTED_INPUT);
    if step.role != Some(StepRole::ToolsCacheRestore)
        || step.id.is_some()
        || uses != super::TOOLS_RESTORE_USES
        || !env.is_empty()
        || with.len() != 2
        || seed_admitted.map(String::as_str) != Some(super::TOOLS_SEED_ADMITTED_EXPRESSION)
        || !crate::cache_p08::is_v2_cache_key_expression(key)
        || step.condition.as_deref() != Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION)
    {
        return Err(RenderError::InvalidWorkflow(
            "tools_cache_restore_shape".to_owned(),
        ));
    }
    Ok(key)
}

/// Keep only an exact cache hit or a validated seed on an empty-cache miss.
/// Semicolons keep this valid after `quote_run_line_env_paths` joins words with spaces.
const TOOLS_CACHE_ADMISSION_SCRIPT: &str = r#"set -eu;
[ -n "$HOME" ] && [ -n "$RUNNER_TEMP" ] || exit 1;
case "$HOME" in /*) ;; *) exit 1 ;; esac;
case "$RUNNER_TEMP" in /*) ;; *) exit 1 ;; esac;
[ "$HOME" != / ] && [ "$RUNNER_TEMP" != / ] || exit 1;
cache_hit="${TOOLS_CACHE_HIT-}";
expected_key="${TOOLS_EXPECTED_KEY-}";
matched_key="${TOOLS_MATCHED_KEY-}";
seed_admitted="${TOOLS_SEED_ADMITTED-}";
keep=false;
if [ "$cache_hit" = true ] && [[ "$expected_key" =~ ^mise-tools-v2-[0-9a-f]{64}$ ]] && [ "$matched_key" = "$expected_key" ]; then
keep=true;
elif [ -z "$cache_hit" ] && [[ "$expected_key" =~ ^mise-tools-v2-[0-9a-f]{64}$ ]] && [ -z "$matched_key" ] && [ "$seed_admitted" = true ]; then
keep=true;
fi;
if [ "$keep" = true ]; then
[ ! -L "$HOME/.local/share/mise" ] || exit 1;
[ ! -L "$RUNNER_TEMP/velnor/rustup" ] || exit 1;
[ ! -L "$RUNNER_TEMP/velnor/cargo/bin" ] || exit 1;
[ ! -L "$RUNNER_TEMP/velnor/cargo/.crates.toml" ] || exit 1;
[ ! -L "$RUNNER_TEMP/velnor/cargo/.crates2.json" ] || exit 1;
exit 0;
fi;
for d in "$HOME/.local/share/mise" "$RUNNER_TEMP/velnor/rustup" "$RUNNER_TEMP/velnor/cargo/bin"; do
[ ! -L "$d" ] || exit 1;
rm -rf "$d";
done;
for f in "$RUNNER_TEMP/velnor/cargo/.crates.toml" "$RUNNER_TEMP/velnor/cargo/.crates2.json"; do
[ ! -L "$f" ] || exit 1;
rm -f "$f";
done"#;

fn restore_step() -> Result<Yaml, RenderError> {
    steps::validate_uses(super::TOOLS_RESTORE_ACTION_USES)?;
    let path = super::TOOLS_CACHE_PATHS.join("\n");
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(super::TOOLS_RESTORE_NAME)),
        ("id".to_owned(), Yaml::str("restore")),
        (
            "uses".to_owned(),
            Yaml::str(super::TOOLS_RESTORE_ACTION_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("key".to_owned(), Yaml::str("${{ inputs.key }}")),
                ("path".to_owned(), Yaml::str(path)),
                ("restore-keys".to_owned(), Yaml::str(String::new())),
            ]),
        ),
    ]))
}

fn admission_step() -> Yaml {
    let env = [
        ("TOOLS_CACHE_HIT", "steps.restore.outputs.cache-hit"),
        ("TOOLS_EXPECTED_KEY", "inputs.key"),
        (
            "TOOLS_MATCHED_KEY",
            "steps.restore.outputs.cache-matched-key",
        ),
        ("TOOLS_SEED_ADMITTED", "inputs.seed-admitted"),
    ]
    .into_iter()
    .map(|(key, value)| {
        let mut expression = String::from("${{ ");
        expression.push_str(value);
        expression.push_str(" }}");
        (key.to_owned(), Yaml::str(expression))
    })
    .collect();
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Keep only an exact cache hit or validated seed on a miss"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("env".to_owned(), Yaml::Map(env)),
        ("run".to_owned(), Yaml::str(TOOLS_CACHE_ADMISSION_SCRIPT)),
    ])
}

fn action_body() -> Result<Yaml, RenderError> {
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Velnor Mise tools cache restore"),
        ),
        (
            "description".to_owned(),
            Yaml::str("Restore the exact renderer-owned V2 Mise tools paths."),
        ),
        (
            "inputs".to_owned(),
            Yaml::Map(vec![
                (
                    "key".to_owned(),
                    Yaml::Map(vec![
                        (
                            "description".to_owned(),
                            Yaml::str("Exact runtime-qualified V2 tools key."),
                        ),
                        ("required".to_owned(), Yaml::Bool(true)),
                    ]),
                ),
                (
                    super::TOOLS_SEED_ADMITTED_INPUT.to_owned(),
                    Yaml::Map(vec![
                        (
                            "description".to_owned(),
                            Yaml::str("Whether the trusted V2 prelude copied the exact seed."),
                        ),
                        ("required".to_owned(), Yaml::Bool(false)),
                        ("default".to_owned(), Yaml::quoted("false")),
                    ]),
                ),
            ]),
        ),
        (
            "runs".to_owned(),
            Yaml::Map(vec![
                ("using".to_owned(), Yaml::str("composite")),
                (
                    "steps".to_owned(),
                    Yaml::Seq(vec![restore_step()?, admission_step()]),
                ),
            ]),
        ),
    ]))
}

/// Build the pinned restore action with its renderer-owned archive paths.
/// # Errors
pub(crate) fn action_file(version: &str) -> Result<RenderedFile, RenderError> {
    let body = action_body()?;
    let bytes = marker::with_marker(version, &crate::yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    let action_directory = super::TOOLS_RESTORE_USES
        .strip_prefix("./")
        .ok_or_else(|| RenderError::InvalidWorkflow("tools_restore_action_path".to_owned()))?;
    Ok(RenderedFile {
        path: format!("{action_directory}/action.yml"),
        bytes,
    })
}

/// Decode the escapes `quote_double` emits, single-pass like YAML.
#[cfg(test)]
fn unescape_double_quoted_scalar(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// Parse the quoted `run:` scalar the way Actions does, then `bash -n` it.
#[cfg(test)]
pub(crate) fn assert_rendered_admission_parses(bytes: &str) {
    let raw = bytes
        .split("run: \"")
        .nth(1)
        .and_then(|rest| rest.split("\"\n").next())
        .expect("admission run scalar");
    let script = unescape_double_quoted_scalar(raw);
    let file = std::env::temp_dir().join("velnor-tools-admission-bash-n.sh");
    std::fs::write(&file, &script).expect("admission script");
    let status = std::process::Command::new("bash")
        .arg("-n")
        .arg(&file)
        .status()
        .expect("bash -n");
    assert!(status.success(), "{script}");
}

#[cfg(test)]
#[path = "cache_steps_tools_restore_tests.rs"]
mod tests;
