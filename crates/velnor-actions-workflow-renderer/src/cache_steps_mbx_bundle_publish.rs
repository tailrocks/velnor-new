// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    COMPLETE_STATE_ROOT_EXPR, COMPLETE_STATE_TRANSPORT_EXPR, CacheFamily,
};
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_EXPORT_CONDITION, MBX_BUNDLE_KEY_EXPR, MBX_BUNDLE_SAVE_CONDITION,
};
use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

use crate::RenderError;
use crate::steps::shell_step;

use super::{
    DISCOVERY_RECORD, MARK_FILE, MAX_ARCHIVE_MEMBERS, MBX_BUNDLE_EXPORT_NAME, MBX_BUNDLE_SAVE_NAME,
    MBX_EXPORT_GROUP_ENV, PUBLICATION_RECORD, RUNNER_TEMP_GUARD, S256_FN, SIZE_FILE,
    compat_head_lines, shell_literal, split_shell,
};

pub(crate) fn bundle_postlude(action: &Step) -> Result<[Step; 2], RenderError> {
    let StepKind::Action { with, env, .. } = &action.kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_bundle_action_kind_mismatch".to_owned(),
        ));
    };
    let rust = with
        .get("toolchain")
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_bundle_toolchain_missing".to_owned()))?;
    let working_directory = with.get("working-directory").map(String::as_str);
    let export = publish_step(rust, working_directory, env.clone())?;
    let save = save_step()?;
    Ok([export, save])
}

