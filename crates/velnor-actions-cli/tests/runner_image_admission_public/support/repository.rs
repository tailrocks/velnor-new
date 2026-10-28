// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::PathBuf, process::Command};

const RUNNER_DOCKERFILE_PATH: &str = "images/runner/ubuntu-26.04/Dockerfile";
const RUNNER_DOCKERFILE: &[u8] =
    include_bytes!("../../../../../images/runner/ubuntu-26.04/Dockerfile");

pub(super) struct GitFixture {
    root: PathBuf,
}

impl GitFixture {
    pub(super) fn new(root: PathBuf) -> Result<Self, Box<dyn Error>> {
        fs::create_dir_all(root.join(".velnor"))?;
        fs::create_dir_all(root.join("images/runner/ubuntu-26.04"))?;
        fs::write(root.join(".velnor/runner.lock"), valid_lock())?;
        fs::write(root.join("tracked.txt"), b"selected source\n")?;
        fs::write(root.join(RUNNER_DOCKERFILE_PATH), RUNNER_DOCKERFILE)?;
        let fixture = Self { root };
        fixture.git(&["init", "--quiet"])?;
        fixture.git(&["config", "user.name", "Admission fixture"])?;
        fixture.git(&["config", "user.email", "admission@example.invalid"])?;
        fixture.git(&["config", "commit.gpgsign", "false"])?;
        fixture.git(&["add", "."])?;
        fixture.git(&["commit", "--quiet", "-m", "fixture source"])?;
        Ok(fixture)
    }

    pub(super) fn add_replacement_ref(
        &self,
        checkout_replacement: bool,
    ) -> Result<(), Box<dyn Error>> {
        let source = self.head()?;
        fs::write(self.root.join("tracked.txt"), b"replacement source B\n")?;
        let lock_path = self.root.join(".velnor/runner.lock");
        let mut lock = fs::read(&lock_path)?;
        lock.extend_from_slice(b"fixture_marker = \"replacement-b\"\n");
        fs::write(lock_path, lock)?;
        let manifest = self.root.join("Cargo.toml");
        if manifest.is_file() {
            fs::write(manifest, b"replacement source B\n")?;
        }
        self.git(&["add", "--all"])?;
        let tree = String::from_utf8(self.git(&["write-tree"])?.stdout)?
            .trim()
            .to_owned();
        let replacement = String::from_utf8(
            self.git(&["commit-tree", &tree, "-m", "unrelated replacement source"])?
                .stdout,
        )?
        .trim()
        .to_owned();
        self.git(&["checkout", "--quiet", &source, "--", "."])?;
        if checkout_replacement {
            self.git(&["checkout", "--quiet", "--detach", &replacement])?;
            self.git(&["update-ref", "--no-deref", "HEAD", &source])?;
        }
        self.git(&["replace", &source, &replacement])?;
        Ok(())
    }

    pub(super) fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub(super) fn head(&self) -> Result<String, Box<dyn Error>> {
        Ok(String::from_utf8(self.git(&["rev-parse", "HEAD"])?.stdout)?
            .trim()
            .to_owned())
    }

    pub(super) fn mutate_checkout(&self, mutation: &str) -> Result<(), Box<dyn Error>> {
        match mutation {
            "worktree" => fs::write(self.root.join("tracked.txt"), b"worktree change\n")?,
            "index" => {
                fs::write(self.root.join("tracked.txt"), b"index change\n")?;
                self.git(&["add", "tracked.txt"])?;
            }
            "untracked" => fs::write(self.root.join("untracked.txt"), b"extra\n")?,
            _ => return Err(format!("unknown checkout mutation: {mutation}").into()),
        }
        Ok(())
    }

    pub(super) fn hide_and_change_lock(&self, flag: &str) -> Result<(), Box<dyn Error>> {
        let exclude = self.root.join(".git/info/exclude");
        let mut excluded = fs::read(&exclude)?;
        excluded.extend_from_slice(b"\n/.velnor/runner.lock\n");
        fs::write(exclude, excluded)?;
        self.git(&["update-index", flag, ".velnor/runner.lock"])?;
        let changed = b"runner_version = \"changed\"\ndocker_version = \"29.9.1\"\nbuildx_version = \"0.38.0\"\ncompose_version = \"5.6.0\"\nnode_version = \"26.12.0\"\n";
        fs::write(self.root.join(".velnor/runner.lock"), changed)?;
        let status = self.git(&["status", "--porcelain"])?;
        if !status.stdout.is_empty() {
            return Err(format!(
                "hidden lock fixture is not Git-clean: {}",
                String::from_utf8_lossy(&status.stdout)
            )
            .into());
        }
        Ok(())
    }

    pub(super) fn hide_and_change_runner_dockerfile(
        &self,
        flag: &str,
    ) -> Result<Vec<u8>, Box<dyn Error>> {
        self.git(&["update-index", flag, RUNNER_DOCKERFILE_PATH])?;
        let path = self.root.join(RUNNER_DOCKERFILE_PATH);
        let mut changed = fs::read(&path)?;
        changed.extend_from_slice(b"\nfixture hidden source change\n");
        fs::write(&path, &changed)?;

        let diff = self.git_raw(&["diff", "--quiet", "HEAD", "--", RUNNER_DOCKERFILE_PATH])?;
        if !diff.status.success() {
            return Err("hidden Runner Dockerfile change is visible to git diff".into());
        }
        let committed = self.git(&["show", &format!("HEAD:{RUNNER_DOCKERFILE_PATH}")])?;
        if committed.stdout != RUNNER_DOCKERFILE || committed.stdout == changed {
            return Err("hidden change is not based on the committed Runner Dockerfile".into());
        }
        let status = self.git(&["status", "--porcelain"])?;
        if !status.stdout.is_empty() {
            return Err(format!(
                "hidden Runner Dockerfile fixture is not Git-clean: {}",
                String::from_utf8_lossy(&status.stdout)
            )
            .into());
        }
        Ok(changed)
    }

