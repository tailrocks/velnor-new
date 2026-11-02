// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use tempfile::TempDir;
use velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS;
use velnor_actions_workflow_renderer::verifier_builders::{
    ToolchainPhase, TreeDigestPin, toolchain_script,
};

pub(crate) const VERSION: &str = "1.99.1";
pub(crate) const TARGET: &str = "x86_64-unknown-linux-gnu";

pub(crate) const FIXTURE_COMPONENTS: &str =
    "cargo-x86_64-unknown-linux-gnu\nrustc-x86_64-unknown-linux-gnu\n";
pub(crate) const FIXTURE_COMPONENTS_SHA256: &str =
    "adfc9d0f163c22aaf13170f015d8fc332bf8254fe1c1541fc1d0497e730621a2";
pub(crate) const FIXTURE_TREE_SHA256: &str =
    "5c7befbb525f104b00a35be16a59dfd960e07945c61654338943543eca45e5ac";

pub(crate) const MANIFEST_CARGO: &str = "lib/rustlib/manifest-cargo-x86_64-unknown-linux-gnu";
pub(crate) const MANIFEST_RUSTC: &str = "lib/rustlib/manifest-rustc-x86_64-unknown-linux-gnu";
pub(crate) const INSTALLER_VERSION: &str = "lib/rustlib/rust-installer-version";
pub(crate) const MULTIRUST_CONFIG: &str = "lib/rustlib/multirust-config.toml";
pub(crate) const CHANNEL_MANIFEST: &str = "lib/rustlib/multirust-channel-manifest.toml";
pub(crate) const MISMATCH: &str = "rust_tree_digest_mismatch";
pub(crate) const CAP: usize = 4_194_304;

pub(crate) const FIXTURE_BOOKKEEPING: &str = "manifest-version = \"2\"\ndate = \"2026-10-15\"\n\n[pkg.cargo.target.x86_64-unknown-linux-gnu]\navailable = true\nurl = \"https://static.rust-lang.org/dist/2026-10-15/cargo-1.99.1-x86_64-unknown-linux-gnu.tar.gz\"\nhash = \"1111111111111111111111111111111111111111111111111111111111111111\"\nxz_url = \"https://static.rust-lang.org/dist/2026-10-15/cargo-1.99.1-x86_64-unknown-linux-gnu.tar.xz\"\nxz_hash = \"2222222222222222222222222222222222222222222222222222222222222222\"\ncomponents = []\nextensions = []\n";

pub(crate) const PHASES: [ToolchainPhase; 2] =
    [ToolchainPhase::Restored, ToolchainPhase::Installed];

pub(crate) const HELPERS: [&str; 5] = ["sha256sum", "shasum", "find", "sort", "curl"];

const PROXY: &str = "http://127.0.0.1:9";

#[derive(Debug)]
pub(crate) struct Outcome {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl Outcome {
    pub(crate) fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }

    pub(crate) fn is_pinned(&self) -> bool {
        self.code == Some(0)
            && self.stdout.contains("matches pinned")
            && !self.stderr.contains("curl: (")
    }

    pub(crate) fn is_not_pinned(&self) -> bool {
        self.code != Some(0) && !self.combined().contains("matches pinned")
    }

    pub(crate) fn fell_through(&self) -> bool {
        self.is_not_pinned()
            && self.combined().contains("pin_mismatch")
            && self.stderr.contains("curl: (7)")
    }

    pub(crate) fn hard_failed(&self, code: &str) -> bool {
        let combined = self.combined();
        self.code.is_some_and(|status| status != 0)
            && self.stderr.contains(code)
            && !combined.contains("matches pinned")
            && !combined.contains("verified file by file")
            && !combined.contains("curl: (")
    }

    pub(crate) fn no_pin(&self) -> bool {
        self.fell_through()
            && self
                .stdout
                .contains("::notice::pin_mismatch: no_pin components=")
    }

    pub(crate) fn assert_pinned(&self) {
        assert!(self.is_pinned(), "{self:?}");
    }

    pub(crate) fn assert_fell_through(&self) {
        assert!(self.fell_through(), "{self:?}");
    }

    pub(crate) fn assert_hard_failed(&self, code: &str) {
        assert!(self.hard_failed(code), "{self:?}");
    }
}

#[derive(Debug)]
pub(crate) struct Fixture {
    home: TempDir,
}

impl Fixture {
    pub(crate) fn with_components(components: &str) -> Self {
        let fixture = Self {
            home: tempfile::tempdir().expect("tempdir"),
        };
        fixture.dir("", 0o755);
        fixture.dir("bin", 0o755);
        fixture.put("bin/rustc", b"rustc\n", 0o755);
        for helper in HELPERS {
            fixture.put(
                &format!("bin/{helper}"),
                b"#!/bin/sh\n: > \"$VELNOR_TEST_MARKER\"\nexit 1\n",
                0o755,
            );
        }
        fixture.dir("etc/empty", 0o755);
        fixture.dir("lib/rustlib", 0o755);
        fixture.put("lib/x.so", b"so\n", 0o644);
        fixture.put("lib/rustlib/components", components.as_bytes(), 0o644);
        fixture.put(INSTALLER_VERSION, b"3", 0o644);
        fixture.put(MANIFEST_CARGO, b"file:lib/x.so\nfile:share/.dot\n", 0o644);
        fixture.put(MANIFEST_RUSTC, b"file:bin/rustc\n", 0o644);
        fixture.put(
            MULTIRUST_CONFIG,
            b"config_version = \"1\"\n\n[[components]]\npkg = \"cargo\"\ntarget = \"x86_64-unknown-linux-gnu\"\nis_extension = false\n",
            0o644,
        );
        fixture.put(CHANNEL_MANIFEST, FIXTURE_BOOKKEEPING.as_bytes(), 0o644);
        fixture.dir("share", 0o755);
        fixture.put("share/.dot", b"dot\n", 0o644);
        fixture
    }

