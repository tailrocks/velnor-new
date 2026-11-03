// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    COMPLETE_STATE_ROOT_EXPR, COMPLETE_STATE_TRANSPORT_EXPR, RUNNER_TEMP_SHELL,
};
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_IMPORT_CONDITION, MBX_BUNDLE_KEY_EXPR, MBX_BUNDLE_MATCHED_KEY_ENV,
    MBX_BUNDLE_MATCHED_KEY_EXPR, MBX_BUNDLE_PRODUCER_SKIP_OUTPUT, MBX_BUNDLE_RESTORE_KEY_ENV,
};
use velnor_actions_contract::{Step, StepRole};

use crate::RenderError;
use crate::state_fragments::S256_FN;
use crate::steps::shell_step;

use super::{
    DISCOVERY_RECORD, MARK_FILE, MAX_COMPONENT_BYTES, MAX_MEMBER_DEPTH, MBX_BUNDLE_IMPORT_NAME,
    MBX_PRODUCER_MARK_NAME, RECOVERY_RECORD, RUNNER_TEMP_GUARD, StateCeilings, compat_head_lines,
    shell_literal, split_shell,
};

const ZSTD_MAGIC_OCTAL: &str = "\\050\\265\\057\\375";

fn ancestors(payload: &str) -> Result<Vec<String>, RenderError> {
    let (parent, _) = split_shell(payload)?;
    let relative = parent
        .strip_prefix(&format!("{RUNNER_TEMP_SHELL}/"))
        .ok_or_else(|| {
            RenderError::InvalidWorkflow("state_transport_not_runner_temp".to_owned())
        })?;
    let mut walked = RUNNER_TEMP_SHELL.to_owned();
    Ok(relative
        .split('/')
        .map(|part| {
            walked = format!("{walked}/{part}");
            format!("\"{walked}\"")
        })
        .collect())
}

fn cleanup_lines(payload_parent: &str, ancestors: &[String]) -> Vec<String> {
    vec![
        "mode=keep".to_owned(),
        "skip=false".to_owned(),
        "unlock() { for t in \"$@\"; do if [ -d \"$t\" ] && [ ! -L \"$t\" ]; then chmod -R u+rwX \"$t\" 2>/dev/null || :; fi; done; }".to_owned(),
        format!("chain() {{ for a in {}; do if [ -L \"$a\" ]; then rm -f \"$a\" || :; return 1; fi; done; return 0; }}", ancestors.join(" ")),
        format!("finish() {{ rc=$?; if [ \"$rc\" -ne 0 ]; then printf '%s\\n' '::warning::cache_corrupt: state snapshot recovery failed; building cold'; mode=purge; fi; if [ \"$mode\" = purge ] && chain; then unlock \"$root\" \"$payload\"; chmod u+rwx \"{payload_parent}\" 2>/dev/null || :; rm -rf \"$root\" \"$payload\" || :; fi; unlock \"$stage\"; rm -rf \"$stage\" \"$d\" || :; if [ -n \"${{GITHUB_OUTPUT-}}\" ]; then printf '%s\\n' \"{MBX_BUNDLE_PRODUCER_SKIP_OUTPUT}=$skip\" >> \"$GITHUB_OUTPUT\" || :; fi; exit 0; }}"),
        "trap finish EXIT".to_owned(),
    ]
}

