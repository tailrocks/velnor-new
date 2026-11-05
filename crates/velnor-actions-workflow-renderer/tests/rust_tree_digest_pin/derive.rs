// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;
use velnor_actions_workflow_renderer::verifier_builders::{ToolchainPhase, TreeDigestPin};

use crate::fixture::{
    CHANNEL_MANIFEST, CHANNEL_URL, Fixture, INSTALLER_VERSION, MANIFEST_CARGO, MANIFEST_RUSTC,
    MISMATCH, MULTIRUST_CONFIG, Outcome, TARBALL_URL, TARGET, VERSION, script_with, sha256_hex,
};
use crate::release::{COMPONENT, channel_manifest, rustup_bookkeeping};

type Tweak<'a> = &'a dyn Fn(&Path);

const STD_LIB: &str = "lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd.rlib";
const SELF_CONTAINED: &str = "lib/rustlib/x86_64-unknown-linux-gnu/lib/self-contained";
const MANIFEST_IN: &str = "manifest.in";
const TOOL: &str = "scripts/derive-rust-tree-digest";
pub(crate) const TIMEOUT_SECONDS: &str = "60";
pub(crate) const TIMED_OUT: i32 = 124;
const SERVE: &str = "get() {\n  printf '%s\\n' \"$2\" >> REQUESTS_FILE\n  case \"$2\" in\n    CHANNEL_URL) cp CHANNEL_FILE \"$1\" ;;\n    TARBALL_URL) cp TARBALL_FILE \"$1\" ;;\n    *) exit 22 ;;\n  esac\n}\n";

fn rustup_tree() -> Fixture {
    let fx = Fixture::with_components(&format!("{COMPONENT}\n"));
    fx.remove("etc");
    fx.remove(MANIFEST_RUSTC);
    fx.dir("lib/rustlib/x86_64-unknown-linux-gnu/lib", 0o755);
    fx.put(STD_LIB, b"rlib\n", 0o644);
    fx.put(MULTIRUST_CONFIG, b"", 0o644);
    fx
}

pub(crate) fn list_files(dir: &Path, rel: &str, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let name = format!("{rel}{}", entry.file_name().to_string_lossy());
        if entry.file_type().expect("file type").is_dir() {
            list_files(&entry.path(), &format!("{name}/"), out);
        } else {
            out.push(name);
        }
    }
}

fn pack_sub(fx: &Fixture, sub: &Path, tweak: Tweak<'_>) {
    fs::create_dir_all(sub).expect("stage");
    let copied = Command::new("cp")
        .arg("-R")
        .arg(format!("{}/.", fx.root().display()))
        .arg(sub)
        .status()
        .expect("copy tree");
    assert!(copied.success());
    for bookkeeping in [
        "lib/rustlib/components",
        INSTALLER_VERSION,
        CHANNEL_MANIFEST,
        MULTIRUST_CONFIG,
        MANIFEST_CARGO,
    ] {
        fs::remove_file(sub.join(bookkeeping)).expect("drop bookkeeping");
    }
    fs::create_dir(sub.join(SELF_CONTAINED)).expect("empty dir");
    let mut files = Vec::new();
    list_files(sub, "", &mut files);
    files.sort();
    let lines = format!("file:{}\n", files.join("\nfile:"));
    fs::write(sub.join(MANIFEST_IN), lines).expect("manifest.in");
    tweak(sub);
}

#[derive(Debug)]
pub(crate) struct Bundle {
    work: TempDir,
    installed: Fixture,
    channel: String,
}

impl Bundle {
    pub(crate) fn new(tweak: Tweak<'_>) -> Self {
        let work = tempfile::tempdir().expect("work dir");
        let installed = rustup_tree();
        let stage = work.path().join("stage/pkg");
        fs::create_dir_all(&stage).expect("stage dir");
        fs::write(stage.join("components"), "sub\n").expect("components");
        pack_sub(&installed, &stage.join("sub"), tweak);
        installed.put(
            MANIFEST_CARGO,
            &fs::read(stage.join("sub").join(MANIFEST_IN)).expect("read manifest.in"),
            0o644,
        );
        let tarball = work.path().join("cargo.tar.xz");
        let packed = Command::new("tar")
            .arg("-cJf")
            .arg(&tarball)
            .arg("-C")
            .arg(work.path().join("stage"))
            .arg("pkg")
            .status()
            .expect("pack tarball");
        assert!(packed.success());
        let tarball_sha256 = sha256_hex(&fs::read(&tarball).expect("read tarball"));
        let channel = channel_manifest(&tarball_sha256);
        fs::write(work.path().join("channel.toml"), &channel).expect("channel");
        installed.put(CHANNEL_MANIFEST, channel.as_bytes(), 0o644);
        installed.serve_channel(channel.as_bytes());
        let bundle = Self {
            work,
            installed,
            channel,
        };
        bundle.install_tool(&sha256_hex(bundle.channel.as_bytes()));
        bundle
    }

