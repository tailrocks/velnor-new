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
}

fn marker(seed: &Path) {
    fs::write(seed.join("PROVENANCE"), "velnor-host-seed-v1\n").expect("provenance");
}

fn run(
    script: &str,
    root: &Path,
    seed: &Path,
    matched: &str,
    prefix: &str,
    exit: &str,
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
    let script = crate::tool_seed_test_support::mock_trust_commands(script, root);
    let mounts = format!("{} ext4 0:77 ro,nosuid", seed.display());
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("HOME", &home)
        .env("RUNNER_TEMP", &runner)
        .env("MATCHED", matched)
        .env("PREFIX", prefix)
        .env("MBX_LOG", &log)
        .env("MBX_EXIT", exit)
        .env("RUNNER_OS", "Linux")
        .env("SEED_TEST_MOUNTS", mounts)
        .env("SEED_TEST_SKIP_OWNER_SCAN", "1")
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
    assert!(
        run[2].contains("seed_root=\"/opt/velnor/seed\""),
        "{}",
        run[2]
    );
    assert!(run[2].contains("seed=\"$seed_root/mbx\""), "{}", run[2]);
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
    fs::create_dir_all(seed.join("mbx/bundle")).expect("bundle");
    marker(&seed);
    fs::write(seed.join("mbx/PREFIX"), "tool-1.98.1-").expect("prefix");
    fs::write(seed.join("mbx/bundle/marker"), "objects").expect("marker");
    let script = import_script(seed.to_str().expect("utf8")).expect("script");
    let (text, calls) = run(&script, &root, &seed, "", "tool-1.98.1-", "0");
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
    let (text, calls) = run(&script, &root, &seed, "", "tool-1.98.1-", "0");
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
    fs::create_dir_all(seed.join("mbx/bundle")).expect("bundle");
    marker(&seed);
    fs::write(seed.join("mbx/PREFIX"), "other-").expect("prefix");
    fs::write(seed.join("mbx/bundle/marker"), "objects").expect("marker");
    let script = import_script(seed.to_str().expect("utf8")).expect("script");
    let (text, calls) = run(&script, &root, &seed, "", "tool-1.98.1-", "0");
    assert!(text.contains("no mbx bundle matched"), "{text}");
    assert!(calls.is_empty(), "{calls}");
    fs::write(seed.join("mbx/PREFIX"), "tool-1.98.1-\nextra\n").expect("multiline prefix");
    let (text, calls) = run(&script, &root, &seed, "", "tool-1.98.1-", "0");
    assert!(text.contains("no mbx bundle matched"), "{text}");
    assert!(
        calls.is_empty(),
        "multiline prefix imported the seed: {calls}"
    );
    assert!(!root.join("rt/mbx-seed-bundle").exists());
    let absent = import_script(root.join("none").to_str().expect("utf8")).expect("script");
    let absent_seed = root.join("none");
    let (text, calls) = run(&absent, &root, &absent_seed, "", "tool-1.98.1-", "0");
    assert!(text.contains("no mbx bundle matched"), "{text}");
    assert!(calls.is_empty(), "{calls}");
    fs::write(seed.join("mbx/PREFIX"), "tool-1.98.1-").expect("restore prefix");
    let (text, _calls) = run(&script, &root, &seed, "", "tool-1.98.1-", "1");
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
    let (text, _) = run(&script, &root, &seed, "exact-key", "other-", "1");
    assert!(
        text.contains("mbx bundle import failed; continuing cold"),
        "{text}"
    );
    assert!(!bundle.exists(), "cache bundle is removed");
    assert!(seed.join("mbx/bundle/marker").exists(), "seed stays");
    fs::remove_dir_all(&root).ok();
}