pub(crate) fn recover_step(
    rust_toolchain: &str,
    _working_directory: Option<&str>,
    mut env: BTreeMap<String, String>,
    ceilings: StateCeilings,
) -> Result<Step, RenderError> {
    super::super::mbx_preflight::validate_rust_toolchain(rust_toolchain)?;
    env.insert(
        MBX_BUNDLE_MATCHED_KEY_ENV.to_owned(),
        MBX_BUNDLE_MATCHED_KEY_EXPR.to_owned(),
    );
    env.insert(
        MBX_BUNDLE_RESTORE_KEY_ENV.to_owned(),
        MBX_BUNDLE_KEY_EXPR.to_owned(),
    );
    let payload = shell_literal(COMPLETE_STATE_TRANSPORT_EXPR)?;
    let root = shell_literal(COMPLETE_STATE_ROOT_EXPR)?;
    let (_, base) = split_shell(&root)?;
    let (payload_parent, _) = split_shell(&payload)?;
    let mut script = vec![
        "set -eu".to_owned(),
        RUNNER_TEMP_GUARD.to_owned(),
        format!("payload=\"{payload}\""),
        format!("root=\"{root}\""),
        format!("base=\"{base}\""),
        format!("record=\"{DISCOVERY_RECORD}\""),
        format!("matched=\"${{{MBX_BUNDLE_MATCHED_KEY_ENV}:-}}\""),
        format!("selected=\"${{{MBX_BUNDLE_RESTORE_KEY_ENV}:-}}\""),
        "d=\"$RUNNER_TEMP/velnor/state-recover\"".to_owned(),
        "stage=\"$RUNNER_TEMP/velnor/state-stage\"".to_owned(),
        "plain=\"$d/plain.tar\"".to_owned(),
    ];
    script.extend(cleanup_lines(&payload_parent, &ancestors(&payload)?));
    script.extend([
        "unlock \"$stage\"; rm -rf \"$d\" \"$stage\" || :".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        S256_FN.to_owned(),
        "cold() { printf '%s\\n' \"velnor: state snapshot no_entry ($1)\"; exit 0; }".to_owned(),
        "corrupt() { printf '%s\\n' \"::warning::cache_corrupt: state snapshot $1; building cold\"; mode=purge; exit 0; }".to_owned(),
        "refused() { printf '%s\\n' \"::warning::cache_corrupt: state snapshot refused ($1); building cold\"; mode=purge; exit 0; }".to_owned(),
        "if chain; then :; else corrupt \"payload path is a symlink\"; fi".to_owned(),
        "if [ ! -e \"$payload\" ] && [ ! -L \"$payload\" ]; then cold no_entry; fi".to_owned(),
        "if [ ! -f \"$payload\" ] || [ -L \"$payload\" ]; then corrupt \"payload is not a regular file\"; fi".to_owned(),
        "if [ ! -s \"$payload\" ]; then corrupt \"payload empty\"; fi".to_owned(),
        "if [ -L \"$root\" ]; then corrupt \"state root is a symlink\"; fi".to_owned(),
        "if [ -z \"$matched\" ]; then cold cold-missing; fi".to_owned(),
        "if [ \"$matched\" != \"$selected\" ]; then refused refused-selection; fi".to_owned(),
        "case \"$matched\" in *[!A-Za-z0-9._-]*) refused refused-provenance ;; esac".to_owned(),
        "if [ ! -f \"$record\" ] || [ -L \"$record\" ]; then refused refused-provenance; fi".to_owned(),
        "snapshot_prefix=\"\"; expected_prefix=\"\"; compat=\"\"; provenance=\"\"; rec_unit=\"\"; rec_mbx=\"\"; rec_toolchain=\"\"; rec_root=\"\"".to_owned(),
        "line=; while IFS= read -r line || [ -n \"$line\" ]; do case \"$line\" in snapshot_prefix=*) snapshot_prefix=${line#snapshot_prefix=} ;; restore_prefix=*) expected_prefix=${line#restore_prefix=} ;; compat=*) compat=${line#compat=} ;; provenance=*) provenance=${line#provenance=} ;; unit=*) rec_unit=${line#unit=} ;; mbx_version=*) rec_mbx=${line#mbx_version=} ;; toolchain=*) rec_toolchain=${line#toolchain=} ;; root=*) rec_root=${line#root=} ;; esac; done < \"$record\"".to_owned(),
        "if [ -z \"$snapshot_prefix\" ] || [ -z \"$expected_prefix\" ] || [ -z \"$compat\" ] || [ -z \"$provenance\" ] || [ -z \"$rec_unit\" ] || [ -z \"$rec_mbx\" ] || [ -z \"$rec_toolchain\" ] || [ -z \"$rec_root\" ]; then refused refused-provenance; fi".to_owned(),
        "case \"$matched\" in \"$snapshot_prefix\"?*) decision=exact-hit ;; \"$expected_prefix\"?*) decision=predecessor-fallback ;; *) refused refused-provenance ;; esac".to_owned(),
    ]);
    script.extend(compat_head_lines(
        "$d",
        "$rec_mbx",
        "$rec_toolchain",
        "$rec_root",
        "$rec_unit",
    ));
    script.extend([
        "compat_now=\"s256-$h\"".to_owned(),
        "if [ \"$compat_now\" != \"$compat\" ]; then corrupt \"coverage drift\"; fi".to_owned(),
    ]);
    script.extend(admission_lines(ceilings));
    script.extend(screen_lines(ceilings));
    script.extend(extract_lines());
    script.extend(measure_lines(ceilings));
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
        "{var}=; {var}_rest=; if {source} > \"$d/{tmp}\"; then :; else corrupt \"{failure}\"; fi; read -r {var} {var}_rest < \"$d/{tmp}\" || :; case \"${var}\" in ''|*[!0-9]*) corrupt \"{failure}\" ;; esac"
    )
}

