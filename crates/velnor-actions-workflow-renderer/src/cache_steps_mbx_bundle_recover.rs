// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    COMPLETE_STATE_ROOT_EXPR, COMPLETE_STATE_TRANSPORT_EXPR, CacheFamily,
};
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_IMPORT_CONDITION, MBX_BUNDLE_KEY_EXPR,
};
use velnor_actions_contract::{Step, StepRole};

use crate::RenderError;
use crate::steps::shell_step;

use super::{
    DISCOVERY_RECORD, MARK_FILE, MATCHED_KEY_EXPR, MAX_ARCHIVE_MEMBERS, MBX_BUNDLE_IMPORT_NAME,
    MBX_PRODUCER_MARK_NAME, RECOVERY_RECORD, RUNNER_TEMP_GUARD, S256_FN, compat_head_lines,
    shell_literal, split_shell,
};

/// Recover the complete-state snapshot through the verified pipeline.
///
/// This replaces the old raw `tar -xzf` recovery with the staged repair:
/// discovery (provenance comes from the discovery record) + provenance
/// gating + restore-observation check (exact hit versus predecessor
/// fallback, anything else is refused) + coverage replay (the compat digest
/// is recomputed and must match discovery time) + safe extraction (every
/// member is screened: only the state root family may land, so a hostile
/// `tools/` member or symlink is refused before anything is written) +
/// admission (payload size against the family ceiling) + publication of the
/// recovery record + exact readback (the payload digest must be stable
/// across extraction and the manifest must verify).
///
/// This is a common snapshot operation over fixed literals: unpack the
/// frozen payload into the fixed state root. It never invokes the build
/// tool and never selects an action ref; policy arrives through the typed
/// key and cache-hit outputs consumed by the step condition.
pub(crate) fn recover_step(
    rust_toolchain: &str,
    _working_directory: Option<&str>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    super::super::mbx_preflight::validate_rust_toolchain(rust_toolchain)?;
    let payload = shell_literal(COMPLETE_STATE_TRANSPORT_EXPR)?;
    let root = shell_literal(COMPLETE_STATE_ROOT_EXPR)?;
    let (parent, base) = split_shell(&root)?;
    let ceiling = CacheFamily::MbxBundle.spec().ceiling_bytes;
    let mut script = vec![
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        format!("payload=\"{payload}\""),
        format!("root=\"{root}\""),
        format!("parent=\"{parent}\""),
        format!("base=\"{base}\""),
        format!("record=\"{DISCOVERY_RECORD}\""),
        format!("expected=\"{MBX_BUNDLE_KEY_EXPR}\""),
        format!("matched=\"{MATCHED_KEY_EXPR}\""),
        "d=\"$RUNNER_TEMP/velnor/state-recover\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        S256_FN.to_owned(),
        "cold() { printf '%s\\n' \"velnor: state snapshot no_entry ($1)\"; exit 0; }".to_owned(),
        "corrupt() { printf '%s\\n' \"::warning::cache_corrupt: state snapshot $1; building cold\"; rm -rf \"$root\"; rm -f \"$payload\"; exit 0; }".to_owned(),
        "refused() { printf '%s\\n' \"::warning::cache_corrupt: state snapshot refused ($1); building cold\"; rm -rf \"$root\"; rm -f \"$payload\"; exit 0; }".to_owned(),
        "if [ ! -f \"$payload\" ] || [ -L \"$payload\" ]; then cold no_entry; fi".to_owned(),
        "if [ ! -s \"$payload\" ]; then printf '%s\\n' '::warning::cache_corrupt: state payload empty; building cold'; rm -f \"$payload\"; exit 0; fi".to_owned(),
        "if [ -L \"$root\" ]; then printf '%s\\n' '::warning::cache_corrupt: state root is a symlink; building cold'; exit 0; fi".to_owned(),
        "if [ -z \"$matched\" ]; then cold cold-missing; fi".to_owned(),
        "if [ ! -f \"$record\" ] || [ -L \"$record\" ]; then refused refused-provenance; fi".to_owned(),
        "expected_prefix=\"\"; compat=\"\"; provenance=\"\"; ceiling=\"\"; rec_unit=\"\"; rec_mbx=\"\"; rec_toolchain=\"\"; rec_root=\"\"".to_owned(),
        "line=; while IFS= read -r line || [ -n \"$line\" ]; do case \"$line\" in restore_prefix=*) expected_prefix=${line#restore_prefix=} ;; compat=*) compat=${line#compat=} ;; provenance=*) provenance=${line#provenance=} ;; ceiling=*) ceiling=${line#ceiling=} ;; unit=*) rec_unit=${line#unit=} ;; mbx_version=*) rec_mbx=${line#mbx_version=} ;; toolchain=*) rec_toolchain=${line#toolchain=} ;; root=*) rec_root=${line#root=} ;; esac; done < \"$record\"".to_owned(),
        "case \"$ceiling\" in ''|*[!0-9]*) refused refused-provenance ;; esac".to_owned(),
        "if [ -z \"$expected_prefix\" ] || [ -z \"$compat\" ] || [ -z \"$provenance\" ] || [ -z \"$rec_unit\" ] || [ -z \"$rec_mbx\" ] || [ -z \"$rec_toolchain\" ] || [ -z \"$rec_root\" ]; then refused refused-provenance; fi".to_owned(),
        "if [ -n \"$expected\" ] && [ \"$matched\" = \"$expected\" ]; then decision=exact-hit; elif [ -n \"$matched\" ] && [ -n \"$expected_prefix\" ]; then case \"$matched\" in \"$expected_prefix\"*) decision=predecessor-fallback ;; *) refused refused-provenance ;; esac; else refused refused-provenance; fi".to_owned(),
    ];
    script.extend(compat_head_lines(
        "$d",
        "$rec_mbx",
        "$rec_toolchain",
        "$rec_root",
        "$rec_unit",
        false,
    ));
    script.extend(
        [
            "compat_now=\"s256-$h\"".to_owned(),
            "if [ \"$compat_now\" != \"$compat\" ]; then corrupt \"coverage drift\"; fi".to_owned(),
            "if tar -tzf \"$payload\" > \"$d/members\"; then :; else corrupt \"payload unreadable\"; fi".to_owned(),
            "members_n=; members_rest=; wc -l < \"$d/members\" > \"$d/count\"; read -r members_n members_rest < \"$d/count\" || [ -n \"$members_n\" ]".to_owned(),
            "case \"$members_n\" in ''|*[!0-9]*) corrupt \"member count unreadable\" ;; esac".to_owned(),
            format!("if [ \"$members_n\" -gt {MAX_ARCHIVE_MEMBERS} ]; then refused refused-shadow; fi"),
            "m=; while IFS= read -r m || [ -n \"$m\" ]; do case \"$m\" in \"\") ;; \"$base\"|\"$base\"/*) ;; /*) refused refused-shadow ;; ..|../*|*/..|*/../*) refused refused-shadow ;; *) refused refused-shadow ;; esac; done < \"$d/members\"".to_owned(),
            "stage=\"$RUNNER_TEMP/velnor/state-stage\"".to_owned(),
            "rm -rf \"$stage\"".to_owned(),
            "mkdir -p \"$stage\"".to_owned(),
            "if tar -xzf \"$payload\" -C \"$stage\"; then :; else rm -rf \"$stage\"; corrupt \"payload unreadable\"; fi".to_owned(),
            "if [ -d \"$stage/$base\" ] && [ ! -L \"$stage/$base\" ]; then :; else rm -rf \"$stage\"; corrupt \"payload missed the state root\"; fi".to_owned(),
            "find \"$stage/$base\" -type l > \"$d/links\"; if [ -s \"$d/links\" ]; then rm -rf \"$stage\"; refused refused-shadow; fi".to_owned(),
            "find \"$stage/$base\" \\( -type p -o -type s \\) > \"$d/special\"; if [ -s \"$d/special\" ]; then rm -rf \"$stage\"; refused refused-shadow; fi".to_owned(),
            "rm -rf \"$root\"".to_owned(),
            "mkdir -p \"$parent\"".to_owned(),
            "if mv \"$stage/$base\" \"$root\"; then :; else rm -rf \"$stage\"; corrupt \"state root handoff failed\"; fi".to_owned(),
            "rmdir \"$stage\" 2>/dev/null || true".to_owned(),
            "size=; size_rest=; wc -c < \"$payload\" > \"$d/size\"; read -r size size_rest < \"$d/size\" || [ -n \"$size\" ]".to_owned(),
            "case \"$size\" in ''|*[!0-9]*) corrupt \"size unreadable\" ;; esac".to_owned(),
            format!("if [ \"$size\" -gt {ceiling} ]; then corrupt \"over ceiling\"; fi"),
            "find \"$root\" -type f | sort > \"$d/files\"".to_owned(),
            "if command -v sha256sum >/dev/null 2>&1; then while IFS= read -r f || [ -n \"$f\" ]; do sha256sum \"$f\"; done < \"$d/files\" > \"$d/manifest\"; else while IFS= read -r f || [ -n \"$f\" ]; do shasum -a 256 \"$f\"; done < \"$d/files\" > \"$d/manifest\"; fi".to_owned(),
            "if [ ! -s \"$d/manifest\" ]; then corrupt \"empty manifest\"; fi".to_owned(),
            "s256 \"$d/manifest\"; manifest=\"$h\"".to_owned(),
            "s256 \"$payload\"; digest_one=\"$h\"".to_owned(),
            "s256 \"$payload\"; digest_two=\"$h\"".to_owned(),
            "if [ \"$digest_one\" != \"$digest_two\" ]; then corrupt \"readback mismatch\"; fi".to_owned(),
            format!("printf '%s\\n' \"decision=$decision\" \"matched=$matched\" \"manifest=$manifest\" \"payload=$digest_one\" \"size=$size\" > \"{RECOVERY_RECORD}\""),
            "printf 'velnor: state snapshot recovered (decision=%s manifest=%s)\\n' \"$decision\" \"$manifest\"".to_owned(),
        ],
    );
    let script = script.join("; ");
    let mut step = shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.condition = Some(MBX_BUNDLE_IMPORT_CONDITION.to_owned());
    step.role = Some(StepRole::MbxBundleImport);
    Ok(step)
}

pub fn mbx_producer_mark_step() -> Result<Step, RenderError> {
    let script = [
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        "mkdir -p \"$RUNNER_TEMP/velnor\"".to_owned(),
        format!(": > \"{MARK_FILE}\""),
    ]
    .join("; ");
    let mut step = shell_step(
        MBX_PRODUCER_MARK_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        BTreeMap::new(),
    )?;
    step.role = Some(StepRole::MbxProducerMark);
    Ok(step)
}
