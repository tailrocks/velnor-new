//! Run the generated bundle scripts against stub `mise`, `mbx`, `du` and `sha256sum`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use velnor_actions_contract::workflow::cache_family::CacheFamily;

use super::super::append_bundle_postludes;
use super::super::bundle_helper_script;
use super::{MBX, RUST, job, run_of, steps};
use crate::cache_steps::MbxTransport;

/// Run an export or import script against stub `mise`, `mbx` and `du`.
struct Sandbox {
    root: std::path::PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("velnor-bundle-{tag}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(root.join("bin")).expect("bin");
        std::fs::create_dir_all(root.join("runner/velnor")).expect("runner");
        std::fs::create_dir_all(root.join("ws")).expect("workspace");
        let helper = root.join("ws/.github/scripts/velnor-mbx-bundle.sh");
        std::fs::create_dir_all(
            helper
                .parent()
                .expect("The helper path has no parent directory."),
        )
        .expect("The test could not create the helper directory.");
        std::fs::write(&helper, bundle_helper_script())
            .expect("The test could not write the bundle helper.");
        write_exec(
            &root.join("bin/mise"),
            // `mise ... exec rust@X -- <program> args` runs the program.
            "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n",
        );
        write_exec(
            &root.join("bin/mbx"),
            "#!/bin/sh\nif [ \"$1 $2\" = 'cache export' ]; then\n  dir=\"\"; for a in \"$@\"; do dir=\"$a\"; done\n  [ \"${STUB_EXPORT_FAIL:-}\" != 1 ] || exit 1\n  mkdir -p \"$dir\" && printf x > \"$dir/blob\"\nelif [ \"$1 $2\" = 'cache import' ]; then\n  [ \"${STUB_IMPORT_FAIL:-}\" != 1 ] || exit 1\n  printf imported > \"$STUB_MARK\"\nfi\n",
        );
        write_exec(
            &root.join("bin/du"),
            "#!/bin/sh\nprintf '%s\\t%s\\n' \"${STUB_KIB:-12}\" \"$2\"\n",
        );
        Self { root }
    }

    fn run(&self, script: &str, extra: &[(&str, &str)]) -> (String, String) {
        let output_file = self.root.join("github-output");
        drop(std::fs::remove_file(&output_file));
        std::fs::write(&output_file, "").expect("output file");
        let path = format!(
            "{}:{}",
            self.root.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new("sh");
        command
            .args(["-c", script])
            .env("PATH", path)
            .env("RUNNER_TEMP", self.root.join("runner"))
            .env("GITHUB_WORKSPACE", self.root.join("ws"))
            .env("GITHUB_OUTPUT", &output_file)
            .env("MBX_CACHE_EXPORT_GROUP", "velnor")
            .env("STUB_MARK", self.root.join("imported"))
            .env("VELNOR_MBX_BUNDLE_ROOT", ".")
            .env("VELNOR_MBX_BUNDLE_RUST_TOOLCHAIN", RUST)
            .env("VELNOR_MBX_BUNDLE_UNIT", "crate")
            .env("VELNOR_MBX_BUNDLE_VERSION", MBX)
            .env("VELNOR_MBX_BUNDLE_TOOLCHAIN", RUST);
        for (key, value) in extra {
            command.env(key, value);
        }
        let output = command.output().expect("script runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        (
            String::from_utf8_lossy(&output.stdout).into_owned(),
            std::fs::read_to_string(&output_file).expect("outputs"),
        )
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.root));
    }
}

fn write_exec(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).expect("write stub");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

fn export_script() -> String {
    let steps = steps(MbxTransport::CrateBundle, None);
    let mut jobs = BTreeMap::from([("rust-demo".to_owned(), job(steps))]);
    append_bundle_postludes(&mut jobs).expect("postlude");
    let steps = &jobs["rust-demo"].steps;
    run_of(&steps[steps.len() - 2]).to_owned()
}

#[test]
fn export_script_refuses_without_the_producer_marker() {
    let sandbox = Sandbox::new("nomark");
    let (stdout, outputs) = sandbox.run(&export_script(), &[]);
    assert!(stdout.contains("producer_not_succeeded"), "{stdout}");
    assert_eq!(outputs.trim(), "exported=false");
}

#[test]
fn export_script_exports_after_the_marker_and_reports_the_flag() {
    let sandbox = Sandbox::new("marked");
    std::fs::write(sandbox.root.join("runner/velnor/mbx-producer-ok"), "").expect("marker");
    let (stdout, outputs) = sandbox.run(&export_script(), &[]);
    assert!(stdout.contains("mbx bundle exported"), "{stdout}");
    assert_eq!(outputs.trim(), "exported=true");
    assert!(sandbox.root.join("runner/velnor/mbx-bundle/blob").is_file());
}

