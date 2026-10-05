//! The import script runs. A seed hit imports a private copy.
//! `mbx cache import` removes that copy. The shared seed stays.
//! A cache miss with no seed stays cold.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_actions_contract::StepKind;

use super::{export_step, import_script, import_step, save_step};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("velnor-mbx-seed-{name}-{}", std::process::id()));
    fs::remove_dir_all(&path).ok();
    fs::create_dir_all(&path).expect("scratch");
    path
}

fn fake_tool(bin: &Path, name: &str, body: &str) {
    let path = bin.join(name);
    fs::write(&path, body).expect("fake");
    let mut perms = fs::metadata(&path).expect("meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).expect("mode");
}

fn fake_mbx(bin: &Path) {
    fake_tool(
        bin,
        "mbx",
        "#!/bin/sh\necho \"$@\" >> \"$MBX_LOG\"\nif [ \"${MBX_EXIT:-0}\" = 1 ]; then exit 1; fi\nif [ \"$1\" = cache ] && [ \"$2\" = import ]; then rm -rf \"$3\"; fi\nexit 0\n",
    );
    fake_tool(bin, "df", "#!/bin/sh\nexit 0\n");
    fake_tool(bin, "uname", "#!/bin/sh\necho Linux\n");
    fake_tool(
        bin,
        "stat",
        "#!/bin/sh\necho \"${SEED_OWNER_UID:-0}\"\n",
    );
    fake_tool(
        bin,
        "findmnt",
        "#!/bin/sh\necho \"${SEED_MOUNT_OPTIONS:-ro,relatime}\"\n",
    );
    fake_tool(
        bin,
        "find",
        "#!/bin/sh\nif [ \"${SEED_UNSAFE:-0}\" = 1 ]; then echo \"$1/untrusted\"; fi\n",
    );
}

fn run(script: &str, root: &Path, matched: &str, prefix: &str, exit: &str) -> (String, String) {
    run_with_seed_state(script, root, matched, prefix, exit, "0", "ro,relatime", "0")
}

fn run_with_seed_state(
    script: &str,
    root: &Path,
    matched: &str,
    prefix: &str,
    exit: &str,
    owner_uid: &str,
    mount_options: &str,
    unsafe_tree: &str,
) -> (String, String) {
    let bin = root.join("bin");
    let home = root.join("home");
    let runner = root.join("rt");
    fs::create_dir_all(&bin).expect("bin");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&runner).expect("rt");
    fake_mbx(&bin);
    let log = root.join("mbx.log");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("HOME", &home)
        .env("RUNNER_TEMP", &runner)
        .env("MATCHED", matched)
        .env("PREFIX", prefix)
        .env("MBX_LOG", &log)
        .env("MBX_EXIT", exit)
        .env("SEED_OWNER_UID", owner_uid)
        .env("SEED_MOUNT_OPTIONS", mount_options)
        .env("SEED_UNSAFE", unsafe_tree)
        .env("PATH", path)
        .output()
        .expect("bash");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");
    let calls = fs::read_to_string(&log).unwrap_or_default();
    (text, calls)
}

#[test]
fn shipped_import_uses_the_fixed_root_and_push_save_stays() {
    let step = import_step().expect("import");
    let StepKind::Shell { run, env } = &step.kind else {
        panic!("import step must be a shell step");
    };
    let script = import_script("/opt/velnor/seed").expect("script");
    assert!(run[2].contains(&script), "{}", run[2]);
    assert!(run[2].contains("/opt/velnor/seed/mbx"), "{}", run[2]);
    assert!(!run[2].contains("test -d"), "{}", run[2]);
    assert_eq!(
        env.get("PREFIX").map(String::as_str),
        Some("${{ steps.mbx-cache-key.outputs.prefix }}")
    );
    let export = export_step().expect("export").condition.expect("if");
    let save = save_step().expect("save").condition.expect("if");
    assert!(export.contains("github.event_name == 'push'"), "{export}");
    assert!(save.contains("github.event_name == 'push'"), "{save}");
    assert!(!export.contains("pull_request"), "{export}");
    assert!(!save.contains("pull_request"), "{save}");
}