    pub(super) fn preserve_hidden_runner_dockerfile_state(
        &self,
        flag: &str,
        expected_bytes: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        let actual_bytes = fs::read(self.root.join(RUNNER_DOCKERFILE_PATH))?;
        if actual_bytes != expected_bytes {
            return Err("Runner Dockerfile bytes changed during admission".into());
        }
        let expected_flag = match flag {
            "--skip-worktree" => b'S',
            "--assume-unchanged" => b'h',
            _ => return Err("unknown Runner Dockerfile index flag".into()),
        };
        let mut expected_record = vec![expected_flag, b' '];
        expected_record.extend_from_slice(RUNNER_DOCKERFILE_PATH.as_bytes());
        expected_record.push(0);
        let actual_record = self.git(&["ls-files", "-v", "-z", "--", RUNNER_DOCKERFILE_PATH])?;
        if actual_record.stdout != expected_record {
            return Err("Runner Dockerfile index flag changed during admission".into());
        }
        Ok(())
    }

    pub(super) fn remove_source_lock_and_create_ignored_replacement(
        &self,
    ) -> Result<(), Box<dyn Error>> {
        fs::write(self.root.join(".gitignore"), b"/.velnor/runner.lock\n")?;
        self.git(&["add", ".gitignore"])?;
        self.git(&["rm", "-f", "--", ".velnor/runner.lock"])?;
        self.git(&["commit", "--quiet", "-m", "remove source runner lock"])?;
        let lock_path = self.root.join(".velnor/runner.lock");
        fs::create_dir_all(lock_path.parent().ok_or("runner lock has no parent")?)?;
        fs::write(&lock_path, valid_lock())?;

        let indexed = self.git(&["ls-files", "--", ".velnor/runner.lock"])?;
        if !indexed.stdout.is_empty() {
            return Err("ignored replacement lock is still tracked".into());
        }
        let ignored = self.git_raw(&["check-ignore", "-q", "--", ".velnor/runner.lock"])?;
        if !ignored.status.success() {
            return Err("replacement lock is not ignored by the repository rule".into());
        }
        let status = self.git(&["status", "--porcelain"])?;
        if !status.stdout.is_empty() {
            return Err(format!(
                "ignored replacement fixture is not Git-clean: {}",
                String::from_utf8_lossy(&status.stdout)
            )
            .into());
        }
        Ok(())
    }

    pub(super) fn advance_head(&self) -> Result<(), Box<dyn Error>> {
        fs::write(self.root.join("tracked.txt"), b"advanced source head\n")?;
        self.git(&["add", "tracked.txt"])?;
        self.git(&["commit", "--quiet", "-m", "advance fixture head"])?;
        Ok(())
    }

    pub(super) fn commit_lock(&self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        self.git(&["update-index", "--no-skip-worktree", ".velnor/runner.lock"])?;
        self.git(&[
            "update-index",
            "--no-assume-unchanged",
            ".velnor/runner.lock",
        ])?;
        fs::write(self.root.join(".velnor/runner.lock"), bytes)?;
        self.git(&["add", ".velnor/runner.lock"])?;
        self.git(&["commit", "--quiet", "-m", "change runner lock"])?;
        Ok(())
    }

    fn git(&self, args: &[&str]) -> Result<std::process::Output, Box<dyn Error>> {
        let output = self.git_raw(args)?;
        if !output.status.success() {
            return Err(format!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(output)
    }

    fn git_raw(&self, args: &[&str]) -> Result<std::process::Output, Box<dyn Error>> {
        Ok(Command::new("git")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()?)
    }
}

pub(super) fn valid_lock() -> Vec<u8> {
    b"runner_version = \"2.338.0\"\ndocker_version = \"29.9.1\"\nbuildx_version = \"0.38.0\"\ncompose_version = \"5.6.0\"\nnode_version = \"26.12.0\"\n".to_vec()
}

pub(super) fn lock_with_mutation(pin: &str, mutation: &str) -> Vec<u8> {
    let mut lines = String::new();
    for (name, version) in [
        ("runner_version", "2.338.0"),
        ("docker_version", "29.9.1"),
        ("buildx_version", "0.38.0"),
        ("compose_version", "5.6.0"),
        ("node_version", "26.12.0"),
    ] {
        if name == pin && mutation == "missing" {
            continue;
        }
        let value = if name == pin {
            match mutation {
                "number" => "42".to_owned(),
                "empty" => "\"\"".to_owned(),
                "leading_whitespace" => format!("\" {version}\""),
                "trailing_whitespace" => format!("\"{version} \""),
                _ => unreachable!("known lock mutation"),
            }
        } else {
            format!("\"{version}\"")
        };
        lines.push_str(name);
        lines.push_str(" = ");
        lines.push_str(&value);
        lines.push('\n');
    }
    lines.into_bytes()
}
