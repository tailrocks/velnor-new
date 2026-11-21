// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::error::CiError;

use super::fetch::Trusted;
use super::fetch_fixed::Located;

const PROXY: &str = "rustup";
const TOOLCHAIN_VARIABLE: &str = "RUSTUP_TOOLCHAIN";

pub(super) struct Resolved {
    pub(super) cargo: PathBuf,
    pub(super) toolchain: OsString,
}

pub(super) fn resolve(
    cargo: &Path,
    home: &Path,
    rustup_home: &Path,
    neutral: &Path,
    trusted: &Trusted,
    fixed: Option<&Located>,
) -> Result<Resolved, CiError> {
    let resolved = std::fs::canonicalize(cargo)
        .map_err(|e| CiError::new(format!("cannot resolve {}: {e}", cargo.display())))?;
    let Some(rustup) = beside(cargo, &resolved, trusted, fixed) else {
        return Ok(Resolved {
            cargo: cargo.to_path_buf(),
            toolchain: chosen()
                .or_else(|| default_toolchain(rustup_home))
                .unwrap_or_else(unpinned),
        });
    };
    let rustup_home = std::fs::canonicalize(rustup_home).map_err(|e| {
        CiError::new(format!(
            "cannot resolve RUSTUP_HOME {}: {e}",
            rustup_home.display()
        ))
    })?;
    let run = |arguments: [&str; 2], toolchain: Option<&str>| -> Result<Output, CiError> {
        let mut command = Command::new(&rustup);
        command
            .current_dir(neutral)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", home)
            .env("RUSTUP_HOME", &rustup_home)
            .env("RUSTUP_AUTO_INSTALL", "0")
            .args(arguments);
        if let Some(toolchain) = toolchain {
            command.env(TOOLCHAIN_VARIABLE, toolchain);
        }
        command
            .output()
            .map_err(|e| CiError::new(format!("cannot run {}: {e}", rustup.display())))
    };
    let name = if let Some(name) = chosen() {
        name.to_string_lossy().into_owned()
    } else {
        let output = run(["show", "active-toolchain"], None)?;
        let text = String::from_utf8_lossy(&output.stdout);
        let name = text
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        if !output.status.success() || !named(&name) {
            return Err(CiError::new(format!(
                "rustup did not name an installed toolchain ({name:?}); refusing to run cargo with the token"
            )));
        }
        name
    };
    let which = run(["which", "cargo"], Some(&name))?;
    let printed = String::from_utf8_lossy(&which.stdout);
    let real = which
        .status
        .success()
        .then(|| real_cargo(&printed, &rustup_home, &[&rustup, &resolved]))
        .flatten()
        .ok_or_else(|| {
            CiError::new(format!(
                "rustup did not resolve toolchain {name} to its own cargo under {}/toolchains; refusing to run a rustup proxy with the token",
                rustup_home.display()
            ))
        })?;
    if fixed.is_some_and(|fixed| !fixed.seals(&real)) {
        return Err(CiError::new(format!(
            "{} is outside the sealed toolchain; refusing to run it with the token",
            real.display()
        )));
    }
    Ok(Resolved {
        cargo: real,
        toolchain: name.into(),
    })
}

pub(super) fn beside(
    cargo: &Path,
    resolved: &Path,
    trusted: &Trusted,
    fixed: Option<&Located>,
) -> Option<PathBuf> {
    if resolved.file_name().is_some_and(|name| name == PROXY) {
        return Some(resolved.to_path_buf());
    }
    [cargo.with_file_name(PROXY), resolved.with_file_name(PROXY)]
        .into_iter()
        .filter_map(|candidate| std::fs::canonicalize(candidate).ok())
        .find(|rustup| {
            rustup.is_file()
                && trusted.within(rustup)
                && fixed.is_none_or(|fixed| fixed.seals(rustup))
        })
}

fn real_cargo(printed: &str, rustup_home: &Path, proxies: &[&Path]) -> Option<PathBuf> {
    let path = std::fs::canonicalize(printed.lines().next()?.trim()).ok()?;
    let bin = path.parent()?;
    let placed = path.file_name().is_some_and(|name| name == "cargo")
        && bin.file_name().is_some_and(|name| name == "bin")
        && bin.parent()?.parent() == Some(rustup_home.join("toolchains").as_path());
    (placed
        && path.is_file()
        && !super::fetch_real_cargo::is_mise_front(&path, &path)
        && proxies.iter().all(|proxy| !same_program(&path, proxy)))
    .then_some(path)
}

fn same_program(first: &Path, second: &Path) -> bool {
    let (Ok(one), Ok(other)) = (std::fs::metadata(first), std::fs::metadata(second)) else {
        return true;
    };
    if (one.dev(), one.ino()) == (other.dev(), other.ino()) {
        return true;
    }
    one.len() == other.len()
        && match (std::fs::read(first), std::fs::read(second)) {
            (Ok(one), Ok(other)) => one == other,
            _ => true,
        }
}

fn unpinned() -> OsString {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    format!("velnor-no-toolchain-{}-{nanos}", std::process::id()).into()
}

fn chosen() -> Option<OsString> {
    std::env::var(TOOLCHAIN_VARIABLE)
        .ok()
        .filter(|name| named(name))
        .map(OsString::from)
}

fn default_toolchain(rustup_home: &Path) -> Option<OsString> {
    let text = std::fs::read_to_string(rustup_home.join("settings.toml")).ok()?;
    let table = text.parse::<toml::Table>().ok()?;
    let name = table.get("default_toolchain")?.as_str()?;
    named(name).then(|| OsString::from(name))
}

fn named(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("velnor-proxy-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&root));
        fs::create_dir_all(root.join("home/toolchains/t/bin"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(root.join("rustup"), "proxy").unwrap_or_else(|e| unreachable!("{e}"));
        fs::canonicalize(&root).unwrap_or_else(|e| unreachable!("{e}"))
    }

    #[test]
    fn a_toolchain_cargo_that_is_a_link_or_copy_of_the_proxy_is_not_a_real_cargo() {
        let root = tree("identity");
        let home = root.join("home");
        let cargo = home.join("toolchains/t/bin/cargo");
        let printed = cargo.display().to_string();
        let proxy = root.join("rustup");
        fs::hard_link(&proxy, &cargo).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(real_cargo(&printed, &home, &[&proxy]), None);
        fs::remove_file(&cargo).unwrap_or_else(|e| unreachable!("{e}"));
        fs::copy(&proxy, &cargo).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(real_cargo(&printed, &home, &[&proxy]), None);
        fs::write(&cargo, "cargo!").unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(real_cargo(&printed, &home, &[&proxy]), Some(cargo));
    }

    #[test]
    fn a_cargo_outside_the_toolchain_bin_directory_is_not_a_real_cargo() {
        let root = tree("placement");
        let home = root.join("home");
        let proxy = root.join("rustup");
        for path in [
            root.join("cargo"),
            home.join("toolchains/cargo"),
            home.join("toolchains/t/cargo"),
        ] {
            fs::write(&path, "cargo!").unwrap_or_else(|e| unreachable!("{e}"));
            assert_eq!(
                real_cargo(&path.display().to_string(), &home, &[&proxy]),
                None
            );
        }
    }
}