    fn script(&self, phase: ToolchainPhase, pin: TreeDigestPin) -> String {
        script_with(self.channel.as_bytes(), phase, &[pin])
    }

    fn pin(derived: &Outcome) -> TreeDigestPin {
        TreeDigestPin::new(
            VERSION,
            TARGET,
            printed(derived, "components_sha256"),
            printed(derived, "tree_sha256"),
        )
    }

    fn install_tool(&self, channel_sha256: &str) {
        let root = self.work.path();
        let catalog = root.join("crates/velnor-actions-contract/src/config");
        fs::create_dir_all(&catalog).expect("catalog dir");
        fs::write(
            catalog.join("rust_channel_manifests.rs"),
            format!("(\n\"{VERSION}\",\n\"{channel_sha256}\",\n\"2026-10-15\",\n),\n"),
        )
        .expect("catalog");
        fs::create_dir_all(root.join("scripts")).expect("scripts dir");
        let tool = fs::read_to_string(crate::pinned::derive_tool()).expect("read script");
        let start = tool.find("get() {").expect("get definition");
        let end = start + tool[start..].find("\n}\n").expect("get end") + 3;
        let serve = SERVE
            .replace(
                "REQUESTS_FILE",
                &root.join("requests").display().to_string(),
            )
            .replace("CHANNEL_URL", CHANNEL_URL)
            .replace(
                "CHANNEL_FILE",
                &root.join("channel.toml").display().to_string(),
            )
            .replace("TARBALL_URL", TARBALL_URL)
            .replace(
                "TARBALL_FILE",
                &root.join("cargo.tar.xz").display().to_string(),
            );
        let tool_path = root.join(TOOL);
        fs::write(
            &tool_path,
            format!("{}{serve}{}", &tool[..start], &tool[end..]),
        )
        .expect("write script");
        fs::set_permissions(&tool_path, fs::Permissions::from_mode(0o755)).expect("chmod script");
    }

    fn derive(&self) -> Outcome {
        self.derive_with_env(&[])
    }

    pub(crate) fn derive_with_args(&self, version: &str, target: &str) -> Outcome {
        self.run_derive(&["--from-manifest", version, target, COMPONENT], &[])
    }

    pub(crate) fn derive_with_env(&self, extra: &[(&str, &str)]) -> Outcome {
        self.run_derive(&["--from-manifest", VERSION, TARGET, COMPONENT], extra)
    }

    pub(crate) fn run_derive(&self, args: &[&str], extra: &[(&str, &str)]) -> Outcome {
        self.run_at(&self.work.path().join(TOOL), args, extra)
    }

    pub(crate) fn run_relative(&self, args: &[&str], extra: &[(&str, &str)]) -> Outcome {
        self.run_at(Path::new(TOOL), args, extra)
    }

    fn run_at(&self, program: &Path, args: &[&str], extra: &[(&str, &str)]) -> Outcome {
        let root = self.work.path();
        let output = timed(program)
            .args(args)
            .current_dir(root)
            .env_clear()
            .envs(extra.iter().copied())
            .env("PATH", "/usr/bin:/bin")
            .output()
            .expect("run derive tool");
        Outcome {
            requests: fs::read_to_string(root.join("requests"))
                .map(|log| log.lines().map(str::to_owned).collect())
                .unwrap_or_default(),
            ..outcome(&output)
        }
    }
}

pub(crate) fn timed(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new("/usr/bin/timeout");
    command.arg(TIMEOUT_SECONDS).arg(program);
    command
}

pub(crate) fn outcome(output: &Output) -> Outcome {
    Outcome {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        requests: Vec::new(),
    }
}

pub(crate) fn printed(outcome: &Outcome, key: &str) -> &'static str {
    let value = outcome
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("no {key} in {outcome:?}"));
    Box::leak(value.to_owned().into_boxed_str())
}

fn append(path: &Path, text: &str) {
    let mut current = fs::read_to_string(path).expect("read");
    current.push_str(text);
    fs::write(path, current).expect("write");
}

#[test]
fn p4_derived_digests_are_accepted_by_the_verifier_in_both_phases() {
    let bundle = Bundle::new(&|_| {});
    let derived = bundle.derive();
    assert_eq!(derived.code, Some(0), "{derived:?}");
    let pin = Bundle::pin(&derived);
    bundle
        .installed
        .run(&bundle.script(ToolchainPhase::Restored, pin))
        .assert_offline_pinned();
    bundle.installed.put(
        CHANNEL_MANIFEST,
        rustup_bookkeeping(&"5".repeat(64)).as_bytes(),
        0o644,
    );
    bundle
        .installed
        .run(&bundle.script(ToolchainPhase::Installed, pin))
        .assert_offline_pinned();
}