/// Publish the complete-state snapshot through admission and publication.
///
/// This replaces the old pack-and-print export with the staged repair:
/// discovery (provenance comes from the discovery record and must be
/// trusted) + coverage replay (the compat digest is recomputed and must
/// match discovery time) + admission (the state root must be built and the
/// payload must fit the family ceiling) + deterministic packing (the
/// archive stream is reproducible so a later readback replays the same
/// bytes) + publication of the publication record + exact readback (the
/// payload digest and member list are re-verified before `exported=true`
/// is reported, so the `published` print below can no longer fire early).
///
/// This is a common snapshot operation over fixed literals: pack the fixed
/// state root into the frozen payload and admit the exact payload bytes
/// against the family ceiling. The admission decision flows through the
/// typed `exported` output consumed by the save condition; the payload is
/// never republished when admission refuses it.
pub(crate) fn publish_step(
    rust_toolchain: &str,
    _working_directory: Option<&str>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    super::super::mbx_preflight::validate_rust_toolchain(rust_toolchain)?;
    let payload = shell_literal(COMPLETE_STATE_TRANSPORT_EXPR)?;
    let root = shell_literal(COMPLETE_STATE_ROOT_EXPR)?;
    let (parent, base) = split_shell(&root)?;
    let (payload_parent, _payload_base) = split_shell(&payload)?;
    let ceiling = CacheFamily::MbxBundle.spec().ceiling_bytes;
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
        "if [ ! -d \"$root\" ] || [ -L \"$root\" ]; then skip state_not_built; fi".to_owned(),
        "command -v tar >/dev/null 2>&1 || { printf '%s\\n' '::warning::cache_unavailable: tar is missing; save refused'; skip tar_missing; }".to_owned(),
        "command -v wc >/dev/null 2>&1 || { printf '%s\\n' '::warning::cache_unavailable: wc is missing; save refused'; skip wc_missing; }".to_owned(),
        "command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || { printf '%s\\n' '::warning::cache_unavailable: no digest tool; save refused'; skip digest_unavailable; }".to_owned(),
        "if [ ! -f \"$record\" ] || [ -L \"$record\" ]; then skip provenance_unavailable; fi".to_owned(),
        "provenance=\"\"; rec_compat=\"\"; rec_unit=\"\"; rec_mbx=\"\"; rec_toolchain=\"\"; rec_root=\"\"".to_owned(),
        "line=; while IFS= read -r line || [ -n \"$line\" ]; do case \"$line\" in provenance=*) provenance=${line#provenance=} ;; compat=*) rec_compat=${line#compat=} ;; unit=*) rec_unit=${line#unit=} ;; mbx_version=*) rec_mbx=${line#mbx_version=} ;; toolchain=*) rec_toolchain=${line#toolchain=} ;; root=*) rec_root=${line#root=} ;; esac; done < \"$record\"".to_owned(),
        "if [ \"$provenance\" != trusted ]; then skip provenance_refused; fi".to_owned(),
        "if [ -z \"$rec_compat\" ] || [ -z \"$rec_unit\" ] || [ -z \"$rec_mbx\" ] || [ -z \"$rec_toolchain\" ] || [ -z \"$rec_root\" ]; then skip provenance_refused; fi".to_owned(),
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
            "if [ \"$compat_now\" != \"$rec_compat\" ]; then skip coverage_drift; fi".to_owned(),
            format!("mkdir -p \"{payload_parent}\""),
            "rm -f \"$payload\"".to_owned(),
            "if tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 -czf \"$payload\" -C \"$parent\" \"$base\"; then :; else rm -f \"$payload\"; if tar -czf \"$payload\" -C \"$parent\" \"$base\"; then :; else rm -f \"$payload\"; printf '%s\\n' '::warning::cache_unavailable: state snapshot pack failed'; skip pack_failed; fi; fi".to_owned(),
            "if [ ! -f \"$payload\" ] || [ -L \"$payload\" ]; then printf '%s\\n' '::warning::cache_unavailable: state payload missing after pack'; skip pack_missing; fi".to_owned(),
            format!("wc -c < \"$payload\" > \"{SIZE_FILE}\""),
            format!("size=; rest=; read -r size rest < \"{SIZE_FILE}\" || [ -n \"$size\" ]"),
            "case \"$size\" in ''|*[!0-9]*) rm -f \"$payload\"; skip size_unreadable ;; esac".to_owned(),
            format!("if [ \"$size\" -gt {ceiling} ]; then rm -f \"$payload\"; printf '%s\\n' '::warning::over_ceiling: state snapshot exceeds the family ceiling; save refused'; skip over_ceiling; fi"),
            "if [ \"$size\" -eq 0 ]; then rm -f \"$payload\"; skip empty_snapshot; fi".to_owned(),
            "s256 \"$payload\"; digest_one=\"$h\"".to_owned(),
            "if tar -tzf \"$payload\" > \"$d/members\"; then :; else rm -f \"$payload\"; skip pack_unreadable; fi".to_owned(),
            "members_n=; members_rest=; wc -l < \"$d/members\" > \"$d/count\"; read -r members_n members_rest < \"$d/count\" || [ -n \"$members_n\" ]".to_owned(),
            "case \"$members_n\" in ''|*[!0-9]*) rm -f \"$payload\"; skip member_count_unreadable ;; esac".to_owned(),
            format!("if [ \"$members_n\" -gt {MAX_ARCHIVE_MEMBERS} ]; then rm -f \"$payload\"; skip member_over_budget; fi"),
            "m=; while IFS= read -r m || [ -n \"$m\" ]; do case \"$m\" in \"\") ;; \"$base\"|\"$base\"/*) ;; *) rm -f \"$payload\"; skip readback_refused ;; esac; done < \"$d/members\"".to_owned(),
            "find \"$root\" -type f | sort > \"$d/files\"".to_owned(),
            "if command -v sha256sum >/dev/null 2>&1; then while IFS= read -r f || [ -n \"$f\" ]; do sha256sum \"$f\"; done < \"$d/files\" > \"$d/manifest\"; else while IFS= read -r f || [ -n \"$f\" ]; do shasum -a 256 \"$f\"; done < \"$d/files\" > \"$d/manifest\"; fi".to_owned(),
            "if [ ! -s \"$d/manifest\" ]; then rm -f \"$payload\"; skip empty_manifest; fi".to_owned(),
            "s256 \"$d/manifest\"; manifest=\"$h\"".to_owned(),
            "s256 \"$payload\"; digest_two=\"$h\"".to_owned(),
            "size_two=; size_two_rest=; wc -c < \"$payload\" > \"$d/size_two\"; read -r size_two size_two_rest < \"$d/size_two\" || [ -n \"$size_two\" ]".to_owned(),
            "if [ \"$digest_one\" != \"$digest_two\" ] || [ \"$size\" != \"$size_two\" ]; then rm -f \"$payload\"; skip readback_mismatch; fi".to_owned(),
            format!("printf '%s\\n' \"payload=$digest_one\" \"size=$size\" \"members=$members_n\" \"manifest=$manifest\" \"compat=$compat_now\" > \"{PUBLICATION_RECORD}\""),
            "printf 'velnor: state snapshot readback verified (sha256:%s)\\n' \"$digest_one\"".to_owned(),
            "if [ -n \"$out\" ]; then printf '%s\\n' 'exported=true' >> \"$out\"; fi".to_owned(),
            "printf 'velnor: state snapshot published (%s bytes, sha256:%s)\\n' \"$size\" \"$digest_one\"".to_owned(),
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

fn save_step() -> Result<Step, RenderError> {
    let mut step = super::super::cache_action_step(
        false,
        crate::cache_steps::TOOLS_SAVE_USES,
        "mbx-bundle",
        MBX_BUNDLE_KEY_EXPR,
        &[],
        &[COMPLETE_STATE_TRANSPORT_EXPR.to_owned()],
    )?;
    MBX_BUNDLE_SAVE_NAME.clone_into(&mut step.name);
    step.condition = Some(MBX_BUNDLE_SAVE_CONDITION.to_owned());
    step.role = Some(StepRole::MbxBundleSave);
    Ok(step)
}

pub(crate) fn append_bundle_postludes(
    jobs: &mut BTreeMap<String, velnor_actions_contract::Job>,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        let restores = count(&job.steps, StepRole::MbxBundleRestore);
        let exports = count(&job.steps, StepRole::MbxBundleExport);
        let marks = count(&job.steps, StepRole::MbxProducerMark);
        if restores == 0 && exports == 0 && marks == 0 {
            continue;
        }
        if marks == 0 && exports == 0 {
            continue;
        }
        if marks != 1 {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_producer_mark_count:{id}"
            )));
        }
        if restores != 1 || exports != 0 {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_bundle_lifecycle:{id}"
            )));
        }
        let action = job
            .steps
            .iter()
            .find(|step| step.role == Some(StepRole::MbxCache))
            .ok_or_else(|| RenderError::InvalidWorkflow(format!("mbx_bundle_no_action:{id}")))?;
        let postlude = bundle_postlude(action)?;
        job.steps.extend(postlude);
    }
    Ok(())
}

fn count(steps: &[Step], role: StepRole) -> usize {
    steps.iter().filter(|step| step.role == Some(role)).count()
}
