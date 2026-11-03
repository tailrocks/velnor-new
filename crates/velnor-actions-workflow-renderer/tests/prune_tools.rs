// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::cache_prune::{
    TOOLS_PRUNE_PREFIX, TOOLS_PRUNE_UNIT, TOOLS_RETAINED_ENTRIES, prune_group_key, prune_script,
    prune_targets,
};

#[test]
fn tools_family_has_a_retained_prune_target() {
    let targets = prune_targets();
    assert!(
        targets
            .iter()
            .any(|(prefix, keep)| prefix == TOOLS_PRUNE_PREFIX && *keep == TOOLS_RETAINED_ENTRIES),
        "unexpected prune targets: {targets:?}"
    );
}

#[test]
fn tools_keys_share_one_prune_group() {
    for key in [
        "mise-tools-v2-33b8bde4f201ea4312115e8b97c50876eaafa5398f6fe2ce130af30459195fb",
        "mise-tools-v2-0000000000000000000000000000000000000000000000000000000000000000",
    ] {
        assert_eq!(prune_group_key(key), TOOLS_PRUNE_UNIT, "key: {key}");
    }
}

#[test]
fn snapshot_keys_group_by_unit_without_digest() {
    assert_eq!(
        prune_group_key(
            "velnor-v2-sources-trusted-s256-e32b46924cd679f385a2d1a7e3c22253aa3c0be67fb101bb0e636963ba16b528"
        ),
        "velnor-v2-sources-trusted"
    );
    assert_eq!(
        prune_group_key(
            "velnor-v3-state-trusted-s256-0d58c66670730fc3af7f9579c097b5b947410057122dc7bddc2482c60f929886"
        ),
        "velnor-v3-state-trusted"
    );
}

#[test]
fn keys_without_snapshot_suffix_keep_identity() {
    assert_eq!(
        prune_group_key("velnor-v3-state-trusted"),
        "velnor-v3-state-trusted"
    );
}

#[test]
fn prune_script_emits_tools_retention() {
    let script = prune_script();
    let invocation = format!("prune '{TOOLS_PRUNE_PREFIX}' {TOOLS_RETAINED_ENTRIES}");
    assert!(
        script.contains(&invocation),
        "prune script is missing the tools retention call"
    );
}

#[cfg(unix)]
const GH_SHIM: &str = "#!/bin/sh\ncase \"$1 $2\" in\n\"cache list\") shift 2; key=; while [ $# -gt 0 ]; do case \"$1\" in --key) key=$2; shift 2;; *) shift;; esac; done; awk -F'\\t' -v p=\"$key\" 'index($2,p)==1' \"$GH_FIXTURE\" ;;\n\"cache delete\") echo \"$3\" >> \"$GH_DELETED\" ;;\nesac\n";

#[test]
#[cfg(unix)]
fn retired_state_entries_are_all_pruned_and_current_ones_kept()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!("velnor-prune-retired-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir)?;
    let hash = |c: char| c.to_string().repeat(64);
    let (a, b, c) = (hash('a'), hash('b'), hash('c'));
    let fixture = format!(
        "11\tvelnor-v2-state-trusted-s256-{a}-s256-{b}\n12\tvelnor-v2-state-trusted-s256-{a}-s256-{c}\n13\tvelnor-v2-state-trusted-s256-{c}-s256-{b}\n21\tvelnor-v3-state-trusted-s256-{a}-s256-{b}\n22\tvelnor-v3-state-trusted-s256-{c}-s256-{b}\n31\tvelnor-v4-state-trusted-union-root-s256-{a}-s256-{b}-s256-{c}\n"
    );
    std::fs::write(dir.join("fixture"), fixture)?;
    let shim = dir.join("gh");
    std::fs::write(&shim, GH_SHIM)?;
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))?;
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(prune_script())
        .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
        .env("GITHUB_REF", "refs/heads/main")
        .env("RUNNER_TEMP", &dir)
        .env("GH_FIXTURE", dir.join("fixture"))
        .env("GH_DELETED", dir.join("deleted"))
        .status()?;
    assert!(status.success(), "prune script failed");
    let mut deleted: Vec<String> = std::fs::read_to_string(dir.join("deleted"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    deleted.sort();
    assert_eq!(
        deleted,
        ["11", "12", "13", "21", "22"],
        "retired v2 and v3 state entries must all be deleted and v4 kept"
    );
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