    pub(crate) fn standard() -> Self {
        Self::with_components(FIXTURE_COMPONENTS)
    }

    pub(crate) fn rustup_home(&self) -> PathBuf {
        self.home.path().join("rustup")
    }

    pub(crate) fn toolchains(&self) -> PathBuf {
        self.rustup_home().join("toolchains")
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.toolchains().join(format!("{VERSION}-{TARGET}"))
    }

    pub(crate) fn marker(&self) -> PathBuf {
        self.home.path().join("marker")
    }

    fn at(&self, rel: &str) -> PathBuf {
        if rel.is_empty() {
            self.root()
        } else {
            self.root().join(rel)
        }
    }

    pub(crate) fn dir(&self, rel: &str, mode: u32) {
        let path = self.at(rel);
        fs::create_dir_all(&path).expect("create dir");
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("chmod dir");
    }

    pub(crate) fn put(&self, rel: &str, bytes: &[u8], mode: u32) {
        let path = self.at(rel);
        fs::write(&path, bytes).expect("write file");
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("chmod file");
    }

    pub(crate) fn chmod(&self, rel: &str, mode: u32) {
        fs::set_permissions(self.at(rel), fs::Permissions::from_mode(mode)).expect("chmod");
    }

    pub(crate) fn remove(&self, rel: &str) {
        let path = self.at(rel);
        if path.is_dir() {
            fs::remove_dir_all(path).expect("remove dir");
        } else {
            fs::remove_file(path).expect("remove file");
        }
    }

    pub(crate) fn link(&self, rel: &str, target: &str) {
        symlink(target, self.at(rel)).expect("symlink");
    }

    pub(crate) fn sibling(&self, name: &str) {
        let path = self.toolchains().join(name);
        fs::create_dir_all(&path).expect("sibling dir");
    }

    pub(crate) fn symlink_root(&self) {
        let real = self.home.path().join("real");
        fs::rename(self.root(), &real).expect("move root");
        symlink(&real, self.root()).expect("symlink root");
    }

    pub(crate) fn run(&self, script: &str) -> Outcome {
        self.run_with_path(script, Path::new("/usr/bin"))
    }

    pub(crate) fn run_with_path(&self, script: &str, path_dir: &Path) -> Outcome {
        let runner_temp = self.home.path().join("runner");
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .env_clear()
            .env("PATH", path_dir)
            .env("HOME", self.home.path())
            .env("RUNNER_TEMP", &runner_temp)
            .env("RUSTUP_HOME", self.rustup_home())
            .env("CARGO_HOME", self.home.path().join("cargo"))
            .env("VELNOR_TEST_MARKER", self.marker())
            .env("HTTPS_PROXY", PROXY)
            .env("https_proxy", PROXY)
            .env("ALL_PROXY", PROXY)
            .env("all_proxy", PROXY)
            .output()
            .expect("spawn sh");
        Outcome {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

pub(crate) type Mutation<'a> = &'a (dyn Fn(&Fixture) + Sync);

pub(crate) struct Row<'a> {
    pub(crate) name: String,
    pub(crate) mutate: Mutation<'a>,
    pub(crate) pin: TreeDigestPin,
}

pub(crate) fn row<'a>(name: &str, mutate: Mutation<'a>) -> Row<'a> {
    Row {
        name: name.to_owned(),
        mutate,
        pin: fixture_pin(),
    }
}

pub(crate) fn fixture_pin() -> TreeDigestPin {
    TreeDigestPin::new(
        VERSION,
        TARGET,
        FIXTURE_COMPONENTS_SHA256,
        FIXTURE_TREE_SHA256,
    )
}

pub(crate) fn script(phase: ToolchainPhase, pins: &[TreeDigestPin]) -> String {
    toolchain_script(VERSION, phase, RUST_CHANNEL_MANIFESTS, pins)
}

pub(crate) fn run_rows(rows: &[Row<'_>], accepts: &(dyn Fn(&Outcome) -> bool + Sync)) {
    let failures = thread::scope(|scope| {
        let handles = rows
            .iter()
            .flat_map(|row| PHASES.map(|phase| (row, phase)))
            .map(|(row, phase)| {
                scope.spawn(move || {
                    let fx = Fixture::standard();
                    (row.mutate)(&fx);
                    let outcome = fx.run(&script(phase, &[row.pin]));
                    (!accepts(&outcome)).then(|| format!("{} {phase:?}: {outcome:?}", row.name))
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().expect("row thread"))
            .collect::<Vec<_>>()
    });
    assert!(failures.is_empty(), "{failures:#?}");
}
