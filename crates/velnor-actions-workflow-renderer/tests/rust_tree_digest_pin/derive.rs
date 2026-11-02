// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;
use velnor_actions_workflow_renderer::verifier_builders::{ToolchainPhase, TreeDigestPin};

use crate::fixture::{
    CHANNEL_MANIFEST, Fixture, INSTALLER_VERSION, MANIFEST_CARGO, MANIFEST_RUSTC, MISMATCH,
    MULTIRUST_CONFIG, Outcome, TARGET, VERSION, script,
};
use crate::release::{
    CHANNEL_URL, COMPONENT, TARBALL_URL, channel_manifest, rustup_bookkeeping, sha256_hex,
};

type Tweak<'a> = &'a dyn Fn(&Path);

const STD_LIB: &str = "lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd.rlib";
const SELF_CONTAINED: &str = "lib/rustlib/x86_64-unknown-linux-gnu/lib/self-contained";
const MANIFEST_IN: &str = "manifest.in";
const SHIM: &str = "#!/bin/sh\nwhile [ \"$#\" -gt 1 ]; do\n  if [ \"$1\" = --output ]; then out=$2; fi\n  shift\ndone\ncase \"$1\" in\n  \"$VELNOR_TEST_CHANNEL_URL\") cp \"$VELNOR_TEST_CHANNEL\" \"$out\" ;;\n  \"$VELNOR_TEST_TARBALL_URL\") cp \"$VELNOR_TEST_TARBALL\" \"$out\" ;;\n  *) exit 22 ;;\nesac\n";

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
struct Bundle {
    work: TempDir,
    installed: Fixture,
}

impl Bundle {
    fn new(tweak: Tweak<'_>) -> Self {
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
        installed.put(
            CHANNEL_MANIFEST,
            rustup_bookkeeping(&tarball_sha256).as_bytes(),
            0o644,
        );
        let bundle = Self { work, installed };
        bundle.install_tool(&sha256_hex(channel.as_bytes()));
        bundle
    }

    fn install_tool(&self, channel_sha256: &str) {
        let root = self.work.path();
        let catalog = root.join("crates/velnor-actions-mise/src");
        fs::create_dir_all(&catalog).expect("catalog dir");
        fs::write(
            catalog.join("catalog_rust_manifests.rs"),
            format!("(\n\"{VERSION}\",\n\"{channel_sha256}\",\n\"2026-10-15\",\n),\n"),
        )
        .expect("catalog");
        fs::create_dir_all(root.join("scripts")).expect("scripts dir");
        fs::copy(
            crate::derive_tool(),
            root.join("scripts/derive-rust-tree-digest"),
        )
        .expect("copy script");
        let renderer = root.join("crates/velnor-actions-workflow-renderer/src");
        fs::create_dir_all(&renderer).expect("renderer dir");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/native_verifiers_tree_digest.rs"),
            renderer.join("native_verifiers_tree_digest.rs"),
        )
        .expect("copy normal form source");
        fs::create_dir_all(root.join("shim")).expect("shim dir");
        let curl = root.join("shim/curl");
        fs::write(&curl, SHIM).expect("curl shim");
        fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).expect("chmod shim");
    }

    fn derive(&self) -> Outcome {
        let root = self.work.path();
        let output = Command::new(root.join("scripts/derive-rust-tree-digest"))
            .args(["--from-manifest", VERSION, TARGET, COMPONENT])
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", root.join("shim").display()),
            )
            .env("TMPDIR", root)
            .env("VELNOR_TEST_CHANNEL_URL", CHANNEL_URL)
            .env("VELNOR_TEST_CHANNEL", root.join("channel.toml"))
            .env("VELNOR_TEST_TARBALL_URL", TARBALL_URL)
            .env("VELNOR_TEST_TARBALL", root.join("cargo.tar.xz"))
            .output()
            .expect("run derive tool");
        outcome(&output)
    }
}

pub(crate) fn outcome(output: &Output) -> Outcome {
    Outcome {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
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
fn p4_derived_digests_are_accepted_by_the_verifier_over_an_undeclared_empty_dir() {
    let bundle = Bundle::new(&|_| {});
    let derived = bundle.derive();
    assert_eq!(derived.code, Some(0), "{derived:?}");
    let pin = TreeDigestPin::new(
        VERSION,
        TARGET,
        printed(&derived, "components_sha256"),
        printed(&derived, "tree_sha256"),
    );
    bundle
        .installed
        .run(&script(ToolchainPhase::Installed, &[pin]))
        .assert_pinned();
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
    let pin = TreeDigestPin::new(
        VERSION,
        TARGET,
        printed(&derived, "components_sha256"),
        printed(&derived, "tree_sha256"),
    );
    let run = || {
        bundle
            .installed
            .run(&script(ToolchainPhase::Installed, &[pin]))
    };
    run().assert_pinned();
    for path in [MANIFEST_CARGO, INSTALLER_VERSION] {
        let original = fs::read(bundle.installed.root().join(path)).expect("read bookkeeping");
        let mut changed = original.clone();
        changed[0] ^= 1;
        bundle.installed.put(path, &changed, 0o644);
        run().assert_hard_failed(MISMATCH);
        bundle.installed.put(path, &original, 0o644);
        run().assert_pinned();
    }
}
