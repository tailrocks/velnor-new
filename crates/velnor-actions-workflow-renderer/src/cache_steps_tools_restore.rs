// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{RenderError, marker, steps, tree::RenderedFile, yaml::Yaml};

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
        || step.id != Some(velnor_actions_contract::StepId::ToolsRestore)
        || uses != super::TOOLS_RESTORE_USES
        || !env.is_empty()
        || with.len() != 3
        || !with
            .get(super::TOOLS_MISE_SHA256_INPUT)
            .is_some_and(|sha| is_mise_sha256(sha))
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

pub(super) fn is_mise_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

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

const INSTALLS_VERIFY_SCRIPT: &str = include_str!("cache_steps_tools_installs_verify.sh");

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
        ("id".to_owned(), Yaml::str("verify")),
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

fn action_outputs() -> Yaml {
    let mut entries = Vec::with_capacity(5);
    for (name, description, value) in [
        (
            "cache-hit",
            "Exact-key hit of the restored V2 tools entry.",
            "${{ steps.restore.outputs.cache-hit }}",
        ),
        (
            "origin-bytes",
            "Origin bytes downloaded while re-verifying restored Mise installs; empty when unmeasured.",
            "${{ steps.verify.outputs.origin-bytes }}",
        ),
        (
            "origin-fetches",
            "Origin archive downloads while re-verifying restored Mise installs.",
            "${{ steps.verify.outputs.origin-fetches }}",
        ),
        (
            "reused-archives",
            "Verified local tool archives reused without an origin download.",
            "${{ steps.verify.outputs.reused-archives }}",
        ),
        (
            "verified-files",
            "Restored install files verified against trusted archives.",
            "${{ steps.verify.outputs.verified-files }}",
        ),
    ] {
        entries.push((
            name.to_owned(),
            Yaml::Map(vec![
                ("description".to_owned(), Yaml::str(description)),
                ("value".to_owned(), Yaml::str(value)),
            ]),
        ));
    }
    Yaml::Map(entries)
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
            ]),
        ),
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

pub(crate) fn action_file(
    version: &str,
    tool_pins: &[String],
) -> Result<RenderedFile, RenderError> {
    let mut union = Vec::with_capacity(tool_pins.len());
    for pin in tool_pins {
        if !union.contains(pin) {
            union.push(pin.clone());
        }
    }
    let body = action_body(&union)?;
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
