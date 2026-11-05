// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    COMPLETE_STATE_TRANSPORT_EXPR, CacheFamily, RUNNER_TEMP_EXPR, RUNNER_TEMP_SHELL,
    S256_DIGEST_BYTES, SLICE_CEILING, StateTrust, StateUnit,
};
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_KEY_EXPR, MBX_BUNDLE_RESTORE_CONDITION, MBX_BUNDLE_ROOT_ENV, MBX_BUNDLE_SNAPSHOT_ENV,
};
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_ATTESTED_ENV;
use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

use crate::RenderError;
use crate::state_fragments::{
    CompatBinding, S256_FN, absent_digest_lines, compat_lines, lock_digest_lines, snapshot_lines,
};
use crate::steps::shell_step;

#[path = "cache_steps_mbx_bundle_publish.rs"]
mod publish;
#[path = "cache_steps_mbx_bundle_recover.rs"]
mod recover;
#[path = "cache_steps_mbx_bundle_unit.rs"]
mod unit;

pub use publish::mbx_bundle_publish_bounded;
pub use recover::mbx_producer_mark_step;
pub use unit::MbxTransport;
pub(crate) use unit::UnitBinding;

pub const MBX_BUNDLE_KEY_NAME: &str = "Compute state snapshot key";
pub const MBX_BUNDLE_RESTORE_NAME: &str = "Restore state snapshot";
pub const MBX_BUNDLE_IMPORT_NAME: &str = "Recover state snapshot";
pub const MBX_PRODUCER_MARK_NAME: &str = "Record state producer success";
pub const MBX_BUNDLE_EXPORT_NAME: &str = "Publish state snapshot";
pub const MBX_EXPORT_GROUP_ENV: &str = "MBX_CACHE_EXPORT_GROUP";
pub const MBX_EXPORT_GROUP_VALUE: &str = "velnor";

const DEFAULT_REF_ENV: &str = "VELNOR_DEFAULT_REF";
const HEAD_FORK_ENV: &str = "VELNOR_PR_HEAD_FORK";
const DEFAULT_REF_EXPR: &str =
    "${{ format('refs/heads/{0}', github.event.repository.default_branch) }}";
const HEAD_FORK_EXPR: &str = "${{ github.event.pull_request.head.repo.fork }}";
const VERSION_ENV: &str = "VELNOR_MBX_BUNDLE_VERSION";
const TOOLCHAIN_ENV: &str = "VELNOR_MBX_BUNDLE_TOOLCHAIN";
const ROOT_ENV: &str = MBX_BUNDLE_ROOT_ENV;
const MARK_FILE: &str = "$RUNNER_TEMP/velnor/state-producer-ok";
const SIZE_FILE: &str = "$RUNNER_TEMP/velnor/state-payload.size";

const DISCOVERY_RECORD: &str = "$RUNNER_TEMP/velnor/state-discovery.record";
const RECOVERY_RECORD: &str = "$RUNNER_TEMP/velnor/state-recovery.record";

/// Cap for the number of archive members the recover and publish scripts
/// accept. Mirrors the orchestrator snapshot archive entry budget so a
/// hostile or degenerate payload is refused before extraction.
const MAX_ARCHIVE_MEMBERS: u64 = 100_000;

const STREAM_SLACK_PER_MEMBER: u64 = 1024;

const STREAM_SLACK_FIXED: u64 = 10_240;

const MAX_MEMBER_DEPTH: u64 = 64;

const MAX_COMPONENT_BYTES: u64 = 255;

const RUNNER_TEMP_GUARD: &str = "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateCeilings {
    stored: u64,
    extracted: u64,
    members: u64,
}

impl StateCeilings {
    #[must_use]
    pub const fn of_unit(unit: &StateUnit) -> Self {
        let ceiling = unit.ceiling();
        Self::new(ceiling.stored(), ceiling.extracted(), MAX_ARCHIVE_MEMBERS)
    }

    pub const SLICE: Self = Self::new(
        SLICE_CEILING.stored(),
        SLICE_CEILING.extracted(),
        MAX_ARCHIVE_MEMBERS,
    );

    #[must_use]
    pub(crate) const fn of_binding(binding: &UnitBinding) -> Self {
        match binding {
            UnitBinding::Fixed(unit) => Self::of_unit(unit),
            UnitBinding::ProducerMatrix => Self::SLICE,
        }
    }

