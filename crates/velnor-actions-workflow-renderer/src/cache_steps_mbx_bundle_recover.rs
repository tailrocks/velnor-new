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

pub(crate) fn recover_step(
    rust_toolchain: &str,
    _working_directory: Option<&str>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    super::super::mbx_preflight::validate_rust_toolchain(rust_toolchain)?;
    let payload = shell_literal(COMPLETE_STATE_TRANSPORT_EXPR)?;
    let root = shell_literal(COMPLETE_STATE_ROOT_EXPR)?;
    let (_, base) = split_shell(&root)?;
    let ceiling = CacheFamily::MbxBundle.spec().ceiling_bytes;
    let ceiling_kb = ceiling.div_ceil(1024);
    let mut script = vec![
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        format!("payload=\"{payload}\""),
        format!("root=\"{root}\""),
        format!("base=\"{base}\""),
        format!("record=\"{DISCOVERY_RECORD}\""),
        format!("expected=\"{MBX_BUNDLE_KEY_EXPR}\""),
        format!("matched=\"{MATCHED_KEY_EXPR}\""),
        "d=\"$RUNNER_TEMP/velnor/state-recover\"".to_owned(),
        "stage=\"$RUNNER_TEMP/velnor/state-stage\"".to_owned(),
        "plain=\"$d/plain.tar\"".to_owned(),
        "unlock() { chmod -R u+rwX \"$stage\" 2>/dev/null || :; }".to_owned(),
        "unlock; rm -rf \"$d\" \"$stage\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        S256_FN.to_owned(),
        "cold() { printf '%s\\n' \"velnor: state snapshot no_entry ($1)\"; exit 0; }".to_owned(),
        "corrupt() { printf '%s\\n' \"::warning::cache_corrupt: state snapshot $1; building cold\"; unlock; rm -rf \"$root\" \"$stage\"; rm -f \"$payload\" \"$plain\"; exit 0; }".to_owned(),
        "refused() { printf '%s\\n' \"::warning::cache_corrupt: state snapshot refused ($1); building cold\"; unlock; rm -rf \"$root\" \"$stage\"; rm -f \"$payload\" \"$plain\"; exit 0; }".to_owned(),
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
    script.extend([
        "compat_now=\"s256-$h\"".to_owned(),
        "if [ \"$compat_now\" != \"$compat\" ]; then corrupt \"coverage drift\"; fi".to_owned(),
    ]);
    script.extend(admission_lines(ceiling));
    script.extend(screen_lines());
    script.extend(extract_lines());
    script.extend(measure_lines(ceiling, ceiling_kb));
    script.extend(import_lines(rust_toolchain));
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

fn read_number(var: &str, source: &str, tmp: &str, failure: &str) -> String {
    format!(
        "{var}=; {var}_rest=; {source} > \"$d/{tmp}\"; read -r {var} {var}_rest < \"$d/{tmp}\" || [ -n \"${var}\" ]; case \"${var}\" in ''|*[!0-9]*) corrupt \"{failure}\" ;; esac"
    )
}

fn admission_lines(ceiling: u64) -> Vec<String> {
    let cap_plus = ceiling + 1;
    vec![
        read_number("size", "wc -c < \"$payload\"", "size", "size unreadable"),
        format!("if [ \"$size\" -gt {ceiling} ]; then refused refused-oversize; fi"),
        "if [ -z \"${MBX_CACHE_DIR:-}\" ] || [ \"$MBX_CACHE_DIR\" != \"$RUNNER_TEMP/velnor/mbx\" ] || [ ! -d \"$MBX_CACHE_DIR/actions\" ]; then corrupt import_unavailable; fi".to_owned(),
        "for tool in gzip tar head tail wc find du sort mise mbx; do command -v \"$tool\" >/dev/null 2>&1 || corrupt import_unavailable; done".to_owned(),
        format!("if {{ gzip -dc \"$payload\" && : > \"$d/inflated\"; }} | head -c {cap_plus} > \"$plain\"; then :; else corrupt \"payload unreadable\"; fi"),
        read_number("plain_n", "wc -c < \"$plain\"", "plain_size", "payload unreadable"),
        format!("if [ \"$plain_n\" -gt {ceiling} ]; then refused refused-oversize; fi"),
        "if [ ! -f \"$d/inflated\" ]; then corrupt \"payload unreadable\"; fi".to_owned(),
        "head -c 262 \"$plain\" | tail -c 5 > \"$d/magic\"; magic=; read -r magic < \"$d/magic\" || [ -n \"$magic\" ]; if [ \"$magic\" != ustar ]; then refused refused-shadow; fi".to_owned(),
    ]
}

fn screen_lines() -> Vec<String> {
    vec![
        "if tar -tf \"$plain\" > \"$d/members\"; then :; else corrupt \"payload unreadable\"; fi".to_owned(),
        "if tar -tvf \"$plain\" > \"$d/verbose\"; then :; else corrupt \"payload unreadable\"; fi".to_owned(),
        read_number("members_n", "wc -l < \"$d/members\"", "count", "member count unreadable"),
        read_number("verbose_n", "wc -l < \"$d/verbose\"", "vcount", "member count unreadable"),
        "if [ \"$members_n\" != \"$verbose_n\" ]; then refused refused-shadow; fi".to_owned(),
        format!("if [ \"$members_n\" -gt {MAX_ARCHIVE_MEMBERS} ]; then refused refused-shadow; fi"),
        "v=; while IFS= read -r v || [ -n \"$v\" ]; do case \"$v\" in -*|d*) ;; *) refused refused-shadow ;; esac; done < \"$d/verbose\"".to_owned(),
        "m=; while IFS= read -r m || [ -n \"$m\" ]; do case \"$m\" in /*|..|../*|*/..|*/../*) refused refused-shadow ;; \"$base\"|\"$base\"/*) ;; *) refused refused-shadow ;; esac; done < \"$d/members\"".to_owned(),
    ]
}

fn extract_lines() -> Vec<String> {
    vec![
        "mkdir -m 700 \"$stage\"".to_owned(),
        "if tar -xf \"$plain\" -C \"$stage\" --no-same-owner; then :; else corrupt \"payload unreadable\"; fi".to_owned(),
        "rm -f \"$plain\"; unlock".to_owned(),
        "if [ -d \"$stage/$base\" ] && [ ! -L \"$stage/$base\" ]; then :; else refused refused-shadow; fi".to_owned(),
        "if find \"$stage\" ! -type f ! -type d > \"$d/special\"; then :; else corrupt \"payload unreadable\"; fi; if [ -s \"$d/special\" ]; then refused refused-shadow; fi".to_owned(),
        "if find \"$stage\" -type f -links +1 > \"$d/linked\"; then :; else corrupt \"payload unreadable\"; fi; if [ -s \"$d/linked\" ]; then refused refused-shadow; fi".to_owned(),
        "if find \"$stage\" -mindepth 1 -maxdepth 1 ! -name \"$base\" > \"$d/beside\"; then :; else corrupt \"payload unreadable\"; fi; if [ -s \"$d/beside\" ]; then refused refused-shadow; fi".to_owned(),
    ]
}

fn measure_lines(ceiling: u64, ceiling_kb: u64) -> Vec<String> {
    vec![
        format!("if du -sb \"$stage/$base\" > \"$d/uncompressed\"; then usize=; usize_rest=; read -r usize usize_rest < \"$d/uncompressed\" || [ -n \"$usize\" ]; case \"$usize\" in ''|*[!0-9]*) corrupt \"uncompressed size unreadable\" ;; esac; if [ \"$usize\" -gt {ceiling} ]; then corrupt \"over ceiling\"; fi; elif du -sk \"$stage/$base\" > \"$d/uncompressed_kb\"; then usize_kb=; usize_kb_rest=; read -r usize_kb usize_kb_rest < \"$d/uncompressed_kb\" || [ -n \"$usize_kb\" ]; case \"$usize_kb\" in ''|*[!0-9]*) corrupt \"uncompressed size unreadable\" ;; esac; if [ \"$usize_kb\" -gt {ceiling_kb} ]; then corrupt \"over ceiling\"; fi; else corrupt \"uncompressed size unreadable\"; fi"),
        "if find \"$stage/$base\" -type f > \"$d/found\"; then :; else corrupt \"payload unreadable\"; fi; sort \"$d/found\" > \"$d/files\"".to_owned(),
        "if command -v sha256sum >/dev/null 2>&1; then while IFS= read -r f || [ -n \"$f\" ]; do sha256sum \"$f\"; done < \"$d/files\" > \"$d/manifest\"; else while IFS= read -r f || [ -n \"$f\" ]; do shasum -a 256 \"$f\"; done < \"$d/files\" > \"$d/manifest\"; fi".to_owned(),
        "if [ ! -s \"$d/manifest\" ]; then corrupt \"empty manifest\"; fi".to_owned(),
        "s256 \"$d/manifest\"; manifest=\"$h\"".to_owned(),
        "s256 \"$payload\"; digest_one=\"$h\"".to_owned(),
        "s256 \"$payload\"; digest_two=\"$h\"".to_owned(),
        "if [ \"$digest_one\" != \"$digest_two\" ]; then corrupt \"readback mismatch\"; fi".to_owned(),
    ]
}

fn import_lines(rust_toolchain: &str) -> Vec<String> {
    vec![
        "cd \"$RUNNER_TEMP/velnor\"".to_owned(),
        format!("if mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- mbx cache import \"$stage/$base\"; then :; else corrupt import_failed; fi"),
        "rm -rf \"$stage\" \"$root\"".to_owned(),
        format!("printf '%s\\n' \"decision=$decision\" \"matched=$matched\" \"manifest=$manifest\" \"payload=$digest_one\" \"size=$size\" > \"{RECOVERY_RECORD}\""),
        "printf 'velnor: state snapshot recovered (decision=%s manifest=%s)\\n' \"$decision\" \"$manifest\"".to_owned(),
    ]
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
