// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::fixture::{Fixture, Outcome, VERSION};
use sha2::{Digest, Sha256};
use velnor_actions_workflow_renderer::verifier_builders::{ToolchainPhase, toolchain_script};

pub(crate) const COMPONENT: &str = "cargo-x86_64-unknown-linux-gnu";
pub(crate) const DIST: &str = "https://static.rust-lang.org/dist";
pub(crate) const CHANNEL_URL: &str = "https://static.rust-lang.org/dist/channel-rust-1.99.1.toml";
pub(crate) const TARBALL_URL: &str =
    "https://static.rust-lang.org/dist/2026-10-15/cargo-1.99.1-x86_64-unknown-linux-gnu.tar.xz";

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .concat()
}

fn packages(tarball_sha256: &str) -> String {
    format!(
        "[pkg.cargo]\nversion = \"0.100.0\"\n\n[pkg.cargo.target.x86_64-unknown-linux-gnu]\navailable = true\nurl = \"{DIST}/2026-10-15/cargo-1.99.1-x86_64-unknown-linux-gnu.tar.gz\"\nhash = \"{h}\"\nxz_url = \"{TARBALL_URL}\"\nxz_hash = \"{tarball_sha256}\"\n",
        h = "3".repeat(64)
    )
}

fn channel_manifest(tarball_sha256: &str) -> String {
    format!(
        "manifest-version = \"2\"\ndate = \"2026-10-15\"\ngit_commit_hash = \"abc\"\n\n{}\n[[artifacts.source-code.target.\"*\"]]\nurl = \"{DIST}/2026-10-15/rustc-1.99.1-src.tar.gz\"\nhash-sha256 = \"{}\"\n",
        packages(tarball_sha256),
        "4".repeat(64)
    )
}

fn rustup_bookkeeping(tarball_sha256: &str) -> String {
    format!(
        "manifest-version = \"2\"\ndate = \"2026-10-15\"\n\n{}components = []\nextensions = []\n",
        packages(tarball_sha256)
    )
}

pub(crate) type Bookkeeping<'a> = &'a dyn Fn(&Release) -> String;

#[derive(Debug)]
pub(crate) struct Release {
    _work: tempfile::TempDir,
    channel: PathBuf,
    channel_sha256: String,
    tarball: PathBuf,
    pub(crate) tarball_sha256: String,
}

impl Release {
    pub(crate) fn of(fx: &Fixture) -> Self {
        let work = tempfile::tempdir().expect("work dir");
        let sub = work.path().join("stage/pkg/sub");
        fs::create_dir_all(&sub).expect("stage");
        let copied = Command::new("cp")
            .arg("-R")
            .arg(format!("{}/.", fx.root().display()))
            .arg(&sub)
            .status()
            .expect("copy tree");
        assert!(copied.success());
        for bookkeeping in [
            "lib/rustlib/components",
            "lib/rustlib/rust-installer-version",
            "lib/rustlib/multirust-channel-manifest.toml",
        ] {
            fs::remove_file(sub.join(bookkeeping)).expect("drop bookkeeping");
        }
        fs::write(work.path().join("stage/pkg/components"), "sub\n").expect("components");
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
        let channel = work.path().join("channel.toml");
        let text = channel_manifest(&tarball_sha256);
        fs::write(&channel, &text).expect("write channel");
        Self {
            channel_sha256: sha256_hex(text.as_bytes()),
            _work: work,
            channel,
            tarball,
            tarball_sha256,
        }
    }

    fn script(&self, phase: ToolchainPhase) -> String {
        let script = toolchain_script(
            VERSION,
            phase,
            &[(VERSION, &self.channel_sha256, "2026-10-15")],
            &[],
        );
        let start = script.find("get() {").expect("get definition");
        let end = start + script[start..].find("; }").expect("get end") + 3;
        let serve = format!(
            "get() {{ case \"$2\" in {CHANNEL_URL}) cp \"{}\" \"$1\";; {TARBALL_URL}) cp \"{}\" \"$1\";; *) exit 22;; esac; }}",
            self.channel.display(),
            self.tarball.display()
        );
        format!("{}{serve}{}", &script[..start], &script[end..])
    }

    pub(crate) fn installed(&self) -> String {
        rustup_bookkeeping(&self.tarball_sha256)
    }

    pub(crate) fn run(&self, fx: &Fixture, phase: ToolchainPhase) -> Outcome {
        fx.run(&self.script(phase))
    }
}

pub(crate) fn fixture_with(bookkeeping: Bookkeeping<'_>) -> (Fixture, Release) {
    let fx = Fixture::with_components(&format!("{COMPONENT}\n"));
    let release = Release::of(&fx);
    fx.put(
        "lib/rustlib/multirust-channel-manifest.toml",
        bookkeeping(&release).as_bytes(),
        0o644,
    );
    (fx, release)
}
