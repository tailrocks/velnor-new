//! Generated composite for the fixed V2 tools-cache restore payload.

use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

use crate::{RenderError, marker, steps, tree::RenderedFile, yaml::Yaml};

/// Validate the only call shape accepted for the fixed restore composite.
/// # Errors
pub(crate) fn validate_call(step: &Step) -> Result<&str, RenderError> {
    validate_call_in_scope(step, false)
}

/// Validate the fixed restore composite inside a source-derived child workflow.
/// # Errors
pub(crate) fn validate_call_for_relocated_child(step: &Step) -> Result<&str, RenderError> {
    validate_call_in_scope(step, true)
}

fn validate_call_in_scope(step: &Step, relocated_child: bool) -> Result<&str, RenderError> {
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
        || step.id != Some(StepId::ToolsRestore)
        || uses != super::TOOLS_RESTORE_USES
        || !env.is_empty()
        || with.len() != 3
        || !with
            .get(super::TOOLS_MISE_SHA256_INPUT)
            .is_some_and(|sha| is_mise_sha256(sha))
        || seed_admitted.map(String::as_str) != Some(super::TOOLS_SEED_ADMITTED_EXPRESSION)
        || !crate::cache_p08::is_v2_cache_key_expression(key)
        || !(matches!(
            step.condition.as_deref(),
            Some(
                crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION
                    | crate::cache_p08::TOOLS_CACHE_PLAN_RESTORE_CONDITION
            )
        ) || (relocated_child
            && step.condition.as_deref()
                == Some(crate::cache_p08::TOOLS_CACHE_RELOCATED_RESTORE_CONDITION)))
    {
        return Err(RenderError::InvalidWorkflow(
            "tools_cache_restore_shape".to_owned(),
        ));
    }
    Ok(key)
}

/// Exact lower-case SHA-256 of a pinned Mise binary.
pub(super) fn is_mise_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Keep only an exact cache hit or a validated seed on an empty-cache miss.
///
/// Setup Mise runs before this restore (the resolver step needs its binary
/// before the identity exists), and the archive owns its binary directory
/// `~/.local/share/mise/bin`. A miss therefore sweeps every other entry of
/// the Mise root, and both paths then re-verify that binary against the
/// job's pinned digest, so no restored or seeded Mise binary runs unchecked.
///
/// Setup Mise has already put the restored `bin/` and `shims/` on `PATH`
/// ahead of the system directories, so the script first resets `PATH` to
/// [`crate::VERIFY_PATH`] (and the step's shell is [`crate::VERIFY_SHELL`]),
/// and `bin/` may hold nothing but what Setup Mise writes there: the binary
/// and its plain `mise-version` record.
/// Semicolons keep this valid after `quote_run_line_env_paths` joins words with spaces.
const TOOLS_CACHE_ADMISSION_SCRIPT: &str = r#"set -eu;
PATH=/usr/bin:/bin:/usr/sbin:/sbin;
export PATH;
[ -n "$HOME" ] && [ -n "$RUNNER_TEMP" ] || exit 1;
case "$HOME" in /*) ;; *) exit 1 ;; esac;
case "$RUNNER_TEMP" in /*) ;; *) exit 1 ;; esac;
[ "$HOME" != / ] && [ "$RUNNER_TEMP" != / ] || exit 1;
cache_hit="${TOOLS_CACHE_HIT-}";
expected_key="${TOOLS_EXPECTED_KEY-}";
matched_key="${TOOLS_MATCHED_KEY-}";
seed_admitted="${TOOLS_SEED_ADMITTED-}";
mise_sha256="${TOOLS_MISE_SHA256-}";
mise_root="$HOME/.local/share/mise";
[[ "$mise_sha256" =~ ^[0-9a-f]{64}$ ]] || exit 1;
verify_mise() {
[ ! -L "$mise_root" ] && [ ! -L "$mise_root/bin" ] && [ -d "$mise_root/bin" ] && [ ! -L "$mise_root/bin/mise" ] && [ -f "$mise_root/bin/mise" ] || { echo "Setup Mise binary is missing" >&2; exit 1; };
while IFS= read -r -d '' entry; do
name=${entry##*/};
case "$name" in
mise) ;;
mise-version) [ -f "$entry" ] && [ ! -L "$entry" ] && [ ! -x "$entry" ] || { echo "mise-version is not a plain Setup Mise record" >&2; exit 1; } ;;
*) printf '%s is not part of the Setup Mise install\n' "bin/$name" >&2; exit 1 ;;
esac;
done < <(find "$mise_root/bin" -mindepth 1 -maxdepth 1 -print0);
if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum "$mise_root/bin/mise"); else actual=$(shasum -a 256 "$mise_root/bin/mise"); fi;
[ "${actual%% *}" = "$mise_sha256" ] || { echo "Mise binary does not match its pinned SHA-256" >&2; exit 1; };
};
keep=false;
if [ "$cache_hit" = true ] && [[ "$expected_key" =~ ^mise-tools-v2-payload-v3-[0-9a-f]{64}$ ]] && [ "$matched_key" = "$expected_key" ]; then
keep=true;
elif [ -z "$cache_hit" ] && [[ "$expected_key" =~ ^mise-tools-v2-payload-v3-[0-9a-f]{64}$ ]] && [ -z "$matched_key" ] && [ "$seed_admitted" = true ]; then
keep=true;
fi;
if [ "$keep" = true ]; then
[ ! -L "$HOME/.local/share/mise" ] || exit 1;
[ ! -L "$RUNNER_TEMP/velnor/rustup" ] || exit 1;
[ ! -L "$RUNNER_TEMP/velnor/cargo/bin" ] || exit 1;
[ ! -L "$RUNNER_TEMP/velnor/cargo/.crates.toml" ] || exit 1;
[ ! -L "$RUNNER_TEMP/velnor/cargo/.crates2.json" ] || exit 1;
verify_mise;
exit 0;
fi;
[ ! -L "$mise_root" ] || exit 1;
if [ -d "$mise_root" ]; then
find "$mise_root" -mindepth 1 -maxdepth 1 ! -name bin -exec rm -rf {} +;
fi;
for d in "$RUNNER_TEMP/velnor/rustup" "$RUNNER_TEMP/velnor/cargo/bin"; do
[ ! -L "$d" ] || exit 1;
rm -rf "$d";
done;
for f in "$RUNNER_TEMP/velnor/cargo/.crates.toml" "$RUNNER_TEMP/velnor/cargo/.crates2.json"; do
[ ! -L "$f" ] || exit 1;
rm -f "$f";
done;
verify_mise"#;

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
        ("TOOLS_MISE_SHA256", "inputs.mise-sha256"),
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
        ("shell".to_owned(), Yaml::str(crate::VERIFY_SHELL)),
        ("env".to_owned(), Yaml::Map(env)),
        ("run".to_owned(), Yaml::str(TOOLS_CACHE_ADMISSION_SCRIPT)),
    ])
}