fn admission_lines(ceilings: StateCeilings) -> Vec<String> {
    let stored = ceilings.stored();
    let extracted = ceilings.extracted();
    let cap = ceilings.stream_cap();
    let cap_plus = cap + 1;
    vec![
        read_number("size", "wc -c < \"$payload\"", "size", "size unreadable"),
        format!("if [ \"$size\" -gt {stored} ]; then refused \"over_stored stored=$size ceiling={stored}\"; fi"),
        "if [ -z \"${MBX_CACHE_DIR:-}\" ] || [ \"$MBX_CACHE_DIR\" != \"$RUNNER_TEMP/velnor/mbx\" ] || [ ! -d \"$MBX_CACHE_DIR/actions\" ]; then corrupt import_unavailable; fi".to_owned(),
        "for tool in zstd cmp tar head tail wc find du sort mise mbx; do command -v \"$tool\" >/dev/null 2>&1 || corrupt import_unavailable; done".to_owned(),
        "ws=${GITHUB_WORKSPACE:-}; ws=${ws%/}; if [ -z \"$ws\" ]; then corrupt import_unavailable; fi".to_owned(),
        "case \"$RUNNER_TEMP/velnor\" in \"$ws\"|\"$ws\"/*) corrupt import_in_workspace ;; esac".to_owned(),
        "expect=${matched##*-s256-}; case \"$expect\" in ''|*[!0-9a-f]*) refused refused-digest ;; esac; if [ \"${#expect}\" -ne 64 ]; then refused refused-digest; fi".to_owned(),
        "if s256 \"$payload\"; then payload_h=\"$h\"; else corrupt \"digest unreadable\"; fi; if [ \"$payload_h\" != \"$expect\" ]; then refused refused-digest; fi".to_owned(),
        "if head -c 4 \"$payload\" > \"$d/zmagic\"; then :; else corrupt \"payload unreadable\"; fi".to_owned(),
        format!("printf '{ZSTD_MAGIC_OCTAL}' > \"$d/zref\""),
        "if cmp -s \"$d/zmagic\" \"$d/zref\"; then :; else refused refused-format; fi".to_owned(),
        format!("if {{ zstd -dc --long=30 \"$payload\" && : > \"$d/inflated\"; }} | head -c {cap_plus} > \"$plain\"; then :; else corrupt \"payload unreadable\"; fi"),
        read_number("plain_n", "wc -c < \"$plain\"", "plain_size", "payload unreadable"),
        format!("if [ \"$plain_n\" -gt {cap} ]; then refused \"over_extracted extracted>=$plain_n ceiling={extracted}\"; fi"),
        "if [ ! -f \"$d/inflated\" ]; then corrupt \"payload unreadable\"; fi".to_owned(),
        "head -c 262 \"$plain\" | tail -c 5 > \"$d/magic\"; magic=; read -r magic < \"$d/magic\" || :; if [ \"$magic\" != ustar ]; then refused refused-shadow; fi".to_owned(),
        "prefix=\"$base/\"; head -c \"${#prefix}\" \"$plain\" > \"$d/first\"; first=; read -r first < \"$d/first\" || :; if [ \"$first\" != \"$prefix\" ]; then refused refused-shadow; fi".to_owned(),
    ]
}

