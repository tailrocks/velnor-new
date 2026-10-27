// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    COMPLETE_STATE_TRANSPORT_EXPR, CacheFamily, RUNNER_TEMP_EXPR, RUNNER_TEMP_SHELL,
};
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_KEY_EXPR, MBX_BUNDLE_RESTORE_KEYS_EXPR,
};
use velnor_actions_contract::{Step, StepId, StepRole};

use crate::RenderError;
use crate::steps::shell_step;

#[path = "cache_steps_mbx_bundle_publish.rs"]
mod publish;
#[path = "cache_steps_mbx_bundle_recover.rs"]
mod recover;

pub(crate) use publish::append_bundle_postludes;
pub use recover::mbx_producer_mark_step;

pub const MBX_BUNDLE_KEY_NAME: &str = "Compute state snapshot key";
pub const MBX_BUNDLE_RESTORE_NAME: &str = "Restore state snapshot";
pub const MBX_BUNDLE_IMPORT_NAME: &str = "Recover state snapshot";
pub const MBX_PRODUCER_MARK_NAME: &str = "Record state producer success";
pub const MBX_BUNDLE_EXPORT_NAME: &str = "Publish state snapshot";
pub const MBX_BUNDLE_SAVE_NAME: &str = "Save state snapshot";
pub const MBX_EXPORT_GROUP_ENV: &str = "MBX_CACHE_EXPORT_GROUP";
pub const MBX_EXPORT_GROUP_VALUE: &str = "velnor";

const DEFAULT_REF_ENV: &str = "VELNOR_DEFAULT_REF";
const HEAD_FORK_ENV: &str = "VELNOR_PR_HEAD_FORK";
const DEFAULT_REF_EXPR: &str =
    "${{ format('refs/heads/{0}', github.event.repository.default_branch) }}";
const HEAD_FORK_EXPR: &str = "${{ github.event.pull_request.head.repo.fork }}";
const VERSION_ENV: &str = "VELNOR_MBX_BUNDLE_VERSION";
const TOOLCHAIN_ENV: &str = "VELNOR_MBX_BUNDLE_TOOLCHAIN";
const ROOT_ENV: &str = "VELNOR_MBX_BUNDLE_ROOT";
const MARK_FILE: &str = "$RUNNER_TEMP/velnor/state-producer-ok";
const SIZE_FILE: &str = "$RUNNER_TEMP/velnor/state-payload.size";

const DISCOVERY_RECORD: &str = "$RUNNER_TEMP/velnor/state-discovery.record";
const RECOVERY_RECORD: &str = "$RUNNER_TEMP/velnor/state-recovery.record";
const PUBLICATION_RECORD: &str = "$RUNNER_TEMP/velnor/state-publication.record";

/// Baked expression for the cache-matched key published by the restore step.
///
/// The recover script classifies the restore observation (exact hit versus
/// predecessor fallback) against the intended key at runtime, so both sides
/// are baked as literal workflow expressions here rather than passed through
/// step env (the env expression allowlist does not cover step outputs).
const MATCHED_KEY_EXPR: &str = "${{steps.mbx-bundle-restore.outputs.cache-matched-key}}";

/// Cap for the number of archive members the recover and publish scripts
/// accept. Mirrors the orchestrator snapshot archive entry budget so a
/// hostile or degenerate payload is refused before extraction.
const MAX_ARCHIVE_MEMBERS: u64 = 100_000;

const RUNNER_TEMP_GUARD: &str = "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac";

fn workspace_root(working_directory: Option<&str>) -> Result<String, RenderError> {
    let Some(dir) = working_directory.filter(|dir| !dir.is_empty() && *dir != ".") else {
        return Ok(".".to_owned());
    };
    crate::action_mbx_shell::target_dir_for_action(&BTreeMap::from([(
        "working-directory".to_owned(),
        dir.to_owned(),
    )]))?;
    Ok(dir.to_owned())
}

