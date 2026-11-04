// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    COMPLETE_STATE_ROOT_EXPR, COMPLETE_STATE_TRANSPORT_EXPR, MBX_BSE_ENV, MBX_BSE_PRODUCER,
    MBX_COMPILE_ENV,
};
use velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_EXPORT_CONDITION;
use velnor_actions_contract::{Step, StepId, StepRole};

use crate::RenderError;
use crate::state_fragments::S256_FN;
use crate::steps::shell_step;

use super::{
    DISCOVERY_RECORD, MARK_FILE, MBX_BUNDLE_EXPORT_NAME, MBX_EXPORT_GROUP_ENV, RECOVERY_RECORD,
    RUNNER_TEMP_GUARD, SIZE_FILE, StateCeilings, compat_head_lines, shell_literal, split_shell,
};

pub fn mbx_bundle_publish_bounded(
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
    ceilings: StateCeilings,
) -> Result<Step, RenderError> {
    publish_step(rust_toolchain, None, env, ceilings)
}

pub(crate) fn publish_step(
    rust_toolchain: &str,
    _working_directory: Option<&str>,
    env: BTreeMap<String, String>,
    ceilings: StateCeilings,
) -> Result<Step, RenderError> {
    super::super::mbx_preflight::validate_rust_toolchain(rust_toolchain)?;
    let payload = shell_literal(COMPLETE_STATE_TRANSPORT_EXPR)?;
    let root = shell_literal(COMPLETE_STATE_ROOT_EXPR)?;
    let (parent, base) = split_shell(&root)?;
    let (payload_parent, _payload_base) = split_shell(&payload)?;
    let stored = ceilings.stored();
    let members = ceilings.members();
    let mut script = vec![
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        format!("payload=\"{payload}\""),
        format!("root=\"{root}\""),
        format!("parent=\"{parent}\""),
        format!("base=\"{base}\""),
        format!("mark=\"{MARK_FILE}\""),
        format!("record=\"{DISCOVERY_RECORD}\""),
        "d=\"$RUNNER_TEMP/velnor/state-publish\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        S256_FN.to_owned(),
        "out=${GITHUB_OUTPUT-}".to_owned(),
        "skip() { if [ -n \"$out\" ]; then printf '%s\\n' 'exported=false' >> \"$out\"; fi; printf 'velnor: state snapshot not published (%s)\\n' \"$1\"; exit 0; }".to_owned(),
        "if [ ! -f \"$mark\" ] || [ -L \"$mark\" ]; then skip producer_not_succeeded; fi".to_owned(),
        format!("[ -n \"${{{MBX_EXPORT_GROUP_ENV}:-}}\" ] || skip export_group_missing"),
        "command -v tar >/dev/null 2>&1 || { printf '%s\\n' '::warning::cache_unavailable: tar is missing; save refused'; skip tar_missing; }".to_owned(),
        "command -v zstd >/dev/null 2>&1 || { printf '%s\\n' '::warning::cache_unavailable: zstd is missing; save refused'; skip zstd_missing; }".to_owned(),
        "tar --version > \"$d/tar_version\" 2>/dev/null || :; tar_line=; read -r tar_line < \"$d/tar_version\" || :; case \"$tar_line\" in *'GNU tar'*) ;; *) printf '%s\\n' '::warning::cache_unavailable: tar is not GNU tar; save refused'; skip tar_not_gnu ;; esac".to_owned(),
        "command -v wc >/dev/null 2>&1 || { printf '%s\\n' '::warning::cache_unavailable: wc is missing; save refused'; skip wc_missing; }".to_owned(),
        "command -v du >/dev/null 2>&1 || { printf '%s\\n' '::warning::cache_unavailable: du is missing; save refused'; skip du_missing; }".to_owned(),
        "command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || { printf '%s\\n' '::warning::cache_unavailable: no digest tool; save refused'; skip digest_unavailable; }".to_owned(),
        "if [ ! -f \"$record\" ] || [ -L \"$record\" ]; then skip provenance_unavailable; fi".to_owned(),
        "provenance=\"\"; rec_snapshot_prefix=\"\"; rec_compat=\"\"; rec_unit=\"\"; rec_mbx=\"\"; rec_toolchain=\"\"; rec_root=\"\"".to_owned(),
        "line=; while IFS= read -r line || [ -n \"$line\" ]; do case \"$line\" in provenance=*) provenance=${line#provenance=} ;; snapshot_prefix=*) rec_snapshot_prefix=${line#snapshot_prefix=} ;; compat=*) rec_compat=${line#compat=} ;; unit=*) rec_unit=${line#unit=} ;; mbx_version=*) rec_mbx=${line#mbx_version=} ;; toolchain=*) rec_toolchain=${line#toolchain=} ;; root=*) rec_root=${line#root=} ;; esac; done < \"$record\"".to_owned(),
        "if [ \"$provenance\" != trusted ]; then skip provenance_refused; fi".to_owned(),
        "if [ -z \"$rec_snapshot_prefix\" ] || [ -z \"$rec_compat\" ] || [ -z \"$rec_unit\" ] || [ -z \"$rec_mbx\" ] || [ -z \"$rec_toolchain\" ] || [ -z \"$rec_root\" ]; then skip provenance_refused; fi".to_owned(),
    ];
    script.extend(exact_hit_lines());
    script.extend(env_gate_lines());
    script.extend(compat_head_lines(
        "$d",
        "$rec_mbx",
        "$rec_toolchain",
        "$rec_root",
        "$rec_unit",
    ));
    script.extend(
        [
            "compat_now=\"s256-$h\"".to_owned(),
            "if [ \"$compat_now\" != \"$rec_compat\" ]; then skip coverage_drift; fi".to_owned(),
            cold_produce_fragment(rust_toolchain, ceilings.extracted()),
            format!("mkdir -p \"{payload_parent}\""),
            "rm -f \"$payload\"".to_owned(),
            "rm -f \"$d/packed\"".to_owned(),
            "if { tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 -cf - -C \"$parent\" \"$base\" && : > \"$d/packed\"; } | zstd -3 --long=30 -T4 -q -o \"$payload\" && [ -f \"$d/packed\" ]; then :; else rm -f \"$payload\"; printf '%s\\n' '::warning::cache_unavailable: state snapshot pack failed'; skip pack_failed; fi".to_owned(),
            "if [ ! -f \"$payload\" ] || [ -L \"$payload\" ]; then printf '%s\\n' '::warning::cache_unavailable: state payload missing after pack'; skip pack_missing; fi".to_owned(),
            format!("wc -c < \"$payload\" > \"{SIZE_FILE}\""),
            format!("size=; rest=; read -r size rest < \"{SIZE_FILE}\" || [ -n \"$size\" ]"),
            "case \"$size\" in ''|*[!0-9]*) rm -f \"$payload\"; skip size_unreadable ;; esac".to_owned(),
            format!("if [ \"$size\" -gt {stored} ]; then rm -f \"$payload\"; printf '%s\\n' \"::warning::state snapshot refused: over_stored stored=$size ceiling={stored}\"; skip over_stored; fi"),
            "if [ \"$size\" -eq 0 ]; then rm -f \"$payload\"; skip empty_snapshot; fi".to_owned(),
            "s256 \"$payload\"; digest_one=\"$h\"".to_owned(),
            "rm -f \"$d/read\"".to_owned(),
            "if { zstd -dc --long=30 \"$payload\" && : > \"$d/read\"; } | tar -tf - > \"$d/members\" && [ -f \"$d/read\" ]; then :; else rm -f \"$payload\"; skip pack_unreadable; fi".to_owned(),
            "members_n=; members_rest=; wc -l < \"$d/members\" > \"$d/count\"; read -r members_n members_rest < \"$d/count\" || [ -n \"$members_n\" ]".to_owned(),
            "case \"$members_n\" in ''|*[!0-9]*) rm -f \"$payload\"; skip member_count_unreadable ;; esac".to_owned(),
            format!("if [ \"$members_n\" -gt {members} ]; then rm -f \"$payload\"; skip member_over_budget; fi"),
            "m=; while IFS= read -r m || [ -n \"$m\" ]; do case \"$m\" in \"\") ;; \"$base\"|\"$base\"/*) ;; *) rm -f \"$payload\"; skip readback_refused ;; esac; done < \"$d/members\"".to_owned(),
            "find \"$root\" -type f | sort > \"$d/files\"".to_owned(),
            "if command -v sha256sum >/dev/null 2>&1; then while IFS= read -r f || [ -n \"$f\" ]; do sha256sum \"$f\"; done < \"$d/files\" > \"$d/manifest\"; else while IFS= read -r f || [ -n \"$f\" ]; do shasum -a 256 \"$f\"; done < \"$d/files\" > \"$d/manifest\"; fi".to_owned(),
            "if [ ! -s \"$d/manifest\" ]; then rm -f \"$payload\"; skip empty_manifest; fi".to_owned(),
            "s256 \"$d/manifest\"; manifest=\"$h\"".to_owned(),
            "s256 \"$payload\"; digest_two=\"$h\"".to_owned(),
            "size_two=; size_two_rest=; wc -c < \"$payload\" > \"$d/size_two\"; read -r size_two size_two_rest < \"$d/size_two\" || [ -n \"$size_two\" ]".to_owned(),
            "if [ \"$digest_one\" != \"$digest_two\" ] || [ \"$size\" != \"$size_two\" ]; then rm -f \"$payload\"; skip readback_mismatch; fi".to_owned(),
            "printf 'velnor: state snapshot readback verified (sha256:%s)\\n' \"$digest_one\"".to_owned(),
            "if [ -n \"$out\" ]; then printf '%s\\n' 'exported=true' >> \"$out\"; fi".to_owned(),
        ],
    );
    let script = script.join("; ");
    let mut step = shell_step(
        MBX_BUNDLE_EXPORT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.id = Some(StepId::MbxBundleExport);
    step.condition = Some(MBX_BUNDLE_EXPORT_CONDITION.to_owned());
    step.role = Some(StepRole::MbxBundleExport);
    Ok(step)
}

fn exact_hit_lines() -> Vec<String> {
    vec![format!(
        "recovered=; if [ -f \"{RECOVERY_RECORD}\" ] && [ ! -L \"{RECOVERY_RECORD}\" ]; then line=; while IFS= read -r line || [ -n \"$line\" ]; do case \"$line\" in decision=*) recovered=${{line#decision=}} ;; esac; done < \"{RECOVERY_RECORD}\"; fi; if [ \"$recovered\" = exact-hit ]; then skip exact_hit; fi"
    )]
}

fn env_gate_lines() -> Vec<String> {
    MBX_COMPILE_ENV
        .iter()
        .chain(&[(MBX_BSE_ENV, MBX_BSE_PRODUCER)])
        .map(|(name, value)| {
            format!("if [ \"${{{name}-}}\" != \"{value}\" ]; then printf '%s\\n' '::warning::state snapshot refused: {name} is not {value}'; skip env_mismatch; fi")
        })
        .collect()
}

fn cold_produce_fragment(rust_toolchain: &str, extracted: u64) -> String {
    let produce = format!(
        "command -v mise >/dev/null 2>&1 || skip export_unavailable; command -v mbx >/dev/null 2>&1 || skip export_unavailable; [ \"${{MBX_CACHE_DIR:-}}\" = \"$RUNNER_TEMP/velnor/mbx\" ] && [ -d \"$MBX_CACHE_DIR/actions\" ] || skip export_unavailable; rm -rf \"$d/export\"; if mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- mbx cache export --group \"$MBX_CACHE_EXPORT_GROUP\" --format directory \"$d/export\"; then :; else rm -rf \"$d/export\"; skip export_failed; fi; [ -d \"$d/export\" ] && [ ! -L \"$d/export\" ] || skip export_empty"
    );
    let measure = format!(
        "if du -sb \"$d/export\" > \"$d/export_bytes\"; then esize=; esize_rest=; read -r esize esize_rest < \"$d/export_bytes\" || [ -n \"$esize\" ]; case \"$esize\" in ''|*[!0-9]*) rm -rf \"$d/export\"; skip export_unreadable ;; esac; if [ \"$esize\" -gt {extracted} ]; then rm -rf \"$d/export\"; printf '%s\\n' \"::warning::state snapshot refused: over_extracted extracted=$esize ceiling={extracted}\"; skip over_extracted; fi; else rm -rf \"$d/export\"; skip export_unreadable; fi"
    );
    let install =
        "mkdir -p \"$parent\"; if mv \"$d/export\" \"$root\"; then :; else skip export_handoff; fi";
    format!("rm -rf \"$root\"; {produce}; {measure}; {install}")
}