fn screen_lines(ceilings: StateCeilings) -> Vec<String> {
    let members = ceilings.members();
    vec![
        "if tar -tf - < \"$plain\" > \"$d/members\"; then :; else corrupt \"payload unreadable\"; fi".to_owned(),
        "if tar -tvf - < \"$plain\" > \"$d/verbose\"; then :; else corrupt \"payload unreadable\"; fi".to_owned(),
        read_number("members_n", "wc -l < \"$d/members\"", "count", "member count unreadable"),
        read_number("verbose_n", "wc -l < \"$d/verbose\"", "vcount", "member count unreadable"),
        "if [ \"$members_n\" != \"$verbose_n\" ]; then refused refused-shadow; fi".to_owned(),
        format!("if [ \"$members_n\" -gt {members} ]; then refused refused-shadow; fi"),
        "v=; while IFS= read -r v || [ -n \"$v\" ]; do case \"$v\" in -*|d*) ;; *) refused refused-shadow ;; esac; done < \"$d/verbose\"".to_owned(),
        format!("shape() {{ rest=$1; n=; while [ -n \"$rest\" ]; do c=${{rest%%/*}}; if [ \"${{#c}}\" -gt {MAX_COMPONENT_BYTES} ]; then refused refused-name; fi; n=\"$n.\"; if [ \"${{#n}}\" -gt {MAX_MEMBER_DEPTH} ]; then refused refused-depth; fi; case \"$rest\" in */*) rest=${{rest#*/}} ;; *) rest= ;; esac; done; }}"),
        "m=; while IFS= read -r m || [ -n \"$m\" ]; do case \"$m\" in /*|..|../*|*/..|*/../*|*\\\\*) refused refused-shadow ;; \"$base\"|\"$base\"/*) ;; *) refused refused-shadow ;; esac; shape \"$m\"; done < \"$d/members\"".to_owned(),
    ]
}

fn extract_lines() -> Vec<String> {
    vec![
        "if mkdir -m 700 \"$stage\"; then :; else corrupt \"payload unreadable\"; fi".to_owned(),
        "if tar -xf - -C \"$stage\" --no-same-owner < \"$plain\"; then :; else corrupt \"payload unreadable\"; fi".to_owned(),
        "rm -f \"$plain\"; unlock \"$stage\"".to_owned(),
        "if [ -d \"$stage/$base\" ] && [ ! -L \"$stage/$base\" ]; then :; else refused refused-shadow; fi".to_owned(),
        "if find \"$stage\" ! -type f ! -type d > \"$d/special\"; then :; else corrupt \"payload unreadable\"; fi; if [ -s \"$d/special\" ]; then refused refused-shadow; fi".to_owned(),
        "if find \"$stage\" -type f -links +1 > \"$d/linked\"; then :; else corrupt \"payload unreadable\"; fi; if [ -s \"$d/linked\" ]; then refused refused-shadow; fi".to_owned(),
        "if find \"$stage\" -mindepth 1 -maxdepth 1 ! -name \"$base\" > \"$d/beside\"; then :; else corrupt \"payload unreadable\"; fi; if [ -s \"$d/beside\" ]; then refused refused-shadow; fi".to_owned(),
    ]
}

fn measure_lines(ceilings: StateCeilings) -> Vec<String> {
    let extracted = ceilings.extracted();
    vec![
        read_number("usize", "du -sb \"$stage/$base\"", "uncompressed", "uncompressed size unreadable"),
        format!("if [ \"$usize\" -gt {extracted} ]; then refused \"over_extracted extracted=$usize ceiling={extracted}\"; fi"),
        "if command -v sha256sum >/dev/null 2>&1; then if find \"$stage/$base\" -type f -exec sha256sum {} + > \"$d/found\"; then :; else corrupt \"payload unreadable\"; fi; else if find \"$stage/$base\" -type f -exec shasum -a 256 {} + > \"$d/found\"; then :; else corrupt \"payload unreadable\"; fi; fi".to_owned(),
        "if sort \"$d/found\" > \"$d/manifest\"; then :; else corrupt \"payload unreadable\"; fi".to_owned(),
        "if [ ! -s \"$d/manifest\" ]; then corrupt \"empty manifest\"; fi".to_owned(),
        "if s256 \"$d/manifest\"; then manifest=\"$h\"; else corrupt \"digest unreadable\"; fi".to_owned(),
        "digest=\"$payload_h\"".to_owned(),
    ]
}

fn import_lines(rust_toolchain: &str) -> Vec<String> {
    vec![
        "if cd \"$RUNNER_TEMP/velnor\"; then :; else corrupt import_unavailable; fi".to_owned(),
        format!("if mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- mbx cache import \"$stage/$base\"; then :; else corrupt import_failed; fi"),
        "if rm -rf \"$stage\" \"$root\"; then :; else corrupt import_failed; fi".to_owned(),
        format!("if printf '%s\\n' \"decision=$decision\" \"matched=$matched\" \"manifest=$manifest\" \"payload=$digest\" \"size=$size\" > \"{RECOVERY_RECORD}\"; then :; else corrupt import_failed; fi"),
        "printf 'velnor: state snapshot recovered (decision=%s manifest=%s)\\n' \"$decision\" \"$manifest\"".to_owned(),
        "if [ \"$decision\" = exact-hit ]; then skip=true; fi".to_owned(),
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