/// Fixed shell literal for one complete-state transport expression.
///
/// Cache action inputs carry the expression form while shell scripts need
/// the shell form. Both derive from the same common family literal so the
/// transport path cannot drift between restore, recovery, publish, and save.
fn shell_literal(expr: &str) -> Result<String, RenderError> {
    expr.strip_prefix(RUNNER_TEMP_EXPR)
        .map(|rest| format!("{RUNNER_TEMP_SHELL}{rest}"))
        .ok_or_else(|| RenderError::InvalidWorkflow("state_transport_not_runner_temp".to_owned()))
}

fn split_shell(path: &str) -> Result<(String, String), RenderError> {
    path.rsplit_once('/')
        .map(|(parent, base)| (parent.to_owned(), base.to_owned()))
        .filter(|(parent, base)| !parent.is_empty() && !base.is_empty())
        .ok_or_else(|| RenderError::InvalidWorkflow("state_transport_bad_path".to_owned()))
}

pub(crate) fn mbx_bundle_prelude(
    unit: &str,
    mbx_version_expr: &str,
    toolchain_expr: &str,
    rust_toolchain: &str,
    working_directory: Option<&str>,
    env: &BTreeMap<String, String>,
) -> Result<[Step; 3], RenderError> {
    let root = workspace_root(working_directory)?;
    let discover = discover_step(unit, mbx_version_expr, toolchain_expr, &root)?;
    let restore = restore_step()?;
    let recover = recover::recover_step(rust_toolchain, working_directory, env.clone())?;
    Ok([discover, restore, recover])
}

/// Shared compatibility-digest shell fragment.
///
/// Both the discovery step and the recovery/publication steps compute the
/// snapshot compatibility digest with the same byte layout: the family
/// identity line, the OS identity lines, then the pinned lock digest. The
/// recovery side recomputes it from the discovery record values and refuses
/// on drift, which is the shell half of the coverage-replay check.
fn compat_head_lines(
    dir: &str,
    version: &str,
    toolchain: &str,
    root: &str,
    unit: &str,
    unit_is_literal: bool,
) -> Vec<String> {
    let spec = CacheFamily::MbxBundle.spec();
    let unit_arg = if unit_is_literal {
        format!("'unit={unit}'")
    } else {
        format!("\"unit={unit}\"")
    };
    vec![
        format!(
            "printf '%s\\n' 'family={}' 'schema={}' '{}' {unit_arg} \"mbx={version}\" \"toolchain={toolchain}\" \"root={root}\" 'ceiling={}' > \"{dir}/compat\"",
            spec.layer,
            spec.schema,
            spec.shell_paths(None).join("' '"),
            spec.ceiling_bytes,
        ),
        format!("uname -s >> \"{dir}/compat\""),
        format!("uname -m >> \"{dir}/compat\""),
        format!("s256 \"{dir}/compat\""),
    ]
}

const S256_FN: &str = "s256() { sha256sum \"$1\" > \"$d/sum\"; h=; rest=; read -r h rest < \"$d/sum\" || [ -n \"$h\" ]; case \"$h\" in ''|*[!0-9a-f]*) printf '%s\\n' 'sha256sum returned no digest' >&2; exit 1 ;; esac; [ \"${#h}\" -eq 64 ] || exit 1; }";

