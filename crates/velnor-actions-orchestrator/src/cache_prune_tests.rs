use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use velnor_actions_contract::StepKind;
use velnor_actions_contract::workflow::cache_family::CacheFamily;

use super::*;

use velnor_actions_contract::StepRole;

fn script_of(job: &Job) -> String {
    let StepKind::Shell { run, .. } = &job.steps[1].kind else {
        panic!("the prune step is a shell step");
    };
    run[2].clone()
}

#[test]
fn only_the_prune_job_holds_actions_write_and_only_after_a_trusted_push() {
    let (id, job) = prune_job(
        "ubuntu-26.04",
        &ToolCatalog::pinned(),
        &["dependency-seed".to_owned()],
    )
    .expect("job");
    assert_eq!(id, "cache-prune");
    assert_eq!(job.permissions, Some(prune_permissions()));
    assert_eq!(prune_permissions().actions, PermissionLevel::Write);
    assert_eq!(prune_permissions().contents, PermissionLevel::None);
    assert_eq!(job.needs, [FINAL_JOB_ID, "dependency-seed"]);
    assert_eq!(
        job.steps[0].role,
        Some(StepRole::PreparePinnedTools),
        "the pinned gh is installed first"
    );
    let condition = job.condition.as_deref().expect("condition");
    for part in [
        "github.event_name == 'push'",
        "github.ref_protected == true",
        "format('refs/heads/{0}', github.event.repository.default_branch)",
        "!cancelled()",
    ] {
        assert!(condition.contains(part), "{part}");
    }
}

#[test]
fn every_family_is_pruned_by_its_own_trusted_prefix_and_retention() {
    let targets = prune_targets();
    assert_eq!(targets.len(), CacheFamily::ALL.len());
    let (_, job) = prune_job("ubuntu-26.04", &ToolCatalog::pinned(), &[]).expect("job");
    let script = script_of(&job);
    for family in CacheFamily::ALL {
        let prefix = format!("{}trusted-", family.key_prefix());
        assert!(
            script.contains(&format!(
                "prune '{prefix}' {}",
                family.spec().retained_entries
            )),
            "{prefix}"
        );
        assert!(!prefix.contains("-pr-"), "the pr scope is never listed");
    }
    assert!(script.contains("--ref \"$ref\""), "one ref only");
    assert!(!script.contains("delete --all"), "no global prune");
}

#[test]
fn the_token_is_scoped_to_this_job_and_the_scrub_overlay_stays() {
    let (_, job) = prune_job("ubuntu-26.04", &ToolCatalog::pinned(), &[]).expect("job");
    let StepKind::Shell { env, .. } = &job.steps[1].kind else {
        panic!("shell");
    };
    assert_eq!(
        env.get("GH_TOKEN").map(String::as_str),
        Some("${{ github.token }}")
    );
    assert_eq!(env.get("GITHUB_TOKEN").map(String::as_str), Some(""));
}

/// Run the generated script against a stub `gh` that serves a fixed list.
#[test]
fn the_script_keeps_the_newest_entries_per_unit_and_deletes_the_rest() {
    let root = std::env::temp_dir().join(format!("velnor-prune-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&root));
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    std::fs::create_dir_all(root.join("runner")).expect("runner");
    let digest = |n: u8| format!("s256-{}", format!("{n:02x}").repeat(32));
    let unit = |u: u8| {
        format!(
            "velnor-v1-mbx-bundle-trusted-s256-{}",
            format!("{u:02x}").repeat(32)
        )
    };
    // Newest first, as `--sort created_at --order desc` returns them.
    let list = format!(
        "11\t{a}-{d1}\n12\t{a}-{d2}\n13\t{b}-{d1}\n14\t{a}-{d3}\n",
        a = unit(0xaa),
        b = unit(0xbb),
        d1 = digest(1),
        d2 = digest(2),
        d3 = digest(3)
    );
    std::fs::write(root.join("list.tsv"), list).expect("list");
    let stub = "#!/bin/sh\ncase \"$1 $2\" in\n  'cache list') case \"$4\" in velnor-v1-mbx-bundle-trusted-*) cat \"$STUB_LIST\" ;; esac ;;\n  'cache delete') printf '%s\\n' \"$3\" >> \"$STUB_DELETED\" ;;\nesac\n";
    let mise = "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n";
    for (name, body) in [("gh", stub), ("mise", mise)] {
        let path = root.join("bin").join(name);
        std::fs::write(&path, body).expect("stub");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let (_, job) = prune_job("ubuntu-26.04", &ToolCatalog::pinned(), &[]).expect("job");
    let output = Command::new("sh")
        .args(["-c", &script_of(&job)])
        .env(
            "PATH",
            format!(
                "{}:{}",
                root.join("bin").display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("RUNNER_TEMP", root.join("runner"))
        .env("VELNOR_DEFAULT_REF", "refs/heads/main")
        .env("STUB_LIST", root.join("list.tsv"))
        .env("STUB_DELETED", root.join("deleted"))
        .output()
        .expect("script runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let deleted = std::fs::read_to_string(root.join("deleted")).expect("deleted");
    // Unit aa keeps its newest entry (retained 1): 11 stays, 12 and 14 go;
    // unit bb has a single entry and is untouched.
    assert_eq!(deleted.lines().collect::<Vec<_>>(), ["12", "14"]);
    drop(std::fs::remove_dir_all(&root));
}
