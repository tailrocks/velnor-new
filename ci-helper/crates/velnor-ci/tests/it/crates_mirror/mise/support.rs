// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Command, Path, PathBuf, Repo, TestResult, fs, rustup_binary};

pub(super) struct Mise {
    binary: PathBuf,
    rustup_home: PathBuf,
    rustc: PathBuf,
}

impl Mise {
    pub(super) fn new(repo: &Repo) -> Result<Self, Box<dyn std::error::Error>> {
        let binary = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|path| path.join("mise"))
            .chain([PathBuf::from(std::env::var_os("HOME").ok_or("HOME")?).join(".local/bin/mise")])
            .find(|path| path.is_file())
            .ok_or("Mise unavailable: integration requires a real installed Mise binary")?;
        let binary = fs::canonicalize(binary)?;
        let output = Command::new(&binary).arg("--version").output()?;
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8(output.stdout)?.starts_with("2026.11."));
        let rustup = rustup_binary().ok_or("rustup unavailable")?;
        let output = Command::new(rustup).args(["which", "cargo"]).output()?;
        assert!(output.status.success(), "{output:?}");
        let cargo = PathBuf::from(String::from_utf8(output.stdout)?.trim());
        let toolchain = cargo.parent().and_then(Path::parent).ok_or("toolchain")?;
        let actual_home = toolchain
            .parent()
            .and_then(Path::parent)
            .ok_or("Rustup home")?;
        let root = repo.root();
        let rustup_home = root.join("rustup-home");
        fs::create_dir_all(&rustup_home)?;
        for entry in fs::read_dir(actual_home)? {
            let entry = entry?;
            std::os::unix::fs::symlink(entry.path(), rustup_home.join(entry.file_name()))?;
        }
        for directory in [
            "mise-data/installs/rust",
            "cargo-home",
            "mise-shims",
            "rust-bin",
        ] {
            fs::create_dir_all(root.join(directory))?;
        }
        for name in ["cargo", "rustc", "rustdoc", "rustup"] {
            let executable = if name == "rustup" {
                rustup.to_path_buf()
            } else {
                cargo.with_file_name(name)
            };
            std::os::unix::fs::symlink(executable, root.join("rust-bin").join(name))?;
        }
        std::os::unix::fs::symlink(
            root.join("rust-bin"),
            root.join("mise-data/installs/rust/1.99.1"),
        )?;
        std::os::unix::fs::symlink(
            rustup.parent().ok_or("Rustup bin")?,
            root.join("cargo-home/bin"),
        )?;
        std::os::unix::fs::symlink(&binary, root.join("mise-shims/cargo"))?;
        repo.write_unstaged("empty.toml", "");
        repo.write_unstaged("configured-wrapper.rs", include_str!("external.rs"));
        let output = Command::new(cargo.with_file_name("rustc"))
            .arg(root.join("configured-wrapper.rs"))
            .arg("-o")
            .arg(root.join("configured-wrapper"))
            .output()?;
        assert!(output.status.success(), "{output:?}");
        fs::create_dir_all(root.join("mbx/bin"))?;
        std::os::unix::fs::symlink(root.join("configured-wrapper"), root.join("mbx/bin/mbx"))?;
        std::os::unix::fs::symlink(
            root.join("configured-wrapper"),
            root.join("mise-shims/cargo-fixture"),
        )?;
        std::os::unix::fs::symlink(
            root.join("configured-wrapper"),
            root.join("mise-shims/cargo-activation"),
        )?;
        std::os::unix::fs::symlink(
            root.join("configured-wrapper"),
            root.join("fixture-other-command"),
        )?;
        Ok(Self {
            binary,
            rustup_home,
            rustc: cargo.with_file_name("rustc"),
        })
    }

    pub(super) fn command(&self, repo: &Repo, program: &Path) -> Command {
        let root = repo.root();
        let mut command = Command::new(program);
        command
            .current_dir(root)
            .env_remove("RUSTUP_TOOLCHAIN")
            .env_remove("MISE_RUSTUP_HOME")
            .env_remove("__MISE_ACTIVE_WRAPPERS")
            .env_remove("MBX_CARGO_SHIM_MODE")
            .env_remove("VELNOR_CARGO_REAL")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env("MISE_DATA_DIR", root.join("mise-data"))
            .env("MISE_CONFIG_DIR", root.join("mise-config"))
            .env("MISE_CACHE_DIR", root.join("mise-cache"))
            .env("MISE_GLOBAL_CONFIG_FILE", root.join("empty.toml"))
            .env("MISE_SYSTEM_CONFIG_FILE", root.join("empty.toml"))
            .env("MISE_TRUSTED_CONFIG_PATHS", root)
            .env("MISE_AUTO_INSTALL", "false")
            .env("MISE_EXEC_AUTO_INSTALL", "false")
            .env("MISE_OFFLINE", "true")
            .env("CARGO_HOME", root.join("cargo-home"))
            .env("RUSTUP_HOME", &self.rustup_home);
        command
    }

    pub(super) fn configure(&self, repo: &Repo, kind: &str) {
        let root = repo.root();
        let wrappers = if kind == "explicit" {
            format!(
                "[wrappers.cargo]\ncommand='{}'\nenv={{ WRAPPER_MARKER='preserved' }}\n",
                root.join("configured-wrapper").display()
            )
        } else if kind == "other" {
            format!(
                "[wrappers.fixture-other]\ncommand='{}'\n",
                root.join("fixture-other-command").display()
            )
        } else {
            String::new()
        };
        let tools = if kind == "implicit" {
            format!(
                "rust={{version='1.99.1', mr_boxington=true}}\nmr-boxington='path:{}'\n",
                root.join("mbx").display()
            )
        } else {
            "rust='1.99.1'\n".to_owned()
        };
        repo.write_unstaged("mise.toml", &format!("[tools]\n{tools}[env]\nCONFIG_ROOT_MARKER='{{{{ config_root }}}}'\nMISE_DATA_DIR='{}'\nCARGO_HOME='{}'\nRUSTUP_HOME='{}'\n{wrappers}[settings]\nauto_install=false\n[settings.rust]\ncargo_home='{}'\nrustup_home='{}'\n", root.join("mise-data").display(), root.join("cargo-home").display(), self.rustup_home.display(), root.join("unused-cargo-home").display(), root.join("unused-rustup-home").display()));
    }

    pub(super) fn activate(&self, repo: &Repo) -> TestResult {
        let output = self
            .command(repo, &self.binary)
            .args(["reshim", "--force"])
            .output()?;
        assert!(output.status.success(), "{output:?}");
        Ok(())
    }

    pub(super) fn dispatch(
        &self,
        repo: &Repo,
        args: &[String],
    ) -> Result<std::process::Output, Box<dyn std::error::Error>> {
        repo.write_unstaged("ca.pem", "fixture");
        let output =
            super::super::fixture::setup_command(repo.root(), "sparse+http://127.0.0.1:1/index/")
                .output()?;
        assert!(output.status.success(), "{output:?}");
        let wrapper = repo.root().join("velnor-cargo/cargo");
        let paths = [
            repo.root().join("mise-shims"),
            wrapper.parent().ok_or("wrapper parent")?.to_path_buf(),
        ]
        .into_iter()
        .chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ))
        .collect::<Vec<_>>();
        let output = self
            .command(repo, &wrapper)
            .args(args)
            .env("__MISE_ACTIVE_WRAPPERS", "[\"unrelated-command\"]")
            .env("PATH", std::env::join_paths(paths)?)
            .env(
                "VELNOR_CARGO_CONFIG",
                repo.root().join("velnor-cargo/config.toml"),
            )
            .env("VELNOR_CARGO_WRAPPER", &wrapper)
            .output()?;
        assert!(output.status.success(), "{output:?}");
        Ok(output)
    }

    pub(super) fn absolute_toolchain(
        &self,
        repo: &Repo,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let selected = repo.root().join("selected toolchain (native)");
        std::os::unix::fs::symlink(
            self.rustc
                .parent()
                .and_then(Path::parent)
                .ok_or("toolchain")?,
            &selected,
        )?;
        Ok(selected)
    }

    pub(super) fn stale_wrapper(&self, repo: &Repo) -> TestResult {
        let directory = repo.root().join("mise-data/command-wrappers/bin");
        fs::create_dir_all(&directory)?;
        std::os::unix::fs::symlink(&self.binary, directory.join("cargo"))?;
        Ok(())
    }

    pub(super) fn nested_fetches(
        &self,
        repo: &Repo,
        wrapper: &Path,
        config: &Path,
        external: bool,
    ) -> TestResult {
        let root = repo.root();
        let paths = [
            root.join("mise-shims"),
            wrapper.parent().ok_or("wrapper parent")?.to_path_buf(),
        ]
        .into_iter()
        .chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ))
        .collect::<Vec<_>>();
        let mut command = self.command(repo, if external { &self.binary } else { wrapper });
        if external {
            command
                .args(["exec", "--"])
                .arg(root.join("mise-data/command-wrappers/bin/cargo"))
                .env("EXTERNAL_MISE_ENTRY", "1");
        } else {
            command.env("__MISE_ACTIVE_WRAPPERS", "malformed upstream marker");
        }
        let output = command
            .arg("fixture")
            .env("CARGO", wrapper)
            .env("MIRROR_FIXTURE_ROOT", root)
            .env_remove("MIRROR_EXPECTED_NO_TOOLCHAIN")
            .env("VELNOR_CARGO_CONFIG", config)
            .env("VELNOR_CARGO_WRAPPER", wrapper)
            .env("PATH", std::env::join_paths(paths)?)
            .env("CARGO_NET_OFFLINE", "false")
            .env("CARGO_HTTP_PROXY", "http://127.0.0.1:1")
            .env("NO_PROXY", "127.0.0.1")
            .env("CARGO_NET_RETRY", "0")
            .env(
                "CARGO_REGISTRIES_CRATES_IO_INDEX",
                "sparse+http://127.0.0.1:1/index/",
            )
            .output()?;
        assert!(output.status.success(), "{output:?}");
        assert!(root.join("child-home-path/registry/src").is_dir());
        assert!(root.join("child-home-env/registry/src").is_dir());
        Ok(())
    }
}