/// Discover the restore candidate and record its provenance.
///
/// This replaces the old key-only computation with a discovery step: the
/// key and restore-keys outputs keep their exact legacy semantics for the
/// transport steps, while the new `provenance`/`decision` outputs plus the
/// discovery record bind every later stage (verify the restore observation,
/// replay the coverage digest, admit the payload, publish, exact readback)
/// to the same provenance classification.
fn discover_step(
    unit: &str,
    version: &str,
    toolchain: &str,
    root: &str,
) -> Result<Step, RenderError> {
    let spec = CacheFamily::MbxBundle.spec();
    let prefix = CacheFamily::MbxBundle.key_prefix();
    let ceiling = spec.ceiling_bytes;
    let mut script = vec![
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        "d=\"$RUNNER_TEMP/velnor/state-snapshot-key\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        S256_FN.to_owned(),
        format!("default_ref=\"${{{DEFAULT_REF_ENV}:-}}\""),
        "trust=none".to_owned(),
        "if [ \"${GITHUB_EVENT_NAME:-}\" = push ] && [ -n \"${default_ref#refs/heads/}\" ] && [ \"${GITHUB_REF:-}\" = \"$default_ref\" ] && [ \"${GITHUB_REF_PROTECTED:-}\" = true ]; then trust=trusted; fi".to_owned(),
    ];
    script.extend(compat_head_lines(
        "$d",
        &format!("${{{VERSION_ENV}}}"),
        &format!("${{{TOOLCHAIN_ENV}}}"),
        &format!("${{{ROOT_ENV}}}"),
        unit,
        true,
    ));
    script.extend(
        [
            "compat=\"s256-$h\"".to_owned(),
            format!("lock=\"$GITHUB_WORKSPACE/${{{ROOT_ENV}}}/Cargo.lock\""),
            "if [ -f \"$lock\" ] && [ ! -L \"$lock\" ]; then s256 \"$lock\"; else printf '%s\\n' absent > \"$d/absent\"; s256 \"$d/absent\"; fi".to_owned(),
            "snap=\"s256-$h\"".to_owned(),
            format!("trusted=\"{prefix}trusted-$compat-\""),
            "key=\"$trusted$snap\"".to_owned(),
            format!("[ \"${{#key}}\" -le {} ] || {{ printf 'cache key exceeds the GitHub cap: %s\\n' \"$key\" >&2; exit 1; }}", velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES),
            "save=false".to_owned(),
            "[ \"$trust\" != trusted ] || save=true".to_owned(),
            "if [ \"$trust\" = trusted ]; then provenance=trusted; decision=admitted; else case \"${GITHUB_EVENT_NAME:-}\" in pull_request) provenance=untrusted ;; *) provenance=ambiguous ;; esac; decision=restore-only; fi".to_owned(),
            "printf '%s\\n' \"key=$key\" \"restore_keys=$trusted\" \"save=$save\" \"provenance=$provenance\" \"decision=$decision\" >> \"$GITHUB_OUTPUT\"".to_owned(),
            format!("record=\"{DISCOVERY_RECORD}\""),
            format!("printf '%s\\n' \"key=$key\" \"restore_prefix=$trusted\" \"compat=$compat\" \"provenance=$provenance\" \"decision=$decision\" \"ceiling={ceiling}\" \"save=$save\" \"unit={unit}\" \"mbx_version=${{{VERSION_ENV}}}\" \"toolchain=${{{TOOLCHAIN_ENV}}}\" \"root=${{{ROOT_ENV}}}\" > \"$record\""),
            "printf 'velnor: state snapshot trust=%s key=%s save=%s provenance=%s decision=%s\\n' \"$trust\" \"$key\" \"$save\" \"$provenance\" \"$decision\"".to_owned(),
        ],
    );
    let script = script.join("; ");
    let env = BTreeMap::from([
        (DEFAULT_REF_ENV.to_owned(), DEFAULT_REF_EXPR.to_owned()),
        (HEAD_FORK_ENV.to_owned(), HEAD_FORK_EXPR.to_owned()),
        (VERSION_ENV.to_owned(), version.to_owned()),
        (TOOLCHAIN_ENV.to_owned(), toolchain.to_owned()),
        (ROOT_ENV.to_owned(), root.to_owned()),
    ]);
    let mut step = shell_step(
        MBX_BUNDLE_KEY_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.id = Some(StepId::MbxBundleKey);
    step.role = Some(StepRole::MbxBundleKey);
    Ok(step)
}

fn restore_step() -> Result<Step, RenderError> {
    let mut step = super::cache_action_step(
        true,
        crate::cache_steps::TOOLS_RESTORE_ACTION_USES,
        "mbx-bundle",
        MBX_BUNDLE_KEY_EXPR,
        &[MBX_BUNDLE_RESTORE_KEYS_EXPR.to_owned()],
        &[COMPLETE_STATE_TRANSPORT_EXPR.to_owned()],
    )?;
    MBX_BUNDLE_RESTORE_NAME.clone_into(&mut step.name);
    step.id = Some(StepId::MbxBundleRestore);
    step.role = Some(StepRole::MbxBundleRestore);
    Ok(step)
}