#[test]
fn seed_hit_imports_the_seed_bundle_and_keeps_it() {
    let root = scratch("hit");
    let seed = root.join("seed");
    fs::create_dir_all(&seed).expect("seed root");
    fs::write(seed.join("PROVENANCE"), "velnor-host-seed-v1").expect("provenance");
    fs::create_dir_all(seed.join("mbx/bundle")).expect("bundle");
    fs::write(seed.join("mbx/PREFIX"), "tool-1.98.1-").expect("prefix");
    fs::write(seed.join("mbx/bundle/marker"), "objects").expect("marker");
    let script = import_script(seed.to_str().expect("utf8")).expect("script");
    let (text, calls) = run(&script, &root, "", "tool-1.98.1-", "0");
    let private = root.join("rt/mbx-seed-bundle");
    assert!(
        calls.contains(&format!("cache import {}", private.display())),
        "{calls}"
    );
    assert!(
        !calls.contains(&format!("cache import {}/mbx/bundle", seed.display())),
        "{calls}"
    );
    assert!(!text.contains("no mbx bundle matched"), "{text}");
    assert!(!private.exists(), "private copy is removed");
    assert_eq!(
        fs::read_to_string(seed.join("mbx/bundle/marker")).expect("kept"),
        "objects"
    );
    let (text, calls) = run(&script, &root, "", "tool-1.98.1-", "0");
    let needle = format!("cache import {}", private.display());
    assert_eq!(calls.matches(&needle).count(), 2, "{calls}");
    assert!(!text.contains("no mbx bundle matched"), "{text}");
    assert_eq!(
        fs::read_to_string(seed.join("mbx/bundle/marker")).expect("second hit"),
        "objects"
    );
    fs::remove_dir_all(&root).ok();
}

#[test]
fn wrong_prefix_absent_seed_and_failed_import_leave_the_seed() {
    let root = scratch("miss");
    let seed = root.join("seed");
    fs::create_dir_all(&seed).expect("seed root");
    fs::write(seed.join("PROVENANCE"), "velnor-host-seed-v1").expect("provenance");
    fs::create_dir_all(seed.join("mbx/bundle")).expect("bundle");
    fs::write(seed.join("mbx/PREFIX"), "other-").expect("prefix");
    fs::write(seed.join("mbx/bundle/marker"), "objects").expect("marker");
    let script = import_script(seed.to_str().expect("utf8")).expect("script");
    let (text, calls) = run(&script, &root, "", "tool-1.98.1-", "0");
    assert!(text.contains("no mbx bundle matched"), "{text}");
    assert!(calls.is_empty(), "{calls}");
    let absent = import_script(root.join("none").to_str().expect("utf8")).expect("script");
    let (text, calls) = run(&absent, &root, "", "tool-1.98.1-", "0");
    assert!(text.contains("no mbx bundle matched"), "{text}");
    assert!(calls.is_empty(), "{calls}");
    let (text, _calls) = run(&script, &root, "", "other-", "1");
    assert!(
        text.contains("mbx bundle import failed; continuing cold"),
        "{text}"
    );
    assert!(
        !root.join("rt/mbx-seed-bundle").exists(),
        "failed import removes only the private copy"
    );
    assert_eq!(
        fs::read_to_string(seed.join("mbx/bundle/marker")).expect("kept"),
        "objects"
    );
    let bundle = root.join("rt/mbx-single-bundle");
    fs::create_dir_all(&bundle).expect("cache bundle");
    fs::write(bundle.join("marker"), "cache").expect("cache marker");
    let (text, _) = run(&script, &root, "exact-key", "other-", "1");
    assert!(
        text.contains("mbx bundle import failed; continuing cold"),
        "{text}"
    );
    assert!(!bundle.exists(), "cache bundle is removed");
    assert!(seed.join("mbx/bundle/marker").exists(), "seed stays");
    fs::remove_dir_all(&root).ok();
}

#[test]
fn mbx_seed_requires_root_owned_read_only_provisioned_tree() {
    let root = scratch("untrusted");
    let seed = root.join("seed");
    fs::create_dir_all(seed.join("mbx/bundle")).expect("bundle");
    fs::write(seed.join("PROVENANCE"), "velnor-host-seed-v1").expect("provenance");
    fs::write(seed.join("mbx/PREFIX"), "tool-1.98.1-").expect("prefix");
    fs::write(seed.join("mbx/bundle/marker"), "objects").expect("marker");
    let script = import_script(seed.to_str().expect("utf8")).expect("script");

    for (owner, mount, unsafe_tree) in [
        ("1000", "ro,relatime", "0"),
        ("0", "rw,relatime", "0"),
        ("0", "ro,relatime", "1"),
    ] {
        let (text, calls) = run_with_seed_state(
            &script,
            &root,
            "",
            "tool-1.98.1-",
            "0",
            owner,
            mount,
            unsafe_tree,
        );
        assert!(text.contains("MBX seed untrusted; continuing cold"), "{text}");
        assert!(calls.is_empty(), "{calls}");
    }
    assert_eq!(
        fs::read_to_string(seed.join("mbx/bundle/marker")).expect("seed remains"),
        "objects"
    );
    fs::remove_dir_all(&root).ok();
}