#[test]
fn d2_derive_prints_the_listing_and_ignores_the_caller_environment() {
    let bundle = Bundle::new(&|_| {});
    let clean = bundle.derive();
    assert_eq!(clean.code, Some(0), "{clean:?}");
    let manifest_line = format!(
        "listing f 0644 {} lib/rustlib/multirust-channel-manifest.toml\n",
        sha256_hex(bundle.channel.as_bytes())
    );
    assert!(clean.stdout.contains(&manifest_line), "{clean:?}");
    assert!(clean.stdout.contains("TreeDigestPin::new("), "{clean:?}");
    for env in [
        ("QUOTING_STYLE", "c"),
        ("BLOCK_SIZE", "1M"),
        ("LC_ALL", "en_US.UTF-8"),
        ("XZ_DEFAULTS", "--memlimit=1B"),
        ("TAR_OPTIONS", "--exclude=*"),
        ("POSIXLY_CORRECT", "1"),
    ] {
        let poisoned = bundle.derive_with_env(&[env]);
        assert_eq!(poisoned.stdout, clean.stdout, "{env:?}: {poisoned:?}");
    }
    let fx = Fixture::standard();
    let output = timed(crate::pinned::derive_tool())
        .arg("--tree")
        .arg(fx.root())
        .env("QUOTING_STYLE", "c")
        .output()
        .expect("run derive tool");
    let tree = outcome(&output);
    assert!(
        tree.stdout.contains(&format!(
            "listing f 0644 {} lib/rustlib/multirust-channel-manifest.toml\n",
            sha256_hex(crate::fixture::FIXTURE_CHANNEL.as_bytes())
        )),
        "{tree:?}"
    );
}

#[test]
fn n8_a_manifest_that_is_not_exactly_the_tarball_files_fails_closed() {
    let control = Bundle::new(&|_| {}).derive();
    assert_eq!(control.code, Some(0), "{control:?}");
    let dir_entry = |sub: &Path| append(&sub.join(MANIFEST_IN), &format!("dir:{SELF_CONTAINED}\n"));
    let missing_file = |sub: &Path| append(&sub.join(MANIFEST_IN), "file:bin/ghost\n");
    let unlisted_file = |sub: &Path| fs::write(sub.join("bin/extra"), b"extra\n").expect("extra");
    let collision = |sub: &Path| {
        fs::write(sub.join(INSTALLER_VERSION), b"3").expect("installer version");
        append(
            &sub.join(MANIFEST_IN),
            &format!("file:{INSTALLER_VERSION}\n"),
        );
    };
    let two_subs = |sub: &Path| {
        let other = sub.with_file_name("sub2");
        fs::create_dir_all(other.join("share")).expect("second sub");
        fs::write(other.join("share/second"), b"second\n").expect("second file");
        fs::write(other.join(MANIFEST_IN), "file:share/second\n").expect("second manifest.in");
        fs::write(sub.with_file_name("components"), "sub\nsub2\n").expect("components");
    };
    let rows: [(&str, Tweak<'_>); 5] = [
        ("dir entry", &dir_entry),
        ("listed file missing", &missing_file),
        ("unlisted file present", &unlisted_file),
        ("tarball ships the installer version", &collision),
        ("two sub-components", &two_subs),
    ];
    let failures = rows
        .into_iter()
        .filter_map(|(name, tweak)| {
            let outcome = Bundle::new(tweak).derive();
            let failed_closed = outcome.code.is_some_and(|code| code != 0)
                && !outcome.stdout.contains("tree_sha256=")
                && !outcome.stdout.contains("TreeDigestPin::new")
                && outcome.stderr.starts_with("derive-rust-tree-digest:");
            (!failed_closed).then(|| format!("{name}: {outcome:?}"))
        })
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn p5_the_derived_pin_binds_the_bookkeeping_bytes_it_reproduces() {
    let bundle = Bundle::new(&|_| {});
    let derived = bundle.derive();
    assert_eq!(derived.code, Some(0), "{derived:?}");
    let pin = Bundle::pin(&derived);
    let run = || {
        bundle
            .installed
            .run(&bundle.script(ToolchainPhase::Restored, pin))
    };
    run().assert_offline_pinned();
    for path in [MANIFEST_CARGO, INSTALLER_VERSION, CHANNEL_MANIFEST] {
        let original = bundle.installed.read(path);
        let mut changed = original.clone();
        changed[0] ^= 1;
        bundle.installed.put(path, &changed, 0o644);
        run().assert_hard_failed(MISMATCH);
        bundle.installed.put(path, &original, 0o644);
        run().assert_offline_pinned();
    }
}