fn action_inputs() -> Yaml {
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
            super::TOOLS_MISE_SHA256_INPUT.to_owned(),
            Yaml::Map(vec![
                (
                    "description".to_owned(),
                    Yaml::str("Pinned SHA-256 of the Setup Mise binary."),
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
    ])
}

fn action_outputs() -> Yaml {
    Yaml::Map(vec![
        (
            "cache-hit".to_owned(),
            Yaml::Map(vec![
                (
                    "description".to_owned(),
                    Yaml::str("Shows whether the restore action found the tools cache key."),
                ),
                (
                    "value".to_owned(),
                    Yaml::str("${{ steps.restore.outputs.cache-hit }}"),
                ),
            ]),
        ),
        (
            "cache-matched-key".to_owned(),
            Yaml::Map(vec![
                (
                    "description".to_owned(),
                    Yaml::str("The tools cache key that the restore action found."),
                ),
                (
                    "value".to_owned(),
                    Yaml::str("${{ steps.restore.outputs.cache-matched-key }}"),
                ),
            ]),
        ),
    ])
}

/// Re-verify every restored Mise install against its pinned archive; a
/// mismatch is discarded so `Prepare pinned tools` reinstalls it.
const INSTALLS_VERIFY_SCRIPT: &str = include_str!("cache_steps_tools_installs_verify.sh");

/// Display name of the composite's install re-verification step.
pub(crate) const INSTALLS_VERIFY_NAME: &str = "Re-verify restored Mise installs";

fn installs_verify_step(tool_pins: &[String]) -> Result<Yaml, RenderError> {
    for pin in tool_pins {
        let fields: Vec<&str> = pin.split(' ').collect();
        let ok = fields.len() == 5
            && fields.iter().all(|field| {
                !field.is_empty()
                    && field
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-:/@+".contains(&b))
            })
            && fields[3].starts_with("https://")
            && fields[4].len() == 64;
        if !ok {
            return Err(RenderError::InvalidWorkflow(format!("bad_tool_pin:{pin}")));
        }
    }
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(INSTALLS_VERIFY_NAME)),
        ("shell".to_owned(), Yaml::str(crate::VERIFY_SHELL)),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "VELNOR_TOOL_PINS".to_owned(),
                Yaml::str(tool_pins.join("\n")),
            )]),
        ),
        ("run".to_owned(), Yaml::str(INSTALLS_VERIFY_SCRIPT)),
    ]))
}

fn action_body(tool_pins: &[String]) -> Result<Yaml, RenderError> {
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Velnor Mise tools cache restore"),
        ),
        (
            "description".to_owned(),
            Yaml::str("Restore the exact renderer-owned V2 Mise tools paths."),
        ),
        ("inputs".to_owned(), action_inputs()),
        ("outputs".to_owned(), action_outputs()),
        (
            "runs".to_owned(),
            Yaml::Map(vec![
                ("using".to_owned(), Yaml::str("composite")),
                (
                    "steps".to_owned(),
                    Yaml::Seq(vec![
                        restore_step()?,
                        admission_step(),
                        installs_verify_step(tool_pins)?,
                    ]),
                ),
            ]),
        ),
    ]))
}

/// Build the pinned restore action with its renderer-owned archive paths.
/// # Errors
pub(crate) fn action_file(
    version: &str,
    tool_pins: &[String],
) -> Result<RenderedFile, RenderError> {
    let body = action_body(tool_pins)?;
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

#[cfg(test)]
#[path = "cache_steps_tools_installs_verify_tests.rs"]
mod installs_verify_tests;

#[cfg(test)]
#[path = "cache_steps_tools_restore_path_tests.rs"]
mod path_tests;