#[test]
fn export_script_refuses_an_over_ceiling_bundle_and_removes_it() {
    let sandbox = Sandbox::new("big");
    std::fs::write(sandbox.root.join("runner/velnor/mbx-producer-ok"), "").expect("marker");
    let over = (CacheFamily::MbxBundle.spec().ceiling_bytes / 1024 + 1).to_string();
    let (stdout, outputs) = sandbox.run(&export_script(), &[("STUB_KIB", &over)]);
    assert!(stdout.contains("over_ceiling"), "{stdout}");
    assert_eq!(outputs.trim(), "exported=false");
    assert!(!sandbox.root.join("runner/velnor/mbx-bundle").exists());
}

#[test]
fn export_script_reports_a_failed_export_without_failing_the_job() {
    let sandbox = Sandbox::new("fail");
    std::fs::write(sandbox.root.join("runner/velnor/mbx-producer-ok"), "").expect("marker");
    let (stdout, outputs) = sandbox.run(&export_script(), &[("STUB_EXPORT_FAIL", "1")]);
    assert!(stdout.contains("export_failed"), "{stdout}");
    assert_eq!(outputs.trim(), "exported=false");
}

fn import_script() -> String {
    run_of(&steps(MbxTransport::CrateBundle, None)[5]).to_owned()
}

#[test]
fn import_script_consumes_the_restored_bundle_and_discards_a_corrupt_one() {
    let sandbox = Sandbox::new("import");
    let (stdout, _) = sandbox.run(&import_script(), &[]);
    assert!(stdout.contains("no_entry"), "{stdout}");

    let dir = sandbox.root.join("runner/velnor/mbx-bundle");
    std::fs::create_dir_all(&dir).expect("bundle dir");
    let (stdout, _) = sandbox.run(&import_script(), &[]);
    assert!(stdout.contains("bundle imported"), "{stdout}");
    assert!(sandbox.root.join("imported").is_file());
    assert!(!dir.exists(), "the bundle directory is consumed");

    std::fs::create_dir_all(&dir).expect("bundle dir");
    let (stdout, _) = sandbox.run(&import_script(), &[("STUB_IMPORT_FAIL", "1")]);
    assert!(stdout.contains("cache_corrupt"), "{stdout}");
    assert!(!dir.exists(), "a corrupt bundle is discarded");
}

#[test]
fn key_script_grants_a_save_only_to_a_trusted_push() {
    let key_script = run_of(&steps(MbxTransport::CrateBundle, None)[3]).to_owned();
    let cases = [
        (
            [
                ("GITHUB_EVENT_NAME", "push"),
                ("GITHUB_REF", "refs/heads/main"),
                ("GITHUB_REF_PROTECTED", "true"),
            ],
            "save=true",
        ),
        (
            [
                ("GITHUB_EVENT_NAME", "pull_request"),
                ("GITHUB_REF", "refs/pull/1/merge"),
                ("GITHUB_REF_PROTECTED", "false"),
            ],
            "save=false",
        ),
        (
            [
                ("GITHUB_EVENT_NAME", "push"),
                ("GITHUB_REF", "refs/heads/main"),
                ("GITHUB_REF_PROTECTED", "false"),
            ],
            "save=false",
        ),
    ];
    for (index, (event, expected)) in cases.iter().enumerate() {
        let sandbox = Sandbox::new(&format!("key{index}"));
        std::fs::write(sandbox.root.join("ws/Cargo.lock"), "lock").expect("lock");
        write_exec(
            &sandbox.root.join("bin/sha256sum"),
            "#!/bin/sh\nprintf '%064d  %s\\n' 7 \"$1\"\n",
        );
        let mut extra = vec![
            ("VELNOR_DEFAULT_REF", "refs/heads/main"),
            ("VELNOR_PR_HEAD_FORK", "false"),
            ("VELNOR_MBX_BUNDLE_VERSION", MBX),
            ("VELNOR_MBX_BUNDLE_TOOLCHAIN", RUST),
            ("VELNOR_MBX_BUNDLE_ROOT", "."),
        ];
        extra.extend_from_slice(event);
        let (_, outputs) = sandbox.run(&key_script, &extra);
        assert!(outputs.contains(expected), "{index}: {outputs}");
        assert!(
            outputs.contains("key=velnor-v1-mbx-bundle-trusted-s256-"),
            "{outputs}"
        );
        assert!(
            outputs.contains("restore_keys=velnor-v1-mbx-bundle-trusted-s256-"),
            "{outputs}"
        );
    }
}