    #[must_use]
    pub const fn new(stored: u64, extracted: u64, members: u64) -> Self {
        Self {
            stored,
            extracted,
            members,
        }
    }

    #[must_use]
    pub const fn stored(self) -> u64 {
        self.stored
    }

    #[must_use]
    pub const fn extracted(self) -> u64 {
        self.extracted
    }

    #[must_use]
    pub const fn members(self) -> u64 {
        self.members
    }

    #[must_use]
    pub const fn stream_cap(self) -> u64 {
        self.extracted + self.members * STREAM_SLACK_PER_MEMBER + STREAM_SLACK_FIXED
    }
}

pub(crate) fn workspace_root(working_directory: Option<&str>) -> Result<String, RenderError> {
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
    unit: &UnitBinding,
    mbx_version_expr: &str,
    toolchain_expr: &str,
    rust_toolchain: &str,
    working_directory: Option<&str>,
    env: &BTreeMap<String, String>,
    ceilings: StateCeilings,
) -> Result<[Step; 3], RenderError> {
    let root = workspace_root(working_directory)?;
    let discover = discover_step(unit, mbx_version_expr, toolchain_expr, &root)?;
    let restore = restore_step()?;
    let recover = recover::recover_step(rust_toolchain, working_directory, env.clone(), ceilings)?;
    Ok([discover, restore, recover])
}

pub(crate) fn compat_head_lines(
    dir: &str,
    version: &str,
    toolchain: &str,
    root: &str,
    unit: &str,
) -> Vec<String> {
    compat_lines(
        dir,
        &CompatBinding {
            unit,
            mbx: version,
            toolchain,
            root,
        },
    )
}

fn snapshot_source(unit: &UnitBinding, root_env: &str) -> Vec<String> {
    if unit.snapshot_from_plan() {
        vec![
            format!("snap=\"${{{MBX_BUNDLE_SNAPSHOT_ENV}-}}\""),
            "case \"$snap\" in s256-*) [ \"${#snap}\" -eq 69 ] || snap= ;; *) snap= ;; esac"
                .to_owned(),
            "case \"${snap#s256-}\" in *[!0-9a-f]*) snap= ;; esac".to_owned(),
            format!(
                "if [ -z \"$snap\" ]; then rm -f \"{DISCOVERY_RECORD}\"; printf '%s\\n' 'reason=snapshot_missing' 'save=false' 'restore=false' >> \"$GITHUB_OUTPUT\"; printf '%s\\n' 'velnor: state snapshot disabled reason=snapshot_missing'; exit 0; fi"
            ),
        ]
    } else {
        let mut lines = vec![format!(
            "lock=\"$GITHUB_WORKSPACE/${{{root_env}}}/Cargo.lock\""
        )];
        lines.extend(lock_digest_lines("$d", "$lock", "lock_h"));
        lines.extend(absent_digest_lines("$d", "absent_h"));
        lines.extend(snapshot_lines(
            "$d",
            "$lock_h",
            "$absent_h",
            "$absent_h",
            "$absent_h",
        ));
        lines
    }
}

