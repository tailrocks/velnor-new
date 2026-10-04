//! Executed adversarial fixtures for the private evidence filesystem boundary.

use std::error::Error;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;

use super::{fake_bin, prepare_controller_root, run_bash, temp_dir};

#[test]
fn exclusive_capture_bounds_and_normalizes_one_json_object() -> Result<(), Box<dyn Error>> {
    let (root, bin, env) = setup("private-positive")?;
    let output = run_bash(
        r#"root="$RUNNER_TEMP/mbx-cancel-controller"
private_capture "$root" "$root/object.json" 64 printf '%s\n' '{"ok":true}'
private_json_valid "$root" "$root/object.json" 64
private_capture "$root" "$root/text.txt" 5 printf 123456 && exit 31
[ ! -e "$root/text.txt" ]
printf accepted"#,
        &root,
        &bin,
        &env,
    )?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout)?, "accepted");
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn symlink_parent_and_existing_symlink_capture_fail_closed() -> Result<(), Box<dyn Error>> {
    let (root, bin, env) = setup("private-symlinks")?;
    let external = root.join("external");
    fs::create_dir(&external)?;
    fs::write(external.join("target"), "untouched")?;
    let output = root.join("mbx-cancel-controller/outside.json");
    symlink(external.join("target"), &output)?;
    let hardlink = root.join("mbx-cancel-controller/hardlink.json");
    fs::hard_link(external.join("target"), &hardlink)?;
    let link_parent = root.join("mbx-cancel-controller/escape");
    symlink(&external, &link_parent)?;
    let body = r#"root="$RUNNER_TEMP/mbx-cancel-controller"
private_capture "$root" "$root/outside.json" 64 printf changed && exit 31
private_capture "$root" "$root/hardlink.json" 64 printf changed && exit 32
private_capture "$root" "$root/escape/new.json" 64 printf changed && exit 33
[ -L "$root/outside.json" ] && [ ! -e "$root/escape/new.json" ]"#;
    let result = run_bash(&body, &root, &bin, &env)?;
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(fs::read_to_string(external.join("target"))?, "untouched");
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn preexisting_or_symlinked_fixed_root_is_rejected() -> Result<(), Box<dyn Error>> {
    for symlink_root in [false, true] {
        let root = temp_dir(if symlink_root {
            "private-symlink-root"
        } else {
            "private-foreign-root"
        })?;
        let bin = fake_bin(&root)?;
        let external = root.join("external");
        fs::create_dir(&external)?;
        let controller = root.join("mbx-cancel-controller");
        if symlink_root {
            symlink(&external, &controller)?;
        } else {
            fs::create_dir(&controller)?;
            fs::set_permissions(&controller, fs::Permissions::from_mode(0o700))?;
        }
        let env = vec![("RUNNER_TEMP".to_owned(), root.display().to_string())];
        let result = run_bash(
            r#"private_root_create "$RUNNER_TEMP/mbx-cancel-controller" && exit 61
printf rejected"#,
            &root,
            &bin,
            &env,
        )?;
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        assert_eq!(String::from_utf8(result.stdout)?, "rejected");
        assert!(controller.exists());
        fs::remove_dir_all(root)?;
    }
    Ok(())
}

#[test]
fn traversal_foreign_roots_and_duplicate_json_documents_are_rejected()
-> Result<(), Box<dyn Error>> {
    let (root, bin, env) = setup("private-malformed")?;
    let output = run_bash(
        r#"root="$RUNNER_TEMP/mbx-cancel-controller"
private_capture "$root" "$root/../escaped.json" 64 printf x && exit 41
private_root_create "$RUNNER_TEMP/foreign-root" && exit 42
private_capture "$root" "$root/multi.json" 64 printf '{}\n{}\n'
private_json_valid "$root" "$root/multi.json" 64 && exit 43
printf rejected"#,
        &root,
        &bin,
        &env,
    )?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout)?, "rejected");
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn duplicated_event_json_never_passes_event_validation() -> Result<(), Box<dyn Error>> {
    let (root, bin, mut env) = setup("private-event")?;
    let event = root.join("event.json");
    fs::write(&event, "{}\n{}\n")?;
    env.push(("GITHUB_EVENT_PATH".to_owned(), event.display().to_string()));
    let output = run_bash("private_event_valid && exit 51; printf rejected", &root, &bin, &env)?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout)?, "rejected");
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn oversized_and_symlinked_event_files_fail_closed() -> Result<(), Box<dyn Error>> {
    let (root, bin, mut env) = setup("private-event-bound")?;
    let event = root.join("event.json");
    fs::write(&event, vec![b' '; 65_537])?;
    env.push(("GITHUB_EVENT_PATH".to_owned(), event.display().to_string()));
    let oversized = run_bash(
        "private_event_valid && exit 71; printf rejected",
        &root,
        &bin,
        &env,
    )?;
    assert!(oversized.status.success());
    assert_eq!(String::from_utf8(oversized.stdout)?, "rejected");

    let target = root.join("event-target.json");
    fs::write(&target, r#"{"inputs":{"mode":"ok"}}"#)?;
    fs::remove_file(&event)?;
    symlink(&target, &event)?;
    let linked = run_bash(
        "private_event_valid && exit 72; printf rejected",
        &root,
        &bin,
        &env,
    )?;
    assert!(linked.status.success());
    assert_eq!(String::from_utf8(linked.stdout)?, "rejected");
    assert_eq!(fs::read_to_string(target)?, r#"{"inputs":{"mode":"ok"}}"#);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn bounded_rest_capture_rejects_duplicate_and_oversized_json() -> Result<(), Box<dyn Error>> {
    let (root, bin, env) = setup("private-api-bound")?;
    let output = run_bash(
        r#"set -euo pipefail
root="$RUNNER_TEMP/mbx-cancel-controller"
gh_api() { printf '{"id":1}\n'; }
private_gh_json "$root" "$root/api.json" --method GET /repos/tailrocks/velnor-new/actions/runs/1
jq -e '.id == 1' "$root/api.json" >/dev/null
gh_api() { printf '{}\n{}\n'; }
private_gh_json "$root" "$root/api.json" --method GET /repos/tailrocks/velnor-new/actions/runs/1 && exit 81
gh_api() { head -c 2097153 /dev/zero; }
private_gh_json "$root" "$root/oversized.json" --method GET /repos/tailrocks/velnor-new/actions/runs/1 && exit 82
[ ! -e "$root/oversized.json" ]
printf accepted"#,
        &root,
        &bin,
        &env,
    )?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout)?, "accepted");
    fs::remove_dir_all(root)?;
    Ok(())
}

fn setup(label: &str) -> Result<(PathBuf, PathBuf, Vec<(String, String)>), Box<dyn Error>> {
    let root = temp_dir(label)?;
    let bin = fake_bin(&root)?;
    let event = root.join("event.json");
    fs::write(&event, r#"{"inputs":{"mode":"mbx-cancel-during-save-controller","probe_id":""}}"#)?;
    let env = vec![
        ("RUNNER_TEMP".to_owned(), root.display().to_string()),
        ("GITHUB_EVENT_PATH".to_owned(), event.display().to_string()),
    ];
    prepare_controller_root(&root, &bin, &env)?;
    Ok((root, bin, env))
}
