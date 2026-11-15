// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_ci::release::{CargoPathEntry, check_cargo_discovery};

const PROVIDER: &str = "[registry]\ncredential-provider = [\"./steal\"]\n";

struct Checkout(PathBuf);

impl Checkout {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("velnor-ci-discovery-{}-{name}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
        Self(dir)
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.0.join(path);
        fs::create_dir_all(file.parent().unwrap_or(Path::new("")))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(file, text).unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn link(&self, from: &str, to: &str) {
        let link = self.0.join(from);
        fs::create_dir_all(link.parent().unwrap_or(Path::new("")))
            .unwrap_or_else(|e| unreachable!("{e}"));
        symlink(to, link).unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn check(&self, file: &str) -> (Option<i32>, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["release", "check-config", file])
            .current_dir(&self.0)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn refused(checkout: &Checkout, file: &str, needle: &str) {
    let (code, stderr) = checkout.check(file);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains(needle), "{needle}: {stderr}");
}

#[test]
fn a_cargo_symlink_hiding_a_credential_provider_is_refused_on_the_checkout() {
    let checkout = Checkout::new("cargo-link");
    checkout.write("release-plz.toml", "[workspace]\n");
    checkout.write("cfg/config.toml", PROVIDER);
    assert_eq!(checkout.check("release-plz.toml").0, Some(0));
    checkout.link(".cargo", "cfg");
    refused(&checkout, "release-plz.toml", ".cargo: a symbolic link");
}

#[test]
fn a_symlink_anywhere_on_the_discovery_path_of_a_root_is_refused() {
    for (link, to) in [
        (".cargo", "elsewhere"),
        ("tools/.cargo", "elsewhere"),
        (".cargo/config.toml", "../elsewhere.toml"),
        ("tools/.cargo/config", "../../elsewhere.toml"),
        ("tools/.cargo/config.toml", "../../elsewhere.toml"),
        ("tools", "real"),
    ] {
        let checkout = Checkout::new("path");
        checkout.write("elsewhere.toml", "[workspace]\n");
        checkout.write("real/release-plz.toml", "[workspace]\n");
        fs::create_dir_all(checkout.0.join("tools/inner")).ok();
        if link == "tools" {
            fs::remove_dir_all(checkout.0.join("tools")).ok();
        } else {
            checkout.write("tools/release-plz.toml", "[workspace]\n");
        }
        if link.starts_with(".cargo/") {
            fs::create_dir_all(checkout.0.join(".cargo")).unwrap_or_else(|e| unreachable!("{e}"));
        }
        if link.starts_with("tools/.cargo/") {
            fs::create_dir_all(checkout.0.join("tools/.cargo"))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        checkout.link(link, to);
        refused(
            &checkout,
            "tools/release-plz.toml",
            &format!("{link}: a symbolic link"),
        );
    }
}

#[test]
fn a_cargo_config_on_the_path_that_runs_a_program_or_redirects_is_refused() {
    for config in [
        "[registry]\ncredential-provider = [\"./steal\"]\n",
        "[registry]\nglobal-credential-providers = [\"./steal\"]\n",
        "[credential-alias]\nx = \"./steal\"\n",
        "[http]\nproxy = \"http://evil\"\n",
        "[http]\ncainfo = \"/tmp/ca.pem\"\n",
        "[build]\nrustc = \"/tmp/evil\"\n",
        "[build]\nrustc-wrapper = \"/tmp/evil\"\n",
        "[build]\nrustc-workspace-wrapper = \"/tmp/evil\"\n",
        "[env]\nLD_PRELOAD = \"/tmp/evil.so\"\n",
    ] {
        for place in [
            ".cargo/config.toml",
            ".cargo/config",
            "tools/.cargo/config.toml",
        ] {
            let checkout = Checkout::new("config");
            checkout.write("tools/release-plz.toml", "[workspace]\n");
            checkout.write(place, config);
            refused(&checkout, "tools/release-plz.toml", place);
        }
    }
}

#[test]
fn a_clean_path_a_builtin_provider_and_a_config_off_the_path_pass() {
    let checkout = Checkout::new("clean");
    checkout.write("tools/release-plz.toml", "[workspace]\n");
    checkout.write(
        ".cargo/config.toml",
        "[registry]\ncredential-provider = \"cargo:token\"\n[build]\njobs = 2\n",
    );
    checkout.write("other/.cargo/config.toml", PROVIDER);
    checkout.link("other/.cargo/config", "config.toml");
    assert_eq!(checkout.check("tools/release-plz.toml").0, Some(0));
}

#[test]
fn the_pure_check_names_the_first_symlink_or_hazard() {
    let entry = |path: &str, symlink: bool, text: Option<&str>| CargoPathEntry {
        path: path.to_owned(),
        symlink,
        text: text.map(str::to_owned),
    };
    assert!(check_cargo_discovery(&[]).is_ok());
    assert!(check_cargo_discovery(&[entry(".cargo", false, None)]).is_ok());
    let linked = check_cargo_discovery(&[entry(".cargo", true, None)]).err();
    assert!(linked.is_some_and(|e| e.to_string().starts_with(".cargo: ")));
    let hazard = check_cargo_discovery(&[entry(".cargo/config.toml", false, Some(PROVIDER))]).err();
    assert!(hazard.is_some_and(|e| e.to_string().starts_with(".cargo/config.toml: ")));
}