fn discover_step(
    unit: &UnitBinding,
    version: &str,
    toolchain: &str,
    root: &str,
) -> Result<Step, RenderError> {
    let scope_prefix = format!(
        "{}{}-{}-",
        CacheFamily::MbxBundle.key_prefix(),
        StateTrust::Trusted.as_str(),
        unit.token()
    );
    let max_payload_segment = S256_DIGEST_BYTES;
    let mut script = vec![
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        "d=\"$RUNNER_TEMP/velnor/state-snapshot-key\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        S256_FN.to_owned(),
        format!("default_ref=\"${{{DEFAULT_REF_ENV}:-}}\""),
        format!("if [ -z \"${{ImageOS-}}\" ]; then rm -f \"{DISCOVERY_RECORD}\"; printf '%s\\n' 'reason=image_os_missing' 'save=false' 'restore=false' >> \"$GITHUB_OUTPUT\"; printf '%s\\n' 'velnor: state snapshot disabled reason=image_os_missing'; exit 0; fi"),
        "trust=none".to_owned(),
        "if [ \"${GITHUB_EVENT_NAME:-}\" = push ] && [ -n \"${default_ref#refs/heads/}\" ] && [ \"${GITHUB_REF:-}\" = \"$default_ref\" ] && [ \"${GITHUB_REF_PROTECTED:-}\" = true ]; then trust=trusted; fi".to_owned(),
    ];
    script.extend(unit.unit_lines());
    script.extend(compat_head_lines(
        "$d",
        &format!("${{{VERSION_ENV}}}"),
        &format!("${{{TOOLCHAIN_ENV}}}"),
        &format!("${{{ROOT_ENV}}}"),
        &unit.token(),
    ));
    script.push("compat=\"s256-$h\"".to_owned());
    script.extend(snapshot_source(unit, ROOT_ENV));
    script.extend([
        format!("restore_prefix=\"{scope_prefix}$compat-\""),
        "snapshot_prefix=\"$restore_prefix$snap-\"".to_owned(),
        "key=\"$snapshot_prefix\"".to_owned(),
        format!("[ \"${{#key}}\" -le {} ] || {{ printf 'cache key exceeds the GitHub cap: %s\\n' \"$key\" >&2; exit 1; }}", velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES - max_payload_segment),
        "save=false".to_owned(),
        "[ \"$trust\" != trusted ] || save=true".to_owned(),
        "if [ \"$trust\" = trusted ]; then provenance=trusted; decision=admitted; else case \"${GITHUB_EVENT_NAME:-}\" in pull_request) provenance=untrusted ;; *) provenance=ambiguous ;; esac; decision=restore-only; fi".to_owned(),
        "restore_key=; restore_digest=; exact=false".to_owned(),
        format!("printf '%s\\n' \"${{{STATE_SEAL_ATTESTED_ENV}-}}\" > \"$d/attested\""),
        "while IFS= read -r cand || [ -n \"$cand\" ]; do case \"$cand\" in \"$restore_prefix\"?*) ;; *) continue ;; esac; if printf '%s\\n' \"${cand#\"$restore_prefix\"}\" | grep -Eq '^s256-[0-9a-f]{64}-s256-[0-9a-f]{64}$'; then restore_key=$cand; break; fi; done < \"$d/attested\"".to_owned(),
        "if [ -n \"$restore_key\" ]; then restore_digest=\"s256-${restore_key##*-s256-}\"; case \"$restore_key\" in \"$snapshot_prefix\"?*) exact=true ;; esac; fi".to_owned(),
        "printf '%s\\n' \"key=$key\" \"save=$save\" \"restore=true\" \"provenance=$provenance\" \"decision=$decision\" \"restore_key=$restore_key\" \"restore_digest=$restore_digest\" \"exact=$exact\" >> \"$GITHUB_OUTPUT\"".to_owned(),
        format!("record=\"{DISCOVERY_RECORD}\""),
        format!("printf '%s\\n' \"key=$key\" \"snapshot_prefix=$snapshot_prefix\" \"restore_prefix=$restore_prefix\" \"compat=$compat\" \"provenance=$provenance\" \"decision=$decision\" \"save=$save\" \"unit={}\" \"mbx_version=${{{VERSION_ENV}}}\" \"toolchain=${{{TOOLCHAIN_ENV}}}\" \"root=${{{ROOT_ENV}}}\" > \"$record\"", unit.token()),
        "printf 'velnor: state snapshot trust=%s key=%s save=%s provenance=%s decision=%s\\n' \"$trust\" \"$key\" \"$save\" \"$provenance\" \"$decision\"".to_owned(),
    ]);
    let script = script.join("; ");
    let mut env = BTreeMap::from([
        (DEFAULT_REF_ENV.to_owned(), DEFAULT_REF_EXPR.to_owned()),
        (HEAD_FORK_ENV.to_owned(), HEAD_FORK_EXPR.to_owned()),
        (VERSION_ENV.to_owned(), version.to_owned()),
        (TOOLCHAIN_ENV.to_owned(), toolchain.to_owned()),
        (ROOT_ENV.to_owned(), root.to_owned()),
    ]);
    env.extend(unit.env());
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
        &[],
        &[COMPLETE_STATE_TRANSPORT_EXPR.to_owned()],
    )?;
    if let StepKind::Action { with, .. } = &mut step.kind {
        with.remove("restore-keys");
    }
    MBX_BUNDLE_RESTORE_NAME.clone_into(&mut step.name);
    step.condition = Some(MBX_BUNDLE_RESTORE_CONDITION.to_owned());
    step.id = Some(StepId::MbxBundleRestore);
    step.role = Some(StepRole::MbxBundleRestore);
    Ok(step)
}
